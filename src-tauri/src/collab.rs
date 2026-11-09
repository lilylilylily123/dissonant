//! Shared sessions, as the app sees them.
//!
//! `dissonant-session` holds the logic and `dissonant-net` moves the bytes; this is what joins
//! them to the document, the engine and the UI. [`AppState::submit`] is the one way an edit
//! reaches the document, solo or not. Messages from the network land on a pump thread, which
//! applies them and tells the engine and the UI at a bounded rate, however fast people are
//! editing.

use crate::{AppState, Snapshot};
use dissonant_core::{Command, Patch, StepOutcome};
use dissonant_net::{format_code, new_code, ConnId, Net, NetEvent, Reach, HOST};
use dissonant_session::{narrow, Bases, GuestEvents, GuestSession, HostEvents, HostSession, PeerInfo, ToGuest, ToHost};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State, UserAttentionType};
use uuid::Uuid;

/// The most often the engine and the UI are told about other people's edits.
const FLUSH_EVERY: Duration = Duration::from_millis(50);
/// A cursor update bigger than this isn't one.
const MAX_PRESENCE: usize = 1000;
/// How long a join code works for, if nobody uses it first.
const CODE_LIFETIME: Duration = Duration::from_secs(5 * 60);

enum Role {
    Host(HostSession),
    Guest(GuestSession),
}

/// The code people can join with right now.
struct JoinCode {
    code: String,
    expires: SystemTime,
}

struct Live {
    /// Which session this is, so a pump can tell it has been left behind.
    generation: u64,
    role: Role,
    net: Net,
    bases: Bases,
    /// A host letting new people in.
    code: Option<JoinCode>,
    peers: Vec<PeerInfo>,
    /// A guest: whether the host can be heard right now. A host always can.
    connected: bool,
    /// A guest: connected, and waiting for the host to say yes.
    waiting: bool,
    /// The words for each open connection, for two people to check against each other.
    words: HashMap<ConnId, [&'static str; 3]>,
}

/// All the app knows about the session, if there is one.
#[derive(Default)]
pub struct Collab {
    live: Mutex<Option<Live>>,
    generation: AtomicU64,
    /// Readable without the lock, from places that hold the document's.
    guest: AtomicBool,
}

impl Collab {
    pub fn is_guest(&self) -> bool {
        self.guest.load(Ordering::SeqCst)
    }

    pub fn active(&self) -> bool {
        self.live.lock().unwrap().is_some()
    }
}

/// Someone waiting for the host to let them in, as the UI shows them.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinRequest {
    pub id: Uuid,
    pub name: String,
    pub color: String,
    pub words: Vec<&'static str>,
}

/// What the UI shows about the session.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    /// "solo", "host" or "guest".
    pub role: &'static str,
    /// A guest who has the project and can hear the host. A host always is.
    pub live: bool,
    /// A guest who had the project and lost the host, and is trying to get back.
    pub reconnecting: bool,
    pub you: Option<Uuid>,
    /// A host's current join code, as it should be shown. None while closed to new people.
    pub code: Option<String>,
    /// When `code` stops working, in unix milliseconds.
    pub code_expires_at: Option<u64>,
    /// A host letting new people ask to come in.
    pub open: bool,
    /// A host: people waiting to be let in, oldest first.
    pub requests: Vec<JoinRequest>,
    /// A guest waiting for the host to let them in.
    pub waiting: bool,
    /// A guest: the words for this connection to the host.
    pub words: Option<Vec<&'static str>>,
    pub peers: Vec<PeerInfo>,
    /// "direct" or "relayed", once there is a connection to speak of.
    pub path: Option<&'static str>,
}

const SOLO: SessionState = SessionState {
    role: "solo",
    live: false,
    reconnecting: false,
    you: None,
    code: None,
    code_expires_at: None,
    open: true,
    requests: vec![],
    waiting: false,
    words: None,
    peers: vec![],
    path: None,
};

