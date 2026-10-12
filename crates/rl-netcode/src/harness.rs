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
    /// `None`: present from tick 0. A spectator may join at the host's
    /// tick `Some(t)` (`t < ticks`): its session is made then
    /// (`Role::Joining`) and the host's `add_spectator` sends it the state.
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
    /// Planted faults: at peer `.0`'s first frame after the host's tick
    /// `.1` at which it has confirmed tick `.1`, [`Factory::corrupt`] changes
    /// its simulation as it was after its last confirmed tick (entry `i`
    /// passes index `i`; `Session::change_confirmed`). Only these peers may desync; the verdict compares them
    /// from their last loaded state on.
    pub corrupt: Vec<(PeerId, u32)>,
    /// The host suspended, like [`PeerPlan::freeze`]: from its tick `.0`
    /// it runs no frame, its clock stands still and arrivals wait, for
    /// `.1 - .0` ticks' worth of time (its own tick stands still too).
    pub host_freeze: Option<(u32, u32)>,
}

/// Watches a run as it happens (the driver's view, not the report's): what
/// a test or a tool wants from a session at the moment, such as a state to
/// keep or the positions of every frame. Both methods do nothing by default.
pub trait Observer<S: Simulation> {
    /// After a peer's frame, once per event it reported, in order; the
    /// session is as the frame left it.
    fn on_event(&mut self, _peer: PeerId, _event: &Event, _session: &mut Session<S>) {}
    /// After a peer's frame (its events included, its datagrams not yet sent).
    fn on_frame(&mut self, _peer: PeerId, _session: &mut Session<S>) {}
}

/// The observer that sees nothing.
impl<S: Simulation> Observer<S> for () {}

/// Builds each peer's simulation and driver.
pub trait Factory<S: Simulation> {
    fn sim(&mut self, peer: PeerId) -> S;
    /// `seat` is `None` for the host.
    fn driver(&mut self, peer: PeerId, seat: Option<Seat>) -> Box<dyn Driver<S>>;
    /// Plants fault `index` of [`Scenario::corrupt`] in a peer's simulation
    /// (through `Session::change_confirmed`, so it survives rollbacks).
    fn corrupt(&mut self, _sim: &mut S, _index: usize) {}
}

/// What a run produced, per peer (the host included).
#[derive(Clone, Debug)]
pub struct Report {
    /// The host's confirmed inputs.
    pub host_log: Vec<[u8; 32]>,
    /// Every peer's confirmed inputs (its log rows below its frontier; a
    /// joined spectator's are zero before its state).
    pub logs: BTreeMap<PeerId, Vec<[u8; 32]>>,
    /// The checksum after every tick `t` with `t % 10 == 9`, from its last simulation.
    pub sims: BTreeMap<PeerId, BTreeMap<u32, u64>>,
    pub stats: BTreeMap<PeerId, Stats>,
    pub events: BTreeMap<PeerId, Vec<Event>>,
    /// What reached each peer.
    pub received: BTreeMap<PeerId, LinkStats>,
    pub duration_us: u64,
    /// Peers the host dropped after [`Event::PeerTimedOut`],
    /// [`Event::PeerLeft`] or [`Event::RepairFailed`]; the verdict skips them.
    pub dropped: BTreeSet<PeerId>,
    /// Peers the host dropped after [`Event::RepairFailed`].
    pub repair_failed: BTreeSet<PeerId>,
    /// Every peer's confirmed sync checksums ([`Session::checksums`]).
    pub checksums: BTreeMap<PeerId, Vec<(u32, u64)>>,
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
    /// The host's: a freeze lasts `until - from` ticks' worth of virtual
    /// time (its own tick stands still), not until the host's tick `until`.
    timed_freeze: bool,
    /// The virtual time the current freeze began.
    frozen_at: Option<u64>,
    /// Virtual time spent frozen: the peer's clock lags by it.
    paused_us: u64,
    /// What arrived while frozen.
    inbox: Vec<(PeerId, Channel, Vec<u8>)>,
    /// Faults still to plant: (index in `Scenario::corrupt`, host tick).
    faults: Vec<(usize, u32)>,
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

