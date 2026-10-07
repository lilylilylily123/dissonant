//! Finding CLAP plugins that are already installed on the machine.
//!
//! Scanning means *loading* every candidate bundle: the only way to learn what is inside a `.clap`
//! is to dlopen it and ask its plugin factory. That is slow (hundreds of milliseconds for a big
//! synth) and it runs arbitrary third-party code, so two rules hold throughout this module:
//! a bundle that misbehaves is logged and skipped rather than allowed to sink the whole scan, and
//! [`ScanCache`] keeps the result on disk so the cost is paid once per installed version.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use clack_host::entry::{PluginEntry, PluginEntryError};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::{PluginDescriptor, PluginFormat, PluginKind};

/// CLAP feature strings that mean "this replaces a track's voice" rather than "this processes
/// audio". Everything else is an effect.
const INSTRUMENT_FEATURES: [&str; 2] = ["instrument", "note-effect"];

// ─── search paths ────────────────────────────────────────────────────────────────────────────

/// The directories a plugin of `format` is installed into on this platform, newest-looking first.
///
/// Paths that do not exist are dropped, so the result is directly walkable. `$CLAP_PATH` is
/// appended (split on `:` on unix, `;` on Windows, like `$PATH`): the CLAP specification lets users
/// point hosts at extra directories, and developers rely on it to test unreleased builds.
pub fn search_paths(format: PluginFormat) -> Vec<PathBuf> {
    // Exhaustive on purpose: a second format must come with its own locations, not inherit CLAP's.
    let PluginFormat::Clap = format;

    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };

    if cfg!(target_os = "macos") {
        if let Some(home) = dirs::home_dir() {
            push(home.join("Library/Audio/Plug-Ins/CLAP"));
        }
        push(PathBuf::from("/Library/Audio/Plug-Ins/CLAP"));
    } else if cfg!(target_os = "windows") {
        if let Some(common) = std::env::var_os("COMMONPROGRAMFILES") {
            push(PathBuf::from(common).join("CLAP"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            push(PathBuf::from(local).join("Programs/Common/CLAP"));
        }
    } else {
        if let Some(home) = dirs::home_dir() {
            push(home.join(".clap"));
        }
        push(PathBuf::from("/usr/lib/clap"));
        push(PathBuf::from("/usr/local/lib/clap"));
    }

    if let Some(extra) = std::env::var_os("CLAP_PATH") {
        for p in std::env::split_paths(&extra) {
            if !p.as_os_str().is_empty() {
                push(p);
            }
        }
    }

    out.retain(|p| p.is_dir());
    out
}

// ─── scanning ────────────────────────────────────────────────────────────────────────────────

/// Walks directories and reports every plugin inside every `.clap` it finds.
pub struct Scanner;

impl Scanner {
    /// Loads every `.clap` bundle under `paths` and returns one descriptor per plugin inside them.
    ///
    /// Bundles that fail to load are logged at warn level and skipped; the scan always finishes.
    pub fn scan(paths: &[PathBuf]) -> Vec<PluginDescriptor> {
        let mut out = Vec::new();
        for bundle in candidate_bundles(paths) {
            out.extend(read_bundle(&bundle));
        }
        out
    }
}

/// Every `.clap` path under `roots`, in walk order.
///
/// A `.clap` is a file on Windows and Linux and a bundle *directory* on macOS, so both are
/// accepted, and the walk never descends into one — the executable inside a bundle is not itself a
/// plugin to report.
fn candidate_bundles(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();

    for root in roots {
        let mut walk = WalkDir::new(root).follow_links(false).into_iter();
        loop {
            let entry = match walk.next() {
                None => break,
                Some(Ok(entry)) => entry,
                Some(Err(err)) => {
                    log::warn!("plugin scan: skipping unreadable path: {err}");
                    continue;
                }
            };

            if entry.path().extension() != Some(OsStr::new(PluginFormat::Clap.extension())) {
                continue;
            }
            if entry.file_type().is_dir() {
                walk.skip_current_dir();
            }
            out.push(entry.into_path());
        }
    }

    out
}

/// Loads one bundle and turns each plugin it exposes into a [`PluginDescriptor`].
///
/// Returns an empty vector (never an error) when the bundle cannot be loaded: a single broken or
/// architecture-mismatched plugin must not cost the user every other plugin they own.
fn read_bundle(path: &Path) -> Vec<PluginDescriptor> {
    let entry = match load_entry(path) {
        Ok(entry) => entry,
        Err(err) => {
            log::warn!("plugin scan: {} failed to load: {err}", path.display());
            return Vec::new();
        }
    };

    let Some(factory) = entry.get_plugin_factory() else {
        log::warn!(
            "plugin scan: {} exposes no plugin factory",
            path.display()
        );
        return Vec::new();
    };

    let mut out = Vec::new();
    for clap in factory.plugin_descriptors() {
        // The CLAP spec requires an id and a name; anything else is a broken entry we cannot
        // address later, since `uid` is built from the id.
        let Some(plugin_id) = clap.id().map(|s| s.to_string_lossy().into_owned()) else {
            log::warn!("plugin scan: {} has a plugin with no id", path.display());
            continue;
        };
        if plugin_id.is_empty() {
            log::warn!("plugin scan: {} has a plugin with a blank id", path.display());
            continue;
        }

        let features: Vec<String> = clap
            .features()
            .map(|f| f.to_string_lossy().into_owned())
            .collect();
        let kind = if features
            .iter()
            .any(|f| INSTRUMENT_FEATURES.contains(&f.as_str()))
        {
            PluginKind::Instrument
        } else {
            PluginKind::Effect
        };

        let text = |s: Option<&std::ffi::CStr>| {
            s.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        };

        out.push(PluginDescriptor {
            uid: PluginDescriptor::make_uid(PluginFormat::Clap, path, &plugin_id),
            format: PluginFormat::Clap,
            path: path.to_path_buf(),
            name: text(clap.name()),
            vendor: text(clap.vendor()),
            version: text(clap.version()),
            plugin_id,
            kind,
            features,
        });
    }

    out
}

/// Loads the CLAP entry at `path`, accepting either a plain dynamic library or a macOS bundle.
///
/// `PluginEntry::load` already handles both, resolving a bundle through its `Info.plist`. That
/// resolution fails for bundles with a missing or malformed `Info.plist` — which real plugins do
/// ship — so this falls back to the conventional `Contents/MacOS/<name>` layout. Loading through
/// the executable path still gives the entry the bundle path it expects, because clack walks back
/// up from `Contents/MacOS`.
pub(crate) fn load_entry(path: &Path) -> Result<PluginEntry, PluginEntryError> {
    // SAFETY: there is no safe way to load a CLAP entry from a file; the plugin is third-party
    // code which this host cannot vet. This is the documented boundary of the format.
    let first = match unsafe { PluginEntry::load(path) } {
        Ok(entry) => return Ok(entry),
        Err(err) => err,
    };

    let Some(executable) = bundle_executable(path) else {
        return Err(first);
    };
    log::debug!(
        "plugin scan: {} did not resolve as a bundle ({first}); trying {}",
        path.display(),
        executable.display()
    );
    // SAFETY: as above.
    unsafe { PluginEntry::load(&executable) }
}

/// The executable inside a macOS-style `.clap` bundle directory, if there is one.
fn bundle_executable(bundle: &Path) -> Option<PathBuf> {
    if !bundle.is_dir() {
        return None;
    }
    let macos = bundle.join("Contents/MacOS");

    // The conventional name is the bundle's own, minus the extension.
    if let Some(stem) = bundle.file_stem() {
        let named = macos.join(stem);
        if named.is_file() {
            return Some(named);
        }
    }

    // Some vendors name the executable differently; a bundle only ever holds one.
    fs::read_dir(&macos)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_file())
}

