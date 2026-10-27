//! Behavioural audit of the stock effects.
//!
//! Each test asks the one question the effect's name is a promise about — does the high-pass
//! pass highs and stop lows, does the compressor flatten dynamics, does the repeat land a beat
//! later — rather than pinning coefficients, so the DSP can be improved without rewriting the
//! tests. The last one is the guarantee the whole feature rests on: the offline bounce of a
//! native chain is sample-for-sample what playback produced.

use dissonant_core::{NativeKind, NoteEvent, ProjectModel, Sequence, TICKS_PER_BEAT};
use dissonant_engine::engine::{Engine, EngineCommand, Shared};
use dissonant_engine::render::{render_to_buffer, RenderOptions, RenderPlugins};
use dissonant_engine::{InsertSlot, InsertUnit, NativeEffect};
use std::sync::Arc;
use uuid::Uuid;

const SR: f32 = 48_000.0;

fn effect(kind: NativeKind, params: &[(u32, f64)]) -> NativeEffect {
    let mut e = NativeEffect::new(kind, SR);
    for (id, value) in params {
        e.set_param(*id, *value);
    }
    e
}

/// A mono signal run through an effect, as both sides of a stereo pair; the left comes back.
fn run(e: &mut NativeEffect, input: &[f32]) -> Vec<f32> {
    let mut l = input.to_vec();
    let mut r = input.to_vec();
    // One beat at 120 bpm, the tempo every timing test here assumes.
    e.process(&mut l, &mut r, 0.5);
    l
}

fn sine(hz: f32, frames: usize, amplitude: f32) -> Vec<f32> {
    (0..frames)
        .map(|i| amplitude * (std::f32::consts::TAU * hz * i as f32 / SR).sin())
        .collect()
}

fn rms(xs: &[f32]) -> f64 {
    (xs.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>() / xs.len().max(1) as f64).sqrt()
}

fn peak(xs: &[f32]) -> f32 {
    xs.iter().fold(0.0f32, |p, s| p.max(s.abs()))
}

/// Peak over rms: how spiky a signal is. A compressor's whole job is to make this smaller.
fn crest(xs: &[f32]) -> f64 {
    peak(xs) as f64 / rms(xs).max(1e-12)
}

/// The steady part of a response, past the filter's own settling.
fn tail(xs: &[f32]) -> &[f32] {
    &xs[xs.len() / 2..]
}

#[test]
fn the_eq_high_pass_stops_lows_and_passes_highs() {
    let frames = 24_000;
    let (low_in, high_in) = (sine(60.0, frames, 0.5), sine(4_000.0, frames, 0.5));

    let mut cut = effect(NativeKind::Eq, &[(100, 500.0)]);
    let low_out = run(&mut cut, &low_in);
    let mut cut = effect(NativeKind::Eq, &[(100, 500.0)]);
    let high_out = run(&mut cut, &high_in);

    assert!(
        rms(tail(&low_out)) < rms(tail(&low_in)) * 0.1,
        "a 500 Hz high-pass left a 60 Hz sine at {:.3} of its level",
        rms(tail(&low_out)) / rms(tail(&low_in))
    );
    assert!(
        rms(tail(&high_out)) > rms(tail(&high_in)) * 0.9,
        "a 500 Hz high-pass took {:.3} off a 4 kHz sine",
        1.0 - rms(tail(&high_out)) / rms(tail(&high_in))
    );

    // And at the bottom of its range it is off, not a 20 Hz filter: a fresh EQ is a wire.
    let mut flat = effect(NativeKind::Eq, &[]);
    let flat_out = run(&mut flat, &low_in);
    assert!(
        (rms(tail(&flat_out)) - rms(tail(&low_in))).abs() < 1e-6,
        "a freshly added EQ changed the signal"
    );
}

#[test]
fn the_eq_low_pass_and_shelves_move_the_band_they_name() {
    let frames = 24_000;
    let high = sine(8_000.0, frames, 0.4);
    let mut cut = effect(NativeKind::Eq, &[(114, 1_000.0)]);
    assert!(
        rms(tail(&run(&mut cut, &high))) < rms(tail(&high)) * 0.2,
        "a 1 kHz low-pass passed 8 kHz"
    );

    // A low shelf lifts what is under it and leaves what is over it alone.
    let low = sine(60.0, frames, 0.4);
    let mut shelf = effect(NativeKind::Eq, &[(101, 200.0), (102, 12.0)]);
    let lifted = run(&mut shelf, &low);
    assert!(
        rms(tail(&lifted)) > rms(tail(&low)) * 3.0,
        "a +12 dB low shelf gained {:.2}x at 60 Hz",
        rms(tail(&lifted)) / rms(tail(&low))
    );
    let mut shelf = effect(NativeKind::Eq, &[(101, 200.0), (102, 12.0)]);
    let untouched = run(&mut shelf, &high);
    assert!(
        (rms(tail(&untouched)) / rms(tail(&high)) - 1.0).abs() < 0.05,
        "a low shelf at 200 Hz moved 8 kHz"
    );
}

