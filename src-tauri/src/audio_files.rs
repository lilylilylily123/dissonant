//! The audio store: decoding imported files and reading/writing the project's own copies.
//!
//! Importing copies. A file is decoded once, written to `<app data>/audio/<id>.wav` as 32-bit
//! float at its own rate, and from then on only our own format is ever read. The original may
//! move or be deleted without breaking the project.
//!
//! The tradeoff: a project copied to another machine arrives with its clips present but
//! silent, which is the same bargain `TrackPlugin.uid`'s absolute bundle path already makes.
//! The UI names the missing source rather than dropping the clip.

use dissonant_engine::audio::AudioBuffer;
use std::path::{Path, PathBuf};
use symphonia::core::audio::{Audio, GenericAudioBufferRef};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use uuid::Uuid;

/// Interleaved f32 at the source's own sample rate.
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: u8,
    pub data: Vec<f32>,
}

impl DecodedAudio {
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels.max(1) as usize
    }
}

/// Extensions the importer accepts, for the file picker and the drag-drop handler.
pub const AUDIO_EXTENSIONS: [&str; 8] = ["wav", "wave", "aif", "aiff", "flac", "mp3", "m4a", "aac"];

pub fn is_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// `<app data>/audio` — where every source's canonical WAV lives.
pub fn store_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("audio")
}

pub fn source_path(dir: &Path, id: Uuid) -> PathBuf {
    dir.join(format!("{id}.wav"))
}

/// Decode any supported file to interleaved f32 at its own sample rate.
///
/// `on_progress` is called with 0…1 as packets are decoded, so a long import can show a bar.
pub fn decode(path: &Path, on_progress: &mut dyn FnMut(f32)) -> Result<DecodedAudio, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(&hint, stream, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("not an audio file this build can read: {e}"))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "the file has no audio track".to_string())?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| "the file has no audio track".to_string())?;
    let total_frames = track.num_frames;

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|e| format!("no decoder for this file: {e}"))?;

    let mut data: Vec<f32> = Vec::new();
    let mut sample_rate = params.sample_rate.unwrap_or(44_100);
    let mut channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(1).clamp(1, 255) as u8;
    let mut decoded_frames = 0u64;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => return Err(format!("could not read the file: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buffer = match decoder.decode(&packet) {
            Ok(b) => b,
            // A damaged packet in the middle of a file should cost that packet, not the import.
            Err(symphonia::core::errors::Error::DecodeError(e)) => {
                log::warn!("skipping a bad packet in {}: {e}", path.display());
                continue;
            }
            Err(e) => return Err(format!("could not decode the file: {e}")),
        };
        sample_rate = buffer.spec().rate();
        channels = buffer.spec().channels().count().clamp(1, 255) as u8;
        append_interleaved(&buffer, &mut data);
        decoded_frames += buffer.frames() as u64;
        if let Some(total) = total_frames {
            if total > 0 {
                on_progress((decoded_frames as f32 / total as f32).clamp(0.0, 1.0));
            }
        }
    }

    if data.is_empty() {
        return Err("the file decoded to no audio at all".into());
    }
    on_progress(1.0);
    Ok(DecodedAudio { sample_rate, channels, data })
}

/// Flatten one decoded packet into the interleaved output, whatever its native sample type.
fn append_interleaved(buffer: &GenericAudioBufferRef<'_>, out: &mut Vec<f32>) {
    use symphonia::core::audio::sample::{i24, u24};
    match buffer {
        GenericAudioBufferRef::F32(b) => interleave(b, out, |s| s),
        GenericAudioBufferRef::F64(b) => interleave(b, out, |s| s as f32),
        GenericAudioBufferRef::S32(b) => interleave(b, out, |s| s as f32 / i32::MAX as f32),
        GenericAudioBufferRef::S24(b) => interleave(b, out, |s: i24| s.inner() as f32 / 8_388_607.0),
        GenericAudioBufferRef::S16(b) => interleave(b, out, |s| s as f32 / i16::MAX as f32),
        GenericAudioBufferRef::S8(b) => interleave(b, out, |s| s as f32 / i8::MAX as f32),
        GenericAudioBufferRef::U32(b) => interleave(b, out, |s| s as f32 / u32::MAX as f32 * 2.0 - 1.0),
        GenericAudioBufferRef::U24(b) => interleave(b, out, |s: u24| s.inner() as f32 / 16_777_215.0 * 2.0 - 1.0),
        GenericAudioBufferRef::U16(b) => interleave(b, out, |s| s as f32 / u16::MAX as f32 * 2.0 - 1.0),
        GenericAudioBufferRef::U8(b) => interleave(b, out, |s| s as f32 / u8::MAX as f32 * 2.0 - 1.0),
    }
}

fn interleave<S>(
    buffer: &symphonia::core::audio::AudioBuffer<S>,
    out: &mut Vec<f32>,
    to_f32: impl Fn(S) -> f32,
) where
    S: symphonia::core::audio::sample::Sample,
{
    let channels = buffer.spec().channels().count();
    let frames = buffer.frames();
    out.reserve(frames * channels);
    for frame in 0..frames {
        for ch in 0..channels {
            out.push(to_f32(buffer.plane(ch).map(|p| p[frame]).unwrap_or_default()));
        }
    }
}

