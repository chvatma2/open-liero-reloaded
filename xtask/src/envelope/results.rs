//! The performance envelope's results (plan 8, spec §3 and §7): the RON
//! `olr-cli stress`, `gpu-bench`, `scenario --time` and `olr-game
//! --frame-log` write (read here as `*Out`), what `cargo xtask envelope`
//! keeps of them (`Envelope`, the file `fixtures/bench/envelope/<host>.ron`)
//! and the pure arithmetic: medians of runs, budgets, breaking points,
//! noise, the comparison with the committed file, merging a bless.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bench::{Host, median};
use crate::olx_timing::OlxFrame;

/// Where the blessed results go.
pub const DIR: &str = "fixtures/bench/envelope";
/// Every run's results (not committed).
pub const LATEST: &str = "target/envelope/latest.ron";
/// The least noise an axis is given (run-to-run spread below it is luck):
/// 5 %, under the smallest noise an axis or GPU path had in the first
/// bless (6 %, 2026-10-12), and the floor `compare` uses for a stored
/// file without noise.
pub const NOISE_FLOOR: f64 = 0.05;

/// Percentiles of a measurement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spread {
    pub p50: f64,
    pub p99: f64,
    pub max: f64,
}

impl Spread {
    /// Of `values` (any order), nearest rank: the value at rank
    /// `ceil(n * q)` (at least 1). The one definition of every percentile
    /// in plan 8 (ruling PF8): `olr_cli::stress::Spread::of` and `olr-game`'s
    /// `stats::percentile` agree (`xtask` does not link `olr-cli`).
    pub fn of(values: &[f64]) -> Spread {
        if values.is_empty() {
            return Spread::default();
        }
        let mut v = values.to_vec();
        v.sort_by(f64::total_cmp);
        let at = |q: f64| v[((v.len() as f64 * q).ceil() as usize).clamp(1, v.len()) - 1];
        Spread { p50: at(0.5), p99: at(0.99), max: at(1.0) }
    }

    /// Each percentile's median over `runs`.
    pub fn median_of(runs: &[Spread]) -> Spread {
        let m = |f: fn(&Spread) -> f64| median(&runs.iter().map(f).collect::<Vec<_>>());
        Spread { p50: m(|s| s.p50), p99: m(|s| s.p99), max: m(|s| s.max) }
    }
}

/// Spec §3's budgets.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Budgets {
    /// OLX's fixed 100 Hz frame (`FRAME_MS`).
    pub tick_ms: f64,
    pub frame_144_ms: f64,
    pub frame_60_ms: f64,
    pub memory_mib: f64,
}

impl Default for Budgets {
    fn default() -> Budgets {
        Budgets {
            tick_ms: 10.0,
            frame_144_ms: 1000.0 / 144.0,
            frame_60_ms: 1000.0 / 60.0,
            memory_mib: 2048.0,
        }
    }
}

// ---- What the tools write (only the fields read here). No field defaults:
// a field renamed in the tool is an error (a failed point saying why), never
// a measured 0; `tool_outputs_parse` ties these to the tools' own output.
// Only the kept results below are lenient (older files still read).

/// `olr-cli stress --out` (`olr_cli::stress::Report`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct StressOut {
    pub tick_us: Spread,
    pub sim_us: Spread,
    pub think_us: Spread,
    pub present_us: Spread,
    pub build_us: Spread,
    pub pool_capacity: usize,
    pub projectiles_peak: usize,
    pub ticks_at_pool_cap: u32,
    pub fx_peak: usize,
    pub ticks_at_fx_cap: u32,
    pub fx_capacity: usize,
    pub events_max: usize,
    pub peak_mib: Option<f64>,
    pub area_cache: (usize, usize),
    pub caps: Vec<String>,
}

/// `olr-cli rollback-cost --out` (`olr_cli::rollback_cost::RollbackCost`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RollbackOut {
    pub level: String,
    pub worms: usize,
    pub depth: u32,
    pub restore_us_p50: f64,
    pub restore_us_p99: f64,
    pub resim_us_p50: f64,
    pub resim_us_p99: f64,
    pub total_us_p99: f64,
    pub snapshot_bytes: usize,
    pub journal_bytes_max: usize,
}

/// `olr-cli gpu-bench --out` (`olr_cli::gpu_bench::GpuReport`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GpuReportOut {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    pub timestamps: bool,
    pub sizes: Vec<SizeOut>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SizeOut {
    pub size: (u32, u32),
    pub tick_ms: Spread,
    pub cpu_ms: Spread,
    pub frame_ms: Spread,
    pub total_ms: Spread,
    pub gpu_ms: Option<Spread>,
    /// Frames whose timestamps gave no real span (left out of `gpu_ms`).
    pub gpu_dropped: u32,
    pub probe: Vec<ProbeOut>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProbeOut {
    pub passes: u32,
    pub ms: Spread,
    /// Probe frames whose timestamps gave no real span (left out of `ms`).
    pub gpu_dropped: u32,
}

/// `olr-game --frame-log` (`olr_game::stats::FrameLog`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct FrameLogOut {
    pub size: (u32, u32),
    pub gpu: String,
    pub frames: usize,
    pub fps: f64,
    pub frame_ms: Spread,
    pub busy_ms: Spread,
}

/// `olr-cli scenario --time` (`olr_cli::timing`).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ScenarioTimingOut {
    pub name: String,
    pub tick_ns: Vec<u64>,
    pub projectiles: Vec<u32>,
    pub rand_calls: Vec<u64>,
    pub matches_olx: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TimingReportOut {
    pub scenarios: Vec<ScenarioTimingOut>,
}

// ---- What is kept.

/// A frame measured for a sweep point (its level drawn, plan 8 ruling P4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrameCheck {
    pub path: String,
    pub size: (u32, u32),
    pub total_ms: Spread,
    pub gpu_ms: Option<Spread>,
}

/// One value of an axis: the medians of its runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Point {
    pub value: String,
    /// The value as a number (for the fit).
    pub n: f64,
    pub runs: usize,
    pub tick_us: Spread,
    pub sim_us: Spread,
    pub think_us: Spread,
    pub present_us: Spread,
    pub build_us: Spread,
    pub projectiles_peak: usize,
    pub fx_peak: usize,
    pub events_max: usize,
    pub peak_mib: f64,
    pub area_cache: (usize, usize),
    /// Hard limits reached (each breaks the point, ruling P7).
    pub caps: Vec<String>,
    /// The pool or effects cap full at its own size (shown, not a break).
    pub saturated: Vec<String>,
    pub load_avg: f64,
    /// The runs' tick p99 spread: (max − min) / median.
    pub spread: f64,
    /// Tick p99 over the tick budget.
    pub budget_share: f64,
    pub frame: Option<FrameCheck>,
}

/// The first value that does not hold, and why.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Breaking {
    pub value: String,
    pub causes: Vec<String>,
}

