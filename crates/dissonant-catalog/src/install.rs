//! Fetching a catalog asset and putting the `.clap` bundles it contains where a scan will find
//! them.
//!
//! Two rules shape everything here. First, bytes we did not verify never reach the plugin
//! folder: the SHA-256 is computed while the download streams, and a mismatch deletes the file
//! and stops. Second, the plugin folder is only ever touched by `rename`, so a crash or a failed
//! write cannot leave half a bundle behind for the host to try to load. That means the scratch
//! space has to live inside the destination — `rename` is only atomic within one filesystem, and
//! the system temp directory frequently is not the same one as `~/Library`.

use crate::catalog::{AssetKind, CatalogAsset, CatalogEntry};
use dissonant_plugins::PluginFormat;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Progress of one [`install`] call, in the order it happens. Exactly one of `Installed` or
/// `Failed` is reported last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallEvent {
    Started { bytes: u64 },
    Progress { done: u64, total: u64 },
    Verifying,
    Unpacking,
    Installed { paths: Vec<PathBuf> },
    Failed { message: String },
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// The vendor only ships a `.dmg`, `.pkg` or `.exe`, or published no checksum we could pin.
    /// Either way we will not run it for the user; the homepage is the honest answer.
    #[error("{name} is only published as an installer we cannot verify; download and run it yourself from {homepage}")]
    ManualInstallOnly { name: String, homepage: String },

    #[error("could not download {url}: {source}")]
    Download {
        url: String,
        #[source]
        source: Box<ureq::Error>,
    },

    #[error("{url} returned HTTP {status}")]
    HttpStatus { url: String, status: u16 },

    /// The server kept sending past the size the catalog pinned, so it is not serving the file
    /// we meant to install and there is no point waiting for the hash to say so.
    #[error("{url} is larger than the {expected} bytes the catalog pins for it")]
    Oversized { url: String, expected: u64 },

    /// The download completed but is not the file the catalog pinned. Nothing was installed.
    #[error("{name} failed its checksum: expected {expected}, got {actual}. Nothing was installed")]
    ChecksumMismatch {
        name: String,
        expected: String,
        actual: String,
    },

    #[error("could not unpack {name}: {message}")]
    Unpack { name: String, message: String },

    /// A well-formed archive that simply has no plugin in it — a packaging change on the
    /// vendor's side, which the catalog entry has to be updated for.
    #[error("{name} unpacked, but contains no .{extension} plugin")]
    NoPluginInArchive { name: String, extension: String },

    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },

    #[error("no per-user plugin folder on this system (no home directory)")]
    NoPluginDir,
}

/// Download `asset`, check it against its pinned hash, and move every `.clap` bundle it holds
/// into `dest_dir`. Returns the installed bundle paths.
///
/// `on_event` is called from this thread as work proceeds; it is the only progress channel, so
/// a caller that wants to show a bar should forward it somewhere non-blocking.
pub fn install(
    entry: &CatalogEntry,
    asset: &CatalogAsset,
    dest_dir: &Path,
    on_event: &mut dyn FnMut(InstallEvent),
) -> Result<Vec<PathBuf>, InstallError> {
    match run(entry, asset, dest_dir, on_event) {
        Ok(paths) => {
            on_event(InstallEvent::Installed {
                paths: paths.clone(),
            });
            Ok(paths)
        }
        Err(e) => {
            // Every failure reports itself the same way, so the UI needs one path, not nine.
            on_event(InstallEvent::Failed {
                message: e.to_string(),
            });
            Err(e)
        }
    }
}

