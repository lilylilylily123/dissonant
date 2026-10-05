//! The Tauri shell: owns the [`Document`] and the [`AudioDevice`], exposes commands to the UI,
//! and streams the playhead back as events. All model edits go through `apply`, which keeps
//! undo/redo in one place and re-feeds the engine a fresh [`Sequence`] after every change.

use dissonant_core::{Command, Document, MasterSettings, NoteEvent, ProjectModel, Sequence};
use dissonant_engine::{default_output_name, render_wav, AudioDevice, EngineCommand, RenderOptions};
use midir::{MidiInput, MidiInputConnection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, State, WindowEvent};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PlayMode {
    #[default]
    Pattern,
    Song,
}

struct PlaybackContext {
    mode: PlayMode,
    pattern_id: Option<Uuid>,
    hear_chords: bool,
    /// The track live input (MIDI / typing keyboard) plays and records into.
    live_track: Option<Uuid>,
    /// Mirrors of engine transport settings, re-sent when the audio device restarts.
    looping: bool,
    loop_region: Option<(f64, f64)>,
}

impl Default for PlaybackContext {
    fn default() -> Self {
        PlaybackContext {
            mode: PlayMode::Pattern,
            pattern_id: None,
            hear_chords: false,
            live_track: None,
            looping: true,
            loop_region: None,
        }
    }
}

/// Record-arm state. Held notes are keyed by (track, pitch) with their start beat + velocity.
#[derive(Default)]
struct Recorder {
    armed: bool,
    /// Grid to snap recorded note starts to (0 = off).
    quantize: f64,
    held: HashMap<(Uuid, u8), (f64, u8)>,
}

pub struct AppState {
    doc: Mutex<Document>,
    audio: Mutex<Option<AudioDevice>>,
    audio_error: Mutex<Option<String>>,
    playback: Mutex<PlaybackContext>,
    exporting: AtomicBool,
    midi: Mutex<Option<MidiInputConnection<()>>>,
    midi_port: Mutex<Option<String>>,
    midi_last_ms: AtomicU64,
    recorder: Mutex<Recorder>,
    /// Tauri app-data dir (recent files, untitled autosaves). Set once in `setup`.
    data_dir: Mutex<Option<PathBuf>>,
    /// Document revision the last autosave captured.
    autosaved_revision: AtomicU64,
    /// The autosave file currently on disk for this document, if any.
    autosave_path: Mutex<Option<PathBuf>>,
    /// Set by `quit` so the close handler lets the window go.
    force_close: AtomicBool,
}

/// What every editing command returns to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub model: ProjectModel,
    pub can_undo: bool,
    pub can_redo: bool,
    pub dirty: bool,
    pub path: Option<String>,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
}

/// An autosave that may hold work newer than its project file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCandidate {
    pub autosave_path: String,
    pub project_path: Option<String>,
    pub saved_at_ms: u64,
}

/// The on-disk autosave format: the model plus where it belongs.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutosaveFile {
    project_path: Option<String>,
    saved_at_ms: u64,
    model: ProjectModel,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatusEvent {
    pub status: AudioStatus,
    pub message: String,
}

