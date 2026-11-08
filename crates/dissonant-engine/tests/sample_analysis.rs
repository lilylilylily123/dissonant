//! Sample analysis against sources we wrote ourselves.
//!
//! Kick trains at known tempos, chord progressions in known keys, silence and white noise:
//! because we wrote the score, every assertion is "the right answer", and each test prints
//! what it measured (`cargo test -- --nocapture`) so the accuracy can be read off rather than
//! taken on trust.

use dissonant_engine::{analyze, Analysis, AudioBuffer};
use std::f64::consts::TAU;

/// Deterministic noise in -1…1.
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 8_388_608.0 - 1.0
    }
}

/// A kick drum at `at` seconds into mono `out`: a 2 ms burst of beater noise over a sine
/// sweeping 150 → 50 Hz and dying over ~60 ms.
fn add_kick(out: &mut [f32], sr: f64, at: f64, amp: f32, noise: &mut Lcg) {
    let first = (at * sr).round() as usize;
    let len = (0.25 * sr) as usize;
    let mut phase = 0.0f64;
    for i in 0..len {
        let Some(slot) = out.get_mut(first + i) else {
            break;
        };
        let t = i as f64 / sr;
        let hz = 50.0 + 100.0 * (-t / 0.03).exp();
        phase += TAU * hz / sr;
        let body = (phase.sin() * (-t / 0.06).exp()) as f32;
        let click = if t < 0.002 { noise.next() * 0.5 } else { 0.0 };
        *slot += amp * (0.8 * body + click);
    }
}

/// Hit times of a train at `bpm` starting at `offset`, every hit fully inside `seconds`.
fn train_times(bpm: f64, offset: f64, seconds: f64) -> Vec<f64> {
    let period = 60.0 / bpm;
    (0..)
        .map(|i| offset + i as f64 * period)
        .take_while(|&t| t + 0.25 < seconds)
        .collect()
}

/// `mono` duplicated onto `channels` channels (the right one a little quieter, so the
/// fold-down really averages).
fn interleave(mono: &[f32], channels: u8, sr: u32) -> AudioBuffer {
    let mut data = Vec::with_capacity(mono.len() * channels as usize);
    for &x in mono {
        for c in 0..channels {
            data.push(if c == 0 { x } else { x * 0.8 });
        }
    }
    AudioBuffer::new(sr, channels, data)
}

fn kick_train(bpm: f64, seconds: f64, sr: u32) -> (AudioBuffer, Vec<f64>) {
    let mut mono = vec![0.0f32; (seconds * sr as f64) as usize];
    let mut noise = Lcg(0x1357_9bdf);
    let times = train_times(bpm, 0.5, seconds);
    for &t in &times {
        add_kick(&mut mono, sr as f64, t, 0.7, &mut noise);
    }
    (interleave(&mono, 2, sr), times)
}

/// Chords (MIDI notes), each held `chord_seconds`, every note four harmonics at 1/n with 20 ms
/// fades so the changes do not click.
fn progression(chords: &[&[i32]], chord_seconds: f64, sr: u32) -> AudioBuffer {
    let srf = sr as f64;
    let per = (chord_seconds * srf) as usize;
    let fade = (0.02 * srf) as usize;
    let mut mono = vec![0.0f32; per * chords.len()];
    for (c, notes) in chords.iter().enumerate() {
        for i in 0..per {
            let t = (c * per + i) as f64 / srf;
            let env = (i.min(per - 1 - i) as f64 / fade as f64).min(1.0);
            let mut v = 0.0f64;
            for &note in notes.iter() {
                let hz = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
                for h in 1..=4 {
                    v += (TAU * hz * h as f64 * t).sin() / h as f64;
                }
            }
            mono[c * per + i] = (0.12 * env * v) as f32;
        }
    }
    interleave(&mono, 2, sr)
}

