//! Online matches in one process (networking spec §9.1, §9.4): a host, its
//! clients and spectators, each with its own `World` and its own bots, play
//! over the network model of `rl-netcode`'s harness; the verdict checks
//! that every peer confirmed the host's inputs and simulated the same match.
//!
//! Every peer builds the same world from the same options (every worm a
//! bot, so the loadouts are identical) and *runs* only the bots of its local
//! slots (spec §3.6: the host those no client holds, client `k` slot `k`),
//! with `rl_online::matches`' world building and `HostBots`: the online
//! game's own.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::{Context as _, bail, ensure};
use clap::ValueEnum as _;
use rl_bots::{AreaCache, Bot};
use rl_content::{Content, MatchOptions, Player};
use rl_netcode::harness::{
    self, Exit, ExitHow, Factory, Failure, HOST, MigrationPlan, Outage, PeerPlan, ProfileChange, Report,
    Scenario,
};
use rl_netcode::migration::WINDOW_US;
use rl_netcode::netsim::{LinkStats, Profile};
use rl_netcode::{
    Driver, Event, FILL_WINDOW, MAX_PREDICTION, PeerId, Seat, Session, Stats, TARGET_MARGIN, TICK_US,
    WorldSim, input_delay,
};
use rl_online::matches::{self, HostBots};
use rl_sim::Keys;
use serde::{Deserialize, Serialize};

use crate::bundle::DesyncBundle;
use crate::spec::{Difficulty, MatchSpec};

/// The first spectator's peer id.
const FIRST_SPECTATOR: PeerId = 100;
/// A spectator's frame period: a 60 Hz display.
const SPECTATOR_FRAME_US: u64 = 16_667;
/// A client's frame period: a 144 Hz display.
const CLIENT_FRAME_US: u64 = 6_944;

/// An online match of bots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetSpec {
    pub mod_id: String,
    pub level_id: String,
    /// The match seed.
    pub seed: u32,
    /// Worms; clients take slots `0..clients`, the host's bots the rest.
    pub slots: u8,
    pub clients: u8,
    pub spectators: u8,
    /// Spectators arrive at this tick (`None`: from the start).
    #[serde(default)]
    pub join_tick: Option<u32>,
    /// A network profile's name (`lan`, `good`, `typical`, `bad`, `awful`).
    pub profile: String,
    /// Clients' clocks run `-drift_ppm..=drift_ppm` fast, spread evenly.
    #[serde(default)]
    pub drift_ppm: i32,
    pub ticks: u32,
    /// `(peer, from tick, until tick)`: nothing passes between host and peer.
    #[serde(default)]
    pub outages: Vec<(u32, u32, u32)>,
    #[serde(default)]
    pub net_seed: u64,
    /// The bots' difficulty (`easy`, `medium`, `hard`, `xtreme`); `mixed` by default.
    #[serde(default)]
    pub difficulty: Option<String>,
    /// Planted faults `(peer, host tick, worm, health)`: at the peer's first
    /// frame after the host's tick at which it has confirmed that tick, the
    /// worm's health is set in that peer's world alone, as it was after its
    /// last confirmed tick (a desync for the harness to repair).
    #[serde(default)]
    pub faults: Vec<(u32, u32, u8, f32)>,
    /// `(peer, host tick, "crash" | "leave")`: peers leaving (peer 0 the
    /// host) when the current host's tick reaches `tick`.
    #[serde(default)]
    pub exits: Vec<(u32, u32, String)>,
    /// The host frozen from tick `.0` for `.1 - .0` ticks' worth of time.
    #[serde(default)]
    pub host_freeze: Option<(u32, u32)>,
    /// `(host tick, peer, profile)`: from then, that peer's link to whoever
    /// hosts takes the profile.
    #[serde(default)]
    pub profile_changes: Vec<(u32, u32, String)>,
    /// Host migration on (spec §3.7); `exits` naming the host and
    /// `host_freeze` turn it on too.
    #[serde(default)]
    pub migration: bool,
}

impl NetSpec {
    /// A file name for this run: readable parts and a hash of the whole
    /// spec (FNV-1a over its RON, 8 hex digits), so specs that differ in
    /// anything get different names.
    pub fn stem(&self) -> String {
        let short = |id: &str| id.trim_start_matches("olx-level-").trim_start_matches("olx-").to_string();
        let ron = ron::to_string(self).unwrap_or_default();
        let hash = ron.bytes().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
        format!(
            "{}-{}-{}-{}s{}c{}v-seed{}-{hash:08x}",
            short(&self.mod_id),
            short(&self.level_id),
            self.profile,
            self.slots,
            self.clients,
            self.spectators,
            self.seed,
        )
    }

    /// The seat of peer `id` in this spec's scenario (clients `1..=clients`
    /// play slot `id - 1`, spectators are `100..`).
    fn scenario_seat(&self, id: PeerId) -> Option<Seat> {
        (1..=PeerId::from(self.clients)).contains(&id).then(|| Seat::Player((id - 1) as u8))
    }

    fn profile(&self) -> anyhow::Result<Profile> {
        Profile::by_name(&self.profile).with_context(|| format!("unknown network profile {:?}", self.profile))
    }

    /// Whether `peer` is in this spec's match: the host, a client or a spectator.
    fn has_peer(&self, peer: PeerId) -> bool {
        peer <= PeerId::from(self.clients)
            || (FIRST_SPECTATOR..FIRST_SPECTATOR + PeerId::from(self.spectators)).contains(&peer)
    }

    /// Host migration is on: asked for, or needed by an exit of the host or
    /// a host freeze (a host gone with migration off ends the match for
    /// nobody, and the verdict would fail on the stalled clients).
    pub fn migrates(&self) -> bool {
        self.migration || self.host_freeze.is_some() || self.exits.iter().any(|e| e.0 == HOST)
    }

    /// The exits as the harness takes them (an exit's `after_us` is 0: at
    /// the tick itself).
    fn harness_exits(&self) -> anyhow::Result<Vec<Exit>> {
        self.exits
            .iter()
            .map(|(peer, tick, how)| {
                ensure!(self.has_peer(*peer), "an exit names peer {peer}, not in the match");
                let how = match how.as_str() {
                    "crash" => ExitHow::Crash,
                    "leave" => ExitHow::Leave,
                    other => bail!("an exit is \"crash\" or \"leave\", not {other:?}"),
                };
                Ok(Exit { peer: *peer, tick: *tick, after_us: 0, how })
            })
            .collect()
    }

    fn harness_profile_changes(&self) -> anyhow::Result<Vec<ProfileChange>> {
        self.profile_changes
            .iter()
            .map(|(tick, peer, name)| {
                ensure!(
                    *peer != HOST && self.has_peer(*peer),
                    "a profile change names peer {peer}, not a client or spectator"
                );
                let profile =
                    Profile::by_name(name).with_context(|| format!("unknown network profile {name:?}"))?;
                Ok(ProfileChange { tick: *tick, peer: *peer, profile })
            })
            .collect()
    }

