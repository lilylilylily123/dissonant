// A real, complete CLAP instrument used as the test fixture.
//
// It is compiled twice: once into the test binary, where `PluginEntry::load_from_clack` loads it
// with no dylib involved, and once as `examples/dissonant_sine_fixture`, a `cdylib` the scanner
// can find on disk. Both halves exercise the same code, so a host test and a scan test cannot
// disagree about what the fixture is.
//
// It is deliberately not a toy: it answers the audio-ports, note-ports, params and state
// extensions, renders a real sine per held note, and persists both of its parameters.
//
// `include!`d into a `mod` by both of its users, so it carries neither a `mod` nor inner
// attributes of its own.

use std::cell::Cell;
use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU64, Ordering};

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts,
    PluginAudioPortsImpl,
};
use clack_extensions::note_ports::{
    NoteDialect, NoteDialects, NotePortInfo, NotePortInfoWriter, PluginNotePorts,
    PluginNotePortsImpl,
};
use clack_extensions::params::{
    ParamDisplayWriter, ParamInfo, ParamInfoFlags, ParamInfoWriter, PluginAudioProcessorParams,
    PluginMainThreadParams, PluginParams,
};
use clack_extensions::state::{PluginState, PluginStateImpl};
use clack_plugin::events::event_types::{NoteOffEvent, NoteOnEvent, ParamValueEvent};
use clack_plugin::events::io::{InputEvents, OutputEvents};
use clack_plugin::events::Match;
use clack_plugin::prelude::*;
use clack_plugin::stream::{InputStream, OutputStream};

/// Identity, duplicated in the test's expectations so a descriptor change has to be deliberate.
pub const PLUGIN_ID: &str = "org.dissonant.test.sine";
pub const PLUGIN_NAME: &str = "Dissonant Test Sine";
pub const PLUGIN_VENDOR: &str = "Dissonant";
pub const PLUGIN_VERSION: &str = "1.2.3";

pub const PARAM_GAIN: u32 = 0;
pub const PARAM_OCTAVE: u32 = 1;

pub const GAIN_DEFAULT: f64 = 0.5;
pub const OCTAVE_DEFAULT: f64 = 0.0;

/// How many notes can sound at once. Fixed so the audio processor never allocates.
const VOICES: usize = 8;

/// Marker on the saved blob, so a truncated or foreign blob is rejected rather than misread.
const STATE_MAGIC: [u8; 4] = *b"DSIN";

// ─── shared state ────────────────────────────────────────────────────────────────────────────

/// Both parameters, reachable from the main thread and the audio thread.
///
/// Stored as raw `f64` bits because that is the widest value CLAP parameters carry and an atomic
/// keeps the audio thread lock-free.
pub struct SineShared {
    gain: AtomicU64,
    octave: AtomicU64,
}

impl SineShared {
    fn get(&self, id: u32) -> Option<f64> {
        match id {
            PARAM_GAIN => Some(f64::from_bits(self.gain.load(Ordering::Relaxed))),
            PARAM_OCTAVE => Some(f64::from_bits(self.octave.load(Ordering::Relaxed))),
            _ => None,
        }
    }

    fn set(&self, id: u32, value: f64) {
        match id {
            PARAM_GAIN => self.gain.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed),
            PARAM_OCTAVE => self
                .octave
                .store(value.clamp(-2.0, 2.0).trunc().to_bits(), Ordering::Relaxed),
            _ => {}
        }
    }

    /// Applies every parameter change in `events`, ignoring anything else.
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

impl PluginShared<'_> for SineShared {}

// ─── main thread ─────────────────────────────────────────────────────────────────────────────

pub struct SineMainThread<'a> {
    shared: &'a SineShared,
    /// Counts `on_main_thread` calls; nothing in the fixture requests them, but the field keeps
    /// the type honest about being main-thread-only.
    callbacks: Cell<u32>,
}

impl<'a> PluginMainThread<'a, SineShared> for SineMainThread<'a> {
    fn on_main_thread(&self) {
        self.callbacks.set(self.callbacks.get() + 1);
    }
}

impl PluginAudioPortsImpl for SineMainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        // An instrument: stereo out, nothing in.
        u32::from(!is_input)
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if is_input || index != 0 {
            return;
        }
        writer.set(&AudioPortInfo {
            id: ClapId::new(0),
            name: b"Stereo Out",
            channel_count: 2,
            flags: AudioPortFlags::IS_MAIN,
            port_type: Some(AudioPortType::STEREO),
            in_place_pair: None,
        });
    }
}

impl PluginNotePortsImpl for SineMainThread<'_> {
    fn count(&self, is_input: bool) -> u32 {
        u32::from(is_input)
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut NotePortInfoWriter) {
        if !is_input || index != 0 {
            return;
        }
        writer.set(&NotePortInfo {
            id: ClapId::new(0),
            name: b"Notes In",
            preferred_dialect: Some(NoteDialect::Clap),
            supported_dialects: NoteDialects::CLAP,
        });
    }
}

