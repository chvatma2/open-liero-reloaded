//! An in-process Nostr relay for tests (feature `test-relay`): `ws://` and
//! `wss://` (a self-signed rcgen certificate). Every test that needs a relay
//! uses this one, so `cargo xtask check` never touches the public internet.
//!
//! NIP-01 as far as discovery needs it: events are verified and stored;
//! replaceable and addressable kinds replace; ephemeral kinds (20000-29999)
//! are forwarded to live subscriptions and never stored; NIP-40 expiry is
//! honoured on read; `REQ` serves `kinds`, `authors`, `#x` tags, `since` and
//! `limit`, then `EOSE`; every `EVENT` gets an `OK`; `CLOSE` ends a
//! subscription. One thread per connection; dropping the relay stops it.
//!
//! Time: the relay's clock is the system clock unless a test sets one with
//! [`TestRelay::set_time`], which keeps expiry tests deterministic.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use crate::nostr::{Event, Filter, FromRelay, MAX_FRAME, ToRelay, decode_to_relay, encode_from_relay};

/// How the relay misbehaves, for the client's failure paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Misbehaviour {
    /// No misbehaviour.
    #[default]
    Honest,
    /// Serves stored events but never sends `EOSE`.
    NeverEose,
    /// Answers every frame with a garbage text frame and nothing else.
    Garbage,
    /// Handles this many frames per connection, then closes it (`0`: closes
    /// at the first frame).
    DropAfter(usize),
    /// Waits this long before handling each frame.
    Slow(Duration),
}

struct Sub {
    conn: u64,
    id: String,
    filters: Vec<Filter>,
    tx: Sender<String>,
}

struct State {
    events: Vec<Event>,
    subs: Vec<Sub>,
    misbehaviour: Misbehaviour,
    now: Option<u64>,
    next_conn: u64,
    conns: Vec<JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn tag_is(tag: &[String], name: char) -> bool {
    tag.first().is_some_and(|n| {
        let mut c = n.chars();
        c.next() == Some(name) && c.next().is_none()
    })
}

fn matches(f: &Filter, e: &Event) -> bool {
    (f.kinds.is_empty() || f.kinds.contains(&e.kind))
        && (f.authors.is_empty() || f.authors.contains(&e.pubkey))
        && f.since.is_none_or(|s| e.created_at >= s)
        && f.tags.iter().all(|(c, vals)| {
            e.tags.iter().any(|t| tag_is(t, *c) && t.get(1).is_some_and(|v| vals.contains(v)))
        })
}

fn matches_any(filters: &[Filter], e: &Event) -> bool {
    filters.iter().any(|f| matches(f, e))
}

fn expired(e: &Event, now: u64) -> bool {
    e.expiration().is_some_and(|x| x <= now)
}

/// Newest first, ties by lowest id (NIP-01).
fn newer(a: &Event, b: &Event) -> bool {
    (a.created_at, std::cmp::Reverse(a.id)) > (b.created_at, std::cmp::Reverse(b.id))
}

fn replaceable(kind: u32) -> bool {
    kind == 0 || kind == 3 || (10_000..20_000).contains(&kind)
}

fn addressable(kind: u32) -> bool {
    (30_000..40_000).contains(&kind)
}

fn same_slot(a: &Event, b: &Event) -> bool {
    if replaceable(a.kind) {
        a.pubkey == b.pubkey && a.kind == b.kind
    } else if addressable(a.kind) {
        a.pubkey == b.pubkey && a.kind == b.kind && a.tag("d").unwrap_or("") == b.tag("d").unwrap_or("")
    } else {
        a.id == b.id
    }
}

impl State {
    fn clock(&self) -> u64 {
        self.now
            .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
    }

    /// Stores (or forwards) an event; the `OK` verdict and message.
    fn publish(&mut self, ev: Event) -> (bool, String) {
        if !ev.verify() {
            return (false, "invalid: bad id or signature".into());
        }
        let now = self.clock();
        if expired(&ev, now) {
            return (false, "invalid: event expired".into());
        }
        if !(20_000..30_000).contains(&ev.kind) {
            if let Some(i) = self.events.iter().position(|old| same_slot(&ev, old)) {
                let regular = !replaceable(ev.kind) && !addressable(ev.kind);
                if regular || !newer(&ev, &self.events[i]) {
                    return (true, "duplicate: already have this event or a newer one".into());
                }
                self.events.remove(i);
            }
            self.events.push(ev.clone());
        }
        for s in &self.subs {
            if matches_any(&s.filters, &ev) {
                let text = encode_from_relay(&FromRelay::Event { sub: s.id.clone(), event: ev.clone() });
                let _ = s.tx.send(text);
            }
        }
        (true, String::new())
    }

