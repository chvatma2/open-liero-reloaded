//! `rl-server --nat-check` (N60): what stands between this machine and
//! the players, step by step, for a playtester to read or send: the base
//! address and the gateway, the configured STUN servers' answers and the
//! router's NAT behaviour, the router's port mapping (a mapping made on a
//! random high port, then deleted), and the configured Nostr relays (each
//! one's status and the TURN relay notes seen on them). It always ends
//! with "nat-check done" (exit 0).
//!
//! It uses the config's servers: public ones only when run by hand; the
//! tests point it at in-process ones (`run_with` and a `FakeGateway`).
//! Its wording is stable: `NAT: symmetric NAT` is grepped by the Docker
//! tests (plan 12b Task 11).

use std::collections::BTreeMap;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs as _, UdpSocket};
use std::time::{Duration, Instant};

use rl_discovery::list::is_global;
use rl_discovery::turn_note::{TurnList, parse_turn_note};
use rl_discovery::{Pool, PoolConfig, PoolEvent, RelayStatus, binding_request, parse_binding_response};
use rl_net::portmap::{MapRequest, MappingState, PortMapper, UPNP_DEADLINE};
use rl_net::probe::NatBehaviour;
use rl_online::join::nat_text;
use rl_proto::PublicKey;

use crate::config::Config;
#[cfg(feature = "test-hooks")]
use crate::config::TestGateway;
use crate::unix_now;

/// How long `rl-server --nat-check` waits at each step at most (STUN
/// answers, the relays' answers).
pub const NAT_CHECK_WAIT: Duration = Duration::from_secs(5);
/// An unanswered STUN request is sent once more after this long.
const STUN_RETRY: Duration = Duration::from_millis(500);
/// The router gets its mapper's longest deadline and a second more.
const ROUTER_WAIT: Duration = Duration::from_secs(UPNP_DEADLINE.as_secs() + 1);
/// The subscription id for TURN notes.
const TURN_SUB: &str = "turn";

/// Where the checks aim instead of the machine's own network (tests).
struct Hooks {
    pmp: SocketAddr,
    ssdp: SocketAddr,
    base: Ipv4Addr,
}

/// Runs every check with the config's servers and writes the report to
/// `out`; each wait is bounded by `deadline` (the binary passes
/// [`NAT_CHECK_WAIT`]) but the router's (5 s). An error only when `out`
/// cannot be written.
pub fn run(config: &Config, deadline: Duration, out: &mut dyn Write) -> anyhow::Result<()> {
    check(config, None, deadline, out)
}

/// [`run`] aimed at a `FakeGateway` and a pinned base address (tests).
#[cfg(feature = "test-hooks")]
pub fn run_with(
    config: &Config,
    hooks: Option<&TestGateway>,
    deadline: Duration,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let hooks = hooks.map(|h| Hooks { pmp: h.pmp, ssdp: h.ssdp, base: h.base });
    check(config, hooks, deadline, out)
}

fn check(
    config: &Config,
    hooks: Option<Hooks>,
    deadline: Duration,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    writeln!(out, "nat-check: what stands between this machine and the players")?;
    let servers: Vec<(String, Option<SocketAddr>)> =
        config.stun.iter().map(|s| (s.clone(), resolve(s))).collect();
    // The base address and the gateway.
    let (bases, gateway) = match &hooks {
        Some(h) => (vec![h.base], ip4(h.pmp.ip())),
        None => rl_net::local::base_addresses(servers.iter().find_map(|(_, a)| *a)),
    };
    let list: Vec<String> = bases.iter().map(ToString::to_string).collect();
    writeln!(out, "base address: {}", if list.is_empty() { "none found".into() } else { list.join(", ") })?;
    match gateway {
        Some(g) => writeln!(out, "gateway: {g}")?,
        None => writeln!(out, "gateway: none found (PCP and NAT-PMP cannot be asked)")?,
    }
    let base = bases.first().copied();
    stun(&servers, base.filter(|_| hooks.is_some()), base, deadline, out)?;
    router(config, &hooks, base, gateway, out)?;
    relays(config, deadline, out)?;
    writeln!(out, "nat-check done")?;
    Ok(())
}

fn ip4(ip: IpAddr) -> Option<Ipv4Addr> {
    match ip {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    }
}

/// A `host:port`'s first IPv4 address.
fn resolve(server: &str) -> Option<SocketAddr> {
    server.to_socket_addrs().ok()?.find(SocketAddr::is_ipv4)
}

