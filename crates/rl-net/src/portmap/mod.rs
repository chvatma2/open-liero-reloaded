//! Port mapping (decision N51, spec §5.3 step 1): a native host asks its
//! router to open its port, through PCP (RFC 6887), falling back to NAT-PMP
//! (RFC 6886), and UPnP-IGD, in parallel; all three clients are our own
//! (ruling PM-1 for UPnP: a LAN device must never crash or exhaust us).
//!
//! `PortMapper` is the driver: one thread per method family (`rl-net pcp`:
//! PCP, then NAT-PMP; `rl-net upnp`), each with its own deadline; the first
//! mapping wins and the other thread deletes anything it mapped. The caller
//! only polls a channel, so the UI thread never waits. The winner renews at
//! half the lifetime and deletes its mappings when the mapper is dropped.

#[cfg(feature = "test-hooks")]
pub mod fake;
pub mod pmp;
pub mod upnp;

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How a mapping was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    Upnp,
    NatPmp,
    Pcp,
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Method::Upnp => "UPnP",
            Method::NatPmp => "NAT-PMP",
            Method::Pcp => "PCP",
        })
    }
}

/// Where a port mapping stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MappingState {
    Off,
    Trying,
    /// UDP on `external`'s port, and TCP on the same port when `tcp`; `usable`: its IP is global
    /// (`rl_proto::is_global`) or `allow_local`.
    Mapped {
        method: Method,
        external: SocketAddrV4,
        tcp: bool,
        usable: bool,
    },
    /// Every method failed: why, per method, in words.
    Failed(String),
}

/// What to map, and where to ask.
#[derive(Clone, Debug)]
pub struct MapRequest {
    /// The base address and the game port.
    pub local: SocketAddrV4,
    /// Also TCP on the same external port (F-G).
    pub tcp: bool,
    /// The default gateway (Task 5's `local::gateway`).
    pub gateway: Option<Ipv4Addr>,
    pub allow_local: bool,
    /// The mapping's description at the router: `"rust-liero"`, never a name.
    pub description: &'static str,
    /// The external port must equal the local one (a relay's ports: no +1 … +7 shift).
    pub exact: bool,
    /// More local ports on the same base (ruling PM-2: a relay's ports),
    /// mapped by the protocol that mapped `local`, after it, with the same
    /// discovery: UDP only, each with its external port equal to its local
    /// one (never shifted), renewed and deleted with `local`'s. Each one's
    /// state is in [`PortMapper::extras`].
    pub extra: Vec<u16>,
    /// None: 2 h for PCP/NAT-PMP, 3600 s for UPnP. Tests set seconds.
    pub lifetime: Option<Duration>,
    /// 5351; tests: a FakeGateway's port.
    pub pmp_port: u16,
    /// 239.255.255.250:1900; tests: a FakeGateway's (never the real group).
    pub ssdp: SocketAddr,
    /// The SSDP search window: `upnp::SEARCH` (2 s); fuzz tests shorten it.
    pub search: Duration,
}

/// The PCP and NAT-PMP server port (RFC 6886 §3.1, RFC 6887 §19.1).
pub const PMP_PORT: u16 = 5351;
/// The SSDP multicast group and port.
pub const SSDP: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);
/// PCP and NAT-PMP mappings' lifetime when the request names none: 2 h.
pub const PMP_LIFETIME: u32 = 7200;
/// UPnP leases' when the request names none.
pub const UPNP_LIFETIME: u32 = 3600;
/// The PCP/NAT-PMP thread's deadline for its first mapping, retries included.
pub const PMP_DEADLINE: Duration = Duration::from_secs(3);
/// The UPnP thread's: the SSDP search (2 s) and the HTTP requests.
pub const UPNP_DEADLINE: Duration = Duration::from_secs(4);
/// How long a dropped mapper waits for its threads to delete their mappings.
pub const DROP_WAIT: Duration = Duration::from_secs(1);
/// The ports tried after the local one (a game port's, not an exact request's).
const SHIFTS: u16 = 7;
/// Slack after the PCP/NAT-PMP deadline before `poll` stops waiting for it.
const PMP_GRACE: Duration = Duration::from_millis(500);

