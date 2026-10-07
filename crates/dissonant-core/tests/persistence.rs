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
    model.clips = vec![Clip::new(pid, 0.0, 16.0), Clip::new(pid, 16.0, 8.0)];
    model.sections = vec![Section::new("intro", 0.0)];

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
    // The legacy arrangement list becomes clips; unknown pattern ids are dropped.
    assert!(decoded.arrangement.is_empty());
    assert_eq!(decoded.clips.len(), 1);
    assert_eq!(decoded.clips[0].start_beat, 0.0);
    assert_eq!(decoded.clips[0].length_beats, 16.0);
}

#[test]
fn legacy_arrangement_lays_clips_back_to_back() {
    let json = r#"{
      "patterns": [ { "id": "22222222-2222-2222-2222-222222222222", "name": "a", "lengthBeats": 8 },
                    { "id": "33333333-3333-3333-3333-333333333333", "name": "b", "lengthBeats": 16 } ],
      "arrangement": [ "22222222-2222-2222-2222-222222222222", "33333333-3333-3333-3333-333333333333", "22222222-2222-2222-2222-222222222222" ]
    }"#;
    let m = ProjectModel::from_json(json).unwrap();
    let starts: Vec<f64> = m.clips.iter().map(|c| c.start_beat).collect();
    assert_eq!(starts, vec![0.0, 8.0, 24.0]);
    assert_eq!(m.song_length(), 32.0);
    assert!(!m.to_json().unwrap().contains("\"arrangement\""));
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

// ---------------------------------------------------------------- the starter project

/// Every melody note's tier against the chord under it, with the starter's locked key.
fn melody_tiers(model: &ProjectModel) -> Vec<Tier> {
    let p = &model.patterns[0];
    let scale = model.key.scale_pitch_classes();
    p.notes(&model.tracks[0].id)
        .iter()
        .map(|n| {
            let chord = p.chords.chord_at(n.start_beat).expect("every melody note sits on a chord");
            TierClassifier.tier(n.pitch, &chord.pitch_classes, scale.as_deref())
        })
        .collect()
}

#[test]
fn starter_has_a_melody_a_groove_and_a_locked_key() {
    let m = ProjectModel::starter();
    assert_eq!(m.key, KeyState::locked(0, ScaleType::Major));
    let p = &m.patterns[0];
    assert_eq!(p.length_beats, 16.0);
    assert_eq!(p.chords.chords().iter().map(|c| c.start_beat).collect::<Vec<_>>(), vec![0.0, 4.0, 8.0, 12.0]);
    assert!(!m.tracks[0].is_drum && m.tracks[1].is_drum);
    assert!(!p.notes(&m.tracks[0].id).is_empty(), "the melody track plays something");
    let hits = p.notes(&m.tracks[1].id);
    let count = |pitch| hits.iter().filter(|n| n.pitch == pitch).count();
    assert_eq!((count(36), count(38), count(42)), (8, 8, 32), "kick on 1 and 3, snare on 2 and 4, 8th hats, 4 bars");
    assert_eq!(m.clips.len(), 1, "the pattern is already in the song");

    let tiers = melody_tiers(&m);
    assert!(tiers.iter().all(|t| *t != Tier::Dissonance), "the starter melody is never flagged: {tiers:?}");
    assert!(tiers.contains(&Tier::ChordTone) && tiers.contains(&Tier::Tension), "it shows more than one tier");
}

#[test]
fn starter_shaped_fits_any_length_and_meter() {
    for (length, bar) in [(32.0, 4.0), (8.0, 4.0), (12.0, 3.0), (14.0, 3.5), (8.0, 2.0)] {
        let m = ProjectModel::starter_shaped(length, bar);
        let p = &m.patterns[0];
        assert_eq!(p.length_beats, length);
        assert_eq!(m.clips[0].length_beats, length);
        assert_eq!(p.chords.chords().len(), 4, "{length}/{bar}: one pass of the progression");
        assert_eq!(p.chords.chords().last().unwrap().end_beat(), length);
        for n in p.notes_by_track.values().flatten() {
            assert!(n.start_beat >= 0.0 && n.end_beat() <= length + 1e-9, "{length}/{bar}: note past the end: {n:?}");
        }
        assert!(melody_tiers(&m).iter().all(|t| *t != Tier::Dissonance), "{length}/{bar}: the melody keeps its tiers");
        let hits = p.notes(&m.tracks[1].id);
        let kicks: Vec<f64> = hits.iter().filter(|n| n.pitch == 36).map(|n| n.start_beat % bar).collect();
        assert!(kicks.contains(&0.0), "{length}/{bar}: a kick on every downbeat");
        assert!(hits.iter().any(|n| n.pitch == 38), "{length}/{bar}: there is a snare");
    }
    // Nonsense input falls back to the default shape instead of an empty pattern.
    assert_eq!(ProjectModel::starter_shaped(0.0, f64::NAN).patterns[0].length_beats, 16.0);
}

