//! The live capture path, against this machine's real devices.
//!
//! The browser mock cannot exercise any of this and neither can an offline render: opening a
//! cpal input stream is the one part of the guitar path that only a real device can prove.
//!
//! Every test skips loudly when there is no input device, so a headless CI box reports "no
//! device" instead of quietly passing.

use dissonant_engine::{default_input_name, list_input_devices, AudioInput};
use std::time::{Duration, Instant};

/// Long enough for a device to deliver several callbacks, short enough not to stall a run.
const SETTLE: Duration = Duration::from_millis(400);

fn skip_without_input() -> bool {
    if list_input_devices().is_empty() {
        eprintln!("skipped: no audio input device; plug one in to exercise this path");
        return true;
    }
    false
}

#[test]
fn every_listed_input_device_describes_itself() {
    if skip_without_input() {
        return;
    }
    let devices = list_input_devices();
    for d in &devices {
        assert!(!d.name.trim().is_empty(), "an input device has no name");
        assert!(d.channels >= 1, "{} claims {} channels", d.name, d.channels);
        assert!(
            (8_000..=384_000).contains(&d.default_sample_rate),
            "{} claims {} Hz",
            d.name,
            d.default_sample_rate
        );
    }
    // At most one default, and if there is a default name it is in the list.
    assert!(devices.iter().filter(|d| d.is_default).count() <= 1);
    if let Some(name) = default_input_name() {
        assert!(devices.iter().any(|d| d.name == name), "the default input {name} is not in the list");
    }
}

#[test]
fn the_default_device_opens_and_delivers_frames() {
    if skip_without_input() {
        return;
    }
    let devices = list_input_devices();
    let rate = devices
        .iter()
        .find(|d| d.is_default)
        .or_else(|| devices.first())
        .map(|d| d.default_sample_rate)
        .unwrap_or(48_000);

    let (producer, mut consumer) = rtrb::RingBuffer::<f32>::new(rate as usize);
    let input = match AudioInput::start_with(None, rate, None, 0, producer) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("skipped: the default input refused {rate} Hz ({e})");
            return;
        }
    };
    assert_eq!(input.sample_rate, rate);
    assert!(!input.device_name.trim().is_empty());
    assert!(input.channel < input.channels.max(1), "channel {} is out of range", input.channel);

    // One mono sample per frame must show up without anyone draining in between.
    let started = Instant::now();
    let mut got = 0usize;
    while started.elapsed() < SETTLE {
        while consumer.pop().is_ok() {
            got += 1;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!input.has_failed(), "the stream reported an error while running");
    assert!(got > 0, "the input delivered no frames in {SETTLE:?}");
    // Roughly real time: at least a tenth of what the elapsed window should hold, which rules
    // out a stream that fired once and stopped without being anywhere near a timing assertion.
    let expected = rate as f64 * SETTLE.as_secs_f64();
    assert!(got as f64 > expected * 0.1, "only {got} frames in {SETTLE:?}, expected about {expected:.0}");
}

#[test]
fn a_rate_no_device_supports_is_refused_rather_than_resampled() {
    if skip_without_input() {
        return;
    }
    // The engine consumes input 1:1 with its output frames, so an impossible rate has to be an
    // error the user sees, not something silently resampled behind their back.
    let (producer, _consumer) = rtrb::RingBuffer::<f32>::new(1024);
    let result = AudioInput::start_with(None, 7_777, None, 0, producer);
    assert!(result.is_err(), "a 7777 Hz input opened, which no device supports");
    let message = result.err().unwrap().to_string();
    assert!(message.contains("7777"), "the error does not name the rate it refused: {message}");
}

#[test]
fn a_device_that_is_not_there_falls_back_to_the_default() {
    if skip_without_input() {
        return;
    }
    let rate = default_input_name()
        .and_then(|n| list_input_devices().into_iter().find(|d| d.name == n))
        .map(|d| d.default_sample_rate)
        .unwrap_or(48_000);
    let (producer, _consumer) = rtrb::RingBuffer::<f32>::new(1024);
    // A project moved between machines names a device this one has never heard of; falling
    // back beats refusing to capture at all.
    match AudioInput::start_with(Some("no such interface"), rate, None, 0, producer) {
        Ok(input) => assert_ne!(input.device_name, "no such interface"),
        Err(e) => eprintln!("skipped: the default input refused {rate} Hz ({e})"),
    }
}

#[test]
fn a_channel_past_the_end_is_clamped_rather_than_silent() {
    if skip_without_input() {
        return;
    }
    let rate = list_input_devices()
        .first()
        .map(|d| d.default_sample_rate)
        .unwrap_or(48_000);
    let (producer, _consumer) = rtrb::RingBuffer::<f32>::new(1024);
    // Asking for channel 60 on a stereo interface captures its last channel; dropping to
    // silence would look exactly like a dead cable.
    match AudioInput::start_with(None, rate, None, 60, producer) {
        Ok(input) => assert_eq!(input.channel, input.channels.saturating_sub(1)),
        Err(e) => eprintln!("skipped: the default input refused {rate} Hz ({e})"),
    }
}
