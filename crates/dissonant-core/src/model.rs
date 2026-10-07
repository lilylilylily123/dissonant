//! The serializable project. Schema v3 (the first Rust schema): adds per-note velocity and
//! persisted master settings on top of the Swift v2 layout.
//!
//! Decoding is tolerant: every field has a `serde(default)` so a file missing a later-added
//! field still opens. JSON keys are camelCase to match the previous app's files.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 5;

/// A diatonic scale family. Expanded later (modes, harmonic minor, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ScaleType {
    #[default]
    Major,
    Minor,
}

/// The project's key. `root_pitch_class == None` means no key is locked yet (cold start).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyState {
    /// Pitch class of the tonic, 0 = C … 11 = B. `None` when no key is set.
    pub root_pitch_class: Option<i32>,
    pub scale: ScaleType,
    /// A user-confirmed key, as opposed to a merely inferred suggestion.
    pub is_locked: bool,
}

impl KeyState {
    pub const NONE: KeyState = KeyState {
        root_pitch_class: None,
        scale: ScaleType::Major,
        is_locked: false,
    };

    pub fn locked(root: i32, scale: ScaleType) -> Self {
        KeyState {
            root_pitch_class: Some(root),
            scale,
            is_locked: true,
        }
    }

    /// The scale's pitch classes, if a key is set.
    pub fn scale_pitch_classes(&self) -> Option<Vec<i32>> {
        self.root_pitch_class
            .map(|root| crate::theory::harmony::scale_pitch_classes(root, self.scale))
    }
}

/// One chord on the chord track, stored as bare pitch classes. `name` is display-only and
/// never required (R9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChordEvent {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub start_beat: f64,
    pub length_beats: f64,
    pub pitch_classes: Vec<i32>,
    #[serde(default)]
    pub name: Option<String>,
}

impl ChordEvent {
    pub fn new(start_beat: f64, length_beats: f64, pitch_classes: Vec<i32>, name: Option<String>) -> Self {
        ChordEvent {
            id: Uuid::new_v4(),
            start_beat,
            length_beats,
            pitch_classes,
            name,
        }
    }

    pub fn end_beat(&self) -> f64 {
        self.start_beat + self.length_beats
    }

    pub fn contains(&self, beat: f64) -> bool {
        beat >= self.start_beat && beat < self.end_beat()
    }
}

fn default_velocity() -> i32 {
    100
}

/// One note in the piano roll. `pitch` is a MIDI note number (0–127), `velocity` 1–127.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEvent {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub start_beat: f64,
    pub length_beats: f64,
    pub pitch: i32,
    #[serde(default = "default_velocity")]
    pub velocity: i32,
    /// The player marked this (dissonant) note as deliberate: the tier cue stays, the flag
    /// stops nagging. Guidance, never a gate (R11).
    #[serde(default)]
    pub intentional: bool,
    /// Kept in the pattern but skipped by the scheduler (drawn hollow).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub muted: bool,
}

impl NoteEvent {
    pub fn new(start_beat: f64, length_beats: f64, pitch: i32) -> Self {
        NoteEvent {
            id: Uuid::new_v4(),
            start_beat,
            length_beats,
            pitch,
            velocity: default_velocity(),
            intentional: false,
            muted: false,
        }
    }

    pub fn with_velocity(mut self, velocity: i32) -> Self {
        self.velocity = velocity;
        self
    }

    pub fn end_beat(&self) -> f64 {
        self.start_beat + self.length_beats
    }

    /// Clamp into the ranges the engine accepts. Used when applying edits from the UI.
    pub fn sanitized(mut self) -> Self {
        self.pitch = self.pitch.clamp(0, 127);
        self.velocity = self.velocity.clamp(1, 127);
        if !self.start_beat.is_finite() || self.start_beat < 0.0 {
            self.start_beat = 0.0;
        }
        if !self.length_beats.is_finite() || self.length_beats < 1.0 / 64.0 {
            self.length_beats = 1.0 / 64.0;
        }
        self
    }
}

