//! `Core` without sockets: two cores on an in-memory wire, a virtual clock.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use olr_net::core::{Core, CoreConfig, CoreEvent, Datagram};
use olr_net::endpoint::{HOST, NetEvent, Why};
use olr_netcode::Channel;

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
    let offer = olr_proto::Signal::Offer {
        protocol: olr_proto::PROTOCOL_VERSION,
        fingerprint: olr_proto::FINGERPRINT,
        sdp: String::new(),
    };
    assert_eq!(core.admit(&offer), Err(olr_proto::Refusal::Full));
    let old =
        olr_proto::Signal::Offer { protocol: 1, fingerprint: olr_proto::FINGERPRINT, sdp: String::new() };
    assert_eq!(core.admit(&old), Err(olr_proto::Refusal::Version));
}

/// Plan 13c: a 0.1 client (protocol 2) offering to a protocol-3 host is
/// refused at the handshake, `Version` (the game shows "the host plays
/// another version"), whether or not its fingerprint happens to match; the
/// host builds no link for it.
#[test]
fn a_protocol_2_client_is_refused_at_the_handshake() {
    assert_eq!(olr_proto::PROTOCOL_VERSION, 3, "protocol 3 is 0.2's");
    let core = Core::new(CoreConfig::host(), Instant::now());
    for fingerprint in [olr_proto::FINGERPRINT, olr_proto::FINGERPRINT ^ 1] {
        let old = olr_proto::Signal::Offer { protocol: 2, fingerprint, sdp: String::new() };
        assert_eq!(core.admit(&old), Err(olr_proto::Refusal::Version), "{fingerprint}");
    }
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
) -> (u64, olr_proto::Signal) {
    client.set_base(at, vec![at]);
    let (setup, sdp) = client.offer_ice(host_at, now);
    let offer = olr_proto::Signal::Offer {
        protocol: olr_proto::PROTOCOL_VERSION,
        fingerprint: olr_proto::FINGERPRINT,
        sdp,
    };
    (setup, offer)
}

/// PF12b-2 and FR12b-1: with `MAX_SIGNALLED_PENDING` signalled setups
/// pending, a new offer evicts the oldest one that no check of ours has
/// been answered by: a silent one, or one whose peer's checks come from
/// forged sources (a flooder holds its own answer's credentials, so its
/// checks are valid, but it cannot answer ours from where it forged them).
#[test]
fn a_full_pending_table_evicts_a_silent_setup() {
    use olr_net::core::MAX_SIGNALLED_PENDING;
    use olr_proto::Signal;
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
    // Eight flood offers whose valid checks reach the host from forged
    // sources (the host's checks and answers never reach them) fill the
    // table, evicting the seven silent setups left.
    let mut flood = Vec::new();
    for i in 0..MAX_SIGNALLED_PENDING {
        let at: SocketAddr = format!("10.0.3.{}:50000", i + 1).parse().unwrap();
        let mut c = Core::new(CoreConfig::client(), now);
        let (setup, offer) = signalled_offer(&mut c, at, host_at, now);
        let Signal::Answer { sdp } = host.answer_ice(&offer, None, now) else { panic!("refused {i}") };
        c.accept_answer_ice(setup, &sdp, None).expect("accepted");
        flood.push(c);
    }
    assert_eq!(host.pending(), MAX_SIGNALLED_PENDING);
    let end = now + Duration::from_secs(1);
    let (mut out, mut forged) = (Vec::new(), 0u32);
    while now < end {
        now += Duration::from_millis(5);
        for c in &mut flood {
            c.poll(now, &mut out, &mut Vec::new());
            for d in out.drain(..) {
                if d.to == host_at {
                    forged += 1;
                    let from = SocketAddr::from(([10, 0, 9, (forged % 250) as u8 + 1], 40000));
                    host.receive(from, &d.bytes, now);
                }
            }
        }
        host.poll(now, &mut out, &mut Vec::new());
        out.clear();
    }
    assert_eq!(host.pending(), MAX_SIGNALLED_PENDING);
    // No check of the host's was answered: a tenth offer is still answered,
    // evicting the oldest flood setup.
    let mut late = Core::new(CoreConfig::client(), now);
    let (_, offer) = signalled_offer(&mut late, "10.0.4.1:50000".parse().unwrap(), host_at, now);
    assert!(matches!(host.answer_ice(&offer, None, now), Signal::Answer { .. }), "a flood locked the host");
    assert_eq!(host.pending(), MAX_SIGNALLED_PENDING);
}

