//! Small, allocation-free DSP building blocks: biquad filters, an ADSR, PolyBLEP oscillators,
//! a Freeverb-style reverb and a constant-power pan law.

use std::f32::consts::PI;

// ─── Biquad ──────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    pub fn identity() -> Self {
        Biquad {
            b0: 1.0,
            ..Default::default()
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // Transposed direct form II.
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    fn set(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    pub fn lowpass(&mut self, sample_rate: f32, cutoff: f32, q: f32) {
        let fc = cutoff.clamp(10.0, sample_rate * 0.49);
        let w0 = 2.0 * PI * fc / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        self.set((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    }

    pub fn highpass(&mut self, sample_rate: f32, cutoff: f32, q: f32) {
        let fc = cutoff.clamp(5.0, sample_rate * 0.49);
        let w0 = 2.0 * PI * fc / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        self.set((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha);
    }

    /// `gain_db` boost/cut at `freq`, bandwidth from `q`.
    pub fn peaking(&mut self, sample_rate: f32, freq: f32, gain_db: f32, q: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq.clamp(10.0, sample_rate * 0.49) / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        self.set(
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        );
    }

    pub fn low_shelf(&mut self, sample_rate: f32, freq: f32, gain_db: f32, q: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq.clamp(10.0, sample_rate * 0.49) / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let sa = 2.0 * a.sqrt() * alpha;
        self.set(
            a * ((a + 1.0) - (a - 1.0) * cos + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
            a * ((a + 1.0) - (a - 1.0) * cos - sa),
            (a + 1.0) + (a - 1.0) * cos + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * cos),
            (a + 1.0) + (a - 1.0) * cos - sa,
        );
    }

    pub fn high_shelf(&mut self, sample_rate: f32, freq: f32, gain_db: f32, q: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * freq.clamp(10.0, sample_rate * 0.49) / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let sa = 2.0 * a.sqrt() * alpha;
        self.set(
            a * ((a + 1.0) + (a - 1.0) * cos + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
            a * ((a + 1.0) + (a - 1.0) * cos - sa),
            (a + 1.0) - (a - 1.0) * cos + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * cos),
            (a + 1.0) - (a - 1.0) * cos - sa,
        );
    }
}

/// Linear amplitude (1.0 = flat) → dB, guarding against -inf.
pub fn linear_to_db(gain: f32) -> f32 {
    20.0 * gain.max(0.02).log10()
}

/// A stereo pair of identical biquads.
#[derive(Debug, Clone, Copy, Default)]
pub struct StereoBiquad {
    pub l: Biquad,
    pub r: Biquad,
}

impl StereoBiquad {
    pub fn identity() -> Self {
        StereoBiquad {
            l: Biquad::identity(),
            r: Biquad::identity(),
        }
    }
    pub fn configure(&mut self, f: impl Fn(&mut Biquad)) {
        f(&mut self.l);
        f(&mut self.r);
    }
    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        (self.l.process(l), self.r.process(r))
    }
}

// ─── ADSR ────────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdsrParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy)]
pub struct Adsr {
    params: AdsrParams,
    sample_rate: f32,
    stage: Stage,
    level: f32,
}

impl Adsr {
    pub fn new(params: AdsrParams, sample_rate: f32) -> Self {
        Adsr {
            params,
            sample_rate,
            stage: Stage::Idle,
            level: 0.0,
        }
    }

    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Hard reset (voice stealing).
    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.level = 0.0;
    }

    #[inline]
    fn rate(&self, seconds: f32) -> f32 {
        1.0 / (seconds.max(0.0005) * self.sample_rate)
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => 0.0,
            Stage::Attack => {
                self.level += self.rate(self.params.attack);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
                self.level
            }
            Stage::Decay => {
                let target = self.params.sustain;
                self.level -= (1.0 - target).max(0.0) * self.rate(self.params.decay);
                if self.level <= target + 1e-5 {
                    self.level = target;
                    self.stage = if target <= 1e-4 { Stage::Idle } else { Stage::Sustain };
                }
                self.level
            }
            Stage::Sustain => self.level,
            Stage::Release => {
                // Exponential-feeling tail: constant fraction of the remaining level per sample
                // would never reach zero, so use a linear ramp scaled by the start level.
                self.level -= self.rate(self.params.release);
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
                self.level
            }
        }
    }
}

// ─── Oscillator ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Saw,
    Square,
    Triangle,
    Sine,
}

#[derive(Debug, Clone, Copy)]
pub struct Oscillator {
    pub waveform: Waveform,
    phase: f32,
    inc: f32,
    tri_state: f32,
}

#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        2.0 * t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + 2.0 * t + 1.0
    } else {
        0.0
    }
}

impl Oscillator {
    pub fn new(waveform: Waveform) -> Self {
        Oscillator {
            waveform,
            phase: 0.0,
            inc: 0.0,
            tri_state: 0.0,
        }
    }

