//! Audio plugins: finding what is installed on disk, and hosting it.
//!
//! The crate is deliberately format-agnostic at its edges — [`PluginFormat`] names the format,
//! [`PluginDescriptor`] identifies one plugin regardless of it, and `host` hands the engine a
//! `PluginAudioUnit` that knows nothing about CLAP. Today CLAP is the only format, because CLAP
//! is MIT and so is this project; every Rust VST3 binding is GPL-3.0, so supporting VST3 means
//! relicensing the app or buying a Steinberg licence. Adding a format is additive: a new
//! [`PluginFormat`] variant, a scanner for it, and an implementation behind the same two types.

pub mod host;
pub mod scan;

pub use host::{HostedPlugin, ParamInfo, PluginAudioUnit, PluginError};
pub use scan::{search_paths, ScanCache, Scanner};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A plugin binary format. One variant today; see the module docs for what another costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginFormat {
    Clap,
}

impl PluginFormat {
    /// The bundle extension this format uses, on every platform.
    pub fn extension(self) -> &'static str {
        match self {
            PluginFormat::Clap => "clap",
        }
    }
}

/// What a plugin is for. Instruments replace a track's built-in voice; effects process audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginKind {
    Instrument,
    Effect,
}

/// One plugin found on disk. A bundle can hold several, so `uid` names the plugin, not the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDescriptor {
    /// Stable identity, and what a project file stores: `"<format>:<bundle path>:<plugin id>"`.
    /// Carrying the path means a moved plugin is reported missing by name rather than silently
    /// resolving to a different one.
    pub uid: String,
    pub format: PluginFormat,
    pub path: PathBuf,
    /// The format's own id for this plugin inside the bundle.
    pub plugin_id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub kind: PluginKind,
    /// The format's own feature strings, kept for display and filtering.
    pub features: Vec<String>,
}

impl PluginDescriptor {
    pub fn make_uid(format: PluginFormat, path: &Path, plugin_id: &str) -> String {
        format!("{}:{}:{}", format.extension(), path.display(), plugin_id)
    }
}
