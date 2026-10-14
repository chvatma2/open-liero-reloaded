//! [`TurnServer`]: a [`TurnCore`] on real sockets. One thread steps the
//! core; one reader thread per socket (the listening one and one per
//! allocation) blocks in `recv_from` with a short timeout and hands the
//! datagrams over a bounded channel, so an idle relay sleeps.

use std::collections::BTreeMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{MAX_DATAGRAM, TurnCore, TurnLimits};

/// A reader wakes this often to see whether it should stop.
const READ_TIMEOUT: Duration = Duration::from_millis(200);
/// The core's thread steps at least this often (expiries, stopping).
const TICK: Duration = Duration::from_millis(100);
/// Datagrams waiting for the core (each at most `MAX_DATAGRAM`: at most
/// 4 MiB); beyond, readers drop and count (as a full socket buffer would).
const QUEUE: usize = 1024;
/// A reader sleeps 1 ms only after this many errors in a row (other than
/// timeouts and resets), so a failing socket does not spin.
const ERRORS_BEFORE_SLEEP: u32 = 16;
/// At most this many datagrams are taken per turn of the core's loop.
const PER_TURN: usize = 1024;

/// A [`TurnServer`]'s setup.
#[derive(Clone, Debug)]
pub struct TurnServerConfig {
    /// Port 0: an ephemeral port (`local_addr` tells). IPv4 only (ruling TR-3).
    pub listen: SocketAddr,
    pub public: IpAddr,
    /// Relay ports; `0..=0`: an OS-assigned ephemeral port per allocation (tests).
    pub ports: RangeInclusive<u16>,
    pub user: String,
    pub pass: String,
    pub limits: TurnLimits,
    /// N62: relay to private and loopback peers too (office and LAN setups, tests).
    pub allow_local: bool,
}

/// What a running relay did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TurnStats {
    pub allocations: usize,
    pub dropped: u64,
    pub relayed_bytes: u64,
}

/// A relay on real sockets. Dropping it stops and joins its threads
/// (within [`READ_TIMEOUT`]).
pub struct TurnServer {
    local: SocketAddr,
    stop: Arc<AtomicBool>,
    stats: Arc<Mutex<TurnStats>>,
    thread: Option<JoinHandle<()>>,
}

/// A datagram read: to the listening port (`port` None) or a relay port.
struct Inbound {
    port: Option<u16>,
    from: SocketAddr,
    bytes: Vec<u8>,
}

