//! The plugin main thread, and the app's view of what is loaded.
//!
//! CLAP gives a plugin's parameter, state and (later) GUI calls to one thread and keeps them
//! there — clack enforces it by making `HostedPlugin` not `Send`. Tauri commands, meanwhile, run
//! on whatever worker the async runtime picks, so a plugin cannot simply live in `AppState`.
//!
//! So every loaded plugin lives on one thread owned by [`PluginHostThread`], and the rest of the
//! app talks to it by message. Each request carries its own reply channel, which makes the public
//! methods here look synchronous while keeping the plugins themselves pinned where CLAP needs
//! them. The one thing that does *not* come back by message is audio: [`HostedPlugin::open`]
//! also yields a `PluginAudioUnit`, which is `Send`, and that is what gets handed to the engine.

use dissonant_plugins::{HostedPlugin, ParamInfo, PluginAudioUnit, PluginDescriptor, PluginFormat, ScanCache, Scanner};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use uuid::Uuid;

/// Blocks the audio engine processes. Plugins are activated for this many frames at most, and
/// `PluginAudioUnit::render_add` chunks anything larger.
const MAX_BLOCK_FRAMES: u32 = 2048;

type Reply<T> = Sender<T>;

enum Request {
    /// List what is installed. `rescan` ignores the on-disk cache.
    Scan { rescan: bool, reply: Reply<Vec<PluginDescriptor>> },
    /// Load `uid` for `track_id`, replacing whatever that track had.
    Open {
        track_id: Uuid,
        uid: String,
        sample_rate: f64,
        reply: Reply<Result<(PluginAudioUnit, String), String>>,
    },
    /// Drop the plugin a track was hosting.
    Close { track_id: Uuid, reply: Reply<()> },
    /// Current parameters, with live values.
    Params { track_id: Uuid, reply: Reply<Vec<ParamInfo>> },
    /// Read the plugin's own state blob, for the undo history and the project file.
    SaveState { track_id: Uuid, reply: Reply<Option<Vec<u8>>> },
    /// Push a state blob back into the plugin (project load, undo, redo).
    LoadState { track_id: Uuid, state: Vec<u8>, reply: Reply<Result<(), String>> },
}

/// Handle to the plugin main thread. Cheap to clone-free share; every method blocks only as long
/// as the plugin thread takes to answer.
pub struct PluginHostThread {
    tx: Mutex<Sender<Request>>,
}

impl PluginHostThread {
    pub fn spawn(cache_path: Option<PathBuf>) -> Self {
        let (tx, rx) = channel::<Request>();
        std::thread::Builder::new()
            .name("dissonant-plugins".into())
            .spawn(move || {
                let mut loaded: HashMap<Uuid, HostedPlugin> = HashMap::new();
                let mut cache = cache_path.as_deref().map(ScanCache::load).unwrap_or_default();
                let mut known: Vec<PluginDescriptor> = Vec::new();
                while let Ok(req) = rx.recv() {
                    match req {
                        Request::Scan { rescan, reply } => {
                            let paths = dissonant_plugins::search_paths(PluginFormat::Clap);
                            known = if rescan {
                                cache = ScanCache::default();
                                Scanner::scan(&paths)
                            } else {
                                cache.scan_cached(&paths)
                            };
                            if let Some(p) = &cache_path {
                                cache.save(p);
                            }
                            let _ = reply.send(known.clone());
                        }
                        Request::Open { track_id, uid, sample_rate, reply } => {
                            // A uid the scan has never seen cannot be resolved to a bundle, and
                            // guessing one would load a different plugin under the same name.
                            if known.is_empty() {
                                known = cache.scan_cached(&dissonant_plugins::search_paths(PluginFormat::Clap));
                            }
                            let desc = known.iter().find(|d| d.uid == uid).cloned();
                            let Some(desc) = desc else {
                                let _ = reply.send(Err("no such plugin".into()));
                                continue;
                            };
                            match HostedPlugin::open(&desc, sample_rate, MAX_BLOCK_FRAMES) {
                                Ok((hosted, unit)) => {
                                    // Drop the previous plugin only once the new one is live, so
                                    // a failed load leaves the track playing what it had.
                                    loaded.insert(track_id, hosted);
                                    let _ = reply.send(Ok((unit, desc.name.clone())));
                                }
                                Err(e) => {
                                    let _ = reply.send(Err(e.to_string()));
                                }
                            }
                        }
                        Request::Close { track_id, reply } => {
                            loaded.remove(&track_id);
                            let _ = reply.send(());
                        }
                        Request::Params { track_id, reply } => {
                            let params = loaded.get_mut(&track_id).map(|p| p.params().to_vec()).unwrap_or_default();
                            let _ = reply.send(params);
                        }
                        Request::SaveState { track_id, reply } => {
                            let state = loaded.get_mut(&track_id).and_then(|p| match p.save_state() {
                                Ok(bytes) => Some(bytes),
                                Err(e) => {
                                    // A plugin without the state extension is legitimate; the
                                    // project simply stores nothing for it.
                                    log::debug!("plugin state unavailable: {e}");
                                    None
                                }
                            });
                            let _ = reply.send(state);
                        }
                        Request::LoadState { track_id, state, reply } => {
                            let result = match loaded.get_mut(&track_id) {
                                Some(p) => p.load_state(&state).map_err(|e| e.to_string()),
                                None => Err("track has no plugin".to_string()),
                            };
                            let _ = reply.send(result);
                        }
                    }
                }
            })
            .expect("plugin host thread");
        PluginHostThread { tx: Mutex::new(tx) }
    }

    /// Send a request and wait for its answer. A dead plugin thread yields `None` rather than
    /// taking the app down: the rest of the DAW works fine without plugins.
    fn ask<T>(&self, make: impl FnOnce(Reply<T>) -> Request) -> Option<T> {
        let (reply_tx, reply_rx) = channel::<T>();
        let sent = self.tx.lock().ok()?.send(make(reply_tx)).is_ok();
        if !sent {
            log::warn!("plugin host thread is gone");
            return None;
        }
        reply_rx.recv().ok()
    }

    pub fn scan(&self, rescan: bool) -> Vec<PluginDescriptor> {
        self.ask(|reply| Request::Scan { rescan, reply }).unwrap_or_default()
    }

    /// Load `uid` on `track_id`, returning the audio half for the engine and the plugin's name.
    pub fn open(&self, track_id: Uuid, uid: &str, sample_rate: f64) -> Result<(PluginAudioUnit, String), String> {
        self.ask(|reply| Request::Open { track_id, uid: uid.to_string(), sample_rate, reply })
            .unwrap_or_else(|| Err("plugin host is not running".into()))
    }

    pub fn close(&self, track_id: Uuid) {
        self.ask(|reply| Request::Close { track_id, reply });
    }

    pub fn params(&self, track_id: Uuid) -> Vec<ParamInfo> {
        self.ask(|reply| Request::Params { track_id, reply }).unwrap_or_default()
    }

    pub fn save_state(&self, track_id: Uuid) -> Option<Vec<u8>> {
        self.ask(|reply| Request::SaveState { track_id, reply }).flatten()
    }

    pub fn load_state(&self, track_id: Uuid, state: Vec<u8>) -> Result<(), String> {
        self.ask(|reply| Request::LoadState { track_id, state, reply })
            .unwrap_or_else(|| Err("plugin host is not running".into()))
    }
}