/// One person's whereabouts, stamped by the host so nobody can speak as somebody else. The
/// location itself is the UI's business and passes through untouched.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    pub peer: Uuid,
    pub location: serde_json::Value,
}

impl Live {
    fn state(&self) -> SessionState {
        match &self.role {
            Role::Host(host) => SessionState {
                role: "host",
                live: true,
                reconnecting: false,
                you: Some(host.id()),
                code: self.code.as_ref().map(|c| format_code(&c.code)),
                code_expires_at: self.code.as_ref().map(|c| {
                    c.expires.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
                }),
                open: host.is_open(),
                requests: host
                    .requests()
                    .iter()
                    .map(|r| JoinRequest {
                        id: r.info.id,
                        name: r.info.name.clone(),
                        color: r.info.color.clone(),
                        words: self.words.get(&r.conn).map(|w| w.to_vec()).unwrap_or_default(),
                    })
                    .collect(),
                waiting: false,
                words: None,
                peers: host.peers(),
                path: None,
            },
            Role::Guest(guest) => SessionState {
                role: "guest",
                live: self.connected && guest.joined(),
                reconnecting: !self.connected && guest.joined(),
                you: Some(guest.id()),
                code: None,
                code_expires_at: None,
                open: true,
                requests: vec![],
                waiting: self.connected && self.waiting,
                words: self.words.get(&HOST).map(|w| w.to_vec()),
                peers: self.peers.clone(),
                path: self.net.path(HOST),
            },
        }
    }

    /// A host: a fresh code, and the one before it stops working.
    fn new_code(&mut self) {
        let code = new_code();
        self.net.open_lobby(&code);
        self.code = Some(JoinCode { code, expires: SystemTime::now() + CODE_LIFETIME });
    }

    /// A host: no code at all, so nobody new can ask to come in.
    fn close_code(&mut self) {
        self.net.close_lobby();
        self.code = None;
    }

    /// A host whose code has run out.
    fn code_expired(&self) -> bool {
        matches!(&self.role, Role::Host(host) if host.is_open())
            && self.code.as_ref().is_none_or(|c| SystemTime::now() >= c.expires)
    }

    fn send_to_host(&self, message: &ToHost) {
        if let Ok(bytes) = serde_json::to_vec(message) {
            self.net.send(HOST, &bytes);
        }
    }

    fn deliver(&self, out: &[(ConnId, ToGuest)], close: Option<ConnId>) {
        for (conn, message) in out {
            if let Ok(bytes) = serde_json::to_vec(message) {
                self.net.send(*conn, &bytes);
            }
        }
        if let Some(conn) = close {
            self.net.close(conn);
        }
    }
}

impl AppState {
    /// The one way an edit reaches the document. `base` is the document revision the UI built the
    /// edit from, which is what keeps a whole-array commit from erasing what somebody else did
    /// in the meantime.
    pub fn submit(&self, command: Command, transient: bool, label: Option<String>, base: Option<u64>) -> Result<(), String> {
        let mut guard = self.collab.live.lock().unwrap();
        let mut doc = self.doc.lock().unwrap();
        let Some(live) = guard.as_mut() else {
            return doc.apply_labeled(command, transient, label).map_err(|e| e.to_string());
        };
        let from = base.and_then(|r| live.bases.at(r)).unwrap_or(doc.model());
        let command = narrow(command, from);
        match &mut live.role {
            Role::Host(host) => {
                let out = host.edit(&mut doc, command, transient, label).map_err(|e| e.to_string())?;
                live.bases.note(doc.revision(), doc.model());
                live.deliver(&out.send, out.close);
            }
            Role::Guest(guest) => {
                if command.adds_audio() {
                    return Err(GUEST_AUDIO.to_string());
                }
                let message = guest.edit(&mut doc, command, transient, label).map_err(|e| e.to_string())?;
                live.bases.note(doc.revision(), doc.model());
                if let Some(message) = message {
                    live.send_to_host(&message);
                }
            }
        }
        Ok(())
    }

