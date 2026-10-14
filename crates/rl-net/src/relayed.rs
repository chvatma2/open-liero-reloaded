//! TURN allocations on the core's socket (decisions N48, N52): a joiner
//! asks at most two relays for a relayed address, offered as a relayed
//! candidate (`Candidate::relayed(relayed, base, "udp")`) to links made
//! afterwards. Sans-IO: one `turn_client_proto::udp::TurnClientUdp` per
//! server, its datagrams handed out with the core's and its answers taken
//! by [`crate::core::Core::receive`].
//!
//! - **Receiving.** A datagram from a relay goes to its client first: data
//!   from a peer reaches the links as arriving at the allocation's relayed
//!   address; what the client ignores goes on to the links.
//! - **Sending.** A link's datagram from a relayed address leaves through
//!   its client (`send_to`), and a channel is bound for each peer address
//!   sent to, so data uses 4-byte ChannelData headers once bound.
//! - **Permissions** are asked for every remote candidate IP when the peer's
//!   SDP arrives (and by each channel bound).
//! - **Release.** An allocation the open link does not use is deleted 5 s
//!   after its `Connected`; a link whose relay dies is lost by ICE.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use turn_client_proto::api::{Transmit, TurnClientApi, TurnConfig, TurnEvent, TurnPollRet, TurnRecvRet};
use turn_client_proto::stun::Instant as TurnInstant;
use turn_client_proto::types::{TransportType, TurnCredentials};
use turn_client_proto::udp::TurnClientUdp;

use rl_proto::is_global;

use crate::candidates::TurnSpec;
use crate::core::{Datagram, SetupId};

/// Where an allocation stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayState {
    Allocating,
    Allocated(SocketAddr),
    /// Refused, unanswered, or the relay stopped answering.
    Failed(String),
    /// Deleted: the link went another way (or the core closed).
    Released,
}

/// Gathering waits at most this long for the allocations.
pub const TURN_WAIT: Duration = Duration::from_secs(2);
/// An allocation the open link does not use is deleted this long after
/// its `Connected` (N48).
pub(crate) const RELEASE_AFTER: Duration = Duration::from_secs(5);
/// At most this many allocations (N48).
const MAX_RELAYS: usize = 2;
/// At most this many allocations are kept, ended ones included.
const MAX_KEPT: usize = 8;
/// At most this many permissions and channels per allocation.
const MAX_PEERS: usize = 32;
/// The setup of the relays' own datagrams: no link's (`Core::sent`
/// ignores them).
const RELAY_SETUP: SetupId = SetupId::MAX;

/// One allocation on one server.
struct Relay {
    server: SocketAddr,
    /// Until the client closes.
    client: Option<TurnClientUdp>,
    /// The socket address the client talks from (datagrams from the server
    /// are given to it as arriving there).
    local: SocketAddr,
    state: RelayState,
    /// The relayed address once allocated (kept after release, so a link
    /// still sending from it is not sent raw).
    addr: Option<SocketAddr>,
    /// Peer IPs with a permission asked for, peer addresses with a channel.
    permitted: BTreeSet<IpAddr>,
    channels: BTreeSet<SocketAddr>,
}

impl Relay {
    /// Takes the client's events: the allocation made or refused. A
    /// relayed address that is one of ours (`own`), or not global (unless
    /// `allow_local`), is refused and the allocation deleted: a hostile
    /// relay must not capture our direct traffic.
    fn events(&mut self, own: &[SocketAddr], allow_local: bool, now: TurnInstant) {
        let Some(client) = &mut self.client else { return };
        while let Some(event) = client.poll_event() {
            match event {
                TurnEvent::AllocationCreated(TransportType::Udp, addr)
                    if self.state == RelayState::Allocating =>
                {
                    let usable = addr.is_ipv4()
                        && !addr.ip().is_unspecified()
                        && addr.port() != 0
                        && !own.contains(&addr)
                        && (allow_local || is_global(addr.ip()));
                    if usable {
                        self.addr = Some(addr);
                        self.state = RelayState::Allocated(addr);
                    } else {
                        let _ = client.delete(now);
                        self.state = RelayState::Failed(format!("the relay gave an unusable address {addr}"));
                    }
                }
                TurnEvent::AllocationCreateFailed(_) if self.state == RelayState::Allocating => {
                    self.state = RelayState::Failed("the relay refused the allocation".into());
                }
                _ => {}
            }
        }
    }
}

/// What a datagram from a relay server was.
pub(crate) enum Received {
    /// The relay's own (an answer, a refresh): taken.
    Consumed,
    /// Data from `peer` to the relayed address `relayed`.
    Peer { relayed: SocketAddr, peer: SocketAddr, bytes: Vec<u8> },
    /// Not a relay's: for the links.
    NotOurs,
}

