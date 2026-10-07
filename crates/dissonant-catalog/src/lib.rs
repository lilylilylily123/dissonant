//! A curated list of free plugins, and a one-click installer for the ones we can verify.
//!
//! Finding a good free plugin normally means knowing which forums to read. The catalog replaces
//! that with a short, hand-checked list: every entry here is genuinely free to use, genuinely
//! ships a CLAP build, and is described in words that assume no production background.
//!
//! The installer deliberately does less than it could. It downloads, checks the pinned SHA-256,
//! and moves `.clap` bundles into place — nothing else. Vendors who only ship a `.dmg`, `.pkg`
//! or `.exe` are still listed, but [`install`] refuses them and points at the homepage, because
//! running someone else's installer unattended is not a decision this app gets to make for the
//! user.

mod catalog;
mod install;

pub use catalog::{builtin, AssetKind, CatalogAsset, CatalogEntry, Platform};
pub use install::{install, user_plugin_dir, InstallError, InstallEvent};
