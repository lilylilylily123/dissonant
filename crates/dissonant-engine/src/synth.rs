//! Polyphonic waveform synth: a pool of oscillator + ADSR voices with velocity, round-robin
//! allocation and oldest-note stealing. Voice presets mirror the original app's `VoiceKind`.

use crate::dsp::{
    midi_to_hz, note_filter, note_pan_gains, Adsr, AdsrParams, NoteFilter, Oscillator, Waveform,
};
use crate::engine::NoteShape;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoicePreset {
    pub waveform: Waveform,
    pub env: AdsrParams,
    pub level: f32,
}

impl VoicePreset {
    /// Look a preset up by its persisted name; unknown names fall back to `saw`.
    pub fn named(name: &str) -> VoicePreset {
        match name {
            "square" => VoicePreset {
                waveform: Waveform::Square,
                env: AdsrParams {
                    attack: 0.004,
                    decay: 0.12,
                    sustain: 0.45,
                    release: 0.10,
                },
                level: 0.22,
            },
            "triangle" => VoicePreset {
                waveform: Waveform::Triangle,
                env: AdsrParams {
                    attack: 0.008,
                    decay: 0.16,
                    sustain: 0.55,
                    release: 0.16,
                },
                level: 0.30,
            },
            "sine" => VoicePreset {
                waveform: Waveform::Sine,
                env: AdsrParams {
                    attack: 0.015,
                    decay: 0.18,
                    sustain: 0.65,
                    release: 0.20,
                },
                level: 0.32,
            },
            "pad" => VoicePreset {
                waveform: Waveform::Saw,
                env: AdsrParams {
                    attack: 0.45,
                    decay: 0.40,
                    sustain: 0.80,
                    release: 0.90,
                },
                level: 0.18,
            },
            "pluck" => VoicePreset {
                waveform: Waveform::Triangle,
                env: AdsrParams {
                    attack: 0.001,
                    decay: 0.16,
                    sustain: 0.0,
                    release: 0.14,
                },
                level: 0.32,
            },
            _ => VoicePreset {
                waveform: Waveform::Saw,
                env: AdsrParams {
                    attack: 0.008,
                    decay: 0.10,
                    sustain: 0.65,
                    release: 0.14,
                },
                level: 0.22,
            },
        }
    }

    pub const ALL: [&'static str; 6] = ["saw", "square", "triangle", "sine", "pad", "pluck"];
}

/// How far a note's `release` scales the preset's own tail, in octaves either way: 0 is a
/// quarter of it, 0.5 leaves it alone, 1 stretches it fourfold.
const RELEASE_OCTAVES: f32 = 2.0;

/// Where a voice sits when its track has told it nothing. Only the chord bed and bare test
/// synths are ever in that state; a channel pushes its own tone down on every `apply`.
const DEFAULT_TONE_HZ: f32 = 20_000.0;

#[derive(Debug, Clone)]
struct Voice {
    osc: Oscillator,
    env: Adsr,
    pitch: Option<u8>,
    amp: f32,
    started_at: u64,
    /// Equal-power gains from the note's `pan`; (1, 1) when it is centred.
    gains: (f32, f32),
    /// The note's Mod X / Mod Y filter. `None` at neutral, and then the voice runs with no
    /// filter in its path at all.
    filter: Option<NoteFilter>,
}

#[derive(Debug, Clone)]
pub struct PolySynth {
    preset: VoicePreset,
    sample_rate: f32,
    /// The track's tone cutoff, which is what a note's Mod X sweeps around.
    tone_hz: f32,
    voices: Vec<Voice>,
    counter: u64,
}

impl PolySynth {
    pub fn new(preset: VoicePreset, sample_rate: f32, polyphony: usize) -> Self {
        let voices = (0..polyphony.max(1))
            .map(|_| Voice {
                osc: Oscillator::new(preset.waveform),
                env: Adsr::new(preset.env, sample_rate),
                pitch: None,
                amp: 0.0,
                started_at: 0,
                gains: (1.0, 1.0),
                filter: None,
            })
            .collect();
        PolySynth {
            preset,
            sample_rate,
            tone_hz: DEFAULT_TONE_HZ,
            voices,
            counter: 0,
        }
    }

