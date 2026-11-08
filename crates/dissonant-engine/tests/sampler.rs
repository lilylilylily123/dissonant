//! The sampler instrument: chops on keys, read from a source in the audio pool.
//!
//! Sample-level claims drive [`SamplerInstrument`] directly, where nothing sits between the
//! voice and the output; playback claims go through the real [`Engine`] (pattern mode, song
//! mode, auditions and the offline renderer), where the master chain's filters mean they are
//! measured with a Goertzel bin and an energy envelope rather than compared sample for sample.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::Arc;

use dissonant_core::{
    AudioSource, Chop, ChopMode, Clip, MasterSettings, NoteEvent, ProjectModel, Sampler,
    SamplerVoice, Sequence, SongPattern, Track, TICKS_PER_BEAT,
};
use dissonant_engine::audio::{AudioBuffer, AudioPool};
use dissonant_engine::engine::{Engine, EngineCommand, Shared};
use dissonant_engine::render::{render_to_buffer, RenderOptions, RenderPlugins};
use dissonant_engine::sampler::{SamplerInstrument, SAMPLER_POLYPHONY};
use uuid::Uuid;

const SR: f32 = 48_000.0;
const SRD: f64 = 48_000.0;
const B: i64 = TICKS_PER_BEAT;
const BASE: u8 = 36;
/// The cut / one-shot end fade, in samples at [`SR`].
const CUT: usize = 192;

// ─── helpers ─────────────────────────────────────────────────────────────────────────────────

/// Deterministic stereo noise with different left and right channels.
fn noise(seconds: f64, rate: u32) -> AudioBuffer {
    let frames = (seconds * rate as f64) as usize;
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        ((state >> 40) as f32 / 8_388_608.0 - 1.0) * 0.5
    };
    let data = (0..frames * 2).map(|_| next()).collect();
    AudioBuffer::new(rate, 2, data)
}

/// A mono source of `seconds` of sine per frequency, back to back.
fn tones(hz: &[f64], seconds: f64, rate: u32) -> AudioBuffer {
    let per = (seconds * rate as f64) as usize;
    let data = hz
        .iter()
        .flat_map(|&f| {
            (0..per).map(move |n| (0.5 * (2.0 * PI * f * n as f64 / rate as f64).sin()) as f32)
        })
        .collect();
    AudioBuffer::new(rate, 1, data)
}

fn pool_of(id: Uuid, buffer: AudioBuffer) -> Arc<AudioPool> {
    let map: HashMap<Uuid, Arc<AudioBuffer>> = [(id, Arc::new(buffer))].into_iter().collect();
    Arc::new(AudioPool::new(map))
}

fn chop(start: f64, end: f64) -> Chop {
    let mut c = Chop::new(start, end);
    c.release_seconds = 0.0;
    c
}

/// A bound instrument over `buffer` with `chops` from [`BASE`], mono like a new sampler.
fn instrument(buffer: AudioBuffer, chops: Vec<Chop>) -> (SamplerInstrument, AudioBuffer) {
    voiced(SamplerVoice::Mono, buffer, chops)
}

/// [`instrument`] with its voice mode chosen.
fn voiced(voice: SamplerVoice, buffer: AudioBuffer, chops: Vec<Chop>) -> (SamplerInstrument, AudioBuffer) {
    let id = Uuid::new_v4();
    let copy = buffer.clone();
    let mut sampler = Sampler::new(id, chops);
    sampler.voice = voice;
    let mut s = SamplerInstrument::new(&sampler, SR);
    s.bind(&pool_of(id, buffer));
    (s, copy)
}

/// Render `frames` in uneven blocks, so a voice has to carry its state across boundaries.
fn run(s: &mut SamplerInstrument, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut l, mut r) = (vec![0.0f32; frames], vec![0.0f32; frames]);
    let mut done = 0;
    for block in [37usize, 256, 1000, 64].iter().cycle() {
        if done >= frames {
            break;
        }
        let n = (*block).min(frames - done);
        s.render_add(&mut l[done..done + n], &mut r[done..done + n]);
        done += n;
    }
    (l, r)
}

