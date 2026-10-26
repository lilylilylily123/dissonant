//! Flattens the song's clips into one continuous timeline. A clip plays its pattern looped
//! from `offset_ticks` for `length_ticks`, starting at `start_tick`; notes and chords that
//! would run past the clip's end are cut there. Pure and testable; song playback and the
//! song-mode playhead read from these.

use crate::chord_track::ChordTrack;
use crate::model::{ChordEvent, Clip, NoteEvent, SongPattern, MIN_NOTE_TICKS};
use std::collections::HashMap;
use uuid::Uuid;

pub struct Arrangement;

impl Arrangement {
    pub fn total_length(clips: &[Clip]) -> i64 {
        clips.iter().map(Clip::end_tick).max().unwrap_or(0)
    }

    /// Visit every (clip, repetition index, pattern, timeline offset of that repetition).
    fn repetitions<'a>(
        patterns: &'a [SongPattern],
        clips: &'a [Clip],
    ) -> impl Iterator<Item = (&'a Clip, u64, &'a SongPattern, i64)> + 'a {
        clips.iter().filter(|c| !c.muted).flat_map(move |clip| {
            let pattern = patterns.iter().find(|p| p.id == clip.pattern_id);
            let mut out = Vec::new();
            if let Some(p) = pattern {
                if p.length_ticks > 0 {
                    let mut rep = 0u64;
                    let mut pos = -clip.offset_ticks.rem_euclid(p.length_ticks);
                    while pos < clip.length_ticks {
                        out.push((clip, rep, p, clip.start_tick + pos));
                        pos += p.length_ticks;
                        rep += 1;
                    }
                }
            }
            out
        })
    }

    /// Notes for one track across the whole song. Each pattern is first laid out internally
    /// (a track that loops shorter than its pattern repeats to fill it), then that layout is
    /// repeated across the clip. Occurrence ids are derived deterministically from (note id,
    /// clip id, repetition) so repeats produce stable, distinct ids the engine can key on.
    pub fn flattened_notes(track_id: &Uuid, patterns: &[SongPattern], clips: &[Clip]) -> Vec<NoteEvent> {
        let looped: HashMap<Uuid, Vec<NoteEvent>> =
            patterns.iter().map(|p| (p.id, p.looped_notes(track_id))).collect();
        let mut result = Vec::new();
        for (clip, rep, pattern, base) in Self::repetitions(patterns, clips) {
            let clip_end = clip.end_tick();
            for note in looped.get(&pattern.id).map(Vec::as_slice).unwrap_or_default() {
                let start = base + note.start_tick;
                if start < clip.start_tick || start >= clip_end {
                    continue;
                }
                let end = (start + note.length_ticks).min(clip_end);
                let mut salt = clip.id.as_bytes().to_vec();
                salt.extend_from_slice(&rep.to_le_bytes());
                result.push(NoteEvent {
                    id: Uuid::new_v5(&note.id, &salt),
                    start_tick: start,
                    length_ticks: (end - start).max(MIN_NOTE_TICKS),
                    ..note.clone()
                });
            }
        }
        result.sort_by_key(|n| n.start_tick);
        result
    }

    pub fn flattened_chords(patterns: &[SongPattern], clips: &[Clip]) -> ChordTrack {
        let looped: HashMap<Uuid, ChordTrack> = patterns.iter().map(|p| (p.id, p.looped_chords())).collect();
        let mut chords = Vec::new();
        for (clip, rep, pattern, base) in Self::repetitions(patterns, clips) {
            let clip_end = clip.end_tick();
            let source = looped.get(&pattern.id).map(ChordTrack::chords).unwrap_or_default();
            for chord in source {
                let start = base + chord.start_tick;
                let end = (start + chord.length_ticks).min(clip_end);
                // A chord that started before the clip still counts from the clip's start.
                let start = start.max(clip.start_tick);
                if end <= start {
                    continue;
                }
                let mut salt = clip.id.as_bytes().to_vec();
                salt.extend_from_slice(&rep.to_le_bytes());
                chords.push(ChordEvent {
                    id: Uuid::new_v5(&chord.id, &salt),
                    start_tick: start,
                    length_ticks: end - start,
                    pitch_classes: chord.pitch_classes.clone(),
                    name: chord.name.clone(),
                    legacy_start_beat: None,
                    legacy_length_beats: None,
                });
            }
        }
        ChordTrack::new(chords)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticks::TICKS_PER_BEAT;

    const BEAT: i64 = TICKS_PER_BEAT;

    fn track() -> Uuid {
        Uuid::from_u128(1)
    }

    fn pattern_a() -> SongPattern {
        let mut p = SongPattern::new("A");
        p.chords = ChordTrack::new(vec![ChordEvent::new(0, 8 * BEAT, vec![0, 4, 7], Some("C".into()))]);
        p.notes_by_track.insert(track(), vec![NoteEvent::new(2 * BEAT, BEAT, 60)]);
        p
    }
    fn pattern_b() -> SongPattern {
        let mut p = SongPattern::new("B");
        p.chords = ChordTrack::new(vec![ChordEvent::new(0, 8 * BEAT, vec![5, 9, 0], Some("F".into()))]);
        p.notes_by_track.insert(track(), vec![NoteEvent::new(4 * BEAT, BEAT, 64)]);
        p
    }

    #[test]
    fn total_length_is_the_last_clip_end() {
        let (a, b) = (pattern_a(), pattern_b());
        let clips = vec![
            Clip::new(a.id, 0, 16 * BEAT),
            Clip::new(b.id, 16 * BEAT, 16 * BEAT),
            Clip::new(a.id, 40 * BEAT, 8 * BEAT),
        ];
        assert_eq!(Arrangement::total_length(&clips), 48 * BEAT);
        assert_eq!(Arrangement::total_length(&[]), 0);
    }

    #[test]
    fn notes_land_at_clip_positions() {
        let (a, b) = (pattern_a(), pattern_b());
        let clips = vec![Clip::new(a.id, 0, 16 * BEAT), Clip::new(b.id, 16 * BEAT, 16 * BEAT)];
        let notes = Arrangement::flattened_notes(&track(), &[a.clone(), b.clone()], &clips);
        assert_eq!(notes.iter().map(|n| n.start_tick).collect::<Vec<_>>(), vec![2 * BEAT, 20 * BEAT]);
    }

    #[test]
    fn a_long_clip_loops_its_pattern_and_a_short_one_trims() {
        let a = pattern_a();
        let long = vec![Clip::new(a.id, 0, 40 * BEAT)];
        let notes = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &long);
        assert_eq!(notes.iter().map(|n| n.start_tick).collect::<Vec<_>>(), vec![2 * BEAT, 18 * BEAT, 34 * BEAT]);
        let short = vec![Clip::new(a.id, 0, 5 * BEAT / 2)];
        let notes = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &short);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].length_ticks, BEAT / 2, "cut at the clip end");
        let none = vec![Clip::new(a.id, 0, BEAT)];
        assert!(Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &none).is_empty());
    }

    #[test]
    fn offset_starts_mid_pattern_and_muted_clips_are_silent() {
        let a = pattern_a();
        let mut c = Clip::new(a.id, 0, 16 * BEAT);
        c.offset_ticks = 2 * BEAT; // pattern tick 2·BEAT lands at clip start
        let notes = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &[c.clone()]);
        assert_eq!(notes.iter().map(|n| n.start_tick).collect::<Vec<_>>(), vec![0]);
        c.muted = true;
        assert!(Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &[c]).is_empty());
    }

    #[test]
    fn chords_follow_clips_and_repeats_get_distinct_stable_ids() {
        let (a, b) = (pattern_a(), pattern_b());
        let clips = vec![Clip::new(a.id, 0, 16 * BEAT), Clip::new(b.id, 16 * BEAT, 16 * BEAT)];
        let chords = Arrangement::flattened_chords(&[a.clone(), b.clone()], &clips);
        assert_eq!(chords.chord_at(0).unwrap().name.as_deref(), Some("C"));
        assert_eq!(chords.chord_at(16 * BEAT).unwrap().name.as_deref(), Some("F"));
        assert!(chords.chord_at(10 * BEAT).is_none(), "the 8-beat chord does not cover beat 10");

        let twice = vec![Clip::new(a.id, 0, 32 * BEAT)];
        let notes = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &twice);
        assert_eq!(notes.len(), 2);
        assert_ne!(notes[0].id, notes[1].id);
        let again = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &twice);
        assert_eq!(notes[1].id, again[1].id);
    }
}
