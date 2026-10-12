//! The N-peer harness (spec §9.1): one host and its clients and spectators,
//! each a [`Session`], play a match over the network model in virtual time;
//! [`verdict`] then checks that every peer confirmed the host's inputs and
//! simulated the same match.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use rl_sim::Keys;

use crate::netsim::{LinkStats, Network, Profile};
use crate::{
    Channel, Config, Driver, Event, PeerId, Role, Seat, Session, Simulation, Stats, TICK_US, TickInfo,
};

/// The host's peer id.
pub const HOST: PeerId = 0;
/// The host's frame period: a player's game at 144 Hz.
pub const HOST_FRAME_US: u64 = 6_944;

/// A client or spectator.
#[derive(Clone, Debug)]
pub struct PeerPlan {
    pub id: PeerId,
    pub seat: Seat,
    /// Its link to the host.
    pub profile: Profile,
    /// How fast its clock runs, in parts per million.
    pub clock_ppm: i32,
    pub input_delay: u8,
    /// Its frame period in local time.
    pub frame_us: u64,
    /// `None`: present from tick 0. Joining mid-match is not supported yet.
    pub join_tick: Option<u32>,
    /// A suspended laptop: from the host's tick `.0` until its tick `.1` the
    /// peer runs no frame, its clock stands still, and what arrives waits
    /// for it in order. The clock stops (as a monotonic clock does in
    /// suspend): with it running the pace would just catch up, and the jump
    /// would go untested.
    pub freeze: Option<(u32, u32)>,
}

/// Nothing passes between the host and `peer` from the host's tick
/// `from_tick` (in virtual time) to `until_tick`.
#[derive(Clone, Debug)]
pub struct Outage {
    pub peer: PeerId,
    pub from_tick: u32,
    pub until_tick: u32,
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub seed: u64,
    pub slots: u8,
    pub peers: Vec<PeerPlan>,
    /// The ticks the verdict checks.
    pub ticks: u32,
    /// Ticks after `ticks` on LAN links, so everything confirms.
    pub settle_ticks: u32,
    pub outages: Vec<Outage>,
    /// No player peer (the host included) may stall.
    pub expect_no_stalls: bool,
}

/// Builds each peer's simulation and driver.
pub trait Factory<S: Simulation> {
    fn sim(&mut self, peer: PeerId) -> S;
    /// `seat` is `None` for the host.
    fn driver(&mut self, peer: PeerId, seat: Option<Seat>) -> Box<dyn Driver<S>>;
}

/// What a run produced, per peer (the host included).
#[derive(Clone, Debug)]
pub struct Report {
    /// The host's confirmed inputs.
    pub host_log: Vec<[u8; 32]>,
    /// Every peer's confirmed inputs (its log rows below its frontier).
    pub logs: BTreeMap<PeerId, Vec<[u8; 32]>>,
    /// The checksum after every tick `t` with `t % 10 == 9`, from its last simulation.
    pub sims: BTreeMap<PeerId, BTreeMap<u32, u64>>,
    pub stats: BTreeMap<PeerId, Stats>,
    pub events: BTreeMap<PeerId, Vec<Event>>,
    /// What reached each peer.
    pub received: BTreeMap<PeerId, LinkStats>,
    pub duration_us: u64,
    /// Peers the host dropped after [`Event::PeerTimedOut`]; the verdict skips them.
    pub dropped: BTreeSet<PeerId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub tick: u32,
    pub what: String,
}

fn fail(tick: u32, what: String) -> Failure {
    Failure { tick, what }
}

/// Wraps a peer's driver and records its checksums.
struct Recorder<S> {
    inner: Box<dyn Driver<S>>,
    sums: BTreeMap<u32, u64>,
}

impl<S: Simulation> Driver<S> for Recorder<S> {
    fn local_inputs(&mut self, sim: &S, tick: u32, slots: &[u8], out: &mut [Keys]) {
        self.inner.local_inputs(sim, tick, slots, out);
    }

    fn simulated(&mut self, sim: &S, info: TickInfo) {
        self.inner.simulated(sim, info);
        if info.tick % 10 == 9 {
            self.sums.insert(info.tick, sim.checksum());
        }
    }
}

