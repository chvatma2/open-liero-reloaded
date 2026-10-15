//! A deterministic network model (spec §9.1): per-direction latency, jitter,
//! bursty loss, duplication and outages, seeded and clock-free.

use alloc::collections::{BTreeMap, BinaryHeap};
use alloc::vec::Vec;
use core::cmp::Reverse;

use crate::{Channel, MAX_CATCHUP, PeerId, Rng, TICK_US};

/// One network condition. A profile's latency is the ping: each direction
/// takes half of it, with half of the jitter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub name: &'static str,
    pub ping_ms: u32,
    /// Plus or minus, on the ping.
    pub jitter_ms: u32,
    /// Mean loss rate.
    pub loss: f64,
    /// Mean length of a run of lost datagrams.
    pub burst: f64,
    /// Chance of a second copy of a surviving datagram.
    pub duplicate: f64,
}

impl Profile {
    pub const LAN: Profile =
        Profile { name: "lan", ping_ms: 1, jitter_ms: 0, loss: 0.0, burst: 1.0, duplicate: 0.0 };
    pub const GOOD: Profile =
        Profile { name: "good", ping_ms: 30, jitter_ms: 5, loss: 0.0, burst: 1.0, duplicate: 0.0 };
    pub const TYPICAL: Profile =
        Profile { name: "typical", ping_ms: 80, jitter_ms: 20, loss: 0.01, burst: 1.0, duplicate: 0.0 };
    pub const BAD: Profile =
        Profile { name: "bad", ping_ms: 150, jitter_ms: 50, loss: 0.05, burst: 2.0, duplicate: 0.01 };
    pub const AWFUL: Profile =
        Profile { name: "awful", ping_ms: 300, jitter_ms: 100, loss: 0.15, burst: 4.0, duplicate: 0.02 };
    pub const ALL: [Profile; 5] = [Self::LAN, Self::GOOD, Self::TYPICAL, Self::BAD, Self::AWFUL];

    pub fn by_name(name: &str) -> Option<Profile> {
        Self::ALL.into_iter().find(|p| p.name == name)
    }

    pub fn rtt_ms(&self) -> u32 {
        self.ping_ms
    }

    /// How many ticks past the end of an outage of `outage_ticks` a player
    /// cut off by it may still stall on this link, its frames every
    /// `frame_us` (ruling SK-2; `tests/timing.rs`'
    /// `a_player_cut_off_plays_on_once_its_link_is_back`):
    /// - a round trip until the host hears it again and resends what it
    ///   lacks (a datagram a tick from its ack on), and another until its
    ///   own new inputs reach the host and the host stops filling its slot,
    ///   each at most the ping plus its jitter and a tick to be sent;
    /// - re-simulating what it missed, `MAX_CATCHUP` ticks a frame while the
    ///   host runs on.
    pub fn outage_recovery_ticks(&self, outage_ticks: u32, frame_us: u64) -> u32 {
        let trip = (u64::from(self.ping_ms + self.jitter_ms) * 1000).div_ceil(TICK_US) + 1;
        let run = (u64::from(MAX_CATCHUP) * TICK_US / frame_us.max(1)).saturating_sub(1).max(1);
        let ticks = 2 * trip + u64::from(outage_ticks).div_ceil(run);
        u32::try_from(ticks).unwrap_or(u32::MAX)
    }
}

/// Datagrams and bytes counted at one peer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkStats {
    pub datagrams: u64,
    pub bytes: u64,
    pub dropped: u64,
}

/// The loss, delay and duplication of one direction of a connection, from
/// its profile and its own random stream: `Network`'s per-link model, and
/// `rl-net`'s impairment stage over real sockets (spec §9.4).
pub struct LinkModel {
    profile: Profile,
    /// Gilbert-Elliott state: true is the lossy state.
    bad: bool,
    rng: Rng,
}

impl LinkModel {
    pub fn new(profile: Profile, rng: Rng) -> Self {
        Self { profile, bad: false, rng }
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    /// Changes the profile; the loss chain restarts in its good state.
    pub fn set_profile(&mut self, profile: Profile) {
        self.profile = profile;
        self.bad = false;
    }

    /// One-way delay in microseconds: ping/2 plus a jitter draw, at least 0.
    pub fn delay_us(&mut self) -> u64 {
        let half = self.profile.ping_ms as f64 * 500.0;
        let spread = self.profile.jitter_ms as f64 * 500.0;
        let d = half + (self.rng.unit() * 2.0 - 1.0) * spread;
        if d <= 0.0 { 0 } else { d as u64 }
    }

    /// Steps the loss state and says whether this datagram is lost. With a
    /// mean run of 1 or less, losses are independent; otherwise a
    /// Gilbert-Elliott chain gives the mean loss and run length.
    pub fn lost(&mut self) -> bool {
        let p = self.profile;
        if p.loss <= 0.0 {
            return false;
        }
        let u = self.rng.unit();
        if p.burst <= 1.0 {
            return u < p.loss;
        }
        let to_bad = (p.loss / (p.burst * (1.0 - p.loss))).min(1.0);
        let to_good = 1.0 / p.burst;
        self.bad = if self.bad { u >= to_good } else { u < to_bad };
        self.bad
    }

    /// Whether a surviving datagram gets a second copy (draws only when the
    /// profile duplicates).
    pub fn duplicated(&mut self) -> bool {
        self.profile.duplicate > 0.0 && self.rng.unit() < self.profile.duplicate
    }
}

/// One direction of a connection.
struct Link {
    model: LinkModel,
    last_reliable: u64,
    outage: Option<(u64, u64)>,
}

impl Link {
    fn in_outage(&self, t: u64) -> bool {
        self.outage.is_some_and(|(a, b)| t >= a && t < b)
    }
}

struct Entry {
    at: u64,
    seq: u64,
    from: PeerId,
    to: PeerId,
    ch: Channel,
    bytes: Vec<u8>,
}

impl PartialEq for Entry {
    fn eq(&self, o: &Self) -> bool {
        (self.at, self.seq) == (o.at, o.seq)
    }
}
impl Eq for Entry {}
impl PartialOrd for Entry {
    fn partial_cmp(&self, o: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Entry {
    fn cmp(&self, o: &Self) -> core::cmp::Ordering {
        (self.at, self.seq).cmp(&(o.at, o.seq))
    }
}

/// The simulated network between peers.
pub struct Network {
    rng: Rng,
    seq: u64,
    links: BTreeMap<(PeerId, PeerId), Link>,
    heap: BinaryHeap<Reverse<Entry>>,
    sent: BTreeMap<PeerId, LinkStats>,
    received: BTreeMap<PeerId, LinkStats>,
}

impl Network {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            seq: 0,
            links: BTreeMap::new(),
            heap: BinaryHeap::new(),
            sent: BTreeMap::new(),
            received: BTreeMap::new(),
        }
    }