    /// No player may stall on a profile up to `bad` (spec §9.2), unless a
    /// link changes to `awful` mid-run. Stalls the run itself causes (a
    /// migration, an outage) are excused by [`judge`]'s windows instead.
    fn expect_no_stalls(&self) -> bool {
        matches!(self.profile.as_str(), "lan" | "good" | "typical" | "bad")
            && !self.profile_changes.iter().any(|c| c.2 == "awful")
    }

    /// The options every peer builds its world from.
    pub fn match_options(&self) -> anyhow::Result<MatchOptions> {
        let difficulty = match &self.difficulty {
            None => Difficulty::Mixed,
            Some(d) => {
                Difficulty::from_str(d, true).map_err(|_| anyhow::anyhow!("unknown difficulty {d:?}"))?
            }
        };
        let mut m =
            MatchSpec::new(&self.mod_id, &self.level_id, usize::from(self.slots), self.seed, self.ticks);
        m.difficulty = difficulty;
        Ok(m.match_options())
    }

    /// The harness scenario: clients are peers `1..=clients` (client `k`
    /// plays slot `k`), spectators peers `100..`.
    pub fn scenario(&self) -> anyhow::Result<Scenario> {
        let profile = self.profile()?;
        ensure!((2..=32).contains(&self.slots), "{} slots: a match has 2 to 32", self.slots);
        ensure!(self.clients <= self.slots, "{} clients for {} slots", self.clients, self.slots);
        ensure!(self.spectators <= 100, "at most 100 spectators");
        if let Some((from, until)) = self.host_freeze {
            ensure!(from < until, "a host freeze {from}:{until} ends before it begins");
        }
        let exits = self.harness_exits()?;
        let profile_changes = self.harness_profile_changes()?;
        let mut peers = Vec::new();
        let n = i64::from(self.clients);
        for k in 0..self.clients {
            let ppm = if n > 1 {
                let d = i64::from(self.drift_ppm);
                (-d + 2 * d * i64::from(k) / (n - 1)) as i32
            } else {
                0
            };
            peers.push(PeerPlan {
                id: 1 + PeerId::from(k),
                seat: Seat::Player(k),
                profile,
                clock_ppm: ppm,
                input_delay: input_delay(profile.rtt_ms()),
                frame_us: CLIENT_FRAME_US,
                join_tick: None,
                freeze: None,
            });
        }
        for s in 0..self.spectators {
            peers.push(PeerPlan {
                id: FIRST_SPECTATOR + PeerId::from(s),
                seat: Seat::Spectator,
                profile,
                clock_ppm: 0,
                input_delay: 0,
                frame_us: SPECTATOR_FRAME_US,
                join_tick: self.join_tick,
                freeze: None,
            });
        }
        Ok(Scenario {
            seed: self.net_seed,
            slots: self.slots,
            peers,
            ticks: self.ticks,
            settle_ticks: 300,
            outages: self
                .outages
                .iter()
                .map(|&(peer, from_tick, until_tick)| Outage { peer, from_tick, until_tick, one_way: false })
                .collect(),
            expect_no_stalls: self.expect_no_stalls(),
            corrupt: self.faults.iter().map(|f| (f.0, f.1)).collect(),
            host_freeze: self.host_freeze,
            exits,
            // Every worm is a bot: the host has no player of its own to abandon.
            migration: self.migrates().then(MigrationPlan::default),
            profile_changes,
        })
    }
}

/// Builds every peer's world and bots.
pub struct WorldFactory<'c> {
    content: &'c Content,
    options: MatchOptions,
    spec: NetSpec,
    /// The bots of a peer's world by slot, from `sim` to `driver`.
    bots: BTreeMap<PeerId, BTreeMap<u8, Bot>>,
    /// Each client's own bot, shared with its driver: a client that takes
    /// the match over keeps it (`host_driver`).
    own: BTreeMap<PeerId, (u8, Rc<RefCell<Bot>>)>,
}

impl<'c> WorldFactory<'c> {
    /// Builds a world once to validate the content and the options.
    pub fn new(content: &'c Content, spec: &NetSpec) -> anyhow::Result<Self> {
        let options = spec.match_options()?;
        content.build_match(&options).with_context(|| format!("setting up {}", spec.stem()))?;
        Ok(WorldFactory { content, options, spec: spec.clone(), bots: BTreeMap::new(), own: BTreeMap::new() })
    }
}

/// A peer's bots: those it hosts (`hosted`) and a client's own (`own`),
/// kept apart so that a client that takes the match over keeps its own bot
/// while the host's bots restart (spec §3.7, N75).
struct PeerBots {
    hosted: HostBots,
    own: Option<(u8, Rc<RefCell<Bot>>)>,
}

impl Driver<WorldSim> for PeerBots {
    fn local_inputs(&mut self, sim: &WorldSim, _tick: u32, slots: &[u8], out: &mut [Keys]) {
        match &self.own {
            Some((slot, bot)) => {
                let mut think = || bot.borrow_mut().think(sim.world(), sim.events());
                self.hosted.press(sim, slots, out, Some((*slot, &mut think)));
            }
            None => self.hosted.press(sim, slots, out, None),
        }
    }
}

impl Factory<WorldSim> for WorldFactory<'_> {
    fn sim(&mut self, peer: PeerId) -> WorldSim {
        // Validated by `new`: only a content directory changing under the
        // run could fail now.
        let built = matches::build_options(self.content, &self.options)
            .unwrap_or_else(|e| panic!("building {}: {e:#}", self.spec.stem()));
        // Each build has its own area cache: bots of different peers never share one.
        self.bots.insert(peer, built.bots);
        built.sim
    }

    fn driver(&mut self, peer: PeerId, seat: Option<Seat>) -> Box<dyn Driver<WorldSim>> {
        // The harness makes a peer's sim before its driver.
        let mut bots = self.bots.remove(&peer).expect("a peer's sim is made before its driver");
        let clients = self.spec.clients;
        let own = match seat {
            Some(Seat::Player(s)) => bots.remove(&s).map(|b| (s, Rc::new(RefCell::new(b)))),
            _ => None,
        };
        self.own.remove(&peer);
        if let Some(o) = &own {
            self.own.insert(peer, o.clone());
        }
        bots.retain(|&slot, _| seat.is_none() && slot >= clients);
        Box::new(PeerBots { hosted: HostBots::new(bots), own })
    }

    /// A client that took over: its own bot kept, the host's bots (the
    /// slots no client plays) restarted as `rl_online::matches::
    /// restart_bots` restarts them: on the current world, the match's seed,
    /// one new area cache.
    fn host_driver(&mut self, peer: PeerId, own: Option<u8>, sim: &WorldSim) -> Box<dyn Driver<WorldSim>> {
        let world = sim.world();
        let cache = AreaCache::new(world.terrain());
        let clients = usize::from(self.spec.clients);
        let seed = self.options.seed;
        let bots = self
            .options
            .players
            .iter()
            .enumerate()
            .skip(clients)
            .filter_map(|(i, p)| match p {
                Player::Bot(cfg) => {
                    Some((i as u8, Bot::with_cache(i, *cfg, seed, world, Arc::clone(&cache))))
                }
                Player::Human | Player::HumanWith(_) => None,
            })
            .collect();
        let own = self.own.get(&peer).filter(|o| Some(o.0) == own).cloned();
        Box::new(PeerBots { hosted: HostBots::new(bots), own })
    }

    /// Fault `index` of the spec: a worm's health. A fault naming no worm is
    /// ignored (the spec is untrusted input).
    fn corrupt(&mut self, sim: &mut WorldSim, index: usize) {
        if let Some(&(_, _, worm, health)) = self.spec.faults.get(index)
            && usize::from(worm) < sim.world().worms().len()
        {
            sim.world_mut().set_health(usize::from(worm), health);
        }
    }
}