/// Tick time against the axis's value, least squares over measured points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fit {
    pub per_unit_us_p50: f64,
    pub per_unit_us_p99: f64,
    pub intercept_us_p99: f64,
    /// The value where the tick p99 would reach the budget.
    pub limit: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sweep {
    pub axis: String,
    pub mod_id: String,
    pub level: String,
    /// What varies and what stays the reference.
    pub what: String,
    /// The largest point spread (at least [`NOISE_FLOOR`]): the comparison's tolerance.
    pub noise: f64,
    pub points: Vec<Point>,
    /// With the 144 Hz frame budget (ruling P4).
    pub breaking_144: Option<Breaking>,
    pub breaking_60: Option<Breaking>,
    pub fit: Option<Fit>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProbeRow {
    pub passes: u32,
    pub ms_p50: f64,
}

/// One resolution of a GPU path.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuSize {
    pub size: (u32, u32),
    pub tick_ms: Spread,
    pub cpu_ms: Spread,
    pub frame_ms: Spread,
    pub total_ms: Spread,
    pub gpu_ms: Option<Spread>,
    /// Frames, over every run, whose timestamps gave no real span (a
    /// driver's fault): left out of `gpu_ms`, not counted as 0 ms.
    pub gpu_dropped: u32,
    /// The same for the probe's frames (left out of its times).
    pub probe_dropped: u32,
    pub spread: f64,
    pub probe: Vec<ProbeRow>,
    /// GPU ms one full-screen probe pass costs (slope through the origin).
    pub ms_per_pass: f64,
    /// Passes that fit beside the frame (its GPU p99, else its frame p99).
    pub passes_144: u32,
    pub passes_60: u32,
}

/// A windowed, uncapped `olr-game` run (one run, ruling P5).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Windowed {
    pub requested: (u32, u32),
    pub size: (u32, u32),
    /// The window got the requested size (else the monitor is smaller).
    pub reached: bool,
    /// The adapter, backend and present mode (`Fifo`: the run was not uncapped).
    pub gpu: String,
    pub frames: usize,
    pub fps: f64,
    pub frame_ms: Spread,
    pub busy_ms: Spread,
}

/// A sweep point's level drawn.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuLevel {
    pub axis: String,
    pub value: String,
    pub size: (u32, u32),
    pub total_ms: Spread,
    pub gpu_ms: Option<Spread>,
    /// Frames whose timestamps gave no real span (left out of `gpu_ms`).
    pub gpu_dropped: u32,
}

/// One GPU path: `wsl-gl`, `windows-dx12`, `windows-vulkan`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuPath {
    pub adapter: String,
    pub backend: String,
    pub device_type: String,
    /// GPU times are timestamps; else CPU-side frame times only.
    pub timestamps: bool,
    pub noise: f64,
    pub sizes: Vec<GpuSize>,
    pub windowed: Vec<Windowed>,
    pub levels: Vec<GpuLevel>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OlxRow {
    pub name: String,
    pub frames: usize,
    pub projectiles_peak: u32,
    pub olx_us: Spread,
    pub olr_us: Spread,
    /// olr-sim's p50 over OLX's.
    pub ratio_p50: f64,
    /// The same projectiles and `rand()` calls every frame.
    pub same_work: bool,
    pub matches_olx: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OlxComparison {
    pub image: String,
    pub build: String,
    pub rows: Vec<OlxRow>,
    /// Scenarios that could not be timed, and why.
    pub notes: Vec<String>,
}

/// One rollback point (plan 9, spec §3.3): restoring the world and
/// re-simulating `depth` ticks, medians of the runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RollbackPoint {
    pub label: String,
    pub level: String,
    pub worms: usize,
    pub depth: u32,
    pub runs: usize,
    pub restore_us_p50: f64,
    pub restore_us_p99: f64,
    pub resim_us_p50: f64,
    pub resim_us_p99: f64,
    pub total_us_p99: f64,
    pub snapshot_bytes: usize,
    pub journal_bytes_max: usize,
    /// The total's budget in µs, for the points that have one.
    pub budget_us: Option<f64>,
    /// Largest relative spread of the runs' totals.
    pub spread: f64,
    /// Why the point failed to run (then its figures are 0).
    pub failure: Option<String>,
}

impl RollbackPoint {
    /// Over its budget (a failed point holds nothing).
    pub fn over_budget(&self) -> bool {
        self.failure.is_some() || self.budget_us.is_some_and(|b| self.total_us_p99 > b)
    }

    /// One line for the console: the figures and the verdict.
    pub fn verdict_line(&self) -> String {
        if self.failure.is_some() {
            return self.verdict();
        }
        format!(
            "restore p99 {:.0} us, re-simulation p99 {:.0} us, total p99 {:.0} us: {}",
            self.restore_us_p99,
            self.resim_us_p99,
            self.total_us_p99,
            self.verdict()
        )
    }

    /// The report's verdict for the total against the budget.
    pub fn verdict(&self) -> String {
        match (&self.failure, self.budget_us) {
            (Some(why), _) => format!("failed: {why}"),
            (None, Some(b)) if self.total_us_p99 > b => {
                format!("BREAKS the {:.0} µs budget (total p99 {:.0} µs)", b, self.total_us_p99)
            }
            (None, Some(b)) => format!("holds ({:.0} µs budget)", b),
            (None, None) => "no budget".into(),
        }
    }
}

/// The rollback axis: its points and the noise the comparison allows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rollback {
    pub what: String,
    pub points: Vec<RollbackPoint>,
    pub noise: f64,
}

/// A rollback point from its runs (each figure's median over them).
pub fn rollback_point(label: &str, budget_us: Option<f64>, runs: &[RollbackOut]) -> RollbackPoint {
    let m = |f: fn(&RollbackOut) -> f64| median(&runs.iter().map(f).collect::<Vec<_>>());
    let first = runs.first().cloned().unwrap_or_default();
    RollbackPoint {
        label: label.into(),
        level: first.level.clone(),
        worms: first.worms,
        depth: first.depth,
        runs: runs.len(),
        restore_us_p50: m(|r| r.restore_us_p50),
        restore_us_p99: m(|r| r.restore_us_p99),
        resim_us_p50: m(|r| r.resim_us_p50),
        resim_us_p99: m(|r| r.resim_us_p99),
        total_us_p99: m(|r| r.total_us_p99),
        snapshot_bytes: first.snapshot_bytes,
        journal_bytes_max: runs.iter().map(|r| r.journal_bytes_max).max().unwrap_or(0),
        budget_us,
        spread: relative_spread(&runs.iter().map(|r| r.total_us_p99).collect::<Vec<_>>()),
        failure: None,
    }
}

/// A rollback point whose run failed.
pub fn failed_rollback_point(label: &str, budget_us: Option<f64>, why: &str) -> RollbackPoint {
    let last = why.lines().rfind(|l| !l.trim().is_empty()).unwrap_or(why);
    RollbackPoint {
        label: label.into(),
        budget_us,
        failure: Some(last.trim().into()),
        ..RollbackPoint::default()
    }
}

