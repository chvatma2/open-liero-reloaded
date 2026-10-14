//! A virtual network of routers (decision N56, feature `test-hooks`):
//! [`Core`]s behind simulated NATs, STUN servers and a public network, on a
//! virtual clock, so hole punching, mapped ports and their failures are
//! tested in `check` without sockets or Docker.
//!
//! - **NATs** (RFC 4787 terms). A datagram from a node behind a NAT gets
//!   an external port: an endpoint-independent mapping (full cone,
//!   address-restricted, port-restricted) keeps the internal port if free,
//!   else the next free one; a symmetric NAT makes a mapping per
//!   destination, its port from the seeded RNG in 1024..65535. Inbound
//!   datagrams to an external port pass the filter: full cone, anyone;
//!   address-restricted, an IP the mapping sent to; port-restricted and
//!   symmetric, the exact address it sent to. A [`Net::forward`] (what a
//!   successful UPnP gives) passes everything. A mapping idle (no
//!   outbound datagram) for 120 s expires. A datagram from inside to the
//!   NAT's own public IP is dropped (no hairpinning); nodes behind the same
//!   NAT reach each other's private addresses directly.
//! - **Delivery** takes 10 ms per hop (sender to wire, through each NAT),
//!   in (time, sequence) order, without loss. A STUN server answers a
//!   binding request with the source it observed.
//! - **Time** advances in 1 ms steps ([`Net::run_until`]); every node's
//!   core is polled at each step and every datagram it sends is reported
//!   sent (`Core::sent`).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use rl_netcode::Rng;
use rl_proto::stun::{binding_request_transaction, binding_response};
use rl_proto::{PROTOCOL_VERSION, Path, Refusal, Signal};

use crate::candidates::TurnSpec;
use crate::core::{Core, CoreConfig, CoreEvent, HOST, NetEvent, Why};

/// One hop's delay.
const HOP: Duration = Duration::from_millis(10);
/// The clock's step.
const STEP: Duration = Duration::from_millis(1);
/// A NAT mapping without outbound traffic for this long is dropped.
const MAPPING_IDLE: Duration = Duration::from_secs(120);
/// The lowest port a NAT maps to.
const LOW_PORT: u16 = 1024;

/// A router's mapping and filtering behaviour (RFC 4787).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NatKind {
    /// Endpoint-independent mapping and filtering.
    FullCone,
    /// Endpoint-independent mapping, address-dependent filtering.
    AddressRestricted,
    /// Endpoint-independent mapping, address-and-port-dependent filtering.
    PortRestricted,
    /// Address-and-port-dependent mapping and filtering.
    Symmetric,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NatId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(pub usize);

struct Mapping {
    internal: SocketAddr,
    /// A symmetric NAT's mapping is for one destination.
    dest: Option<SocketAddr>,
    port: u16,
    /// Where it sent datagrams (the filter's memory).
    sent: BTreeSet<SocketAddr>,
    last: Instant,
}

struct Nat {
    kind: NatKind,
    public: Ipv4Addr,
    mappings: Vec<Mapping>,
    /// External port to internal address.
    forwards: BTreeMap<u16, SocketAddr>,
}

struct Node {
    core: Core,
    at: SocketAddr,
    behind: Option<NatId>,
    events: Vec<NetEvent>,
    /// `Connected` to `HOST` and not disconnected since (a client).
    connected: bool,
}

/// A datagram on its way to `to`: on the public network, or (`lan`) to a
/// node behind the sender's NAT.
struct Flight {
    from: SocketAddr,
    to: SocketAddr,
    bytes: Vec<u8>,
    lan: Option<usize>,
}

/// The virtual network.
pub struct Net {
    rng: Rng,
    now: Instant,
    nats: Vec<Nat>,
    servers: Vec<SocketAddr>,
    nodes: Vec<Node>,
    flights: BTreeMap<(Instant, u64), Flight>,
    seq: u64,
}

