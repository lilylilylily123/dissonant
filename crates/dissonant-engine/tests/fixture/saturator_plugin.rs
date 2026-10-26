// A real CLAP effect, built as this crate's fixture for the insert chain.
//
// The engine's chain is made of hosted plugins, so proving anything about it needs genuine
// plugins: `dissonant-plugins` has its own fixtures for the hosting itself, and this one exists
// because the question the *engine* has to answer is different. A chain's order is only
// observable through an effect that does not commute with the next one, and a linear filter or a
// plain gain commutes with anything — put two of them either way round and the output is the
// same. So the fixture is a saturator: `tanh(drive * x)`, the most ordinary nonlinearity in
// audio, which makes "boost then trim" and "trim then boost" measurably different signals.
//
// Stateless on purpose. Block-splitting, port folding and state blobs are the plugin layer's
// contract and are tested there; here the only thing that must hold is that what the chain hands
// the plugin comes back changed, in order, and only when the slot is enabled.
//
// The drive is a const generic rather than a parameter so the fixture needs no `params`
// extension and no host-side setup: each build *is* its setting. `include!`d into a `mod` by each
// of its two entry points, so it carries neither a `mod` nor inner attributes of its own.

use clack_extensions::audio_ports::{
    AudioPortFlags, AudioPortInfo, AudioPortInfoWriter, AudioPortType, PluginAudioPorts, PluginAudioPortsImpl,
};
use clack_plugin::prelude::*;

pub const PLUGIN_VENDOR: &str = "Dissonant";
pub const PLUGIN_VERSION: &str = "0.4.2";

/// Drive of each build, in thousandths. One pushes the signal well into the knee, the other
/// pulls it back out; neither is a plain gain, so their order is audible.
pub const BOOST_MILLI: u32 = 6_000;
pub const TRIM_MILLI: u32 = 200;

/// Widest block this fixture will process in one call, and the only reason it needs no allocator.
const MAX_BLOCK: usize = 4096;

/// Stereo is all the mixer carries, either side of a slot.
const MAX_CHANNELS: usize = 2;

/// Below this the saturator has nothing left to ring with — it is stateless, so this is simply
/// "the input went quiet" — and it asks to be put to sleep.
const SILENCE: f32 = 1e-9;

// ─── main thread ─────────────────────────────────────────────────────────────────────────────

pub struct SaturatorShared;

impl PluginShared<'_> for SaturatorShared {}

pub struct SaturatorMainThread<const DRIVE_MILLI: u32>;

impl<const DRIVE_MILLI: u32> PluginMainThread<'_, SaturatorShared> for SaturatorMainThread<DRIVE_MILLI> {}

impl<const DRIVE_MILLI: u32> PluginAudioPortsImpl for SaturatorMainThread<DRIVE_MILLI> {
    fn count(&self, _is_input: bool) -> u32 {
        1
    }

    fn get(&self, index: u32, is_input: bool, writer: &mut AudioPortInfoWriter) {
        if index != 0 {
            return;
        }
        writer.set(&AudioPortInfo {
            id: ClapId::new(0),
            name: if is_input { b"Audio In" } else { b"Audio Out" },
            channel_count: MAX_CHANNELS as u32,
            flags: AudioPortFlags::IS_MAIN,
            port_type: Some(AudioPortType::STEREO),
            // Out of place, which is every host's fallback and what Dissonant's host does.
            in_place_pair: None,
        });
    }
}

// ─── audio thread ────────────────────────────────────────────────────────────────────────────

pub struct SaturatorAudio<const DRIVE_MILLI: u32>;

