//! The plugin main thread, exercised the way the Tauri commands drive it.
//!
//! `dissonant-plugins` proves a CLAP plugin loads and renders. This covers the layer above: that
//! requests cross the thread boundary and come back, that a plugin stays loaded per track, and
//! that state survives a save/load round trip — the path project files and undo depend on.
//!
//! Needs a real CLAP plugin, which a checkout does not ship, so it resolves one from the
//! machine's own plugin folders and reports a skip when there is none.

use dissonant_app_lib::plugins::PluginHostThread;
use dissonant_plugins::{PluginFormat, PluginKind};
use uuid::Uuid;

const SR: f64 = 48_000.0;

fn host() -> PluginHostThread {
    // No cache file: a test must not race the real app's scan cache in the user's cache dir.
    PluginHostThread::spawn(None)
}

#[test]
fn the_host_thread_scans_opens_and_round_trips_plugin_state() {
    let host = host();
    let found = host.scan(true);
    let Some(desc) = found.iter().find(|d| d.kind == PluginKind::Instrument).cloned() else {
        eprintln!("skipped: no CLAP instrument installed; install one to exercise this path");
        return;
    };
    assert_eq!(desc.format, PluginFormat::Clap);
    assert!(!desc.name.is_empty() && !desc.plugin_id.is_empty(), "scan returned a nameless plugin: {desc:?}");

    let track = Uuid::new_v4();
    let (_unit, name) = host.open(track, &desc.uid, SR).expect("open the plugin");
    assert_eq!(name, desc.name, "open reported a different plugin than the one asked for");

    // Parameters come back across the thread with live values, which is what lets a reopened
    // project draw its knobs where the user left them.
    let params = host.params(track);
    if !params.is_empty() {
        assert!(
            params.iter().all(|p| p.min <= p.max && p.value >= p.min && p.value <= p.max),
            "a parameter came back outside its own range"
        );
    }

    // The round trip project files and undo rely on.
    if let Some(state) = host.save_state(track) {
        assert!(!state.is_empty(), "the plugin reported state but handed back nothing");
        host.load_state(track, state).expect("restore the state we just saved");
    }

    host.close(track);
    assert!(host.params(track).is_empty(), "closing a track left its plugin loaded");
}

#[test]
fn an_unknown_plugin_is_refused_by_name_rather_than_guessed_at() {
    let host = host();
    host.scan(true);
    // `expect_err` would need the success half to be `Debug`, and a live plugin is not.
    match host.open(Uuid::new_v4(), "clap:/nowhere/Nothing.clap:com.example.nothing", SR) {
        Err(e) => assert_eq!(e, "no such plugin"),
        Ok(_) => panic!("a uid that is not installed resolved to some other plugin"),
    }
}

#[test]
fn state_cannot_be_pushed_into_an_id_that_hosts_nothing() {
    let host = host();
    let err = host.load_state(Uuid::new_v4(), vec![1, 2, 3]).expect_err("there is no plugin to load into");
    assert_eq!(err, "nothing is loaded under that id");
}

#[test]
fn every_catalog_entry_this_computer_can_install_names_a_clap_bundle() {
    // The browser only offers one-click install for assets it can verify and unpack; anything
    // else must route to the vendor page. Pinning that here stops an unpinned binary from
    // quietly becoming installable.
    let here = dissonant_catalog::Platform::current();
    for entry in dissonant_catalog::builtin() {
        let Some(asset) = entry.asset_for(here, PluginFormat::Clap) else { continue };
        match asset.kind {
            dissonant_catalog::AssetKind::Installer => {
                assert!(
                    entry.homepage.starts_with("https://"),
                    "{} can only be installed by hand but has no page to send the user to",
                    entry.name
                );
            }
            _ => assert_eq!(
                asset.sha256.len(),
                64,
                "{} is offered for one-click install without a pinned checksum",
                entry.name
            ),
        }
    }
}
