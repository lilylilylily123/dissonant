//! `dissonant-net` — moves bytes between the host of a session and its guests.
//!
//! Each guest holds one iroh connection to the host. On it there is one ordered, reliable stream
//! for edits, and unreliable datagrams for cursors, where a late update is worth nothing. iroh
//! dials by public key, connects directly where it can and falls back to a relay where it can't,
//! so nobody has to forward a port, and everything is end-to-end encrypted either way. Machines
//! on one network also find each other by mDNS, so a room with no internet still works.
//!
//! Getting in takes a six-character code. The code stretches into the key of a lobby that lives
//! only as long as the code: a guest dials the lobby, proves it knows the code, and is told where
//! the host really is. The host's own endpoint turns away every device that hasn't come through
//! the lobby or been let in, before it reads a byte. Each code lets one device through, once.
//!
//! This crate knows nothing about what the bytes mean. Everything arrives as a [`NetEvent`] on a
//! plain channel, so the caller needs no async of its own.

mod code;

pub use code::{format_code, new_code, parse_code, CODE_LEN};

use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, SecretKey};
use iroh_mdns_address_lookup::MdnsAddressLookup;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use parking_lot::Mutex;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::task::JoinSet;

const ALPN: &[u8] = b"dissonant/session/2";
const LOBBY_ALPN: &[u8] = b"dissonant/lobby/1";
/// What this app calls itself on the local network, so it only finds its own kind.
const MDNS_SERVICE: &str = "dissonant";
/// A whole project with plugin state can be large. Past this, something has gone wrong.
const MAX_FRAME: usize = 64 * 1024 * 1024;
/// The host's address, as the lobby hands it over.
const MAX_LOBBY_FRAME: usize = 64 * 1024;
/// How long to wait for a relay before going on with what we have. Without one only people on
/// the same network can connect, which is still worth offering.
const ONLINE_WAIT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a guest keeps looking for the lobby. A code only just made may not be findable for
/// a few seconds, and a wrong one never will be.
const KNOCK_WINDOW: Duration = Duration::from_secs(30);
/// How long a lobby waits for a guest to prove the code once connected.
const PROOF_WAIT: Duration = Duration::from_secs(10);
/// How long a device that came through the lobby may take to reach the host.
const KNOCK_VALID: Duration = Duration::from_secs(10 * 60);
/// How long a guest keeps trying after losing the host.
const RECONNECT_WINDOW: Duration = Duration::from_secs(60);
const RECONNECT_PAUSE: Duration = Duration::from_secs(2);

/// A connection, numbered by this end. A guest's one connection is [`HOST`].
pub type ConnId = u64;
pub const HOST: ConnId = 0;

/// A device's public key: who is on the other end of a connection, as the handshake proved it.
pub type DeviceKey = [u8; 32];

/// The far end of a new connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub key: DeviceKey,
    /// The same three words on both ends, and different with anyone in between: for two people
    /// to read to each other.
    pub words: [&'static str; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub enum NetEvent {
    Connected(ConnId, Peer),
    Message(ConnId, Vec<u8>),
    Datagram(ConnId, Vec<u8>),
    Disconnected(ConnId),
    /// A host: a device proved the code and was told where to find us. The code is spent.
    Knocked(DeviceKey),
    /// A host: something found the lobby and couldn't prove the code. The code is spent.
    BadKnock,
    /// A guest stopped trying to reach the host.
    GaveUp(String),
}

/// Which networks to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The internet, through n0's public relays and address lookup, and the local network.
    Public,
    /// This network only, found by mDNS: no relay, nothing leaves the building.
    Local,
    /// Through a relay and never directly: what two people behind hostile routers end up with.
    /// For tests.
    RelayOnly,
}

/// Which devices may open a connection to the host.
#[derive(Default)]
struct Gate {
    /// `None` for someone let in; a deadline for someone the lobby just sent over.
    allowed: HashMap<EndpointId, Option<Instant>>,
}

