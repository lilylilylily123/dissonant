//! Running a CLAP plugin: one handle for the app thread, one for the audio thread.
//!
//! The split into [`HostedPlugin`] and [`PluginAudioUnit`] is not a stylistic choice, it is forced
//! by CLAP's thread model and by clack's encoding of it. A `clack_host::PluginInstance` is
//! deliberately **not** `Send`: the plugin's `[main-thread]` callbacks (parameter queries, state
//! save/load, GUI) may only ever run on the thread that created the instance. Its audio processor
//! *is* `Send`, and that is the only half the realtime thread is allowed to touch.
//!
//! So [`HostedPlugin`] stays with the app: it owns the instance, the parameter list and the state
//! blob. [`PluginAudioUnit`] goes to the mixer: it owns the processor and every buffer it will ever
//! need. Loading, activation and all allocation happen in [`HostedPlugin::open`], on the app
//! thread, because dlopen-ing a plugin takes hundreds of milliseconds and would blow the audio
//! deadline many times over.
//!
//! Dropping order matters: drop the [`PluginAudioUnit`] before the [`HostedPlugin`] it came from.
//! clack refuses to destroy an instance whose audio processor is still alive, and the instance
//! would leak instead. [`HostedPlugin`]'s destructor logs loudly if it is dropped the wrong way
//! round.

use std::ffi::CString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use clack_extensions::audio_ports::{AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
use clack_extensions::state::{PluginState, StateError};
use clack_host::entry::PluginEntryError;
use clack_host::events::event_types::{NoteOffEvent, NoteOnEvent, ParamValueEvent};
use clack_host::events::io::EventBuffer;
use clack_host::events::{Match, Pckn, UnknownEvent};
use clack_host::prelude::*;
use clack_host::process::audio_buffers::{
    AudioPortBuffer, AudioPortBufferType, AudioPorts, InputAudioBuffers, InputChannel,
};
use serde::{Deserialize, Serialize};

use crate::scan::load_entry;
use crate::{PluginDescriptor, PluginKind};

/// Stereo is what the mixer speaks, and what a plugin with no audio-ports extension is assumed to
/// produce.
const DEFAULT_OUTPUT_CHANNELS: usize = 2;

/// Headroom for note and parameter events queued between two render calls. Sized so a dense bar of
/// chords plus automation never reaches `EventBuffer`'s growth path, which would allocate.
const EVENT_CAPACITY: usize = 256;

// ─── parameters ──────────────────────────────────────────────────────────────────────────────

/// One automatable parameter of a hosted plugin, as the UI needs to draw it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamInfo {
    /// The plugin's own stable id, which is what automation and project files store.
    pub id: u32,
    pub name: String,
    /// The plugin's grouping path for this parameter, e.g. `"Oscillators/Wavetable 1"`. Often
    /// empty.
    pub module: String,
    pub min: f64,
    pub max: f64,
    /// What the parameter resets to, i.e. where double-click-to-reset snaps a knob.
    pub default: f64,
    /// What the parameter is set to right now. Re-read on every [`HostedPlugin::params`] call.
    pub value: f64,
    /// The parameter only takes integral values; a knob should snap and a readout should not show
    /// decimals.
    pub is_stepped: bool,
}

// ─── errors ──────────────────────────────────────────────────────────────────────────────────

/// Everything that can go wrong between picking a plugin and hearing it.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("could not load {path}: {source}")]
    Load {
        path: PathBuf,
        #[source]
        source: PluginEntryError,
    },

    #[error("plugin id {0:?} is not usable as a C string")]
    InvalidPluginId(String),

    #[error("could not instantiate {plugin_id} from {path}: {source}")]
    Instantiate {
        plugin_id: String,
        path: PathBuf,
        #[source]
        source: PluginInstanceError,
    },

    #[error("{plugin_id} refused to activate at {sample_rate} Hz with {max_frames} frames: {source}")]
    Activate {
        plugin_id: String,
        sample_rate: f64,
        max_frames: u32,
        #[source]
        source: PluginInstanceError,
    },

    #[error("{0} does not implement the CLAP state extension, so its settings cannot be saved")]
    NoStateExtension(String),

    #[error("{plugin_id} failed to transfer its state: {source}")]
    State {
        plugin_id: String,
        #[source]
        source: StateError,
    },
}

// ─── host callbacks ──────────────────────────────────────────────────────────────────────────

