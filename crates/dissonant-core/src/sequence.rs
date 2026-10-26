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
    /// One pattern looped on its own track, under the slice of the song's progression that
    /// starts at `chord_offset_ticks` (where the pattern sits in the song, so its tiering
    /// matches what the arrangement will sound like).
    pub fn from_pattern(
        model: &ProjectModel,
        pattern_id: &Uuid,
        chord_offset_ticks: i64,
    ) -> Option<Self> {
        let pattern = model.pattern(pattern_id)?;
        let length = pattern.length_ticks.max(TICKS_PER_BEAT);
        let offset = chord_offset_ticks.max(0);
        let tracks = model
            .tracks
            .iter()
            .map(|t| {
                SequenceTrack::from_track(
                    t,
                    if t.id == pattern.track_id {
                        pattern.notes.clone()
                    } else {
                        vec![]
                    },
                )
            })
            .collect();
        Some(Sequence {
            tracks,
            chords: ChordTrack::window(model.chords.chords(), offset, length),
            length_ticks: length,
            ..Self::silent(model, length)
        })
    }

    /// Every track with no notes and no chords, for pattern mode with nothing to edit.
    pub fn silent(model: &ProjectModel, length_ticks: i64) -> Self {
        Sequence {
            tracks: model
                .tracks
                .iter()
                .map(|t| SequenceTrack::from_track(t, vec![]))
                .collect(),
            chords: ChordTrack::default(),
            length_ticks: length_ticks.max(TICKS_PER_BEAT),
            tempo_bpm: model.tempo,
            swing: model.swing,
            swing_grid_ticks: model.swing_grid_ticks,
            bar_ticks: bar_ticks(model.time_signature),
            tempo_map: TempoMap::constant(model.tempo),
            // Audio clips live on the song timeline; pattern mode is a notes-only loop.
            audio: vec![],
            master_inserts: model.master_inserts.clone(),
        }
    }

    /// Also play another pattern (the ghost) on its own track, looped from tick 0 under this
    /// sequence's length and cut at its end, so the pattern being edited can be heard against
    /// it (a melody over the drums). Repeats get stable, distinct ids derived from the note id
    /// and the repetition. An unknown ghost id leaves the sequence as it was.
    ///
    /// `level` (0…1.5, 1 = as the track is) sets how loud the ghost plays. On a track of its own
    /// it scales that track's volume, which the engine changes without cutting notes; on the
    /// track being edited (which already has the edited pattern's notes) it scales the ghost
    /// notes' velocities instead, so the notes being written keep their level.
    pub fn with_ghost(mut self, model: &ProjectModel, ghost_id: &Uuid, level: f64) -> Self {
        let Some(ghost) = model.pattern(ghost_id) else {
            return self;
        };
        let Some(track) = self.tracks.iter_mut().find(|t| t.id == ghost.track_id) else {
            return self;
        };
        if ghost.length_ticks <= 0 {
            return self;
        }
        let level = if level.is_finite() {
            level.clamp(0.0, 1.5)
        } else {
            1.0
        };
        let shared = !track.notes.is_empty();
        if shared && level <= 0.0 {
            return self;
        }
        if !shared {
            track.volume *= level;
        }
        let velocity = |v: i32| {
            if shared {
                ((v as f64) * level).round().clamp(1.0, 127.0) as i32
            } else {
                v
            }
        };
        let end = self.length_ticks;
        let mut base = 0;
        let mut rep = 0u64;
        while base < end {
            for note in &ghost.notes {
                let start = base + note.start_tick;
                if start >= end {
                    continue;
                }
                let mut salt = b"ghost".to_vec();
                salt.extend_from_slice(&rep.to_le_bytes());
                track.notes.push(NoteEvent {
                    id: Uuid::new_v5(&note.id, &salt),
                    start_tick: start,
                    length_ticks: note
                        .length_ticks
                        .min(end - start)
                        .max(crate::model::MIN_NOTE_TICKS),
                    velocity: velocity(note.velocity),
                    ..note.clone()
                });
            }
            base += ghost.length_ticks;
            rep += 1;
        }
        track.notes.sort_by_key(|n| n.start_tick);
        self
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
            .map(|c| {
                map.tick_at_seconds(c.start_seconds + c.length_seconds)
                    .ceil() as i64
            })
            .max()
            .unwrap_or(0);
        // The song's hard end is the loop, but a take recorded past it must still be heard.
        let total = model.song_length_ticks.max(audio_end);
        Sequence {
            tracks,
            chords: model.chords.clone(),
            // Same floor as `from_pattern`: a loaded project can carry a zero or negative
            // length (nothing clamps it on decode), and a non-positive loop length stops song
            // mode from ever wrapping and makes offline render bail out.
            length_ticks: total.max(TICKS_PER_BEAT),
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
    use crate::model::{ChordEvent, Clip};

    const BEAT: i64 = TICKS_PER_BEAT;

    #[test]
    fn pattern_mode_plays_only_its_own_track() {
        let model = ProjectModel::starter();
        let (melody, drums) = (model.patterns[0].clone(), model.patterns[1].clone());
        let p = Sequence::from_pattern(&model, &melody.id, 0).unwrap();
        assert_eq!(p.length_ticks, 16 * BEAT);
        assert_eq!(p.tracks.len(), 2, "every track is there");
        assert_eq!(p.tracks[0].notes.len(), melody.notes.len());
        assert!(
            p.tracks[1].notes.is_empty(),
            "the drums are silent while the melody is edited"
        );
        let d = Sequence::from_pattern(&model, &drums.id, 0).unwrap();
        assert!(d.tracks[0].notes.is_empty());
        assert_eq!(d.tracks[1].notes.len(), drums.notes.len());
        assert!(Sequence::from_pattern(&model, &Uuid::new_v4(), 0).is_none());
    }

    #[test]
    fn a_ghost_pattern_loops_under_the_edited_one_on_its_own_track() {
        let mut model = ProjectModel::starter();
        let (melody, drums) = (model.patterns[0].id, model.patterns[1].id);
        model.patterns[1].length_ticks = 4 * BEAT;
        model.patterns[1].notes = vec![
            NoteEvent::new(0, BEAT / 4, 36),
            NoteEvent::new(3 * BEAT, 2 * BEAT, 38),
        ];
        let p = Sequence::from_pattern(&model, &melody, 0)
            .unwrap()
            .with_ghost(&model, &drums, 1.0);
        let starts: Vec<i64> = p.tracks[1].notes.iter().map(|n| n.start_tick).collect();
        assert_eq!(
            starts,
            vec![
                0,
                3 * BEAT,
                4 * BEAT,
                7 * BEAT,
                8 * BEAT,
                11 * BEAT,
                12 * BEAT,
                15 * BEAT
            ],
            "a 4-beat ghost loops four times under 16 beats"
        );
        assert_eq!(
            p.tracks[1].notes.last().unwrap().length_ticks,
            BEAT,
            "cut at the loop end"
        );
        let ids: std::collections::HashSet<_> = p.tracks[1].notes.iter().map(|n| n.id).collect();
        assert_eq!(ids.len(), 8, "every repeat has its own id");
        assert_eq!(
            p.tracks[0].notes.len(),
            model.patterns[0].notes.len(),
            "the edited pattern is untouched"
        );
        let same = Sequence::from_pattern(&model, &melody, 0).unwrap();
        assert_eq!(
            same.clone().with_ghost(&model, &Uuid::new_v4(), 1.0),
            same,
            "an unknown ghost changes nothing"
        );
    }

    #[test]
    fn the_ghost_level_scales_its_own_track_or_only_its_notes_on_a_shared_one() {
        let mut model = ProjectModel::starter();
        let (melody, drums) = (model.patterns[0].id, model.patterns[1].id);
        let base = Sequence::from_pattern(&model, &melody, 0).unwrap();
        let quiet = base.clone().with_ghost(&model, &drums, 0.5);
        assert_eq!(
            quiet.tracks[1].volume,
            base.tracks[1].volume * 0.5,
            "its own track: the track volume"
        );
        assert_eq!(
            quiet.tracks[1]
                .notes
                .iter()
                .map(|n| n.velocity)
                .collect::<Vec<_>>(),
            model.patterns[1]
                .notes
                .iter()
                .map(|n| n.velocity)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            quiet.tracks[0], base.tracks[0],
            "the edited track is untouched"
        );

        // A second melody pattern ghosted under the first: same track, so only its notes soften.
        let mut other = model.patterns[0].clone();
        other.id = Uuid::new_v4();
        other.notes = vec![NoteEvent::new(0, BEAT, 121).with_velocity(100)];
        let other_id = other.id;
        model.patterns.push(other);
        let base = Sequence::from_pattern(&model, &melody, 0).unwrap();
        let shared = base.clone().with_ghost(&model, &other_id, 0.5);
        assert_eq!(
            shared.tracks[0].volume, base.tracks[0].volume,
            "the track level stays"
        );
        let ghost_notes: Vec<_> = shared.tracks[0]
            .notes
            .iter()
            .filter(|n| n.pitch == 121)
            .map(|n| n.velocity)
            .collect();
        assert!(
            !ghost_notes.is_empty() && ghost_notes.iter().all(|&v| v == 50),
            "{ghost_notes:?}"
        );
        assert_eq!(
            base.clone().with_ghost(&model, &other_id, 0.0),
            base,
            "silent on a shared track: no ghost notes"
        );
    }

    #[test]
    fn pattern_mode_chords_are_the_song_window_at_the_offset() {
        let mut model = ProjectModel::empty();
        model.patterns[0].length_ticks = 8 * BEAT;
        model.chords = ChordTrack::new(vec![
            ChordEvent::new(0, 4 * BEAT, vec![0, 4, 7], Some("C".into())),
            ChordEvent::new(4 * BEAT, 8 * BEAT, vec![5, 9, 0], Some("F".into())),
            ChordEvent::new(12 * BEAT, 4 * BEAT, vec![7, 11, 2], Some("G".into())),
        ]);
        let pid = model.patterns[0].id;
        let at = |offset: i64| -> Vec<(i64, i64, String)> {
            Sequence::from_pattern(&model, &pid, offset)
                .unwrap()
                .chords
                .chords()
                .iter()
                .map(|c| (c.start_tick, c.length_ticks, c.name.clone().unwrap()))
                .collect()
        };
        assert_eq!(
            at(0),
            vec![(0, 4 * BEAT, "C".into()), (4 * BEAT, 4 * BEAT, "F".into())],
            "from bar 1, cut at the pattern end"
        );
        assert_eq!(
            at(8 * BEAT),
            vec![(0, 4 * BEAT, "F".into()), (4 * BEAT, 4 * BEAT, "G".into())],
            "a chord already sounding starts at 0"
        );
        assert!(at(16 * BEAT).is_empty(), "past the progression: no chords");
        assert_eq!(at(-8 * BEAT), at(0), "a negative offset reads as bar 1");
    }

    #[test]
    fn song_mode_uses_the_explicit_length_and_project_chords() {
        let mut model = ProjectModel::starter();
        let melody = model.patterns[0].id;
        model.patterns[0].notes = vec![NoteEvent::new(0, BEAT, 60)];
        model.clips = vec![
            Clip::new(melody, 0, 16 * BEAT),
            Clip::new(melody, 16 * BEAT, 16 * BEAT),
        ];
        model.song_length_ticks = 64 * BEAT;
        let s = Sequence::from_song(&model);
        assert_eq!(
            s.length_ticks,
            64 * BEAT,
            "the song ends where it says, not at the last clip"
        );
        assert_eq!(s.tracks[0].notes.len(), 2);
        assert!(
            s.tracks[1].notes.is_empty(),
            "the drum pattern has no clips any more"
        );
        assert_eq!(
            s.chords.chord_at(17 * BEAT).unwrap().name.as_deref(),
            Some("C")
        );
    }

    #[test]
    fn an_empty_song_still_has_its_length() {
        let mut model = ProjectModel::empty();
        model.song_length_ticks = 8 * BEAT;
        assert_eq!(Sequence::from_song(&model).length_ticks, 8 * BEAT);
        let silent = Sequence::silent(&model, 0);
        assert_eq!(silent.length_ticks, BEAT);
        assert_eq!(silent.tracks.len(), 1);
        assert!(silent.chords.is_empty());
    }
}
