//! Online matches in one process (networking spec §9.1, §9.4): a host, its
//! clients and spectators, each with its own `World` and its own bots, play
//! over the network model of `rl-netcode`'s harness; the verdict checks
//! that every peer confirmed the host's inputs and simulated the same match.
//!
//! Every peer builds the same world from the same options (every worm a
//! bot, so the loadouts are identical) and *runs* only the bots of its local
//! slots (spec §3.6: the host those no client holds, client `k` slot `k`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail, ensure};
use clap::ValueEnum as _;
use rl_bots::Bot;
use rl_content::{Content, MatchOptions};
use rl_netcode::harness::{self, Factory, Failure, HOST, Outage, PeerPlan, Report, Scenario};
use rl_netcode::netsim::{LinkStats, Profile};
use rl_netcode::{Driver, PeerId, Seat, Stats, WorldSim, input_delay};
use rl_sim::Keys;
use serde::{Deserialize, Serialize};

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
    pub outages: Vec<(u16, u32, u32)>,
    #[serde(default)]
    pub net_seed: u64,
    /// The bots' difficulty (`easy`, `medium`, `hard`, `xtreme`); `mixed` by default.
    #[serde(default)]
    pub difficulty: Option<String>,
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

    fn profile(&self) -> anyhow::Result<Profile> {
        Profile::by_name(&self.profile).with_context(|| format!("unknown network profile {:?}", self.profile))
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
            expect_no_stalls: matches!(self.profile.as_str(), "lan" | "good" | "typical" | "bad"),
            corrupt: Vec::new(),
        })
    }
}

/// Builds every peer's world and bots.
pub struct WorldFactory<'c> {
    content: &'c Content,
    options: MatchOptions,
    spec: NetSpec,
    /// The bots of a peer's world, from `sim` to `driver`.
    bots: BTreeMap<PeerId, Vec<Bot>>,
}

impl<'c> WorldFactory<'c> {
    /// Builds a world once to validate the content and the options.
    pub fn new(content: &'c Content, spec: &NetSpec) -> anyhow::Result<Self> {
        let options = spec.match_options()?;
        content.build_match(&options).with_context(|| format!("setting up {}", spec.stem()))?;
        Ok(WorldFactory { content, options, spec: spec.clone(), bots: BTreeMap::new() })
    }
}

impl Factory<WorldSim> for WorldFactory<'_> {
    fn sim(&mut self, peer: PeerId) -> WorldSim {
        // Validated by `new`: only a content directory changing under the
        // run could fail now.
        let built = self
            .content
            .build_match(&self.options)
            .unwrap_or_else(|e| panic!("building {}: {e:#}", self.spec.stem()));
        // Each build has its own area cache: bots of different peers never share one.
        self.bots.insert(peer, built.bots);
        WorldSim::new(built.world)
    }

    fn driver(&mut self, peer: PeerId, seat: Option<Seat>) -> Box<dyn Driver<WorldSim>> {
        // The harness makes a peer's sim before its driver.
        let mut bots = self.bots.remove(&peer).expect("a peer's sim is made before its driver");
        let clients = self.spec.clients;
        bots.retain(|b| {
            let slot = b.worm();
            match seat {
                None => slot >= usize::from(clients),
                Some(Seat::Player(s)) => slot == usize::from(s),
                Some(Seat::Spectator) => false,
            }
        });
        Box::new(BotDriver { bots })
    }
}

/// Presses the keys of a peer's local bots.
struct BotDriver {
    bots: Vec<Bot>,
}

impl Driver<WorldSim> for BotDriver {
    fn local_inputs(&mut self, sim: &WorldSim, _tick: u32, slots: &[u8], out: &mut [Keys]) {
        for (&slot, key) in slots.iter().zip(out.iter_mut()) {
            *key = match self.bots.iter_mut().find(|b| b.worm() == usize::from(slot)) {
                Some(bot) => bot.think(sim.world(), sim.events()),
                None => Keys(0),
            };
        }
    }

    fn simulated(&mut self, _sim: &WorldSim, _info: rl_netcode::TickInfo) {}
}

/// A finished run.
pub struct NetRun {
    pub spec: NetSpec,
    pub report: Report,
    pub verdict: Result<(), Failure>,
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
    }
}

/// Plays the match and checks it. A harness error is a failed verdict (with
/// an empty report); an error here is bad content or a bad spec.
pub fn run(content: &Content, spec: &NetSpec) -> anyhow::Result<NetRun> {
    let scenario = spec.scenario()?;
    let mut factory = WorldFactory::new(content, spec)?;
    let (report, verdict) = match harness::run(&scenario, &mut factory) {
        Ok(report) => {
            let verdict = harness::verdict(&scenario, &report, &mut factory);
            (report, verdict)
        }
        Err(f) => (empty_report(), Err(f)),
    };
    Ok(NetRun { spec: spec.clone(), report, verdict })
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
        ];
        let stems: std::collections::BTreeSet<_> = variants.iter().map(NetSpec::stem).collect();
        assert_eq!(stems.len(), variants.len());
        assert!(!stems.contains(&a.stem()));
        assert_eq!(a.stem(), spec().stem(), "the stem is stable");
    }
}