const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);
const MAX_RECENT: usize = 12;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayheadEvent {
    pub beat: f64,
    pub playing: bool,
    pub master_peak: [f32; 2],
    pub track_peaks: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatus {
    pub running: bool,
    pub sample_rate: Option<u32>,
    pub error: Option<String>,
    pub device_name: Option<String>,
}

impl AppState {
    fn new() -> Self {
        AppState {
            doc: Mutex::new(Document::new(ProjectModel::starter())),
            audio: Mutex::new(None),
            audio_error: Mutex::new(None),
            playback: Mutex::new(PlaybackContext::default()),
            exporting: AtomicBool::new(false),
            midi: Mutex::new(None),
            midi_port: Mutex::new(None),
            midi_last_ms: AtomicU64::new(0),
            recorder: Mutex::new(Recorder::default()),
            data_dir: Mutex::new(None),
            autosaved_revision: AtomicU64::new(0),
            autosave_path: Mutex::new(None),
            force_close: AtomicBool::new(false),
        }
    }

    fn live_track(&self) -> Option<Uuid> {
        let ctx = self.playback.lock().unwrap();
        ctx.live_track
            .filter(|id| self.doc.lock().unwrap().model().track(id).is_some())
            .or_else(|| self.doc.lock().unwrap().model().tracks.first().map(|t| t.id))
    }

    fn engine_position(&self) -> Option<(f64, bool)> {
        let audio = self.audio.lock().unwrap();
        audio.as_ref().map(|a| (a.shared().position_beats(), a.shared().is_playing()))
    }

    fn snapshot(&self) -> Snapshot {
        let doc = self.doc.lock().unwrap();
        Snapshot {
            model: doc.model().clone(),
            can_undo: doc.can_undo(),
            can_redo: doc.can_redo(),
            dirty: doc.is_dirty(),
            path: doc.path.as_ref().map(|p| p.display().to_string()),
            undo_label: doc.undo_label().map(str::to_string),
            redo_label: doc.redo_label().map(str::to_string),
        }
    }

    fn audio_status(&self) -> AudioStatus {
        let audio = self.audio.lock().unwrap();
        AudioStatus {
            running: audio.is_some(),
            sample_rate: audio.as_ref().map(|a| a.sample_rate),
            error: self.audio_error.lock().unwrap().clone(),
            device_name: audio.as_ref().map(|a| a.device_name.clone()),
        }
    }

    /// Re-send the transport settings the engine forgets when its device is rebuilt.
    fn resend_transport(&self) {
        let (hear_chords, looping, loop_region) = {
            let ctx = self.playback.lock().unwrap();
            (ctx.hear_chords, ctx.looping, ctx.loop_region)
        };
        self.send(EngineCommand::SetHearChords(hear_chords));
        self.send(EngineCommand::SetLooping(looping));
        match loop_region {
            Some((start, end)) => self.send(EngineCommand::SetLoop { start, end }),
            None => self.send(EngineCommand::ClearLoop),
        }
    }

    /// Tear the device down and open the current default again, keeping the transport where
    /// it was. Returns the new status.
    fn restart_audio(&self) -> AudioStatus {
        let was = self.engine_position();
        *self.audio.lock().unwrap() = None;
        self.start_audio();
        self.sync_engine();
        self.resend_transport();
        if let Some((beat, playing)) = was {
            self.send(EngineCommand::Seek { beat });
            if playing {
                self.send(EngineCommand::Play);
            }
        }
        self.audio_status()
    }

    // ── Files on disk: recent list and autosave ───────────────────────────────────────────

    fn data_dir(&self) -> Option<PathBuf> {
        self.data_dir.lock().unwrap().clone()
    }

    fn recent_path(&self) -> Option<PathBuf> {
        self.data_dir().map(|d| d.join("recent.json"))
    }

    fn load_recent(&self) -> Vec<String> {
        let Some(path) = self.recent_path() else { return vec![] };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
            .unwrap_or_default()
    }

    fn save_recent(&self, list: &[String]) {
        if let Some(path) = self.recent_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_string_pretty(list) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    fn remember_recent(&self, file: &std::path::Path) {
        let shown = file.display().to_string();
        let mut list = self.load_recent();
        list.retain(|p| p != &shown);
        list.insert(0, shown);
        list.truncate(MAX_RECENT);
        self.save_recent(&list);
    }

    /// Where this document's autosave goes: next to the project, or in app data for untitled.
    fn autosave_target(&self, project: Option<&std::path::Path>) -> Option<PathBuf> {
        match project {
            Some(p) => Some(autosave_path_for(p)),
            None => self.data_dir().map(|d| d.join("autosave").join("untitled.autosave.json")),
        }
    }

    /// Write an autosave if the document changed since the last one. Returns the path written.
    fn autosave(&self) -> Option<PathBuf> {
        let (json, path, revision) = {
            let doc = self.doc.lock().unwrap();
            if !doc.is_dirty() || doc.revision() == self.autosaved_revision.load(Ordering::Relaxed) {
                return None;
            }
            let target = self.autosave_target(doc.path.as_deref())?;
            let file = AutosaveFile {
                project_path: doc.path.as_ref().map(|p| p.display().to_string()),
                saved_at_ms: now_ms(),
                model: doc.model().clone(),
            };
            (serde_json::to_string(&file).ok()?, target, doc.revision())
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path)).is_err() {
            let _ = std::fs::remove_file(&tmp);
            log::warn!("autosave failed: {}", path.display());
            return None;
        }
        self.autosaved_revision.store(revision, Ordering::Relaxed);
        *self.autosave_path.lock().unwrap() = Some(path.clone());
        Some(path)
    }

    /// Delete this document's autosave (after a real save, or when the document is replaced).
    fn clear_autosave(&self) {
        if let Some(path) = self.autosave_path.lock().unwrap().take() {
            let _ = std::fs::remove_file(path);
        }
        let revision = self.doc.lock().unwrap().revision();
        self.autosaved_revision.store(revision, Ordering::Relaxed);
    }

    /// Autosaves lying around from an earlier run: the untitled slot plus one next to any
    /// recent project, when it is newer than the project itself.
    fn recovery_candidates(&self) -> Vec<RecoveryCandidate> {
        let mut out = Vec::new();
        let mut consider = |autosave: PathBuf| {
            let Ok(json) = std::fs::read_to_string(&autosave) else { return };
            let Ok(file) = serde_json::from_str::<AutosaveFile>(&json) else { return };
            if let Some(project) = &file.project_path {
                // Skip autosaves older than the project they belong to (a stale leftover).
                let project_ms = std::fs::metadata(project)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64);
                if matches!(project_ms, Some(ms) if ms >= file.saved_at_ms) {
                    let _ = std::fs::remove_file(&autosave);
                    return;
                }
            }
            out.push(RecoveryCandidate {
                autosave_path: autosave.display().to_string(),
                project_path: file.project_path,
                saved_at_ms: file.saved_at_ms,
            });
        };
        if let Some(untitled) = self.autosave_target(None) {
            if untitled.exists() {
                consider(untitled);
            }
        }
        for recent in self.load_recent() {
            let path = autosave_path_for(std::path::Path::new(&recent));
            if path.exists() {
                consider(path);
            }
        }
        out.sort_by_key(|c| std::cmp::Reverse(c.saved_at_ms));
        out
    }

    /// Replace the document and forget the previous one's autosave.
    fn load_model(&self, model: ProjectModel, path: Option<PathBuf>, keep_dirty: bool) {
        self.clear_autosave();
        {
            let mut doc = self.doc.lock().unwrap();
            if keep_dirty {
                doc.restore(model, path);
            } else {
                doc.replace(model, path);
            }
        }
        self.playback.lock().unwrap().pattern_id = None;
        self.send(EngineCommand::Stop);
        self.send(EngineCommand::Seek { beat: 0.0 });
        self.sync_engine();
    }

    fn send(&self, cmd: EngineCommand) {
        if let Some(audio) = self.audio.lock().unwrap().as_ref() {
            if let Err(e) = audio.send(cmd) {
                log::warn!("engine command dropped: {e}");
            }
        }
    }

    /// Rebuild the engine's sequence from the current model and playback context.
    fn sync_engine(&self) {
        let seq = {
            let doc = self.doc.lock().unwrap();
            let ctx = self.playback.lock().unwrap();
            build_sequence(doc.model(), ctx.mode, ctx.pattern_id)
        };
        let master = self.doc.lock().unwrap().model().master.clone();
        self.send(EngineCommand::SetSequence(Arc::new(seq)));
        self.send(EngineCommand::SetMaster(master));
    }

    fn start_audio(&self) {
        let master = self.doc.lock().unwrap().model().master.clone();
        match AudioDevice::start(master) {
            Ok(device) => {
                *self.audio.lock().unwrap() = Some(device);
                *self.audio_error.lock().unwrap() = None;
            }
            Err(e) => {
                log::error!("audio device failed to start: {e}");
                *self.audio_error.lock().unwrap() = Some(e.to_string());
            }
        }
    }
}

