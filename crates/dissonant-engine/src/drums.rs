//! A drum kit synthesized into one-shot buffers at construction (no bundled samples). Kick =
//! pitched sine drop, snare = tone + noise + snap, hat/clap = shaped noise. Each kit pitch is
//! retriggered by restarting its buffer (like the original `.interrupts` scheduling).

use crate::dsp::{note_filter, note_pan_gains, NoteFilter};
use crate::engine::NoteShape;

/// General-MIDI-ish drum map, top-to-bottom display order. Shared with the UI via the model.
pub const KIT: [(&str, i32); 4] = [("hat", 42), ("clap", 39), ("snare", 38), ("kick", 36)];

/// How far a hit's `release` scales its baked decay, in octaves either way: 0 is a quarter of
/// the one-shot's own length, 0.5 leaves it alone, 1 stretches it fourfold.
const RELEASE_OCTAVES: f32 = 2.0;

/// Nepers of extra decay applied across a one-shot's whole length at `release = 0`. The same
/// constant, negative, is what a long release gives back.
const RELEASE_SHAPE: f32 = 3.0;

/// Ceiling on the gain a long `release` may claw back out of a decayed tail. The buffers are
/// baked, so there is no material past their end to stretch into — what a long release can do
/// is stop the decay that is already there from vanishing, and this keeps that from turning
/// the last few samples into a blast.
const RELEASE_CEILING: f32 = 6.0;

/// Where a kit sits when nothing has told it its track's voicing. See `PolySynth`.
const DEFAULT_TONE_HZ: f32 = 20_000.0;

#[derive(Debug, Clone)]
struct Playing {
    drum: usize,
    /// Read position in the one-shot, fractional because `fine` resamples it.
    pos: f64,
    /// Samples of source per sample of output: `2^(fine/1200)`.
    rate: f64,
    gain: f32,
    /// Equal-power gains from the hit's `pan`; (1, 1) when it is centred.
    gains: (f32, f32),
    /// Running `release` shaping gain and its per-sample factor; both exactly 1 at neutral.
    env: f32,
    env_step: f32,
    /// The hit's Mod X / Mod Y filter; `None` at neutral.
    filter: Option<NoteFilter>,
    /// True when pan, fine, release and both mod axes are all at rest: the hit then takes the
    /// original straight copy loop and lands bit-for-bit where it always did.
    neutral: bool,
}

impl Playing {
    /// Add this hit's next `out_l.len()` samples into the pair, advancing its read position.
    fn render(&mut self, buf: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let (gl, gr) = self.gains;
        if self.neutral {
            // Untouched hit: straight copy at unity on both sides, exactly as before.
            let start = self.pos as usize;
            let n = out_l.len().min(buf.len().saturating_sub(start));
            for i in 0..n {
                let s = buf[start + i] * self.gain;
                out_l[i] += s * gl;
                out_r[i] += s * gr;
            }
            self.pos += n as f64;
            return;
        }
        for i in 0..out_l.len() {
            let idx = self.pos as usize;
            if idx >= buf.len() {
                self.pos = buf.len() as f64;
                return;
            }
            // The sample past the last one is the silence the one-shot decays into, so the
            // final sample interpolates toward itself rather than being dropped.
            let frac = (self.pos - idx as f64) as f32;
            let next = buf.get(idx + 1).copied().unwrap_or(buf[idx]);
            let mut s = buf[idx] + (next - buf[idx]) * frac;
            if let Some(filter) = &mut self.filter {
                s = filter.process(s);
            }
            s *= self.gain * self.env;
            out_l[i] += s * gl;
            out_r[i] += s * gr;
            self.env = (self.env * self.env_step).min(RELEASE_CEILING);
            self.pos += self.rate;
        }
    }
}

#[derive(Debug, Clone)]
pub struct DrumKit {
    sample_rate: f32,
    tone_hz: f32,
    buffers: Vec<Vec<f32>>,
    playing: Vec<Playing>,
}

/// Tiny deterministic noise source so renders are reproducible.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    }
}

impl DrumKit {
    pub fn new(sample_rate: f32) -> Self {
        let buffers = KIT
            .iter()
            .map(|(name, _)| Self::synthesize(name, sample_rate))
            .collect();
        DrumKit {
            sample_rate,
            tone_hz: DEFAULT_TONE_HZ,
            buffers,
            playing: Vec::with_capacity(KIT.len()),
        }
    }

    /// The cutoff a step's Mod X is measured against; see [`crate::synth::PolySynth::set_tone_hz`].
    pub fn set_tone_hz(&mut self, hz: f32) {
        self.tone_hz = hz;
    }

