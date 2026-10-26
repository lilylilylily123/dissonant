//! The document: a [`ProjectModel`] plus undo/redo, mutated only through [`Command`]s.
//!
//! Every command is validated and applied here, so the UI, tests and any future scripting or
//! collaboration layer all go through one door. Undo is snapshot-based (the model is small);
//! continuous controls (sliders) send `transient` commands which coalesce into one undo step
//! per gesture.

use crate::midi_file::ImportedTrack;
use crate::model::{
    AmpRig, AudioClip, AudioSource, ChordEvent, Clip, Insert, KeyState, MasterSettings, NoteEvent, ProjectModel, Section,
    SongPattern, Track, TrackPlugin, MAX_INSERTS, MIN_NOTE_TICKS,
};
use crate::tempo::TempoPoint;
use crate::theory::harmony;
use crate::ticks::TICKS_PER_BEAT;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackParam {
    Volume,
    ReverbSend,
    Tone,
    Pan,
}

/// Which effect chain an insert command addresses: one track's, or the master's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InsertTarget {
    Track(Uuid),
    Master,
}

/// Every edit the UI can make. Serialized as `{"type": "...", ...}` over Tauri IPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Command {
    SetTempo { bpm: f64 },
    SetKey { key: KeyState },
    AddTrack { is_drum: bool },
    DeleteTrack { id: Uuid },
    RenameTrack { id: Uuid, name: String },
    SetTrackMuted { id: Uuid, muted: bool },
    SetTrackSoloed { id: Uuid, soloed: bool },
    MoveTrack { id: Uuid, up: bool },
    SetTrackVoice { id: Uuid, voice: String },
    SetTrackParam { id: Uuid, param: TrackParam, value: f64 },
    /// `#rrggbb`, or `None` to fall back to the palette.
    SetTrackColor { id: Uuid, color: Option<String> },
    /// Load a plugin on a track, or `None` to go back to the built-in voice.
    SetTrackPlugin { id: Uuid, plugin: Option<TrackPlugin> },
    /// The plugin's own state blob (base64) after the user edited it in the plugin's window.
    SetPluginState { id: Uuid, state: String },
    /// Add an effect to a chain, at `index` or appended. The slot's id is minted here.
    AddInsert { target: InsertTarget, uid: String, name: String, index: Option<usize> },
    RemoveInsert { target: InsertTarget, insert_id: Uuid },
    /// Move an insert within its chain; `to_index` is clamped to the chain.
    MoveInsert { target: InsertTarget, insert_id: Uuid, to_index: usize },
    /// Bypass (`on: false`) or re-enable an insert, leaving it in place in the chain.
    SetInsertEnabled { target: InsertTarget, insert_id: Uuid, on: bool },
    /// The effect's own state blob (base64) after the user edited it. Sent `transient` while
    /// a plugin window is open, so a run of them coalesces into one undo step like a knob drag.
    SetInsertState { target: InsertTarget, insert_id: Uuid, state: String },
    AddPattern,
    DuplicatePattern { id: Uuid },
    DeletePattern { id: Uuid },
    RenamePattern { id: Uuid, name: String },
    SetPatternLength { id: Uuid, ticks: i64 },
    /// Loop length in ticks of one track inside a pattern; `None` makes it fill the pattern.
    SetTrackLoop { pattern_id: Uuid, track_id: Uuid, ticks: Option<i64> },
    /// Loop length in ticks of a pattern's chord track; `None` makes it fill the pattern.
    SetChordLoop { pattern_id: Uuid, ticks: Option<i64> },
    /// Replace the notes of one track in one pattern (the roll commits whole arrays).
    SetNotes { pattern_id: Uuid, track_id: Uuid, notes: Vec<NoteEvent> },
    /// Replace a pattern's chord track.
    SetChords { pattern_id: Uuid, chords: Vec<ChordEvent> },
    /// Place a pattern on the song. `length_ticks` defaults to the pattern length.
    AddClip { pattern_id: Uuid, start_tick: i64, length_ticks: Option<i64> },
    /// Move / resize / re-offset / mute a clip (matched by id).
    UpdateClip { clip: Clip },
    RemoveClip { id: Uuid },
    AddSection { name: String, start_tick: i64 },
    UpdateSection { section: Section },
    RemoveSection { id: Uuid },
    SetMaster { master: MasterSettings },
    SetTimeSignature { numerator: u32, denominator: u32 },
    /// Global swing in percent (50–75) and its grid in ticks (480 or 240).
    SetSwing { swing: f64, grid_ticks: i64 },
    /// Add one track per imported MIDI track, with its notes in `pattern_id`, in one undo step.
    /// The pattern grows to `length_ticks` when that is longer.
    ImportTracks { pattern_id: Uuid, tracks: Vec<ImportedTrack>, length_ticks: i64 },
    /// A tempo change on the song timeline (replaces any point already on that tick).
    AddTempoPoint { tick: i64, bpm: f64, ramp: bool },
    UpdateTempoPoint { point: TempoPoint },
    RemoveTempoPoint { id: Uuid },
    /// A track that holds audio and a guitar rig instead of notes.
    AddAudioTrack { name: String },
    /// Replace an audio track's whole rig (one knob drag is one of these).
    SetTrackAmp { id: Uuid, amp: AmpRig },
    /// One undo step for "a new recording appeared": the source and its first clip together,
    /// since a source with nothing pointing at it is invisible and a clip without its source
    /// is dropped on load.
    AddAudioTake { track_id: Uuid, source: AudioSource, clip: AudioClip },
    /// Move / trim / fade / mute an audio clip (matched by id).
    UpdateAudioClip { clip: AudioClip },
    RemoveAudioClip { id: Uuid },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EditError {
    #[error("no such track")]
    NoSuchTrack,
    #[error("no such pattern")]
    NoSuchPattern,
    #[error("a project needs at least one track")]
    LastTrack,
    #[error("a project needs at least one pattern")]
    LastPattern,
    #[error("invalid value")]
    InvalidValue,
    #[error("track has no plugin")]
    NoPluginOnTrack,
    #[error("no such clip")]
    NoSuchClip,
    #[error("no such section")]
    NoSuchSection,
    #[error("no such tempo point")]
    NoSuchTempoPoint,
    #[error("no such audio clip")]
    NoSuchAudioClip,
    #[error("no such audio source")]
    NoSuchAudioSource,
    #[error("not an audio track")]
    NotAnAudioTrack,
    #[error("no such insert")]
    NoSuchInsert,
    #[error("an insert chain holds at most {MAX_INSERTS} effects")]
    TooManyInserts,
}

/// One undo step: the model before the edit, and what the edit was ("move notes").
#[derive(Debug, Clone)]
struct HistoryEntry {
    model: ProjectModel,
    label: String,
}

