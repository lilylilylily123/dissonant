//! A second fixture: the most minimal CLAP plugin that is still legal.
//!
//! It declares no extensions at all — no parameters, no state, not even audio ports — which is the
//! case the host has to degrade gracefully for rather than panic on. It is also an effect, so the
//! scanner's instrument-versus-effect decision has something to disagree with the sine fixture
//! about.

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
        _audio: Audio,
        _events: Events,
    ) -> Result<ProcessStatus, PluginError> {
        // Produces nothing and asks to be left alone, which is exactly what it claims to be.
        Ok(ProcessStatus::Sleep)
    }
}

clack_plugin::clack_export_entry!(SinglePluginEntry<BarePlugin>);
