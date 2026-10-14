//! How a host is reached (N47, N54): its `Reachability` in words, and its
//! listener on Nostr, which takes the offers addressed to its key and
//! publishes the endpoint's answers.
//!
//! The listener runs on a thread of its own (`rl-online listen`): it owns
//! the relay pool and the `Inbox`, whose checks verify every event's
//! signature again before the rate bucket (rl-discovery's `handshake`), so
//! a flood of handshakes costs the game's UI thread nothing. The peer's
//! poll answers the offers it passes on (`Endpoint::answer_signalled`, at
//! most the inbox's rate, 10 a second) and hands the answers back to be
//! sealed and published. The thread reads the system's unix time (event
//! dates) and drops its pool itself (up to 2 s), never on the caller's
//! thread.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use rl_discovery::{Inbound, Inbox, Pool, PoolConfig, PoolEvent, RelayStatus, Request};
use rl_lobby::Identity;
use rl_net::endpoint::Reach;
use rl_net::portmap::MappingState;
use rl_net::probe::NatBehaviour;
use rl_proto::Signal;

use crate::join::unix_now;

/// The host's subscription to its handshakes is made again this often, with
/// a fresh `since` (the inbox takes nothing older than a minute anyway).
pub const RESUBSCRIBE: Duration = Duration::from_secs(30);
/// At most this many offers wait for the peer's poll; more are dropped
/// (the inbox's rate, 10 a second with a burst of 20, keeps far below).
const OFFERS_QUEUED: usize = 64;
/// The listener thread's pace: it waits this long for answers between its
/// looks at the relays.
const TICK: Duration = Duration::from_millis(5);
/// The inbox's subscription id.
const INBOX_SUB: &str = "handshakes";

/// Whether and where a host listens for joins through Nostr (N54), and the
/// address it was told it has.
#[derive(Clone, Debug)]
pub struct HostListen {
    /// The relays to listen on (Public and Invite games, servers); `None`
    /// for a LAN game, which does not listen.
    pub pool: Option<PoolConfig>,
    /// The configured public address (rl-server's `public_address`, the game's setting): `Reachability.configured`.
    pub public_address: Option<IpAddr>,
}

impl HostListen {
    /// No listening, no configured address.
    pub fn off() -> HostListen {
        HostListen { pool: None, public_address: None }
    }
}

/// Where the host's listener stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listening {
    /// Not listening (a LAN game, or a client).
    Off,
    /// Some relay has neither connected nor failed yet, and none is
    /// connected.
    Connecting,
    /// `relays` of the `of` relays are connected.
    On { relays: usize, of: usize },
    /// Every relay failed at least once and none is connected (they are
    /// retried).
    NoRelay,
}

/// How a host can be reached: its port, the configured public address,
/// the endpoint's candidates and mapping, and its listener.
#[derive(Clone, Debug, PartialEq)]
pub struct Reachability {
    /// The game's port (TCP and UDP).
    pub port: u16,
    pub configured: Option<IpAddr>,
    pub reach: Reach,
    pub listening: Listening,
    /// The endpoint's first STUN round answered or gave up
    /// (`Endpoint::gathered`: at most `GATHER_DEADLINE` after the start,
    /// at once without STUN servers).
    pub stun_settled: bool,
}

impl Reachability {
    /// N47: the usable mapping's external address (UDP and TCP, `tcp: true`), else the configured address on `port`.
    pub fn direct(&self) -> Option<SocketAddr> {
        match &self.reach.mapping {
            MappingState::Mapped { external, tcp: true, usable: true, .. } => Some(SocketAddr::V4(*external)),
            _ => self.configured.map(|ip| SocketAddr::new(ip, self.port)),
        }
    }

    /// The mapping attempt finished (Mapped, Failed or Off) and the first STUN round settled. Callers wait at most 5 s for it.
    pub fn settled(&self) -> bool {
        self.reach.mapping != MappingState::Trying && self.stun_settled
    }

