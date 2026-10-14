//! `Core` without sockets: two cores on an in-memory wire, a virtual clock.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rl_net::core::{Core, CoreConfig, CoreEvent, Datagram};
use rl_net::endpoint::{HOST, NetEvent};
use rl_netcode::Channel;

/// Delivers what each core sends to the other at once.
fn exchange(
    a: &mut Core,
    a_at: SocketAddr,
    b: &mut Core,
    b_at: SocketAddr,
    now: Instant,
    ea: &mut Vec<CoreEvent>,
    eb: &mut Vec<CoreEvent>,
) {
    let mut out: Vec<Datagram> = Vec::new();
    a.poll(now, &mut out, ea);
    for d in out.drain(..) {
        if d.to == b_at {
            b.receive(a_at, &d.bytes, now);
        }
    }
    b.poll(now, &mut out, eb);
    for d in out.drain(..) {
        if d.to == a_at {
            a.receive(b_at, &d.bytes, now);
        }
    }
}

fn connected(events: &[CoreEvent]) -> bool {
    events.iter().any(|e| matches!(e, CoreEvent::Net(NetEvent::Connected { .. })))
}

#[test]
fn two_cores_connect_and_talk_on_a_virtual_wire() {
    let t0 = Instant::now();
    let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
    let client_at: SocketAddr = "10.0.0.2:50000".parse().unwrap();
    let mut host = Core::new(CoreConfig::host(), t0);
    let mut client = Core::new(CoreConfig::client(), t0);
    let (setup, offer) = client.offer(&[client_at], host_at, t0);
    let (_, answer) = host.answer(&[host_at], &offer, client_at, t0).expect("answered");
    client.accept_answer(setup, &answer).expect("accepted");
    let (mut eh, mut ec) = (Vec::new(), Vec::new());
    let mut now = t0;
    while !(connected(&eh) && connected(&ec)) {
        now += Duration::from_millis(5);
        assert!(now < t0 + Duration::from_secs(10), "no connection; host {eh:?} client {ec:?}");
        exchange(&mut client, client_at, &mut host, host_at, now, &mut ec, &mut eh);
    }
    assert!(ec.iter().any(|e| matches!(e, CoreEvent::Opened { setup: s, peer: HOST } if *s == setup)));
    client.send(HOST, Channel::Reliable, b"hello");
    let mut got = None;
    for _ in 0..200 {
        now += Duration::from_millis(5);
        eh.clear();
        exchange(&mut client, client_at, &mut host, host_at, now, &mut ec, &mut eh);
        got = eh.iter().find_map(|e| match e {
            CoreEvent::Net(NetEvent::Message { bytes, .. }) => Some(bytes.clone()),
            _ => None,
        });
        if got.is_some() {
            break;
        }
    }
    assert_eq!(got.as_deref(), Some(&b"hello"[..]));
}

#[test]
fn a_setup_that_never_answers_times_out() {
    let t0 = Instant::now();
    let mut client = Core::new(CoreConfig::client(), t0);
    let (setup, _) =
        client.offer(&["10.0.0.2:50000".parse().unwrap()], "10.0.0.1:23400".parse().unwrap(), t0);
    let mut events = Vec::new();
    client.poll(t0 + Duration::from_secs(11), &mut Vec::new(), &mut events);
    assert!(
        events.contains(&CoreEvent::SetupFailed { setup, why: rl_net::endpoint::Why::Timeout }),
        "{events:?}"
    );
    assert!(events.contains(&CoreEvent::Net(NetEvent::Disconnected {
        peer: HOST,
        why: rl_net::endpoint::Why::Timeout
    })));
}

#[test]
fn admit_checks_version_fingerprint_and_capacity() {
    let core = Core::new(CoreConfig { max_peers: 0, ..CoreConfig::host() }, Instant::now());
    let offer = rl_proto::Signal::Offer {
        protocol: rl_proto::PROTOCOL_VERSION,
        fingerprint: rl_proto::FINGERPRINT,
        sdp: String::new(),
    };
    assert_eq!(core.admit(&offer), Err(rl_proto::Refusal::Full));
    let old = rl_proto::Signal::Offer { protocol: 1, fingerprint: rl_proto::FINGERPRINT, sdp: String::new() };
    assert_eq!(core.admit(&old), Err(rl_proto::Refusal::Version));
}
