//! Oracle scenarios timed in `olr-sim` (plan 8, the OLX comparison): each
//! frame's `World::tick`, then the work it did (projectiles, `rand()`
//! calls since the seed), to set beside OLX's frame times of the same
//! scenario (`tools/olx-oracle`'s `ORACLE_TIMING`). The trace record of a
//! frame is made outside the timed span.

use std::time::Instant;

use olr_content::Content;
use olr_oracle::{Scenario, Trace};
use serde::{Deserialize, Serialize};

/// One scenario's frames.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScenarioTiming {
    pub name: String,
    /// Frames 0..n: the tick (and a scenario bonus placed before it), ns.
    pub tick_ns: Vec<u64>,
    pub projectiles: Vec<u32>,
    pub rand_calls: Vec<u64>,
    /// Whether the simulation reproduced OLX's trace in every checked
    /// field; `None` when the trace holds only frame -1 (a timing trace).
    pub matches_olx: Option<bool>,
}

/// What `olr-cli scenario --time` writes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TimingReport {
    pub scenarios: Vec<ScenarioTiming>,
}

/// Plays `sc` from `expected`'s header and frame -1, timing each frame.
pub fn time_scenario(
    content: &Content,
    name: &str,
    sc: &Scenario,
    expected: &Trace,
) -> anyhow::Result<ScenarioTiming> {
    let t0: u64 = expected.header.options.get("T0").and_then(|t| t.parse().ok()).unwrap_or(0);
    let mut out = ScenarioTiming { name: name.into(), ..ScenarioTiming::default() };
    let mut last = Instant::now();
    olr_scenario::play(content, sc, expected, |world, f, _| {
        let ns = last.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        if f >= 0 {
            let frame = olr_scenario::record(world, f, t0);
            out.tick_ns.push(ns);
            out.projectiles.push(frame.p.len() as u32);
            out.rand_calls.push(frame.rc.unwrap_or(0));
        }
        last = Instant::now();
    })?;
    if expected.frames.len() > 1 {
        let actual = olr_scenario::replay(content, sc, expected)?;
        out.matches_olx = Some(
            olr_oracle::compare_fields(expected, &actual, &sc.tolerances, olr_scenario::CHECKED).is_none(),
        );
    }
    Ok(out)
}