// ─── cache ───────────────────────────────────────────────────────────────────────────────────

/// Identity of a bundle as far as the cache is concerned: if neither changed, neither did the
/// plugins inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Fingerprint {
    /// Whole seconds and nanoseconds since the epoch, kept apart because JSON numbers are f64 and
    /// a nanosecond count does not survive that round trip.
    mtime_secs: u64,
    mtime_nanos: u32,
    size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    #[serde(flatten)]
    fingerprint: Fingerprint,
    descriptors: Vec<PluginDescriptor>,
}

/// A disk-backed memo of previous scans, keyed by bundle path.
///
/// Rescanning is only skipped when the bundle's executable has the same modification time and size
/// as when it was cached, so an in-place plugin update is picked up without the user asking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScanCache {
    entries: HashMap<PathBuf, CacheEntry>,
}

impl ScanCache {
    /// Reads the cache written by [`save`](Self::save).
    ///
    /// A missing, unreadable or corrupt file is not an error: it yields an empty cache, which makes
    /// the next scan a full one. The cache is an optimisation, never a source of truth.
    pub fn load(path: &Path) -> ScanCache {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return ScanCache::default(),
            Err(err) => {
                log::warn!("plugin cache: {} unreadable: {err}", path.display());
                return ScanCache::default();
            }
        };

