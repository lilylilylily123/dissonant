//! Behavioural audit of plugin discovery and hosting.
//!
//! Every test here loads and runs a *real* CLAP plugin. Two of them are built from this crate's own
//! fixtures (`tests/fixture/`) and compiled as `cdylib` examples, so the scanner has genuine
//! binaries to walk and the host has genuine plugin code to talk to. Nothing is mocked and nothing
//! is skipped: if the fixture is missing, that is a failure, not a reason to pass quietly.
//!
//! Pitch claims are measured with a Goertzel bin rather than eyeballed from samples, the same way
//! `dissonant-engine`'s behaviour tests do.

use std::f64::consts::PI;
use std::fs;
use std::path::{Path, PathBuf};

use dissonant_plugins::host::{HostedPlugin, ParamInfo, PluginAudioUnit, PluginError};
use dissonant_plugins::scan::{search_paths, ScanCache, Scanner};
use dissonant_plugins::{PluginDescriptor, PluginFormat, PluginKind};

/// The fixture plugin's own identity, pulled from its source so a rename cannot silently pass.
///
/// The test binary only reads its constants — the plugin itself runs from the `cdylib` copy — so
/// the bulk of it is unreferenced here on purpose.
mod fixture {
    #![allow(dead_code)]

    include!("fixture/sine_plugin.rs");
}

const SR: f64 = 48_000.0;
const MAX_FRAMES: u32 = 512;

// ─── helpers ─────────────────────────────────────────────────────────────────────────────────

/// Where `cargo test` left the compiled fixture examples.
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

fn sine_dylib() -> PathBuf {
    example_dylib("dissonant_sine_fixture")
}

/// Installs the fixture as a plain `.clap` file, the layout Windows and Linux use.
fn install_flat(dir: &Path, name: &str, source: &Path) -> PathBuf {
    let target = dir.join(format!("{name}.clap"));
    fs::copy(source, &target).expect("copy fixture");
    target
}

/// Installs the fixture as a macOS-style `.clap` bundle directory.
///
/// `with_plist` decides whether the bundle is resolvable through `Info.plist` (the normal path) or
/// only through the conventional `Contents/MacOS/<name>` location (the fallback path).
fn install_bundle(dir: &Path, name: &str, source: &Path, with_plist: bool) -> PathBuf {
    let bundle = dir.join(format!("{name}.clap"));
    let macos = bundle.join("Contents/MacOS");
    fs::create_dir_all(&macos).expect("create bundle");
    fs::copy(source, macos.join(name)).expect("copy fixture into bundle");

    if with_plist {
        fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key><string>{name}</string>
    <key>CFBundleIdentifier</key><string>org.dissonant.test.{name}</string>
    <key>CFBundlePackageType</key><string>BNDL</string>
</dict>
</plist>
"#
            ),
        )
        .expect("write Info.plist");
    }

    bundle
}

/// The single descriptor a directory holding exactly one plugin must produce.
fn only_descriptor(dir: &Path) -> PluginDescriptor {
    let mut found = Scanner::scan(&[dir.to_path_buf()]);
    assert_eq!(found.len(), 1, "expected exactly one plugin in {dir:?}");
    found.remove(0)
}

fn open(desc: &PluginDescriptor) -> (HostedPlugin, PluginAudioUnit) {
    HostedPlugin::open(desc, SR, MAX_FRAMES).expect("fixture opens")
}

/// Renders `frames` into a fresh stereo pair.
fn render(unit: &mut PluginAudioUnit, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0.0f32; frames];
    let mut r = vec![0.0f32; frames];
    unit.render_add(&mut l, &mut r);
    (l, r)
}

/// Amplitude of the sinusoidal component nearest `freq`, via Goertzel.
fn tone_amplitude(x: &[f32], freq: f64) -> f64 {
    let n = x.len() as f64;
    if n < 8.0 {
        return 0.0;
    }
    let k = (freq / SR * n).round().max(1.0);
    let w = 2.0 * PI * k / n;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s = v as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    let re = s1 - s2 * w.cos();
    let im = s2 * w.sin();
    2.0 * (re * re + im * im).sqrt() / n
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |a, v| a.max(v.abs()))
}

fn midi_hz(pitch: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(pitch) - 69.0) / 12.0)
}

fn param(params: &[ParamInfo], id: u32) -> &ParamInfo {
    params
        .iter()
        .find(|p| p.id == id)
        .unwrap_or_else(|| panic!("parameter {id} is missing from {params:?}"))
}