/// A link's datagram, through a relay or not.
pub(crate) enum Outgoing {
    /// Not from a relayed address: sent as it is.
    Direct,
    /// Wrapped for the relay server `to`.
    Wrapped { to: SocketAddr, bytes: Vec<u8> },
    /// From a relayed address that cannot send now (no permission yet, or
    /// released): dropped, as a lossy path would.
    Dropped,
}

/// The core's allocations.
pub(crate) struct Relays {
    relays: Vec<Relay>,
    /// Our own candidate addresses, never a relayed one.
    own: Vec<SocketAddr>,
    /// N62: local relayed addresses and peers allowed.
    allow_local: bool,
    /// The clients' time zero.
    epoch: Instant,
    /// When the latest `allocate` was called (`TURN_WAIT`).
    started: Option<Instant>,
}

impl Relays {
    pub(crate) fn new(allow_local: bool, now: Instant) -> Relays {
        Relays { relays: Vec::new(), own: Vec::new(), allow_local, epoch: now, started: None }
    }

    /// Our own candidate addresses changed.
    pub(crate) fn set_own(&mut self, own: Vec<SocketAddr>) {
        self.own = own;
    }

    /// Whether relays may be asked to reach `ip` (they refuse non-global
    /// peers, TR-1, unless local ones are allowed).
    fn reachable(&self, ip: IpAddr) -> bool {
        ip.is_ipv4() && (self.allow_local || is_global(ip))
    }

    fn time(&self, now: Instant) -> TurnInstant {
        let nanos = now.saturating_duration_since(self.epoch).as_nanos();
        TurnInstant::from_nanos(i64::try_from(nanos).unwrap_or(i64::MAX))
    }

    /// Allocates on each server not yet asked (IPv4), from `local`, while
    /// fewer than two allocations are live.
    pub(crate) fn allocate(&mut self, servers: &[TurnSpec], local: Option<SocketAddr>, now: Instant) {
        let local = local.unwrap_or((Ipv4Addr::UNSPECIFIED, 0).into());
        // Ended entries go, those that relayed for a link last (their
        // address keeps a link's datagrams from leaving raw), at most a few.
        self.relays.retain(|r| r.client.is_some() || r.addr.is_some());
        while self.relays.len() > MAX_KEPT {
            let Some(i) = self.relays.iter().position(|r| r.client.is_none()) else { break };
            self.relays.remove(i);
        }
        for spec in servers.iter().filter(|s| s.addr.is_ipv4()) {
            let live = self
                .relays
                .iter()
                .filter(|r| matches!(r.state, RelayState::Allocating | RelayState::Allocated(_)))
                .count();
            if live >= MAX_RELAYS {
                break;
            }
            if self.relays.iter().any(|r| r.server == spec.addr && r.client.is_some()) {
                continue;
            }
            let config = TurnConfig::new(TurnCredentials::new(&spec.user, &spec.pass));
            let (client, state) = if local.port() == 0 {
                (None, RelayState::Failed("the socket has no address".into()))
            } else {
                (Some(TurnClientUdp::allocate(local, spec.addr, config)), RelayState::Allocating)
            };
            self.relays.push(Relay {
                server: spec.addr,
                client,
                local,
                state,
                addr: None,
                permitted: BTreeSet::new(),
                channels: BTreeSet::new(),
            });
            self.started = Some(now);
        }
    }

    /// Every allocation settled (allocated, failed or released), or
    /// `TURN_WAIT` passed since `allocate` (true without one).
    pub(crate) fn settled(&self, now: Instant) -> bool {
        self.relays.iter().all(|r| r.state != RelayState::Allocating)
            || self.started.is_some_and(|t| now >= t + TURN_WAIT)
    }

    pub(crate) fn states(&self) -> Vec<(SocketAddr, RelayState)> {
        self.relays.iter().map(|r| (r.server, r.state.clone())).collect()
    }