/// A socket's reader thread.
struct Reader {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Reader {
    fn spawn(
        socket: UdpSocket,
        port: Option<u16>,
        tx: SyncSender<Inbound>,
        dropped: Arc<AtomicU64>,
    ) -> io::Result<Reader> {
        socket.set_read_timeout(Some(READ_TIMEOUT))?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::Builder::new().name("turn-reader".into()).spawn(move || {
            // The whole datagram is read (a smaller buffer fails on
            // Windows, WSAEMSGSIZE); only small ones are copied on.
            let mut buf = vec![0u8; 65_536];
            let mut errors = 0;
            while !flag.load(Ordering::Relaxed) {
                match socket.recv_from(&mut buf) {
                    Ok((n, from)) => {
                        errors = 0;
                        if n > MAX_DATAGRAM {
                            dropped.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                        let inbound = Inbound { port, from, bytes: buf[..n].to_vec() };
                        match tx.try_send(inbound) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => {
                                dropped.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(TrySendError::Disconnected(_)) => return,
                        }
                    }
                    // A timeout, or on Windows a reset from an ICMP port
                    // unreachable (spoofable): read on at once.
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::WouldBlock
                                | io::ErrorKind::TimedOut
                                | io::ErrorKind::ConnectionReset
                        ) => {}
                    // Anything else ends nothing either; a socket failing
                    // again and again does not spin.
                    Err(_) => {
                        errors += 1;
                        if errors >= ERRORS_BEFORE_SLEEP {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                }
            }
        })?;
        Ok(Reader { stop, thread: Some(thread) })
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A relay socket for an allocation: a free port of `ports` on `ip`, or
/// an OS-assigned one for `0..=0`.
fn bind_relay(
    ip: IpAddr,
    ports: &RangeInclusive<u16>,
    taken: &BTreeMap<u16, UdpSocket>,
) -> Option<(u16, UdpSocket)> {
    let socket = if *ports == (0..=0) {
        UdpSocket::bind((ip, 0)).ok()?
    } else {
        ports
            .clone()
            .filter(|p| *p != 0 && !taken.contains_key(p))
            .find_map(|p| UdpSocket::bind((ip, p)).ok())?
    };
    let port = socket.local_addr().ok()?.port();
    (!taken.contains_key(&port)).then_some((port, socket))
}

impl TurnServer {
    /// Binds `cfg.listen` and starts the relay; an IPv6 `listen` or
    /// `public` is refused (`InvalidInput`): per-IP limits need IPv4.
    pub fn start(cfg: TurnServerConfig) -> io::Result<TurnServer> {
        if !cfg.listen.is_ipv4() || !cfg.public.is_ipv4() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a TURN relay listens on IPv4 only"));
        }
        let listen = UdpSocket::bind(cfg.listen)?;
        let local = listen.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Mutex::new(TurnStats::default()));
        let mut core = TurnCore::new(local, cfg.public, &cfg.user, &cfg.pass, cfg.limits, Instant::now());
        core.set_allow_local(cfg.allow_local);
        let (tx, rx) = sync_channel(QUEUE);
        let dropped = Arc::new(AtomicU64::new(0));
        let reader = Reader::spawn(listen.try_clone()?, None, tx.clone(), dropped.clone())?;
        let (thread_stop, thread_stats) = (stop.clone(), stats.clone());
        let ports = cfg.ports;
        let thread = std::thread::Builder::new().name("turn-server".into()).spawn(move || {
            Relay {
                core,
                listen,
                ports,
                relays: BTreeMap::new(),
                readers: BTreeMap::new(),
                retiring: Vec::new(),
                tx,
                dropped,
                stats: thread_stats,
            }
            .run(rx, reader, &thread_stop)
        })?;
        Ok(TurnServer { local, stop, stats, thread: Some(thread) })
    }

    /// The listening address (the bound port when `listen`'s was 0).
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    pub fn stats(&self) -> TurnStats {
        self.stats.lock().map(|s| *s).unwrap_or_default()
    }
}

impl Drop for TurnServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The core's thread.
struct Relay {
    core: TurnCore,
    listen: UdpSocket,
    ports: RangeInclusive<u16>,
    /// Relay sockets (the sending side) and their readers.
    relays: BTreeMap<u16, UdpSocket>,
    readers: BTreeMap<u16, Reader>,
    /// Stopped readers, joined once finished (never waited for).
    retiring: Vec<Reader>,
    tx: SyncSender<Inbound>,
    /// Datagrams the readers dropped (too large, or the queue full).
    dropped: Arc<AtomicU64>,
    stats: Arc<Mutex<TurnStats>>,
}

impl Relay {
    fn run(mut self, rx: Receiver<Inbound>, listen_reader: Reader, stop: &AtomicBool) {
        let ip = match self.listen.local_addr().map(|a| a.ip()) {
            Ok(ip) if !ip.is_unspecified() => ip,
            _ => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        };
        let listen_port = self.listen.local_addr().map_or(0, |a| a.port());
        let mut out = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            let first = match rx.recv_timeout(TICK) {
                Ok(inbound) => Some(inbound),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            let now = Instant::now();
            for inbound in first.into_iter().chain(rx.try_iter().take(PER_TURN)) {
                match inbound.port {
                    None => self.core.receive(inbound.from, &inbound.bytes, now),
                    Some(port) => self.core.receive_relayed(port, inbound.from, &inbound.bytes, now),
                }
            }
            for req in self.core.take_allocation_requests() {
                let port = self.open(ip);
                self.core.allocated(req, port, now);
            }
            self.core.poll(now, &mut out);
            for o in out.drain(..) {
                let socket = if o.from_port == listen_port {
                    Some(&self.listen)
                } else {
                    self.relays.get(&o.from_port)
                };
                if let Some(socket) = socket {
                    let _ = socket.send_to(&o.bytes, o.to);
                }
            }
            for port in self.core.released() {
                self.relays.remove(&port);
                if let Some(reader) = self.readers.remove(&port) {
                    reader.stop();
                    self.retiring.push(reader);
                }
            }
            self.retiring.retain(|r| !r.finished());
            if let Ok(mut s) = self.stats.lock() {
                *s = TurnStats {
                    allocations: self.core.allocations(),
                    dropped: self.core.dropped() + self.dropped.load(Ordering::Relaxed),
                    relayed_bytes: self.core.relayed_bytes(),
                };
            }
        }
        listen_reader.stop();
        for reader in self.readers.values() {
            reader.stop();
        }
        // Dropping the readers joins them.
    }

    /// A relay socket and its reader for a new allocation.
    fn open(&mut self, ip: IpAddr) -> Option<u16> {
        let (port, socket) = bind_relay(ip, &self.ports, &self.relays)?;
        let reader =
            Reader::spawn(socket.try_clone().ok()?, Some(port), self.tx.clone(), self.dropped.clone())
                .ok()?;
        self.relays.insert(port, socket);
        self.readers.insert(port, reader);
        Some(port)
    }
}