impl MapRequest {
    /// The game's mapping at the real router: the local port first, TCP too
    /// when `tcp`, 2 h (PCP/NAT-PMP) or 3600 s (UPnP), no local addresses.
    pub fn new(local: SocketAddrV4, tcp: bool, gateway: Option<Ipv4Addr>) -> MapRequest {
        MapRequest {
            local,
            tcp,
            gateway,
            allow_local: false,
            description: "rust-liero",
            exact: false,
            extra: Vec::new(),
            lifetime: None,
            pmp_port: PMP_PORT,
            ssdp: SSDP,
            search: upnp::SEARCH,
        }
    }

    /// The lifetime to ask for, in seconds: the request's (at least 1), else `default`.
    fn lifetime_or(&self, default: u32) -> u32 {
        self.lifetime.map_or(default, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX).max(1))
    }

    /// The external ports to try, in order: the local one, then up to 7 more
    /// (none for an exact request).
    fn ports(&self) -> Vec<u16> {
        let local = self.local.port();
        let shifts = if self.exact { 0 } else { SHIFTS };
        (0..=shifts).filter_map(|k| local.checked_add(k)).collect()
    }
}

/// The two threads: PCP then NAT-PMP, and UPnP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Worker {
    Pmp = 0,
    Upnp = 1,
}

/// What a thread tells the mapper.
enum Report {
    Mapped {
        method: Method,
        external: SocketAddrV4,
        tcp: bool,
    },
    Failed {
        worker: Worker,
        why: String,
    },
    /// The winner's renewal was refused: the mapping is gone.
    Dropped(String),
    /// A renewal read another external address (an ISP renumbered the
    /// router, ruling FR12b-2): the mapping's address now.
    Moved(SocketAddrV4),
    /// An extra port (`MapRequest::extra`): mapped by the winner, or why not.
    Extra {
        port: u16,
        mapped: Result<(Method, SocketAddrV4), String>,
    },
}

/// The threads' common board: who won, who gave up, whether to stop.
#[derive(Default)]
struct Board {
    winner: Option<Worker>,
    /// A thread `poll` gave up on (its deadline passed): it may not win.
    closed: [bool; 2],
    /// A thread that has asked its router for a mapping: the mapper's drop
    /// waits for it to delete what it holds.
    touched: [bool; 2],
    stop: bool,
    /// A deletion at the mapper's end that the router did not confirm.
    unconfirmed: bool,
}

struct Shared {
    board: Mutex<Board>,
    wake: Condvar,
    /// Per thread: it won (set with `Board::winner`). The panic guard reads
    /// this, never the lock, so its `drop` cannot block or deadlock.
    won: [AtomicBool; 2],
}

impl Shared {
    fn board(&self) -> MutexGuard<'_, Board> {
        self.board.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A thread's handle on the mapper: its request, the board and the channel.
pub(crate) struct Ctx {
    req: MapRequest,
    worker: Worker,
    /// The last instant this thread may win.
    deadline: Instant,
    shared: Arc<Shared>,
    tx: Sender<Report>,
}

impl Ctx {
    /// Whether this thread still works toward a first mapping: nobody won,
    /// the mapper lives and the deadline has not passed.
    pub(crate) fn live(&self) -> bool {
        let b = self.shared.board();
        !b.stop && b.winner.is_none() && !b.closed[self.worker as usize] && Instant::now() < self.deadline
    }

    pub(crate) fn stopped(&self) -> bool {
        self.shared.board().stop
    }

    /// Marks this thread as about to ask for mappings.
    fn touch(&self) {
        self.shared.board().touched[self.worker as usize] = true;
    }

    /// Claims the win for a mapping; false when another thread won, the
    /// mapper is gone or the deadline passed (the caller then deletes it).
    fn claim(&self, method: Method, external: SocketAddrV4, tcp: bool) -> bool {
        let mut b = self.shared.board();
        if b.stop || b.winner.is_some() || b.closed[self.worker as usize] || Instant::now() >= self.deadline {
            return false;
        }
        b.winner = Some(self.worker);
        self.shared.won[self.worker as usize].store(true, Ordering::Relaxed);
        let _ = self.tx.send(Report::Mapped { method, external, tcp });
        true
    }

    fn fail(&self, why: String) {
        let _ = self.tx.send(Report::Failed { worker: self.worker, why });
    }

    fn extra(&self, port: u16, mapped: Result<(Method, SocketAddrV4), String>) {
        let _ = self.tx.send(Report::Extra { port, mapped });
    }

