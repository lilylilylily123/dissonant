//! The chord track: ordered chords over time, and the lookup the rest of the engine depends
//! on — "what chord is sounding at tick T". This is the authoritative harmonic context that
//! drives the piano-roll tiering (R4, R10).
//!
//! Chords occupy half-open tick intervals `[start, start + length)`. Where two chords overlap,
//! the later-starting one wins (so a freshly dropped chord takes precedence).

use crate::model::ChordEvent;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordTrack {
    #[serde(default)]
    chords: Vec<ChordEvent>,
}

impl ChordTrack {
    pub fn new(mut chords: Vec<ChordEvent>) -> Self {
        chords.sort_by_key(|c| c.start_tick);
        ChordTrack { chords }
    }

    pub fn chords(&self) -> &[ChordEvent] {
        &self.chords
    }

    pub fn is_empty(&self) -> bool {
        self.chords.is_empty()
    }

    /// The chord sounding at `tick`, or `None` in a gap or empty track (drives cold start).
    pub fn chord_at(&self, tick: i64) -> Option<&ChordEvent> {
        self.chords.iter().rev().find(|c| c.contains(tick))
    }

    pub fn get(&self, id: &Uuid) -> Option<&ChordEvent> {
        self.chords.iter().find(|c| &c.id == id)
    }

    pub fn add(&mut self, chord: ChordEvent) {
        self.chords.push(chord);
        self.resort();
    }

    pub fn remove(&mut self, id: &Uuid) {
        self.chords.retain(|c| &c.id != id);
    }

    /// Replace a chord (matched by id) in place; re-sorts if its start moved.
    pub fn update(&mut self, chord: ChordEvent) {
        if let Some(existing) = self.chords.iter_mut().find(|c| c.id == chord.id) {
            *existing = chord;
            self.resort();
        }
    }

    /// Tick of the last chord's end (0 for an empty track).
    pub fn end_tick(&self) -> i64 {
        self.chords.iter().map(ChordEvent::end_tick).max().unwrap_or(0)
    }

    /// Beat fields from an older file folded into the tick fields. The order can change with
    /// them, so the track is re-sorted.
    pub(crate) fn migrated(&mut self) {
        for c in &mut self.chords {
            c.migrated();
        }
        self.resort();
    }

    fn resort(&mut self) {
        self.chords.sort_by_key(|c| c.start_tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ticks::TICKS_PER_BEAT;

    fn c() -> ChordEvent {
        ChordEvent::new(0, 4 * TICKS_PER_BEAT, vec![0, 4, 7], Some("C".into()))
    }
    fn f() -> ChordEvent {
        ChordEvent::new(4 * TICKS_PER_BEAT, 4 * TICKS_PER_BEAT, vec![5, 9, 0], Some("F".into()))
    }

    #[test]
    fn lookup_across_boundaries_is_half_open() {
        let t = ChordTrack::new(vec![f(), c()]);
        assert_eq!(t.chord_at(0).unwrap().name.as_deref(), Some("C"));
        assert_eq!(t.chord_at(4 * TICKS_PER_BEAT - 1).unwrap().name.as_deref(), Some("C"));
        assert_eq!(t.chord_at(4 * TICKS_PER_BEAT).unwrap().name.as_deref(), Some("F"));
        assert!(t.chord_at(8 * TICKS_PER_BEAT).is_none());
        assert!(t.chord_at(-TICKS_PER_BEAT).is_none());
    }

    #[test]
    fn empty_track_is_none_everywhere() {
        let t = ChordTrack::default();
        assert!(t.is_empty());
        assert!(t.chord_at(0).is_none());
    }

    #[test]
    fn overlap_later_start_wins() {
        let mut t = ChordTrack::new(vec![c()]);
        t.add(ChordEvent::new(2 * TICKS_PER_BEAT, 4 * TICKS_PER_BEAT, vec![7, 11, 2], Some("G".into())));
        assert_eq!(t.chord_at(3 * TICKS_PER_BEAT).unwrap().name.as_deref(), Some("G"));
        assert_eq!(t.chord_at(TICKS_PER_BEAT).unwrap().name.as_deref(), Some("C"));
    }

    #[test]
    fn update_and_remove() {
        let mut t = ChordTrack::new(vec![c(), f()]);
        let id = t.chords()[1].id;
        let mut moved = t.get(&id).unwrap().clone();
        moved.start_tick = 8 * TICKS_PER_BEAT;
        t.update(moved);
        assert!(t.chord_at(5 * TICKS_PER_BEAT).is_none());
        assert_eq!(t.chord_at(9 * TICKS_PER_BEAT).unwrap().name.as_deref(), Some("F"));
        t.remove(&id);
        assert_eq!(t.chords().len(), 1);
        assert_eq!(t.end_tick(), 4 * TICKS_PER_BEAT);
    }
}
