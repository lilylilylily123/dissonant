//! A hosted plugin heard through the mixer.
//!
//! The plugin crate proves a CLAP plugin loads and renders; this proves the *channel* path —
//! that `Instrument::Plugin` is panned, gained and metered like any other instrument, and that
//! the engine's command seam installs and removes one without the audio thread ever loading it.
//!
//! These tests need a real CLAP plugin, which a checkout does not ship, so they resolve one from
//! the machine's own plugin folders and skip when there is none. Skipping is reported, never
//! silent: a green run that tested nothing is worse than a red one.

use dissonant_core::{NoteEvent, ProjectModel, Sequence};
use dissonant_engine::engine::{Engine, EngineCommand, Shared};
use dissonant_engine::Instrument;
use dissonant_plugins::{HostedPlugin, PluginDescriptor, PluginFormat, Scanner};
use std::sync::Arc;

const SR: f32 = 44_100.0;

/// The first instrument plugin installed on this machine, if any.
fn an_instrument() -> Option<PluginDescriptor> {
    let paths = dissonant_plugins::search_paths(PluginFormat::Clap);
    Scanner::scan(&paths)
        .into_iter()
        .find(|d| d.kind == dissonant_plugins::PluginKind::Instrument)
}

fn rms(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

/// Model with one track carrying one long note, so the plugin has something to play.
fn model() -> ProjectModel {
    let mut m = ProjectModel::empty();
    m.patterns[0].length_beats = 8.0;
    let id = m.tracks[0].id;
    m.patterns[0].notes_by_track.insert(id, vec![NoteEvent::new(0.0, 4.0, 69)]);
    m
}

fn engine_for(m: &ProjectModel) -> Engine {
    let mut e = Engine::new(SR, Arc::new(Shared::default()), &m.master);
    let seq = Sequence::from_pattern(m, &m.patterns[0].id).expect("pattern");
    e.handle(EngineCommand::SetSequence(Arc::new(seq)));
    e
}

fn run(e: &mut Engine, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    e.process(&mut out);
    out
}

#[test]
fn a_hosted_plugin_sounds_through_the_mixer_and_stops_when_it_is_cleared() {
    let Some(desc) = an_instrument() else {
        eprintln!("skipped: no CLAP instrument installed; install one to exercise this path");
        return;
    };
    let m = model();
    let track_id = m.tracks[0].id;
    let mut e = engine_for(&m);

    // Built on the app thread, exactly as `src-tauri` does it, then handed over.
    let (_hosted, unit) = HostedPlugin::open(&desc, SR as f64, 2048).expect("open plugin");
    e.handle(EngineCommand::SetInstrument { track_id, instrument: Box::new(Instrument::Plugin(Box::new(unit))) });
    e.handle(EngineCommand::Play);

    let with_plugin = run(&mut e, 22_050);
    assert!(
        rms(&with_plugin) > 1e-4,
        "{} produced no audio through the channel (rms {})",
        desc.name,
        rms(&with_plugin)
    );

    // Clearing restores the track's built-in voice. The note is still sounding, so the channel
    // stays audible — what must change is that the plugin is gone, which the drop proves by not
    // deadlocking or crashing the audio thread.
    e.handle(EngineCommand::Seek { beat: 0.0 });
    e.handle(EngineCommand::ClearInstrument { track_id });
    e.handle(EngineCommand::Play);
    let after = run(&mut e, 22_050);
    assert!(rms(&after) > 1e-4, "clearing the plugin left the track silent instead of back on its voice");
}

#[test]
fn a_plugin_channel_is_panned_and_gained_like_any_other() {
    let Some(desc) = an_instrument() else {
        eprintln!("skipped: no CLAP instrument installed; install one to exercise this path");
        return;
    };
    let mut m = model();
    m.tracks[0].pan = -1.0;
    let track_id = m.tracks[0].id;
    let mut e = engine_for(&m);
    let (_hosted, unit) = HostedPlugin::open(&desc, SR as f64, 2048).expect("open plugin");
    e.handle(EngineCommand::SetInstrument { track_id, instrument: Box::new(Instrument::Plugin(Box::new(unit))) });
    e.handle(EngineCommand::Play);

    let out = run(&mut e, 22_050);
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    let right: Vec<f32> = out.iter().skip(1).step_by(2).copied().collect();
    assert!(rms(&left) > 1e-4, "hard left gave no left channel");
    assert!(
        rms(&right) < rms(&left) * 0.2,
        "pan did not reach the plugin channel (l {}, r {})",
        rms(&left),
        rms(&right)
    );
}