/// A finished run.
pub struct NetRun {
    pub spec: NetSpec,
    pub report: Report,
    pub verdict: Result<(), Failure>,
    /// The desync bundles written (`run_with`).
    pub bundles: Vec<PathBuf>,
}

fn empty_report() -> Report {
    Report {
        host_log: Vec::new(),
        logs: BTreeMap::new(),
        sims: BTreeMap::new(),
        stats: BTreeMap::new(),
        events: BTreeMap::new(),
        received: BTreeMap::new(),
        duration_us: 0,
        dropped: Default::default(),
        repair_failed: Default::default(),
        checksums: BTreeMap::new(),
        hosts: Vec::new(),
        migrations: Vec::new(),
        host_lost: Default::default(),
        gave_up: BTreeMap::new(),
        exited: Default::default(),
        late: Default::default(),
        held: Vec::new(),
        left_from: BTreeMap::new(),
        left_behind: Default::default(),
    }
}

/// Plays the match and checks it. A harness error is a failed verdict (with
/// an empty report); an error here is bad content or a bad spec.
pub fn run(content: &Content, spec: &NetSpec) -> anyhow::Result<NetRun> {
    run_with(content, spec, None)
}

/// [`run`]; with `bundles` set, a client that reports a desync writes a
/// bundle to `<dir>/net-<stem>-peer<k>.rlds`.
pub fn run_with(content: &Content, spec: &NetSpec, bundles: Option<&Path>) -> anyhow::Result<NetRun> {
    let scenario = spec.scenario()?;
    let mut factory = WorldFactory::new(content, spec)?;
    let recorder = BundleRecorder { spec, dir: bundles, pending: BTreeMap::new(), done: Vec::new() };
    let mut watch = Watch { bundles: recorder, stalls: StallTracker::default() };
    let (report, verdict) = match harness::run_observed(&scenario, &mut factory, &mut watch) {
        Ok(report) => {
            let episodes = watch.stalls.episodes();
            let verdict = judge(spec, &scenario, &report, &episodes, &mut factory);
            (report, verdict)
        }
        Err(f) => (empty_report(), Err(f)),
    };
    let written = watch.bundles.finish()?;
    Ok(NetRun { spec: spec.clone(), report, verdict, bundles: written })
}

/// N66: on LAN, good and typical links the match continues within 3 s of
/// the peers finding their host gone (from the first `HostSilent` or
/// `PeerLeft` to the new host's resume), when nothing but hosts left; on
/// `bad` and `awful` links within the first successor's window.
pub const N66_US: u64 = 3_000_000;

/// The run's verdict: `harness::verdict`, with a stall rule of its own
/// when stalls are expected at all (`expect_no_stalls`: no `awful` link) and
/// a run causes some itself (plan 12c Task 8's decision RS-2, Task 11's
/// SK-1). A host gone stalls its players at the prediction limit before they
/// find it gone, until the new host's resume reaches them; an outage stalls
/// the peer it cuts off (more than `MAX_PREDICTION` ticks of it always do).
/// The harness's `expect_no_stalls` would flag both, so every stall episode
/// is judged by [`stalls_within_windows`] instead: it must begin and end
/// (the player runs a tick again, by the current host's tick) inside one
/// excused window. With migration on, [`n66`] bounds each migration's length.
fn judge(
    spec: &NetSpec,
    sc: &Scenario,
    r: &Report,
    episodes: &[StallEpisode],
    f: &mut WorldFactory<'_>,
) -> Result<(), Failure> {
    let worsened = spec.profile_changes.iter().any(|c| worsened_from(spec, c.1).is_some());
    if sc.migration.is_none() && sc.outages.is_empty() && !worsened {
        return harness::verdict(sc, r, f);
    }
    harness::verdict(&Scenario { expect_no_stalls: false, ..sc.clone() }, r, f)?;
    if sc.expect_no_stalls {
        stalls_within_windows(spec, r, episodes)?;
    }
    n66(spec, r)
}

/// A player's stall episode: the tick it stalled before, and the current
/// host's tick when it ran a tick again (`None`: it never did).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StallEpisode {
    pub peer: PeerId,
    pub tick: u32,
    pub ended: Option<u32>,
}

/// A span of the current host's ticks in which some peers' stalls are
/// excused.
struct Window {
    ticks: core::ops::RangeInclusive<u32>,
    /// `None`: every player's stall; `Some(p)`: peer `p`'s, everyone's
    /// while `p` is the current host (an outage cuts a host off from all),
    /// and that of every player whose link stalls on `p`'s fills
    /// ([`fills_stall`]).
    peer: Option<PeerId>,
}

/// Whether a player on `profile` stalls while the host fills another
/// player's slot (that player cut off): the host's frames then confirm only
/// up to `FILL_WINDOW` ticks behind its tick, and reach the player one
/// trip later, while the player runs a trip and `TARGET_MARGIN` ahead of
/// the host; at `MAX_PREDICTION` past what it has confirmed it stalls.
/// True on `bad` (two trips of up to 10 ticks: 33), not on `typical` (25);
/// net-soak's `bad-16s4c0v-seed3660989505`, the toy harness alike.
fn fills_stall(profile: &Profile) -> bool {
    let trip = (u64::from(profile.ping_ms + profile.jitter_ms) * 1000 / 2).div_ceil(TICK_US) as u32;
    2 * trip + FILL_WINDOW + TARGET_MARGIN.unsigned_abs() >= MAX_PREDICTION
}