/// `song.dissonant` → `song.dissonant.autosave.json`, next to the project.
fn autosave_path_for(project: &std::path::Path) -> PathBuf {
    let mut name = project.file_name().map(|n| n.to_os_string()).unwrap_or_else(|| "project".into());
    name.push(".autosave.json");
    project.with_file_name(name)
}

fn build_sequence(model: &ProjectModel, mode: PlayMode, pattern_id: Option<Uuid>) -> Sequence {
    match mode {
        PlayMode::Song if !model.clips.is_empty() => Sequence::from_song(model),
        _ => {
            let pid = pattern_id
                .filter(|id| model.pattern(id).is_some())
                .or_else(|| model.patterns.first().map(|p| p.id));
            pid.and_then(|id| Sequence::from_pattern(model, &id))
                .unwrap_or_else(|| Sequence::from_song(model))
        }
    }
}

// ─── Document commands ───────────────────────────────────────────────────────────────────────

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> Snapshot {
    state.snapshot()
}

#[tauri::command]
fn apply(state: State<'_, AppState>, command: Command, transient: Option<bool>, label: Option<String>) -> Result<Snapshot, String> {
    state
        .doc
        .lock()
        .unwrap()
        .apply_labeled(command, transient.unwrap_or(false), label)
        .map_err(|e| e.to_string())?;
    state.sync_engine();
    Ok(state.snapshot())
}

