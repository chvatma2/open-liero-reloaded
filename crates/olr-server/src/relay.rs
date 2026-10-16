//! A volunteer TURN relay (N52, `olr-server --relay`): our own `TurnServer`
//! on the relay's port, its ports mapped at the router with the external
//! port equal to the local one, and a TURN note on the server's Nostr
//! relays every minute (also for Lan and Invite servers: a relay is public
//! by nature).
//!
//! The relay starts only once its public IP is known, since its relayed
//! addresses are `public:<local port>` (`TurnServerConfig::public` is fixed
//! at the start): the configured `public_address`; else the external IP of
//! the listening port's exact mapping; else the game socket's
//! server-reflexive IP once STUN settled, and then only when the host is
//! not behind a NAT (`NatBehaviour::Open`). Otherwise it is not started.
//!
//! With port mapping, one `PortMapper` asks for the listening port and
//! every relay port (ruling PM-2: one discovery), and the relay waits until
//! the router answered for each: when the router maps, the TURN server
//! hands out only the relay ports it mapped exactly (Task 8 review I1: a
//! relayed address is `public:<local port>`, so any other port would be
//! unreachable), and announces at most that many allocations.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use olr_discovery::announce::{ANNOUNCE_TTL, CLOSE_TTL, REPUBLISH};
use olr_discovery::list::is_global;
use olr_discovery::turn_note::{TURN_NOTE_V, TurnNote, turn_note};
use olr_discovery::{Event, Region};
use olr_lobby::Identity;
use olr_net::portmap::{MapRequest, MappingState, PortMapper};
use olr_net::probe::NatBehaviour;
use olr_net::turn::{RelayPorts, TurnLimits, TurnServer, TurnServerConfig, TurnStats};
use olr_online::Reachability;

use crate::config::RelayConfig;
use crate::{aux, line, unix_now};

/// Said when the relay has no address to announce.
pub const NO_PUBLIC_ADDRESS: &str =
    "relay: not announced: no public address (set public_address, or enable port mapping)";
/// The relay's load is looked at this often (the log says each change).
const LOAD_EVERY: Duration = Duration::from_secs(1);

/// Where the relay's port mappings ask: the base address and the gateway,
/// and under the test hooks a `FakeGateway`'s ports.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MapTarget {
    pub(crate) base: Ipv4Addr,
    pub(crate) gateway: Option<Ipv4Addr>,
    /// Tests: the fake's PCP/NAT-PMP port and SSDP socket, and the
    /// mappings' lifetime.
    pub(crate) pmp_port: Option<u16>,
    pub(crate) ssdp: Option<SocketAddr>,
    pub(crate) lifetime: Option<Duration>,
}

/// The ports the relay asked its router to open (UDP, exact): the first
/// (the listening port, else the first relay port) and the extras, in one
/// `PortMapper`; which ones the log has said.
struct Mapping {
    first: u16,
    mapper: PortMapper,
    said: BTreeSet<u16>,
    /// The summary line was said (every port answered).
    settled: bool,
}

impl Mapping {
    /// Every port's state: the first, then the extras.
    fn states(&self) -> Vec<(u16, MappingState)> {
        let mut all = vec![(self.first, self.mapper.state().clone())];
        all.extend(self.mapper.extras().iter().cloned());
        all
    }

    /// The router answered for every port.
    fn answered(&self) -> bool {
        *self.mapper.state() != MappingState::Trying
            && self.mapper.extras().iter().all(|(_, s)| *s != MappingState::Trying)
    }

    /// The router maps: the first port is mapped, usable.
    fn router_maps(&self) -> bool {
        matches!(self.mapper.state(), MappingState::Mapped { usable: true, .. })
    }
}

/// The relay's mappings: waiting for the base address (found on a thread:
/// the OS's interfaces can take ~350 ms), asked, or none.
enum Maps {
    Finding(Receiver<Option<MapTarget>>),
    Asked(Box<Mapping>),
    None,
}