/// The callback surface this host exposes to plugins.
///
/// No host-side extensions are declared. Every CLAP host extension is optional, and declaring one
/// obliges the host to honour it; Dissonant honours none of `clap_host_params`,
/// `clap_host_state` or the port-rescan calls today, so claiming them would be a lie a plugin
/// could act on.
struct DissonantHost;

#[derive(Default)]
struct HostShared {
    /// Set when the plugin asks for `on_main_thread()`. Drained by the main-thread entry points on
    /// [`HostedPlugin`], which are the only places this host is known to be on that thread.
    callback_requested: AtomicBool,
}

impl SharedHandler<'_> for HostShared {
    fn request_restart(&self) {
        // Honouring this means deactivating, which needs the audio unit back from the mixer.
        // Dissonant keeps a plugin at one fixed sample rate and block size for its whole life, so
        // the request is recorded for diagnosis and otherwise declined.
        log::debug!("plugin requested a restart; Dissonant does not re-activate plugins in place");
    }

    fn request_process(&self) {
        // The mixer decides when a channel renders. A plugin that wants to be woken while idle is
        // covered by `PluginAudioUnit::is_silent` returning false as soon as anything is queued.
        log::debug!("plugin requested processing");
    }

    fn request_callback(&self) {
        self.callback_requested.store(true, Ordering::Release);
    }
}

impl HostHandlers for DissonantHost {
    type Shared<'a> = HostShared;
    type MainThread<'a> = ();
    type AudioProcessor<'a> = ();
}

fn host_info() -> HostInfo {
    // PANIC: all four strings are compile-time constants with no interior NUL.
    HostInfo::new(
        "Dissonant",
        "Dissonant",
        "https://github.com/lilylilylily123/dissonant",
        env!("CARGO_PKG_VERSION"),
    )
    .expect("host info is constant and NUL-free")
}

// ─── app thread ──────────────────────────────────────────────────────────────────────────────

/// A loaded plugin, living on the thread that loaded it.
///
/// See the [module docs](self) for why this is separate from [`PluginAudioUnit`], and for the
/// drop-order rule.
pub struct HostedPlugin {
    descriptor: PluginDescriptor,
    instance: PluginInstance<DissonantHost>,
    params_ext: Option<PluginParams>,
    state_ext: Option<PluginState>,
    params: Vec<ParamInfo>,
}

impl HostedPlugin {
    /// Loads, instantiates and activates `desc`, returning the app-thread handle and the audio
    /// unit to hand to the mixer.
    ///
    /// Every allocation the audio thread will need is made here: event buffers, port tables and
    /// the scratch audio buffer are all sized for `max_frames` and never grow afterwards.
    ///
    /// `max_frames` is the largest block the mixer will ever ask for. Asking for more is still
    /// correct — [`PluginAudioUnit::render_add`] splits the request — but costs an extra `process`
    /// call per block.
    pub fn open(
        desc: &PluginDescriptor,
        sample_rate: f64,
        max_frames: u32,
    ) -> Result<(HostedPlugin, PluginAudioUnit), PluginError> {
        // A zero-frame configuration is rejected by CLAP, and a zero-size buffer would make every
        // render a no-op rather than an audible failure.
        let max_frames = max_frames.max(1);

        let entry = load_entry(&desc.path).map_err(|source| PluginError::Load {
            path: desc.path.clone(),
            source,
        })?;

        let plugin_id = CString::new(desc.plugin_id.as_str())
            .map_err(|_| PluginError::InvalidPluginId(desc.plugin_id.clone()))?;

        let mut instance = PluginInstance::<DissonantHost>::new(
            |_| HostShared::default(),
            |_| (),
            &entry,
            &plugin_id,
            &host_info(),
        )
        .map_err(|source| PluginError::Instantiate {
            plugin_id: desc.plugin_id.clone(),
            path: desc.path.clone(),
            source,
        })?;

        let shared = instance.plugin_shared_handle();
        let params_ext = shared.get_extension::<PluginParams>();
        let state_ext = shared.get_extension::<PluginState>();
        let ports_ext = shared.get_extension::<PluginAudioPorts>();

        let (in_channels, out_channels) = port_channel_counts(&mut instance, ports_ext, desc.kind);
        let params = read_params(&mut instance, params_ext);

        let processor = instance
            .activate(
                |_, _| (),
                PluginAudioConfiguration {
                    sample_rate,
                    min_frames_count: 1,
                    max_frames_count: max_frames,
                },
            )
            .map_err(|source| PluginError::Activate {
                plugin_id: desc.plugin_id.clone(),
                sample_rate,
                max_frames,
                source,
            })?;

        let max_frames = max_frames as usize;
        let unit = PluginAudioUnit {
            processor: processor.into(),
            kind: desc.kind,
            input_events: EventBuffer::with_capacity(EVENT_CAPACITY),
            output_events: EventBuffer::with_capacity(EVENT_CAPACITY),
            in_ports: AudioPorts::with_capacity(in_channels.max(1), 1),
            out_ports: AudioPorts::with_capacity(out_channels, 1),
            input: vec![0.0; in_channels * max_frames],
            scratch: vec![0.0; out_channels * max_frames],
            in_channels,
            out_channels,
            max_frames,
            held: 0,
            pending_events: false,
            status: ProcessStatus::Sleep,
        };

        Ok((
            HostedPlugin {
                descriptor: desc.clone(),
                instance,
                params_ext,
                state_ext,
                params,
            },
            unit,
        ))
    }

