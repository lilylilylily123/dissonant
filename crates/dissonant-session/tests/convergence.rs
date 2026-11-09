//! A whole session with no network: a host and guests editing at random while their messages
//! arrive late, connections drop, and everyone undoes things. However it goes, once the messages
//! stop everyone has to be looking at the same project.

use dissonant_core::{ChordEvent, Command, Document, NoteEvent, ProjectModel, StepOutcome, TrackParam};
use dissonant_session::{narrow, Bases, ConnId, DeviceKey, GuestSession, HostEvents, HostSession, ToGuest, ToHost, PROTOCOL_VERSION};
use std::collections::VecDeque;

/// Small and seeded, so a failure comes back from its seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

struct Client {
    conn: ConnId,
    key: DeviceKey,
    session: GuestSession,
    doc: Document,
    bases: Bases,
    to_host: VecDeque<ToHost>,
    from_host: VecDeque<ToGuest>,
    online: bool,
}

struct Sim {
    host: HostSession,
    doc: Document,
    bases: Bases,
    guests: Vec<Client>,
    next_conn: ConnId,
}

fn starter() -> ProjectModel {
    let mut m = ProjectModel::empty();
    m.patterns[0].notes = (0..6).map(|i| NoteEvent::new(i * 240, 240, 60 + i as i32)).collect();
    m.song_length_ticks = 64 * 960;
    m
}

/// The project, without what is each person's own.
fn shared(m: &ProjectModel) -> ProjectModel {
    let mut m = m.clone();
    for t in &mut m.tracks {
        t.muted = false;
        t.soloed = false;
    }
    m.loop_region = None;
    m.punch_region = None;
    m
}

impl Sim {
    fn new(guests: usize) -> Sim {
        let mut sim = Sim {
            host: HostSession::new("host".into(), "#fff".into()),
            doc: Document::new(starter()),
            bases: Bases::default(),
            guests: vec![],
            next_conn: 1,
        };
        for i in 0..guests {
            let session = GuestSession::new("guest".into(), "#000".into());
            let hello = session.hello();
            let conn = sim.next_conn;
            sim.next_conn += 1;
            let key = [i as u8 + 1; 32];
            sim.host.on_connect(conn, key);
            sim.guests.push(Client {
                conn,
                key,
                session,
                doc: Document::new(ProjectModel::empty()),
                bases: Bases::default(),
                to_host: VecDeque::from([hello]),
                from_host: VecDeque::new(),
                online: true,
            });
        }
        sim.drain();
        // Everyone is waiting to be let in; the host says yes to all of them.
        let waiting: Vec<_> = sim.host.requests().iter().map(|r| r.info.id).collect();
        for id in waiting {
            let out = sim.host.answer(&sim.doc, id, true);
            sim.route(out);
        }
        sim.drain();
        sim
    }

    fn route(&mut self, out: HostEvents) {
        for (conn, message) in out.send {
            if let Some(g) = self.guests.iter_mut().find(|g| g.conn == conn) {
                g.from_host.push_back(message);
            }
        }
    }

    fn deliver_to_host(&mut self, i: usize) {
        let Some(message) = self.guests[i].to_host.pop_front() else { return };
        let out = self.host.on_message(&mut self.doc, self.guests[i].conn, message);
        self.bases.note(self.doc.revision(), self.doc.model());
        self.route(out);
    }

    fn deliver_to_guest(&mut self, i: usize) {
        let g = &mut self.guests[i];
        let Some(message) = g.from_host.pop_front() else { return };
        let out = g.session.on_message(&mut g.doc, message);
        g.bases.note(g.doc.revision(), g.doc.model());
        g.to_host.extend(out.send);
        assert_eq!(out.ended, None, "a guest was thrown out");
    }

    fn drain(&mut self) {
        loop {
            let mut moved = false;
            for i in 0..self.guests.len() {
                if !self.guests[i].online {
                    continue;
                }
                while !self.guests[i].to_host.is_empty() {
                    self.deliver_to_host(i);
                    moved = true;
                }
                while !self.guests[i].from_host.is_empty() {
                    self.deliver_to_guest(i);
                    moved = true;
                }
            }
            if !moved {
                return;
            }
        }
    }

    fn drop_connection(&mut self, i: usize) {
        let out = self.host.on_disconnect(self.guests[i].conn);
        self.route(out);
        let g = &mut self.guests[i];
        g.session.disconnected();
        g.online = false;
        g.to_host.clear();
        g.from_host.clear();
    }