impl Net {
    pub fn new(seed: u64) -> Net {
        Net {
            rng: Rng::new(seed),
            now: Instant::now(),
            nats: Vec::new(),
            servers: Vec::new(),
            nodes: Vec::new(),
            flights: BTreeMap::new(),
            seq: 0,
        }
    }

    pub fn nat(&mut self, kind: NatKind, public: Ipv4Addr) -> NatId {
        self.nats.push(Nat { kind, public, mappings: Vec::new(), forwards: BTreeMap::new() });
        NatId(self.nats.len() - 1)
    }

    /// A router mapping (what a successful UPnP gives): `external_port` on the NAT to `to`.
    pub fn forward(&mut self, nat: NatId, external_port: u16, to: SocketAddr) {
        if let Some(n) = self.nats.get_mut(nat.0) {
            n.forwards.insert(external_port, to);
        }
    }

    pub fn stun_server(&mut self, at: SocketAddr) -> SocketAddr {
        self.servers.push(at);
        at
    }

    /// A host core (CoreConfig::host()) at `at`, behind `nat` (or on the public network).
    /// Nodes are numbered in creation order from 0.
    pub fn host(&mut self, behind: Option<NatId>, at: SocketAddr) -> NodeId {
        self.node(CoreConfig::host(), behind, at)
    }

    pub fn client(&mut self, behind: Option<NatId>, at: SocketAddr) -> NodeId {
        self.node(CoreConfig::client(), behind, at)
    }

    fn node(&mut self, cfg: CoreConfig, behind: Option<NatId>, at: SocketAddr) -> NodeId {
        // Each node's seeds come from the net's.
        let cfg = CoreConfig { impair_seed: self.rng.next_u64(), stun_seed: self.rng.next_u64(), ..cfg };
        let mut core = Core::new(cfg, self.now);
        core.set_base(at, vec![at]);
        self.nodes.push(Node { core, at, behind, events: Vec::new(), connected: false });
        NodeId(self.nodes.len() - 1)
    }

    pub fn core(&mut self, n: NodeId) -> &mut Core {
        &mut self.nodes[n.0].core
    }

    pub fn now(&self) -> Instant {
        self.now
    }

    /// core.stun(servers) (and, from Task 3, core.allocate(turn)).
    pub fn gather(&mut self, n: NodeId, stun: &[SocketAddr], turn: &[TurnSpec]) {
        let _ = turn;
        let now = self.now;
        self.nodes[n.0].core.stun(stun.to_vec(), now);
    }

    pub fn gathered(&self, n: NodeId) -> bool {
        self.nodes[n.0].core.gathered(self.now)
    }

    /// Where the public network reaches node `n` (its NAT's IP and its own
    /// port behind one): the client's hint of its host.
    fn public_face(&self, n: NodeId) -> SocketAddr {
        let node = &self.nodes[n.0];
        match node.behind {
            Some(nat) => SocketAddr::new(IpAddr::V4(self.nats[nat.0].public), node.at.port()),
            None => node.at,
        }
    }

    /// offer_ice on the client, answer_ice on the host, accept_answer_ice: the signalling Nostr would carry.
    pub fn signal(&mut self, client: NodeId, host: NodeId) -> Result<(), Refusal> {
        let now = self.now;
        let hint = self.public_face(host);
        let c = &mut self.nodes[client.0].core;
        let (setup, sdp) = c.offer_ice(hint, now);
        let offer = Signal::Offer { protocol: PROTOCOL_VERSION, fingerprint: c.fingerprint(), sdp };
        let client_mapped = c.local().mapped;
        let h = &mut self.nodes[host.0].core;
        let answer = h.answer_ice(&offer, client_mapped, now);
        let host_mapped = h.local().mapped;
        let node = &mut self.nodes[client.0];
        let refused = match answer {
            Signal::Answer { sdp } if node.core.accept_answer_ice(setup, &sdp, host_mapped).is_ok() => {
                return Ok(());
            }
            Signal::Refused { reason } => reason,
            _ => Refusal::Malformed,
        };
        // As the endpoint does: `Refused` is all the client hears.
        node.core.cancel(setup, Why::Refused, &mut Vec::new());
        node.events.push(NetEvent::Refused { why: refused });
        Err(refused)
    }

