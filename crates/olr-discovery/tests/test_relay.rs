//! The in-process test relay: storage, replacement, expiry, ephemeral
//! forwarding, filters, TLS, misbehaviour and shutdown.

use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use olr_discovery::*;
use olr_lobby::Identity;
use rustls::pki_types::{CertificateDer, ServerName};
use tungstenite::{Message, WebSocket};

trait Stream: Read + Write + Send {}
impl<T: Read + Write + Send> Stream for T {}
struct Conn(Box<dyn Stream>);
impl Read for Conn {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(b)
    }
}
impl Write for Conn {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

struct Client(WebSocket<Conn>);

/// What the next read brought.
#[derive(Debug, PartialEq)]
enum Next {
    Frame(Box<FromRelay>),
    /// Nothing within the read timeout; the connection is open.
    Timeout,
    /// The relay closed the connection.
    Closed,
}

/// Connects with a 5 s timeout for the TCP connect and the handshake.
fn tcp(port: u16) -> TcpStream {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
    s
}

impl Client {
    fn connect(relay: &TestRelay) -> Client {
        Client::connect_timeout(relay, Duration::from_secs(5))
    }

    /// Connects (5 s), then reads with `timeout`.
    fn connect_timeout(relay: &TestRelay, timeout: Duration) -> Client {
        let s = tcp(relay.port());
        let ctl = s.try_clone().unwrap();
        let url = relay.url();
        let (ws, _) = tungstenite::client(url.as_str(), Conn(Box::new(s))).unwrap();
        ctl.set_read_timeout(Some(timeout)).unwrap();
        Client(ws)
    }

    fn connect_tls(relay: &TestRelay, root: &[u8]) -> Result<Client, String> {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(CertificateDer::from(root.to_vec())).unwrap();
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let conn =
            rustls::ClientConnection::new(Arc::new(config), ServerName::try_from("localhost").unwrap())
                .unwrap();
        let stream = rustls::StreamOwned::new(conn, tcp(relay.port()));
        let url = relay.url();
        tungstenite::client(url.as_str(), Conn(Box::new(stream)))
            .map(|(ws, _)| Client(ws))
            .map_err(|e| e.to_string())
    }

    fn send(&mut self, m: &ToRelay) {
        self.0.send(Message::text(encode_to_relay(m))).unwrap();
    }

    fn next(&mut self) -> Next {
        use tungstenite::error::{Error, ProtocolError};
        loop {
            match self.0.read() {
                Ok(Message::Text(t)) => return Next::Frame(Box::new(decode_from_relay(t.as_str()).unwrap())),
                Ok(Message::Close(_)) => return Next::Closed,
                Ok(_) => {}
                Err(Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    return Next::Timeout;
                }
                Err(
                    Error::ConnectionClosed
                    | Error::AlreadyClosed
                    | Error::Protocol(ProtocolError::ResetWithoutClosingHandshake),
                ) => return Next::Closed,
                Err(Error::Io(e))
                    if matches!(
                        e.kind(),
                        ErrorKind::UnexpectedEof
                            | ErrorKind::ConnectionReset
                            | ErrorKind::ConnectionAborted
                            | ErrorKind::BrokenPipe
                    ) =>
                {
                    return Next::Closed;
                }
                Err(e) => panic!("neither a frame, a timeout nor a close: {e}"),
            }
        }
    }

    fn recv(&mut self) -> FromRelay {
        match self.next() {
            Next::Frame(f) => *f,
            other => panic!("expected a frame from the relay, got {other:?}"),
        }
    }

    /// Publishes an event; the relay's verdict.
    fn publish(&mut self, e: &Event) -> (bool, String) {
        self.send(&ToRelay::Event(e.clone()));
        match self.recv() {
            FromRelay::Ok { id, accepted, message } => {
                assert_eq!(id, e.id);
                (accepted, message)
            }
            other => panic!("expected OK, got {other:?}"),
        }
    }

    /// REQ, then CLOSE: the stored events up to EOSE.
    fn query(&mut self, sub: &str, filters: Vec<Filter>) -> Vec<Event> {
        let out = self.subscribe(sub, filters);
        self.send(&ToRelay::Close { sub: sub.into() });
        out
    }

