//! The project's time base: integer ticks.
//!
//! Musical time is stored and exchanged as whole ticks so that two edits that look identical
//! on screen really are identical in the file — binary fractions of a beat drift as soon as
//! anything is scaled, quantized or round-tripped, and a note at "beat 1" that saves as
//! 0.9999999999 is a bug nobody can see. 960 ticks per quarter note is the usual DAW PPQ: it
//! divides by 2, 3, 4, 5 and 16, so triplets, quintuplets and 1/64 notes are all exact, and a
//! standard MIDI file's 480 PPQ converts without loss.

use crate::model::TimeSignature;

/// Ticks in one quarter-note beat.
pub const TICKS_PER_BEAT: i64 = 960;

/// Beats to ticks, rounding half away from zero. Non-finite input is 0 rather than a panic:
/// this runs on values that came off the wire.
pub fn beats_to_ticks(beats: f64) -> i64 {
    if !beats.is_finite() {
        return 0;
    }
    (beats * TICKS_PER_BEAT as f64).round() as i64
}

/// Ticks back to beats, for the real-valued corners that are left (tempo integration, DSP).
pub fn ticks_to_beats(ticks: i64) -> f64 {
    ticks as f64 / TICKS_PER_BEAT as f64
}

/// Ticks in one bar of `ts`. Rounded, since 7/8 is 3.5 beats.
pub fn bar_ticks(ts: TimeSignature) -> i64 {
    beats_to_ticks(ts.beats_per_bar())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_round_trip_and_tolerate_nonsense() {
        assert_eq!(beats_to_ticks(0.25), 240);
        assert_eq!(beats_to_ticks(1.0 / 3.0), 320, "triplets are exact at 960");
        assert_eq!(beats_to_ticks(-0.5), -480);
        assert_eq!(beats_to_ticks(f64::NAN), 0);
        assert_eq!(beats_to_ticks(f64::INFINITY), 0);
        assert_eq!(ticks_to_beats(960), 1.0);
        assert_eq!(bar_ticks(TimeSignature::default()), 4 * TICKS_PER_BEAT);
        assert_eq!(bar_ticks(TimeSignature { numerator: 7, denominator: 8 }), 3360);
    }
}
