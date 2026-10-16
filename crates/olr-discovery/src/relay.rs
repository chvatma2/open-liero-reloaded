//! One relay connection: the driver thread behind each of the [`Pool`]'s
//! relays, over `ws://` or `wss://` (tungstenite with rustls).
//!
//! The driver connects (5 s per address, IPv4 first), handshakes (5 s in
//! all), then serves the connection: it sends the pool's subscriptions
//! (again after every reconnect), its queued events, and hands verified
//! events, `EOSE` and `OK` back through the shared [`Link`]. Limits drop
//! the connection: a frame above `max_frame`, a sustained rate above
//! 200 frames/s or 1 MiB/s, a frame that is not a Nostr message, no `EOSE`
//! within 10 s of a `REQ`, nothing at all within 10 s of a ping (sent
//! after 30 s of silence), a send not done within 5 s. A missing `OK` is
//! reported as not accepted, and the connection kept. Failed or dropped,
//! it waits 1, 2, 4 … 60 s and tries again. (Defaults: see [`Timings`].)
//!
//! The socket is non-blocking (ruling PF12-4) behind an I/O budget
//! ([`Sock`]), so one thread both reads and writes, keeps its deadlines and
//! notices the pool going away; where nothing can move, it waits [`POLL`].
//! `WouldBlock` is the socket's ordinary answer, never a timeout, so no
//! socket is ever retried after an `SO_RCVTIMEO`/`SO_SNDTIMEO` expiry
//! (whose state Windows leaves undefined). A partial read or write is
//! counted by the OS as usual: rustls keeps a partial TLS record and
//! unsent records, tungstenite a partial frame and its unflushed output,
//! and both resume on the next call.
//!
//! [`Pool`]: crate::pool::Pool

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use olr_proto::PublicKey;
use tungstenite::client::IntoClientRequest;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::{MaybeTlsStream, Mode};
use tungstenite::{Connector, HandshakeError, Message, WebSocket};

use crate::nostr::{Event, Filter, FromRelay, ToRelay, decode_from_relay, encode_to_relay};
use crate::pool::{PoolEvent, RelayStatus, Timings};

/// The read budget of one turn of the loop: a relay streaming without
/// pause is read for this long before the queue and deadlines are seen to.
pub(crate) const TICK: Duration = Duration::from_millis(50);
/// The wait when the socket cannot move (nothing to read, or its send
/// buffer full), before trying again.
const POLL: Duration = Duration::from_millis(10);
/// TCP connect timeout, per address.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The sends of one turn of the loop share this deadline; past it, the
/// connection is dropped.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// Stopping, what is queued gets this long to go out.
const STOP_FLUSH: Duration = Duration::from_secs(1);
const BACKOFF_FIRST: Duration = Duration::from_secs(1);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// A connection that stayed up this long resets the backoff; one dropped
/// sooner (say, at every first frame) keeps doubling it.
const HEALTHY: Duration = Duration::from_secs(60);

/// Outgoing frames queued per relay; beyond, the oldest are dropped.
pub(crate) const MAX_OUTBOX: usize = 256;
/// Incoming events held per relay between polls; beyond, the newest are
/// dropped and counted.
pub(crate) const MAX_INBOX_EVENTS: usize = 4096;
/// The same bound in bytes (frame sizes), so 4096 large events cannot
/// pile up hundreds of MiB when nobody polls.
pub(crate) const MAX_INBOX_BYTES: usize = 8 << 20;

/// Sustained incoming rates; bursts of [`BURST`] times the rate pass (the
/// stored events answering a `REQ` arrive at once).
const FRAMES_PER_SEC: f64 = 200.0;
const BYTES_PER_SEC: f64 = 1024.0 * 1024.0;
const BURST: f64 = 5.0;

/// The slot a replaceable or addressable event occupies: a newer one
/// published for the same slot replaces a queued older one.
pub(crate) type Slot = (PublicKey, u32, String);

/// A queued `EVENT` frame.
pub(crate) struct Outgoing {
    pub slot: Option<Slot>,
    pub id: [u8; 32],
    pub frame: String,
}

