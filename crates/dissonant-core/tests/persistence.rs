use dissonant_core::*;

#[test]
fn round_trip_preserves_all_state() {
    let mut model = ProjectModel::starter();
    let tid = model.tracks[0].id;
    let pid = model.patterns[0].id;
    model.tempo = 96.0;
    model.key = KeyState::locked(0, ScaleType::Major);
    model.tracks[0].soloed = true;
    model.tracks[0].pan = -0.3;
    model.master.reverb_wet = 0.25;
    model.patterns[0]
        .notes_by_track
        .insert(tid, vec![NoteEvent::new(0.0, 1.0, 60).with_velocity(77)]);
    model.arrangement = vec![pid, pid];

    let json = model.to_json().unwrap();
    let decoded = ProjectModel::from_json(&json).unwrap();
    assert_eq!(decoded, model);
}

#[test]
fn empty_project_defaults() {
    let empty = ProjectModel::empty();
    assert_eq!(empty.tracks.len(), 1);
    assert_eq!(empty.patterns.len(), 1);
    assert!(empty.patterns[0].notes_by_track.is_empty());
    assert!(empty.patterns[0].chords.is_empty());
    assert_eq!(empty.tempo, 120.0);
    assert_eq!(empty.key, KeyState::NONE);
}

#[test]
fn decoding_missing_fields_does_not_fail() {
    let decoded = ProjectModel::from_json(r#"{ "tempo": 140 }"#).unwrap();
    assert_eq!(decoded.tempo, 140.0);
    assert_eq!(decoded.tracks.len(), 1);
    assert_eq!(decoded.patterns.len(), 1);
    assert_eq!(decoded.schema_version, SCHEMA_VERSION);

    let decoded = ProjectModel::from_json("{}").unwrap();
    assert_eq!(decoded.tempo, 120.0);
    assert_eq!(decoded.master, MasterSettings::default());
}

#[test]
fn notes_without_velocity_default_to_100_and_bad_values_are_clamped() {
    let track = "11111111-1111-1111-1111-111111111111";
    let json = format!(
        r#"{{
          "tracks": [ {{ "id": "{track}", "name": "lead" }} ],
          "patterns": [ {{
            "id": "22222222-2222-2222-2222-222222222222", "name": "p",
            "chords": {{ "chords": [ {{ "id": "33333333-3333-3333-3333-333333333333", "startBeat": 0, "lengthBeats": 4, "pitchClasses": [0,4,7], "name": "C" }} ] }},
            "notesByTrack": {{ "{track}": [ {{ "id": "44444444-4444-4444-4444-444444444444", "startBeat": 1, "lengthBeats": 1, "pitch": 300 }} ] }}
          }} ],
          "arrangement": [ "22222222-2222-2222-2222-222222222222", "99999999-9999-9999-9999-999999999999" ]
        }}"#
    );
    let decoded = ProjectModel::from_json(&json).unwrap();
    let tid = decoded.tracks[0].id;
    let notes = decoded.patterns[0].notes(&tid);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].velocity, 100);
    assert_eq!(notes[0].pitch, 127);
    assert_eq!(decoded.tracks[0].voice, "saw");
    assert_eq!(decoded.tracks[0].tone, 18_000.0);
    assert_eq!(decoded.patterns[0].chords.chord_at(1.0).unwrap().name.as_deref(), Some("C"));
    // Unknown pattern ids are dropped from the arrangement.
    assert_eq!(decoded.arrangement.len(), 1);
}

#[test]
fn json_uses_camel_case_keys() {
    let json = ProjectModel::starter().to_json().unwrap();
    assert!(json.contains("\"schemaVersion\""));
    assert!(json.contains("\"notesByTrack\""));
    assert!(json.contains("\"lengthBeats\""));
    assert!(!json.contains("length_beats"));
}

#[test]
fn imports_swift_flat_array_notes_by_track() {
    // The Swift app's Codable wrote `[UUID: [NoteEvent]]` as [key, value, key, value].
    let json = r#"{
      "schemaVersion": 2, "tempo": 110,
      "key": { "rootPitchClass": 7, "scale": "major", "isLocked": true },
      "tracks": [ { "id": "11111111-1111-1111-1111-111111111111", "name": "melody", "voice": "pluck", "muted": false, "soloed": false, "isDrum": false, "volume": 1, "reverbSend": 0.2, "tone": 9000, "pan": 0 } ],
      "patterns": [ { "id": "22222222-2222-2222-2222-222222222222", "name": "verse", "lengthBeats": 16,
        "chords": { "chords": [] },
        "notesByTrack": [ "11111111-1111-1111-1111-111111111111", [ { "id": "44444444-4444-4444-4444-444444444444", "startBeat": 2, "lengthBeats": 0.5, "pitch": 67 } ] ] } ],
      "arrangement": [ "22222222-2222-2222-2222-222222222222" ]
    }"#;
    let m = ProjectModel::from_json(json).unwrap();
    let tid = m.tracks[0].id;
    assert_eq!(m.patterns[0].notes(&tid).len(), 1);
    assert_eq!(m.patterns[0].notes(&tid)[0].pitch, 67);
    assert_eq!(m.patterns[0].notes(&tid)[0].velocity, 100);
    assert_eq!(m.key.root_pitch_class, Some(7));
    assert_eq!(m.time_signature, TimeSignature::default());
    assert_eq!(m.schema_version, SCHEMA_VERSION);
}
