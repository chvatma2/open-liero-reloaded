//! Headless bot matches (spec §7.1, §7.3): a Deathmatch of bots on
//! imported content, played as fast as it goes, recorded, with the
//! invariants checked after every tick and a panic caught as a failure
//! whose replay reproduces it.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use anyhow::{Context as _, ensure};
use olr_bots::Bot;
use olr_content::{BuiltMatch, Content};
use olr_replay::{MAX_TICKS, ModeSpec, Recorder, Replay};
use olr_sim::{EndReason, EventKind, Keys, MAX_WORMS, MatchResult, MatchSetup, TickEvents, World};

use crate::assets::LevelAssets;
use crate::coverage::Coverage;
use crate::invariants;
use crate::spec::MatchSpec;

/// Test hooks.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Panic inside tick `n` (0-based), as a bug in the simulation would:
    /// the failure capture's test (`olr-cli match --fail-at`).
    pub fail_at: Option<u32>,
}

/// What failed in a match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// A bot's `think` or the world's `tick` panicked.
    Panic,
    /// An invariant broke after the tick (`invariants::check`).
    Invariant,
    /// A bot's `think` spent more work units than its budget.
    Budget,
    /// The recorder refused the tick.
    Record,
    /// The same match played again went differently.
    Determinism,
    /// The recorded replay did not reproduce the match.
    Replay,
}

/// A failed match: in tick `tick` (0-based; a replay played `--until tick`
/// shows the world before it).
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub tick: u32,
    pub kind: FailureKind,
    pub message: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} failure at tick {}: {}", self.kind, self.tick, self.message)
    }
}

/// How a match ended.
#[derive(Clone, Debug, PartialEq)]
pub enum End {
    /// It played every tick it was given.
    TickLimit,
    /// The mode ended it in tick `tick` (0-based).
    GameOver {
        tick: u32,
        result: MatchResult,
    },
    Failed(Failure),
}

impl fmt::Display for End {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            End::TickLimit => write!(f, "tick limit"),
            End::GameOver { tick, result } => {
                let why = match result.reason {
                    EndReason::KillLimit => "kill limit",
                    EndReason::TimeLimit => "time limit",
                    EndReason::LastStanding => "last worm standing",
                    EndReason::Other(_) => "the mode's reason",
                };
                write!(f, "game over by {why} in tick {tick}, winner worm {}", result.winner)
            }
            End::Failed(x) => write!(f, "failed at tick {}: {:?}: {}", x.tick, x.kind, x.message),
        }
    }
}

/// Timings and the bots' work.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Each tick's wall time (the bots' thinking and `World::tick`), in ns.
    pub tick_ns: Vec<u32>,
    /// The bots' part of it, in ns (`tick_ns - think_ns` is the world's).
    pub think_ns: Vec<u32>,
    /// The most units one `think` spent, all of them, and how many thinks.
    pub units_max: u32,
    pub units_total: u64,
    pub thinks: u64,
    /// The smallest budget of a bot.
    pub budget_min: u32,
}

impl Stats {
    /// Percentile `q` (0 to 1) of the tick times from tick `from` on, in µs.
    pub fn percentile_us(&self, from: usize, q: f64) -> f64 {
        let mut t = self.tick_ns.get(from..).unwrap_or(&[]).to_vec();
        if t.is_empty() {
            return 0.0;
        }
        t.sort_unstable();
        let i = ((t.len() as f64 * q).ceil() as usize).clamp(1, t.len()) - 1;
        f64::from(t[i]) / 1000.0
    }
}

/// A finished match.
#[derive(Clone, Debug)]
pub struct MatchRun {
    pub spec: MatchSpec,
    pub end: End,
    /// Ticks played (a failing tick included).
    pub ticks: u32,
    pub replay: Replay,
    pub stats: Stats,
    pub coverage: Coverage,
}

impl MatchRun {
    pub fn failure(&self) -> Option<&Failure> {
        match &self.end {
            End::Failed(f) => Some(f),
            _ => None,
        }
    }

    /// The 99th-percentile tick time, in µs.
    pub fn p99_us(&self) -> f64 {
        self.stats.percentile_us(0, 0.99)
    }

    /// One line: how it ended, the tick times and the bots' work.
    pub fn summary(&self) -> String {
        let s = &self.stats;
        let mean = s.units_total.checked_div(s.thinks).unwrap_or(0);
        format!(
            "{}: {} after {} ticks; tick (bots + sim) p50 {:.0} µs, p99 {:.0} µs, max {:.0} µs; \
             think units max {} of {}, mean {mean}; {} shots, {} kills",
            self.spec.stem(),
            self.end,
            self.ticks,
            s.percentile_us(0, 0.5),
            self.p99_us(),
            s.percentile_us(0, 1.0),
            s.units_max,
            s.budget_min,
            self.coverage.shots(),
            self.coverage.kills.values().sum::<u64>(),
        )
    }
}

