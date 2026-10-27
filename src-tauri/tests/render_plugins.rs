//! What the offline render is handed, and what it admits it could not get.
//!
//! The export path opens its own plugin instances per pass. A uid this machine cannot resolve
//! must come back as a name the UI can print — the old behaviour, falling through to the
//! built-in voice, wrote a finished-looking file of a mix nobody made.

use dissonant_app_lib::{open_render_plugins, plugins::PluginHostThread};
use dissonant_core::{Insert, InsertTarget, ProjectModel, TrackPlugin};

const SR: f64 = 48_000.0;
const NOWHERE: &str = "clap:/nowhere/Nothing.clap:com.example.nothing";

fn model() -> ProjectModel {
    let mut m = ProjectModel::empty();
    m.tracks[0].plugin = Some(TrackPlugin {
        uid: NOWHERE.into(),
        name: "Dexed".into(),
        state: None,
    });
    m.tracks[0].inserts = vec![Insert::new(NOWHERE, "lamb")];
    m.master_inserts = vec![Insert::new(NOWHERE, "Pro-Q")];
    m
}

#[test]
fn a_plugin_that_cannot_open_is_named_rather_than_substituted() {
    // No cache file: a test must not race the real app's scan cache in the user's cache dir.
    let host = PluginHostThread::spawn(None);
    let m = model();
    let rig = open_render_plugins(&host, &m, SR);

    assert!(
        rig.opened.is_empty(),
        "nothing resolvable was asked for, yet something opened"
    );
    assert!(
        rig.plugins.instruments.is_empty(),
        "a plugin that would not open was replaced by some other instrument"
    );
    assert_eq!(
        rig.missing,
        ["Dexed", "lamb", "Pro-Q"],
        "the export did not report what it went without"
    );

    // The slot still holds its place with no unit, so everything after it keeps its index.
    let targets: Vec<InsertTarget> = rig.plugins.inserts.iter().map(|(t, _)| *t).collect();
    assert_eq!(
        targets,
        [InsertTarget::Track(m.tracks[0].id), InsertTarget::Master]
    );
    for (_, chain) in &rig.plugins.inserts {
        assert_eq!(chain.len(), 1);
        assert!(chain[0].unit.is_none());
    }
}

#[test]
fn a_muted_track_costs_the_render_no_plugin_loads() {
    let host = PluginHostThread::spawn(None);
    let mut m = model();
    m.tracks[0].muted = true;
    let rig = open_render_plugins(&host, &m, SR);

    // A muted track is never fed a note, so a stem pass must not pay to load its rig — and its
    // plugins are not missing from a mix they were never in.
    assert_eq!(rig.missing, ["Pro-Q"]);
    assert_eq!(
        rig.plugins.inserts.len(),
        1,
        "a muted track's chain was opened anyway"
    );
}