/// Fills a host's pending table with `MAX_SIGNALLED_PENDING` setups whose
/// STUN checks go both ways (DTLS is dropped, so none opens), from `at(i)`;
/// then a ninth offer's reply.
fn answered_table(at: impl Fn(usize) -> SocketAddr) -> (olr_proto::Signal, Vec<CoreEvent>) {
    use olr_net::core::MAX_SIGNALLED_PENDING;
    use olr_proto::Signal;
    use olr_proto::stun::is_stun;
    let t0 = Instant::now();
    let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
    let mut host = Core::new(CoreConfig::host(), t0);
    host.set_base(host_at, vec![host_at]);
    let mut joiners = Vec::new();
    for i in 0..MAX_SIGNALLED_PENDING {
        let mut c = Core::new(CoreConfig::client(), t0);
        let (setup, offer) = signalled_offer(&mut c, at(i), host_at, t0);
        let Signal::Answer { sdp } = host.answer_ice(&offer, None, t0) else { panic!("refused {i}") };
        c.accept_answer_ice(setup, &sdp, None).expect("accepted");
        joiners.push((c, at(i)));
    }
    let (mut now, mut out, mut events) = (t0, Vec::new(), Vec::new());
    while now < t0 + Duration::from_secs(1) {
        now += Duration::from_millis(5);
        for (c, c_at) in &mut joiners {
            c.poll(now, &mut out, &mut Vec::new());
            for d in out.drain(..).filter(|d| d.to == host_at && is_stun(&d.bytes)) {
                host.receive(*c_at, &d.bytes, now);
            }
        }
        host.poll(now, &mut out, &mut events);
        for d in out.drain(..).filter(|d| is_stun(&d.bytes)) {
            if let Some((c, _)) = joiners.iter_mut().find(|(_, c_at)| *c_at == d.to) {
                c.receive(host_at, &d.bytes, now);
            }
        }
    }
    assert_eq!(host.pending(), MAX_SIGNALLED_PENDING, "{events:?}");
    let mut late = Core::new(CoreConfig::client(), now);
    let (_, offer) = signalled_offer(&mut late, "10.0.4.1:50000".parse().unwrap(), host_at, now);
    let reply = host.answer_ice(&offer, None, now);
    host.poll(now, &mut out, &mut events);
    (reply, events)
}

/// FR12b-1: a setup whose peer answered one of our checks is protected
/// from eviction (it receives where it says), so a full table of them
/// refuses a newcomer `Busy`; but at most 2 per source IP are, so one
/// real IP cannot hold the table.
#[test]
fn answered_checks_protect_at_most_two_setups_per_ip() {
    use olr_proto::{Refusal, Signal};
    let (reply, events) = answered_table(|i| format!("10.0.3.{}:50000", i + 1).parse().unwrap());
    assert_eq!(reply, Signal::Refused { reason: Refusal::Busy }, "{events:?}");
    let (reply, events) = answered_table(|i| format!("10.0.3.1:{}", 50000 + i).parse().unwrap());
    assert!(matches!(reply, Signal::Answer { .. }), "one IP held the table: {reply:?}");
    assert_eq!(events.iter().filter(|e| matches!(e, CoreEvent::SetupFailed { .. })).count(), 1, "{events:?}");
}

/// Final review A-M4: without `allow_local`, a signalled offer's (or
/// answer's) loopback candidates are dropped, so a hostile joiner cannot aim
/// the host's checks at the host's own 127.0.0.1 services; with it (tests,
/// natsim) they stay.
#[test]
fn loopback_candidates_need_allow_local() {
    use olr_proto::Signal;
    let checks_to = |allow_local: bool| {
        let t0 = Instant::now();
        let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
        let mut host = Core::new(CoreConfig { allow_local, ..CoreConfig::host() }, t0);
        host.set_base(host_at, vec![host_at]);
        let target: SocketAddr = "127.0.0.1:631".parse().unwrap();
        let mut joiner = Core::new(CoreConfig::client(), t0);
        let (_, offer) = signalled_offer(&mut joiner, target, host_at, t0);
        assert!(matches!(host.answer_ice(&offer, None, t0), Signal::Answer { .. }));
        let mut out = Vec::new();
        for ms in (0..2000).step_by(10) {
            host.poll(t0 + Duration::from_millis(ms), &mut out, &mut Vec::new());
        }
        out.iter().filter(|d| d.to == target).count()
    };
    assert_eq!(checks_to(false), 0, "no check aimed at the host's own loopback");
    assert!(checks_to(true) > 0, "allow_local keeps loopback candidates");
}