    /// Undo or redo this person's own last edit.
    pub fn step(&self, undo: bool) -> StepOutcome {
        let mut guard = self.collab.live.lock().unwrap();
        let mut doc = self.doc.lock().unwrap();
        let Some(live) = guard.as_mut() else {
            return if undo { doc.undo_step() } else { doc.redo_step() };
        };
        let outcome = match &mut live.role {
            Role::Host(host) => {
                let (outcome, out) = host.step(&mut doc, undo);
                live.deliver(&out.send, out.close);
                outcome
            }
            Role::Guest(guest) => {
                let (outcome, message) = guest.step(&mut doc, undo);
                if let Some(message) = message {
                    live.send_to_host(&message);
                }
                outcome
            }
        };
        live.bases.note(doc.revision(), doc.model());
        outcome
    }

    pub fn session_state(&self) -> SessionState {
        self.collab.live.lock().unwrap().as_ref().map_or(SOLO, Live::state)
    }

    /// Refuses what would swap the project out from under a session.
    pub fn refuse_in_session(&self) -> Result<(), String> {
        if self.collab.active() {
            return Err("end the session first: this would replace the project for everyone in it.".into());
        }
        Ok(())
    }

    /// Refuses what would bring in audio only this machine has.
    pub fn refuse_guest_audio(&self) -> Result<(), String> {
        if self.collab.is_guest() {
            return Err(GUEST_AUDIO.to_string());
        }
        Ok(())
    }

    /// Leave or end whatever session there is, saying goodbye. Blocks briefly: for shutting down.
    pub fn close_session(&self) {
        if let Some(live) = self.take_live() {
            say_goodbye(live);
        }
    }

    fn take_live(&self) -> Option<Live> {
        let live = self.collab.live.lock().unwrap().take();
        self.collab.guest.store(false, Ordering::SeqCst);
        self.collab.generation.fetch_add(1, Ordering::SeqCst);
        live
    }
}

const GUEST_AUDIO: &str = "only the host can add audio in a session for now: a take recorded here wouldn't reach them.";

fn say_goodbye(mut live: Live) {
    match &mut live.role {
        Role::Host(host) => {
            let out = host.end();
            live.deliver(&out.send, None);
        }
        Role::Guest(guest) => {
            let bye = guest.bye();
            live.send_to_host(&bye);
        }
    }
    live.net.shutdown();
}

fn emit_session(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _ = app.emit("session", state.session_state());
}

/// What the engine and the UI still have to hear about.
#[derive(Default)]
struct Followup {
    loaded: bool,
    changed: bool,
    audio: bool,
    plugins: bool,
    session: bool,
    /// Someone is asking to be let in: the host has to notice, wherever they are.
    attention: bool,
}

impl Followup {
    fn note(&mut self, patch: &Patch) {
        self.changed = true;
        // No patch to read means the view was rebuilt, so anything may have moved.
        if patch.is_empty() {
            self.audio = true;
            self.plugins = true;
        }
        self.audio |= patch.touches("audioSources");
        self.plugins |= patch.touches("masterInserts")
            || patch.touches_track_field("plugin")
            || patch.touches_track_field("inserts");
    }

    fn flush(&mut self, app: &AppHandle) {
        let state = app.state::<AppState>();
        if self.loaded {
            state.reset_engine_to_document();
        } else if self.changed {
            state.sync_engine();
            if self.plugins {
                state.sync_plugins();
            }
            if self.audio {
                state.sync_audio_pool();
            }
        }
        if self.loaded || self.changed {
            let _ = app.emit("document", state.snapshot());
        }
        if self.session {
            emit_session(app);
        }
        if self.attention {
            for window in app.webview_windows().values() {
                let _ = window.request_user_attention(Some(UserAttentionType::Critical));
            }
        }
        *self = Followup::default();
    }

    fn pending(&self) -> bool {
        self.loaded || self.changed || self.session || self.attention
    }
}