#[tauri::command]
fn commit_gesture(state: State<'_, AppState>) -> Snapshot {
    state.doc.lock().unwrap().commit_gesture();
    state.snapshot()
}

#[tauri::command]
fn undo(state: State<'_, AppState>) -> Snapshot {
    state.doc.lock().unwrap().undo();
    state.sync_engine();
    state.snapshot()
}

#[tauri::command]
fn redo(state: State<'_, AppState>) -> Snapshot {
    state.doc.lock().unwrap().redo();
    state.sync_engine();
    state.snapshot()
}

#[tauri::command]
fn new_project(state: State<'_, AppState>, starter: Option<bool>) -> Snapshot {
    let model = if starter.unwrap_or(true) {
        ProjectModel::starter()
    } else {
        ProjectModel::empty()
    };
    state.load_model(model, None, false);
    state.snapshot()
}

#[tauri::command]
fn open_project(state: State<'_, AppState>, path: String) -> Result<Snapshot, String> {
    let json = std::fs::read_to_string(&path).map_err(|e| format!("could not read {path}: {e}"))?;
    let model = ProjectModel::from_json(&json).map_err(|e| format!("not a dissonant project: {e}"))?;
    let file = PathBuf::from(&path);
    state.remember_recent(&file);
    state.load_model(model, Some(file), false);
    Ok(state.snapshot())
}

#[tauri::command]
fn save_project(state: State<'_, AppState>, path: Option<String>) -> Result<Snapshot, String> {
    let mut doc = state.doc.lock().unwrap();
    let target = match path {
        Some(p) => PathBuf::from(p),
        None => doc.path.clone().ok_or_else(|| "no file path yet — use Save As".to_string())?,
    };
    let json = doc.model().to_json().map_err(|e| e.to_string())?;
    std::fs::write(&target, json).map_err(|e| format!("could not write {}: {e}", target.display()))?;
    doc.path = Some(target.clone());
    doc.mark_saved();
    drop(doc);
    state.remember_recent(&target);
    state.clear_autosave();
    // A stale autosave next to the file would otherwise be offered on the next launch.
    let _ = std::fs::remove_file(autosave_path_for(&target));
    Ok(state.snapshot())
}

// ─── Window, recent files, recovery ─────────────────────────────────────────────────────────

/// Close the app for real (the close handler asks first while the document is dirty).
#[tauri::command]
fn quit(app: AppHandle, state: State<'_, AppState>) {
    state.force_close.store(true, Ordering::SeqCst);
    state.clear_autosave();
    app.exit(0);
}

#[tauri::command]
fn recent_files(state: State<'_, AppState>) -> Vec<String> {
    state.load_recent().into_iter().filter(|p| std::path::Path::new(p).exists()).collect()
}

#[tauri::command]
fn clear_recent(state: State<'_, AppState>) {
    state.save_recent(&[]);
}

