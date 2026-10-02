//! A drum kit synthesized into one-shot buffers at construction (no bundled samples). Kick =
//! pitched sine drop, snare = tone + noise + snap, hat/clap = shaped noise. Each kit pitch is
//! retriggered by restarting its buffer (like the original `.interrupts` scheduling).

/// General-MIDI-ish drum map, top-to-bottom display order. Shared with the UI via the model.
pub const KIT: [(&str, i32); 4] = [("hat", 42), ("clap", 39), ("snare", 38), ("kick", 36)];

#[derive(Debug, Clone)]
struct Playing {
    drum: usize,
    pos: usize,
    gain: f32,
}

#[derive(Debug, Clone)]
pub struct DrumKit {
    buffers: Vec<Vec<f32>>,
    playing: Vec<Playing>,
}

/// Tiny deterministic noise source so renders are reproducible.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    }
}

impl DrumKit {
    pub fn new(sample_rate: f32) -> Self {
        let buffers = KIT.iter().map(|(name, _)| Self::synthesize(name, sample_rate)).collect();
        DrumKit {
            buffers,
            playing: Vec::with_capacity(KIT.len()),
        }
    }

    pub fn pitch_index(pitch: u8) -> Option<usize> {
        KIT.iter().position(|(_, p)| *p == pitch as i32)
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        let Some(drum) = Self::pitch_index(pitch) else { return };
        let gain = 0.35 + 0.65 * (velocity as f32 / 127.0);
        if let Some(p) = self.playing.iter_mut().find(|p| p.drum == drum) {
            p.pos = 0;
            p.gain = gain;
        } else {
            self.playing.push(Playing { drum, pos: 0, gain });
        }
    }

    pub fn is_silent(&self) -> bool {
        self.playing.is_empty()
    }

    pub fn render_add(&mut self, out: &mut [f32]) {
        for p in &mut self.playing {
            let buf = &self.buffers[p.drum];
            let n = out.len().min(buf.len().saturating_sub(p.pos));
            for (o, s) in out[..n].iter_mut().zip(&buf[p.pos..p.pos + n]) {
                *o += s * p.gain;
            }
            p.pos += n;
        }
        let buffers = &self.buffers;
        self.playing.retain(|p| p.pos < buffers[p.drum].len());
    }

    fn synthesize(name: &str, sample_rate: f32) -> Vec<f32> {
        let duration = match name {
            "kick" => 0.34,
            "snare" | "clap" => 0.20,
            _ => 0.06,
        };
        let frames = (duration * sample_rate) as usize;
        let mut rng = Lcg(0x9E3779B97F4A7C15);
        let two_pi = std::f32::consts::TAU;
        (0..frames)
            .map(|i| {
                let t = i as f32 / sample_rate;
                let s = match name {
                    "kick" => {
                        let freq = 110.0 * (-t * 28.0).exp() + 45.0;
                        (two_pi * freq * t).sin() * (-t * 8.0).exp()
                    }
                    "snare" => {
                        let body = ((two_pi * 175.0 * t).sin() + 0.5 * (two_pi * 280.0 * t).sin()) * 0.5 * (-t * 32.0).exp();
                        let noise = rng.next() * (-t * 16.0).exp();
                        let snap = if t < 0.004 { rng.next() * 0.7 } else { 0.0 };
                        body + noise * 0.85 + snap
                    }
                    "clap" => {
                        let burst = if (two_pi * 50.0 * t).sin() > 0.0 || t > 0.03 { 1.0 } else { 0.4 };
                        rng.next() * burst * (-t * 22.0).exp()
                    }
                    _ => rng.next() * (-t * 65.0).exp(),
                };
                s * 0.7
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kick_renders_and_finishes() {
        let mut kit = DrumKit::new(44_100.0);
        kit.note_on(36, 127);
        let mut buf = vec![0.0; 2048];
        kit.render_add(&mut buf);
        assert!(buf.iter().any(|x| x.abs() > 0.05));
        let mut rest = vec![0.0; 44_100];
        kit.render_add(&mut rest);
        assert!(kit.is_silent());
    }

    #[test]
    fn unknown_pitch_is_ignored_and_retrigger_restarts() {
        let mut kit = DrumKit::new(44_100.0);
        kit.note_on(99, 100);
        assert!(kit.is_silent());
        kit.note_on(38, 100);
        let mut buf = vec![0.0; 1000];
        kit.render_add(&mut buf);
        kit.note_on(38, 100);
        assert_eq!(kit.playing.len(), 1);
        assert_eq!(kit.playing[0].pos, 0);
    }
}