    /// What this plugin was opened from, including the `uid` a project file stores.
    pub fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    /// The plugin's parameters, with [`ParamInfo::value`] refreshed from the plugin.
    ///
    /// Only `value` is re-read on each call; everything else is cached from
    /// [`open`](Self::open), because a plugin may not change a parameter's identity, range or
    /// default without being deactivated first. Refreshing matters after
    /// [`load_state`](Self::load_state) or after the user has moved a knob in the plugin's own
    /// window: the cached defaults would otherwise be displayed as if they were the current sound.
    ///
    /// A plugin with no parameters extension returns an empty slice. That is an ordinary plugin,
    /// not a failure.
    pub fn params(&mut self) -> &[ParamInfo] {
        self.service_callbacks();

        if let Some(ext) = self.params_ext {
            let handle = self.instance.plugin_handle();
            for param in &mut self.params {
                if let Some(id) = ClapId::from_raw(param.id) {
                    if let Some(value) = ext.get_value(&handle, id) {
                        param.value = value;
                    }
                }
            }
        }

        &self.params
    }

    /// The plugin's own opaque settings blob, to be stored in the project file.
    pub fn save_state(&mut self) -> Result<Vec<u8>, PluginError> {
        self.service_callbacks();

        let ext = self
            .state_ext
            .ok_or_else(|| PluginError::NoStateExtension(self.descriptor.name.clone()))?;

        let mut bytes = Vec::new();
        let handle = self.instance.plugin_handle();
        ext.save(&handle, &mut bytes)
            .map_err(|source| PluginError::State {
                plugin_id: self.descriptor.plugin_id.clone(),
                source,
            })?;
        Ok(bytes)
    }

    /// Restores a blob previously produced by [`save_state`](Self::save_state).
    ///
    /// Parameter values move with the state, so a following [`params`](Self::params) call reports
    /// the restored values.
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), PluginError> {
        self.service_callbacks();

        let ext = self
            .state_ext
            .ok_or_else(|| PluginError::NoStateExtension(self.descriptor.name.clone()))?;

        let mut reader = std::io::Cursor::new(bytes);
        let handle = self.instance.plugin_handle();
        ext.load(&handle, &mut reader)
            .map_err(|source| PluginError::State {
                plugin_id: self.descriptor.plugin_id.clone(),
                source,
            })
    }

    /// Runs the plugin's `on_main_thread` callback if it asked for one.
    ///
    /// Plugins use that callback to finish work they started on another thread, so it is drained
    /// at the top of every main-thread entry point rather than on a timer.
    fn service_callbacks(&mut self) {
        let requested = self
            .instance
            .access_shared_handler(|shared| shared.callback_requested.swap(false, Ordering::AcqRel));
        if requested {
            self.instance.call_on_main_thread_callback();
        }
    }
}

impl Drop for HostedPlugin {
    fn drop(&mut self) {
        // Deactivating returns the plugin's processing resources before the instance itself is
        // destroyed. It only succeeds once the audio unit is gone; see the module docs.
        if self.instance.try_deactivate().is_err() {
            log::warn!(
                "{} was dropped while its audio unit was still alive; the plugin instance will leak",
                self.descriptor.name
            );
        }
    }
}