/// A core with 10 host addresses that STUN answered and a router mapped.
fn crowded(cfg: CoreConfig, t0: Instant) -> (Core, SocketAddr, SocketAddr, SocketAddr) {
    let base: SocketAddr = "192.168.1.10:23400".parse().unwrap();
    let server: SocketAddr = "198.51.100.1:3478".parse().unwrap();
    let srflx: SocketAddr = "203.0.113.10:40000".parse().unwrap();
    let mapped: SocketAddr = "203.0.113.10:23400".parse().unwrap();
    let mut core = Core::new(cfg, t0);
    let hosts = (0..10).map(|i| format!("10.9.{i}.1:23400").parse().unwrap()).collect();
    core.set_base(base, hosts);
    core.stun(vec![server], t0);
    let mut out = Vec::new();
    core.poll(t0, &mut out, &mut Vec::new());
    let request = out.iter().find(|d| d.to == server).expect("a binding request");
    let transaction: [u8; 12] = request.bytes[8..20].try_into().unwrap();
    core.receive(server, &olr_proto::stun::binding_response(&transaction, srflx, true), t0);
    assert_eq!(core.local().srflx, Some(srflx));
    core.set_mapped(Some(mapped), false);
    (core, base, srflx, mapped)
}

/// The far side's `sanitize` keeps 8 candidates: an offer or answer from
/// a machine with many interfaces still carries its server-reflexive and
/// mapped candidates (review I-1), and the base among its hosts.
#[test]
fn reflexive_candidates_survive_many_host_addresses() {
    use olr_net::candidates::{MAX_CANDIDATES, remote_candidates, sanitize};
    let t0 = Instant::now();
    let check = |sdp: &str, base, srflx, mapped| {
        let kept = sanitize(sdp);
        let r = remote_candidates(&kept, None);
        assert!(r.srflx.contains(&srflx) && r.srflx.contains(&mapped), "{sdp}");
        assert!(r.host.contains(&base), "{sdp}");
        assert_eq!(r.host.len() + r.srflx.len() + r.relay.len(), MAX_CANDIDATES);
    };
    let (mut client, base, srflx, mapped) = crowded(CoreConfig::client(), t0);
    let host_at: SocketAddr = "198.51.100.9:23400".parse().unwrap();
    let (_, offer) = client.offer_ice(host_at, t0);
    check(&offer, base, srflx, mapped);
    let (mut host, base, srflx, mapped) = crowded(CoreConfig::host(), t0);
    let mut joiner = Core::new(CoreConfig::client(), t0);
    let (_, offer) = signalled_offer(&mut joiner, "10.0.2.1:50000".parse().unwrap(), host_at, t0);
    let olr_proto::Signal::Answer { sdp } = host.answer_ice(&offer, None, t0) else { panic!("refused") };
    check(&sdp, base, srflx, mapped);
}

/// The first `Connected` event in `events`, as (peer, addr, path).
fn connected_event(events: &[CoreEvent]) -> Option<(u32, SocketAddr, olr_proto::Path)> {
    events.iter().find_map(|e| match e {
        CoreEvent::Net(NetEvent::Connected { peer, addr, path }) => Some((*peer, *addr, *path)),
        _ => None,
    })
}