/// Where a panic happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Bot(usize),
    Sim,
}

/// A match being played, a tick at a time.
pub struct Match {
    spec: MatchSpec,
    opts: Options,
    /// The setup `World::new` got and the mode, to rebuild the world when a
    /// panic left it broken.
    setup: MatchSetup,
    mode: ModeSpec,
    world: World,
    bots: Vec<Bot>,
    out: TickEvents,
    keys: Vec<Keys>,
    /// Every tick's keys, row by row.
    log: Vec<Keys>,
    rec: Recorder,
    ticks: u32,
    phase: Phase,
    over_budget: Option<(usize, u32, u32)>,
    stats: Stats,
    coverage: Coverage,
    end: Option<End>,
    /// The replay of a failed match, made when it failed.
    failed_replay: Option<Replay>,
}

impl Match {
    /// The match `spec` describes on `content`'s packs (or a generated
    /// level: [`LevelAssets::load`]), assembled by
    /// `olr_content::build_match` (as `olr-game` does): bots with OLX's
    /// loadouts (a chaos bot's random, rotating on each spawn), one shared
    /// area cache, thinking in worm order.
    pub fn new(content: &Content, spec: &MatchSpec, opts: &Options) -> anyhow::Result<Match> {
        let assets = LevelAssets::load(content, &spec.mod_id, &spec.level_id)?;
        Match::with_assets(&assets, spec, opts)
    }

