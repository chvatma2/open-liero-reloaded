//! The N-peer harness (spec §9.1): one host and its clients and spectators,
//! each a [`Session`], play a match over the network model in virtual time;
//! [`verdict`] then checks that every peer confirmed the host's inputs and
//! simulated the same match. With [`Scenario::migration`] the harness also
//! drives host migrations (spec §3.7) as rl-online does: who tries which
//! successor ([`Migration`]), the rejoins, the take-over, the resume.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use rl_sim::Keys;

use crate::migration::{MAX_MIGRATIONS, Migration, RETRY_US, STEP_DOWN_US, Step};
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
    /// Peers leaving, in any order.
    pub exits: Vec<Exit>,
    /// Host migration on (spec §3.7); `None`: a host gone ends nothing and
    /// moves nothing, as before plan 12c.
    pub migration: Option<MigrationPlan>,
    /// Links changing their profile mid-run.
    pub profile_changes: Vec<ProfileChange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitHow {
    Crash,
    Leave,
}

/// `peer` (the host, `HOST`, or any other) leaves `after_us` of virtual
/// time after the current host's tick reached `tick` (the host's tick
/// stands still during a migration, time does not): `Crash` (no frame,
/// nothing sent or received from then on) or `Leave` (`Session::leave`,
/// transmitted, then gone).
#[derive(Clone, Debug)]
pub struct Exit {
    pub peer: PeerId,
    pub tick: u32,
    pub after_us: u64,
    pub how: ExitHow,
}

/// Host migration on (spec §3.7).
#[derive(Clone, Debug, Default)]
pub struct MigrationPlan {
    /// The first host's own player's slot (abandoned once it is gone); its
    /// other local slots are bots, which a new host plays.
    pub host_human: Option<u8>,
    /// `(peer, µs)`: this peer's joins to a successor take this long after
    /// contact (a slow path through relays), instead of three pings.
    pub slow_joins: Vec<(PeerId, u64)>,
}

/// From the current host's tick `tick`, `peer`'s link to whoever hosts
/// takes `profile`.
#[derive(Clone, Debug)]
pub struct ProfileChange {
    pub tick: u32,
    pub peer: PeerId,
    pub profile: Profile,
}

/// One migration, as it happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Migrated {
    pub from: PeerId,
    pub to: PeerId,
    /// Virtual time the first peer found the host gone (`HostSilent`,
    /// `PeerLeft`).
    pub silent_us: u64,
    /// Virtual time the new host resumed, and at which tick.
    pub resumed_us: u64,
    pub tick: u32,
}

/// What one survivor of a migration held when it resumed (the new host as
/// it took over, each rejoined peer at the resume): per slot, its known
/// prefix of final inputs from its log's origin. The verdict finds every
/// value in the last host's log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    /// Its index in [`Report::migrations`].
    pub migration: usize,
    pub peer: PeerId,
    pub origin: u32,
    pub slots: Vec<Vec<u8>>,
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
    /// The driver of a peer that took over: its own slot as before (`own`),
    /// and every bot slot (spec §3.7: bots restart).
    fn host_driver(&mut self, peer: PeerId, _own: Option<u8>, _sim: &S) -> Box<dyn Driver<S>> {
        self.driver(peer, None)
    }
}

/// What a run produced, per peer (the host included).
#[derive(Clone, Debug)]
pub struct Report {
    /// The last host's confirmed inputs.
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
    /// Who hosted, in order (`HOST` first).
    pub hosts: Vec<PeerId>,
    pub migrations: Vec<Migrated>,
    /// Peers that gave up (spec §3.7's host lost).
    pub host_lost: BTreeSet<PeerId>,
    /// Peers that crashed, left or stepped down.
    pub exited: BTreeSet<PeerId>,
    /// Peers that reached a new host after its resume and watch from a
    /// state (N72): the verdict checks them from their last loaded state.
    pub late: BTreeSet<PeerId>,
    /// What each migration's survivors held.
    pub held: Vec<Held>,
    /// The host each exited, host-lost or left-behind peer followed as it
    /// went (an exited or left-behind host: itself): the verdict bounds its
    /// rows by what that host's migration's survivors held.
    pub left_from: BTreeMap<PeerId, PeerId>,
    /// Former hosts still running at the end: cut off from their peers
    /// (their own network down past `HOST_SILENT_US` while they ran on),
    /// they cannot know the peers moved to a successor, and confirm their
    /// own fills for the peers' slots. Not survivors, as an exited host.
    pub left_behind: BTreeSet<PeerId>,
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
    /// Its seat (`None`: the first host).
    seat: Option<Seat>,
    /// Whom it follows (itself once it hosts).
    host: PeerId,
    /// Crashed, left, stepped down or gave up: no frame, nothing received.
    gone: bool,
    /// The virtual time of its last frame (a host's step-down, N70).
    last_frame_us: Option<u64>,
    /// Migrations it went through in this match (N74).
    migrations: u32,
    /// Its migration, from the moment it found its host gone until an
    /// attempt was welcomed (or it took over).
    migrating: Option<Migrating>,
    /// A successor that took over, until its resume.
    taking: Option<Taking>,
}

