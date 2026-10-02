//! Offline rendering: run the same [`Engine`] faster than real time into a WAV file.
//! Deterministic, no device needed — this is what export, stems and tests use.

use crate::engine::{Engine, EngineCommand, Shared};
use dissonant_core::{MasterSettings, Sequence, Tempo};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("nothing to render: the sequence is empty")]
    Empty,
    #[error("could not write WAV: {0}")]
    Wav(#[from] hound::Error),
}

#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub sample_rate: u32,
    /// Extra seconds after the last beat so releases and reverb tails aren't cut.
    pub tail_seconds: f64,
    pub hear_chords: bool,
    pub master: MasterSettings,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            sample_rate: 44_100,
            tail_seconds: 1.5,
            hear_chords: false,
            master: MasterSettings::default(),
        }
    }
}

/// Render `sequence` once (no looping) plus a tail, as interleaved stereo f32.
pub fn render_to_buffer(sequence: Arc<Sequence>, options: &RenderOptions) -> Result<Vec<f32>, RenderError> {
    if sequence.length_beats <= 0.0 {
        return Err(RenderError::Empty);
    }
    let sr = options.sample_rate as f32;
    let mut engine = Engine::new(sr, Arc::new(Shared::default()), &options.master);
    engine.handle(EngineCommand::SetLooping(false));
    engine.handle(EngineCommand::SetHearChords(options.hear_chords));
    let tempo = Tempo::new(sequence.tempo_bpm);
    let seconds = tempo.seconds_for_beats(sequence.length_beats) + options.tail_seconds;
    engine.handle(EngineCommand::SetSequence(sequence));
    engine.handle(EngineCommand::Play);

    let total_frames = (seconds * sr as f64).ceil() as usize;
    let block = 1024;
    let mut out = Vec::with_capacity(total_frames * 2);
    let mut buf = vec![0.0f32; block * 2];
    let mut done = 0;
    while done < total_frames {
        let n = (total_frames - done).min(block);
        engine.process(&mut buf[..n * 2]);
        out.extend_from_slice(&buf[..n * 2]);
        done += n;
    }
    Ok(out)
}

/// Render to a 16-bit stereo WAV file.
pub fn render_wav(sequence: Arc<Sequence>, path: &Path, options: &RenderOptions) -> Result<(), RenderError> {
    let samples = render_to_buffer(sequence, options)?;
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: options.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for s in samples {
        writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dissonant_core::{NoteEvent, ProjectModel};

    #[test]
    fn renders_a_wav_with_the_expected_length() {
        let mut model = ProjectModel::starter();
        let (tid, pid) = (model.tracks[0].id, model.patterns[0].id);
        model.patterns[0].length_beats = 4.0;
        model.patterns[0].notes_by_track.insert(tid, vec![NoteEvent::new(0.0, 1.0, 60)]);
        let seq = Arc::new(Sequence::from_pattern(&model, &pid).unwrap());
        let dir = std::env::temp_dir().join(format!("dissonant-render-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.wav");
        let opts = RenderOptions {
            tail_seconds: 0.5,
            ..Default::default()
        };
        render_wav(seq, &path, &opts).unwrap();
        let reader = hound::WavReader::open(&path).unwrap();
        // 4 beats @120 = 2 s + 0.5 s tail = 2.5 s → 110250 frames.
        assert_eq!(reader.duration(), 110_250);
        let peak = reader.into_samples::<i16>().map(|s| s.unwrap().abs()).max().unwrap();
        assert!(peak > 1000);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn empty_sequence_is_an_error() {
        let seq = Arc::new(Sequence::default());
        assert!(matches!(render_to_buffer(seq, &RenderOptions::default()), Err(RenderError::Empty)));
    }
}