    /// Advances the clock in 1 ms steps, delivering datagrams, until `done` or `limit`; true when `done`.
    pub fn run_until(&mut self, limit: Duration, mut done: impl FnMut(&mut Net) -> bool) -> bool {
        let end = self.now + limit;
        loop {
            if done(self) {
                return true;
            }
            if self.now >= end {
                return false;
            }
            self.step();
        }
    }

    /// The path of the client's link to its host once Connected (None before, or after a failure).
    pub fn connected(&self, client: NodeId) -> Option<Path> {
        let node = &self.nodes[client.0];
        if node.connected { node.core.path(HOST) } else { None }
    }

    /// Every NetEvent the node reported so far (kept; not drained).
    pub fn events(&mut self, n: NodeId) -> Vec<NetEvent> {
        self.nodes[n.0].events.clone()
    }

    /// The ISP gives the NAT a new public IP: its mappings are dropped.
    pub fn renumber(&mut self, nat: NatId, public: Ipv4Addr) {
        if let Some(n) = self.nats.get_mut(nat.0) {
            n.public = public;
            n.mappings.clear();
        }
    }

    /// One clock step: expiries, arrivals, then every node's poll.
    fn step(&mut self) {
        self.now += STEP;
        let now = self.now;
        for nat in &mut self.nats {
            nat.mappings.retain(|m| now.saturating_duration_since(m.last) < MAPPING_IDLE);
        }
        while let Some(entry) = self.flights.first_entry() {
            if entry.key().0 > now {
                break;
            }
            let flight = entry.remove();
            self.arrive(flight);
        }
        for i in 0..self.nodes.len() {
            let (mut out, mut events) = (Vec::new(), Vec::new());
            self.nodes[i].core.poll(now, &mut out, &mut events);
            for d in out {
                self.nodes[i].core.sent(&d, true);
                self.send(i, d.to, d.bytes);
            }
            let node = &mut self.nodes[i];
            for event in events {
                let CoreEvent::Net(event) = event else { continue };
                match event {
                    NetEvent::Connected { peer: HOST, .. } => node.connected = true,
                    NetEvent::Disconnected { peer: HOST, .. } => node.connected = false,
                    _ => {}
                }
                node.events.push(event);
            }
        }
    }

    /// Puts a datagram on the wire: `hops` from its sender, one more into
    /// a NAT on the far side.
    fn fly(&mut self, from: SocketAddr, to: SocketAddr, bytes: Vec<u8>, hops: u32, lan: Option<usize>) {
        let inbound = lan.is_none() && self.nats.iter().any(|n| IpAddr::V4(n.public) == to.ip());
        let at = self.now + HOP * (hops + u32::from(inbound));
        self.seq += 1;
        self.flights.insert((at, self.seq), Flight { from, to, bytes, lan });
    }

    /// Node `i` sends `bytes` to `to`.
    fn send(&mut self, i: usize, to: SocketAddr, bytes: Vec<u8>) {
        let (at, behind) = (self.nodes[i].at, self.nodes[i].behind);
        let Some(nat) = behind else { return self.fly(at, to, bytes, 1, None) };
        if let Some(j) = self.nodes.iter().position(|n| n.behind == Some(nat) && n.at == to) {
            return self.fly(at, to, bytes, 1, Some(j));
        }
        let public = self.nats[nat.0].public;
        if to.ip() == IpAddr::V4(public) {
            return; // No hairpinning.
        }
        if let Some(port) = self.map_out(nat.0, at, to) {
            self.fly(SocketAddr::new(IpAddr::V4(public), port), to, bytes, 2, None);
        }
    }