/// The running relay and its notes.
struct Running {
    server: TurnServer,
    public: Ipv4Addr,
    /// What the note announces: the config's, at most the relay ports.
    capacity: u16,
    /// The last note's time and `created_at` (none yet: not announced).
    noted: Option<(Instant, u64)>,
    load: Option<usize>,
    load_checked: Option<Instant>,
}

enum Phase {
    /// Waiting for the public address.
    Waiting,
    Running(Box<Running>),
    /// Not started (the log said why).
    Off,
}

/// A volunteer TURN relay (N52).
pub(crate) struct Relay {
    cfg: RelayConfig,
    identity: Identity,
    region: Region,
    allow_local: bool,
    configured: Option<IpAddr>,
    user: String,
    pass: String,
    maps: Maps,
    phase: Phase,
    /// The public address is the listening port's mapping's: it follows
    /// the router's external address (ruling FR12b-2).
    follows_mapping: bool,
    /// The last note's `created_at`, kept across a restart (notes never go
    /// back in time).
    last_created: u64,
}

/// The relay's mappings being deleted (the mapper's `close` on a thread).
pub(crate) struct Unmapping {
    done: Option<Receiver<bool>>,
}

impl Unmapping {
    /// Waits until the mappings are deleted (the mapper waits at most
    /// `portmap::DROP_WAIT`, 1 s) or `until`; `true` when the router
    /// confirmed every deletion (or there was none).
    pub(crate) fn wait(self, until: Instant) -> bool {
        let Some(rx) = self.done else { return true };
        // Disconnected (no thread) or past `until`: not confirmed.
        rx.recv_timeout(until.saturating_duration_since(Instant::now())).unwrap_or(false)
    }
}