/// Asks every STUN server from one socket (each once more after 500 ms),
/// says each answer, then the NAT behaviour: `Open` when an answer is our
/// own address, else from the first two answers (N48).
fn stun(
    servers: &[(String, Option<SocketAddr>)],
    bind: Option<Ipv4Addr>,
    base: Option<Ipv4Addr>,
    deadline: Duration,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    if servers.is_empty() {
        writeln!(out, "STUN: no servers configured")?;
        writeln!(out, "NAT: {}", nat_text(NatBehaviour::Unknown))?;
        return Ok(());
    }
    let socket = match UdpSocket::bind((bind.unwrap_or(Ipv4Addr::UNSPECIFIED), 0)) {
        Ok(s) => s,
        Err(e) => {
            writeln!(out, "STUN: no socket ({e})")?;
            return Ok(());
        }
    };
    let local = socket.local_addr().ok().zip(base).map(|(l, b)| SocketAddr::from((b, l.port())));
    let mut asks: Vec<(SocketAddr, [u8; 12], Option<SocketAddr>)> = Vec::new();
    for (_, addr) in servers {
        if let Some(a) = addr
            && !asks.iter().any(|(s, ..)| s == a)
        {
            asks.push((*a, rl_net::random_bytes::<12>().unwrap_or([0; 12]), None));
        }
    }
    let begun = Instant::now();
    let mut sent = 0;
    let mut buf = [0u8; 1500];
    while asks.iter().any(|a| a.2.is_none()) && begun.elapsed() < deadline {
        let due = if sent == 0 { true } else { sent == 1 && begun.elapsed() >= STUN_RETRY };
        if due {
            sent += 1;
            for (to, t, answer) in &asks {
                if answer.is_none() {
                    let _ = socket.send_to(&binding_request(*t), to);
                }
            }
        }
        let _ = socket.set_read_timeout(Some(Duration::from_millis(20)));
        let Ok((len, from)) = socket.recv_from(&mut buf) else { continue };
        if let Some(ask) = asks.iter_mut().find(|a| a.0 == from && a.2.is_none()) {
            ask.2 = parse_binding_response(&buf[..len], &ask.1);
        }
    }
    let mut answers = Vec::new();
    for (name, addr) in servers {
        match addr.and_then(|a| asks.iter().find(|x| x.0 == a)) {
            None => writeln!(out, "STUN {name}: cannot resolve (no IPv4 address)")?,
            Some((_, _, Some(mapped))) => {
                writeln!(out, "STUN {name}: {mapped}")?;
                answers.push(*mapped);
            }
            Some(_) => writeln!(out, "STUN {name}: no answer")?,
        }
    }
    let nat = judge(&answers, local);
    writeln!(out, "NAT: {}", nat_text(nat))?;
    if nat == NatBehaviour::Unknown {
        writeln!(out, "(two STUN servers must answer to tell the router's behaviour)")?;
    }
    Ok(())
}

/// N48's judgement of one round: our own address seen means no NAT; two
/// answers alike, a NAT that keeps ports; different, a symmetric NAT.
fn judge(answers: &[SocketAddr], local: Option<SocketAddr>) -> NatBehaviour {
    if answers.iter().any(|a| Some(*a) == local) {
        return NatBehaviour::Open;
    }
    match answers {
        [a, b, ..] if a == b => NatBehaviour::Preserving,
        [_, _, ..] => NatBehaviour::Symmetric,
        _ => NatBehaviour::Unknown,
    }
}

/// A mapping of a random high UDP port, its result, then its deletion.
fn router(
    config: &Config,
    hooks: &Option<Hooks>,
    base: Option<Ipv4Addr>,
    gateway: Option<Ipv4Addr>,
    out: &mut dyn Write,
) -> anyhow::Result<()> {
    let Some(base) = base else {
        writeln!(out, "router: not asked (no local address)")?;
        return Ok(());
    };
    let random = rl_net::random_bytes::<2>().map_or(0, u16::from_le_bytes);
    let port = 49152 + random % 16384;
    writeln!(out, "router: asking it to open UDP port {port}")?;
    let mut req = MapRequest::new(SocketAddrV4::new(base, port), false, gateway);
    req.exact = true;
    req.allow_local = config.allow_local;
    if let Some(h) = hooks {
        req.pmp_port = h.pmp.port();
        req.ssdp = h.ssdp;
    }
    let mut mapper = PortMapper::start(req);
    let begun = Instant::now();
    while *mapper.state() == MappingState::Trying && begun.elapsed() < ROUTER_WAIT {
        let _ = mapper.poll();
        std::thread::sleep(Duration::from_millis(10));
    }
    let state = mapper.state().clone();
    match &state {
        MappingState::Mapped { method, external, usable, .. } => {
            writeln!(out, "router: port {port} opened ({method}) at {external}")?;
            if !usable {
                writeln!(
                    out,
                    "router: but its address {} is not public (double NAT or carrier-grade NAT): players cannot \
                     reach the opened port",
                    external.ip()
                )?;
            }
        }
        MappingState::Failed(why) => writeln!(out, "router: port {port} not opened: {why}")?,
        MappingState::Trying | MappingState::Off => {
            writeln!(out, "router: no answer within {} s", ROUTER_WAIT.as_secs())?
        }
    }
    // Deleted, waiting at most 1 s for the router to confirm it.
    let confirmed = mapper.close();
    match (&state, confirmed) {
        (MappingState::Mapped { .. }, true) => writeln!(out, "router: the mapping is deleted")?,
        (MappingState::Mapped { .. }, false) => {
            writeln!(out, "router: deleting the mapping: the router did not confirm it within 1 s")?
        }
        _ => {}
    }
    Ok(())
}