    fn reconnect(&mut self, i: usize) {
        let conn = self.next_conn;
        self.next_conn += 1;
        let g = &mut self.guests[i];
        g.conn = conn;
        g.online = true;
        let hello = g.session.hello();
        g.to_host.push_back(hello);
        self.host.on_connect(conn, g.key);
    }
}

/// A random edit, made from `base`: what this person saw when they started, maybe a while ago.
fn random_command(rng: &mut Rng, now: &ProjectModel, base: &ProjectModel) -> Command {
    let track = now.tracks[rng.below(now.tracks.len())].id;
    let pattern = now.patterns.get(rng.below(now.patterns.len().max(1)));
    match rng.below(14) {
        0 => Command::SetTempo { bpm: 60.0 + rng.below(120) as f64 + 0.1 },
        1 => Command::AddTrack { is_drum: rng.chance(30) },
        2 if now.tracks.len() > 2 => Command::DeleteTrack { id: track },
        3 => Command::SetTrackParam { id: track, param: TrackParam::Volume, value: rng.below(100) as f64 / 100.0 },
        4 => Command::AddPattern { track_id: track },
        5 => Command::InsertTime { at: rng.below(32) as i64 * 960, ticks: 960 },
        6 => Command::DeleteTime { at: rng.below(32) as i64 * 960, ticks: 960 },
        7 => Command::SetTrackMuted { id: track, muted: rng.chance(50) },
        8 => Command::SetLoopRegion { region: Some((960, 960 * (2 + rng.below(30) as i64))) },
        9 => {
            let chords = vec![ChordEvent::new(rng.below(16) as i64 * 960, 960, vec![0, 4, 7], None)];
            Command::SetChords { chords }
        }
        10 if pattern.is_some() => Command::AddClip {
            pattern_id: pattern.unwrap().id,
            start_tick: rng.below(32) as i64 * 960,
            length_ticks: None,
        },
        _ => {
            let Some(pattern) = pattern else { return Command::SetSwing { swing: 55.0, grid_ticks: 240 } };
            let id = pattern.id;
            let mut notes = base.pattern(&id).map(|p| p.notes.clone()).unwrap_or_default();
            match rng.below(3) {
                0 => notes.push(NoteEvent::new(rng.below(64) as i64 * 120, 120, 40 + rng.below(40) as i32)),
                1 if !notes.is_empty() => {
                    notes.remove(rng.below(notes.len()));
                }
                _ if !notes.is_empty() => {
                    let i = rng.below(notes.len());
                    notes[i].pitch = 40 + rng.below(40) as i32;
                }
                _ => {}
            }
            Command::SetNotes { pattern_id: id, notes }
        }
    }
}

fn run(seed: u64, steps: usize, guests: usize) {
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    let mut sim = Sim::new(guests);
    let mut last_seen: Vec<u64> = sim.guests.iter().map(|g| g.doc.revision()).collect();
    for _ in 0..steps {
        let who = rng.below(guests + 1);
        match rng.below(100) {
            // The host edits.
            0..=19 if who == guests => {
                let base = sim.doc.model().clone();
                let command = narrow(random_command(&mut rng, sim.doc.model(), &base), &base);
                let transient = rng.chance(20);
                if let Ok(out) = sim.host.edit(&mut sim.doc, command, transient, None) {
                    sim.route(out);
                }
                if !transient {
                    sim.doc.commit_gesture();
                }
                sim.bases.note(sim.doc.revision(), sim.doc.model());
            }
            // A guest edits, from a view that may be well behind.
            0..=44 if who < guests => {
                let g = &mut sim.guests[who];
                let base = g.bases.at(last_seen[who]).cloned().unwrap_or_else(|| g.doc.model().clone());
                let command = narrow(random_command(&mut rng, g.doc.model(), &base), &base);
                if let Ok(Some(message)) = g.session.edit(&mut g.doc, command, false, None) {
                    g.to_host.push_back(message);
                }
                g.bases.note(g.doc.revision(), g.doc.model());
                if rng.chance(50) {
                    last_seen[who] = g.doc.revision();
                }
            }
            45..=52 => {
                if who == guests {
                    let (_, out) = sim.host.step(&mut sim.doc, rng.chance(70));
                    sim.route(out);
                } else {
                    let undo = rng.chance(70);
                    let g = &mut sim.guests[who];
                    if let (_, Some(message)) = g.session.step(&mut g.doc, undo) {
                        g.to_host.push_back(message);
                    }
                }
            }
            53..=56 if who < guests => {
                if sim.guests[who].online {
                    sim.drop_connection(who);
                } else {
                    sim.reconnect(who);
                }
            }
            _ if who < guests && sim.guests[who].online => {
                if rng.chance(50) {
                    sim.deliver_to_host(who);
                } else {
                    sim.deliver_to_guest(who);
                }
            }
            _ => {}
        }
    }
    for i in 0..guests {
        if !sim.guests[i].online {
            sim.reconnect(i);
        }
    }
    sim.drain();
    let host = shared(sim.doc.model());
    for (i, g) in sim.guests.iter().enumerate() {
        let mine = shared(g.doc.model());
        if mine != host {
            let gap = dissonant_core::Patch::between(&host, &mine);
            let text = serde_json::to_string(&gap).unwrap();
            panic!("seed {seed}: guest {i} drifted: {}", &text[..text.len().min(1500)]);
        }
    }
}