/// Decide what a host's handling of something means for the app, and send what it produced.
fn after_host(live: &Live, out: HostEvents, todo: &mut Followup) {
    live.deliver(&out.send, out.close);
    if let Some(key) = &out.admit {
        live.net.admit(key);
    }
    if let Some(key) = &out.revoke {
        live.net.revoke(key);
    }
    if let Some(patch) = &out.changed {
        todo.note(patch);
    }
    todo.session |= out.peers_changed || out.requests_changed;
    todo.attention |= out.asked;
}

/// The same for a guest. Returns why the session ended, if it did.
fn after_guest(app: &AppHandle, live: &mut Live, out: GuestEvents, todo: &mut Followup) -> Option<String> {
    for message in &out.send {
        live.send_to_host(message);
    }
    if out.loaded {
        live.waiting = false;
        todo.loaded = true;
        todo.session = true;
    }
    if out.waiting {
        live.waiting = true;
        todo.session = true;
    }
    if let Some(patch) = &out.changed {
        todo.note(patch);
    }
    if let Some(peers) = out.peers {
        live.peers = peers;
        todo.session = true;
    }
    if let Some(notice) = out.notice {
        let _ = app.emit("notice", notice);
    }
    out.ended
}

fn relay_presence(app: &AppHandle, live: &Live, from: ConnId, bytes: &[u8]) {
    if bytes.len() > MAX_PRESENCE {
        return;
    }
    match &live.role {
        Role::Host(host) => {
            let Some(peer) = host.client_on(from) else { return };
            let Ok(location) = serde_json::from_slice::<serde_json::Value>(bytes) else { return };
            let presence = Presence { peer, location };
            if let Ok(stamped) = serde_json::to_vec(&presence) {
                for conn in host.others(Some(from)) {
                    live.net.datagram(conn, &stamped);
                }
            }
            let _ = app.emit("peer-presence", presence);
        }
        Role::Guest(_) => {
            if let Ok(presence) = serde_json::from_slice::<Presence>(bytes) {
                let _ = app.emit("peer-presence", presence);
            }
        }
    }
}

/// Handle one thing from the network. Returns why the session ended, if it did.
fn handle(app: &AppHandle, generation: u64, event: NetEvent, todo: &mut Followup) -> Result<(), Option<String>> {
    let state = app.state::<AppState>();
    let mut guard = state.collab.live.lock().unwrap();
    let Some(live) = guard.as_mut().filter(|l| l.generation == generation) else { return Err(None) };
    let mut doc = state.doc.lock().unwrap();
    let ended = match event {
        NetEvent::Connected(conn, peer) => {
            live.words.insert(conn, peer.words);
            match &mut live.role {
                Role::Host(host) => host.on_connect(conn, peer.key),
                Role::Guest(guest) => {
                    let hello = guest.hello();
                    live.connected = true;
                    live.send_to_host(&hello);
                    todo.session = true;
                }
            }
            None
        }
        NetEvent::Disconnected(conn) => {
            live.words.remove(&conn);
            match &mut live.role {
                Role::Host(host) => {
                    let out = host.on_disconnect(conn);
                    after_host(live, out, todo);
                }
                Role::Guest(guest) => {
                    guest.disconnected();
                    live.connected = false;
                    live.waiting = false;
                    todo.session = true;
                }
            }
            None
        }
        // Each code lets one device through: the next person needs the next code.
        NetEvent::Knocked(_) => {
            if matches!(&live.role, Role::Host(host) if host.is_open()) {
                live.new_code();
                todo.session = true;
            }
            None
        }
        NetEvent::BadKnock => {
            if matches!(&live.role, Role::Host(host) if host.is_open()) {
                live.new_code();
                todo.session = true;
            }
            let _ = app.emit("notice", "someone found your session without the code and was turned away. there's a new code now.");
            None
        }
        NetEvent::GaveUp(why) => Some(why),
        NetEvent::Datagram(conn, bytes) => {
            relay_presence(app, live, conn, &bytes);
            None
        }
        NetEvent::Message(conn, bytes) => match &mut live.role {
            Role::Host(host) => {
                match serde_json::from_slice::<ToHost>(&bytes) {
                    Ok(message) => {
                        let out = host.on_message(&mut doc, conn, message);
                        after_host(live, out, todo);
                    }
                    Err(e) => log::warn!("unreadable message from a guest: {e}"),
                }
                None
            }
            Role::Guest(guest) => match serde_json::from_slice::<ToGuest>(&bytes) {
                Ok(message) => {
                    let out = guest.on_message(&mut doc, message);
                    after_guest(app, live, out, todo)
                }
                Err(e) => {
                    log::warn!("unreadable message from the host: {e}");
                    None
                }
            },
        },
    };
    live.bases.note(doc.revision(), doc.model());
    match ended {
        Some(reason) => Err(Some(reason)),
        None => Ok(()),
    }
}