fn default_voice() -> String {
    "saw".to_string()
}
fn one() -> f64 {
    1.0
}
fn default_tone() -> f64 {
    18_000.0
}

/// One instrument track: name, voice, mute/solo and its own bus settings. Notes live in
/// patterns keyed by track id, so a track is reused across every pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub name: String,
    /// A synth voice preset name (`saw`, `square`, `triangle`, `sine`, `pad`, `pluck`).
    #[serde(default = "default_voice")]
    pub voice: String,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub soloed: bool,
    /// A drum track uses the procedural kit and a step grid instead of a synth + piano roll.
    #[serde(default)]
    pub is_drum: bool,
    /// Bus gain (0–1.5).
    #[serde(default = "one")]
    pub volume: f64,
    /// Reverb wet mix (0–1).
    #[serde(default)]
    pub reverb_send: f64,
    /// Low-pass cutoff in Hz.
    #[serde(default = "default_tone")]
    pub tone: f64,
    /// -1 (left) … 1 (right).
    #[serde(default)]
    pub pan: f64,
    /// Display color (`#rrggbb`). `None` means "pick from the palette by position".
    #[serde(default)]
    pub color: Option<String>,
}

impl Track {
    pub fn new(name: impl Into<String>) -> Self {
        Track {
            id: Uuid::new_v4(),
            name: name.into(),
            voice: default_voice(),
            muted: false,
            soloed: false,
            is_drum: false,
            volume: 1.0,
            reverb_send: 0.0,
            tone: default_tone(),
            pan: 0.0,
            color: None,
        }
    }

    pub fn drums(name: impl Into<String>) -> Self {
        let mut t = Track::new(name);
        t.is_drum = true;
        t
    }
}

fn default_pattern_length() -> f64 {
    16.0
}

/// A loop — a named section like "verse" or "chorus". Holds its own chord progression (so
/// guidance is per-section) and the notes for each track, keyed by track id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongPattern {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub name: String,
    #[serde(default = "default_pattern_length")]
    pub length_beats: f64,
    #[serde(default)]
    pub chords: crate::chord_track::ChordTrack,
    #[serde(default, deserialize_with = "de_notes_by_track")]
    pub notes_by_track: HashMap<Uuid, Vec<NoteEvent>>,
}

/// `notesByTrack` is a JSON object keyed by track id — except in files written by the
/// original Swift app, whose `Codable` encoded a `[UUID: [NoteEvent]]` as a flat array of
/// alternating keys and values. Accept both.
#[derive(Deserialize)]
#[serde(untagged)]
enum NotesByTrackRepr {
    Map(HashMap<Uuid, Vec<NoteEvent>>),
    Flat(Vec<serde_json::Value>),
}

fn de_notes_by_track<'de, D: Deserializer<'de>>(d: D) -> Result<HashMap<Uuid, Vec<NoteEvent>>, D::Error> {
    match NotesByTrackRepr::deserialize(d)? {
        NotesByTrackRepr::Map(m) => Ok(m),
        NotesByTrackRepr::Flat(values) => {
            let mut out = HashMap::new();
            for pair in values.chunks(2) {
                if pair.len() == 2 {
                    let key: Uuid = serde_json::from_value(pair[0].clone()).map_err(serde::de::Error::custom)?;
                    let notes: Vec<NoteEvent> = serde_json::from_value(pair[1].clone()).map_err(serde::de::Error::custom)?;
                    out.insert(key, notes);
                }
            }
            Ok(out)
        }
    }
}

impl SongPattern {
    pub fn new(name: impl Into<String>) -> Self {
        SongPattern {
            id: Uuid::new_v4(),
            name: name.into(),
            length_beats: default_pattern_length(),
            chords: Default::default(),
            notes_by_track: HashMap::new(),
        }
    }