impl Gate {
    fn knocked(&mut self, id: EndpointId) {
        self.allowed.entry(id).or_insert(Some(Instant::now() + KNOCK_VALID));
    }

    fn lets_in(&mut self, id: &EndpointId) -> bool {
        let now = Instant::now();
        self.allowed.retain(|_, until| until.is_none_or(|t| t > now));
        self.allowed.contains_key(id)
    }
}

/// The lobby for the current code, if there is one.
#[derive(Default)]
struct LobbySlot {
    /// Bumped by every new code, so a lobby still starting up can tell it has been overtaken.
    generation: u64,
    endpoint: Option<Endpoint>,
}

/// What the writer task for a connection is told to do.
enum Out {
    Frame(Vec<u8>),
    /// Everything queued has gone: close.
    Close,
}

#[derive(Clone)]
struct Link {
    out: tokio_mpsc::UnboundedSender<Out>,
    conn: Connection,
}

/// A running network end. Dropping it without [`Net::shutdown`] cuts connections abruptly.
pub struct Net {
    runtime: Runtime,
    endpoint: Endpoint,
    reach: Reach,
    links: Arc<Mutex<HashMap<ConnId, Link>>>,
    stopping: Arc<AtomicBool>,
    gate: Arc<Mutex<Gate>>,
    lobby: Arc<Mutex<LobbySlot>>,
    events: mpsc::Sender<NetEvent>,
}

fn runtime() -> Result<Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("dissonant-net")
        .enable_all()
        .build()
        .map_err(|e| e.to_string())
}

/// Open an endpoint. `key` is for a lobby, whose key comes from its code; `advertise` is whether
/// to answer when the local network asks who is there.
async fn bind(reach: Reach, alpns: Vec<Vec<u8>>, key: Option<SecretKey>, advertise: bool) -> Result<Endpoint, String> {
    let mdns = MdnsAddressLookup::builder().service_name(MDNS_SERVICE).advertise(advertise);
    let mut builder = match reach {
        Reach::Public => Endpoint::builder(presets::N0).address_lookup(mdns),
        Reach::Local => Endpoint::builder(presets::Minimal).relay_mode(RelayMode::Disabled).address_lookup(mdns),
        Reach::RelayOnly => Endpoint::builder(presets::N0).clear_ip_transports(),
    };
    if let Some(key) = key {
        builder = builder.secret_key(key);
    }
    let endpoint = builder.alpns(alpns).bind().await.map_err(|e| format!("could not open the network: {e}"))?;
    if reach != Reach::Local && tokio::time::timeout(ONLINE_WAIT, endpoint.online()).await.is_err() {
        log::warn!("no relay reached: only people on this network will be able to connect");
    }
    Ok(endpoint)
}

fn frame(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 4);
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
    out
}

async fn read_frame(recv: &mut RecvStream, max: usize) -> Option<Vec<u8>> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.ok()?;
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        log::warn!("dropping a connection over a {len}-byte message");
        return None;
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await.ok()?;
    Some(buf)
}

/// Who is on the far end of `conn`, and the words for it. `own` is this end's key, and
/// `hosting` says which of the two is the host.
async fn peer(own: EndpointId, conn: &Connection, hosting: bool) -> Peer {
    let key = *conn.remote_id().as_bytes();
    let own = *own.as_bytes();
    let (host, guest) = if hosting { (own, key) } else { (key, own) };
    let words = tokio::task::spawn_blocking(move || code::verification_words(&host, &guest))
        .await
        .unwrap_or(["?", "?", "?"]);
    Peer { key, words }
}

