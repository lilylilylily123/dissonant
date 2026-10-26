use dissonant_core::*;

const BEAT: i64 = TICKS_PER_BEAT;

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
        .insert(tid, vec![NoteEvent::new(0, BEAT, 60).with_velocity(77)]);
    model.clips = vec![Clip::new(pid, 0, 16 * BEAT), Clip::new(pid, 16 * BEAT, 8 * BEAT)];
    model.sections = vec![Section::new("intro", 0)];

    let json = model.to_json().unwrap();
    let decoded = ProjectModel::from_json(&json).unwrap();
    assert_eq!(decoded, model);
}

#[test]
fn audio_tracks_clips_and_rigs_round_trip() {
    let mut model = ProjectModel::starter();
    let mut guitar = Track::audio("guitar");
    let rig = guitar.amp.as_mut().expect("an audio track is created with a rig");
    rig.amp.model = AmpModel::Fuzz;
    rig.amp.gain = 9.5;
    rig.cab.model = CabModel::TwoByTwelve;
    rig.input_db = -3.0;
    rig.pedals = vec![
        Pedal::Comp { on: true, sustain: 7.0, level: 5.0 },
        Pedal::Delay { on: true, time_ms: 420.0, feedback: 4.0, mix: 3.0 },
    ];
    let gid = guitar.id;
    model.tracks.push(guitar);

    let source = AudioSource { id: uuid::Uuid::new_v4(), name: "riff.wav".into(), frames: 96_000, sample_rate: 48_000, channels: 2 };
    let mut clip = AudioClip::new(gid, source.id, 8 * BEAT, 2.0);
    clip.gain = 0.7;
    clip.fade_in_seconds = 0.2;
    clip.offset_seconds = 0.5;
    model.audio_sources = vec![source];
    model.audio_clips = vec![clip];

    let decoded = ProjectModel::from_json(&model.to_json().unwrap()).unwrap();
    assert_eq!(decoded, model);
}

#[test]
fn a_project_with_no_audio_writes_no_audio_keys() {
    // Opening and re-saving an old project must not grow it with empty collections.
    let json = ProjectModel::starter().to_json().unwrap();
    assert!(!json.contains("audioClips"), "empty audio clips were written:\n{json}");
    assert!(!json.contains("audioSources"), "empty audio sources were written:\n{json}");
}

#[test]
fn a_file_written_before_audio_tracks_existed_opens_with_no_audio() {
    let v7 = r#"{
        "schemaVersion": 7,
        "tempo": 110,
        "tracks": [{"id": "11111111-1111-1111-1111-111111111111", "name": "lead", "voice": "saw"}],
        "patterns": [{"id": "22222222-2222-2222-2222-222222222222", "name": "p1", "lengthBeats": 16}]
    }"#;
    let m = ProjectModel::from_json(v7).unwrap();
    assert_eq!(m.schema_version, SCHEMA_VERSION);
    assert!(!m.tracks[0].is_audio);
    assert_eq!(m.tracks[0].amp, None);
    assert!(m.audio_clips.is_empty());
    assert!(m.audio_sources.is_empty());
    assert_eq!(m.patterns[0].length_ticks, 16 * BEAT);
}

#[test]
fn loading_drops_audio_clips_whose_track_or_source_is_gone() {
    let mut m = ProjectModel::starter();
    let guitar = Track::audio("guitar");
    let gid = guitar.id;
    m.tracks.push(guitar);
    let source = AudioSource { id: uuid::Uuid::new_v4(), name: "take".into(), frames: 1_000, sample_rate: 48_000, channels: 1 };
    let sid = source.id;
    m.audio_sources = vec![source];
    m.audio_clips = vec![
        AudioClip::new(gid, sid, 0, 1.0),                              // keeper
        AudioClip::new(uuid::Uuid::new_v4(), sid, 0, 1.0),             // no such track
        AudioClip::new(gid, uuid::Uuid::new_v4(), 0, 1.0),             // no such source
        AudioClip::new(m.tracks[0].id, sid, 0, 1.0),                   // not an audio track
        AudioClip::new(gid, sid, 0, 0.0),                              // zero length
    ];
    let decoded = ProjectModel::from_json(&m.to_json().unwrap()).unwrap();
    assert_eq!(decoded.audio_clips.len(), 1);
    assert_eq!(decoded.audio_clips[0].track_id, gid);
    // The source survives even when nothing points at it, so undo can bring a take back
    // without decoding the file again.
    assert_eq!(decoded.audio_sources.len(), 1);
}