    /// [`Match::new`] on assets already loaded (a stress run keeps them for
    /// its presentation; a generated level is made once).
    pub fn with_assets(assets: &LevelAssets, spec: &MatchSpec, opts: &Options) -> anyhow::Result<Match> {
        ensure!(spec.ticks <= MAX_TICKS, "at most {MAX_TICKS} ticks");
        ensure!((2..=MAX_WORMS).contains(&spec.bots), "a Deathmatch of 2 to {MAX_WORMS} bots");
        let built = olr_content::build_match(assets.match_assets(), &spec.match_options())?;
        let BuiltMatch { setup, mode, world, bots, recorder: rec } = built;
        let stats = Stats {
            tick_ns: Vec::with_capacity(spec.ticks as usize),
            think_ns: Vec::with_capacity(spec.ticks as usize),
            budget_min: bots.iter().map(Bot::budget).min().unwrap_or(0),
            ..Stats::default()
        };
        Ok(Match {
            spec: spec.clone(),
            opts: *opts,
            setup,
            mode,
            world,
            out: TickEvents::new(),
            keys: vec![Keys(0); bots.len()],
            log: Vec::with_capacity(spec.ticks as usize * bots.len()),
            bots,
            rec,
            ticks: 0,
            phase: Phase::Sim,
            over_budget: None,
            stats,
            coverage: Coverage::default(),
            end: None,
            failed_replay: None,
        })
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// Ticks played so far.
    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    /// The last tick's events.
    pub fn events(&self) -> &TickEvents {
        &self.out
    }

    /// The areas in the bots' shared cache.
    pub fn cache_len(&self) -> usize {
        self.bots.first().map_or(0, |b| b.cache().len())
    }

    /// The bots' shared area cache's capacity (0 without bots).
    pub fn cache_capacity(&self) -> usize {
        self.bots.first().map_or(0, |b| b.cache().capacity())
    }

    /// The 99th-percentile tick time from tick `from` on, in µs.
    pub fn p99_us_since(&self, from: usize) -> f64 {
        self.stats.percentile_us(from, 0.99)
    }

    /// The bots think (in worm order) and the world ticks. Allocates
    /// nothing; a panic propagates (see [`Match::step`]).
    pub fn advance(&mut self) {
        let start = Instant::now();
        for (i, b) in self.bots.iter_mut().enumerate() {
            self.phase = Phase::Bot(i);
            self.keys[i] = b.think(&self.world, &self.out);
            let (used, budget) = (b.units_used(), b.budget());
            self.stats.units_max = self.stats.units_max.max(used);
            self.stats.units_total += u64::from(used);
            self.stats.thinks += 1;
            if used > budget && self.over_budget.is_none() {
                self.over_budget = Some((i, used, budget));
            }
        }
        self.phase = Phase::Sim;
        let think = start.elapsed().as_nanos().min(u128::from(u32::MAX)) as u32;
        self.stats.think_ns.push(think);
        if self.opts.fail_at == Some(self.ticks) {
            panic!("injected failure (--fail-at {})", self.ticks);
        }
        self.world.tick(&self.keys, &mut self.out);
        let ns = start.elapsed().as_nanos().min(u128::from(u32::MAX)) as u32;
        self.stats.tick_ns.push(ns);
    }

    /// After [`Match::advance`]: records the tick, checks the invariants
    /// and counts the coverage.
    pub fn settle(&mut self) -> Result<(), Failure> {
        let tick = self.ticks;
        let fail = |kind, message| Failure { tick, kind, message };
        if let Some((bot, used, budget)) = self.over_budget.take() {
            return Err(fail(FailureKind::Budget, format!("bot {bot} spent {used} units of {budget}")));
        }
        self.rec.push(&self.keys, &self.world).map_err(|e| fail(FailureKind::Record, e.to_string()))?;
        self.log.extend_from_slice(&self.keys);
        self.ticks += 1;
        invariants::check(&self.world, &self.out).map_err(|m| fail(FailureKind::Invariant, m))?;
        self.coverage.observe(&self.world, &self.out);
        for e in &self.out {
            if let EventKind::GameOver(result) = e.kind {
                self.end = Some(End::GameOver { tick, result });
            }
        }
        Ok(())
    }

    /// Plays one tick, catching a panic as a failure; gives whether the
    /// match goes on.
    pub fn step(&mut self) -> bool {
        if self.end.is_none() && self.ticks >= self.spec.ticks {
            self.end = Some(End::TickLimit);
        }
        if self.end.is_some() {
            return false;
        }
        match catch_unwind(AssertUnwindSafe(|| self.advance())) {
            Ok(()) => {
                if let Err(f) = self.settle() {
                    if f.kind == FailureKind::Record {
                        self.failed_replay = Some(self.rebuilt_replay(true));
                    }
                    self.end = Some(End::Failed(f));
                }
            }
            Err(payload) => self.panicked(payload.as_ref()),
        }
        if self.end.is_none() && self.ticks >= self.spec.ticks {
            self.end = Some(End::TickLimit);
        }
        self.end.is_none()
    }

    fn panicked(&mut self, payload: &(dyn std::any::Any + Send)) {
        let msg = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic".into());
        let (message, replay) = match self.phase {
            // The world is untouched: a bot's keys are not replayed, the
            // match is (`olr-cli match` with the same spec).
            Phase::Bot(i) => (format!("bot {i} panicked: {msg}"), self.rec.clone().finish(&self.world).ok()),
            Phase::Sim => (format!("the tick panicked: {msg}"), Some(self.rebuilt_replay(true))),
        };
        self.failed_replay = replay;
        self.end = Some(End::Failed(Failure { tick: self.ticks, kind: FailureKind::Panic, message }));
    }

    /// The replay up to the last recorded tick, then (with `failing`) the
    /// tick being played, on a world rebuilt by replaying the recorded keys.
    fn rebuilt_replay(&self, failing: bool) -> Replay {
        let mut w = World::new(
            self.setup.clone(),
            self.mode.game_mode(&self.setup.settings).expect("the match's mode"),
        )
        .expect("the match's setup");
        let mut out = TickEvents::new();
        for row in self.log.chunks(self.keys.len().max(1)) {
            w.tick(row, &mut out);
        }
        let rec = self.rec.clone();
        let r = if failing { rec.finish_failed(&w, &self.keys) } else { rec.finish(&w) };
        r.expect("the rebuilt world is at the recorded tick")
    }

    /// The match as it ended (or as far as it was played).
    pub fn finish(mut self) -> MatchRun {
        self.coverage.add_actions(self.world.actions_triggered());
        let end = self.end.take().unwrap_or(End::TickLimit);
        let replay = match self.failed_replay.take() {
            Some(r) => r,
            None => self.rec.clone().finish(&self.world).unwrap_or_else(|_| self.rebuilt_replay(false)),
        };
        MatchRun {
            spec: self.spec,
            end,
            ticks: replay.ticks(),
            replay,
            stats: self.stats,
            coverage: self.coverage,
        }
    }
}

/// Plays the match `spec` describes to its end.
pub fn play(content: &Content, spec: &MatchSpec, opts: &Options) -> anyhow::Result<MatchRun> {
    let mut m = Match::new(content, spec, opts).with_context(|| format!("setting up {}", spec.stem()))?;
    while m.step() {}
    Ok(m.finish())
}
