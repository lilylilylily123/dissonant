//! The three-tier classifier — the product's core mechanic.
//!
//! Rule: chord tones win; otherwise a pitch a half step from any chord tone is dissonance; an
//! out-of-key pitch is dissonance; everything else (in-key, ≥ a whole step from every chord
//! tone) is a tension.

use super::{normalize, semitone_distance};
use serde::{Deserialize, Serialize};

/// How a note relates to the chord sounding right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    /// A note of the chord itself. Rock-solid.
    ChordTone,
    /// In-key and at least a whole step from every chord tone. Spicy but good.
    Tension,
    /// A half step from a chord tone, or out of key. Flagged — never blocked.
    Dissonance,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TierClassifier;

impl TierClassifier {
    /// Classify one pitch class against a chord, optionally constrained to a key's scale.
    pub fn tier(&self, pitch_class: i32, chord: &[i32], key: Option<&[i32]>) -> Tier {
        let p = normalize(pitch_class);
        let chord: Vec<i32> = chord.iter().map(|&c| normalize(c)).collect();
        if chord.contains(&p) {
            return Tier::ChordTone;
        }
        let nearest = chord.iter().map(|&c| semitone_distance(p, c)).min().unwrap_or(12);
        if nearest == 1 {
            return Tier::Dissonance;
        }
        if let Some(key) = key {
            if !key.iter().any(|&k| normalize(k) == p) {
                return Tier::Dissonance;
            }
        }
        Tier::Tension
    }

    /// Precomputed tier for every pitch class 0–11, so notes look up by `pitch % 12`.
    pub fn tier_map(&self, chord: &[i32], key: Option<&[i32]>) -> [Tier; 12] {
        let mut map = [Tier::Tension; 12];
        for (pc, slot) in map.iter_mut().enumerate() {
            *slot = self.tier(pc as i32, chord, key);
        }
        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const C_MAJOR_SCALE: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];

    #[test]
    fn c_major_triad_no_key() {
        let c = TierClassifier;
        let chord = [0, 4, 7];
        for pc in [0, 4, 7] {
            assert_eq!(c.tier(pc, &chord, None), Tier::ChordTone);
        }
        assert_eq!(c.tier(2, &chord, None), Tier::Tension);
        assert_eq!(c.tier(9, &chord, None), Tier::Tension);
        assert_eq!(c.tier(5, &chord, None), Tier::Dissonance); // half step above E
        assert_eq!(c.tier(1, &chord, None), Tier::Dissonance); // half step above C
    }

    #[test]
    fn cmaj7_in_c_major() {
        let c = TierClassifier;
        let chord = [0, 4, 7, 11];
        assert_eq!(c.tier(11, &chord, Some(&C_MAJOR_SCALE)), Tier::ChordTone);
        assert_eq!(c.tier(2, &chord, Some(&C_MAJOR_SCALE)), Tier::Tension);
        assert_eq!(c.tier(5, &chord, Some(&C_MAJOR_SCALE)), Tier::Dissonance);
    }

    #[test]
    fn out_of_key_is_dissonance_even_when_interval_math_says_tension() {
        let c = TierClassifier;
        // A# over C major triad: a whole step from C and three from G → tension by interval,
        // but out of key.
        assert_eq!(c.tier(10, &[0, 4, 7], None), Tier::Tension);
        assert_eq!(c.tier(10, &[0, 4, 7], Some(&C_MAJOR_SCALE)), Tier::Dissonance);
    }

    #[test]
    fn minor_and_dominant_chord_tones() {
        let c = TierClassifier;
        for pc in [9, 0, 4] {
            assert_eq!(c.tier(pc, &[9, 0, 4], None), Tier::ChordTone);
        }
        for pc in [7, 11, 2, 5] {
            assert_eq!(c.tier(pc, &[7, 11, 2, 5], None), Tier::ChordTone);
        }
    }

    #[test]
    fn every_pitch_class_has_exactly_one_tier_and_enharmonics_agree() {
        let c = TierClassifier;
        let map = c.tier_map(&[0, 4, 7], Some(&C_MAJOR_SCALE));
        assert_eq!(map.len(), 12);
        assert_eq!(c.tier(13, &[0, 4, 7], None), c.tier(1, &[0, 4, 7], None));
        assert_eq!(c.tier(-1, &[0, 4, 7], None), c.tier(11, &[0, 4, 7], None));
        assert_eq!(c.tier(4, &[12, 16, 19], None), Tier::ChordTone);
    }
}
