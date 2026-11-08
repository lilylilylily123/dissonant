//! The sampler instrument: a sampler track's chops, played from MIDI keys.
//!
//! Chop *i* answers pitch `base_pitch + i` and plays its slice of one decoded source at the
//! source's own pitch and speed — resampled only for the output rate, with the same 4-point
//! Catmull-Rom reader audio clips use, so at equal rates a chop comes out sample for sample.
//!
//! The source's buffer is bound by [`SamplerInstrument::bind`] whenever the sequence or the
//! audio pool changes; that is the only lookup. Striking a note copies everything a voice needs
//! into the voice, so editing a chop never disturbs one already sounding, and rendering touches
//! nothing but the voices and the bound buffer: no allocation, no lock, no map.

use crate::audio::{catmull_rom, AudioBuffer, AudioPool};
use dissonant_core::{ChopMode, Sampler, SamplerVoice, MAX_CHOPS};
use std::sync::Arc;
use uuid::Uuid;

/// Voices one sampler track sounds at once. Enough for a dense pattern of chops whose tails and
/// gate releases overlap; past it the oldest voice is stolen.
pub const SAMPLER_POLYPHONY: usize = 32;

/// How long a voice takes to get out of the way when it is cut short — retriggered, choked,
/// stopped by the transport — and the fade a one-shot chop ends on. Long enough not to click,
/// short enough to read as a cut rather than a fade.
const CUT_SECONDS: f64 = 0.004;

/// One chop as the voices need it: the core [`dissonant_core::Chop`] without its id and name,
/// so refreshing the list on a sequence change copies a few numbers and never a string.
#[derive(Debug, Clone, Copy)]
struct ChopPlay {
    start_seconds: f64,
    end_seconds: f64,
    gain: f32,
    reverse: bool,
    gate: bool,
    attack_seconds: f64,
    release_seconds: f64,
    choke: u8,
}

/// One sounding chop. Everything is fixed at the strike and counted in output samples, and the
/// read position is derived from the sample count rather than accumulated, so a long chop does
/// not drift off its source by rounding.
#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    active: bool,
    /// Which chop it plays, for retriggering.
    chop: usize,
    /// The key that struck it, for note-off.
    pitch: u8,
    choke: u8,
    gate: bool,
    /// A gate voice whose key is still down and that has not reached its chop's end.
    held: bool,
    /// Source frame read at the voice's first sample.
    origin: f64,
    /// Source frames per output sample; negative when the chop plays reversed.
    step: f64,
    /// Output samples sounded so far.
    elapsed: u64,
    /// Output samples until the chop's end.
    length: f64,
    /// Output samples until the read leaves the source, where even a releasing gate voice stops.
    edge: f64,
    gain: f32,
    /// Output samples the attack ramp takes; 0 starts at full level.
    attack: f64,
    /// Output samples a gate voice's release takes.
    release: f64,
    /// Level multiplier once the voice is let go, falling to zero; 1 until then.
    fade: f32,
    /// Taken off `fade` every sample; 0 while the voice is not fading.
    fade_step: f32,
    /// Strike order, so stealing takes the oldest voice.
    age: u64,
}

impl Voice {
    /// Fade out from wherever the level stands over `samples`, or stop outright when that is
    /// under one sample. Never slows a fade already under way: a release cut short by a
    /// retrigger has to get out of the way in the cut's time, not the release's.
    fn fade_over(&mut self, samples: f64) {
        self.held = false;
        if samples < 1.0 {
            self.active = false;
            return;
        }
        self.fade_step = self.fade_step.max(self.fade / samples as f32);
    }

    /// Add this voice into the outputs until the block or the voice runs out.
    fn render_add(&mut self, buffer: &AudioBuffer, out_l: &mut [f32], out_r: &mut [f32], cut: f64) {
        for (l, r) in out_l.iter_mut().zip(out_r.iter_mut()) {
            let n = self.elapsed as f64;
            if n >= self.length {
                if !self.gate {
                    self.active = false;
                    return;
                }
                // A gate voice still held at its chop's end releases there, reading on past
                // the end of the chop while it fades.
                if self.held {
                    self.fade_over(self.release);
                    if !self.active {
                        return;
                    }
                }
            }
            if n >= self.edge {
                self.active = false;
                return;
            }
            let pos = self.origin + self.step * n;
            let whole = pos.floor();
            let frac = (pos - whole) as f32;
            let i = whole as isize;
            let (l0, r0) = buffer.frame(i - 1);
            let (l1, r1) = buffer.frame(i);
            let (l2, r2) = buffer.frame(i + 1);
            let (l3, r3) = buffer.frame(i + 2);

            let mut env = self.gain * self.fade;
            if n < self.attack {
                env *= (n / self.attack) as f32;
            }
            if !self.gate {
                let tail = (self.length - n) / cut;
                if tail < 1.0 {
                    env *= tail as f32;
                }
            }
            *l += catmull_rom(l0, l1, l2, l3, frac) * env;
            *r += catmull_rom(r0, r1, r2, r3, frac) * env;

            self.elapsed += 1;
            if self.fade_step > 0.0 {
                self.fade -= self.fade_step;
                if self.fade <= 0.0 {
                    self.active = false;
                    return;
                }
            }
        }
    }
}

/// A sampler track's instrument: its chops, the bound source, and a fixed pool of voices.
#[derive(Debug)]
pub struct SamplerInstrument {
    source_id: Uuid,
    base_pitch: u8,
    /// Mono cuts everything sounding on each strike; poly layers, cut only by chop and choke.
    voice: SamplerVoice,
    chops: Vec<ChopPlay>,
    /// The source's samples, or `None` while the pool has no such source: every note is
    /// silent then, and the UI shows the audio as missing.
    buffer: Option<Arc<AudioBuffer>>,
    /// Always [`SAMPLER_POLYPHONY`] long; a free voice is one that is not `active`.
    voices: Vec<Voice>,
    sample_rate: f32,
    strikes: u64,
}

