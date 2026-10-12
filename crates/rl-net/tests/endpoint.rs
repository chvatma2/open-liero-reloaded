//! Endpoints over real localhost sockets (spec §5.1, §5.4, §9.4): a host
//! and its clients connect through WebSocket signalling, carry both
//! channels, refuse what they must and drop peers that break the limits.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rl_net::endpoint::{Endpoint, EndpointConfig, Gate, HOST, NetEvent, Why};
use rl_netcode::netsim::Profile;
use rl_netcode::{Channel, PeerId};
use rl_proto::Refusal;

fn config() -> EndpointConfig {
    EndpointConfig { port: 0, ..EndpointConfig::default() }
}

fn open_gate() -> Gate {
    Box::new(|_, _| Ok(()))
}

fn host_addr(host: &Endpoint) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], host.local_port()))
}

/// A host and its clients, polled together.
struct Net {
    host: Endpoint,
    host_events: Vec<NetEvent>,
    clients: Vec<(Endpoint, Vec<NetEvent>)>,
}

impl Net {
    fn new(cfg: EndpointConfig) -> Net {
        Net { host: Endpoint::host(cfg, open_gate()).unwrap(), host_events: Vec::new(), clients: Vec::new() }
    }

    /// Adds a client; returns its index.
    fn join(&mut self, cfg: EndpointConfig) -> usize {
        let client = Endpoint::connect(cfg, host_addr(&self.host)).unwrap();
        self.clients.push((client, Vec::new()));
        self.clients.len() - 1
    }

    fn poll(&mut self) {
        let now = Instant::now();
        self.host.poll(now, &mut self.host_events);
        for (client, events) in &mut self.clients {
            client.poll(now, events);
        }
    }

    /// Polls until `done` holds; panics after `limit` with `what`.
    fn run_until(&mut self, limit: Duration, what: &str, mut done: impl FnMut(&mut Net) -> bool) {
        let start = Instant::now();
        while !done(self) {
            assert!(start.elapsed() < limit, "{what} within {limit:?}");
            self.poll();
            std::thread::sleep(Duration::from_micros(300));
        }
    }

    fn connected(&self, client: usize) -> bool {
        self.clients[client].1.iter().any(|e| matches!(e, NetEvent::Connected { peer: HOST, .. }))
    }

    /// Joins a client and waits until both sides see the connection;
    /// returns the client's index and its peer id at the host.
    fn join_and_wait(&mut self, cfg: EndpointConfig) -> (usize, PeerId) {
        let before = self.host_connections().len();
        let c = self.join(cfg);
        self.run_until(Duration::from_secs(5), "the client connects", |n| {
            n.connected(c) && n.host_connections().len() > before
        });
        (c, self.host_connections()[before])
    }

    fn host_connections(&self) -> Vec<PeerId> {
        self.host_events
            .iter()
            .filter_map(|e| match e {
                NetEvent::Connected { peer, .. } => Some(*peer),
                _ => None,
            })
            .collect()
    }

    fn host_disconnect(&self, peer: PeerId) -> Option<Why> {
        self.host_events.iter().find_map(|e| match e {
            NetEvent::Disconnected { peer: p, why } if *p == peer => Some(*why),
            _ => None,
        })
    }

    /// The host sends every message it got back to its sender.
    fn echo(&mut self) {
        for e in &self.host_events {
            if let NetEvent::Message { peer, channel, bytes } = e {
                self.host.send(*peer, *channel, bytes);
            }
        }
        self.host_events.retain(|e| !matches!(e, NetEvent::Message { .. }));
    }
}