/// A host's code that has run out gets replaced.
fn renew_code(app: &AppHandle, generation: u64, todo: &mut Followup) {
    let state = app.state::<AppState>();
    let mut guard = state.collab.live.lock().unwrap();
    if let Some(live) = guard.as_mut().filter(|l| l.generation == generation && l.code_expired()) {
        live.new_code();
        todo.session = true;
    }
}

/// Everything from the network comes through here, on one thread.
fn pump(app: AppHandle, generation: u64, events: Receiver<NetEvent>) {
    let mut todo = Followup::default();
    let mut flushed = Instant::now();
    loop {
        match events.recv_timeout(FLUSH_EVERY / 2) {
            Ok(event) => match handle(&app, generation, event, &mut todo) {
                Ok(()) => {}
                Err(None) => return,
                Err(Some(reason)) => {
                    todo.flush(&app);
                    finish(&app, reason);
                    return;
                }
            },
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        renew_code(&app, generation, &mut todo);
        if todo.pending() && flushed.elapsed() >= FLUSH_EVERY {
            todo.flush(&app);
            flushed = Instant::now();
        }
    }
}

/// The session ended from the far side. Whatever the person was looking at stays, as an untitled
/// project of their own.
fn finish(app: &AppHandle, reason: String) {
    let state = app.state::<AppState>();
    if let Some(live) = state.take_live() {
        std::thread::spawn(move || live.net.shutdown());
    }
    emit_session(app);
    let _ = app.emit("document", state.snapshot());
    let _ = app.emit("session-ended", reason);
}

fn start(app: &AppHandle, role: Role, net: Net, events: Receiver<NetEvent>, generation: u64) {
    let state = app.state::<AppState>();
    let guest = matches!(role, Role::Guest(_));
    let mut bases = Bases::default();
    {
        let doc = state.doc.lock().unwrap();
        bases.note(doc.revision(), doc.model());
    }
    let mut live = Live {
        generation,
        role,
        net,
        bases,
        code: None,
        peers: vec![],
        connected: !guest,
        waiting: false,
        words: HashMap::new(),
    };
    if !guest {
        live.new_code();
    }
    *state.collab.live.lock().unwrap() = Some(live);
    state.collab.guest.store(guest, Ordering::SeqCst);
    let app = app.clone();
    let _ = std::thread::Builder::new().name("dissonant-session".into()).spawn(move || pump(app, generation, events));
}

// ─── Commands ────────────────────────────────────────────────────────────────────────────────

/// Start sharing the open project.
#[tauri::command]
pub async fn share_start(app: AppHandle, name: String, color: String) -> Result<SessionState, String> {
    app.state::<AppState>().refuse_in_session()?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let app = handle;
        let (tx, rx) = channel();
        let net = Net::host(Reach::Public, tx)?;
        let state = app.state::<AppState>();
        let generation = state.collab.generation.fetch_add(1, Ordering::SeqCst) + 1;
        start(&app, Role::Host(HostSession::new(name, color)), net, rx, generation);
        Ok(state.session_state())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stop sharing. Guests keep what they had as a project of their own.
#[tauri::command]
pub fn share_stop(state: State<'_, AppState>) -> SessionState {
    if let Some(live) = state.take_live() {
        std::thread::spawn(move || say_goodbye(live));
    }
    state.session_state()
}

/// A fresh code. The one before stops working; people already in stay in.
#[tauri::command]
pub fn share_new_code(state: State<'_, AppState>) -> SessionState {
    let mut guard = state.collab.live.lock().unwrap();
    if let Some(live) = guard.as_mut().filter(|l| matches!(&l.role, Role::Host(host) if host.is_open())) {
        live.new_code();
    }
    guard.as_ref().map_or(SOLO, Live::state)
}

/// Let new people ask to come in, or stop them. People already in stay in either way.
#[tauri::command]
pub fn share_set_open(state: State<'_, AppState>, open: bool) -> Result<SessionState, String> {
    let mut guard = state.collab.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return Err("you aren't sharing anything.".into()) };
    let Role::Host(host) = &mut live.role else { return Err("only the host can do that.".into()) };
    host.set_open(open);
    if open {
        live.new_code();
    } else {
        live.close_code();
    }
    Ok(live.state())
}

/// Let someone who is waiting in, or send them away.
#[tauri::command]
pub fn answer_join(app: AppHandle, peer: Uuid, allow: bool) {
    let state = app.state::<AppState>();
    let mut guard = state.collab.live.lock().unwrap();
    if let Some(live) = guard.as_mut() {
        if let Role::Host(host) = &mut live.role {
            let out = host.answer(&state.doc.lock().unwrap(), peer, allow);
            after_host(live, out, &mut Followup::default());
        }
    }
    drop(guard);
    emit_session(&app);
}

/// Put someone out of the session, for good: their device can't come back, even with a new code.
#[tauri::command]
pub fn remove_peer(app: AppHandle, peer: Uuid) {
    let state = app.state::<AppState>();
    let mut guard = state.collab.live.lock().unwrap();
    if let Some(live) = guard.as_mut() {
        if let Role::Host(host) = &mut live.role {
            let out = host.remove(peer);
            after_host(live, out, &mut Followup::default());
        }
    }
    drop(guard);
    emit_session(&app);
}

/// Join a session. Returns once connecting has begun; the `session` event says how it went.
#[tauri::command]
pub async fn join_session(app: AppHandle, code: String, name: String, color: String) -> Result<SessionState, String> {
    app.state::<AppState>().refuse_in_session()?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let app = handle;
        let (tx, rx) = channel();
        let net = Net::join(Reach::Public, &code, tx)?;
        let state = app.state::<AppState>();
        let generation = state.collab.generation.fetch_add(1, Ordering::SeqCst) + 1;
        start(&app, Role::Guest(GuestSession::new(name, color)), net, rx, generation);
        Ok(state.session_state())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Leave a session. The project stays open as an untitled copy.
#[tauri::command]
pub fn leave_session(state: State<'_, AppState>) -> Snapshot {
    if let Some(live) = state.take_live() {
        std::thread::spawn(move || say_goodbye(live));
    }
    state.snapshot()
}

#[tauri::command]
pub fn get_session(state: State<'_, AppState>) -> SessionState {
    state.session_state()
}

/// Where this person is right now. Lossy and newest-wins, so nothing waits on it.
#[tauri::command]
pub fn send_presence(app: AppHandle, location: serde_json::Value) {
    let state = app.state::<AppState>();
    let guard = state.collab.live.lock().unwrap();
    let Some(live) = guard.as_ref() else { return };
    match &live.role {
        Role::Host(host) => {
            let presence = Presence { peer: host.id(), location };
            if let Ok(bytes) = serde_json::to_vec(&presence) {
                if bytes.len() <= MAX_PRESENCE {
                    for conn in host.others(None) {
                        live.net.datagram(conn, &bytes);
                    }
                }
            }
        }
        Role::Guest(_) => {
            if let Ok(bytes) = serde_json::to_vec(&location) {
                if bytes.len() <= MAX_PRESENCE {
                    live.net.datagram(HOST, &bytes);
                }
            }
        }
    }
}
