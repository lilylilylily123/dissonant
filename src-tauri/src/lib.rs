//! The Tauri shell: owns the [`Document`] and the [`AudioDevice`], exposes commands to the UI,
//! and streams the playhead back as events. All model edits go through `apply`, which keeps
//! undo/redo in one place and re-feeds the engine a fresh [`Sequence`] after every change.

use dissonant_core::{Command, Document, MasterSettings, ProjectModel, Sequence};
use dissonant_engine::{render_wav, AudioDevice, EngineCommand, RenderOptions};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PlayMode {
    #[default]
    Pattern,
    Song,
}

#[derive(Default)]
struct PlaybackContext {
    mode: PlayMode,
    pattern_id: Option<Uuid>,
    hear_chords: bool,
}

pub struct AppState {
    doc: Mutex<Document>,
    audio: Mutex<Option<AudioDevice>>,
    audio_error: Mutex<Option<String>>,
    playback: Mutex<PlaybackContext>,
    exporting: AtomicBool,
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
}

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
}

impl AppState {
    fn new() -> Self {
        AppState {
            doc: Mutex::new(Document::new(ProjectModel::starter())),
            audio: Mutex::new(None),
            audio_error: Mutex::new(None),
            playback: Mutex::new(PlaybackContext::default()),
            exporting: AtomicBool::new(false),
        }
    }

    fn snapshot(&self) -> Snapshot {
        let doc = self.doc.lock().unwrap();
        Snapshot {
            model: doc.model().clone(),
            can_undo: doc.can_undo(),
            can_redo: doc.can_redo(),
            dirty: doc.is_dirty(),
            path: doc.path.as_ref().map(|p| p.display().to_string()),
        }
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

fn build_sequence(model: &ProjectModel, mode: PlayMode, pattern_id: Option<Uuid>) -> Sequence {
    match mode {
        PlayMode::Song if !model.arrangement.is_empty() => Sequence::from_song(model),
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
fn apply(state: State<'_, AppState>, command: Command, transient: Option<bool>) -> Result<Snapshot, String> {
    state
        .doc
        .lock()
        .unwrap()
        .apply(command, transient.unwrap_or(false))
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
    state.doc.lock().unwrap().replace(model, None);
    state.playback.lock().unwrap().pattern_id = None;
    state.send(EngineCommand::Stop);
    state.send(EngineCommand::Seek { beat: 0.0 });
    state.sync_engine();
    state.snapshot()
}

#[tauri::command]
fn open_project(state: State<'_, AppState>, path: String) -> Result<Snapshot, String> {
    let json = std::fs::read_to_string(&path).map_err(|e| format!("could not read {path}: {e}"))?;
    let model = ProjectModel::from_json(&json).map_err(|e| format!("not a dissonant project: {e}"))?;
    state.doc.lock().unwrap().replace(model, Some(PathBuf::from(&path)));
    state.playback.lock().unwrap().pattern_id = None;
    state.send(EngineCommand::Stop);
    state.send(EngineCommand::Seek { beat: 0.0 });
    state.sync_engine();
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
    doc.path = Some(target);
    doc.mark_saved();
    drop(doc);
    Ok(state.snapshot())
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
fn audition(state: State<'_, AppState>, track_id: Uuid, pitch: u8, velocity: Option<u8>, seconds: Option<f32>) {
    state.send(EngineCommand::Audition {
        track_id,
        pitch,
        velocity: velocity.unwrap_or(100),
        seconds: seconds.unwrap_or(0.25),
    });
}

#[tauri::command]
fn audition_off(state: State<'_, AppState>, track_id: Uuid, pitch: u8) {
    state.send(EngineCommand::AuditionOff { track_id, pitch });
}

#[tauri::command]
fn audio_status(state: State<'_, AppState>) -> AudioStatus {
    let audio = state.audio.lock().unwrap();
    AudioStatus {
        running: audio.is_some(),
        sample_rate: audio.as_ref().map(|a| a.sample_rate),
        error: state.audio_error.lock().unwrap().clone(),
    }
}

#[tauri::command]
fn restart_audio(state: State<'_, AppState>) -> AudioStatus {
    *state.audio.lock().unwrap() = None;
    state.start_audio();
    state.sync_engine();
    audio_status(state)
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
        let seq = if doc.model().arrangement.is_empty() {
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

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::new())
        .setup(|app| {
            let state = app.state::<AppState>();
            state.start_audio();
            state.sync_engine();
            spawn_playhead_emitter(app.handle().clone());
            Ok(())
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running dissonant");
}

#[allow(dead_code)]
fn _master_type_is_used(_: MasterSettings) {}