/// One peer's migration away from its host `from`.
struct Migrating {
    from: PeerId,
    /// `from`'s succession: (peer, can host).
    order: Vec<(PeerId, bool)>,
    m: Migration,
    attempt: Option<Attempt>,
    /// Its own turn (`Step::Host`): its gate is open.
    gate: bool,
    /// N69: it heard its old host while in contact with a successor; if
    /// that contact is lost, it goes back (rl-online's `Planner`).
    heard: bool,
}

/// An attempt to join `order[to]`.
struct Attempt {
    to: usize,
    stage: Stage,
}

#[derive(Clone, Copy)]
enum Stage {
    /// The answer comes at this virtual time.
    Ask(u64),
    /// Refused: asked again at this time.
    Retry(u64),
    /// No answer (a dead successor, an outage): until the window moves on.
    Silent,
    /// Contact: the join completes at this time.
    Join(u64),
}

/// A successor that took over from `from`, collecting until its resume.
struct Taking {
    from: PeerId,
    /// Every other succession entry holding a slot or a spectator seat from
    /// the start (rl-lobby's `away_in_match`); dead ones too.
    expected: BTreeSet<PeerId>,
    back: BTreeSet<PeerId>,
    /// Its window's end: no waiting for peers not yet back after it (N68).
    deadline: u64,
    /// What it held as it took over.
    held: Held,
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

    /// The virtual time of its next frame (`u64::MAX` while frozen or gone).
    fn next_frame_at(&self) -> u64 {
        match self.frozen_at {
            _ if self.gone => u64::MAX,
            Some(_) => u64::MAX,
            None => virtual_time(self.next_frame, self.ppm).saturating_add(self.paused_us),
        }
    }

    /// Its gate is open: it hosts, or its turn as a successor came.
    fn hosting(&self) -> bool {
        !self.gone && (self.session.is_host() || self.migrating.as_ref().is_some_and(|m| m.gate))
    }