    /// A datagram arrives: at a node on the LAN, a STUN server, a public
    /// node, or a NAT that lets it through (or not).
    fn arrive(&mut self, f: Flight) {
        let now = self.now;
        if let Some(j) = f.lan {
            return self.nodes[j].core.receive(f.from, &f.bytes, now);
        }
        if self.servers.contains(&f.to) {
            if let Some(transaction) = binding_request_transaction(&f.bytes) {
                let answer = binding_response(&transaction, f.from, true);
                self.fly(f.to, f.from, answer, 1, None);
            }
            return;
        }
        if let Some(node) = self.nodes.iter_mut().find(|n| n.behind.is_none() && n.at == f.to) {
            return node.core.receive(f.from, &f.bytes, now);
        }
        let Some(k) = self.nats.iter().position(|n| IpAddr::V4(n.public) == f.to.ip()) else { return };
        let Some(internal) = self.map_in(k, f.from, f.to.port()) else { return };
        if let Some(node) = self.nodes.iter_mut().find(|n| n.behind == Some(NatId(k)) && n.at == internal) {
            node.core.receive(f.from, &f.bytes, now);
        }
    }

    /// Whether NAT `k` may map `port` for `internal`: no mapping holds it
    /// and no forward to another address does.
    fn free(&self, k: usize, port: u16, internal: SocketAddr) -> bool {
        let nat = &self.nats[k];
        port >= LOW_PORT
            && !nat.mappings.iter().any(|m| m.port == port)
            && nat.forwards.get(&port).is_none_or(|to| *to == internal)
    }

    /// The external port of NAT `k` for a datagram from `internal` to
    /// `dest` (a mapping made or refreshed); `None` when every port is taken.
    fn map_out(&mut self, k: usize, internal: SocketAddr, dest: SocketAddr) -> Option<u16> {
        let now = self.now;
        let key = (self.nats[k].kind == NatKind::Symmetric).then_some(dest);
        if let Some(m) = self.nats[k].mappings.iter_mut().find(|m| m.internal == internal && m.dest == key) {
            m.last = now;
            m.sent.insert(dest);
            return Some(m.port);
        }
        let span = u64::from(u16::MAX - LOW_PORT);
        let port = if key.is_some() {
            // A random port, or the first free one after it.
            let start = LOW_PORT + self.rng.below(span) as u16;
            self.next_free(k, start, internal)?
        } else {
            self.next_free(k, internal.port(), internal)?
        };
        let sent = BTreeSet::from([dest]);
        self.nats[k].mappings.push(Mapping { internal, dest: key, port, sent, last: now });
        Some(port)
    }

    /// The first free port from `start` up, wrapping past 65535 to 1024.
    fn next_free(&self, k: usize, start: u16, internal: SocketAddr) -> Option<u16> {
        let start = start.max(LOW_PORT);
        (start..=u16::MAX).chain(LOW_PORT..start).find(|&p| self.free(k, p, internal))
    }

