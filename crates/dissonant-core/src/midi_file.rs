//! Standard MIDI files: export a pattern or the whole song (one MIDI track per Dissonant
//! track, chord names as markers), and import a file into a pattern as new tracks.

use crate::arrangement::Arrangement;
use crate::model::{NoteEvent, ProjectModel, Track, MIN_NOTE_TICKS};
use crate::ticks::TICKS_PER_BEAT;
use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use thiserror::Error;
use uuid::Uuid;

/// Ticks per quarter note written and assumed on import when the file says nothing. This is
/// the file format's own resolution, not the project's: 960 % 480 == 0, so both directions are
/// exact and a round trip loses nothing.
pub const PPQ: u16 = 480;

#[derive(Debug, Error)]
pub enum MidiError {
    #[error("no such pattern")]
    NoSuchPattern,
    #[error("nothing to export")]
    Empty,
    #[error("not a MIDI file: {0}")]
    Parse(String),
    #[error("could not write MIDI: {0}")]
    Write(String),
}

/// What to export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiScope {
    Pattern(Uuid),
    Song,
}

/// One track read from a MIDI file, ready to become a Dissonant track.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedTrack {
    pub name: String,
    pub is_drum: bool,
    pub notes: Vec<NoteEvent>,
}

/// Everything an import yields; `length_ticks` is the end of the last note rounded up to a bar.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedMidi {
    pub tracks: Vec<ImportedTrack>,
    pub tempo_bpm: Option<f64>,
    pub length_ticks: i64,
}

/// A project tick on the file's own grid.
fn smf_tick(tick: i64) -> u64 {
    (tick * PPQ as i64 / TICKS_PER_BEAT).max(0) as u64
}

/// A file tick (at `ppq`) on the project's grid.
fn project_tick(tick: u64, ppq: i64) -> i64 {
    tick as i64 * TICKS_PER_BEAT / ppq
}

type TrackNotes<'a> = Vec<(&'a Track, Vec<NoteEvent>)>;
type ChordMarkers = Vec<(i64, String)>;

/// Build the SMF (format 1) for a scope. Track 0 carries tempo, time signature and chord
/// markers; every Dissonant track becomes one MIDI track (drums on channel 10).
pub fn export(model: &ProjectModel, scope: MidiScope) -> Result<Vec<u8>, MidiError> {
    // Gather (track, notes) and the chord list first; names must outlive the Smf.
    let (per_track, chords): (TrackNotes, ChordMarkers) = match scope {
        MidiScope::Pattern(id) => {
            let p = model.pattern(&id).ok_or(MidiError::NoSuchPattern)?;
            // Export what the pattern plays, repeats and all, not just the first turn.
            let chords = p.looped_chords();
            (
                model.tracks.iter().map(|t| (t, p.looped_notes(&t.id))).collect(),
                chords.chords().iter().filter_map(|c| c.name.clone().map(|n| (c.start_tick, n))).collect(),
            )
        }
        MidiScope::Song => (
            model
                .tracks
                .iter()
                .map(|t| (t, Arrangement::flattened_notes(&t.id, &model.patterns, &model.clips)))
                .collect(),
            Arrangement::flattened_chords(&model.patterns, &model.clips)
                .chords()
                .iter()
                .filter_map(|c| c.name.clone().map(|n| (c.start_tick, n)))
                .collect(),
        ),
    };
    if per_track.iter().all(|(_, n)| n.is_empty()) {
        return Err(MidiError::Empty);
    }

    let names: Vec<String> = per_track.iter().map(|(t, _)| t.name.clone()).collect();
    let marker_names: Vec<String> = chords.iter().map(|(_, n)| n.clone()).collect();
    let mut smf = Smf::new(Header::new(Format::Parallel, Timing::Metrical(u15::new(PPQ))));

    // Conductor track.
    let mut conductor: Vec<(u64, TrackEventKind)> = vec![
        (0, TrackEventKind::Meta(MetaMessage::Tempo(u24::new((60_000_000.0 / model.tempo).round() as u32)))),
        (
            0,
            TrackEventKind::Meta(MetaMessage::TimeSignature(
                model.time_signature.numerator as u8,
                (model.time_signature.denominator as f64).log2() as u8,
                24,
                8,
            )),
        ),
    ];
    for ((tick, _), name) in chords.iter().zip(marker_names.iter()) {
        conductor.push((smf_tick(*tick), TrackEventKind::Meta(MetaMessage::Marker(name.as_bytes()))));
    }
    smf.tracks.push(to_deltas(conductor));

    for ((track, notes), name) in per_track.iter().zip(names.iter()) {
        let mut events: Vec<(u64, TrackEventKind)> = vec![(0, TrackEventKind::Meta(MetaMessage::TrackName(name.as_bytes())))];
        for n in notes.iter().filter(|n| !n.muted) {
            // A note's colour group is its MIDI channel, the way FL Studio treats colour.
            // Drums stay on channel 10 whatever colour they carry.
            let channel = u4::new(if track.is_drum { 9 } else { n.color.min(15) });
            let key = u7::new(n.pitch.clamp(0, 127) as u8);
            let vel = u7::new(n.velocity.clamp(1, 127) as u8);
            let on = smf_tick(n.shifted_start());
            let off = smf_tick(n.shifted_start() + n.length_ticks).max(on + 1);
            events.push((on, TrackEventKind::Midi { channel, message: MidiMessage::NoteOn { key, vel } }));
            events.push((off, TrackEventKind::Midi { channel, message: MidiMessage::NoteOff { key, vel: u7::new(0) } }));
        }
        smf.tracks.push(to_deltas(events));
    }

    let mut out = Vec::new();
    smf.write_std(&mut out).map_err(|e| MidiError::Write(e.to_string()))?;
    Ok(out)
}