#[test]
fn everyone_ends_up_looking_at_the_same_project() {
    // Set DISSONANT_SIM_RUNS=2000 for a long soak; a failure reproduces from the seed it prints.
    let runs = std::env::var("DISSONANT_SIM_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(60);
    for seed in 1..=runs {
        run(seed, 300, 1 + (seed as usize % 3));
    }
}

fn one_guest() -> Sim {
    Sim::new(1)
}

#[test]
fn two_people_adding_notes_keep_both() {
    let mut sim = one_guest();
    let pattern = sim.doc.model().patterns[0].id;
    let base = sim.doc.model().clone();
    let mine = {
        let mut n = base.patterns[0].notes.clone();
        n.push(NoteEvent::new(5000, 120, 70));
        narrow(Command::SetNotes { pattern_id: pattern, notes: n }, &base)
    };
    let theirs = {
        let mut n = base.patterns[0].notes.clone();
        n.push(NoteEvent::new(6000, 120, 71));
        narrow(Command::SetNotes { pattern_id: pattern, notes: n }, &base)
    };
    let out = sim.host.edit(&mut sim.doc, mine, false, None).unwrap();
    sim.route(out);
    let g = &mut sim.guests[0];
    let message = g.session.edit(&mut g.doc, theirs, false, None).unwrap().unwrap();
    g.to_host.push_back(message);
    sim.drain();
    assert_eq!(sim.doc.model().patterns[0].notes.len(), 8);
    assert_eq!(shared(sim.guests[0].doc.model()), shared(sim.doc.model()));
}

#[test]
fn a_note_added_during_my_drag_survives_my_drag() {
    let mut sim = one_guest();
    let pattern = sim.doc.model().patterns[0].id;
    // The guest starts dragging from what it sees now.
    let started_from = sim.guests[0].doc.model().clone();
    // The host adds a note while the guest is mid-drag.
    let mut added = sim.doc.model().patterns[0].notes.clone();
    added.push(NoteEvent::new(7000, 120, 71));
    let base = sim.doc.model().clone();
    let command = narrow(Command::SetNotes { pattern_id: pattern, notes: added }, &base);
    let out = sim.host.edit(&mut sim.doc, command, false, None).unwrap();
    sim.route(out);
    sim.drain();
    // The drag lands: an array that has never heard of that note.
    let mut moved = started_from.patterns[0].notes.clone();
    moved[0].pitch = 30;
    let command = narrow(Command::SetNotes { pattern_id: pattern, notes: moved }, &started_from);
    let g = &mut sim.guests[0];
    let message = g.session.edit(&mut g.doc, command, false, None).unwrap().unwrap();
    g.to_host.push_back(message);
    sim.drain();
    let notes = &sim.doc.model().patterns[0].notes;
    assert_eq!(notes.len(), 7, "the host's note is still there");
    assert!(notes.iter().any(|n| n.pitch == 30), "and the drag landed");
    assert_eq!(shared(sim.guests[0].doc.model()), shared(sim.doc.model()));
}

#[test]
fn undo_only_takes_back_your_own_edit() {
    let mut sim = one_guest();
    let g = &mut sim.guests[0];
    let message = g.session.edit(&mut g.doc, Command::SetTempo { bpm: 111.0 }, false, None).unwrap().unwrap();
    g.to_host.push_back(message);
    sim.drain();
    let id = sim.doc.model().tracks[0].id;
    let out = sim.host.edit(&mut sim.doc, Command::RenameTrack { id, name: "mine".into() }, false, None).unwrap();
    sim.route(out);
    sim.drain();
    let g = &mut sim.guests[0];
    let (outcome, message) = g.session.step(&mut g.doc, true);
    assert!(matches!(outcome, StepOutcome::Applied(_)));
    g.to_host.push_back(message.unwrap());
    sim.drain();
    assert_ne!(sim.doc.model().tempo, 111.0, "the guest's tempo edit is undone");
    assert_eq!(sim.doc.model().tracks[0].name, "mine", "the host's rename is not");
    assert_eq!(shared(sim.guests[0].doc.model()), shared(sim.doc.model()));
}

#[test]
fn undo_during_a_gesture_leaves_the_other_persons_edit_out_of_it() {
    let mut sim = one_guest();
    let id = sim.doc.model().tracks[0].id;
    let volume = sim.doc.model().tracks[0].volume;
    let drag = Command::SetTrackParam { id, param: TrackParam::Volume, value: 0.2 };
    let out = sim.host.edit(&mut sim.doc, drag, true, None).unwrap();
    sim.route(out);
    let g = &mut sim.guests[0];
    let message = g.session.edit(&mut g.doc, Command::SetTempo { bpm: 133.0 }, false, None).unwrap().unwrap();
    g.to_host.push_back(message);
    sim.drain();
    sim.doc.commit_gesture();
    let (_, out) = sim.host.step(&mut sim.doc, true);
    sim.route(out);
    sim.drain();
    assert_eq!(sim.doc.model().tracks[0].volume, volume);
    assert_eq!(sim.doc.model().tempo, 133.0);
}

#[test]
fn listening_state_stays_with_each_person() {
    let mut sim = one_guest();
    let id = sim.doc.model().tracks[0].id;
    let g = &mut sim.guests[0];
    assert!(g.session.edit(&mut g.doc, Command::SetTrackMuted { id, muted: true }, false, None).unwrap().is_none());
    assert!(g.session.edit(&mut g.doc, Command::SetTrackSoloed { id, soloed: true }, false, None).unwrap().is_none());
    let region = Some((960, 1920));
    assert!(g.session.edit(&mut g.doc, Command::SetLoopRegion { region }, false, None).unwrap().is_none());
    sim.drain();
    assert!(!sim.doc.model().tracks[0].muted);
    assert_eq!(sim.doc.model().loop_region, None);
    // A shared edit lands on the guest and leaves its listening state alone, and time opened
    // before its loop carries the loop along.
    let out = sim.host.edit(&mut sim.doc, Command::InsertTime { at: 0, ticks: 960 }, false, None).unwrap();
    sim.route(out);
    sim.drain();
    let g = sim.guests[0].doc.model();
    assert!(g.tracks[0].muted && g.tracks[0].soloed);
    assert_eq!(g.loop_region, Some((1920, 2880)));
    assert_eq!(sim.doc.model().loop_region, None);
}

#[test]
fn a_guest_cant_bring_in_audio_the_host_will_never_have() {
    assert!(Command::AddAudioTake {
        track_id: uuid::Uuid::new_v4(),
        source: dissonant_core::AudioSource {
            id: uuid::Uuid::new_v4(),
            name: "take".into(),
            frames: 1,
            sample_rate: 48000,
            channels: 1
        },
        clip: dissonant_core::AudioClip::new(uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), 0, 1.0),
    }
    .adds_audio());
}