#[derive(Debug, Clone)]
pub struct Document {
    model: ProjectModel,
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    /// Snapshot taken at the start of a transient gesture, pushed to `undo` when it ends.
    pending: Option<HistoryEntry>,
    pub path: Option<std::path::PathBuf>,
    /// The model as it was at the last save point; `is_dirty` is a comparison against it rather
    /// than a sticky flag, so undoing back to the saved state really is "no unsaved changes".
    saved: Option<ProjectModel>,
    /// Bumps on every change to the model (edits, undo, redo, replace). Autosave compares it.
    revision: u64,
}

const MAX_UNDO: usize = 200;

impl Document {
    pub fn new(model: ProjectModel) -> Self {
        Document {
            saved: Some(model.clone()),
            model,
            undo: vec![],
            redo: vec![],
            pending: None,
            path: None,
            revision: 0,
        }
    }

    pub fn model(&self) -> &ProjectModel {
        &self.model
    }

    /// Unsaved changes, measured against the last save point rather than "an edit happened" —
    /// undoing back to the saved state (or a gesture that returns to its starting value) leaves
    /// the document clean, so the quit prompt only fires when the file really differs.
    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.model)
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.model.clone());
    }

    /// True only when an undo would actually change something: an open gesture that has not
    /// moved off its starting value is not undoable, and promising one lights a dead menu item.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.pending.as_ref().is_some_and(|p| p.model != self.model)
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// What the next undo would revert ("move notes"), if anything.
    pub fn undo_label(&self) -> Option<&str> {
        self.pending
            .as_ref()
            .map(|e| e.label.as_str())
            .or_else(|| self.undo.last().map(|e| e.label.as_str()))
    }

    /// What the next redo would re-apply, if anything.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    /// Monotonic change counter; two equal revisions mean the model has not changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Apply a command. `transient` edits (slider drags) don't create their own undo step;
    /// the first one in a run snapshots the model and the next non-transient command (or
    /// [`Document::commit_gesture`]) turns that snapshot into a single undo entry.
    pub fn apply(&mut self, command: Command, transient: bool) -> Result<(), EditError> {
        self.apply_labeled(command, transient, None)
    }

    /// [`Document::apply`] with an explicit undo label. The UI knows *what* an edit was
    /// ("resize notes") better than the command does ("edit notes"); when it does not say,
    /// [`Command::label`] is used.
    pub fn apply_labeled(&mut self, command: Command, transient: bool, label: Option<String>) -> Result<(), EditError> {
        let label = label.filter(|l| !l.trim().is_empty()).unwrap_or_else(|| command.label());
        let before = self.model.clone();
        let mut next = self.model.clone();
        apply_to(&mut next, command)?;
        if next == before {
            return Ok(());
        }
        if transient {
            if self.pending.is_none() {
                self.pending = Some(HistoryEntry { model: before, label });
            }
        } else {
            self.commit_gesture();
            self.push_undo(HistoryEntry { model: before, label });
        }
        self.redo.clear();
        self.model = next;
        self.touch();
        Ok(())
    }

    /// End a run of transient edits, making it one undo step.
    pub fn commit_gesture(&mut self) {
        if let Some(entry) = self.pending.take() {
            if entry.model != self.model {
                self.push_undo(entry);
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        self.commit_gesture();
        match self.undo.pop() {
            Some(entry) => {
                let current = std::mem::replace(&mut self.model, entry.model);
                self.redo.push(HistoryEntry { model: current, label: entry.label });
                self.touch();
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        self.commit_gesture();
        match self.redo.pop() {
            Some(entry) => {
                let current = std::mem::replace(&mut self.model, entry.model);
                self.undo.push(HistoryEntry { model: current, label: entry.label });
                self.touch();
                true
            }
            None => false,
        }
    }

    /// Replace the whole model (new / open). Clears history.
    pub fn replace(&mut self, model: ProjectModel, path: Option<std::path::PathBuf>) {
        self.saved = Some(model.clone());
        self.model = model;
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.path = path;
        self.revision += 1;
    }

    /// Replace the model but keep it marked dirty — used when restoring an autosave, which is
    /// by definition unsaved work. Dropping the save point is what makes it dirty: there is no
    /// file this model is known to match.
    pub fn restore(&mut self, model: ProjectModel, path: Option<std::path::PathBuf>) {
        self.replace(model, path);
        self.saved = None;
    }

    /// Note a change to the model. Dirtiness is derived from the save point, so this only has
    /// to move the revision counter autosave watches.
    fn touch(&mut self) {
        self.revision += 1;
    }

    fn push_undo(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }
}

impl Command {
    /// A short, lower-case description for the Edit menu ("Undo set tempo").
    pub fn label(&self) -> String {
        use Command::*;
        match self {
            SetTempo { .. } => "set tempo",
            SetKey { .. } => "set key",
            AddTrack { is_drum: true } => "add drum track",
            AddTrack { .. } => "add track",
            DeleteTrack { .. } => "delete track",
            RenameTrack { .. } => "rename track",
            SetTrackMuted { muted: true, .. } => "mute track",
            SetTrackMuted { .. } => "unmute track",
            SetTrackSoloed { soloed: true, .. } => "solo track",
            SetTrackSoloed { .. } => "unsolo track",
            MoveTrack { .. } => "reorder tracks",
            SetTrackVoice { .. } => "change instrument",
            SetTrackParam { param, .. } => match param {
                TrackParam::Volume => "change volume",
                TrackParam::ReverbSend => "change reverb send",
                TrackParam::Tone => "change tone",
                TrackParam::Pan => "change pan",
            },
            SetTrackColor { .. } => "color track",
            SetTrackPlugin { .. } => "change plugin",
            SetPluginState { .. } => "edit plugin",
            // Only `AddInsert` carries the effect's name; for the rest the UI passes the name
            // it is showing through `apply_labeled`, the same as it does for note edits.
            AddInsert { name, .. } => {
                let name = name.trim();
                return if name.is_empty() { "add insert".to_string() } else { format!("add {name}") };
            }
            RemoveInsert { .. } => "remove insert",
            MoveInsert { .. } => "reorder insert",
            SetInsertEnabled { on: true, .. } => "enable insert",
            SetInsertEnabled { .. } => "bypass insert",
            SetInsertState { .. } => "insert settings",
            AddPattern => "add pattern",
            DuplicatePattern { .. } => "duplicate pattern",
            DeletePattern { .. } => "delete pattern",
            RenamePattern { .. } => "rename pattern",
            SetPatternLength { .. } => "change pattern length",
            SetTrackLoop { .. } => "change track loop",
            SetChordLoop { .. } => "change chord loop",
            SetNotes { .. } => "edit notes",
            SetChords { .. } => "edit chords",
            AddClip { .. } => "add clip",
            UpdateClip { .. } => "edit clip",
            RemoveClip { .. } => "remove clip",
            AddSection { .. } => "add section",
            UpdateSection { .. } => "edit section",
            RemoveSection { .. } => "remove section",
            SetMaster { .. } => "change master",
            SetTimeSignature { .. } => "set time signature",
            SetSwing { .. } => "set swing",
            ImportTracks { .. } => "import MIDI",
            AddTempoPoint { .. } => "add tempo change",
            UpdateTempoPoint { .. } => "edit tempo change",
            RemoveTempoPoint { .. } => "remove tempo change",
            AddAudioTrack { .. } => "add audio track",
            SetTrackAmp { .. } => "edit rig",
            AddAudioTake { .. } => "add audio",
            UpdateAudioClip { .. } => "edit audio clip",
            RemoveAudioClip { .. } => "remove audio clip",
        }
        .to_string()
    }
}

/// A loop length on its way into a pattern: either absent ("fill the pattern") or at least one
/// beat. Clamping against the pattern happens when it is read, so a loop set before the pattern
/// is shortened comes back when the pattern grows again.
fn checked_loop(ticks: Option<i64>) -> Result<Option<i64>, EditError> {
    match ticks {
        Some(t) if t < crate::model::MIN_LOOP_TICKS => Err(EditError::InvalidValue),
        other => Ok(other),
    }
}

/// Shape-only base64 check: the standard alphabet, padded to a multiple of four with at most
/// two trailing `=`. Core never decodes a plugin's state — it has no business knowing what the
/// bytes mean — but a blob that cannot possibly be base64 is a bug at the edit, not at load,
/// and refusing it here keeps unreadable garbage out of saved projects.
fn is_base64(s: &str) -> bool {
    if !s.len().is_multiple_of(4) {
        return false;
    }
    let body = s.trim_end_matches('=');
    if s.len() - body.len() > 2 {
        return false;
    }
    body.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

/// The effect chain an [`InsertTarget`] names. The master's chain always exists; a track's
/// only exists while the track does.
fn chain_mut(m: &mut ProjectModel, target: InsertTarget) -> Result<&mut Vec<Insert>, EditError> {
    match target {
        InsertTarget::Track(id) => Ok(&mut m.track_mut(&id).ok_or(EditError::NoSuchTrack)?.inserts),
        InsertTarget::Master => Ok(&mut m.master_inserts),
    }
}

fn insert_mut(m: &mut ProjectModel, target: InsertTarget, insert_id: Uuid) -> Result<&mut Insert, EditError> {
    chain_mut(m, target)?.iter_mut().find(|i| i.id == insert_id).ok_or(EditError::NoSuchInsert)
}

fn apply_to(m: &mut ProjectModel, command: Command) -> Result<(), EditError> {
    use Command::*;
    match command {
        SetTempo { bpm } => {
            if !bpm.is_finite() {
                return Err(EditError::InvalidValue);
            }
            m.tempo = bpm.clamp(crate::tempo::Tempo::MIN_BPM, crate::tempo::Tempo::MAX_BPM);
        }
        SetKey { key } => m.key = key,
        AddTrack { is_drum } => {
            let n = m.tracks.len() + 1;
            m.tracks.push(if is_drum {
                Track::drums("drums")
            } else {
                Track::new(format!("track {n}"))
            });
        }
        DeleteTrack { id } => {
            if m.tracks.len() <= 1 {
                return Err(EditError::LastTrack);
            }
            let before = m.tracks.len();
            m.tracks.retain(|t| t.id != id);
            if m.tracks.len() == before {
                return Err(EditError::NoSuchTrack);
            }
            for p in &mut m.patterns {
                p.notes_by_track.remove(&id);
                p.track_loop_ticks.remove(&id);
            }
            m.audio_clips.retain(|c| c.track_id != id);
        }
        RenameTrack { id, name } => {
            // Matches RenamePattern: a blank name makes the track unidentifiable everywhere.
            if name.trim().is_empty() {
                return Err(EditError::InvalidValue);
            }
            m.track_mut(&id).ok_or(EditError::NoSuchTrack)?.name = name;
        }
        SetTrackMuted { id, muted } => m.track_mut(&id).ok_or(EditError::NoSuchTrack)?.muted = muted,
        SetTrackSoloed { id, soloed } => m.track_mut(&id).ok_or(EditError::NoSuchTrack)?.soloed = soloed,
        MoveTrack { id, up } => {
            let i = m.tracks.iter().position(|t| t.id == id).ok_or(EditError::NoSuchTrack)?;
            let j = if up { i.checked_sub(1) } else { Some(i + 1) };
            if let Some(j) = j.filter(|&j| j < m.tracks.len()) {
                m.tracks.swap(i, j);
            }
        }
        SetTrackVoice { id, voice } => m.track_mut(&id).ok_or(EditError::NoSuchTrack)?.voice = voice,
        SetTrackParam { id, param, value } => {
            if !value.is_finite() {
                return Err(EditError::InvalidValue);
            }
            let t = m.track_mut(&id).ok_or(EditError::NoSuchTrack)?;
            match param {
                TrackParam::Volume => t.volume = value.clamp(0.0, 1.5),
                TrackParam::ReverbSend => t.reverb_send = value.clamp(0.0, 1.0),
                TrackParam::Tone => t.tone = value.clamp(200.0, 20_000.0),
                TrackParam::Pan => t.pan = value.clamp(-1.0, 1.0),
            }
        }
        SetTrackColor { id, color } => {
            if let Some(c) = &color {
                let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
                if !ok {
                    return Err(EditError::InvalidValue);
                }
            }
            m.track_mut(&id).ok_or(EditError::NoSuchTrack)?.color = color.map(|c| c.to_lowercase());
        }
        SetTrackPlugin { id, plugin } => {
            if let Some(p) = &plugin {
                // A blank uid names nothing: the track would claim to be plugin-driven and
                // silently fall back to its voice on every load.
                if p.uid.trim().is_empty() {
                    return Err(EditError::InvalidValue);
                }
                if p.state.as_deref().is_some_and(|s| !is_base64(s)) {
                    return Err(EditError::InvalidValue);
                }
            }
            let t = m.track_mut(&id).ok_or(EditError::NoSuchTrack)?;
            let previous = t.plugin.as_ref().map(|p| p.uid.clone());
            t.plugin = plugin.map(|mut p| {
                // A state blob only means anything to the plugin that wrote it; carrying it
                // across a swap feeds one plugin another's bytes.
                if previous.as_deref() != Some(p.uid.as_str()) {
                    p.state = None;
                }
                p
            });
        }
        SetPluginState { id, state } => {
            if !is_base64(&state) {
                return Err(EditError::InvalidValue);
            }
            let t = m.track_mut(&id).ok_or(EditError::NoSuchTrack)?;
            // State with no plugin to own it would be resurrected onto whatever plugin is
            // loaded next, which is exactly the mismatch the uid check above prevents.
            t.plugin.as_mut().ok_or(EditError::NoPluginOnTrack)?.state = Some(state);
        }
        AddInsert { target, uid, name, index } => {
            // A blank uid names nothing: the chain would carry a slot that can never load.
            if uid.trim().is_empty() {
                return Err(EditError::InvalidValue);
            }
            let chain = chain_mut(m, target)?;
            if chain.len() >= MAX_INSERTS {
                return Err(EditError::TooManyInserts);
            }
            let at = index.unwrap_or(chain.len()).min(chain.len());
            chain.insert(at, Insert::new(uid, name));
        }
        RemoveInsert { target, insert_id } => {
            let chain = chain_mut(m, target)?;
            let before = chain.len();
            chain.retain(|i| i.id != insert_id);
            if chain.len() == before {
                return Err(EditError::NoSuchInsert);
            }
        }
        MoveInsert { target, insert_id, to_index } => {
            let chain = chain_mut(m, target)?;
            let from = chain.iter().position(|i| i.id == insert_id).ok_or(EditError::NoSuchInsert)?;
            // Dragging past either end lands on that end rather than failing: the UI's drop
            // targets and the chain's bounds do not have to agree.
            let to = to_index.min(chain.len() - 1);
            let slot = chain.remove(from);
            chain.insert(to, slot);
        }
        SetInsertEnabled { target, insert_id, on } => {
            insert_mut(m, target, insert_id)?.enabled = on;
        }
        SetInsertState { target, insert_id, state } => {
            if !is_base64(&state) {
                return Err(EditError::InvalidValue);
            }
            insert_mut(m, target, insert_id)?.state = Some(state);
        }
        AddPattern => {
            let mut p = SongPattern::new(format!("pattern {}", m.patterns.len() + 1));
            let root = m.key.root_pitch_class.unwrap_or(0);
            p.chords = crate::chord_track::ChordTrack::new(harmony::progression(
                &harmony::starters()[0].degrees,
                root,
                m.key.scale,
                p.length_ticks,
                4 * TICKS_PER_BEAT,
            ));
            m.patterns.push(p);
        }
        DuplicatePattern { id } => {
            let i = m.patterns.iter().position(|p| p.id == id).ok_or(EditError::NoSuchPattern)?;
            let mut copy = m.patterns[i].clone();
            copy.id = Uuid::new_v4();
            copy.name = format!("{} copy", copy.name);
            for notes in copy.notes_by_track.values_mut() {
                for n in notes.iter_mut() {
                    n.id = Uuid::new_v4();
                }
            }
            let chords: Vec<ChordEvent> = copy
                .chords
                .chords()
                .iter()
                .cloned()
                .map(|mut c| {
                    c.id = Uuid::new_v4();
                    c
                })
                .collect();
            copy.chords = crate::chord_track::ChordTrack::new(chords);
            m.patterns.insert(i + 1, copy);
        }
        DeletePattern { id } => {
            if m.patterns.len() <= 1 {
                return Err(EditError::LastPattern);
            }
            let before = m.patterns.len();
            m.patterns.retain(|p| p.id != id);
            if m.patterns.len() == before {
                return Err(EditError::NoSuchPattern);
            }
            m.clips.retain(|c| c.pattern_id != id);
        }
        RenamePattern { id, name } => {
            if name.trim().is_empty() {
                return Err(EditError::InvalidValue);
            }
            m.pattern_mut(&id).ok_or(EditError::NoSuchPattern)?.name = name;
        }
        SetPatternLength { id, ticks } => {
            if ticks < TICKS_PER_BEAT {
                return Err(EditError::InvalidValue);
            }
            m.pattern_mut(&id).ok_or(EditError::NoSuchPattern)?.length_ticks = ticks;
        }
        SetTrackLoop { pattern_id, track_id, ticks } => {
            if m.track(&track_id).is_none() {
                return Err(EditError::NoSuchTrack);
            }
            let ticks = checked_loop(ticks)?;
            let p = m.pattern_mut(&pattern_id).ok_or(EditError::NoSuchPattern)?;
            match ticks {
                Some(t) => p.track_loop_ticks.insert(track_id, t),
                None => p.track_loop_ticks.remove(&track_id),
            };
            // The loop is now the track's whole extent, so anything past it is cut rather than
            // kept as material that is drawn somewhere but never heard. One undo step restores it.
            let turn = p.track_loop(&track_id);
            if let Some(notes) = p.notes_by_track.get_mut(&track_id) {
                notes.retain(|n| n.start_tick < turn);
                for n in notes.iter_mut() {
                    n.length_ticks = (turn - n.start_tick).min(n.length_ticks).max(MIN_NOTE_TICKS);
                }
            }
        }
        SetChordLoop { pattern_id, ticks } => {
            let ticks = checked_loop(ticks)?;
            let p = m.pattern_mut(&pattern_id).ok_or(EditError::NoSuchPattern)?;
            p.chord_loop_ticks = ticks;
            let turn = p.chord_loop();
            let chords: Vec<ChordEvent> = p
                .chords
                .chords()
                .iter()
                .filter(|c| c.start_tick < turn)
                .map(|c| ChordEvent { length_ticks: (turn - c.start_tick).min(c.length_ticks), ..c.clone() })
                .collect();
            p.chords = crate::chord_track::ChordTrack::new(chords);
        }
        SetNotes { pattern_id, track_id, notes } => {
            if m.track(&track_id).is_none() {
                return Err(EditError::NoSuchTrack);
            }
            let p = m.pattern_mut(&pattern_id).ok_or(EditError::NoSuchPattern)?;
            let mut notes: Vec<NoteEvent> = notes.into_iter().map(NoteEvent::sanitized).collect();
            notes.sort_by(|a, b| a.start_tick.cmp(&b.start_tick).then(a.pitch.cmp(&b.pitch)));
            if notes.is_empty() {
                p.notes_by_track.remove(&track_id);
            } else {
                p.notes_by_track.insert(track_id, notes);
            }
        }
        SetChords { pattern_id, chords } => {
            let p = m.pattern_mut(&pattern_id).ok_or(EditError::NoSuchPattern)?;
            let chords: Vec<ChordEvent> = chords
                .into_iter()
                .filter(|c| c.length_ticks > 0 && c.start_tick >= 0)
                .map(|mut c| {
                    c.pitch_classes = c.pitch_classes.iter().map(|&pc| crate::theory::normalize(pc)).collect();
                    c.pitch_classes.sort_unstable();
                    c.pitch_classes.dedup();
                    c
                })
                .collect();
            p.chords = crate::chord_track::ChordTrack::new(chords);
        }
        AddClip { pattern_id, start_tick, length_ticks } => {
            let p = m.pattern(&pattern_id).ok_or(EditError::NoSuchPattern)?;
            let len = length_ticks.unwrap_or(p.length_ticks);
            if start_tick < 0 || len <= 0 {
                return Err(EditError::InvalidValue);
            }
            m.clips.push(Clip::new(pattern_id, start_tick, len));
            m.clips.sort_by_key(|c| c.start_tick);
        }
        UpdateClip { clip } => {
            if m.pattern(&clip.pattern_id).is_none() {
                return Err(EditError::NoSuchPattern);
            }
            if clip.start_tick < 0 || clip.length_ticks <= 0 || clip.offset_ticks < 0 {
                return Err(EditError::InvalidValue);
            }
            let slot = m.clips.iter_mut().find(|c| c.id == clip.id).ok_or(EditError::NoSuchClip)?;
            *slot = clip;
            m.clips.sort_by_key(|c| c.start_tick);
        }
        RemoveClip { id } => {
            let before = m.clips.len();
            m.clips.retain(|c| c.id != id);
            if m.clips.len() == before {
                return Err(EditError::NoSuchClip);
            }
        }
        AddSection { name, start_tick } => {
            if start_tick < 0 {
                return Err(EditError::InvalidValue);
            }
            let name = if name.trim().is_empty() { format!("section {}", m.sections.len() + 1) } else { name };
            m.sections.push(Section::new(name, start_tick));
            m.sections.sort_by_key(|s| s.start_tick);
        }
        UpdateSection { section } => {
            if section.start_tick < 0 || section.name.trim().is_empty() {
                return Err(EditError::InvalidValue);
            }
            let slot = m.sections.iter_mut().find(|s| s.id == section.id).ok_or(EditError::NoSuchSection)?;
            *slot = section;
            m.sections.sort_by_key(|s| s.start_tick);
        }
        RemoveSection { id } => {
            let before = m.sections.len();
            m.sections.retain(|s| s.id != id);
            if m.sections.len() == before {
                return Err(EditError::NoSuchSection);
            }
        }
        SetTimeSignature { numerator, denominator } => {
            if !(1..=16).contains(&numerator) || ![2, 4, 8, 16].contains(&denominator) {
                return Err(EditError::InvalidValue);
            }
            m.time_signature = crate::model::TimeSignature { numerator, denominator };
        }
        ImportTracks { pattern_id, tracks, length_ticks } => {
            if m.pattern(&pattern_id).is_none() {
                return Err(EditError::NoSuchPattern);
            }
            if tracks.is_empty() || length_ticks < 0 {
                return Err(EditError::InvalidValue);
            }
            for t in tracks {
                let mut track = if t.is_drum { Track::drums(&t.name) } else { Track::new(&t.name) };
                if track.name.trim().is_empty() {
                    track.name = format!("track {}", m.tracks.len() + 1);
                }
                let mut notes: Vec<NoteEvent> = t.notes.into_iter().map(NoteEvent::sanitized).collect();
                notes.sort_by(|a, b| a.start_tick.cmp(&b.start_tick).then(a.pitch.cmp(&b.pitch)));
                let id = track.id;
                m.tracks.push(track);
                if !notes.is_empty() {
                    m.pattern_mut(&pattern_id).ok_or(EditError::NoSuchPattern)?.notes_by_track.insert(id, notes);
                }
            }
            let p = m.pattern_mut(&pattern_id).ok_or(EditError::NoSuchPattern)?;
            if length_ticks > p.length_ticks {
                p.length_ticks = length_ticks;
            }
        }
        AddTempoPoint { tick, bpm, ramp } => {
            if tick < 0 || !bpm.is_finite() {
                return Err(EditError::InvalidValue);
            }
            let bpm = bpm.clamp(crate::tempo::Tempo::MIN_BPM, crate::tempo::Tempo::MAX_BPM);
            m.tempo_points.retain(|p| p.tick != tick);
            m.tempo_points.push(TempoPoint::new(tick, bpm, ramp));
            m.tempo_points.sort_by_key(|p| p.tick);
        }
        UpdateTempoPoint { point } => {
            if point.tick < 0 || !point.bpm.is_finite() {
                return Err(EditError::InvalidValue);
            }
            let slot = m.tempo_points.iter_mut().find(|p| p.id == point.id).ok_or(EditError::NoSuchTempoPoint)?;
            *slot = TempoPoint { bpm: point.bpm.clamp(crate::tempo::Tempo::MIN_BPM, crate::tempo::Tempo::MAX_BPM), ..point };
            // Keep one point per tick: the edited one wins.
            let (id, tick) = (slot.id, slot.tick);
            m.tempo_points.retain(|p| p.id == id || p.tick != tick);
            m.tempo_points.sort_by_key(|p| p.tick);
        }
        RemoveTempoPoint { id } => {
            let before = m.tempo_points.len();
            m.tempo_points.retain(|p| p.id != id);
            if m.tempo_points.len() == before {
                return Err(EditError::NoSuchTempoPoint);
            }
        }
        SetSwing { swing, grid_ticks } => {
            if !swing.is_finite() || !(grid_ticks == TICKS_PER_BEAT / 2 || grid_ticks == TICKS_PER_BEAT / 4) {
                return Err(EditError::InvalidValue);
            }
            m.swing = swing.clamp(50.0, 75.0);
            m.swing_grid_ticks = grid_ticks;
        }
        SetMaster { master } => {
            let v = [
                master.gain,
                master.reverb_wet,
                master.low_cut_hz,
                master.high_cut_hz,
                master.low_eq,
                master.mid_eq,
                master.high_eq,
            ];
            if v.iter().any(|x| !x.is_finite()) {
                return Err(EditError::InvalidValue);
            }
            m.master = MasterSettings {
                gain: master.gain.clamp(0.0, 1.5),
                reverb_wet: master.reverb_wet.clamp(0.0, 1.0),
                low_cut_hz: master.low_cut_hz.clamp(10.0, 2_000.0),
                high_cut_hz: master.high_cut_hz.clamp(500.0, 20_000.0),
                low_eq: master.low_eq.clamp(0.0, 2.0),
                mid_eq: master.mid_eq.clamp(0.0, 2.0),
                high_eq: master.high_eq.clamp(0.0, 2.0),
            };
        }
        AddAudioTrack { name } => {
            let name = if name.trim().is_empty() { "audio".to_string() } else { name };
            m.tracks.push(Track::audio(name));
        }
        SetTrackAmp { id, amp } => {
            let amp = amp.sanitized().ok_or(EditError::InvalidValue)?;
            let t = m.track_mut(&id).ok_or(EditError::NoSuchTrack)?;
            if !t.is_audio {
                return Err(EditError::NotAnAudioTrack);
            }
            t.amp = Some(amp);
        }
        AddAudioTake { track_id, source, clip } => {
            // The clip must name the track and source it arrived with: a take that points
            // somewhere else would be dropped by `normalized` and silently vanish.
            if clip.track_id != track_id || clip.source_id != source.id {
                return Err(EditError::InvalidValue);
            }
            if source.sample_rate == 0 || source.channels == 0 || source.frames == 0 {
                return Err(EditError::InvalidValue);
            }
            let t = m.track(&track_id).ok_or(EditError::NoSuchTrack)?;
            if !t.is_audio {
                return Err(EditError::NotAnAudioTrack);
            }
            let clip = clip.sanitized().ok_or(EditError::InvalidValue)?;
            match m.audio_sources.iter_mut().find(|s| s.id == source.id) {
                Some(slot) => *slot = source,
                None => m.audio_sources.push(source),
            }
            m.audio_clips.push(clip);
            m.audio_clips.sort_by_key(|c| c.start_tick);
        }
        UpdateAudioClip { clip } => {
            if m.audio_source(&clip.source_id).is_none() {
                return Err(EditError::NoSuchAudioSource);
            }
            if !m.track(&clip.track_id).is_some_and(|t| t.is_audio) {
                return Err(EditError::NotAnAudioTrack);
            }
            let clip = clip.sanitized().ok_or(EditError::InvalidValue)?;
            let slot = m.audio_clips.iter_mut().find(|c| c.id == clip.id).ok_or(EditError::NoSuchAudioClip)?;
            *slot = clip;
            m.audio_clips.sort_by_key(|c| c.start_tick);
        }
        RemoveAudioClip { id } => {
            let before = m.audio_clips.len();
            m.audio_clips.retain(|c| c.id != id);
            if m.audio_clips.len() == before {
                return Err(EditError::NoSuchAudioClip);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ScaleType;

    fn doc() -> Document {
        Document::new(ProjectModel::starter())
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut d = doc();
        let original = d.model().clone();
        d.apply(Command::SetTempo { bpm: 90.0 }, false).unwrap();
        assert_eq!(d.model().tempo, 90.0);
        assert!(d.undo());
        assert_eq!(d.model(), &original);
        assert!(d.redo());
        assert_eq!(d.model().tempo, 90.0);
        assert!(!d.redo());
    }

    #[test]
    fn noop_command_does_not_create_undo_step() {
        let mut d = doc();
        d.apply(Command::SetTempo { bpm: 120.0 }, false).unwrap();
        assert!(!d.can_undo());
    }

    #[test]
    fn transient_edits_coalesce_into_one_undo_step() {
        let mut d = doc();
        let id = d.model().tracks[0].id;
        for v in [0.9, 0.8, 0.7, 0.6] {
            d.apply(Command::SetTrackParam { id, param: TrackParam::Volume, value: v }, true).unwrap();
        }
        d.commit_gesture();
        assert_eq!(d.model().tracks[0].volume, 0.6);
        assert!(d.undo());
        assert_eq!(d.model().tracks[0].volume, 1.0);
        assert!(!d.undo());
    }

    #[test]
    fn transient_run_is_closed_by_next_committed_command() {
        let mut d = doc();
        let id = d.model().tracks[0].id;
        d.apply(Command::SetTrackParam { id, param: TrackParam::Pan, value: -0.5 }, true).unwrap();
        d.apply(Command::SetTempo { bpm: 100.0 }, false).unwrap();
        assert!(d.undo()); // tempo
        assert_eq!(d.model().tracks[0].pan, -0.5);
        assert!(d.undo()); // pan gesture
        assert_eq!(d.model().tracks[0].pan, 0.0);
    }

    #[test]
    fn delete_track_removes_notes_and_guards_last() {
        let mut d = Document::new(ProjectModel::empty());
        assert_eq!(d.apply(Command::DeleteTrack { id: d.model().tracks[0].id }, false), Err(EditError::LastTrack));
        d.apply(Command::AddTrack { is_drum: false }, false).unwrap();
        let (pid, tid) = (d.model().patterns[0].id, d.model().tracks[1].id);
        d.apply(
            Command::SetNotes { pattern_id: pid, track_id: tid, notes: vec![NoteEvent::new(0, TICKS_PER_BEAT, 60)] },
            false,
        )
        .unwrap();
        d.apply(Command::DeleteTrack { id: tid }, false).unwrap();
        assert_eq!(d.model().tracks.len(), 1);
        assert!(d.model().patterns[0].notes_by_track.is_empty());
    }

    #[test]
    fn set_notes_sanitizes_and_sorts() {
        let mut d = doc();
        let (pid, tid) = (d.model().patterns[0].id, d.model().tracks[0].id);
        let notes = vec![
            NoteEvent::new(4 * TICKS_PER_BEAT, TICKS_PER_BEAT, 200).with_velocity(999),
            NoteEvent::new(-3 * TICKS_PER_BEAT, 0, 60),
        ];
        d.apply(Command::SetNotes { pattern_id: pid, track_id: tid, notes }, false).unwrap();
        let stored = d.model().patterns[0].notes(&tid);
        assert_eq!(stored[0].start_tick, 0);
        assert!(stored[0].length_ticks > 0);
        assert_eq!(stored[1].pitch, 127);
        assert_eq!(stored[1].velocity, 127);
    }

    #[test]
    fn add_pattern_seeds_chords_in_the_project_key() {
        let mut d = doc();
        d.apply(Command::SetKey { key: KeyState::locked(7, ScaleType::Major) }, false).unwrap();
        d.apply(Command::AddPattern, false).unwrap();
        let p = &d.model().patterns[1];
        assert_eq!(p.chords.chord_at(0).unwrap().name.as_deref(), Some("G"));
    }

    #[test]
    fn delete_pattern_removes_its_clips_and_guards_last() {
        let mut d = doc();
        let first = d.model().patterns[0].id;
        assert_eq!(d.apply(Command::DeletePattern { id: first }, false), Err(EditError::LastPattern));
        d.apply(Command::DuplicatePattern { id: first }, false).unwrap();
        let second = d.model().patterns[1].id;
        d.apply(Command::AddClip { pattern_id: second, start_tick: 16 * TICKS_PER_BEAT, length_ticks: None }, false).unwrap();
        d.apply(Command::AddClip { pattern_id: first, start_tick: 32 * TICKS_PER_BEAT, length_ticks: Some(8 * TICKS_PER_BEAT) }, false)
            .unwrap();
        assert_eq!(d.model().clips.len(), 3);
        d.apply(Command::DeletePattern { id: first }, false).unwrap();
        assert_eq!(d.model().clips.len(), 1);
        assert_eq!(d.model().clips[0].pattern_id, second);
    }

    #[test]
    fn clips_are_validated_sorted_and_removable() {
        let mut d = doc();
        let pid = d.model().patterns[0].id;
        assert_eq!(
            d.apply(Command::AddClip { pattern_id: Uuid::new_v4(), start_tick: 0, length_ticks: None }, false),
            Err(EditError::NoSuchPattern)
        );
        assert_eq!(
            d.apply(Command::AddClip { pattern_id: pid, start_tick: -1, length_ticks: None }, false),
            Err(EditError::InvalidValue)
        );
        d.apply(Command::AddClip { pattern_id: pid, start_tick: 64 * TICKS_PER_BEAT, length_ticks: Some(8 * TICKS_PER_BEAT) }, false)
            .unwrap();
        d.apply(Command::AddClip { pattern_id: pid, start_tick: 32 * TICKS_PER_BEAT, length_ticks: Some(8 * TICKS_PER_BEAT) }, false)
            .unwrap();
        let starts: Vec<i64> = d.model().clips.iter().map(|c| c.start_tick).collect();
        assert_eq!(starts, vec![0, 32 * TICKS_PER_BEAT, 64 * TICKS_PER_BEAT]);
        assert_eq!(d.model().song_length(), 72 * TICKS_PER_BEAT);
        let mut c = d.model().clips[1].clone();
        c.length_ticks = 0;
        assert_eq!(d.apply(Command::UpdateClip { clip: c.clone() }, false), Err(EditError::InvalidValue));
        c.length_ticks = 24 * TICKS_PER_BEAT;
        c.offset_ticks = 4 * TICKS_PER_BEAT;
        d.apply(Command::UpdateClip { clip: c.clone() }, false).unwrap();
        assert_eq!(d.model().clip(&c.id).unwrap().offset_ticks, 4 * TICKS_PER_BEAT);
        d.apply(Command::RemoveClip { id: c.id }, false).unwrap();
        assert_eq!(d.model().clips.len(), 2);
        assert_eq!(d.apply(Command::RemoveClip { id: c.id }, false), Err(EditError::NoSuchClip));
    }

    #[test]
    fn sections_carry_their_own_key() {
        let mut d = doc();
        d.apply(Command::SetKey { key: KeyState::locked(0, ScaleType::Major) }, false).unwrap();
        d.apply(Command::AddSection { name: "verse".into(), start_tick: 0 }, false).unwrap();
        d.apply(Command::AddSection { name: "".into(), start_tick: 16 * TICKS_PER_BEAT }, false).unwrap();
        assert_eq!(d.model().sections[1].name, "section 2");
        let mut bridge = d.model().sections[1].clone();
        bridge.key = Some(KeyState::locked(9, ScaleType::Minor));
        d.apply(Command::UpdateSection { section: bridge.clone() }, false).unwrap();
        assert_eq!(d.model().key_at(3 * TICKS_PER_BEAT).root_pitch_class, Some(0));
        assert_eq!(d.model().key_at(20 * TICKS_PER_BEAT).root_pitch_class, Some(9));
        assert_eq!(d.model().section_at(20 * TICKS_PER_BEAT).map(|s| s.name.as_str()), Some("section 2"));
        d.apply(Command::RemoveSection { id: bridge.id }, false).unwrap();
        assert_eq!(d.model().key_at(20 * TICKS_PER_BEAT).root_pitch_class, Some(0));
    }

    #[test]
    fn duplicate_pattern_gets_fresh_ids() {
        let mut d = doc();
        let (pid, tid) = (d.model().patterns[0].id, d.model().tracks[0].id);
        d.apply(
            Command::SetNotes { pattern_id: pid, track_id: tid, notes: vec![NoteEvent::new(0, TICKS_PER_BEAT, 60)] },
            false,
        )
        .unwrap();
        d.apply(Command::DuplicatePattern { id: pid }, false).unwrap();
        let (a, b) = (&d.model().patterns[0], &d.model().patterns[1]);
        assert_ne!(a.id, b.id);
        assert_ne!(a.notes(&tid)[0].id, b.notes(&tid)[0].id);
        assert_ne!(a.chords.chords()[0].id, b.chords.chords()[0].id);
        assert_eq!(b.name, "pattern 1 copy");
    }

    #[test]
    fn track_color_is_validated() {
        let mut d = doc();
        let id = d.model().tracks[0].id;
        d.apply(Command::SetTrackColor { id, color: Some("#FF3B30".into()) }, false).unwrap();
        assert_eq!(d.model().tracks[0].color.as_deref(), Some("#ff3b30"));
        assert_eq!(
            d.apply(Command::SetTrackColor { id, color: Some("red".into()) }, false),
            Err(EditError::InvalidValue)
        );
        d.apply(Command::SetTrackColor { id, color: None }, false).unwrap();
        assert!(d.model().tracks[0].color.is_none());
    }

    #[test]
    fn time_signature_is_validated() {
        let mut d = doc();
        d.apply(Command::SetTimeSignature { numerator: 7, denominator: 8 }, false).unwrap();
        assert_eq!(d.model().time_signature.beats_per_bar(), 3.5);
        assert_eq!(d.apply(Command::SetTimeSignature { numerator: 0, denominator: 4 }, false), Err(EditError::InvalidValue));
        assert_eq!(d.apply(Command::SetTimeSignature { numerator: 4, denominator: 3 }, false), Err(EditError::InvalidValue));
    }

    #[test]
    fn undo_labels_follow_the_history() {
        let mut d = doc();
        assert_eq!(d.undo_label(), None);
        d.apply(Command::SetTempo { bpm: 90.0 }, false).unwrap();
        assert_eq!(d.undo_label(), Some("set tempo"));
        let (pid, tid) = (d.model().patterns[0].id, d.model().tracks[0].id);
        d.apply_labeled(
            Command::SetNotes { pattern_id: pid, track_id: tid, notes: vec![NoteEvent::new(0, TICKS_PER_BEAT, 60)] },
            false,
            Some("place note".into()),
        )
        .unwrap();
        assert_eq!(d.undo_label(), Some("place note"));
        assert_eq!(d.redo_label(), None);
        d.undo();
        assert_eq!(d.undo_label(), Some("set tempo"));
        assert_eq!(d.redo_label(), Some("place note"));
        d.redo();
        assert_eq!(d.undo_label(), Some("place note"));
        // A transient gesture carries its label while it is still open.
        let id = tid;
        d.apply(Command::SetTrackParam { id, param: TrackParam::Pan, value: 0.3 }, true).unwrap();
        assert_eq!(d.undo_label(), Some("change pan"));
        d.commit_gesture();
        assert_eq!(d.undo_label(), Some("change pan"));
        // An empty label falls back to the command's own.
        d.apply_labeled(Command::SetTempo { bpm: 95.0 }, false, Some("  ".into())).unwrap();
        assert_eq!(d.undo_label(), Some("set tempo"));
    }

    #[test]
    fn revision_bumps_on_every_change_and_restore_stays_dirty() {
        let mut d = doc();
        let r0 = d.revision();
        d.apply(Command::SetTempo { bpm: 120.0 }, false).unwrap(); // no-op
        assert_eq!(d.revision(), r0);
        d.apply(Command::SetTempo { bpm: 99.0 }, false).unwrap();
        assert!(d.revision() > r0);
        let r1 = d.revision();
        d.undo();
        assert!(d.revision() > r1);
        d.mark_saved();
        assert!(!d.is_dirty());
        d.restore(ProjectModel::starter(), None);
        assert!(d.is_dirty());
        assert!(!d.can_undo());
    }

    #[test]
    fn import_tracks_is_one_undo_step() {
        let mut d = doc();
        let pid = d.model().patterns[0].id;
        let before = d.model().tracks.len();
        d.apply(
            Command::ImportTracks {
                pattern_id: pid,
                tracks: vec![
                    ImportedTrack { name: "piano".into(), is_drum: false, notes: vec![NoteEvent::new(0, TICKS_PER_BEAT, 60)] },
                    ImportedTrack { name: "kit".into(), is_drum: true, notes: vec![NoteEvent::new(0, TICKS_PER_BEAT / 4, 36)] },
                ],
                length_ticks: 32 * TICKS_PER_BEAT,
            },
            false,
        )
        .unwrap();
        assert_eq!(d.model().tracks.len(), before + 2);
        assert!(d.model().tracks[before + 1].is_drum);
        assert_eq!(d.model().patterns[0].length_ticks, 32 * TICKS_PER_BEAT);
        assert_eq!(d.undo_label(), Some("import MIDI"));
        d.undo();
        assert_eq!(d.model().tracks.len(), before);
        assert_eq!(d.model().patterns[0].length_ticks, 16 * TICKS_PER_BEAT);
    }

    #[test]
    fn tempo_points_are_edited_one_per_tick() {
        let mut d = doc();
        d.apply(Command::AddTempoPoint { tick: 16 * TICKS_PER_BEAT, bpm: 90.0, ramp: false }, false).unwrap();
        d.apply(Command::AddTempoPoint { tick: 16 * TICKS_PER_BEAT, bpm: 95.0, ramp: true }, false).unwrap();
        assert_eq!(d.model().tempo_points.len(), 1);
        assert_eq!(d.model().tempo_points[0].bpm, 95.0);
        assert!(d.model().tempo_points[0].ramp);
        assert_eq!(d.model().tempo_map().bpm_at_tick((20 * TICKS_PER_BEAT) as f64), 95.0);
        let mut p = d.model().tempo_points[0].clone();
        p.tick = 8 * TICKS_PER_BEAT;
        p.bpm = 999.0;
        d.apply(Command::UpdateTempoPoint { point: p.clone() }, false).unwrap();
        assert_eq!(d.model().tempo_points[0].bpm, 300.0);
        assert_eq!(d.model().tempo_points[0].tick, 8 * TICKS_PER_BEAT);
        d.apply(Command::RemoveTempoPoint { id: p.id }, false).unwrap();
        assert!(d.model().tempo_points.is_empty());
        assert_eq!(d.apply(Command::RemoveTempoPoint { id: p.id }, false), Err(EditError::NoSuchTempoPoint));
        assert_eq!(
            d.apply(Command::AddTempoPoint { tick: -1, bpm: 100.0, ramp: false }, false),
            Err(EditError::InvalidValue)
        );
    }

    #[test]
    fn swing_is_validated() {
        let mut d = doc();
        d.apply(Command::SetSwing { swing: 90.0, grid_ticks: TICKS_PER_BEAT / 4 }, false).unwrap();
        assert_eq!(d.model().swing, 75.0);
        assert_eq!(d.model().swing_grid_ticks, TICKS_PER_BEAT / 4);
        assert_eq!(
            d.apply(Command::SetSwing { swing: 60.0, grid_ticks: 300 }, false),
            Err(EditError::InvalidValue)
        );
    }
    /// Shortening a loop is also an edit to the material: the loop is the track's whole
    /// extent, so what no longer fits is cut (and one undo brings it back) instead of lingering
    /// as notes the roll shows but the engine never plays.
    #[test]
    fn shortening_a_loop_trims_the_material_past_it_and_undo_restores_it() {
        let mut d = Document::new(ProjectModel::starter());
        let (tid, pid) = (d.model().tracks[0].id, d.model().patterns[0].id);
        let notes = vec![
            NoteEvent::new(0, TICKS_PER_BEAT, 60),
            NoteEvent::new(6 * TICKS_PER_BEAT, 4 * TICKS_PER_BEAT, 62),
            NoteEvent::new(12 * TICKS_PER_BEAT, TICKS_PER_BEAT, 64),
        ];
        d.apply(Command::SetNotes { pattern_id: pid, track_id: tid, notes }, false).unwrap();

        d.apply(Command::SetTrackLoop { pattern_id: pid, track_id: tid, ticks: Some(8 * TICKS_PER_BEAT) }, false).unwrap();
        let kept = d.model().patterns[0].notes(&tid);
        assert_eq!(kept.len(), 2, "the note past the 8-beat loop should be gone");
        assert_eq!(kept[1].length_ticks, 2 * TICKS_PER_BEAT, "the note overrunning the loop should end at it");

        d.apply(Command::SetChordLoop { pattern_id: pid, ticks: Some(8 * TICKS_PER_BEAT) }, false).unwrap();
        let chords = d.model().patterns[0].chords.chords();
        assert_eq!(chords.len(), 2, "the starter's third and fourth chords sit past an 8-beat loop");

        d.undo();
        assert_eq!(d.model().patterns[0].chords.chords().len(), 4);
        d.undo();
        assert_eq!(d.model().patterns[0].notes(&tid).len(), 3);
    }

    /// Lengthening or clearing a loop never touches the material.
    #[test]
    fn widening_a_loop_leaves_the_material_alone() {
        let mut d = Document::new(ProjectModel::starter());
        let (tid, pid) = (d.model().tracks[0].id, d.model().patterns[0].id);
        let notes = vec![NoteEvent::new(0, TICKS_PER_BEAT, 60), NoteEvent::new(12 * TICKS_PER_BEAT, TICKS_PER_BEAT, 64)];
        d.apply(Command::SetNotes { pattern_id: pid, track_id: tid, notes }, false).unwrap();
        d.apply(Command::SetTrackLoop { pattern_id: pid, track_id: tid, ticks: Some(16 * TICKS_PER_BEAT) }, false).unwrap();
        assert_eq!(d.model().patterns[0].notes(&tid).len(), 2);
        d.apply(Command::SetTrackLoop { pattern_id: pid, track_id: tid, ticks: None }, false).unwrap();
        assert_eq!(d.model().patterns[0].notes(&tid).len(), 2);
        assert!(d.model().patterns[0].track_loop_ticks.is_empty());
    }

    #[test]
    fn intentional_flag_survives_set_notes() {
        let mut d = doc();
        let (pid, tid) = (d.model().patterns[0].id, d.model().tracks[0].id);
        let mut n = NoteEvent::new(0, TICKS_PER_BEAT, 65);
        n.intentional = true;
        d.apply(Command::SetNotes { pattern_id: pid, track_id: tid, notes: vec![n] }, false).unwrap();
        assert!(d.model().patterns[0].notes(&tid)[0].intentional);
    }

}