/// Run one connection until it ends: read frames and datagrams into `events`, write what arrives
/// on `out`. Returns when the connection is gone.
async fn serve(
    id: ConnId,
    conn: Connection,
    mut send: SendStream,
    mut recv: RecvStream,
    mut out: tokio_mpsc::UnboundedReceiver<Out>,
    events: mpsc::Sender<NetEvent>,
) {
    let writer = {
        let conn = conn.clone();
        async move {
            while let Some(item) = out.recv().await {
                match item {
                    Out::Frame(bytes) => {
                        if send.write_all(&bytes).await.is_err() {
                            break;
                        }
                    }
                    Out::Close => {
                        let _ = send.finish();
                        // Give the far end a moment to read what was just written.
                        let _ = tokio::time::timeout(Duration::from_secs(1), send.stopped()).await;
                        conn.close(0u32.into(), b"bye");
                        break;
                    }
                }
            }
        }
    };
    let reader = {
        let events = events.clone();
        async move {
            while let Some(bytes) = read_frame(&mut recv, MAX_FRAME).await {
                if events.send(NetEvent::Message(id, bytes)).is_err() {
                    break;
                }
            }
        }
    };
    let datagrams = {
        let conn = conn.clone();
        let events = events.clone();
        async move {
            while let Ok(bytes) = conn.read_datagram().await {
                if events.send(NetEvent::Datagram(id, bytes.to_vec())).is_err() {
                    break;
                }
            }
        }
    };
    tokio::select! {
        _ = writer => {}
        _ = reader => {}
        _ = datagrams => {}
    }
}

impl Net {
    fn new(runtime: Runtime, endpoint: Endpoint, reach: Reach, events: mpsc::Sender<NetEvent>) -> Net {
        Net {
            runtime,
            endpoint,
            reach,
            links: Arc::default(),
            stopping: Arc::new(AtomicBool::new(false)),
            gate: Arc::default(),
            lobby: Arc::default(),
            events,
        }
    }

    /// Start hosting. Nobody can get in until there is a lobby: see [`Net::open_lobby`].
    pub fn host(reach: Reach, events: mpsc::Sender<NetEvent>) -> Result<Net, String> {
        let runtime = runtime()?;
        let endpoint = runtime.block_on(bind(reach, vec![ALPN.to_vec()], None, true))?;
        let net = Net::new(runtime, endpoint, reach, events);
        net.runtime.spawn(accept_loop(net.endpoint.clone(), net.links.clone(), net.gate.clone(), net.events.clone()));
        Ok(net)
    }

    /// Find a host by its code, and keep reconnecting for a while if the connection drops.
    /// Fails at once on something that isn't a code; a code nobody answers to arrives later as
    /// [`NetEvent::GaveUp`].
    pub fn join(reach: Reach, code: &str, events: mpsc::Sender<NetEvent>) -> Result<Net, String> {
        let code = parse_code(code)?;
        let runtime = runtime()?;
        // Nobody needs to find a guest: it does the dialling.
        let endpoint = runtime.block_on(bind(reach, vec![], None, false))?;
        let net = Net::new(runtime, endpoint, reach, events);
        let (endpoint, links, stopping, events) =
            (net.endpoint.clone(), net.links.clone(), net.stopping.clone(), net.events.clone());
        net.runtime.spawn(async move {
            match knock(&endpoint, &code).await {
                Some(addr) => dial_loop(endpoint, addr, links, stopping, events).await,
                None => {
                    let why = "no session answered to that code. codes change every 5 minutes and after each use: ask for the current one.";
                    let _ = events.send(NetEvent::GaveUp(why.to_string()));
                }
            }
        });
        Ok(net)
    }

    /// This device's key.
    pub fn key(&self) -> DeviceKey {
        *self.endpoint.id().as_bytes()
    }