fn run(
    entry: &CatalogEntry,
    asset: &CatalogAsset,
    dest_dir: &Path,
    on_event: &mut dyn FnMut(InstallEvent),
) -> Result<Vec<PathBuf>, InstallError> {
    if asset.kind == AssetKind::Installer || !is_sha256_hex(&asset.sha256) {
        return Err(InstallError::ManualInstallOnly {
            name: entry.name.clone(),
            homepage: entry.homepage.clone(),
        });
    }

    fs::create_dir_all(dest_dir).map_err(|e| InstallError::Io {
        context: format!("could not create {}", dest_dir.display()),
        source: e,
    })?;

    // Scratch lives inside the destination so the final move is a same-filesystem rename. The
    // leading dot keeps it out of the way of a plugin scan that happens to run mid-install.
    let scratch = Scratch::new(dest_dir)?;
    let archive = scratch.path().join("download");

    download(&asset.url, &archive, asset.bytes, on_event)?;

    on_event(InstallEvent::Verifying);
    let actual = sha256_file(&archive)?;
    if actual != asset.sha256.to_ascii_lowercase() {
        // `scratch` drops below, taking the unverified bytes with it.
        return Err(InstallError::ChecksumMismatch {
            name: entry.name.clone(),
            expected: asset.sha256.to_ascii_lowercase(),
            actual,
        });
    }

    on_event(InstallEvent::Unpacking);
    let unpacked = scratch.path().join("unpacked");
    fs::create_dir(&unpacked).map_err(|e| InstallError::Io {
        context: format!("could not create {}", unpacked.display()),
        source: e,
    })?;
    match asset.kind {
        AssetKind::Zip => unpack_zip(&archive, &unpacked),
        AssetKind::TarGz => unpack_tar_gz(&archive, &unpacked),
        AssetKind::Installer => unreachable!("rejected above"),
    }
    .map_err(|message| InstallError::Unpack {
        name: entry.name.clone(),
        message,
    })?;

    let extension = asset.format.extension();
    let bundles = find_bundles(&unpacked, extension);
    if bundles.is_empty() {
        return Err(InstallError::NoPluginInArchive {
            name: entry.name.clone(),
            extension: extension.to_string(),
        });
    }

    let mut installed = Vec::with_capacity(bundles.len());
    for bundle in &bundles {
        installed.push(place(bundle, dest_dir, scratch.path())?);
    }
    installed.sort();
    Ok(installed)
}

// ─── download and hashing ────────────────────────────────────────────────────────────────────