#[test]
fn a_hand_edited_audio_track_cannot_also_be_a_drum_track() {
    let mut m = ProjectModel::starter();
    let mut t = Track::audio("guitar");
    t.is_drum = true;
    t.amp = None;
    m.tracks.push(t);
    let decoded = ProjectModel::from_json(&m.to_json().unwrap()).unwrap();
    let guitar = decoded.tracks.iter().find(|t| t.is_audio).unwrap();
    assert!(!guitar.is_drum);
    assert!(guitar.amp.is_some(), "an audio track always carries a rig");
    // …and a note track never carries one.
    assert_eq!(decoded.tracks[0].amp, None);
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
    assert_eq!(empty.patterns[0].length_ticks, 16 * BEAT);
    assert_eq!(empty.swing_grid_ticks, BEAT / 2);
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
    assert_eq!(notes[0].start_tick, BEAT);
    assert_eq!(decoded.tracks[0].voice, "saw");
    assert_eq!(decoded.tracks[0].tone, 18_000.0);
    assert_eq!(decoded.patterns[0].chords.chord_at(BEAT).unwrap().name.as_deref(), Some("C"));
    // The legacy arrangement list becomes clips; unknown pattern ids are dropped.
    assert!(decoded.arrangement.is_empty());
    assert_eq!(decoded.clips.len(), 1);
    assert_eq!(decoded.clips[0].start_tick, 0);
    assert_eq!(decoded.clips[0].length_ticks, 16 * BEAT);
}

#[test]
fn legacy_arrangement_lays_clips_back_to_back() {
    let json = r#"{
      "patterns": [ { "id": "22222222-2222-2222-2222-222222222222", "name": "a", "lengthBeats": 8 },
                    { "id": "33333333-3333-3333-3333-333333333333", "name": "b", "lengthBeats": 16 } ],
      "arrangement": [ "22222222-2222-2222-2222-222222222222", "33333333-3333-3333-3333-333333333333", "22222222-2222-2222-2222-222222222222" ]
    }"#;
    let m = ProjectModel::from_json(json).unwrap();
    let starts: Vec<i64> = m.clips.iter().map(|c| c.start_tick).collect();
    assert_eq!(starts, vec![0, 8 * BEAT, 24 * BEAT]);
    assert_eq!(m.song_length(), 32 * BEAT);
    assert!(!m.to_json().unwrap().contains("\"arrangement\""));
}

#[test]
fn json_uses_camel_case_tick_keys() {
    let json = ProjectModel::starter().to_json().unwrap();
    assert!(json.contains("\"schemaVersion\""));
    assert!(json.contains("\"notesByTrack\""));
    assert!(json.contains("\"lengthTicks\""));
    assert!(json.contains("\"swingGridTicks\""));
    assert!(!json.contains("length_ticks"));
}

