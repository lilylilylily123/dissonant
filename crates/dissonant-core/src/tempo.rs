//! Beats ⇄ seconds ⇄ samples.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tempo {
    pub bpm: f64,
}

impl Tempo {
    pub const MIN_BPM: f64 = 20.0;
    pub const MAX_BPM: f64 = 300.0;

    pub fn new(bpm: f64) -> Self {
        Tempo {
            bpm: bpm.clamp(Self::MIN_BPM, Self::MAX_BPM),
        }
    }

    pub fn seconds_per_beat(&self) -> f64 {
        60.0 / self.bpm
    }

    pub fn seconds_for_beats(&self, beats: f64) -> f64 {
        beats * self.seconds_per_beat()
    }

    pub fn beats_for_seconds(&self, seconds: f64) -> f64 {
        seconds / self.seconds_per_beat()
    }

    pub fn samples_per_beat(&self, sample_rate: f64) -> f64 {
        self.seconds_per_beat() * sample_rate
    }

    pub fn beats_for_samples(&self, samples: f64, sample_rate: f64) -> f64 {
        samples / self.samples_per_beat(sample_rate)
    }
}

/// Swing as a piecewise-linear warp of the beat line. Beats are grouped in pairs of `grid`
/// (0.5 = eighth-note swing); the second half of each pair is pushed later so that it lands at
/// `2·grid·swing/100` instead of `grid`. 50 % is straight, 66.7 % the triplet feel, 75 % a hard
/// shuffle. Monotonic, so note order (and the lengths of notes that straddle a pair) survives.
pub fn swing_warp(beat: f64, swing_percent: f64, grid: f64) -> f64 {
    if grid <= 0.0 || !(50.0..=75.0).contains(&swing_percent) || swing_percent == 50.0 {
        return beat;
    }
    let pair = 2.0 * grid;
    let late = pair * swing_percent / 100.0; // where the off-subdivision ends up
    let base = (beat / pair).floor() * pair;
    let u = beat - base;
    let warped = if u <= grid {
        u * (late / grid)
    } else {
        late + (u - grid) * ((pair - late) / grid)
    };
    base + warped
}

impl Default for Tempo {
    fn default() -> Self {
        Tempo { bpm: 120.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beats_and_seconds() {
        let t = Tempo::new(120.0);
        assert!((t.seconds_for_beats(1.0) - 0.5).abs() < 1e-12);
        assert!((t.beats_for_seconds(2.0) - 4.0).abs() < 1e-12);
        assert!((t.samples_per_beat(44_100.0) - 22_050.0).abs() < 1e-9);
    }

    #[test]
    fn swing_warp_moves_offbeats_and_keeps_downbeats() {
        // Straight: identity.
        assert_eq!(swing_warp(0.5, 50.0, 0.5), 0.5);
        // Triplet feel on eighths: the "and" lands at 2/3 of the beat.
        assert!((swing_warp(0.5, 200.0 / 3.0, 0.5) - 2.0 / 3.0).abs() < 1e-9);
        // Downbeats never move.
        for b in [0.0, 1.0, 2.0, 7.0] {
            assert!((swing_warp(b, 66.0, 0.5) - b).abs() < 1e-12);
        }
        // Hard shuffle on sixteenths: the second sixteenth of each eighth lands at 3/8 of the beat.
        assert!((swing_warp(0.25, 75.0, 0.25) - 0.375).abs() < 1e-12);
        // Monotonic.
        let mut last = -1.0;
        for i in 0..400 {
            let w = swing_warp(i as f64 / 100.0, 70.0, 0.5);
            assert!(w >= last);
            last = w;
        }
        // Out-of-range swing is ignored.
        assert_eq!(swing_warp(0.5, 90.0, 0.5), 0.5);
    }

    #[test]
    fn clamps_bpm() {
        assert_eq!(Tempo::new(5.0).bpm, Tempo::MIN_BPM);
        assert_eq!(Tempo::new(999.0).bpm, Tempo::MAX_BPM);
    }
}