/// The host at the host tick `tick`: the last migration's successor before
/// it, else the first host.
fn host_at(r: &Report, tick: u32) -> PeerId {
    r.migrations.iter().rfind(|m| m.tick <= tick).map_or(HOST, |m| m.to)
}

/// The host tick reached at the virtual time `tick * TICK_US` of an outage
/// (the harness's outages are in virtual time): during a migration (from
/// the first peer finding its host gone to the resume) the migration's
/// tick; after one the new host counts on from its resume tick at its
/// resume time; before, a host that froze (and did not step down) is
/// behind the clock by the freeze.
fn host_tick_at(spec: &NetSpec, r: &Report, outage_tick: u32) -> u32 {
    let us = u64::from(outage_tick) * TICK_US;
    let ticks = |from_us: u64, at: u32| {
        let ahead = us.saturating_sub(from_us) / TICK_US;
        at.saturating_add(u32::try_from(ahead).unwrap_or(u32::MAX))
    };
    // The match stands at a migration's tick from its departure to its
    // resume (SK-4's `typical-3s3c1v-seed2454907585`: an outage begun
    // meanwhile was placed 6 s of ticks later).
    if let Some(m) = r.migrations.iter().find(|m| m.silent_us <= us && us < m.resumed_us) {
        return m.tick;
    }
    if let Some(m) = r.migrations.iter().rfind(|m| m.resumed_us <= us) {
        return ticks(m.resumed_us, m.tick);
    }
    match spec.host_freeze {
        Some((from, until)) if outage_tick >= until => outage_tick - (until - from),
        _ => outage_tick,
    }
}

/// The excused windows: each migration's ([`migration_windows`]) and, per
/// outage of a player, its span in host ticks ([`host_tick_at`]) from
/// `MAX_PREDICTION` before it to its recovery after (SK-1, SK-2); an outage
/// of a spectator excuses nothing: no player waits for it. A migration
/// called off (N69) needs none of its own: only an outage silences a host
/// for 2 s without a step-down, and its window covers it.
fn excused_windows(spec: &NetSpec, r: &Report) -> Vec<Window> {
    let mut windows: Vec<Window> =
        migration_windows(spec, r).into_iter().map(|ticks| Window { ticks, peer: None }).collect();
    for (peer, from, until) in merged_outages(&spec.outages) {
        if (1..=PeerId::from(spec.clients)).contains(&peer) {
            let (from, until) = (host_tick_at(spec, r, from), host_tick_at(spec, r, until));
            // SK-2: past its end, the peer's recovery on its slowest link.
            let recovery = slowest_profile(spec, peer)
                .outage_recovery_ticks(until.saturating_sub(from), CLIENT_FRAME_US)
                .max(MAX_PREDICTION);
            windows.push(Window {
                ticks: from.saturating_sub(MAX_PREDICTION)..=until.saturating_add(recovery),
                peer: Some(peer),
            });
        }
    }
    windows
}

/// Each peer's outages with the overlapping ones joined: the network cuts
/// a link in several at once until the last is over (rl-netcode's
/// `Link::outage_end`), so their stalls are one outage's (SK-4, net-soak's
/// `typical-8s4c0v-seed2732616080`). By peer, then start.
fn merged_outages(outages: &[(PeerId, u32, u32)]) -> Vec<(PeerId, u32, u32)> {
    let mut sorted = outages.to_vec();
    sorted.sort_unstable();
    let mut out: Vec<(PeerId, u32, u32)> = Vec::new();
    for (peer, from, until) in sorted {
        match out.last_mut() {
            Some(last) if last.0 == peer && from <= last.2 => last.2 = last.2.max(until),
            _ => out.push((peer, from, until)),
        }
    }
    out
}

/// The slowest link (ping plus jitter) `peer` has in the run: the spec's
/// profile, or one a profile change gives it.
fn slowest_profile(spec: &NetSpec, peer: PeerId) -> Profile {
    let base = spec.profile().unwrap_or(Profile::LAN);
    spec.profile_changes
        .iter()
        .filter(|c| c.1 == peer)
        .filter_map(|c| Profile::by_name(&c.2))
        .fold(base, |a, b| if b.ping_ms + b.jitter_ms > a.ping_ms + a.jitter_ms { b } else { a })
}

/// The host tick from which `peer`'s link is worse (ping plus jitter) than
/// the match's profile, on which every input delay was chosen (fixed per
/// match, N2): its first profile change to one. Ruling SK-6: from then on
/// its stalls are not judged, as no stall is after a change to `awful`.
fn worsened_from(spec: &NetSpec, peer: PeerId) -> Option<u32> {
    let base = spec.profile().unwrap_or(Profile::LAN);
    let slow = |p: &Profile| p.ping_ms + p.jitter_ms;
    spec.profile_changes
        .iter()
        .filter(|c| c.1 == peer)
        .filter(|c| Profile::by_name(&c.2).is_some_and(|p| slow(&p) > slow(&base)))
        .map(|c| c.0)
        .min()
}

/// Every player's stall episode (the host's and the clients', not
/// dropped) begins and ends within one excused window ([`excused_windows`]),
/// but for a player whose link worsened, from the change on
/// ([`worsened_from`]).
fn stalls_within_windows(spec: &NetSpec, r: &Report, episodes: &[StallEpisode]) -> Result<(), Failure> {
    let windows = excused_windows(spec, r);
    let player = |p: PeerId| p == HOST || (1..=PeerId::from(spec.clients)).contains(&p);
    // A peer that went (host lost, crashed, left) runs no tick again: its
    // last episode never ends, and is no stall of the match's.
    let went = |e: &&StallEpisode| {
        e.ended.is_none() && (r.host_lost.contains(&e.peer) || r.exited.contains(&e.peer))
    };
    let worse = |e: &&StallEpisode| worsened_from(spec, e.peer).is_some_and(|t| e.tick >= t);
    for e in
        episodes.iter().filter(|e| player(e.peer) && !r.dropped.contains(&e.peer) && !went(e) && !worse(e))
    {
        let covering: Vec<&Window> = windows
            .iter()
            .filter(|w| {
                w.ticks.contains(&e.tick)
                    && w.peer.is_none_or(|p| {
                        p == e.peer || p == host_at(r, e.tick) || fills_stall(&slowest_profile(spec, e.peer))
                    })
            })
            .collect();
        if covering.is_empty() {
            return Err(Failure { tick: e.tick, what: format!("player peer {} stalled", e.peer) });
        }
        if !covering.iter().any(|w| e.ended.is_some_and(|end| end <= *w.ticks.end())) {
            let until = e.ended.map_or_else(|| "the run's end".into(), |t| format!("host tick {t}"));
            let last = covering.iter().map(|w| *w.ticks.end()).max().unwrap_or(0);
            return Err(Failure {
                tick: e.tick,
                what: format!(
                    "player peer {} stalled until {until}, past its window's end plus MAX_PREDICTION ({last})",
                    e.peer
                ),
            });
        }
    }
    Ok(())
}