    /// Sleeps until `at` (`None`: for ever) or the mapper's drop; false
    /// when dropped.
    fn sleep_until(&self, at: Option<Instant>) -> bool {
        let mut b = self.shared.board();
        loop {
            if b.stop {
                return false;
            }
            b = match at {
                None => self.shared.wake.wait(b).unwrap_or_else(|e| e.into_inner()),
                Some(at) => {
                    let now = Instant::now();
                    if now >= at {
                        return true;
                    }
                    self.shared.wake.wait_timeout(b, at - now).unwrap_or_else(|e| e.into_inner()).0
                }
            };
        }
    }
}

/// An external port granted for one protocol, and for how long (seconds;
/// 0: a permanent UPnP lease).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Grant {
    pub port: u16,
    pub lifetime: u32,
}

/// Why a mapping request failed.
#[derive(Debug)]
pub(crate) enum MapError {
    /// This external port is taken: try the next one.
    Conflict(String),
    /// The router refused, or answered nonsense: stop (or, for TCP, map UDP alone).
    Fatal(String),
    /// No usable answer (silence, a closed port, a cut answer): stop; a
    /// renewal tries again while the mapping lives.
    Silent(String),
}

/// One router protocol, as the pairing logic and the renewal use it.
pub(crate) trait Router {
    fn method(&self) -> Method;
    /// Maps `udp` (else TCP) from `external` (a suggestion for PCP/NAT-PMP)
    /// to local port `internal`.
    fn map(&mut self, udp: bool, internal: u16, external: u16, ctx: &Ctx) -> Result<Grant, MapError>;
    /// Deletes a mapping, bounded in time: whether the router confirmed it.
    fn unmap(&mut self, udp: bool, internal: u16, external: u16) -> bool;
    /// The external IPv4 address the mappings are on.
    fn external_ip(&self) -> Option<Ipv4Addr>;
    /// After a renewal: reads the external address again where the
    /// renewal's answers do not carry it (NAT-PMP asks, UPnP calls
    /// `GetExternalIPAddress`); a failure keeps the last one (FR12b-2).
    fn refresh_external(&mut self, _ctx: &Ctx) {}
    /// Starts a renewal round: its own deadline for the retries.
    fn renewing(&mut self) {}
}

/// The mappings a thread holds for one local port: UDP and maybe TCP on
/// one external port.
#[derive(Clone, Copy, Debug)]
struct Pair {
    /// The local port.
    internal: u16,
    port: u16,
    tcp: bool,
    /// Seconds until the shorter lifetime ends; 0 never ends.
    lifetime: u32,
}

impl Pair {
    /// Deletes both: whether the router confirmed each.
    fn delete(&self, r: &mut dyn Router) -> bool {
        let udp = r.unmap(true, self.internal, self.port);
        let tcp = !self.tcp || r.unmap(false, self.internal, self.port);
        udp && tcp
    }
}

/// The shorter lifetime, 0 counting as endless.
fn shorter(a: u32, b: u32) -> u32 {
    match (a, b) {
        (0, x) | (x, 0) => x,
        (a, b) => a.min(b),
    }
}