#[tauri::command]
fn recovery_candidates(state: State<'_, AppState>) -> Vec<RecoveryCandidate> {
    state.recovery_candidates()
}

/// Load an autosave as the current (dirty) document, bound to its original path if it had one.
#[tauri::command]
fn restore_autosave(state: State<'_, AppState>, autosave_path: String) -> Result<Snapshot, String> {
    let json = std::fs::read_to_string(&autosave_path).map_err(|e| format!("could not read autosave: {e}"))?;
    let file: AutosaveFile = serde_json::from_str(&json).map_err(|e| format!("not an autosave: {e}"))?;
    let path = file.project_path.map(PathBuf::from);
    state.load_model(file.model.normalized(), path, true);
    // The restored document now owns this autosave file.
    *state.autosave_path.lock().unwrap() = Some(PathBuf::from(autosave_path));
    Ok(state.snapshot())
}

#[tauri::command]
fn discard_autosave(autosave_path: String) {
    let _ = std::fs::remove_file(autosave_path);
}

/// Write the autosave now (the UI calls this before risky operations; the timer covers the rest).
#[tauri::command]
fn autosave_now(state: State<'_, AppState>) -> Option<String> {
    state.autosave().map(|p| p.display().to_string())
}

// ─── Transport ───────────────────────────────────────────────────────────────────────────────

#[tauri::command]
fn set_playback_context(state: State<'_, AppState>, mode: PlayMode, pattern_id: Option<Uuid>) {
    {
        let mut ctx = state.playback.lock().unwrap();
        let changed = ctx.mode != mode || ctx.pattern_id != pattern_id;
        ctx.mode = mode;
        ctx.pattern_id = pattern_id;
        if changed {
            state.send(EngineCommand::Seek { beat: 0.0 });
        }
    }
    state.sync_engine();
}

#[tauri::command]
fn transport_play(state: State<'_, AppState>) {
    state.send(EngineCommand::Play);
}

#[tauri::command]
fn transport_stop(state: State<'_, AppState>) {
    state.send(EngineCommand::Stop);
}

#[tauri::command]
fn transport_seek(state: State<'_, AppState>, beat: f64) {
    state.send(EngineCommand::Seek { beat });
}

#[tauri::command]
fn set_hear_chords(state: State<'_, AppState>, on: bool) {
    state.playback.lock().unwrap().hear_chords = on;
    state.send(EngineCommand::SetHearChords(on));
}

#[tauri::command]
fn audition(state: State<'_, AppState>, track_id: Uuid, pitch: u8, velocity: Option<u8>) {
    state.send(EngineCommand::Audition {
        track_id,
        pitch,
        velocity: velocity.unwrap_or(100),
    });
}

#[tauri::command]
fn audition_off(state: State<'_, AppState>, track_id: Uuid, pitch: u8) {
    state.send(EngineCommand::AuditionOff { track_id, pitch });
}

#[tauri::command]
fn audio_status(state: State<'_, AppState>) -> AudioStatus {
    state.audio_status()
}

#[tauri::command]
fn restart_audio(state: State<'_, AppState>) -> AudioStatus {
    state.restart_audio()
}

// ─── Live input: MIDI + typing keyboard, with record-arm ──────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MidiStatus {
    pub inputs: Vec<String>,
    pub open: Option<String>,
    pub armed: bool,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn midi_inputs() -> Vec<String> {
    match MidiInput::new("dissonant-scan") {
        Ok(input) => input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect(),
        Err(_) => vec![],
    }
}

/// A note started (controller or typing keyboard): sound it on the live track and, when
/// armed and playing, remember when it started.
fn live_note_on(app: &AppHandle, pitch: u8, velocity: u8) {
    let state = app.state::<AppState>();
    let Some(track_id) = state.live_track() else { return };
    state.send(EngineCommand::NoteOn { track_id, pitch, velocity });
    state.midi_last_ms.store(now_ms(), Ordering::Relaxed);
    let mut rec = state.recorder.lock().unwrap();
    if rec.armed {
        if let Some((beat, true)) = state.engine_position() {
            rec.held.insert((track_id, pitch), (beat, velocity));
        }
    }
    let _ = app.emit("midi-activity", pitch);
}