fn report(label: &str, a: &Analysis) {
    println!(
        "{label}: bpm {:?} (conf {:.2}), key {:?} {} (conf {:.2}), {} onsets",
        a.bpm,
        a.bpm_confidence,
        a.key_root,
        if a.key_minor { "minor" } else { "major" },
        a.key_confidence,
        a.onsets.len()
    );
}

fn assert_tempo(bpm: f64) {
    let (buffer, _) = kick_train(bpm, 20.0, 48_000);
    let a = analyze(&buffer);
    report(&format!("{bpm} BPM train"), &a);
    let found = a.bpm.expect("a kick train has a tempo");
    assert!((found - bpm).abs() <= 1.0, "heard {found} BPM for {bpm}");
    assert!(a.bpm_confidence > 0.5, "confidence {}", a.bpm_confidence);
}

#[test]
fn a_kick_train_at_90_bpm_reads_as_90_bpm() {
    assert_tempo(90.0);
}

#[test]
fn a_kick_train_at_120_bpm_reads_as_120_bpm() {
    assert_tempo(120.0);
}

#[test]
fn a_kick_train_at_140_bpm_reads_as_140_bpm_not_its_half_time() {
    assert_tempo(140.0);
}

#[test]
fn every_kick_of_a_train_is_one_onset_within_ten_milliseconds() {
    for bpm in [90.0, 120.0, 140.0] {
        let (buffer, truth) = kick_train(bpm, 12.0, 48_000);
        let a = analyze(&buffer);
        let worst = truth
            .iter()
            .map(|&t| {
                a.onsets
                    .iter()
                    .map(|o| (o.0 - t).abs())
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0f64, f64::max);
        println!(
            "{bpm} BPM: {} onsets for {} kicks, worst error {:.2} ms",
            a.onsets.len(),
            truth.len(),
            worst * 1e3
        );
        assert_eq!(a.onsets.len(), truth.len());
        assert!(worst <= 0.010, "worst onset error {:.2} ms", worst * 1e3);
        assert!(a.onsets.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(a.onsets.iter().all(|o| o.1 > 0.0 && o.1 <= 1.0));
        assert!(a.onsets.iter().any(|o| o.1 == 1.0));
    }
}

#[test]
fn a_single_hit_is_one_onset_and_no_tempo() {
    let sr = 48_000;
    let mut mono = vec![0.0f32; 4 * sr as usize];
    add_kick(&mut mono, sr as f64, 1.3, 0.7, &mut Lcg(7));
    let a = analyze(&interleave(&mono, 2, sr));
    report("single hit", &a);
    assert_eq!(a.onsets.len(), 1);
    assert!(
        (a.onsets[0].0 - 1.3).abs() <= 0.010,
        "onset at {}",
        a.onsets[0].0
    );
    assert_eq!(a.onsets[0].1, 1.0);
    assert_eq!(a.bpm, None);
    assert_eq!(a.bpm_confidence, 0.0);
}

#[test]
fn a_c_major_progression_reads_as_c_major() {
    // C – F – G – C, around middle C.
    let chords: [&[i32]; 4] = [&[60, 64, 67], &[60, 65, 69], &[59, 62, 67], &[60, 64, 67]];
    let a = analyze(&progression(&chords, 1.5, 44_100));
    report("C major progression", &a);
    assert_eq!(a.key_root, Some(0));
    assert!(!a.key_minor);
    assert!(a.key_confidence > 0.0);
}

#[test]
fn an_a_minor_progression_reads_as_a_minor() {
    // Am – Dm – E – Am: the G# of the dominant is what tells it from C major.
    let chords: [&[i32]; 4] = [&[57, 60, 64], &[57, 62, 65], &[56, 59, 64], &[57, 60, 64]];
    let a = analyze(&progression(&chords, 1.5, 44_100));
    report("A minor progression", &a);
    assert_eq!(a.key_root, Some(9));
    assert!(a.key_minor);
    assert!(a.key_confidence > 0.0);
}

#[test]
fn silence_has_no_tempo_no_key_and_no_onsets() {
    let a = analyze(&AudioBuffer::new(48_000, 2, vec![0.0; 2 * 48_000 * 5]));
    assert_eq!(a, Analysis::default());
}

#[test]
fn an_empty_or_rateless_buffer_is_silence() {
    assert_eq!(
        analyze(&AudioBuffer::new(48_000, 2, vec![])),
        Analysis::default()
    );
    assert_eq!(
        analyze(&AudioBuffer::new(0, 1, vec![0.5; 1_000])),
        Analysis::default()
    );
}

#[test]
fn white_noise_has_no_key_and_no_tempo() {
    let mut noise = Lcg(0xdead_beef);
    let data: Vec<f32> = (0..2 * 48_000 * 6).map(|_| 0.3 * noise.next()).collect();
    let a = analyze(&AudioBuffer::new(48_000, 2, data));
    report("white noise", &a);
    assert!(a.key_root.is_none() || a.key_confidence < 0.2, "{a:?}");
    assert_eq!(a.bpm, None);
}

#[test]
fn analysis_is_deterministic() {
    let (buffer, _) = kick_train(120.0, 6.0, 44_100);
    assert_eq!(analyze(&buffer), analyze(&buffer));
}

#[test]
fn onsets_are_capped_to_the_512_strongest_in_time_order() {
    // 600 hits, every sixth one quieter: the cap drops 88 of the quiet ones and no loud one.
    let sr = 12_000;
    let mut mono = vec![0.0f32; 61 * sr as usize];
    let mut noise = Lcg(42);
    let mut loud = Vec::new();
    for i in 0..600 {
        let t = 0.3 + i as f64 * 0.1;
        let quiet = i % 6 == 5;
        add_kick(
            &mut mono,
            sr as f64,
            t,
            if quiet { 0.2 } else { 0.7 },
            &mut noise,
        );
        if !quiet {
            loud.push(t);
        }
    }
    let a = analyze(&AudioBuffer::new(sr, 1, mono));
    assert_eq!(a.onsets.len(), 512);
    assert!(a.onsets.windows(2).all(|w| w[0].0 < w[1].0));
    assert!(a.onsets.iter().any(|o| o.1 == 1.0));
    for t in loud {
        assert!(
            a.onsets.iter().any(|o| (o.0 - t).abs() <= 0.010),
            "loud hit at {t} was dropped"
        );
    }
}

/// Ten minutes of 48 kHz stereo: kicks, chords and hiss together. Only meaningful in a
/// release build: `cargo test -p dissonant-engine --release --test sample_analysis --
/// --ignored --nocapture`.
#[test]
#[ignore = "timing; run in release with --ignored"]
fn ten_minutes_of_stereo_analyses_well_under_five_seconds() {
    let sr = 48_000u32;
    let seconds = 600.0;
    let n = (seconds * sr as f64) as usize;
    let mut mono = vec![0.0f32; n];
    let mut noise = Lcg(99);
    for t in train_times(120.0, 0.5, seconds) {
        add_kick(&mut mono, sr as f64, t, 0.6, &mut noise);
    }
    for (i, slot) in mono.iter_mut().enumerate() {
        let t = i as f64 / sr as f64;
        let chord = (t / 2.0) as usize % 4;
        let root = [60.0, 65.0, 67.0, 60.0][chord];
        let tone: f64 = [0.0, 4.0, 7.0]
            .iter()
            .map(|&s| (TAU * 440.0 * 2f64.powf((root + s - 69.0) / 12.0) * t).sin())
            .sum();
        *slot += (0.05 * tone) as f32 + 0.01 * noise.next();
    }
    let buffer = interleave(&mono, 2, sr);
    let started = std::time::Instant::now();
    let a = analyze(&buffer);
    let elapsed = started.elapsed();
    report("ten minutes", &a);
    println!("analysed 10 min of 48 kHz stereo in {elapsed:?}");
    assert!(elapsed.as_secs_f64() < 5.0, "took {elapsed:?}");
}
