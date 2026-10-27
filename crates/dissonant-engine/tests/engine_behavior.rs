//! Behavioural audit of the realtime engine and mixer.
//!
//! Everything here is offline: either `render::render_to_buffer` or a hand-driven
//! `Engine::process` loop. No audio device, no sleeps, no randomness.
//!
//! Spectral claims are measured with a Goertzel bin rather than eyeballed from samples.

use std::f64::consts::PI;
use std::sync::Arc;

use dissonant_core::{
    AutoTarget, AutomationLane, Breakpoint, Clip, Curve, MasterParam, MasterSettings, NoteEvent,
    ProjectModel, Sequence, SongPattern, TrackParam, TICKS_PER_BEAT,
};
use dissonant_engine::channel::Master;
use dissonant_engine::engine::{Engine, EngineCommand, Shared};
use dissonant_engine::render::{render_to_buffer, RenderOptions, RenderPlugins};

const SR: f32 = 44_100.0;
const SRD: f64 = 44_100.0;
/// Ticks per beat, short enough to keep each test's musical intent readable.
const B: i64 = TICKS_PER_BEAT;

// ─── helpers ─────────────────────────────────────────────────────────────────────────────────

fn model(voice: &str, length_ticks: i64) -> ProjectModel {
    let mut m = ProjectModel::empty();
    m.tracks[0].voice = voice.to_string();
    m.patterns[0].length_ticks = length_ticks;
    m
}

/// Put `notes` in the track's own pattern, making one (as long as the first) on first use.
fn set_notes(m: &mut ProjectModel, track: usize, notes: Vec<NoteEvent>) {
    let id = m.tracks[track].id;
    let length = m.patterns[0].length_ticks;
    match m.patterns.iter_mut().find(|p| p.track_id == id) {
        Some(p) => p.notes = notes,
        None => {
            let mut p = SongPattern::new(id, "p");
            p.length_ticks = length;
            p.notes = notes;
            m.patterns.push(p);
        }
    }
}

/// The first pattern's loop with every track's own pattern playing along at pattern-local
/// ticks, as if each were placed at tick 0 (so multi-track mixes can be measured).
fn sequence(m: &ProjectModel) -> Arc<Sequence> {
    let mut seq = Sequence::from_pattern(m, &m.patterns[0].id, 0).expect("pattern exists");
    for t in &mut seq.tracks {
        if let Some(p) = m.patterns.iter().find(|p| p.track_id == t.id) {
            t.notes = p.notes.clone();
        }
    }
    Arc::new(seq)
}

/// Offline render of the model's first pattern. Returns de-interleaved (left, right).
fn render(m: &ProjectModel, tail_seconds: f64) -> (Vec<f32>, Vec<f32>) {
    let opts = RenderOptions {
        sample_rate: SR as u32,
        tail_seconds,
        master: m.master.clone(),
        // Measurements below compare raw sample values, so no normalising, dither or extra loops.
        ..RenderOptions::default()
    };
    let inter = render_to_buffer(sequence(m), &opts, RenderPlugins::default()).expect("render");
    split(&inter)
}

fn split(interleaved: &[f32]) -> (Vec<f32>, Vec<f32>) {
    (
        interleaved.iter().step_by(2).copied().collect(),
        interleaved.iter().skip(1).step_by(2).copied().collect(),
    )
}

/// Drive the engine by hand (looping stays on) and return de-interleaved stereo.
fn run(engine: &mut Engine, frames: usize, block: usize) -> (Vec<f32>, Vec<f32>) {
    let mut out = Vec::with_capacity(frames * 2);
    let mut buf = vec![0.0f32; block * 2];
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(block);
        engine.process(&mut buf[..n * 2]);
        out.extend_from_slice(&buf[..n * 2]);
        done += n;
    }
    split(&out)
}

fn engine_with(m: &ProjectModel) -> (Engine, Arc<Shared>) {
    let shared = Arc::new(Shared::default());
    let mut e = Engine::new(SR, Arc::clone(&shared), &m.master);
    e.handle(EngineCommand::SetSequence(sequence(m)));
    (e, shared)
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |a, b| a.max(b.abs()))
}

fn rms(x: &[f32]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len() as f64).sqrt()
}

/// Amplitude of the sinusoidal component nearest `freq`, via Goertzel.
fn tone_amplitude(x: &[f32], freq: f64) -> f64 {
    let n = x.len() as f64;
    if n < 8.0 {
        return 0.0;
    }
    let k = (freq / SRD * n).round().max(1.0);
    let w = 2.0 * PI * k / n;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s = v as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    let re = s1 - s2 * w.cos();
    let im = s2 * w.sin();
    2.0 * (re * re + im * im).sqrt() / n
}

fn secs(s: f64) -> usize {
    (s * SRD) as usize
}

/// Largest sample-to-sample step inside a window — a proxy for an audible click.
fn max_slew(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0f32, |a, w| a.max((w[1] - w[0]).abs()))
}

fn midi_hz(p: i32) -> f64 {
    440.0 * 2f64.powf((p as f64 - 69.0) / 12.0)
}

fn assert_finite(x: &[f32], what: &str) {
    for (i, s) in x.iter().enumerate() {
        assert!(s.is_finite(), "{what}: sample {i} is {s}");
        assert!(s.abs() <= 1.0, "{what}: sample {i} = {s} escapes [-1, 1]");
    }
}

// ─── 1. pan ──────────────────────────────────────────────────────────────────────────────────

fn panned_peaks(pan: f64) -> (f32, f32) {
    let mut m = model("sine", 2 * B);
    m.tracks[0].pan = pan;
    set_notes(&mut m, 0, vec![NoteEvent::new(0, B, 69)]);
    let (l, r) = render(&m, 0.5);
    (peak(&l), peak(&r))
}

