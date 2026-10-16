//! Who presses the keys of a session's worms: the player alone (the
//! sandbox), the player and bots with a recording (a Deathmatch), or a
//! replay's recorded keys.

use std::path::{Path, PathBuf};
use std::time::Instant;

use olr_bots::Bot;
use olr_content::BuiltMatch;
use olr_replay::{Cursor, MAX_TICKS, Mismatch, Recorder, Replay};
use olr_sim::{Keys, TickEvents, World};

/// The recording goes on this long after the match ended (ticks; OLX's
/// `GAMEOVER_WAIT`, 3 s, after which the physics stops), then it is
/// finished: the replay holds the match and its last explosions.
pub const RECORD_AFTER_END: u64 = 300;

/// Per-tick bot think times kept for the percentiles (an hour at 100 Hz);
/// later ticks count in the maximum only.
const KEPT_THINKS: usize = 360_000;

/// The bots of a Deathmatch, their timing, and the match's recording.
pub struct MatchDriver {
    /// In worm order (`Bot::worm`), sharing one area cache: they think in
    /// this order, once a tick, before the world ticks.
    pub bots: Vec<Bot>,
    recorder: Option<Recorder>,
    /// The recording once finished.
    replay: Option<Replay>,
    /// Every tick's think time of all the bots together, in ns.
    pub think_ns: Vec<u32>,
    pub think_ns_max: u32,
    /// The most work units one `think` spent, and the smallest budget.
    pub units_max: u32,
    pub budget_min: u32,
    /// Thinks that spent more units than their budget (never, by design).
    pub over_budget: u64,
    warned: bool,
}

impl MatchDriver {
    /// The driver of `built`'s match, and its world.
    pub fn new(built: BuiltMatch) -> (World, MatchDriver) {
        let BuiltMatch { world, bots, recorder, .. } = built;
        let budget_min = bots.iter().map(Bot::budget).min().unwrap_or(0);
        let d = MatchDriver {
            bots,
            recorder: Some(recorder),
            replay: None,
            think_ns: Vec::with_capacity(KEPT_THINKS.min(60_000)),
            think_ns_max: 0,
            units_max: 0,
            budget_min,
            over_budget: 0,
            warned: false,
        };
        (world, d)
    }

    /// Before `world` ticks: every bot thinks (in worm order) on the world
    /// and the last tick's `events`, writing its worm's keys; the time and
    /// work are measured. The recording is finished first when it is full
    /// or `ended` (ticks since the match ended) reached [`RECORD_AFTER_END`].
    pub fn think(&mut self, world: &World, events: &TickEvents, keys: &mut [Keys], ended: Option<u64>) {
        let full = self.recorder.as_ref().is_some_and(|r| r.ticks() >= MAX_TICKS);
        if full || ended.is_some_and(|t| t >= RECORD_AFTER_END) {
            self.finish(world);
        }
        let start = Instant::now();
        for b in &mut self.bots {
            let k = b.think(world, events);
            if let Some(slot) = keys.get_mut(b.worm()) {
                *slot = k;
            }
            self.units_max = self.units_max.max(b.units_used());
            if b.units_used() > b.budget() {
                self.over_budget += 1;
            }
        }
        let ns = start.elapsed().as_nanos().min(u128::from(u32::MAX)) as u32;
        self.think_ns_max = self.think_ns_max.max(ns);
        if self.think_ns.len() < KEPT_THINKS {
            self.think_ns.push(ns);
        }
    }

    /// After the tick: records `keys` (as the world got them).
    pub fn record(&mut self, keys: &[Keys], world: &World) {
        let Some(rec) = &mut self.recorder else { return };
        if let Err(e) = rec.push(keys, world) {
            if !self.warned {
                eprintln!("warning: the recording stopped: {e}");
                self.warned = true;
            }
            self.recorder = None;
        }
    }

    /// Finishes the recording at `world` (the world after the last tick
    /// recorded), once.
    fn finish(&mut self, world: &World) {
        let Some(rec) = self.recorder.take() else { return };
        match rec.finish(world) {
            Ok(r) => self.replay = Some(r),
            Err(e) => eprintln!("warning: the recording could not be finished: {e}"),
        }
    }