// ─── scanning ────────────────────────────────────────────────────────────────────────────────

#[test]
fn scanning_a_directory_fills_in_every_field_of_the_plugin_it_finds() {
    let dir = tempfile::tempdir().unwrap();
    let installed = install_flat(dir.path(), "Sine", &sine_dylib());

    let found = only_descriptor(dir.path());

    assert_eq!(found.plugin_id, fixture::PLUGIN_ID);
    assert_eq!(found.name, fixture::PLUGIN_NAME);
    assert_eq!(found.vendor, fixture::PLUGIN_VENDOR);
    assert_eq!(found.version, fixture::PLUGIN_VERSION);
    assert_eq!(found.format, PluginFormat::Clap);
    assert_eq!(found.path, installed);
    assert_eq!(
        found.uid,
        PluginDescriptor::make_uid(PluginFormat::Clap, &installed, fixture::PLUGIN_ID),
        "the uid has to name the bundle as well as the plugin, so a moved plugin is reported \
         missing rather than resolving to a different one"
    );
    assert!(
        found.features.iter().any(|f| f == "instrument"),
        "the plugin's own feature strings must survive the scan: {:?}",
        found.features
    );
}

#[test]
fn a_plugin_declaring_instrument_features_is_an_instrument_and_everything_else_is_an_effect() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    install_flat(dir.path(), "Bare", &example_dylib("dissonant_bare_fixture"));

    let found = Scanner::scan(&[dir.path().to_path_buf()]);
    assert_eq!(found.len(), 2);

    let sine = found
        .iter()
        .find(|d| d.plugin_id == fixture::PLUGIN_ID)
        .expect("sine fixture found");
    let bare = found
        .iter()
        .find(|d| d.plugin_id != fixture::PLUGIN_ID)
        .expect("bare fixture found");

    assert_eq!(sine.kind, PluginKind::Instrument);
    assert_eq!(bare.kind, PluginKind::Effect);
}

#[test]
fn a_bundle_directory_is_reported_by_its_bundle_path_not_its_executable() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = install_bundle(dir.path(), "Sine", &sine_dylib(), true);

    let found = only_descriptor(dir.path());

    assert_eq!(found.path, bundle, "the project file stores the bundle");
    assert_eq!(found.plugin_id, fixture::PLUGIN_ID);
}

#[test]
fn a_bundle_without_a_usable_info_plist_is_still_loaded_through_its_executable() {
    let dir = tempfile::tempdir().unwrap();
    let bundle = install_bundle(dir.path(), "Sine", &sine_dylib(), false);

    let found = only_descriptor(dir.path());

    assert_eq!(found.path, bundle);
    assert_eq!(found.name, fixture::PLUGIN_NAME);
}

#[test]
fn a_file_that_is_not_a_plugin_is_skipped_instead_of_sinking_the_scan() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Rubbish.clap"), b"this is not a shared object").unwrap();
    fs::create_dir_all(dir.path().join("Empty.clap/Contents/MacOS")).unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());

    let found = Scanner::scan(&[dir.path().to_path_buf()]);

    assert_eq!(
        found.len(),
        1,
        "one broken plugin must not cost the user every other plugin they own"
    );
    assert_eq!(found[0].plugin_id, fixture::PLUGIN_ID);
}

#[test]
fn plugins_nested_in_vendor_folders_are_found_but_bundle_innards_are_not_walked() {
    let dir = tempfile::tempdir().unwrap();
    let vendor = dir.path().join("Some Vendor/Series");
    fs::create_dir_all(&vendor).unwrap();
    install_flat(&vendor, "Sine", &sine_dylib());
    install_bundle(dir.path(), "Bundled", &sine_dylib(), true);

    let found = Scanner::scan(&[dir.path().to_path_buf()]);

    assert_eq!(
        found.len(),
        2,
        "the executable inside a bundle is not itself a plugin to report: {found:#?}"
    );
}

#[test]
fn a_missing_search_directory_is_simply_absent_rather_than_an_error() {
    let missing = PathBuf::from("/definitely/not/a/plugin/directory");
    assert!(Scanner::scan(&[missing]).is_empty());
}