    /// The virtual time of its attempt's next stage.
    fn attempt_due(&self) -> u64 {
        match self.migrating.as_ref().and_then(|m| m.attempt.as_ref()).map(|a| a.stage) {
            Some(Stage::Ask(t) | Stage::Retry(t) | Stage::Join(t)) => t,
            _ => u64::MAX,
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
    let host = match role {
        Role::Host { .. } => id,
        Role::Client { host, .. } | Role::Spectator { host } | Role::Joining { host } => host,
    };
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
        seat,
        host,
        gone: false,
        last_frame_us: None,
        migrations: 0,
        migrating: None,
        taking: None,
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
    if let Some(e) = sc.exits.iter().find(|e| e.peer != HOST && !ids.contains(&e.peer)) {
        return Err(fail(e.tick, format!("peer {} exits but is not in the match", e.peer)));
    }
    if let Some(c) = sc.profile_changes.iter().find(|c| !ids.contains(&c.peer)) {
        return Err(fail(c.tick, format!("peer {}'s profile changes but it is not in the match", c.peer)));
    }
    Ok(())
}

/// Plays the scenario. Virtual time moves in 1 ms steps (skipping steps in
/// which nothing is due): freezes begin or end, exits, profile changes and
/// outages, deliveries, migrations (with [`Scenario::migration`]), then
/// every peer whose frame has come advances, transmits and reports its
/// events. A host drops a peer it reports timed out or failed to repair
/// (`peer_lost`). The run ends when the current host's tick reaches
/// `ticks + settle_ticks` and every peer not dropped, gone or lost had
/// confirmed `ticks` ticks before its last advance (which re-simulated them
/// on their final inputs) and simulated them; or when no host remains and
/// every peer not dropped is gone or lost (still `Ok`).
///
/// Migrations, as rl-online drives them (spec §3.7, N65-N74):
/// - **The succession:** players in `peers` order, then spectators present
///   from the start (they can host), then joined spectators (followers). A
///   new host's is the same order without itself; a late peer moves to its
///   end as a follower.
/// - **The start:** a peer's `HostSilent`, or `PeerLeft` from its host,
///   begins its [`Migration`] (virtual time) and suspends it; past
///   [`MAX_MIGRATIONS`] it is host lost at once.
/// - **Attempts** (the peer's current profile): a successor whose gate is
///   open answers after one ping (contact) and welcomes it two pings later
///   (or `slow_joins`' time); one alive but not hosting refuses after one
///   ping, asked again [`RETRY_US`] later; a dead or frozen one, or one
///   across an outage, does not answer until the window moves on. A
///   successor whose gate closes (its window ended uncommitted, N69, give-up)
///   breaks the contacts with it (`lost_contact`).
/// - **A rejoin:** a link to the successor; the successor commits at the
///   first (`take_over` with the bots, `host_driver`), `rejoined`; the peer
///   `rehost`s and drops its migration. After the resume `rejoined` is
///   false: the peer gets a new `Role::Joining` session and the host's
///   `add_spectator` (N72).
/// - **The resume:** when every expected peer is back and every peer back
///   reported, or at the successor's window's end with every peer back
///   reported (N68, C1). A successor that expects nobody commits and
///   resumes at once.
/// - **N69:** a peer neither committed nor in contact that hears its old
///   host again (`HostHeard`) unsuspends and drops its migration; one in
///   contact remembers it and does so when that contact is lost (the
///   successor went back too), as rl-online's `Planner` does.
/// - **N70:** a host whose frame gap reaches [`STEP_DOWN_US`] and whose
///   succession has an entry that can host leaves.
/// - **Give-up:** host lost; the peer runs no more frames.
///
/// Profile changes and outages apply to the link between the peer and
/// whoever hosts it at the moment; a link made during an outage has it too.
pub fn run<S: Simulation>(sc: &Scenario, f: &mut dyn Factory<S>) -> Result<Report, Failure> {
    run_observed(sc, f, &mut ())
}

/// What a run keeps besides its peers.
struct Shared {
    net: Network,
    /// Each host's succession: (peer, can host).
    successions: BTreeMap<PeerId, Vec<(PeerId, bool)>>,
    /// Each peer's link profile now.
    profiles: BTreeMap<PeerId, Profile>,
    /// Each peer's outage now installed (virtual times).
    installed: BTreeMap<PeerId, (u64, u64)>,
    /// The bot slots a new host plays.
    bots: Vec<u8>,
    hosts: Vec<PeerId>,
    migrations: Vec<Migrated>,
    held: Vec<Held>,
    host_lost: BTreeSet<PeerId>,
    exited: BTreeSet<PeerId>,
    late: BTreeSet<PeerId>,
    left_from: BTreeMap<PeerId, PeerId>,
    /// Per old host, when each peer found it gone.
    began: BTreeMap<PeerId, BTreeMap<PeerId, u64>>,
}

impl Shared {
    /// Connects `peer` to its new host `host` with the peer's profile; an
    /// outage of either covers it.
    fn link(&mut self, peer: PeerId, host: PeerId, now: u64) {
        let profile = self.profiles.get(&peer).copied().unwrap_or(Profile::LAN);
        self.net.connect(peer, host, profile);
        for x in [peer, host] {
            if let Some(&(a, b)) = self.installed.get(&x)
                && b > now
            {
                self.net.outage(peer, host, a, b);
            }
        }
    }
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
    let mut changes: Vec<&ProfileChange> = sc.profile_changes.iter().collect();
    // Popped from the end: those of one tick in their order.
    changes.sort_by_key(|c| c.tick);
    changes.reverse();
    let mut exits: Vec<&Exit> = sc.exits.iter().collect();
    exits.sort_by_key(|e| e.tick);
    exits.reverse();
    // Exits whose tick came, by the virtual time they fall due.
    let mut timed: Vec<(u64, &Exit)> = Vec::new();
    let mut dropped: BTreeSet<PeerId> = BTreeSet::new();
    let mut repair_failed: BTreeSet<PeerId> = BTreeSet::new();

    // The succession: players, spectators from the start, followers.
    let mut order: Vec<(PeerId, bool)> =
        sc.peers.iter().filter(|p| matches!(p.seat, Seat::Player(_))).map(|p| (p.id, true)).collect();
    order.extend(
        sc.peers.iter().filter(|p| p.seat == Seat::Spectator && p.join_tick.is_none()).map(|p| (p.id, true)),
    );
    order.extend(sc.peers.iter().filter(|p| p.join_tick.is_some()).map(|p| (p.id, false)));
    let plan = sc.migration.as_ref();
    let human = plan.and_then(|m| m.host_human);
    let held_slots: Vec<u8> =
        sc.peers.iter().filter_map(|p| if let Seat::Player(s) = p.seat { Some(s) } else { None }).collect();
    let mut w = Shared {
        net,
        successions: [(HOST, order)].into(),
        profiles: sc.peers.iter().map(|p| (p.id, p.profile)).collect(),
        installed: BTreeMap::new(),
        bots: (0..sc.slots).filter(|s| !held_slots.contains(s) && Some(*s) != human).collect(),
        hosts: alloc::vec![HOST],
        migrations: Vec::new(),
        held: Vec::new(),
        host_lost: BTreeSet::new(),
        exited: BTreeSet::new(),
        late: BTreeSet::new(),
        left_from: BTreeMap::new(),
        began: BTreeMap::new(),
    };

    let end = sc.ticks.saturating_add(sc.settle_ticks);
    let limit_us = (end as u64 * TICK_US).saturating_mul(4).saturating_add(60_000_000);
    let mut settled = false;
    let mut now = 0u64;
    let mut host_tick = 0;
    loop {
        let current = w.hosts.last().copied().unwrap_or(HOST);
        let ci = index.get(&current).copied().unwrap_or(0);
        // Spectators joining mid-match: their session begins now, at the
        // current host.
        while let Some(&jp) = joiners.last().filter(|p| p.join_tick.is_some_and(|t| host_tick >= t))
            && !peers[ci].gone
        {
            joiners.pop();
            if current != HOST {
                w.link(jp.id, current, now);
            }
            let host_now = peers[ci].local(now);
            peers[ci].session.add_spectator(jp.id, host_now);
            index.insert(jp.id, peers.len());
            peers.push(make_peer(sc, f, Some(jp), Role::Joining { host: current }, now));
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
                        if !p.gone {
                            p.session.receive(from, ch, &bytes, local);
                        }
                    }
                }
                _ => {}
            }
        }
        // Exits: the current host's tick came, then their time.
        while let Some(e) = exits.last().copied().filter(|e| host_tick >= e.tick) {
            exits.pop();
            timed.push((now.saturating_add(e.after_us), e));
        }
        while let Some(k) = timed.iter().position(|&(at, _)| now >= at) {
            let (_, e) = timed.remove(k);
            let Some(p) = index.get(&e.peer).map(|&i| &mut peers[i]) else { continue };
            if p.gone {
                continue;
            }
            w.left_from.insert(p.id, p.host);
            if e.how == ExitHow::Leave {
                p.session.leave();
                let id = p.id;
                p.session.transmit(&mut |to, ch: Channel, bytes| w.net.send(now, id, to, ch, bytes));
            }
            p.gone = true;
            w.exited.insert(e.peer);
        }
        // Profile changes, on the link to the peer's host now.
        while let Some(c) = changes.last().copied().filter(|c| host_tick >= c.tick) {
            changes.pop();
            w.profiles.insert(c.peer, c.profile);
            let host = index.get(&c.peer).map_or(HOST, |&i| peers[i].host);
            w.net.set_profile(host, c.peer, c.profile);
        }
        for (&peer, list) in outages.iter_mut() {
            let free = w.installed.get(&peer).is_none_or(|&(_, until)| now >= until);
            if free && let Some((a, b)) = list.pop() {
                let host = index.get(&peer).map_or(HOST, |&i| peers[i].host);
                if host == peer {
                    // A host's own network: every link of its peers.
                    for q in peers.iter().filter(|q| q.host == peer && q.id != peer) {
                        w.net.outage(peer, q.id, a, b);
                    }
                } else {
                    w.net.outage(host, peer, a, b);
                }
                w.installed.insert(peer, (a, b));
            }
        }
        w.net.deliver_due(now, &mut |from, to, ch, bytes| {
            // Nothing reaches a peer that is gone.
            if let Some(&i) = index.get(&to)
                && !peers[i].gone
            {
                let p = &mut peers[i];
                if p.frozen_at.is_some() {
                    p.inbox.push((from, ch, bytes));
                } else {
                    let local = p.local(now);
                    p.session.receive(from, ch, &bytes, local);
                }
            }
        });
        if let Some(plan) = plan {
            migrate(sc, plan, f, &mut peers, &index, &mut w, now);
        }
        // Whom each peer follows: a host drops only its own peers.
        let following: BTreeMap<PeerId, PeerId> = peers.iter().map(|p| (p.id, p.host)).collect();
        for p in peers.iter_mut() {
            let local = p.local(now);
            if p.gone || p.frozen_at.is_some() || local < p.next_frame {
                continue;
            }
            p.next_frame += p.frame_us;
            // N70: a host stalled this long steps down, when someone can
            // take over.
            let gap = p.last_frame_us.map_or(0, |l| now.saturating_sub(l));
            p.last_frame_us = Some(now);
            if plan.is_some()
                && p.session.is_host()
                && gap >= STEP_DOWN_US
                && w.successions.get(&p.id).is_some_and(|o| o.iter().any(|e| e.1))
            {
                p.session.leave();
                let id = p.id;
                p.session.transmit(&mut |to, ch: Channel, bytes| w.net.send(now, id, to, ch, bytes));
                p.gone = true;
                w.exited.insert(id);
                w.left_from.insert(id, id);
                continue;
            }
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
            if p.session.is_host() {
                let ours = |peer: PeerId| following.get(&peer).is_none_or(|&h| h == id);
                for e in new.iter().filter(|e| match **e {
                    Event::PeerTimedOut { peer }
                    | Event::PeerLeft { peer }
                    | Event::RepairFailed { peer } => ours(peer),
                    _ => false,
                }) {
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
            } else if plan.is_some() {
                for e in &new {
                    match *e {
                        Event::HostSilent => begin(p, &mut w, now),
                        Event::PeerLeft { peer } if peer == p.host => begin(p, &mut w, now),
                        Event::HostHeard => give_back(p, &mut w, local),
                        _ => {}
                    }
                }
            }
            for e in &new {
                obs.on_event(id, e, &mut p.session);
            }
            obs.on_frame(id, &mut p.session);
            if !p.gone {
                p.session.transmit(&mut |to, ch: Channel, bytes| w.net.send(now, id, to, ch, bytes));
            }
            events.entry(id).or_default().extend(new);
        }
        let current = w.hosts.last().copied().unwrap_or(HOST);
        let ci = index.get(&current).copied().unwrap_or(0);
        host_tick = peers[ci].session.current_tick();
        if !settled && host_tick >= sc.ticks {
            for p in &sc.peers {
                let host = index.get(&p.id).map_or(HOST, |&i| peers[i].host);
                w.net.set_profile(host, p.id, Profile::LAN);
                w.profiles.insert(p.id, Profile::LAN);
            }
            settled = true;
        }
        let behind = peers.iter().find(|p| !p.done && !p.gone && !dropped.contains(&p.id));
        if !peers[ci].gone && host_tick >= end && behind.is_none() {
            break;
        }
        // No host remains: every peer left, crashed or gave up.
        if peers.iter().all(|p| p.gone || dropped.contains(&p.id)) {
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
        // The next step: the next delivery, frame, attempt or outage
        // change, on a millisecond boundary.
        let mut next = w.net.next_delivery().unwrap_or(u64::MAX);
        for p in &peers {
            next = next.min(p.next_frame_at()).min(p.thaws_at().unwrap_or(u64::MAX)).min(p.attempt_due());
        }
        for &(at, _) in &timed {
            next = next.min(at);
        }
        for (peer, &(_, until)) in &w.installed {
            if until > now && outages.get(peer).is_some_and(|l| !l.is_empty()) {
                next = next.min(until);
            }
        }
        now = next.div_ceil(1000).saturating_mul(1000).max(now + 1000);
    }

    let last = w.hosts.last().copied().unwrap_or(HOST);
    // Former hosts still running: their peers moved on without them.
    let left_behind: BTreeSet<PeerId> = w
        .hosts
        .iter()
        .filter(|&&h| h != last && index.get(&h).is_some_and(|&i| !peers[i].gone))
        .copied()
        .collect();
    for &h in &left_behind {
        w.left_from.insert(h, h);
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
        hosts: w.hosts,
        migrations: w.migrations,
        host_lost: w.host_lost,
        exited: w.exited,
        late: w.late,
        held: w.held,
        left_from: w.left_from,
        left_behind,
    };
    for p in peers {
        let log = p.session.input_log();
        // A joined spectator's log begins at its state: zeros before.
        let rows: Vec<[u8; 32]> = (0..log.frontier()).map(|t| log.row(t).unwrap_or([0; 32])).collect();
        report.checksums.insert(p.id, p.session.checksums().to_vec());
        if p.id == last {
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
        report.received.insert(p.id, w.net.received(p.id));
    }
    Ok(report)
}

/// What `session` holds: per slot, its known prefix from its log's origin.
fn held_of<S: Simulation>(migration: usize, peer: PeerId, session: &Session<S>) -> Held {
    let log = session.input_log();
    let origin = log.origin();
    let slots = (0..log.slots())
        .map(|s| (origin..log.known_upto(s).max(origin)).map(|t| log.get(t, s).unwrap_or(0)).collect())
        .collect();
    Held { migration, peer, origin, slots }
}

/// `p` found its host gone (`HostSilent`, `PeerLeft`): its migration
/// begins, suspended; past [`MAX_MIGRATIONS`] it is host lost at once.
fn begin<S: Simulation>(p: &mut Peer<S>, w: &mut Shared, now: u64) {
    if p.migrating.is_some() || p.taking.is_some() || p.session.is_host() {
        return;
    }
    let from = p.host;
    w.began.entry(from).or_default().entry(p.id).or_insert(now);
    if p.migrations >= MAX_MIGRATIONS {
        p.gone = true;
        w.host_lost.insert(p.id);
        w.left_from.insert(p.id, from);
        return;
    }
    let order = w.successions.get(&from).cloned().unwrap_or_default();
    let can_host = order.iter().map(|e| e.1).collect();
    let me = order.iter().position(|e| e.0 == p.id);
    p.session.suspend();
    p.migrating = Some(Migrating {
        from,
        order,
        m: Migration::new(can_host, me, now),
        attempt: None,
        gate: false,
        heard: false,
    });
}

/// N69: `p` hears its old host again. Neither committed nor in contact,
/// it returns to it; in contact, it remembers, and returns when that
/// contact is lost ([`lost_contact`]).
fn give_back<S: Simulation>(p: &mut Peer<S>, w: &mut Shared, local: u64) {
    let Some(m) = p.migrating.as_mut() else { return };
    if matches!(m.attempt, Some(Attempt { stage: Stage::Join(_), .. })) {
        m.heard = true;
        return;
    }
    call_off(p, w, local);
}

/// `p`'s migration is called off (N69): back to its old host, which its
/// session still follows.
fn call_off<S: Simulation>(p: &mut Peer<S>, w: &mut Shared, local: u64) {
    let Some(m) = &p.migrating else { return };
    if let Some(b) = w.began.get_mut(&m.from) {
        b.remove(&p.id);
    }
    p.migrating = None;
    p.session.unsuspend(local);
}

/// One step of every migration at virtual time `now`: steps, closed gates,
/// attempts, rejoins, take-overs and resumes.
fn migrate<S: Simulation>(
    sc: &Scenario,
    plan: &MigrationPlan,
    f: &mut dyn Factory<S>,
    peers: &mut [Peer<S>],
    index: &BTreeMap<PeerId, usize>,
    w: &mut Shared,
    now: u64,
) {
    // Steps: give-up, the successor's own turn, the successor to try.
    let mut commits = Vec::new();
    for (i, p) in peers.iter_mut().enumerate() {
        if p.gone || p.frozen_at.is_some() {
            continue;
        }
        let ping = w.profiles.get(&p.id).map_or(1_000, |pr| pr.ping_ms as u64 * 1_000);
        let Some(mg) = p.migrating.as_mut() else { continue };
        match mg.m.step(now) {
            Step::GiveUp => {
                p.migrating = None;
                p.gone = true;
                w.host_lost.insert(p.id);
                w.left_from.insert(p.id, p.host);
            }
            Step::Wait => {
                mg.gate = false;
                mg.attempt = None;
            }
            Step::Host => {
                mg.gate = true;
                mg.attempt = None;
                let nobody = mg.order.iter().all(|e| !e.1 || e.0 == p.id);
                if nobody {
                    commits.push(i);
                }
            }
            Step::Try(to) => {
                mg.gate = false;
                if mg.attempt.as_ref().is_none_or(|a| a.to != to) {
                    mg.attempt = Some(Attempt { to, stage: Stage::Ask(now + ping) });
                }
            }
        }
    }
    // A successor expecting nobody commits and resumes at once (N68).
    for i in commits {
        commit(f, peers, w, i, now);
    }
    // A gate closed (an uncommitted successor's window ended, its old host
    // came back, it gave up or crashed): its contacts break (D8).
    let open: BTreeSet<PeerId> = peers.iter().filter(|p| p.hosting()).map(|p| p.id).collect();
    for p in peers.iter_mut() {
        let Some(mg) = p.migrating.as_ref() else { continue };
        if let Some(Attempt { to, stage: Stage::Join(_) }) = mg.attempt
            && !open.contains(&mg.order[to].0)
        {
            lost_contact(p, w, to, now);
        }
    }
    // Attempts whose stage is due.
    let outage = |peer: PeerId| {
        sc.outages.iter().any(|o| {
            o.peer == peer && now >= o.from_tick as u64 * TICK_US && now < o.until_tick as u64 * TICK_US
        })
    };
    for i in 0..peers.len() {
        let p = &peers[i];
        if p.gone || p.frozen_at.is_some() {
            continue;
        }
        let Some(mg) = &p.migrating else { continue };
        let Some(a) = &mg.attempt else { continue };
        let (to, stage) = (a.to, a.stage);
        let s = mg.order[to].0;
        let si = index.get(&s).copied();
        let reachable = si.is_some_and(|si| !peers[si].gone && peers[si].frozen_at.is_none())
            && !outage(p.id)
            && !outage(s);
        let ping = w.profiles.get(&p.id).map_or(1_000, |pr| pr.ping_ms as u64 * 1_000);
        let join = plan.slow_joins.iter().find(|j| j.0 == p.id).map_or(2 * ping, |j| j.1);
        let next = match stage {
            Stage::Ask(at) if now >= at => Some(if !reachable {
                Stage::Silent
            } else if open.contains(&s) {
                Stage::Join(now + join)
            } else {
                Stage::Retry(now + RETRY_US)
            }),
            Stage::Retry(at) if now >= at => Some(Stage::Ask(now + ping)),
            Stage::Join(at) if now >= at => None,
            _ => continue,
        };
        // The join completes, or fails (lost contact).
        if next.is_none()
            && let Some(si) = si.filter(|&si| reachable && peers[si].hosting())
        {
            rejoin(sc, f, peers, w, i, si, now);
            continue;
        }
        let Some(mg) = peers[i].migrating.as_mut() else { continue };
        match next {
            Some(stage) => {
                if let Stage::Join(_) = stage {
                    mg.m.contact(to);
                }
                mg.attempt = Some(Attempt { to, stage });
            }
            None => lost_contact(&mut peers[i], w, to, now),
        }
    }
    // Resumes: every expected peer back (or the window ended) and every
    // peer back reported.
    for i in 0..peers.len() {
        let p = &peers[i];
        let Some(t) = &p.taking else { continue };
        if p.gone || p.frozen_at.is_some() {
            continue;
        }
        let all_back = t.expected.is_subset(&t.back);
        if (all_back || now >= t.deadline) && p.session.awaiting_reports().is_empty() {
            resume(peers, index, w, i, now);
        }
    }
}

/// `p`'s contact with succession entry `to` is lost; having heard its old
/// host meanwhile, it goes back to it (N69).
fn lost_contact<S: Simulation>(p: &mut Peer<S>, w: &mut Shared, to: usize, now: u64) {
    let Some(mg) = p.migrating.as_mut() else { return };
    mg.m.lost_contact(to);
    mg.attempt = None;
    if mg.heard {
        let local = p.local(now);
        call_off(p, w, local);
    }
}

/// Successor `peers[i]` commits: it takes over from its old host, plays
/// the bots, and collects until its resume.
fn commit<S: Simulation>(f: &mut dyn Factory<S>, peers: &mut [Peer<S>], w: &mut Shared, i: usize, now: u64) {
    let p = &mut peers[i];
    let Some(mg) = p.migrating.take() else { return };
    let expected = mg.order.iter().filter(|e| e.1 && e.0 != p.id).map(|e| e.0).collect();
    let deadline = mg.m.window_end(now);
    let own = match p.seat {
        Some(Seat::Player(s)) => Some(s),
        _ => None,
    };
    let held = held_of(0, p.id, &p.session);
    let local = p.local(now);
    p.session.take_over(&w.bots, local);
    p.driver.inner = f.host_driver(p.id, own, p.session.sim());
    p.host = p.id;
    p.migrations += 1;
    // A freeze to come lasts its ticks' worth of time, as the first host's
    // (its own tick stands still while it is frozen).
    p.timed_freeze = true;
    let mut order = mg.order;
    order.retain(|e| e.0 != p.id);
    w.successions.insert(p.id, order);
    w.hosts.push(p.id);
    p.taking = Some(Taking { from: mg.from, expected, back: BTreeSet::new(), deadline, held });
}

/// `peers[pi]`'s join to `peers[si]` completes: the successor commits on
/// the first, then takes the peer back (or, after its resume, as a
/// spectator arriving mid-match, N72).
fn rejoin<S: Simulation>(
    sc: &Scenario,
    f: &mut dyn Factory<S>,
    peers: &mut [Peer<S>],
    w: &mut Shared,
    pi: usize,
    si: usize,
    now: u64,
) {
    if peers[si].migrating.is_some() {
        commit(f, peers, w, si, now);
    }
    let (p, s) = if pi < si {
        let (a, b) = peers.split_at_mut(si);
        (&mut a[pi], &mut b[0])
    } else {
        let (a, b) = peers.split_at_mut(pi);
        (&mut b[0], &mut a[si])
    };
    p.migrating = None;
    p.migrations += 1;
    p.host = s.id;
    w.link(p.id, s.id, now);
    let seat = p.seat.unwrap_or(Seat::Spectator);
    if s.session.rejoined(p.id, seat, s.local(now)) {
        p.session.rehost(s.id, p.local(now));
        if let Some(t) = s.taking.as_mut() {
            t.back.insert(p.id);
        }
        return;
    }
    // Late (N72): a spectator joining mid-match, watching from a state.
    let local = p.local(now);
    p.session = Session::new(Config::new(sc.slots, 0), Role::Joining { host: s.id }, f.sim(p.id), local);
    p.driver.inner = f.driver(p.id, Some(Seat::Spectator));
    p.driver.sums.clear();
    p.seat = Some(Seat::Spectator);
    p.done = false;
    s.session.add_spectator(p.id, s.local(now));
    w.late.insert(p.id);
    if let Some(order) = w.successions.get_mut(&s.id) {
        order.retain(|e| e.0 != p.id);
        order.push((p.id, false));
    }
}

/// New host `peers[i]` resumes: what its survivors held is recorded.
fn resume<S: Simulation>(
    peers: &mut [Peer<S>],
    index: &BTreeMap<PeerId, usize>,
    w: &mut Shared,
    i: usize,
    now: u64,
) {
    let Some(t) = peers[i].taking.take() else { return };
    let local = peers[i].local(now);
    let tick = peers[i].session.resume(local);
    let k = w.migrations.len();
    w.held.push(Held { migration: k, ..t.held });
    for b in &t.back {
        if let Some(&bi) = index.get(b)
            && !peers[bi].gone
        {
            w.held.push(held_of(k, *b, &peers[bi].session));
        }
    }
    let silent_us = w.began.get(&t.from).and_then(|b| b.values().min().copied()).unwrap_or(now);
    w.migrations.push(Migrated { from: t.from, to: peers[i].id, silent_us, resumed_us: now, tick });
}

/// Checks a run: every surviving peer confirmed `ticks` ticks of the last
/// host's inputs, and its checksums (recorded while simulating, and the
/// session's confirmed ones) match an offline replay of them; no stalls
/// (when expected), malformed messages, inconsistent inputs, refused
/// states, or desyncs but of corrupted peers. Dropped, exited and host-lost
/// peers are not survivors. A spectator that joined is compared from its
/// state on (inputs from its first, checksums from its last), a late peer
/// from its last; a corrupted peer's checksums from its last loaded state
/// on. Migrations: every final input a survivor held ([`Report::held`]) is
/// in the last host's log; a peer that went (exited, host lost, or a former
/// host left behind) confirmed rows that agree with it up to, per slot, the
/// furthest prefix the survivors of its host's migration held (an exited
/// host's last frames may have reached nobody, pre-flight B3; a host cut
/// off confirms its own fills), or all of them when its host is the last.
pub fn verdict<S: Simulation>(sc: &Scenario, r: &Report, f: &mut dyn Factory<S>) -> Result<(), Failure> {
    let ticks = sc.ticks;
    let kept = |peer: &PeerId| {
        !r.dropped.contains(peer)
            && !r.exited.contains(peer)
            && !r.host_lost.contains(peer)
            && !r.left_behind.contains(peer)
    };
    let corrupted = |peer: PeerId| sc.corrupt.iter().any(|c| c.0 == peer);
    let late = |peer: PeerId| r.late.contains(&peer);
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
        let need = joined(peer) || corrupted(peer) || late(peer);
        let (inputs, sums) = match (need, first, last) {
            (false, ..) => (0, 0),
            // A late peer's old session's loads came before its new one's.
            (true, Some(_), Some(b)) if late(peer) => (b, b),
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
    // No final value a survivor held at a migration is lost.
    let last = &r.host_log;
    for h in &r.held {
        for (s, values) in h.slots.iter().enumerate() {
            for (k, &v) in values.iter().enumerate() {
                let t = h.origin as usize + k;
                if t >= last.len() {
                    break;
                }
                if last[t][s] != v {
                    return Err(fail(
                        t as u32,
                        format!(
                            "peer {} held {v:#x} for slot {s} at migration {}, the last host confirmed {:#x}",
                            h.peer, h.migration, last[t][s]
                        ),
                    ));
                }
            }
        }
    }
    // A peer that went (an exited or left-behind host, an exited or
    // host-lost peer): its confirmed rows agree with the last host's as far
    // as the survivors of its host's migration held (that host's last
    // frames may have reached nobody, B3; a host cut off fills its peers'
    // slots); all of them when its host is the last.
    let last_host = r.hosts.last().copied().unwrap_or(HOST);
    for (&peer, &host) in r.left_from.iter().filter(|(p, _)| !r.dropped.contains(p)) {
        let mut upto = [0usize; 32];
        match r.migrations.iter().position(|m| m.from == host) {
            Some(k) => {
                for h in r.held.iter().filter(|h| h.migration == k) {
                    for (s, values) in h.slots.iter().enumerate().take(32) {
                        upto[s] = upto[s].max(h.origin as usize + values.len());
                    }
                }
            }
            None if host == last_host => upto = [usize::MAX; 32],
            None => continue,
        }
        // A spectator that joined or came late: from its last state on.
        let start = if joined(peer) || late(peer) {
            let loaded = r.events.get(&peer).and_then(|e| {
                e.iter().rev().find_map(|e| match *e {
                    Event::StateLoaded { tick } => Some(tick as usize + 1),
                    _ => None,
                })
            });
            match loaded {
                Some(t) => t,
                None => continue,
            }
        } else {
            0
        };
        let rows = r.logs.get(&peer).map_or(&[][..], |v| &v[..]);
        for (t, row) in rows.iter().enumerate().take(last.len()).skip(start) {
            if let Some(s) = (0..sc.slots as usize).find(|&s| t < upto[s] && row[s] != last[t][s]) {
                return Err(fail(
                    t as u32,
                    format!("peer {peer}, gone, confirmed slot {s} otherwise than the last host"),
                ));
            }
        }
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
    for (&peer, events) in r.events.iter().filter(|(p, _)| !r.dropped.contains(p)) {
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