fn src(buffer: &AudioBuffer, frame: usize) -> (f32, f32) {
    (buffer.data[frame * 2], buffer.data[frame * 2 + 1])
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Magnitude of `hz` in `x`, normalised by length.
fn goertzel(x: &[f32], hz: f64, sr: f64) -> f64 {
    let w = 2.0 * PI * hz / sr;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &v in x {
        let s0 = v as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2).max(0.0).sqrt() / x.len() as f64
}

/// A project at 120 bpm with one sampler track over `source`, whose own pattern (returned)
/// holds `notes` in a loop `length` ticks long. The default melody track stays silent.
fn sampler_project(
    source: &AudioSource,
    chops: Vec<Chop>,
    notes: Vec<NoteEvent>,
    length: i64,
) -> (ProjectModel, Uuid, Uuid) {
    let mut m = ProjectModel::empty();
    m.tempo = 120.0;
    m.tracks[0].muted = true;
    m.audio_sources.push(source.clone());
    let t = Track::sampler("sample", Sampler::new(source.id, chops));
    let tid = t.id;
    m.tracks.push(t);
    let mut p = SongPattern::new(tid, "sample 1");
    p.length_ticks = length;
    p.notes = notes;
    let pid = p.id;
    m.patterns.push(p);
    // Raw sample values are measured: no limiter pumping, nothing but the fixed filters.
    m.master = MasterSettings {
        limiter: false,
        ..MasterSettings::default()
    };
    (m, tid, pid)
}

fn source_for(buffer: &AudioBuffer) -> AudioSource {
    AudioSource {
        id: Uuid::new_v4(),
        name: "sample".into(),
        frames: buffer.frames as u64,
        sample_rate: buffer.sample_rate,
        channels: buffer.channels,
        origin_url: None,
    }
}

fn note(start: i64, length: i64, pitch: u8) -> NoteEvent {
    let mut n = NoteEvent::new(start, length, pitch as i32);
    n.velocity = 127;
    n
}

/// Offline render of one pattern through the real engine; the left channel.
fn render_pattern(m: &ProjectModel, pid: &Uuid, pool: Option<Arc<AudioPool>>) -> Vec<f32> {
    let opts = RenderOptions {
        sample_rate: SR as u32,
        tail_seconds: 0.5,
        master: m.master.clone(),
        audio_pool: pool,
        ..RenderOptions::default()
    };
    let seq = Arc::new(Sequence::from_pattern(m, pid, 0).expect("pattern"));
    let out = render_to_buffer(seq, &opts, RenderPlugins::default()).expect("render");
    out.iter().step_by(2).copied().collect()
}

fn drive(e: &mut Engine, frames: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames);
    let mut buf = vec![0.0f32; 512 * 2];
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(512);
        e.process(&mut buf[..n * 2]);
        out.extend(buf[..n * 2].iter().step_by(2));
        done += n;
    }
    out
}

fn secs(s: f64) -> usize {
    (s * SRD).round() as usize
}

// ─── the voice, sample for sample ────────────────────────────────────────────────────────────

#[test]
fn a_key_reproduces_its_chop_sample_for_sample() {
    let chops = vec![chop(0.0, 0.1), chop(0.1, 0.25), chop(0.25, 0.4)];
    let (mut s, buffer) = instrument(noise(0.5, 48_000), chops);
    s.note_on(BASE + 1, 127);
    let (l, r) = run(&mut s, secs(0.3));
    let (start, len) = (secs(0.1), secs(0.15));
    for k in 0..len - CUT {
        assert_eq!((l[k], r[k]), src(&buffer, start + k), "frame {k}");
    }
    // The 4 ms fade lands on the chop's end, and nothing follows it.
    let (last_l, _) = src(&buffer, start + len - 1);
    assert!(l[len - 1].abs() <= last_l.abs() / CUT as f32 + 1e-7);
    assert!(l[len..].iter().chain(&r[len..]).all(|x| *x == 0.0));
    assert!(s.is_silent());
}

