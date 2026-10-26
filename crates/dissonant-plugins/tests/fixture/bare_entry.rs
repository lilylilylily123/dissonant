//! A second fixture: the most minimal CLAP plugin that is still legal.
//!
//! It declares no extensions at all — no parameters, no state, not even audio ports — which is the
//! case the host has to degrade gracefully for rather than panic on. It is also an effect, so the
//! scanner's instrument-versus-effect decision has something to disagree with the sine fixture
//! about.
//!
//! Having no audio-ports extension, it takes whatever ports the host decided to give it. It copies
//! them straight through, which is both the only honest thing an effect with no opinions can do
//! and the one way a test can see what the host assumed.

use clack_plugin::prelude::*;

pub const PLUGIN_ID: &str = "org.dissonant.test.bare";
pub const PLUGIN_NAME: &str = "Dissonant Test Bare";

pub struct BarePlugin;
pub struct BareMainThread;
pub struct BareAudio;

impl PluginMainThread<'_, ()> for BareMainThread {}

impl Plugin for BarePlugin {
    type AudioProcessor<'a> = BareAudio;
    type Shared<'a> = ();
    type MainThread<'a> = BareMainThread;
}

impl DefaultPluginFactory for BarePlugin {
    fn get_descriptor() -> PluginDescriptor {
        use clack_plugin::plugin::features::*;

        PluginDescriptor::new(PLUGIN_ID, PLUGIN_NAME)
            .with_vendor("Dissonant")
            .with_version("0.1.0")
            .with_features([AUDIO_EFFECT, STEREO])
    }

    fn new_shared(_host: HostSharedHandle<'_>) -> Result<(), PluginError> {
        Ok(())
    }

    fn new_main_thread<'a>(
        _host: HostMainThreadHandle<'a>,
        _shared: &'a (),
    ) -> Result<BareMainThread, PluginError> {
        Ok(BareMainThread)
    }
}

impl<'a> PluginAudioProcessor<'a, (), BareMainThread> for BareAudio {
    fn activate(
        _host: HostAudioProcessorHandle<'a>,
        _main_thread: &BareMainThread,
        _shared: &'a (),
        _audio_config: PluginAudioConfiguration,
    ) -> Result<Self, PluginError> {
        Ok(BareAudio)
    }

    fn process(
        &mut self,
        _process: Process,
        mut audio: Audio,
        _events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        let frames = audio.frames_count() as usize;
        let Some(mut port) = audio.port_pair(0) else {
            // The host gave it no ports at all, which is the letter of the spec for a plugin with
            // no audio-ports extension. Nothing to do, and nothing to complain about.
            return Ok(ProcessStatus::Continue);
        };
        let mut channels = port
            .channels()?
            .into_f32()
            .ok_or(PluginError::Message("host asked for f64 audio"))?;

        for channel in channels.iter_mut() {
            match channel {
                ChannelPair::InputOutput(input, output) => {
                    output[..frames].copy_from_slice(&input[..frames]);
                }
                // The host handed over one buffer for both: the input is already in place.
                ChannelPair::InPlace(_) => {}
                // An output with nothing feeding it can only be silence.
                ChannelPair::OutputOnly(output) => output[..frames].fill(0.0),
                ChannelPair::InputOnly(_) => {}
            }
        }

        Ok(ProcessStatus::Continue)
    }
}

clack_plugin::clack_export_entry!(SinglePluginEntry<BarePlugin>);
