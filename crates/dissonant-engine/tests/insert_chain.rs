//! Behavioural audit of the effect insert chains: a track's, and the master's.
//!
//! Every test here runs a *real* CLAP effect through the engine. They are built from this
//! crate's own fixture (`tests/fixture/`) and compiled as `cdylib` examples, so nothing is
//! mocked, nothing depends on what happens to be installed on the machine, and nothing is
//! skipped: if a fixture is missing that is a failure, not a reason to pass quietly.
//!
//! The fixture is a saturator at two drives — `boost` and `trim` — because the two questions a
//! chain has to answer are "did the signal go through this slot?" and "in which order?", and
//! only a nonlinear effect can answer the second one. Two gains, or two filters, commute.

use dissonant_core::{InsertTarget, NoteEvent, ProjectModel, Sequence, TICKS_PER_BEAT};
use dissonant_engine::engine::{Engine, EngineCommand, Shared};
use dissonant_engine::render::{render_to_buffer, RenderOptions, RenderPlugins};
use dissonant_engine::InsertSlot;
use dissonant_plugins::{
    HostedPlugin, PluginAudioUnit, PluginDescriptor, PluginFormat, PluginKind,
};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

/// The fixture's own identity, pulled from its source so a rename cannot silently pass. Only its
/// constants are read here — the plugin itself runs from the `cdylib` copy.
mod fixture {
    #![allow(dead_code)]

    include!("fixture/saturator_plugin.rs");
}

type Boost = fixture::SaturatorPlugin<{ fixture::BOOST_MILLI }>;
type Trim = fixture::SaturatorPlugin<{ fixture::TRIM_MILLI }>;

const SR: f32 = 44_100.0;
const MAX_FRAMES: u32 = 512;

/// Where `cargo test` left a compiled fixture example.
fn example_dylib(stem: &str) -> PathBuf {
    let mut dir = std::env::current_exe().expect("test binary has a path");
    dir.pop(); // the test binary itself
    if dir.ends_with("deps") {
        dir.pop();
    }
    let file = if cfg!(target_os = "windows") {
        format!("{stem}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{stem}.dylib")
    } else {
        format!("lib{stem}.so")
    };
    let path = dir.join("examples").join(&file);
    assert!(
        path.is_file(),
        "fixture plugin {} was not built; `cargo test` is expected to build example targets",
        path.display()
    );
    path
}

/// A descriptor for one of the two fixture builds, as the scanner would have produced it.
fn descriptor(stem: &str, plugin_id: &str, name: &str) -> PluginDescriptor {
    let path = example_dylib(stem);
    PluginDescriptor {
        uid: format!("clap:{}:{plugin_id}", path.display()),
        format: PluginFormat::Clap,
        path,
        plugin_id: plugin_id.to_string(),
        name: name.to_string(),
        vendor: fixture::PLUGIN_VENDOR.to_string(),
        version: fixture::PLUGIN_VERSION.to_string(),
        kind: PluginKind::Effect,
        features: vec!["audio-effect".into(), "distortion".into()],
    }
}

fn boost() -> PluginDescriptor {
    descriptor("dissonant_saturator_boost_fixture", Boost::ID, Boost::NAME)
}

fn trim() -> PluginDescriptor {
    descriptor("dissonant_saturator_trim_fixture", Trim::ID, Trim::NAME)
}

/// Loads an effect the way `src-tauri` does: the plugin handle stays on the app side (here, the
/// test's own stack) and only the audio unit crosses into the engine. The handle must outlive
/// the unit, which is why every caller holds on to what this returns.
fn open(desc: &PluginDescriptor) -> (HostedPlugin, Box<PluginAudioUnit>) {
    let (hosted, unit) =
        HostedPlugin::open(desc, SR as f64, MAX_FRAMES).expect("open fixture effect");
    (hosted, Box::new(unit))
}

/// Model with one track holding one long note, so the chain has something to chew on.
fn model() -> ProjectModel {
    let mut m = ProjectModel::empty();
    m.tracks[0].voice = "sine".into();
    m.patterns[0].length_ticks = 8 * TICKS_PER_BEAT;
    m.patterns[0].notes = vec![NoteEvent::new(0, 4 * TICKS_PER_BEAT, 69)];
    m
}

fn engine_for(m: &ProjectModel) -> Engine {
    let mut e = Engine::new(SR, Arc::new(Shared::default()), &m.master);
    let seq = Sequence::from_pattern(m, &m.patterns[0].id, 0).expect("pattern");
    e.handle(EngineCommand::SetSequence(Arc::new(seq)));
    e
}

/// One block of interleaved stereo, left channel only (the note is centred, so the two sides
/// carry the same signal and one of them is the whole story).
fn run(e: &mut Engine, frames: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; frames * 2];
    e.process(&mut out);
    out.iter().step_by(2).copied().collect()
}

fn peak(xs: &[f32]) -> f32 {
    xs.iter().fold(0.0f32, |p, s| p.max(s.abs()))
}