struct Peer<S: Simulation> {
    id: PeerId,
    session: Session<S>,
    driver: Recorder<S>,
    ppm: i32,
    frame_us: u64,
    /// The local time of its next frame.
    next_frame: u64,
    /// The freeze still to come.
    freeze: Option<(u32, u32)>,
    /// The virtual time the current freeze began.
    frozen_at: Option<u64>,
    /// Virtual time spent frozen: the peer's clock lags by it.
    paused_us: u64,
    /// What arrived while frozen.
    inbox: Vec<(PeerId, Channel, Vec<u8>)>,
    /// Its last advance began with `ticks` confirmed (so it re-simulated
    /// them on their final inputs) and ended at or past `ticks`. Ending on
    /// "frontier ≥ ticks" alone could stop before a pending rollback ran.
    done: bool,
}

impl<S: Simulation> Peer<S> {
    /// Its clock at virtual time `now_us`.
    fn local(&self, now_us: u64) -> u64 {
        local_time(self.frozen_at.unwrap_or(now_us).saturating_sub(self.paused_us), self.ppm)
    }

    /// The virtual time of its next frame (`u64::MAX` while frozen).
    fn next_frame_at(&self) -> u64 {
        match self.frozen_at {
            Some(_) => u64::MAX,
            None => virtual_time(self.next_frame, self.ppm).saturating_add(self.paused_us),
        }
    }
}

/// The peer's clock at virtual time `now_us`.
fn local_time(now_us: u64, ppm: i32) -> u64 {
    let t = now_us as i128 * (1_000_000 + ppm as i128) / 1_000_000;
    t.clamp(0, u64::MAX as i128) as u64
}

/// The first virtual time at which the peer's clock reads `local_us`.
fn virtual_time(local_us: u64, ppm: i32) -> u64 {
    let rate = (1_000_000 + ppm as i128).max(1);
    let t = (local_us as i128 * 1_000_000 + rate - 1) / rate;
    t.clamp(0, u64::MAX as i128) as u64
}

fn check_scenario(sc: &Scenario) -> Result<(), Failure> {
    if !(1..=32).contains(&sc.slots) {
        return Err(fail(0, format!("{} slots: a match has 1 to 32", sc.slots)));
    }
    let mut ids = Vec::new();
    let mut seats = Vec::new();
    for p in &sc.peers {
        if let Some(t) = p.join_tick {
            return Err(fail(t, format!("peer {} joins mid-match: not supported yet", p.id)));
        }
        if p.id == HOST || ids.contains(&p.id) {
            return Err(fail(0, format!("peer id {} is the host's or repeated", p.id)));
        }
        ids.push(p.id);
        if let Seat::Player(s) = p.seat {
            if s >= sc.slots || seats.contains(&s) {
                return Err(fail(0, format!("peer {} plays slot {s}: out of range or taken", p.id)));
            }
            seats.push(s);
        }
        if p.frame_us == 0 {
            return Err(fail(0, format!("peer {} has a zero frame period", p.id)));
        }
    }
    Ok(())
}