/// Sort absolute-tick events (note-offs before note-ons at the same tick) and convert to deltas.
fn to_deltas(mut events: Vec<(u64, TrackEventKind<'_>)>) -> Vec<TrackEvent<'_>> {
    let rank = |k: &TrackEventKind| match k {
        TrackEventKind::Meta(_) => 0,
        TrackEventKind::Midi { message: MidiMessage::NoteOff { .. }, .. } => 1,
        _ => 2,
    };
    events.sort_by(|a, b| a.0.cmp(&b.0).then(rank(&a.1).cmp(&rank(&b.1))));
    let mut last = 0u64;
    let mut out: Vec<TrackEvent> = events
        .into_iter()
        .map(|(at, kind)| {
            let delta = at - last;
            last = at;
            TrackEvent { delta: u28::new(delta.min(u28::max_value().as_int() as u64) as u32), kind }
        })
        .collect();
    out.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
    out
}

/// The channel a note arrived on becomes its colour group, so an export/import round trip
/// keeps the grouping. Channel 10 is percussion and carries no colour.
fn colored(mut note: NoteEvent, channel: u8) -> NoteEvent {
    note.color = if channel == 9 { 0 } else { channel.min(15) };
    note
}

/// Read a MIDI file into tracks of notes. Format 0 files (one track, many channels) are split
/// by channel; tracks without notes are dropped.
pub fn import(bytes: &[u8], bar_ticks: i64) -> Result<ImportedMidi, MidiError> {
    let smf = Smf::parse(bytes).map_err(|e| MidiError::Parse(e.to_string()))?;
    let ppq = match smf.header.timing {
        Timing::Metrical(t) => t.as_int() as i64,
        Timing::Timecode(..) => PPQ as i64,
    }
    .max(1);
    let mut tempo_bpm = None;
    let mut tracks = Vec::new();
    for (ti, track) in smf.tracks.iter().enumerate() {
        let mut at = 0u64;
        let mut name: Option<String> = None;
        // Per (channel, key): start tick + velocity, for pairing note-offs.
        let mut held: std::collections::HashMap<(u8, u8), (u64, u8)> = std::collections::HashMap::new();
        let mut per_channel: std::collections::BTreeMap<u8, Vec<NoteEvent>> = std::collections::BTreeMap::new();
        for ev in track {
            at += ev.delta.as_int() as u64;
            match ev.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(n)) => name = Some(String::from_utf8_lossy(n).trim().to_string()),
                TrackEventKind::Meta(MetaMessage::Tempo(t)) if tempo_bpm.is_none() => {
                    tempo_bpm = Some(60_000_000.0 / t.as_int() as f64);
                }
                TrackEventKind::Midi { channel, message } => {
                    let ch = channel.as_int();
                    match message {
                        MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            held.insert((ch, key.as_int()), (at, vel.as_int()));
                        }
                        MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                            if let Some((start, vel)) = held.remove(&(ch, key.as_int())) {
                                let start_tick = project_tick(start, ppq);
                                let len = project_tick(at - start, ppq).max(MIN_NOTE_TICKS);
                                per_channel.entry(ch).or_default().push(
                                    colored(NoteEvent::new(start_tick, len, key.as_int() as i32), ch)
                                        .with_velocity(vel as i32),
                                );
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        // Anything still held at the end gets a beat.
        for ((ch, key), (start, vel)) in held {
            per_channel.entry(ch).or_default().push(
                colored(NoteEvent::new(project_tick(start, ppq), TICKS_PER_BEAT, key as i32), ch).with_velocity(vel as i32),
            );
        }
        let multi = per_channel.len() > 1;
        for (ch, mut notes) in per_channel {
            notes.sort_by(|a, b| a.start_tick.cmp(&b.start_tick).then(a.pitch.cmp(&b.pitch)));
            let base = name.clone().unwrap_or_else(|| format!("midi {}", ti + 1));
            tracks.push(ImportedTrack {
                name: if multi { format!("{base} ch{}", ch + 1) } else { base },
                is_drum: ch == 9,
                notes,
            });
        }
    }
    if tracks.is_empty() {
        return Err(MidiError::Empty);
    }
    let end = tracks
        .iter()
        .flat_map(|t| t.notes.iter().map(NoteEvent::end_tick))
        .max()
        .unwrap_or(0);
    let bar = bar_ticks.max(1);
    let length_ticks = ((end + bar - 1) / bar * bar).max(bar);
    Ok(ImportedMidi { tracks, tempo_bpm, length_ticks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Clip;

    const BEAT: i64 = TICKS_PER_BEAT;
    const BAR: i64 = 4 * BEAT;

    #[test]
    fn export_then_import_round_trips_notes_and_names() {
        let mut model = ProjectModel::starter();
        model.tempo = 100.0;
        let (tid, pid) = (model.tracks[0].id, model.patterns[0].id);
        let drums = model.tracks[1].id;
        model.tracks[0].name = "lead".into();
        let mut muted = NoteEvent::new(3 * BEAT, BEAT, 72);
        muted.muted = true;
        model.patterns[0].notes_by_track.insert(
            tid,
            vec![
                NoteEvent::new(0, BEAT / 2, 60).with_velocity(90),
                NoteEvent::new(3 * BEAT / 2, BEAT, 67),
                muted,
            ],
        );
        model.patterns[0].notes_by_track.insert(drums, vec![NoteEvent::new(0, BEAT / 4, 36)]);

        let bytes = export(&model, MidiScope::Pattern(pid)).unwrap();
        let back = import(&bytes, BAR).unwrap();
        assert_eq!(back.tempo_bpm.map(|t| t.round()), Some(100.0));
        assert_eq!(back.tracks.len(), 2);
        let lead = back.tracks.iter().find(|t| t.name == "lead").unwrap();
        assert!(!lead.is_drum);
        assert_eq!(lead.notes.len(), 2, "muted notes are not exported");
        assert_eq!(lead.notes[0].pitch, 60);
        assert_eq!(lead.notes[0].velocity, 90);
        assert_eq!(lead.notes[0].length_ticks, BEAT / 2, "960 → 480 → 960 is exact");
        assert_eq!(lead.notes[1].start_tick, 3 * BEAT / 2);
        let kit = back.tracks.iter().find(|t| t.name == "drums").unwrap();
        assert!(kit.is_drum, "channel 10 marks drums");
        assert_eq!(back.length_ticks, BAR);
    }

    #[test]
    fn song_export_follows_clips_and_empty_is_an_error() {
        let mut model = ProjectModel::starter();
        let (tid, pid) = (model.tracks[0].id, model.patterns[0].id);
        model.patterns[0].notes_by_track.insert(tid, vec![NoteEvent::new(0, BEAT, 60)]);
        model.clips = vec![Clip::new(pid, 0, 16 * BEAT), Clip::new(pid, 16 * BEAT, 16 * BEAT)];
        let back = import(&export(&model, MidiScope::Song).unwrap(), BAR).unwrap();
        let lead = &back.tracks[0];
        assert_eq!(lead.notes.len(), 2);
        assert_eq!(lead.notes[1].start_tick, 16 * BEAT);
        assert_eq!(back.length_ticks, 20 * BEAT, "last note ends at beat 17 → rounded up to the bar");

        let empty = ProjectModel::empty();
        assert!(matches!(export(&empty, MidiScope::Song), Err(MidiError::Empty)));
        assert!(matches!(import(b"not midi", BAR), Err(MidiError::Parse(_))));
    }

    #[test]
    fn colour_groups_round_trip_as_midi_channels_and_shift_moves_the_export() {
        let mut model = ProjectModel::starter();
        let (tid, pid) = (model.tracks[0].id, model.patterns[0].id);
        model.tracks[0].name = "lead".into();
        let mut red = NoteEvent::new(0, BEAT, 60);
        red.color = 3;
        let mut late = NoteEvent::new(2 * BEAT, BEAT, 64);
        late.shift_ticks = BEAT / 4;
        model.patterns[0].notes_by_track.insert(tid, vec![red, late]);
        model.patterns[0].notes_by_track.remove(&model.tracks[1].id);

        let back = import(&export(&model, MidiScope::Pattern(pid)).unwrap(), BAR).unwrap();
        // Two colour groups on one track come back as one track per channel.
        assert_eq!(back.tracks.len(), 2);
        let coloured = back.tracks.iter().find(|t| t.notes[0].pitch == 60).unwrap();
        assert_eq!(coloured.notes[0].color, 3, "channel 4 is colour group 3");
        let shifted = back.tracks.iter().find(|t| t.notes[0].pitch == 64).unwrap();
        assert_eq!(
            shifted.notes[0].start_tick,
            9 * BEAT / 4,
            "shift is baked into the exported time, not dropped"
        );
    }
}
