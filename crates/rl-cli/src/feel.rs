//! The feel numbers (networking spec §9.2): what an online match feels like
//! on each network profile, measured from seeded `net-sim` runs in virtual
//! time, so they are the same on every machine. `cargo xtask net-feel`
//! gates them against `fixtures/net/feel.ron`.
//!
//! Every number covers the match itself: from the start until the host
//! reaches the spec's last tick (the settle ticks after it are left out).
//!
//! - Rollback depth: `to - from` of every client's `Event::Rollback`;
//!   nearest-rank percentiles over all clients.
//! - Correction: at the end of each client frame the displayed tick `D =
//!   current - 1` and every worm's position are remembered; when a later
//!   rollback re-simulates `D`, the largest distance between a remembered
//!   and the re-simulated position of a worm other than the client's own
//!   is a correction (when above 0). The p99 over all clients.
//! - Own corrections: `Stats::own_corrections` per client and minute of
//!   match; the worst client.
//! - Stalls: every player peer's (the host's too) stall episodes (a run of
//!   stalled advances counts once, `Stats::stalls`), to the end of the run.
//! - Bytes: what reached a client (down) and what it sent (up), each
//!   datagram counting `rl_proto::OVERHEAD` more, per second of match; the
//!   worst client.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context as _, anyhow};
use rl_content::Content;
use rl_netcode::harness::{self, Factory, HOST, Observer};
use rl_netcode::{Driver, Event, PeerId, Seat, Session, TICK_US, TickInfo, WorldSim};
use rl_sim::Keys;
use serde::{Deserialize, Serialize};

use crate::netsim::{NetSpec, WorldFactory};

/// The profiles measured, in order.
pub const PROFILES: [&str; 5] = ["lan", "good", "typical", "bad", "awful"];
/// Each profile is measured with these seeds (match and network).
pub const SEEDS: std::ops::RangeInclusive<u32> = 1..=2;
/// The note of a report.
pub const NOTE: &str = "Seeded, virtual-time feel numbers (plan 9); host-independent.";

/// One profile's numbers (the worse of its seeds).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileFeel {
    pub profile: String,
    /// Ticks, over all clients' rollbacks.
    pub rollback_p50: u32,
    pub rollback_p99: u32,
    /// Other worms' visible jump, px, over corrections.
    pub correction_px_p99: f32,
    /// Per client, the worst client.
    pub own_corrections_per_min: f32,
    /// Stall episodes, all players.
    pub stalls: u64,
    /// The worst client, with `OVERHEAD` per datagram.
    pub down_bytes_per_s: u32,
    pub up_bytes_per_s: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeelReport {
    pub note: String,
    pub profiles: Vec<ProfileFeel>,
    /// 32 worms, one client, `typical` ([`bandwidth_spec`]): the budget of
    /// spec §3.1.
    pub bandwidth32: ProfileFeel,
}

/// 16 worms, 7 clients, Classic on Castle Strike, 3000 ticks.
pub fn feel_spec(profile: &str, seed: u32) -> NetSpec {
    NetSpec {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-castlestrike".into(),
        seed,
        slots: 16,
        clients: 7,
        spectators: 0,
        join_tick: None,
        profile: profile.into(),
        drift_ppm: 0,
        ticks: 3_000,
        outages: vec![],
        net_seed: u64::from(seed),
        difficulty: None,
        faults: vec![],
    }
}

/// 32 worms, one client, `typical`, 3000 ticks: the most a client receives.
pub fn bandwidth_spec() -> NetSpec {
    NetSpec { slots: 32, clients: 1, ..feel_spec("typical", 1) }
}