    pub fn notes(&self, track_id: &Uuid) -> &[NoteEvent] {
        self.notes_by_track.get(track_id).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn default_low_cut() -> f64 {
    20.0
}

/// The master chain's persisted settings (previously lived in UI state and was lost on save).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MasterSettings {
    pub gain: f64,
    pub reverb_wet: f64,
    pub low_cut_hz: f64,
    pub high_cut_hz: f64,
    pub low_eq: f64,
    pub mid_eq: f64,
    pub high_eq: f64,
}

impl Default for MasterSettings {
    fn default() -> Self {
        MasterSettings {
            gain: 1.0,
            reverb_wet: 0.0,
            low_cut_hz: default_low_cut(),
            high_cut_hz: default_tone(),
            low_eq: 1.0,
            mid_eq: 1.0,
            high_eq: 1.0,
        }
    }
}

/// A pattern placed on the song timeline. The pattern loops from `offset_beats` for
/// `length_beats`, so a clip can be trimmed, extended or started mid-pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub pattern_id: Uuid,
    pub start_beat: f64,
    pub length_beats: f64,
    #[serde(default)]
    pub offset_beats: f64,
    #[serde(default)]
    pub muted: bool,
}

impl Clip {
    pub fn new(pattern_id: Uuid, start_beat: f64, length_beats: f64) -> Self {
        Clip {
            id: Uuid::new_v4(),
            pattern_id,
            start_beat,
            length_beats,
            offset_beats: 0.0,
            muted: false,
        }
    }

    pub fn end_beat(&self) -> f64 {
        self.start_beat + self.length_beats
    }
}

/// A named marker on the song (intro, verse, drop…). With a `key`, the section modulates:
/// patterns placed inside it are tiered against that key instead of the project key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    pub name: String,
    pub start_beat: f64,
    #[serde(default)]
    pub key: Option<KeyState>,
    #[serde(default)]
    pub color: Option<String>,
}

impl Section {
    pub fn new(name: impl Into<String>, start_beat: f64) -> Self {
        Section {
            id: Uuid::new_v4(),
            name: name.into(),
            start_beat,
            key: None,
            color: None,
        }
    }
}

/// Beats per bar and the beat unit. Only the numerator changes grids and bar math; the
/// engine counts in quarter-note beats regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TimeSignature {
    pub numerator: u32,
    pub denominator: u32,
}

impl Default for TimeSignature {
    fn default() -> Self {
        TimeSignature { numerator: 4, denominator: 4 }
    }
}

impl TimeSignature {
    /// Quarter-note beats in one bar (3/4 → 3, 6/8 → 3, 7/8 → 3.5).
    pub fn beats_per_bar(&self) -> f64 {
        self.numerator as f64 * 4.0 / self.denominator.max(1) as f64
    }
}

fn default_tempo() -> f64 {
    120.0
}
fn default_schema() -> u32 {
    SCHEMA_VERSION
}
/// The starter melody: one phrase per chord of I–IV–V–vi in C, as `(start, length, pitch,
/// velocity)` inside a 4-beat chord slot. Mostly chord tones, plus two tensions (D over C,
/// B leaning into C over Am) so the roll shows more than one tier on the first look.
const STARTER_MELODY: [[(f64, f64, i32, i32); 3]; 4] = [
    [(0.0, 1.5, 76, 100), (1.5, 0.5, 74, 80), (2.0, 2.0, 72, 96)], // C:  E  D  C
    [(0.0, 1.0, 69, 100), (1.0, 1.0, 72, 88), (2.0, 2.0, 77, 96)], // F:  A  C  F
    [(0.0, 1.5, 74, 100), (1.5, 0.5, 71, 80), (2.0, 2.0, 67, 96)], // G:  D  B  G
    [(0.0, 1.5, 72, 100), (1.5, 0.5, 71, 80), (2.0, 2.0, 69, 96)], // Am: C  B  A
];

