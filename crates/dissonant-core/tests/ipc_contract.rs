//! The UI↔core wire contract. `ui/src/types.ts` declares `Command` as a camelCase tagged
//! union; this file pins every variant's JSON exactly as the UI sends it over Tauri IPC, so a
//! field-name drift fails here instead of silently rejecting edits at runtime.

use dissonant_core::document::{Command, TrackParam};
use serde_json::json;
use uuid::Uuid;

fn parse(v: serde_json::Value) -> Command {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("UI payload rejected: {v} — {e}"))
}

#[test]
fn every_command_variant_round_trips_from_the_ui_payload() {
    let id = Uuid::new_v4();
    let pattern_id = Uuid::new_v4();
    let track_id = Uuid::new_v4();

    assert_eq!(parse(json!({"type": "setTempo", "bpm": 128.0})), Command::SetTempo { bpm: 128.0 });

    assert!(matches!(
        parse(json!({"type": "setKey", "key": {"rootPitchClass": 2, "scale": "minor", "isLocked": true}})),
        Command::SetKey { .. }
    ));

    assert_eq!(parse(json!({"type": "addTrack", "isDrum": true})), Command::AddTrack { is_drum: true });
    assert_eq!(parse(json!({"type": "deleteTrack", "id": id})), Command::DeleteTrack { id });
    assert_eq!(
        parse(json!({"type": "renameTrack", "id": id, "name": "bass"})),
        Command::RenameTrack { id, name: "bass".into() }
    );
    assert_eq!(
        parse(json!({"type": "setTrackMuted", "id": id, "muted": true})),
        Command::SetTrackMuted { id, muted: true }
    );
    assert_eq!(
        parse(json!({"type": "setTrackSoloed", "id": id, "soloed": true})),
        Command::SetTrackSoloed { id, soloed: true }
    );
    assert_eq!(parse(json!({"type": "moveTrack", "id": id, "up": false})), Command::MoveTrack { id, up: false });
    assert_eq!(
        parse(json!({"type": "setTrackVoice", "id": id, "voice": "pluck"})),
        Command::SetTrackVoice { id, voice: "pluck".into() }
    );
    assert_eq!(
        parse(json!({"type": "setTrackParam", "id": id, "param": "reverbSend", "value": 0.3})),
        Command::SetTrackParam { id, param: TrackParam::ReverbSend, value: 0.3 }
    );
    assert_eq!(
        parse(json!({"type": "setTrackColor", "id": id, "color": "#ff8800"})),
        Command::SetTrackColor { id, color: Some("#ff8800".into()) }
    );
    assert_eq!(parse(json!({"type": "setTrackColor", "id": id, "color": null})), Command::SetTrackColor { id, color: None });

    assert_eq!(parse(json!({"type": "addPattern"})), Command::AddPattern);
    assert_eq!(parse(json!({"type": "duplicatePattern", "id": id})), Command::DuplicatePattern { id });
    assert_eq!(parse(json!({"type": "deletePattern", "id": id})), Command::DeletePattern { id });
    assert_eq!(
        parse(json!({"type": "renamePattern", "id": id, "name": "chorus"})),
        Command::RenamePattern { id, name: "chorus".into() }
    );
    assert_eq!(
        parse(json!({"type": "setPatternLength", "id": id, "beats": 32.0})),
        Command::SetPatternLength { id, beats: 32.0 }
    );

    // The piano roll's commit path — one `setNotes` per gesture.
    let notes = parse(json!({
        "type": "setNotes",
        "patternId": pattern_id,
        "trackId": track_id,
        "notes": [{"id": Uuid::new_v4(), "startBeat": 1.5, "lengthBeats": 0.25, "pitch": 60, "velocity": 100}],
    }));
    match notes {
        Command::SetNotes { pattern_id: p, track_id: t, notes } => {
            assert_eq!((p, t), (pattern_id, track_id));
            assert_eq!(notes.len(), 1);
            assert_eq!(notes[0].start_beat, 1.5);
            assert_eq!(notes[0].length_beats, 0.25);
            assert_eq!(notes[0].pitch, 60);
            assert_eq!(notes[0].velocity, 100);
        }
        other => panic!("expected SetNotes, got {other:?}"),
    }

    let chords = parse(json!({
        "type": "setChords",
        "patternId": pattern_id,
        "chords": [{"id": Uuid::new_v4(), "startBeat": 0.0, "lengthBeats": 4.0, "pitchClasses": [0, 4, 7], "name": "C"}],
    }));
    match chords {
        Command::SetChords { pattern_id: p, chords } => {
            assert_eq!(p, pattern_id);
            assert_eq!(chords[0].pitch_classes, vec![0, 4, 7]);
        }
        other => panic!("expected SetChords, got {other:?}"),
    }

    assert_eq!(
        parse(json!({"type": "setArrangement", "arrangement": [pattern_id]})),
        Command::SetArrangement { arrangement: vec![pattern_id] }
    );

    assert!(matches!(
        parse(json!({
            "type": "setMaster",
            "master": {"gain": 1.0, "reverbWet": 0.0, "lowCutHz": 20.0, "highCutHz": 18000.0, "lowEq": 1.0, "midEq": 1.0, "highEq": 1.0},
        })),
        Command::SetMaster { .. }
    ));
}