#[test]
fn pan_moves_energy_between_channels() {
    let (hard_l_left, hard_l_right) = panned_peaks(-1.0);
    let (center_l, center_r) = panned_peaks(0.0);
    let (hard_r_left, hard_r_right) = panned_peaks(1.0);

    assert!(
        hard_l_left > 0.01,
        "pan -1 produced no left signal: {hard_l_left}"
    );
    assert!(
        hard_l_right < hard_l_left * 1e-3,
        "pan -1 leaked into the right channel: l={hard_l_left} r={hard_l_right}"
    );
    assert!(
        hard_r_right > 0.01,
        "pan +1 produced no right signal: {hard_r_right}"
    );
    assert!(
        hard_r_left < hard_r_right * 1e-3,
        "pan +1 leaked into the left channel: l={hard_r_left} r={hard_r_right}"
    );

    // Centre must be balanced...
    assert!(
        (center_l - center_r).abs() < center_l * 1e-3,
        "centre pan is not balanced: l={center_l} r={center_r}"
    );
    // ...and constant power: -3 dB per side relative to a hard-panned side.
    let ratio = center_l / hard_l_left;
    assert!(
        (ratio - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.02,
        "centre/hard ratio {ratio} is not the constant-power -3 dB ({})",
        std::f32::consts::FRAC_1_SQRT_2
    );
}

#[test]
fn pan_is_monotonic_across_the_sweep() {
    let mut last_bias = f32::NEG_INFINITY;
    for pan in [-1.0, -0.5, 0.0, 0.5, 1.0] {
        let (l, r) = panned_peaks(pan);
        let bias = r - l;
        assert!(
            bias > last_bias,
            "pan {pan} did not move energy further right (bias {bias} <= {last_bias})"
        );
        last_bias = bias;
    }
}

// ─── 2. per-track volume / tone / reverb send ────────────────────────────────────────────────

#[test]
fn track_volume_scales_the_output() {
    let build = |vol: f64| {
        let mut m = model("sine", 2 * B);
        m.tracks[0].volume = vol;
        set_notes(&mut m, 0, vec![NoteEvent::new(0, B, 69)]);
        let (l, _) = render(&m, 0.5);
        peak(&l)
    };
    let full = build(1.0);
    let half = build(0.5);
    let zero = build(0.0);
    assert!(full > 0.05, "unity volume is inaudible: {full}");
    assert!(
        ((half / full) - 0.5).abs() < 0.02,
        "volume 0.5 gave {half} against {full} (ratio {})",
        half / full
    );
    assert!(zero < 1e-6, "volume 0 still produced {zero}");
}

#[test]
fn track_tone_lowpasses_a_bright_voice() {
    // A 220 Hz saw: fundamental well under the cutoff, 8th harmonic well over it.
    let build = |tone: f64| {
        let mut m = model("saw", 2 * B);
        m.tracks[0].tone = tone;
        set_notes(&mut m, 0, vec![NoteEvent::new(0, B, 57)]);
        let (l, _) = render(&m, 0.5);
        // Measure inside the sustain, away from the attack transient.
        let w = l[secs(0.10)..secs(0.40)].to_vec();
        (tone_amplitude(&w, 220.0), tone_amplitude(&w, 1760.0))
    };
    let (open_f0, open_h8) = build(18_000.0);
    let (dark_f0, dark_h8) = build(300.0);

    assert!(open_h8 > 0.0, "bright render has no 8th harmonic");
    assert!(
        dark_f0 / open_f0 > 0.6,
        "tone 300 Hz crushed the 220 Hz fundamental too ({dark_f0} vs {open_f0}) — that is a volume knob, not a tone knob"
    );
    assert!(
        dark_h8 / open_h8 < 0.1,
        "tone 300 Hz left {:.1}% of the 1760 Hz energy — the lowpass is not doing anything",
        100.0 * dark_h8 / open_h8
    );
}

#[test]
fn reverb_send_adds_a_tail_and_zero_adds_none() {
    let build = |send: f64| {
        let mut m = model("sine", 2 * B);
        m.tracks[0].reverb_send = send;
        set_notes(&mut m, 0, vec![NoteEvent::new(0, B / 2, 69)]);
        let (l, _) = render(&m, 1.5);
        l
    };
    // note off at 0.25 s, "sine" release is 0.20 s → dry signal is gone well before 0.7 s.
    let window = |l: &[f32]| rms(&l[secs(0.80)..secs(1.10)]);

    let dry = build(0.0);
    let wet = build(0.6);
    assert!(peak(&dry) > 0.05, "dry render is silent");
    assert!(
        window(&dry) < 1e-6,
        "reverb_send = 0 still produced a tail (rms {})",
        window(&dry)
    );
    assert!(
        window(&wet) > 1e-3,
        "reverb_send = 0.6 produced no tail after note-off (rms {})",
        window(&wet)
    );
}

/// Deterministic broadband noise — the honest worst case for a reverb's gain staging.
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

#[test]
fn the_reverb_mix_knob_changes_ambience_not_level() {
    // Both "reverb" knobs in the UI are a 0–100% dry/wet mix. Sweeping one must not change how
    // loud the bus is: with a steady input the output rms has to hold across the whole sweep.
    let level = |wet: f32| {
        let mut rv = dissonant_engine::dsp::Reverb::new(SR);
        rv.wet = wet;
        let mut rng = Lcg(1);
        let mut out = Vec::with_capacity(secs(4.0));
        for _ in 0..secs(4.0) {
            let x = rng.next() * 0.3;
            out.push(rv.process(x, x).0);
        }
        // Skip the build-up; measure the steady state.
        rms(&out[secs(1.5)..])
    };
    let dry = level(0.0);
    assert!(dry > 0.1, "the dry reference is wrong: {dry}");
    for wet in [0.25f32, 0.5, 0.75, 1.0] {
        let got = level(wet);
        assert!(
            (got / dry - 1.0).abs() < 0.15,
            "reverb at {wet:.2} renders at {:.2}x the dry level ({got:.4} vs {dry:.4}) — the knob is a volume control",
            got / dry
        );
    }
}

#[test]
fn a_fully_wet_track_does_not_slam_the_master_clamp() {
    // A sustained note with the track reverb wide open must stay in the same ballpark as the
    // dry take, rather than being pushed into the limiter at `Master::process`.
    let bus = |send: f64| {
        let mut m = model("sine", 8 * B);
        m.tracks[0].reverb_send = send;
        set_notes(&mut m, 0, vec![NoteEvent::new(0, 6 * B, 69)]);
        let (l, _) = render(&m, 1.0);
        // 6 beats = 3 s; measure once the reverb has fully built up.
        (
            peak(&l[secs(1.8)..secs(2.8)]),
            rms(&l[secs(1.8)..secs(2.8)]),
        )
    };
    let (dry_peak, dry_rms) = bus(0.0);
    for send in [0.25f64, 0.5, 1.0] {
        let (p, r) = bus(send);
        assert!(
            p < dry_peak * 1.25,
            "reverb_send {send} pushed the bus peak to {p} against a dry {dry_peak}"
        );
        assert!(
            r > dry_rms * 0.6 && r < dry_rms * 1.4,
            "reverb_send {send} moved the bus level to {r} against a dry {dry_rms}"
        );
    }
}

// ─── 3. mute / solo ──────────────────────────────────────────────────────────────────────────

/// Two sine tracks on unrelated pitches so each one's presence is a single Goertzel bin.
const A_PITCH: i32 = 69; // 440.0 Hz
const B_PITCH: i32 = 74; // 587.3 Hz

fn two_track_model() -> ProjectModel {
    let mut m = model("sine", 2 * B);
    let mut b = dissonant_core::Track::new("b");
    b.voice = "sine".into();
    m.tracks.push(b);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, B, A_PITCH)]);
    set_notes(&mut m, 1, vec![NoteEvent::new(0, B, B_PITCH)]);
    m
}

fn track_energies(m: &ProjectModel) -> (f64, f64) {
    let (l, _) = render(m, 0.5);
    let w = l[secs(0.10)..secs(0.40)].to_vec();
    (
        tone_amplitude(&w, midi_hz(A_PITCH)),
        tone_amplitude(&w, midi_hz(B_PITCH)),
    )
}

#[test]
fn mute_silences_only_that_track() {
    let m = two_track_model();
    let (a0, b0) = track_energies(&m);
    assert!(
        a0 > 0.01 && b0 > 0.01,
        "baseline should have both tracks: {a0} / {b0}"
    );

    let mut muted = two_track_model();
    muted.tracks[0].muted = true;
    let (a1, b1) = track_energies(&muted);
    assert!(
        a1 < a0 * 0.01,
        "muted track A still sounds: {a1} (was {a0})"
    );
    assert!(
        (b1 - b0).abs() < b0 * 0.01,
        "muting A changed B: {b1} (was {b0})"
    );
}

