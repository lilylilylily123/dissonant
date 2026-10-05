//! Flattens the song's clips into one continuous timeline. A clip plays its pattern looped
//! from `offset_beats` for `length_beats`, starting at `start_beat`; notes and chords that
//! would run past the clip's end are cut there. Pure and testable; song playback and the
//! song-mode playhead read from these.

use crate::chord_track::ChordTrack;
use crate::model::{ChordEvent, Clip, NoteEvent, SongPattern};
use uuid::Uuid;

pub struct Arrangement;

impl Arrangement {
    pub fn total_length(clips: &[Clip]) -> f64 {
        clips.iter().map(Clip::end_beat).fold(0.0, f64::max)
    }

    /// Visit every (clip, repetition index, pattern, timeline offset of that repetition).
    fn repetitions<'a>(
        patterns: &'a [SongPattern],
        clips: &'a [Clip],
    ) -> impl Iterator<Item = (&'a Clip, u64, &'a SongPattern, f64)> + 'a {
        clips.iter().filter(|c| !c.muted).flat_map(move |clip| {
            let pattern = patterns.iter().find(|p| p.id == clip.pattern_id);
            let mut out = Vec::new();
            if let Some(p) = pattern {
                if p.length_beats > 0.0 {
                    let mut rep = 0u64;
                    let mut pos = -clip.offset_beats.rem_euclid(p.length_beats);
                    while pos < clip.length_beats {
                        out.push((clip, rep, p, clip.start_beat + pos));
                        pos += p.length_beats;
                        rep += 1;
                    }
                }
            }
            out
        })
    }

    /// Notes for one track across the whole song. Occurrence ids are derived
    /// deterministically from (note id, clip id, repetition) so repeats produce stable,
    /// distinct ids the engine can key on.
    pub fn flattened_notes(track_id: &Uuid, patterns: &[SongPattern], clips: &[Clip]) -> Vec<NoteEvent> {
        let mut result = Vec::new();
        for (clip, rep, pattern, base) in Self::repetitions(patterns, clips) {
            let clip_end = clip.end_beat();
            for note in pattern.notes(track_id) {
                let start = base + note.start_beat;
                if start < clip.start_beat || start >= clip_end {
                    continue;
                }
                let end = (start + note.length_beats).min(clip_end);
                let mut salt = clip.id.as_bytes().to_vec();
                salt.extend_from_slice(&rep.to_le_bytes());
                result.push(NoteEvent {
                    id: Uuid::new_v5(&note.id, &salt),
                    start_beat: start,
                    length_beats: (end - start).max(1.0 / 64.0),
                    pitch: note.pitch,
                    velocity: note.velocity,
                    intentional: note.intentional,
                    muted: note.muted,
                });
            }
        }
        result.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
        result
    }

    pub fn flattened_chords(patterns: &[SongPattern], clips: &[Clip]) -> ChordTrack {
        let mut chords = Vec::new();
        for (clip, rep, pattern, base) in Self::repetitions(patterns, clips) {
            let clip_end = clip.end_beat();
            for chord in pattern.chords.chords() {
                let start = base + chord.start_beat;
                let end = (start + chord.length_beats).min(clip_end);
                // A chord that started before the clip still counts from the clip's start.
                let start = start.max(clip.start_beat);
                if end <= start {
                    continue;
                }
                let mut salt = clip.id.as_bytes().to_vec();
                salt.extend_from_slice(&rep.to_le_bytes());
                chords.push(ChordEvent {
                    id: Uuid::new_v5(&chord.id, &salt),
                    start_beat: start,
                    length_beats: end - start,
                    pitch_classes: chord.pitch_classes.clone(),
                    name: chord.name.clone(),
                });
            }
        }
        ChordTrack::new(chords)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> Uuid {
        Uuid::from_u128(1)
    }

    fn pattern_a() -> SongPattern {
        let mut p = SongPattern::new("A");
        p.chords = ChordTrack::new(vec![ChordEvent::new(0.0, 8.0, vec![0, 4, 7], Some("C".into()))]);
        p.notes_by_track.insert(track(), vec![NoteEvent::new(2.0, 1.0, 60)]);
        p
    }
    fn pattern_b() -> SongPattern {
        let mut p = SongPattern::new("B");
        p.chords = ChordTrack::new(vec![ChordEvent::new(0.0, 8.0, vec![5, 9, 0], Some("F".into()))]);
        p.notes_by_track.insert(track(), vec![NoteEvent::new(4.0, 1.0, 64)]);
        p
    }

    #[test]
    fn total_length_is_the_last_clip_end() {
        let (a, b) = (pattern_a(), pattern_b());
        let clips = vec![Clip::new(a.id, 0.0, 16.0), Clip::new(b.id, 16.0, 16.0), Clip::new(a.id, 40.0, 8.0)];
        assert_eq!(Arrangement::total_length(&clips), 48.0);
        assert_eq!(Arrangement::total_length(&[]), 0.0);
    }

    #[test]
    fn notes_land_at_clip_positions() {
        let (a, b) = (pattern_a(), pattern_b());
        let clips = vec![Clip::new(a.id, 0.0, 16.0), Clip::new(b.id, 16.0, 16.0)];
        let notes = Arrangement::flattened_notes(&track(), &[a.clone(), b.clone()], &clips);
        assert_eq!(notes.iter().map(|n| n.start_beat).collect::<Vec<_>>(), vec![2.0, 20.0]);
    }

    #[test]
    fn a_long_clip_loops_its_pattern_and_a_short_one_trims() {
        let a = pattern_a();
        let long = vec![Clip::new(a.id, 0.0, 40.0)];
        let notes = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &long);
        assert_eq!(notes.iter().map(|n| n.start_beat).collect::<Vec<_>>(), vec![2.0, 18.0, 34.0]);
        let short = vec![Clip::new(a.id, 0.0, 2.5)];
        let notes = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &short);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].length_beats, 0.5, "cut at the clip end");
        let none = vec![Clip::new(a.id, 0.0, 1.0)];
        assert!(Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &none).is_empty());
    }

    #[test]
    fn offset_starts_mid_pattern_and_muted_clips_are_silent() {
        let a = pattern_a();
        let mut c = Clip::new(a.id, 0.0, 16.0);
        c.offset_beats = 2.0; // pattern beat 2 lands at clip start
        let notes = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &[c.clone()]);
        assert_eq!(notes.iter().map(|n| n.start_beat).collect::<Vec<_>>(), vec![0.0]);
        c.muted = true;
        assert!(Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &[c]).is_empty());
    }

    #[test]
    fn chords_follow_clips_and_repeats_get_distinct_stable_ids() {
        let (a, b) = (pattern_a(), pattern_b());
        let clips = vec![Clip::new(a.id, 0.0, 16.0), Clip::new(b.id, 16.0, 16.0)];
        let chords = Arrangement::flattened_chords(&[a.clone(), b.clone()], &clips);
        assert_eq!(chords.chord_at(0.0).unwrap().name.as_deref(), Some("C"));
        assert_eq!(chords.chord_at(16.0).unwrap().name.as_deref(), Some("F"));
        assert!(chords.chord_at(10.0).is_none(), "the 8-beat chord does not cover beat 10");

        let twice = vec![Clip::new(a.id, 0.0, 32.0)];
        let notes = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &twice);
        assert_eq!(notes.len(), 2);
        assert_ne!(notes[0].id, notes[1].id);
        let again = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &twice);
        assert_eq!(notes[1].id, again[1].id);
    }
}
