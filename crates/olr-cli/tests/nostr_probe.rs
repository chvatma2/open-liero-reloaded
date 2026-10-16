//! The relay and STUN probes' verdicts against the in-process test relay and
//! STUN responder. The real probe runs once by hand; no test touches the
//! public internet.

use std::net::{TcpListener, UdpSocket};
use std::time::Duration;

use olr_cli::nostr_probe::{
    PROBE_CONTENT, PROBE_EXPIRY, PROBE_TAG, ProbeConfig, RelayReport, StunReport, needs_payment_or_auth,
    probe_event, probe_handshake, probe_key, probe_relay, probe_stun, relay_table, stun_table,
};
use olr_discovery::handshake::MAX_HANDSHAKE_CONTENT;
use olr_discovery::{KIND_ANNOUNCE, KIND_HANDSHAKE, KIND_TURN, Misbehaviour, StunResponder, TestRelay};

const T: u64 = 1_800_000_000;

fn cfg() -> ProbeConfig {
    ProbeConfig { step: Duration::from_secs(3), extra_roots: Vec::new() }
}

/// Probes `relay` at time `T`; `during_wait` runs in place of the wait
/// between the two reads.
fn probe(relay: &TestRelay, during_wait: impl FnOnce()) -> RelayReport {
    relay.set_time(T);
    let key = probe_key().expect("a random key");
    let mut once = Some(during_wait);
    probe_relay(&relay.url(), &key, T, &cfg(), &mut || {
        if let Some(f) = once.take() {
            f();
        }
    })
}

#[test]
fn an_honest_relay_passes() {
    let relay = TestRelay::start();
    let r = probe(&relay, || {});
    assert!(r.connected(), "{r:?}");
    assert!(r.read_now && r.read_later && r.turn_read_now && r.turn_read_later, "{r:?}");
    assert!(r.accepts() && r.keeps() && r.accepts_turn() && r.keeps_turn(), "{r:?}");
    assert!(r.forwards() && r.free(), "{r:?}");
    assert!(r.passes());
    assert!(r.connect_ms.is_some() && r.ok_ms.is_some());
    // Exactly two stored notes: the probe notes, nothing else.
    let stored = relay.events();
    assert_eq!(stored.len(), 2, "{stored:?}");
    for (e, kind) in stored.iter().zip([KIND_ANNOUNCE, KIND_TURN]) {
        assert_eq!((e.kind, e.content.as_str(), e.created_at), (kind, PROBE_CONTENT, T));
        assert_eq!(e.tag("t"), Some(PROBE_TAG));
        assert_eq!(e.tag("d"), Some(PROBE_TAG), "never a TURN list's tag");
        assert_eq!(e.expiration(), Some(T + PROBE_EXPIRY));
    }
}

#[test]
fn a_paid_relay_fails_as_not_free() {
    let relay = TestRelay::start();
    relay.set_misbehaviour(Misbehaviour::Refuse("restricted: you must pay to publish here"));
    let r = probe(&relay, || {});
    assert!(r.connected());
    assert!(!r.accepts() && !r.free() && !r.forwards() && !r.keeps() && !r.passes(), "{r:?}");
    assert!(relay.events().is_empty());

    relay.set_misbehaviour(Misbehaviour::Refuse("auth-required: we only accept events from members"));
    let r = probe(&relay, || {});
    assert!(!r.free() && !r.passes(), "{r:?}");
}

#[test]
fn a_relay_refusing_the_kind_does_not_accept() {
    let relay = TestRelay::start();
    relay.set_misbehaviour(Misbehaviour::RefuseKind(KIND_ANNOUNCE));
    let r = probe(&relay, || {});
    assert!(!r.accepts() && !r.keeps() && !r.passes(), "{r:?}");
    assert!(r.accepts_turn() && r.keeps_turn(), "{r:?}");
    assert!(r.free() && r.forwards(), "the ephemeral kind still goes through: {r:?}");

    relay.set_misbehaviour(Misbehaviour::RefuseKind(KIND_TURN));
    let r = probe(&relay, || {});
    assert!(r.accepts() && !r.accepts_turn() && !r.keeps_turn() && !r.passes(), "{r:?}");
    let table = relay_table(&[r]);
    assert!(table.contains("TURN note OK: blocked") && table.contains("FAIL"), "{table}");
}