    pub fn pitch_index(pitch: u8) -> Option<usize> {
        KIT.iter().position(|(_, p)| *p == pitch as i32)
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8, shape: NoteShape) {
        let Some(drum) = Self::pitch_index(pitch) else {
            return;
        };
        let gain = 0.35 + 0.65 * (velocity as f32 / 127.0);
        let neutral = shape == NoteShape::neutral();
        // `fine` is the step sequencer's "step pitch": resample the one-shot rather than
        // retune an oscillator, because a drum *is* its buffer.
        let rate = (shape.fine as f64 / 1200.0).exp2();
        // `release` reshapes the decay the buffer was baked with. Per-sample it is one
        // multiply, and at rest the factor is exactly 1.0 so nothing is touched at all.
        let seconds = self.buffers[drum].len() as f32 / self.sample_rate;
        let tail = ((shape.release.clamp(0.0, 1.0) - 0.5) * 2.0 * RELEASE_OCTAVES).exp2();
        let env_step =
            (-RELEASE_SHAPE * (1.0 / tail - 1.0) / (seconds.max(1e-4) * self.sample_rate)).exp();
        let hit = Playing {
            drum,
            pos: 0.0,
            rate,
            gain,
            gains: note_pan_gains(shape.pan),
            env: 1.0,
            env_step,
            filter: note_filter(self.sample_rate, self.tone_hz, shape.mod_x, shape.mod_y),
            neutral,
        };
        // Retriggering a drum restarts it in place rather than stacking a second copy.
        if let Some(p) = self.playing.iter_mut().find(|p| p.drum == drum) {
            *p = hit;
        } else {
            self.playing.push(hit);
        }
    }

    pub fn is_silent(&self) -> bool {
        self.playing.is_empty()
    }

