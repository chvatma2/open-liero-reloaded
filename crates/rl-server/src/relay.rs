//! A volunteer TURN relay (N52, `rl-server --relay`): our own `TurnServer`
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

use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use rl_discovery::announce::{ANNOUNCE_TTL, CLOSE_TTL, REPUBLISH};
use rl_discovery::list::is_global;
use rl_discovery::turn_note::{TURN_NOTE_V, TurnNote, turn_note};
use rl_discovery::{Event, Region};
use rl_lobby::Identity;
use rl_net::portmap::{MapRequest, MappingState, PortMapper};
use rl_net::probe::NatBehaviour;
use rl_net::turn::{TurnLimits, TurnServer, TurnServerConfig, TurnStats};
use rl_online::Reachability;

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
    /// Tests: the fake's PCP/NAT-PMP port and SSDP socket.
    pub(crate) pmp_port: Option<u16>,
    pub(crate) ssdp: Option<SocketAddr>,
}

/// One port the relay asked its router to open (UDP, exact).
struct Mapping {
    port: u16,
    mapper: PortMapper,
    state: MappingState,
}

/// The relay's mappings: waiting for the base address (found on a thread:
/// the OS's interfaces can take ~350 ms), asked, or none.
enum Maps {
    Finding(Receiver<Option<MapTarget>>),
    Asked(Vec<Mapping>),
    None,
}

/// The running relay and its notes.
struct Running {
    server: TurnServer,
    public: Ipv4Addr,
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
}

/// The relay's mappings being deleted (each mapper's drop on a thread).
pub(crate) struct Unmapping {
    done: Vec<Receiver<()>>,
}

impl Unmapping {
    /// Waits until every mapping is deleted (each mapper waits at most
    /// `portmap::DROP_WAIT`, 1 s) or `until`; `true` when all were.
    pub(crate) fn wait(self, until: Instant) -> bool {
        self.done.into_iter().all(|rx| {
            match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => true,
                Err(RecvTimeoutError::Timeout) => false,
            }
        })
    }
}