/// Plays the scenario. Virtual time moves in 1 ms steps (skipping steps in
/// which nothing is due): freezes begin or end, deliveries, then every peer
/// whose frame has come advances, transmits and reports its events. The
/// host drops a peer it reports timed out (`peer_lost`). The run ends when
/// the host's current tick reaches `ticks + settle_ticks` and every peer
/// not dropped had confirmed `ticks` ticks before its last advance (which
/// re-simulated them on their final inputs) and simulated them.
pub fn run<S: Simulation>(sc: &Scenario, f: &mut dyn Factory<S>) -> Result<Report, Failure> {
    check_scenario(sc)?;
    let mut net = Network::new(sc.seed);
    for p in &sc.peers {
        net.connect(HOST, p.id, p.profile);
    }
    let mut peers: Vec<Peer<S>> = Vec::with_capacity(sc.peers.len() + 1);
    let host_role = Role::Host { peers: sc.peers.iter().map(|p| (p.id, p.seat)).collect() };
    peers.push(Peer {
        id: HOST,
        session: Session::new(Config::new(sc.slots, 0), host_role, f.sim(HOST), 0),
        driver: Recorder { inner: f.driver(HOST, None), sums: BTreeMap::new() },
        ppm: 0,
        frame_us: HOST_FRAME_US,
        next_frame: 0,
        freeze: None,
        frozen_at: None,
        paused_us: 0,
        inbox: Vec::new(),
        done: false,
    });
    for p in &sc.peers {
        let (role, delay) = match p.seat {
            Seat::Player(slot) => (Role::Client { host: HOST, slot }, p.input_delay),
            Seat::Spectator => (Role::Spectator { host: HOST }, 0),
        };
        peers.push(Peer {
            id: p.id,
            session: Session::new(Config::new(sc.slots, delay), role, f.sim(p.id), 0),
            driver: Recorder { inner: f.driver(p.id, Some(p.seat)), sums: BTreeMap::new() },
            ppm: p.clock_ppm,
            frame_us: p.frame_us,
            next_frame: 0,
            freeze: p.freeze,
            frozen_at: None,
            paused_us: 0,
            inbox: Vec::new(),
            done: false,
        });
    }
    let index: BTreeMap<PeerId, usize> = peers.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let mut events: BTreeMap<PeerId, Vec<Event>> = peers.iter().map(|p| (p.id, Vec::new())).collect();

    // Outages, per peer in start order; one is installed at a time.
    let mut outages: BTreeMap<PeerId, Vec<(u64, u64)>> = BTreeMap::new();
    for o in &sc.outages {
        let (a, b) = (o.from_tick as u64 * TICK_US, o.until_tick as u64 * TICK_US);
        outages.entry(o.peer).or_default().push((a, b));
    }
    for list in outages.values_mut() {
        list.sort();
        list.reverse(); // popped from the end
    }
    let mut installed: BTreeMap<PeerId, u64> = BTreeMap::new();
    let mut dropped: BTreeSet<PeerId> = BTreeSet::new();

    let end = sc.ticks.saturating_add(sc.settle_ticks);
    let limit_us = (end as u64 * TICK_US).saturating_mul(4).saturating_add(60_000_000);
    let mut settled = false;
    let mut now = 0u64;
    let mut host_tick = 0;
    loop {
        for p in peers.iter_mut() {
            match (p.freeze, p.frozen_at) {
                (Some((from, until)), None) if host_tick >= from && host_tick < until => {
                    p.frozen_at = Some(now)
                }
                (Some((_, until)), Some(at)) if host_tick >= until => {
                    p.paused_us += now - at;
                    p.frozen_at = None;
                    p.freeze = None;
                    let local = p.local(now);
                    for (from, ch, bytes) in core::mem::take(&mut p.inbox) {
                        p.session.receive(from, ch, &bytes, local);
                    }
                }
                _ => {}
            }
        }
        for (&peer, list) in outages.iter_mut() {
            let free = installed.get(&peer).is_none_or(|&until| now >= until);
            if free && let Some((a, b)) = list.pop() {
                net.outage(HOST, peer, a, b);
                installed.insert(peer, b);
            }
        }
        net.deliver_due(now, &mut |from, to, ch, bytes| {
            if let Some(&i) = index.get(&to) {
                let p = &mut peers[i];
                if p.frozen_at.is_some() {
                    p.inbox.push((from, ch, bytes));
                } else {
                    let local = p.local(now);
                    p.session.receive(from, ch, &bytes, local);
                }
            }
        });
        for p in peers.iter_mut() {
            let local = p.local(now);
            if p.frozen_at.is_some() || local < p.next_frame {
                continue;
            }
            p.next_frame += p.frame_us;
            let confirmed = p.session.frontier() >= sc.ticks;
            p.session.advance(local, &mut p.driver);
            p.done = confirmed && p.session.current_tick() >= sc.ticks;
            let id = p.id;
            let new = p.session.drain_events();
            if id == HOST {
                for e in &new {
                    if let Event::PeerTimedOut { peer } = *e {
                        p.session.peer_lost(peer);
                        dropped.insert(peer);
                    }
                }
            }
            p.session.transmit(&mut |to, ch: Channel, bytes| net.send(now, id, to, ch, bytes));
            events.entry(id).or_default().extend(new);
        }
        host_tick = peers[0].session.current_tick();
        if !settled && host_tick >= sc.ticks {
            for p in &sc.peers {
                net.set_profile(HOST, p.id, Profile::LAN);
            }
            settled = true;
        }
        let behind = peers.iter().find(|p| !p.done && !dropped.contains(&p.id));
        if host_tick >= end && behind.is_none() {
            break;
        }
        if now >= limit_us {
            let what = match behind {
                Some(p) if host_tick >= end => format!(
                    "peer {} reached only tick {} (frontier {}) of {} in time",
                    p.id,
                    p.session.current_tick(),
                    p.session.frontier(),
                    sc.ticks
                ),
                _ => format!("the host reached only tick {host_tick} of {end} in time"),
            };
            return Err(fail(host_tick, what));
        }
        // The next step: the next delivery, frame or outage change, on a
        // millisecond boundary.
        let mut next = net.next_delivery().unwrap_or(u64::MAX);
        for p in &peers {
            next = next.min(p.next_frame_at());
        }
        for (peer, &until) in &installed {
            if until > now && outages.get(peer).is_some_and(|l| !l.is_empty()) {
                next = next.min(until);
            }
        }
        now = next.div_ceil(1000).saturating_mul(1000).max(now + 1000);
    }

    let mut report = Report {
        host_log: Vec::new(),
        logs: BTreeMap::new(),
        sims: BTreeMap::new(),
        stats: BTreeMap::new(),
        events,
        received: BTreeMap::new(),
        duration_us: now,
        dropped,
    };
    for p in peers {
        let log = p.session.input_log();
        let rows: Vec<[u8; 32]> = (0..log.frontier()).filter_map(|t| log.row(t)).collect();
        if p.id == HOST {
            report.host_log = rows.clone();
        }
        report.logs.insert(p.id, rows);
        report.events.entry(p.id).or_default().extend({
            let mut s = p.session;
            let e = s.drain_events();
            report.stats.insert(p.id, s.stats().clone());
            e
        });
        report.sims.insert(p.id, p.driver.sums);
        report.received.insert(p.id, net.received(p.id));
    }
    Ok(report)
}

