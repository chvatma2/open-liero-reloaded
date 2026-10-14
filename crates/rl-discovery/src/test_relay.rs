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
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
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
    /// Stops reading and writing (pings go unanswered); a connection made
    /// meanwhile is held without a handshake.
    Silent,
    /// Writes one byte every 40 ms and never completes: a connection made
    /// meanwhile gets an endless handshake response (on the TCP stream,
    /// under any TLS), an established one an endless 64 KiB text frame.
    /// Nothing is read meanwhile.
    Drip,
    /// Reads the socket at most 1 KiB at a time, pausing 1 ms every 8
    /// reads (under any TLS): a peer that accepts writes slowly.
    SlowReads,
    /// Refuses every event with this `OK` message, such as a paid relay's
    /// `"restricted: …"` or `"auth-required: …"`; nothing is stored or
    /// forwarded.
    Refuse(&'static str),
    /// Refuses events of this kind (`"blocked: kind … not accepted"`).
    RefuseKind(u32),
    /// Serves a stored event only while the relay's clock is less than this
    /// many seconds past its `created_at` (a relay that drops notes).
    KeepFor(u64),
    /// Accepts ephemeral events but forwards them to nobody.
    NoForward,
    /// At the first frame it reads, writes half of a 256-byte text frame
    /// and closes the connection (a relay that dies mid-frame).
    HalfFrame,
    /// Accepts the first this many ephemeral events (counted from when it
    /// is set) and forwards them to nobody, then forwards again (a relay
    /// losing the first offers).
    DropEphemeral(u32),
}

/// The relay's side of a TCP connection: under [`Misbehaviour::SlowReads`]
/// it reads in small chunks with pauses.
struct Trickle<'a> {
    tcp: TcpStream,
    shared: &'a Shared,
    reads: u32,
}

impl Read for Trickle<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.shared.lock().misbehaviour != Misbehaviour::SlowReads {
            return self.tcp.read(buf);
        }
        self.reads = self.reads.wrapping_add(1);
        if self.reads.is_multiple_of(8) {
            thread::sleep(Duration::from_millis(1));
        }
        let n = buf.len().min(1024);
        self.tcp.read(&mut buf[..n])
    }
}

impl Write for Trickle<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

const DRIP_EVERY: Duration = Duration::from_millis(40);

/// Writes `head`, then `a`s, one byte every [`DRIP_EVERY`], until the relay
/// stops or the peer goes away.
fn drip<W: Write>(shared: &Shared, w: &mut W, head: &[u8]) {
    for b in head.iter().copied().chain(std::iter::repeat(b'a')) {
        if shared.stop.load(Ordering::SeqCst) || w.write_all(&[b]).and_then(|()| w.flush()).is_err() {
            return;
        }
        thread::sleep(DRIP_EVERY);
    }
}