/// Channel counts of the plugin's main input and output ports.
///
/// A plugin without the audio-ports extension has no ports at all by the CLAP spec, but hosts that
/// take that literally break a good number of real plugins, so ports are assumed from what the
/// plugin is for: an instrument gets an output only, an effect gets a stereo pair on both sides,
/// since an effect with no input to process would be pointless.
///
/// The two sides are read independently, so a plugin whose layouts differ — mono in, stereo out,
/// or the reverse — is described as it really is; [`PluginAudioUnit`] converts between that and
/// the mixer's stereo.
fn port_channel_counts(
    instance: &mut PluginInstance<DissonantHost>,
    ports: Option<PluginAudioPorts>,
    kind: PluginKind,
) -> (usize, usize) {
    let Some(ports) = ports else {
        return match kind {
            PluginKind::Instrument => (0, DEFAULT_OUTPUT_CHANNELS),
            PluginKind::Effect => (DEFAULT_OUTPUT_CHANNELS, DEFAULT_OUTPUT_CHANNELS),
        };
    };

    let handle = instance.plugin_handle();
    let mut buffer = AudioPortInfoBuffer::new();

    let channels = |is_input: bool, buffer: &mut AudioPortInfoBuffer| -> usize {
        if ports.count(&handle, is_input) == 0 {
            return 0;
        }
        ports
            .get(&handle, 0, is_input, buffer)
            .map(|info| info.channel_count as usize)
            .unwrap_or(0)
    };

    let in_channels = channels(true, &mut buffer);
    let out_channels = channels(false, &mut buffer).max(1);
    (in_channels, out_channels)
}

/// Reads the full parameter list once, at open time.
fn read_params(
    instance: &mut PluginInstance<DissonantHost>,
    params: Option<PluginParams>,
) -> Vec<ParamInfo> {
    let Some(params) = params else {
        return Vec::new();
    };

    let handle = instance.plugin_handle();
    let count = params.count(&handle);
    let mut buffer = ParamInfoBuffer::new();
    let mut out = Vec::with_capacity(count as usize);

    for index in 0..count {
        let Some(info) = params.get_info(&handle, index, &mut buffer) else {
            // A plugin that reports N parameters but cannot describe one of them is misbehaving;
            // the rest of its list is still usable.
            log::warn!("plugin parameter {index} could not be read");
            continue;
        };

        out.push(ParamInfo {
            id: info.id.get(),
            name: String::from_utf8_lossy(info.name).into_owned(),
            module: String::from_utf8_lossy(info.module).into_owned(),
            min: info.min_value,
            max: info.max_value,
            default: info.default_value,
            value: params.get_value(&handle, info.id).unwrap_or(info.default_value),
            is_stepped: info.flags.contains(ParamInfoFlags::IS_STEPPED),
        });
    }

    out
}

// ─── audio thread ────────────────────────────────────────────────────────────────────────────

/// The realtime half of a hosted plugin: the processor plus every buffer it needs.
///
/// `Send`, so it can be handed to the audio thread, and allocation-free once
/// [`HostedPlugin::open`] has returned.
pub struct PluginAudioUnit {
    processor: PluginAudioProcessor<DissonantHost>,
    /// What the plugin is for, which decides how it is fed and when it may be skipped.
    kind: PluginKind,
    /// Notes and parameter changes queued since the last render, in insertion order.
    input_events: EventBuffer,
    /// Where the plugin writes events back. Nothing upstream consumes them, so it is emptied every
    /// `process` call to keep it from growing.
    output_events: EventBuffer,
    in_ports: AudioPorts,
    out_ports: AudioPorts,
    /// De-interleaved plugin input, `in_channels` blocks of `max_frames`. It stays permanently
    /// zero for an instrument — which is fed silence on any input port it insists on declaring —
    /// and is refilled from the caller's buffers by every
    /// [`process_replace`](PluginAudioUnit::process_replace) chunk.
    input: Vec<f32>,
    /// De-interleaved plugin output, `out_channels` blocks of `max_frames`.
    scratch: Vec<f32>,
    in_channels: usize,
    out_channels: usize,
    max_frames: usize,
    /// One bit per MIDI pitch currently held down.
    held: u128,
    /// Set by every queued event: a parameter change can make a silent plugin sound, so it must
    /// not be skipped before the plugin has seen it.
    pending_events: bool,
    status: ProcessStatus,
}