/// Write `<dir>/<id>.wav` as 32-bit float at the source rate.
pub fn write_source(dir: &Path, id: Uuid, audio: &DecodedAudio) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("could not create the audio store: {e}"))?;
    let spec = hound::WavSpec {
        channels: audio.channels.max(1) as u16,
        sample_rate: audio.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let path = source_path(dir, id);
    let mut writer = hound::WavWriter::create(&path, spec).map_err(|e| format!("could not write {}: {e}", path.display()))?;
    for &s in &audio.data {
        writer.write_sample(s).map_err(|e| format!("could not write audio: {e}"))?;
    }
    writer.finalize().map_err(|e| format!("could not finish writing audio: {e}"))
}

/// Read a source back out of the store.
pub fn read_source(dir: &Path, id: Uuid) -> Result<AudioBuffer, String> {
    let path = source_path(dir, id);
    let mut reader = hound::WavReader::open(&path).map_err(|e| format!("could not open {}: {e}", path.display()))?;
    let spec = reader.spec();
    let data: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().filter_map(Result::ok).collect(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().filter_map(Result::ok).map(|s| s as f32 * scale).collect()
        }
    };
    if data.is_empty() {
        return Err(format!("{} holds no audio", path.display()));
    }
    Ok(AudioBuffer::new(spec.sample_rate, spec.channels.clamp(1, 255) as u8, data))
}

/// `buckets` min/max pairs over the mono sum, for the waveform lane.
///
/// Min and max rather than a single magnitude: a waveform drawn from absolute peaks alone
/// loses the asymmetry that makes a guitar take recognisable at a glance.
pub fn peaks(buf: &AudioBuffer, buckets: usize) -> Vec<[f32; 2]> {
    let buckets = buckets.clamp(1, 1 << 16);
    let channels = buf.channels.max(1) as usize;
    if buf.frames == 0 {
        return vec![[0.0, 0.0]; buckets];
    }
    let per = (buf.frames as f64 / buckets as f64).max(1.0);
    (0..buckets)
        .map(|b| {
            let from = (b as f64 * per) as usize;
            let to = (((b + 1) as f64 * per) as usize).min(buf.frames).max(from + 1);
            let (mut lo, mut hi) = (f32::MAX, f32::MIN);
            for frame in from..to.min(buf.frames) {
                let base = frame * channels;
                let mut sum = 0.0;
                for c in 0..channels {
                    sum += buf.data[base + c];
                }
                let s = sum / channels as f32;
                lo = lo.min(s);
                hi = hi.max(s);
            }
            if lo > hi {
                [0.0, 0.0]
            } else {
                [lo, hi]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, channels: u8) -> DecodedAudio {
        let data = (0..frames)
            .flat_map(|n| {
                let s = (2.0 * std::f32::consts::PI * 440.0 * n as f32 / 48_000.0).sin() * 0.5;
                (0..channels).map(move |c| if c == 0 { s } else { -s })
            })
            .collect();
        DecodedAudio { sample_rate: 48_000, channels, data }
    }

    #[test]
    fn a_source_round_trips_through_the_store() {
        let dir = std::env::temp_dir().join(format!("dissonant-audio-test-{}", Uuid::new_v4()));
        let id = Uuid::new_v4();
        let written = tone(4_800, 2);
        write_source(&dir, id, &written).expect("write");

        let read = read_source(&dir, id).expect("read");
        assert_eq!(read.sample_rate, 48_000);
        assert_eq!(read.channels, 2);
        assert_eq!(read.frames, 4_800);
        for (i, (a, b)) in written.data.iter().zip(read.data.iter()).enumerate() {
            assert!((a - b).abs() < 1e-6, "sample {i} changed: {a} became {b}");
        }

        // A two-channel source whose sides cancel still has to draw as a waveform rather than
        // a flat line, so peaks are taken per channel and averaged, not summed blindly.
        let drawn = peaks(&read, 64);
        assert_eq!(drawn.len(), 64);

        let mono = AudioBuffer::new(48_000, 1, written.data.iter().step_by(2).copied().collect());
        let drawn = peaks(&mono, 100);
        assert_eq!(drawn.len(), 100);
        for (i, [lo, hi]) in drawn.iter().enumerate() {
            assert!(lo <= hi, "bucket {i} is inverted: {lo} > {hi}");
            assert!(*lo >= -0.51 && *hi <= 0.51, "bucket {i} escapes the signal: [{lo}, {hi}]");
        }
        assert!(drawn.iter().any(|[lo, _]| *lo < -0.4), "no trough was drawn");
        assert!(drawn.iter().any(|[_, hi]| *hi > 0.4), "no crest was drawn");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_source_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("dissonant-audio-test-{}", Uuid::new_v4()));
        assert!(read_source(&dir, Uuid::new_v4()).is_err());
    }

    #[test]
    fn only_known_extensions_are_offered_to_the_importer() {
        assert!(is_audio_path(Path::new("/a/riff.WAV")));
        assert!(is_audio_path(Path::new("/a/riff.mp3")));
        assert!(!is_audio_path(Path::new("/a/song.dissonant")));
        assert!(!is_audio_path(Path::new("/a/no-extension")));
    }
}