    /// When its current freeze ends: the virtual time of a timed one.
    fn thaws_at(&self) -> Option<u64> {
        match (self.freeze, self.frozen_at) {
            (Some((from, until)), Some(at)) if self.timed_freeze => {
                Some(at.saturating_add(until.saturating_sub(from) as u64 * TICK_US))
            }
            _ => None,
        }
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

/// The host (`plan` `None`) or a peer, made at virtual time `now`: its
/// clock and its first frame start there.
fn make_peer<S: Simulation>(
    sc: &Scenario,
    f: &mut dyn Factory<S>,
    plan: Option<&PeerPlan>,
    role: Role,
    now: u64,
) -> Peer<S> {
    let (id, seat, ppm, frame_us, delay) = match plan {
        Some(p) => (p.id, Some(p.seat), p.clock_ppm, p.frame_us, p.input_delay),
        None => (HOST, None, 0, HOST_FRAME_US, 0),
    };
    let delay = if matches!(role, Role::Client { .. }) { delay } else { 0 };
    let local = local_time(now, ppm);
    Peer {
        id,
        session: Session::new(Config::new(sc.slots, delay), role, f.sim(id), local),
        driver: Recorder { inner: f.driver(id, seat), sums: BTreeMap::new() },
        ppm,
        frame_us,
        next_frame: local,
        freeze: match plan {
            Some(p) => p.freeze,
            None => sc.host_freeze,
        },
        timed_freeze: plan.is_none(),
        frozen_at: None,
        paused_us: 0,
        inbox: Vec::new(),
        faults: sc.corrupt.iter().enumerate().filter(|(_, c)| c.0 == id).map(|(i, c)| (i, c.1)).collect(),
        done: false,
    }
}

fn check_scenario(sc: &Scenario) -> Result<(), Failure> {
    if !(1..=32).contains(&sc.slots) {
        return Err(fail(0, format!("{} slots: a match has 1 to 32", sc.slots)));
    }
    let mut ids = Vec::new();
    let mut seats = Vec::new();
    for p in &sc.peers {
        if let Some(t) = p.join_tick
            && (p.seat != Seat::Spectator || t >= sc.ticks)
        {
            return Err(fail(
                t,
                format!("peer {} joins at {t}: only a spectator, before tick {}", p.id, sc.ticks),
            ));
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
    if let Some(&(peer, t)) = sc.corrupt.iter().find(|(peer, _)| !ids.contains(peer)) {
        return Err(fail(t, format!("peer {peer} is corrupted but not in the match")));
    }
    Ok(())
}

/// Plays the scenario. Virtual time moves in 1 ms steps (skipping steps in
/// which nothing is due): freezes begin or end, deliveries, then every peer
/// whose frame has come advances, transmits and reports its events. The
/// host drops a peer it reports timed out or failed to repair
/// (`peer_lost`). The run ends when the host's current tick reaches
/// `ticks + settle_ticks` and every peer not dropped had confirmed `ticks`
/// ticks before its last advance (which re-simulated them on their final
/// inputs) and simulated them.
pub fn run<S: Simulation>(sc: &Scenario, f: &mut dyn Factory<S>) -> Result<Report, Failure> {
    run_observed(sc, f, &mut ())
}

/// [`run`], with `obs` told of every frame and event as they happen.
pub fn run_observed<S: Simulation>(
    sc: &Scenario,
    f: &mut dyn Factory<S>,
    obs: &mut dyn Observer<S>,
) -> Result<Report, Failure> {
    check_scenario(sc)?;
    let mut net = Network::new(sc.seed);
    for p in &sc.peers {
        net.connect(HOST, p.id, p.profile);
    }
    let (present, mut joiners): (Vec<&PeerPlan>, Vec<&PeerPlan>) =
        sc.peers.iter().partition(|p| p.join_tick.is_none());
    joiners.sort_by_key(|p| core::cmp::Reverse(p.join_tick)); // popped from the end
    let mut peers: Vec<Peer<S>> = Vec::with_capacity(sc.peers.len() + 1);
    let host_role = Role::Host { peers: present.iter().map(|p| (p.id, p.seat)).collect() };
    peers.push(make_peer(sc, f, None, host_role, 0));
    for p in &present {
        let role = match p.seat {
            Seat::Player(slot) => Role::Client { host: HOST, slot },
            Seat::Spectator => Role::Spectator { host: HOST },
        };
        peers.push(make_peer(sc, f, Some(p), role, 0));
    }
    let mut index: BTreeMap<PeerId, usize> = peers.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let mut events: BTreeMap<PeerId, Vec<Event>> = sc.peers.iter().map(|p| (p.id, Vec::new())).collect();
    events.insert(HOST, Vec::new());

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
    let mut repair_failed: BTreeSet<PeerId> = BTreeSet::new();

    let end = sc.ticks.saturating_add(sc.settle_ticks);
    let limit_us = (end as u64 * TICK_US).saturating_mul(4).saturating_add(60_000_000);
    let mut settled = false;
    let mut now = 0u64;
    let mut host_tick = 0;
    loop {
        // Spectators joining mid-match: their session begins now.
        while let Some(&plan) = joiners.last().filter(|p| p.join_tick.is_some_and(|t| host_tick >= t)) {
            joiners.pop();
            let host_now = peers[0].local(now);
            peers[0].session.add_spectator(plan.id, host_now);
            index.insert(plan.id, peers.len());
            peers.push(make_peer(sc, f, Some(plan), Role::Joining { host: HOST }, now));
        }
        for p in peers.iter_mut() {
            let thawed = match p.thaws_at() {
                Some(t) => now >= t,
                None => p.freeze.is_some_and(|(_, until)| host_tick >= until),
            };
            match (p.freeze, p.frozen_at) {
                (Some((from, until)), None) if host_tick >= from && host_tick < until => {
                    p.frozen_at = Some(now)
                }
                (Some(_), Some(at)) if thawed => {
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
            while let Some(i) = p.faults.iter().position(|&(_, t)| host_tick > t && p.session.frontier() > t)
            {
                let (index, _) = p.faults.remove(i);
                p.session.change_confirmed(&mut p.driver, |sim| f.corrupt(sim, index));
            }
            let confirmed = p.session.frontier() >= sc.ticks;
            p.session.advance(local, &mut p.driver);
            p.done = confirmed && p.session.current_tick() >= sc.ticks;
            let id = p.id;
            let new = p.session.drain_events();
            if id == HOST {
                for e in &new {
                    match *e {
                        Event::PeerTimedOut { peer } => {
                            p.session.peer_lost(peer);
                            dropped.insert(peer);
                        }
                        // The session abandoned it already.
                        Event::PeerLeft { peer } => {
                            dropped.insert(peer);
                        }
                        Event::RepairFailed { peer } => {
                            p.session.peer_lost(peer);
                            dropped.insert(peer);
                            repair_failed.insert(peer);
                        }
                        _ => {}
                    }
                }
            }
            for e in &new {
                obs.on_event(id, e, &mut p.session);
            }
            obs.on_frame(id, &mut p.session);
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
            next = next.min(p.next_frame_at()).min(p.thaws_at().unwrap_or(u64::MAX));
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
        repair_failed,
        checksums: BTreeMap::new(),
    };
    for p in peers {
        let log = p.session.input_log();
        // A joined spectator's log begins at its state: zeros before.
        let rows: Vec<[u8; 32]> = (0..log.frontier()).map(|t| log.row(t).unwrap_or([0; 32])).collect();
        report.checksums.insert(p.id, p.session.checksums().to_vec());
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
/// and its checksums (recorded while simulating, and the session's
/// confirmed ones) match an offline replay of them; no stalls (when
/// expected), malformed messages, inconsistent inputs, refused states, or
/// desyncs but of corrupted peers. Dropped peers are skipped. A spectator
/// that joined is compared from its state on (inputs from its first,
/// checksums from its last); a corrupted peer's checksums from its last
/// loaded state on.
pub fn verdict<S: Simulation>(sc: &Scenario, r: &Report, f: &mut dyn Factory<S>) -> Result<(), Failure> {
    let ticks = sc.ticks;
    let kept = |peer: &PeerId| !r.dropped.contains(peer);
    let corrupted = |peer: PeerId| sc.corrupt.iter().any(|c| c.0 == peer);
    let joined = |peer: PeerId| sc.peers.iter().any(|p| p.id == peer && p.join_tick.is_some());
    // Where each peer's inputs and checksums are compared from.
    let mut from: BTreeMap<PeerId, (u32, u32)> = BTreeMap::new();
    for (&peer, events) in r.events.iter().filter(|(p, _)| kept(p)) {
        let mut loads = events.iter().filter_map(|e| match *e {
            Event::StateLoaded { tick } => Some(tick.saturating_add(1)),
            _ => None,
        });
        let first = loads.next();
        let last = loads.next_back().or(first);
        let need = joined(peer) || corrupted(peer);
        let (inputs, sums) = match (need, first, last) {
            (false, ..) => (0, 0),
            (true, Some(a), Some(b)) => (if joined(peer) { a } else { 0 }, b),
            _ => return Err(fail(0, format!("peer {peer} never loaded a state"))),
        };
        from.insert(peer, (inputs, sums));
    }
    let from_of = |peer: &PeerId| from.get(peer).copied().unwrap_or((0, 0));
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
        let start = from_of(&peer).0 as usize;
        if let Some(t) = (start..ticks as usize).find(|&t| rows[t] != r.host_log[t]) {
            return Err(fail(t as u32, format!("peer {peer} confirmed other inputs than the host")));
        }
    }
    let mut sim = f.sim(HOST);
    let slots = sc.slots as usize;
    let mut replay = BTreeMap::new();
    for t in 0..ticks {
        let row = &r.host_log[t as usize];
        let keys: Vec<Keys> = row[..slots].iter().map(|&k| Keys(k)).collect();
        sim.tick(&keys);
        if t % 10 != 9 {
            continue;
        }
        let want = sim.checksum();
        replay.insert(t, want);
        for (&peer, sums) in r.sims.iter().filter(|(p, _)| kept(p)) {
            if t < from_of(&peer).1 {
                continue;
            }
            match sums.get(&t) {
                Some(&got) if got == want => {}
                Some(&got) => {
                    return Err(fail(t, format!("peer {peer} checksum {got:#x}, the replay {want:#x}")));
                }
                None => return Err(fail(t, format!("peer {peer} recorded no checksum"))),
            }
        }
    }
    for (&peer, sums) in r.checksums.iter().filter(|(p, _)| kept(p)) {
        let start = from_of(&peer).1;
        for &(t, got) in sums.iter().filter(|(t, _)| (start..ticks).contains(t)) {
            if replay.get(&t) != Some(&got) {
                return Err(fail(
                    t,
                    format!("peer {peer} confirmed checksum {got:#x} differs from the replay"),
                ));
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
                Event::StateRejected { tick } => {
                    return Err(fail(tick, format!("peer {peer} refused the state after tick {tick}")));
                }
                Event::Desync { tick, peer: other, .. } => {
                    let faulty = if peer == HOST { other } else { peer };
                    if !corrupted(faulty) && kept(&faulty) {
                        return Err(fail(tick, format!("peer {faulty} desynced at tick {tick}")));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