/// A plain groove for the starter's drum track, one bar at a time: kick on the downbeat (and
/// mid-bar in even meters), snare on the other beats, hats on every 8th. Hits sit on the drum
/// grid's 8th steps and anything past the pattern's end is dropped.
fn starter_groove(length: f64, bar: f64) -> Vec<NoteEvent> {
    const KICK: i32 = 36;
    const SNARE: i32 = 38;
    const HAT: i32 = 42;
    let mut hits = Vec::new();
    let mut bar_start = 0.0;
    while bar_start < length - 1e-9 {
        let mut push = |offset: f64, pitch: i32, velocity: i32| {
            let start = bar_start + offset;
            if offset < bar - 1e-9 && start < length - 1e-9 {
                hits.push(NoteEvent::new(start, 0.25_f64.min(length - start), pitch).with_velocity(velocity));
            }
        };
        let whole_beats = bar.ceil() as i32;
        // A second kick mid-bar only in whole, even meters of 4+ beats (4/4, 6/4): in 2/4 it
        // would take the snare's place.
        let mid_kick = (bar.fract() == 0.0 && whole_beats >= 4 && whole_beats % 2 == 0).then_some(whole_beats / 2);
        for beat in 0..whole_beats {
            let b = beat as f64;
            if beat == 0 || Some(beat) == mid_kick {
                push(b, KICK, 120);
            } else {
                push(b, SNARE, 100);
            }
        }
        let mut step = 0.0;
        while step < bar - 1e-9 {
            push(step, HAT, if step.fract() == 0.0 { 80 } else { 60 });
            step += 0.5;
        }
        bar_start += bar;
    }
    hits.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat).then(a.pitch.cmp(&b.pitch)));
    hits
}

fn default_tracks() -> Vec<Track> {
    vec![Track::new("melody")]
}
fn default_patterns() -> Vec<SongPattern> {
    vec![SongPattern::new("pattern 1")]
}

/// The full serializable project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectModel {
    #[serde(default = "default_schema")]
    pub schema_version: u32,
    #[serde(default = "default_tempo")]
    pub tempo: f64,
    #[serde(default)]
    pub key: KeyState,
    #[serde(default = "default_tracks")]
    pub tracks: Vec<Track>,
    #[serde(default = "default_patterns")]
    pub patterns: Vec<SongPattern>,
    /// The song: clips placed on the timeline (all tracks share the clip lane, since a
    /// pattern holds every track's notes).
    #[serde(default)]
    pub clips: Vec<Clip>,
    /// Song markers, optionally with their own key.
    #[serde(default)]
    pub sections: Vec<Section>,
    /// Legacy (schema ≤ 3): an ordered list of pattern ids played back to back. Folded into
    /// `clips` by [`ProjectModel::normalized`] and never written again.
    #[serde(default, skip_serializing)]
    pub arrangement: Vec<Uuid>,
    #[serde(default)]
    pub master: MasterSettings,
    #[serde(default)]
    pub time_signature: TimeSignature,
    /// Global swing: 50 = straight, 66.7 = triplet feel, 75 = hard shuffle. See
    /// [`crate::tempo::swing_warp`].
    #[serde(default = "default_swing")]
    pub swing: f64,
    /// The subdivision swing acts on, in beats (0.5 = eighths, 0.25 = sixteenths).
    #[serde(default = "default_swing_grid")]
    pub swing_grid: f64,
    /// Tempo changes along the song (empty = `tempo` throughout). Pattern mode plays at the
    /// base tempo.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tempo_points: Vec<crate::tempo::TempoPoint>,
}

fn default_swing() -> f64 {
    50.0
}
fn default_swing_grid() -> f64 {
    0.5
}

impl Default for ProjectModel {
    fn default() -> Self {
        ProjectModel::empty()
    }
}

