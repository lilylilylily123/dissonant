//! The document: a [`ProjectModel`] plus undo/redo, mutated only through [`Command`]s.
//!
//! Every command is validated and applied here, so the UI, tests and any future scripting or
//! collaboration layer all go through one door. Undo is snapshot-based (the model is small);
//! continuous controls (sliders) send `transient` commands which coalesce into one undo step
//! per gesture.

use crate::model::{ChordEvent, KeyState, MasterSettings, NoteEvent, ProjectModel, SongPattern, Track};
use crate::theory::harmony;
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

/// Every edit the UI can make. Serialized as `{"type": "...", ...}` over Tauri IPC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
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
    AddPattern,
    DuplicatePattern { id: Uuid },
    DeletePattern { id: Uuid },
    RenamePattern { id: Uuid, name: String },
    SetPatternLength { id: Uuid, beats: f64 },
    /// Replace the notes of one track in one pattern (the roll commits whole arrays).
    SetNotes { pattern_id: Uuid, track_id: Uuid, notes: Vec<NoteEvent> },
    /// Replace a pattern's chord track.
    SetChords { pattern_id: Uuid, chords: Vec<ChordEvent> },
    SetArrangement { arrangement: Vec<Uuid> },
    SetMaster { master: MasterSettings },
    SetTimeSignature { numerator: u32, denominator: u32 },
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
}

#[derive(Debug, Clone)]
pub struct Document {
    model: ProjectModel,
    undo: Vec<ProjectModel>,
    redo: Vec<ProjectModel>,
    /// Snapshot taken at the start of a transient gesture, pushed to `undo` when it ends.
    pending: Option<ProjectModel>,
    pub path: Option<std::path::PathBuf>,
    dirty: bool,
}

const MAX_UNDO: usize = 200;

impl Document {
    pub fn new(model: ProjectModel) -> Self {
        Document {
            model,
            undo: vec![],
            redo: vec![],
            pending: None,
            path: None,
            dirty: false,
        }
    }

