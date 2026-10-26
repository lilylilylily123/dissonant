// A real, complete CLAP effect used as the fixture for the insert path.
//
// It is the counterpart of `sine_plugin.rs`: compiled into the test binary for its constants, and
// compiled three times as `cdylib` examples — stereo, mono-in and mono-out — so the host's
// channel folding has genuine plugins with genuine port layouts to talk to. The layout is a const
// generic pair rather than three copies of this file, so the three builds cannot drift apart.
//
// The processing is a one-pole low-pass followed by a gain. Both halves earn their place: the gain
// makes "did the host replace the buffer or add to it?" an arithmetic question, and the filter
// carries state across `process` calls, so a block split into chunks only matches an unsplit
// reference if the host kept the plugin's timeline intact.
//
// `include!`d into a `mod` by each of its users, so it carries neither a `mod` nor inner
// attributes of its own.

use std::sync::atomic::{AtomicU64, Ordering};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams, PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::events::event_types::ParamValueEvent;
use clack_plugin::events::io::{InputEvents, OutputEvents};
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};

pub const PLUGIN_VENDOR: &str = "Dissonant";
pub const PLUGIN_VERSION: &str = "0.4.2";

pub const PARAM_GAIN: u32 = 0;
pub const PARAM_DAMPING: u32 = 1;

/// Unity would hide a host that adds instead of replacing, so the fixture ships quiet.
pub const GAIN_DEFAULT: f64 = 0.5;
pub const GAIN_MAX: f64 = 2.0;
/// Zero damping makes the one-pole a wire: `a * x + (1 - a) * y` with `a == 1` is exactly `x`,
/// so a test can predict the output to the bit.
pub const DAMPING_DEFAULT: f64 = 0.0;

/// Widest block this fixture will process in one call, and the only reason it needs no allocator.
const MAX_BLOCK: usize = 4096;

/// Most channels either side of the filter can have. The host folds everything down to stereo.
const MAX_CHANNELS: usize = 2;

/// Below this, the plugin considers its tail finished and asks to be put to sleep.
const SILENCE: f32 = 1e-6;

/// Marker on the saved blob, so a truncated or foreign blob is rejected rather than misread.
const STATE_MAGIC: [u8; 4] = *b"DFIL";

// ─── shared state ────────────────────────────────────────────────────────────────────────────

/// Both parameters, reachable from the main thread and the audio thread.
pub struct FilterShared {
    gain: AtomicU64,
    damping: AtomicU64,
}

impl FilterShared {
    fn get(&self, id: u32) -> Option<f64> {
        match id {
            PARAM_GAIN => Some(f64::from_bits(self.gain.load(Ordering::Relaxed))),
            PARAM_DAMPING => Some(f64::from_bits(self.damping.load(Ordering::Relaxed))),
            _ => None,
        }
    }

    fn set(&self, id: u32, value: f64) {
        match id {
            PARAM_GAIN => self
                .gain
                .store(value.clamp(0.0, GAIN_MAX).to_bits(), Ordering::Relaxed),
            PARAM_DAMPING => self
                .damping
                .store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed),
            _ => {}
        }
    }

    /// Applies every parameter change in `events`, ignoring anything else. An effect is never sent
    /// notes, so there is nothing else here to read.
    fn apply(&self, events: &InputEvents) {
        for event in events {
            if let Some(event) = event.as_event::<ParamValueEvent>() {
                if let Some(id) = event.param_id() {
                    self.set(id.get(), event.value());
                }
            }
        }
    }
}

impl PluginShared<'_> for FilterShared {}

// ─── main thread ─────────────────────────────────────────────────────────────────────────────

pub struct FilterMainThread<'a, const IN: usize, const OUT: usize> {
    shared: &'a FilterShared,
}

impl<'a, const IN: usize, const OUT: usize> PluginMainThread<'a, FilterShared>
    for FilterMainThread<'a, IN, OUT>
{
}

impl<const IN: usize, const OUT: usize> PluginAudioPortsImpl for FilterMainThread<'_, IN, OUT> {
    fn count(&self, is_input: bool) -> u32 {
        u32::from(if is_input { IN > 0 } else { OUT > 0 })
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if index != 0 {
            return;
        }
        let channels = if is_input { IN } else { OUT };
        if channels == 0 {
            return;
        }
        let name: &[u8] = if is_input { b"Audio In" } else { b"Audio Out" };
        writer.set(&AudioPortInfo {
            id: ClapId::new(0),
            name,
            channel_count: channels as u32,
            flags: AudioPortFlags::IS_MAIN,
            port_type: Some(if channels == 1 {
                AudioPortType::MONO
            } else {
                AudioPortType::STEREO
            }),
            // The fixture filters out of place, which is every host's fallback anyway.
            in_place_pair: None,
        });
    }
}