impl ProjectModel {
    /// A brand-new project: one melody track, one empty pattern, no key.
    pub fn empty() -> Self {
        ProjectModel {
            schema_version: SCHEMA_VERSION,
            tempo: default_tempo(),
            key: KeyState::NONE,
            tracks: default_tracks(),
            patterns: default_patterns(),
            clips: vec![],
            sections: vec![],
            arrangement: vec![],
            master: MasterSettings::default(),
            time_signature: TimeSignature::default(),
            swing: default_swing(),
            swing_grid: default_swing_grid(),
            tempo_points: vec![],
        }
    }

    /// The song's tempo map (base tempo plus points).
    pub fn tempo_map(&self) -> crate::tempo::TempoMap {
        crate::tempo::TempoMap::new(self.tempo, &self.tempo_points)
    }

    /// A new project that plays music the moment space is pressed: I–IV–V–vi in C (key
    /// locked), a short melody over it and a drum groove, over one 4-bar 4/4 pattern placed
    /// in the song. See [`ProjectModel::starter_shaped`].
    pub fn starter() -> Self {
        Self::starter_shaped(16.0, 4.0)
    }

    /// The starter at any pattern length and bar size (the new-project defaults). The four
    /// chords split the pattern evenly and the melody stretches with them, so every note
    /// keeps its tier. The groove repeats per bar at its own speed instead of stretching.
    pub fn starter_shaped(length_beats: f64, beats_per_bar: f64) -> Self {
        let length = if length_beats.is_finite() && length_beats > 0.0 { length_beats } else { default_pattern_length() };
        let bar = if beats_per_bar.is_finite() && beats_per_bar > 0.0 { beats_per_bar } else { 4.0 };
        let mut model = ProjectModel::empty();
        model.key = KeyState::locked(0, ScaleType::Major);
        let slot = length / STARTER_MELODY.len() as f64;
        model.patterns[0].length_beats = length;
        model.patterns[0].chords = crate::chord_track::ChordTrack::new(crate::theory::harmony::progression(
            &[0, 3, 4, 5],
            0,
            ScaleType::Major,
            length,
            slot,
        ));

        model.tracks[0].voice = "pluck".into();
        model.tracks[0].reverb_send = 0.2;
        let melody_id = model.tracks[0].id;
        let scale = slot / 4.0;
        let melody = STARTER_MELODY
            .iter()
            .enumerate()
            .flat_map(|(i, phrase)| {
                phrase.iter().map(move |&(start, len, pitch, vel)| {
                    NoteEvent::new(i as f64 * slot + start * scale, len * scale, pitch).with_velocity(vel)
                })
            })
            .collect();
        model.patterns[0].notes_by_track.insert(melody_id, melody);

        let drums = Track::drums("drums");
        let drums_id = drums.id;
        model.tracks.push(drums);
        model.patterns[0].notes_by_track.insert(drums_id, starter_groove(length, bar));

        let p = &model.patterns[0];
        model.clips = vec![Clip::new(p.id, 0.0, p.length_beats)];
        model
    }

    pub fn clip(&self, id: &Uuid) -> Option<&Clip> {
        self.clips.iter().find(|c| &c.id == id)
    }

    pub fn section(&self, id: &Uuid) -> Option<&Section> {
        self.sections.iter().find(|s| &s.id == id)
    }

    /// End of the last clip (0 for an empty song).
    pub fn song_length(&self) -> f64 {
        self.clips.iter().map(Clip::end_beat).fold(0.0, f64::max)
    }

    /// The section containing `beat` (the last one starting at or before it).
    pub fn section_at(&self, beat: f64) -> Option<&Section> {
        self.sections
            .iter()
            .filter(|s| s.start_beat <= beat)
            .max_by(|a, b| a.start_beat.total_cmp(&b.start_beat))
    }

    /// The key in force at `beat`: the section's key if it has one, else the project key.
    pub fn key_at(&self, beat: f64) -> KeyState {
        self.section_at(beat).and_then(|s| s.key).unwrap_or(self.key)
    }

