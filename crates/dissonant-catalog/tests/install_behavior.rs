//! What `install` does to the plugin folder, proved end to end.
//!
//! Nothing here touches the internet: the archives are built in-test and served by a one-shot
//! HTTP server on loopback, so the real download, hash and unpack path runs exactly as it does
//! in production while the test stays offline and deterministic.

use dissonant_catalog::{install, user_plugin_dir, AssetKind, CatalogAsset, CatalogEntry, InstallError, InstallEvent, Platform};
use dissonant_plugins::{PluginFormat, PluginKind};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

// ─── fixtures ────────────────────────────────────────────────────────────────────────────────

/// One archive member: path inside the archive, contents, and unix mode.
type Member<'a> = (&'a str, &'a [u8], u32);

/// A macOS-shaped CLAP bundle plus the sibling builds a real vendor archive carries, which the
/// installer must leave alone.
fn bundle_members(binary: &'static [u8]) -> Vec<Member<'static>> {
    vec![
        ("Example.clap/Contents/Info.plist", b"<plist/>", 0o644),
        ("Example.clap/Contents/MacOS/Example", binary, 0o755),
        ("Example.vst3/Contents/Info.plist", b"<plist/>", 0o644),
        ("README.txt", b"read me", 0o644),
    ]
}

fn zip_bytes(members: &[Member]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, body, mode) in members {
        let options = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
        writer.start_file(*name, options).unwrap();
        writer.write_all(body).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn tar_gz_bytes(members: &[Member]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut builder = tar::Builder::new(encoder);
    for (name, body, mode) in members {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(*mode);
        header.set_cksum();
        builder.append_data(&mut header, *name, *body).unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap()
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

// ─── a one-shot server on loopback ───────────────────────────────────────────────────────────

/// Answers exactly one GET with `body`. Kept to one request so a test that accidentally retries
/// fails loudly instead of passing twice.
struct Host {
    url: String,
    _thread: JoinHandle<()>,
}

fn serve(body: Vec<u8>) -> Host {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/fixture", listener.local_addr().unwrap());
    let thread = thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        // The request is irrelevant; drain it so the client is not blocked writing headers.
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) if line == "\r\n" || line == "\n" => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\
             Connection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    });
    Host { url, _thread: thread }
}

// ─── entries under test ──────────────────────────────────────────────────────────────────────

fn entry() -> CatalogEntry {
    CatalogEntry {
        id: "example".into(),
        name: "Example".into(),
        vendor: "Nobody".into(),
        summary: "A plugin that exists only in this test.".into(),
        tags: vec!["free".into()],
        kind: PluginKind::Instrument,
        license: "MIT".into(),
        homepage: "https://example.invalid/example".into(),
        assets: Vec::new(),
    }
}

fn asset(kind: AssetKind, url: &str, body: &[u8], sha256_hex: &str) -> CatalogAsset {
    CatalogAsset {
        platform: Platform::current(),
        format: PluginFormat::Clap,
        kind,
        url: url.into(),
        sha256: sha256_hex.into(),
        bytes: body.len() as u64,
    }
}

fn dest() -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("CLAP");
    (root, dir)
}

fn entries_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Event stream reduced to its phases, with runs of `Progress` collapsed to one entry.
fn phases(events: &[InstallEvent]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for e in events {
        let name = match e {
            InstallEvent::Started { .. } => "started",
            InstallEvent::Progress { .. } => "progress",
            InstallEvent::Verifying => "verifying",
            InstallEvent::Unpacking => "unpacking",
            InstallEvent::Installed { .. } => "installed",
            InstallEvent::Failed { .. } => "failed",
        };
        if out.last() != Some(&name) {
            out.push(name);
        }
    }
    out
}

// ─── the happy path ──────────────────────────────────────────────────────────────────────────

#[test]
fn installing_a_verified_zip_leaves_the_clap_bundle_and_nothing_else() {
    let archive = zip_bytes(&bundle_members(b"binary-v1"));
    let host = serve(archive.clone());
    let (_root, dir) = dest();

    let installed = install(
        &entry(),
        &asset(AssetKind::Zip, &host.url, &archive, &sha256(&archive)),
        &dir,
        &mut |_| {},
    )
    .expect("a correctly pinned archive should install");

    assert_eq!(installed, vec![dir.join("Example.clap")]);
    assert_eq!(
        entries_in(&dir),
        vec!["Example.clap"],
        "the sibling .vst3, the README and the installer's own scratch space must not survive"
    );
    assert_eq!(
        std::fs::read(dir.join("Example.clap/Contents/MacOS/Example")).unwrap(),
        b"binary-v1",
        "the whole bundle tree, not just its top directory, has to be moved"
    );
}