/// What a pool and one driver share.
pub(crate) struct LinkState {
    pub status: RelayStatus,
    outbox: VecDeque<Outgoing>,
    subs: BTreeMap<String, Vec<Filter>>,
    /// Bumped at every change of `subs`, so the driver re-reads them.
    subs_gen: u64,
    inbox: VecDeque<PoolEvent>,
    inbox_events: usize,
    inbox_bytes: usize,
    pub dropped: u64,
    /// The relay's clock minus this machine's, in seconds, from its last
    /// handshake's `Date` header (FR12b-3).
    pub clock: Option<i64>,
}

pub(crate) struct Link {
    pub index: usize,
    pub url: String,
    state: Mutex<LinkState>,
    stop: AtomicBool,
    budget: Mutex<Budget>,
}

/// The current connection's I/O budget ([`Sock`]).
#[derive(Clone, Copy)]
struct Budget {
    read_until: Instant,
    write_until: Instant,
}

impl Link {
    pub fn new(index: usize, url: String) -> Link {
        Link {
            index,
            url,
            state: Mutex::new(LinkState {
                status: RelayStatus::Connecting,
                outbox: VecDeque::new(),
                subs: BTreeMap::new(),
                subs_gen: 0,
                inbox: VecDeque::new(),
                inbox_events: 0,
                inbox_bytes: 0,
                clock: None,
                dropped: 0,
            }),
            stop: AtomicBool::new(false),
            budget: Mutex::new(Budget { read_until: Instant::now(), write_until: Instant::now() }),
        }
    }

    fn budget(&self) -> Budget {
        *self.budget.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Re-arms the I/O budget: reads until `read_until`, writes until
    /// `write_until`.
    fn arm(&self, read_until: Instant, write_until: Instant) {
        *self.budget.lock().unwrap_or_else(PoisonError::into_inner) = Budget { read_until, write_until };
    }

    /// Re-arms the read budget only.
    fn arm_read(&self, read_until: Instant) {
        self.budget.lock().unwrap_or_else(PoisonError::into_inner).read_until = read_until;
    }

    fn write_expired(&self) -> bool {
        Instant::now() >= self.budget().write_until
    }

    pub fn lock(&self) -> MutexGuard<'_, LinkState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Queues an event: a queued one for the same slot is replaced, and a
    /// full queue drops its oldest.
    pub fn queue(&self, out: Outgoing) {
        let mut st = self.lock();
        if out.slot.is_some() {
            st.outbox.retain(|o| o.slot != out.slot);
        }
        if st.outbox.len() >= MAX_OUTBOX {
            st.outbox.pop_front();
        }
        st.outbox.push_back(out);
    }

    pub fn set_sub(&self, sub: &str, filters: Option<Vec<Filter>>) {
        let mut st = self.lock();
        match filters {
            Some(f) => {
                st.subs.insert(sub.to_owned(), f);
            }
            None => {
                st.subs.remove(sub);
            }
        }
        st.subs_gen += 1;
    }

    /// Moves everything received since the last call to `out`, in order.
    pub fn drain(&self, out: &mut Vec<PoolEvent>) {
        let mut st = self.lock();
        out.extend(st.inbox.drain(..));
        st.inbox_events = 0;
        st.inbox_bytes = 0;
    }

    pub fn set_status(&self, status: RelayStatus) {
        let mut st = self.lock();
        st.status = status.clone();
        st.inbox.push_back(PoolEvent::Status { relay: self.index, status });
    }

    fn push(&self, ev: PoolEvent) {
        self.lock().inbox.push_back(ev);
    }

    fn push_event(&self, event: Event, size: usize) {
        let mut st = self.lock();
        if st.inbox_events >= MAX_INBOX_EVENTS || st.inbox_bytes + size > MAX_INBOX_BYTES {
            st.dropped += 1;
            return;
        }
        st.inbox_events += 1;
        st.inbox_bytes += size;
        st.inbox.push_back(PoolEvent::Event { relay: self.index, event });
    }
}

/// What every driver of a pool uses.
#[derive(Clone)]
pub(crate) struct DriverConfig {
    /// `None` if rustls could not be configured: `wss://` relays fail.
    pub tls: Option<Arc<rustls::ClientConfig>>,
    pub max_frame: usize,
    pub timings: Timings,
}

/// The reconnect delays: 1, 2, 4 … 60 s, each ±20 %.
#[derive(Debug)]
pub(crate) struct Backoff {
    next: Duration,
}

impl Backoff {
    pub fn new() -> Backoff {
        Backoff { next: BACKOFF_FIRST }
    }