#[test]
fn solo_silences_every_non_soloed_track() {
    let base = track_energies(&two_track_model());
    let mut soloed = two_track_model();
    soloed.tracks[1].soloed = true;
    let (a, b) = track_energies(&soloed);
    assert!(
        a < base.0 * 0.01,
        "track A still sounds while B is soloed: {a}"
    );
    assert!(
        (b - base.1).abs() < base.1 * 0.01,
        "soloed track B changed level: {b}"
    );
}

#[test]
fn solo_beats_mute_exactly_as_sequence_track_audible_says() {
    let mut m = two_track_model();
    m.tracks[0].muted = true;
    m.tracks[0].soloed = true;
    let seq = sequence(&m);
    assert!(seq.any_solo());
    // The contract: with any solo active, `audible` ignores `muted`.
    assert!(
        seq.tracks[0].audible(true),
        "contract changed: solo should beat mute"
    );
    assert!(!seq.tracks[1].audible(true));

    let base = track_energies(&two_track_model());
    let (a, b) = track_energies(&m);
    assert!(
        a > base.0 * 0.5,
        "track is soloed *and* muted — SequenceTrack::audible says it sounds, engine gave {a}"
    );
    assert!(b < base.1 * 0.01, "non-soloed track B still sounds: {b}");
}

// ─── 4. note lifecycle ───────────────────────────────────────────────────────────────────────

/// Index of the last sample above `thresh`, i.e. where the track actually stops making sound.
fn last_sounding(x: &[f32], thresh: f32) -> usize {
    x.iter().rposition(|s| s.abs() > thresh).unwrap_or(0)
}

#[test]
fn notes_stop_at_their_end_tick() {
    let mut m = model("sine", 4 * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, B, 69)]);
    let (l, _) = render(&m, 0.5);
    // 1 beat @120 bpm = 0.5 s, "sine" release = 0.2 s → silence from ~0.70 s onwards.
    assert!(rms(&l[secs(0.10)..secs(0.40)]) > 1e-3, "note never sounded");
    assert!(
        rms(&l[secs(0.85)..secs(2.0)]) < 1e-6,
        "voice is stuck past the note's end tick (rms {})",
        rms(&l[secs(0.85)..secs(2.0)])
    );
}

#[test]
fn length_ticks_is_honored_not_a_fixed_duration() {
    // `sine`'s release is a linear ramp that takes `release` seconds from full scale, so the
    // tail after note-off is at most 0.2 s and shrinks with the level the note was at. What
    // must hold is that the note sustains right up to its end tick and stops within one
    // release afterwards — and that doubling the length doubles the sound.
    let probe = |len: i64| {
        let mut m = model("sine", 8 * B);
        set_notes(&mut m, 0, vec![NoteEvent::new(0, len, 69)]);
        let (l, _) = render(&m, 0.5);
        let end = len as f64 / B as f64 * 0.5; // seconds, @120 bpm
        let before_end = rms(&l[secs(end - 0.05)..secs(end)]);
        (last_sounding(&l, 1e-4) as f64 / SRD, before_end)
    };
    let (short, short_sustain) = probe(B / 4); // ends at 0.125 s
    let (long, long_sustain) = probe(3 * B); // ends at 1.500 s

    assert!(
        short_sustain > 1e-3 && long_sustain > 1e-3,
        "note died before its end tick"
    );
    assert!(
        short > 0.125 && short < 0.125 + 0.21,
        "a 0.25-beat note sounded for {short:.3} s; expected its 0.125 s plus at most one 0.2 s release"
    );
    assert!(
        long > 1.5 && long < 1.5 + 0.21,
        "a 3-beat note sounded for {long:.3} s; expected its 1.5 s plus at most one 0.2 s release"
    );
    assert!(
        ((long - short) - 1.375).abs() < 0.05,
        "the extra 2.75 beats added {:.3} s of sound, not 1.375 s — length_ticks is not driving the duration",
        long - short
    );
}

#[test]
fn a_note_shorter_than_one_block_still_sounds() {
    // 38 ticks (0.04 beats) @120 bpm = 873 samples; both the note-on and the note-off land
    // inside one 4096-frame block, so the engine must still render the sub-block between them.
    let mut m = model("sine", 4 * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, 38, 69)]);
    let (mut e, _) = engine_with(&m);
    e.handle(EngineCommand::SetLooping(false));
    e.handle(EngineCommand::Play);
    let (l, _) = run(&mut e, 4096 * 4, 4096);
    assert!(
        peak(&l) > 0.02,
        "a sub-block note was swallowed entirely (peak {})",
        peak(&l)
    );
    assert!(
        last_sounding(&l, 1e-4) as f64 / SRD > 0.02,
        "the sub-block note barely existed"
    );
}

#[test]
fn repeated_same_pitch_notes_retrigger() {
    let mut m = model("sine", 4 * B);
    set_notes(
        &mut m,
        0,
        vec![NoteEvent::new(0, B, 69), NoteEvent::new(B, B, 69)],
    );
    let (l, _) = render(&m, 0.5);
    // Second note runs 0.5 s → 1.0 s. If it never retriggered, the first note's 0.2 s release
    // would have ended by 0.7 s.
    assert!(
        rms(&l[secs(0.80)..secs(0.95)]) > 1e-3,
        "the second note never fired"
    );
    assert!(
        rms(&l[secs(1.30)..secs(2.0)]) < 1e-6,
        "the retriggered note is stuck past its end tick"
    );
}

#[test]
fn overlapping_same_pitch_notes_do_not_cut_each_other_off() {
    // note A: beats 0–2 (0.0–1.0 s), note B: beats 1–3 (0.5–1.5 s), same pitch.
    // A's note-off at 1.0 s must not take B with it; B has to keep sounding to 1.5 s.
    let mut m = model("sine", 6 * B);
    set_notes(
        &mut m,
        0,
        vec![NoteEvent::new(0, 2 * B, 69), NoteEvent::new(B, 2 * B, 69)],
    );
    let (l, _) = render(&m, 0.5);
    let during_b = rms(&l[secs(1.15)..secs(1.45)]);
    assert!(
        during_b > 1e-3,
        "the overlapping note died with the first note-off (rms {during_b} between 1.15 s and 1.45 s)"
    );
    assert!(
        rms(&l[secs(1.85)..secs(2.6)]) < 1e-6,
        "the overlapping pair left a stuck voice"
    );
}

// ─── 5. loop boundaries ──────────────────────────────────────────────────────────────────────

/// Onsets are measured at the engine's output, which the master limiter's 1.5 ms lookahead
/// (≈66 samples at 44.1 k) shifts as a whole. That is latency, not a timing error, so the
/// tolerance below has to clear it.
const ONSET_SLACK: i64 = 128;

fn onsets(x: &[f32], thresh: f32) -> Vec<usize> {
    let mut out = Vec::new();
    let mut armed = true;
    let mut quiet = 0usize;
    for (i, s) in x.iter().enumerate() {
        if s.abs() > thresh {
            if armed {
                out.push(i);
                armed = false;
            }
            quiet = 0;
        } else {
            quiet += 1;
            if quiet > 2_000 {
                armed = true;
            }
        }
    }
    out
}