fn rms(xs: &[f32]) -> f64 {
    (xs.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / xs.len() as f64).sqrt()
}

/// Adds an effect to a chain and returns the plugin handle to keep alive, plus the slot id.
fn add(
    e: &mut Engine,
    target: InsertTarget,
    index: usize,
    desc: &PluginDescriptor,
) -> (HostedPlugin, Uuid) {
    let (hosted, unit) = open(desc);
    let id = Uuid::new_v4();
    e.handle(EngineCommand::AddInsert {
        target,
        index,
        slot: InsertSlot::new(id, Some(unit)),
    });
    (hosted, id)
}

#[test]
fn an_insert_changes_what_a_track_renders() {
    let m = model();
    let track = InsertTarget::Track(m.tracks[0].id);

    let mut dry_engine = engine_for(&m);
    dry_engine.handle(EngineCommand::Play);
    let dry = run(&mut dry_engine, 8_192);

    let mut wet_engine = engine_for(&m);
    let (_hosted, _id) = add(&mut wet_engine, track, 0, &boost());
    wet_engine.handle(EngineCommand::Play);
    let wet = run(&mut wet_engine, 8_192);

    // The source has to be worth saturating, or "nothing changed" would prove nothing.
    assert!(
        peak(&dry) > 0.05,
        "the test's own source is too quiet to judge: peak {}",
        peak(&dry)
    );
    // A saturator driven at 6× lifts the body of the signal and flattens its tops, so the
    // loudness moves a long way, not by a rounding error.
    assert!(
        rms(&wet) > rms(&dry) * 1.5,
        "the insert did not reach the mix (dry rms {}, wet rms {})",
        rms(&dry),
        rms(&wet)
    );
    assert!(
        peak(&wet) > peak(&dry),
        "saturation should not make the track quieter"
    );
}

#[test]
fn a_bypassed_insert_is_bit_identical_to_no_insert() {
    let m = model();
    let track = InsertTarget::Track(m.tracks[0].id);

    let mut bare = engine_for(&m);
    bare.handle(EngineCommand::Play);
    let without = run(&mut bare, 8_192);

    let mut bypassed = engine_for(&m);
    let (_hosted, id) = add(&mut bypassed, track, 0, &boost());
    bypassed.handle(EngineCommand::SetInsertEnabled {
        target: track,
        id,
        on: false,
    });
    bypassed.handle(EngineCommand::Play);
    let with_bypassed = run(&mut bypassed, 8_192);

    // Not "close": a bypassed slot is skipped, so the samples must be the same bits. Anything
    // else means the chain is converting or copying the signal on a path that should be inert.
    assert_eq!(
        without, with_bypassed,
        "a bypassed insert altered the output"
    );

    // And re-enabling it still works — bypass must not have cost the slot its plugin.
    bypassed.handle(EngineCommand::Seek { tick: 0.0 });
    bypassed.handle(EngineCommand::SetInsertEnabled {
        target: track,
        id,
        on: true,
    });
    bypassed.handle(EngineCommand::Play);
    let re_enabled = run(&mut bypassed, 8_192);
    assert!(
        rms(&re_enabled) > rms(&without) * 1.5,
        "re-enabling the insert left it bypassed"
    );
}

#[test]
fn the_order_of_two_effects_is_audible_and_moving_one_changes_it() {
    let m = model();
    let track = InsertTarget::Track(m.tracks[0].id);

    // boost → trim: the signal is squashed flat and then pulled down.
    let mut first = engine_for(&m);
    let (_b1, boost_id) = add(&mut first, track, 0, &boost());
    let (_t1, _) = add(&mut first, track, 1, &trim());
    first.handle(EngineCommand::Play);
    let boost_then_trim = run(&mut first, 8_192);

    // trim → boost: the same two effects the other way round.
    let mut second = engine_for(&m);
    let (_t2, _) = add(&mut second, track, 0, &trim());
    let (_b2, _) = add(&mut second, track, 1, &boost());
    second.handle(EngineCommand::Play);
    let trim_then_boost = run(&mut second, 8_192);

    // Level alone understates it: what really separates the two is shape. Saturating first
    // flattens the tops of the note, pulling its peak down toward its own average; trimming
    // first keeps the signal inside the knee, so the dynamics survive the chain intact.
    let squashed = peak(&boost_then_trim) as f64 / rms(&boost_then_trim);
    let clean = peak(&trim_then_boost) as f64 / rms(&trim_then_boost);
    assert!(
        clean > squashed * 1.1,
        "chain order made no audible difference (boost→trim crest {squashed}, trim→boost crest {clean})"
    );
    assert!(
        rms(&trim_then_boost) > rms(&boost_then_trim) * 1.2,
        "chain order left the two levels alike (boost→trim rms {}, trim→boost rms {})",
        rms(&boost_then_trim),
        rms(&trim_then_boost)
    );

    // Moving the first slot to the end has to produce the second chain, not a third thing.
    // Both engines are run through the same seek and the same second block, so the voice they
    // are rendering is in the same place in both.
    for e in [&mut first, &mut second] {
        e.handle(EngineCommand::Seek { tick: 0.0 });
    }
    first.handle(EngineCommand::MoveInsert {
        target: track,
        id: boost_id,
        to: 1,
    });
    first.handle(EngineCommand::Play);
    second.handle(EngineCommand::Play);
    let moved = run(&mut first, 8_192);
    let built = run(&mut second, 8_192);
    assert!(
        (rms(&moved) - rms(&built)).abs() < rms(&built) * 0.01,
        "reordering did not match the chain built in that order (moved rms {}, built rms {})",
        rms(&moved),
        rms(&built)
    );
}