        match serde_json::from_slice(&bytes) {
            Ok(cache) => cache,
            Err(err) => {
                log::warn!("plugin cache: {} is corrupt, rescanning: {err}", path.display());
                ScanCache::default()
            }
        }
    }

    /// Scans `paths`, loading only the bundles whose fingerprint changed since the last call.
    ///
    /// Entries for bundles that are no longer installed are dropped, so the cache tracks the
    /// machine rather than growing forever.
    pub fn scan_cached(&mut self, paths: &[PathBuf]) -> Vec<PluginDescriptor> {
        let bundles = candidate_bundles(paths);
        let mut fresh = HashMap::with_capacity(bundles.len());
        let mut out = Vec::new();

        for bundle in bundles {
            let fingerprint = fingerprint(&bundle);

            // Without a fingerprint there is nothing to revalidate against, so always reload.
            if let (Some(fingerprint), Some(cached)) = (fingerprint, self.entries.get(&bundle)) {
                if cached.fingerprint == fingerprint {
                    out.extend(cached.descriptors.iter().cloned());
                    fresh.insert(bundle, cached.clone());
                    continue;
                }
            }

            let descriptors = read_bundle(&bundle);
            out.extend(descriptors.iter().cloned());
            if let Some(fingerprint) = fingerprint {
                fresh.insert(
                    bundle,
                    CacheEntry {
                        fingerprint,
                        descriptors,
                    },
                );
            }
        }

        self.entries = fresh;
        out
    }

    /// Writes the cache, creating the parent directory if needed.
    ///
    /// Failures are logged rather than returned: a cache that cannot be written only costs the
    /// next scan some time, and callers have nothing useful to do about it.
    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                log::warn!("plugin cache: cannot create {}: {err}", parent.display());
                return;
            }
        }
        match serde_json::to_vec(self) {
            Ok(bytes) => {
                if let Err(err) = fs::write(path, bytes) {
                    log::warn!("plugin cache: cannot write {}: {err}", path.display());
                }
            }
            Err(err) => log::warn!("plugin cache: cannot serialise: {err}"),
        }
    }
}

/// Modification time and size of the binary a bundle path ultimately points at.
///
/// A macOS bundle directory's own mtime does not change when its executable is replaced, so the
/// executable is what gets fingerprinted.
fn fingerprint(bundle: &Path) -> Option<Fingerprint> {
    let target = bundle_executable(bundle);
    let target = target.as_deref().unwrap_or(bundle);

    let meta = fs::metadata(target).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;

    Some(Fingerprint {
        mtime_secs: mtime.as_secs(),
        mtime_nanos: mtime.subsec_nanos(),
        size: meta.len(),
    })
}