    /// A host: let people in with `code`, and stop letting them in with any code before it.
    /// Returns at once; the lobby takes a moment to be findable.
    pub fn open_lobby(&self, code: &str) {
        let generation = self.retire_lobby();
        let code = parse_code(code).unwrap_or_default();
        let (reach, slot, host, gate, events) =
            (self.reach, self.lobby.clone(), self.endpoint.clone(), self.gate.clone(), self.events.clone());
        self.runtime.spawn(async move {
            let (day, _) = code::today();
            let Ok(derived) = tokio::task::spawn_blocking(move || code::derive(&code, day)).await else { return };
            let lobby = match bind(reach, vec![LOBBY_ALPN.to_vec()], Some(derived.lobby), true).await {
                Ok(lobby) => lobby,
                Err(e) => {
                    log::warn!("could not open a lobby: {e}");
                    return;
                }
            };
            let overtaken = {
                let mut slot = slot.lock();
                let overtaken = slot.generation != generation;
                if !overtaken {
                    slot.endpoint = Some(lobby.clone());
                }
                overtaken
            };
            if overtaken {
                lobby.close().await;
                return;
            }
            lobby_loop(lobby, derived.proof, host, gate, events).await;
        });
    }

    /// A host: stop letting anyone new in. People already in stay in.
    pub fn close_lobby(&self) {
        self.retire_lobby();
    }

    fn retire_lobby(&self) -> u64 {
        let mut slot = self.lobby.lock();
        slot.generation += 1;
        if let Some(old) = slot.endpoint.take() {
            self.runtime.spawn(async move { old.close().await });
        }
        slot.generation
    }

    /// A host: this device is in, and may come back whenever its connection drops.
    pub fn admit(&self, key: &DeviceKey) {
        if let Ok(id) = EndpointId::from_bytes(key) {
            self.gate.lock().allowed.insert(id, None);
        }
    }

    /// A host: this device may not connect again without a new code.
    pub fn revoke(&self, key: &DeviceKey) {
        if let Ok(id) = EndpointId::from_bytes(key) {
            self.gate.lock().allowed.remove(&id);
        }
    }

    /// Send a message on a connection, in order, reliably.
    pub fn send(&self, conn: ConnId, bytes: &[u8]) {
        if let Some(link) = self.links.lock().get(&conn) {
            let _ = link.out.send(Out::Frame(frame(bytes)));
        }
    }

    /// Send a datagram: it may arrive late, out of order, or not at all.
    pub fn datagram(&self, conn: ConnId, bytes: &[u8]) {
        if let Some(link) = self.links.lock().get(&conn) {
            let _ = link.conn.send_datagram(bytes.to_vec().into());
        }
    }

    /// Close a connection once what is queued for it has gone out.
    pub fn close(&self, conn: ConnId) {
        if let Some(link) = self.links.lock().remove(&conn) {
            let _ = link.out.send(Out::Close);
        }
    }

    /// How this machine is reaching the host right now, for the person: "direct" or "relayed".
    pub fn path(&self, conn: ConnId) -> Option<&'static str> {
        let links = self.links.lock();
        let link = links.get(&conn)?;
        let paths = link.conn.paths();
        let selected = paths.iter().find(|p| p.is_selected())?;
        Some(if selected.is_relay() { "relayed" } else { "direct" })
    }

    /// Say goodbye on every connection and stop. Blocks for up to about two seconds, so keep it
    /// off any thread somebody is waiting on.
    pub fn shutdown(self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.retire_lobby();
        for (_, link) in self.links.lock().drain() {
            let _ = link.out.send(Out::Close);
        }
        let endpoint = self.endpoint.clone();
        self.runtime.block_on(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let _ = tokio::time::timeout(Duration::from_millis(1500), endpoint.close()).await;
        });
        self.runtime.shutdown_background();
    }
}