/// The results file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Envelope {
    pub note: String,
    pub host: Host,
    pub date: String,
    pub quick: bool,
    pub runs: usize,
    pub seconds: f32,
    pub budgets: Budgets,
    pub sweeps: BTreeMap<String, Sweep>,
    pub gpu: BTreeMap<String, GpuPath>,
    pub olx: Option<OlxComparison>,
    /// The rollback cost (plan 9); absent in older files.
    pub rollback: Option<Rollback>,
    /// Parts a bless did not measure, kept from the file before.
    pub kept: Vec<String>,
}

fn relative_spread(v: &[f64]) -> f64 {
    let m = median(v);
    let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(a, b), &x| (a.min(x), b.max(x)));
    if m > 0.0 && v.len() > 1 { (hi - lo) / m } else { 0.0 }
}

/// A point from its runs: each figure's median.
pub fn point(value: &str, n: f64, runs: &[StressOut], load_avg: f64) -> Point {
    let sp = |f: fn(&StressOut) -> Spread| Spread::median_of(&runs.iter().map(f).collect::<Vec<_>>());
    let m = |f: fn(&StressOut) -> f64| median(&runs.iter().map(f).collect::<Vec<_>>());
    let first = runs.first().cloned().unwrap_or_default();
    let tick_us = sp(|r| r.tick_us);
    let mut saturated = Vec::new();
    if first.ticks_at_pool_cap > 0 {
        saturated.push(format!("pool full ({}) for {} ticks", first.pool_capacity, first.ticks_at_pool_cap));
    }
    if first.ticks_at_fx_cap > 0 {
        saturated.push(format!("effects full ({}) for {} ticks", first.fx_capacity, first.ticks_at_fx_cap));
    }
    Point {
        value: value.into(),
        n,
        runs: runs.len(),
        tick_us,
        sim_us: sp(|r| r.sim_us),
        think_us: sp(|r| r.think_us),
        present_us: sp(|r| r.present_us),
        build_us: sp(|r| r.build_us),
        projectiles_peak: first.projectiles_peak,
        fx_peak: first.fx_peak,
        events_max: first.events_max,
        peak_mib: m(|r| r.peak_mib.unwrap_or(0.0)),
        area_cache: first.area_cache,
        caps: first.caps.clone(),
        saturated,
        load_avg,
        spread: relative_spread(&runs.iter().map(|r| r.tick_us.p99).collect::<Vec<_>>()),
        budget_share: tick_us.p99 / 1000.0 / Budgets::default().tick_ms,
        frame: None,
    }
}

/// A point whose run failed (a crash, the memory ceiling): the failure is its cap.
pub fn failed_point(value: &str, n: f64, why: &str, load_avg: f64) -> Point {
    let last_line = why.lines().rfind(|l| !l.trim().is_empty()).unwrap_or(why);
    Point {
        value: value.into(),
        n,
        caps: vec![format!("failed: {}", last_line.trim())],
        load_avg,
        ..Point::default()
    }
}

/// Why `p` does not hold (empty: it holds), with the frame budget `frame_budget_ms`.
pub fn causes(p: &Point, b: &Budgets, frame_budget_ms: f64) -> Vec<String> {
    let mut c = Vec::new();
    if p.runs > 0 && p.tick_us.p99 / 1000.0 > b.tick_ms {
        c.push(format!("tick p99 {:.2} ms > {} ms", p.tick_us.p99 / 1000.0, b.tick_ms));
    }
    if p.peak_mib > b.memory_mib {
        c.push(format!("memory {:.0} MiB > {:.0} MiB", p.peak_mib, b.memory_mib));
    }
    if let (Some(f), Some(frame)) = (&p.frame, p.frame_p99_ms())
        && frame > frame_budget_ms
    {
        c.push(format!(
            "frame p99 {frame:.2} ms ({:.2} drawn + {:.2} bots) > {frame_budget_ms:.1} ms",
            f.total_ms.p99,
            p.think_us.p99 / 1000.0
        ));
    }
    c.extend(p.caps.iter().map(|x| format!("cap: {x}")));
    c
}

/// The first point that does not hold.
pub fn breaking(points: &[Point], b: &Budgets, frame_budget_ms: f64) -> Option<Breaking> {
    points.iter().find_map(|p| {
        let c = causes(p, b, frame_budget_ms);
        (!c.is_empty()).then(|| Breaking { value: p.value.clone(), causes: c })
    })
}

/// Least squares of tick time against the value, over measured points (two at least).
pub fn fit(points: &[Point], b: &Budgets) -> Option<Fit> {
    let pts: Vec<&Point> = points.iter().filter(|p| p.runs > 0).collect();
    if pts.len() < 2 {
        return None;
    }
    let line = |y: fn(&Point) -> f64| {
        let n = pts.len() as f64;
        let (sx, sy) = (pts.iter().map(|p| p.n).sum::<f64>(), pts.iter().map(|&p| y(p)).sum::<f64>());
        let sxx = pts.iter().map(|p| p.n * p.n).sum::<f64>();
        let sxy = pts.iter().map(|&p| p.n * y(p)).sum::<f64>();
        let slope = (n * sxy - sx * sy) / (n * sxx - sx * sx);
        (slope, (sy - slope * sx) / n)
    };
    let (p50, _) = line(|p| p.tick_us.p50);
    let (p99, c99) = line(|p| p.tick_us.p99);
    let limit = if p99 > 0.0 { (b.tick_ms * 1000.0 - c99) / p99 } else { f64::INFINITY };
    Some(Fit { per_unit_us_p50: p50, per_unit_us_p99: p99, intercept_us_p99: c99, limit })
}

/// One resolution from its runs, with the probe's cost and what fits.
pub fn gpu_size(runs: &[SizeOut], b: &Budgets) -> GpuSize {
    let sp = |f: fn(&SizeOut) -> Spread| Spread::median_of(&runs.iter().map(f).collect::<Vec<_>>());
    let gpu: Vec<Spread> = runs.iter().filter_map(|r| r.gpu_ms).collect();
    let gpu_ms = (!gpu.is_empty()).then(|| Spread::median_of(&gpu));
    let first = runs.first().cloned().unwrap_or_default();
    let probe: Vec<ProbeRow> = first
        .probe
        .iter()
        .enumerate()
        .map(|(i, p)| ProbeRow {
            passes: p.passes,
            ms_p50: median(&runs.iter().filter_map(|r| r.probe.get(i)).map(|q| q.ms.p50).collect::<Vec<_>>()),
        })
        .collect();
    let (kk, km) = probe.iter().fold((0.0, 0.0), |(kk, km), r| {
        let k = f64::from(r.passes);
        (kk + k * k, km + k * r.ms_p50)
    });
    let ms_per_pass = if kk > 0.0 { km / kk } else { 0.0 };
    let frame_ms = sp(|r| r.frame_ms);
    let base = gpu_ms.map_or(frame_ms.p99, |g| g.p99);
    let fits = |budget: f64| {
        if ms_per_pass <= 1e-6 {
            9999
        } else {
            ((budget - base).max(0.0) / ms_per_pass).floor().min(9999.0) as u32
        }
    };
    GpuSize {
        size: first.size,
        tick_ms: sp(|r| r.tick_ms),
        cpu_ms: sp(|r| r.cpu_ms),
        frame_ms,
        total_ms: sp(|r| r.total_ms),
        gpu_ms,
        gpu_dropped: runs.iter().map(|r| r.gpu_dropped).sum(),
        probe_dropped: runs.iter().flat_map(|r| &r.probe).map(|p| p.gpu_dropped).sum(),
        spread: relative_spread(&runs.iter().map(|r| r.total_ms.p99).collect::<Vec<_>>()),
        probe,
        ms_per_pass,
        passes_144: fits(b.frame_144_ms),
        passes_60: fits(b.frame_60_ms),
    }
}