    pub fn track(&self, id: &Uuid) -> Option<&Track> {
        self.tracks.iter().find(|t| &t.id == id)
    }

    pub fn track_mut(&mut self, id: &Uuid) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| &t.id == id)
    }

    pub fn pattern(&self, id: &Uuid) -> Option<&SongPattern> {
        self.patterns.iter().find(|p| &p.id == id)
    }

    pub fn pattern_mut(&mut self, id: &Uuid) -> Option<&mut SongPattern> {
        self.patterns.iter_mut().find(|p| &p.id == id)
    }

    /// Normalize after decoding a foreign/older file: bump the schema, make sure there is at
    /// least one track and one pattern, clamp every note, and drop references to things that
    /// aren't in the project any more.
    pub fn normalized(mut self) -> Self {
        // Only ever bump forward: relabelling a newer file as ours would make the next save
        // overwrite it with fields this version dropped.
        self.schema_version = self.schema_version.max(SCHEMA_VERSION);
        if self.tracks.is_empty() {
            self.tracks = default_tracks();
        }
        if self.patterns.is_empty() {
            self.patterns = default_patterns();
        }
        let track_ids: Vec<Uuid> = self.tracks.iter().map(|t| t.id).collect();
        for pattern in &mut self.patterns {
            // Notes keyed by a track that no longer exists are invisible and never played, but
            // survive every save; drop them instead of growing the file forever.
            pattern.notes_by_track.retain(|tid, _| track_ids.contains(tid));
            for notes in pattern.notes_by_track.values_mut() {
                for n in notes.iter_mut() {
                    *n = n.clone().sanitized();
                }
            }
        }
        let pattern_ids: Vec<Uuid> = self.patterns.iter().map(|p| p.id).collect();
        // Legacy flat arrangement → clips, back to back.
        if self.clips.is_empty() && !self.arrangement.is_empty() {
            let mut at = 0.0;
            for pid in &self.arrangement {
                if let Some(p) = self.patterns.iter().find(|p| &p.id == pid) {
                    self.clips.push(Clip::new(p.id, at, p.length_beats));
                    at += p.length_beats;
                }
            }
        }
        self.arrangement.clear();
        if !self.swing.is_finite() {
            self.swing = default_swing();
        }
        self.swing = self.swing.clamp(50.0, 75.0);
        if !(self.swing_grid == 0.5 || self.swing_grid == 0.25) {
            self.swing_grid = default_swing_grid();
        }
        self.tempo_points.retain(|p| p.beat.is_finite() && p.beat >= 0.0 && p.bpm.is_finite());
        for p in &mut self.tempo_points {
            p.bpm = p.bpm.clamp(crate::tempo::Tempo::MIN_BPM, crate::tempo::Tempo::MAX_BPM);
        }
        self.tempo_points.sort_by(|a, b| a.beat.total_cmp(&b.beat));
        self.clips.retain(|c| pattern_ids.contains(&c.pattern_id) && c.length_beats > 0.0 && c.start_beat >= 0.0);
        self.clips.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
        self.sections.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
        self
    }

    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        use serde::de::Error;
        // Every field defaults, and serde also accepts a struct in sequence form — so `[]` and
        // `[1,2]` would both decode as a blank project and the next save would destroy whatever
        // file the user actually opened. A project is an object.
        let value: serde_json::Value = serde_json::from_str(json)?;
        if !value.is_object() {
            return Err(serde_json::Error::custom("not a dissonant project: expected a JSON object"));
        }
        let decoded: ProjectModel = serde_json::from_value(value)?;
        if decoded.schema_version > SCHEMA_VERSION {
            return Err(serde_json::Error::custom(format!(
                "project was saved by a newer version of dissonant (schema {}, this build reads {})",
                decoded.schema_version, SCHEMA_VERSION
            )));
        }
        Ok(decoded.normalized())
    }
}
