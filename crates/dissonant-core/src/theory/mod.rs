//! Music theory: scales, diatonic harmony, the three-tier classifier, key detection and the
//! highlight engine that ties them to the chord track. Pure integer arithmetic on pitch
//! classes 0–11 — no theory library, trivially testable.

pub mod harmony;
pub mod highlight;
pub mod key_detector;
pub mod tier;

/// Wrap any integer onto the 12-tone circle (0–11).
pub fn normalize(pc: i32) -> i32 {
    pc.rem_euclid(12)
}

/// Shortest distance between two pitch classes around the circle (0–6).
pub fn semitone_distance(a: i32, b: i32) -> i32 {
    let d = (a - b).rem_euclid(12);
    d.min(12 - d)
}