/// Each migration within [`N66_US`] on LAN, good and typical links (else
/// `WINDOW_US`), when only hosts left: no other exit, outage or profile
/// change (a successor waits for a departed peer up to its window).
fn n66(spec: &NetSpec, r: &Report) -> Result<(), Failure> {
    let clean = spec.outages.is_empty()
        && spec.profile_changes.is_empty()
        && spec.exits.iter().all(|e| r.hosts.contains(&e.0));
    if clean {
        let fast = matches!(spec.profile.as_str(), "lan" | "good" | "typical");
        let bound = if fast { N66_US } else { WINDOW_US };
        for (k, m) in r.migrations.iter().enumerate() {
            let took = m.resumed_us.saturating_sub(m.silent_us);
            if took > bound {
                return Err(Failure {
                    tick: m.tick,
                    what: format!(
                        "migration {k} from peer {} to peer {} took {} ms, over {} ms (N66)",
                        m.from,
                        m.to,
                        took / 1_000,
                        bound / 1_000
                    ),
                });
            }
        }
    }
    Ok(())
}

/// The ticks within which each migration excuses a stall episode: from its
/// old host's departure (its exit's tick, or the host freeze's start; else
/// the resume tick) less `MAX_PREDICTION` to the resume tick plus
/// `MAX_PREDICTION`.
fn migration_windows(spec: &NetSpec, r: &Report) -> Vec<core::ops::RangeInclusive<u32>> {
    r.migrations
        .iter()
        .map(|m| {
            let exit = spec.exits.iter().filter(|e| e.0 == m.from && e.1 <= m.tick).map(|e| e.1).max();
            let freeze = spec.host_freeze.filter(|f| m.from == HOST && f.0 <= m.tick).map(|f| f.0);
            let gone = exit.into_iter().chain(freeze).max().unwrap_or(m.tick);
            gone.saturating_sub(MAX_PREDICTION)..=m.tick.saturating_add(MAX_PREDICTION)
        })
        .collect()
}

/// Collects a client's desync bundle: started at its `Desync`, completed by
/// the state the host sends, written when that state is loaded.
struct BundleRecorder<'a> {
    spec: &'a NetSpec,
    dir: Option<&'a Path>,
    pending: BTreeMap<PeerId, DesyncBundle>,
    /// Written (or failed to write): one bundle per peer.
    done: Vec<(PeerId, anyhow::Result<PathBuf>)>,
}

impl BundleRecorder<'_> {
    fn write(&mut self, peer: PeerId) {
        let (Some(dir), Some(b)) = (self.dir, self.pending.remove(&peer)) else { return };
        let path = dir.join(format!("net-{}-peer{peer}.rlds", self.spec.stem()));
        let result = std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&path, b.encode()))
            .map(|()| path.clone())
            .with_context(|| format!("writing {}", path.display()));
        self.done.push((peer, result));
    }

    /// Writes bundles still waiting for a state; the paths, or the first write error.
    fn finish(mut self) -> anyhow::Result<Vec<PathBuf>> {
        let waiting: Vec<PeerId> = self.pending.keys().copied().collect();
        for peer in waiting {
            self.write(peer);
        }
        self.done.into_iter().map(|(_, r)| r).collect()
    }
}

impl harness::Observer<WorldSim> for BundleRecorder<'_> {
    fn on_event(&mut self, peer: PeerId, event: &Event, session: &mut Session<WorldSim>) {
        if self.dir.is_none() || !matches!(self.spec.scenario_seat(peer), Some(Seat::Player(_))) {
            return;
        }
        let rows = |s: &Session<WorldSim>| -> Vec<[u8; 32]> {
            let log = s.input_log();
            (0..log.frontier()).map(|t| log.row(t).unwrap_or([0; 32])).collect()
        };
        match *event {
            Event::Desync { tick, remote, .. }
                if !self.pending.contains_key(&peer) && !self.done.iter().any(|d| d.0 == peer) =>
            {
                self.pending.insert(
                    peer,
                    DesyncBundle {
                        spec: self.spec.clone(),
                        peer,
                        fingerprint: rl_proto::FINGERPRINT,
                        platform: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
                        inputs: rows(session),
                        local_checksums: session.checksums().to_vec(),
                        host: (tick, remote),
                        local_state: session.encode_confirmed_state(),
                        host_state: None,
                    },
                );
            }
            Event::StateArrived { tick, ref bytes } => {
                if let Some(b) = self.pending.get_mut(&peer) {
                    b.host_state = Some((tick, bytes.clone()));
                }
            }
            Event::StateLoaded { .. } => {
                if let Some(b) = self.pending.get_mut(&peer) {
                    // The inputs may have grown since the report: the host's
                    // state can be past them.
                    b.inputs = rows(session);
                }
                self.write(peer);
            }
            _ => {}
        }
    }
}

/// Follows every player's stall episodes as they happen: when each began
/// (`Event::Stalled`, its tick standing still while stalled) and the
/// current host's tick when the player ran a tick again (the episode's end;
/// the event marks only its start).
#[derive(Default)]
struct StallTracker {
    /// The current host's tick: the first host's, then from each
    /// migration's resume the new host's. Not the furthest any host
    /// reached: an old host cut off from its peers plays on alone, ahead
    /// of the match (left behind), and would end every episode late.
    clock: u32,
    /// The current host: the first, then whoever resumed a migration.
    host: PeerId,
    episodes: Vec<StallEpisode>,
    /// Each peer's episode still running.
    open: BTreeMap<PeerId, usize>,
}

impl StallTracker {
    fn episodes(&self) -> Vec<StallEpisode> {
        self.episodes.clone()
    }
}

impl harness::Observer<WorldSim> for StallTracker {
    fn on_event(&mut self, peer: PeerId, event: &Event, session: &mut Session<WorldSim>) {
        if let Event::Resumed { .. } = *event
            && session.is_host()
        {
            self.host = peer;
            self.clock = session.current_tick();
        }
        if let Event::Stalled { tick } = *event {
            if let Some(i) = self.open.remove(&peer) {
                self.episodes[i].ended = Some(self.clock);
            }
            self.open.insert(peer, self.episodes.len());
            self.episodes.push(StallEpisode { peer, tick, ended: None });
        }
    }

    fn on_frame(&mut self, peer: PeerId, session: &mut Session<WorldSim>) {
        if peer == self.host {
            self.clock = session.current_tick();
        }
        if let Some(&i) = self.open.get(&peer)
            && session.current_tick() > self.episodes[i].tick
        {
            self.episodes[i].ended = Some(self.clock);
            self.open.remove(&peer);
        }
    }
}

/// The run's observers: the desync bundles and the stall episodes.
struct Watch<'a> {
    bundles: BundleRecorder<'a>,
    stalls: StallTracker,
}

