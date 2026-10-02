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
    fn clamps_bpm() {
        assert_eq!(Tempo::new(5.0).bpm, Tempo::MIN_BPM);
        assert_eq!(Tempo::new(999.0).bpm, Tempo::MAX_BPM);
    }
}