/// Holds a connection, doing nothing, while the relay is `Silent`.
fn hold(shared: &Shared) {
    while !shared.stop.load(Ordering::SeqCst) && shared.lock().misbehaviour == Misbehaviour::Silent {
        thread::sleep(Duration::from_millis(15));
    }
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
    /// Ephemeral events dropped under `DropEphemeral`.
    dropped_ephemeral: u32,
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

fn matches_any(filters: &[Filter], e: &Event) -> bool {
    filters.iter().any(|f| f.matches(e))
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
        match self.misbehaviour {
            Misbehaviour::Refuse(message) => return (false, message.into()),
            Misbehaviour::RefuseKind(k) if k == ev.kind => {
                return (false, format!("blocked: kind {k} not accepted"));
            }
            Misbehaviour::NoForward if (20_000..30_000).contains(&ev.kind) => return (true, String::new()),
            Misbehaviour::DropEphemeral(n)
                if (20_000..30_000).contains(&ev.kind) && self.dropped_ephemeral < n =>
            {
                self.dropped_ephemeral += 1;
                return (true, String::new());
            }
            _ => {}
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
        let kept = |e: &Event| match self.misbehaviour {
            Misbehaviour::KeepFor(secs) => e.created_at.saturating_add(secs) > now,
            _ => true,
        };
        let mut out: Vec<Event> = Vec::new();
        for f in filters {
            let mut hits: Vec<&Event> =
                self.events.iter().filter(|e| !expired(e, now) && kept(e) && f.matches(e)).collect();
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
    ip: IpAddr,
    port: u16,
    tls: bool,
    accept: Option<JoinHandle<()>>,
}

impl TestRelay {
    /// A `ws://127.0.0.1:<port>` relay.
    pub fn start() -> TestRelay {
        TestRelay::start_on(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
    }

    /// A `ws://` relay listening on `addr` (port 0: any free port), such as
    /// `0.0.0.0:7777` in a Docker network (`rl-cli nostr-relay`).
    pub fn start_on(addr: SocketAddr) -> TestRelay {
        TestRelay::spawn(addr, None)
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
        let relay = TestRelay::spawn(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Some(Arc::new(config)));
        (relay, der)
    }

    fn spawn(addr: SocketAddr, tls: Option<Arc<rustls::ServerConfig>>) -> TestRelay {
        let listener = TcpListener::bind(addr).expect("bind the test relay");
        listener.set_nonblocking(true).expect("nonblocking listener");
        let local = listener.local_addr().expect("local address");
        let port = local.port();
        // A relay on every interface is reached locally through loopback.
        let ip = match local.ip() {
            IpAddr::V4(a) if a.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(a) if a.is_unspecified() => IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
            ip => ip,
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                events: Vec::new(),
                subs: Vec::new(),
                misbehaviour: Misbehaviour::Honest,
                dropped_ephemeral: 0,
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
        TestRelay { shared, ip, port, tls: is_tls, accept: Some(accept) }
    }

    /// The relay's URL: `ws://<address>:<port>` (`127.0.0.1` for
    /// [`TestRelay::start`], loopback for an unspecified address) or
    /// `wss://localhost:<port>`.
    pub fn url(&self) -> String {
        if self.tls {
            format!("wss://localhost:{}", self.port)
        } else {
            format!("ws://{}", SocketAddr::from((self.ip, self.port)))
        }
    }

    /// The listening port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The stored events (never the ephemeral ones), oldest first.
    pub fn events(&self) -> Vec<Event> {
        self.shared.lock().events.clone()
    }

    /// Makes the relay misbehave from now on, on every connection.
    pub fn set_misbehaviour(&self, m: Misbehaviour) {
        let mut st = self.shared.lock();
        st.misbehaviour = m;
        st.dropped_ephemeral = 0;
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
fn serve(shared: &Shared, conn: u64, mut stream: TcpStream, tls: Option<&Arc<rustls::ServerConfig>>) {
    // Copied out first: the guard must not live through the match.
    let mis = shared.lock().misbehaviour;
    match mis {
        Misbehaviour::Drip => {
            return drip(shared, &mut stream, b"HTTP/1.1 101 Switching Protocols\r\nX-Drip: ");
        }
        Misbehaviour::Silent => return hold(shared),
        _ => {}
    }
    let Ok(ctl) = stream.try_clone() else { return };
    let _ = stream.set_nodelay(true);
    let _ = ctl.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = ctl.set_write_timeout(Some(Duration::from_secs(5)));
    let config = WebSocketConfig::default().max_message_size(Some(MAX_FRAME)).max_frame_size(Some(MAX_FRAME));
    let stream = Trickle { tcp: stream, shared, reads: 0 };
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
        let mis = shared.lock().misbehaviour;
        match mis {
            Misbehaviour::Silent => {
                hold(shared);
                continue;
            }
            // A server's text frame (unmasked) of 65535 bytes, dripped.
            // (No close frame after it: the stream is beyond repair.)
            Misbehaviour::Drip => {
                drip(shared, ws.get_mut(), &[0x81, 126, 0xff, 0xff]);
                shared.lock().subs.retain(|s| s.conn != conn);
                return;
            }
            _ => {}
        }
        while let Ok(text) = rx.try_recv() {
            if !send(&mut ws, text) {
                break 'conn;
            }
        }
        match ws.read() {
            Ok(Message::Text(_)) if mis == Misbehaviour::HalfFrame => {
                let w = ws.get_mut();
                let _ = w.write_all(&[0x81, 126, 0x01, 0x00]);
                let _ = w.write_all(&[b'['; 100]);
                let _ = w.flush();
                let _ = ctl.shutdown(std::net::Shutdown::Both);
                shared.lock().subs.retain(|s| s.conn != conn);
                return;
            }
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