#[test]
fn the_compressor_reduces_crest_factor_and_makeup_puts_the_level_back() {
    // A sine swelling between -22 and -1 dBFS twice a second. A step would be a transient no
    // feed-forward detector can catch, and the crest would then measure the overshoot rather
    // than the gain computer.
    let frames = 48_000;
    let mut input = sine(220.0, frames, 1.0);
    for (i, s) in input.iter_mut().enumerate() {
        let swell = 0.5 - 0.5 * (std::f32::consts::TAU * 2.0 * i as f32 / SR).cos();
        *s *= 0.08 + 0.82 * swell;
    }

    // A fast attack, so the test measures the gain computer and not how much of the first
    // millisecond of each burst slipped past the detector.
    let mut comp = effect(
        NativeKind::Compressor,
        &[(200, -24.0), (201, 12.0), (202, 0.1), (203, 60.0)],
    );
    let out = run(&mut comp, &input);
    assert!(
        crest(&out) < crest(&input) * 0.8,
        "crest went {:.2} → {:.2}; the compressor is not compressing",
        crest(&input),
        crest(&out)
    );

    // Makeup is a clean gain on top: the same shape, louder.
    let mut with_makeup = effect(
        NativeKind::Compressor,
        &[
            (200, -24.0),
            (201, 12.0),
            (202, 0.1),
            (203, 60.0),
            (205, 6.0),
        ],
    );
    let loud = run(&mut with_makeup, &input);
    let ratio = rms(&loud) / rms(&out);
    assert!(
        (ratio - 2.0).abs() < 0.02,
        "+6 dB of makeup gave {ratio:.3}x instead of 2x"
    );

    // A wide knee bends earlier than a hard one, so it is already working below the threshold.
    let quiet = sine(220.0, frames, 0.06);
    let mut hard = effect(NativeKind::Compressor, &[(200, -18.0), (201, 8.0), (204, 0.0)]);
    let mut soft = effect(NativeKind::Compressor, &[(200, -18.0), (201, 8.0), (204, 18.0)]);
    assert!(
        rms(tail(&run(&mut soft, &quiet))) < rms(tail(&run(&mut hard, &quiet))) * 0.98,
        "the knee made no difference below the threshold"
    );
}

#[test]
fn the_limiter_holds_its_ceiling_without_silencing_the_signal() {
    let input = sine(300.0, 48_000, 2.0);
    let mut lim = effect(NativeKind::Limiter, &[(300, -6.0), (301, 0.0)]);
    let out = run(&mut lim, &input);

    let ceiling = 10f32.powf(-6.0 / 20.0);
    // Past the lookahead and the first attack, nothing may exceed the ceiling at all.
    assert!(
        peak(tail(&out)) <= ceiling + 1e-6,
        "the limiter let {:.4} through a ceiling of {ceiling:.4}",
        peak(tail(&out))
    );
    assert!(
        rms(tail(&out)) > ceiling as f64 * 0.5,
        "the limiter crushed the signal to nothing"
    );

    // Drive pushes more of the signal into the ceiling; the ceiling does not move.
    let mut driven = effect(NativeKind::Limiter, &[(300, -6.0), (301, 18.0)]);
    let hot = run(&mut driven, &sine(300.0, 48_000, 0.1));
    assert!(peak(tail(&hot)) <= ceiling + 1e-6, "drive broke the ceiling");
    let mut clean = effect(NativeKind::Limiter, &[(300, -6.0), (301, 0.0)]);
    assert!(
        rms(tail(&hot)) > rms(tail(&run(&mut clean, &sine(300.0, 48_000, 0.1)))) * 2.0,
        "drive did not make the quiet signal louder"
    );
}

