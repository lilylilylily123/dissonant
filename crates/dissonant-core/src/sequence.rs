//! The flattened, engine-facing view of what to play: one list of notes per track, the chord
//! track for the bed, and a loop length. Built from either the selected pattern (pattern mode)
//! or the whole arrangement (song mode). The engine never looks at `ProjectModel` directly.

use crate::arrangement::Arrangement;
use crate::chord_track::ChordTrack;
use crate::model::{NoteEvent, ProjectModel, Track, TrackPlugin};
use crate::tempo::TempoMap;
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
    /// Set when the track is plugin-driven: the engine hosts this instead of `voice`.
    pub plugin: Option<TrackPlugin>,
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
            plugin: track.plugin.clone(),
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
    /// Swing percent (50 = none) and grid, applied by the scheduler to note starts and ends.
    pub swing: f64,
    pub swing_grid: f64,
    /// Quarter-note beats per bar, for the metronome accent.
    pub beats_per_bar: f64,
    /// Beats ⇄ seconds. Constant at `tempo_bpm` in pattern mode; the song's map in song mode.
    pub tempo_map: TempoMap,
}

impl Sequence {
    pub fn from_pattern(model: &ProjectModel, pattern_id: &Uuid) -> Option<Self> {
        let pattern = model.pattern(pattern_id)?;
        // Each track and the chord track are laid out independently across the pattern, so a
        // 2-bar drum loop arrives as two copies under one turn of a 4-bar progression.
        let tracks = model
            .tracks
            .iter()
            .map(|t| SequenceTrack::from_track(t, pattern.looped_notes(&t.id)))
            .collect();
        Some(Sequence {
            tracks,
            chords: pattern.looped_chords(),
            length_beats: pattern.length_beats.max(1.0),
            tempo_bpm: model.tempo,
            swing: model.swing,
            swing_grid: model.swing_grid,
            beats_per_bar: model.time_signature.beats_per_bar(),
            tempo_map: TempoMap::constant(model.tempo),
        })
    }

    pub fn from_song(model: &ProjectModel) -> Self {
        let tracks = model
            .tracks
            .iter()
            .map(|t| {
                SequenceTrack::from_track(
                    t,
                    Arrangement::flattened_notes(&t.id, &model.patterns, &model.clips),
                )
            })
            .collect();
        let total = Arrangement::total_length(&model.clips);
        let fallback = model.patterns.first().map(|p| p.length_beats).unwrap_or(16.0);
        Sequence {
            tracks,
            chords: Arrangement::flattened_chords(&model.patterns, &model.clips),
            // Same floor as `from_pattern`: a loaded project can carry a zero, negative or
            // non-finite `length_beats` (nothing clamps it on decode), and a non-positive loop
            // length stops song mode from ever wrapping and makes offline render bail out.
            length_beats: if total > 0.0 { total } else { fallback }.max(1.0),
            tempo_bpm: model.tempo,
            swing: model.swing,
            swing_grid: model.swing_grid,
            beats_per_bar: model.time_signature.beats_per_bar(),
            tempo_map: model.tempo_map(),
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
        model.clips = vec![crate::model::Clip::new(pattern_id, 0.0, 16.0), crate::model::Clip::new(pattern_id, 16.0, 16.0)];

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

    /// The headline case: a 2-bar drum loop under a 4-bar progression plays twice, and the
    /// chords stay on their own 4-bar turn.
    #[test]
    fn tracks_and_chords_loop_independently_inside_a_pattern() {
        let mut model = ProjectModel::empty();
        model.tracks.push(crate::model::Track::drums("drums"));
        let (lead, drums) = (model.tracks[0].id, model.tracks[1].id);
        let pattern_id = model.patterns[0].id;
        let p = &mut model.patterns[0];
        p.length_beats = 16.0;
        p.notes_by_track.insert(drums, vec![NoteEvent::new(0.0, 0.25, 36), NoteEvent::new(4.0, 0.25, 38)]);
        p.notes_by_track.insert(lead, vec![NoteEvent::new(0.0, 1.0, 60)]);
        p.chords = crate::chord_track::ChordTrack::new(vec![
            crate::model::ChordEvent::new(0.0, 4.0, vec![0, 4, 7], Some("C".into())),
            crate::model::ChordEvent::new(4.0, 4.0, vec![5, 9, 0], Some("F".into())),
        ]);
        p.track_loop_beats.insert(drums, 8.0);
        p.chord_loop_beats = Some(8.0);

        let seq = Sequence::from_pattern(&model, &pattern_id).unwrap();
        let drum_track = seq.tracks.iter().find(|t| t.id == drums).unwrap();
        assert_eq!(
            drum_track.notes.iter().map(|n| n.start_beat).collect::<Vec<_>>(),
            vec![0.0, 4.0, 8.0, 12.0],
            "the 2-bar drum loop should play twice across the 4-bar pattern"
        );
        let lead_track = seq.tracks.iter().find(|t| t.id == lead).unwrap();
        assert_eq!(lead_track.notes.len(), 1, "a track with no loop of its own still fills the pattern once");
        assert_eq!(seq.chords.chord_at(1.0).unwrap().name.as_deref(), Some("C"));
        assert_eq!(seq.chords.chord_at(9.0).unwrap().name.as_deref(), Some("C"), "the chord loop repeats too");
        assert_eq!(seq.chords.chord_at(13.0).unwrap().name.as_deref(), Some("F"));
        let ids: std::collections::HashSet<_> = drum_track.notes.iter().map(|n| n.id).collect();
        assert_eq!(ids.len(), 4, "every repeat needs its own id or the engine cannot tell them apart");
    }

    /// A note hanging over the end of its loop is cut there rather than bleeding into the
    /// next turn and colliding with the copy that starts it.
    #[test]
    fn a_loop_trims_what_overruns_its_own_turn() {
        let mut model = ProjectModel::empty();
        let track_id = model.tracks[0].id;
        let pattern_id = model.patterns[0].id;
        let p = &mut model.patterns[0];
        p.length_beats = 16.0;
        p.notes_by_track.insert(track_id, vec![NoteEvent::new(3.0, 8.0, 60)]);
        p.track_loop_beats.insert(track_id, 4.0);

        let seq = Sequence::from_pattern(&model, &pattern_id).unwrap();
        let notes = &seq.tracks[0].notes;
        assert_eq!(notes.len(), 4);
        assert!(notes.iter().all(|n| (n.length_beats - 1.0).abs() < 1e-9), "each turn should end at its own boundary");
        assert_eq!(notes.iter().map(|n| n.start_beat).collect::<Vec<_>>(), vec![3.0, 7.0, 11.0, 15.0]);
    }

    /// Loops are a pattern-internal layout, so the song's clip repeats carry them along.
    #[test]
    fn clip_repeats_carry_the_pattern_internal_loops() {
        let mut model = ProjectModel::empty();
        let track_id = model.tracks[0].id;
        let pattern_id = model.patterns[0].id;
        let p = &mut model.patterns[0];
        p.length_beats = 16.0;
        p.notes_by_track.insert(track_id, vec![NoteEvent::new(0.0, 1.0, 60)]);
        p.track_loop_beats.insert(track_id, 4.0);
        model.clips = vec![crate::model::Clip::new(pattern_id, 0.0, 32.0)];

        let seq = Sequence::from_song(&model);
        assert_eq!(seq.tracks[0].notes.len(), 8, "four turns per pattern, twice over the clip");
    }
}