#[test]
fn looping_retriggers_at_the_loop_point() {
    // 2-beat loop = 1.0 s @120 bpm, one note at the top of each cycle.
    let mut m = model("sine", 2 * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, B / 2, 69)]);
    let (mut e, _) = engine_with(&m);
    e.handle(EngineCommand::Play);
    let (l, _) = run(&mut e, secs(3.05), 512);
    let o = onsets(&l, 1e-3);
    assert!(o.len() >= 3, "expected 3 loop retriggers, found {:?}", o);
    for (cycle, idx) in o.iter().take(3).enumerate() {
        let expected = cycle * 44_100;
        assert!(
            (*idx as i64 - expected as i64).abs() < ONSET_SLACK,
            "cycle {cycle} onset at {idx}, expected ~{expected}"
        );
    }
}
/// The headline case, heard rather than inspected: a track's own 2-beat pattern, placed
/// across 8 beats of song, sounds four times, on the beat each turn starts. A track loops at
/// its pattern's length — there is no separate per-track loop.
#[test]
fn a_pattern_shorter_than_its_clip_sounds_on_every_turn() {
    let mut m = model("sine", 2 * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, B / 2, 69)]);
    m.song_length_ticks = 8 * B;
    m.clips = vec![Clip::new(m.patterns[0].id, 0, 8 * B)];

    let mut e = Engine::new(SR, Arc::new(Shared::default()), &m.master);
    e.handle(EngineCommand::SetSequence(Arc::new(Sequence::from_song(
        &m,
    ))));
    e.handle(EngineCommand::Play);
    // 8 beats @120 bpm = 4.0 s; stop short of the song's own loop point.
    let (l, _) = run(&mut e, secs(3.9), 512);
    let o = onsets(&l, 1e-3);
    assert_eq!(o.len(), 4, "expected one hit per 2-beat turn, found {o:?}");
    for (turn, idx) in o.iter().enumerate() {
        let expected = turn * 44_100; // one second per 2-beat turn
        assert!(
            (*idx as i64 - expected as i64).abs() < ONSET_SLACK,
            "turn {turn} sounded at {idx}, expected ~{expected}"
        );
    }
}

#[test]
fn a_note_crossing_the_loop_end_does_not_leak_into_the_next_cycle() {
    // 2-beat loop; the note runs beats 1.5–2.5, i.e. 0.5 beats past the loop end.
    let mut m = model("sine", 2 * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(3 * B / 2, B, 69)]);
    let (mut e, _) = engine_with(&m);
    e.handle(EngineCommand::Play);
    let (l, _) = run(&mut e, secs(3.0), 512);

    // Cycle 2 spans 1.0–2.0 s; the note restarts at 1.75 s. Between the loop seam's release
    // tail (<= 1.25 s) and 1.75 s the track must be silent — otherwise the voice leaked.
    let gap = rms(&l[secs(1.30)..secs(1.72)]);
    assert!(
        gap < 1e-6,
        "a voice leaked across the loop seam (rms {gap})"
    );
    assert!(
        rms(&l[secs(1.80)..secs(1.95)]) > 1e-3,
        "the crossing note did not restart in the next cycle"
    );
}

#[test]
fn disabling_looping_plays_the_sequence_once() {
    let mut m = model("sine", 2 * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, B / 2, 69)]);
    let (mut e, _) = engine_with(&m);
    e.handle(EngineCommand::SetLooping(false));
    e.handle(EngineCommand::Play);
    let (l, _) = run(&mut e, secs(3.0), 512);
    assert_eq!(
        onsets(&l, 1e-3).len(),
        1,
        "non-looping playback repeated itself"
    );
}

// ─── 6. voice stealing ───────────────────────────────────────────────────────────────────────

const POLYPHONY: usize = 12;

#[test]
fn exceeding_polyphony_steals_the_oldest_voice() {
    // 16 simultaneous notes on a 12-voice synth. Notes fire in list order, so the four
    // lowest pitches must be the ones stolen.
    let mut m = model("sine", 4 * B);
    m.tracks[0].volume = 0.15; // keep the sum clear of the master clamp so the spectrum is clean
    let notes: Vec<NoteEvent> = (60..76).map(|p| NoteEvent::new(0, 2 * B, p)).collect();
    set_notes(&mut m, 0, notes);
    let (l, r) = render(&m, 0.5);
    assert_finite(&l, "left");
    assert_finite(&r, "right");

    let w = l[secs(0.10)..secs(0.60)].to_vec();
    let amp = |p: i32| tone_amplitude(&w, midi_hz(p));
    for stolen in 60..64 {
        assert!(
            amp(stolen) < 1e-3,
            "pitch {stolen} should have been stolen but still rings at {}",
            amp(stolen)
        );
    }
    for kept in 64..76 {
        assert!(
            amp(kept) > 1e-3,
            "pitch {kept} is inside the last {POLYPHONY} notes but is silent ({})",
            amp(kept)
        );
    }
}

#[test]
fn voice_stealing_stays_in_range_and_free_of_nan() {
    let mut m = model("saw", 4 * B);
    m.tracks[0].volume = 1.5;
    m.tracks[0].reverb_send = 0.8;
    m.master.gain = 1.5;
    // 24 notes, staggered by a twentieth of a beat so steals happen mid-sustain rather than
    // all at sample 0.
    let notes: Vec<NoteEvent> = (0..24)
        .map(|i| NoteEvent::new(i as i64 * (B / 20), 2 * B, 48 + i))
        .collect();
    set_notes(&mut m, 0, notes);
    let (l, r) = render(&m, 1.0);
    assert_finite(&l, "left");
    assert_finite(&r, "right");
    assert!(
        peak(&l) > 0.5,
        "the overloaded render went quiet: {}",
        peak(&l)
    );
}

#[test]
fn stealing_a_sounding_voice_does_not_click() {
    // 12 sustained low notes (65–130 Hz, so the waveform's own slew per sample is tiny), then
    // a 13th a quarter-second in forces the oldest to be reassigned mid-sustain. Slamming the
    // stolen voice's oscillator and envelope back to zero shows up as a step discontinuity
    // many times the natural slew — that step is the click.
    let mut m = model("sine", 8 * B);
    let mut notes: Vec<NoteEvent> = (0..POLYPHONY)
        .map(|i| NoteEvent::new(0, 4 * B, 36 + i as i32).with_velocity(127))
        .collect();
    notes.push(NoteEvent::new(B / 2, 2 * B, 80).with_velocity(127)); // 0.25 s in
    set_notes(&mut m, 0, notes);
    let (l, _) = render(&m, 0.5);
    assert_finite(&l, "left");

    let steady = max_slew(&l[secs(0.60)..secs(0.90)]);
    let at_steal = max_slew(&l[secs(0.245)..secs(0.258)]);
    assert!(steady > 0.0);
    assert!(
        at_steal < steady * 1.6,
        "voice stealing produced a {at_steal} step against a {steady} steady-state slew ({:.1}x) — that is a click",
        at_steal / steady
    );
}

// ─── 7. master chain ─────────────────────────────────────────────────────────────────────────