    /// The next delay, jittered by up to 20 % either way so the clients
    /// of one build do not all reconnect to a recovering relay at once.
    pub fn next_delay(&mut self) -> Duration {
        let d = self.next;
        self.next = (d * 2).min(BACKOFF_MAX);
        use std::hash::{BuildHasher, Hasher, RandomState};
        let mut h = RandomState::new().build_hasher();
        h.write_u64(d.as_nanos() as u64);
        jittered(d, h.finish())
    }

    pub fn reset(&mut self) {
        self.next = BACKOFF_FIRST;
    }
}

/// `d` scaled by 0.8 to 1.2 as `r` goes from 0 to `u64::MAX`.
fn jittered(d: Duration, r: u64) -> Duration {
    let f = 0.8 + 0.4 * (r as f64 / u64::MAX as f64);
    d.mul_f64(f)
}

/// Token buckets for the incoming frame and byte rates.
#[derive(Debug)]
pub(crate) struct Limiter {
    frames: f64,
    bytes: f64,
    at: Instant,
}

impl Limiter {
    pub fn new(now: Instant) -> Limiter {
        Limiter { frames: FRAMES_PER_SEC * BURST, bytes: BYTES_PER_SEC * BURST, at: now }
    }

    /// Accounts one frame of `len` bytes received at `now`; the error
    /// says which rate it broke.
    pub fn frame(&mut self, now: Instant, len: usize) -> Result<(), String> {
        let dt = now.saturating_duration_since(self.at).as_secs_f64();
        self.at = now;
        self.frames = (self.frames + dt * FRAMES_PER_SEC).min(FRAMES_PER_SEC * BURST) - 1.0;
        self.bytes = (self.bytes + dt * BYTES_PER_SEC).min(BYTES_PER_SEC * BURST) - len as f64;
        if self.frames < 0.0 {
            Err(format!("more than {FRAMES_PER_SEC} frames/s"))
        } else if self.bytes < 0.0 {
            Err("more than 1 MiB/s".into())
        } else {
            Ok(())
        }
    }
}

/// The driver's TCP stream, non-blocking. Reads and writes go through an
/// I/O budget the driver re-arms ([`Link::arm`]): once the read budget
/// (one [`TICK`]) is spent, or the pool is stopping, every read returns
/// `WouldBlock`; once the cumulative write deadline has passed, every
/// write does. A relay streaming or dripping without end therefore cannot
/// keep the driver inside one read or one handshake past its deadlines or
/// past a stop. tungstenite resumes an interrupted handshake, and rustls
/// stops cleanly, on `WouldBlock`.
pub(crate) struct Sock {
    tcp: TcpStream,
    link: Arc<Link>,
}

fn would_block() -> io::Error {
    io::ErrorKind::WouldBlock.into()
}

impl Read for Sock {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.link.stopping() || Instant::now() >= self.link.budget().read_until {
            return Err(would_block());
        }
        self.tcp.read(buf)
    }
}

