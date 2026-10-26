//! The flattened, engine-facing view of what to play: one list of notes per track, the chord
//! track for the bed, and a loop length. Built from either the selected pattern (pattern mode)
//! or the whole arrangement (song mode). The engine never looks at `ProjectModel` directly.

use crate::arrangement::Arrangement;
use crate::chord_track::ChordTrack;
use crate::model::{AmpRig, Insert, NoteEvent, ProjectModel, Track, TrackPlugin};
use crate::tempo::TempoMap;
use crate::ticks::{bar_ticks, TICKS_PER_BEAT};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct SequenceTrack {
    pub id: Uuid,
    pub voice: String,
    pub is_drum: bool,
    pub is_audio: bool,
    pub muted: bool,
    pub soloed: bool,
    pub volume: f64,
    pub reverb_send: f64,
    pub tone: f64,
    pub pan: f64,
    /// Set when the track is plugin-driven: the engine hosts this instead of `voice`.
    pub plugin: Option<TrackPlugin>,
    /// Set on an audio track: the rig its audio and live input run through.
    pub amp: Option<AmpRig>,
    /// This track's effect chain, in order, applied pre-fader.
    pub inserts: Vec<Insert>,
    pub notes: Vec<NoteEvent>,
}

impl SequenceTrack {
    fn from_track(track: &Track, notes: Vec<NoteEvent>) -> Self {
        SequenceTrack {
            id: track.id,
            voice: track.voice.clone(),
            is_drum: track.is_drum,
            is_audio: track.is_audio,
            muted: track.muted,
            soloed: track.soloed,
            volume: track.volume,
            reverb_send: track.reverb_send,
            tone: track.tone,
            pan: track.pan,
            plugin: track.plugin.clone(),
            amp: track.amp.clone(),
            inserts: track.inserts.clone(),
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
    pub length_ticks: i64,
    pub tempo_bpm: f64,
    /// Swing percent (50 = none) and grid in ticks, applied by the scheduler to note starts
    /// and ends.
    pub swing: f64,
    pub swing_grid_ticks: i64,
    /// Ticks per bar, for the metronome accent.
    pub bar_ticks: i64,
    /// Ticks ⇄ seconds. Constant at `tempo_bpm` in pattern mode; the song's map in song mode.
    pub tempo_map: TempoMap,
    /// Audio placed on the song timeline, in seconds. Empty in pattern mode.
    pub audio: Vec<SequenceAudioClip>,
    /// The master effect chain, in order, ahead of the master bus processing.
    pub master_inserts: Vec<Insert>,
}

/// An audio clip flattened for the engine: start and length in seconds, so the audio thread
/// never touches the tempo map. Audio does not warp, so only the start moves with tempo.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceAudioClip {
    pub track_id: Uuid,
    pub source_id: Uuid,
    pub start_seconds: f64,
    pub length_seconds: f64,
    pub offset_seconds: f64,
    pub gain: f64,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,
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
            length_ticks: pattern.length_ticks.max(TICKS_PER_BEAT),
            tempo_bpm: model.tempo,
            swing: model.swing,
            swing_grid_ticks: model.swing_grid_ticks,
            bar_ticks: bar_ticks(model.time_signature),
            tempo_map: TempoMap::constant(model.tempo),
            // Audio clips live on the song timeline; pattern mode is a notes-only loop.
            audio: vec![],
            master_inserts: model.master_inserts.clone(),
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
        let map = model.tempo_map();
        let audio: Vec<SequenceAudioClip> = model
            .audio_clips
            .iter()
            .filter(|c| !c.muted)
            .map(|c| SequenceAudioClip {
                track_id: c.track_id,
                source_id: c.source_id,
                start_seconds: map.seconds_at_tick(c.start_tick as f64),
                length_seconds: c.length_seconds,
                offset_seconds: c.offset_seconds,
                gain: c.gain,
                fade_in_seconds: c.fade_in_seconds,
                fade_out_seconds: c.fade_out_seconds,
            })
            .collect();
        // The loop has to cover the audio too, or a song that is nothing but a guitar take
        // would wrap instantly and never play it. Rounded up: stopping mid-tick would clip
        // the tail of the take.
        let audio_end = audio
            .iter()
            .map(|c| map.tick_at_seconds(c.start_seconds + c.length_seconds).ceil() as i64)
            .max()
            .unwrap_or(0);
        let total = Arrangement::total_length(&model.clips).max(audio_end);
        let fallback = model.patterns.first().map(|p| p.length_ticks).unwrap_or(16 * TICKS_PER_BEAT);
        Sequence {
            tracks,
            chords: Arrangement::flattened_chords(&model.patterns, &model.clips),
            // Same floor as `from_pattern`: a loaded project can carry a zero or negative
            // `length_ticks` (nothing clamps it on decode), and a non-positive loop length
            // stops song mode from ever wrapping and makes offline render bail out.
            length_ticks: if total > 0 { total } else { fallback }.max(TICKS_PER_BEAT),
            tempo_bpm: model.tempo,
            swing: model.swing,
            swing_grid_ticks: model.swing_grid_ticks,
            bar_ticks: bar_ticks(model.time_signature),
            tempo_map: map,
            audio,
            master_inserts: model.master_inserts.clone(),
        }
    }