impl harness::Observer<WorldSim> for Watch<'_> {
    fn on_event(&mut self, peer: PeerId, event: &Event, session: &mut Session<WorldSim>) {
        self.bundles.on_event(peer, event, session);
        self.stalls.on_event(peer, event, session);
    }

    fn on_frame(&mut self, peer: PeerId, session: &mut Session<WorldSim>) {
        self.bundles.on_frame(peer, session);
        self.stalls.on_frame(peer, session);
    }
}

/// What `net-<stem>.ron` holds, in `target/failures/` and `fixtures/net/cases/`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetCase {
    pub spec: NetSpec,
    /// What failed, for the reader.
    #[serde(default)]
    pub failure: Option<String>,
}

impl NetCase {
    /// Reads a case file (the parse error names the path).
    pub fn load(path: &Path) -> anyhow::Result<NetCase> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        ron::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }
}

/// Writes the case of a failed run to `<dir>/net-<stem>.ron`.
pub fn write_failure(dir: &Path, run: &NetRun) -> anyhow::Result<PathBuf> {
    let Err(f) = &run.verdict else { bail!("{} did not fail", run.spec.stem()) };
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let case = NetCase { spec: run.spec.clone(), failure: Some(format!("{} at tick {}", f.what, f.tick)) };
    let text = ron::ser::to_string_pretty(&case, ron::ser::PrettyConfig::default())?;
    let path = dir.join(format!("net-{}.ron", run.spec.stem()));
    std::fs::write(&path, text + "\n").with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// The depth below which `q` of the rollbacks fall (the histogram's index).
fn depth_percentile(s: &Stats, q: f64) -> usize {
    let total: u64 = s.depth_hist.iter().sum();
    if total == 0 {
        return 0;
    }
    let rank = ((total as f64 * q).ceil() as u64).max(1);
    let mut seen = 0;
    for (depth, n) in s.depth_hist.iter().enumerate() {
        seen += n;
        if seen >= rank {
            return depth;
        }
    }
    s.depth_hist.len() - 1
}

impl NetRun {
    /// The profile, the peers and one line per client (and the host).
    pub fn summary(&self) -> String {
        let s = &self.spec;
        let secs = (self.report.duration_us as f64 / 1e6).max(1e-9);
        let mut out = format!(
            "{}: profile {}, {} slots, {} clients, {} spectators, {} ticks ({:.1} s virtual)",
            s.stem(),
            s.profile,
            s.slots,
            s.clients,
            s.spectators,
            s.ticks,
            secs
        );
        for (id, st) in &self.report.stats {
            let who = if *id == HOST {
                "host".to_string()
            } else if *id >= FIRST_SPECTATOR {
                format!("spectator {id}")
            } else {
                format!("client {id}")
            };
            let recv = self.report.received.get(id).copied().unwrap_or(LinkStats::default());
            out += &format!(
                "\n  {who}: rollbacks {} (depth p50 {}, p99 {}), fills {}, stalls {}, desyncs {}, {:.0} B/s received",
                st.rollbacks,
                depth_percentile(st, 0.5),
                depth_percentile(st, 0.99),
                st.fills,
                st.stalls,
                st.desyncs,
                st.bytes_in.max(recv.bytes) as f64 / secs,
            );
        }
        for m in &self.report.migrations {
            out += &format!(
                "\nmigrated: {} -> {} at tick {} ({} ms after the host was found gone)",
                m.from,
                m.to,
                m.tick,
                m.resumed_us.saturating_sub(m.silent_us) / 1_000
            );
        }
        if !self.report.host_lost.is_empty() {
            let lost: Vec<String> = self.report.host_lost.iter().map(PeerId::to_string).collect();
            out += &format!("\nhost lost: {}", lost.join(", "));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> NetSpec {
        NetSpec {
            mod_id: "olx-classic".into(),
            level_id: "olx-level-castlestrike".into(),
            seed: 1,
            slots: 8,
            clients: 3,
            spectators: 1,
            join_tick: None,
            profile: "typical".into(),
            drift_ppm: 0,
            ticks: 600,
            outages: vec![],
            net_seed: 1,
            difficulty: None,
            faults: vec![],
            exits: vec![],
            host_freeze: None,
            profile_changes: vec![],
            migration: false,
        }
    }

    #[test]
    fn specs_that_differ_in_anything_have_different_stems() {
        let a = spec();
        let variants = [
            NetSpec { outages: vec![(1, 100, 200)], ..spec() },
            NetSpec { outages: vec![(1, 100, 300)], ..spec() },
            NetSpec { ticks: 601, ..spec() },
            NetSpec { drift_ppm: 5, ..spec() },
            NetSpec { join_tick: Some(5), ..spec() },
            NetSpec { difficulty: Some("hard".into()), ..spec() },
            NetSpec { net_seed: 2, ..spec() },
            NetSpec { exits: vec![(0, 300, "crash".into())], ..spec() },
            NetSpec { exits: vec![(0, 300, "leave".into())], ..spec() },
            NetSpec { host_freeze: Some((300, 470)), ..spec() },
            NetSpec { profile_changes: vec![(300, 1, "bad".into())], ..spec() },
            NetSpec { migration: true, ..spec() },
        ];
        let stems: std::collections::BTreeSet<_> = variants.iter().map(NetSpec::stem).collect();
        assert_eq!(stems.len(), variants.len());
        assert!(!stems.contains(&a.stem()));
        assert_eq!(a.stem(), spec().stem(), "the stem is stable");
    }

    /// The resume tick of [`migrated`]'s migration.
    const R: u32 = 420;

    /// Stall episodes `(peer, tick, ended)` judged against one migration
    /// resumed at tick [`R`], from a report alone.
    fn migrated(spec: &NetSpec, stalls: &[(PeerId, u32, Option<u32>)]) -> Result<(), Failure> {
        let mut r = empty_report();
        r.hosts = vec![HOST, 1];
        r.migrations.push(harness::Migrated { from: HOST, to: 1, silent_us: 0, resumed_us: 0, tick: R });
        let episodes: Vec<StallEpisode> =
            stalls.iter().map(|&(peer, tick, ended)| StallEpisode { peer, tick, ended }).collect();
        stalls_within_windows(spec, &r, &episodes)
    }

    /// Plan 12c Task 8's decision, narrowed by ruling RS-2: a stall episode
    /// the host's departure causes is excused when it begins from the exit
    /// (less `MAX_PREDICTION`) to the resume plus `MAX_PREDICTION` and ends
    /// by then too; any other, or a longer one, fails.
    #[test]
    fn only_stalls_within_a_migration_are_excused() {
        let s = NetSpec { exits: vec![(0, 400, "crash".into())], ..spec() };
        let end = R + MAX_PREDICTION;
        // The departure's episodes, ended by the resume's frames.
        let inside = [(1, 400 - MAX_PREDICTION, Some(R + 3)), (2, 410, Some(end)), (1, end, Some(end))];
        assert!(migrated(&s, &inside).is_ok());
        let early = migrated(&s, &[(2, 400 - MAX_PREDICTION - 1, Some(401))]).unwrap_err();
        assert_eq!(early.what, "player peer 2 stalled");
        let late = migrated(&s, &[(HOST, end + 1, Some(end + 2))]).unwrap_err();
        assert_eq!(late.tick, end + 1);
        // Begun inside the window but running past it.
        let long = migrated(&s, &[(1, 410, Some(end + 1))]).unwrap_err();
        assert!(long.what.contains("past its window"), "{}", long.what);
        assert!(migrated(&s, &[(1, 410, None)]).is_err(), "an episode that never ended");
        // The review's case: an unrelated 0.6 s outage just after the
        // resume stalls a client from R + 5 for 60 ticks of the host.
        assert!(migrated(&s, &[(2, R + 5, Some(R + 65))]).is_err());
        // A frozen host's migration counts from the freeze.
        let f = NetSpec { host_freeze: Some((300, 470)), ..spec() };
        assert!(migrated(&f, &[(1, 300 - MAX_PREDICTION, Some(R))]).is_ok());
        assert!(migrated(&f, &[(1, 300 - MAX_PREDICTION - 1, Some(R))]).is_err());
        // A spectator never stalls the verdict.
        assert!(migrated(&s, &[(FIRST_SPECTATOR, 10, None)]).is_ok());
    }

    /// Task 11's SK-1: an outage excuses the stalls of the peer it cuts off
    /// (everyone's while that peer hosts), from `MAX_PREDICTION` before it to
    /// its recovery after (SK-2: `Profile::outage_recovery_ticks` on the
    /// peer's slowest link, at least `MAX_PREDICTION`), and nothing else.
    #[test]
    fn only_stalls_within_an_outage_are_excused() {
        let (from, until) = (150, 270);
        let s = NetSpec { outages: vec![(1, from, until)], ..spec() };
        let judge = |s: &NetSpec, stalls: &[(PeerId, u32, Option<u32>)], migrated: bool| {
            let mut r = empty_report();
            if migrated {
                r.hosts = vec![HOST, 1];
                r.migrations.push(harness::Migrated {
                    from: HOST,
                    to: 1,
                    silent_us: 0,
                    resumed_us: 100 * TICK_US,
                    tick: 100,
                });
            }
            let e: Vec<StallEpisode> =
                stalls.iter().map(|&(peer, tick, ended)| StallEpisode { peer, tick, ended }).collect();
            stalls_within_windows(s, &r, &e)
        };
        // Typical, 120 ticks: two round trips of 10 + 1 ticks, 120 ticks
        // re-simulated 13 ticks a host tick: 32.
        let recovery = Profile::TYPICAL.outage_recovery_ticks(until - from, CLIENT_FRAME_US);
        assert_eq!(recovery, 32);
        let end = until + recovery;
        assert!(judge(&s, &[(1, from - MAX_PREDICTION, Some(end)), (1, 200, Some(210))], false).is_ok());
        // Before the window, past it, never ended, or another peer's.
        assert!(judge(&s, &[(1, from - MAX_PREDICTION - 1, Some(from))], false).is_err());
        assert!(judge(&s, &[(1, 200, Some(end + 1))], false).is_err());
        assert!(judge(&s, &[(1, 200, None)], false).is_err());
        assert_eq!(judge(&s, &[(2, 200, Some(210))], false).unwrap_err().what, "player peer 2 stalled");
        assert!(judge(&s, &[(HOST, 200, Some(210))], false).is_err(), "the host is not the outage's peer");
        // Peer 1 hosting after a migration: the outage cuts everyone off from it.
        assert!(judge(&s, &[(2, 200, Some(210)), (HOST, 200, Some(210))], true).is_ok());
        // The outage is in virtual time, the windows in host ticks: a host
        // that resumed at tick 100 at virtual tick 160 is 60 ticks behind it.
        let mut late = empty_report();
        late.migrations.push(harness::Migrated {
            from: HOST,
            to: 1,
            silent_us: 95 * TICK_US,
            resumed_us: 160 * TICK_US,
            tick: 100,
        });
        assert_eq!(host_tick_at(&s, &late, 200), 140);
        assert_eq!(host_tick_at(&s, &late, 90), 90, "before the departure: the clock");
        // SK-4's `typical-3s3c1v-seed2454907585`: the match stands at the
        // migration's tick until the resume, so an outage begun meanwhile
        // begins there (not 50 ticks on, as if the host had run).
        assert_eq!(host_tick_at(&s, &late, 150), 100, "during the migration: its tick");
        let cut = NetSpec { outages: vec![(1, 150, 190)], ..s.clone() };
        let rec = Profile::TYPICAL.outage_recovery_ticks(130 - 100, CLIENT_FRAME_US).max(MAX_PREDICTION);
        let e = [StallEpisode { peer: 1, tick: 110, ended: Some(130 + rec) }];
        assert!(stalls_within_windows(&cut, &late, &e).is_ok());
        let frozen = NetSpec { host_freeze: Some((100, 130)), ..spec() };
        assert_eq!(host_tick_at(&frozen, &empty_report(), 200), 170, "a freeze that did not migrate");
        // The slowest link the peer has in the run counts: bad from tick
        // 400 on (after it, SK-6 judges none of its stalls).
        let slow = NetSpec { profile_changes: vec![(400, 1, "bad".into())], ..s.clone() };
        let bad = Profile::BAD.outage_recovery_ticks(until - from, CLIENT_FRAME_US);
        assert!(judge(&slow, &[(1, 200, Some(until + bad))], false).is_ok());
        assert!(judge(&slow, &[(1, 200, Some(until + bad + 1))], false).is_err());
        // On bad links the host's fills for the peer cut off stall the
        // others too (`fills_stall`): theirs are excused alike; on typical
        // ones they are not (above).
        let bad = NetSpec { profile: "bad".into(), ..s.clone() };
        assert!(judge(&bad, &[(2, 200, Some(210))], false).is_ok());
        assert!(judge(&bad, &[(2, 100, Some(110))], false).is_err(), "outside the window");
        assert!(fills_stall(&Profile::BAD) && !fills_stall(&Profile::TYPICAL));
        // A peer that went (host lost) never ends its last episode: no
        // stall of the match's. One still in the match does.
        let mut lost = empty_report();
        lost.host_lost.insert(1);
        let gone =
            |r: &Report| stalls_within_windows(&s, r, &[StallEpisode { peer: 1, tick: 200, ended: None }]);
        assert!(gone(&lost).is_ok());
        assert!(gone(&empty_report()).is_err());
        // A spectator's outage excuses no player.
        let sp = NetSpec { outages: vec![(FIRST_SPECTATOR, from, until)], ..spec() };
        assert!(judge(&sp, &[(1, 200, Some(210))], false).is_err());
        // An outage of a peer that is not in the match excuses nothing.
        let ghost = NetSpec { outages: vec![(7, from, until)], ..spec() };
        assert!(judge(&ghost, &[(1, 200, Some(210))], false).is_err());
    }

    /// A peer's overlapping outages cut its link until the last is over
    /// (the network chains them): one window from the first's start to the
    /// recovery after the whole, not one per outage (net-soak's
    /// `typical-8s4c0v-seed2732616080`: 796-1169 and 970-1345, a stall from
    /// 819 to 1359 was in neither window alone).
    #[test]
    fn overlapping_outages_of_a_peer_excuse_as_one() {
        let s = NetSpec { outages: vec![(1, 300, 500), (1, 150, 400), (1, 600, 650)], ..spec() };
        let judge = |stalls: &[(PeerId, u32, Option<u32>)]| {
            let e: Vec<StallEpisode> =
                stalls.iter().map(|&(peer, tick, ended)| StallEpisode { peer, tick, ended }).collect();
            stalls_within_windows(&s, &empty_report(), &e)
        };
        let end = 500 + Profile::TYPICAL.outage_recovery_ticks(500 - 150, CLIENT_FRAME_US);
        assert!(judge(&[(1, 150 - MAX_PREDICTION, Some(end))]).is_ok());
        assert!(judge(&[(1, 160, Some(end + 1))]).is_err(), "past the whole's recovery");
        // One that begins after the whole's end is its own.
        let next = 650 + Profile::TYPICAL.outage_recovery_ticks(50, CLIENT_FRAME_US).max(MAX_PREDICTION);
        assert!(judge(&[(1, 600, Some(next))]).is_ok());
        assert!(judge(&[(1, 160, Some(next))]).is_err(), "two outages apart are two windows");
    }

    /// Ruling SK-6: a player whose link turns worse mid-match keeps the
    /// input delay chosen on the better one (fixed per match), so from the
    /// change on its stalls are not judged, as an `awful` change's are not
    /// (net-soak's `lan-10s6c1v-seed335636286`: lan to bad, `d` 0). Before
    /// the change, other players, and a change to a better link are judged
    /// as before.
    #[test]
    fn a_player_whose_link_worsens_is_not_judged_from_the_change() {
        let s = NetSpec { profile: "good".into(), profile_changes: vec![(300, 1, "bad".into())], ..spec() };
        let judge = |s: &NetSpec, stalls: &[(PeerId, u32, Option<u32>)]| {
            let e: Vec<StallEpisode> =
                stalls.iter().map(|&(peer, tick, ended)| StallEpisode { peer, tick, ended }).collect();
            stalls_within_windows(s, &empty_report(), &e)
        };
        assert!(judge(&s, &[(1, 300, Some(310)), (1, 500, None)]).is_ok());
        assert!(judge(&s, &[(1, 299, Some(301))]).is_err(), "before the change");
        assert!(judge(&s, &[(2, 310, Some(311))]).is_err(), "another player");
        let better =
            NetSpec { profile: "typical".into(), profile_changes: vec![(300, 1, "good".into())], ..spec() };
        assert!(judge(&better, &[(1, 310, Some(311))]).is_err(), "a better link");
        // Then back to a better one: still the worse change's player.
        let back =
            NetSpec { profile_changes: vec![(300, 1, "bad".into()), (400, 1, "good".into())], ..s.clone() };
        assert!(judge(&back, &[(1, 450, Some(451))]).is_ok());
        assert_eq!(worsened_from(&s, 1), Some(300));
        assert_eq!(worsened_from(&better, 1), None);
    }

    /// N66: on a typical link a migration of more than 3 s fails, unless
    /// something besides the host left (a successor waits for a departed
    /// peer up to its window).
    #[test]
    fn a_slow_migration_fails_n66() {
        let mut r = empty_report();
        r.hosts = vec![HOST, 1];
        let m = |took| harness::Migrated {
            from: HOST,
            to: 1,
            silent_us: 1_000_000,
            resumed_us: 1_000_000 + took,
            tick: 420,
        };
        let s = NetSpec { exits: vec![(0, 400, "crash".into())], ..spec() };
        r.migrations = vec![m(N66_US)];
        assert!(n66(&s, &r).is_ok());
        r.migrations = vec![m(N66_US + 1_000)];
        assert!(n66(&s, &r).unwrap_err().what.contains("N66"));
        let other = NetSpec { exits: vec![(0, 400, "crash".into()), (2, 300, "leave".into())], ..spec() };
        assert!(n66(&other, &r).is_ok());
        let bad = NetSpec { profile: "bad".into(), ..s.clone() };
        assert!(n66(&bad, &r).is_ok(), "bad links: the first successor's window");
        r.migrations = vec![m(WINDOW_US + 1_000)];
        assert!(n66(&bad, &r).is_err());
    }

    #[test]
    fn exits_and_profile_changes_are_checked() {
        let bad = |s: NetSpec| s.scenario().unwrap_err().to_string();
        assert!(bad(NetSpec { exits: vec![(0, 300, "explode".into())], ..spec() }).contains("explode"));
        assert!(bad(NetSpec { exits: vec![(4, 300, "crash".into())], ..spec() }).contains("peer 4"));
        assert!(bad(NetSpec { host_freeze: Some((300, 300)), ..spec() }).contains("freeze"));
        assert!(
            bad(NetSpec { profile_changes: vec![(300, 1, "dreadful".into())], ..spec() })
                .contains("dreadful")
        );
        assert!(bad(NetSpec { profile_changes: vec![(300, 0, "bad".into())], ..spec() }).contains("peer 0"));
        let sc = NetSpec { exits: vec![(0, 300, "leave".into()), (100, 200, "crash".into())], ..spec() }
            .scenario()
            .unwrap();
        assert!(sc.migration.is_some(), "an exit of the host turns migration on");
        assert_eq!(sc.exits.len(), 2);
        assert!(spec().scenario().unwrap().migration.is_none());
        assert!(
            NetSpec { exits: vec![(1, 300, "crash".into())], ..spec() }
                .scenario()
                .unwrap()
                .migration
                .is_none()
        );
        assert!(NetSpec { host_freeze: Some((300, 470)), ..spec() }.scenario().unwrap().migration.is_some());
        assert!(NetSpec { migration: true, ..spec() }.scenario().unwrap().migration.is_some());
        let awful = NetSpec { profile_changes: vec![(300, 1, "awful".into())], ..spec() };
        assert!(!awful.scenario().unwrap().expect_no_stalls);
    }
}