fn master_render(pitch: i32, voice: &str, tweak: impl Fn(&mut MasterSettings)) -> Vec<f32> {
    let mut m = model(voice, 2 * B);
    tweak(&mut m.master);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, B, pitch)]);
    let (l, _) = render(&m, 0.5);
    l[secs(0.10)..secs(0.40)].to_vec()
}

#[test]
fn master_gain_scales_the_output() {
    let full = peak(&master_render(69, "sine", |_| {}));
    let half = peak(&master_render(69, "sine", |s| s.gain = 0.5));
    assert!(full > 0.05, "unity master gain is inaudible: {full}");
    assert!(
        ((half / full) - 0.5).abs() < 0.02,
        "master gain 0.5 gave {half} against {full}"
    );
}

#[test]
fn master_low_cut_removes_low_frequencies() {
    let f = midi_hz(36); // 65.4 Hz
    let open = tone_amplitude(&master_render(36, "sine", |_| {}), f);
    let cut = tone_amplitude(&master_render(36, "sine", |s| s.low_cut_hz = 300.0), f);
    assert!(open > 1e-3, "the 65 Hz test tone never rendered: {open}");
    assert!(
        cut / open < 0.2,
        "low cut at 300 Hz left {:.0}% of a 65 Hz tone",
        100.0 * cut / open
    );
}

#[test]
fn master_high_cut_removes_high_frequencies() {
    let open = master_render(57, "saw", |_| {});
    let cut = master_render(57, "saw", |s| s.high_cut_hz = 600.0);
    let (f0, h8) = (220.0, 1760.0);
    assert!(tone_amplitude(&open, h8) > 1e-4, "no high harmonics to cut");
    assert!(
        tone_amplitude(&cut, f0) / tone_amplitude(&open, f0) > 0.7,
        "high cut at 600 Hz also ate the 220 Hz fundamental"
    );
    assert!(
        tone_amplitude(&cut, h8) / tone_amplitude(&open, h8) < 0.25,
        "high cut at 600 Hz left {:.0}% of the 1760 Hz energy",
        100.0 * tone_amplitude(&cut, h8) / tone_amplitude(&open, h8)
    );
}

/// A probe pitch, the master tweak that should move it, and the band's name.
type EqCase = (i32, fn(&mut MasterSettings), &'static str);

#[test]
fn master_eq_bands_each_move_their_own_band() {
    // low shelf @120 Hz, peaking @1 kHz, high shelf @6 kHz. Probe each with a tone that sits
    // squarely inside that band, and check the other bands leave it alone.
    let cases: [EqCase; 3] = [
        (31, |s| s.low_eq = 2.0, "low"),    //  49 Hz
        (83, |s| s.mid_eq = 2.0, "mid"),    // 988 Hz
        (127, |s| s.high_eq = 2.0, "high"), // 12.5 kHz
    ];
    for (pitch, boost, name) in cases {
        let f = midi_hz(pitch);
        let flat = tone_amplitude(&master_render(pitch, "sine", |_| {}), f);
        let boosted = tone_amplitude(&master_render(pitch, "sine", boost), f);
        assert!(
            flat > 1e-4,
            "{name} probe tone ({f:.0} Hz) did not render: {flat}"
        );
        assert!(
            boosted / flat > 1.5,
            "{name} EQ at 2.0 only moved {f:.0} Hz by {:.2}x",
            boosted / flat
        );
    }
}

#[test]
fn master_clamp_never_emits_nan() {
    let mut m = model("saw", 2 * B);
    m.tracks[0].volume = 1.5;
    m.tracks[0].reverb_send = 1.0;
    m.master = MasterSettings {
        gain: 1.5,
        reverb_wet: 1.0,
        low_cut_hz: 5.0,
        high_cut_hz: 21_000.0,
        low_eq: 0.0,
        mid_eq: 4.0,
        high_eq: 4.0,
        ..MasterSettings::default()
    };
    let notes: Vec<NoteEvent> = (0..16)
        .map(|i| NoteEvent::new(0, 2 * B, 40 + i * 4))
        .collect();
    set_notes(&mut m, 0, notes);
    let (l, r) = render(&m, 1.0);
    assert_finite(&l, "left");
    assert_finite(&r, "right");
}

/// A mix loud enough to ask for far more than full scale: sixteen saws, every gain stage open.
fn overdriven_model() -> ProjectModel {
    let mut m = model("saw", 2 * B);
    m.tracks[0].volume = 1.5;
    m.master.gain = 1.5;
    let notes: Vec<NoteEvent> = (0..16)
        .map(|i| NoteEvent::new(0, 2 * B, 40 + i * 3).with_velocity(127))
        .collect();
    set_notes(&mut m, 0, notes);
    m
}

fn ceiling_of(m: &ProjectModel) -> f32 {
    10f32.powf(m.master.ceiling_db as f32 / 20.0)
}

/// Run `blocks` blocks of 256 frames, returning the audio and the highest master meter reading
/// seen along the way. The meter is a decaying hold, so reading it once at the end of a long
/// run measures the decay, not the music.
fn run_watching_the_meter(
    e: &mut Engine,
    shared: &Shared,
    frames: usize,
) -> (Vec<f32>, Vec<f32>, f32) {
    let mut out = Vec::with_capacity(frames * 2);
    let mut buf = vec![0.0f32; 256 * 2];
    let mut meter = 0.0f32;
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(256);
        e.process(&mut buf[..n * 2]);
        out.extend_from_slice(&buf[..n * 2]);
        let (ml, mr) = shared.master_peak();
        meter = meter.max(ml).max(mr);
        done += n;
    }
    let (l, r) = split(&out);
    (l, r, meter)
}

#[test]
fn the_limiter_holds_the_ceiling_while_the_meter_reports_the_true_level() {
    let m = overdriven_model();
    let ceiling = ceiling_of(&m);
    let (mut e, shared) = engine_with(&m);
    e.handle(EngineCommand::SetLooping(false));
    e.handle(EngineCommand::Play);
    let (l, r, meter) = run_watching_the_meter(&mut e, &shared, secs(1.2));

    let out = peak(&l).max(peak(&r));
    assert!(out > 0.1, "the overdriven render went quiet: {out}");
    assert!(
        out <= ceiling + 1e-5,
        "the limiter let {out} through a {ceiling} ceiling"
    );

    // The meter is read before the limiter, so it reports the mix as it really is — a bar
    // pinned at the ceiling would say the mix is fine while it is dB over.
    assert!(
        meter > out * 1.05,
        "the meter reported {meter} against a {out} output: that is the limited signal, not the mix"
    );
    assert!(shared.master_over(), "an over this loud was not latched");
}

#[test]
fn the_clip_latch_survives_the_silence_after_it_and_only_clear_clip_lowers_it() {
    let m = overdriven_model();
    let (mut e, shared) = engine_with(&m);
    e.handle(EngineCommand::SetLooping(false));
    e.handle(EngineCommand::Play);
    run(&mut e, secs(0.3), 256);
    assert!(shared.master_over());

    e.handle(EngineCommand::Stop);
    let (l, _) = run(&mut e, secs(2.0), 256);
    assert!(
        peak(&l[secs(1.0)..]) < 1e-4,
        "the transport did not go quiet"
    );
    assert!(
        shared.master_over(),
        "the clip latch cleared itself once the sound stopped"
    );

    e.handle(EngineCommand::ClearClip);
    run(&mut e, secs(0.1), 256);
    assert!(!shared.master_over(), "ClearClip did not lower the latch");
}