#[test]
fn reverse_plays_the_chop_backwards() {
    let mut c = chop(0.1, 0.2);
    c.reverse = true;
    let (mut s, buffer) = instrument(noise(0.3, 48_000), vec![c]);
    s.note_on(BASE, 127);
    let (l, r) = run(&mut s, secs(0.15));
    let (end, len) = (secs(0.2), secs(0.1));
    for k in 0..len - CUT {
        assert_eq!((l[k], r[k]), src(&buffer, end - 1 - k), "frame {k}");
    }
    assert!(l[len..].iter().all(|x| *x == 0.0));
}

#[test]
fn velocity_and_gain_scale_the_chop_and_the_attack_is_linear() {
    let mut c = chop(0.0, 0.2);
    c.gain = 2.0;
    c.attack_seconds = 0.01;
    let (mut s, buffer) = instrument(noise(0.3, 48_000), vec![c]);
    s.note_on(BASE, 64);
    let (l, _) = run(&mut s, secs(0.1));
    let gain = 2.0 * (0.25 + 0.75 * 64.0 / 127.0);
    let attack = secs(0.01);
    for (k, x) in l.iter().enumerate() {
        let ramp = (k as f32 / attack as f32).min(1.0);
        let want = src(&buffer, k).0 * gain * ramp;
        assert!((x - want).abs() < 1e-6, "frame {k}: {x} vs {want}");
    }
}

#[test]
fn a_one_shot_plays_on_past_its_note_off() {
    let (mut s, buffer) = instrument(noise(0.3, 48_000), vec![chop(0.0, 0.2)]);
    s.note_on(BASE, 127);
    let (head, _) = run(&mut s, 1_000);
    s.note_off(BASE);
    let (tail, _) = run(&mut s, secs(0.25) - 1_000);
    let l: Vec<f32> = head.into_iter().chain(tail).collect();
    for (k, x) in l.iter().enumerate().take(secs(0.2) - CUT) {
        assert_eq!(*x, src(&buffer, k).0, "frame {k}");
    }
    assert!(l[secs(0.2)..].iter().all(|x| *x == 0.0));
}

#[test]
fn a_gate_stops_after_its_release() {
    let mut c = chop(0.0, 0.2);
    c.mode = ChopMode::Gate;
    c.release_seconds = 0.01;
    let (mut s, buffer) = instrument(noise(0.3, 48_000), vec![c]);
    s.note_on(BASE, 127);
    let (head, _) = run(&mut s, 1_000);
    // Held, a gate chop has no end fade: every sample is the source's.
    for (k, x) in head.iter().enumerate() {
        assert_eq!(*x, src(&buffer, k).0, "frame {k}");
    }
    s.note_off(BASE);
    let (tail, _) = run(&mut s, secs(0.1));
    let release = secs(0.01);
    assert!(peak(&tail[..release / 2]) > 0.05, "the release is a fade, not a cut");
    // Float steps may leave one last sample at a vanishing level before the voice stops.
    assert!(tail[release + 1..].iter().all(|x| *x == 0.0));
    assert!(s.is_silent());
}

#[test]
fn a_held_gate_releases_at_its_chop_end_reading_past_it() {
    let mut c = chop(0.0, 0.1);
    c.mode = ChopMode::Gate;
    c.release_seconds = 0.05;
    let (mut s, buffer) = instrument(noise(0.3, 48_000), vec![c]);
    s.note_on(BASE, 127);
    let (l, _) = run(&mut s, secs(0.3));
    let (end, release) = (secs(0.1), secs(0.05));
    // Past the chop's end the source carries on under a falling level.
    for k in [end, end + release / 2, end + release - 10] {
        let fade = 1.0 - (k - end) as f32 / release as f32;
        let want = src(&buffer, k).0 * fade;
        assert!((l[k] - want).abs() < 1e-4, "frame {k}: {} vs {want}", l[k]);
    }
    assert!(l[end + release + 1..].iter().all(|x| *x == 0.0));
}