impl Write for Sock {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.link.write_expired() {
            return Err(would_block());
        }
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

type Ws = WebSocket<MaybeTlsStream<Sock>>;

enum ConnectError {
    /// Retried after the backoff.
    Retry(String),
    /// A bad URL: never retried.
    Fatal(String),
}

/// Resolves, connects (IPv4 first) and handshakes.
fn connect(link: &Arc<Link>, cfg: &DriverConfig) -> Result<Ws, ConnectError> {
    let request = link
        .url
        .as_str()
        .into_client_request()
        .map_err(|e| ConnectError::Fatal(format!("bad relay URL: {e}")))?;
    let uri = request.uri();
    let mode = tungstenite::client::uri_mode(uri)
        .map_err(|_| ConnectError::Fatal("not a ws:// or wss:// URL".into()))?;
    let host = uri.host().ok_or_else(|| ConnectError::Fatal("no host in the relay URL".into()))?;
    let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    let port = uri.port_u16().unwrap_or(match mode {
        Mode::Plain => 80,
        Mode::Tls => 443,
    });
    let connector = match (mode, &cfg.tls) {
        (Mode::Plain, _) => Connector::Plain,
        (Mode::Tls, Some(tls)) => Connector::Rustls(Arc::clone(tls)),
        (Mode::Tls, None) => return Err(ConnectError::Fatal("TLS is unavailable".into())),
    };

    let mut addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| ConnectError::Retry(format!("cannot resolve {host}: {e}")))?
        .collect();
    addrs.sort_by_key(SocketAddr::is_ipv6); // Stable: the resolver's order within a family.
    let mut last = format!("no address for {host}");
    let mut stream = None;
    for addr in addrs {
        if link.stopping() {
            return Err(ConnectError::Retry("stopped".into()));
        }
        match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last = format!("cannot connect to {addr}: {e}"),
        }
    }
    let stream = stream.ok_or(ConnectError::Retry(last))?;
    let io = |e: io::Error| ConnectError::Retry(format!("socket: {e}"));
    stream.set_nodelay(true).map_err(io)?;
    stream.set_nonblocking(true).map_err(io)?;

    let config =
        WebSocketConfig::default().max_message_size(Some(cfg.max_frame)).max_frame_size(Some(cfg.max_frame));
    // Every round reads for at most one TICK; writes share the deadline;
    // between rounds, the driver waits for the socket.
    let deadline = Instant::now() + cfg.timings.handshake;
    link.arm(Instant::now() + TICK, deadline);
    let sock = Sock { tcp: stream, link: Arc::clone(link) };
    let mut attempt = tungstenite::client_tls_with_config(request, sock, Some(config), Some(connector));
    loop {
        match attempt {
            Ok((ws, response)) => {
                // The relay's clock against ours (its `Date`, if any).
                let date = response.headers().get("date").and_then(|v| v.to_str().ok());
                link.lock().clock = date.and_then(crate::http_date::parse).map(|relay| {
                    let ours = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs());
                    i64::try_from(relay).unwrap_or(i64::MAX).saturating_sub(i64::try_from(ours).unwrap_or(0))
                });
                return Ok(ws);
            }
            Err(HandshakeError::Failure(e)) => return Err(ConnectError::Retry(format!("handshake: {e}"))),
            Err(HandshakeError::Interrupted(mid)) => {
                if link.stopping() {
                    return Err(ConnectError::Retry("stopped".into()));
                }
                if Instant::now() >= deadline {
                    return Err(ConnectError::Retry("handshake timed out".into()));
                }
                thread::sleep(POLL);
                link.arm(Instant::now() + TICK, deadline);
                attempt = mid.handshake();
            }
        }
    }
}

/// Runs one relay until the pool stops it.
pub(crate) fn run(link: &Arc<Link>, cfg: &DriverConfig) {
    let mut backoff = Backoff::new();
    while !link.stopping() {
        link.set_status(RelayStatus::Connecting);
        match connect(link, cfg) {
            Err(ConnectError::Fatal(reason)) => {
                link.set_status(RelayStatus::Failed(reason));
                return;
            }
            Err(ConnectError::Retry(reason)) => {
                if link.stopping() {
                    return;
                }
                link.set_status(RelayStatus::Failed(reason));
            }
            Ok(mut ws) => {
                link.set_status(RelayStatus::Connected);
                let up = Instant::now();
                match Session::new(link, cfg.timings).serve(&mut ws) {
                    End::Stop => return,
                    End::Dropped(reason) => {
                        link.set_status(RelayStatus::Dropped(reason));
                        if up.elapsed() >= HEALTHY {
                            backoff.reset();
                        }
                    }
                }
            }
        }
        let until = Instant::now() + backoff.next_delay();
        while !link.stopping() && Instant::now() < until {
            thread::sleep(TICK);
        }
    }
}