/// The master chain up to the limiter is linear, so a signal driven four times harder must come
/// out exactly four times harder — clamped. That is the old behaviour, to the last bit.
#[test]
fn limiter_off_reproduces_the_old_hard_clamp() {
    let settings = MasterSettings {
        gain: 1.2,
        reverb_wet: 0.3,
        limiter: false,
        // A ceiling the clamp must ignore: with the limiter off, ±1.0 is the only bound.
        ceiling_db: -6.0,
        ..MasterSettings::default()
    };
    let source: Vec<f32> = (0..4096)
        .map(|i| (i as f32 * 0.07).sin() * 0.2 + (i as f32 * 0.31).sin() * 0.05)
        .collect();

    let mut quiet = Master::new(SR, &settings);
    let (mut ql, mut qr) = (source.clone(), source.clone());
    quiet.process(&mut ql, &mut qr);

    let mut loud = Master::new(SR, &settings);
    let (mut ll, mut lr) = (
        source.iter().map(|x| x * 4.0).collect::<Vec<_>>(),
        source.iter().map(|x| x * 4.0).collect::<Vec<_>>(),
    );
    loud.process(&mut ll, &mut lr);

    assert!(
        ql.iter().all(|x| x.abs() < 1.0),
        "the quiet reference already clipped, so it cannot stand in for the chain"
    );
    assert!(
        ll.iter().any(|x| (x.abs() - 1.0).abs() < 1e-6),
        "the loud pass never reached the clamp: peak {}",
        peak(&ll)
    );
    for (i, (got, dry)) in ll.iter().zip(&ql).enumerate() {
        let want = (dry * 4.0).clamp(-1.0, 1.0);
        assert!(
            (got - want).abs() < 1e-4,
            "sample {i}: limiter-off gave {got}, the old clamp gives {want}"
        );
    }
}

#[test]
fn the_offline_render_is_limited_too() {
    let m = overdriven_model();
    let ceiling = ceiling_of(&m);
    let (l, r) = render(&m, 0.5);
    let limited = peak(&l).max(peak(&r));
    assert!(limited > 0.1, "the export went quiet: {limited}");
    assert!(
        limited <= ceiling + 1e-5,
        "the export peaked at {limited} against a {ceiling} ceiling"
    );

    let mut raw = m.clone();
    raw.master.limiter = false;
    let (rl, rr) = render(&raw, 0.5);
    let clamped = peak(&rl).max(peak(&rr));
    assert!(
        clamped > ceiling + 1e-3,
        "the opt-out render peaked at {clamped}: nothing was actually over the ceiling to limit"
    );
}

// ─── 8. meters ───────────────────────────────────────────────────────────────────────────────

/// Step the engine one block at a time, checking the published meter against the audio that
/// block actually produced. `Shared`'s meters are decaying peak-holds, so after every block
/// the reading must be at least that block's peak and never above the running maximum.
///
/// The limiter is off here only so the two are sample-aligned: it is metered on its input and
/// delays its output by the lookahead, which would put the meter a block ahead of the audio.
#[test]
fn master_peak_tracks_each_block_then_falls_back_to_zero() {
    let mut m = model("sine", 8 * B);
    m.master.limiter = false;
    set_notes(&mut m, 0, vec![NoteEvent::new(0, B / 2, 69)]);
    let (mut e, shared) = engine_with(&m);
    e.handle(EngineCommand::SetLooping(false));
    e.handle(EngineCommand::Play);

    let mut buf = vec![0.0f32; 256 * 2];
    let mut running = 0.0f32;
    let mut ever_lit = false;
    for _ in 0..(secs(0.2) / 256) {
        e.process(&mut buf);
        let (left, _) = split(&buf);
        let block = peak(&left);
        running = running.max(block);
        let (pl, _) = shared.master_peak();
        assert!(
            pl >= block - 1e-6,
            "master meter reads {pl} but the block peaked at {block}"
        );
        assert!(
            pl <= running + 1e-6,
            "master meter reads {pl}, above the running max {running}"
        );
        ever_lit |= pl > 0.0;
    }
    assert!(
        ever_lit && running > 0.05,
        "nothing was rendered to meter: {running}"
    );

    run(&mut e, secs(2.0), 256);
    assert_eq!(
        shared.master_peak(),
        (0.0, 0.0),
        "master meter never fell back after silence"
    );
}

#[test]
fn track_peak_falls_back_to_zero_after_the_track_goes_silent() {
    for block in [256usize, 4096] {
        for voice in ["sine", "pluck"] {
            let mut m = model(voice, 16 * B);
            set_notes(&mut m, 0, vec![NoteEvent::new(0, B / 2, 69)]);
            let (mut e, shared) = engine_with(&m);
            e.handle(EngineCommand::SetLooping(false));
            e.handle(EngineCommand::Play);

            run(&mut e, secs(0.2), block);
            assert!(
                shared.track_peak(0) > 0.0,
                "{voice}/{block}: track meter is dead while the track is sounding"
            );

            // 0.25 s note + release: the track is silent long before this run ends. The
            // peak-hold decays per *block*, so a big buffer needs more wall-clock time.
            run(&mut e, secs(8.0), block);
            assert_eq!(
                shared.track_peak(0),
                0.0,
                "{voice}/{block}: track meter froze instead of returning to zero"
            );
        }
    }
}

#[test]
fn a_drum_track_meter_also_returns_to_zero() {
    let mut m = model("sine", 16 * B);
    m.tracks[0].is_drum = true;
    set_notes(&mut m, 0, vec![NoteEvent::new(0, B / 4, 36)]);
    let (mut e, shared) = engine_with(&m);
    e.handle(EngineCommand::SetLooping(false));
    e.handle(EngineCommand::Play);
    run(&mut e, secs(0.2), 4096);
    assert!(
        shared.track_peak(0) > 0.0,
        "drum meter is dead while the kick sounds"
    );
    run(&mut e, secs(8.0), 4096);
    assert_eq!(
        shared.track_peak(0),
        0.0,
        "drum meter froze after the one-shot ended"
    );
}

#[test]
fn a_silent_project_leaves_every_meter_at_zero() {
    let m = model("sine", 4 * B);
    let (mut e, shared) = engine_with(&m);
    e.handle(EngineCommand::Play);
    let (l, r) = run(&mut e, secs(1.0), 256);
    assert_eq!(peak(&l), 0.0);
    assert_eq!(peak(&r), 0.0);
    assert_eq!(shared.master_peak(), (0.0, 0.0));
    assert_eq!(shared.track_peak(0), 0.0);
}

// ─── resuming a note the playhead landed inside ──────────────────────────────────────────────