#[test]
fn an_install_reports_its_phases_in_order() {
    let archive = zip_bytes(&bundle_members(b"binary-v1"));
    let host = serve(archive.clone());
    let (_root, dir) = dest();

    let mut events = Vec::new();
    install(
        &entry(),
        &asset(AssetKind::Zip, &host.url, &archive, &sha256(&archive)),
        &dir,
        &mut |e| events.push(e),
    )
    .unwrap();

    assert_eq!(
        phases(&events),
        ["started", "progress", "verifying", "unpacking", "installed"]
    );

    let total = archive.len() as u64;
    assert!(matches!(events[0], InstallEvent::Started { bytes } if bytes == total));

    let progress: Vec<(u64, u64)> = events
        .iter()
        .filter_map(|e| match e {
            InstallEvent::Progress { done, total } => Some((*done, *total)),
            _ => None,
        })
        .collect();
    assert!(!progress.is_empty(), "a download reports no progress at all");
    assert!(
        progress.windows(2).all(|w| w[0].0 < w[1].0),
        "progress must only ever move forwards: {progress:?}"
    );
    assert_eq!(
        progress.last(),
        Some(&(total, total)),
        "the last progress report must reach the total, or a bar never fills"
    );

    match events.last() {
        Some(InstallEvent::Installed { paths }) => {
            assert_eq!(paths, &[dir.join("Example.clap")]);
        }
        other => panic!("expected a final Installed event, got {other:?}"),
    }
}