    /// The stored, unexpired events a REQ selects: per filter the newest
    /// `limit`, the union newest first.
    fn query(&self, filters: &[Filter]) -> Vec<Event> {
        let now = self.clock();
        let mut out: Vec<Event> = Vec::new();
        for f in filters {
            let mut hits: Vec<&Event> =
                self.events.iter().filter(|e| !expired(e, now) && matches(f, e)).collect();
            hits.sort_by(|a, b| (b.created_at, a.id).cmp(&(a.created_at, b.id)));
            hits.truncate(f.limit.map_or(usize::MAX, |l| l as usize));
            for e in hits {
                if !out.iter().any(|o| o.id == e.id) {
                    out.push(e.clone());
                }
            }
        }
        out.sort_by(|a, b| (b.created_at, a.id).cmp(&(a.created_at, b.id)));
        out
    }
}

/// An in-process relay: dropped = stopped.
pub struct TestRelay {
    shared: Arc<Shared>,
    port: u16,
    tls: bool,
    accept: Option<JoinHandle<()>>,
}

impl TestRelay {
    /// A `ws://127.0.0.1:<port>` relay.
    pub fn start() -> TestRelay {
        TestRelay::spawn(None)
    }

    /// A `wss://localhost:<port>` relay with a fresh self-signed
    /// certificate; the second value is its DER, the root to trust. The
    /// relay listens on `127.0.0.1` only.
    pub fn start_tls() -> (TestRelay, Vec<u8>) {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
                .expect("self-signed certificate");
        let der = cert.der().to_vec();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], key.into())
        .expect("server certificate");
        let relay = TestRelay::spawn(Some(Arc::new(config)));
        (relay, der)
    }

    fn spawn(tls: Option<Arc<rustls::ServerConfig>>) -> TestRelay {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the test relay");
        listener.set_nonblocking(true).expect("nonblocking listener");
        let port = listener.local_addr().expect("local address").port();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                events: Vec::new(),
                subs: Vec::new(),
                misbehaviour: Misbehaviour::Honest,
                now: None,
                next_conn: 0,
                conns: Vec::new(),
            }),
            stop: AtomicBool::new(false),
        });
        let is_tls = tls.is_some();
        let accept = {
            let shared = Arc::clone(&shared);
            thread::spawn(move || accept_loop(&shared, &listener, tls.as_ref()))
        };
        TestRelay { shared, port, tls: is_tls, accept: Some(accept) }
    }

    /// The relay's URL: `ws://127.0.0.1:<port>` or `wss://localhost:<port>`.
    pub fn url(&self) -> String {
        if self.tls {
            format!("wss://localhost:{}", self.port)
        } else {
            format!("ws://127.0.0.1:{}", self.port)
        }
    }

    /// The listening port on `127.0.0.1`.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The stored events (never the ephemeral ones), oldest first.
    pub fn events(&self) -> Vec<Event> {
        self.shared.lock().events.clone()
    }

    /// Makes the relay misbehave from now on, on every connection.
    pub fn set_misbehaviour(&self, m: Misbehaviour) {
        self.shared.lock().misbehaviour = m;
    }

    /// Sets the relay's clock (unix seconds) for expiry; the default is the
    /// system clock.
    pub fn set_time(&self, unix: u64) {
        self.shared.lock().now = Some(unix);
    }
}

fn join_bounded(h: JoinHandle<()>, deadline: Instant) {
    while !h.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    if h.is_finished() {
        let _ = h.join();
    }
    // A thread stuck past the bound is detached.
}

impl Drop for TestRelay {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(2);
        // The accept thread owns the listener: joining it closes the port.
        if let Some(h) = self.accept.take() {
            join_bounded(h, deadline);
        }
        let conns = std::mem::take(&mut self.shared.lock().conns);
        for h in conns {
            join_bounded(h, deadline);
        }
    }
}

