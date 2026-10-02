//! Flattens a pattern arrangement into a single continuous timeline — the song. Each helper
//! concatenates patterns in arrangement order, offsetting note/chord beats by the running
//! length. Pure and testable; song playback and the song-mode playhead read from these.

use crate::chord_track::ChordTrack;
use crate::model::{ChordEvent, NoteEvent, SongPattern};
use uuid::Uuid;

pub struct Arrangement;

impl Arrangement {
    pub fn total_length(patterns: &[SongPattern], arrangement: &[Uuid]) -> f64 {
        arrangement
            .iter()
            .map(|pid| patterns.iter().find(|p| &p.id == pid).map(|p| p.length_beats).unwrap_or(0.0))
            .sum()
    }

    /// Notes for one track across the whole song. Occurrence ids are derived
    /// deterministically from (note id, occurrence index) so repeated patterns produce stable,
    /// distinct ids the engine can key on.
    pub fn flattened_notes(track_id: &Uuid, patterns: &[SongPattern], arrangement: &[Uuid]) -> Vec<NoteEvent> {
        let mut result = Vec::new();
        let mut offset = 0.0;
        for (occurrence, pid) in arrangement.iter().enumerate() {
            let Some(pattern) = patterns.iter().find(|p| &p.id == pid) else { continue };
            for note in pattern.notes(track_id) {
                result.push(NoteEvent {
                    id: Uuid::new_v5(&note.id, &(occurrence as u64).to_le_bytes()),
                    start_beat: note.start_beat + offset,
                    length_beats: note.length_beats,
                    pitch: note.pitch,
                    velocity: note.velocity,
                    intentional: note.intentional,
                });
            }
            offset += pattern.length_beats;
        }
        result
    }

    pub fn flattened_chords(patterns: &[SongPattern], arrangement: &[Uuid]) -> ChordTrack {
        let mut chords = Vec::new();
        let mut offset = 0.0;
        for (occurrence, pid) in arrangement.iter().enumerate() {
            let Some(pattern) = patterns.iter().find(|p| &p.id == pid) else { continue };
            for chord in pattern.chords.chords() {
                chords.push(ChordEvent {
                    id: Uuid::new_v5(&chord.id, &(occurrence as u64).to_le_bytes()),
                    start_beat: chord.start_beat + offset,
                    length_beats: chord.length_beats,
                    pitch_classes: chord.pitch_classes.clone(),
                    name: chord.name.clone(),
                });
            }
            offset += pattern.length_beats;
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
    fn total_length() {
        let (a, b) = (pattern_a(), pattern_b());
        assert_eq!(Arrangement::total_length(&[a.clone(), b.clone()], &[a.id, b.id, a.id]), 48.0);
    }

    #[test]
    fn notes_offset_by_pattern_length() {
        let (a, b) = (pattern_a(), pattern_b());
        let notes = Arrangement::flattened_notes(&track(), &[a.clone(), b.clone()], &[a.id, b.id]);
        let mut starts: Vec<f64> = notes.iter().map(|n| n.start_beat).collect();
        starts.sort_by(f64::total_cmp);
        assert_eq!(starts, vec![2.0, 20.0]);
    }

    #[test]
    fn chords_offset() {
        let (a, b) = (pattern_a(), pattern_b());
        let chords = Arrangement::flattened_chords(&[a.clone(), b.clone()], &[a.id, b.id]);
        assert_eq!(chords.chord_at(0.0).unwrap().name.as_deref(), Some("C"));
        assert_eq!(chords.chord_at(16.0).unwrap().name.as_deref(), Some("F"));
    }

    #[test]
    fn repeated_pattern_duplicates_notes_with_distinct_stable_ids() {
        let a = pattern_a();
        let notes = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &[a.id, a.id]);
        assert_eq!(notes.len(), 2);
        assert_ne!(notes[0].id, notes[1].id);
        let again = Arrangement::flattened_notes(&track(), std::slice::from_ref(&a), &[a.id, a.id]);
        assert_eq!(notes[1].id, again[1].id);
    }

    #[test]
    fn empty_arrangement() {
        let a = pattern_a();
        assert_eq!(Arrangement::total_length(std::slice::from_ref(&a), &[]), 0.0);
        assert!(Arrangement::flattened_notes(&track(), &[a], &[]).is_empty());
    }
}