/// A note ended: release it and, if it was being recorded, write it into the pattern.
fn live_note_off(app: &AppHandle, pitch: u8) {
    let state = app.state::<AppState>();
    let Some(track_id) = state.live_track() else { return };
    state.send(EngineCommand::NoteOff { track_id, pitch });
    let held = state.recorder.lock().unwrap().held.remove(&(track_id, pitch));
    let Some((start, velocity)) = held else { return };
    let Some((end, _)) = state.engine_position() else { return };
    let quantize = state.recorder.lock().unwrap().quantize;

    let (pattern_id, length_beats, existing) = {
        let ctx = state.playback.lock().unwrap();
        let doc = state.doc.lock().unwrap();
        let model = doc.model();
        let Some(p) = ctx
            .pattern_id
            .and_then(|id| model.pattern(&id))
            .or_else(|| model.patterns.first())
        else {
            return;
        };
        (p.id, p.length_beats, p.notes(&track_id).to_vec())
    };
    // In song mode the engine position is a song position; fold it into the pattern.
    let start_in = start % length_beats;
    let mut len = end - start;
    if len < 0.0 {
        len += length_beats; // the loop wrapped while the key was held
    }
    let grid = if quantize > 0.0 { quantize } else { 1.0 / 32.0 };
    let snapped_start = (start_in / grid).round() * grid;
    let snapped_len = ((len / grid).round() * grid).max(grid);
    let note = NoteEvent::new(snapped_start.min(length_beats - grid).max(0.0), snapped_len, pitch as i32).with_velocity(velocity as i32);

    let mut notes = existing;
    // Replace a note that starts on the same pitch and grid slot instead of stacking.
    notes.retain(|n| !(n.pitch == note.pitch && (n.start_beat - note.start_beat).abs() < grid / 2.0));
    notes.push(note);
    let applied = state
        .doc
        .lock()
        .unwrap()
        .apply(Command::SetNotes { pattern_id, track_id, notes }, false);
    if applied.is_ok() {
        state.sync_engine();
        let _ = app.emit("document", state.snapshot());
    }
}

#[tauri::command]
fn midi_status(state: State<'_, AppState>) -> MidiStatus {
    MidiStatus {
        inputs: midi_inputs(),
        open: state.midi_port.lock().unwrap().clone(),
        armed: state.recorder.lock().unwrap().armed,
    }
}

#[tauri::command]
fn open_midi_input(app: AppHandle, state: State<'_, AppState>, name: Option<String>) -> Result<MidiStatus, String> {
    let input = MidiInput::new("dissonant").map_err(|e| e.to_string())?;
    let ports = input.ports();
    let port = match &name {
        Some(n) => ports.iter().find(|p| input.port_name(p).ok().as_deref() == Some(n.as_str())),
        None => ports.first(),
    }
    .cloned()
    .ok_or_else(|| "no MIDI input found".to_string())?;
    let port_name = input.port_name(&port).unwrap_or_else(|_| "midi".into());
    let handle = app.clone();
    let conn = input
        .connect(
            &port,
            "dissonant-in",
            move |_stamp, message, _| {
                if message.len() < 3 {
                    return;
                }
                match message[0] & 0xF0 {
                    0x90 if message[2] > 0 => live_note_on(&handle, message[1], message[2]),
                    0x80 | 0x90 => live_note_off(&handle, message[1]),
                    _ => {}
                }
            },
            (),
        )
        .map_err(|e| e.to_string())?;
    *state.midi.lock().unwrap() = Some(conn);
    *state.midi_port.lock().unwrap() = Some(port_name);
    Ok(midi_status(state))
}

#[tauri::command]
fn close_midi_input(state: State<'_, AppState>) -> MidiStatus {
    *state.midi.lock().unwrap() = None;
    *state.midi_port.lock().unwrap() = None;
    midi_status(state)
}

#[tauri::command]
fn set_live_track(state: State<'_, AppState>, track_id: Option<Uuid>) {
    state.playback.lock().unwrap().live_track = track_id;
}