#[test]
fn the_clap_path_environment_variable_adds_directories_that_exist() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("CLAP_PATH", dir.path());
    let with_existing = search_paths(PluginFormat::Clap);

    std::env::set_var("CLAP_PATH", "/definitely/not/a/plugin/directory");
    let with_missing = search_paths(PluginFormat::Clap);
    std::env::remove_var("CLAP_PATH");

    assert!(
        with_existing.contains(&dir.path().to_path_buf()),
        "CLAP_PATH is how developers point a host at an unreleased build: {with_existing:?}"
    );
    assert!(
        !with_missing
            .iter()
            .any(|p| p == Path::new("/definitely/not/a/plugin/directory")),
        "paths that do not exist are dropped so the result is directly walkable"
    );
}

// ─── cache ───────────────────────────────────────────────────────────────────────────────────

/// A name no real plugin would report, written into a saved cache so that a descriptor carrying it
/// can only have come from the cache and not from the file on disk.
const DOCTORED: &str = "Served From The Cache";

/// Scans `roots`, saves the result, rewrites every cached plugin name to [`DOCTORED`], and hands
/// back the reloaded cache.
///
/// This is how "did it reload?" is observed. Replacing the plugin file and watching the scan fail
/// does not work: `dlopen` keys loaded images by path, so re-loading a path this process has
/// already opened returns the image it already has, whatever is on disk now.
fn doctored_cache(cache_file: &Path, roots: &[PathBuf]) -> ScanCache {
    let mut cache = ScanCache::default();
    assert_eq!(cache.scan_cached(roots).len(), 1);
    cache.save(cache_file);

    let mut json: serde_json::Value =
        serde_json::from_slice(&fs::read(cache_file).unwrap()).unwrap();
    for (_path, entry) in json.as_object_mut().expect("the cache is a map of paths") {
        for descriptor in entry["descriptors"].as_array_mut().expect("descriptors") {
            descriptor["name"] = serde_json::Value::String(DOCTORED.to_string());
        }
    }
    fs::write(cache_file, serde_json::to_vec(&json).unwrap()).unwrap();

    ScanCache::load(cache_file)
}

/// Moves `path`'s modification time forward, the way reinstalling a plugin would.
fn touch(path: &Path) {
    let now = fs::metadata(path).unwrap().modified().unwrap() + std::time::Duration::from_secs(60);
    let file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_times(fs::FileTimes::new().set_modified(now)).unwrap();
}

/// Swaps the contents of `path` for `bytes`, the way an installer updates a plugin: a new file is
/// written alongside and renamed over the old one.
///
/// Writing over the file in place would be a test bug rather than a realistic update — the old
/// image is still mapped into this process from the previous scan, and macOS kills a process that
/// touches a modified mapping.
fn replace_plugin_file(path: &Path, bytes: &[u8]) {
    let staging = path.with_extension("staging");
    fs::write(&staging, bytes).unwrap();
    fs::rename(&staging, path).unwrap();
}

#[test]
fn an_unchanged_plugin_is_served_from_the_cache_rather_than_reloaded() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let roots = vec![dir.path().to_path_buf()];

    let mut cache = doctored_cache(&dir.path().join("plugins.json"), &roots);
    let found = cache.scan_cached(&roots);

    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].name, DOCTORED,
        "the fingerprint has not moved, so the bundle must not be dlopened a second time"
    );
}

#[test]
fn a_plugin_whose_modification_time_moved_is_reloaded() {
    let dir = tempfile::tempdir().unwrap();
    let installed = install_flat(dir.path(), "Sine", &sine_dylib());
    let roots = vec![dir.path().to_path_buf()];

    let mut cache = doctored_cache(&dir.path().join("plugins.json"), &roots);
    touch(&installed);
    let found = cache.scan_cached(&roots);

    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].name,
        fixture::PLUGIN_NAME,
        "an in-place plugin update has to be picked up without the user asking for a rescan"
    );
}

#[test]
fn a_plugin_whose_size_changed_is_reloaded() {
    let dir = tempfile::tempdir().unwrap();
    let installed = install_flat(dir.path(), "Sine", &sine_dylib());
    let roots = vec![dir.path().to_path_buf()];

    let mut cache = doctored_cache(&dir.path().join("plugins.json"), &roots);
    replace_plugin_file(&installed, b"a plugin of an entirely different length");
    let found = cache.scan_cached(&roots);

    assert!(
        !found.iter().any(|d| d.name == DOCTORED),
        "a different size means a different plugin, so the cached entry has to be discarded"
    );
}