/// OLX's runs and olr-sim's of one scenario, in µs.
pub fn olx_row(name: &str, olx: &[Vec<OlxFrame>], rl: &[ScenarioTimingOut]) -> OlxRow {
    let us = |ns: &[u64]| Spread::of(&ns.iter().map(|&n| n as f64 / 1000.0).collect::<Vec<_>>());
    let olx_us = Spread::median_of(
        &olx.iter().map(|r| us(&r.iter().map(|f| f.ns).collect::<Vec<_>>())).collect::<Vec<_>>(),
    );
    let olr_us = Spread::median_of(&rl.iter().map(|r| us(&r.tick_ns)).collect::<Vec<_>>());
    let (o, r) = (olx.first().cloned().unwrap_or_default(), rl.first().cloned().unwrap_or_default());
    let same_work = o.len() == r.projectiles.len()
        && o.len() == r.rand_calls.len()
        && o.iter().zip(&r.projectiles).all(|(f, &p)| f.projectiles == p)
        && o.iter().zip(&r.rand_calls).all(|(f, &c)| f.rand_calls == c);
    OlxRow {
        name: name.into(),
        frames: o.len(),
        projectiles_peak: o.iter().map(|f| f.projectiles).max().unwrap_or(0),
        olx_us,
        olr_us,
        ratio_p50: if olx_us.p50 > 0.0 { olr_us.p50 / olx_us.p50 } else { 0.0 },
        same_work,
        matches_olx: r.matches_olx,
    }
}

/// Noise, frames from the GPU paths' levels (the WSL path first; a point
/// without a level has none), fits and breaking points. It derives all of
/// them from the measurements, so it can run again (as [`merged`] does).
pub fn finish(env: &mut Envelope) {
    let b = env.budgets;
    let levels: Vec<(String, GpuLevel)> = ["wsl-gl", "windows-dx12", "windows-vulkan"]
        .iter()
        .filter_map(|k| env.gpu.get(*k).map(|p| (k.to_string(), p.levels.clone())))
        .flat_map(|(k, ls)| ls.into_iter().map(move |l| (k.clone(), l)))
        .collect();
    for sweep in env.sweeps.values_mut() {
        for p in &mut sweep.points {
            p.frame =
                levels.iter().find(|(_, l)| l.axis == sweep.axis && l.value == p.value).map(|(path, l)| {
                    FrameCheck { path: path.clone(), size: l.size, total_ms: l.total_ms, gpu_ms: l.gpu_ms }
                });
        }
        sweep.noise = sweep.points.iter().map(|p| p.spread).fold(NOISE_FLOOR, f64::max);
        sweep.breaking_144 = breaking(&sweep.points, &b, b.frame_144_ms);
        sweep.breaking_60 = breaking(&sweep.points, &b, b.frame_60_ms);
        sweep.fit = if sweep.axis == "map" { None } else { fit(&sweep.points, &b) };
    }
    if let Some(r) = &mut env.rollback {
        r.noise = r.points.iter().map(|p| p.spread).fold(NOISE_FLOOR, f64::max);
    }
    for path in env.gpu.values_mut() {
        path.noise = path.sizes.iter().map(|s| s.spread).fold(NOISE_FLOOR, f64::max);
    }
}

/// A measurement against the committed one.
#[derive(Clone, Debug, PartialEq)]
pub struct Flag {
    pub name: String,
    pub baseline: f64,
    pub measured: f64,
    pub noise: f64,
}

impl Flag {
    /// Slower by more than the noise (an improvement never is).
    pub fn flagged(&self) -> bool {
        self.baseline > 0.0 && self.measured > self.baseline * (1.0 + self.noise)
    }

    pub fn line(&self) -> String {
        let change = if self.baseline > 0.0 { (self.measured / self.baseline - 1.0) * 100.0 } else { 0.0 };
        format!(
            "{} {:<44} {:>10.3} (committed {:>10.3}) {:+6.1} % (noise {:.0} %)",
            if self.flagged() { "!" } else { " " },
            self.name,
            self.measured,
            self.baseline,
            change,
            self.noise * 100.0
        )
    }
}

/// Tick p99 (ms) of every sweep point and frame p99 and probe ms per pass
/// of every GPU size measured in both, with the committed noise (at least
/// [`NOISE_FLOOR`]: an older file without it would flag every slowdown).
pub fn compare(stored: &Envelope, now: &Envelope) -> Vec<Flag> {
    let mut out = Vec::new();
    for (axis, s) in &now.sweeps {
        let Some(old) = stored.sweeps.get(axis) else { continue };
        for p in &s.points {
            if let Some(o) = old.points.iter().find(|o| o.value == p.value && o.runs > 0 && p.runs > 0) {
                let name = format!("{axis} {} tick p99 ms", p.value);
                out.push(Flag {
                    name,
                    baseline: o.tick_us.p99 / 1000.0,
                    measured: p.tick_us.p99 / 1000.0,
                    noise: old.noise.max(NOISE_FLOOR),
                });
            }
        }
    }
    if let (Some(r), Some(old)) = (&now.rollback, &stored.rollback) {
        for p in &r.points {
            if let Some(o) =
                old.points.iter().find(|o| o.label == p.label && o.failure.is_none() && p.failure.is_none())
            {
                out.push(Flag {
                    name: format!("rollback {} total p99 ms", p.label),
                    baseline: o.total_us_p99 / 1000.0,
                    measured: p.total_us_p99 / 1000.0,
                    noise: old.noise.max(NOISE_FLOOR),
                });
            }
        }
    }
    for (key, g) in &now.gpu {
        let Some(old) = stored.gpu.get(key) else { continue };
        for s in &g.sizes {
            if let Some(o) = old.sizes.iter().find(|o| o.size == s.size) {
                let at = format!("{key} {}x{}", s.size.0, s.size.1);
                out.push(Flag {
                    name: format!("{at} frame p99 ms"),
                    baseline: o.total_ms.p99,
                    measured: s.total_ms.p99,
                    noise: old.noise.max(NOISE_FLOOR),
                });
                out.push(Flag {
                    name: format!("{at} ms per pass"),
                    baseline: o.ms_per_pass,
                    measured: s.ms_per_pass,
                    noise: old.noise.max(NOISE_FLOOR),
                });
            }
        }
    }
    out
}