#[test]
fn retriggering_and_choking_cut_the_old_voice_on_a_poly_sampler() {
    let mut open = chop(0.0, 0.2);
    open.choke = 1;
    let mut closed = chop(0.2, 0.4);
    closed.choke = 1;
    let free = chop(0.4, 0.6);
    let (mut s, buffer) = voiced(SamplerVoice::Poly, noise(0.7, 48_000), vec![open, closed, free]);

    // Choke: the second hit in group 1 leaves only itself sounding after the 4 ms cut.
    s.note_on(BASE, 127);
    run(&mut s, 1_000);
    s.note_on(BASE + 1, 127);
    let (l, _) = run(&mut s, 5_000);
    for (k, x) in l.iter().enumerate().skip(CUT + 1).take(4_000) {
        assert_eq!(*x, src(&buffer, secs(0.2) + k).0, "frame {k}");
    }

    // A chop outside the group does not choke it.
    s.note_on(BASE + 2, 127);
    let (l, _) = run(&mut s, 1_000);
    let both = src(&buffer, secs(0.2) + 5_000 + 500).0 + src(&buffer, secs(0.4) + 500).0;
    assert!((l[500] - both).abs() < 1e-6);

    // Retrigger: the same chop again starts over, the old voice gone after the cut.
    let (mut s, buffer) = voiced(SamplerVoice::Poly, noise(0.3, 48_000), vec![chop(0.0, 0.2)]);
    s.note_on(BASE, 127);
    run(&mut s, 3_000);
    s.note_on(BASE, 127);
    let (l, _) = run(&mut s, 2_000);
    for (k, x) in l.iter().enumerate().skip(CUT + 1) {
        assert_eq!(*x, src(&buffer, k).0, "frame {k}");
    }
}

#[test]
fn mono_cuts_a_different_chop_and_poly_layers_it() {
    let chops = || vec![chop(0.0, 0.2), chop(0.2, 0.4)];
    let (mut mono, buffer) = voiced(SamplerVoice::Mono, noise(0.5, 48_000), chops());
    mono.note_on(BASE, 127);
    run(&mut mono, 1_000);
    mono.note_on(BASE + 1, 127);
    let (l, _) = run(&mut mono, 5_000);
    for (k, x) in l.iter().enumerate().skip(CUT + 1).take(4_000) {
        assert_eq!(*x, src(&buffer, secs(0.2) + k).0, "frame {k}");
    }

    let (mut poly, buffer) = voiced(SamplerVoice::Poly, noise(0.5, 48_000), chops());
    poly.note_on(BASE, 127);
    run(&mut poly, 1_000);
    poly.note_on(BASE + 1, 127);
    let (l, _) = run(&mut poly, 1_000);
    let both = src(&buffer, 1_000 + 500).0 + src(&buffer, secs(0.2) + 500).0;
    assert!((l[500] - both).abs() < 1e-6);
}

#[test]
fn mono_ignores_choke_groups() {
    // Different groups, and no group at all: in mono every hit still cuts what is sounding.
    let mut a = chop(0.0, 0.2);
    a.choke = 1;
    let mut b = chop(0.2, 0.4);
    b.choke = 2;
    let c = chop(0.4, 0.6);
    let (mut s, buffer) = voiced(SamplerVoice::Mono, noise(0.7, 48_000), vec![a, b, c]);
    s.note_on(BASE, 127);
    run(&mut s, 1_000);
    s.note_on(BASE + 1, 127);
    run(&mut s, 1_000);
    s.note_on(BASE + 2, 127);
    let (l, _) = run(&mut s, 2_000);
    for (k, x) in l.iter().enumerate().skip(CUT + 1) {
        assert_eq!(*x, src(&buffer, secs(0.4) + k).0, "frame {k}");
    }
}

