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
use rl_netcode::{Driver, Event, MAX_PREDICTION, PeerId, Seat, Session, Stats, WorldSim, input_delay};
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
    /// link changes to `awful` mid-run or an outage cuts a peer off (an
    /// outage of 0.28 s, `MAX_PREDICTION` ticks, or more stalls its peer
    /// whatever the profile: found by `net-soak`, which draws outages of
    /// 0.5 s and more).
    fn expect_no_stalls(&self) -> bool {
        matches!(self.profile.as_str(), "lan" | "good" | "typical" | "bad")
            && !self.profile_changes.iter().any(|c| c.2 == "awful")
            && self.outages.is_empty()
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
                .map(|&(peer, from_tick, until_tick)| Outage { peer, from_tick, until_tick })
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
        exited: Default::default(),
        late: Default::default(),
        held: Vec::new(),
        left_from: BTreeMap::new(),
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

/// The run's verdict: `harness::verdict`, and with host migration on a
/// rule of its own for stalls (plan 12c Task 8's decision, ruling RS-2). A
/// host gone stalls its players at the prediction limit before they find
/// it gone, and that episode lasts until the new host's resume reaches
/// them: the harness's `expect_no_stalls` would flag it. So a stall
/// episode is excused only when it begins between its migration's
/// departure (the old host's exit or freeze) less `MAX_PREDICTION` and the
/// resume tick plus `MAX_PREDICTION`, and ends (the player runs a tick
/// again) by the current host's tick resume plus `MAX_PREDICTION`; any
/// other stall, or one lasting longer, fails, and the migration's length
/// is bounded instead ([`N66_US`]).
fn judge(
    spec: &NetSpec,
    sc: &Scenario,
    r: &Report,
    episodes: &[StallEpisode],
    f: &mut WorldFactory<'_>,
) -> Result<(), Failure> {
    if sc.migration.is_none() || !sc.expect_no_stalls {
        return harness::verdict(sc, r, f);
    }
    harness::verdict(&Scenario { expect_no_stalls: false, ..sc.clone() }, r, f)?;
    stalls_within_migrations(spec, r, episodes)?;
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

/// Every player's stall episode (the host's and the clients', not
/// dropped) begins and ends within a migration's window
/// ([`migration_windows`]).
fn stalls_within_migrations(spec: &NetSpec, r: &Report, episodes: &[StallEpisode]) -> Result<(), Failure> {
    let windows = migration_windows(spec, r);
    let player = |p: PeerId| p == HOST || (1..=PeerId::from(spec.clients)).contains(&p);
    for e in episodes.iter().filter(|e| player(e.peer) && !r.dropped.contains(&e.peer)) {
        let Some(w) = windows.iter().find(|w| w.contains(&e.tick)) else {
            return Err(Failure { tick: e.tick, what: format!("player peer {} stalled", e.peer) });
        };
        if e.ended.is_none_or(|end| end > *w.end()) {
            let until = e.ended.map_or_else(|| "the run's end".into(), |t| format!("host tick {t}"));
            return Err(Failure {
                tick: e.tick,
                what: format!(
                    "player peer {} stalled until {until}, past its migration's resume plus MAX_PREDICTION ({})",
                    e.peer,
                    w.end()
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
    /// The furthest tick a host (old or new) reached so far.
    clock: u32,
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
    fn on_event(&mut self, peer: PeerId, event: &Event, _session: &mut Session<WorldSim>) {
        if let Event::Stalled { tick } = *event {
            if let Some(i) = self.open.remove(&peer) {
                self.episodes[i].ended = Some(self.clock);
            }
            self.open.insert(peer, self.episodes.len());
            self.episodes.push(StallEpisode { peer, tick, ended: None });
        }
    }

    fn on_frame(&mut self, peer: PeerId, session: &mut Session<WorldSim>) {
        if session.is_host() {
            self.clock = self.clock.max(session.current_tick());
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
        stalls_within_migrations(spec, &r, &episodes)
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
        assert!(long.what.contains("past its migration's resume"), "{}", long.what);
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