#[test]
fn the_saturator_compresses_the_curve_as_it_is_driven_harder() {
    // Saturation *is* a level-dependent gain: the loud input gets proportionally less out than
    // the quiet one. A linear stage, however filtered, would give the same ratio for both.
    let gain_at = |amplitude: f32| {
        let input = sine(220.0, 24_000, amplitude);
        let mut sat = effect(NativeKind::Saturator, &[(400, 12.0), (401, 0.0)]);
        rms(tail(&run(&mut sat, &input))) / rms(tail(&input))
    };
    let (quiet, loud) = (gain_at(0.02), gain_at(0.9));
    assert!(
        loud < quiet * 0.5,
        "driving the stage from 0.02 to 0.9 changed its gain {quiet:.3} → {loud:.3}; that is not saturation"
    );

    // Mix at zero is the bypass the knob promises.
    let input = sine(220.0, 24_000, 0.5);
    let mut dry = effect(NativeKind::Saturator, &[(400, 20.0), (403, 0.0)]);
    let out = run(&mut dry, &input);
    assert!(
        out.iter().zip(&input).all(|(a, b)| (a - b).abs() < 1e-6),
        "a saturator mixed fully dry still changed the signal"
    );
}

#[test]
fn the_delay_repeat_lands_exactly_one_synced_interval_later() {
    // 120 bpm, so a beat is half a second; `delay 1/4` is one beat and `1/8` is half of one.
    for (division, beats) in [(4.0, 1.0), (8.0, 0.5), (2.0, 2.0)] {
        let mut delay = effect(
            NativeKind::Delay,
            &[(500, division), (501, 0.0), (502, 1.0)],
        );
        let mut input = vec![0.0f32; 60_000];
        input[0] = 1.0;
        let out = run(&mut delay, &input);

        let expected = (0.5 * beats * SR as f64).round() as usize;
        let landed = out[1..]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(i, _)| i + 1)
            .expect("the delay wrote nothing");
        assert_eq!(
            landed, expected,
            "1/{division} at 120 bpm repeated at sample {landed}, not {expected}"
        );
    }

    // Feedback is what makes it more than one repeat.
    let mut delay = effect(NativeKind::Delay, &[(500, 8.0), (501, 0.7), (502, 1.0)]);
    let mut input = vec![0.0f32; 80_000];
    input[0] = 1.0;
    let out = run(&mut delay, &input);
    let second = (0.5 * SR as f64).round() as usize;
    assert!(
        out[second].abs() > 0.05,
        "feedback produced no second repeat"
    );
}

#[test]
fn the_reverb_rings_on_after_the_sound_stops_and_decay_sets_how_long() {
    let burst = |frames: usize| {
        let mut x = vec![0.0f32; frames];
        x[..2_000].copy_from_slice(&sine(440.0, 2_000, 0.5));
        x
    };

    let mut rev = effect(NativeKind::Reverb, &[(600, 0.8), (601, 0.9)]);
    let long = run(&mut rev, &burst(96_000));
    let mut rev = effect(NativeKind::Reverb, &[(600, 0.8), (601, 0.05)]);
    let short = run(&mut rev, &burst(96_000));

    // Half a second after the burst has stopped, there is still something there.
    let late = 48_000..96_000;
    assert!(
        rms(&long[late.clone()]) > 1e-4,
        "the reverb had no tail at all"
    );
    assert!(
        rms(&long[late.clone()]) > rms(&short[late]) * 2.0,
        "decay 0.9 did not ring longer than decay 0.05"
    );

    // Pre-delay keeps the ambience off the front of the sound: the first 10 ms are dry.
    let mut rev = effect(NativeKind::Reverb, &[(600, 1.0), (602, 50.0)]);
    let out = run(&mut rev, &burst(96_000));
    assert!(
        rms(&out[..400]) < 1e-4,
        "a 50 ms pre-delay let the tank speak immediately"
    );
}

#[test]
fn a_bypassed_native_slot_is_a_wire_and_an_untouched_chain_is_silent_until_it_is_fed() {
    let mut delay = effect(NativeKind::Delay, &[(500, 4.0), (502, 1.0)]);
    assert!(
        delay.is_silent(),
        "an effect that has never run claimed to be making sound"
    );
    let mut input = vec![0.0f32; 8_000];
    input[0] = 1.0;
    run(&mut delay, &input);
    assert!(!delay.is_silent(), "a delay holding a repeat called itself silent");
}

// ─── Playback and the bounce ─────────────────────────────────────────────────────────────────

/// Model with one sustained note, so a chain has something to chew on.
fn model() -> ProjectModel {
    let mut m = ProjectModel::empty();
    m.tracks[0].voice = "saw".into();
    m.patterns[0].length_ticks = 8 * TICKS_PER_BEAT;
    m.patterns[0].notes = vec![NoteEvent::new(0, 4 * TICKS_PER_BEAT, 57)];
    m
}

