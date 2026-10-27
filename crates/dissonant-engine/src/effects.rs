//! The stock effects, as audio.
//!
//! One struct per entry in [`dissonant_core::effects`], all of them built out of the primitives
//! in [`crate::dsp`] that the guitar rig has been using all along. The whole module obeys the
//! audio thread's rules: a [`NativeEffect`] is constructed on the app thread, where allocating a
//! reverb tank or a two-second delay line is free, and after that nothing it does allocates,
//! locks or blocks — `set_param` recomputes coefficients and `process` runs a loop.
//!
//! Parameters arrive by the id [`dissonant_core::effects::NativeParam::id`] gives them, the same
//! id a project file and an automation lane store, so there is exactly one name for a knob from
//! the UI all the way down to the filter it moves.

use crate::dsp::{
    db_to_linear, Compressor, DelayLine, Echo, Limiter, Reverb, StereoBiquad, TubeStage,
};
use dissonant_core::effects::{NativeKind, ParamMap};

/// Output below this counts as silence, so a chain whose input has stopped can be skipped once
/// its tails have run out. The same floor the channel meter uses.
const SILENCE: f32 = 1e-5;

/// Longest repeat the delay can be asked for: a whole bar of 4/4 at 30 bpm.
const MAX_DELAY_S: f32 = 8.0;

/// Longest pre-delay the reverb offers, in seconds. Matches the parameter's own ceiling.
const MAX_PREDELAY_S: f32 = 0.2;

/// One stock effect, ready to run.
///
/// `is_silent` is answered from the last block's output rather than from a per-effect tail
/// length: a reverb that is still ringing is audibly not silent, and once it has fallen below
/// the floor the chain can be skipped until something feeds it again.
#[derive(Debug)]
pub struct NativeEffect {
    kind: NativeKind,
    unit: Unit,
    /// Peak of the last block this effect wrote.
    last_peak: f32,
}

#[derive(Debug)]
enum Unit {
    Eq(Box<Eq>),
    Compressor(Comp),
    Limiter(Lim),
    Saturator(Box<Sat>),
    Delay(Box<Dly>),
    Reverb(Box<Rev>),
}

impl NativeEffect {
    /// Build an effect at its defaults. Allocates (a reverb tank, a delay line, the oversampled
    /// tube's filters), so it belongs on the app thread — the engine is handed the finished
    /// thing, exactly as it is handed a loaded plugin.
    pub fn new(kind: NativeKind, sample_rate: f32) -> Self {
        let unit = match kind {
            NativeKind::Eq => Unit::Eq(Box::new(Eq::new(sample_rate))),
            NativeKind::Compressor => Unit::Compressor(Comp::new(sample_rate)),
            NativeKind::Limiter => Unit::Limiter(Lim::new(sample_rate)),
            NativeKind::Saturator => Unit::Saturator(Box::new(Sat::new(sample_rate))),
            NativeKind::Delay => Unit::Delay(Box::new(Dly::new(sample_rate))),
            NativeKind::Reverb => Unit::Reverb(Box::new(Rev::new(sample_rate))),
        };
        let mut effect = NativeEffect {
            kind,
            unit,
            last_peak: 0.0,
        };
        for param in kind.params() {
            effect.set_param(param.id, param.default);
        }
        effect
    }

    /// Build an effect and dial in a slot's stored values in one go, which is what loading a
    /// project and adding an effect both want.
    pub fn with_params(kind: NativeKind, sample_rate: f32, params: &ParamMap) -> Self {
        let mut effect = NativeEffect::new(kind, sample_rate);
        for param in kind.params() {
            effect.set_param(param.id, kind.value_of(params, param.id));
        }
        effect
    }

    pub fn kind(&self) -> NativeKind {
        self.kind
    }

    /// Move one knob. Called from the audio thread on every drag frame, so it recomputes only
    /// what the id names and never touches the heap. An id this effect does not own is ignored
    /// rather than clamped onto something else.
    pub fn set_param(&mut self, id: u32, value: f64) {
        let Some(param) = self.kind.param(id) else {
            return;
        };
        let v = param.clamp(value) as f32;
        match &mut self.unit {
            Unit::Eq(e) => e.set(id, v),
            Unit::Compressor(c) => c.set(id, v),
            Unit::Limiter(l) => l.set(id, v),
            Unit::Saturator(s) => s.set(id, v),
            Unit::Delay(d) => d.set(id, v),
            Unit::Reverb(r) => r.set(id, v),
        }
    }