    pub fn model(&self) -> &ProjectModel {
        &self.model
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.pending.is_some()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Apply a command. `transient` edits (slider drags) don't create their own undo step;
    /// the first one in a run snapshots the model and the next non-transient command (or
    /// [`Document::commit_gesture`]) turns that snapshot into a single undo entry.
    pub fn apply(&mut self, command: Command, transient: bool) -> Result<(), EditError> {
        let before = self.model.clone();
        let mut next = self.model.clone();
        apply_to(&mut next, command)?;
        if next == before {
            return Ok(());
        }
        if transient {
            if self.pending.is_none() {
                self.pending = Some(before);
            }
        } else {
            self.commit_gesture();
            self.push_undo(before);
        }
        self.redo.clear();
        self.model = next;
        self.dirty = true;
        Ok(())
    }

    /// End a run of transient edits, making it one undo step.
    pub fn commit_gesture(&mut self) {
        if let Some(snapshot) = self.pending.take() {
            if snapshot != self.model {
                self.push_undo(snapshot);
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        self.commit_gesture();
        match self.undo.pop() {
            Some(previous) => {
                self.redo.push(std::mem::replace(&mut self.model, previous));
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        self.commit_gesture();
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(std::mem::replace(&mut self.model, next));
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    /// Replace the whole model (new / open). Clears history.
    pub fn replace(&mut self, model: ProjectModel, path: Option<std::path::PathBuf>) {
        self.model = model;
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.path = path;
        self.dirty = false;
    }

    fn push_undo(&mut self, snapshot: ProjectModel) {
        self.undo.push(snapshot);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }
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
            }
        }
        RenameTrack { id, name } => m.track_mut(&id).ok_or(EditError::NoSuchTrack)?.name = name,
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
        AddPattern => {
            let mut p = SongPattern::new(format!("pattern {}", m.patterns.len() + 1));
            let root = m.key.root_pitch_class.unwrap_or(0);
            p.chords = crate::chord_track::ChordTrack::new(harmony::progression(
                &harmony::starters()[0].degrees,
                root,
                m.key.scale,
                p.length_beats,
                4.0,
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
            m.arrangement.retain(|pid| *pid != id);
        }
        RenamePattern { id, name } => {
            if name.trim().is_empty() {
                return Err(EditError::InvalidValue);
            }
            m.pattern_mut(&id).ok_or(EditError::NoSuchPattern)?.name = name;
        }
        SetPatternLength { id, beats } => {
            if !beats.is_finite() || beats < 1.0 {
                return Err(EditError::InvalidValue);
            }
            m.pattern_mut(&id).ok_or(EditError::NoSuchPattern)?.length_beats = beats;
        }
        SetNotes { pattern_id, track_id, notes } => {
            if m.track(&track_id).is_none() {
                return Err(EditError::NoSuchTrack);
            }
            let p = m.pattern_mut(&pattern_id).ok_or(EditError::NoSuchPattern)?;
            let mut notes: Vec<NoteEvent> = notes.into_iter().map(NoteEvent::sanitized).collect();
            notes.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat).then(a.pitch.cmp(&b.pitch)));
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
                .filter(|c| c.length_beats > 0.0 && c.start_beat >= 0.0)
                .map(|mut c| {
                    c.pitch_classes = c.pitch_classes.iter().map(|&pc| crate::theory::normalize(pc)).collect();
                    c.pitch_classes.sort_unstable();
                    c.pitch_classes.dedup();
                    c
                })
                .collect();
            p.chords = crate::chord_track::ChordTrack::new(chords);
        }
        SetArrangement { arrangement } => {
            m.arrangement = arrangement.into_iter().filter(|id| m.patterns.iter().any(|p| &p.id == id)).collect();
        }
        SetTimeSignature { numerator, denominator } => {
            if !(1..=16).contains(&numerator) || ![2, 4, 8, 16].contains(&denominator) {
                return Err(EditError::InvalidValue);
            }
            m.time_signature = crate::model::TimeSignature { numerator, denominator };
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
            Command::SetNotes { pattern_id: pid, track_id: tid, notes: vec![NoteEvent::new(0.0, 1.0, 60)] },
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
            NoteEvent::new(4.0, 1.0, 200).with_velocity(999),
            NoteEvent::new(-3.0, 0.0, 60),
        ];
        d.apply(Command::SetNotes { pattern_id: pid, track_id: tid, notes }, false).unwrap();
        let stored = d.model().patterns[0].notes(&tid);
        assert_eq!(stored[0].start_beat, 0.0);
        assert!(stored[0].length_beats > 0.0);
        assert_eq!(stored[1].pitch, 127);
        assert_eq!(stored[1].velocity, 127);
    }

    #[test]
    fn add_pattern_seeds_chords_in_the_project_key() {
        let mut d = doc();
        d.apply(Command::SetKey { key: KeyState::locked(7, ScaleType::Major) }, false).unwrap();
        d.apply(Command::AddPattern, false).unwrap();
        let p = &d.model().patterns[1];
        assert_eq!(p.chords.chord_at(0.0).unwrap().name.as_deref(), Some("G"));
    }

    #[test]
    fn delete_pattern_updates_arrangement_and_guards_last() {
        let mut d = doc();
        let first = d.model().patterns[0].id;
        assert_eq!(d.apply(Command::DeletePattern { id: first }, false), Err(EditError::LastPattern));
        d.apply(Command::DuplicatePattern { id: first }, false).unwrap();
        let second = d.model().patterns[1].id;
        d.apply(Command::SetArrangement { arrangement: vec![first, second, first] }, false).unwrap();
        d.apply(Command::DeletePattern { id: first }, false).unwrap();
        assert_eq!(d.model().arrangement, vec![second]);
    }

    #[test]
    fn duplicate_pattern_gets_fresh_ids() {
        let mut d = doc();
        let (pid, tid) = (d.model().patterns[0].id, d.model().tracks[0].id);
        d.apply(
            Command::SetNotes { pattern_id: pid, track_id: tid, notes: vec![NoteEvent::new(0.0, 1.0, 60)] },
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
    fn intentional_flag_survives_set_notes() {
        let mut d = doc();
        let (pid, tid) = (d.model().patterns[0].id, d.model().tracks[0].id);
        let mut n = NoteEvent::new(0.0, 1.0, 65);
        n.intentional = true;
        d.apply(Command::SetNotes { pattern_id: pid, track_id: tid, notes: vec![n] }, false).unwrap();
        assert!(d.model().patterns[0].notes(&tid)[0].intentional);
    }

    #[test]
    fn arrangement_drops_unknown_pattern_ids() {
        let mut d = doc();
        let pid = d.model().patterns[0].id;
        d.apply(Command::SetArrangement { arrangement: vec![pid, Uuid::new_v4()] }, false).unwrap();
        assert_eq!(d.model().arrangement, vec![pid]);
    }
}