impl PluginMainThreadParams for SineMainThread<'_> {
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
                max_value: 1.0,
                default_value: GAIN_DEFAULT,
            },
            1 => ParamInfo {
                id: ClapId::new(PARAM_OCTAVE),
                flags: ParamInfoFlags::IS_AUTOMATABLE | ParamInfoFlags::IS_STEPPED,
                cookie: Default::default(),
                name: b"Octave",
                module: b"Tuning",
                min_value: -2.0,
                max_value: 2.0,
                default_value: OCTAVE_DEFAULT,
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
            PARAM_GAIN => write!(writer, "{value:.2}"),
            PARAM_OCTAVE => write!(writer, "{:+}", value as i32),
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

impl PluginStateImpl for SineMainThread<'_> {
    fn save(&self, output: &mut OutputStream) -> Result<(), PluginError> {
        use std::io::Write;
        let mut blob = Vec::with_capacity(20);
        blob.extend_from_slice(&STATE_MAGIC);
        blob.extend_from_slice(&self.shared.get(PARAM_GAIN).unwrap_or(GAIN_DEFAULT).to_le_bytes());
        blob.extend_from_slice(
            &self
                .shared
                .get(PARAM_OCTAVE)
                .unwrap_or(OCTAVE_DEFAULT)
                .to_le_bytes(),
        );
        output.write_all(&blob).map_err(|_| PluginError::Message("state write failed"))
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
        self.shared.set(PARAM_OCTAVE, number(12));
        Ok(())
    }
}

// ─── audio thread ────────────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Default)]
struct Voice {
    key: u8,
    active: bool,
    phase: f64,
}

pub struct SineAudio<'a> {
    shared: &'a SineShared,
    sample_rate: f64,
    voices: [Voice; VOICES],
}

impl<'a> PluginAudioProcessor<'a, SineShared, SineMainThread<'a>> for SineAudio<'a> {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &SineMainThread<'a>,
        shared: &'a SineShared,
        audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(Self {
            shared,
            sample_rate: audio_config.sample_rate,
            voices: [Voice::default(); VOICES],
        })
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        for event in events.input {
            if let Some(note) = event.as_event::<NoteOnEvent>() {
                if let Match::Specific(key) = note.key() {
                    self.note_on(key as u8);
                }
            } else if let Some(note) = event.as_event::<NoteOffEvent>() {
                match note.key() {
                    Match::Specific(key) => self.note_off(key as u8),
                    Match::All => self.voices = [Voice::default(); VOICES],
                }
            } else if let Some(param) = event.as_event::<ParamValueEvent>() {
                if let Some(id) = param.param_id() {
                    self.shared.set(id.get(), param.value());
                }
            }
        }

        let gain = self.shared.get(PARAM_GAIN).unwrap_or(GAIN_DEFAULT);
        let octave = self.shared.get(PARAM_OCTAVE).unwrap_or(OCTAVE_DEFAULT);
        let frames = audio.frames_count() as usize;

        let mut port = audio
            .output_port(0)
            .ok_or(PluginError::Message("no output port"))?;
        let mut channels = port
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("host asked for f64 audio"))?;

        // Render the mix once, then copy it to every channel: this is a mono source panned centre.
        let mut mix = [0.0f32; 4096];
        let frames = frames.min(mix.len());

        for voice in self.voices.iter_mut().filter(|v| v.active) {
            let step = TAU * midi_hz(voice.key, octave) / self.sample_rate;
            for sample in mix.iter_mut().take(frames) {
                *sample += (voice.phase.sin() * gain) as f32;
                voice.phase += step;
                if voice.phase >= TAU {
                    voice.phase -= TAU;
                }
            }
        }

        for channel in channels.iter_mut() {
            channel[..frames].copy_from_slice(&mix[..frames]);
        }

        // Nothing is sounding and nothing will until the next event: let the host skip us.
        Ok(if self.voices.iter().any(|v| v.active) {
            ProcessStatus::Continue
        } else {
            ProcessStatus::Sleep
        })
    }
}

impl SineAudio<'_> {
    fn note_on(&mut self, key: u8) {
        // Steal the oldest slot if all voices are busy; a test never gets there, but a plugin that
        // silently drops notes would be a bad fixture.
        let slot = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or(0);
        self.voices[slot] = Voice {
            key,
            active: true,
            phase: 0.0,
        };
    }

    fn note_off(&mut self, key: u8) {
        for voice in self.voices.iter_mut() {
            if voice.active && voice.key == key {
                *voice = Voice::default();
            }
        }
    }
}

impl PluginAudioProcessorParams for SineAudio<'_> {
    fn flush(&mut self, input_parameter_changes: &InputEvents, _output: &mut OutputEvents) {
        self.shared.apply(input_parameter_changes);
    }
}

fn midi_hz(key: u8, octave: f64) -> f64 {
    440.0 * 2f64.powf((f64::from(key) - 69.0) / 12.0 + octave)
}

// ─── plugin ──────────────────────────────────────────────────────────────────────────────────

pub struct SinePlugin;

impl Plugin for SinePlugin {
    type AudioProcessor<'a> = SineAudio<'a>;
    type Shared<'a> = SineShared;
    type MainThread<'a> = SineMainThread<'a>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&SineShared>) {
        builder
            .register::<PluginAudioPorts>()
            .register::<PluginNotePorts>()
            .register::<PluginParams>()
            .register::<PluginState>();
    }
}

impl DefaultPluginFactory for SinePlugin {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::*;

        PluginDescriptor::new(PLUGIN_ID, PLUGIN_NAME)
            .with_vendor(PLUGIN_VENDOR)
            .with_version(PLUGIN_VERSION)
            .with_description("A sine per held note, for exercising Dissonant's CLAP host.")
            .with_features([INSTRUMENT, SYNTHESIZER, STEREO])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<SineShared, PluginError> {
        Ok(SineShared {
            gain: AtomicU64::new(GAIN_DEFAULT.to_bits()),
            octave: AtomicU64::new(OCTAVE_DEFAULT.to_bits()),
        })
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        shared: &'a SineShared,
    ) -> Result<SineMainThread<'a>, PluginError> {
        Ok(SineMainThread {
            shared,
            callbacks: Cell::new(0),
        })
    }
}

pub type SineEntry = SinglePluginEntry<SinePlugin>;