/// What the plugin's input port carries for one `process` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortInput {
    /// Silence, flagged constant so a plugin is free to skip reading it at all.
    Silence,
    /// The caller's audio, already copied into `input` by `fill_input`.
    Audio,
}

impl PluginAudioUnit {
    /// Queues a note-on at the start of the next rendered block.
    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        let event = NoteOnEvent::new(0, pckn(pitch), f64::from(velocity) / 127.0);
        self.push(&event);
        if pitch < 128 {
            self.held |= 1u128 << pitch;
        }
    }

    /// Queues a note-off at the start of the next rendered block.
    pub fn note_off(&mut self, pitch: u8) {
        let event = NoteOffEvent::new(0, pckn(pitch), 0.0);
        self.push(&event);
        if pitch < 128 {
            self.held &= !(1u128 << pitch);
        }
    }

    /// Releases every held note with a single wildcard note-off, as CLAP defines it.
    pub fn all_notes_off(&mut self) {
        let event = NoteOffEvent::new(0, Pckn::match_all(), 0.0);
        self.push(&event);
        self.held = 0;
    }

    /// Queues a parameter change. Ignores `id`s CLAP cannot represent.
    pub fn set_param(&mut self, id: u32, value: f64) {
        let Some(id) = ClapId::from_raw(id) else {
            return;
        };
        let event = ParamValueEvent::new(0, id, Pckn::match_all(), value);
        self.push(&event);
    }

    /// Whether the mixer can skip this unit entirely.
    ///
    /// For an instrument: true only when the plugin itself asked to be put to sleep, no note is
    /// held, and nothing is waiting to be delivered to it. Any other `ProcessStatus` — including
    /// a release tail — keeps the channel awake.
    ///
    /// For an effect: true only when the plugin asked to be put to sleep, whatever is queued for
    /// it. An effect is driven by its input rather than by notes, so the answer is "has the tail
    /// finished", and a reverb reporting `Tail` or `Continue` must keep being processed after its
    /// input has gone quiet. It is **not** a licence to bypass an effect whose input is sounding:
    /// the chain only consults this once the signal reaching the slot is already silent.
    pub fn is_silent(&self) -> bool {
        match self.kind {
            PluginKind::Instrument => {
                !self.pending_events && self.held == 0 && self.status == ProcessStatus::Sleep
            }
            PluginKind::Effect => self.status == ProcessStatus::Sleep,
        }
    }

    /// Renders the plugin and **adds** its stereo output into `l` and `r`.
    ///
    /// This is the instrument path: the plugin is fed silence on any input port it declares.
    ///
    /// Requests longer than the `max_frames` this unit was opened with are split into several
    /// `process` calls, so nothing here allocates or resizes. A plugin whose main output is mono
    /// is added to both sides; extra channels beyond the first two are ignored, which is what the
    /// stereo mixer can carry.
    pub fn render_add(&mut self, l: &mut [f32], r: &mut [f32]) {
        let frames = l.len().min(r.len());
        let mut done = 0;

        while done < frames {
            let chunk = (frames - done).min(self.max_frames);
            self.process_chunk(chunk, PortInput::Silence);

            let left = &self.scratch[..chunk];
            let right = if self.out_channels >= 2 {
                &self.scratch[self.max_frames..self.max_frames + chunk]
            } else {
                left
            };
            for i in 0..chunk {
                l[done + i] += left[i];
                r[done + i] += right[i];
            }

            // Every queued event is stamped at frame 0, so it belongs to the first chunk only.
            // Clearing here also satisfies the contract that a render drains the event queue.
            self.input_events.clear();
            done += chunk;
        }

        self.input_events.clear();
        self.pending_events = false;
    }

    /// Runs the plugin over `l` and `r` and writes its output back over them.
    ///
    /// This is the effect path, and it **replaces**: an insert hears the signal reaching it and
    /// decides entirely what leaves the slot, wet/dry included. The caller's buffers are copied
    /// into this unit's own input before the plugin runs and overwritten from its output
    /// afterwards, so the plugin never sees the mixer's memory and `l` and `r` may be any two
    /// buffers the caller likes, including ones it reuses for the rest of the chain.
    ///
    /// Blocks longer than `max_frames` are split into several `process` calls, exactly as in
    /// [`render_add`](Self::render_add), and nothing here allocates. A plugin with a mono input
    /// hears the mean of the two sides; one with a mono output is written to both; channels
    /// beyond the first two are fed silence and ignored respectively, which is what the stereo
    /// mixer can carry. A plugin that declares no input at all — rare, but legal for an effect —
    /// still has its output written over the caller's buffers.
    pub fn process_replace(&mut self, l: &mut [f32], r: &mut [f32]) {
        let frames = l.len().min(r.len());
        let mut done = 0;

        while done < frames {
            let chunk = (frames - done).min(self.max_frames);
            self.fill_input(&l[done..done + chunk], &r[done..done + chunk]);
            self.process_chunk(chunk, PortInput::Audio);

            let left = &self.scratch[..chunk];
            let right = if self.out_channels >= 2 {
                &self.scratch[self.max_frames..self.max_frames + chunk]
            } else {
                left
            };
            l[done..done + chunk].copy_from_slice(left);
            r[done..done + chunk].copy_from_slice(right);

            // As in `render_add`: frame-zero events belong to the first chunk only.
            self.input_events.clear();
            done += chunk;
        }

        self.input_events.clear();
        self.pending_events = false;
    }

    /// Copies one chunk of the caller's stereo pair into the plugin's input buffer.
    ///
    /// Channels past the second keep the zeros they were opened with: the mixer has nothing to
    /// put there, and silence is the only honest filler.
    fn fill_input(&mut self, l: &[f32], r: &[f32]) {
        let frames = l.len();
        match self.in_channels {
            0 => {}
            // Mono in: the centre of the pair, not an arbitrary side, so nothing is lost when the
            // two differ.
            1 => {
                let mono = &mut self.input[..frames];
                for (sample, (left, right)) in mono.iter_mut().zip(l.iter().zip(r)) {
                    *sample = 0.5 * (left + right);
                }
            }
            _ => {
                self.input[..frames].copy_from_slice(l);
                self.input[self.max_frames..self.max_frames + frames].copy_from_slice(r);
            }
        }
    }

    /// One `process` call of at most `max_frames`, leaving the result in `scratch`.
    fn process_chunk(&mut self, frames: usize, port_input: PortInput) {
        let Self {
            processor,
            input_events,
            output_events,
            in_ports,
            out_ports,
            input,
            scratch,
            in_channels,
            max_frames,
            ..
        } = self;

        // A plugin is free to return without writing anything — one that reports `Sleep` usually
        // does — so the samples this chunk is about to read have to start at zero rather than hold
        // whatever the previous chunk left there. Only the frames in play are touched.
        for channel in scratch.chunks_mut(*max_frames) {
            channel[..frames].fill(0.0);
        }
        output_events.clear();

        let mut inputs = if *in_channels == 0 {
            InputAudioBuffers::empty()
        } else {
            in_ports.with_input_buffers([AudioPortBuffer {
                latency: 0,
                channels: AudioPortBufferType::f32_input_only(input.chunks_mut(*max_frames).map(
                    |channel| match port_input {
                        PortInput::Silence => InputChannel::constant(channel),
                        PortInput::Audio => InputChannel::variable(channel),
                    },
                )),
            }])
        };
        let mut outputs = out_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only(scratch.chunks_mut(*max_frames)),
        }]);

        // The buffers are sized for max_frames; a shorter chunk is expressed by narrowing the
        // frame count rather than by re-slicing, which would need a fresh port table.
        let frames = frames as u32;
        inputs.truncate(frames);
        outputs.truncate(frames);

        let events = input_events.as_input();
        let mut out_events = output_events.as_output();

        let status = match processor.ensure_processing_started() {
            Ok(started) => started.process(&inputs, &mut outputs, &events, &mut out_events, None, None),
            Err(err) => Err(err),
        };

        self.status = match status {
            Ok(status) => status,
            Err(err) => {
                // `scratch` is already zeroed, so a failed block is silence rather than noise.
                log::warn!("plugin process failed: {err}");
                ProcessStatus::Sleep
            }
        };
    }

    fn push<E: AsRef<UnknownEvent> + ?Sized>(&mut self, event: &E) {
        self.input_events.push(event);
        self.pending_events = true;
    }
}

/// Targets one key on the first note port and first channel, with no note id: the engine plays
/// plain MIDI-shaped notes and does not track per-note expression.
fn pckn(pitch: u8) -> Pckn {
    Pckn {
        port_index: Match::Specific(0),
        channel: Match::Specific(0),
        key: Match::Specific(u16::from(pitch)),
        note_id: Match::All,
    }
}