/// 16 lowercase hex characters from the system's randomness.
fn credential() -> Option<String> {
    let bytes = rl_net::random_bytes::<8>().ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `first-last`, or "ephemeral" for `(0, 0)`.
fn ports_text(cfg: &RelayConfig) -> String {
    match cfg.ports {
        (0, 0) => "ephemeral".into(),
        (a, b) => format!("{a}-{b}"),
    }
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
                    std::thread::Builder::new().name("rl-server relay base".into()).spawn(move || {
                        let (ips, gateway) = rl_net::local::base_addresses(None);
                        let target =
                            ips.first().map(|&base| MapTarget { base, gateway, pmp_port: None, ssdp: None });
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
        let created = r.noted.map_or(0, |(_, c)| c).max(unix_now());
        r.noted = Some((now, created));
        let (public, port) = (r.public, r.server.local_addr().port());
        let note = self.note(public, port, self.cfg.capacity, load);
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
        match self.configured {
            Some(IpAddr::V4(ip)) => return Some(usable(ip)),
            Some(IpAddr::V6(ip)) => {
                line(log, format_args!("relay: public_address {ip} is IPv6: a relay needs IPv4"));
                self.configured = None;
            }
            None => {}
        }
        // The listening port's exact mapping, once the router answered.
        match &self.maps {
            Maps::Finding(_) => return None,
            Maps::Asked(maps) => match maps.iter().find(|m| m.port == self.cfg.port).map(|m| &m.state) {
                Some(MappingState::Trying) => return None,
                Some(MappingState::Mapped { external, usable: true, .. })
                    if external.port() == self.cfg.port =>
                {
                    return Some(usable(*external.ip()));
                }
                _ => {}
            },
            Maps::None => {}
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

    /// Starts the TURN server on `public`.
    fn start(&mut self, public: Ipv4Addr, log: &mut dyn std::io::Write) {
        let limits = TurnLimits {
            capacity: usize::from(self.cfg.capacity),
            per_ip_allocations: usize::from(self.cfg.allocations_per_ip),
            per_ip_bytes_per_s: self.cfg.user_kib_s.saturating_mul(1024),
            ..TurnLimits::default()
        };
        let started = TurnServer::start(TurnServerConfig {
            listen: SocketAddr::from((Ipv4Addr::UNSPECIFIED, self.cfg.port)),
            public: IpAddr::V4(public),
            ports: self.cfg.port_range(),
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
                        ports_text(&self.cfg)
                    ),
                );
                self.phase = Phase::Running(Box::new(Running {
                    server,
                    public,
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

    /// Asks the router for the ports once the base address is known, and
    /// says each port that cannot be mapped exactly (never shifted: the
    /// relay reports `public:<local port>`).
    fn poll_maps(&mut self, log: &mut dyn std::io::Write) {
        if let Maps::Finding(rx) = &self.maps {
            self.maps = match rx.try_recv() {
                Err(TryRecvError::Empty) => return,
                Ok(Some(target)) => Maps::Asked(self.ask(&target)),
                Ok(None) | Err(TryRecvError::Disconnected) => {
                    line(log, format_args!("relay: ports not mapped: no local address found"));
                    Maps::None
                }
            };
        }
        let Maps::Asked(maps) = &mut self.maps else { return };
        let mut mapped = 0;
        let mut changed = false;
        for m in maps.iter_mut() {
            if let Some(state) = m.mapper.poll() {
                changed = true;
                match &state {
                    MappingState::Failed(why) => {
                        line(log, format_args!("relay: port {} not mapped: {why}", m.port))
                    }
                    MappingState::Mapped { external, usable: false, .. } => line(
                        log,
                        format_args!(
                            "relay: port {} not mapped: the router's address {} is not public (double NAT or \
                             carrier-grade NAT)",
                            m.port,
                            external.ip()
                        ),
                    ),
                    MappingState::Mapped { method, external, .. } if m.port == self.cfg.port => {
                        line(log, format_args!("relay: port {} mapped ({method}) at {external}", m.port))
                    }
                    _ => {}
                }
                m.state = state;
            }
            mapped += usize::from(matches!(m.state, MappingState::Mapped { usable: true, .. }));
        }
        let relay_ports = maps.iter().filter(|m| m.port != self.cfg.port).count();
        if changed && relay_ports > 0 && maps.iter().all(|m| m.state != MappingState::Trying) {
            let listening = usize::from(maps.iter().any(|m| {
                m.port == self.cfg.port && matches!(m.state, MappingState::Mapped { usable: true, .. })
            }));
            line(log, format_args!("relay: {} of {relay_ports} relay ports mapped", mapped - listening));
        }
    }

    /// One exact UDP mapping per port: the listening port (unless
    /// ephemeral) and each relay port.
    fn ask(&self, target: &MapTarget) -> Vec<Mapping> {
        let listening = (self.cfg.port != 0).then_some(self.cfg.port);
        let relay = (self.cfg.ports != (0, 0)).then(|| self.cfg.port_range());
        listening
            .into_iter()
            .chain(relay.into_iter().flatten())
            .map(|port| {
                let mut req = MapRequest::new(SocketAddrV4::new(target.base, port), false, target.gateway);
                req.exact = true;
                req.allow_local = self.allow_local;
                if let Some(p) = target.pmp_port {
                    req.pmp_port = p;
                }
                if let Some(ssdp) = target.ssdp {
                    req.ssdp = ssdp;
                }
                Mapping { port, mapper: PortMapper::start(req), state: MappingState::Trying }
            })
            .collect()
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
            Maps::Asked(maps) => maps
                .into_iter()
                .map(|m| {
                    let (tx, rx) = mpsc::channel();
                    let mapper = m.mapper;
                    // If no thread starts, the mapper is dropped here (≤ 1 s).
                    let _ = std::thread::Builder::new().name("rl-server unmap".into()).spawn(move || {
                        drop(mapper);
                        let _ = tx.send(());
                    });
                    rx
                })
                .collect(),
            _ => Vec::new(),
        };
        (closing, Unmapping { done })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_net::portmap::fake::{FakeGateway, Misbehaviour};

    fn log_text(log: &[u8]) -> String {
        String::from_utf8_lossy(log).into_owned()
    }

    /// Pre-flight fix #11: every relay port is mapped with its external
    /// port equal to the local one; a port the router would move is said
    /// and left unmapped, never shifted. The listening port's mapping gives
    /// the public address (the fake's 127.0.0.2, usable with
    /// `allow_local`): the relay starts and announces it there. At its
    /// shutdown the closing note goes out and the mappings are deleted.
    #[test]
    fn relay_ports_are_mapped_exactly_or_not_at_all() {
        let gw = FakeGateway::start(true, true, true, Ipv4Addr::new(127, 0, 0, 2), Misbehaviour::Honest);
        // A free port for the relay (its relay ports are only mapped here).
        let base =
            std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap().local_addr().unwrap().port();
        let base = base.min(u16::MAX - 3);
        gw.hold(base + 2);
        let cfg = RelayConfig { port: base, ports: (base + 1, base + 3), ..RelayConfig::default() };
        let target = MapTarget {
            base: Ipv4Addr::LOCALHOST,
            gateway: Some(Ipv4Addr::LOCALHOST),
            pmp_port: Some(gw.pmp().port()),
            ssdp: Some(gw.ssdp()),
        };
        let mut log = Vec::new();
        let id = Identity::from_secret([7; 32]).unwrap();
        let mut relay =
            Relay::new(cfg, id, Region::Europe, true, None, Some(Some(target)), &mut log).unwrap();
        let begun = Instant::now();
        let mut no_reach = || None;
        let mut notes = Vec::new();
        while !log_text(&log).contains("relay ports mapped") || notes.is_empty() {
            assert!(begun.elapsed() < Duration::from_secs(8), "{}", log_text(&log));
            notes.extend(relay.poll(Instant::now(), &mut no_reach, &mut log));
            std::thread::sleep(Duration::from_millis(5));
        }
        let text = log_text(&log);
        assert!(text.contains(&format!("relay: port {base} mapped (")), "{text}");
        assert!(text.contains(&format!("relay: port {} not mapped: ", base + 2)), "{text}");
        assert!(text.contains("relay: 2 of 3 relay ports mapped"), "{text}");
        let started = format!("relay ports {}-{}, announced as 127.0.0.2:{base}", base + 1, base + 3);
        assert!(text.contains(&started), "{text}");
        let note = rl_discovery::parse_turn_note(&notes[0]).unwrap();
        assert_eq!(
            (note.addr, note.port, note.capacity, note.load),
            (Ipv4Addr::new(127, 0, 0, 2), base, 32, 0)
        );
        let mut external: Vec<u16> = gw
            .mappings()
            .iter()
            .map(|&(_, udp, port, local)| {
                assert!(udp, "UDP only");
                assert_eq!(port, local.port(), "the external port is the local one");
                port
            })
            .collect();
        external.sort_unstable();
        external.dedup();
        assert_eq!(external, vec![base, base + 1, base + 3]);
        let (closing, unmapping) = relay.shutdown(&mut log);
        let closing = rl_discovery::parse_turn_note(&closing.expect("a closing note")).unwrap();
        assert_eq!((closing.capacity, closing.port), (0, base));
        assert!(unmapping.wait(Instant::now() + Duration::from_secs(2)));
        assert!(gw.mappings().is_empty(), "{:?}", gw.mappings());
    }
}