#[test]
fn a_plugin_that_was_uninstalled_leaves_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let installed = install_flat(dir.path(), "Sine", &sine_dylib());
    let roots = vec![dir.path().to_path_buf()];
    let cache_file = dir.path().join("cache/plugins.json");

    let mut cache = ScanCache::default();
    assert_eq!(cache.scan_cached(&roots).len(), 1);
    fs::remove_file(&installed).unwrap();
    assert!(cache.scan_cached(&roots).is_empty());

    cache.save(&cache_file);
    let reloaded = ScanCache::load(&cache_file);
    let mut reloaded = reloaded;
    assert!(
        reloaded.scan_cached(&roots).is_empty(),
        "the cache tracks the machine rather than growing forever"
    );
}

#[test]
fn a_saved_cache_reports_the_same_plugins_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let roots = vec![dir.path().to_path_buf()];
    let cache_file = dir.path().join("state/plugins.json");

    let mut first = ScanCache::default();
    let expected = first.scan_cached(&roots);
    first.save(&cache_file);
    assert!(cache_file.is_file(), "save creates its parent directory");

    let mut second = ScanCache::load(&cache_file);
    assert_eq!(second.scan_cached(&roots), expected);
}

#[test]
fn a_corrupt_cache_file_degrades_to_a_full_scan() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let roots = vec![dir.path().to_path_buf()];

    let cache_file = dir.path().join("plugins.json");
    fs::write(&cache_file, b"{ this is not json").unwrap();

    let mut cache = ScanCache::load(&cache_file);
    assert_eq!(
        cache.scan_cached(&roots).len(),
        1,
        "the cache is an optimisation, never a source of truth"
    );
}

#[test]
fn a_cache_file_that_does_not_exist_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut cache = ScanCache::load(&dir.path().join("never-written.json"));
    assert!(cache.scan_cached(&[]).is_empty());
}

// ─── hosting ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_held_note_sounds_at_the_pitch_it_was_played_at() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (_plugin, mut unit) = open(&only_descriptor(dir.path()));

    assert!(unit.is_silent(), "nothing has been played yet");

    unit.note_on(69, 100);
    assert!(!unit.is_silent(), "a held note is not a channel to skip");
    let (l, r) = render(&mut unit, 4096);

    let at_pitch = tone_amplitude(&l, midi_hz(69));
    let elsewhere = tone_amplitude(&l, midi_hz(76));
    assert!(
        at_pitch > 0.3,
        "expected a strong 440 Hz component, got {at_pitch}"
    );
    assert!(
        elsewhere < at_pitch / 20.0,
        "the energy must be at the played pitch, not smeared: {elsewhere} vs {at_pitch}"
    );
    assert_eq!(l, r, "a centred mono source reaches both sides equally");
}

#[test]
fn a_second_note_adds_a_second_pitch_without_displacing_the_first() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (_plugin, mut unit) = open(&only_descriptor(dir.path()));

    unit.note_on(60, 100);
    unit.note_on(67, 100);
    let (l, _r) = render(&mut unit, 4096);

    assert!(tone_amplitude(&l, midi_hz(60)) > 0.3);
    assert!(tone_amplitude(&l, midi_hz(67)) > 0.3);
}

#[test]
fn releasing_a_note_lets_the_mixer_skip_the_channel_again() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (_plugin, mut unit) = open(&only_descriptor(dir.path()));

    unit.note_on(69, 100);
    let (sounding, _) = render(&mut unit, 1024);
    assert!(peak(&sounding) > 0.1);

    unit.note_off(69);
    let (released, _) = render(&mut unit, 1024);

    assert!(
        peak(&released) < 1e-6,
        "a released note leaves silence, not a stuck voice: peak {}",
        peak(&released)
    );
    assert!(
        unit.is_silent(),
        "the plugin asked to sleep and nothing is held, so the channel costs nothing"
    );
}

#[test]
fn all_notes_off_releases_notes_the_host_never_tracked_individually() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (_plugin, mut unit) = open(&only_descriptor(dir.path()));

    unit.note_on(60, 100);
    unit.note_on(64, 100);
    unit.note_on(67, 100);
    render(&mut unit, 256);

    unit.all_notes_off();
    let (after, _) = render(&mut unit, 1024);

    assert!(peak(&after) < 1e-6, "peak {}", peak(&after));
    assert!(unit.is_silent());
}