/// `new`, plus the sweeps, GPU paths and OLX table of `old` it did not
/// measure (listed in `kept`), finished again: a kept sweep's frames and
/// breaking points follow the GPU levels measured now.
pub fn merged(old: Option<Envelope>, new: Envelope) -> Envelope {
    let mut m = new;
    m.kept.clear();
    let Some(old) = old else {
        finish(&mut m);
        return m;
    };
    for (k, v) in old.sweeps {
        if !m.sweeps.contains_key(&k) {
            m.kept.push(format!("sweep {k}"));
            m.sweeps.insert(k, v);
        }
    }
    for (k, v) in old.gpu {
        if !m.gpu.contains_key(&k) {
            m.kept.push(format!("gpu {k}"));
            m.gpu.insert(k, v);
        }
    }
    if m.olx.is_none() && old.olx.is_some() {
        m.kept.push("olx".into());
        m.olx = old.olx;
    }
    if m.rollback.is_none() && old.rollback.is_some() {
        m.kept.push("rollback".into());
        m.rollback = old.rollback;
    }
    finish(&mut m);
    m
}

/// Why `stored` cannot be compared with `now` (or merged into by a
/// bless): a quick run and a full one, or other match lengths, measure
/// different things. `None`: they can.
pub fn incomparable(stored: &Envelope, now: &Envelope) -> Option<String> {
    let kind = |e: &Envelope| if e.quick { "quick" } else { "full" };
    (stored.quick != now.quick || stored.seconds != now.seconds).then(|| {
        format!(
            "the stored results are a {} run of {} s points, this is a {} run of {} s points: not compared",
            kind(stored),
            stored.seconds,
            kind(now),
            now.seconds
        )
    })
}

/// The file name's host part: the CPU model's slug and the CPU count.
pub fn host_key(host: &Host) -> String {
    format!("{}-{}cpus", olr_data::slug::slug(&host.cpu), host.cpus)
}

/// `fixtures/bench/envelope/<host>.ron`.
pub fn path_for(host: &Host) -> String {
    format!("{DIR}/{}.ron", host_key(host))
}

/// The results at `path`, if there are any.
pub fn load(path: &Path) -> Result<Option<Envelope>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => ron::from_str(&text).map(Some).map_err(|e| format!("{}: {e}", path.display())),
        Err(_) => Ok(None),
    }
}

/// The committed results at `path` for the comparison and a bless: an
/// unreadable file is treated as absent, with a notice saying so (the
/// measuring is done by then; it must not fail on an old file).
pub fn load_or_notice(path: &Path) -> (Option<Envelope>, Option<String>) {
    match load(path) {
        Ok(env) => (env, None),
        Err(e) => {
            (None, Some(format!("cannot read the stored results ({e}): not compared; a bless replaces them")))
        }
    }
}

pub fn write(path: &Path, env: &Envelope) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = ron::ser::to_string_pretty(env, ron::ser::PrettyConfig::default())?;
    std::fs::write(path, text + "\n")?;
    Ok(())
}

/// The one-minute load average (`/proc/loadavg`).
pub fn load_average() -> Option<f64> {
    std::fs::read_to_string("/proc/loadavg").ok()?.split_whitespace().next()?.parse().ok()
}

impl Point {
    /// The whole frame's p99 (spec §3), ms: the level's tick and frame
    /// (`gpu-bench`, which replays recorded inputs, so its tick has no
    /// bots' thinking) plus the point's bots' thinking p99 from the sweep.
    /// A sum of p99s: a bound, not a measured percentile.
    pub fn frame_p99_ms(&self) -> Option<f64> {
        self.frame.as_ref().map(|f| f.total_ms.p99 + self.think_us.p99 / 1000.0)
    }