/// The nearest-rank percentile `q` of sorted values (0 if none).
fn percentile<T: Copy + Default>(sorted: &[T], q: f64) -> T {
    if sorted.is_empty() {
        return T::default();
    }
    let rank = ((sorted.len() as f64 * q).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

/// What one client showed.
#[derive(Default)]
struct ClientFeel {
    /// Displayed ticks not yet re-simulated: every worm's position.
    shown: BTreeMap<u32, Vec<(f32, f32)>>,
    corrections: Vec<f32>,
    rollbacks: Vec<u32>,
    /// Its statistics when the match ended: own corrections, bytes in, bytes out.
    at_end: Option<(u64, u64, u64)>,
}

/// Shared by the observer and the clients' drivers.
struct Shared {
    /// The host has not reached the last tick yet.
    live: bool,
    clients: BTreeMap<PeerId, ClientFeel>,
}

type SharedRef = Rc<RefCell<Shared>>;

fn positions(sim: &WorldSim) -> Vec<(f32, f32)> {
    sim.world().worms().iter().map(|w| (w.pos.x, w.pos.y)).collect()
}

/// A client's driver that compares re-simulated displayed ticks.
struct FeelDriver {
    inner: Box<dyn Driver<WorldSim>>,
    peer: PeerId,
    /// The client's own worm, left out of its corrections.
    own: usize,
    shared: SharedRef,
}

impl Driver<WorldSim> for FeelDriver {
    fn local_inputs(&mut self, sim: &WorldSim, tick: u32, slots: &[u8], out: &mut [Keys]) {
        self.inner.local_inputs(sim, tick, slots, out);
    }

    fn simulated(&mut self, sim: &WorldSim, info: TickInfo) {
        self.inner.simulated(sim, info);
        if !info.resim {
            return;
        }
        let mut shared = self.shared.borrow_mut();
        let live = shared.live;
        let Some(c) = shared.clients.get_mut(&self.peer) else { return };
        let Some(shown) = c.shown.remove(&info.tick) else { return };
        let jump = sim
            .world()
            .worms()
            .iter()
            .zip(&shown)
            .enumerate()
            .filter(|(i, _)| *i != self.own)
            .map(|(_, (w, &(x, y)))| {
                let (dx, dy) = (w.pos.x - x, w.pos.y - y);
                (dx * dx + dy * dy).sqrt()
            })
            .fold(0.0_f32, f32::max);
        if live && jump > 0.0 {
            c.corrections.push(jump);
        }
    }
}

/// Wraps the world factory: clients' drivers become [`FeelDriver`]s.
struct FeelFactory<'c> {
    inner: WorldFactory<'c>,
    shared: SharedRef,
}

impl Factory<WorldSim> for FeelFactory<'_> {
    fn sim(&mut self, peer: PeerId) -> WorldSim {
        self.inner.sim(peer)
    }

    fn driver(&mut self, peer: PeerId, seat: Option<Seat>) -> Box<dyn Driver<WorldSim>> {
        let inner = self.inner.driver(peer, seat);
        match seat {
            Some(Seat::Player(slot)) => {
                self.shared.borrow_mut().clients.insert(peer, ClientFeel::default());
                Box::new(FeelDriver { inner, peer, own: usize::from(slot), shared: self.shared.clone() })
            }
            _ => inner,
        }
    }

    fn corrupt(&mut self, sim: &mut WorldSim, index: usize) {
        self.inner.corrupt(sim, index);
    }
}

/// Remembers displayed ticks and rollbacks; ends the match when the host
/// reaches its last tick.
struct FeelObserver {
    ticks: u32,
    shared: SharedRef,
}

impl Observer<WorldSim> for FeelObserver {
    fn on_event(&mut self, peer: PeerId, event: &Event, _session: &mut Session<WorldSim>) {
        let mut shared = self.shared.borrow_mut();
        let live = shared.live;
        if let (true, Event::Rollback { from, to }, Some(c)) = (live, event, shared.clients.get_mut(&peer)) {
            c.rollbacks.push(to - from);
        }
    }

    fn on_frame(&mut self, peer: PeerId, session: &mut Session<WorldSim>) {
        let mut shared = self.shared.borrow_mut();
        if peer == HOST {
            if shared.live && session.current_tick() >= self.ticks {
                shared.live = false;
            }
            return;
        }
        let live = shared.live;
        let Some(c) = shared.clients.get_mut(&peer) else { return };
        if !live {
            if c.at_end.is_none() {
                let s = session.stats();
                c.at_end = Some((s.own_corrections, s.bytes_in, s.bytes_out));
            }
            return;
        }
        // Confirmed ticks are never re-simulated.
        let frontier = session.frontier();
        while c.shown.first_key_value().is_some_and(|(&t, _)| t < frontier) {
            c.shown.pop_first();
        }
        if let Some(d) = session.current_tick().checked_sub(1) {
            c.shown.insert(d, positions(session.sim()));
        }
    }
}