impl<'a, const DRIVE_MILLI: u32> PluginAudioProcessor<'a, SaturatorShared, SaturatorMainThread<DRIVE_MILLI>>
    for SaturatorAudio<DRIVE_MILLI>
{
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &SaturatorMainThread<DRIVE_MILLI>,
        _shared: &'a SaturatorShared,
        _audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(SaturatorAudio)
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        _events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let drive = DRIVE_MILLI as f32 / 1_000.0;
        let frames = (audio.frames_count() as usize).min(MAX_BLOCK);

        // The input port has to be read out in full before the output port can be borrowed.
        let mut source = [[0.0f32; MAX_BLOCK]; MAX_CHANNELS];
        {
            let port = audio.input_port(0).ok_or(PluginError::Message("no input port"))?;
            let channels = port
                .channels()?
                .into_f32()
                .ok_or(PluginError::Message("host asked for f64 audio"))?;
            for (index, slot) in source.iter_mut().enumerate() {
                // One channel in: both sides hear it. None at all: silence, and the saturator
                // of silence is silence.
                let taken = channels.channel(index as u32).or_else(|| channels.channel(0));
                if let Some(taken) = taken {
                    let n = frames.min(taken.len());
                    slot[..n].copy_from_slice(&taken[..n]);
                }
            }
        }

        let mut port = audio.output_port(0).ok_or(PluginError::Message("no output port"))?;
        let mut channels = port
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("host asked for f64 audio"))?;

        let mut sounding = false;
        for (index, out) in channels.iter_mut().enumerate().take(MAX_CHANNELS) {
            for (sample, x) in out[..frames].iter_mut().zip(&source[index][..frames]) {
                *sample = (drive * x).tanh();
                sounding |= sample.abs() > SILENCE;
            }
        }

        Ok(if sounding {
            ProcessStatus::Continue
        } else {
            ProcessStatus::Sleep
        })
    }
}

// ─── plugin ──────────────────────────────────────────────────────────────────────────────────

pub struct SaturatorPlugin<const DRIVE_MILLI: u32>;

impl<const DRIVE_MILLI: u32> SaturatorPlugin<DRIVE_MILLI> {
    /// Identity, one per build, so two copies of this code in one directory are still two
    /// different plugins to the host.
    pub const ID: &'static str = match DRIVE_MILLI {
        BOOST_MILLI => "org.dissonant.test.saturator.boost",
        _ => "org.dissonant.test.saturator.trim",
    };

    pub const NAME: &'static str = match DRIVE_MILLI {
        BOOST_MILLI => "Dissonant Test Saturator Boost",
        _ => "Dissonant Test Saturator Trim",
    };
}

impl<const DRIVE_MILLI: u32> Plugin for SaturatorPlugin<DRIVE_MILLI> {
    type AudioProcessor<'a> = SaturatorAudio<DRIVE_MILLI>;
    type Shared<'a> = SaturatorShared;
    type MainThread<'a> = SaturatorMainThread<DRIVE_MILLI>;

    fn declare_extensions(builder: &mut PluginExtensions<Self>, _shared: Option<&SaturatorShared>) {
        builder.register::<PluginAudioPorts>();
    }
}

impl<const DRIVE_MILLI: u32> DefaultPluginFactory for SaturatorPlugin<DRIVE_MILLI> {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::*;

        PluginDescriptor::new(Self::ID, Self::NAME)
            .with_vendor(PLUGIN_VENDOR)
            .with_version(PLUGIN_VERSION)
            .with_description("A saturator, for exercising Dissonant's insert chains.")
            .with_features([AUDIO_EFFECT, DISTORTION, STEREO])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<SaturatorShared, PluginError> {
        Ok(SaturatorShared)
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        _shared: &'a SaturatorShared,
    ) -> Result<SaturatorMainThread<DRIVE_MILLI>, PluginError> {
        Ok(SaturatorMainThread)
    }
}

/// The two builds this fixture ships as, one `cdylib` each.
pub type BoostEntry = SinglePluginEntry<SaturatorPlugin<BOOST_MILLI>>;
pub type TrimEntry = SinglePluginEntry<SaturatorPlugin<TRIM_MILLI>>;