#[test]
fn the_oldest_voice_is_stolen_past_the_polyphony() {
    let n = SAMPLER_POLYPHONY + 1;
    let chops = (0..n)
        .map(|i| chop(i as f64 * 0.02, i as f64 * 0.02 + 0.5))
        .collect();
    let (mut s, buffer) = voiced(SamplerVoice::Poly, noise(1.2, 48_000), chops);
    for i in 0..n {
        s.note_on(BASE + i as u8, 127);
        run(&mut s, 10);
    }
    // Every voice but the first is still playing its chop, 10 samples further on per strike.
    let (l, _) = run(&mut s, 1);
    let want: f32 = (1..n)
        .map(|i| src(&buffer, secs(i as f64 * 0.02) + (n - i) * 10).0)
        .sum();
    assert!((l[0] - want).abs() < 1e-5, "{} vs {want}", l[0]);
}

#[test]
fn a_slower_source_plays_at_its_own_speed_on_a_faster_engine() {
    let (mut s, _) = instrument(tones(&[440.0], 1.0, 44_100), vec![chop(0.0, 0.5)]);
    s.note_on(BASE, 127);
    let (l, _) = run(&mut s, secs(0.6));
    let last = l.iter().rposition(|x| *x != 0.0).unwrap();
    assert!((last as i64 - secs(0.5) as i64).abs() <= 1, "ended at {last}");
    let body = &l[..secs(0.4)];
    assert!(goertzel(body, 440.0, SRD) > 10.0 * goertzel(body, 440.0 * 48_000.0 / 44_100.0, SRD));
}

#[test]
fn a_missing_source_or_an_unmapped_key_is_silent() {
    let (mut s, _) = instrument(noise(0.3, 48_000), vec![chop(0.0, 0.1), chop(0.1, 0.2)]);
    for pitch in [0, BASE - 1, BASE + 2, 127] {
        s.note_on(pitch, 127);
    }
    assert!(s.is_silent());
    assert!(peak(&run(&mut s, 1_000).0) == 0.0);

    // Bound, struck, then the pool loses the source: the voice goes with it.
    s.note_on(BASE, 127);
    assert!(!s.is_silent());
    s.bind(&AudioPool::default());
    assert!(s.is_silent());
    s.note_on(BASE, 127);
    assert!(peak(&run(&mut s, 1_000).0) == 0.0);
}

// ─── through the engine ──────────────────────────────────────────────────────────────────────

/// Three chops, each a different tone, so which one played can be heard through the master.
fn tone_project(notes: Vec<NoteEvent>) -> (ProjectModel, Uuid, Uuid, Arc<AudioPool>) {
    let buffer = tones(&[220.0, 660.0, 1_320.0], 0.4, 48_000);
    let source = source_for(&buffer);
    let chops = vec![chop(0.0, 0.4), chop(0.4, 0.8), chop(0.8, 1.2)];
    let (m, tid, pid) = sampler_project(&source, chops, notes, 4 * B);
    let pool = pool_of(source.id, buffer);
    (m, tid, pid, pool)
}

#[test]
fn a_pattern_note_plays_its_chop_in_the_render() {
    let (m, _, pid, pool) = tone_project(vec![note(0, B, BASE + 1)]);
    let l = render_pattern(&m, &pid, Some(pool));
    let body = &l[secs(0.05)..secs(0.35)];
    let want = goertzel(body, 660.0, SRD);
    assert!(want > 0.1, "chop 1 must sound: {want}");
    assert!(want > 20.0 * goertzel(body, 220.0, SRD));
    assert!(want > 20.0 * goertzel(body, 1_320.0, SRD));
    assert!(peak(&l[secs(0.45)..secs(1.9)]) < 1e-3, "a one-shot ends with its chop");
}