    /// Run one block in place. `beat_seconds` is the length of a beat where the playhead is;
    /// only the delay reads it.
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], beat_seconds: f32) {
        match &mut self.unit {
            Unit::Eq(e) => e.process(l, r),
            Unit::Compressor(c) => c.process(l, r),
            Unit::Limiter(x) => x.process(l, r),
            Unit::Saturator(s) => s.process(l, r),
            Unit::Delay(d) => d.process(l, r, beat_seconds),
            Unit::Reverb(x) => x.process(l, r),
        }
        let mut peak = 0.0f32;
        for i in 0..l.len() {
            peak = peak.max(l[i].abs().max(r[i].abs()));
        }
        self.last_peak = peak;
    }

    /// Whether the effect has stopped making sound. An effect that has never run is silent, so
    /// a chain of fresh effects costs nothing until something plays into it.
    pub fn is_silent(&self) -> bool {
        self.last_peak < SILENCE
    }
}

// ─── EQ ──────────────────────────────────────────────────────────────────────────────────────

/// High-pass, low shelf, three peaks, high shelf, low-pass, in that order.
///
/// Every stage is always in the cascade, even at its neutral setting, so moving a knob off and
/// back on does not change the phase response underneath the others. The two cuts are the
/// exception the user can hear: at the ends of their ranges they are set to identity rather
/// than to a 20 Hz / 20 kHz filter, because a high-pass "off" has to mean off.
#[derive(Debug)]
struct Eq {
    sample_rate: f32,
    high_pass: StereoBiquad,
    low_shelf: StereoBiquad,
    bands: [StereoBiquad; 3],
    high_shelf: StereoBiquad,
    low_pass: StereoBiquad,
    /// Raw values, so changing one half of a two-knob stage can rebuild it from both.
    hp_hz: f32,
    lp_hz: f32,
    low: (f32, f32),
    high: (f32, f32),
    band: [(f32, f32, f32); 3],
}

impl Eq {
    fn new(sample_rate: f32) -> Self {
        Eq {
            sample_rate,
            high_pass: StereoBiquad::identity(),
            low_shelf: StereoBiquad::identity(),
            bands: [StereoBiquad::identity(); 3],
            high_shelf: StereoBiquad::identity(),
            low_pass: StereoBiquad::identity(),
            hp_hz: 20.0,
            lp_hz: 20_000.0,
            low: (120.0, 0.0),
            high: (6_000.0, 0.0),
            band: [(220.0, 0.0, 1.0), (1_200.0, 0.0, 1.0), (5_000.0, 0.0, 1.0)],
        }
    }

    fn set(&mut self, id: u32, v: f32) {
        match id {
            100 => {
                self.hp_hz = v;
                self.rebuild_cuts();
            }
            114 => {
                self.lp_hz = v;
                self.rebuild_cuts();
            }
            101 => {
                self.low.0 = v;
                self.rebuild_shelves();
            }
            102 => {
                self.low.1 = v;
                self.rebuild_shelves();
            }
            112 => {
                self.high.0 = v;
                self.rebuild_shelves();
            }
            113 => {
                self.high.1 = v;
                self.rebuild_shelves();
            }
            103..=111 => {
                let i = ((id - 103) / 3) as usize;
                match (id - 103) % 3 {
                    0 => self.band[i].0 = v,
                    1 => self.band[i].1 = v,
                    _ => self.band[i].2 = v,
                }
                self.rebuild_band(i);
            }
            _ => {}
        }
    }

    fn rebuild_cuts(&mut self) {
        let sr = self.sample_rate;
        let (hp, lp) = (self.hp_hz, self.lp_hz);
        if hp <= 20.0 {
            self.high_pass = StereoBiquad::identity();
        } else {
            self.high_pass.configure(|b| b.highpass(sr, hp, 0.707));
        }
        if lp >= 20_000.0 {
            self.low_pass = StereoBiquad::identity();
        } else {
            self.low_pass.configure(|b| b.lowpass(sr, lp, 0.707));
        }
    }

    fn rebuild_shelves(&mut self) {
        let sr = self.sample_rate;
        let (lf, lg) = self.low;
        let (hf, hg) = self.high;
        self.low_shelf.configure(|b| b.low_shelf(sr, lf, lg, 0.7));
        self.high_shelf.configure(|b| b.high_shelf(sr, hf, hg, 0.7));
    }

    fn rebuild_band(&mut self, i: usize) {
        let sr = self.sample_rate;
        let (f, g, q) = self.band[i];
        self.bands[i].configure(|b| b.peaking(sr, f, g, q));
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for i in 0..l.len() {
            let (mut a, mut b) = self.high_pass.process(l[i], r[i]);
            (a, b) = self.low_shelf.process(a, b);
            for band in &mut self.bands {
                (a, b) = band.process(a, b);
            }
            (a, b) = self.high_shelf.process(a, b);
            (a, b) = self.low_pass.process(a, b);
            l[i] = a;
            r[i] = b;
        }
    }
}

