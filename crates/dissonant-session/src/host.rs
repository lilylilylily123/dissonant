//! The host's side of a session.
//!
//! Nobody new gets in on their own say-so. The network only brings a device this far if it came
//! through the lobby with the current code; after that the host is asked, and until they say yes
//! the device sees nothing of the project. Who someone is afterwards is their device key, which
//! the connection's handshake proves: the client id a guest sends is visible to everyone in the
//! session, so it can name a place but never claim one.

use crate::protocol::{OpBody, PeerInfo, ToGuest, ToHost, PROTOCOL_VERSION};
use dissonant_core::{ids, Command, Document, EditError, Patch, StepOutcome, TimeEdit, SCHEMA_VERSION};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// A connection, as the network layer numbers them.
pub type ConnId = u64;

/// A device's public key, as the connection's handshake proved it.
pub type DeviceKey = [u8; 32];

/// A session holds this many people, host included.
const MAX_PEOPLE: usize = 8;

struct Guest {
    info: PeerInfo,
    key: DeviceKey,
    /// The last op seen from this guest. Survives a dropped connection.
    last_seq: u64,
    conn: Option<ConnId>,
}

/// Someone waiting for the host to let them in.
#[derive(Debug, Clone, PartialEq)]
pub struct JoinRequest {
    pub info: PeerInfo,
    pub key: DeviceKey,
    pub conn: ConnId,
}

/// What came of handling something: messages to send, and what the app has to do about it.
#[derive(Debug, Default)]
pub struct HostEvents {
    pub send: Vec<(ConnId, ToGuest)>,
    /// The project changed by this. The app's engine has to catch up.
    pub changed: Option<Patch>,
    pub peers_changed: bool,
    /// Who is waiting to be let in changed.
    pub requests_changed: bool,
    /// Someone new is asking to be let in: worth getting the host's attention.
    pub asked: bool,
    /// A device that is in now, and may reconnect whenever it likes.
    pub admit: Option<DeviceKey>,
    /// A device that may not connect again without a new code.
    pub revoke: Option<DeviceKey>,
    /// A connection to close once `send` has gone out.
    pub close: Option<ConnId>,
}

pub struct HostSession {
    id: Uuid,
    name: String,
    color: String,
    revision: u64,
    guests: HashMap<Uuid, Guest>,
    /// Oldest first.
    requests: Vec<JoinRequest>,
    /// Who is on each open connection.
    keys: HashMap<ConnId, DeviceKey>,
    /// Removed by the host. Coming back takes a new code and the host saying yes again.
    removed: HashSet<DeviceKey>,
    /// Letting new people ask to come in.
    open: bool,
}