#[test]
fn one_shot_outlasts_its_note_and_gate_does_not() {
    let notes = vec![note(0, B / 5, BASE)];
    let (mut m, tid, pid, pool) = tone_project(notes);
    let one_shot = render_pattern(&m, &pid, Some(Arc::clone(&pool)));
    assert!(peak(&one_shot[secs(0.2)..secs(0.3)]) > 0.2, "the one-shot plays on");

    let t = m.tracks.iter_mut().find(|t| t.id == tid).unwrap();
    let sampler = t.sampler.as_mut().unwrap();
    sampler.chops[0].mode = ChopMode::Gate;
    sampler.chops[0].release_seconds = 0.01;
    let gate = render_pattern(&m, &pid, Some(pool));
    assert!(peak(&gate[secs(0.02)..secs(0.08)]) > 0.2, "the gate sounds while held");
    assert!(peak(&gate[secs(0.2)..secs(0.3)]) < 1e-3, "the gate stops after its release");
}

#[test]
fn a_sequenced_hit_on_a_mono_track_cuts_the_chop_before_it() {
    // Chop 0 (220 Hz) runs 0.4 s; chop 1 (660 Hz) lands at 0.25 s. The window holds whole
    // cycles of both tones, so neither leaks into the other's bin.
    let (mut m, tid, pid, pool) = tone_project(vec![note(0, B / 4, BASE), note(B / 2, B / 4, BASE + 1)]);
    let window = |l: &[f32]| l[secs(0.3)..secs(0.4)].to_vec();
    let late = |l: &[f32]| goertzel(&window(l), 220.0, SRD);
    let mono = render_pattern(&m, &pid, Some(Arc::clone(&pool)));
    assert!(late(&mono) < 1e-3, "mono cut chop 0: {}", late(&mono));
    assert!(goertzel(&window(&mono), 660.0, SRD) > 0.1);

    let t = m.tracks.iter_mut().find(|t| t.id == tid).unwrap();
    t.sampler.as_mut().unwrap().voice = SamplerVoice::Poly;
    let poly = render_pattern(&m, &pid, Some(pool));
    assert!(late(&poly) > 0.1, "poly layers chop 0 under chop 1");
}

#[test]
fn a_missing_source_renders_silence() {
    let (m, _, pid, _) = tone_project(vec![note(0, B, BASE)]);
    let l = render_pattern(&m, &pid, None);
    assert!(peak(&l) < 1e-6);
}

#[test]
fn song_mode_plays_sampler_clips_whichever_arrives_first() {
    let (mut m, _, pid, pool) = tone_project(vec![note(0, B, BASE + 2)]);
    m.clips = vec![Clip::new(pid, 2 * B, 4 * B)];
    // Sequence before pool: the channel exists unbound and the pool binds it.
    let mut e = Engine::new(SR, Arc::new(Shared::default()), &m.master);
    e.handle(EngineCommand::SetSequence(Arc::new(Sequence::from_song(&m))));
    e.handle(EngineCommand::SetAudioPool(pool));
    e.handle(EngineCommand::SetLooping(false));
    e.handle(EngineCommand::Play);
    let l = drive(&mut e, secs(1.5));
    assert!(peak(&l[..secs(0.95)]) < 1e-3, "nothing before the clip");
    let body = &l[secs(1.05)..secs(1.35)];
    assert!(goertzel(body, 1_320.0, SRD) > 0.1);
}

#[test]
fn auditions_and_live_keys_play_with_the_transport_stopped() {
    let (m, tid, pid, pool) = tone_project(vec![]);
    let mut e = Engine::new(SR, Arc::new(Shared::default()), &m.master);
    e.handle(EngineCommand::SetAudioPool(pool));
    e.handle(EngineCommand::SetSequence(Arc::new(
        Sequence::from_pattern(&m, &pid, 0).unwrap(),
    )));
    e.handle(EngineCommand::Audition {
        track_id: tid,
        pitch: BASE,
        velocity: 127,
        seconds: 0.05,
    });
    let l = drive(&mut e, secs(0.5));
    let body = &l[secs(0.05)..secs(0.35)];
    assert!(goertzel(body, 220.0, SRD) > 0.1, "a short audition still plays the one-shot out");

    e.handle(EngineCommand::NoteOn {
        track_id: tid,
        pitch: BASE + 1,
        velocity: 127,
    });
    let l = drive(&mut e, secs(0.3));
    assert!(goertzel(&l[secs(0.05)..], 660.0, SRD) > 0.1);
    e.handle(EngineCommand::NoteOff {
        track_id: tid,
        pitch: BASE + 1,
    });
    // An unmapped key on the same track stays silent.
    e.handle(EngineCommand::NoteOn {
        track_id: tid,
        pitch: BASE + 3,
        velocity: 127,
    });
    let l = drive(&mut e, secs(0.5));
    assert!(peak(&l[secs(0.2)..]) < 1e-3);
}