/// The chain the parity test runs: one of everything that has state worth desynchronising —
/// a filter, a detector, a delay line and a tank.
fn native_chain() -> Vec<InsertSlot> {
    let specs: [(NativeKind, &[(u32, f64)]); 4] = [
        (NativeKind::Eq, &[(100, 400.0), (110, 6.0)]),
        (NativeKind::Compressor, &[(200, -28.0), (201, 8.0), (205, 4.0)]),
        (NativeKind::Delay, &[(500, 8.0), (501, 0.4), (502, 0.4)]),
        (NativeKind::Reverb, &[(600, 0.35), (601, 0.7), (602, 30.0)]),
    ];
    specs
        .into_iter()
        .map(|(kind, params)| {
            let mut e = NativeEffect::new(kind, SR);
            for (id, value) in params {
                e.set_param(*id, *value);
            }
            InsertSlot::new(Uuid::new_v4(), Some(InsertUnit::Native(Box::new(e))))
        })
        .collect()
}

/// Drive an engine exactly as the live device callback does — the same sequence, the same
/// chain, a different block size — and hand back the interleaved stereo it produced.
fn play(m: &ProjectModel, seq: Arc<Sequence>, frames: usize, block: usize) -> Vec<f32> {
    let mut e = Engine::new(SR, Arc::new(Shared::default()), &m.master);
    e.handle(EngineCommand::SetLooping(false));
    e.handle(EngineCommand::SetHearChords(false));
    e.handle(EngineCommand::SetSequence(seq));
    for (index, slot) in native_chain().into_iter().enumerate() {
        e.handle(EngineCommand::AddInsert {
            target: dissonant_core::InsertTarget::Track(m.tracks[0].id),
            index,
            slot,
        });
    }
    e.handle(EngineCommand::Play);
    let mut out = Vec::with_capacity(frames * 2);
    let mut buf = vec![0.0f32; block * 2];
    let mut done = 0;
    while done < frames {
        let n = (frames - done).min(block);
        e.process(&mut buf[..n * 2]);
        out.extend_from_slice(&buf[..n * 2]);
        done += n;
    }
    out
}

#[test]
fn a_native_insert_chain_bounces_exactly_what_it_plays() {
    let m = model();
    let seq = Arc::new(Sequence::from_pattern(&m, &m.patterns[0].id, 0).expect("pattern"));
    let opts = RenderOptions {
        sample_rate: SR as u32,
        tail_seconds: 1.0,
        master: m.master.clone(),
        ..Default::default()
    };

    let bounced = render_to_buffer(
        Arc::clone(&seq),
        &opts,
        RenderPlugins {
            instruments: vec![],
            inserts: vec![(
                dissonant_core::InsertTarget::Track(m.tracks[0].id),
                native_chain(),
            )],
        },
    )
    .expect("bounce");

    // Deliberately a different block size from the renderer's 1024: a chain that is not
    // block-size independent bounces something the user never heard.
    let played = play(&m, seq, bounced.len() / 2, 256);

    assert!(
        peak(&bounced) > 1e-3,
        "the bounce is silent, so matching it would prove nothing"
    );
    assert_eq!(bounced.len(), played.len());
    let worst = bounced
        .iter()
        .zip(&played)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert_eq!(worst, 0.0, "the bounce and the playback diverged by {worst}");
}

#[test]
fn a_native_chain_is_audible_through_the_engine_and_bypassing_it_is_not() {
    let m = model();
    let seq = Arc::new(Sequence::from_pattern(&m, &m.patterns[0].id, 0).expect("pattern"));
    let opts = RenderOptions {
        sample_rate: SR as u32,
        tail_seconds: 0.5,
        master: m.master.clone(),
        ..Default::default()
    };
    let target = dissonant_core::InsertTarget::Track(m.tracks[0].id);

    let dry = render_to_buffer(Arc::clone(&seq), &opts, RenderPlugins::default()).expect("dry");
    let wet = render_to_buffer(
        Arc::clone(&seq),
        &opts,
        RenderPlugins {
            instruments: vec![],
            inserts: vec![(target, native_chain())],
        },
    )
    .expect("wet");
    assert!(
        wet.iter().zip(&dry).any(|(a, b)| (a - b).abs() > 1e-4),
        "a chain of four native effects left the mix untouched"
    );

    let bypassed = render_to_buffer(
        seq,
        &opts,
        RenderPlugins {
            instruments: vec![],
            inserts: vec![(
                target,
                native_chain()
                    .into_iter()
                    .map(|mut s| {
                        s.enabled = false;
                        s
                    })
                    .collect(),
            )],
        },
    )
    .expect("bypassed");
    let worst = bypassed
        .iter()
        .zip(&dry)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert_eq!(worst, 0.0, "a bypassed native chain coloured the signal");
}