// ─── Compressor ──────────────────────────────────────────────────────────────────────────────

/// Stereo-linked: one detector over the louder of the two sides, one gain applied to both.
/// Compressing the channels independently would pull the image toward whichever side is quiet.
#[derive(Debug)]
struct Comp {
    comp: Compressor,
    sample_rate: f32,
    threshold_db: f32,
    ratio: f32,
    makeup_db: f32,
    attack_ms: f32,
    release_ms: f32,
}

impl Comp {
    fn new(sample_rate: f32) -> Self {
        Comp {
            comp: Compressor::new(sample_rate),
            sample_rate,
            threshold_db: -18.0,
            ratio: 4.0,
            makeup_db: 0.0,
            attack_ms: 10.0,
            release_ms: 150.0,
        }
    }

    fn set(&mut self, id: u32, v: f32) {
        match id {
            200 => self.threshold_db = v,
            201 => self.ratio = v,
            202 => self.attack_ms = v,
            203 => self.release_ms = v,
            204 => {
                self.comp.set_knee(v);
                return;
            }
            205 => self.makeup_db = v,
            _ => return,
        }
        self.comp
            .set(self.threshold_db, self.ratio, self.makeup_db);
        self.comp
            .set_times(self.sample_rate, self.attack_ms, self.release_ms);
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let makeup = self.comp.makeup();
        for i in 0..l.len() {
            let key = if l[i].abs() > r[i].abs() { l[i] } else { r[i] };
            let g = self.comp.gain_for(key) * makeup;
            l[i] *= g;
            r[i] *= g;
        }
    }
}

// ─── Limiter ─────────────────────────────────────────────────────────────────────────────────

/// Drive into the master bus's own lookahead limiter, with the ceiling on a knob. Unlike the
/// compressor this one guarantees its ceiling rather than asking for it, which is why it is a
/// separate effect and not a ratio of 20.
#[derive(Debug)]
struct Lim {
    limiter: Limiter,
    drive: f32,
    ceiling: f32,
}

impl Lim {
    fn new(sample_rate: f32) -> Self {
        Lim {
            limiter: Limiter::new(sample_rate),
            drive: 1.0,
            ceiling: db_to_linear(-0.3),
        }
    }

    fn set(&mut self, id: u32, v: f32) {
        match id {
            300 => self.ceiling = db_to_linear(v),
            301 => self.drive = db_to_linear(v),
            302 => self.limiter.set_release_ms(v),
            _ => {}
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let (drive, ceiling) = (self.drive, self.ceiling);
        for i in 0..l.len() {
            let (a, b) = self.limiter.process(l[i] * drive, r[i] * drive, ceiling);
            l[i] = a;
            r[i] = b;
        }
    }
}

// ─── Saturator ───────────────────────────────────────────────────────────────────────────────

/// One 12AX7 stage per side, blended back against the dry signal.
///
/// A tube stage is not unity: a hot setting is loud and the output trim is how you put it back.
/// The dry/wet blend is what makes it usable on a bus, where all you want is the first few
/// percent of the curve.
#[derive(Debug)]
struct Sat {
    left: TubeStage,
    right: TubeStage,
    sample_rate: f32,
    gain: f32,
    bias: f32,
    emphasis_hz: f32,
    mix: f32,
    out: f32,
}

impl Sat {
    fn new(sample_rate: f32) -> Self {
        Sat {
            left: TubeStage::new(sample_rate),
            right: TubeStage::new(sample_rate),
            sample_rate,
            gain: 3.0,
            bias: -0.2,
            emphasis_hz: 0.0,
            mix: 1.0,
            out: 1.0,
        }
    }

    fn set(&mut self, id: u32, v: f32) {
        match id {
            400 => self.gain = v,
            401 => self.bias = v,
            402 => self.emphasis_hz = v,
            403 => {
                self.mix = v;
                return;
            }
            404 => {
                self.out = db_to_linear(v);
                return;
            }
            _ => return,
        }
        let (sr, g, bias, hz) = (self.sample_rate, self.gain, self.bias, self.emphasis_hz);
        self.left.set(sr, g, bias, hz);
        self.right.set(sr, g, bias, hz);
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let (mix, out) = (self.mix, self.out);
        for i in 0..l.len() {
            let (dl, dr) = (l[i], r[i]);
            l[i] = (dl + mix * (self.left.process(dl) - dl)) * out;
            r[i] = (dr + mix * (self.right.process(dr) - dr)) * out;
        }
    }
}

// ─── Delay ───────────────────────────────────────────────────────────────────────────────────

/// Tempo-synced feedback delay, one [`Echo`] per side.
///
/// The repeat time is derived from the song's tempo every block rather than stored, so a tempo
/// change — including a ramp — carries the repeats with it and an offline bounce of a song with
/// a tempo map lands them in exactly the same places playback did.
///
/// [`Echo`] glides to a new time like tape, which is the right behaviour for a knob and the
/// wrong one for the very first block: a delay added mid-song would spend 50 ms sliding from
/// the constructor's 300 ms to where it belongs. So the first block it ever runs jumps.
#[derive(Debug)]
struct Dly {
    left: Echo,
    right: Echo,
    /// `delay 1/x`: the note value a repeat lands on, against a quarter-note beat.
    division: f32,
    feedback: f32,
    mix: f32,
    primed: bool,
}

impl Dly {
    fn new(sample_rate: f32) -> Self {
        Dly {
            left: Echo::new(sample_rate, MAX_DELAY_S),
            right: Echo::new(sample_rate, MAX_DELAY_S),
            division: 8.0,
            feedback: 0.35,
            mix: 0.3,
            primed: false,
        }
    }