/// Each relay's status within `deadline` (sooner once every relay
/// answered), and the TURN relays announced on them.
fn relays(config: &Config, deadline: Duration, out: &mut dyn Write) -> anyhow::Result<()> {
    if config.relays.is_empty() {
        writeln!(out, "relays: none configured")?;
        return Ok(());
    }
    let mut pool = Pool::start(PoolConfig { relays: config.relays.clone(), ..PoolConfig::default() });
    pool.subscribe(TURN_SUB, vec![TurnList::filter(unix_now())]);
    let n = config.relays.len();
    let mut status: Vec<Option<String>> = vec![None; n];
    let mut eose = vec![false; n];
    // Each relay's latest note by key: its time and the note.
    let mut notes: BTreeMap<PublicKey, (u64, rl_discovery::TurnNote)> = BTreeMap::new();
    let mut events = Vec::new();
    let begun = Instant::now();
    let settled = |status: &[Option<String>], eose: &[bool]| {
        status.iter().zip(eose).all(|(s, e)| matches!(s.as_deref(), Some(s) if s != "connected") || *e)
    };
    while begun.elapsed() < deadline && !settled(&status, &eose) {
        pool.poll(&mut events);
        for e in events.drain(..) {
            match e {
                PoolEvent::Status { relay, status: s } if relay < n => match s {
                    RelayStatus::Connected => status[relay] = Some("connected".into()),
                    RelayStatus::Failed(why) | RelayStatus::Dropped(why) if status[relay].is_none() => {
                        status[relay] = Some(format!("unreachable: {why}"));
                    }
                    _ => {}
                },
                PoolEvent::Eose { relay, .. } if relay < n => eose[relay] = true,
                PoolEvent::Event { event, .. } => {
                    if let Ok(note) = parse_turn_note(&event)
                        && notes.get(&event.pubkey).is_none_or(|(t, _)| *t < event.created_at)
                    {
                        notes.insert(event.pubkey, (event.created_at, note));
                    }
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    for (url, s) in config.relays.iter().zip(&status) {
        match s {
            Some(s) => writeln!(out, "relay {url}: {s}")?,
            None => writeln!(out, "relay {url}: no answer within {} s", deadline.as_secs_f32())?,
        }
    }
    let open: Vec<&rl_discovery::TurnNote> =
        notes.values().map(|(_, n)| n).filter(|n| n.capacity > 0).collect();
    writeln!(out, "TURN notes: {}", open.len())?;
    for note in open {
        let local = if is_global(IpAddr::V4(note.addr)) { "" } else { " [not a public address]" };
        writeln!(
            out,
            "TURN relay {}:{} ({:?}, {} of {} in use){local}",
            note.addr, note.port, note.region, note.load, note.capacity
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_discovery::announce::ANNOUNCE_TTL;
    use rl_discovery::turn_note::{TURN_NOTE_V, TurnNote, turn_note};
    use rl_discovery::{Region, StunResponder, TestRelay};
    use rl_lobby::Identity;
    use rl_net::portmap::fake::{FakeGateway, Misbehaviour};
    use rl_net::turn::{RelayPorts, TurnLimits, TurnServer, TurnServerConfig};

    fn hooks(gw: &FakeGateway) -> TestGateway {
        TestGateway { pmp: gw.pmp(), ssdp: gw.ssdp(), base: Ipv4Addr::LOCALHOST, lifetime: None }
    }

    fn report(config: &Config, gw: &FakeGateway) -> String {
        let mut out = Vec::new();
        run_with(config, Some(&hooks(gw)), Duration::from_secs(3), &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// N60 with in-process fakes only: the base and gateway, STUN (a
    /// `TurnServer` answers our own address: no NAT), the router (a
    /// `FakeGateway`'s mapping made and deleted), a relay and the TURN
    /// note on it; then two STUN servers seeing different ports:
    /// "NAT: symmetric NAT" (Task 11 greps it).
    #[test]
    fn nat_check_reports_every_step() {
        let relay = TestRelay::start();
        let note = TurnNote {
            v: TURN_NOTE_V,
            addr: Ipv4Addr::new(81, 2, 69, 142),
            port: 3478,
            region: Region::Europe,
            capacity: 32,
            load: 3,
            user: "u".into(),
            pass: "p".into(),
        };
        let event =
            turn_note(&Identity::from_secret([5; 32]).unwrap(), &note, unix_now(), ANNOUNCE_TTL, [1; 32]);
        let mut publisher = Pool::start(PoolConfig { relays: vec![relay.url()], ..PoolConfig::default() });
        publisher.publish(event.clone());
        let begun = Instant::now();
        while !relay.events().iter().any(|e| e.id == event.id) {
            assert!(begun.elapsed() < Duration::from_secs(5), "the note is stored");
            std::thread::sleep(Duration::from_millis(5));
        }
        let turn = TurnServer::start(TurnServerConfig {
            listen: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            public: IpAddr::V4(Ipv4Addr::LOCALHOST),
            ports: RelayPorts::Ephemeral,
            user: "u".into(),
            pass: "p".into(),
            limits: TurnLimits::generous(),
            allow_local: true,
        })
        .unwrap();
        let gw = FakeGateway::start(true, true, true, Ipv4Addr::new(203, 0, 113, 7), Misbehaviour::Honest);
        let config = Config {
            stun: vec![turn.local_addr().to_string()],
            relays: vec![relay.url()],
            ..Config::default()
        };
        let text = report(&config, &gw);
        for want in [
            "base address: 127.0.0.1\n",
            "gateway: 127.0.0.1\n",
            &format!("STUN {}: 127.0.0.1:", turn.local_addr()),
            "NAT: no NAT\n",
            "router: asking it to open UDP port ",
            "opened (",
            "router: but its address 203.0.113.7 is not public",
            "router: the mapping is deleted\n",
            &format!("relay {}: connected\n", relay.url()),
            "TURN notes: 1\n",
            "TURN relay 81.2.69.142:3478 (Europe, 3 of 32 in use)\n",
            "nat-check done\n",
        ] {
            assert!(text.contains(want), "{want:?} in:\n{text}");
        }
        assert!(gw.mappings().is_empty(), "deleted: {:?}", gw.mappings());

        // Two servers, two mapped ports: a symmetric NAT. No relays.
        let a = StunResponder::start(SocketAddr::from(([81, 2, 69, 142], 40000)));
        let b = StunResponder::start(SocketAddr::from(([81, 2, 69, 142], 40001)));
        let config = Config { stun: vec![a.server(), b.server()], relays: vec![], ..Config::default() };
        let text = report(&config, &gw);
        for want in [
            &format!("STUN {}: 81.2.69.142:40000\n", a.server()),
            &format!("STUN {}: 81.2.69.142:40001\n", b.server()),
            "NAT: symmetric NAT\n",
            "relays: none configured\n",
            "nat-check done\n",
        ] {
            assert!(text.contains(want), "{want:?} in:\n{text}");
        }
        // A router that deletes only `LATE` after the request: "deleted" is
        // said once it confirmed, and nothing is left then.
        let late =
            FakeGateway::start(true, false, false, Ipv4Addr::new(203, 0, 113, 7), Misbehaviour::LateUpnp);
        let config = Config { stun: vec![], relays: vec![], allow_local: true, ..Config::default() };
        let text = report(&config, &late);
        assert!(
            text.contains("opened (UPnP)") && text.contains("router: the mapping is deleted\n"),
            "{text}"
        );
        assert!(late.mappings().is_empty(), "{:?}", late.mappings());
        // No STUN server: said, and the NAT not known.
        let config = Config { stun: vec![], relays: vec![], ..Config::default() };
        let text = report(&config, &gw);
        assert!(text.contains("STUN: no servers configured\nNAT: not known yet\n"), "{text}");
    }

    #[test]
    fn the_nat_is_judged_from_two_answers() {
        let a = |s: &str| s.parse::<SocketAddr>().unwrap();
        let local = Some(a("192.168.1.5:5000"));
        assert_eq!(judge(&[a("192.168.1.5:5000")], local), NatBehaviour::Open);
        assert_eq!(judge(&[a("81.2.69.142:5000"), a("81.2.69.142:5000")], local), NatBehaviour::Preserving);
        assert_eq!(judge(&[a("81.2.69.142:5000"), a("81.2.69.142:6000")], local), NatBehaviour::Symmetric);
        assert_eq!(judge(&[a("81.2.69.142:5000")], local), NatBehaviour::Unknown);
        assert_eq!(judge(&[], local), NatBehaviour::Unknown);
    }
}