    /// The relayed addresses allocated now.
    pub(crate) fn addresses(&self) -> Vec<SocketAddr> {
        self.relays
            .iter()
            .filter_map(|r| match r.state {
                RelayState::Allocated(addr) => Some(addr),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn any_allocated(&self) -> bool {
        self.relays.iter().any(|r| matches!(r.state, RelayState::Allocated(_)))
    }

    /// Gives a datagram from `from` to the relay client of that server.
    pub(crate) fn receive(&mut self, from: SocketAddr, bytes: &[u8], now: Instant) -> Received {
        let t = self.time(now);
        for r in self.relays.iter_mut().filter(|r| r.server == from) {
            let Some(client) = &mut r.client else { continue };
            let received = match client.recv(Transmit::new(bytes, TransportType::Udp, from, r.local), t) {
                TurnRecvRet::Ignored(_) => continue,
                TurnRecvRet::PeerData(data) => match r.state {
                    RelayState::Allocated(relayed) => {
                        Received::Peer { relayed, peer: data.peer, bytes: data.data().to_vec() }
                    }
                    _ => Received::Consumed,
                },
                TurnRecvRet::Handled | TurnRecvRet::PeerIcmp { .. } => Received::Consumed,
            };
            r.events(&self.own, self.allow_local, t);
            return received;
        }
        Received::NotOurs
    }

    /// Asks every allocation for permissions to the peer IPs `ips` (IPv4).
    pub(crate) fn permit(&mut self, ips: impl IntoIterator<Item = IpAddr> + Clone, now: Instant) {
        let t = self.time(now);
        let allow_local = self.allow_local;
        for r in &mut self.relays {
            let (RelayState::Allocated(_), Some(client)) = (&r.state, &mut r.client) else { continue };
            for ip in ips.clone().into_iter().filter(|ip| ip.is_ipv4() && (allow_local || is_global(*ip))) {
                if r.permitted.len() < MAX_PEERS && r.permitted.insert(ip) {
                    // Already pending or made (by a channel): nothing to do.
                    let _ = client.create_permission(TransportType::Udp, ip, t);
                }
            }
        }
    }

    /// A link's datagram from `from` to `to`: wrapped when `from` is a
    /// relayed address.
    pub(crate) fn send(&mut self, from: SocketAddr, to: SocketAddr, bytes: &[u8], now: Instant) -> Outgoing {
        let t = self.time(now);
        let reachable = self.reachable(to.ip());
        let Some(r) = self.relays.iter_mut().find(|r| r.addr == Some(from)) else { return Outgoing::Direct };
        if !reachable {
            return Outgoing::Dropped;
        }
        let (RelayState::Allocated(_), Some(client)) = (&r.state, &mut r.client) else {
            return Outgoing::Dropped;
        };
        if r.channels.len() < MAX_PEERS && r.channels.insert(to) {
            r.permitted.insert(to.ip());
            // Already bound, or the allocation is gone: sent unbound.
            let _ = client.bind_channel(TransportType::Udp, to, t);
        }
        match client.send_to(TransportType::Udp, to, bytes, t) {
            Ok(Some(build)) => {
                let tx = build.build();
                Outgoing::Wrapped { to: tx.to, bytes: tx.data }
            }
            _ => Outgoing::Dropped,
        }
    }

    /// Deletes the allocations whose relayed address is not in `used`.
    pub(crate) fn release_unused(&mut self, used: &BTreeSet<SocketAddr>, now: Instant) {
        let t = self.time(now);
        for r in &mut self.relays {
            let (RelayState::Allocated(addr), Some(client)) = (&r.state, &mut r.client) else { continue };
            if !used.contains(addr) {
                let _ = client.delete(t);
                r.state = RelayState::Released;
            }
        }
    }

    /// Deletes every allocation (the core closes).
    pub(crate) fn release_all(&mut self, now: Instant) {
        self.release_unused(&BTreeSet::new(), now);
    }

    /// Steps the clients (timers, refreshes, retransmits) and hands out
    /// their datagrams.
    pub(crate) fn poll(&mut self, now: Instant, out: &mut Vec<Datagram>) {
        let t = self.time(now);
        for r in &mut self.relays {
            let Some(client) = &mut r.client else { continue };
            let mut closed = false;
            for _ in 0..16 {
                match client.poll(t) {
                    TurnPollRet::WaitUntil(_) => break,
                    TurnPollRet::Closed => {
                        closed = true;
                        break;
                    }
                    // TCP allocations only.
                    TurnPollRet::AllocateTcpSocket { .. } | TurnPollRet::TcpClose { .. } => {}
                }
            }
            r.events(&self.own, self.allow_local, t);
            let Some(client) = &mut r.client else { continue };
            while let Some(tx) = client.poll_transmit(t) {
                out.push(Datagram { to: tx.to, bytes: tx.data.as_ref().to_vec(), setup: RELAY_SETUP });
            }
            if closed {
                r.client = None;
                r.state = match &r.state {
                    RelayState::Allocating => RelayState::Failed("the relay did not answer".into()),
                    RelayState::Allocated(_) => RelayState::Failed("the relay stopped answering".into()),
                    state => state.clone(),
                };
            }
        }
    }
}