// ---------------------------------------------------------------- vibes

#[test]
fn every_vibe_builds_a_playable_project() {
    let infos = vibes();
    assert_eq!(infos.len(), 5);
    for info in &infos {
        let m = ProjectModel::from_vibe(&info.id).unwrap_or_else(|| panic!("{} builds", info.id));
        assert_eq!(m.tempo, info.tempo, "{}", info.id);
        assert_eq!(m.key, info.key, "{}: key locked as advertised", info.id);
        assert!(m.key.is_locked);
        let p = &m.patterns[0];
        assert_eq!(p.length_beats, 16.0);
        assert_eq!(m.clips.len(), 1);
        assert!(p.notes(&m.tracks[0].id).is_empty(), "{}: the lead is left for the player", info.id);
        for n in p.notes_by_track.values().flatten() {
            assert!(n.start_beat >= 0.0 && n.end_beat() <= 16.0 + 1e-9, "{}: {n:?} past the end", info.id);
        }
        // Every bass note is the root of the chord it sits on, low.
        let bass = p.notes(&m.tracks[1].id);
        assert!(!bass.is_empty());
        for n in bass {
            let chord = p.chords.chord_at(n.start_beat).unwrap();
            assert_eq!(n.pitch.rem_euclid(12), chord.pitch_classes[0].rem_euclid(12), "{}: bass plays roots", info.id);
            assert!((32..=43).contains(&n.pitch), "{}: bass pitch {} out of range", info.id, n.pitch);
        }
        // A kick on every downbeat, whatever the groove.
        let kicks: Vec<f64> = p.notes(&m.tracks[2].id).iter().filter(|n| n.pitch == 36).map(|n| n.start_beat).collect();
        for bar in 0..4 {
            assert!(kicks.contains(&(bar as f64 * 4.0)), "{}: kick on bar {bar}", info.id);
        }
    }
    assert!(ProjectModel::from_vibe("polka").is_none());
}

/// The browser mock (ui/src/bridge.ts) mirrors the vibes; `bridge.test.ts` asserts these same
/// values, so a change on either side fails a test.
#[test]
fn vibe_values_shared_with_the_ui() {
    let described: Vec<String> = vibes()
        .iter()
        .map(|info| {
            let m = ProjectModel::from_vibe(&info.id).unwrap();
            let p = &m.patterns[0];
            let names: Vec<&str> = m.tracks.iter().map(|t| t.name.as_str()).collect();
            let chords: Vec<&str> = p.chords.chords().iter().map(|c| c.name.as_deref().unwrap_or("")).collect();
            let count = |track: usize, pitch: Option<i32>| p.notes(&m.tracks[track].id).iter().filter(|n| pitch.is_none_or(|x| n.pitch == x)).count();
            format!(
                "{}|{}|{}|{}|{}|{}|kick {}|bass {}|{}",
                info.id,
                info.tempo,
                m.swing,
                names.join(","),
                chords.join(" "),
                m.tracks[0].voice,
                count(2, Some(36)),
                count(1, None),
                count(2, None),
            )
        })
        .collect();
    assert_eq!(
        described,
        [
            "lofi|80|60|keys,bass,drums|Gm C F Dm|triangle|kick 12|bass 12|52",
            "postpunk|148|50|lead,bass,drums|Em C G D|square|kick 12|bass 32|52",
            "ambient|70|50|pad,bass,drums|D G|pad|kick 4|bass 4|8",
            "songwriter|96|50|melody,bass,drums|G D Em C|pluck|kick 8|bass 8|48",
            "club|124|50|lead,bass,drums|Am G F G|square|kick 16|bass 16|40",
        ]
    );
}
