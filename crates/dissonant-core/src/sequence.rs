//! The flattened, engine-facing view of what to play: one list of notes per track, the chord
//! track for the bed, and a loop length. Built from either the selected pattern (pattern mode)
//! or the whole arrangement (song mode). The engine never looks at `ProjectModel` directly.

use crate::arrangement::Arrangement;
use crate::chord_track::ChordTrack;
use crate::model::{NoteEvent, ProjectModel, Track};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceTrack {
    pub id: Uuid,
    pub voice: String,
    pub is_drum: bool,
    pub muted: bool,
    pub soloed: bool,
    pub volume: f64,
    pub reverb_send: f64,
    pub tone: f64,
    pub pan: f64,
    pub notes: Vec<NoteEvent>,
}

impl SequenceTrack {
    fn from_track(track: &Track, notes: Vec<NoteEvent>) -> Self {
        SequenceTrack {
            id: track.id,
            voice: track.voice.clone(),
            is_drum: track.is_drum,
            muted: track.muted,
            soloed: track.soloed,
            volume: track.volume,
            reverb_send: track.reverb_send,
            tone: track.tone,
            pan: track.pan,
            notes,
        }
    }

    /// Audible given the project-wide solo state.
    pub fn audible(&self, any_solo: bool) -> bool {
        if any_solo {
            self.soloed
        } else {
            !self.muted
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sequence {
    pub tracks: Vec<SequenceTrack>,
    pub chords: ChordTrack,
    pub length_beats: f64,
    pub tempo_bpm: f64,
}

impl Sequence {
    pub fn from_pattern(model: &ProjectModel, pattern_id: &Uuid) -> Option<Self> {
        let pattern = model.pattern(pattern_id)?;
        let tracks = model
            .tracks
            .iter()
            .map(|t| SequenceTrack::from_track(t, pattern.notes(&t.id).to_vec()))
            .collect();
        Some(Sequence {
            tracks,
            chords: pattern.chords.clone(),
            length_beats: pattern.length_beats.max(1.0),
            tempo_bpm: model.tempo,
        })
    }

    pub fn from_song(model: &ProjectModel) -> Self {
        let tracks = model
            .tracks
            .iter()
            .map(|t| {
                SequenceTrack::from_track(
                    t,
                    Arrangement::flattened_notes(&t.id, &model.patterns, &model.arrangement),
                )
            })
            .collect();
        let total = Arrangement::total_length(&model.patterns, &model.arrangement);
        let fallback = model.patterns.first().map(|p| p.length_beats).unwrap_or(16.0);
        Sequence {
            tracks,
            chords: Arrangement::flattened_chords(&model.patterns, &model.arrangement),
            length_beats: if total > 0.0 { total } else { fallback },
            tempo_bpm: model.tempo,
        }
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.soloed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_and_song_sequences() {
        let mut model = ProjectModel::starter();
        let track_id = model.tracks[0].id;
        let pattern_id = model.patterns[0].id;
        model.patterns[0]
            .notes_by_track
            .insert(track_id, vec![NoteEvent::new(0.0, 1.0, 60)]);
        model.arrangement = vec![pattern_id, pattern_id];

        let p = Sequence::from_pattern(&model, &pattern_id).unwrap();
        assert_eq!(p.length_beats, 16.0);
        assert_eq!(p.tracks[0].notes.len(), 1);
        assert!(!p.chords.is_empty());

        let s = Sequence::from_song(&model);
        assert_eq!(s.length_beats, 32.0);
        assert_eq!(s.tracks[0].notes.len(), 2);
        assert_eq!(s.chords.chord_at(17.0).unwrap().name.as_deref(), Some("C"));
    }

    #[test]
    fn empty_song_falls_back_to_first_pattern_length() {
        let mut model = ProjectModel::empty();
        model.patterns[0].length_beats = 8.0;
        assert_eq!(Sequence::from_song(&model).length_beats, 8.0);
    }
}
