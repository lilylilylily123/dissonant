//! The serializable project. Schema v3 (the first Rust schema): adds per-note velocity and
//! persisted master settings on top of the Swift v2 layout.
//!
//! Decoding is tolerant: every field has a `serde(default)` so a file missing a later-added
//! field still opens. JSON keys are camelCase to match the previous app's files.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 3;

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
}

impl NoteEvent {
    pub fn new(start_beat: f64, length_beats: f64, pitch: i32) -> Self {
        NoteEvent {
            id: Uuid::new_v4(),
            start_beat,
            length_beats,
            pitch,
            velocity: default_velocity(),
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
    #[serde(default)]
    pub notes_by_track: HashMap<Uuid, Vec<NoteEvent>>,
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

fn default_tempo() -> f64 {
    120.0
}
fn default_schema() -> u32 {
    SCHEMA_VERSION
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
    /// The song: an ordered list of pattern ids, played back to back.
    #[serde(default)]
    pub arrangement: Vec<Uuid>,
    #[serde(default)]
    pub master: MasterSettings,
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
            arrangement: vec![],
            master: MasterSettings::default(),
        }
    }

    /// A new project pre-seeded with a I–IV–V–vi progression in C in its first pattern,
    /// plus a drum track, so the first loop is seconds away.
    pub fn starter() -> Self {
        let mut model = ProjectModel::empty();
        let chords = crate::theory::harmony::progression(&[0, 3, 4, 5], 0, ScaleType::Major, 16.0, 4.0);
        model.patterns[0].chords = crate::chord_track::ChordTrack::new(chords);
        model.tracks.push(Track::drums("drums"));
        model.arrangement = vec![model.patterns[0].id];
        model
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
        self.arrangement.retain(|id| pattern_ids.contains(id));
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