/// Maps UDP, and TCP on the same external port when the request asks
/// (ruling F-G): TCP first at each port tried, then UDP at TCP's port; a UDP
/// grant on another port is deleted and asked once more at TCP's, else both
/// go and the next port is tried. With no pair, UDP alone (`tcp: false`). An
/// exact request takes the local port or nothing. Nothing is left mapped on
/// failure.
fn map_pair(r: &mut dyn Router, ctx: &Ctx) -> Result<Pair, String> {
    let req = &ctx.req;
    let local = req.local.port();
    if local == 0 {
        return Err("no local port".into());
    }
    let wrong_port = |got: u16| format!("the router offered port {got}, not {local}");
    let mut why = String::new();
    ctx.touch();
    if req.tcp {
        for &p in &req.ports() {
            if !ctx.live() {
                return Err("stopped".into());
            }
            let t = match r.map(false, local, p, ctx) {
                Ok(t) => t,
                Err(MapError::Conflict(w)) => {
                    why = w;
                    continue;
                }
                // TCP refused outright (a router that maps UDP only, say):
                // UDP alone.
                Err(MapError::Fatal(w)) => {
                    why = w;
                    break;
                }
                Err(MapError::Silent(w)) => return Err(w),
            };
            if req.exact && t.port != local {
                r.unmap(false, local, t.port);
                why = wrong_port(t.port);
                break;
            }
            let mut granted = None;
            for attempt in 0..2 {
                match r.map(true, local, t.port, ctx) {
                    Ok(u) if u.port == t.port => {
                        granted = Some(u);
                        break;
                    }
                    Ok(u) => {
                        r.unmap(true, local, u.port);
                        why = format!("UDP was granted port {} for TCP's {}", u.port, t.port);
                        if attempt == 1 || !ctx.live() {
                            break;
                        }
                    }
                    Err(MapError::Conflict(w)) => {
                        why = w;
                        break;
                    }
                    Err(MapError::Fatal(w) | MapError::Silent(w)) => {
                        r.unmap(false, local, t.port);
                        return Err(w);
                    }
                }
            }
            match granted {
                Some(u) => {
                    let lifetime = shorter(t.lifetime, u.lifetime);
                    return Ok(Pair { internal: local, port: t.port, tcp: true, lifetime });
                }
                None => {
                    r.unmap(false, local, t.port);
                }
            }
        }
    }
    for &p in &req.ports() {
        if !ctx.live() {
            return Err("stopped".into());
        }
        match r.map(true, local, p, ctx) {
            Ok(u) if req.exact && u.port != local => {
                r.unmap(true, local, u.port);
                return Err(wrong_port(u.port));
            }
            Ok(u) => return Ok(Pair { internal: local, port: u.port, tcp: false, lifetime: u.lifetime }),
            Err(MapError::Conflict(w)) => why = w,
            Err(MapError::Fatal(w) | MapError::Silent(w)) => return Err(w),
        }
    }
    Err(if why.is_empty() { "no port could be mapped".into() } else { why })
}

/// Tries one protocol: on a mapping, claims the win and holds it (renewing)
/// until the mapper is dropped. `Ok(())` once it held a mapping or lost the
/// race (nothing is left mapped); `Err(why)` when it could not map.
fn try_router(r: &mut dyn Router, ctx: &Ctx) -> Result<(), String> {
    let pair = map_pair(r, ctx)?;
    let Some(ip) = r.external_ip() else {
        pair.delete(r);
        return Err("the router named no external IPv4 address".into());
    };
    if !ctx.claim(r.method(), SocketAddrV4::new(ip, pair.port), pair.tcp) {
        pair.delete(r);
        return Ok(());
    }
    let extras = map_extras(r, ctx, ip);
    hold(r, ctx, pair, extras, ip);
    Ok(())
}

/// The winner maps each extra port (ruling PM-2), UDP, exactly: the
/// granted ones, each reported.
fn map_extras(r: &mut dyn Router, ctx: &Ctx, ip: Ipv4Addr) -> Vec<Pair> {
    let mut held = Vec::new();
    for &p in &ctx.req.extra {
        if ctx.stopped() {
            break;
        }
        // Each request gets the protocol's own deadline, as a renewal does.
        r.renewing();
        let mapped = match r.map(true, p, p, ctx) {
            Ok(g) if g.port == p => {
                held.push(Pair { internal: p, port: p, tcp: false, lifetime: g.lifetime });
                Ok((r.method(), SocketAddrV4::new(ip, p)))
            }
            Ok(g) => {
                r.unmap(true, p, g.port);
                Err(format!("{}: the router offered port {}, not {p}", r.method(), g.port))
            }
            Err(MapError::Conflict(w) | MapError::Fatal(w) | MapError::Silent(w)) => {
                Err(format!("{}: {w}", r.method()))
            }
        };
        ctx.extra(p, mapped);
    }
    held
}