/// Every schema ≤ 8 time key folds into its tick field at exactly 960 PPQ, including the
/// fractional ones a quantize grid produces (0.25 beats → 240 ticks).
#[test]
fn a_schema_8_file_migrates_every_beat_field_to_ticks() {
    let track = "11111111-1111-1111-1111-111111111111";
    let pattern = "22222222-2222-2222-2222-222222222222";
    let json = format!(
        r#"{{
          "schemaVersion": 8,
          "tempo": 120,
          "swing": 60,
          "swingGrid": 0.25,
          "tracks": [ {{ "id": "{track}", "name": "lead" }} ],
          "patterns": [ {{
            "id": "{pattern}", "name": "p", "lengthBeats": 8,
            "chords": {{ "chords": [ {{ "id": "33333333-3333-3333-3333-333333333333", "startBeat": 0.5, "lengthBeats": 1.5, "pitchClasses": [0,4,7], "name": "C" }} ] }},
            "notesByTrack": {{ "{track}": [ {{ "id": "44444444-4444-4444-4444-444444444444", "startBeat": 0.25, "lengthBeats": 0.75, "pitch": 60, "shift": -0.125 }} ] }},
            "trackLoopBeats": {{ "{track}": 4 }},
            "chordLoopBeats": 2
          }} ],
          "clips": [ {{ "id": "55555555-5555-5555-5555-555555555555", "patternId": "{pattern}", "startBeat": 16, "lengthBeats": 8, "offsetBeats": 1.5 }} ],
          "sections": [ {{ "id": "66666666-6666-6666-6666-666666666666", "name": "intro", "startBeat": 2.5 }} ],
          "tempoPoints": [ {{ "id": "77777777-7777-7777-7777-777777777777", "beat": 12.5, "bpm": 140, "ramp": true }} ]
        }}"#
    );
    let m = ProjectModel::from_json(&json).unwrap();
    assert_eq!(m.schema_version, SCHEMA_VERSION);

    let p = &m.patterns[0];
    assert_eq!(p.length_ticks, 8 * BEAT);
    let tid = m.tracks[0].id;
    let note = &p.notes(&tid)[0];
    assert_eq!(note.start_tick, 240, "0.25 beats is 240 ticks");
    assert_eq!(note.length_ticks, 720);
    assert_eq!(note.shift_ticks, -120);
    let chord = &p.chords.chords()[0];
    assert_eq!((chord.start_tick, chord.length_ticks), (480, 1440));
    assert_eq!(p.track_loop_ticks.get(&tid).copied(), Some(4 * BEAT));
    assert_eq!(p.chord_loop_ticks, Some(2 * BEAT));

    assert_eq!(m.clips[0].start_tick, 16 * BEAT);
    assert_eq!(m.clips[0].length_ticks, 8 * BEAT);
    assert_eq!(m.clips[0].offset_ticks, 1440);
    assert_eq!(m.sections[0].start_tick, 2400);
    assert_eq!(m.tempo_points[0].tick, 12_000);
    assert_eq!(m.swing_grid_ticks, BEAT / 4);
    assert_eq!(m.swing, 60.0);
}

/// Migration is one-way: what a migrated project writes back carries ticks only, so the next
/// load has nothing left to fold.
#[test]
fn a_migrated_project_is_written_back_without_legacy_beat_keys() {
    let track = "11111111-1111-1111-1111-111111111111";
    let json = format!(
        r#"{{
          "schemaVersion": 8,
          "swingGrid": 0.5,
          "tracks": [ {{ "id": "{track}", "name": "lead" }} ],
          "patterns": [ {{
            "id": "22222222-2222-2222-2222-222222222222", "name": "p", "lengthBeats": 8,
            "chords": {{ "chords": [ {{ "id": "33333333-3333-3333-3333-333333333333", "startBeat": 0, "lengthBeats": 4, "pitchClasses": [0,4,7] }} ] }},
            "notesByTrack": {{ "{track}": [ {{ "id": "44444444-4444-4444-4444-444444444444", "startBeat": 1, "lengthBeats": 1, "pitch": 60, "shift": 0.25 }} ] }},
            "trackLoopBeats": {{ "{track}": 4 }},
            "chordLoopBeats": 4
          }} ],
          "clips": [ {{ "id": "55555555-5555-5555-5555-555555555555", "patternId": "22222222-2222-2222-2222-222222222222", "startBeat": 0, "lengthBeats": 8, "offsetBeats": 2 }} ],
          "sections": [ {{ "id": "66666666-6666-6666-6666-666666666666", "name": "intro", "startBeat": 4 }} ],
          "tempoPoints": [ {{ "id": "77777777-7777-7777-7777-777777777777", "beat": 4, "bpm": 140 }} ]
        }}"#
    );
    let written = ProjectModel::from_json(&json).unwrap().to_json().unwrap();
    for key in [
        "\"startBeat\"",
        "\"lengthBeats\"",
        "\"offsetBeats\"",
        "\"trackLoopBeats\"",
        "\"chordLoopBeats\"",
        "\"swingGrid\"",
        "\"shift\"",
        "\"beat\"",
    ] {
        assert!(!written.contains(key), "{key} survived the migration:\n{written}");
    }
    // Reloading what we wrote changes nothing.
    let once = ProjectModel::from_json(&written).unwrap();
    assert_eq!(once, ProjectModel::from_json(&once.to_json().unwrap()).unwrap());
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
    assert_eq!(m.patterns[0].notes(&tid)[0].start_tick, 2 * BEAT);
    assert_eq!(m.patterns[0].notes(&tid)[0].length_ticks, BEAT / 2);
    assert_eq!(m.key.root_pitch_class, Some(7));
    assert_eq!(m.time_signature, TimeSignature::default());
    assert_eq!(m.schema_version, SCHEMA_VERSION);
}