fn accept_loop(shared: &Arc<Shared>, listener: &TcpListener, tls: Option<&Arc<rustls::ServerConfig>>) {
    while !shared.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let (shared2, tls2) = (Arc::clone(shared), tls.cloned());
                let mut st = shared.lock();
                let conn = st.next_conn;
                st.next_conn += 1;
                st.conns.retain(|h| !h.is_finished());
                st.conns.push(thread::spawn(move || serve(&shared2, conn, stream, tls2.as_ref())));
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(3)),
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

/// Handshakes (TLS lazily, inside the WebSocket handshake) and serves.
fn serve(shared: &Shared, conn: u64, stream: TcpStream, tls: Option<&Arc<rustls::ServerConfig>>) {
    let Ok(ctl) = stream.try_clone() else { return };
    let _ = stream.set_nodelay(true);
    let _ = ctl.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = ctl.set_write_timeout(Some(Duration::from_secs(5)));
    let config = WebSocketConfig::default().max_message_size(Some(MAX_FRAME)).max_frame_size(Some(MAX_FRAME));
    match tls {
        None => {
            if let Ok(ws) = tungstenite::accept_with_config(stream, Some(config)) {
                run(shared, conn, ws, &ctl);
            }
        }
        Some(cfg) => {
            let Ok(sc) = rustls::ServerConnection::new(Arc::clone(cfg)) else { return };
            let stream = rustls::StreamOwned::new(sc, stream);
            if let Ok(ws) = tungstenite::accept_with_config(stream, Some(config)) {
                run(shared, conn, ws, &ctl);
            }
        }
    }
}

fn send<S: Read + Write>(ws: &mut WebSocket<S>, text: String) -> bool {
    ws.send(Message::text(text)).is_ok()
}

fn run<S: Read + Write>(shared: &Shared, conn: u64, mut ws: WebSocket<S>, ctl: &TcpStream) {
    // Short read timeouts let the loop forward live events and notice `stop`.
    let _ = ctl.set_read_timeout(Some(Duration::from_millis(15)));
    let (tx, rx): (Sender<String>, Receiver<String>) = mpsc::channel();
    let mut frames = 0usize;
    'conn: while !shared.stop.load(Ordering::SeqCst) {
        while let Ok(text) = rx.try_recv() {
            if !send(&mut ws, text) {
                break 'conn;
            }
        }
        match ws.read() {
            Ok(Message::Text(t)) => {
                frames += 1;
                if !handle(shared, conn, &mut ws, &tx, t.as_str(), frames) {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
    shared.lock().subs.retain(|s| s.conn != conn);
    let _ = ws.close(None);
    let _ = ws.flush();
}

/// Handles one frame; false closes the connection.
fn handle<S: Read + Write>(
    shared: &Shared,
    conn: u64,
    ws: &mut WebSocket<S>,
    tx: &Sender<String>,
    text: &str,
    frames: usize,
) -> bool {
    let mis = shared.lock().misbehaviour;
    match mis {
        Misbehaviour::DropAfter(n) if frames > n => return false,
        Misbehaviour::Slow(d) => thread::sleep(d),
        Misbehaviour::Garbage => return send(ws, "\u{1}garbage [[{ not nostr".into()),
        _ => {}
    }
    let keep = match decode_to_relay(text) {
        Err(e) => send(ws, encode_from_relay(&FromRelay::Notice(format!("error: {e}")))),
        Ok(ToRelay::Event(ev)) => {
            let id = ev.id;
            let (accepted, message) = shared.lock().publish(ev);
            send(ws, encode_from_relay(&FromRelay::Ok { id, accepted, message }))
        }
        Ok(ToRelay::Req { sub, filters }) => {
            let stored = {
                let mut st = shared.lock();
                st.subs.retain(|s| !(s.conn == conn && s.id == sub));
                let stored = st.query(&filters);
                st.subs.push(Sub { conn, id: sub.clone(), filters, tx: tx.clone() });
                stored
            };
            let mut ok = true;
            for event in stored {
                ok &= send(ws, encode_from_relay(&FromRelay::Event { sub: sub.clone(), event }));
            }
            if mis != Misbehaviour::NeverEose {
                ok &= send(ws, encode_from_relay(&FromRelay::Eose { sub }));
            }
            ok
        }
        Ok(ToRelay::Close { sub }) => {
            shared.lock().subs.retain(|s| !(s.conn == conn && s.id == sub));
            true
        }
    };
    keep && !matches!(mis, Misbehaviour::DropAfter(n) if frames >= n)
}
