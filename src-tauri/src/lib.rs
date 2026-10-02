//! The Tauri shell: owns the [`Document`] and the [`AudioDevice`], exposes commands to the UI,
//! and streams the playhead back as events. All model edits go through `apply`, which keeps
//! undo/redo in one place and re-feeds the engine a fresh [`Sequence`] after every change.

use dissonant_core::{Command, Document, MasterSettings, NoteEvent, ProjectModel, Sequence};
use dissonant_engine::{render_wav, AudioDevice, EngineCommand, RenderOptions};
use midir::{MidiInput, MidiInputConnection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
    /// The track live input (MIDI / typing keyboard) plays and records into.
    live_track: Option<Uuid>,
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
            midi: Mutex::new(None),
            midi_port: Mutex::new(None),
            midi_last_ms: AtomicU64::new(0),
            recorder: Mutex::new(Recorder::default()),
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
    state.send(EngineCommand::SetLoop { start, end });
}

#[tauri::command]
fn clear_loop(state: State<'_, AppState>) {
    state.send(EngineCommand::ClearLoop);
}

#[tauri::command]
fn set_looping(state: State<'_, AppState>, on: bool) {
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running dissonant");
}

#[allow(dead_code)]
fn _master_type_is_used(_: MasterSettings) {}