/// Checks a run: every peer confirmed `ticks` ticks of the host's inputs,
/// and its checksums match an offline replay of them; no stalls (when
/// expected), malformed messages or inconsistent inputs. Dropped peers are
/// skipped.
pub fn verdict<S: Simulation>(sc: &Scenario, r: &Report, f: &mut dyn Factory<S>) -> Result<(), Failure> {
    let ticks = sc.ticks;
    let kept = |peer: &PeerId| !r.dropped.contains(peer);
    for (&peer, rows) in r.logs.iter().filter(|(p, _)| kept(p)) {
        if rows.len() < ticks as usize {
            return Err(fail(
                rows.len() as u32,
                format!("peer {peer} confirmed {} of {ticks} ticks", rows.len()),
            ));
        }
    }
    if r.host_log.len() < ticks as usize {
        return Err(fail(r.host_log.len() as u32, format!("the host confirmed {} ticks", r.host_log.len())));
    }
    for (&peer, rows) in r.logs.iter().filter(|(p, _)| kept(p)) {
        if let Some(t) = (0..ticks as usize).find(|&t| rows[t] != r.host_log[t]) {
            return Err(fail(t as u32, format!("peer {peer} confirmed other inputs than the host")));
        }
    }
    let mut sim = f.sim(HOST);
    let slots = sc.slots as usize;
    for t in 0..ticks {
        let row = &r.host_log[t as usize];
        let keys: Vec<Keys> = row[..slots].iter().map(|&k| Keys(k)).collect();
        sim.tick(&keys);
        if t % 10 != 9 {
            continue;
        }
        let want = sim.checksum();
        for (&peer, sums) in r.sims.iter().filter(|(p, _)| kept(p)) {
            match sums.get(&t) {
                Some(&got) if got == want => {}
                Some(&got) => {
                    return Err(fail(t, format!("peer {peer} checksum {got:#x}, the replay {want:#x}")));
                }
                None => return Err(fail(t, format!("peer {peer} recorded no checksum"))),
            }
        }
    }
    let players: Vec<PeerId> = core::iter::once(HOST)
        .chain(sc.peers.iter().filter(|p| matches!(p.seat, Seat::Player(_))).map(|p| p.id))
        .collect();
    for (&peer, events) in r.events.iter().filter(|(p, _)| kept(p)) {
        for e in events {
            match *e {
                Event::Stalled { tick } if sc.expect_no_stalls && players.contains(&peer) => {
                    return Err(fail(tick, format!("player peer {peer} stalled")));
                }
                Event::Malformed { peer: from } => {
                    return Err(fail(0, format!("peer {peer} got a malformed message from {from}")));
                }
                Event::Inconsistent { peer: from, tick, slot } => {
                    return Err(fail(
                        tick,
                        format!("peer {peer} got an inconsistent input for slot {slot} from {from}"),
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}
