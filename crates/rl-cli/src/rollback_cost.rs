//! The rollback cost (plan 9, networking spec §3.3, decision N1): what a
//! client pays when a remote input proves a prediction wrong, restoring the
//! world and re-simulating `depth` ticks with the keys already known. The
//! bots' thinking is not timed: online the bots think on the host only,
//! once, and a re-simulated tick takes the recorded keys.

use std::time::Instant;

use anyhow::bail;
use rl_content::{BuiltMatch, Content};
use rl_sim::{Keys, TickEvents};
use serde::{Deserialize, Serialize};

use crate::assets::LevelAssets;
use crate::stress::{self, Spread};

/// Ticks played with the bots before the first sample.
pub const WARM_TICKS: u32 = 500;
/// Ticks played with the bots between two samples.
pub const SAMPLE_EVERY: u32 = 10;
/// The budget at the reference configuration (16 worms, Castle Strike), µs.
pub const BUDGET_REFERENCE_US: f64 = 2000.0;
/// The budget at the 4096 × 4096 envelope point, µs.
pub const BUDGET_4096_US: f64 = 4000.0;

/// What `rl-cli rollback-cost` prints and writes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RollbackCost {
    pub level: String,
    pub worms: usize,
    pub depth: u32,
    pub restore_us_p50: f64,
    pub restore_us_p99: f64,
    pub resim_us_p50: f64,
    pub resim_us_p99: f64,
    /// Restore plus re-simulation of the same sample.
    pub total_us_p99: f64,
    /// The snapshot's non-terrain state: the dynamic state's raw encoding
    /// less one byte per terrain cell (the journal keeps the terrain).
    pub snapshot_bytes: usize,
    /// The most the terrain journal held after `depth` ticks.
    pub journal_bytes_max: usize,
}

impl RollbackCost {
    /// The report of `restore_us[i]` and `resim_us[i]`, sample by sample
    /// (nearest-rank percentiles).
    pub fn of(
        level: &str,
        worms: usize,
        depth: u32,
        restore_us: &[f64],
        resim_us: &[f64],
        snapshot_bytes: usize,
        journal_bytes_max: usize,
    ) -> RollbackCost {
        let total: Vec<f64> = restore_us.iter().zip(resim_us).map(|(a, b)| a + b).collect();
        let (restore, resim, total) = (Spread::of(restore_us), Spread::of(resim_us), Spread::of(&total));
        RollbackCost {
            level: level.into(),
            worms,
            depth,
            restore_us_p50: restore.p50,
            restore_us_p99: restore.p99,
            resim_us_p50: resim.p50,
            resim_us_p99: resim.p99,
            total_us_p99: total.p99,
            snapshot_bytes,
            journal_bytes_max,
        }
    }

    pub fn text(&self) -> String {
        format!(
            "rollback {} worms {} depth {}: restore p50 {:.0} p99 {:.0} us, re-simulation p50 {:.0} p99 {:.0} us, \
             total p99 {:.0} us; snapshot {} B, journal at most {} B\n",
            self.level,
            self.worms,
            self.depth,
            self.restore_us_p50,
            self.restore_us_p99,
            self.resim_us_p50,
            self.resim_us_p99,
            self.total_us_p99,
            self.snapshot_bytes,
            self.journal_bytes_max
        )
    }
}

/// Plays `mod_id` on `level_id` with `worms` chaos bots, then takes `samples`
/// samples: save, play `depth` ticks (the bots think), time the restore, time
/// the re-simulation of those ticks with their keys, and check the world is
/// where it was (its checksum).
pub fn measure(
    content: &Content,
    mod_id: &str,
    level_id: &str,
    worms: usize,
    depth: u32,
    samples: usize,
) -> anyhow::Result<RollbackCost> {
    if depth == 0 || samples == 0 {
        bail!("depth and samples must be at least 1");
    }
    let assets = LevelAssets::load(content, mod_id, level_id)?;
    let spec = stress::spec(mod_id, level_id, worms, 3000, 0.0, 1);
    let BuiltMatch { mut world, mut bots, .. } =
        rl_content::build_match(assets.match_assets(), &spec.match_options())?;
    world.enable_rollback();
    let n = bots.len();
    let (mut events, mut keys) = (TickEvents::new(), vec![Keys(0); n]);
    let mut play = |world: &mut rl_sim::World, events: &mut TickEvents, rows: Option<&mut Vec<Keys>>| {
        for (i, b) in bots.iter_mut().enumerate() {
            keys[i] = b.think(world, events);
        }
        world.tick(&keys, events);
        if let Some(rows) = rows {
            rows.extend_from_slice(&keys);
        }
    };
    for _ in 0..WARM_TICKS {
        play(&mut world, &mut events, None);
    }
    let cells = world.terrain().width() as usize * world.terrain().height() as usize;
    let snapshot_bytes = world.state().to_raw().len().saturating_sub(cells);
    let (mut restore_us, mut resim_us, mut journal_max) = (Vec::new(), Vec::new(), 0);
    let mut rows = Vec::with_capacity(depth as usize * n);
    for s in 0..samples {
        if s > 0 {
            for _ in 0..SAMPLE_EVERY {
                play(&mut world, &mut events, None);
            }
        }
        let snap = world.save();
        world.forget_before(&snap);
        rows.clear();
        for _ in 0..depth {
            play(&mut world, &mut events, Some(&mut rows));
        }
        journal_max = journal_max.max(world.journal_bytes());
        let expected = world.sync_checksum();
        let t0 = Instant::now();
        world.restore(&snap).map_err(|e| anyhow::anyhow!("restore: {e:?}"))?;
        let t1 = Instant::now();
        for row in rows.chunks(n) {
            world.tick(row, &mut events);
        }
        let t2 = Instant::now();
        restore_us.push((t1 - t0).as_secs_f64() * 1e6);
        resim_us.push((t2 - t1).as_secs_f64() * 1e6);
        if world.sync_checksum() != expected {
            bail!("sample {s}: the re-simulation did not reproduce the world (a determinism bug)");
        }
    }
    Ok(RollbackCost::of(level_id, n, depth, &restore_us, &resim_us, snapshot_bytes, journal_max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank_and_total_is_per_sample() {
        // 100 samples: restore i us, re-simulation 1000 - i us (total always 1000, except one outlier).
        let restore: Vec<f64> = (1..=100).map(f64::from).collect();
        let mut resim: Vec<f64> = (1..=100).map(|i| 1000.0 - f64::from(i)).collect();
        resim[99] = 5000.0;
        let r = RollbackCost::of("lvl", 16, 12, &restore, &resim, 123, 456);
        assert_eq!((r.restore_us_p50, r.restore_us_p99), (50.0, 99.0));
        assert_eq!(r.resim_us_p50, 950.0);
        assert_eq!(r.resim_us_p99, 999.0);
        // The outlier sample's total is 5100; p99 is the 99th of 100 totals: 1000.
        assert_eq!(r.total_us_p99, 1000.0);
        assert_eq!(
            (r.level.as_str(), r.worms, r.depth, r.snapshot_bytes, r.journal_bytes_max),
            ("lvl", 16, 12, 123, 456)
        );
        assert!(r.text().contains("total p99 1000 us"));
    }

    #[test]
    fn no_samples_is_a_zero_report() {
        let r = RollbackCost::of("l", 2, 1, &[], &[], 0, 0);
        assert_eq!(r.total_us_p99, 0.0);
    }
}