#[test]
fn a_relay_dropping_notes_does_not_keep() {
    let relay = TestRelay::start();
    relay.set_misbehaviour(Misbehaviour::KeepFor(60));
    // The wait: 200 s pass on the relay's clock.
    let r = probe(&relay, || relay.set_time(T + 200));
    assert!(r.accepts() && r.read_now && !r.read_later, "{r:?}");
    assert!(!r.keeps() && !r.passes());
    assert!(r.forwards() && r.free());
}

#[test]
fn a_relay_not_forwarding_ephemeral_events_fails() {
    let relay = TestRelay::start();
    relay.set_misbehaviour(Misbehaviour::NoForward);
    let r = probe(&relay, || {});
    assert!(r.accepts() && r.keeps() && r.free(), "{r:?}");
    assert!(!r.forwards() && !r.passes());
}

#[test]
fn an_unreachable_relay_fails() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let key = probe_key().unwrap();
    let r = probe_relay(&format!("ws://127.0.0.1:{port}"), &key, T, &cfg(), &mut || {});
    assert!(!r.connected() && r.failure.is_some(), "{r:?}");
    assert!(!r.accepts() && !r.free() && !r.passes());
    let table = relay_table(&[r]);
    assert!(table.contains("FAIL"), "{table}");
}

#[test]
fn probe_events_are_marked_and_expire() {
    let key = probe_key().unwrap();
    for kind in [KIND_ANNOUNCE, KIND_HANDSHAKE] {
        let e = probe_event(&key, kind, T);
        assert!(e.verify());
        assert_eq!((e.kind, e.content.as_str(), e.pubkey), (kind, PROBE_CONTENT, key.public()));
        assert_eq!(e.tag("t"), Some(PROBE_TAG), "never the game's own tag");
        assert_eq!(e.tag("d"), Some(PROBE_TAG));
        assert_eq!(e.expiration(), Some(T + 600));
        assert_eq!(e.tags.len(), 3);
    }
    assert_ne!(probe_key().unwrap().public(), key.public(), "a fresh key per run");
    // The ephemeral one carries the largest handshake: 16 KiB of random base64.
    let e = probe_handshake(&key, T);
    assert!(e.verify());
    assert_eq!((e.kind, e.content.len()), (KIND_HANDSHAKE, MAX_HANDSHAKE_CONTENT));
    assert!(e.content.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/'));
    assert_eq!(e.tag("t"), Some(PROBE_TAG));
    assert_eq!(e.expiration(), Some(T + 600));
    assert_ne!(e.content, probe_handshake(&key, T).content, "random");
}

#[test]
fn refusals_for_payment_or_auth_are_recognised() {
    for m in
        ["restricted: not paid", "auth-required: log in", "blocked: pay at https://x", "Payment required"]
    {
        assert!(needs_payment_or_auth(m), "{m}");
    }
    for m in ["", "blocked: kind 31470 not accepted", "invalid: bad signature", "rate-limited: slow down"] {
        assert!(!needs_payment_or_auth(m), "{m}");
    }
}

#[test]
fn stun_probe_measures_an_answer() {
    let responder = StunResponder::start("198.51.100.4:4444".parse().unwrap());
    let r = probe_stun(&responder.server(), Duration::from_millis(500));
    assert!(r.answered(), "{r:?}");
    assert_eq!(r.family, Some("IPv4"));
    assert!(r.rtt_ms.is_some());
    assert_eq!(responder.answered(), 1);

    let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
    let r: StunReport = probe_stun(&silent.local_addr().unwrap().to_string(), Duration::from_millis(100));
    assert!(!r.answered() && r.rtt_ms.is_none(), "{r:?}");
    let r = probe_stun("not a server", Duration::from_millis(100));
    assert!(!r.answered() && r.error.is_some(), "{r:?}");

    let table = stun_table(&[r]);
    assert!(table.contains("not a server") && table.contains("FAIL"), "{table}");
}