/// The winner: renews at half the lifetime. A refused renewal reports the
/// mapping dropped; a silent one is tried again halfway to the mapping's
/// end (RFC 6886 §3.3 retries too), and only when the mapping has run out
/// is it reported dropped. A renewal reads the external address again,
/// and reports it when it changed from `ip` (ruling FR12b-2), the extras'
/// too. Deletes the mappings when the mapper is dropped.
fn hold(r: &mut dyn Router, ctx: &Ctx, pair: Pair, mut extras: Vec<Pair>, mut ip: Ipv4Addr) {
    let half = |lifetime: u32| (Duration::from_secs(lifetime.into()) / 2).max(Duration::from_secs(1));
    let first = extras.iter().fold(pair.lifetime, |l, e| shorter(l, e.lifetime));
    let mut ends = (first != 0).then(|| Instant::now() + Duration::from_secs(first.into()));
    let mut next = (first != 0).then(|| Instant::now() + half(first));
    loop {
        if !ctx.sleep_until(next) {
            let mut confirmed = pair.delete(r);
            for e in &extras {
                confirmed &= e.delete(r);
            }
            ctx.shared.board().unconfirmed |= !confirmed;
            return;
        }
        #[cfg(feature = "test-hooks")]
        injected_panic(ctx.req.local.port());
        r.renewing();
        let why = match renew(r, ctx, pair) {
            Ok(lifetime) => {
                // The extras, each on its own deadline; one refused is
                // deleted and reported alone.
                let mut lifetime = lifetime;
                extras.retain(|e| {
                    r.renewing();
                    match renew(r, ctx, *e) {
                        Ok(l) => {
                            lifetime = shorter(lifetime, l);
                            true
                        }
                        Err(MapError::Conflict(w) | MapError::Fatal(w) | MapError::Silent(w)) => {
                            e.delete(r);
                            ctx.extra(
                                e.internal,
                                Err(format!("the router dropped the mapping: {}: {w}", r.method())),
                            );
                            false
                        }
                    }
                });
                r.renewing();
                r.refresh_external(ctx);
                if let Some(now_ip) = r.external_ip().filter(|&a| a != ip && !a.is_unspecified()) {
                    ip = now_ip;
                    let _ = ctx.tx.send(Report::Moved(SocketAddrV4::new(ip, pair.port)));
                    for e in &extras {
                        ctx.extra(e.internal, Ok((r.method(), SocketAddrV4::new(ip, e.port))));
                    }
                }
                let now = Instant::now();
                ends = (lifetime != 0).then(|| now + Duration::from_secs(lifetime.into()));
                next = (lifetime != 0).then(|| now + half(lifetime));
                continue;
            }
            Err(MapError::Silent(why)) => {
                let now = Instant::now();
                let left = ends.map_or(Duration::ZERO, |e| e.saturating_duration_since(now));
                if left > Duration::from_secs(2) {
                    next = Some(now + (left / 2).max(Duration::from_secs(1)));
                    continue;
                }
                format!("the renewal got no answer before the mapping ran out ({why})")
            }
            Err(MapError::Conflict(why) | MapError::Fatal(why)) => format!("the renewal failed: {why}"),
        };
        pair.delete(r);
        for e in &extras {
            e.delete(r);
        }
        let _ = ctx.tx.send(Report::Dropped(format!("{}: {why}", r.method())));
        return;
    }
}