fn messages(events: &[NetEvent], channel: Channel) -> Vec<Vec<u8>> {
    events
        .iter()
        .filter_map(|e| match e {
            NetEvent::Message { channel: c, bytes, .. } if *c == channel => Some(bytes.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn endpoints_are_send() {
    fn send<T: Send>() {}
    send::<Endpoint>();
}

#[test]
fn a_host_and_three_clients_carry_both_channels() {
    let mut net = Net::new(config());
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(net.join_and_wait(config()).1);
    }
    assert_eq!(ids, [1, 2, 3], "host side peer ids count from 1");
    let peers: Vec<_> = net.host.peers().collect();
    assert_eq!(peers.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2, 3]);
    assert!(peers.iter().all(|p| p.1.ip() == std::net::Ipv4Addr::LOCALHOST), "{peers:?}");
    assert_eq!(net.clients[0].0.peers().map(|p| p.0).collect::<Vec<_>>(), [HOST]);

    let mut sent = 0u16;
    net.run_until(Duration::from_secs(10), "every echo", |n| {
        if sent < 100 {
            for (c, (client, _)) in n.clients.iter_mut().enumerate() {
                let [lo, hi] = sent.to_le_bytes();
                client.send(HOST, Channel::Unreliable, &[c as u8, 0, lo, hi]);
                client.send(HOST, Channel::Reliable, &[c as u8, 1, lo, hi]);
            }
            sent += 1;
        }
        n.echo();
        n.clients.iter().all(|(_, e)| {
            messages(e, Channel::Unreliable).len() >= 100 && messages(e, Channel::Reliable).len() >= 100
        })
    });
    for (c, (client, events)) in net.clients.iter().enumerate() {
        let expect = |kind: u8| -> Vec<Vec<u8>> {
            (0..100u16).map(|i| [&[c as u8, kind][..], &i.to_le_bytes()].concat()).collect()
        };
        assert_eq!(messages(events, Channel::Reliable), expect(1), "reliable messages arrive in order");
        let mut unreliable = messages(events, Channel::Unreliable);
        unreliable.sort_by_key(|m| u16::from_le_bytes([m[2], m[3]]));
        assert_eq!(unreliable, expect(0), "every unreliable message arrives once");
        let stats = client.stats(HOST).unwrap();
        assert!(stats.datagrams_sent > 0 && stats.datagrams_received > 0, "{stats:?}");
        assert!(stats.bytes_sent > 0 && stats.bytes_received > 0, "{stats:?}");
        assert_eq!(stats.sends_dropped, 0);
    }
    assert!(net.host.stats(1).is_some_and(|s| s.datagrams_received > 0));
    assert!(net.host.stats(9).is_none());
    assert!(!net.host_events.iter().any(|e| matches!(e, NetEvent::Disconnected { .. })));
}

#[test]
fn a_client_with_another_fingerprint_is_refused() {
    let mut net = Net::new(config());
    let other = rl_proto::FINGERPRINT ^ 1;
    let c = net.join(EndpointConfig { fingerprint_override: Some(other), ..config() });
    net.run_until(Duration::from_secs(5), "the refusal", |n| !n.clients[c].1.is_empty());
    assert_eq!(net.clients[c].1, [NetEvent::Refused { why: Refusal::Version }]);
    assert!(net.host_events.is_empty(), "{:?}", net.host_events);
}

#[test]
fn a_full_host_refuses_the_next_client() {
    let mut net = Net::new(EndpointConfig { max_peers: 1, ..config() });
    net.join_and_wait(config());
    let c = net.join(config());
    net.run_until(Duration::from_secs(5), "the refusal", |n| !n.clients[c].1.is_empty());
    assert_eq!(net.clients[c].1, [NetEvent::Refused { why: Refusal::Full }]);
    assert_eq!(net.host.peers().count(), 1);
}

#[test]
fn the_gate_refuses_with_its_reason() {
    let mut cfg = config();
    cfg.max_peers = 4;
    let mut host = Endpoint::host(cfg, Box::new(|_, _| Err(Refusal::Banned))).unwrap();
    let mut client = Endpoint::connect(config(), host_addr(&host)).unwrap();
    let (mut he, mut ce) = (Vec::new(), Vec::new());
    let start = Instant::now();
    while ce.is_empty() {
        assert!(start.elapsed() < Duration::from_secs(5), "the refusal within 5 s");
        host.poll(Instant::now(), &mut he);
        client.poll(Instant::now(), &mut ce);
        std::thread::sleep(Duration::from_micros(300));
    }
    assert_eq!(ce, [NetEvent::Refused { why: Refusal::Banned }]);
    assert!(he.is_empty());
}

#[test]
fn floods_and_oversized_messages_drop_the_peer() {
    let mut net = Net::new(config());
    let (flooder, flooder_id) = net.join_and_wait(config());
    let (big, big_id) = net.join_and_wait(config());
    let (good, good_id) = net.join_and_wait(config());

    // 2000 datagrams in 1 s from one client, a 63 KB reliable message from
    // another (above rl_net::limits::MAX_RELIABLE; Link refuses above 64 KiB), a
    // message every 10 ms from the third.
    let start = Instant::now();
    let (mut flooded, mut ticks) = (0u32, 0u32);
    net.clients[big].0.send(HOST, Channel::Reliable, &vec![7; 63_000]);
    net.run_until(Duration::from_secs(5), "both bad peers dropped", |n| {
        let due = (start.elapsed().as_micros() / 500).min(2000) as u32;
        while flooded < due {
            n.clients[flooder].0.send(HOST, Channel::Unreliable, &flooded.to_le_bytes());
            flooded += 1;
        }
        let due = (start.elapsed().as_millis() / 10) as u32;
        while ticks < due {
            n.clients[good].0.send(HOST, Channel::Reliable, &ticks.to_le_bytes());
            ticks += 1;
        }
        n.echo();
        n.host_disconnect(flooder_id).is_some() && n.host_disconnect(big_id).is_some()
    });
    assert_eq!(net.host_disconnect(flooder_id), Some(Why::Limits));
    assert_eq!(net.host_disconnect(big_id), Some(Why::Malformed));
    // The bad clients see their link end.
    net.run_until(Duration::from_secs(3), "the bad clients see the close", |n| {
        [flooder, big]
            .iter()
            .all(|&c| n.clients[c].1.iter().any(|e| matches!(e, NetEvent::Disconnected { peer: HOST, .. })))
    });
    // The good client stays and is still answered.
    let echoed = messages(&net.clients[good].1, Channel::Reliable).len();
    net.clients[good].0.send(HOST, Channel::Reliable, b"still here");
    net.run_until(Duration::from_secs(3), "the good client's echo", |n| {
        n.echo();
        messages(&n.clients[good].1, Channel::Reliable).len() > echoed
            && messages(&n.clients[good].1, Channel::Reliable).last().is_some_and(|m| m == b"still here")
    });
    assert_eq!(net.host_disconnect(good_id), None);
    assert_eq!(net.host.peers().map(|p| p.0).collect::<Vec<_>>(), [good_id]);
    assert!(!net.clients[good].1.iter().any(|e| matches!(e, NetEvent::Disconnected { .. })));
}

/// Polls `host` on a thread until the returned flag is set; its events
/// collect in the returned list.
fn host_thread(
    mut host: Endpoint,
) -> (Arc<AtomicBool>, Arc<Mutex<Vec<NetEvent>>>, std::thread::JoinHandle<Endpoint>) {
    let stop = Arc::new(AtomicBool::new(false));
    let events = Arc::new(Mutex::new(Vec::new()));
    let (s, e) = (stop.clone(), events.clone());
    let thread = std::thread::spawn(move || {
        let mut got = Vec::new();
        while !s.load(Ordering::Relaxed) {
            host.poll(Instant::now(), &mut got);
            e.lock().unwrap().append(&mut got);
            std::thread::sleep(Duration::from_micros(300));
        }
        host
    });
    (stop, events, thread)
}

/// Reads from `stream` until the host closes it (end of stream or reset);
/// panics after 3 s.
fn closed_by_host(stream: &mut TcpStream) {
    stream.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
    let start = Instant::now();
    let mut buf = [0u8; 4096];
    loop {
        assert!(start.elapsed() < Duration::from_secs(3), "the host closes the connection within 3 s");
        match stream.read(&mut buf) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => return,
        }
    }
}

#[test]
fn garbage_and_oversized_signalling_close_the_connection_quietly() {
    let host = Endpoint::host(config(), open_gate()).unwrap();
    let addr = host_addr(&host);
    let (stop, events, thread) = host_thread(host);

    // Not a WebSocket handshake.
    let mut garbage = TcpStream::connect(addr).unwrap();
    garbage.write_all(b"HELLO THERE\r\n\r\n\x00\x01\x02 not http at all").unwrap();
    closed_by_host(&mut garbage);

    // A real handshake, then a 20 KB frame (above MAX_SIGNAL).
    let stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let (mut ws, _) = tungstenite::client::client(format!("ws://{addr}/"), stream).unwrap();
    ws.send(tungstenite::Message::Binary(vec![1u8; 20_000].into())).unwrap();
    closed_by_host(ws.get_mut());

    // A handshake whose frame is a valid message but not an offer.
    let stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let (mut ws, _) = tungstenite::client::client(format!("ws://{addr}/"), stream).unwrap();
    let answer = rl_proto::encode_signal(&rl_proto::Signal::Answer { sdp: "v=0".into() });
    ws.send(tungstenite::Message::Binary(answer.into())).unwrap();
    closed_by_host(ws.get_mut());

    stop.store(true, Ordering::Relaxed);
    let mut host = thread.join().unwrap();
    assert!(events.lock().unwrap().is_empty(), "{:?}", events.lock().unwrap());

    // The host still takes a proper client.
    let mut client = Endpoint::connect(config(), addr).unwrap();
    let (mut he, mut ce) = (Vec::new(), Vec::new());
    let start = Instant::now();
    while !ce.iter().any(|e| matches!(e, NetEvent::Connected { .. })) {
        assert!(start.elapsed() < Duration::from_secs(5), "a client connects afterwards");
        host.poll(Instant::now(), &mut he);
        client.poll(Instant::now(), &mut ce);
        std::thread::sleep(Duration::from_micros(300));
    }
}

#[test]
fn a_closed_client_is_seen_at_once() {
    let mut net = Net::new(config());
    let (_, id) = net.join_and_wait(config());
    let (client, _) = net.clients.pop().unwrap();
    drop(client);
    let start = Instant::now();
    net.run_until(Duration::from_secs(1), "the host sees the close", |n| n.host_disconnect(id).is_some());
    eprintln!("close seen in {:?}", start.elapsed());
    assert_eq!(net.host_disconnect(id), Some(Why::Closed));
    assert_eq!(net.host.peers().count(), 0);
}

#[test]
fn a_host_closing_a_peer_reports_nothing_and_the_client_sees_it() {
    let mut net = Net::new(config());
    let (c, id) = net.join_and_wait(config());
    net.host.close(id);
    assert_eq!(net.host.peers().count(), 0);
    net.run_until(Duration::from_secs(2), "the client sees the close", |n| {
        n.clients[c].1.iter().any(|e| matches!(e, NetEvent::Disconnected { peer: HOST, why: Why::Closed }))
    });
    assert_eq!(net.host_disconnect(id), None, "no event for a local close");
}

#[test]
fn a_client_whose_host_is_not_there_gives_up() {
    // A port nobody listens on: the TCP connection is refused at once.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let mut client = Endpoint::connect(config(), addr).unwrap();
    let mut events = Vec::new();
    let start = Instant::now();
    while events.is_empty() {
        assert!(start.elapsed() < Duration::from_secs(6), "a refused TCP connection ends the attempt");
        client.poll(Instant::now(), &mut events);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(events, [NetEvent::Disconnected { peer: HOST, why: Why::Closed }]);
    assert_eq!(client.peers().count(), 0);
}

#[test]
fn reliable_messages_arrive_in_order_through_a_bad_network() {
    let bad = EndpointConfig { impair: Some(Profile::BAD), impair_seed: 7, ..config() };
    let mut net = Net::new(EndpointConfig { impair_seed: 8, ..bad.clone() });
    let (c, id) = net.join_and_wait_within(bad, Duration::from_secs(15));
    let start = Instant::now();
    for i in 0..1000u32 {
        net.clients[c].0.send(HOST, Channel::Reliable, &i.to_le_bytes());
    }
    net.run_until(Duration::from_secs(20), "1000 reliable messages through `bad`", |n| {
        n.host_events.iter().filter(|e| matches!(e, NetEvent::Message { .. })).count() >= 1000
    });
    eprintln!("1000 messages through `bad` in {:?}", start.elapsed());
    let got: Vec<u32> = messages(&net.host_events, Channel::Reliable)
        .iter()
        .map(|m| u32::from_le_bytes(m[..].try_into().unwrap()))
        .collect();
    assert_eq!(got, (0..1000).collect::<Vec<_>>());
    assert!(net.host_disconnect(id).is_none());
}

impl Net {
    fn join_and_wait_within(&mut self, cfg: EndpointConfig, limit: Duration) -> (usize, PeerId) {
        let c = self.join(cfg);
        self.run_until(limit, "the client connects", |n| n.connected(c) && !n.host_connections().is_empty());
        (c, self.host_connections()[0])
    }
}

/// About 10 s: a client that stops answering is timed out, and a joiner
/// whose link never opens (a valid offer, then no UDP) is closed quietly
/// after the setup timeout.
#[test]
fn silent_peers_are_timed_out() {
    let host = Endpoint::host(config(), open_gate()).unwrap();
    let addr = host_addr(&host);
    let (stop, events, thread) = host_thread(host);
    let mut client = Endpoint::connect(config(), addr).unwrap();
    let mut ce = Vec::new();
    let start = Instant::now();
    while !ce.iter().any(|e| matches!(e, NetEvent::Connected { .. })) {
        assert!(start.elapsed() < Duration::from_secs(5), "the client connects");
        client.poll(Instant::now(), &mut ce);
        std::thread::sleep(Duration::from_micros(300));
    }
    // From now on the client is never polled again.

    let stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let (mut ws, _) = tungstenite::client::client(format!("ws://{addr}/"), stream).unwrap();
    let (_link, sdp) = rl_net::link::Link::offerer(&["127.0.0.1:9".parse().unwrap()], Instant::now());
    let offer = rl_proto::Signal::Offer {
        protocol: rl_proto::PROTOCOL_VERSION,
        fingerprint: rl_proto::FINGERPRINT,
        sdp,
    };
    ws.send(tungstenite::Message::Binary(rl_proto::encode_signal(&offer).into())).unwrap();
    let answer = ws.read().unwrap();
    assert!(
        matches!(rl_proto::decode_signal(&answer.into_data()), Ok(rl_proto::Signal::Answer { .. })),
        "the host answers a valid offer"
    );
    let joined = Instant::now();
    ws.get_mut().set_read_timeout(Some(Duration::from_millis(50))).unwrap();
    let start = Instant::now();
    let mut buf = [0u8; 1024];
    loop {
        assert!(start.elapsed() < Duration::from_secs(13), "the host gives up on the link");
        match ws.get_mut().read(&mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
    let waited = joined.elapsed();
    assert!(waited >= Duration::from_secs(9), "not before the setup timeout: {waited:?}");
    let start = Instant::now();
    while !events.lock().unwrap().iter().any(|e| matches!(e, NetEvent::Disconnected { .. })) {
        assert!(start.elapsed() < Duration::from_secs(5), "the silent client is timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
    stop.store(true, Ordering::Relaxed);
    let host = thread.join().unwrap();
    let events = events.lock().unwrap();
    assert!(
        matches!(
            events[..],
            [NetEvent::Connected { peer: 1, .. }, NetEvent::Disconnected { peer: 1, why: Why::Timeout }]
        ),
        "{events:?}"
    );
    assert_eq!(host.peers().count(), 0);
    drop(client);
}

/// A raw WebSocket to the host at `addr` (blocking, reads time out after
/// 2 s), its handshake done; the host must be polled meanwhile.
fn raw_ws(addr: SocketAddr) -> tungstenite::Result<tungstenite::WebSocket<TcpStream>> {
    let stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    match tungstenite::client::client(format!("ws://{addr}/"), stream) {
        Ok((ws, _)) => Ok(ws),
        Err(tungstenite::HandshakeError::Failure(e)) => Err(e),
        Err(tungstenite::HandshakeError::Interrupted(_)) => Err(tungstenite::Error::ConnectionClosed),
    }
}

/// A valid offer whose only candidate is `candidate`.
fn offer_from(candidate: &str) -> (rl_net::link::Link, Vec<u8>) {
    let (link, sdp) = rl_net::link::Link::offerer(&[candidate.parse().unwrap()], Instant::now());
    let offer = rl_proto::Signal::Offer {
        protocol: rl_proto::PROTOCOL_VERSION,
        fingerprint: rl_proto::FINGERPRINT,
        sdp,
    };
    (link, rl_proto::encode_signal(&offer))
}

/// Sends `offer` on `ws` and returns the host's reply.
fn exchange(ws: &mut tungstenite::WebSocket<TcpStream>, offer: Vec<u8>) -> Option<rl_proto::Signal> {
    ws.send(tungstenite::Message::Binary(offer.into())).ok()?;
    rl_proto::decode_signal(&ws.read().ok()?.into_data()).ok()
}

/// Pings in a tight loop (before or after an offer) cannot keep the host
/// inside `poll`: every poll returns quickly and both connections close.
#[test]
fn a_pinging_peer_cannot_hold_the_host() {
    let mut host = Endpoint::host(config(), open_gate()).unwrap();
    let addr = host_addr(&host);
    let ready = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let closed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let attackers: Vec<_> = [false, true]
        .into_iter()
        .map(|after_offer| {
            let (ready, closed) = (ready.clone(), closed.clone());
            std::thread::spawn(move || {
                let mut ws = raw_ws(addr).unwrap();
                if after_offer {
                    let reply = exchange(&mut ws, offer_from("127.0.0.1:9").1);
                    assert!(matches!(reply, Some(rl_proto::Signal::Answer { .. })), "{reply:?}");
                }
                ready.fetch_add(1, Ordering::SeqCst);
                // Masked empty pings, written straight to the socket.
                let pings = [0x89u8, 0x80, 0, 0, 0, 0].repeat(1000);
                let stream = ws.get_mut();
                stream.set_write_timeout(Some(Duration::from_millis(20))).unwrap();
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(3) {
                    match stream.write(&pings) {
                        Ok(_) => {}
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(_) => {
                            closed.fetch_add(1, Ordering::SeqCst);
                            return;
                        }
                    }
                }
            })
        })
        .collect();
    let mut events = Vec::new();
    let start = Instant::now();
    while ready.load(Ordering::SeqCst) < 2 {
        assert!(start.elapsed() < Duration::from_secs(5), "the attackers connect");
        host.poll(Instant::now(), &mut events);
        std::thread::sleep(Duration::from_micros(300));
    }
    let (start, mut slowest) = (Instant::now(), Duration::ZERO);
    while closed.load(Ordering::SeqCst) < 2 {
        assert!(start.elapsed() < Duration::from_secs(3), "the host closes both connections");
        let t = Instant::now();
        host.poll(Instant::now(), &mut events);
        slowest = slowest.max(t.elapsed());
        std::thread::sleep(Duration::from_micros(300));
    }
    for attacker in attackers {
        attacker.join().unwrap();
    }
    assert!(slowest < Duration::from_millis(50), "the slowest poll took {slowest:?}");
    assert!(events.is_empty(), "{events:?}");
}

/// One IP holds at most `MAX_PER_IP` connections that are not open yet, so
/// a flood of offers from it leaves room for others.
#[test]
fn one_address_cannot_hold_every_pending_slot() {
    let host = Endpoint::host(EndpointConfig { max_peers: 16, ..config() }, open_gate()).unwrap();
    let addr = host_addr(&host);
    let (stop, events, thread) = host_thread(host);
    let mut answered = 0;
    let mut held = Vec::new();
    for _ in 0..20 {
        let Ok(mut ws) = raw_ws(addr) else { continue };
        if let Some(rl_proto::Signal::Answer { .. }) = exchange(&mut ws, offer_from("127.0.0.1:9").1) {
            answered += 1;
            held.push(ws);
        }
    }
    assert_eq!(answered, rl_net::endpoint::MAX_PER_IP, "offers answered from one address");

    // A joiner from another address of this machine still gets in.
    let other = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").and_then(|()| s.local_addr()))
        .map(|a| a.ip())
        .ok()
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    match other {
        None => eprintln!("skipped the second address: this machine has no non-loopback address"),
        Some(ip) => {
            let mut client = Endpoint::connect(config(), SocketAddr::new(ip, addr.port())).unwrap();
            let mut ce = Vec::new();
            let start = Instant::now();
            while !ce.iter().any(|e| matches!(e, NetEvent::Connected { .. })) {
                assert!(start.elapsed() < Duration::from_secs(5), "a joiner from {ip} connects: {ce:?}");
                client.poll(Instant::now(), &mut ce);
                std::thread::sleep(Duration::from_micros(300));
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
    thread.join().unwrap();
    drop(held);
    let events = events.lock().unwrap();
    assert!(events.iter().all(|e| matches!(e, NetEvent::Connected { .. })), "{events:?}");
}

/// An offer naming another IP than the joiner's is answered, its forged
/// candidate dropped (ruling T4-A: the host never sends checks there).
#[test]
fn an_offer_naming_another_address_is_answered_without_it() {
    let host = Endpoint::host(config(), open_gate()).unwrap();
    let addr = host_addr(&host);
    let (stop, events, thread) = host_thread(host);
    let mut ws = raw_ws(addr).unwrap();
    let reply = exchange(&mut ws, offer_from("10.1.2.3:9").1);
    stop.store(true, Ordering::Relaxed);
    thread.join().unwrap();
    assert!(matches!(reply, Some(rl_proto::Signal::Answer { .. })), "{reply:?}");
    assert!(events.lock().unwrap().is_empty());
}

/// A client whose offer names an address the host cannot reach (behind NAT:
/// its private address) still connects: with no candidate left, the host
/// learns the peer-reflexive one from the client's checks.
#[test]
fn a_client_behind_nat_connects_through_its_reflexive_address() {
    let mut net = Net::new(config());
    let nat = EndpointConfig { offer_ip_override: Some("10.9.9.9".parse().unwrap()), ..config() };
    let (c, id) = net.join_and_wait(nat);
    net.clients[c].0.send(HOST, Channel::Reliable, b"hello");
    net.run_until(Duration::from_secs(3), "the echo", |n| {
        n.echo();
        !messages(&n.clients[c].1, Channel::Reliable).is_empty()
    });
    assert_eq!(net.host.peers().map(|p| p.0).collect::<Vec<_>>(), [id]);
}

/// A connected peer's address that floods raw datagrams no link can use
/// (undecryptable DTLS) is dropped for its limits; others stay.
#[test]
fn raw_junk_from_a_peer_drops_it() {
    let host = Endpoint::host(config(), open_gate()).unwrap();
    let addr = host_addr(&host);
    let (stop, events, thread) = host_thread(host);
    let mut good = Endpoint::connect(config(), addr).unwrap();
    let mut ge = Vec::new();
    let start = Instant::now();
    while !ge.iter().any(|e| matches!(e, NetEvent::Connected { .. })) {
        assert!(start.elapsed() < Duration::from_secs(5), "the good client connects");
        good.poll(Instant::now(), &mut ge);
        std::thread::sleep(Duration::from_micros(300));
    }

    // A hand-made client: its own socket, link and signalling.
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_nonblocking(true).unwrap();
    let local = udp.local_addr().unwrap();
    let (mut link, offer) = offer_from(&local.to_string());
    let mut ws = raw_ws(addr).unwrap();
    let Some(rl_proto::Signal::Answer { sdp }) = exchange(&mut ws, offer) else { panic!("no answer") };
    link.accept_answer(&sdp).unwrap();
    let pump = |link: &mut rl_net::link::Link| {
        let mut buf = [0u8; 2048];
        while let Ok((n, from)) = udp.recv_from(&mut buf) {
            link.receive(from, local, &buf[..n], Instant::now());
        }
        let mut out = Vec::new();
        link.poll(Instant::now(), &mut out, &mut Vec::new());
        for t in out {
            let _ = udp.send_to(&t.bytes, t.to);
        }
    };
    let start = Instant::now();
    while !link.is_open() {
        assert!(start.elapsed() < Duration::from_secs(5), "the hand-made client connects");
        pump(&mut link);
        good.poll(Instant::now(), &mut ge);
        std::thread::sleep(Duration::from_micros(300));
    }
    drop(ws);

    // Undecryptable DTLS application data from the peer's address.
    let mut junk = vec![23u8, 0xfe, 0xfd];
    junk.extend((0..1197).map(|i| (i * 31 % 251) as u8));
    let dropped = |events: &Mutex<Vec<NetEvent>>| {
        events.lock().unwrap().iter().find_map(|e| match e {
            NetEvent::Disconnected { peer: 2, why } => Some(*why),
            _ => None,
        })
    };
    let start = Instant::now();
    while dropped(&events).is_none() {
        assert!(start.elapsed() < Duration::from_secs(5), "the flooding peer is dropped");
        for _ in 0..200 {
            let _ = udp.send_to(&junk, addr);
        }
        good.poll(Instant::now(), &mut ge);
    }
    eprintln!("flood dropped in {:?}", start.elapsed());
    assert_eq!(dropped(&events), Some(Why::Limits));
    // The good client is still answered by the host.
    good.send(HOST, Channel::Reliable, b"ping");
    let start = Instant::now();
    stop.store(true, Ordering::Relaxed);
    let mut host = thread.join().unwrap();
    let mut he = Vec::new();
    while !he.iter().any(|e| matches!(e, NetEvent::Message { peer: 1, .. })) {
        assert!(start.elapsed() < Duration::from_secs(3), "the good client's message arrives");
        host.poll(Instant::now(), &mut he);
        good.poll(Instant::now(), &mut ge);
        std::thread::sleep(Duration::from_micros(300));
    }
    assert_eq!(host.peers().map(|p| p.0).collect::<Vec<_>>(), [1]);
    assert!(!ge.iter().any(|e| matches!(e, NetEvent::Disconnected { .. })), "{ge:?}");
}