    /// Where NAT `k` lets a datagram from `from` to its `port` in, if its
    /// filter passes it.
    fn map_in(&self, k: usize, from: SocketAddr, port: u16) -> Option<SocketAddr> {
        let nat = &self.nats[k];
        if let Some(&to) = nat.forwards.get(&port) {
            return Some(to);
        }
        let m = nat.mappings.iter().find(|m| m.port == port)?;
        let pass = match nat.kind {
            NatKind::FullCone => true,
            NatKind::AddressRestricted => m.sent.iter().any(|s| s.ip() == from.ip()),
            NatKind::PortRestricted | NatKind::Symmetric => m.sent.contains(&from),
        };
        pass.then_some(m.internal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    const PUBLIC: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 10);

    /// A NAT of `kind` whose node at 192.168.1.10:5000 sent to a server.
    fn sent(kind: NatKind) -> (Net, usize, u16) {
        let mut net = Net::new(1);
        let nat = net.nat(kind, PUBLIC).0;
        let port = net.map_out(nat, a("192.168.1.10:5000"), a("198.51.100.1:3478")).unwrap();
        (net, nat, port)
    }

    #[test]
    fn a_port_restricted_nat_drops_an_unsolicited_datagram() {
        let (net, nat, port) = sent(NatKind::PortRestricted);
        assert_eq!(port, 5000, "the internal port is kept");
        assert_eq!(net.map_in(nat, a("198.51.100.1:3478"), port), Some(a("192.168.1.10:5000")));
        assert_eq!(net.map_in(nat, a("198.51.100.1:3479"), port), None, "another port of the server");
        assert_eq!(net.map_in(nat, a("198.51.100.9:40000"), port), None, "a stranger");
        assert_eq!(net.map_in(nat, a("198.51.100.1:3478"), 5001), None, "no mapping");
    }

    #[test]
    fn a_full_cone_passes_an_unsolicited_datagram() {
        let (net, nat, port) = sent(NatKind::FullCone);
        assert_eq!(net.map_in(nat, a("198.51.100.9:40000"), port), Some(a("192.168.1.10:5000")));
        let (net, nat, port) = sent(NatKind::AddressRestricted);
        assert_eq!(net.map_in(nat, a("198.51.100.1:9999"), port), Some(a("192.168.1.10:5000")));
        assert_eq!(net.map_in(nat, a("198.51.100.9:3478"), port), None);
    }

    #[test]
    fn a_symmetric_nat_gives_two_destinations_two_ports() {
        let (mut net, nat, port) = sent(NatKind::Symmetric);
        let other = net.map_out(nat, a("192.168.1.10:5000"), a("198.51.100.2:3478")).unwrap();
        assert_ne!(port, other);
        assert!(port >= LOW_PORT && other >= LOW_PORT);
        // Each port answers only its own destination.
        assert_eq!(net.map_in(nat, a("198.51.100.2:3478"), port), None);
        assert_eq!(net.map_in(nat, a("198.51.100.2:3478"), other), Some(a("192.168.1.10:5000")));
        // The same destination keeps its mapping; an endpoint-independent
        // NAT gives both destinations one port.
        assert_eq!(net.map_out(nat, a("192.168.1.10:5000"), a("198.51.100.1:3478")), Some(port));
        let (mut net, nat, port) = sent(NatKind::PortRestricted);
        assert_eq!(net.map_out(nat, a("192.168.1.10:5000"), a("198.51.100.2:3478")), Some(port));
    }

    #[test]
    fn a_taken_port_moves_on_and_a_forward_passes_everything() {
        let (mut net, nat, port) = sent(NatKind::PortRestricted);
        let second = net.map_out(nat, a("192.168.1.11:5000"), a("198.51.100.1:3478")).unwrap();
        assert_eq!((port, second), (5000, 5001));
        net.forward(NatId(nat), 23400, a("192.168.1.12:23400"));
        assert_eq!(net.map_in(nat, a("198.51.100.9:1"), 23400), Some(a("192.168.1.12:23400")));
        // The forward's own address maps to its port; another takes the next.
        assert_eq!(net.map_out(nat, a("192.168.1.13:23400"), a("198.51.100.1:3478")), Some(23401));
        assert_eq!(net.map_out(nat, a("192.168.1.12:23400"), a("198.51.100.1:3478")), Some(23400));
    }

    /// Each node's core takes its seeds from the net's: two nodes' STUN
    /// transactions differ.
    #[test]
    fn nodes_are_seeded_apart() {
        let mut net = Net::new(1);
        let server = net.stun_server(a("198.51.100.1:3478"));
        let (x, y) = (net.client(None, a("203.0.113.1:5000")), net.client(None, a("203.0.113.2:5000")));
        let now = net.now();
        let mut requests = Vec::new();
        for n in [x, y] {
            net.core(n).stun(vec![server], now);
            let mut out = Vec::new();
            net.core(n).poll(now, &mut out, &mut Vec::new());
            requests.push(out.remove(0).bytes);
        }
        assert_ne!(requests[0][8..20], requests[1][8..20]);
    }

    #[test]
    fn an_idle_mapping_expires() {
        let (mut net, nat, port) = sent(NatKind::FullCone);
        net.run_until(MAPPING_IDLE - Duration::from_secs(1), |_| false);
        assert!(net.map_in(nat, a("198.51.100.9:1"), port).is_some());
        net.run_until(Duration::from_secs(1), |_| false);
        assert_eq!(net.map_in(nat, a("198.51.100.9:1"), port), None);
    }
}
