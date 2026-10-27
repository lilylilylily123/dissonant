//! What the MIDI recorder writes, and what it refuses to invent.
//!
//! The recorder used to fold a song position into the pattern with a modulo, erase an earlier
//! pass over the same loop, and leave one undo step per note. All three made the file disagree
//! with the performance, which is the only thing a recorder is for.

use dissonant_app_lib::{place_recorded_note, PlayMode, Placement, Take};
use dissonant_core::{Clip, Document, NoteEvent, ProjectModel, TICKS_PER_BEAT};

const BAR: i64 = 4 * TICKS_PER_BEAT;
const SIXTEENTH: i64 = TICKS_PER_BEAT / 4;

/// One four-bar pattern, placed on the song as a clip four bars long at `at`.
fn song_with_clip(at: i64) -> ProjectModel {
    let mut model = ProjectModel::empty();
    model.patterns[0].length_ticks = 4 * BAR;
    let pattern_id = model.patterns[0].id;
    model.clips.push(Clip::new(pattern_id, at, 4 * BAR));
    model
}

#[test]
fn a_take_over_a_clip_at_bar_17_lands_where_it_was_played() {
    // Bar 17 is clip tick 0; the note falls one bar and one beat into the clip.
    let model = song_with_clip(16 * BAR);
    let start = (16 * BAR + BAR + TICKS_PER_BEAT) as f64;
    let placed = place_recorded_note(
        &model,
        PlayMode::Song,
        &model.patterns[0],
        start,
        start + TICKS_PER_BEAT as f64,
        Some(SIXTEENTH),
    );
    assert_eq!(
        placed,
        Placement::At {
            start_tick: BAR + TICKS_PER_BEAT,
            length_ticks: TICKS_PER_BEAT
        },
        "the song tick did not resolve through the clip that is playing"
    );
}

#[test]
fn a_clip_that_starts_the_pattern_part_way_in_records_there() {
    // A clip offset by two bars is heard from the pattern's bar 3, so that is where a note
    // played on its downbeat belongs.
    let mut model = song_with_clip(8 * BAR);
    model.clips[0].offset_ticks = 2 * BAR;
    let start = (8 * BAR) as f64;
    let placed = place_recorded_note(&model, PlayMode::Song, &model.patterns[0], start, start + 240.0, Some(SIXTEENTH));
    assert_eq!(placed, Placement::At { start_tick: 2 * BAR, length_ticks: 240 });
}

#[test]
fn a_note_played_where_the_pattern_is_not_placed_is_dropped_not_folded() {
    let model = song_with_clip(16 * BAR);
    // Bar 1: the clip does not start for another sixteen bars. The old modulo filed this at
    // pattern tick 0, on top of whatever is already there.
    let placed = place_recorded_note(&model, PlayMode::Song, &model.patterns[0], 0.0, 480.0, Some(SIXTEENTH));
    assert_eq!(placed, Placement::NotPlaying);
}

#[test]
fn pattern_mode_still_records_against_the_pattern_itself() {
    // No clip anywhere: in pattern mode the engine tick is already pattern time.
    let mut model = ProjectModel::empty();
    model.patterns[0].length_ticks = 4 * BAR;
    let placed = place_recorded_note(
        &model,
        PlayMode::Pattern,
        &model.patterns[0],
        (2 * BAR) as f64,
        (2 * BAR + 480) as f64,
        Some(SIXTEENTH),
    );
    assert_eq!(placed, Placement::At { start_tick: 2 * BAR, length_ticks: 480 });
}

#[test]
fn without_a_grid_the_take_keeps_the_feel_it_was_played_with() {
    let mut model = ProjectModel::empty();
    model.patterns[0].length_ticks = 4 * BAR;
    // 37 ticks late off the beat, and a hair under a quarter long.
    let placed = place_recorded_note(&model, PlayMode::Pattern, &model.patterns[0], 997.0, 1900.4, None);
    assert_eq!(placed, Placement::At { start_tick: 997, length_ticks: 903 });

    // The same performance on a sixteenth grid is pulled onto it.
    let snapped = place_recorded_note(&model, PlayMode::Pattern, &model.patterns[0], 997.0, 1900.4, Some(SIXTEENTH));
    assert_eq!(snapped, Placement::At { start_tick: 960, length_ticks: 960 });
}

#[test]
fn a_second_pass_over_the_loop_layers_onto_the_first() {
    let mut model = ProjectModel::empty();
    model.patterns[0].length_ticks = 4 * BAR;
    let pattern_id = model.patterns[0].id;
    let mut doc = Document::new(model);

    // Pass one: a note on the downbeat.
    let mut take = Take::default();
    assert!(take.write(&mut doc, pattern_id, NoteEvent::new(0, 480, 60), SIXTEENTH / 2));
    take.finish(&mut doc);

    // Pass two over the same loop plays the same key in the same slot. It is a new layer, not
    // a correction of the first — the old dedupe swallowed the whole earlier pass.
    let mut take = Take::default();
    assert!(take.write(&mut doc, pattern_id, NoteEvent::new(0, 480, 60), SIXTEENTH / 2));
    take.finish(&mut doc);

    let notes = &doc.model().pattern(&pattern_id).unwrap().notes;
    assert_eq!(notes.len(), 2, "the second pass erased the first instead of stacking");

    // Within one pass a double trigger on the same slot still collapses to one note.
    let mut take = Take::default();
    assert!(take.write(&mut doc, pattern_id, NoteEvent::new(0, 480, 60), SIXTEENTH / 2));
    assert!(take.write(&mut doc, pattern_id, NoteEvent::new(2, 480, 60), SIXTEENTH / 2));
    take.finish(&mut doc);
    assert_eq!(doc.model().pattern(&pattern_id).unwrap().notes.len(), 3);
}

#[test]
fn a_twelve_note_take_is_one_undo_step() {
    let mut model = ProjectModel::empty();
    model.patterns[0].length_ticks = 4 * BAR;
    let pattern_id = model.patterns[0].id;
    let mut doc = Document::new(model);
    let before = doc.model().clone();

    let mut take = Take::default();
    for i in 0..12 {
        let note = NoteEvent::new(i * SIXTEENTH, SIXTEENTH, 60 + i as i32);
        assert!(take.write(&mut doc, pattern_id, note, SIXTEENTH / 2));
    }
    take.finish(&mut doc);

    assert_eq!(doc.model().pattern(&pattern_id).unwrap().notes.len(), 12);
    assert_eq!(doc.undo_label(), Some("record"));
    assert!(doc.undo(), "the take left nothing to undo");
    assert_eq!(
        doc.model().pattern(&pattern_id).unwrap().notes.len(),
        0,
        "one undo took back part of the take, not the take"
    );
    assert_eq!(doc.model(), &before);
    assert!(!doc.can_undo(), "the take left more than one undo step behind");
}