impl<const IN: usize, const OUT: usize> PluginMainThreadParams for FilterMainThread<'_, IN, OUT> {
    fn count(&self) -> u32 {
        2
    }

    fn get_info(&self, param_index: u32, info: &mut ParamInfoWriter) {
        let described = match param_index {
            0 => ParamInfo {
                id: ClapId::new(PARAM_GAIN),
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Gain",
                module: b"Output",
                min_value: 0.0,
                max_value: GAIN_MAX,
                default_value: GAIN_DEFAULT,
            },
            1 => ParamInfo {
                id: ClapId::new(PARAM_DAMPING),
                flags: ParamInfoFlags::IS_AUTOMATABLE,
                cookie: Default::default(),
                name: b"Damping",
                module: b"Filter",
                min_value: 0.0,
                max_value: 1.0,
                default_value: DAMPING_DEFAULT,
            },
            _ => return,
        };
        info.set(&described);
    }

    fn get_value(&self, param_id: ClapId) -> Option<f64> {
        self.shared.get(param_id.get())
    }

    fn value_to_text(
        &self,
        param_id: ClapId,
        value: f64,
        writer: &mut ParamDisplayWriter,
    ) -> core::fmt::Result {
        use core::fmt::Write;
        match param_id.get() {
            PARAM_GAIN | PARAM_DAMPING => write!(writer, "{value:.2}"),
            _ => Err(core::fmt::Error),
        }
    }

    fn text_to_value(&self, _param_id: ClapId, text: &std::ffi::CStr) -> Option<f64> {
        text.to_str().ok()?.trim().parse().ok()
    }

    fn flush(&self, input_parameter_changes: &InputEvents, _output: &mut OutputEvents) {
        self.shared.apply(input_parameter_changes);
    }
}

impl<const IN: usize, const OUT: usize> PluginStateImpl for FilterMainThread<'_, IN, OUT> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        use std::io::Write;
        let mut blob = Vec::with_capacity(20);
        blob.extend_from_slice(&STATE_MAGIC);
        blob.extend_from_slice(&self.shared.get(PARAM_GAIN).unwrap_or(GAIN_DEFAULT).to_le_bytes());
        blob.extend_from_slice(
            &self
                .shared
                .get(PARAM_DAMPING)
                .unwrap_or(DAMPING_DEFAULT)
                .to_le_bytes(),
        );
        output
            .write_all(&blob)
            .map_err(|_| PluginError::Message("state write failed"))
    }

    fn load(&self, input: &mut InputStream) -> Result<(), PluginError> {
        use std::io::Read;
        let mut blob = [0u8; 20];
        input
            .read_exact(&mut blob)
            .map_err(|_| PluginError::Message("state read failed"))?;
        if blob[..4] != STATE_MAGIC {
            return Err(PluginError::Message("state blob is not ours"));
        }
        let number = |at: usize| f64::from_le_bytes(blob[at..at + 8].try_into().unwrap());
        self.shared.set(PARAM_GAIN, number(4));
        self.shared.set(PARAM_DAMPING, number(12));
        Ok(())
    }
}

// ─── audio thread ────────────────────────────────────────────────────────────────────────────

pub struct FilterAudio<'a, const IN: usize, const OUT: usize> {
    shared: &'a FilterShared,
    /// One filter memory per output channel; this is what makes chunking observable.
    state: [f32; MAX_CHANNELS],
}