    pub fn render_add(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let frames = out_l.len().min(out_r.len());
        for p in &mut self.playing {
            p.render(
                &self.buffers[p.drum],
                &mut out_l[..frames],
                &mut out_r[..frames],
            );
        }
        let buffers = &self.buffers;
        self.playing
            .retain(|p| p.pos < buffers[p.drum].len() as f64);
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
                        let body = ((two_pi * 175.0 * t).sin() + 0.5 * (two_pi * 280.0 * t).sin())
                            * 0.5
                            * (-t * 32.0).exp();
                        let noise = rng.next() * (-t * 16.0).exp();
                        let snap = if t < 0.004 { rng.next() * 0.7 } else { 0.0 };
                        body + noise * 0.85 + snap
                    }
                    "clap" => {
                        let burst = if (two_pi * 50.0 * t).sin() > 0.0 || t > 0.03 {
                            1.0
                        } else {
                            0.4
                        };
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

    const SR: f32 = 44_100.0;

    fn hit(pitch: u8, shape: NoteShape, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut kit = DrumKit::new(SR);
        kit.note_on(pitch, 127, shape);
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        kit.render_add(&mut l, &mut r);
        (l, r)
    }

    fn rms(xs: &[f32]) -> f32 {
        (xs.iter().map(|x| x * x).sum::<f32>() / xs.len() as f32).sqrt()
    }

    fn bright_rms(xs: &[f32]) -> f32 {
        rms(&xs.windows(2).map(|w| w[1] - w[0]).collect::<Vec<f32>>())
    }

    /// Frames until the hit stops sounding, rendered a block at a time.
    fn frames_until_silent(kit: &mut DrumKit) -> usize {
        let (mut l, mut r) = (vec![0.0; 128], vec![0.0; 128]);
        let mut n = 0;
        while !kit.is_silent() && n < SR as usize * 4 {
            l.fill(0.0);
            r.fill(0.0);
            kit.render_add(&mut l, &mut r);
            n += 128;
        }
        n
    }

    #[test]
    fn kick_renders_and_finishes() {
        let mut kit = DrumKit::new(SR);
        kit.note_on(36, 127, NoteShape::neutral());
        let (mut l, mut r) = (vec![0.0; 2048], vec![0.0; 2048]);
        kit.render_add(&mut l, &mut r);
        assert!(l.iter().any(|x| x.abs() > 0.05));
        let (mut el, mut er) = (vec![0.0; 44_100], vec![0.0; 44_100]);
        kit.render_add(&mut el, &mut er);
        assert!(kit.is_silent());
    }

    #[test]
    fn unknown_pitch_is_ignored_and_retrigger_restarts() {
        let mut kit = DrumKit::new(SR);
        kit.note_on(99, 100, NoteShape::neutral());
        assert!(kit.is_silent());
        kit.note_on(38, 100, NoteShape::neutral());
        let (mut l, mut r) = (vec![0.0; 1000], vec![0.0; 1000]);
        kit.render_add(&mut l, &mut r);
        kit.note_on(38, 100, NoteShape::neutral());
        assert_eq!(kit.playing.len(), 1);
        assert_eq!(kit.playing[0].pos, 0.0);
    }

    /// The bypass proof for one-shots: an unshaped hit is the baked buffer times the velocity
    /// gain and nothing else, on both sides.
    #[test]
    fn a_neutral_hit_is_the_raw_one_shot() {
        let kit = DrumKit::new(SR);
        let drum = DrumKit::pitch_index(36).unwrap();
        let gain = 0.35 + 0.65 * (127.0 / 127.0);
        let expected: Vec<f32> = kit.buffers[drum][..2048].iter().map(|s| s * gain).collect();
        let (l, r) = hit(36, NoteShape::neutral(), 2048);
        assert_eq!(l, expected, "left side drifted from the baked one-shot");
        assert_eq!(r, expected, "right side drifted from the baked one-shot");
    }

    #[test]
    fn fine_resamples_the_one_shot() {
        let length = |fine: i32| {
            let mut kit = DrumKit::new(SR);
            kit.note_on(
                36,
                127,
                NoteShape {
                    fine,
                    ..NoteShape::neutral()
                },
            );
            frames_until_silent(&mut kit)
        };
        let plain = length(0);
        // An octave of step pitch reads the buffer twice as fast, so the hit is half as long.
        let up = length(1200);
        assert!(
            plain > 10_000,
            "the reference kick should run its whole buffer: {plain}"
        );
        let ratio = up as f32 / plain as f32;
        assert!(
            (ratio - 0.5).abs() < 0.02,
            "+1200 cents ran {up} frames against {plain} ({ratio})"
        );
        assert!(
            length(-1200) > plain,
            "a negative step pitch must stretch the hit"
        );
    }

    #[test]
    fn pan_places_the_hit_and_centre_stays_balanced() {
        let (l, r) = hit(
            36,
            NoteShape {
                pan: 0.0,
                ..NoteShape::neutral()
            },
            4096,
        );
        assert!(rms(&l) > 0.01);
        assert!(rms(&r) < rms(&l) * 1e-6, "hard left leaked into the right");

        let (l, r) = hit(
            36,
            NoteShape {
                pan: 1.0,
                ..NoteShape::neutral()
            },
            4096,
        );
        assert!(rms(&r) > 0.01);
        assert!(rms(&l) < rms(&r) * 1e-6);

        let (l, r) = hit(36, NoteShape::neutral(), 4096);
        assert_eq!(l, r, "a centred hit must be identical on both sides");
    }

    #[test]
    fn release_lengthens_the_tail() {
        // How far into the hit the last audible sample sits.
        let audible_until = |release: f32| {
            let (l, _) = hit(
                36,
                NoteShape {
                    release,
                    ..NoteShape::neutral()
                },
                20_000,
            );
            l.iter().rposition(|s| s.abs() > 0.01).unwrap_or(0)
        };
        let short = audible_until(0.0);
        let long = audible_until(1.0);
        assert!(short > 0, "even a cut-short kick has to speak");
        assert!(
            long > short * 2,
            "release 1.0 died at {long}, release 0.0 at {short}"
        );

        // The kick's own decay still reaches the end of its buffer, so measure how much is
        // *left* that late: a long release holds the tail up, a short one has thrown it away.
        let late_energy = |release: f32| {
            let (l, _) = hit(
                36,
                NoteShape {
                    release,
                    ..NoteShape::neutral()
                },
                15_000,
            );
            rms(&l[10_000..])
        };
        assert!(late_energy(1.0) > late_energy(0.5) * 2.0);
        assert!(late_energy(0.5) > late_energy(0.0) * 10.0);
    }

    #[test]
    fn mod_x_closes_the_filter_and_neutral_leaves_it_open() {
        let render = |mod_x: f32| {
            let mut kit = DrumKit::new(SR);
            kit.set_tone_hz(4_000.0);
            kit.note_on(
                42,
                127,
                NoteShape {
                    mod_x,
                    ..NoteShape::neutral()
                },
            );
            let (mut l, mut r) = (vec![0.0; 4096], vec![0.0; 4096]);
            kit.render_add(&mut l, &mut r);
            l
        };
        let dark = render(0.0);
        let open = render(1.0);
        assert!(rms(&open) > 0.001, "the open hat must speak");
        assert!(
            bright_rms(&dark) < bright_rms(&open) * 0.25,
            "mod_x 0 was not duller: {} vs {}",
            bright_rms(&dark),
            bright_rms(&open)
        );
        assert!(bright_rms(&render(0.5)) > bright_rms(&dark) * 4.0);
    }
}
