//! `Core` without sockets: two cores on an in-memory wire, a virtual clock.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use rl_net::core::{Core, CoreConfig, CoreEvent, Datagram};
use rl_net::endpoint::{HOST, NetEvent, Why};
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
    assert!(events.contains(&CoreEvent::SetupFailed { setup, why: Why::Timeout }), "{events:?}");
    assert!(events.contains(&CoreEvent::Net(NetEvent::Disconnected { peer: HOST, why: Why::Timeout })));
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

/// Two cores with the client's offer answered and taken.
fn set_up(t0: Instant, client_at: SocketAddr, host_at: SocketAddr) -> (Core, Core, u64) {
    let mut host = Core::new(CoreConfig::host(), t0);
    let mut client = Core::new(CoreConfig::client(), t0);
    let (setup, offer) = client.offer(&[client_at], host_at, t0);
    let (_, answer) = host.answer(&[host_at], &offer, client_at, t0).expect("answered");
    client.accept_answer(setup, &answer).expect("accepted");
    (client, host, setup)
}

/// The stats count what the caller reports sent (`Core::sent`), the
/// handshake's datagrams too, from before the link had a peer id.
#[test]
fn datagrams_sent_while_connecting_count_in_the_stats() {
    let t0 = Instant::now();
    let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
    let client_at: SocketAddr = "10.0.0.2:50000".parse().unwrap();
    let (mut client, mut host, _) = set_up(t0, client_at, host_at);
    let (mut eh, mut ec) = (Vec::new(), Vec::new());
    let (mut sent, mut bytes, mut refused, mut before_open) = (0u64, 0u64, 0u64, 0u64);
    let mut out = Vec::new();
    let mut now = t0;
    while !(connected(&eh) && connected(&ec)) {
        now += Duration::from_millis(5);
        assert!(now < t0 + Duration::from_secs(10), "no connection; host {eh:?} client {ec:?}");
        client.poll(now, &mut out, &mut ec);
        for d in out.drain(..) {
            // The socket refuses the client's first datagram (ICE sends
            // its check again).
            let ok = sent + refused > 0;
            client.sent(&d, ok);
            if !ok {
                refused += 1;
                continue;
            }
            sent += 1;
            bytes += d.bytes.len() as u64;
            if !connected(&ec) {
                before_open += 1;
            }
            host.receive(client_at, &d.bytes, now);
        }
        host.poll(now, &mut out, &mut eh);
        for d in out.drain(..) {
            host.sent(&d, true);
            client.receive(host_at, &d.bytes, now);
        }
    }
    assert!(before_open > 0);
    let stats = client.stats(HOST).expect("open");
    assert_eq!((stats.datagrams_sent, stats.bytes_sent, stats.send_failures), (sent, bytes, refused));
    assert_eq!(refused, 1);
}

/// `cancel` drops a connecting setup at once (a client hears why, no
/// `SetupFailed` follows) and its link's last datagrams leave with the next
/// poll: the host, already open, learns the link is closed.
#[test]
fn a_cancelled_setup_tells_the_peer_with_the_next_poll() {
    let t0 = Instant::now();
    let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
    let client_at: SocketAddr = "10.0.0.2:50000".parse().unwrap();
    let (mut client, mut host, setup) = set_up(t0, client_at, host_at);
    let (mut eh, mut ec) = (Vec::new(), Vec::new());
    let mut now = t0;
    // The host opens first: in each exchange it polls after the client.
    while !connected(&eh) {
        now += Duration::from_millis(5);
        assert!(now < t0 + Duration::from_secs(10), "no connection; host {eh:?}");
        exchange(&mut client, client_at, &mut host, host_at, now, &mut ec, &mut eh);
    }
    assert!(!connected(&ec) && client.pending() == 1, "{ec:?}");
    let mut events = Vec::new();
    client.cancel(setup, Why::Closed, &mut events);
    assert_eq!(events, [CoreEvent::Net(NetEvent::Disconnected { peer: HOST, why: Why::Closed })]);
    assert_eq!(client.pending(), 0);
    let mut out = Vec::new();
    client.poll(now, &mut out, &mut events);
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(!out.is_empty() && out.iter().all(|d| d.to == host_at), "{out:?}");
    for d in out.drain(..) {
        host.receive(client_at, &d.bytes, now);
    }
    client.poll(now + Duration::from_millis(5), &mut out, &mut events);
    assert!(out.is_empty(), "the outbox goes once: {out:?}");
    eh.clear();
    host.poll(now, &mut Vec::new(), &mut eh);
    assert!(eh.contains(&CoreEvent::Net(NetEvent::Disconnected { peer: 1, why: Why::Closed })), "{eh:?}");
}