impl HostSession {
    pub fn new(name: String, color: String) -> Self {
        HostSession {
            id: Uuid::new_v4(),
            name,
            color,
            revision: 0,
            guests: HashMap::new(),
            requests: Vec::new(),
            keys: HashMap::new(),
            removed: HashSet::new(),
            open: true,
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether new people may ask to come in. People already in stay in, and can come back.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// People waiting to be let in, oldest first.
    pub fn requests(&self) -> &[JoinRequest] {
        &self.requests
    }

    /// Everyone connected, host first.
    pub fn peers(&self) -> Vec<PeerInfo> {
        let host = PeerInfo { id: self.id, name: self.name.clone(), color: self.color.clone(), is_host: true };
        let mut guests: Vec<PeerInfo> = self.guests.values().filter(|g| g.conn.is_some()).map(|g| g.info.clone()).collect();
        guests.sort_by_key(|p| p.id);
        std::iter::once(host).chain(guests).collect()
    }

    fn conns(&self) -> impl Iterator<Item = ConnId> + '_ {
        self.guests.values().filter_map(|g| g.conn)
    }

    fn guest_on(&self, conn: ConnId) -> Option<Uuid> {
        self.guests.iter().find(|(_, g)| g.conn == Some(conn)).map(|(id, _)| *id)
    }

    /// Whoever is on `conn`, for stamping their cursor. Nobody, until they are let in.
    pub fn client_on(&self, conn: ConnId) -> Option<Uuid> {
        self.guest_on(conn)
    }

    /// Every open connection of someone let in, except `conn`.
    pub fn others(&self, conn: Option<ConnId>) -> Vec<ConnId> {
        self.conns().filter(|c| Some(*c) != conn).collect()
    }

    /// A connection opened, from the device with this key.
    pub fn on_connect(&mut self, conn: ConnId, key: DeviceKey) {
        self.keys.insert(conn, key);
    }

    fn tell_peers(&self, out: &mut HostEvents) {
        let peers = self.peers();
        out.send.extend(self.conns().map(|c| (c, ToGuest::Peers { peers: peers.clone() })));
        out.peers_changed = true;
    }

    /// Tell everyone about a change. It has to go out even when it's empty if a guest is waiting
    /// on its answer.
    fn publish(&mut self, out: &mut HostEvents, origin: Uuid, seq: u64, patch: Patch, time_edit: Option<TimeEdit>) {
        self.revision += 1;
        let message = ToGuest::Applied { revision: self.revision, origin, seq, patch: patch.clone(), time_edit };
        out.send.extend(self.conns().map(|c| (c, message.clone())));
        if !patch.is_empty() {
            out.changed = Some(patch);
        }
    }

    /// The host's own edit, with history as usual.
    pub fn edit(
        &mut self,
        doc: &mut Document,
        command: Command,
        transient: bool,
        label: Option<String>,
    ) -> Result<HostEvents, EditError> {
        let mut out = HostEvents::default();
        let before = doc.model().clone();
        let time_edit = command.time_edit();
        doc.apply_labeled(command, transient, label)?;
        let patch = Patch::between(&before, doc.model()).without_personal();
        if !patch.is_empty() {
            self.publish(&mut out, self.id, 0, patch, time_edit);
        }
        Ok(out)
    }

    /// The host's own undo or redo. Returns the outcome so the app can say when it was blocked.
    pub fn step(&mut self, doc: &mut Document, undo: bool) -> (StepOutcome, HostEvents) {
        let mut out = HostEvents::default();
        let outcome = if undo { doc.undo_step() } else { doc.redo_step() };
        if let StepOutcome::Applied(change) = &outcome {
            let patch = change.without_personal();
            if !patch.is_empty() {
                self.publish(&mut out, self.id, 0, patch, None);
            }
        }
        (outcome, out)
    }

    pub fn on_message(&mut self, doc: &mut Document, conn: ConnId, message: ToHost) -> HostEvents {
        let mut out = HostEvents::default();
        match message {
            ToHost::Hello { protocol, schema, client, name, color } => {
                let info = PeerInfo { id: client, name, color, is_host: false };
                self.on_hello(doc, conn, protocol, schema, info, &mut out);
            }
            ToHost::Op { seq, op_id, body } => {
                if let Some(client) = self.guest_on(conn) {
                    self.on_op(doc, conn, client, seq, op_id, body, &mut out);
                }
            }
            ToHost::Bye => {
                if let Some(client) = self.guest_on(conn) {
                    self.guests.remove(&client);
                    self.tell_peers(&mut out);
                }
                self.drop_request(conn, &mut out);
                out.close = Some(conn);
            }
        }
        out
    }

    fn refuse(conn: ConnId, reason: &str, out: &mut HostEvents) {
        out.send.push((conn, ToGuest::Refused { reason: reason.to_string() }));
        out.close = Some(conn);
    }

    fn full(&self) -> bool {
        1 + self.guests.len() >= MAX_PEOPLE
    }

    fn on_hello(&mut self, doc: &Document, conn: ConnId, protocol: u32, schema: u32, info: PeerInfo, out: &mut HostEvents) {
        let Some(&key) = self.keys.get(&conn) else {
            return Self::refuse(conn, "couldn't tell which device this is. try joining again.", out);
        };
        if protocol != PROTOCOL_VERSION || schema != SCHEMA_VERSION {
            return Self::refuse(conn, "you and the host are on different versions of dissonant. use the same build.", out);
        }
        if self.removed.contains(&key) {
            return Self::refuse(conn, "the host removed you from this session.", out);
        }
        // Back after a dropped connection: the same place, if it is the same device.
        if let Some(guest) = self.guests.get(&info.id) {
            if guest.key != key {
                out.revoke = Some(key);
                return Self::refuse(conn, "that place in the session belongs to someone else.", out);
            }
            return self.welcome(doc, conn, info, out);
        }
        // Still waiting, on a new connection.
        if let Some(request) = self.requests.iter_mut().find(|r| r.key == key) {
            request.conn = conn;
            request.info = info;
            out.send.push((conn, ToGuest::Waiting));
            out.requests_changed = true;
            return;
        }
        if !self.open {
            return Self::refuse(conn, "the host isn't letting anyone new in right now.", out);
        }
        if self.full() {
            return Self::refuse(conn, &format!("this session is full ({MAX_PEOPLE} people)."), out);
        }
        self.requests.push(JoinRequest { info, key, conn });
        out.send.push((conn, ToGuest::Waiting));
        out.requests_changed = true;
        out.asked = true;
    }

    /// Let someone in, or not.
    pub fn answer(&mut self, doc: &Document, client: Uuid, allow: bool) -> HostEvents {
        let mut out = HostEvents::default();
        let Some(at) = self.requests.iter().position(|r| r.info.id == client) else { return out };
        let request = self.requests.remove(at);
        out.requests_changed = true;
        if !allow {
            out.revoke = Some(request.key);
            Self::refuse(request.conn, "the host didn't let you in.", &mut out);
        } else if self.full() {
            out.revoke = Some(request.key);
            Self::refuse(request.conn, &format!("this session is full ({MAX_PEOPLE} people)."), &mut out);
        } else {
            out.admit = Some(request.key);
            let guest = Guest { info: request.info.clone(), key: request.key, last_seq: 0, conn: None };
            self.guests.insert(request.info.id, guest);
            self.welcome(doc, request.conn, request.info, &mut out);
        }
        out
    }

    /// Someone let in gets the project on `conn`.
    fn welcome(&mut self, doc: &Document, conn: ConnId, info: PeerInfo, out: &mut HostEvents) {
        let Some(guest) = self.guests.get_mut(&info.id) else { return };
        guest.info = info;
        guest.conn = Some(conn);
        let last_seq = guest.last_seq;
        // Solo is what the host is listening to right now, not a state of the song: a guest who
        // inherited it would join to one track and no idea why.
        let mut model = doc.model().clone();
        for t in &mut model.tracks {
            t.soloed = false;
        }
        out.send.push((
            conn,
            ToGuest::Welcome { revision: self.revision, last_seq, model: Box::new(model), peers: self.peers() },
        ));
        self.tell_peers(out);
    }

    fn drop_request(&mut self, conn: ConnId, out: &mut HostEvents) {
        let before = self.requests.len();
        self.requests.retain(|r| r.conn != conn);
        out.requests_changed |= self.requests.len() != before;
    }

    #[allow(clippy::too_many_arguments)]
    fn on_op(
        &mut self,
        doc: &mut Document,
        conn: ConnId,
        client: Uuid,
        seq: u64,
        op_id: Uuid,
        body: OpBody,
        out: &mut HostEvents,
    ) {
        let Some(guest) = self.guests.get_mut(&client) else { return };
        if seq <= guest.last_seq {
            return;
        }
        guest.last_seq = seq;
        match body {
            OpBody::Command(command) => {
                if command.is_personal() || command.adds_audio() {
                    let reason = "that can't be done from a guest's side".to_string();
                    out.send.push((conn, ToGuest::Rejected { seq, reason }));
                    return;
                }
                let time_edit = command.time_edit();
                match ids::scoped(op_id, || doc.apply_external(command)) {
                    Ok(change) => self.publish(out, client, seq, change.without_personal(), time_edit),
                    Err(e) => out.send.push((conn, ToGuest::Rejected { seq, reason: e.to_string() })),
                }
            }
            OpBody::Patch(patch) => {
                let change = doc.apply_external_patch(&patch.without_personal());
                self.publish(out, client, seq, change.without_personal(), None);
            }
        }
    }

    /// A connection dropped. A guest's place is kept, in case it comes back; someone still
    /// waiting stops waiting.
    pub fn on_disconnect(&mut self, conn: ConnId) -> HostEvents {
        let mut out = HostEvents::default();
        self.keys.remove(&conn);
        self.drop_request(conn, &mut out);
        if let Some(client) = self.guest_on(conn) {
            if let Some(guest) = self.guests.get_mut(&client) {
                guest.conn = None;
            }
            self.tell_peers(&mut out);
        }
        out
    }

    /// Put someone out of the session for good.
    pub fn remove(&mut self, client: Uuid) -> HostEvents {
        let mut out = HostEvents::default();
        if let Some(guest) = self.guests.remove(&client) {
            self.removed.insert(guest.key);
            out.revoke = Some(guest.key);
            if let Some(conn) = guest.conn {
                let reason = "the host removed you from this session.".to_string();
                out.send.push((conn, ToGuest::Ended { reason }));
                out.close = Some(conn);
            }
        }
        self.tell_peers(&mut out);
        out
    }

    /// End the session for everyone, waiting or in.
    pub fn end(&mut self) -> HostEvents {
        let mut out = HostEvents::default();
        let reason = "the host ended the session.".to_string();
        let waiting = self.requests.iter().map(|r| r.conn);
        out.send.extend(self.conns().chain(waiting).map(|c| (c, ToGuest::Ended { reason: reason.clone() })));
        out
    }
}