    /// What the lobby and the server's log say: the router's port, the
    /// configured address, a symmetric NAT, and the listener.
    pub fn lines(&self) -> Vec<String> {
        let p = self.port;
        let fallback = "players connect by hole punching or a TURN relay";
        let mut lines = Vec::new();
        match &self.reach.mapping {
            MappingState::Mapped { method, external, usable: true, .. } => lines.push(format!(
                "Port {p} is open on your router ({method}): players join directly at {external}"
            )),
            MappingState::Mapped { method, external, usable: false, .. } => lines.push(format!(
                "Your router opened port {p} ({method}), but its own address {} is not public (double NAT or \
                 carrier-grade NAT): {fallback}",
                external.ip()
            )),
            MappingState::Trying => lines.push(format!("Asking your router to open port {p}…")),
            MappingState::Failed(why) => {
                lines.push(format!("Your router did not open port {p} ({why}): {fallback}"))
            }
            MappingState::Off if self.configured.is_none() => {
                lines.push(format!("Port mapping is off: {fallback}"))
            }
            MappingState::Off => {}
        }
        if let Some(ip) = self.configured {
            lines.push(format!(
                "Players join directly at {} (your public address setting)",
                SocketAddr::new(ip, p)
            ));
        }
        if self.reach.nat == NatBehaviour::Symmetric {
            lines.push(
                "Your router changes ports for every destination (symmetric NAT): hole punching will fail; \
                 players need the open port or a TURN relay"
                    .into(),
            );
        }
        match self.listening {
            Listening::On { relays, of } => {
                lines.push(format!("Taking joins through {relays} of {of} relays"))
            }
            Listening::NoRelay => lines.push("No relay reachable: only LAN and direct joins work".into()),
            Listening::Connecting => lines.push("Connecting to the relays…".into()),
            Listening::Off => {}
        }
        lines
    }
}

/// What a pool's relays did, from its `Status` events: connected now, ever
/// connected, ever failed to connect.
#[derive(Clone, Debug)]
pub(crate) struct Relays {
    connected: Vec<bool>,
    ever_connected: Vec<bool>,
    /// A connection could not be made (`Failed`) at least once.
    failed: Vec<bool>,
    /// Failed, or dropped once connected.
    lost: Vec<bool>,
}

impl Relays {
    pub(crate) fn new(n: usize) -> Relays {
        Relays {
            connected: vec![false; n],
            ever_connected: vec![false; n],
            failed: vec![false; n],
            lost: vec![false; n],
        }
    }

    pub(crate) fn status(&mut self, relay: usize, status: &RelayStatus) {
        let (Some(connected), Some(ever), Some(failed), Some(lost)) = (
            self.connected.get_mut(relay),
            self.ever_connected.get_mut(relay),
            self.failed.get_mut(relay),
            self.lost.get_mut(relay),
        ) else {
            return;
        };
        *connected = *status == RelayStatus::Connected;
        *ever |= *connected;
        *failed |= matches!(status, RelayStatus::Failed(_));
        *lost |= matches!(status, RelayStatus::Failed(_) | RelayStatus::Dropped(_));
    }

    /// Every relay failed to connect and none ever did (vacuously so
    /// without relays).
    pub(crate) fn none_reachable(&self) -> bool {
        self.failed.iter().all(|&f| f) && !self.ever_connected.iter().any(|&c| c)
    }

    /// Whether relay `relay` could never be reached.
    pub(crate) fn unreachable(&self, relay: usize) -> bool {
        self.failed.get(relay).copied().unwrap_or(true)
            && !self.ever_connected.get(relay).copied().unwrap_or(false)
    }

    pub(crate) fn listening(&self) -> Listening {
        let n = self.connected.len();
        let on = self.connected.iter().filter(|&&c| c).count();
        let settled = self.ever_connected.iter().zip(&self.lost).all(|(&c, &l)| c || l);
        if on > 0 {
            Listening::On { relays: on, of: n }
        } else if settled {
            Listening::NoRelay
        } else {
            Listening::Connecting
        }
    }
}

/// An answer the peer made, for the listener to seal and publish.
struct Answered {
    req: Request,
    signal: Signal,
    mapped: Option<SocketAddr>,
}

/// The host's listener (N54): a thread with the relay pool and the inbox.
/// Dropping it stops the thread, which drops its pool itself.
pub(crate) struct Listener {
    offers: Receiver<Request>,
    answers: Sender<Answered>,
    status: Arc<Mutex<Listening>>,
    stop: Arc<AtomicBool>,
}

