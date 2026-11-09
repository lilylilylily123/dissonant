//! Join codes, and the words two people read to each other to check nobody is in between.
//!
//! A code is six characters, about 30 bits: short enough to read out, far too short to be a key.
//! So it is stretched with Argon2id into the key of a short-lived lobby, which is how a guest
//! finds the host, and a separate proof, which is how the lobby knows the guest typed the code
//! rather than just spotted the lobby on the network. The stretching is what keeps a code that
//! lives five minutes out of reach of guessing, and the UTC day in the salt is what keeps anyone
//! from working them all out once and keeping the table.

use argon2::{Algorithm, Argon2, Params, Version};
use iroh::SecretKey;
use std::time::{SystemTime, UNIX_EPOCH};

pub const CODE_LEN: usize = 6;
/// No 0/O, 1/I/L: nothing that reads as something else.
const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
const DAY: u64 = 24 * 60 * 60;
/// How far apart two clocks can be, around midnight, and still agree on a code.
const CLOCK_SLACK: u64 = 60 * 60;

/// What a code stretches into.
pub(crate) struct Derived {
    pub lobby: SecretKey,
    pub proof: [u8; 32],
}

fn stretch(password: &[u8], salt: &[u8], out: &mut [u8]) {
    // 64 MiB and three passes: a fraction of a second here, and per guess for anyone guessing.
    let params = Params::new(64 * 1024, 3, 1, Some(out.len())).expect("valid argon2 parameters");
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password, salt, out)
        .expect("argon2 accepts these lengths");
}

/// A fresh code, unformatted.
pub fn new_code() -> String {
    let mut code = String::with_capacity(CODE_LEN);
    while code.len() < CODE_LEN {
        // iroh's key generation is a CSPRNG; borrowing it keeps a second random crate out.
        for byte in SecretKey::generate().to_bytes() {
            // Drop the top of the byte range so every character is as likely as the next.
            if (byte as usize) < ALPHABET.len() * (256 / ALPHABET.len()) && code.len() < CODE_LEN {
                code.push(ALPHABET[byte as usize % ALPHABET.len()] as char);
            }
        }
    }
    code
}

/// What somebody typed, as a code: any case, spaces and dashes ignored.
pub fn parse_code(input: &str) -> Result<String, String> {
    let code: String =
        input.chars().filter(|c| !c.is_whitespace() && *c != '-').map(|c| c.to_ascii_uppercase()).collect();
    if code.chars().any(|c| matches!(c, '0' | 'O' | '1' | 'I' | 'L')) {
        return Err("codes never use 0, O, 1, I or L. check the code again.".into());
    }
    if code.len() != CODE_LEN || !code.bytes().all(|b| ALPHABET.contains(&b)) {
        return Err(format!("a code is {CODE_LEN} letters and numbers, like K7Q-4MX."));
    }
    Ok(code)
}

/// A code for showing: `K7Q4MX` as `K7Q-4MX`.
pub fn format_code(code: &str) -> String {
    let (a, b) = code.split_at(code.len().min(CODE_LEN / 2));
    format!("{a}-{b}")
}

/// The UTC day number, and how far into it we are.
pub(crate) fn today() -> (u64, u64) {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    (now / DAY, now % DAY)
}

/// The days a code typed now might have been made on, given clocks that differ a little.
pub(crate) fn candidate_days() -> Vec<u64> {
    let (day, into) = today();
    let mut days = vec![day];
    if into < CLOCK_SLACK {
        days.push(day.saturating_sub(1));
    }
    if into > DAY - CLOCK_SLACK {
        days.push(day + 1);
    }
    days
}

pub(crate) fn derive(code: &str, day: u64) -> Derived {
    let mut out = [0u8; 64];
    stretch(code.as_bytes(), format!("dissonant/join/1/{day}").as_bytes(), &mut out);
    let mut lobby = [0u8; 32];
    let mut proof = [0u8; 32];
    lobby.copy_from_slice(&out[..32]);
    proof.copy_from_slice(&out[32..]);
    Derived { lobby: SecretKey::from_bytes(&lobby), proof }
}

/// Equal, in time that doesn't depend on where they differ.
pub(crate) fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Three words that come out the same on both ends of a connection, and only on those two ends.
/// Stretched like a code, so someone in the middle can't just try keys until the words match.
pub(crate) fn verification_words(host: &[u8; 32], guest: &[u8; 32]) -> [&'static str; 3] {
    let mut keys = [0u8; 64];
    keys[..32].copy_from_slice(host);
    keys[32..].copy_from_slice(guest);
    // Argon2 won't give fewer than four bytes; the words take three.
    let mut out = [0u8; 4];
    stretch(&keys, b"dissonant/words/1", &mut out);
    [WORDS[out[0] as usize], WORDS[out[1] as usize], WORDS[out[2] as usize]]
}