enum End {
    Stop,
    Dropped(String),
}

/// About the memory an event holds: its strings' bytes and headers.
fn footprint(e: &Event) -> usize {
    let string = std::mem::size_of::<String>();
    let tags: usize = e
        .tags
        .iter()
        .map(|t| std::mem::size_of::<Vec<String>>() + t.iter().map(|v| string + v.len()).sum::<usize>())
        .sum();
    std::mem::size_of::<Event>() + e.content.len() + tags
}

/// "5 s", or "500 ms" below a whole second.
fn span(d: Duration) -> String {
    if d.subsec_nanos() == 0 { format!("{} s", d.as_secs()) } else { format!("{} ms", d.as_millis()) }
}

/// One connection's bookkeeping.
struct Session<'a> {
    link: &'a Link,
    timings: Timings,
    /// The subscriptions sent on this connection.
    sent: BTreeMap<String, Vec<Filter>>,
    subs_gen: Option<u64>,
    awaiting_eose: BTreeMap<String, Instant>,
    awaiting_ok: VecDeque<([u8; 32], Instant)>,
    limiter: Limiter,
    /// When anything last arrived, and when we pinged since.
    last_rx: Instant,
    ping_sent: Option<Instant>,
}

impl<'a> Session<'a> {
    fn new(link: &'a Link, timings: Timings) -> Session<'a> {
        let now = Instant::now();
        Session {
            link,
            timings,
            sent: BTreeMap::new(),
            subs_gen: None,
            awaiting_eose: BTreeMap::new(),
            awaiting_ok: VecDeque::new(),
            limiter: Limiter::new(now),
            last_rx: now,
            ping_sent: None,
        }
    }

    /// Sends one message within the armed write deadline. Unless
    /// `closing`, a stop interrupts it (the closing path sends what is
    /// left).
    fn send(&self, ws: &mut Ws, msg: Message, closing: bool) -> Result<(), String> {
        // On `WouldBlock`, tungstenite keeps the frame in its buffer.
        let r = ws.write(msg).and_then(|()| ws.flush());
        self.finish(ws, r, closing)
    }

    /// Flushes until `r` (the first attempt) is through: while the send
    /// buffer is full, waits [`POLL`] and flushes again, until the write
    /// deadline.
    fn finish(&self, ws: &mut Ws, mut r: tungstenite::Result<()>, closing: bool) -> Result<(), String> {
        loop {
            match r {
                Ok(()) => return Ok(()),
                Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    if self.link.write_expired() {
                        return Err("send timed out".into());
                    }
                    if !closing && self.link.stopping() {
                        return Err("stopped".into());
                    }
                }
                Err(e) => return Err(format!("send: {e}")),
            }
            thread::sleep(POLL);
            r = ws.flush();
        }
    }