/// Without str0m's stats (a test hook), a signalled link reports
/// `Connected` 1.5 s after it opened, `HolePunched`, with the address its
/// datagrams came from (never 0.0.0.0, which the lobby's IP bans key
/// on); a TCP-signalled link reports `Direct` at once.
#[test]
fn without_stats_connected_comes_after_the_wait_with_a_real_address() {
    use olr_proto::{Path, Signal};
    let t0 = Instant::now();
    let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
    let client_at: SocketAddr = "10.0.0.2:50000".parse().unwrap();
    let quiet = |cfg: CoreConfig| CoreConfig { suppress_stats: true, ..cfg };
    // Signalled.
    let mut host = Core::new(quiet(CoreConfig::host()), t0);
    host.set_base(host_at, vec![host_at]);
    let mut client = Core::new(quiet(CoreConfig::client()), t0);
    let (setup, offer) = signalled_offer(&mut client, client_at, host_at, t0);
    let Signal::Answer { sdp } = host.answer_ice(&offer, None, t0) else { panic!("refused") };
    client.accept_answer_ice(setup, &sdp, None).expect("accepted");
    let (mut eh, mut ec) = (Vec::new(), Vec::new());
    let (mut opened, mut now) = (None, t0);
    while !(connected(&eh) && connected(&ec)) {
        now += Duration::from_millis(5);
        assert!(now < t0 + Duration::from_secs(10), "no connection; host {eh:?} client {ec:?}");
        exchange(&mut client, client_at, &mut host, host_at, now, &mut ec, &mut eh);
        if opened.is_none() && eh.iter().any(|e| matches!(e, CoreEvent::Opened { .. })) {
            opened = Some(now);
            assert!(!connected(&eh), "Connected waits for the path");
        }
    }
    let waited = now - opened.expect("opened");
    assert!(waited >= Duration::from_millis(1490), "{waited:?}");
    assert_eq!(connected_event(&eh), Some((1, client_at, Path::HolePunched)));
    assert_eq!(connected_event(&ec), Some((HOST, host_at, Path::HolePunched)));
    assert_eq!(host.path(1), Some(Path::HolePunched));
    assert_eq!(host.peers().collect::<Vec<_>>(), [(1, client_at)]);
    // TCP-signalled.
    let mut host = Core::new(quiet(CoreConfig::host()), t0);
    let mut client = Core::new(quiet(CoreConfig::client()), t0);
    let (setup, offer) = client.offer(&[client_at], host_at, t0);
    let (_, answer) = host.answer(&[host_at], &offer, client_at, t0).expect("answered");
    client.accept_answer(setup, &answer).expect("accepted");
    let (mut eh, mut ec) = (Vec::new(), Vec::new());
    let mut now = t0;
    while !connected(&eh) {
        now += Duration::from_millis(5);
        assert!(now < t0 + Duration::from_secs(10), "no connection; host {eh:?}");
        exchange(&mut client, client_at, &mut host, host_at, now, &mut ec, &mut eh);
        if eh.iter().any(|e| matches!(e, CoreEvent::Opened { .. })) {
            assert!(connected(&eh), "a TCP-signalled link is Connected as it opens");
        }
    }
    assert_eq!(connected_event(&eh), Some((1, client_at, Path::Direct)));
}

/// A core never asked to STUN has nothing to gather (review M-6).
#[test]
fn without_stun_a_core_is_gathered() {
    let t0 = Instant::now();
    assert!(Core::new(CoreConfig::client(), t0).gathered(t0));
    let mut asked = Core::new(CoreConfig::client(), t0);
    asked.stun(vec!["198.51.100.1:3478".parse().unwrap()], t0);
    assert!(!asked.gathered(t0));
    assert!(asked.gathered(t0 + olr_net::probe::STUN_WAIT));
}

/// N49: a host's TCP-signalled joiner, its path as the host names it: the
/// host holds `mapped` (address, TCP mapped too), and the joiner comes
/// from `client_at` (in memory only: nothing is sent).
fn tcp_join_path(
    allow_local: bool,
    mapped: Option<(SocketAddr, bool)>,
    client_at: SocketAddr,
) -> olr_proto::Path {
    let t0 = Instant::now();
    let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
    let mut host = Core::new(CoreConfig { allow_local, ..CoreConfig::host() }, t0);
    if let Some((addr, tcp)) = mapped {
        host.set_mapped(Some(addr), tcp);
    }
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
    let client_path = ec.iter().find_map(|e| match e {
        CoreEvent::Net(NetEvent::Connected { path, .. }) => Some(*path),
        _ => None,
    });
    assert_eq!(client_path, Some(olr_proto::Path::Direct), "the joiner's TCP path is Direct");
    let (peer, path) = eh
        .iter()
        .find_map(|e| match e {
            CoreEvent::Net(NetEvent::Connected { peer, path, .. }) => Some((*peer, *path)),
            _ => None,
        })
        .unwrap();
    assert_eq!(host.path(peer), Some(path));
    path
}

/// N49: a host holding a usable TCP mapping names a TCP join from a global
/// address (or any, with `allow_local`) `MappedPort`; otherwise `Direct`.
#[test]
fn a_mapped_host_names_tcp_joins_mapped_port() {
    use olr_proto::Path::{Direct, MappedPort};
    let mapped: SocketAddr = "203.0.113.10:23400".parse().unwrap();
    let global: SocketAddr = "1.2.3.4:50000".parse().unwrap();
    let private: SocketAddr = "10.0.0.2:50000".parse().unwrap();
    assert_eq!(tcp_join_path(false, Some((mapped, true)), global), MappedPort);
    assert_eq!(tcp_join_path(false, Some((mapped, true)), private), Direct, "a LAN joiner");
    assert_eq!(tcp_join_path(true, Some((mapped, true)), private), MappedPort, "allow_local");
    assert_eq!(tcp_join_path(false, Some((mapped, false)), global), Direct, "UDP mapped only");
    assert_eq!(tcp_join_path(true, None, global), Direct, "no mapping");
}

