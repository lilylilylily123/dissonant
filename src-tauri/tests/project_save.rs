//! Saving a project must never cost the player the file they already had.
//!
//! The old `save_project` wrote the target in place: a write that failed part way through left
//! a truncated project where the work used to be. It now writes a sibling and renames, keeps
//! one backup generation, and holds on to the autosave until the rename actually lands.

use dissonant_app_lib::{with_project_extension, write_project_atomically, AppState};
use std::path::{Path, PathBuf};

/// A directory of this test's own, under the OS temp dir.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dissonant-save-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn chmod(dir: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

#[test]
fn a_save_that_cannot_be_written_leaves_the_project_and_its_autosave_alone() {
    let dir = temp_dir("readonly");
    let target = dir.join("song.dissonant");
    let autosave = dir.join("song.dissonant.autosave.json");
    std::fs::write(&target, "the work so far").unwrap();
    std::fs::write(&autosave, "{\"model\":1}").unwrap();
    chmod(&dir, 0o555);

    let state = AppState::new();
    let result = state.save_to(&target);

    // Put the directory back before asserting, or a failure leaves an undeletable mess.
    chmod(&dir, 0o755);
    assert!(result.is_err(), "a save into a read-only folder reported success");
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "the work so far",
        "the failed save damaged the file it could not replace"
    );
    assert!(autosave.exists(), "the failed save dropped the safety net");
    assert!(!dir.join("song.dissonant.tmp").exists(), "a half-written sibling was left behind");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_rename_that_fails_keeps_the_autosave_even_though_the_folder_is_writable() {
    // The target is a non-empty directory, so the write and the backup both get as far as they
    // can and only the rename fails — the case a read-only folder cannot show, because there
    // nothing can be deleted anyway.
    let dir = temp_dir("rename");
    let target = dir.join("song.dissonant");
    std::fs::create_dir_all(target.join("in the way")).unwrap();
    let autosave = dir.join("song.dissonant.autosave.json");
    std::fs::write(&autosave, "{\"model\":1}").unwrap();

    let state = AppState::new();
    assert!(state.save_to(&target).is_err(), "renaming onto a directory reported success");
    assert!(autosave.exists(), "the failed save dropped the safety net");
    assert!(!dir.join("song.dissonant.tmp").exists(), "a half-written sibling was left behind");
    assert!(
        state.snapshot().path.is_none(),
        "the document took the name of a file the save never wrote"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_save_over_an_existing_project_keeps_one_backup_generation() {
    let dir = temp_dir("backup");
    let target = dir.join("song.dissonant");
    std::fs::write(&target, "yesterday").unwrap();
    let autosave = dir.join("song.dissonant.autosave.json");
    std::fs::write(&autosave, "{\"model\":1}").unwrap();

    write_project_atomically(&target, "today").unwrap();
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "today");
    assert_eq!(
        std::fs::read_to_string(dir.join("song.dissonant.bak")).unwrap(),
        "yesterday",
        "the file that was replaced was not kept"
    );
    assert!(!dir.join("song.dissonant.tmp").exists());

    // A real save goes on to clear the autosave beside the file.
    let state = AppState::new();
    state.save_to(&target).unwrap();
    assert!(!autosave.exists(), "a saved project still offers its autosave on next launch");
    assert_eq!(state.snapshot().path.as_deref(), Some(target.display().to_string().as_str()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_with_no_extension_is_saved_as_a_project_file() {
    // The open dialog filters on `.dissonant`, so a bare name would be unopenable.
    assert_eq!(with_project_extension(PathBuf::from("/tmp/song")), PathBuf::from("/tmp/song.dissonant"));
    // A name the player chose an extension for is left as it is, including a backup copy.
    assert_eq!(with_project_extension(PathBuf::from("/tmp/song.json")), PathBuf::from("/tmp/song.json"));
    assert_eq!(
        with_project_extension(PathBuf::from("/tmp/song.v2.dissonant")),
        PathBuf::from("/tmp/song.v2.dissonant")
    );
}