#[test]
fn every_message_survives_the_wire() {
    let mut note = NoteEvent::new(0, 240, 60);
    note.velocity = 77;
    let op = ToHost::Op {
        seq: 4,
        op_id: uuid::Uuid::new_v4(),
        body: dissonant_session::OpBody::Command(Command::PatchNotes {
            pattern_id: uuid::Uuid::new_v4(),
            upsert: vec![note],
            remove: vec![uuid::Uuid::new_v4()],
        }),
    };
    let wire = serde_json::to_vec(&op).unwrap();
    assert_eq!(serde_json::from_slice::<ToHost>(&wire).unwrap(), op);
    let welcome = ToGuest::Welcome { revision: 1, last_seq: 2, model: Box::new(starter()), peers: vec![] };
    let wire = serde_json::to_vec(&welcome).unwrap();
    assert_eq!(serde_json::from_slice::<ToGuest>(&wire).unwrap(), welcome);
}

/// A hello from a device, claiming to be `client`.
fn hello_as(client: uuid::Uuid) -> ToHost {
    ToHost::Hello {
        protocol: PROTOCOL_VERSION,
        schema: dissonant_core::SCHEMA_VERSION,
        client,
        name: "someone".into(),
        color: "#000".into(),
    }
}

fn welcomed(out: &HostEvents, conn: ConnId) -> bool {
    out.send.iter().any(|(c, m)| *c == conn && matches!(m, ToGuest::Welcome { .. }))
}

fn refused(out: &HostEvents, conn: ConnId) -> bool {
    out.close == Some(conn) && out.send.iter().any(|(c, m)| *c == conn && matches!(m, ToGuest::Refused { .. }))
}