#[test]
fn removing_an_insert_returns_the_track_to_dry() {
    let m = model();
    let track = InsertTarget::Track(m.tracks[0].id);

    let mut bare = engine_for(&m);
    bare.handle(EngineCommand::Play);
    let dry = run(&mut bare, 8_192);

    let mut e = engine_for(&m);
    let (hosted, id) = add(&mut e, track, 0, &boost());
    e.handle(EngineCommand::Play);
    let wet = run(&mut e, 8_192);
    assert!(rms(&wet) > rms(&dry) * 1.5);

    // The bare engine takes the same seek, so both are rendering the same part of the same
    // note. They are not bit-identical: the master's filters and send still carry state from
    // the block the saturator was loud in, and the limiter carries a lookahead buffer full of
    // it plus a gain envelope still releasing. Those all die out inside the first half, so the
    // question — is the slot gone? — is asked of the second half, where the level has to be the
    // dry one again rather than the saturated one.
    bare.handle(EngineCommand::Seek { tick: 0.0 });
    e.handle(EngineCommand::Seek { tick: 0.0 });
    e.handle(EngineCommand::RemoveInsert { target: track, id });
    bare.handle(EngineCommand::Play);
    e.handle(EngineCommand::Play);
    let dry_again = rms(&run(&mut bare, 8_192)[4_096..]);
    let after = rms(&run(&mut e, 8_192)[4_096..]);
    assert!(
        (after - dry_again).abs() < dry_again * 0.02,
        "removing the insert did not restore the dry signal (dry rms {dry_again}, after rms {after})"
    );
    // The engine dropped the unit when the slot went; the handle outlives it, as it must.
    drop(hosted);
}

#[test]
fn a_master_insert_runs_on_the_mixed_bus() {
    let m = model();

    let mut bare = engine_for(&m);
    bare.handle(EngineCommand::Play);
    let dry = run(&mut bare, 8_192);

    let mut e = engine_for(&m);
    let (_hosted, _) = add(&mut e, InsertTarget::Master, 0, &boost());
    e.handle(EngineCommand::Play);
    let wet = run(&mut e, 8_192);
    assert!(
        rms(&wet) > rms(&dry) * 1.5,
        "the master insert did not reach the output (dry rms {}, wet rms {})",
        rms(&dry),
        rms(&wet)
    );

    // Bypassing the master chain is the same bit-for-bit promise the tracks make.
    let mut bypassed = engine_for(&m);
    let (_hosted2, bid) = add(&mut bypassed, InsertTarget::Master, 0, &boost());
    bypassed.handle(EngineCommand::SetInsertEnabled {
        target: InsertTarget::Master,
        id: bid,
        on: false,
    });
    bypassed.handle(EngineCommand::Play);
    assert_eq!(
        dry,
        run(&mut bypassed, 8_192),
        "a bypassed master insert altered the output"
    );
}

/// The offline renderer is the same engine, so it has to run the same chains. Without this the
/// export is a dry mix of a project the user never mixed.
#[test]
fn the_offline_render_runs_the_insert_chain() {
    let m = model();
    let seq = Arc::new(Sequence::from_pattern(&m, &m.patterns[0].id, 0).expect("pattern"));
    let opts = RenderOptions {
        sample_rate: SR as u32,
        tail_seconds: 0.0,
        master: m.master.clone(),
        ..Default::default()
    };

    let dry = render_to_buffer(Arc::clone(&seq), &opts, RenderPlugins::default()).expect("dry");
    assert!(
        peak(&dry) > 1e-3,
        "the dry render is silent, so a difference would prove nothing"
    );

    // The handle has to outlive the unit the engine took, exactly as on the app side.
    let (_hosted, unit) = open(&boost());
    let plugins = RenderPlugins {
        instruments: vec![],
        inserts: vec![(
            InsertTarget::Track(m.tracks[0].id),
            vec![InsertSlot::new(Uuid::new_v4(), Some(unit))],
        )],
    };
    let wet = render_to_buffer(seq, &opts, plugins).expect("wet");

    assert_eq!(dry.len(), wet.len());
    assert!(
        rms(&wet) > rms(&dry) * 1.5,
        "the offline render ignored the insert chain (dry rms {}, wet rms {})",
        rms(&dry),
        rms(&wet)
    );
}
