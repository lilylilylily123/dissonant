//! The curated list itself: what a free plugin is called, where its build lives, and the hash
//! that build must have.
//!
//! The data is `catalog.json` next to this file, baked in with `include_str!` so the app ships
//! one binary and a first-run browse needs no network at all. Entries are hand-written: every
//! `sha256` here was produced by downloading that exact URL, because a hash we did not compute
//! ourselves is worth nothing.

use dissonant_plugins::{PluginFormat, PluginKind};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

/// An operating system and CPU pair we publish builds for. macOS is split by architecture
/// because a plugin has to be native to the host process — Rosetta cannot bridge the two
/// inside one audio app — even though most vendors ship one universal bundle for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    MacArm,
    MacIntel,
    Windows,
    Linux,
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const CURRENT_PLATFORM: Platform = Platform::MacArm;
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
const CURRENT_PLATFORM: Platform = Platform::MacIntel;
#[cfg(target_os = "windows")]
const CURRENT_PLATFORM: Platform = Platform::Windows;
#[cfg(target_os = "linux")]
const CURRENT_PLATFORM: Platform = Platform::Linux;

#[cfg(not(any(
    all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    target_os = "windows",
    target_os = "linux",
)))]
compile_error!(
    "dissonant-catalog has no Platform for this target; add a Platform variant and a \
     CURRENT_PLATFORM arm before building here"
);

impl Platform {
    /// The platform this binary was compiled for. Resolved at compile time, so a build can only
    /// ever offer downloads it could actually run.
    pub fn current() -> Platform {
        CURRENT_PLATFORM
    }
}

/// How a vendor packages a build, which decides whether we can install it unattended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssetKind {
    Zip,
    TarGz,
    /// A `.dmg`, `.pkg` or `.exe` the user has to run themselves. We list these so the browser
    /// can still point at them, but [`crate::install`] refuses to fetch one: the file is not a
    /// plugin bundle we can place, and running a vendor installer unattended is not our call.
    Installer,
}

/// One downloadable build of one [`CatalogEntry`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogAsset {
    pub platform: Platform,
    pub format: PluginFormat,
    pub kind: AssetKind,
    pub url: String,
    /// Lowercase hex SHA-256 of the bytes at `url`, or empty for [`AssetKind::Installer`],
    /// which we never download. An unpinned non-installer asset is a bug, not a shrug: the
    /// installer treats an empty hash exactly like a mismatch and refuses.
    pub sha256: String,
    pub bytes: u64,
}

/// One plugin a user can browse and install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    /// Stable slug, used as the key in install events and in the UI's list. Never reused.
    pub id: String,
    pub name: String,
    pub vendor: String,
    /// One line, in the words of someone who has never read a synthesis manual.
    pub summary: String,
    /// What such a person would type into a search box: "pad", "bass", "reverb", "free".
    pub tags: Vec<String>,
    pub kind: PluginKind,
    pub license: String,
    pub homepage: String,
    pub assets: Vec<CatalogAsset>,
}

impl CatalogEntry {
    /// The build for this platform and format, if we publish one.
    pub fn asset_for(&self, platform: Platform, format: PluginFormat) -> Option<&CatalogAsset> {
        self.assets
            .iter()
            .find(|a| a.platform == platform && a.format == format)
    }
}

const CATALOG_JSON: &str = include_str!("../catalog.json");

/// The baked-in catalog. Parsed once; a malformed `catalog.json` is a build-time mistake that
/// the crate's own tests catch, so panicking here is better than quietly serving an empty list
/// that looks like "no plugins available".
pub fn builtin() -> &'static [CatalogEntry] {
    static ENTRIES: LazyLock<Vec<CatalogEntry>> = LazyLock::new(|| {
        serde_json::from_str(CATALOG_JSON)
            .expect("dissonant-catalog/catalog.json is not a valid CatalogEntry list")
    });
    &ENTRIES
}
