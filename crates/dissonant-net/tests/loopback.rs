//! Real sockets, real encryption, on this machine.

use dissonant_net::{format_code, new_code, Net, NetEvent, Reach, HOST};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId};
use iroh_mdns_address_lookup::MdnsAddressLookup;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

fn next(rx: &Receiver<NetEvent>) -> NetEvent {
    rx.recv_timeout(Duration::from_secs(40)).expect("nothing arrived")
}

fn hosting() -> (Net, Receiver<NetEvent>, String) {
    let (tx, rx) = channel();
    let host = Net::host(Reach::Local, tx).unwrap();
    let code = new_code();
    host.open_lobby(&code);
    (host, rx, code)
}

/// A host and a guest that came in with a code, typed the way a person might.
fn pair() -> (Net, Receiver<NetEvent>, Net, Receiver<NetEvent>, u64) {
    let (host, host_rx, code) = hosting();
    let (guest_tx, guest_rx) = channel();
    let guest = Net::join(Reach::Local, &format!(" {} ", format_code(&code).to_lowercase()), guest_tx).unwrap();
    let NetEvent::Connected(HOST, host_side) = next(&guest_rx) else { panic!("the guest never reached the host") };
    assert_eq!(host_side.key, host.key());
    // A QUIC stream only exists at the far end once something is written to it.
    guest.send(HOST, b"hello");
    let mut knocked = false;
    let (conn, guest_side) = loop {
        match next(&host_rx) {
            NetEvent::Knocked(key) => knocked = key == guest.key(),
            NetEvent::Connected(conn, peer) => break (conn, peer),
            other => panic!("unexpected {other:?}"),
        }
    };
    assert_eq!(guest_side.key, guest.key());
    assert_eq!(guest_side.words, host_side.words, "both ends read the same words");
    if !knocked {
        assert_eq!(next(&host_rx), NetEvent::Knocked(guest.key()));
    }
    assert_eq!(next(&host_rx), NetEvent::Message(conn, b"hello".to_vec()));
    (host, host_rx, guest, guest_rx, conn)
}

#[test]
fn messages_arrive_in_order_both_ways() {
    let (host, host_rx, guest, guest_rx, conn) = pair();
    for i in 0..200u32 {
        guest.send(HOST, &i.to_be_bytes());
        host.send(conn, &(i + 1000).to_be_bytes());
    }
    for i in 0..200u32 {
        assert_eq!(next(&host_rx), NetEvent::Message(conn, i.to_be_bytes().to_vec()));
        assert_eq!(next(&guest_rx), NetEvent::Message(HOST, (i + 1000).to_be_bytes().to_vec()));
    }
    host.shutdown();
    guest.shutdown();
}

#[test]
fn a_large_project_gets_through() {
    let (host, _host_rx, guest, guest_rx, conn) = pair();
    let big = vec![7u8; 8 * 1024 * 1024];
    host.send(conn, &big);
    assert_eq!(next(&guest_rx), NetEvent::Message(HOST, big));
    host.shutdown();
    guest.shutdown();
}

#[test]
fn datagrams_get_through() {
    let (host, host_rx, guest, _guest_rx, conn) = pair();
    // Unreliable by design, so one lost packet must not fail the test.
    for _ in 0..20 {
        guest.datagram(HOST, b"cursor");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(next(&host_rx), NetEvent::Datagram(conn, b"cursor".to_vec()));
    host.shutdown();
    guest.shutdown();
}

#[test]
fn a_closed_connection_delivers_what_was_queued_first() {
    let (host, _host_rx, guest, guest_rx, conn) = pair();
    host.send(conn, b"session over");
    host.close(conn);
    assert_eq!(next(&guest_rx), NetEvent::Message(HOST, b"session over".to_vec()));
    host.shutdown();
    guest.shutdown();
}

#[test]
fn a_code_lets_one_device_through_once() {
    let (host, host_rx, code) = hosting();
    let (tx, first_rx) = channel();
    let first = Net::join(Reach::Local, &code, tx).unwrap();
    assert!(matches!(next(&first_rx), NetEvent::Connected(HOST, _)));
    assert_eq!(next(&host_rx), NetEvent::Knocked(first.key()));

    let (tx, second_rx) = channel();
    let second = Net::join(Reach::Local, &code, tx).unwrap();
    assert!(matches!(next(&second_rx), NetEvent::GaveUp(_)), "a spent code found the host");
    first.shutdown();
    second.shutdown();
    host.shutdown();
}

#[test]
fn a_device_that_found_the_host_without_a_code_is_turned_away() {
    let (host, host_rx, _code) = hosting();
    let host_id = EndpointId::from_bytes(&host.key()).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let reason = runtime.block_on(async {
        // Everything a stranger on the same network can learn: the host's key, from mDNS.
        let stranger = Endpoint::builder(presets::Minimal)
            .address_lookup(MdnsAddressLookup::builder().service_name("dissonant"))
            .bind()
            .await
            .unwrap();
        let conn = stranger.connect(EndpointAddr::from(host_id), b"dissonant/session/2").await.unwrap();
        let reason = conn.closed().await;
        stranger.close().await;
        reason
    });
    assert!(format!("{reason:?}").contains("not invited"), "{reason:?}");
    assert!(host_rx.recv_timeout(Duration::from_secs(2)).is_err(), "the host heard from a stranger");
    host.shutdown();
}

#[test]
fn something_that_isnt_a_code_fails_at_once() {
    let (tx, _rx) = channel();
    assert!(Net::join(Reach::Local, "dissonant-eyJhZGRyIjp7fX0", tx).is_err());
}

/// Takes about two minutes: a guest only learns a crashed host is gone by hearing nothing. Run it
/// with `cargo test -p dissonant-net --test loopback -- --ignored --nocapture`.
#[test]
#[ignore]
fn a_guest_gives_up_on_a_host_that_vanished() {
    let (host, _host_rx, guest, guest_rx, _conn) = pair();
    let started = std::time::Instant::now();
    // No goodbye: this is what a crash looks like from the far end.
    drop(host);
    let mut lost = None;
    loop {
        match guest_rx.recv_timeout(Duration::from_secs(180)).expect("the guest never gave up") {
            NetEvent::Disconnected(HOST) => lost = Some(started.elapsed()),
            NetEvent::GaveUp(why) => {
                println!("lost after {lost:?}, gave up after {:?}: {why}", started.elapsed());
                assert!(lost.is_some(), "it noticed the loss first");
                break;
            }
            _ => {}
        }
    }
    guest.shutdown();
}