/// Stream `url` into `path`, reporting progress as the bytes arrive.
fn download(
    url: &str,
    path: &Path,
    expected_bytes: u64,
    on_event: &mut dyn FnMut(InstallEvent),
) -> Result<(), InstallError> {
    let response = ureq::get(url).call().map_err(|e| InstallError::Download {
        url: url.to_string(),
        source: Box::new(e),
    })?;

    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(InstallError::HttpStatus {
            url: url.to_string(),
            status,
        });
    }

    // The catalog's own figure is the fallback: a redirected CDN sometimes omits the header, and
    // a progress bar that reads zero is worse than one that reads the size we pinned.
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(expected_bytes);
    on_event(InstallEvent::Started { bytes: total });

    let mut reader = response.into_body().into_reader();
    let mut file = File::create(path).map_err(|e| InstallError::Io {
        context: format!("could not create {}", path.display()),
        source: e,
    })?;

    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| InstallError::Io {
            context: format!("could not read {url}"),
            source: e,
        })?;
        if n == 0 {
            break;
        }
        done += n as u64;
        // The catalog pins the size as well as the hash, so a server that keeps sending can be
        // cut off here rather than filling the user's disk before the hash check can reject it.
        if done > expected_bytes {
            return Err(InstallError::Oversized {
                url: url.to_string(),
                expected: expected_bytes,
            });
        }
        file.write_all(&buf[..n]).map_err(|e| InstallError::Io {
            context: format!("could not write {}", path.display()),
            source: e,
        })?;
        on_event(InstallEvent::Progress { done, total });
    }
    file.flush().map_err(|e| InstallError::Io {
        context: format!("could not write {}", path.display()),
        source: e,
    })?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, InstallError> {
    let mut file = File::open(path).map_err(|e| InstallError::Io {
        context: format!("could not open {}", path.display()),
        source: e,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| InstallError::Io {
            context: format!("could not read {}", path.display()),
            source: e,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

// ─── unpacking ───────────────────────────────────────────────────────────────────────────────

/// Resolve an archive member's path against `root`, rejecting anything that would escape it.
/// Archives are downloaded from the internet, so `../../.ssh/authorized_keys` is a real input.
fn safe_join(root: &Path, name: &Path) -> Option<PathBuf> {
    let mut out = root.to_path_buf();
    for part in name.components() {
        match part {
            std::path::Component::Normal(p) => out.push(p),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

fn unpack_zip(archive: &Path, into: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut member = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = member.enclosed_name().ok_or_else(|| {
            format!("archive member {:?} has an unsafe path", member.name())
        })?;
        let Some(target) = safe_join(into, &name) else {
            return Err(format!("archive member {} escapes the archive", name.display()));
        };
        if member.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = File::create(&target).map_err(|e| e.to_string())?;
        io::copy(&mut member, &mut out).map_err(|e| e.to_string())?;
        // A macOS .clap is a bundle directory whose Contents/MacOS/<name> must stay executable;
        // zip stores that bit and std::fs::File::create does not reproduce it.
        set_mode(&target, member.unix_mode())?;
    }
    Ok(())
}

fn unpack_tar_gz(archive: &Path, into: &Path) -> Result<(), String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    tar.set_preserve_permissions(true);
    tar.set_overwrite(true);
    // `unpack` already refuses members that escape the destination.
    tar.unpack(into).map_err(|e| e.to_string())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(mode) = mode {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) -> Result<(), String> {
    // Windows has no executable bit; a .clap there is a plain DLL.
    Ok(())
}

/// Every path under `root` whose own name ends in `.{extension}`, never descending into one it
/// already matched. On macOS a `.clap` is a directory, so the whole tree below it belongs to the
/// bundle; on Windows and Linux it is a single shared library.
///
/// Matching the extension exactly is also what keeps the `.vst3` and `.component` builds that
/// sit next to it in the same archive from being installed.
fn find_bundles(root: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == extension) {
                found.push(path);
            } else if entry.file_type().is_ok_and(|t| t.is_dir()) {
                queue.push(path);
            }
        }
    }
    found.sort();
    found
}

// ─── placing bundles ─────────────────────────────────────────────────────────────────────────

/// Move `bundle` into `dest_dir`, replacing any existing install of the same name.
///
/// The old copy is renamed aside first rather than deleted, so if the move in fails we can put
/// it back: the user ends up with the version they had, never with nothing.
fn place(bundle: &Path, dest_dir: &Path, scratch: &Path) -> Result<PathBuf, InstallError> {
    let name = bundle.file_name().ok_or_else(|| InstallError::Io {
        context: format!("{} has no file name", bundle.display()),
        source: io::Error::new(io::ErrorKind::InvalidInput, "unnamed bundle"),
    })?;
    let target = dest_dir.join(name);
    let backup = scratch.join("replaced").join(name);

    let replacing = target.exists();
    if replacing {
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).map_err(|e| InstallError::Io {
                context: format!("could not create {}", parent.display()),
                source: e,
            })?;
        }
        fs::rename(&target, &backup).map_err(|e| InstallError::Io {
            context: format!("could not move the existing {} aside", target.display()),
            source: e,
        })?;
    }

    match fs::rename(bundle, &target) {
        Ok(()) => {
            if replacing {
                let _ = remove_any(&backup);
            }
            Ok(target)
        }
        Err(e) => {
            if replacing {
                let _ = fs::rename(&backup, &target);
            }
            Err(InstallError::Io {
                context: format!("could not install {}", target.display()),
                source: e,
            })
        }
    }
}

fn remove_any(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// A scratch directory inside the plugin folder, removed however the install ends — including
/// the checksum-mismatch path, so unverified bytes never outlive the attempt.
struct Scratch(PathBuf);

impl Scratch {
    fn new(dest_dir: &Path) -> Result<Scratch, InstallError> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = dest_dir.join(format!(
            ".dissonant-install-{}-{nanos}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).map_err(|e| InstallError::Io {
            context: format!("could not create {}", path.display()),
            source: e,
        })?;
        Ok(Scratch(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// ─── where plugins live ──────────────────────────────────────────────────────────────────────

/// The per-user folder this platform's hosts scan for `format`, created if it does not exist.
///
/// Per-user, not system-wide: installing to `/Library` needs an admin prompt, and a plugin only
/// this user downloaded has no business being on everyone's machine.
pub fn user_plugin_dir(format: PluginFormat) -> Option<PathBuf> {
    let dir = match format {
        PluginFormat::Clap => {
            if cfg!(target_os = "macos") {
                home()?.join("Library/Audio/Plug-Ins/CLAP")
            } else if cfg!(target_os = "windows") {
                PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Programs\\Common\\CLAP")
            } else {
                home()?.join(".clap")
            }
        }
    };
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}