#[tauri::command]
fn set_record(state: State<'_, AppState>, armed: bool, quantize: Option<f64>) -> MidiStatus {
    let mut rec = state.recorder.lock().unwrap();
    rec.armed = armed;
    if let Some(q) = quantize {
        rec.quantize = q.max(0.0);
    }
    if !armed {
        rec.held.clear();
    }
    drop(rec);
    midi_status(state)
}

#[tauri::command]
fn note_on(app: AppHandle, pitch: u8, velocity: Option<u8>) {
    live_note_on(&app, pitch, velocity.unwrap_or(100));
}

#[tauri::command]
fn note_off(app: AppHandle, pitch: u8) {
    live_note_off(&app, pitch);
}

#[tauri::command]
fn set_loop(state: State<'_, AppState>, start: f64, end: f64) {
    state.playback.lock().unwrap().loop_region = if end > start { Some((start, end)) } else { None };
    state.send(EngineCommand::SetLoop { start, end });
}

#[tauri::command]
fn clear_loop(state: State<'_, AppState>) {
    state.playback.lock().unwrap().loop_region = None;
    state.send(EngineCommand::ClearLoop);
}

#[tauri::command]
fn set_looping(state: State<'_, AppState>, on: bool) {
    state.playback.lock().unwrap().looping = on;
    state.send(EngineCommand::SetLooping(on));
}

// ─── Export ──────────────────────────────────────────────────────────────────────────────────

/// Render the song (or the current pattern when the arrangement is empty) to a WAV file,
/// faster than real time, on a worker thread. Resolves when the file is written.
#[tauri::command]
async fn export_wav(state: State<'_, AppState>, path: String, tail_seconds: Option<f64>) -> Result<String, String> {
    if state.exporting.swap(true, Ordering::SeqCst) {
        return Err("an export is already running".into());
    }
    let (sequence, master, hear_chords) = {
        let doc = state.doc.lock().unwrap();
        let ctx = state.playback.lock().unwrap();
        let seq = if doc.model().clips.is_empty() {
            build_sequence(doc.model(), PlayMode::Pattern, ctx.pattern_id)
        } else {
            Sequence::from_song(doc.model())
        };
        (seq, doc.model().master.clone(), ctx.hear_chords)
    };
    let options = RenderOptions {
        sample_rate: 44_100,
        tail_seconds: tail_seconds.unwrap_or(1.5),
        hear_chords,
        master,
    };
    let target = PathBuf::from(&path);
    let result = tauri::async_runtime::spawn_blocking(move || render_wav(Arc::new(sequence), &target, &options))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r.map_err(|e| e.to_string()));
    state.exporting.store(false, Ordering::SeqCst);
    result.map(|_| path)
}

// ─── Wiring ──────────────────────────────────────────────────────────────────────────────────

fn spawn_playhead_emitter(app: AppHandle) {
    std::thread::Builder::new()
        .name("dissonant-playhead".into())
        .spawn(move || {
            let mut last: Option<(u64, bool)> = None;
            loop {
                std::thread::sleep(Duration::from_millis(16));
                let state = app.state::<AppState>();
                let payload = {
                    let audio = state.audio.lock().unwrap();
                    let Some(device) = audio.as_ref() else { continue };
                    let shared = device.shared();
                    let beat = shared.position_beats();
                    let playing = shared.is_playing();
                    let (l, r) = shared.master_peak();
                    let track_count = state
                        .doc
                        .lock()
                        .unwrap()
                        .model()
                        .tracks
                        .len()
                        .min(dissonant_engine::MAX_TRACKS);
                    let track_peaks = (0..track_count).map(|i| shared.track_peak(i)).collect();
                    PlayheadEvent {
                        beat,
                        playing,
                        master_peak: [l, r],
                        track_peaks,
                    }
                };
                let key = (payload.beat.to_bits(), payload.playing);
                if last == Some(key) && payload.master_peak == [0.0, 0.0] {
                    continue; // nothing moved; don't spam the webview
                }
                last = Some(key);
                let _ = app.emit("playhead", &payload);
            }
        })
        .expect("playhead thread");
}