    pub fn any_solo(&self) -> bool {
        self.tracks.iter().any(|t| t.soloed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEAT: i64 = TICKS_PER_BEAT;

    #[test]
    fn pattern_and_song_sequences() {
        let mut model = ProjectModel::starter();
        let track_id = model.tracks[0].id;
        let pattern_id = model.patterns[0].id;
        model.patterns[0]
            .notes_by_track
            .insert(track_id, vec![NoteEvent::new(0, BEAT, 60)]);
        model.clips = vec![
            crate::model::Clip::new(pattern_id, 0, 16 * BEAT),
            crate::model::Clip::new(pattern_id, 16 * BEAT, 16 * BEAT),
        ];

        let p = Sequence::from_pattern(&model, &pattern_id).unwrap();
        assert_eq!(p.length_ticks, 16 * BEAT);
        assert_eq!(p.tracks[0].notes.len(), 1);
        assert!(!p.chords.is_empty());

        let s = Sequence::from_song(&model);
        assert_eq!(s.length_ticks, 32 * BEAT);
        assert_eq!(s.tracks[0].notes.len(), 2);
        assert_eq!(s.chords.chord_at(17 * BEAT).unwrap().name.as_deref(), Some("C"));
    }

    #[test]
    fn empty_song_falls_back_to_first_pattern_length() {
        let mut model = ProjectModel::empty();
        model.patterns[0].length_ticks = 8 * BEAT;
        assert_eq!(Sequence::from_song(&model).length_ticks, 8 * BEAT);
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
        p.length_ticks = 16 * BEAT;
        p.notes_by_track.insert(drums, vec![NoteEvent::new(0, BEAT / 4, 36), NoteEvent::new(4 * BEAT, BEAT / 4, 38)]);
        p.notes_by_track.insert(lead, vec![NoteEvent::new(0, BEAT, 60)]);
        p.chords = crate::chord_track::ChordTrack::new(vec![
            crate::model::ChordEvent::new(0, 4 * BEAT, vec![0, 4, 7], Some("C".into())),
            crate::model::ChordEvent::new(4 * BEAT, 4 * BEAT, vec![5, 9, 0], Some("F".into())),
        ]);
        p.track_loop_ticks.insert(drums, 8 * BEAT);
        p.chord_loop_ticks = Some(8 * BEAT);

        let seq = Sequence::from_pattern(&model, &pattern_id).unwrap();
        let drum_track = seq.tracks.iter().find(|t| t.id == drums).unwrap();
        assert_eq!(
            drum_track.notes.iter().map(|n| n.start_tick).collect::<Vec<_>>(),
            vec![0, 4 * BEAT, 8 * BEAT, 12 * BEAT],
            "the 2-bar drum loop should play twice across the 4-bar pattern"
        );
        let lead_track = seq.tracks.iter().find(|t| t.id == lead).unwrap();
        assert_eq!(lead_track.notes.len(), 1, "a track with no loop of its own still fills the pattern once");
        assert_eq!(seq.chords.chord_at(BEAT).unwrap().name.as_deref(), Some("C"));
        assert_eq!(seq.chords.chord_at(9 * BEAT).unwrap().name.as_deref(), Some("C"), "the chord loop repeats too");
        assert_eq!(seq.chords.chord_at(13 * BEAT).unwrap().name.as_deref(), Some("F"));
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
        p.length_ticks = 16 * BEAT;
        p.notes_by_track.insert(track_id, vec![NoteEvent::new(3 * BEAT, 8 * BEAT, 60)]);
        p.track_loop_ticks.insert(track_id, 4 * BEAT);

        let seq = Sequence::from_pattern(&model, &pattern_id).unwrap();
        let notes = &seq.tracks[0].notes;
        assert_eq!(notes.len(), 4);
        assert!(notes.iter().all(|n| n.length_ticks == BEAT), "each turn should end at its own boundary");
        assert_eq!(
            notes.iter().map(|n| n.start_tick).collect::<Vec<_>>(),
            vec![3 * BEAT, 7 * BEAT, 11 * BEAT, 15 * BEAT]
        );
    }

    /// Loops are a pattern-internal layout, so the song's clip repeats carry them along.
    #[test]
    fn clip_repeats_carry_the_pattern_internal_loops() {
        let mut model = ProjectModel::empty();
        let track_id = model.tracks[0].id;
        let pattern_id = model.patterns[0].id;
        let p = &mut model.patterns[0];
        p.length_ticks = 16 * BEAT;
        p.notes_by_track.insert(track_id, vec![NoteEvent::new(0, BEAT, 60)]);
        p.track_loop_ticks.insert(track_id, 4 * BEAT);
        model.clips = vec![crate::model::Clip::new(pattern_id, 0, 32 * BEAT)];

        let seq = Sequence::from_song(&model);
        assert_eq!(seq.tracks[0].notes.len(), 8, "four turns per pattern, twice over the clip");
    }
}