#[test]
fn a_queued_parameter_change_keeps_the_channel_awake_until_the_plugin_has_seen_it() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (_plugin, mut unit) = open(&only_descriptor(dir.path()));

    assert!(unit.is_silent());
    unit.set_param(fixture::PARAM_GAIN, 1.0);
    assert!(
        !unit.is_silent(),
        "skipping the render would strand the parameter change in the queue"
    );

    render(&mut unit, 256);
    assert!(unit.is_silent(), "once delivered, the channel is quiet again");
}

#[test]
fn changing_a_parameter_changes_what_the_plugin_renders() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let desc = only_descriptor(dir.path());

    let level_at = |gain: f64| {
        let (_plugin, mut unit) = open(&desc);
        unit.set_param(fixture::PARAM_GAIN, gain);
        unit.note_on(69, 127);
        let (l, _) = render(&mut unit, 4096);
        tone_amplitude(&l, midi_hz(69))
    };

    let quiet = level_at(0.25);
    let loud = level_at(1.0);

    assert!(quiet > 0.1 && loud > 0.1, "{quiet} / {loud}");
    assert!(
        (loud / quiet - 4.0).abs() < 0.2,
        "gain is linear, so four times the value is four times the amplitude: {loud} / {quiet}"
    );
}

#[test]
fn a_stepped_parameter_change_retunes_the_plugin() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (_plugin, mut unit) = open(&only_descriptor(dir.path()));

    unit.set_param(fixture::PARAM_OCTAVE, 1.0);
    unit.note_on(69, 100);
    let (l, _) = render(&mut unit, 4096);

    assert!(
        tone_amplitude(&l, 880.0) > 0.3,
        "an octave up from A440 is A880"
    );
    assert!(tone_amplitude(&l, 440.0) < 0.05);
}

#[test]
fn render_add_sums_into_the_buffer_it_is_given() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (_plugin, mut unit) = open(&only_descriptor(dir.path()));

    unit.note_on(69, 100);
    let mut l = vec![1.0f32; 512];
    let mut r = vec![-1.0f32; 512];
    unit.render_add(&mut l, &mut r);

    // The first sample of a sine starting at phase zero is zero, so the existing content has to
    // show through untouched there, and be offset by the plugin everywhere else.
    assert_eq!(l[0], 1.0);
    assert_eq!(r[0], -1.0);
    assert!(
        l.iter().any(|v| (*v - 1.0).abs() > 0.1),
        "the mixer's existing content must be added to, not replaced"
    );
    assert!(l.iter().all(|v| v.is_finite()));
}

#[test]
fn a_block_longer_than_the_configured_maximum_still_renders_one_continuous_tone() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let desc = only_descriptor(dir.path());

    // Deliberately tiny: 4096 frames is eight process calls, and any seam between them would show
    // up as a smeared Goertzel bin.
    let (_plugin, mut unit) = HostedPlugin::open(&desc, SR, 512).expect("fixture opens");
    unit.note_on(69, 100);
    let (l, _) = render(&mut unit, 4096);

    assert!(
        tone_amplitude(&l, midi_hz(69)) > 0.3,
        "chunked rendering must not break phase continuity"
    );
    assert!(l.iter().all(|v| v.is_finite()));
}

#[test]
fn parameters_report_their_identity_range_and_current_value() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (mut plugin, _unit) = open(&only_descriptor(dir.path()));

    let params = plugin.params().to_vec();
    assert_eq!(params.len(), 2);

    let gain = param(&params, fixture::PARAM_GAIN);
    assert_eq!(gain.name, "Gain");
    assert_eq!(gain.module, "Output");
    assert_eq!(gain.min, 0.0);
    assert_eq!(gain.max, 1.0);
    assert_eq!(gain.default, fixture::GAIN_DEFAULT);
    assert_eq!(gain.value, fixture::GAIN_DEFAULT);
    assert!(!gain.is_stepped);

    let octave = param(&params, fixture::PARAM_OCTAVE);
    assert_eq!(octave.name, "Octave");
    assert_eq!(octave.module, "Tuning");
    assert_eq!(octave.min, -2.0);
    assert_eq!(octave.max, 2.0);
    assert!(
        octave.is_stepped,
        "a stepped parameter has to be drawn as a stepped knob"
    );
}