impl Listener {
    /// Listens on `cfg`'s relays for handshakes addressed to `me`.
    pub(crate) fn start(cfg: PoolConfig, me: Identity) -> io::Result<Listener> {
        let (offer_tx, offers) = mpsc::sync_channel(OFFERS_QUEUED);
        let (answers, answer_rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Relays::new(cfg.relays.len()).listening()));
        let stop = Arc::new(AtomicBool::new(false));
        let (s, st) = (Arc::clone(&status), Arc::clone(&stop));
        thread::Builder::new()
            .name("rl-online listen".into())
            .spawn(move || listen(cfg, me, &offer_tx, &answer_rx, &s, &st))?;
        Ok(Listener { offers, answers, status, stop })
    }

    /// The new offers, in order.
    pub(crate) fn offers(&mut self) -> Vec<Request> {
        let mut out = Vec::new();
        while let Ok(req) = self.offers.try_recv() {
            out.push(req);
        }
        out
    }

    /// The endpoint's answer (or refusal) to `req`, with the host's mapped
    /// address, to be sealed and published.
    pub(crate) fn answer(&self, req: Request, signal: Signal, mapped: Option<SocketAddr>) {
        // A send fails only once the thread has ended: nothing to do then.
        let _ = self.answers.send(Answered { req, signal, mapped });
    }

    pub(crate) fn listening(&self) -> Listening {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// The listener thread: subscribes to the inbox's filter (again every
/// [`RESUBSCRIBE`]), passes new offers to the peer, publishes repeats'
/// cached replies and the peer's answers, and keeps the status.
fn listen(
    cfg: PoolConfig,
    me: Identity,
    offers: &SyncSender<Request>,
    answers: &Receiver<Answered>,
    status: &Mutex<Listening>,
    stop: &AtomicBool,
) {
    let mut relays = Relays::new(cfg.relays.len());
    let mut pool = Pool::start(cfg);
    let mut inbox = Inbox::new(me);
    let mut rand = || rl_net::random_bytes::<32>().unwrap_or([0; 32]);
    pool.subscribe(INBOX_SUB, vec![inbox.filter(unix_now())]);
    let mut subscribed = Instant::now();
    let mut events = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        match answers.recv_timeout(TICK) {
            Ok(a) => {
                let e = inbox.reply(&a.req, a.signal, a.mapped, Instant::now(), unix_now(), &mut rand);
                pool.publish(e);
                // The others waiting go at once.
                loop {
                    match answers.try_recv() {
                        Ok(a) => {
                            let e = inbox.reply(
                                &a.req,
                                a.signal,
                                a.mapped,
                                Instant::now(),
                                unix_now(),
                                &mut rand,
                            );
                            pool.publish(e);
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The peer is gone.
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let now = Instant::now();
        pool.poll(&mut events);
        for e in events.drain(..) {
            match e {
                PoolEvent::Event { event, .. } => match inbox.offer(&event, now, unix_now()) {
                    // A full queue drops the offer (the peer does not poll).
                    Inbound::Offer(req) => drop(offers.try_send(req)),
                    Inbound::Repeat(reply) => pool.publish(reply),
                    Inbound::Ignored(_) => {}
                },
                PoolEvent::Status { relay, status } => relays.status(relay, &status),
                PoolEvent::Eose { .. } | PoolEvent::Ok { .. } => {}
            }
        }
        *status.lock().unwrap_or_else(PoisonError::into_inner) = relays.listening();
        if now.saturating_duration_since(subscribed) >= RESUBSCRIBE {
            pool.subscribe(INBOX_SUB, vec![inbox.filter(unix_now())]);
            subscribed = now;
        }
    }
    // The pool's drop (up to 2 s) happens here, off the peer's thread.
    drop(pool);
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, SocketAddrV4};

    use rl_net::portmap::Method;

    use super::*;

    fn reach(mapping: MappingState, nat: NatBehaviour) -> Reach {
        Reach { base: None, srflx: None, nat, mapping, relays: Vec::new() }
    }

    fn reachability(mapping: MappingState, configured: Option<IpAddr>, listening: Listening) -> Reachability {
        Reachability {
            port: 23400,
            configured,
            reach: reach(mapping, NatBehaviour::Unknown),
            listening,
            stun_settled: true,
        }
    }

    const EXTERNAL: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 7), 23401);

    #[test]
    fn every_line_reads_as_written() {
        let mapped =
            |usable| MappingState::Mapped { method: Method::Upnp, external: EXTERNAL, tcp: true, usable };
        let one = |r: Reachability| {
            let lines = r.lines();
            assert_eq!(lines.len(), 1, "{lines:?}");
            lines[0].clone()
        };
        assert_eq!(
            one(reachability(mapped(true), None, Listening::Off)),
            "Port 23400 is open on your router (UPnP): players join directly at 203.0.113.7:23401"
        );
        assert_eq!(
            one(reachability(mapped(false), None, Listening::Off)),
            "Your router opened port 23400 (UPnP), but its own address 203.0.113.7 is not public (double NAT or \
             carrier-grade NAT): players connect by hole punching or a TURN relay"
        );
        assert_eq!(
            one(reachability(MappingState::Trying, None, Listening::Off)),
            "Asking your router to open port 23400…"
        );
        assert_eq!(
            one(reachability(MappingState::Failed("no router answered".into()), None, Listening::Off)),
            "Your router did not open port 23400 (no router answered): players connect by hole punching or a TURN \
             relay"
        );
        assert_eq!(
            one(reachability(MappingState::Off, None, Listening::Off)),
            "Port mapping is off: players connect by hole punching or a TURN relay"
        );
        let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 4));
        assert_eq!(
            one(reachability(MappingState::Off, Some(ip), Listening::Off)),
            "Players join directly at 198.51.100.4:23400 (your public address setting)"
        );
        let mut symmetric = reachability(MappingState::Off, Some(ip), Listening::Off);
        symmetric.reach.nat = NatBehaviour::Symmetric;
        assert_eq!(
            symmetric.lines()[1],
            "Your router changes ports for every destination (symmetric NAT): hole punching will fail; players need \
             the open port or a TURN relay"
        );
        let listening = |l| reachability(MappingState::Off, Some(ip), l).lines()[1].clone();
        assert_eq!(listening(Listening::On { relays: 3, of: 8 }), "Taking joins through 3 of 8 relays");
        assert_eq!(listening(Listening::NoRelay), "No relay reachable: only LAN and direct joins work");
        assert_eq!(listening(Listening::Connecting), "Connecting to the relays…");
        // The order: the router, the address, the NAT, the listener.
        let mut all = reachability(mapped(true), Some(ip), Listening::NoRelay);
        all.reach.nat = NatBehaviour::Symmetric;
        let lines = all.lines();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].starts_with("Port 23400 is open") && lines[1].starts_with("Players join directly"));
        assert!(lines[2].contains("symmetric NAT") && lines[3].starts_with("No relay"));
    }

    #[test]
    fn the_direct_address_needs_tcp_on_a_usable_mapping() {
        let ip = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 4));
        let mapped =
            |tcp, usable| MappingState::Mapped { method: Method::Pcp, external: EXTERNAL, tcp, usable };
        assert_eq!(
            reachability(mapped(true, true), Some(ip), Listening::Off).direct(),
            Some(EXTERNAL.into())
        );
        assert_eq!(
            reachability(mapped(false, true), Some(ip), Listening::Off).direct(),
            Some(SocketAddr::new(ip, 23400))
        );
        assert_eq!(reachability(mapped(true, false), None, Listening::Off).direct(), None);
        assert_eq!(reachability(MappingState::Off, None, Listening::Off).direct(), None);
    }

    #[test]
    fn settled_waits_for_the_router_and_stun() {
        let mut r = reachability(MappingState::Trying, None, Listening::Connecting);
        assert!(!r.settled());
        r.reach.mapping = MappingState::Failed("x".into());
        assert!(r.settled());
        r.stun_settled = false;
        assert!(!r.settled());
    }

    #[test]
    fn the_listener_status_follows_the_relays() {
        let mut r = Relays::new(2);
        assert_eq!(r.listening(), Listening::Connecting);
        r.status(0, &RelayStatus::Failed("refused".into()));
        assert_eq!(r.listening(), Listening::Connecting, "one still trying");
        r.status(1, &RelayStatus::Connected);
        assert_eq!(r.listening(), Listening::On { relays: 1, of: 2 });
        r.status(1, &RelayStatus::Dropped("gone".into()));
        assert_eq!(r.listening(), Listening::NoRelay);
        r.status(0, &RelayStatus::Connecting);
        assert_eq!(r.listening(), Listening::NoRelay, "no flapping while retrying");
        assert!(!r.none_reachable(), "one connected once");
        r.status(9, &RelayStatus::Connected);
        assert_eq!(Relays::new(0).listening(), Listening::NoRelay);
        assert!(Relays::new(0).none_reachable());
    }
}