    fn serve(&mut self, ws: &mut Ws) -> End {
        loop {
            let now = Instant::now();
            if self.link.stopping() {
                // Best effort, within STOP_FLUSH: what is queued (a
                // closing note) goes out first.
                self.link.arm(now, now + STOP_FLUSH);
                let queued: Vec<Outgoing> = self.link.lock().outbox.drain(..).collect();
                for o in queued {
                    if self.send(ws, Message::text(o.frame), true).is_err() {
                        break;
                    }
                }
                let closed = ws.close(None);
                let _ = self.finish(ws, closed, true);
                return End::Stop;
            }
            // Sends first (no reading meanwhile), then one TICK of reading.
            self.link.arm(now, now + WRITE_TIMEOUT);
            if let Err(reason) = self.send_pending(ws) {
                if self.link.stopping() {
                    continue;
                }
                return End::Dropped(reason);
            }
            if let Err(reason) = self.check_deadlines(ws) {
                if self.link.stopping() {
                    continue;
                }
                return End::Dropped(reason);
            }
            self.link.arm_read(Instant::now() + TICK);
            let result = match ws.read() {
                Ok(m) => {
                    self.last_rx = Instant::now();
                    self.ping_sent = None;
                    match m {
                        Message::Text(t) => self.limit(t.len()).and_then(|()| self.handle(t.as_str())),
                        Message::Binary(b) | Message::Ping(b) | Message::Pong(b) => self.limit(b.len()),
                        Message::Close(_) => Err("closed by the relay".into()),
                        Message::Frame(_) => Ok(()),
                    }
                }
                // Nothing (more) to read: wait for the socket. A burst is
                // drained at full speed, without this wait.
                Err(tungstenite::Error::Io(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(POLL);
                    Ok(())
                }
                Err(tungstenite::Error::Capacity(e)) => Err(format!("frame too large: {e}")),
                Err(e) => Err(e.to_string()),
            };
            if let Err(reason) = result {
                return End::Dropped(reason);
            }
        }
    }

    /// The EOSE window (dropping), the OK window (reported, the connection
    /// kept) and liveness: a ping after `idle_ping` of silence, dropping if
    /// nothing at all arrives within `ping_answer` of it.
    fn check_deadlines(&mut self, ws: &mut Ws) -> Result<(), String> {
        let now = Instant::now();
        let t = self.timings;
        if let Some(sub) =
            self.awaiting_eose.iter().find(|(_, s)| now.saturating_duration_since(**s) >= t.eose)
        {
            return Err(format!("no EOSE for \"{}\" within {}", sub.0, span(t.eose)));
        }
        while let Some(&(id, sent)) = self.awaiting_ok.front() {
            if now.saturating_duration_since(sent) < t.ok {
                break;
            }
            self.awaiting_ok.pop_front();
            let message = format!("no answer within {}", span(t.ok));
            self.link.push(PoolEvent::Ok { relay: self.link.index, id, accepted: false, message });
        }
        match self.ping_sent {
            Some(at) if now.saturating_duration_since(at) >= t.ping_answer => Err("no answer to ping".into()),
            None if now.saturating_duration_since(self.last_rx) >= t.idle_ping => {
                self.ping_sent = Some(now);
                self.send(ws, Message::Ping(Default::default()), false)
            }
            _ => Ok(()),
        }
    }

    fn limit(&mut self, len: usize) -> Result<(), String> {
        self.limiter.frame(Instant::now(), len)
    }

    /// Brings the relay's subscriptions in line with the pool's and sends
    /// the queued events. The events are taken off the queue first: if a
    /// send fails, those not yet sent are lost with the connection (a
    /// frame that broke a connection is not retried forever; announcements
    /// are re-published every minute anyway).
    fn send_pending(&mut self, ws: &mut Ws) -> Result<(), String> {
        let (frames, queued) = {
            let mut st = self.link.lock();
            let mut frames = Vec::new();
            if self.subs_gen != Some(st.subs_gen) {
                self.subs_gen = Some(st.subs_gen);
                for sub in self.sent.keys() {
                    if !st.subs.contains_key(sub) {
                        frames.push(ToRelay::Close { sub: sub.clone() });
                    }
                }
                for (sub, filters) in &st.subs {
                    if self.sent.get(sub) != Some(filters) {
                        frames.push(ToRelay::Req { sub: sub.clone(), filters: filters.clone() });
                    }
                }
                self.sent = st.subs.clone();
            }
            (frames, st.outbox.drain(..).collect::<Vec<_>>())
        };
        let now = Instant::now();
        for f in frames {
            match &f {
                ToRelay::Req { sub, .. } => {
                    self.awaiting_eose.insert(sub.clone(), now);
                }
                ToRelay::Close { sub } => {
                    self.awaiting_eose.remove(sub);
                }
                ToRelay::Event(_) => {}
            }
            self.send(ws, Message::text(encode_to_relay(&f)), false)?;
        }
        for o in queued {
            self.awaiting_ok.push_back((o.id, now));
            self.send(ws, Message::text(o.frame), false)?;
        }
        Ok(())
    }

    fn handle(&mut self, text: &str) -> Result<(), String> {
        let index = self.link.index;
        match decode_from_relay(text) {
            Ok(FromRelay::Event { sub, event }) => {
                // Late events of a closed subscription and forgeries are
                // dropped here, so a forged id never shadows the real event
                // in the pool's dedupe.
                // Only what the filters select (a relay may push anything
                // under our subscription), counted by its memory: a tag of
                // 5 wire bytes takes tens in memory (final review A-M5).
                if self.sent.get(&sub).is_some_and(|fs| fs.iter().any(|f| f.matches(&event)))
                    && event.verify()
                {
                    let size = text.len().max(footprint(&event));
                    self.link.push_event(event, size);
                }
            }
            Ok(FromRelay::Eose { sub }) => {
                if self.awaiting_eose.remove(&sub).is_some() {
                    self.link.push(PoolEvent::Eose { relay: index, sub });
                }
            }
            Ok(FromRelay::Ok { id, accepted, message }) => {
                // An answer after the OK window was already reported.
                if let Some(i) = self.awaiting_ok.iter().position(|(a, _)| *a == id) {
                    self.awaiting_ok.remove(i);
                    self.link.push(PoolEvent::Ok { relay: index, id, accepted, message });
                }
            }
            Ok(FromRelay::Closed { sub, .. }) => {
                // The relay ended (or refused) the subscription: no EOSE
                // will come.
                self.awaiting_eose.remove(&sub);
            }
            Ok(FromRelay::Notice(_)) => {}
            // Not a relay frame at all (not JSON, or not an array headed by
            // a type) drops the connection; a type we do not know (`AUTH`,
            // `COUNT`) or one malformed event is skipped.
            Err(e) if e.is_frame_error() => return Err(format!("not a Nostr message ({e})")),
            Err(_) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_to_a_minute_and_resets() {
        let mut b = Backoff::new();
        let within = |d: Duration, base: f64| (base * 0.8..=base * 1.2).contains(&d.as_secs_f64());
        for base in [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 60.0, 60.0, 60.0] {
            let d = b.next_delay();
            assert!(within(d, base), "{d:?} for {base} s");
        }
        b.reset();
        assert!(within(b.next_delay(), 1.0));
    }

    /// Clients of one build do not all reconnect to a recovering relay at
    /// the same 1, 2, 4 … 60 s marks: each delay is jittered by up to 20 %
    /// (final review A-M7).
    #[test]
    fn backoff_delays_are_jittered() {
        let firsts: std::collections::BTreeSet<Duration> =
            (0..20).map(|_| Backoff::new().next_delay()).collect();
        assert!(firsts.len() > 1, "20 first delays all {firsts:?}");
        assert_eq!(jittered(Duration::from_secs(10), 0), Duration::from_secs(8));
        assert_eq!(jittered(Duration::from_secs(10), u64::MAX), Duration::from_secs(12));
    }

    #[test]
    fn the_limiter_allows_a_burst_but_not_a_sustained_flood() {
        let t0 = Instant::now();
        let mut l = Limiter::new(t0);
        // A burst of 1000 frames at once passes; the next one does not.
        for _ in 0..1000 {
            assert!(l.frame(t0, 100).is_ok());
        }
        assert!(l.frame(t0, 100).unwrap_err().contains("frames/s"));
        // 200 frames/s sustained pass.
        let mut l = Limiter::new(t0);
        for i in 1..=4000u64 {
            assert!(l.frame(t0 + Duration::from_millis(i * 5), 100).is_ok(), "frame {i}");
        }
        // 500 frames/s sustained break once the burst is spent (~3.3 s).
        let mut l = Limiter::new(t0);
        let broke = (1..=4000u64).find(|i| l.frame(t0 + Duration::from_millis(i * 2), 100).is_err());
        assert!(broke.is_some_and(|i| (1500..1800).contains(&i)), "{broke:?}");
        // 2 MiB/s in 64 KiB frames breaks the byte rate, not the frame rate.
        let mut l = Limiter::new(t0);
        let err = (1..=1000u64).find_map(|i| l.frame(t0 + Duration::from_millis(i * 31), 64 << 10).err());
        assert_eq!(err.as_deref(), Some("more than 1 MiB/s"));
    }

    fn out(n: u8, slot: Option<&str>) -> Outgoing {
        Outgoing { slot: slot.map(|d| ([1; 32], 31470, d.to_owned())), id: [n; 32], frame: n.to_string() }
    }

    #[test]
    fn the_outbox_keeps_the_newest_per_slot_and_drops_the_oldest() {
        let link = Link::new(0, "ws://x".into());
        link.queue(out(1, Some("a")));
        link.queue(out(2, None));
        link.queue(out(3, Some("a")));
        let ids: Vec<u8> = link.lock().outbox.iter().map(|o| o.id[0]).collect();
        assert_eq!(ids, [2, 3]);
        for n in 0..=255u8 {
            link.queue(out(n, None));
        }
        let st = link.lock();
        assert_eq!(st.outbox.len(), MAX_OUTBOX);
        assert_eq!(st.outbox.front().map(|o| o.id[0]), Some(0));
    }

    #[test]
    fn the_inbox_drops_the_newest_events_beyond_its_bound() {
        let link = Link::new(3, "ws://x".into());
        let e = Event {
            id: [0; 32],
            pubkey: [0; 32],
            created_at: 0,
            kind: 1,
            tags: Vec::new(),
            content: String::new(),
            sig: [0; 64],
        };
        for _ in 0..MAX_INBOX_EVENTS + 10 {
            link.push_event(e.clone(), 100);
        }
        link.set_status(RelayStatus::Connected);
        assert_eq!(link.lock().dropped, 10);
        let mut v = Vec::new();
        link.drain(&mut v);
        assert_eq!(v.len(), MAX_INBOX_EVENTS + 1);
        assert_eq!(v.last(), Some(&PoolEvent::Status { relay: 3, status: RelayStatus::Connected }));
        // Bytes bound too.
        link.push_event(e.clone(), MAX_INBOX_BYTES);
        link.push_event(e, 1);
        assert_eq!(link.lock().dropped, 11);
    }

    /// The driver passes on only events its filters select (a relay may
    /// push any kind under our subscription), and counts each by its
    /// memory, not its wire bytes: `[""]` tags take 5 bytes on the wire and
    /// tens in memory (final review A-M5).
    #[test]
    fn the_inbox_takes_selected_events_counted_by_memory() {
        use crate::nostr::{FromRelay, encode_from_relay};
        let link = Link::new(0, "ws://x".into());
        let mut s = Session::new(&link, Timings::default());
        let t = vec!["t".to_string(), "open-liero-reloaded".to_string()];
        s.sent.insert(
            "s".into(),
            vec![Filter {
                kinds: vec![31470],
                tags: vec![('t', vec!["open-liero-reloaded".into()])],
                ..Filter::default()
            }],
        );
        let id = olr_lobby::Identity::from_secret([5; 32]).unwrap();
        let frame = |event| encode_from_relay(&FromRelay::Event { sub: "s".into(), event });
        // Another kind, or untagged: not ours.
        let other = Event::sign(&id, 1000, 1, vec![t.clone()], "x".into(), [1; 32]);
        let untagged = Event::sign(&id, 1000, 31470, Vec::new(), "x".into(), [1; 32]);
        for e in [other, untagged] {
            s.handle(&frame(e)).unwrap();
        }
        assert_eq!(link.lock().inbox_events, 0, "events the filters do not select are dropped");
        // Selected, with 20 000 empty tags: counted by its memory.
        let mut tags = vec![t];
        tags.extend((0..20_000).map(|_| vec![String::new()]));
        let heavy = frame(Event::sign(&id, 1000, 31470, tags, String::new(), [1; 32]));
        s.handle(&heavy).unwrap();
        let st = link.lock();
        assert_eq!(st.inbox_events, 1);
        assert!(
            st.inbox_bytes >= 20_000 * 48,
            "{} bytes counted for {} on the wire",
            st.inbox_bytes,
            heavy.len()
        );
    }
}