impl SamplerInstrument {
    /// An instrument for `sampler` at the output `sample_rate`, with no buffer bound yet.
    pub fn new(sampler: &Sampler, sample_rate: f32) -> Self {
        let mut s = SamplerInstrument {
            source_id: sampler.source_id,
            base_pitch: sampler.base_pitch,
            voice: sampler.voice,
            chops: Vec::with_capacity(MAX_CHOPS),
            buffer: None,
            voices: vec![Voice::default(); SAMPLER_POLYPHONY],
            sample_rate,
            strikes: 0,
        };
        s.update(sampler);
        s
    }

    /// Take a sequence's new chop list. Sounding voices keep the chop they were struck with;
    /// a changed source is picked up by the [`SamplerInstrument::bind`] that follows.
    pub fn update(&mut self, sampler: &Sampler) {
        self.source_id = sampler.source_id;
        self.base_pitch = sampler.base_pitch;
        self.voice = sampler.voice;
        self.chops.clear();
        self.chops.extend(sampler.chops.iter().map(|c| ChopPlay {
            start_seconds: c.start_seconds,
            end_seconds: c.end_seconds,
            gain: c.gain as f32,
            reverse: c.reverse,
            gate: c.mode == ChopMode::Gate,
            attack_seconds: c.attack_seconds,
            release_seconds: c.release_seconds,
            choke: c.choke,
        }));
    }

    /// Point at the pool's buffer for this sampler's source. The app re-sends the pool with
    /// the same `Arc`s whenever any source changes, so a rebind to the same buffer keeps every
    /// voice; a different buffer (or none) silences them, since their positions belong to the
    /// old one.
    pub fn bind(&mut self, pool: &AudioPool) {
        let next = pool.get(&self.source_id);
        let same = match (&self.buffer, next) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.buffer = next.cloned();
            for v in &mut self.voices {
                v.active = false;
            }
        }
    }

    /// Strike the chop on `pitch`. A key outside the chops, or a missing source, plays nothing.
    /// Every strike goes through here — sequenced, live key, pad, audition — so a mono sampler
    /// cuts the same way whichever door the hit came in by.
    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        let Some(buffer) = self.buffer.as_deref() else {
            return;
        };
        if buffer.sample_rate == 0 || buffer.frames == 0 || self.sample_rate <= 0.0 {
            return;
        }
        let Some(index) = pitch.checked_sub(self.base_pitch).map(usize::from) else {
            return;
        };
        let Some(chop) = self.chops.get(index).copied() else {
            return;
        };
        let out_sr = f64::from(self.sample_rate);
        let src_sr = f64::from(buffer.sample_rate);
        let cut = CUT_SECONDS * out_sr;

        // Mono: anything sounding gets out of the way, choke groups or not — one voice, like a
        // hardware sampler's mono mode. Poly: the same chop again, or anything in its choke
        // group.
        let mono = self.voice == SamplerVoice::Mono;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            if mono || v.chop == index || (chop.choke != 0 && v.choke == chop.choke) {
                v.fade_over(cut);
            }
        }

        let rate = src_sr / out_sr;
        let (start, end) = (chop.start_seconds * src_sr, chop.end_seconds * src_sr);
        let frames = buffer.frames as f64;
        let (origin, step, edge) = if chop.reverse {
            let origin = end - 1.0;
            (origin, -rate, (origin + 1.0) / rate)
        } else {
            (start, rate, (frames - start) / rate)
        };
        let length = ((end - start) / rate).min(edge);
        if length < 1.0 {
            return;
        }

        self.strikes += 1;
        let slot = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            None => self
                .voices
                .iter()
                .enumerate()
                .min_by_key(|(_, v)| v.age)
                .map_or(0, |(i, _)| i),
        };
        self.voices[slot] = Voice {
            active: true,
            chop: index,
            pitch,
            choke: chop.choke,
            gate: chop.gate,
            held: chop.gate,
            origin,
            step,
            elapsed: 0,
            length,
            edge,
            gain: chop.gain * (0.25 + 0.75 * f32::from(velocity.min(127)) / 127.0),
            attack: chop.attack_seconds * out_sr,
            release: chop.release_seconds * out_sr,
            fade: 1.0,
            fade_step: 0.0,
            age: self.strikes,
        };
    }

    /// Let go of `pitch`: a held gate chop releases; a one-shot plays on to its end.
    pub fn note_off(&mut self, pitch: u8) {
        for v in &mut self.voices {
            if v.active && v.held && v.pitch == pitch {
                let release = v.release;
                v.fade_over(release);
            }
        }
    }

    /// Stop everything: the transport stopped or seeked, a loop wrapped, or the notes changed
    /// under the playhead. Unlike a key's note-off this reaches one-shots too, cut in
    /// [`CUT_SECONDS`] — a minute-long chop that played on after Stop would be a bug. Gate
    /// voices release as they would for their key.
    pub fn all_notes_off(&mut self) {
        let cut = CUT_SECONDS * f64::from(self.sample_rate);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let fade = if v.gate { v.release } else { cut };
            v.fade_over(fade);
        }
    }

    pub fn is_silent(&self) -> bool {
        self.voices.iter().all(|v| !v.active)
    }

    /// Add every sounding voice into `out_l` / `out_r`.
    pub fn render_add(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let Some(buffer) = self.buffer.as_deref() else {
            return;
        };
        let cut = CUT_SECONDS * f64::from(self.sample_rate);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.render_add(buffer, out_l, out_r, cut);
        }
    }
}