#[test]
fn silencing_the_track_cuts_one_shots_a_note_off_cannot() {
    let (mut m, tid, pid, pool) = tone_project(vec![]);
    // Poly, so both auditions sound together.
    m.tracks.iter_mut().find(|t| t.id == tid).unwrap().sampler.as_mut().unwrap().voice = SamplerVoice::Poly;
    let mut e = Engine::new(SR, Arc::new(Shared::default()), &m.master);
    e.handle(EngineCommand::SetAudioPool(pool));
    e.handle(EngineCommand::SetSequence(Arc::new(
        Sequence::from_pattern(&m, &pid, 0).unwrap(),
    )));
    for pitch in [BASE, BASE + 1] {
        e.handle(EngineCommand::Audition { track_id: tid, pitch, velocity: 127, seconds: 0.05 });
    }
    let l = drive(&mut e, secs(0.1));
    assert!(goertzel(&l[secs(0.02)..], 220.0, SRD) > 0.1 && goertzel(&l[secs(0.02)..], 660.0, SRD) > 0.1);
    e.handle(EngineCommand::AuditionOff { track_id: tid, pitch: BASE });
    let l = drive(&mut e, secs(0.1));
    assert!(goertzel(&l[secs(0.02)..], 220.0, SRD) > 0.1, "a note-off leaves a one-shot ringing");

    e.handle(EngineCommand::SilenceTrack { track_id: tid });
    let l = drive(&mut e, secs(0.2));
    // The cut is 4 ms; the master's fixed filters ring out over the next few.
    assert!(peak(&l[secs(0.03)..]) < 1e-3, "both chops are cut");
}

#[test]
fn an_audition_on_a_mono_track_cuts_the_chop_already_sounding() {
    let (m, tid, pid, pool) = tone_project(vec![]);
    let mut e = Engine::new(SR, Arc::new(Shared::default()), &m.master);
    e.handle(EngineCommand::SetAudioPool(pool));
    e.handle(EngineCommand::SetSequence(Arc::new(
        Sequence::from_pattern(&m, &pid, 0).unwrap(),
    )));
    e.handle(EngineCommand::Audition { track_id: tid, pitch: BASE, velocity: 127, seconds: 0.05 });
    drive(&mut e, secs(0.05));
    e.handle(EngineCommand::NoteOn { track_id: tid, pitch: BASE + 1, velocity: 127 });
    let l = drive(&mut e, secs(0.2));
    assert!(goertzel(&l[secs(0.03)..], 220.0, SRD) < 1e-3, "the live key cut the audition");
    assert!(goertzel(&l[secs(0.03)..], 660.0, SRD) > 0.1);
}

#[test]
fn a_44k_source_renders_at_its_own_duration_on_a_48k_engine() {
    let buffer = tones(&[440.0], 1.0, 44_100);
    let source = source_for(&buffer);
    let (m, _, pid) = sampler_project(&source, vec![chop(0.0, 0.5)], vec![note(0, 2 * B, BASE)], 4 * B);
    let l = render_pattern(&m, &pid, Some(pool_of(source.id, buffer)));
    let first = l.iter().position(|x| x.abs() > 0.01).unwrap();
    let last = l.iter().rposition(|x| x.abs() > 0.01).unwrap();
    let span = (last - first) as i64;
    assert!((span - secs(0.5) as i64).abs() < 250, "played {span} samples");
    let body = &l[first + secs(0.05)..first + secs(0.45)];
    assert!(goertzel(body, 440.0, SRD) > 10.0 * goertzel(body, 479.0, SRD));
}