#[test]
fn insert_chains_round_trip_with_their_order_bypass_and_state() {
    let mut model = ProjectModel::starter();
    let mut delay = Insert::new("clap:/CLAP/ValhallaDelay.clap:com.valhalladsp.delay", "Valhalla Delay");
    delay.state = Some("ZGVsYXk=".into());
    let mut eq = Insert::new("clap:/CLAP/Pro-Q.clap:com.fabfilter.proq3", "Pro-Q");
    eq.enabled = false;
    model.tracks[0].inserts = vec![delay, eq.clone()];
    model.master_inserts = vec![eq];

    let decoded = ProjectModel::from_json(&model.to_json().unwrap()).unwrap();
    assert_eq!(decoded, model);
    // Order is meaning, not presentation: the chain must come back in the order it went in.
    assert_eq!(
        decoded.tracks[0].inserts.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        ["Valhalla Delay", "Pro-Q"]
    );
}

#[test]
fn a_project_with_no_inserts_writes_no_insert_keys() {
    // Opening and re-saving a project that has never used an effect must not grow it.
    let json = ProjectModel::starter().to_json().unwrap();
    assert!(!json.contains("\"inserts\""), "an empty chain was written:\n{json}");
    assert!(!json.contains("masterInserts"), "an empty master chain was written:\n{json}");
}

#[test]
fn a_file_written_before_inserts_existed_opens_with_empty_chains() {
    let v9 = r#"{
        "schemaVersion": 9,
        "tempo": 110,
        "tracks": [{"id": "11111111-1111-1111-1111-111111111111", "name": "lead", "voice": "saw"}],
        "patterns": [{"id": "22222222-2222-2222-2222-222222222222", "name": "p1", "lengthTicks": 7680}]
    }"#;
    let m = ProjectModel::from_json(v9).unwrap();
    assert!(m.tracks[0].inserts.is_empty());
    assert!(m.master_inserts.is_empty());
}

#[test]
fn loading_drops_unusable_inserts_and_caps_the_chain() {
    // All three are only reachable by hand-editing a file, and all three would otherwise be
    // carried forever: a slot that can never load, two slots the plugin layer cannot tell
    // apart, and a chain deeper than the engine will process.
    let mut m = ProjectModel::starter();
    let dup = Insert::new("clap:/CLAP/Pro-Q.clap:com.fabfilter.proq3", "Pro-Q");
    let mut chain = vec![
        Insert { uid: "   ".into(), ..Insert::new("x", "nameless") },
        dup.clone(),
        dup.clone(),
    ];
    chain.extend((0..MAX_INSERTS).map(|i| Insert::new("clap:/CLAP/x.clap:x", format!("eq {i}"))));
    m.tracks[0].inserts = chain.clone();
    m.master_inserts = chain;

    let loaded = ProjectModel::from_json(&m.to_json().unwrap()).unwrap();
    for inserts in [&loaded.tracks[0].inserts, &loaded.master_inserts] {
        assert_eq!(inserts.len(), MAX_INSERTS, "the chain was not capped");
        assert!(inserts.iter().all(|i| !i.uid.trim().is_empty()), "a uid-less insert survived");
        assert_eq!(inserts.iter().filter(|i| i.id == dup.id).count(), 1, "a duplicated insert id survived");
        assert_eq!(inserts[0].name, "Pro-Q", "the surviving order was disturbed");
    }
}