/// Plays a host and a client on the virtual wire, each sending the
/// netcode's traffic (an unreliable message every 10 ms tick, a reliable
/// one every 100 ms) for `secs` seconds, with every datagram from the
/// client (`from_client`) or from the host lost from 2 s to `2 + stall` s.
/// Returns either side's `Disconnected`, else the stale messages both
/// dropped and the unreliable messages each received in the last second.
fn stalled_traffic(from_client: bool, stall: u64, secs: u64) -> Result<(u64, usize, usize), CoreEvent> {
    let t0 = Instant::now();
    let host_at: SocketAddr = "10.0.0.1:23400".parse().unwrap();
    let client_at: SocketAddr = "10.0.0.2:50000".parse().unwrap();
    let (mut client, mut host, _) = set_up(t0, client_at, host_at);
    let (mut eh, mut ec) = (Vec::new(), Vec::new());
    let mut now = t0;
    while !(connected(&eh) && connected(&ec)) {
        now += Duration::from_millis(5);
        assert!(now < t0 + Duration::from_secs(10), "no connection; host {eh:?} client {ec:?}");
        exchange(&mut client, client_at, &mut host, host_at, now, &mut ec, &mut eh);
    }
    let peer = eh
        .iter()
        .find_map(|e| match e {
            CoreEvent::Net(NetEvent::Connected { peer, .. }) => Some(*peer),
            _ => None,
        })
        .expect("connected");
    let unreliable = |events: &[CoreEvent]| {
        events
            .iter()
            .filter(|e| matches!(e, CoreEvent::Net(NetEvent::Message { channel: Channel::Unreliable, .. })))
            .count()
    };
    let (mut host_got, mut client_got) = (0, 0);
    let start = now;
    let mut out = Vec::new();
    for ms in 1..=secs * 1000 {
        now = start + Duration::from_millis(ms);
        let stalled = (2000..(2 + stall) * 1000).contains(&ms);
        if ms % 10 == 0 {
            client.send(HOST, Channel::Unreliable, &[ms as u8; 60]);
            host.send(peer, Channel::Unreliable, &[ms as u8; 200]);
        }
        if ms % 100 == 0 {
            client.send(HOST, Channel::Reliable, &[1; 24]);
            host.send(peer, Channel::Reliable, &[2; 48]);
        }
        eh.clear();
        ec.clear();
        client.poll(now, &mut out, &mut ec);
        for d in out.drain(..) {
            if !(stalled && from_client) {
                host.receive(client_at, &d.bytes, now);
            }
        }
        host.poll(now, &mut out, &mut eh);
        for d in out.drain(..) {
            if !(stalled && !from_client) {
                client.receive(host_at, &d.bytes, now);
            }
        }
        if let Some(e) =
            eh.iter().chain(&ec).find(|e| matches!(e, CoreEvent::Net(NetEvent::Disconnected { .. })))
        {
            return Err(e.clone());
        }
        if ms > (secs - 1) * 1000 {
            host_got += unreliable(&eh);
            client_got += unreliable(&ec);
        }
    }
    let stale = client.stats(HOST).unwrap().stale_dropped + host.stats(peer).unwrap().stale_dropped;
    Ok((stale, host_got, client_got))
}

/// A link whose datagrams are lost for seconds (a lossy path backing its
/// SCTP retransmission timeout off, worse under CPU load) keeps its
/// unreliable messages within the other side's limits once it recovers:
/// what could not leave during the stall is stale and dropped, never a
/// flood, and fresh messages flow again. The bot-grid flake of 2026-10-14:
/// after a 6.6 s stall a client's 543 queued inputs reached the host
/// within a second, past `max_datagrams_per_s`, and the host dropped an
/// honest client.
#[test]
fn a_stalled_links_unreliable_backlog_stays_within_the_limits() {
    for from_client in [true, false] {
        let side = if from_client { "client" } else { "host" };
        let (stale, host_got, client_got) =
            stalled_traffic(from_client, 6, 14).unwrap_or_else(|e| panic!("stalled from the {side}: {e:?}"));
        assert!(stale > 0, "stalled from the {side}: nothing dropped as stale");
        assert!(
            host_got >= 90 && client_got >= 90,
            "stalled from the {side}: {host_got} and {client_got} in the last second"
        );
    }
}