/// Clicking the ruler in the middle of a long note must play that note, not silence until the
/// next one starts. Scheduling alone only fires a note at its start tick.
#[test]
fn seeking_into_a_sustained_note_sounds_it() {
    let mut m = model("sine", 8 * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, 6 * B, 69)]);
    let (mut e, _shared) = engine_with(&m);
    e.handle(EngineCommand::SetLooping(false));
    e.handle(EngineCommand::Seek {
        tick: (3 * B) as f64,
    });
    e.handle(EngineCommand::Play);

    let (left, _) = run(&mut e, secs(0.5), 512);
    assert!(
        peak(&left) > 0.01,
        "seeking into the note gave silence: peak {}",
        peak(&left)
    );

    // It is the note under the playhead, at its own pitch (A4 = 440 Hz), not a retrigger of the
    // pattern from the top.
    assert!(
        tone_amplitude(&left, 440.0) > 0.01,
        "the resumed note is not sounding its pitch"
    );

    // And it still ends where it should rather than hanging: 3 beats left at 120 bpm = 1.5 s,
    // plus the voice's release.
    let (tail, _) = run(&mut e, secs(2.5), 512);
    let end = tail.len() - secs(0.3);
    assert!(peak(&tail[end..]) < 1e-3, "the resumed note never stopped");
}

/// Starting playback (not just seeking) from inside a note must sound it too — stop, scrub,
/// play is the ordinary way to audition the middle of a pattern.
#[test]
fn playing_from_inside_a_note_sounds_it() {
    let mut m = model("sine", 8 * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(2 * B, 4 * B, 69)]);
    let (mut e, _shared) = engine_with(&m);
    e.handle(EngineCommand::Seek {
        tick: (4 * B) as f64,
    });
    let (before, _) = run(&mut e, secs(0.2), 512);
    assert_eq!(peak(&before), 0.0, "a stopped engine must stay silent");

    e.handle(EngineCommand::Play);
    let (left, _) = run(&mut e, secs(0.2), 512);
    assert!(
        peak(&left) > 0.01,
        "playback from inside a note gave silence"
    );
}

/// A drum hit is a one-shot; resuming must not retrigger it from the middle.
#[test]
fn seeking_does_not_retrigger_a_drum_hit() {
    let mut m = ProjectModel::empty();
    m.patterns[0].length_ticks = 8 * B;
    m.tracks[0].is_drum = true;
    set_notes(&mut m, 0, vec![NoteEvent::new(0, 4 * B, 36)]);
    let (mut e, _shared) = engine_with(&m);
    e.handle(EngineCommand::Seek {
        tick: (2 * B) as f64,
    });
    e.handle(EngineCommand::Play);
    let (left, _) = run(&mut e, secs(0.4), 512);
    assert!(
        peak(&left) < 1e-3,
        "seeking past a drum hit replayed it: peak {}",
        peak(&left)
    );
}

// ─── muted tracks still release their meter ──────────────────────────────────────────────────

/// Muting a sounding track silences it, but its meter must fall instead of freezing lit at the
/// level it had when the mute landed.
#[test]
fn muting_a_sounding_track_lets_its_meter_fall() {
    let mut m = model("sine", 16 * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, 12 * B, 69)]);
    let (mut e, shared) = engine_with(&m);
    e.handle(EngineCommand::Play);
    run(&mut e, secs(0.3), 512);
    assert!(shared.track_peak(0) > 0.01, "the track never lit its meter");

    m.tracks[0].muted = true;
    e.handle(EngineCommand::SetSequence(sequence(&m)));
    let (left, _) = run(&mut e, secs(2.0), 512);
    // The voice releases rather than cutting dead, so judge the steady state, not the tail.
    let settled = secs(0.5);
    assert!(
        peak(&left[settled..]) < 1e-3,
        "a muted track was still audible"
    );
    assert_eq!(
        shared.track_peak(0),
        0.0,
        "the muted track's meter froze lit"
    );
}

// ─── meter ballistics are a time constant, not a per-call factor ─────────────────────────────

/// Identical audio must produce the same meter fall whatever buffer size the device hands us:
/// the decay is per unit time, so it cannot depend on how the block happens to be chopped up.
#[test]
fn meter_release_does_not_depend_on_the_buffer_size() {
    let mut lit = Vec::new();
    for block in [128usize, 512, 4096] {
        let mut m = model("sine", 16 * B);
        set_notes(&mut m, 0, vec![NoteEvent::new(0, B / 2, 69)]);
        let (mut e, shared) = engine_with(&m);
        e.handle(EngineCommand::SetLooping(false));
        e.handle(EngineCommand::Play);

        let mut elapsed = 0usize;
        while elapsed < secs(6.0) && (elapsed < secs(0.5) || shared.track_peak(0) > 0.0) {
            run(&mut e, block, block);
            elapsed += block;
        }
        lit.push(elapsed as f64 / SRD);
    }
    let (min, max) = (
        lit.iter().cloned().fold(f64::MAX, f64::min),
        lit.iter().cloned().fold(0.0, f64::max),
    );
    assert!(
        max - min < 0.15,
        "meter release swings with the buffer size: {lit:?} seconds for 128/512/4096 frames"
    );
}

// ─── automation ──────────────────────────────────────────────────────────────────────────────

/// A sustained low note, long enough to measure an envelope across.
fn held_note_model(beats: i64) -> ProjectModel {
    let mut m = model("sine", beats * B);
    set_notes(&mut m, 0, vec![NoteEvent::new(0, beats * B, 48)]);
    m
}

/// Put one lane on the model, replacing any lane already on that target.
fn automate(m: &mut ProjectModel, target: AutoTarget, points: Vec<Breakpoint>) {
    m.automation.retain(|l| l.target != target);
    let mut lane = AutomationLane::new(target);
    lane.points = points;
    lane.tidy();
    m.automation.push(lane);
}

/// Level of the window `[from, to)` seconds of a live (not offline) engine pass.
fn window_rms(x: &[f32], from: f64, to: f64) -> f64 {
    rms(&x[secs(from).min(x.len())..secs(to).min(x.len())])
}

/// Drawing a fade on a track's volume lane has to be heard: the same sequence rendered with and
/// without the lane must start at the same level and end far quieter with it.
#[test]
fn a_volume_lane_fades_the_track_through_the_engine() {
    let plain = held_note_model(8);
    let mut faded = plain.clone();
    let track = faded.tracks[0].id;
    automate(
        &mut faded,
        AutoTarget::Track(track, TrackParam::Volume),
        vec![
            Breakpoint::new(0, 1.0, Curve::Step),
            Breakpoint::new(8 * B, 0.0, Curve::Ramp),
        ],
    );

    let pass = |m: &ProjectModel| {
        let (mut e, _) = engine_with(m);
        e.handle(EngineCommand::SetLooping(false));
        e.handle(EngineCommand::Play);
        run(&mut e, secs(4.0), 512).0
    };
    let (dry, wet) = (pass(&plain), pass(&faded));

    // 8 beats at 120 bpm is 4 seconds, so the lane spans the whole pass.
    let head = (window_rms(&dry, 0.1, 0.4), window_rms(&wet, 0.1, 0.4));
    let tail = (window_rms(&dry, 3.4, 3.9), window_rms(&wet, 3.4, 3.9));
    assert!(
        (head.1 - head.0).abs() < head.0 * 0.15,
        "the lane starts at unity, so the opening should match: dry {:.4} vs lane {:.4}",
        head.0,
        head.1
    );
    assert!(
        tail.1 < tail.0 * 0.2 && tail.0 > 1e-4,
        "the fade should have taken the tail down: dry {:.4} vs lane {:.4}",
        tail.0,
        tail.1
    );
    assert!(
        wet.iter().all(|s| s.is_finite()),
        "a fade must not produce non-finite samples"
    );
}