#[test]
fn nobody_new_sees_the_project_until_the_host_says_yes() {
    let mut host = HostSession::new("h".into(), "#fff".into());
    let mut doc = Document::new(starter());
    let guest = GuestSession::new("g".into(), "#000".into());
    host.on_connect(1, [7; 32]);
    let out = host.on_message(&mut doc, 1, guest.hello());
    assert!(out.asked && !welcomed(&out, 1));
    assert_eq!(out.send, vec![(1, ToGuest::Waiting)]);
    assert_eq!(host.requests().len(), 1);
    // Edits made meanwhile don't reach them, and nor does anyone's cursor.
    let out = host.edit(&mut doc, Command::SetTempo { bpm: 99.0 }, false, None).unwrap();
    assert!(out.send.is_empty());
    assert!(host.others(None).is_empty());

    let out = host.answer(&doc, guest.id(), true);
    assert!(welcomed(&out, 1));
    assert_eq!(out.admit, Some([7; 32]));
    assert!(host.requests().is_empty());
    assert!(host.peers().iter().any(|p| p.id == guest.id()));
}

#[test]
fn someone_the_host_turns_down_is_sent_away_and_their_device_forgotten() {
    let mut host = HostSession::new("h".into(), "#fff".into());
    let mut doc = Document::new(starter());
    let guest = GuestSession::new("g".into(), "#000".into());
    host.on_connect(1, [7; 32]);
    host.on_message(&mut doc, 1, guest.hello());
    let out = host.answer(&doc, guest.id(), false);
    assert!(refused(&out, 1));
    assert_eq!(out.revoke, Some([7; 32]));
    assert!(host.requests().is_empty());
    assert_eq!(host.peers().len(), 1);
}

#[test]
fn someone_who_stops_waiting_drops_off_the_list() {
    let mut host = HostSession::new("h".into(), "#fff".into());
    let mut doc = Document::new(starter());
    let guest = GuestSession::new("g".into(), "#000".into());
    host.on_connect(1, [7; 32]);
    host.on_message(&mut doc, 1, guest.hello());
    let out = host.on_disconnect(1);
    assert!(out.requests_changed);
    assert!(host.requests().is_empty());
    // An answer that comes too late lets nobody in.
    let out = host.answer(&doc, guest.id(), true);
    assert!(out.send.is_empty() && out.admit.is_none());
}

#[test]
fn nobody_can_take_another_guests_place() {
    let mut sim = one_guest();
    let victim = sim.guests[0].session.id();
    sim.drop_connection(0);
    // Everyone in a session can see everyone's id, so a stranger can claim it.
    sim.host.on_connect(99, [200; 32]);
    let out = sim.host.on_message(&mut sim.doc, 99, hello_as(victim));
    assert!(refused(&out, 99));
    assert_eq!(out.revoke, Some([200; 32]));
    assert!(!sim.host.peers().iter().any(|p| p.id == victim));
    // The real one still gets back in.
    sim.reconnect(0);
    sim.drain();
    let out = sim.host.edit(&mut sim.doc, Command::SetTempo { bpm: 77.0 }, false, None).unwrap();
    sim.route(out);
    sim.drain();
    assert_eq!(sim.guests[0].doc.model().tempo, 77.0);
}

#[test]
fn closing_the_session_keeps_new_people_out_but_lets_people_already_in_back() {
    let mut sim = one_guest();
    sim.host.set_open(false);
    sim.drop_connection(0);
    sim.reconnect(0);
    sim.drain();
    let out = sim.host.edit(&mut sim.doc, Command::SetTempo { bpm: 77.0 }, false, None).unwrap();
    sim.route(out);
    sim.drain();
    assert_eq!(sim.guests[0].doc.model().tempo, 77.0);

    let stranger = GuestSession::new("s".into(), "#000".into());
    sim.host.on_connect(99, [200; 32]);
    let out = sim.host.on_message(&mut sim.doc, 99, stranger.hello());
    assert!(refused(&out, 99));
    assert!(sim.host.requests().is_empty());
}

#[test]
fn someone_removed_cant_ask_again_from_the_same_device() {
    let mut sim = one_guest();
    let (id, key) = (sim.guests[0].session.id(), sim.guests[0].key);
    let out = sim.host.remove(id);
    assert_eq!(out.revoke, Some(key));
    // A fresh session on the same device is still the same device.
    let again = GuestSession::new("guest".into(), "#000".into());
    sim.host.on_connect(99, key);
    let out = sim.host.on_message(&mut sim.doc, 99, again.hello());
    assert!(refused(&out, 99));
    assert!(sim.host.requests().is_empty());
}