/// Answer at a lobby until one device has tried it, rightly or wrongly. Whoever proves the code
/// is told where the host is and let through the gate; anyone else spends the code for nothing.
async fn lobby_loop(lobby: Endpoint, proof: [u8; 32], host: Endpoint, gate: Arc<Mutex<Gate>>, events: mpsc::Sender<NetEvent>) {
    let spent = Arc::new(AtomicBool::new(false));
    while let Some(incoming) = lobby.accept().await {
        let (lobby, host, gate, events, spent) = (lobby.clone(), host.clone(), gate.clone(), events.clone(), spent.clone());
        tokio::spawn(async move {
            let Ok(accepting) = incoming.accept() else { return };
            let Ok(conn) = accepting.await else { return };
            let offered = tokio::time::timeout(PROOF_WAIT, async {
                let (send, mut recv) = conn.accept_bi().await.ok()?;
                Some((send, read_frame(&mut recv, proof.len()).await?))
            })
            .await
            .ok()
            .flatten();
            if spent.swap(true, Ordering::SeqCst) {
                conn.close(1u32.into(), b"spent");
                return;
            }
            let Some((mut send, _)) = offered.filter(|(_, offered)| code::same(offered, &proof)) else {
                log::warn!("someone reached the lobby without the code");
                conn.close(2u32.into(), b"wrong code");
                let _ = events.send(NetEvent::BadKnock);
                lobby.close().await;
                return;
            };
            let guest = conn.remote_id();
            gate.lock().knocked(guest);
            if let Ok(addr) = serde_json::to_vec(&host.addr()) {
                let _ = send.write_all(&frame(&addr)).await;
                let _ = send.finish();
                let _ = tokio::time::timeout(Duration::from_secs(2), send.stopped()).await;
            }
            let _ = events.send(NetEvent::Knocked(*guest.as_bytes()));
            conn.close(0u32.into(), b"see you there");
            lobby.close().await;
        });
    }
}

async fn accept_loop(
    endpoint: Endpoint,
    links: Arc<Mutex<HashMap<ConnId, Link>>>,
    gate: Arc<Mutex<Gate>>,
    events: mpsc::Sender<NetEvent>,
) {
    let next = Arc::new(AtomicU64::new(HOST + 1));
    while let Some(incoming) = endpoint.accept().await {
        let (links, gate, events, next, own) = (links.clone(), gate.clone(), events.clone(), next.clone(), endpoint.id());
        tokio::spawn(async move {
            let Ok(accepting) = incoming.accept() else { return };
            let Ok(conn) = accepting.await else { return };
            if !gate.lock().lets_in(&conn.remote_id()) {
                log::warn!("turned away a device that didn't come in with a code");
                conn.close(3u32.into(), b"not invited");
                return;
            }
            let Ok((send, recv)) = conn.accept_bi().await else { return };
            let peer = peer(own, &conn, true).await;
            let id = next.fetch_add(1, Ordering::SeqCst);
            let (out, rx) = tokio_mpsc::unbounded_channel();
            links.lock().insert(id, Link { out, conn: conn.clone() });
            let _ = events.send(NetEvent::Connected(id, peer));
            serve(id, conn, send, recv, rx, events.clone()).await;
            links.lock().remove(&id);
            let _ = events.send(NetEvent::Disconnected(id));
        });
    }
}

/// Prove the code at its lobby and learn where the host is. Tries every day the code could have
/// been made on at once, and keeps trying for a while: a new lobby takes a moment to be findable.
async fn knock(endpoint: &Endpoint, code: &str) -> Option<EndpointAddr> {
    let mut tries = JoinSet::new();
    for day in code::candidate_days() {
        let (endpoint, code) = (endpoint.clone(), code.to_string());
        tries.spawn(async move {
            let derived = tokio::task::spawn_blocking(move || code::derive(&code, day)).await.ok()?;
            let lobby = EndpointAddr::from(derived.lobby.public());
            let started = Instant::now();
            loop {
                match knock_once(&endpoint, &lobby, &derived.proof).await {
                    Ok(addr) => return Some(addr),
                    Err(e) => log::info!("no lobby yet: {e}"),
                }
                if started.elapsed() >= KNOCK_WINDOW {
                    return None;
                }
                tokio::time::sleep(RECONNECT_PAUSE).await;
            }
        });
    }
    while let Some(done) = tries.join_next().await {
        if let Ok(Some(addr)) = done {
            return Some(addr);
        }
    }
    None
}