/// Watches the output device: restarts on the new default when the stream errors (device
/// unplugged) or the system default changes (headphones plugged in), and keeps retrying while
/// no device is available. Tells the UI through the `audio-status` event.
fn spawn_audio_watchdog(app: AppHandle) {
    std::thread::Builder::new()
        .name("dissonant-audio-watchdog".into())
        .spawn(move || {
            let mut last_attempt = std::time::Instant::now();
            loop {
                std::thread::sleep(Duration::from_millis(1000));
                let state = app.state::<AppState>();
                let (running, failed, current) = {
                    let audio = state.audio.lock().unwrap();
                    (
                        audio.is_some(),
                        audio.as_ref().map(|a| a.has_failed()).unwrap_or(false),
                        audio.as_ref().map(|a| a.device_name.clone()),
                    )
                };
                let default = default_output_name();
                let reason = if failed {
                    Some("audio device lost".to_string())
                } else if running && default.is_some() && default != current {
                    Some(format!("output switched to {}", default.clone().unwrap_or_default()))
                } else if !running && default.is_some() && last_attempt.elapsed() >= Duration::from_secs(3) {
                    Some("audio device available".to_string())
                } else {
                    None
                };
                let Some(reason) = reason else { continue };
                last_attempt = std::time::Instant::now();
                log::info!("{reason}: restarting audio");
                let status = state.restart_audio();
                let message = if status.running {
                    format!("{reason} — now on {}", status.device_name.clone().unwrap_or_default())
                } else {
                    format!("{reason} — {}", status.error.clone().unwrap_or_else(|| "no output".into()))
                };
                let _ = app.emit("audio-status", AudioStatusEvent { status, message });
            }
        })
        .expect("audio watchdog thread");
}

/// Writes `<project>.autosave.json` while the document is dirty and changing.
fn spawn_autosaver(app: AppHandle) {
    std::thread::Builder::new()
        .name("dissonant-autosave".into())
        .spawn(move || {
            let mut last_write = std::time::Instant::now();
            loop {
                std::thread::sleep(Duration::from_secs(5));
                if last_write.elapsed() < AUTOSAVE_INTERVAL {
                    continue;
                }
                let state = app.state::<AppState>();
                if let Some(path) = state.autosave() {
                    log::debug!("autosaved to {}", path.display());
                    last_write = std::time::Instant::now();
                }
            }
        })
        .expect("autosave thread");
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .setup(|app| {
            let state = app.state::<AppState>();
            match app.path().app_data_dir() {
                Ok(dir) => {
                    let _ = std::fs::create_dir_all(&dir);
                    *state.data_dir.lock().unwrap() = Some(dir);
                }
                Err(e) => log::warn!("no app data dir: {e}"),
            }
            state.start_audio();
            state.sync_engine();
            spawn_playhead_emitter(app.handle().clone());
            spawn_audio_watchdog(app.handle().clone());
            spawn_autosaver(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                let state = window.state::<AppState>();
                let dirty = state.doc.lock().unwrap().is_dirty();
                if dirty && !state.force_close.load(Ordering::SeqCst) {
                    // Keep the window; the UI asks save / discard / cancel and calls `quit`.
                    api.prevent_close();
                    let _ = window.emit("close-requested", ());
                } else {
                    state.clear_autosave();
                }
            }
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                let project = paths.iter().find(|p| {
                    matches!(
                        p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
                        Some("dissonant") | Some("json")
                    )
                });
                if let Some(p) = project {
                    let _ = window.emit("file-dropped", p.display().to_string());
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            apply,
            commit_gesture,
            undo,
            redo,
            new_project,
            open_project,
            save_project,
            set_playback_context,
            transport_play,
            transport_stop,
            transport_seek,
            set_hear_chords,
            audition,
            audition_off,
            audio_status,
            restart_audio,
            export_wav,
            midi_status,
            open_midi_input,
            close_midi_input,
            set_live_track,
            set_record,
            note_on,
            note_off,
            set_loop,
            clear_loop,
            set_looping,
            quit,
            recent_files,
            clear_recent,
            recovery_candidates,
            restore_autosave,
            discard_autosave,
            autosave_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running dissonant");
}

#[allow(dead_code)]
fn _master_type_is_used(_: MasterSettings) {}