    /// Connects `a` and `b` in both directions with the same profile.
    pub fn connect(&mut self, a: PeerId, b: PeerId, profile: Profile) {
        for key in [(a, b), (b, a)] {
            let rng = self.rng.fork();
            self.links
                .insert(key, Link { model: LinkModel::new(profile, rng), last_reliable: 0, outage: None });
        }
    }

    pub fn set_profile(&mut self, a: PeerId, b: PeerId, profile: Profile) {
        for key in [(a, b), (b, a)] {
            if let Some(l) = self.links.get_mut(&key) {
                l.model.set_profile(profile);
            }
        }
    }

    /// Nothing arrives between the two peers from `from_us` until `until_us`.
    pub fn outage(&mut self, a: PeerId, b: PeerId, from_us: u64, until_us: u64) {
        for key in [(a, b), (b, a)] {
            if let Some(l) = self.links.get_mut(&key) {
                l.outage = Some((from_us, until_us));
            }
        }
    }

    pub fn send(&mut self, now_us: u64, from: PeerId, to: PeerId, ch: Channel, bytes: Vec<u8>) {
        let Some(link) = self.links.get_mut(&(from, to)) else {
            debug_assert!(false, "send between unconnected peers {from} and {to}");
            return;
        };
        let s = self.sent.entry(from).or_default();
        s.datagrams += 1;
        s.bytes += bytes.len() as u64;
        let mut arrivals: [Option<u64>; 2] = [None, None];
        match ch {
            Channel::Unreliable => {
                // The chain steps for every datagram, outage or not.
                let lost = link.model.lost();
                let first = now_us + link.model.delay_us();
                let second = link.model.duplicated().then(|| now_us + link.model.delay_us());
                if lost || link.in_outage(now_us) {
                    self.received.entry(to).or_default().dropped += 1;
                    return;
                }
                // Nothing arrives inside an outage, so a datagram (or its
                // duplicate) due inside one is dropped.
                if !link.in_outage(first) {
                    arrivals[0] = Some(first);
                }
                arrivals[1] = second.filter(|&t| !link.in_outage(t));
                if arrivals == [None, None] {
                    self.received.entry(to).or_default().dropped += 1;
                    return;
                }
            }
            Channel::Reliable => {
                let mut d = link.model.delay_us();
                let ping_us = link.model.profile.ping_ms as u64 * 1000;
                // Every loss draw (from the link's own loss chain, shared with
                // the unreliable datagrams) is a retransmission one ping later.
                let mut tries = 0;
                while tries < 64 && link.model.lost() {
                    d += ping_us;
                    tries += 1;
                }
                let mut at = now_us + d;
                if let Some((_, until)) = link.outage
                    && (link.in_outage(now_us) || link.in_outage(at))
                {
                    at = until + d;
                }
                at = at.max(link.last_reliable);
                link.last_reliable = at;
                arrivals[0] = Some(at);
            }
        }
        for at in arrivals.into_iter().flatten() {
            self.seq += 1;
            self.heap.push(Reverse(Entry { at, seq: self.seq, from, to, ch, bytes: bytes.clone() }));
        }
    }

    pub fn next_delivery(&self) -> Option<u64> {
        self.heap.peek().map(|e| e.0.at)
    }

    pub fn deliver_due(&mut self, now_us: u64, out: &mut dyn FnMut(PeerId, PeerId, Channel, Vec<u8>)) {
        while self.heap.peek().is_some_and(|e| e.0.at <= now_us) {
            let Some(Reverse(e)) = self.heap.pop() else { break };
            let r = self.received.entry(e.to).or_default();
            r.datagrams += 1;
            r.bytes += e.bytes.len() as u64;
            out(e.from, e.to, e.ch, e.bytes);
        }
    }

    /// What reached `peer`.
    pub fn received(&self, peer: PeerId) -> LinkStats {
        self.received.get(&peer).copied().unwrap_or_default()
    }

    pub fn sent(&self, peer: PeerId) -> LinkStats {
        self.sent.get(&peer).copied().unwrap_or_default()
    }
}