impl<'a, const IN: usize, const OUT: usize>
    PluginAudioProcessor<'a, FilterShared, FilterMainThread<'a, IN, OUT>>
    for FilterAudio<'a, IN, OUT>
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &FilterMainThread<'a, IN, OUT>,
        shared: &'a FilterShared,
        _audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            shared,
            state: [0.0; MAX_CHANNELS],
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        self.shared.apply(events.input);

        let gain = self.shared.get(PARAM_GAIN).unwrap_or(GAIN_DEFAULT) as f32;
        let damping = self.shared.get(PARAM_DAMPING).unwrap_or(DAMPING_DEFAULT) as f32;
        // The one-pole coefficient. 1.0 is a wire, 0.0 freezes the filter where it stands.
        let coeff = 1.0 - damping;
        let frames = (audio.frames_count() as usize).min(MAX_BLOCK);

        // The input port has to be read out in full before the output port can be borrowed, and
        // the two layouts need not match, so each output channel's source is resolved up front.
        let mut source = [[0.0f32; MAX_BLOCK]; MAX_CHANNELS];
        {
            let port = audio
                .input_port(0)
                .ok_or(PluginError::Message("no input port"))?;
            let channels = port
                .channels()?
                .into_f32()
                .ok_or(PluginError::Message("host asked for f64 audio"))?;
            let given = channels.channel_count() as usize;

            for (index, slot) in source.iter_mut().enumerate().take(OUT.min(MAX_CHANNELS)) {
                let mixed: Option<&[f32]> = match given {
                    0 => None,
                    // Same width either side: straight through.
                    n if n == OUT => channels.channel(index as u32),
                    // Fewer channels in than out: every output hears the one that exists.
                    1 => channels.channel(0),
                    // More in than out: fold the pair down its middle rather than drop a side.
                    _ => {
                        let left = channels.channel(0).unwrap_or(&[]);
                        let right = channels.channel(1).unwrap_or(&[]);
                        for (sample, (l, r)) in
                            slot[..frames].iter_mut().zip(left.iter().zip(right))
                        {
                            *sample = 0.5 * (l + r);
                        }
                        None
                    }
                };
                if let Some(mixed) = mixed {
                    let taken = frames.min(mixed.len());
                    slot[..taken].copy_from_slice(&mixed[..taken]);
                }
            }
        }

        let mut port = audio
            .output_port(0)
            .ok_or(PluginError::Message("no output port"))?;
        let mut channels = port
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("host asked for f64 audio"))?;

        let mut sounding = false;
        for (index, out) in channels.iter_mut().enumerate().take(MAX_CHANNELS) {
            let state = &mut self.state[index];
            for (sample, x) in out[..frames].iter_mut().zip(&source[index][..frames]) {
                *state = coeff * x + (1.0 - coeff) * *state;
                *sample = *state * gain;
                sounding |= sample.abs() > SILENCE;
            }
        }

        // An effect sleeps only once its tail has run out; while the filter is still ringing it
        // has to keep being called, even if nothing is arriving at its input any more.
        Ok(if sounding {
            ProcessStatus::Continue
        } else {
            ProcessStatus::Sleep
        })
    }
}

impl<const IN: usize, const OUT: usize> PluginAudioProcessorParams for FilterAudio<'_, IN, OUT> {
    fn flush(&mut self, input_parameter_changes: &InputEvents, _output: &mut OutputEvents) {
        self.shared.apply(input_parameter_changes);
    }
}

// ─── plugin ──────────────────────────────────────────────────────────────────────────────────

pub struct FilterPlugin<const IN: usize, const OUT: usize>;

impl<const IN: usize, const OUT: usize> FilterPlugin<IN, OUT> {
    /// Identity, one per layout, duplicated in the test's expectations so a change has to be
    /// deliberate. Three builds of the same code in one scan directory would otherwise be
    /// indistinguishable.
    pub const ID: &'static str = match (IN, OUT) {
        (1, _) => "org.dissonant.test.filter.monoin",
        (_, 1) => "org.dissonant.test.filter.monoout",
        _ => "org.dissonant.test.filter",
    };

    pub const NAME: &'static str = match (IN, OUT) {
        (1, _) => "Dissonant Test Filter Mono In",
        (_, 1) => "Dissonant Test Filter Mono Out",
        _ => "Dissonant Test Filter",
    };
}

impl<const IN: usize, const OUT: usize> Plugin for FilterPlugin<IN, OUT> {
    type AudioProcessor<'a> = FilterAudio<'a, IN, OUT>;
    type Shared<'a> = FilterShared;
    type MainThread<'a> = FilterMainThread<'a, IN, OUT>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&FilterShared>) {
        // No note ports: an effect has nothing to say about notes, and the host must open it all
        // the same.
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginParams>()
            .register::<PluginState>();
    }
}

impl<const IN: usize, const OUT: usize> DefaultPluginFactory for FilterPlugin<IN, OUT> {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::*;

        let width = if IN == 1 && OUT == 1 { MONO } else { STEREO };
        PluginDescriptor::new(Self::ID, Self::NAME)
            .with_vendor(PLUGIN_VENDOR)
            .with_version(PLUGIN_VERSION)
            .with_description("A damped one-pole and a gain, for exercising Dissonant's inserts.")
            .with_features([AUDIO_EFFECT, FILTER, width])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<FilterShared, PluginError> {
        Ok(FilterShared {
            gain: AtomicU64::new(GAIN_DEFAULT.to_bits()),
            damping: AtomicU64::new(DAMPING_DEFAULT.to_bits()),
        })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a FilterShared,
    ) -> Result<FilterMainThread<'a, IN, OUT>, PluginError> {
        Ok(FilterMainThread { shared })
    }
}

/// The three layouts this fixture is built as, one `cdylib` each.
pub type StereoFilterEntry = SinglePluginEntry<FilterPlugin<2, 2>>;
pub type MonoInFilterEntry = SinglePluginEntry<FilterPlugin<1, 2>>;
pub type MonoOutFilterEntry = SinglePluginEntry<FilterPlugin<2, 1>>;