/// One run's numbers. Fails on bad content, a bad spec or a failed verdict.
pub fn measure_one(content: &Content, spec: &NetSpec) -> anyhow::Result<ProfileFeel> {
    let scenario = spec.scenario()?;
    let shared: SharedRef = Rc::new(RefCell::new(Shared { live: true, clients: BTreeMap::new() }));
    let mut factory = FeelFactory { inner: WorldFactory::new(content, spec)?, shared: shared.clone() };
    let mut obs = FeelObserver { ticks: spec.ticks, shared: shared.clone() };
    let report = harness::run_observed(&scenario, &mut factory, &mut obs)
        .map_err(|f| anyhow!("{} failed at tick {}: {}", spec.stem(), f.tick, f.what))?;
    harness::verdict(&scenario, &report, &mut factory)
        .map_err(|f| anyhow!("{} failed at tick {}: {}", spec.stem(), f.tick, f.what))?;

    let shared = shared.borrow();
    let secs = (u64::from(spec.ticks) * TICK_US) as f64 / 1e6;
    let minutes = secs / 60.0;
    let mut rollbacks: Vec<u32> = shared.clients.values().flat_map(|c| c.rollbacks.iter().copied()).collect();
    rollbacks.sort_unstable();
    let mut corrections: Vec<f32> =
        shared.clients.values().flat_map(|c| c.corrections.iter().copied()).collect();
    corrections.sort_by(f32::total_cmp);
    let mut ends = Vec::new();
    for (peer, c) in &shared.clients {
        ends.push(c.at_end.with_context(|| format!("client {peer} saw no frame after the match"))?);
    }
    let worst = |f: fn(&(u64, u64, u64)) -> u64| ends.iter().map(f).max().unwrap_or(0);
    let players = std::iter::once(HOST).chain(shared.clients.keys().copied());
    let stalls = players.filter_map(|p| report.stats.get(&p)).map(|s| s.stalls).sum();
    Ok(ProfileFeel {
        profile: spec.profile.clone(),
        rollback_p50: percentile(&rollbacks, 0.5),
        rollback_p99: percentile(&rollbacks, 0.99),
        correction_px_p99: percentile(&corrections, 0.99),
        own_corrections_per_min: (worst(|e| e.0) as f64 / minutes) as f32,
        stalls,
        down_bytes_per_s: (worst(|e| e.1) as f64 / secs).round() as u32,
        up_bytes_per_s: (worst(|e| e.2) as f64 / secs).round() as u32,
    })
}

/// Per metric, the worse of `a` and `b`.
fn worse(a: ProfileFeel, b: &ProfileFeel) -> ProfileFeel {
    ProfileFeel {
        profile: a.profile,
        rollback_p50: a.rollback_p50.max(b.rollback_p50),
        rollback_p99: a.rollback_p99.max(b.rollback_p99),
        correction_px_p99: a.correction_px_p99.max(b.correction_px_p99),
        own_corrections_per_min: a.own_corrections_per_min.max(b.own_corrections_per_min),
        stalls: a.stalls.max(b.stalls),
        down_bytes_per_s: a.down_bytes_per_s.max(b.down_bytes_per_s),
        up_bytes_per_s: a.up_bytes_per_s.max(b.up_bytes_per_s),
    }
}

/// Every profile ([`PROFILES`] × [`SEEDS`], the worse of the seeds per
/// metric) and [`bandwidth_spec`], the runs in parallel threads.
pub fn measure(content: &Content) -> anyhow::Result<FeelReport> {
    let mut specs: Vec<NetSpec> =
        PROFILES.iter().flat_map(|p| SEEDS.map(move |seed| feel_spec(p, seed))).collect();
    specs.push(bandwidth_spec());
    let next = AtomicUsize::new(0);
    let results: Mutex<BTreeMap<usize, anyhow::Result<ProfileFeel>>> = Mutex::new(BTreeMap::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(specs.len());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(spec) = specs.get(i) else { break };
                    let r = measure_one(content, spec);
                    results.lock().unwrap_or_else(|e| e.into_inner()).insert(i, r);
                }
            });
        }
    });
    let results = results.into_inner().unwrap_or_else(|e| e.into_inner());
    let mut by_profile: BTreeMap<usize, ProfileFeel> = BTreeMap::new();
    let mut bandwidth32 = None;
    for (i, r) in results {
        let f = r?;
        if i == specs.len() - 1 {
            bandwidth32 = Some(f);
            continue;
        }
        let p = PROFILES.iter().position(|&p| p == f.profile).context("a measured profile")?;
        let merged = match by_profile.remove(&p) {
            Some(prev) => worse(prev, &f),
            None => f,
        };
        by_profile.insert(p, merged);
    }
    Ok(FeelReport {
        note: NOTE.into(),
        profiles: by_profile.into_values().collect(),
        bandwidth32: bandwidth32.context("the bandwidth run")?,
    })
}

impl ProfileFeel {
    /// One line: `<profile>: rollback p50 3 p99 12, ...`.
    pub fn line(&self) -> String {
        format!(
            "{}: rollback p50 {} p99 {}, correction p99 {:.1} px, own corrections {:.2}/min, \
             stalls {}, down {} B/s, up {} B/s",
            self.profile,
            self.rollback_p50,
            self.rollback_p99,
            self.correction_px_p99,
            self.own_corrections_per_min,
            self.stalls,
            self.down_bytes_per_s,
            self.up_bytes_per_s
        )
    }
}