#[test]
fn a_tar_gz_is_unpacked_the_same_way_as_a_zip() {
    let archive = tar_gz_bytes(&bundle_members(b"binary-tgz"));
    let host = serve(archive.clone());
    let (_root, dir) = dest();

    install(
        &entry(),
        &asset(AssetKind::TarGz, &host.url, &archive, &sha256(&archive)),
        &dir,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(entries_in(&dir), vec!["Example.clap"]);
    assert_eq!(
        std::fs::read(dir.join("Example.clap/Contents/MacOS/Example")).unwrap(),
        b"binary-tgz"
    );
}

#[cfg(unix)]
#[test]
fn an_installed_bundle_stays_executable() {
    use std::os::unix::fs::PermissionsExt;

    let archive = zip_bytes(&bundle_members(b"binary-v1"));
    let host = serve(archive.clone());
    let (_root, dir) = dest();

    install(
        &entry(),
        &asset(AssetKind::Zip, &host.url, &archive, &sha256(&archive)),
        &dir,
        &mut |_| {},
    )
    .unwrap();

    let mode = std::fs::metadata(dir.join("Example.clap/Contents/MacOS/Example"))
        .unwrap()
        .permissions()
        .mode();
    assert!(
        mode & 0o111 != 0,
        "a macOS bundle whose binary lost its executable bit will not load: mode {mode:o}"
    );
}

#[test]
fn reinstalling_replaces_the_old_bundle_rather_than_merging_into_it() {
    let (_root, dir) = dest();

    let first = zip_bytes(&[
        ("Example.clap/Contents/MacOS/Example", b"binary-v1", 0o755),
        ("Example.clap/Contents/stale.txt", b"from the old build", 0o644),
    ]);
    let host = serve(first.clone());
    install(
        &entry(),
        &asset(AssetKind::Zip, &host.url, &first, &sha256(&first)),
        &dir,
        &mut |_| {},
    )
    .unwrap();

    let second = zip_bytes(&[("Example.clap/Contents/MacOS/Example", b"binary-v2", 0o755)]);
    let host = serve(second.clone());
    install(
        &entry(),
        &asset(AssetKind::Zip, &host.url, &second, &sha256(&second)),
        &dir,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(entries_in(&dir), vec!["Example.clap"]);
    assert_eq!(
        std::fs::read(dir.join("Example.clap/Contents/MacOS/Example")).unwrap(),
        b"binary-v2"
    );
    assert!(
        !dir.join("Example.clap/Contents/stale.txt").exists(),
        "an upgrade left a file from the previous version inside the bundle"
    );
}

// ─── refusals ────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_zip_that_fails_its_checksum_installs_nothing_and_cleans_up_after_itself() {
    let archive = zip_bytes(&bundle_members(b"binary-v1"));
    let host = serve(archive.clone());
    let (_root, dir) = dest();

    // A perfectly well-formed hash of some other file: the download succeeds, the check does not.
    let wrong = sha256(b"not this archive");
    let mut events = Vec::new();
    let error = install(
        &entry(),
        &asset(AssetKind::Zip, &host.url, &archive, &wrong),
        &dir,
        &mut |e| events.push(e),
    )
    .expect_err("unverified bytes must never be installed");

    match &error {
        InstallError::ChecksumMismatch { expected, actual, .. } => {
            assert_eq!(expected, &wrong);
            assert_eq!(actual, &sha256(&archive));
        }
        other => panic!("expected a checksum mismatch, got {other}"),
    }

    assert_eq!(
        entries_in(&dir),
        Vec::<String>::new(),
        "the plugin folder must be empty: no bundle, and no leftover download either"
    );
    assert_eq!(
        phases(&events),
        ["started", "progress", "verifying", "failed"],
        "the caller must be told it failed at verification, not at unpacking"
    );
}

#[test]
fn an_installer_only_asset_sends_the_user_to_the_homepage() {
    let (_root, dir) = dest();
    // Nothing is listening here, so reaching the network at all would surface as a download
    // error instead — the refusal has to happen before any request.
    let unreachable = "http://127.0.0.1:1/never-fetched";

    let mut events = Vec::new();
    let error = install(
        &entry(),
        &asset(AssetKind::Installer, unreachable, b"", ""),
        &dir,
        &mut |e| events.push(e),
    )
    .expect_err("a vendor installer must not be run for the user");

    match &error {
        InstallError::ManualInstallOnly { name, homepage } => {
            assert_eq!(name, "Example");
            assert_eq!(homepage, "https://example.invalid/example");
        }
        other => panic!("expected a manual-install refusal, got {other}"),
    }
    assert!(
        error.to_string().contains("https://example.invalid/example"),
        "the message has to carry the homepage, since that is the user's only way forward: {error}"
    );
    assert_eq!(phases(&events), ["failed"]);
    assert!(!dir.exists(), "a refused install must not even create the folder");
}

#[test]
fn an_archive_with_no_pinned_checksum_is_refused_like_an_installer() {
    let (_root, dir) = dest();
    let error = install(
        &entry(),
        &asset(AssetKind::Zip, "http://127.0.0.1:1/never-fetched", b"", ""),
        &dir,
        &mut |_| {},
    )
    .expect_err("an unpinned archive is exactly as untrustworthy as an installer");
    assert!(matches!(error, InstallError::ManualInstallOnly { .. }));
}

#[test]
fn an_archive_with_no_plugin_inside_is_reported_instead_of_quietly_succeeding() {
    let archive = zip_bytes(&[
        ("Example.vst3/Contents/Info.plist", b"<plist/>", 0o644),
        ("README.txt", b"read me", 0o644),
    ]);
    let host = serve(archive.clone());
    let (_root, dir) = dest();

    let error = install(
        &entry(),
        &asset(AssetKind::Zip, &host.url, &archive, &sha256(&archive)),
        &dir,
        &mut |_| {},
    )
    .expect_err("an archive carrying only a .vst3 gives us nothing to install");

    assert!(matches!(error, InstallError::NoPluginInArchive { .. }), "{error}");
    assert_eq!(entries_in(&dir), Vec::<String>::new());
}

// ─── where the bundles go ────────────────────────────────────────────────────────────────────

#[test]
fn the_user_plugin_dir_is_the_folder_clap_hosts_actually_scan() {
    let dir = user_plugin_dir(PluginFormat::Clap).expect("every supported platform has one");
    let suffix = if cfg!(target_os = "macos") {
        "Library/Audio/Plug-Ins/CLAP"
    } else if cfg!(target_os = "windows") {
        "Programs\\Common\\CLAP"
    } else {
        ".clap"
    };
    assert!(
        dir.ends_with(suffix),
        "{} is not the per-user CLAP folder",
        dir.display()
    );
    assert!(
        dir.is_dir(),
        "the folder has to exist, or the first install has nowhere to land"
    );
}