#[test]
fn parameter_values_are_re_read_so_a_restored_project_shows_the_restored_sound() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (mut plugin, _unit) = open(&only_descriptor(dir.path()));

    assert_eq!(param(plugin.params(), fixture::PARAM_GAIN).value, 0.5);

    let state = plugin.save_state().expect("fixture saves state");
    let saved = state.clone();

    // Move the parameter the way the plugin's own window would, then put the old state back.
    let changed = {
        let mut blob = saved.clone();
        blob[4..12].copy_from_slice(&0.125f64.to_le_bytes());
        blob
    };
    plugin.load_state(&changed).expect("state loads");

    let params = plugin.params();
    assert_eq!(
        param(params, fixture::PARAM_GAIN).value,
        0.125,
        "params() must re-read values, or every knob would show its factory default"
    );
    assert_eq!(
        param(params, fixture::PARAM_GAIN).default,
        0.5,
        "the default is what double-click-to-reset snaps to, and does not move"
    );
}

#[test]
fn saved_state_round_trips_a_changed_parameter() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let desc = only_descriptor(dir.path());

    let blob = {
        let (mut plugin, mut unit) = open(&desc);
        unit.set_param(fixture::PARAM_GAIN, 0.75);
        unit.set_param(fixture::PARAM_OCTAVE, -1.0);
        render(&mut unit, 64); // deliver the changes
        drop(unit);
        plugin.save_state().expect("state saves")
    };
    assert!(!blob.is_empty());

    let (mut restored, _unit) = open(&desc);
    restored.load_state(&blob).expect("state loads");

    let params = restored.params();
    assert_eq!(param(params, fixture::PARAM_GAIN).value, 0.75);
    assert_eq!(param(params, fixture::PARAM_OCTAVE).value, -1.0);
}

#[test]
fn a_rejected_state_blob_is_an_error_rather_than_a_silent_reset() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (mut plugin, _unit) = open(&only_descriptor(dir.path()));

    let err = plugin
        .load_state(b"not this plugin's state")
        .expect_err("a foreign blob is refused");
    assert!(matches!(err, PluginError::State { .. }), "{err}");
}

#[test]
fn a_plugin_with_no_extensions_is_hosted_without_parameters_or_state() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Bare", &example_dylib("dissonant_bare_fixture"));
    let (mut plugin, mut unit) = open(&only_descriptor(dir.path()));

    assert!(
        plugin.params().is_empty(),
        "a plugin with no parameters extension is ordinary, not broken"
    );

    let err = plugin.save_state().expect_err("there is no state to save");
    assert!(matches!(err, PluginError::NoStateExtension(_)), "{err}");
    assert!(
        err.to_string().contains("Dissonant Test Bare"),
        "the message has to name the plugin the user picked: {err}"
    );

    // It still renders: silence, added to whatever was already there.
    unit.note_on(69, 100);
    let mut l = vec![0.25f32; 256];
    let mut r = vec![0.25f32; 256];
    unit.render_add(&mut l, &mut r);
    assert!(l.iter().all(|v| (*v - 0.25).abs() < f32::EPSILON));
    assert!(r.iter().all(|v| (*v - 0.25).abs() < f32::EPSILON));
}

#[test]
fn opening_a_descriptor_whose_plugin_is_gone_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let mut desc = only_descriptor(dir.path());
    desc.path = dir.path().join("Moved.clap");

    let err = HostedPlugin::open(&desc, SR, MAX_FRAMES).err().expect("the bundle is not there");
    assert!(matches!(err, PluginError::Load { .. }), "{err}");
}

#[test]
fn opening_a_plugin_id_the_bundle_does_not_contain_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let mut desc = only_descriptor(dir.path());
    desc.plugin_id = "org.dissonant.test.absent".to_string();

    let err = HostedPlugin::open(&desc, SR, MAX_FRAMES).err().expect("no such plugin in the bundle");
    assert!(matches!(err, PluginError::Instantiate { .. }), "{err}");
}

#[test]
fn a_hosted_plugin_remembers_the_descriptor_it_was_opened_from() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let desc = only_descriptor(dir.path());
    let (plugin, _unit) = open(&desc);

    assert_eq!(plugin.descriptor(), &desc);
}

#[test]
fn the_audio_unit_can_be_moved_to_another_thread() {
    let dir = tempfile::tempdir().unwrap();
    install_flat(dir.path(), "Sine", &sine_dylib());
    let (_plugin, mut unit) = open(&only_descriptor(dir.path()));

    // This is the whole reason the type is split in two: the mixer lives on its own thread.
    let peak_seen = std::thread::spawn(move || {
        unit.note_on(69, 100);
        let (l, _) = render(&mut unit, 1024);
        (peak(&l), unit)
    })
    .join()
    .expect("audio thread finished");

    assert!(peak_seen.0 > 0.1);
}