async fn knock_once(endpoint: &Endpoint, lobby: &EndpointAddr, proof: &[u8; 32]) -> Result<EndpointAddr, String> {
    let conn = tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(lobby.clone(), LOBBY_ALPN))
        .await
        .map_err(|_| "timed out".to_string())?
        .map_err(|e| e.to_string())?;
    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    send.write_all(&frame(proof)).await.map_err(|e| e.to_string())?;
    let _ = send.finish();
    let bytes = read_frame(&mut recv, MAX_LOBBY_FRAME).await.ok_or("the lobby said nothing")?;
    conn.close(0u32.into(), b"thanks");
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

async fn dial(endpoint: &Endpoint, addr: &EndpointAddr) -> Result<(Connection, SendStream, RecvStream), String> {
    let conn = tokio::time::timeout(CONNECT_TIMEOUT, endpoint.connect(addr.clone(), ALPN))
        .await
        .map_err(|_| "timed out".to_string())?
        .map_err(|e| e.to_string())?;
    let (send, recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    Ok((conn, send, recv))
}

async fn dial_loop(
    endpoint: Endpoint,
    addr: EndpointAddr,
    links: Arc<Mutex<HashMap<ConnId, Link>>>,
    stopping: Arc<AtomicBool>,
    events: mpsc::Sender<NetEvent>,
) {
    let mut trying_since = Instant::now();
    let mut ever_connected = false;
    // The same two keys every time, so the same words.
    let mut known: Option<Peer> = None;
    while !stopping.load(Ordering::SeqCst) {
        match dial(&endpoint, &addr).await {
            Ok((conn, send, recv)) => {
                ever_connected = true;
                let peer = match known.clone() {
                    Some(peer) => peer,
                    None => peer(endpoint.id(), &conn, false).await,
                };
                known = Some(peer.clone());
                let (out, rx) = tokio_mpsc::unbounded_channel();
                links.lock().insert(HOST, Link { out, conn: conn.clone() });
                let _ = events.send(NetEvent::Connected(HOST, peer));
                serve(HOST, conn, send, recv, rx, events.clone()).await;
                links.lock().remove(&HOST);
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                let _ = events.send(NetEvent::Disconnected(HOST));
                trying_since = Instant::now();
            }
            Err(e) => {
                log::info!("could not reach the host: {e}");
                let window = if ever_connected { RECONNECT_WINDOW } else { CONNECT_TIMEOUT };
                if trying_since.elapsed() >= window {
                    let why = if ever_connected {
                        "lost the connection to the host."
                    } else {
                        "couldn't reach the host. check they're still sharing."
                    };
                    let _ = events.send(NetEvent::GaveUp(why.to_string()));
                    return;
                }
            }
        }
        tokio::time::sleep(RECONNECT_PAUSE).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Someone who spotted the lobby on the network, but never saw the code.
    #[test]
    fn a_wrong_proof_spends_the_code_and_tells_the_host() {
        let (tx, rx) = mpsc::channel();
        let host = Net::host(Reach::Local, tx).unwrap();
        let code = new_code();
        host.open_lobby(&code);
        let lobby = EndpointAddr::from(code::derive(&code, code::today().0).lobby.public());
        let runtime = runtime().unwrap();
        let answer = runtime.block_on(async {
            let snoop = bind(Reach::Local, vec![], None, false).await.unwrap();
            let mut answer = Err(String::new());
            for _ in 0..15 {
                answer = knock_once(&snoop, &lobby, &[0; 32]).await;
                // Reached it, and was sent away: that is the one try that counts.
                if answer.as_ref().is_err_and(|e| e == "the lobby said nothing") {
                    break;
                }
                tokio::time::sleep(RECONNECT_PAUSE).await;
            }
            snoop.close().await;
            answer
        });
        assert!(answer.is_err(), "the lobby gave the host away");
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), NetEvent::BadKnock);
        host.shutdown();
    }
}