    fn set(&mut self, id: u32, v: f32) {
        match id {
            500 => self.division = v.max(1.0),
            501 => self.feedback = v,
            502 => self.mix = v,
            _ => {}
        }
    }

    /// The gap between repeats: `4 / division` beats, so 4 is one beat and 8 is half of one.
    fn seconds(&self, beat_seconds: f32) -> f32 {
        (beat_seconds * 4.0 / self.division).clamp(0.001, MAX_DELAY_S)
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32], beat_seconds: f32) {
        let seconds = self.seconds(beat_seconds);
        if !self.primed {
            self.left.reset(seconds);
            self.right.reset(seconds);
            self.primed = true;
        }
        let (fb, mix) = (self.feedback, self.mix);
        for i in 0..l.len() {
            l[i] = self.left.process(l[i], seconds, fb, mix);
            r[i] = self.right.process(r[i], seconds, fb, mix);
        }
    }
}

// ─── Reverb ──────────────────────────────────────────────────────────────────────────────────

/// Pre-delay and a damping cut into a Freeverb tank.
///
/// Both sit in front of the tank, not behind it: filtering the wet return would dull the tail
/// without changing what the tank is fed, and delaying it would push the dry signal's own
/// timing around. The dry path is the one [`Reverb`] keeps internally, so the mix knob stays a
/// constant-power crossfade.
#[derive(Debug)]
struct Rev {
    reverb: Reverb,
    pre_l: DelayLine,
    pre_r: DelayLine,
    damp: StereoBiquad,
    sample_rate: f32,
    /// Pre-delay as a read distance, never below one sample — a [`DelayLine`] read of zero is
    /// not a tap at all.
    pre_samples: f32,
    mix: f32,
}

impl Rev {
    fn new(sample_rate: f32) -> Self {
        let max = (MAX_PREDELAY_S * sample_rate) as usize + 4;
        let mut rev = Rev {
            reverb: Reverb::new(sample_rate),
            pre_l: DelayLine::new(max),
            pre_r: DelayLine::new(max),
            damp: StereoBiquad::identity(),
            sample_rate,
            pre_samples: 1.0,
            mix: 0.25,
        };
        rev.reverb.wet = 0.25;
        rev
    }

    fn set(&mut self, id: u32, v: f32) {
        match id {
            600 => {
                self.mix = v;
                self.reverb.wet = v;
            }
            601 => self.reverb.set_decay(v),
            602 => {
                let samples = v * 0.001 * self.sample_rate;
                self.pre_samples = samples.clamp(1.0, self.pre_l.capacity());
            }
            603 => {
                let sr = self.sample_rate;
                self.damp.configure(|b| b.lowpass(sr, v, 0.707));
            }
            _ => {}
        }
    }

    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        // Below audibility the tank is skipped outright, exactly as `Reverb::process` does, so
        // a reverb parked at zero costs a multiply.
        if self.mix <= 0.0001 {
            return;
        }
        let d = self.pre_samples;
        for i in 0..l.len() {
            let (dry_l, dry_r) = (l[i], r[i]);
            let (pl, pr) = (self.pre_l.read(d), self.pre_r.read(d));
            self.pre_l.write(dry_l);
            self.pre_r.write(dry_r);
            let (fl, fr) = self.damp.process(pl, pr);
            // The tank crossfades against whatever it is handed as dry, so it is handed the
            // delayed, damped signal and then the real dry is swapped back in: the ambience is
            // late and dull, the direct sound is neither.
            let (wl, wr) = self.reverb.process(fl, fr);
            let g = (1.0 - self.mix).sqrt();
            l[i] = wl - fl * g + dry_l * g;
            r[i] = wr - fr * g + dry_r * g;
        }
    }
}