    /// REQ and stay subscribed: the stored events up to EOSE.
    fn subscribe(&mut self, sub: &str, filters: Vec<Filter>) -> Vec<Event> {
        self.send(&ToRelay::Req { sub: sub.into(), filters });
        let mut out = Vec::new();
        loop {
            match self.recv() {
                FromRelay::Event { sub: s, event } => {
                    assert_eq!(s, sub);
                    out.push(event);
                }
                FromRelay::Eose { sub: s } => {
                    assert_eq!(s, sub);
                    return out;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }
}

fn id(n: u8) -> Identity {
    Identity::from_secret([n; 32]).unwrap()
}

fn tags(t: &[&[&str]]) -> Vec<Vec<String>> {
    t.iter().map(|t| t.iter().map(|s| s.to_string()).collect()).collect()
}

fn ev(who: u8, at: u64, kind: u32, t: &[&[&str]], content: &str) -> Event {
    Event::sign(&id(who), at, kind, tags(t), content.into(), [who; 32])
}

fn announce(who: u8, at: u64, content: &str) -> Event {
    let exp = (at + 180).to_string();
    ev(who, at, KIND_ANNOUNCE, &[&["d", "rust-liero"], &["t", "rust-liero"], &["expiration", &exp]], content)
}

fn kinds(k: u32) -> Filter {
    Filter { kinds: vec![k], ..Filter::default() }
}

#[test]
fn stores_replaces_and_expires() {
    let relay = TestRelay::start();
    relay.set_time(1000);
    let mut c = Client::connect(&relay);

    let a1 = announce(1, 1000, "first");
    assert_eq!(c.publish(&a1), (true, String::new()));
    assert_eq!(relay.events(), vec![a1.clone()]);
    assert_eq!(c.query("s", vec![kinds(KIND_ANNOUNCE)]), vec![a1.clone()]);

    // A newer note from the same key replaces it; an older one does not.
    let a2 = announce(1, 1030, "second");
    assert!(c.publish(&a2).0);
    assert_eq!(relay.events(), vec![a2.clone()]);
    let (accepted, message) = c.publish(&a1);
    assert!(accepted && message.starts_with("duplicate"), "{message}");
    assert_eq!(relay.events(), vec![a2.clone()]);
    // Another key, or another `d`, is another slot.
    let b = announce(2, 1031, "other key");
    let a_other_d = ev(1, 1032, KIND_ANNOUNCE, &[&["d", "elsewhere"]], "other d");
    assert!(c.publish(&b).0 && c.publish(&a_other_d).0);
    assert_eq!(relay.events().len(), 3);
    // A regular event is stored once; a republish is a duplicate.
    let note = ev(1, 1033, 1, &[], "hi");
    assert!(c.publish(&note).0 && c.publish(&note).0);
    assert_eq!(relay.events().len(), 4);
    // A replaceable kind replaces by (key, kind).
    let m1 = ev(1, 1034, 10002, &[], "one");
    let m2 = ev(1, 1035, 10002, &[], "two");
    assert!(c.publish(&m1).0 && c.publish(&m2).0);
    assert_eq!(c.query("r", vec![kinds(10002)]), vec![m2]);

    // A bad signature is refused, and not stored.
    let mut forged = announce(3, 1040, "forged");
    forged.content.push('!');
    let (accepted, message) = c.publish(&forged);
    assert!(!accepted && message.starts_with("invalid"), "{message}");
    assert!(relay.events().iter().all(|e| e.id != forged.id));
    // An event already expired is refused.
    let (accepted, message) = c.publish(&announce(3, 700, "old"));
    assert!(!accepted && message.starts_with("invalid"), "{message}");

    // NIP-40: expired notes are not served (they stay stored).
    let ann = vec![kinds(KIND_ANNOUNCE)];
    assert_eq!(c.query("a", ann.clone()).len(), 3);
    relay.set_time(1031 + 180);
    let live: Vec<Event> = c.query("b", ann.clone());
    assert_eq!(live.len(), 1, "only the note whose expiry is later: {live:?}");
    assert_eq!(live[0].id, a_other_d.id);
    relay.set_time(5000);
    assert_eq!(c.query("c", ann), vec![a_other_d]);
}

#[test]
fn ephemeral_events_reach_live_subscriptions_only() {
    let relay = TestRelay::start();
    relay.set_time(1000);
    let mut listener = Client::connect(&relay);
    let mut other = Client::connect(&relay);
    let mut sender = Client::connect(&relay);
    assert!(listener.subscribe("hs", vec![kinds(KIND_HANDSHAKE)]).is_empty());
    assert!(other.subscribe("elsewhere", vec![kinds(1)]).is_empty());

    let e = ev(1, 1000, KIND_HANDSHAKE, &[&["p", "abc"]], "offer");
    assert_eq!(sender.publish(&e), (true, String::new()));
    assert_eq!(listener.recv(), FromRelay::Event { sub: "hs".into(), event: e.clone() });
    assert!(relay.events().is_empty(), "ephemeral events are never stored");
    // A later subscription finds nothing.
    assert!(sender.query("late", vec![kinds(KIND_HANDSHAKE)]).is_empty());

    // Stored events reach live subscriptions too; CLOSE ends one.
    let note = ev(1, 1001, 1, &[], "live");
    sender.publish(&note);
    other.send(&ToRelay::Close { sub: "elsewhere".into() });
    listener.send(&ToRelay::Close { sub: "hs".into() });
    assert!(listener.query("probe", vec![kinds(2)]).is_empty());
    let note2 = ev(1, 1002, 1, &[], "after close");
    sender.publish(&note2);
    // `hs` is closed: a second handshake event reaches nobody. Had it
    // reached `listener`, it would arrive before probe2's EOSE (the relay
    // forwards before its OK), and `query` would panic on it.
    let e2 = ev(1, 1003, KIND_HANDSHAKE, &[&["p", "abc"]], "offer 2");
    assert_eq!(sender.publish(&e2), (true, String::new()));
    assert!(listener.query("probe2", vec![kinds(2)]).is_empty());
    // `other` received `note` (live) but not `note2` (closed); a frame from
    // a closed sub would arrive before this query's EOSE and fail the assert.
    assert_eq!(other.recv(), FromRelay::Event { sub: "elsewhere".into(), event: note });
    assert!(other.query("x", vec![kinds(2)]).is_empty());
}

#[test]
fn filters_select_by_kind_author_and_tags() {
    let relay = TestRelay::start();
    relay.set_time(1000);
    let mut c = Client::connect(&relay);
    let e1 = ev(1, 100, 1, &[&["t", "rust-liero"], &["f", "aaaa"]], "e1");
    let e2 = ev(2, 200, 1, &[&["t", "other"], &["f", "bbbb"]], "e2");
    let e3 = ev(1, 300, 2, &[&["p", "pk"], &["d", "x"]], "e3");
    let e4 = ev(2, 400, 30000, &[&["d", "slot"], &["t", "rust-liero"]], "e4");
    for e in [&e1, &e2, &e3, &e4] {
        assert!(c.publish(e).0);
    }
    let ids = |v: Vec<Event>| v.into_iter().map(|e| e.content).collect::<Vec<_>>();
    let q = |c: &mut Client, f: Filter| ids(c.query("q", vec![f]));
    // Newest first.
    assert_eq!(q(&mut c, Filter::default()), ["e4", "e3", "e2", "e1"]);
    assert_eq!(q(&mut c, kinds(1)), ["e2", "e1"]);
    let by = |n: u8| Filter { authors: vec![id(n).public()], ..Filter::default() };
    assert_eq!(q(&mut c, by(1)), ["e3", "e1"]);
    let tag = |c: char, v: &[&str]| Filter {
        tags: vec![(c, v.iter().map(|s| s.to_string()).collect())],
        ..Filter::default()
    };
    assert_eq!(q(&mut c, tag('t', &["rust-liero"])), ["e4", "e1"]);
    assert_eq!(q(&mut c, tag('t', &["rust-liero", "other"])), ["e4", "e2", "e1"]);
    assert_eq!(q(&mut c, tag('f', &["bbbb"])), ["e2"]);
    assert_eq!(q(&mut c, tag('p', &["pk"])), ["e3"]);
    assert_eq!(q(&mut c, tag('d', &["x"])), ["e3"]);
    assert!(q(&mut c, tag('t', &["nothing"])).is_empty());
    // Conditions combine with AND.
    let both = Filter { kinds: vec![1], authors: vec![id(2).public()], ..tag('t', &["other"]) };
    assert_eq!(q(&mut c, both), ["e2"]);
    assert!(q(&mut c, Filter { kinds: vec![2], ..by(2) }).is_empty());
    // since and limit.
    assert_eq!(q(&mut c, Filter { since: Some(300), ..Filter::default() }), ["e4", "e3"]);
    assert_eq!(q(&mut c, Filter { limit: Some(2), ..Filter::default() }), ["e4", "e3"]);
    assert_eq!(q(&mut c, Filter { limit: Some(0), ..Filter::default() }), Vec::<String>::new());
    // Several filters are a union without duplicates.
    let two = c.query("u", vec![kinds(1), by(2)]);
    assert_eq!(ids(two), ["e4", "e2", "e1"]);
    // No filters select nothing.
    assert!(c.query("none", vec![]).is_empty());
}

#[test]
fn tls_works_with_the_given_root() {
    let (relay, root) = TestRelay::start_tls();
    assert!(relay.url().starts_with("wss://localhost:"), "{}", relay.url());
    relay.set_time(1000);
    let mut c = Client::connect_tls(&relay, &root).expect("a TLS connection with the given root");
    let e = announce(1, 1000, "over tls");
    assert!(c.publish(&e).0);
    assert_eq!(c.query("s", vec![kinds(KIND_ANNOUNCE)]), vec![e]);

    // Another relay's certificate is not trusted by this root's holder.
    let (other, other_root) = TestRelay::start_tls();
    assert_ne!(root, other_root);
    assert!(Client::connect_tls(&other, &root).is_err());
    assert!(Client::connect_tls(&other, &other_root).is_ok());
}

#[test]
fn misbehaviours() {
    let relay = TestRelay::start();
    relay.set_time(1000);
    let e = ev(1, 1000, 1, &[], "stored");
    Client::connect(&relay).publish(&e);

    // NeverEose: stored events arrive, EOSE never does.
    relay.set_misbehaviour(Misbehaviour::NeverEose);
    let mut c = Client::connect_timeout(&relay, Duration::from_millis(300));
    c.send(&ToRelay::Req { sub: "s".into(), filters: vec![kinds(1)] });
    assert_eq!(c.recv(), FromRelay::Event { sub: "s".into(), event: e.clone() });
    assert_eq!(c.next(), Next::Timeout, "open, but no EOSE");

    // Garbage: a frame that is not Nostr.
    relay.set_misbehaviour(Misbehaviour::Garbage);
    let mut c = Client::connect_timeout(&relay, Duration::from_millis(500));
    c.send(&ToRelay::Close { sub: "s".into() });
    match c.0.read().unwrap() {
        Message::Text(t) => assert!(decode_from_relay(t.as_str()).is_err()),
        other => panic!("{other:?}"),
    }

    // DropAfter(1): the first frame is served, then the connection closes.
    relay.set_misbehaviour(Misbehaviour::DropAfter(1));
    let mut c = Client::connect_timeout(&relay, Duration::from_millis(500));
    c.send(&ToRelay::Req { sub: "s".into(), filters: vec![kinds(1)] });
    assert_eq!(c.recv(), FromRelay::Event { sub: "s".into(), event: e.clone() });
    assert_eq!(c.recv(), FromRelay::Eose { sub: "s".into() });
    assert_eq!(c.next(), Next::Closed);

    // Slow: the answer comes late, but comes.
    relay.set_misbehaviour(Misbehaviour::Slow(Duration::from_millis(200)));
    let mut c = Client::connect_timeout(&relay, Duration::from_secs(5));
    let t = std::time::Instant::now();
    assert_eq!(c.query("s", vec![kinds(1)]), vec![e]);
    assert!(t.elapsed() >= Duration::from_millis(200));

    relay.set_misbehaviour(Misbehaviour::Honest);
    assert_eq!(Client::connect(&relay).query("s", vec![kinds(9)]), vec![]);
}

#[test]
fn refusing_forgetting_and_not_forwarding() {
    let relay = TestRelay::start();
    relay.set_time(1000);
    let mut c = Client::connect(&relay);
    let note = announce(1, 1000, "note");
    let hs = ev(1, 1000, KIND_HANDSHAKE, &[], "offer");

    // Refuse: every event, with the given message; nothing stored.
    relay.set_misbehaviour(Misbehaviour::Refuse("restricted: pay first"));
    assert_eq!(c.publish(&note), (false, "restricted: pay first".to_string()));
    assert_eq!(c.publish(&hs), (false, "restricted: pay first".to_string()));
    assert!(relay.events().is_empty());

    // RefuseKind: that kind only.
    relay.set_misbehaviour(Misbehaviour::RefuseKind(KIND_ANNOUNCE));
    let (accepted, message) = c.publish(&note);
    assert!(!accepted && message.starts_with("blocked:"), "{message}");
    assert_eq!(c.publish(&ev(1, 1000, 1, &[], "text")), (true, String::new()));

    // NoForward: an ephemeral event is accepted and reaches nobody.
    relay.set_misbehaviour(Misbehaviour::NoForward);
    let mut listener = Client::connect(&relay);
    assert!(listener.subscribe("hs", vec![kinds(KIND_HANDSHAKE)]).is_empty());
    assert_eq!(c.publish(&hs), (true, String::new()));
    assert!(listener.query("after", vec![kinds(2)]).is_empty(), "a forwarded event would precede this EOSE");

    // KeepFor: served until `created_at` + 60 on the relay's clock.
    relay.set_misbehaviour(Misbehaviour::KeepFor(60));
    assert!(c.publish(&note).0);
    assert_eq!(c.query("a", vec![kinds(KIND_ANNOUNCE)]), vec![note.clone()]);
    relay.set_time(1059);
    assert_eq!(c.query("b", vec![kinds(KIND_ANNOUNCE)]), vec![note.clone()]);
    relay.set_time(1060);
    assert!(c.query("c", vec![kinds(KIND_ANNOUNCE)]).is_empty());
    relay.set_misbehaviour(Misbehaviour::Honest);
    assert_eq!(c.query("d", vec![kinds(KIND_ANNOUNCE)]), vec![note]);
}

#[test]
fn garbage_input_is_answered_not_fatal() {
    let relay = TestRelay::start();
    let mut c = Client::connect(&relay);
    for junk in ["nope", "[]", "[\"AUTH\",1]", "[\"EVENT\",{\"id\":1}]"] {
        c.0.send(Message::text(junk)).unwrap();
        assert!(matches!(c.recv(), FromRelay::Notice(m) if m.starts_with("error")), "{junk}");
    }
    // The relay closes as soon as it reads the frame's length, which may be
    // before this side wrote all of it: then the send itself finds the
    // connection closed (a broken pipe under load), which is the same
    // state, so it is waited for by reading, not by the send's result.
    match c.0.send(Message::text("x".repeat(MAX_FRAME + 10))) {
        Ok(()) => {}
        Err(tungstenite::Error::Io(e))
            if matches!(
                e.kind(),
                ErrorKind::BrokenPipe | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
            ) => {}
        Err(e) => panic!("the oversized frame's send: {e}"),
    }
    assert_eq!(c.next(), Next::Closed, "an oversized frame closes the connection");
    // The relay itself lives on.
    assert!(Client::connect(&relay).query("s", vec![kinds(1)]).is_empty());
}

#[test]
fn dropping_the_relay_closes_the_port() {
    let relay = TestRelay::start();
    let port = relay.port();
    let mut c = Client::connect(&relay);
    assert!(c.query("s", vec![kinds(1)]).is_empty());
    let t = std::time::Instant::now();
    drop(relay);
    assert!(t.elapsed() < Duration::from_secs(2), "shutdown is bounded: {:?}", t.elapsed());
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
    assert_eq!(c.next(), Next::Closed, "open connections are closed");
}

#[test]
fn drop_ephemeral_drops_the_first_ones_then_forwards() {
    let relay = TestRelay::start();
    relay.set_time(1000);
    let mut listener = Client::connect(&relay);
    let mut sender = Client::connect(&relay);
    assert!(listener.subscribe("hs", vec![kinds(KIND_HANDSHAKE)]).is_empty());
    relay.set_misbehaviour(Misbehaviour::DropEphemeral(2));
    for i in 0..2 {
        let lost = ev(1, 1000 + i, KIND_HANDSHAKE, &[], &format!("lost {i}"));
        assert_eq!(sender.publish(&lost), (true, String::new()), "accepted, then dropped");
    }
    // Stored kinds are not counted.
    assert!(sender.publish(&ev(1, 1000, 1, &[], "stored")).0);
    assert!(listener.query("after", vec![kinds(2)]).is_empty(), "a forwarded event would precede this EOSE");
    let third = ev(1, 1003, KIND_HANDSHAKE, &[], "third");
    assert_eq!(sender.publish(&third), (true, String::new()));
    assert_eq!(listener.recv(), FromRelay::Event { sub: "hs".into(), event: third });
}

#[test]
fn start_on_listens_where_asked() {
    let relay = TestRelay::start_on("127.0.0.1:0".parse().unwrap()).unwrap();
    assert_eq!(relay.url(), format!("ws://127.0.0.1:{}", relay.port()));
    assert!(Client::connect(&relay).query("s", vec![kinds(1)]).is_empty());
    // Another loopback address.
    let other = TestRelay::start_on("127.0.0.2:0".parse().unwrap()).unwrap();
    assert_eq!(other.url(), format!("ws://127.0.0.2:{}", other.port()));
    let s = TcpStream::connect(("127.0.0.2", other.port())).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let (mut ws, _) = tungstenite::client(other.url().as_str(), s).unwrap();
    ws.send(Message::text(encode_to_relay(&ToRelay::Req { sub: "s".into(), filters: vec![kinds(1)] })))
        .unwrap();
    let eose = loop {
        if let Message::Text(t) = ws.read().unwrap() {
            break decode_from_relay(t.as_str()).unwrap();
        }
    };
    assert_eq!(eose, FromRelay::Eose { sub: "s".into() });
}