/// 16 lowercase hex characters from the system's randomness.
fn credential() -> Option<String> {
    let bytes = olr_net::random_bytes::<8>().ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The ports as runs (`49160-49175, 49177-49191`), or "ephemeral".
fn ports_text(ports: &RelayPorts) -> String {
    let RelayPorts::Only(set) = ports else { return "ephemeral".into() };
    let mut runs: Vec<(u16, u16)> = Vec::new();
    for &p in set {
        match runs.last_mut() {
            Some((_, last)) if last.checked_add(1) == Some(p) => *last = p,
            _ => runs.push((p, p)),
        }
    }
    let run = |(a, b): &(u16, u16)| if a == b { a.to_string() } else { format!("{a}-{b}") };
    runs.iter().map(run).collect::<Vec<_>>().join(", ")
}

impl Relay {
    /// A relay as `cfg` says: fresh credentials, and with `port_mapping`
    /// its listening port and relay ports asked of the router (`target`,
    /// else the OS's base address and gateway). `None` (said in the log)
    /// when the system gives no randomness.
    pub(crate) fn new(
        cfg: RelayConfig,
        identity: Identity,
        region: Region,
        allow_local: bool,
        configured: Option<IpAddr>,
        port_mapping: Option<Option<MapTarget>>,
        log: &mut dyn std::io::Write,
    ) -> Option<Relay> {
        let (Some(user), Some(pass)) = (credential(), credential()) else {
            line(log, format_args!("relay: not started: the system gives no randomness for its credentials"));
            return None;
        };
        let maps = match port_mapping {
            None => Maps::None,
            Some(Some(target)) => {
                let (tx, rx) = mpsc::channel();
                let _ = tx.send(Some(target));
                Maps::Finding(rx)
            }
            Some(None) => {
                let (tx, rx) = mpsc::channel();
                let found =
                    std::thread::Builder::new().name("olr-server relay base".into()).spawn(move || {
                        let (ips, gateway) = olr_net::local::base_addresses(None);
                        let target = ips.first().map(|&base| MapTarget {
                            base,
                            gateway,
                            pmp_port: None,
                            ssdp: None,
                            lifetime: None,
                        });
                        let _ = tx.send(target);
                    });
                if found.is_ok() { Maps::Finding(rx) } else { Maps::None }
            }
        };
        line(log, format_args!("relay: waiting for the public address"));
        Some(Relay {
            cfg,
            identity,
            region,
            allow_local,
            configured,
            user,
            pass,
            maps,
            phase: Phase::Waiting,
            follows_mapping: false,
            last_created: 0,
        })
    }

    /// The TURN server's listening address, once it runs.
    pub(crate) fn local_addr(&self) -> Option<SocketAddr> {
        match &self.phase {
            Phase::Running(r) => Some(r.server.local_addr()),
            _ => None,
        }
    }

    pub(crate) fn stats(&self) -> Option<TurnStats> {
        match &self.phase {
            Phase::Running(r) => Some(r.server.stats()),
            _ => None,
        }
    }

    /// The mappings' news, the start once the public address is known, and
    /// a note when one is due (to publish on the server's relays).
    /// `reach` is the host's reachability (asked only while waiting).
    pub(crate) fn poll(
        &mut self,
        now: Instant,
        reach: &mut dyn FnMut() -> Option<Reachability>,
        log: &mut dyn std::io::Write,
    ) -> Option<Event> {
        self.poll_maps(log);
        if matches!(self.phase, Phase::Waiting) {
            match self.public(reach, log) {
                None => return None,
                Some(Ok(ip)) => self.start(ip, log),
                Some(Err(why)) => {
                    line(log, format_args!("{why}"));
                    self.phase = Phase::Off;
                }
            }
        }
        self.follow_mapping(log);
        let Phase::Running(r) = &mut self.phase else { return None };
        if r.load_checked.is_none_or(|t| now.saturating_duration_since(t) >= LOAD_EVERY) {
            r.load_checked = Some(now);
            let load = r.server.stats().allocations;
            if r.load.is_some_and(|l| l != load) {
                line(log, format_args!("relay: {load} allocations"));
            }
            r.load = Some(load);
        }
        if r.noted.is_some_and(|(t, _)| now.saturating_duration_since(t) < REPUBLISH) {
            return None;
        }
        let load = r.load.unwrap_or(0);
        let created = match r.noted {
            Some((_, c)) => c.max(unix_now()),
            // The first note of this start: after any earlier start's.
            None if self.last_created > 0 => unix_now().max(self.last_created + 1),
            None => unix_now(),
        };
        self.last_created = created;
        r.noted = Some((now, created));
        let (public, port, capacity) = (r.public, r.server.local_addr().port(), r.capacity);
        let note = self.note(public, port, capacity, load);
        Some(turn_note(&self.identity, &note, created, ANNOUNCE_TTL, aux()))
    }

    fn note(&self, addr: Ipv4Addr, port: u16, capacity: u16, load: usize) -> TurnNote {
        TurnNote {
            v: TURN_NOTE_V,
            addr,
            port,
            region: self.region,
            capacity,
            load: u16::try_from(load).unwrap_or(u16::MAX).min(capacity),
            user: self.user.clone(),
            pass: self.pass.clone(),
        }
    }

    /// The public IPv4 address to relay on: `Some(Ok)` to start, `Some(Err)`
    /// with the log line when the relay cannot be announced, `None` while
    /// it is not known yet.
    fn public(
        &mut self,
        reach: &mut dyn FnMut() -> Option<Reachability>,
        log: &mut dyn std::io::Write,
    ) -> Option<Result<Ipv4Addr, String>> {
        let usable = |ip: Ipv4Addr| {
            if is_global(IpAddr::V4(ip)) || self.allow_local {
                Ok(ip)
            } else {
                Err(format!(
                    "relay: not announced: {ip} is not a public address (allow_local: true for a LAN)"
                ))
            }
        };
        // The configured address, once the mapping (if any) answered.
        let configured = match self.configured {
            Some(IpAddr::V4(ip)) => Some(ip),
            Some(IpAddr::V6(ip)) => {
                line(log, format_args!("relay: public_address {ip} is IPv6: a relay needs IPv4"));
                self.configured = None;
                None
            }
            None => None,
        };
        // The listening port's exact mapping, once the router answered for
        // every port.
        match &self.maps {
            Maps::Finding(_) => return None,
            Maps::Asked(m) if !m.answered() => return None,
            _ if configured.is_some() => {}
            Maps::Asked(m) => match m.mapper.state() {
                MappingState::Mapped { external, usable: true, .. }
                    if m.first == self.cfg.port && external.port() == self.cfg.port =>
                {
                    self.follows_mapping = true;
                    return Some(usable(*external.ip()));
                }
                _ => {}
            },
            Maps::None => {}
        }
        if let Some(configured) = configured {
            return Some(usable(configured));
        }
        // The game socket's server-reflexive address, once STUN settled:
        // only a host behind no NAT is reachable there.
        let r = reach()?;
        if !r.stun_settled {
            return None;
        }
        match r.reach.srflx {
            Some(SocketAddr::V4(srflx)) if r.reach.nat == NatBehaviour::Open => {
                let ip = *srflx.ip();
                line(log, format_args!("relay: no port mapping: relying on {ip} being reachable"));
                Some(usable(ip))
            }
            _ => Some(Err(NO_PUBLIC_ADDRESS.into())),
        }
    }

    /// The relay ports to hand out: ephemeral ones for `(0, 0)`; when the
    /// router maps, only those it mapped exactly; else (no mapping, or the
    /// router never mapped: a manual forward) every configured one.
    fn relay_ports(&self) -> RelayPorts {
        if self.cfg.ports == (0, 0) {
            return RelayPorts::Ephemeral;
        }
        match &self.maps {
            Maps::Asked(m) if m.router_maps() => RelayPorts::Only(
                m.states()
                    .into_iter()
                    .filter(|(p, s)| {
                        self.cfg.port_range().contains(p)
                            && matches!(s, MappingState::Mapped { external, usable: true, .. } if external.port() == *p)
                    })
                    .map(|(p, _)| p)
                    .collect(),
            ),
            _ => RelayPorts::range(self.cfg.port_range()),
        }
    }

    /// Starts the TURN server on `public`, on the relay ports it may use,
    /// announcing at most that many allocations.
    fn start(&mut self, public: Ipv4Addr, log: &mut dyn std::io::Write) {
        let ports = self.relay_ports();
        let capacity = match &ports {
            RelayPorts::Ephemeral => self.cfg.capacity,
            RelayPorts::Only(set) => self.cfg.capacity.min(u16::try_from(set.len()).unwrap_or(u16::MAX)),
        };
        if capacity == 0 {
            line(log, format_args!("relay: not started: the router mapped none of its relay ports exactly"));
            self.phase = Phase::Off;
            return;
        }
        let limits = TurnLimits {
            capacity: usize::from(capacity),
            per_ip_allocations: usize::from(self.cfg.allocations_per_ip),
            per_ip_bytes_per_s: self.cfg.user_kib_s.saturating_mul(1024),
            ..TurnLimits::default()
        };
        let started = TurnServer::start(TurnServerConfig {
            listen: SocketAddr::from((Ipv4Addr::UNSPECIFIED, self.cfg.port)),
            public: IpAddr::V4(public),
            ports: ports.clone(),
            user: self.user.clone(),
            pass: self.pass.clone(),
            limits,
            allow_local: self.allow_local,
        });
        match started {
            Ok(server) => {
                let port = server.local_addr().port();
                line(
                    log,
                    format_args!(
                        "relay: TURN on port {port}, relay ports {}, announced as {public}:{port}",
                        ports_text(&ports)
                    ),
                );
                if capacity < self.cfg.capacity {
                    line(log, format_args!("relay: capacity {capacity}: one allocation per relay port"));
                }
                self.phase = Phase::Running(Box::new(Running {
                    server,
                    public,
                    capacity,
                    noted: None,
                    load: None,
                    load_checked: None,
                }));
            }
            Err(e) => {
                line(log, format_args!("relay: cannot start on port {}: {e}", self.cfg.port));
                self.phase = Phase::Off;
            }
        }
    }

    /// A relay running on its mapping's address restarts on the router's
    /// new external address (an ISP renumbered it, ruling FR12b-2): its
    /// relayed addresses are `public:<port>`, fixed at the start, and the
    /// old ones reach nobody. The next poll announces it.
    fn follow_mapping(&mut self, log: &mut dyn std::io::Write) {
        let (Phase::Running(r), Maps::Asked(m)) = (&self.phase, &self.maps) else { return };
        let MappingState::Mapped { external, usable: true, .. } = m.mapper.state() else { return };
        let ip = *external.ip();
        if !self.follows_mapping || ip == r.public {
            return;
        }
        // The old server goes first: the new one listens on its port.
        self.phase = Phase::Waiting;
        line(log, format_args!("relay: the router's address is now {ip}: restarted"));
        self.start(ip, log);
    }

    /// Asks the router for the ports once the base address is known, and
    /// says each port that cannot be mapped exactly (never shifted: the
    /// relay reports `public:<local port>`).
    fn poll_maps(&mut self, log: &mut dyn std::io::Write) {
        if let Maps::Finding(rx) = &self.maps {
            self.maps = match rx.try_recv() {
                Err(TryRecvError::Empty) => return,
                Ok(Some(target)) => match self.ask(&target) {
                    Some(m) => Maps::Asked(Box::new(m)),
                    None => Maps::None,
                },
                Ok(None) | Err(TryRecvError::Disconnected) => {
                    line(log, format_args!("relay: ports not mapped: no local address found"));
                    Maps::None
                }
            };
        }
        let Maps::Asked(m) = &mut self.maps else { return };
        m.mapper.poll();
        for (port, state) in m.states() {
            if state == MappingState::Trying || m.said.contains(&port) {
                continue;
            }
            m.said.insert(port);
            match &state {
                MappingState::Failed(why) => line(log, format_args!("relay: port {port} not mapped: {why}")),
                MappingState::Mapped { external, usable: false, .. } => line(
                    log,
                    format_args!(
                        "relay: port {port} not mapped: the router's address {} is not public (double NAT or \
                         carrier-grade NAT)",
                        external.ip()
                    ),
                ),
                MappingState::Mapped { method, external, .. } if port == self.cfg.port => {
                    line(log, format_args!("relay: port {port} mapped ({method}) at {external}"))
                }
                _ => {}
            }
        }
        if !m.settled && m.answered() {
            m.settled = true;
            let range = self.cfg.port_range();
            let relay: Vec<(u16, MappingState)> = m
                .states()
                .into_iter()
                .filter(|(p, _)| self.cfg.ports != (0, 0) && range.contains(p))
                .collect();
            let mapped =
                relay.iter().filter(|(_, s)| matches!(s, MappingState::Mapped { usable: true, .. })).count();
            if !relay.is_empty() {
                line(log, format_args!("relay: {mapped} of {} relay ports mapped", relay.len()));
            }
        }
    }

    /// One mapper (ruling PM-2) for every port, UDP, exact: the listening
    /// port (unless ephemeral) first, then each relay port. `None`: no port
    /// to map.
    fn ask(&self, target: &MapTarget) -> Option<Mapping> {
        let listening = (self.cfg.port != 0).then_some(self.cfg.port);
        let relay = (self.cfg.ports != (0, 0)).then(|| self.cfg.port_range());
        let mut ports = listening.into_iter().chain(relay.into_iter().flatten());
        let first = ports.next()?;
        let mut req = MapRequest::new(SocketAddrV4::new(target.base, first), false, target.gateway);
        req.exact = true;
        req.extra = ports.collect();
        req.allow_local = self.allow_local;
        if let Some(p) = target.pmp_port {
            req.pmp_port = p;
        }
        if let Some(ssdp) = target.ssdp {
            req.ssdp = ssdp;
        }
        req.lifetime = target.lifetime;
        Some(Mapping { first, mapper: PortMapper::start(req), said: BTreeSet::new(), settled: false })
    }

    /// Stops: the closing note (capacity 0, `CLOSE_TTL`; `TurnList` takes
    /// it despite its 10 s gap) when the relay was announced, the TURN
    /// server dropped, and the mappings being deleted.
    pub(crate) fn shutdown(self, log: &mut dyn std::io::Write) -> (Option<Event>, Unmapping) {
        let closing = match &self.phase {
            Phase::Running(r) => r.noted.map(|(_, last)| {
                let note = self.note(r.public, r.server.local_addr().port(), 0, 0);
                line(log, format_args!("relay: closing note published"));
                turn_note(&self.identity, &note, unix_now().max(last + 1), CLOSE_TTL, aux())
            }),
            _ => None,
        };
        drop(self.phase);
        let done = match self.maps {
            Maps::Asked(m) => {
                let (tx, rx) = mpsc::channel();
                let mapper = m.mapper;
                // If no thread starts, the mapper is dropped with the
                // closure (≤ 1 s) and the wait says unconfirmed.
                let _ = std::thread::Builder::new().name("olr-server unmap".into()).spawn(move || {
                    let _ = tx.send(mapper.close());
                });
                Some(rx)
            }
            _ => None,
        };
        (closing, Unmapping { done })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_net::portmap::fake::{FakeGateway, Misbehaviour};

    fn log_text(log: &[u8]) -> String {
        String::from_utf8_lossy(log).into_owned()
    }

    /// Pre-flight fix #11 and the Task 8 review (I1, PM-2): one mapper asks
    /// for every relay port; each is mapped with its external port equal to
    /// the local one, and a port the router would move (held by another
    /// client) is said, never shifted. The relay waits for every answer,
    /// then hands out only the ports mapped exactly and announces at most
    /// that many allocations. The TURN server listens on an ephemeral port
    /// (port 0; the relay ports are only mapped here, never bound). At its
    /// shutdown the closing note goes out and every mapping is deleted,
    /// confirmed by the router.
    #[test]
    fn only_exactly_mapped_relay_ports_are_handed_out() {
        let gw = FakeGateway::start(true, true, true, Ipv4Addr::new(127, 0, 0, 2), Misbehaviour::Honest);
        let first = 49160;
        gw.hold(first + 2);
        let cfg = RelayConfig { port: 0, ports: (first, first + 3), ..RelayConfig::default() };
        let target = MapTarget {
            base: Ipv4Addr::LOCALHOST,
            gateway: Some(Ipv4Addr::LOCALHOST),
            pmp_port: Some(gw.pmp().port()),
            ssdp: Some(gw.ssdp()),
            lifetime: None,
        };
        let mut log = Vec::new();
        let id = Identity::from_secret([7; 32]).unwrap();
        let configured = Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2)));
        let mut relay =
            Relay::new(cfg, id, Region::Europe, true, configured, Some(Some(target)), &mut log).unwrap();
        let begun = Instant::now();
        let mut no_reach = || None;
        let mut notes = Vec::new();
        while notes.is_empty() {
            assert!(begun.elapsed() < Duration::from_secs(8), "{}", log_text(&log));
            notes.extend(relay.poll(Instant::now(), &mut no_reach, &mut log));
            std::thread::sleep(Duration::from_millis(5));
        }
        let text = log_text(&log);
        assert!(text.contains(&format!("relay: port {} not mapped: ", first + 2)), "{text}");
        assert!(text.contains("relay: 3 of 4 relay ports mapped"), "{text}");
        let started = format!(
            "relay ports {first}-{}, {}, announced as 127.0.0.2:{}",
            first + 1,
            first + 3,
            relay.local_addr().unwrap().port()
        );
        assert!(text.contains(&started), "{text}");
        assert!(text.contains("relay: capacity 3: one allocation per relay port"), "{text}");
        let note = olr_discovery::parse_turn_note(&notes[0]).unwrap();
        assert_eq!((note.addr, note.capacity, note.load), (Ipv4Addr::new(127, 0, 0, 2), 3, 0));
        let held: Vec<(bool, u16, u16)> = gw.mappings().iter().map(|m| (m.1, m.2, m.3.port())).collect();
        let exact: Vec<(bool, u16, u16)> =
            [first, first + 1, first + 3].iter().map(|&p| (true, p, p)).collect();
        assert_eq!(held, exact, "UDP, each on its own port");
        let (closing, unmapping) = relay.shutdown(&mut log);
        let closing = olr_discovery::parse_turn_note(&closing.expect("a closing note")).unwrap();
        assert_eq!(closing.capacity, 0);
        assert!(unmapping.wait(Instant::now() + Duration::from_secs(2)), "confirmed");
        assert!(gw.mappings().is_empty(), "{:?}", gw.mappings());
    }

    /// Ruling FR12b-2: a relay announced at its listening port's mapped
    /// address follows the router's new external address (an ISP
    /// renumbering it; renewals every second here): restarted on it, and
    /// announced there at once.
    #[test]
    fn a_relay_follows_its_routers_new_address() {
        let (before, after) = (Ipv4Addr::new(127, 0, 0, 2), Ipv4Addr::new(127, 0, 0, 3));
        let gw = FakeGateway::start(true, false, false, before, Misbehaviour::Honest);
        // A free UDP port to listen on (the mapping is exact: its port).
        let port =
            std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap().local_addr().unwrap().port();
        let cfg = RelayConfig { port, ports: (0, 0), ..RelayConfig::default() };
        let target = MapTarget {
            base: Ipv4Addr::LOCALHOST,
            gateway: None,
            pmp_port: Some(gw.pmp().port()),
            ssdp: Some(gw.ssdp()),
            lifetime: Some(Duration::from_secs(2)),
        };
        let mut log = Vec::new();
        let id = Identity::from_secret([7; 32]).unwrap();
        let mut relay =
            Relay::new(cfg, id, Region::Europe, true, None, Some(Some(target)), &mut log).unwrap();
        let begun = Instant::now();
        let mut no_reach = || None;
        let mut notes = Vec::new();
        let mut moved = false;
        loop {
            assert!(begun.elapsed() < Duration::from_secs(10), "{}", log_text(&log));
            if let Some(note) = relay.poll(Instant::now(), &mut no_reach, &mut log) {
                notes.push(olr_discovery::parse_turn_note(&note).unwrap().addr);
            }
            if !moved && !notes.is_empty() {
                gw.set_external(after);
                moved = true;
            }
            if notes.last() == Some(&after) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(notes, [before, after]);
        let text = log_text(&log);
        assert!(text.contains(&format!("relay: the router's address is now {after}: restarted")), "{text}");
        assert!(text.contains(&format!("announced as {after}:{port}")), "{text}");
        assert_eq!(relay.local_addr().map(|a| a.port()), Some(port));
        let (_, unmapping) = relay.shutdown(&mut log);
        assert!(unmapping.wait(Instant::now() + Duration::from_secs(2)), "confirmed");
    }

    #[test]
    fn port_runs_are_said_compactly() {
        let only = |p: &[u16]| RelayPorts::Only(p.iter().copied().collect());
        assert_eq!(
            ports_text(&only(&[49160, 49161, 49162, 49164, 49166, 49167])),
            "49160-49162, 49164, 49166-49167"
        );
        assert_eq!(ports_text(&RelayPorts::Ephemeral), "ephemeral");
        assert_eq!(ports_text(&RelayPorts::range(49160..=49191)), "49160-49191");
    }
}