/// One renewal of every mapping held, at its own port: the new lifetime.
fn renew(r: &mut dyn Router, ctx: &Ctx, pair: Pair) -> Result<u32, MapError> {
    let mut lifetime = 0;
    for udp in [false, true] {
        if !udp && !pair.tcp {
            continue;
        }
        match r.map(udp, pair.internal, pair.port, ctx) {
            Ok(g) if g.port == pair.port => lifetime = shorter(lifetime, g.lifetime),
            Ok(g) => {
                r.unmap(udp, pair.internal, g.port);
                return Err(MapError::Fatal(format!("it moved to port {}", g.port)));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(lifetime)
}

/// A router's words, made safe to show: printable, at most 160 characters.
pub(crate) fn tidy(text: &str) -> String {
    let mut s: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).take(160).collect();
    if text.chars().nth(160).is_some() {
        s.push('…');
    }
    s
}

/// The local ports whose mappings a mapper of this process is deleting,
/// each with how many such mappers (ruling RH-1): a new mapper of one of
/// them waits on its own threads, at most `DROP_WAIT`, before it asks its
/// router, so an old deletion that lands late never removes the new
/// mapping (a game left and hosted again at once on the same port).
static DELETING: Mutex<BTreeMap<u16, u32>> = Mutex::new(BTreeMap::new());
/// Wakes the mappers waiting on [`DELETING`].
static DELETED: Condvar = Condvar::new();

fn deleting() -> MutexGuard<'static, BTreeMap<u16, u32>> {
    DELETING.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Whether a mapper of this process is deleting a mapping of one of `ports`.
fn being_deleted(ports: &[u16]) -> bool {
    let d = deleting();
    ports.iter().any(|p| d.contains_key(p))
}

/// Waits until no mapper of this process deletes a mapping of `ports`, or
/// `until` (a mapper's own thread, never the caller's).
fn wait_deleted(ports: &[u16], until: Instant) {
    let mut d = deleting();
    while ports.iter().any(|p| d.contains_key(p)) {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        d = DELETED.wait_timeout(d, left).unwrap_or_else(|e| e.into_inner()).0;
    }
}

/// Maps the host's port at its router (N51), off the caller's thread.
pub struct PortMapper {
    shared: Arc<Shared>,
    rx: Receiver<Report>,
    threads: Vec<(Worker, JoinHandle<()>)>,
    state: MappingState,
    /// Each extra port's state (`MapRequest::extra`), in the request's order.
    extras: Vec<(u16, MappingState)>,
    /// What `poll` last returned.
    reported: MappingState,
    failures: [Option<String>; 2],
    /// When `poll` stops waiting for each thread.
    deadlines: [Instant; 2],
    allow_local: bool,
    /// Its local ports (`local`'s and the extras), and whether they are in
    /// [`DELETING`] (from `begin_delete` until the deletion's wait ended).
    ports: Vec<u16>,
    deleting: bool,
}

impl PortMapper {
    /// Starts mapping `req.local`: PCP then NAT-PMP on one thread, UPnP on
    /// another. Never blocks. A build with `test-hooks` maps only through
    /// 127.0.0.1 (a `FakeGateway`): a request for a real router, or the real
    /// SSDP group, fails at once.
    pub fn start(req: MapRequest) -> PortMapper {
        let now = Instant::now();
        let shared = Arc::new(Shared {
            board: Mutex::new(Board::default()),
            wake: Condvar::new(),
            won: Default::default(),
        });
        let (tx, rx) = mpsc::channel();
        let mut extras: Vec<(u16, MappingState)> = Vec::new();
        for &p in &req.extra {
            if p != req.local.port() && !extras.iter().any(|e| e.0 == p) {
                extras.push((p, MappingState::Trying));
            }
        }
        let mut req = req;
        req.extra = extras.iter().map(|e| e.0).collect();
        let ports: Vec<u16> = std::iter::once(req.local.port()).chain(req.extra.iter().copied()).collect();
        // An earlier mapper of these ports still deleting (RH-1): the
        // threads wait for it first, and their deadlines move with it.
        let after = if being_deleted(&ports) { DROP_WAIT } else { Duration::ZERO };
        let mut mapper = PortMapper {
            shared: Arc::clone(&shared),
            rx,
            threads: Vec::new(),
            state: MappingState::Trying,
            extras,
            reported: MappingState::Trying,
            failures: [None, None],
            deadlines: [now + after + PMP_DEADLINE + PMP_GRACE, now + after + UPNP_DEADLINE],
            allow_local: req.allow_local,
            ports: ports.clone(),
            deleting: false,
        };
        if let Some(why) = refused_in_tests(&req) {
            mapper.state = MappingState::Failed(why);
            mapper.fail_extras("the first port was not mapped");
            return mapper;
        }
        for (worker, name) in [(Worker::Pmp, "rl-net pcp"), (Worker::Upnp, "rl-net upnp")] {
            let ctx = Ctx {
                req: req.clone(),
                worker,
                deadline: mapper.deadlines[worker as usize],
                shared: Arc::clone(&shared),
                tx: tx.clone(),
            };
            let run = match worker {
                Worker::Pmp => pmp::run,
                Worker::Upnp => upnp::run,
            };
            let guard = PanicGuard { worker, tx: tx.clone(), shared: Arc::clone(&shared) };
            let ports = ports.clone();
            let body = move || {
                let _guard = guard;
                if !after.is_zero() {
                    wait_deleted(&ports, now + after);
                }
                run(ctx);
            };
            match std::thread::Builder::new().name(name.into()).spawn(body) {
                Ok(handle) => mapper.threads.push((worker, handle)),
                Err(e) => {
                    mapper.failures[worker as usize] = Some(match worker {
                        Worker::Pmp => format!("PCP and NAT-PMP: no thread ({e})"),
                        Worker::Upnp => format!("UPnP: no thread ({e})"),
                    })
                }
            }
        }
        mapper.settle(now);
        mapper
    }

    /// The state when it changed since the last poll.
    pub fn poll(&mut self) -> Option<MappingState> {
        loop {
            match self.rx.try_recv() {
                Ok(Report::Mapped { method, external, tcp }) => {
                    if self.state == MappingState::Trying {
                        let usable = self.allow_local || rl_proto::is_global(IpAddr::V4(*external.ip()));
                        self.state = MappingState::Mapped { method, external, tcp, usable };
                    }
                }
                Ok(Report::Failed { worker, why }) => {
                    self.failures[worker as usize].get_or_insert(why);
                }
                Ok(Report::Moved(moved)) => {
                    let usable = self.allow_local || rl_proto::is_global(IpAddr::V4(*moved.ip()));
                    if let MappingState::Mapped { external, usable: u, .. } = &mut self.state {
                        (*external, *u) = (moved, usable);
                    }
                }
                Ok(Report::Dropped(why)) => {
                    self.state = MappingState::Failed(format!("the router dropped the mapping: {why}"));
                    self.fail_extras("the router dropped the first port's mapping");
                }
                Ok(Report::Extra { port, mapped }) => {
                    let usable = |ip: Ipv4Addr| self.allow_local || rl_proto::is_global(IpAddr::V4(ip));
                    let state = match mapped {
                        Ok((method, external)) => MappingState::Mapped {
                            method,
                            external,
                            tcp: false,
                            usable: usable(*external.ip()),
                        },
                        Err(why) => MappingState::Failed(why),
                    };
                    if let Some(e) = self.extras.iter_mut().find(|e| e.0 == port) {
                        e.1 = state;
                    }
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        self.settle(Instant::now());
        if self.state == self.reported {
            return None;
        }
        self.reported = self.state.clone();
        Some(self.state.clone())
    }

    pub fn state(&self) -> &MappingState {
        &self.state
    }

    /// Each extra port's state (`MapRequest::extra`, ruling PM-2), as of
    /// the last `poll`: `Trying` until the winner asked the router for it.
    pub fn extras(&self) -> &[(u16, MappingState)] {
        &self.extras
    }

    /// Deletes the mappings (as the drop does, waiting at most `DROP_WAIT`)
    /// and says whether the router confirmed every deletion (true when
    /// nothing was held).
    pub fn close(mut self) -> bool {
        let finished = self.stop_and_wait();
        finished && !self.shared.board().unconfirmed
    }

    /// Extras still trying fail with the first port.
    fn fail_extras(&mut self, why: &str) {
        for e in &mut self.extras {
            if !matches!(e.1, MappingState::Failed(_)) {
                e.1 = MappingState::Failed(why.into());
            }
        }
    }

    /// Tells the threads to delete their mappings and waits at most
    /// `DROP_WAIT` for those that asked their router for one: whether
    /// every one finished.
    fn stop_and_wait(&mut self) -> bool {
        self.begin_delete();
        self.shared.board().stop = true;
        self.shared.wake.notify_all();
        let end = Instant::now() + DROP_WAIT;
        let finished = loop {
            let touched = self.shared.board().touched;
            let waiting = self.threads.iter().any(|(w, h)| touched[*w as usize] && !h.is_finished());
            if !waiting {
                break true;
            }
            if Instant::now() >= end {
                break false;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        for (_, handle) in self.threads.drain(..) {
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
        self.end_delete();
        finished
    }

    /// Marks its ports as being deleted (ruling RH-1), at once on the
    /// caller's thread (an endpoint shutting down calls it before handing
    /// the mapper to a thread, so a game hosted again right after never
    /// maps first); idempotent. The deletion's end clears it.
    pub(crate) fn begin_delete(&mut self) {
        if !std::mem::replace(&mut self.deleting, true) {
            let mut d = deleting();
            for p in &self.ports {
                *d.entry(*p).or_insert(0) += 1;
            }
        }
    }

    /// The deletion's wait ended: its ports are free for a new mapper.
    fn end_delete(&mut self) {
        if std::mem::replace(&mut self.deleting, false) {
            let mut d = deleting();
            for p in &self.ports {
                if let Some(n) = d.get_mut(p) {
                    *n -= 1;
                    if *n == 0 {
                        d.remove(p);
                    }
                }
            }
            DELETED.notify_all();
        }
    }

    /// While trying: gives up on threads past their deadline (they may no
    /// longer win), and fails once every thread failed.
    fn settle(&mut self, now: Instant) {
        if self.state != MappingState::Trying {
            return;
        }
        for (w, timeout) in [
            (Worker::Pmp, "PCP and NAT-PMP: the gateway did not answer within 3 s"),
            (Worker::Upnp, "UPnP: the router did not answer within 4 s"),
        ] {
            let i = w as usize;
            if self.failures[i].is_none() && now >= self.deadlines[i] {
                let mut b = self.shared.board();
                if b.winner != Some(w) {
                    b.closed[i] = true;
                    self.failures[i] = Some(timeout.into());
                }
            }
        }
        if let [Some(pmp), Some(upnp)] = &self.failures {
            self.state = MappingState::Failed(format!("{pmp}; {upnp}"));
            self.fail_extras("the first port was not mapped");
        }
    }
}

impl Drop for PortMapper {
    /// Tells the threads to delete their mappings and waits at most
    /// `DROP_WAIT` for those that asked their router for one; a thread
    /// still searching holds nothing, and one stuck in a router's HTTP is
    /// detached (it deletes what it maps if it ever gets an answer).
    fn drop(&mut self) {
        self.stop_and_wait();
    }
}

/// Reports a thread's panic (a bug: no input may cause one) as its failure,
/// or as the mapping dropped once it had won; counted for the tests
/// (`worker_panics`).
struct PanicGuard {
    worker: Worker,
    tx: Sender<Report>,
    shared: Arc<Shared>,
}

impl Drop for PanicGuard {
    fn drop(&mut self) {
        if std::thread::panicking() {
            #[cfg(feature = "test-hooks")]
            if !INJECTED.with(std::cell::Cell::get) {
                PANICS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            let why = match self.worker {
                Worker::Pmp => "PCP and NAT-PMP: the client panicked",
                Worker::Upnp => "UPnP: the client panicked",
            };
            let won = self.shared.won[self.worker as usize].load(Ordering::Relaxed);
            let report = if won {
                Report::Dropped(why.into())
            } else {
                Report::Failed { worker: self.worker, why: why.into() }
            };
            let _ = self.tx.send(report);
        }
    }
}

#[cfg(feature = "test-hooks")]
static PANICS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Local ports whose winning thread panics at its first renewal.
#[cfg(feature = "test-hooks")]
static RENEWAL_PANICS: Mutex<std::collections::BTreeSet<u16>> = Mutex::new(std::collections::BTreeSet::new());

#[cfg(feature = "test-hooks")]
thread_local! {
    /// This thread's panic was injected by `panic_at_renewal`: not counted.
    static INJECTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Test hook (feature `test-hooks`): the winner of a mapper for local port
/// `port` panics at its first renewal, standing in for a bug.
#[cfg(feature = "test-hooks")]
pub fn panic_at_renewal(port: u16) {
    RENEWAL_PANICS.lock().unwrap_or_else(|e| e.into_inner()).insert(port);
}

#[cfg(feature = "test-hooks")]
fn injected_panic(port: u16) {
    if RENEWAL_PANICS.lock().unwrap_or_else(|e| e.into_inner()).remove(&port) {
        INJECTED.with(|i| i.set(true));
        panic!("test-hooks: a panic injected at the renewal of port {port}");
    }
}

/// How many mapper threads have panicked in this process (feature
/// `test-hooks`): the fuzz tests require 0.
#[cfg(feature = "test-hooks")]
pub fn worker_panics() -> usize {
    PANICS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Why a build with `test-hooks` refuses `req`: tests map through a
/// `FakeGateway` on 127.0.0.1 only, never the real router or SSDP group.
#[cfg(feature = "test-hooks")]
fn refused_in_tests(req: &MapRequest) -> Option<String> {
    let loopback = req.ssdp.ip().is_loopback() && req.gateway.is_none_or(|g| g.is_loopback());
    (!loopback).then(|| {
        "PCP, NAT-PMP and UPnP: a test build maps only through a FakeGateway on 127.0.0.1".to_string()
    })
}

#[cfg(not(feature = "test-hooks"))]
fn refused_in_tests(_req: &MapRequest) -> Option<String> {
    None
}