    /// One line for the console.
    pub fn line(&self) -> String {
        if self.runs == 0 {
            return self.caps.join("; ");
        }
        format!(
            "tick p50/p99 {:.2}/{:.2} ms ({:.0} % of the budget), present p99 {:.2} ms, build p99 {:.2} ms, \
             {} projectiles, {} effects, {:.0} MiB{}",
            self.tick_us.p50 / 1000.0,
            self.tick_us.p99 / 1000.0,
            self.budget_share * 100.0,
            self.present_us.p99 / 1000.0,
            self.build_us.p99 / 1000.0,
            self.projectiles_peak,
            self.fx_peak,
            self.peak_mib,
            if self.caps.is_empty() { String::new() } else { format!("; caps: {}", self.caps.join("; ")) }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(p50: f64, p99: f64) -> Spread {
        Spread { p50, p99, max: p99 * 1.5 }
    }

    fn run(tick_p99_us: f64, mib: f64) -> StressOut {
        StressOut {
            tick_us: s(tick_p99_us / 2.0, tick_p99_us),
            peak_mib: Some(mib),
            area_cache: (10, 4096),
            ..StressOut::default()
        }
    }

    fn pt(value: &str, n: f64, tick_p99_us: f64) -> Point {
        point(value, n, &[run(tick_p99_us, 100.0)], 0.1)
    }

    #[test]
    fn points_take_the_median_of_their_runs() {
        let p = point("16", 16.0, &[run(900.0, 100.0), run(1000.0, 300.0), run(5000.0, 200.0)], 0.2);
        assert_eq!((p.tick_us.p99, p.peak_mib, p.runs), (1000.0, 200.0, 3));
        assert!((p.spread - 4.1).abs() < 1e-9, "(5000 - 900) / 1000: {}", p.spread);
        assert!((p.budget_share - 0.1).abs() < 1e-9, "1 ms of 10");
        let pool = StressOut { ticks_at_pool_cap: 5, pool_capacity: 3000, ..run(100.0, 1.0) };
        assert_eq!(
            point("3000", 3000.0, &[pool], 0.0).saturated.len(),
            1,
            "a full pool is saturated, not a cap"
        );
    }

    #[test]
    fn breaking_point_is_the_first_value_that_does_not_hold() {
        let b = Budgets::default();
        let mut points = vec![pt("2", 2.0, 500.0), pt("4", 4.0, 12_000.0), pt("8", 8.0, 30_000.0)];
        let br = breaking(&points, &b, b.frame_144_ms).unwrap();
        assert_eq!(br.value, "4");
        assert!(br.causes[0].starts_with("tick p99 12.00 ms"), "{:?}", br.causes);
        points[1].tick_us.p99 = 900.0;
        points[0].peak_mib = 4096.0;
        assert!(causes(&points[0], &b, b.frame_144_ms)[0].starts_with("memory 4096 MiB"));
        points[0].peak_mib = 1.0;
        points[1].frame = Some(FrameCheck { total_ms: s(5.0, 9.0), ..FrameCheck::default() });
        assert_eq!(breaking(&points, &b, b.frame_144_ms).unwrap().value, "4", "9 ms misses 144 Hz");
        assert_eq!(breaking(&points, &b, b.frame_60_ms).unwrap().value, "8", "but holds 60 Hz");
        points[0].caps.push("the bots' area cache is full (4096 areas)".into());
        assert_eq!(breaking(&points, &b, b.frame_60_ms).unwrap().value, "2");
    }

    /// gpu-bench replays recorded inputs, so its tick has no route
    /// finding: the frame judged is its tick and frame plus the point's
    /// bots' thinking p99 from the sweep (spec §3: the whole frame).
    #[test]
    fn the_frame_judged_includes_the_bots_thinking() {
        let b = Budgets::default();
        let mut p = pt("4096x4096", 1.0, 1300.0);
        p.think_us = s(400.0, 1200.0);
        p.frame = Some(FrameCheck { total_ms: s(5.0, 6.0), ..FrameCheck::default() });
        assert!((p.frame_p99_ms().unwrap() - 7.2).abs() < 1e-9, "{:?}", p.frame_p99_ms());
        let c = causes(&p, &b, b.frame_144_ms);
        assert_eq!(c, ["frame p99 7.20 ms (6.00 drawn + 1.20 bots) > 6.9 ms"], "6 ms alone would hold");
        assert!(causes(&p, &b, b.frame_60_ms).is_empty());
    }

    /// Review Focus 1.
    #[test]
    fn a_failed_point_is_a_breaking_cause() {
        let b = Budgets::default();
        let points = vec![
            pt("1", 1.0, 100.0),
            failed_point("8", 8.0, "olr-cli stress: 16400 MiB resident, over 16384", 0.0),
        ];
        let br = breaking(&points, &b, b.frame_144_ms).unwrap();
        assert_eq!(br.value, "8");
        assert!(br.causes[0].contains("failed: olr-cli stress: 16400 MiB"), "{:?}", br.causes);
        assert!(fit(&points, &b).is_none(), "one measured point fits nothing");
    }

    #[test]
    fn worm_cost_is_a_least_squares_slope() {
        let b = Budgets::default();
        let points: Vec<Point> =
            [2.0, 4.0, 8.0].iter().map(|&n| pt(&n.to_string(), n, 100.0 + 50.0 * n)).collect();
        let f = fit(&points, &b).unwrap();
        assert!(
            (f.per_unit_us_p99 - 50.0).abs() < 1e-6 && (f.intercept_us_p99 - 100.0).abs() < 1e-6,
            "{f:?}"
        );
        assert!((f.limit - 198.0).abs() < 1e-6, "(10000 - 100) / 50: {}", f.limit);
    }

    #[test]
    fn probe_passes_fit_the_budget() {
        let b = Budgets::default();
        let probe = |k: u32, ms: f64| ProbeOut { passes: k, ms: s(ms, ms), gpu_dropped: 0 };
        let size = SizeOut {
            size: (3840, 2160),
            frame_ms: s(2.0, 2.5),
            total_ms: s(3.0, 3.5),
            gpu_ms: Some(s(0.8, 0.94)),
            probe: vec![probe(0, 0.0), probe(2, 0.5), probe(4, 1.0)],
            ..SizeOut::default()
        };
        let g = gpu_size(&[size], &b);
        assert!((g.ms_per_pass - 0.25).abs() < 1e-9, "{}", g.ms_per_pass);
        assert_eq!((g.passes_144, g.passes_60), (24, 62), "(6.944 - 0.94) / 0.25, (16.667 - 0.94) / 0.25");
    }

    #[test]
    fn dropped_gpu_spans_are_counted_over_the_runs() {
        let run = |frames: u32, probe: [u32; 2]| SizeOut {
            gpu_dropped: frames,
            probe: probe.iter().map(|&d| ProbeOut { gpu_dropped: d, ..ProbeOut::default() }).collect(),
            ..SizeOut::default()
        };
        let g = gpu_size(&[run(2, [0, 1]), run(0, [3, 0]), run(1, [0, 0])], &Budgets::default());
        assert_eq!((g.gpu_dropped, g.probe_dropped), (3, 4));
    }

    #[test]
    fn olx_rows_compare_the_same_work() {
        let olx = vec![
            (0..4)
                .map(|f| OlxFrame { frame: f, ns: 2000, projectiles: f as u32, rand_calls: 10 * f as u64 })
                .collect(),
        ];
        let rl = ScenarioTimingOut {
            name: "x".into(),
            tick_ns: vec![1000; 4],
            projectiles: vec![0, 1, 2, 3],
            rand_calls: vec![0, 10, 20, 30],
            matches_olx: Some(true),
        };
        let row = olx_row("x", &olx, std::slice::from_ref(&rl));
        assert!(row.same_work && (row.ratio_p50 - 0.5).abs() < 1e-9 && row.projectiles_peak == 3, "{row:?}");
        let other = ScenarioTimingOut { projectiles: vec![0, 1, 2, 4], ..rl.clone() };
        assert!(!olx_row("x", &olx, &[other]).same_work);
        let short = ScenarioTimingOut { rand_calls: vec![0, 10, 20], ..rl };
        assert!(
            !olx_row("x", &olx, &[short]).same_work,
            "a missing frame's rand() calls are not the same work"
        );
    }

    /// Review Focus 4.
    #[test]
    fn regressions_beyond_noise_are_flagged_not_failed() {
        let sweep = |p99: f64| Sweep {
            axis: "worms".into(),
            noise: 0.10,
            points: vec![pt("16", 16.0, p99)],
            ..Sweep::default()
        };
        let mut stored = Envelope::default();
        stored.sweeps.insert("worms".into(), sweep(1000.0));
        let flagged = |p99: f64| {
            let mut now = Envelope::default();
            now.sweeps.insert("worms".into(), sweep(p99));
            compare(&stored, &now).into_iter().filter(Flag::flagged).count()
        };
        assert_eq!(flagged(1090.0), 0, "within the axis's 10 % noise");
        assert_eq!(flagged(1200.0), 1, "beyond it");
        assert_eq!(flagged(500.0), 0, "an improvement");
    }

    /// Review Focus 3.
    #[test]
    fn another_hosts_results_only_notice() {
        let here = Host { cpus: 12, cpu: "X".into(), adapter: "llvmpipe".into() };
        let stored = Envelope { host: Host { cpus: 8, ..here.clone() }, ..Envelope::default() };
        assert!(crate::bench::host_notice(&stored.host, &here).unwrap().contains("another machine"));
        assert_eq!(path_for(&here), "fixtures/bench/envelope/x-12cpus.ron");
    }

    /// A tool's output missing a field xtask reads (renamed in the tool)
    /// is an error, never a measured 0: only the stored results are
    /// lenient (Review Focus 3).
    #[test]
    fn a_tool_output_missing_a_field_is_an_error() {
        let spread = "(p50: 1.0, p99: 2.0, max: 3.0)";
        let probe = format!("(passes: 2, ms: {spread}, gpu_dropped: 0)");
        let size = format!(
            "(size: (8, 8), tick_ms: {spread}, cpu_ms: {spread}, frame_ms: {spread}, total_ms: {spread}, \
             gpu_ms: None, gpu_dropped: 0, probe: [{probe}])"
        );
        assert!(ron::from_str::<SizeOut>(&size).is_ok(), "{size}");
        let renamed = size.replace("total_ms", "whole_ms");
        let e = ron::from_str::<SizeOut>(&renamed).unwrap_err().to_string();
        assert!(e.contains("total_ms"), "{e}");
        assert!(ron::from_str::<ProbeOut>(&probe.replace("passes", "count")).is_err());
        let frame_log = format!(
            "(size: (8, 8), gpu: \"x\", frames: 3, fps: 60.0, frame_ms: {spread}, busy_ms: {spread})"
        );
        assert!(ron::from_str::<FrameLogOut>(&frame_log).is_ok());
        assert!(ron::from_str::<FrameLogOut>(&frame_log.replace("busy_ms", "work_ms")).is_err());
        let timing = "(name: \"x\", tick_ns: [1], projectiles: [0], rand_calls: [0], matches_olx: None)";
        assert!(ron::from_str::<ScenarioTimingOut>(timing).is_ok());
        assert!(ron::from_str::<ScenarioTimingOut>(&timing.replace("tick_ns", "ns")).is_err());
        assert!(ron::from_str::<TimingReportOut>("()").is_err());
        assert!(ron::from_str::<GpuReportOut>("(adapter: \"x\")").is_err());
        assert!(ron::from_str::<StressOut>("(tick_us: (p50: 1.0, p99: 2.0, max: 3.0))").is_err());
    }

    /// The tools' own output (`fixtures/bench/tool-output/`, kept equal to
    /// what `olr-cli` and `olr-game` write by their `tool_output` and
    /// `frame_log_sample` tests) reads with every field xtask uses set:
    /// none of them is a default 0.
    #[test]
    fn tool_outputs_parse() {
        let read = |name: &str| {
            std::fs::read_to_string(crate::root().join("fixtures/bench/tool-output").join(name)).unwrap()
        };
        let set = |s: Spread| s.p50 > 0.0 && s.p99 > 0.0 && s.max > 0.0;
        let r: StressOut = ron::from_str(&read("stress.ron")).unwrap();
        assert!(
            [r.tick_us, r.sim_us, r.think_us, r.present_us, r.build_us].into_iter().all(set)
                && [r.pool_capacity, r.projectiles_peak, r.fx_peak, r.fx_capacity, r.events_max]
                    .iter()
                    .all(|&n| n > 0)
                && r.ticks_at_pool_cap > 0
                && r.ticks_at_fx_cap > 0
                && r.peak_mib.is_some()
                && r.area_cache.0 > 0
                && !r.caps.is_empty(),
            "{r:?}"
        );
        let g: GpuReportOut = ron::from_str(&read("gpu-bench.ron")).unwrap();
        assert!(!g.adapter.is_empty() && !g.backend.is_empty() && !g.device_type.is_empty() && g.timestamps);
        let z = &g.sizes[0];
        assert!(z.size.0 > 0 && [z.tick_ms, z.cpu_ms, z.frame_ms, z.total_ms].into_iter().all(set), "{z:?}");
        assert!(z.gpu_ms.is_some_and(set) && z.gpu_dropped > 0, "{z:?}");
        assert!(z.probe[1].passes > 0 && set(z.probe[1].ms) && z.probe[1].gpu_dropped > 0, "{z:?}");
        let f: FrameLogOut = ron::from_str(&read("frame-log.ron")).unwrap();
        assert!(
            f.size.0 > 0
                && !f.gpu.is_empty()
                && f.frames > 0
                && f.fps > 0.0
                && set(f.frame_ms)
                && set(f.busy_ms),
            "{f:?}"
        );
        let rb: RollbackOut = ron::from_str(&read("rollback.ron")).unwrap();
        assert!(
            !rb.level.is_empty()
                && rb.worms > 0
                && rb.depth > 0
                && [rb.restore_us_p50, rb.restore_us_p99, rb.resim_us_p50, rb.resim_us_p99, rb.total_us_p99]
                    .iter()
                    .all(|&x| x > 0.0)
                && rb.snapshot_bytes > 0
                && rb.journal_bytes_max > 0,
            "{rb:?}"
        );
        let t: TimingReportOut = ron::from_str(&read("scenario-time.ron")).unwrap();
        let sc = &t.scenarios[0];
        assert!(
            !sc.name.is_empty()
                && !sc.tick_ns.is_empty()
                && !sc.projectiles.is_empty()
                && !sc.rand_calls.is_empty()
                && sc.matches_olx.is_some(),
            "{sc:?}"
        );
    }

    /// Review Focus 3.
    #[test]
    fn old_results_parse_with_defaults() {
        let old = "(host: (cpus: 12, cpu: \"X\"), sweeps: {\"worms\": (axis: \"worms\", points: [(value: \"2\")])})";
        let e: Envelope = ron::from_str(old).unwrap();
        assert_eq!(e.sweeps["worms"].points[0].value, "2");
        assert!(e.gpu.is_empty() && e.olx.is_none() && e.rollback.is_none());
        assert_eq!(e.budgets, Budgets::default());
    }

    /// Plan 9: a bless of another part keeps the stored rollback axis, and
    /// the axis blessed alone keeps the sweeps.
    #[test]
    fn rollback_merges_without_disturbing_other_parts() {
        let point = |label: &str, total: f64| RollbackPoint {
            label: label.into(),
            runs: 3,
            total_us_p99: total,
            budget_us: Some(2000.0),
            ..RollbackPoint::default()
        };
        let old = Envelope {
            sweeps: BTreeMap::from([(
                "worms".to_string(),
                Sweep { axis: "worms".into(), ..Sweep::default() },
            )]),
            rollback: Some(Rollback { points: vec![point("ref-12", 1500.0)], ..Rollback::default() }),
            ..Envelope::default()
        };
        let only_worms =
            merged(Some(old.clone()), Envelope { sweeps: old.sweeps.clone(), ..Envelope::default() });
        assert_eq!(only_worms.rollback.map(|r| r.points), old.rollback.clone().map(|r| r.points));
        assert_eq!(only_worms.kept, ["rollback"]);
        let only_rollback = merged(
            Some(old.clone()),
            Envelope {
                rollback: Some(Rollback { points: vec![point("ref-12", 1800.0)], ..Rollback::default() }),
                ..Envelope::default()
            },
        );
        assert!(only_rollback.sweeps.contains_key("worms"));
        assert_eq!(only_rollback.rollback.as_ref().unwrap().points[0].total_us_p99, 1800.0);
        assert_eq!(only_rollback.kept, ["sweep worms"]);
        // A slower total beyond the noise is flagged; compare reads both.
        let flags = compare(&old, &only_rollback);
        assert!(flags.iter().any(|f| f.name == "rollback ref-12 total p99 ms" && f.flagged()), "{flags:?}");
    }

    #[test]
    fn rollback_points_are_judged_against_their_budget() {
        let out =
            |total: f64| RollbackOut { total_us_p99: total, restore_us_p99: 1.0, ..RollbackOut::default() };
        let p = rollback_point("ref-12", Some(2000.0), &[out(1900.0), out(2100.0), out(2000.0)]);
        assert_eq!((p.total_us_p99, p.runs, p.over_budget()), (2000.0, 3, false));
        assert!(p.verdict().starts_with("holds"), "{}", p.verdict());
        let p = rollback_point("ref-12", Some(2000.0), &[out(2500.0)]);
        assert!(p.over_budget() && p.verdict().contains("BREAKS the 2000 µs budget"), "{}", p.verdict());
        assert!(!rollback_point("ref-28", None, &[out(9000.0)]).over_budget());
        assert!(failed_rollback_point("x", Some(1.0), "boom\n").over_budget());
    }

    /// Ruling T7-A: a quick run is never judged against a full one.
    #[test]
    fn quick_and_full_runs_are_not_compared() {
        let full = Envelope { seconds: 20.0, ..Envelope::default() };
        assert_eq!(incomparable(&full, &full.clone()), None);
        let quick = Envelope { quick: true, seconds: 5.0, ..Envelope::default() };
        let why = incomparable(&full, &quick).unwrap();
        assert!(why.contains("a full run of 20 s") && why.contains("a quick run of 5 s"), "{why}");
        assert!(incomparable(&full, &Envelope { seconds: 30.0, ..full.clone() }).is_some(), "other lengths");
    }

    /// Ruling T7-A: a killed run's cause names its status.
    #[test]
    fn a_killed_run_names_its_signal() {
        use std::os::unix::process::ExitStatusExt as _;
        let killed = std::process::Output {
            status: std::process::ExitStatus::from_raw(9),
            stdout: vec![],
            stderr: vec![],
        };
        let p = failed_point("8", 8.0, &crate::envelope::sweeps::failure("olr-cli stress", &killed), 0.0);
        assert!(
            p.caps[0].starts_with("failed: olr-cli stress (signal: 9") && p.caps[0].ends_with("no output"),
            "{:?}",
            p.caps
        );
        let exited = std::process::Output {
            status: std::process::ExitStatus::from_raw(1 << 8),
            stdout: b"report\n".to_vec(),
            stderr: b"error: 16400 MiB resident, over 16384\n\n".to_vec(),
        };
        let why = crate::envelope::sweeps::failure("olr-cli stress", &exited);
        assert_eq!(why, "olr-cli stress (exit status: 1): error: 16400 MiB resident, over 16384");
    }

    /// A bless of some parts keeps the others, and the kept sweeps' frames
    /// and breaking points follow the GPU levels measured now.
    #[test]
    fn a_merged_bless_is_finished_again() {
        let level = |total: f64| GpuLevel {
            axis: "map".into(),
            value: "4096x4096".into(),
            size: (2560, 1440),
            total_ms: s(total / 2.0, total),
            ..GpuLevel::default()
        };
        let mut old = Envelope::default();
        old.sweeps.insert(
            "map".into(),
            Sweep { axis: "map".into(), points: vec![pt("4096x4096", 1.0, 1000.0)], ..Sweep::default() },
        );
        old.gpu.insert("wsl-gl".into(), GpuPath { levels: vec![level(1.0)], ..GpuPath::default() });
        finish(&mut old);
        assert_eq!(old.sweeps["map"].points[0].frame.as_ref().unwrap().total_ms.p99, 1.0);
        assert!(old.sweeps["map"].breaking_144.is_none());
        let mut new = Envelope::default();
        new.gpu.insert("wsl-gl".into(), GpuPath { levels: vec![level(9.0)], ..GpuPath::default() });
        finish(&mut new);
        let m = merged(Some(old), new);
        assert_eq!(m.kept, ["sweep map"]);
        let p = &m.sweeps["map"].points[0];
        assert_eq!(p.frame.as_ref().unwrap().total_ms.p99, 9.0, "the frame measured now");
        assert_eq!(m.sweeps["map"].breaking_144.as_ref().map(|b| b.value.as_str()), Some("4096x4096"));
        let mut gone = m.clone();
        gone.gpu.clear();
        finish(&mut gone);
        assert!(gone.sweeps["map"].points[0].frame.is_none(), "no level, no frame");
    }

    /// A stored file without `noise` (an older one) is judged with the floor,
    /// not with 0 (which would flag every slowdown).
    #[test]
    fn a_stored_file_without_noise_uses_the_floor() {
        let sweep = |p99: f64, noise: f64| Sweep {
            axis: "worms".into(),
            noise,
            points: vec![pt("16", 16.0, p99)],
            ..Sweep::default()
        };
        let mut stored = Envelope::default();
        stored.sweeps.insert("worms".into(), sweep(1000.0, 0.0));
        stored.gpu.insert(
            "wsl-gl".into(),
            GpuPath {
                sizes: vec![GpuSize { size: (8, 8), total_ms: s(1.0, 1.0), ..GpuSize::default() }],
                ..GpuPath::default()
            },
        );
        let flagged = |p99: f64, total: f64| {
            let mut now = Envelope::default();
            now.sweeps.insert("worms".into(), sweep(p99, 0.0));
            now.gpu.insert(
                "wsl-gl".into(),
                GpuPath {
                    sizes: vec![GpuSize { size: (8, 8), total_ms: s(total, total), ..GpuSize::default() }],
                    ..GpuPath::default()
                },
            );
            compare(&stored, &now).iter().filter(|f| f.flagged()).map(|f| f.name.clone()).collect::<Vec<_>>()
        };
        assert!(flagged(1030.0, 1.03).is_empty(), "3 % is within the 5 % floor");
        assert_eq!(flagged(1100.0, 1.10), ["worms 16 tick p99 ms", "wsl-gl 8x8 frame p99 ms"]);
    }

    /// An unreadable committed file is a notice (treated as absent), never
    /// a failure after the measuring.
    #[test]
    fn an_unparseable_committed_file_is_a_notice() {
        let dir = crate::root().join("target/envelope-test");
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("bad.ron");
        std::fs::write(&bad, "(sweeps: {\"worms\": oops})").unwrap();
        let (env, notice) = load_or_notice(&bad);
        assert!(env.is_none());
        let n = notice.unwrap();
        assert!(n.contains("bad.ron") && n.contains("not compared"), "{n}");
        assert_eq!(load_or_notice(&dir.join("missing.ron")), (None, None));
    }

    #[test]
    fn a_bless_keeps_what_it_did_not_measure() {
        let mut old = Envelope::default();
        old.sweeps.insert("art".into(), Sweep::default());
        old.gpu.insert("windows-dx12".into(), GpuPath::default());
        let mut new = Envelope::default();
        new.sweeps.insert("worms".into(), Sweep::default());
        let m = merged(Some(old), new);
        assert_eq!(m.sweeps.keys().collect::<Vec<_>>(), ["art", "worms"]);
        assert_eq!(m.kept, ["sweep art", "gpu windows-dx12"]);
    }
}