    /// The match's replay, finished at `world` if it was still recording,
    /// left in place.
    pub fn replay(&mut self, world: &World) -> Option<&Replay> {
        self.finish(world);
        self.replay.as_ref()
    }

    /// The match's replay, finished at `world` if it was still recording.
    pub fn take_replay(&mut self, world: &World) -> Option<Replay> {
        self.finish(world);
        self.replay.take()
    }

    /// Percentile `q` (0 to 1) of the per-tick think times, in ms.
    pub fn think_ms(&self, q: f64) -> f64 {
        percentile_ms(&self.think_ns, q)
    }

    /// The same over the last 1000 ticks (the F2 panel, every frame).
    pub fn recent_think_ms(&self, q: f64) -> f64 {
        percentile_ms(&self.think_ns[self.think_ns.len().saturating_sub(1000)..], q)
    }
}

/// Percentile `q` (0 to 1, nearest rank) of `ns`, in ms; 0 when empty.
pub fn percentile_ms(ns: &[u32], q: f64) -> f64 {
    if ns.is_empty() {
        return 0.0;
    }
    let mut t = ns.to_vec();
    t.sort_unstable();
    let i = ((t.len() as f64 * q).ceil() as usize).clamp(1, t.len()) - 1;
    f64::from(t[i]) / 1e6
}

/// A replay played into the world, its checksums checked as it goes.
pub struct ReplayDriver {
    pub replay: Box<Replay>,
    pub cursor: Cursor,
    /// The first recorded checksum the world did not reproduce.
    pub mismatch: Option<Mismatch>,
}

impl ReplayDriver {
    pub fn new(replay: Box<Replay>) -> ReplayDriver {
        ReplayDriver { replay, cursor: Cursor::default(), mismatch: None }
    }

    /// Plays the next recorded tick on `world`; false at the end (the
    /// world is left as it is). A mismatch is kept (the first) and logged
    /// once; the replay plays on.
    pub fn step(&mut self, world: &mut World, events: &mut TickEvents) -> bool {
        let before = self.cursor.tick();
        match self.cursor.step(&self.replay, world, events) {
            Ok(played) => played,
            Err(m) => {
                if self.mismatch.is_none() {
                    eprintln!("warning: the replay diverged: {m}");
                    self.mismatch = Some(m);
                }
                // Tick 0's checksum is checked before the first tick.
                self.cursor.tick() > before
            }
        }
    }

    pub fn is_done(&self) -> bool {
        self.cursor.is_done(&self.replay)
    }
}

/// Where match `n` (0 the first, then the rematches) of a run recorded to
/// `base` is saved: `base`, then `<stem>-2.<ext>`, `<stem>-3.<ext>`, ...
pub fn record_path(base: &Path, n: u32) -> PathBuf {
    if n == 0 {
        return base.to_path_buf();
    }
    let stem = base.file_stem().map_or("replay".into(), |s| s.to_string_lossy().into_owned());
    let name = match base.extension() {
        Some(ext) => format!("{stem}-{}.{}", n + 1, ext.to_string_lossy()),
        None => format!("{stem}-{}", n + 1),
    };
    base.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_by_nearest_rank() {
        let ns: Vec<u32> = (1..=100).map(|i| i * 1_000_000).collect();
        assert_eq!(percentile_ms(&ns, 0.5), 50.0);
        assert_eq!(percentile_ms(&ns, 0.99), 99.0);
        assert_eq!(percentile_ms(&ns, 1.0), 100.0);
        assert_eq!(percentile_ms(&[], 0.5), 0.0);
    }

    #[test]
    fn rematches_record_next_to_the_first() {
        let base = Path::new("out/dm.rlrp");
        assert_eq!(record_path(base, 0), PathBuf::from("out/dm.rlrp"));
        assert_eq!(record_path(base, 1), PathBuf::from("out/dm-2.rlrp"));
        assert_eq!(record_path(Path::new("dm"), 2), PathBuf::from("dm-3"));
    }
}