    pub fn preset(&self) -> &VoicePreset {
        &self.preset
    }

    /// The cutoff a note's Mod X is measured against. The channel pushes its tone down here
    /// whenever the track changes, so a note only ever carries its *offset* from the track's
    /// own voicing rather than an absolute frequency.
    pub fn set_tone_hz(&mut self, hz: f32) {
        self.tone_hz = hz;
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8, shape: NoteShape) {
        self.counter += 1;
        let idx = match self
            .voices
            .iter()
            .position(|v| v.pitch.is_none() && !v.env.is_active())
        {
            Some(i) => i,
            None => match self.voices.iter().position(|v| v.pitch.is_none()) {
                // Releasing voice: reuse the one that started longest ago.
                Some(_) => self
                    .voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.pitch.is_none())
                    .min_by_key(|(_, v)| v.started_at)
                    .map(|(i, _)| i)
                    .unwrap(),
                None => self
                    .voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.started_at)
                    .map(|(i, _)| i)
                    .unwrap(),
            },
        };
        let v = &mut self.voices[idx];
        if !v.env.is_active() {
            // A free voice is already at zero; resetting keeps renders deterministic.
            v.env.reset();
            v.osc.reset();
        }
        // A *stolen* voice keeps its phase and envelope level: slamming either to zero while
        // it is still sounding is a step discontinuity, i.e. an audible click. The attack
        // simply ramps on from wherever the old note had got to.
        v.osc.set_frequency(
            midi_to_hz(pitch as f32) * (shape.fine as f32 / 1200.0).exp2(),
            self.sample_rate,
        );
        v.amp = (velocity as f32 / 127.0) * self.preset.level;
        v.env.set_release(
            self.preset.env.release
                * ((shape.release.clamp(0.0, 1.0) - 0.5) * 2.0 * RELEASE_OCTAVES).exp2(),
        );
        v.gains = note_pan_gains(shape.pan);
        v.filter = note_filter(self.sample_rate, self.tone_hz, shape.mod_x, shape.mod_y);
        v.pitch = Some(pitch);
        v.started_at = self.counter;
        v.env.gate_on();
    }

    /// Release *one* voice: the oldest one still holding `pitch`. Note-ons and note-offs are
    /// 1:1, so releasing every matching voice would make two overlapping notes of the same
    /// pitch cut each other short — the first note's end would silence the second.
    pub fn note_off(&mut self, pitch: u8) {
        let oldest = self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.pitch == Some(pitch))
            .min_by_key(|(_, v)| v.started_at)
            .map(|(i, _)| i);
        if let Some(i) = oldest {
            self.voices[i].env.gate_off();
            self.voices[i].pitch = None;
        }
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            if v.pitch.is_some() {
                v.env.gate_off();
                v.pitch = None;
            }
        }
    }

    pub fn is_silent(&self) -> bool {
        self.voices.iter().all(|v| !v.env.is_active())
    }

    /// Render into `out_l` / `out_r` (additive). Each voice is placed by its own note's pan;
    /// the channel's pan and tone sit downstream of this.
    pub fn render_add(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        let frames = out_l.len().min(out_r.len());
        for v in self.voices.iter_mut().filter(|v| v.env.is_active()) {
            let (gl, gr) = v.gains;
            match &mut v.filter {
                // Neutral Mod X / Mod Y: no filter in the path and unity gains, so this is
                // exactly the arithmetic the mono synth did before notes could be shaped.
                None => {
                    for i in 0..frames {
                        let s = v.osc.tick() * v.env.tick() * v.amp;
                        out_l[i] += s * gl;
                        out_r[i] += s * gr;
                    }
                }
                Some(filter) => {
                    for i in 0..frames {
                        let s = filter.process(v.osc.tick()) * v.env.tick() * v.amp;
                        out_l[i] += s * gl;
                        out_r[i] += s * gr;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{midi_to_hz, Adsr, Oscillator};

    const SR: f32 = 48_000.0;

    fn shaped(preset: &str, pitch: u8, shape: NoteShape, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut s = PolySynth::new(VoicePreset::named(preset), SR, 1);
        s.note_on(pitch, 100, shape);
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        s.render_add(&mut l, &mut r);
        (l, r)
    }

    fn rms(xs: &[f32]) -> f32 {
        (xs.iter().map(|x| x * x).sum::<f32>() / xs.len() as f32).sqrt()
    }

    /// Energy above roughly a quarter of Nyquist: a first difference is a crude high-pass, and
    /// that is all a "is this duller?" comparison needs.
    fn bright_rms(xs: &[f32]) -> f32 {
        let d: Vec<f32> = xs.windows(2).map(|w| w[1] - w[0]).collect();
        rms(&d)
    }

    fn zero_crossings(xs: &[f32]) -> usize {
        xs.windows(2)
            .filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0))
            .count()
    }

    #[test]
    fn plays_and_releases() {
        let mut s = PolySynth::new(VoicePreset::named("saw"), SR, 4);
        assert!(s.is_silent());
        s.note_on(60, 100, NoteShape::neutral());
        let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
        s.render_add(&mut l, &mut r);
        assert!(l.iter().any(|x| x.abs() > 0.01));
        s.note_off(60);
        let (mut tl, mut tr) = (vec![0.0; 48_000], vec![0.0; 48_000]);
        s.render_add(&mut tl, &mut tr);
        assert!(s.is_silent());
    }

    #[test]
    fn steals_oldest_voice_when_full() {
        let mut s = PolySynth::new(VoicePreset::named("pad"), SR, 2);
        s.note_on(60, 100, NoteShape::neutral());
        s.note_on(64, 100, NoteShape::neutral());
        s.note_on(67, 100, NoteShape::neutral());
        let sounding: Vec<u8> = s.voices.iter().filter_map(|v| v.pitch).collect();
        assert_eq!(sounding.len(), 2);
        assert!(!sounding.contains(&60));
    }

    #[test]
    fn velocity_scales_amplitude() {
        let peak = |vel: u8| {
            let mut s = PolySynth::new(VoicePreset::named("sine"), SR, 1);
            s.note_on(69, vel, NoteShape::neutral());
            let (mut l, mut r) = (vec![0.0; 9600], vec![0.0; 9600]);
            s.render_add(&mut l, &mut r);
            l.iter().fold(0.0f32, |m, x| m.max(x.abs()))
        };
        assert!(peak(127) > peak(40) * 2.0);
    }

    /// The bypass proof. A note nobody has shaped has to come out of the stereo voice as
    /// exactly the samples the mono voice used to produce: oscillator × envelope × amp, with
    /// no filter in the path and no pan scaling. Not "close" — the same bits, on both sides.
    #[test]
    fn a_neutral_note_renders_bit_for_bit_as_the_unshaped_voice() {
        let preset = VoicePreset::named("saw");
        let mut osc = Oscillator::new(preset.waveform);
        osc.set_frequency(midi_to_hz(69.0), SR);
        let mut env = Adsr::new(preset.env, SR);
        env.gate_on();
        let amp = (100.0 / 127.0) * preset.level;
        let expected: Vec<f32> = (0..4800).map(|_| osc.tick() * env.tick() * amp).collect();

        let (l, r) = shaped("saw", 69, NoteShape::neutral(), 4800);
        assert_eq!(l, expected, "left side drifted from the unshaped voice");
        assert_eq!(r, expected, "right side drifted from the unshaped voice");
    }

    #[test]
    fn fine_cents_retune_the_voice() {
        let frames = 9600;
        let plain = shaped("sine", 69, NoteShape::neutral(), frames).0;
        let octave_up = shaped(
            "sine",
            69,
            NoteShape {
                fine: 1200,
                ..NoteShape::neutral()
            },
            frames,
        )
        .0;
        let (a, b) = (zero_crossings(&plain), zero_crossings(&octave_up));
        assert!(a > 100, "the reference note must actually oscillate: {a}");
        let ratio = b as f32 / a as f32;
        assert!(
            (ratio - 2.0).abs() < 0.05,
            "+1200 cents gave {b} crossings against {a} ({ratio})"
        );
        // And the sign is right: a negative detune goes down, not up.
        let octave_down = shaped(
            "sine",
            69,
            NoteShape {
                fine: -1200,
                ..NoteShape::neutral()
            },
            frames,
        )
        .0;
        assert!((zero_crossings(&octave_down) as f32 / a as f32 - 0.5).abs() < 0.05);
    }

    #[test]
    fn pan_places_the_voice_and_centre_stays_balanced() {
        let (l, r) = shaped(
            "sine",
            69,
            NoteShape {
                pan: 0.0,
                ..NoteShape::neutral()
            },
            9600,
        );
        assert!(rms(&l) > 0.01, "hard left must still speak");
        assert!(
            rms(&r) < rms(&l) * 1e-6,
            "hard left leaked into the right: {}",
            rms(&r)
        );

        let (l, r) = shaped(
            "sine",
            69,
            NoteShape {
                pan: 1.0,
                ..NoteShape::neutral()
            },
            9600,
        );
        assert!(rms(&r) > 0.01);
        assert!(rms(&l) < rms(&r) * 1e-6);

        let (l, r) = shaped("sine", 69, NoteShape::neutral(), 9600);
        assert_eq!(l, r, "a centred note must be identical on both sides");
    }

    #[test]
    fn release_lengthens_the_tail() {
        let tail_frames = |release: f32| {
            let mut s = PolySynth::new(VoicePreset::named("sine"), SR, 1);
            s.note_on(
                69,
                100,
                NoteShape {
                    release,
                    ..NoteShape::neutral()
                },
            );
            let (mut l, mut r) = (vec![0.0; 4800], vec![0.0; 4800]);
            s.render_add(&mut l, &mut r);
            s.note_off(69);
            let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
            let mut n = 0;
            while !s.is_silent() && n < SR as usize * 8 {
                l.fill(0.0);
                r.fill(0.0);
                s.render_add(&mut l, &mut r);
                n += 256;
            }
            n
        };
        let short = tail_frames(0.0);
        let long = tail_frames(1.0);
        assert!(short > 0, "even the shortest release has to ring briefly");
        assert!(
            long > short * 4,
            "release 1.0 gave {long} frames against {short} at 0.0"
        );
    }

    #[test]
    fn mod_x_closes_the_filter_and_neutral_leaves_it_open() {
        let render = |mod_x: f32| {
            let mut s = PolySynth::new(VoicePreset::named("saw"), SR, 1);
            s.set_tone_hz(2_000.0);
            s.note_on(
                69,
                100,
                NoteShape {
                    mod_x,
                    ..NoteShape::neutral()
                },
            );
            let (mut l, mut r) = (vec![0.0; 9600], vec![0.0; 9600]);
            s.render_add(&mut l, &mut r);
            l
        };
        let dark = render(0.0);
        let open = render(1.0);
        assert!(rms(&open) > 0.001, "the open voice must speak");
        assert!(
            bright_rms(&dark) < bright_rms(&open) * 0.25,
            "mod_x 0 was not duller: {} vs {}",
            bright_rms(&dark),
            bright_rms(&open)
        );
        // Neutral is the bypass, so it keeps every bit of the raw saw's brightness.
        let neutral = render(0.5);
        assert!(bright_rms(&neutral) > bright_rms(&dark) * 4.0);
    }

    #[test]
    fn mod_y_rings_the_filter_at_its_cutoff() {
        let render = |mod_y: f32| {
            let mut s = PolySynth::new(VoicePreset::named("saw"), SR, 1);
            s.set_tone_hz(2_000.0);
            s.note_on(
                69,
                100,
                NoteShape {
                    mod_x: 0.4,
                    mod_y,
                    ..NoteShape::neutral()
                },
            );
            let (mut l, mut r) = (vec![0.0; 9600], vec![0.0; 9600]);
            s.render_add(&mut l, &mut r);
            l
        };
        // Resonance peaks the band around the cutoff, so the same source comes out louder.
        assert!(rms(&render(1.0)) > rms(&render(0.0)) * 1.5);
    }
}