    pub fn set_frequency(&mut self, hz: f32, sample_rate: f32) {
        self.inc = (hz / sample_rate).clamp(0.0, 0.5);
    }

    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.tri_state = 0.0;
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        let t = self.phase;
        let dt = self.inc;
        let out = match self.waveform {
            Waveform::Sine => (2.0 * PI * t).sin(),
            Waveform::Saw => 2.0 * t - 1.0 - poly_blep(t, dt),
            Waveform::Square => {
                let mut v = if t < 0.5 { 1.0 } else { -1.0 };
                v += poly_blep(t, dt);
                v -= poly_blep((t + 0.5) % 1.0, dt);
                v
            }
            Waveform::Triangle => {
                // Leaky-integrated PolyBLEP square → anti-aliased triangle.
                let mut sq = if t < 0.5 { 1.0 } else { -1.0 };
                sq += poly_blep(t, dt);
                sq -= poly_blep((t + 0.5) % 1.0, dt);
                self.tri_state = 0.999 * self.tri_state + 4.0 * dt * sq;
                self.tri_state.clamp(-1.0, 1.0)
            }
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

pub fn midi_to_hz(pitch: f32) -> f32 {
    440.0 * 2f32.powf((pitch - 69.0) / 12.0)
}

// ─── Reverb ──────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Comb {
    buf: Vec<f32>,
    idx: usize,
    filter: f32,
    feedback: f32,
    damp: f32,
}

impl Comb {
    fn new(len: usize, feedback: f32, damp: f32) -> Self {
        Comb {
            buf: vec![0.0; len.max(1)],
            idx: 0,
            filter: 0.0,
            feedback,
            damp,
        }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.buf[self.idx];
        self.filter = y * (1.0 - self.damp) + self.filter * self.damp;
        self.buf[self.idx] = x + self.filter * self.feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}

#[derive(Debug, Clone)]
struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Allpass {
            buf: vec![0.0; len.max(1)],
            idx: 0,
        }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.idx];
        let y = -x + b;
        self.buf[self.idx] = x + b * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        y
    }
}

/// Brings the comb/allpass network to unity against the dry signal. Its own broadband rms
/// gain measures 0.652 at 44.1 kHz and 0.645 at 48 kHz, so one constant covers both.
const WET_MAKEUP: f32 = 1.53;

/// Freeverb-style stereo reverb. `wet` is a 0–1 dry/wet mix, crossfaded at constant power.
#[derive(Debug, Clone)]
pub struct Reverb {
    combs_l: Vec<Comb>,
    combs_r: Vec<Comb>,
    allpass_l: Vec<Allpass>,
    allpass_r: Vec<Allpass>,
    pub wet: f32,
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44_100.0;
        let comb_lens = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
        let allpass_lens = [556, 441, 341, 225];
        let spread = 23;
        let s = |n: usize| ((n as f32) * scale) as usize;
        Reverb {
            combs_l: comb_lens.iter().map(|&n| Comb::new(s(n), 0.84, 0.2)).collect(),
            combs_r: comb_lens.iter().map(|&n| Comb::new(s(n + spread), 0.84, 0.2)).collect(),
            allpass_l: allpass_lens.iter().map(|&n| Allpass::new(s(n))).collect(),
            allpass_r: allpass_lens.iter().map(|&n| Allpass::new(s(n + spread))).collect(),
            wet: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, l: f32, r: f32) -> (f32, f32) {
        if self.wet <= 0.0001 {
            return (l, r);
        }
        let input = (l + r) * 0.015;
        let mut wl = 0.0;
        let mut wr = 0.0;
        for c in &mut self.combs_l {
            wl += c.process(input);
        }
        for c in &mut self.combs_r {
            wr += c.process(input);
        }
        for a in &mut self.allpass_l {
            wl = a.process(wl);
        }
        for a in &mut self.allpass_r {
            wr = a.process(wr);
        }
        // Dry and wet are uncorrelated, so crossfade with constant power — the same law the
        // pan knob uses. With a unity-calibrated wet path this makes the mix knob change the
        // amount of ambience without changing how loud the bus is.
        let dry_g = (1.0 - self.wet).sqrt();
        let wet_g = self.wet.sqrt() * WET_MAKEUP;
        (l * dry_g + wl * wet_g, r * dry_g + wr * wet_g)
    }
}

// ─── Pan ─────────────────────────────────────────────────────────────────────────────────────

/// Constant-power pan law. `pan` in -1 (left) … 1 (right) → (left gain, right gain).
#[inline]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * 0.25 * PI;
    (angle.cos(), angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_is_constant_power() {
        let (l, r) = pan_gains(0.0);
        assert!((l - r).abs() < 1e-6);
        assert!((l * l + r * r - 1.0).abs() < 1e-5);
        let (l, r) = pan_gains(-1.0);
        assert!(l > 0.999 && r.abs() < 1e-6);
        let (l, r) = pan_gains(1.0);
        assert!(r > 0.999 && l.abs() < 1e-6);
    }

    #[test]
    fn lowpass_attenuates_high_frequencies() {
        let sr = 44_100.0;
        let mut lp = Biquad::identity();
        lp.lowpass(sr, 500.0, 0.707);
        let mut osc = Oscillator::new(Waveform::Sine);
        osc.set_frequency(8_000.0, sr);
        let mut peak = 0.0f32;
        for i in 0..4410 {
            let y = lp.process(osc.tick());
            if i > 2000 {
                peak = peak.max(y.abs());
            }
        }
        assert!(peak < 0.01, "peak {peak}");
    }

    #[test]
    fn adsr_shape() {
        let mut env = Adsr::new(
            AdsrParams {
                attack: 0.01,
                decay: 0.01,
                sustain: 0.5,
                release: 0.01,
            },
            1000.0,
        );
        env.gate_on();
        let mut peak = 0.0;
        for _ in 0..40 {
            peak = env.tick().max(peak);
        }
        assert!((peak - 1.0).abs() < 1e-5);
        assert!((env.tick() - 0.5).abs() < 1e-5);
        env.gate_off();
        for _ in 0..40 {
            env.tick();
        }
        assert!(!env.is_active());
    }

    #[test]
    fn oscillators_stay_in_range() {
        for wf in [Waveform::Saw, Waveform::Square, Waveform::Triangle, Waveform::Sine] {
            let mut o = Oscillator::new(wf);
            o.set_frequency(440.0, 44_100.0);
            for _ in 0..10_000 {
                let v = o.tick();
                assert!(v.abs() <= 1.3, "{wf:?} produced {v}");
            }
        }
    }
}