const WORDS: [&str; 256] = [
    "acorn", "amber", "anchor", "apple", "arrow", "aspen", "atlas", "autumn",
    "badge", "bagel", "bamboo", "banjo", "barley", "basil", "beacon", "beaver",
    "berry", "bison", "blossom", "bramble", "breeze", "brick", "bridge", "brook",
    "bubble", "bucket", "cabin", "cactus", "camel", "candle", "canoe", "canyon",
    "carrot", "castle", "cedar", "cello", "cherry", "chess", "cider", "cinder",
    "cliff", "clover", "cobalt", "cocoa", "comet", "coral", "cotton", "cougar",
    "coyote", "crane", "crater", "cricket", "crystal", "cupcake", "daisy", "delta",
    "denim", "desert", "diesel", "dingo", "dolphin", "domino", "donkey", "dragon",
    "drum", "dune", "eagle", "echo", "ember", "emerald", "falcon", "feather",
    "fern", "fiddle", "finch", "fjord", "flame", "flint", "forest", "fossil",
    "fox", "frost", "galaxy", "garden", "garlic", "gecko", "geyser", "ginger",
    "glacier", "globe", "goose", "granite", "grape", "gravel", "guitar", "gull",
    "hammer", "harbor", "harp", "hazel", "heron", "hickory", "honey", "hornet",
    "husky", "igloo", "indigo", "island", "ivory", "jacket", "jaguar", "jasmine",
    "jelly", "jungle", "kayak", "kettle", "kiwi", "koala", "ladder", "lagoon",
    "lantern", "lava", "lemon", "lily", "lime", "lizard", "llama", "lobster",
    "locket", "lotus", "lunar", "magnet", "mango", "maple", "marble", "meadow",
    "melon", "meteor", "mint", "mitten", "moose", "mosaic", "moss", "muffin",
    "mustard", "nectar", "needle", "nickel", "noodle", "nutmeg", "oasis", "ocean",
    "olive", "onion", "orange", "orbit", "orchid", "otter", "owl", "paddle",
    "panda", "papaya", "parrot", "peach", "peanut", "pebble", "pecan", "pepper",
    "pickle", "pigeon", "pine", "pirate", "pixel", "planet", "plum", "pocket",
    "polar", "pony", "poppy", "prairie", "pretzel", "puffin", "pumpkin", "quartz",
    "quill", "rabbit", "radar", "radish", "raven", "reef", "ribbon", "river",
    "robin", "rocket", "rose", "ruby", "saddle", "saffron", "salmon", "sapphire",
    "satin", "scarf", "seal", "shadow", "shell", "sierra", "silver", "sketch",
    "sloth", "snail", "sparrow", "spruce", "squid", "stone", "storm", "sugar",
    "summit", "sunset", "swan", "tango", "teapot", "thistle", "thunder", "tiger",
    "timber", "toast", "tomato", "topaz", "tornado", "trumpet", "tulip", "tundra",
    "turtle", "umbrella", "valley", "velvet", "violet", "violin", "volcano", "waffle",
    "walnut", "walrus", "willow", "window", "wizard", "wolf", "yarrow", "yogurt",
    "zebra", "zephyr", "bonfire", "hamster", "lemur", "oyster", "quokka", "puma",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_reads_back_however_it_was_typed() {
        let code = new_code();
        assert_eq!(code.len(), CODE_LEN);
        assert_eq!(parse_code(&format_code(&code).to_lowercase()).unwrap(), code);
        assert_eq!(parse_code(" k7q 4mx ").unwrap(), "K7Q4MX");
    }

    #[test]
    fn lookalike_characters_are_turned_away_with_a_reason() {
        assert!(parse_code("K7Q-4M0").unwrap_err().contains("0, O"));
        assert!(parse_code("K7Q-4ML").unwrap_err().contains("0, O"));
        assert!(parse_code("K7Q-4M").is_err());
        assert!(parse_code("K7Q-4MX9").is_err());
        assert!(parse_code("K7Q-4M#").is_err());
    }

    #[test]
    fn a_code_stretches_differently_on_another_day() {
        let a = derive("K7Q4MX", 20_000);
        let b = derive("K7Q4MX", 20_001);
        assert_ne!(a.lobby.public(), b.lobby.public());
        assert_ne!(a.proof, b.proof);
        assert_eq!(a.proof, derive("K7Q4MX", 20_000).proof);
    }

    #[test]
    fn the_words_depend_on_which_end_is_which() {
        let a = *SecretKey::generate().public().as_bytes();
        let b = *SecretKey::generate().public().as_bytes();
        assert_ne!(verification_words(&a, &b), verification_words(&b, &a));
    }
}