/// The two curve kinds are the lane's whole vocabulary: a ramp interpolates between its
/// neighbours, a step holds the previous value until its own tick and then jumps.
#[test]
fn a_ramp_interpolates_where_a_step_holds() {
    let level = |curve: Curve| {
        let mut m = held_note_model(8);
        let t = m.tracks[0].id;
        automate(
            &mut m,
            AutoTarget::Track(t, TrackParam::Volume),
            vec![
                Breakpoint::new(0, 1.0, Curve::Step),
                Breakpoint::new(8 * B, 0.1, curve),
            ],
        );
        let (mut e, _) = engine_with(&m);
        e.handle(EngineCommand::SetLooping(false));
        e.handle(EngineCommand::Play);
        let out = run(&mut e, secs(4.0), 512).0;
        // Halfway through the span: a ramp is at 0.55, a step is still at 1.0.
        window_rms(&out, 1.9, 2.1)
    };
    let (ramped, stepped) = (level(Curve::Ramp), level(Curve::Step));
    assert!(
        ramped < stepped * 0.75,
        "a ramp should already be halfway down where a step still holds: {ramped:.4} vs {stepped:.4}"
    );
}

/// A parameter that moves must move per sample, not per buffer. A steep fade stepped once per
/// block puts a jump of a tenth of full scale into the signal; ramped inside the block the
/// largest sample-to-sample step stays the one the note itself makes.
#[test]
fn a_moving_volume_lane_does_not_zipper_at_block_boundaries() {
    let mut m = held_note_model(8);
    let t = m.tracks[0].id;
    let plain = m.clone();
    // Down to silence inside a tenth of a second: at 512 frames that is ~9 blocks, so a
    // per-block step would be over a tenth of the gain range each time.
    automate(
        &mut m,
        AutoTarget::Track(t, TrackParam::Volume),
        vec![
            Breakpoint::new(0, 1.0, Curve::Step),
            Breakpoint::new(B / 20, 0.0, Curve::Ramp),
        ],
    );
    let pass = |m: &ProjectModel| {
        let (mut e, _) = engine_with(m);
        e.handle(EngineCommand::SetLooping(false));
        e.handle(EngineCommand::Play);
        run(&mut e, secs(0.5), 512).0
    };
    let (dry, faded) = (pass(&plain), pass(&m));
    let (dry_slew, fade_slew) = (max_slew(&dry), max_slew(&faded));
    assert!(
        fade_slew <= dry_slew * 1.05,
        "the fade adds steps of its own: {fade_slew:.5} against the note's own {dry_slew:.5}"
    );
}

/// The master gain lane rides the whole mix, after the tracks and the master inserts.
#[test]
fn a_master_gain_lane_rides_the_whole_mix() {
    let plain = held_note_model(8);
    let mut faded = plain.clone();
    automate(
        &mut faded,
        AutoTarget::Master(MasterParam::Gain),
        vec![
            Breakpoint::new(0, 1.0, Curve::Step),
            Breakpoint::new(8 * B, 0.0, Curve::Ramp),
        ],
    );
    let pass = |m: &ProjectModel| {
        let (mut e, _) = engine_with(m);
        e.handle(EngineCommand::SetLooping(false));
        e.handle(EngineCommand::Play);
        run(&mut e, secs(4.0), 512).0
    };
    let (dry, wet) = (pass(&plain), pass(&faded));
    assert!(
        window_rms(&wet, 3.4, 3.9) < window_rms(&dry, 3.4, 3.9) * 0.2,
        "the master lane should have faded the mix out"
    );
    assert!(
        (window_rms(&wet, 0.1, 0.4) - window_rms(&dry, 0.1, 0.4)).abs()
            < window_rms(&dry, 0.1, 0.4) * 0.15,
        "the master lane starts at unity, so the opening should match"
    );
}

/// Phase 1's promise is that the export is the mix. A lane that fades the track live has to
/// fade it in the offline bounce too, through the same `Sequence`.
#[test]
fn the_offline_bounce_honours_the_same_volume_lane() {
    let plain = held_note_model(8);
    let mut faded = plain.clone();
    let t = faded.tracks[0].id;
    automate(
        &mut faded,
        AutoTarget::Track(t, TrackParam::Volume),
        vec![
            Breakpoint::new(0, 1.0, Curve::Step),
            Breakpoint::new(8 * B, 0.0, Curve::Ramp),
        ],
    );
    let (dry, _) = render(&plain, 0.0);
    let (wet, _) = render(&faded, 0.0);
    assert!(
        window_rms(&wet, 3.4, 3.9) < window_rms(&dry, 3.4, 3.9) * 0.2,
        "the exported WAV did not fade: {:.4} against {:.4}",
        window_rms(&wet, 3.4, 3.9),
        window_rms(&dry, 3.4, 3.9)
    );
    assert!(
        (window_rms(&wet, 0.1, 0.4) - window_rms(&dry, 0.1, 0.4)).abs()
            < window_rms(&dry, 0.1, 0.4) * 0.15,
        "the export should open at the same level as the dry mix"
    );
}

/// Pan, tone and reverb send are automatable on the same footing as volume.
#[test]
fn pan_tone_and_send_lanes_all_reach_the_strip() {
    let base = held_note_model(4);
    let t = base.tracks[0].id;

    let mut panned = base.clone();
    automate(
        &mut panned,
        AutoTarget::Track(t, TrackParam::Pan),
        vec![Breakpoint::new(0, -1.0, Curve::Step)],
    );
    let (l, r) = render(&panned, 0.0);
    assert!(
        rms(&l) > rms(&r) * 4.0,
        "a pan lane pinned hard left should leave the right side near silent"
    );

    // A 220 Hz saw, like the static tone test: fundamental well under the cutoff, 8th harmonic
    // well over it, so a closed lane is a lowpass and not a volume change.
    let mut saw = model("saw", 2 * B);
    set_notes(&mut saw, 0, vec![NoteEvent::new(0, B, 57)]);
    let harmonic = |m: &ProjectModel| {
        let l = render(m, 0.5).0;
        tone_amplitude(&l[secs(0.10)..secs(0.40)], 1760.0)
    };
    let mut toned = saw.clone();
    let saw_track = saw.tracks[0].id;
    automate(
        &mut toned,
        AutoTarget::Track(saw_track, TrackParam::Tone),
        vec![Breakpoint::new(0, 300.0, Curve::Step)],
    );
    assert!(
        harmonic(&toned) < harmonic(&saw) * 0.1,
        "a tone lane closed to 300 Hz left {:.1}% of the 1760 Hz energy",
        100.0 * harmonic(&toned) / harmonic(&saw)
    );

    let mut sent = base.clone();
    automate(
        &mut sent,
        AutoTarget::Track(t, TrackParam::ReverbSend),
        vec![Breakpoint::new(0, 1.0, Curve::Step)],
    );
    // The send's tail is what a reverb adds: measure after the note has stopped.
    let wet_tail = window_rms(&render(&sent, 2.0).0, 2.1, 2.9);
    let dry_tail = window_rms(&render(&base, 2.0).0, 2.1, 2.9);
    assert!(
        wet_tail > dry_tail * 2.0,
        "a send lane should leave a tail: {wet_tail:.5} against {dry_tail:.5}"
    );
}
