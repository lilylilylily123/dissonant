//! The shipped `catalog.json` is data, and data rots quietly. These tests are the guard rail:
//! they say nothing about how the catalog is parsed and everything about what a user would hit
//! if an entry were half-written — a blank summary in the browser, two entries fighting over one
//! id, or worst of all an asset with no checksum behind it.

use dissonant_catalog::{builtin, AssetKind, Platform};
use dissonant_plugins::PluginFormat;
use std::collections::HashSet;

const ALL_PLATFORMS: [Platform; 4] = [
    Platform::MacArm,
    Platform::MacIntel,
    Platform::Windows,
    Platform::Linux,
];

#[test]
fn the_shipped_catalog_parses_and_is_not_empty() {
    // `builtin` panics on malformed JSON; reaching the assert at all is half the test.
    assert!(
        !builtin().is_empty(),
        "a catalog with no entries would show the user an empty browser"
    );
}

#[test]
fn every_entry_has_everything_the_browser_needs_to_show_it() {
    for entry in builtin() {
        let id = &entry.id;
        assert!(!id.trim().is_empty(), "an entry has no id");
        assert!(!entry.name.trim().is_empty(), "{id} has no name");
        assert!(!entry.vendor.trim().is_empty(), "{id} has no vendor");
        assert!(!entry.summary.trim().is_empty(), "{id} has no summary");
        assert!(!entry.license.trim().is_empty(), "{id} has no license");
        assert!(
            !entry.tags.is_empty() && entry.tags.iter().all(|t| !t.trim().is_empty()),
            "{id} has no usable tags, so nobody can find it by searching"
        );
        assert!(
            entry.homepage.starts_with("https://"),
            "{id} has homepage {:?}, which is not an https URL",
            entry.homepage
        );
        assert!(
            !entry.assets.is_empty(),
            "{id} lists no builds, so it can never be installed or pointed at"
        );
    }
}

#[test]
fn no_two_entries_share_an_id() {
    // Ids key install events and the project file's plugin references; a collision would make
    // one entry silently shadow the other.
    let mut seen = HashSet::new();
    for entry in builtin() {
        assert!(
            seen.insert(entry.id.as_str()),
            "{} appears twice in the catalog",
            entry.id
        );
    }
}

#[test]
fn every_asset_we_offer_to_install_is_pinned_to_a_checksum() {
    for entry in builtin() {
        for asset in &entry.assets {
            assert!(
                asset.url.starts_with("https://"),
                "{} has a non-https download: {:?}",
                entry.id,
                asset.url
            );
            match asset.kind {
                // Installers are never fetched, so there is nothing to pin and the size is only
                // shown to the user before they go to the homepage.
                AssetKind::Installer => assert!(
                    asset.sha256.is_empty(),
                    "{} pins a checksum on an installer it will never download",
                    entry.id
                ),
                AssetKind::Zip | AssetKind::TarGz => {
                    assert_eq!(
                        asset.sha256.len(),
                        64,
                        "{} has a {}-character checksum; SHA-256 is 64 hex digits",
                        entry.id,
                        asset.sha256.len()
                    );
                    assert!(
                        asset
                            .sha256
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                        "{} has a checksum that is not lowercase hex: {:?}",
                        entry.id,
                        asset.sha256
                    );
                }
            }
            assert!(
                asset.bytes > 0,
                "{} lists a zero-byte download, which cannot be right",
                entry.id
            );
        }
    }
}

#[test]
fn asking_for_a_platform_returns_a_build_for_that_platform() {
    for entry in builtin() {
        for platform in ALL_PLATFORMS {
            let Some(asset) = entry.asset_for(platform, PluginFormat::Clap) else {
                continue;
            };
            assert_eq!(asset.platform, platform, "{} returned a foreign build", entry.id);
            assert_eq!(asset.format, PluginFormat::Clap);
        }
    }
}

#[test]
fn a_platform_an_entry_does_not_build_for_yields_nothing() {
    // Dragonfly Reverb publishes no Linux archive in a kind we can unpack, and silently handing
    // back the Windows build would install a DLL into a Linux plugin folder.
    let dragonfly = builtin()
        .iter()
        .find(|e| e.id == "dragonfly-reverb")
        .expect("catalog should still carry dragonfly-reverb");
    assert!(dragonfly
        .asset_for(Platform::Linux, PluginFormat::Clap)
        .is_none());
    assert!(dragonfly
        .asset_for(Platform::Windows, PluginFormat::Clap)
        .is_some());
}

#[test]
fn this_machines_platform_can_install_something() {
    // A catalog that offers the running platform nothing but homepage links is a catalog that
    // looks broken, whichever machine it is browsed on.
    let installable = builtin()
        .iter()
        .filter(|e| {
            e.asset_for(Platform::current(), PluginFormat::Clap)
                .is_some_and(|a| a.kind != AssetKind::Installer)
        })
        .count();
    assert!(
        installable > 0,
        "nothing in the catalog can be one-click installed on {:?}",
        Platform::current()
    );
}
