//! The shipped `catalog.json` is data, and data rots quietly. These tests are the guard rail:
//! they say nothing about how the catalog is parsed and everything about what a user would hit
//! if an entry were half-written — a blank summary in the browser, two entries fighting over one
//! id, or worst of all an asset with no checksum behind it.
//!
//! They run offline on purpose. Whether a URL still resolves is a question for the network and
//! for whoever next edits the catalog; whether the entry is *coherent* is a question these
//! tests must answer on every build.

use dissonant_catalog::{builtin, AssetKind, Platform};
use dissonant_plugins::{PluginFormat, PluginKind};
use std::collections::HashSet;

const ALL_PLATFORMS: [Platform; 4] = [
    Platform::MacArm,
    Platform::MacIntel,
    Platform::Windows,
    Platform::Linux,
];

/// Every word the catalog is allowed to tag an entry with. The browser renders tags as filter
/// chips, so a tag used by exactly one entry filters nothing and a near-miss like "drum" beside
/// "drums" splits one chip into two half-empty ones. Growing this list is a deliberate act;
/// a typo is not.
const TAG_VOCABULARY: &[&str] = &[
    // what the thing is
    "synth",
    "sampler",
    "drums",
    "organ",
    "fx",
    // what an instrument is for
    "bass",
    "lead",
    "pad",
    "keys",
    "strings",
    "bell",
    "arp",
    // what an effect does
    "reverb",
    "delay",
    "chorus",
    "distortion",
    "saturation",
    "eq",
    "compressor",
    // how it sounds
    "warm",
    "bright",
    "vintage",
    "lo-fi",
    "metallic",
    "analogue",
    // how it makes sound
    "fm",
    "acid",
    "tape",
    "wavetable",
    // practicalities
    "free",
    "cheap-cpu",
    "presets",
];

/// A summary is one line under a name in a list. Past this it wraps, pushes the next entry down
/// the page, and stops being skimmable.
const MAX_SUMMARY_CHARS: usize = 100;

/// Words that mean nothing to someone who has never read a synthesis manual. The catalog exists
/// to translate for exactly that person, so a summary containing one of these has been written
/// for the wrong reader. Kept short so it stays defensible rather than becoming a style police.
const JARGON: &[&str] = &[
    "oversampl",
    "zero-delay",
    "biquad",
    "adsr",
    "lfo",
    "aliasing",
    "iir",
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
fn every_entry_carries_enough_tags_to_be_found_by_more_than_one_search() {
    // One tag is a label, not a search surface: a user who types "pad" and a user who types
    // "warm" are both looking for the same handful of entries and both have to land on them.
    for entry in builtin() {
        let id = &entry.id;
        assert!(
            entry.tags.len() >= 2,
            "{id} has {} tag(s); an entry findable by only one word is near-invisible",
            entry.tags.len()
        );
        let mut seen = HashSet::new();
        for tag in &entry.tags {
            assert!(
                seen.insert(tag.as_str()),
                "{id} lists the tag {tag:?} twice"
            );
        }
    }
}

#[test]
fn tags_come_from_the_controlled_vocabulary() {
    let vocabulary: HashSet<&str> = TAG_VOCABULARY.iter().copied().collect();
    for entry in builtin() {
        for tag in &entry.tags {
            assert!(
                vocabulary.contains(tag.as_str()),
                "{} uses the tag {tag:?}, which is not in TAG_VOCABULARY — either it is a typo \
                 for an existing tag, or the vocabulary needs to grow on purpose",
                entry.id
            );
        }
    }
}

#[test]
fn no_vocabulary_word_is_dead_weight() {
    // A chip nobody can click is worse than no chip: it tells the user a category exists and
    // then hands them an empty list. Dropping an entry that was the sole user of a tag has to
    // drop the tag too.
    let used: HashSet<&str> = builtin()
        .iter()
        .flat_map(|e| e.tags.iter().map(String::as_str))
        .collect();
    let unused: Vec<&str> = TAG_VOCABULARY
        .iter()
        .copied()
        .filter(|t| !used.contains(t))
        .collect();
    assert!(
        unused.is_empty(),
        "TAG_VOCABULARY declares tags no entry uses: {unused:?}"
    );
}

#[test]
fn summaries_stay_short_and_speak_plainly() {
    for entry in builtin() {
        let id = &entry.id;
        let len = entry.summary.chars().count();
        assert!(
            len <= MAX_SUMMARY_CHARS,
            "{id} has a {len}-character summary; over {MAX_SUMMARY_CHARS} it stops fitting on \
             one line"
        );
        let lowered = entry.summary.to_lowercase();
        for word in JARGON {
            assert!(
                !lowered.contains(word),
                "{id}'s summary contains {word:?}, which means nothing to someone with no \
                 production background: {:?}",
                entry.summary
            );
        }
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
                // shown to the user before they go to the homepage. An installer that *did*
                // carry a hash would look installable to a reader of this file and is not.
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
fn one_entry_never_lists_two_builds_for_the_same_platform() {
    // `asset_for` returns the first match, so a second one is unreachable data that nobody
    // would notice was wrong.
    for entry in builtin() {
        let mut seen = HashSet::new();
        for asset in &entry.assets {
            assert!(
                seen.insert((asset.platform, asset.format)),
                "{} lists two {:?} builds for {:?}",
                entry.id,
                asset.format,
                asset.platform
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
            assert_eq!(
                asset.platform, platform,
                "{} returned a foreign build",
                entry.id
            );
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
fn every_platform_can_one_click_both_an_instrument_and_an_effect() {
    // A platform served only by homepage links has a browser that looks broken: rows of plugins
    // and no install button anywhere. Something to play and something to put it through is the
    // floor for calling the catalog usable at all.
    for platform in ALL_PLATFORMS {
        for kind in [PluginKind::Instrument, PluginKind::Effect] {
            let installable = builtin()
                .iter()
                .filter(|e| e.kind == kind)
                .filter(|e| {
                    e.asset_for(platform, PluginFormat::Clap)
                        .is_some_and(|a| a.kind != AssetKind::Installer)
                })
                .count();
            assert!(
                installable > 0,
                "no {kind:?} in the catalog can be one-click installed on {platform:?}"
            );
        }
    }
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