/// A signalled offer from a client core at `at` to the host core.
fn signalled_offer(
    client: &mut Core,
    at: SocketAddr,
    host_at: SocketAddr,
    now: Instant,
) -> (u64, rl_proto::Signal) {
    client.set_base(at, vec![at]);
    let (setup, sdp) = client.offer_ice(host_at, now);
    let offer = rl_proto::Signal::Offer {
        protocol: rl_proto::PROTOCOL_VERSION,
        fingerprint: rl_proto::FINGERPRINT,
        sdp,
    };
    (setup, offer)
}

/// PF12b-2: with `MAX_SIGNALLED_PENDING` signalled setups pending, a new
/// offer evicts the oldest one whose peer sent no connectivity check; only
/// when every one has is it refused `Busy`.
#[test]
fn a_full_pending_table_evicts_a_silent_setup() {
    use rl_net::core::MAX_SIGNALLED_PENDING;
    use rl_proto::{Refusal, Signal};
    let t0 = Instant::now();
    let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
    let mut host = Core::new(CoreConfig::host(), t0);
    host.set_base(host_at, vec![host_at]);
    // Eight silent offerers: answered, never heard from again.
    for i in 0..MAX_SIGNALLED_PENDING {
        let at: SocketAddr = format!("10.0.1.{}:50000", i + 1).parse().unwrap();
        let mut spare = Core::new(CoreConfig::client(), t0);
        let (_, offer) = signalled_offer(&mut spare, at, host_at, t0);
        assert!(matches!(host.answer_ice(&offer, None, t0), Signal::Answer { .. }));
    }
    assert_eq!(host.pending(), MAX_SIGNALLED_PENDING);
    // A ninth, live, takes a silent one's place and connects.
    let client_at: SocketAddr = "10.0.2.1:50000".parse().unwrap();
    let mut client = Core::new(CoreConfig::client(), t0);
    let (setup, offer) = signalled_offer(&mut client, client_at, host_at, t0);
    let Signal::Answer { sdp } = host.answer_ice(&offer, None, t0) else { panic!("refused") };
    client.accept_answer_ice(setup, &sdp, None).expect("accepted");
    assert_eq!(host.pending(), MAX_SIGNALLED_PENDING);
    let (mut eh, mut ec) = (Vec::new(), Vec::new());
    let mut now = t0;
    while !(connected(&eh) && connected(&ec)) {
        now += Duration::from_millis(5);
        assert!(now < t0 + Duration::from_secs(10), "no connection; host {eh:?} client {ec:?}");
        exchange(&mut client, client_at, &mut host, host_at, now, &mut ec, &mut eh);
    }
    assert_eq!(eh.iter().filter(|e| matches!(e, CoreEvent::SetupFailed { .. })).count(), 1, "{eh:?}");
    assert_eq!(host.pending(), MAX_SIGNALLED_PENDING - 1);
    // Eight live offerers whose checks reach the host (its answers never
    // reach them) fill the table, evicting the seven silent setups left.
    let mut live = Vec::new();
    for i in 0..MAX_SIGNALLED_PENDING {
        let at: SocketAddr = format!("10.0.3.{}:50000", i + 1).parse().unwrap();
        let mut c = Core::new(CoreConfig::client(), now);
        let (setup, offer) = signalled_offer(&mut c, at, host_at, now);
        let Signal::Answer { sdp } = host.answer_ice(&offer, None, now) else { panic!("refused {i}") };
        c.accept_answer_ice(setup, &sdp, None).expect("accepted");
        live.push((c, at));
    }
    assert_eq!(host.pending(), MAX_SIGNALLED_PENDING);
    let end = now + Duration::from_secs(1);
    let mut out = Vec::new();
    while now < end {
        now += Duration::from_millis(5);
        for (c, at) in &mut live {
            c.poll(now, &mut out, &mut Vec::new());
            for d in out.drain(..) {
                if d.to == host_at {
                    host.receive(*at, &d.bytes, now);
                }
            }
        }
        host.poll(now, &mut out, &mut Vec::new());
        out.clear();
    }
    assert_eq!(host.pending(), MAX_SIGNALLED_PENDING);
    // Every pending setup has heard a check: a tenth is refused.
    let mut late = Core::new(CoreConfig::client(), now);
    let (_, offer) = signalled_offer(&mut late, "10.0.4.1:50000".parse().unwrap(), host_at, now);
    assert_eq!(host.answer_ice(&offer, None, now), Signal::Refused { reason: Refusal::Busy });
}
