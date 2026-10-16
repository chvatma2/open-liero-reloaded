//! Benchmarks with stored baselines (spec §7, §21 G8): `cargo xtask bench`
//! and `full`'s `bench` step.
//!
//! - **divan benchmarks** ([`BENCHES`]: `olr-sim`'s tick with the pool
//!   full and 32 worms, `olr-render`'s offscreen frame at 1080p and 1440p,
//!   `olr-cli`'s 8-bot tick and draw-list build on the imported content),
//!   run in the bench (release) profile [`RUNS`] times; each one's median
//!   of the runs' medians is compared with `fixtures/bench/baselines.ron`.
//! - **the short stress run** ([`STRESS`]: both mods × OLX's pool and a
//!   raised one, [`STRESS_SECONDS`] of a 32-chaos-bot match each, through
//!   `olr-cli stress`, determinism checked on the first run); the median
//!   tick time and offscreen frame time against `fixtures/bench/stress.ron`.
//!
//! **Noise guard.** A measurement fails only above the baseline by more
//! than [`TOLERANCE`] (10 %), on the median of [`RUNS`] runs. This machine
//! is shared (WSL2, 12 logical CPUs on 6 cores): `tick_32_worms` measured
//! 4.0-4.6 µs a run when idle, but often +25 % with 2 other threads busy
//! and +35 % to +60 % with 6 to 12 (as when one lands on its core's other
//! hyperthread), and the [`RUNS`] runs of a target take about 7 s
//! together, so one burst of other work moves their median. And it ran
//! 13-20 % slower from some byte-identical copies of its executable than
//! from others, the same copy every time (where the file's pages lie in
//! memory), so every run of one built file draws the same. Hence:
//! - each run is from a fresh copy of the executable ([`Copies`]), so the
//!   [`RUNS`] runs and each retry are independent draws;
//! - the bench targets are built first ([`build_benches`]), then the
//!   machine is let [`settle`]: until fewer than [`SETTLE_BUSY_CPUS`] CPUs
//!   are busy (`/proc/stat`, every process in the VM, not ours alone) over
//!   [`SETTLE_SAMPLE`], for at most [`SETTLE_MAX`] (then a notice says how
//!   busy it stayed);
//! - a benchmark over the tolerance is measured again in up to
//!   [`RETRIES`] later windows, each [`RETRY_PAUSE`] after the last and
//!   settled, each the median of [`RUNS`] runs of its whole target (as
//!   the baseline was measured, so a real regression shows in every
//!   window; a function run alone measured 2-3 % faster); the
//!   best window counts, as noise only ever slows a run down. A notice
//!   lists every window.
//!
//! A measurement without a baseline, or a baseline not measured (no
//! adapter, no content), is a notice, never a failure. `cargo xtask bench
//! --bless --reason "<why>"` measures everything and writes both files,
//! logged in `fixtures/blessings.log`. Baselines are this machine's.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::root;

/// The divan benchmarks as `(package, bench target)`.
pub const BENCHES: &[(&str, &str)] =
    &[("olr-sim", "tick"), ("olr-render", "frame"), ("olr-cli", "game"), ("olr-cli", "rollback")];

pub const BASELINES: &str = "fixtures/bench/baselines.ron";
pub const STRESS_BASELINE: &str = "fixtures/bench/stress.ron";

/// Runs whose median is compared.
pub const RUNS: usize = 3;
/// More windows of [`RUNS`] runs for a benchmark over the tolerance; the
/// best window's median counts.
pub const RETRIES: usize = 2;
/// The wait before each retry window, so it does not fall in the same
/// burst of other work (on the Windows side too, which `/proc/stat` does
/// not show).
pub const RETRY_PAUSE: std::time::Duration = std::time::Duration::from_secs(5);
/// The machine counts as settled with fewer CPUs than this busy.
pub const SETTLE_BUSY_CPUS: f64 = 1.0;
/// How long one look at the machine's load lasts.
pub const SETTLE_SAMPLE: std::time::Duration = std::time::Duration::from_secs(1);
/// The longest wait for the machine to settle before measuring anyway.
pub const SETTLE_MAX: std::time::Duration = std::time::Duration::from_secs(60);
/// How much slower than its baseline a measurement may be.
pub const TOLERANCE: f64 = 0.10;
/// How much more peak memory than its baseline a stress run may take.
pub const MEMORY_TOLERANCE: f64 = 0.25;

/// The tolerance of measurement `name`.
pub fn tolerance(name: &str) -> f64 {
    if name.ends_with("/peak_mib") { MEMORY_TOLERANCE } else { TOLERANCE }
}

/// The machine the baselines were measured on: they hold only there.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Host {
    /// Logical CPUs.
    pub cpus: usize,
    /// The CPU's model name (`/proc/cpuinfo`).
    pub cpu: String,
    /// The graphics adapter offscreen renders use (`olr-render`'s `adapter`
    /// example), or `none`.
    pub adapter: String,
}

impl Host {
    /// This machine.
    pub fn here() -> Host {
        let cpus = std::thread::available_parallelism().map_or(0, usize::from);
        let cpu = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|t| {
                let line = t.lines().find(|l| l.starts_with("model name"))?.to_string();
                Some(line.split_once(':')?.1.trim().to_string())
            })
            .unwrap_or_default();
        let adapter = match crate::check::probe_adapter() {
            Ok(Ok(a)) => a,
            _ => "none".into(),
        };
        Host { cpus, cpu, adapter }
    }
}

/// What to say when baselines measured on `stored` are compared on `here`
/// (`None`: the same machine).
pub fn host_notice(stored: &Host, here: &Host) -> Option<String> {
    (stored != here).then(|| {
        format!(
            "the baselines are from another machine ({} CPUs, {:?}, {:?}; here {} CPUs, {:?}, {:?}): \
             not gated; run `cargo xtask bench --bless --reason \"<why>\"` here",
            stored.cpus, stored.cpu, stored.adapter, here.cpus, here.cpu, here.adapter
        )
    })
}

/// The short stress run's configurations: `(mod, pool)` on Castle Strike
/// with 32 chaos bots.
pub const STRESS: &[(&str, usize)] =
    &[("olx-mw-1-0", 3000), ("olx-mw-1-0", 20_000), ("olx-classic", 3000), ("olx-classic", 20_000)];
pub const STRESS_LEVEL: &str = "olx-level-castlestrike";
/// Seconds of match time a configuration plays in `full`.
pub const STRESS_SECONDS: f32 = 20.0;

/// `fixtures/bench/baselines.ron`: each benchmark's median, in ns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Baselines {
    pub note: String,
    pub host: Host,
    /// `<package>/<bench target>/<function>` to ns.
    pub benches: BTreeMap<String, f64>,
    /// Baselines a bless could not measure (no adapter, no content) and
    /// kept from the file before.
    pub kept: Vec<String>,
}

/// The baselines a bless writes: `new`'s, plus `old`'s it did not
/// measure, listed as kept.
pub fn blessed(old: &Baselines, new: BTreeMap<String, f64>, host: Host) -> Baselines {
    let mut benches = new;
    let mut kept = Vec::new();
    for (name, &ns) in &old.benches {
        if !benches.contains_key(name) {
            benches.insert(name.clone(), ns);
            kept.push(name.clone());
        }
    }
    let note = "Medians in ns (`cargo xtask bench --bless --reason`); valid on `host` only.".into();
    Baselines { note, host, benches, kept }
}

/// `fixtures/bench/stress.ron`: each configuration's medians.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StressBaselines {
    pub note: String,
    pub host: Host,
    pub seconds: f32,
    pub configs: BTreeMap<String, StressBaseline>,
}

/// One configuration's medians (the gated ones) and what it did (for a
/// reader: these change whenever the simulation does).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StressBaseline {
    /// The bots' thinking and the world's tick, p50, in µs (gated).
    pub tick_us_p50: f64,
    /// The offscreen frame, p50, in ms (gated; 0 without an adapter).
    pub render_ms_p50: f64,
    /// The process's peak memory, in MiB (gated at [`MEMORY_TOLERANCE`];
    /// 0 where `/proc` is missing).
    pub peak_mib: f64,
    pub tick_us_p99: f64,
    pub projectiles_peak: usize,
    pub fx_peak: usize,
    pub events_max: usize,
}

/// What `olr-cli stress --out` writes, the parts read here (`olr_cli::stress::Report`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Report {
    pub name: String,
    pub tick_us: Spread,
    pub render: Option<RenderReport>,
    pub projectiles_peak: usize,
    pub fx_peak: usize,
    pub events_max: usize,
    pub peak_mib: Option<f64>,
    pub deterministic: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Spread {
    pub p50: f64,
    pub p99: f64,
    pub max: f64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct RenderReport {
    pub adapter: String,
    pub frame_ms: Spread,
}

/// A time as divan prints it (`3.135 µs`), in ns.
pub fn parse_time(text: &str) -> Option<f64> {
    let mut parts = text.split_whitespace();
    let n: f64 = parts.next()?.parse().ok()?;
    let scale = match parts.next()? {
        "ns" => 1.0,
        "µs" | "us" => 1e3,
        "ms" => 1e6,
        "s" => 1e9,
        _ => return None,
    };
    Some(n * scale)
}

/// The medians of divan's table in `out`, by `<prefix>/<function>`.
pub fn parse_divan(out: &str, prefix: &str) -> BTreeMap<String, f64> {
    let mut found = BTreeMap::new();
    for line in out.lines() {
        let cols: Vec<&str> = line.split('│').collect();
        if cols.len() < 4 {
            continue;
        }
        // "├─ name   fastest" (a top-level line, the target's name, has no
        // tree mark and no times: it is skipped).
        let first = cols[0].trim_start_matches(['├', '╰', '─', '│', ' ']);
        let Some(name) = first.split_whitespace().next() else { continue };
        if !cols[0].contains('─') {
            continue;
        }
        if let Some(median) = parse_time(cols[2].trim()) {
            found.insert(format!("{prefix}/{name}"), median);
        }
    }
    found
}

/// The median of `v` (the lower middle of an even count).
pub fn median(v: &[f64]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v.get((v.len().max(1) - 1) / 2).copied().unwrap_or(0.0)
}

fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

/// A built bench target: its executable and its package's directory
/// (where `cargo bench` would run it).
#[derive(Clone, Debug)]
pub struct BenchBin {
    pub package: &'static str,
    pub bench: &'static str,
    pub exe: PathBuf,
    pub dir: PathBuf,
}

/// Builds every bench target (bench profile, each as `cargo bench -p
/// <package> --bench <target>` would, so features unify the same), so no
/// build runs between measurements: their executables.
fn build_benches(envs: &[(&str, PathBuf)]) -> Result<Vec<BenchBin>, String> {
    let mut bins = Vec::new();
    for &(package, bench) in BENCHES {
        let mut c = cargo();
        c.args(["bench", "--quiet", "--no-run", "--message-format=json", "-p", package, "--bench", bench]);
        for (k, v) in envs {
            c.env(k, v);
        }
        let out = c.output().map_err(|e| format!("cannot run cargo: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        let found = bench_executable(&out.stdout, bench);
        let (exe, dir) = found.ok_or_else(|| format!("{package}/{bench}: cargo named no executable"))?;
        bins.push(BenchBin { package, bench, exe, dir });
    }
    Ok(bins)
}

/// Bench target `bench`'s executable and its package's directory, from
/// `cargo bench --no-run --message-format=json`'s output.
pub fn bench_executable(json: &[u8], bench: &str) -> Option<(PathBuf, PathBuf)> {
    cargo_metadata::Message::parse_stream(json).find_map(|m| match m {
        Ok(cargo_metadata::Message::CompilerArtifact(a)) if a.target.is_bench() && a.target.name == bench => {
            let dir = a.manifest_path.parent()?.as_std_path().to_path_buf();
            Some((a.executable?.into_std_path_buf(), dir))
        }
        _ => None,
    })
}

/// Where [`Copies`] puts its executables.
pub const COPIES: &str = "target/bench-copies";

/// Fresh copies of bench executables, one a run, kept until dropped.
///
/// `tick_32_worms` ran 13-20 % slower from some byte-identical copies of
/// its executable than from others, the same copy every time (where the
/// file's pages lie in memory), so a run of the built file alone is one
/// draw for every run and retry until it is rebuilt. Each run here is a
/// new copy, a new draw; all are kept until the step ends, so a copy
/// cannot take the pages a removed one left.
pub struct Copies {
    dir: PathBuf,
    made: usize,
}

impl Copies {
    pub fn new() -> Result<Copies, String> {
        let dir = root().join(COPIES);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Copies { dir, made: 0 })
    }

    /// One run of `bin` from a fresh copy, every function: its medians,
    /// and whether it said it skipped (`skip ...`: no adapter, no content).
    fn run(
        &mut self,
        bin: &BenchBin,
        envs: &[(&str, PathBuf)],
    ) -> Result<(BTreeMap<String, f64>, bool), String> {
        let (package, bench) = (bin.package, bin.bench);
        self.made += 1;
        let copy = self.dir.join(format!("{bench}-{}", self.made));
        std::fs::copy(&bin.exe, &copy)
            .map_err(|e| format!("{} to {}: {e}", bin.exe.display(), copy.display()))?;
        let mut c = Command::new(&copy);
        c.arg("--bench").current_dir(&bin.dir);
        for (k, v) in envs {
            c.env(k, v);
        }
        let out = c.output().map_err(|e| format!("cannot run {}: {e}", copy.display()))?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            return Err(format!("{package}/{bench}: {text}{}", String::from_utf8_lossy(&out.stderr)));
        }
        let skip = text.lines().find(|l| l.starts_with("skip "));
        if let Some(skip) = skip {
            println!("     {skip}");
        }
        Ok((parse_divan(&text, &format!("{package}/{bench}")), skip.is_some()))
    }
}

impl Drop for Copies {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The aggregate `cpu` line of `/proc/stat`: (busy, total) jiffies, busy
/// being everything but idle and I/O wait (steal included: the host took
/// the CPU).
pub fn cpu_jiffies(stat: &str) -> Option<(u64, u64)> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line.split_whitespace().skip(1).map(|x| x.parse().ok()).collect::<Option<_>>()?;
    // user nice system idle iowait irq softirq steal (guest time is in user).
    let total: u64 = v.iter().take(8).sum();
    let idle = v.get(3)? + v.get(4).copied().unwrap_or(0);
    Some((total - idle, total))
}

/// How many of `cpus` CPUs were busy between two [`cpu_jiffies`] readings.
pub fn busy_cpus(before: (u64, u64), after: (u64, u64), cpus: usize) -> f64 {
    let total = after.1.saturating_sub(before.1);
    if total == 0 {
        return 0.0;
    }
    after.0.saturating_sub(before.0) as f64 / total as f64 * cpus as f64
}

/// How the wait for the machine to settle ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Settle {
    /// Fewer than [`SETTLE_BUSY_CPUS`] busy after waiting this long.
    Idle(std::time::Duration),
    /// Still this many CPUs busy at [`SETTLE_MAX`].
    Busy(f64),
    /// No `/proc/stat` to read.
    Unknown,
}

/// Waits until fewer than [`SETTLE_BUSY_CPUS`] CPUs are busy over a
/// [`SETTLE_SAMPLE`], at most [`SETTLE_MAX`].
pub fn settle() -> Settle {
    let read = || std::fs::read_to_string("/proc/stat").ok().as_deref().and_then(cpu_jiffies);
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    let start = std::time::Instant::now();
    loop {
        let Some(before) = read() else { return Settle::Unknown };
        std::thread::sleep(SETTLE_SAMPLE);
        let Some(after) = read() else { return Settle::Unknown };
        let busy = busy_cpus(before, after, cpus);
        if busy < SETTLE_BUSY_CPUS {
            return Settle::Idle(start.elapsed());
        }
        if start.elapsed() >= SETTLE_MAX {
            return Settle::Busy(busy);
        }
    }
}

/// A notice for a wait that ended busy, naming what was measured.
fn settle_notice(s: Settle, what: &str) -> Option<String> {
    match s {
        Settle::Busy(busy) => Some(format!(
            "{what}: measured on a busy machine ({busy:.1} CPUs in use after {} s; settled is under {SETTLE_BUSY_CPUS})",
            SETTLE_MAX.as_secs()
        )),
        _ => None,
    }
}

/// Each benchmark's measurements, by name.
type Measured = BTreeMap<String, Vec<f64>>;

/// What [`measure_benches`] found.
struct Measurement {
    bins: Vec<BenchBin>,
    runs: Measured,
    /// The targets that said they skipped (`<package>/<bench target>`).
    skipped: BTreeSet<String>,
    /// A notice when the machine stayed busy.
    busy: Option<String>,
}

/// Every benchmark `runs` times, built first, on a settled machine, each
/// run from a fresh copy.
fn measure_benches(
    runs: usize,
    envs: &[(&str, PathBuf)],
    copies: &mut Copies,
) -> Result<Measurement, String> {
    let bins = build_benches(envs)?;
    let busy = settle_notice(settle(), "the benchmarks");
    let mut all: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut skipped = BTreeSet::new();
    for _ in 0..runs {
        for bin in &bins {
            let (package, bench) = (bin.package, bin.bench);
            let (found, skip) = copies.run(bin, envs)?;
            if skip {
                skipped.insert(format!("{package}/{bench}"));
            }
            for (name, ns) in found {
                all.entry(name).or_default().push(ns);
            }
        }
    }
    Ok(Measurement { bins, runs: all, skipped, busy })
}

/// `olr-cli` in the release profile, built.
pub fn olr_cli() -> Result<PathBuf, String> {
    olr_cli_with(false)
}

/// [`olr_cli`]; `offline` builds without the network (cargo's `--offline`).
pub fn olr_cli_with(offline: bool) -> Result<PathBuf, String> {
    let mut build = cargo();
    build.args(["build", "--quiet", "--release", "-p", "olr-cli"]);
    if offline {
        build.arg("--offline");
    }
    let out = build.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(root().join("target/release/olr-cli"))
}

/// One stress configuration through `olr-cli stress`: its report. With
/// `stream`, its progress and report are shown as it plays.
pub fn stress_run(
    cli: &Path,
    content: &Path,
    (mod_id, pool): (&str, usize),
    seconds: f32,
    verify: bool,
    stream: bool,
) -> Result<Report, String> {
    let out_file = root().join(format!("target/stress/{mod_id}-pool{pool}.ron"));
    if let Some(dir) = out_file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut c = Command::new(cli);
    c.current_dir(root())
        .args(["stress", "--mod", mod_id, "--level", STRESS_LEVEL, "--worms", "32"])
        .args(["--pool", &pool.to_string(), "--seconds", &seconds.to_string()])
        .arg("--content")
        .arg(content)
        .arg("--out")
        .arg(&out_file);
    if !verify {
        c.arg("--no-verify");
    }
    let ok = if stream {
        c.status().map_err(|e| format!("cannot run olr-cli: {e}"))?.success()
    } else {
        let out = c.output().map_err(|e| format!("cannot run olr-cli: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "olr-cli stress {mod_id} pool {pool} failed:\n{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        true
    };
    if !ok {
        return Err(format!("olr-cli stress {mod_id} pool {pool} failed"));
    }
    let text = std::fs::read_to_string(&out_file).map_err(|e| format!("{}: {e}", out_file.display()))?;
    ron::from_str(&text).map_err(|e| format!("{}: {e}", out_file.display()))
}

/// Every stress configuration `runs` times (determinism checked on the
/// first): each one's configuration and reports, by report name.
type StressRuns = BTreeMap<String, ((&'static str, usize), Vec<Report>)>;

fn measure_stress(cli: &Path, content: &Path, runs: usize) -> Result<StressRuns, String> {
    let mut all = StressRuns::new();
    for run in 0..runs {
        for &cfg in STRESS {
            let r = stress_run(cli, content, cfg, STRESS_SECONDS, run == 0, false)?;
            if run == 0 && r.deterministic != Some(true) {
                return Err(format!("{}: not checked deterministic", r.name));
            }
            all.entry(r.name.clone()).or_insert_with(|| (cfg, Vec::new())).1.push(r);
        }
    }
    Ok(all)
}

/// The gated medians of `configs`, by `<configuration>/<measurement>`
/// (a frame or memory figure of 0 was not measured: no adapter, no `/proc`).
fn gated(configs: &BTreeMap<String, StressBaseline>) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    for (k, b) in configs {
        m.insert(format!("{k}/tick_us_p50"), b.tick_us_p50);
        if b.render_ms_p50 > 0.0 {
            m.insert(format!("{k}/render_ms_p50"), b.render_ms_p50);
        }
        if b.peak_mib > 0.0 {
            m.insert(format!("{k}/peak_mib"), b.peak_mib);
        }
    }
    m
}

/// What the stress runs could not measure, as [`compare`]'s skipped
/// prefixes: frames without an adapter, memory without `/proc`.
fn stress_skipped(runs: &StressRuns) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for (name, (_, reports)) in runs {
        if reports.iter().any(|r| r.render.is_none()) {
            s.insert(format!("{name}/render_ms_p50"));
        }
        if reports.iter().any(|r| r.peak_mib.is_none()) {
            s.insert(format!("{name}/peak_mib"));
        }
    }
    s
}

/// A configuration's medians over its runs.
fn stress_medians(runs: &[Report]) -> StressBaseline {
    let m = |f: &dyn Fn(&Report) -> f64| median(&runs.iter().map(f).collect::<Vec<_>>());
    StressBaseline {
        tick_us_p50: m(&|r| r.tick_us.p50),
        render_ms_p50: m(&|r| r.render.as_ref().map_or(0.0, |x| x.frame_ms.p50)),
        tick_us_p99: m(&|r| r.tick_us.p99),
        projectiles_peak: runs.first().map_or(0, |r| r.projectiles_peak),
        fx_peak: runs.first().map_or(0, |r| r.fx_peak),
        events_max: runs.first().map_or(0, |r| r.events_max),
        peak_mib: m(&|r| r.peak_mib.unwrap_or(0.0)),
    }
}

/// How one measurement compares with its baseline.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub name: String,
    pub baseline: f64,
    pub measured: f64,
}

impl Verdict {
    /// The measurement over the baseline, as a fraction (0.05 is 5 % slower).
    pub fn change(&self) -> f64 {
        if self.baseline > 0.0 { self.measured / self.baseline - 1.0 } else { 0.0 }
    }

    pub fn regressed(&self) -> bool {
        self.change() > tolerance(&self.name)
    }

    fn line(&self, unit: &str, scale: f64) -> String {
        format!(
            "{:<58} {:>10.2} {unit} (baseline {:>10.2}) {:+6.1} %",
            self.name,
            self.measured / scale,
            self.baseline / scale,
            self.change() * 100.0
        )
    }
}

/// Measurements against their baselines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Comparison {
    pub verdicts: Vec<Verdict>,
    /// Measurements without a baseline, and baselines whose target skipped.
    pub notices: Vec<String>,
    /// Baselines nothing measured though their target ran: failures.
    pub missing: Vec<String>,
}

/// The verdicts of `measured` (each a median) against `baseline`; a
/// baseline not measured is a notice when a prefix in `skipped` covers it
/// (its target said it skipped), else missing.
pub fn compare(
    baseline: &BTreeMap<String, f64>,
    measured: &BTreeMap<String, f64>,
    skipped: &BTreeSet<String>,
) -> Comparison {
    let mut c = Comparison::default();
    for (name, &b) in baseline {
        match measured.get(name) {
            Some(&m) => c.verdicts.push(Verdict { name: name.clone(), baseline: b, measured: m }),
            None if skipped.iter().any(|p| name.starts_with(p.as_str())) => {
                c.notices.push(format!("{name}: skipped here (no adapter or content)"));
            }
            None => c.missing.push(name.clone()),
        }
    }
    for name in measured.keys().filter(|n| !baseline.contains_key(*n)) {
        c.notices.push(format!("{name}: no baseline (`cargo xtask bench --bless --reason ...`)"));
    }
    c
}

fn read_ron<T: for<'de> Deserialize<'de>>(path: &str) -> Result<T, String> {
    let text = std::fs::read_to_string(root().join(path)).map_err(|e| format!("{path}: {e}"))?;
    ron::from_str(&text).map_err(|e| format!("{path}: {e}"))
}

fn write_ron<T: Serialize>(path: &str, v: &T) -> anyhow::Result<()> {
    let text = ron::ser::to_string_pretty(v, ron::ser::PrettyConfig::default())?;
    let p = root().join(path);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(p, text + "\n")?;
    Ok(())
}

/// `full`'s `bench` step: the divan benchmarks against their baselines
/// (not gated, with a notice, on another machine than theirs).
pub fn bench_step(envs: &[(&str, PathBuf)], verbose: bool) -> Result<(), String> {
    let base: Baselines = read_ron(BASELINES)?;
    if let Some(n) = host_notice(&base.host, &Host::here()) {
        println!("     notice: {BASELINES}: {n}");
        return Ok(());
    }
    let mut copies = Copies::new()?;
    let m = measure_benches(RUNS, envs, &mut copies)?;
    let medians: BTreeMap<String, f64> = m.runs.iter().map(|(k, v)| (k.clone(), median(v))).collect();
    let mut c = compare(&base.benches, &medians, &m.skipped);
    c.notices.extend(m.busy);
    let mut retried = Vec::new();
    for v in c.verdicts.iter_mut().filter(|v| v.regressed()) {
        // The noise guard: later windows of runs of its target.
        let first = v.measured;
        let mut windows = Vec::new();
        for _ in 0..RETRIES {
            std::thread::sleep(RETRY_PAUSE);
            let settled = settle();
            let mut window = Vec::new();
            for _ in 0..RUNS {
                window.push(rerun(&v.name, &m.bins, envs, &mut copies)?);
            }
            windows.push((median(&window), settled));
            v.measured = best_window(first, &windows);
            if !v.regressed() {
                break;
            }
        }
        retried.push(window_notice(v, first, &windows));
    }
    c.notices.extend(retried);
    report("bench", &c, "µs", 1e3, verbose)
}

/// One more run of benchmark `name`'s target (`<package>/<bench
/// target>/<function>`), all of it as the baseline's runs (a function run
/// alone measured 2-3 % faster) and from a fresh copy: the benchmark's
/// median. A run that did not measure it is an error, never a pass (the
/// retries once filtered by a name divan matched to nothing, and so never
/// measured anything).
fn rerun(
    name: &str,
    bins: &[BenchBin],
    envs: &[(&str, PathBuf)],
    copies: &mut Copies,
) -> Result<f64, String> {
    let (package, bench) = split_name(name);
    let bin = bins
        .iter()
        .find(|b| b.package == package && b.bench == bench)
        .ok_or_else(|| format!("{name}: no bench target {package}/{bench}"))?;
    let (again, _) = copies.run(bin, envs)?;
    again.get(name).copied().ok_or_else(|| format!("{name}: its target ran again but did not measure it"))
}

/// The measurement a benchmark over its tolerance keeps: the best of its
/// first median and its retry windows' medians (noise only slows a run
/// down; a real regression slows every window).
pub fn best_window(first: f64, windows: &[(f64, Settle)]) -> f64 {
    windows.iter().map(|w| w.0).fold(first, f64::min)
}

/// The notice for a benchmark measured again: each window's change, and
/// whether its machine was busy.
fn window_notice(v: &Verdict, first: f64, windows: &[(f64, Settle)]) -> String {
    let change = |ns: f64| if v.baseline > 0.0 { (ns / v.baseline - 1.0) * 100.0 } else { 0.0 };
    let each: Vec<String> = windows
        .iter()
        .map(|(ns, s)| match s {
            Settle::Busy(busy) => format!("{:+.1} % (busy: {busy:.1} CPUs)", change(*ns)),
            _ => format!("{:+.1} %", change(*ns)),
        })
        .collect();
    format!(
        "{}: {:+.1} %, then {} in {} later window(s) of {RUNS} runs",
        v.name,
        change(first),
        each.join(", "),
        windows.len()
    )
}

fn retry_notice(v: &Verdict, first: f64) -> String {
    format!(
        "{}: {:+.1} %, then {:+.1} % at best of {RETRIES} more runs",
        v.name,
        first * 100.0,
        v.change() * 100.0
    )
}

fn split_name(name: &str) -> (&str, &str) {
    let mut parts = name.split('/');
    (parts.next().unwrap_or_default(), parts.next().unwrap_or_default())
}

/// Prints the verdicts (every one when `verbose`, else a summary line)
/// and the notices; `Err` lists the regressions and the missing.
fn report(what: &str, c: &Comparison, unit: &str, scale: f64, verbose: bool) -> Result<(), String> {
    let table: Vec<String> = c.verdicts.iter().map(|v| format!("     {}", v.line(unit, scale))).collect();
    if verbose {
        println!("{}", table.join("\n"));
    }
    for n in &c.notices {
        println!("     notice: {n}");
    }
    let bad: Vec<String> = c.verdicts.iter().filter(|v| v.regressed()).map(|v| v.line(unit, scale)).collect();
    if !bad.is_empty() || !c.missing.is_empty() {
        let mut err = if verbose { String::new() } else { table.join("\n") + "\n" };
        if !bad.is_empty() {
            err.push_str(&format!(
                "over the baseline by more than {:.0} % ({:.0} % for memory; median of {RUNS} runs, then the best of {RETRIES} retries):\n{}\n",
                TOLERANCE * 100.0,
                MEMORY_TOLERANCE * 100.0,
                bad.join("\n")
            ));
        }
        if !c.missing.is_empty() {
            err.push_str(&format!(
                "baselined but measured by nothing (its target did not skip): {:?}\n",
                c.missing
            ));
        }
        return Err(err.trim_end().to_string());
    }
    if !verbose && let Some(worst) = c.verdicts.iter().max_by(|a, b| a.change().total_cmp(&b.change())) {
        println!(
            "     {what}: {} within tolerance of the baselines (worst {:+.1} %, {})",
            c.verdicts.len(),
            worst.change() * 100.0,
            worst.name
        );
    }
    Ok(())
}

/// `full`'s `stress` step: the short stress run against its baseline (the
/// tick, frame and memory medians), every configuration deterministic
/// (checked on any machine; the medians are gated only on the baseline's).
pub fn stress_step(content: Option<&Path>, verbose: bool) -> Result<(), String> {
    let Some(content) = content else {
        println!("skip stress (no OLX content: --offline)");
        return Ok(());
    };
    let base: StressBaselines = read_ron(STRESS_BASELINE)?;
    let cli = olr_cli()?;
    let runs = measure_stress(&cli, content, RUNS)?;
    if let Some(n) = host_notice(&base.host, &Host::here()) {
        println!("     notice: {STRESS_BASELINE}: {n}");
        return Ok(());
    }
    let measured: BTreeMap<String, StressBaseline> =
        runs.iter().map(|(k, (_, r))| (k.clone(), stress_medians(r))).collect();
    let mut c = compare(&gated(&base.configs), &gated(&measured), &stress_skipped(&runs));
    let mut retried = Vec::new();
    for v in c.verdicts.iter_mut().filter(|v| v.regressed()) {
        let first = v.change();
        // The noise guard: the best of more runs of that configuration.
        let Some((config, metric)) = v.name.rsplit_once('/') else { continue };
        let Some(&(cfg, _)) = runs.get(config) else { continue };
        for _ in 0..RETRIES {
            let r = stress_run(&cli, content, cfg, STRESS_SECONDS, false, false)?;
            let x = match metric {
                "tick_us_p50" => r.tick_us.p50,
                "peak_mib" => r.peak_mib.unwrap_or(0.0),
                _ => r.render.map_or(0.0, |x| x.frame_ms.p50),
            };
            if x > 0.0 {
                v.measured = v.measured.min(x);
            }
        }
        retried.push(retry_notice(v, first));
    }
    c.notices.extend(retried);
    if let Some(r) = runs.values().next().and_then(|(_, r)| r.first()).and_then(|r| r.render.as_ref()) {
        println!("     stress adapter: {}", r.adapter);
    }
    report("stress", &c, "", 1.0, verbose)
}

/// `cargo xtask stress`: every configuration for `seconds` of match time
/// (determinism checked), each report as it comes, then a summary.
pub fn stress_command(seconds: f32) -> anyhow::Result<bool> {
    let paths = crate::fetch::verify()?;
    let cli = olr_cli().map_err(anyhow::Error::msg)?;
    let mut done = Vec::new();
    for &cfg in STRESS {
        println!("== {} on {STRESS_LEVEL}, 32 chaos bots, pool {}, {seconds} s", cfg.0, cfg.1);
        match stress_run(&cli, &paths.content, cfg, seconds, true, true) {
            Ok(r) => done.push(r),
            Err(e) => {
                println!("FAIL {e}");
                return Ok(false);
            }
        }
    }
    println!(
        "\n{:<52} {:>20} {:>12} {:>5} {:>9} {:>6}",
        "configuration", "tick µs p50/p99/max", "projectiles", "fx", "frame ms", "MiB"
    );
    for r in &done {
        let frame = r.render.as_ref().map_or("-".to_string(), |x| format!("{:.2}", x.frame_ms.p50));
        let tick = format!("{:.0}/{:.0}/{:.0}", r.tick_us.p50, r.tick_us.p99, r.tick_us.max);
        let mib = r.peak_mib.unwrap_or(0.0);
        println!(
            "{:<52} {tick:>20} {:>12} {:>5} {frame:>9} {mib:>6.0}",
            r.name, r.projectiles_peak, r.fx_peak
        );
    }
    if let Some(a) = done.iter().find_map(|r| r.render.as_ref()) {
        println!("offscreen frames on {}", a.adapter);
    }
    Ok(true)
}

/// `cargo xtask bench`: both steps, or with `bless` the baselines written
/// and logged.
pub fn run(bless: Option<&str>, offline: bool) -> anyhow::Result<bool> {
    let paths = if offline { None } else { Some(crate::fetch::verify()?) };
    let envs: Vec<(&str, PathBuf)> = paths.iter().map(|p| ("OLR_CONTENT_DIR", p.content.clone())).collect();
    let content = paths.as_ref().map(|p| p.content.as_path());
    let Some(reason) = bless else {
        let mut ok = true;
        for (name, result) in [("bench", bench_step(&envs, true)), ("stress", stress_step(content, true))] {
            match result {
                Ok(()) => println!("ok   {name}"),
                Err(e) => {
                    println!("FAIL {name}\n{e}");
                    ok = false;
                }
            }
        }
        return Ok(ok);
    };
    anyhow::ensure!(!reason.trim().is_empty(), "--reason must say why the baselines change");
    let before = [BASELINES, STRESS_BASELINE].map(|p| std::fs::read(root().join(p)).ok());
    let host = Host::here();
    println!("measuring the benchmarks ({RUNS} runs) on {host:?}...");
    let mut copies = Copies::new().map_err(anyhow::Error::msg)?;
    let m = measure_benches(RUNS, &envs, &mut copies).map_err(anyhow::Error::msg)?;
    drop(copies);
    if let Some(n) = m.busy {
        println!("notice: {n}");
    }
    let benches = m.runs.iter().map(|(k, v)| (k.clone(), median(v))).collect();
    let old: Baselines = read_ron(BASELINES).unwrap_or_default();
    let new = blessed(&old, benches, host.clone());
    if !new.kept.is_empty() {
        println!("kept from before (not measured now): {:?}", new.kept);
    }
    write_ron(BASELINES, &new)?;
    if let Some(content) = content {
        println!("measuring the stress runs ({RUNS} runs)...");
        let cli = olr_cli().map_err(anyhow::Error::msg)?;
        let runs = measure_stress(&cli, content, RUNS).map_err(anyhow::Error::msg)?;
        let configs = runs.iter().map(|(k, (_, r))| (k.clone(), stress_medians(r))).collect();
        let note =
            "`full`'s short stress run: medians of 3 runs (tick µs and frame ms gated at +10 %, peak MiB at \
                    +25 %; the rest for a reader); valid on `host` only."
                .to_string();
        write_ron(STRESS_BASELINE, &StressBaselines { note, host, seconds: STRESS_SECONDS, configs })?;
    } else {
        println!("the stress baseline is kept as it was (no OLX content: --offline)");
    }
    let changed: Vec<String> = [BASELINES, STRESS_BASELINE]
        .iter()
        .zip(before)
        .filter(|(p, b)| std::fs::read(root().join(p)).ok() != *b)
        .map(|(p, _)| p.to_string())
        .collect();
    crate::bless::log_blessing("bench", reason, &changed)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIVAN: &str = "Timer precision: 10 ns
tick               fastest       │ slowest       │ median        │ mean          │ samples │ iters
├─ tick_32_worms   3.095 µs      │ 6.732 µs      │ 3.135 µs      │ 3.355 µs      │ 60      │ 60
╰─ tick_full_pool  219.5 µs      │ 378.8 µs      │ 226.1 µs      │ 239.2 µs      │ 60      │ 60
";

    #[test]
    fn divan_tables_parse() {
        let m = parse_divan(DIVAN, "olr-sim/tick");
        assert_eq!(m.len(), 2);
        assert!((m["olr-sim/tick/tick_32_worms"] - 3135.0).abs() < 1e-6);
        assert!((m["olr-sim/tick/tick_full_pool"] - 226_100.0).abs() < 1e-6);
        assert_eq!(parse_time("4.414 ms"), Some(4_414_000.0));
        assert_eq!(parse_time("12 ns"), Some(12.0));
        assert_eq!(parse_time("x"), None);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.0);
    }

    #[test]
    fn regressions_need_more_than_the_tolerance() {
        let base: BTreeMap<String, f64> = [("a".into(), 100.0), ("b".into(), 100.0)].into();
        let now: BTreeMap<String, f64> =
            [("a".into(), 109.0), ("b".into(), 111.0), ("new".into(), 5.0)].into();
        let c = compare(&base, &now, &BTreeSet::new());
        let bad: Vec<&str> = c.verdicts.iter().filter(|v| v.regressed()).map(|v| v.name.as_str()).collect();
        assert_eq!(bad, ["b"]);
        assert_eq!(c.notices.len(), 1, "no baseline yet: {:?}", c.notices);
        assert!(c.missing.is_empty());
        assert!(report("t", &c, "ns", 1.0, false).is_err());
    }

    /// A baselined measurement that nothing measured fails, unless its
    /// target said it skipped (no adapter, no content): then a notice.
    #[test]
    fn a_baseline_measured_by_nothing_fails_unless_skipped() {
        let base: BTreeMap<String, f64> = [
            ("olr-sim/tick/a".into(), 1.0),
            ("olr-render/frame/f".into(), 1.0),
            ("olr-cli/game/g".into(), 1.0),
        ]
        .into();
        let now: BTreeMap<String, f64> = [("olr-sim/tick/a".into(), 1.0)].into();
        let skipped: BTreeSet<String> = ["olr-render/frame".to_string()].into();
        let c = compare(&base, &now, &skipped);
        assert_eq!(c.missing, ["olr-cli/game/g"]);
        assert!(c.notices.iter().any(|n| n.starts_with("olr-render/frame/f")), "{:?}", c.notices);
        assert!(report("t", &c, "ns", 1.0, false).unwrap_err().contains("olr-cli/game/g"));
    }

    /// Peak memory has a wider tolerance than times.
    #[test]
    fn memory_has_its_own_tolerance() {
        assert_eq!(tolerance("x/tick_us_p50"), TOLERANCE);
        assert_eq!(tolerance("x/peak_mib"), MEMORY_TOLERANCE);
        let v = |name: &str, measured| Verdict { name: name.into(), baseline: 100.0, measured };
        assert!(v("c/tick_us_p50", 120.0).regressed());
        assert!(!v("c/peak_mib", 120.0).regressed());
        assert!(v("c/peak_mib", 126.0).regressed());
        let b = StressBaseline { tick_us_p50: 1.0, render_ms_p50: 2.0, peak_mib: 3.0, ..Default::default() };
        let g = gated(&[("c".to_string(), b)].into());
        assert_eq!(g.keys().collect::<Vec<_>>(), ["c/peak_mib", "c/render_ms_p50", "c/tick_us_p50"]);
    }

    /// A bless keeps the baselines it could not measure, and says so.
    #[test]
    fn bless_keeps_what_it_could_not_measure() {
        let old = Baselines {
            benches: [("olr-render/frame/f".into(), 5.0), ("olr-sim/tick/a".into(), 1.0)].into(),
            ..Baselines::default()
        };
        let new: BTreeMap<String, f64> = [("olr-sim/tick/a".into(), 2.0)].into();
        let b = blessed(&old, new, Host::default());
        assert_eq!(b.benches["olr-sim/tick/a"], 2.0);
        assert_eq!(b.benches["olr-render/frame/f"], 5.0, "kept");
        assert_eq!(b.kept, ["olr-render/frame/f"]);
        let again = blessed(&b, [("olr-render/frame/f".into(), 6.0)].into(), Host::default());
        assert_eq!(again.kept, ["olr-sim/tick/a"], "the frame measured again is no longer kept");
        assert_eq!(again.benches["olr-render/frame/f"], 6.0);
    }

    /// The bench target's executable is read from cargo's JSON messages.
    #[test]
    fn bench_executables_come_from_cargos_messages() {
        let json = r#"{"reason":"compiler-artifact","package_id":"path+file:///w/crates/olr-sim#0.1.0","manifest_path":"/w/crates/olr-sim/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"olr_sim","src_path":"/w/crates/olr-sim/src/lib.rs","edition":"2024","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"3","debuginfo":0,"debug_assertions":false,"overflow_checks":false,"test":false},"features":[],"filenames":["/w/target/release/deps/libolr_sim-1.rlib"],"executable":null,"fresh":true}
{"reason":"compiler-artifact","package_id":"path+file:///w/crates/olr-sim#0.1.0","manifest_path":"/w/crates/olr-sim/Cargo.toml","target":{"kind":["bench"],"crate_types":["bin"],"name":"tick","src_path":"/w/crates/olr-sim/benches/tick.rs","edition":"2024","doc":false,"doctest":false,"test":false},"profile":{"opt_level":"3","debuginfo":0,"debug_assertions":false,"overflow_checks":false,"test":true},"features":[],"filenames":["/w/target/release/deps/tick-e23f"],"executable":"/w/target/release/deps/tick-e23f","fresh":false}
{"reason":"build-finished","success":true}
"#;
        let (exe, dir) = bench_executable(json.as_bytes(), "tick").expect("the bench artifact");
        assert_eq!(exe, Path::new("/w/target/release/deps/tick-e23f"));
        assert_eq!(dir, Path::new("/w/crates/olr-sim"));
        assert_eq!(bench_executable(json.as_bytes(), "frame"), None);
        assert_eq!(bench_executable(b"not json", "tick"), None);
    }

    /// A retry runs the benchmark's target: `<package>/<bench target>`.
    #[test]
    fn retries_run_the_benchmarks_target() {
        assert_eq!(split_name("olr-cli/game/bots_8_tick"), ("olr-cli", "game"));
    }

    /// The machine's load from two `/proc/stat` readings, in busy CPUs.
    #[test]
    fn busy_cpus_come_from_proc_stat() {
        let a = "cpu  100 0 50 1000 10 0 5 0 0 0\ncpu0 1 2 3 4 5 6 7 8 9 10\n";
        // 12 CPUs, 1200 jiffies pass: 300 busy (user, system, softirq,
        // steal), 900 idle or waiting: 3 CPUs busy.
        let b = "cpu  250 0 100 1800 110 0 25 80 0 0\n";
        assert_eq!(cpu_jiffies(a), Some((155, 1165)));
        assert_eq!(cpu_jiffies(b), Some((455, 2365)));
        assert!((busy_cpus(cpu_jiffies(a).unwrap(), cpu_jiffies(b).unwrap(), 12) - 3.0).abs() < 1e-9);
        assert_eq!(busy_cpus((5, 10), (5, 10), 12), 0.0, "no time passed");
        assert_eq!(cpu_jiffies("intr 1 2 3"), None);
        assert_eq!(cpu_jiffies("cpu  1 x 3"), None);
        assert!(settle_notice(Settle::Busy(4.25), "b").unwrap().contains("4.2 CPUs in use"));
        assert_eq!(settle_notice(Settle::Idle(SETTLE_SAMPLE), "b"), None);
    }

    /// A benchmark over its tolerance passes when a later window is within
    /// it (noise only slows), and fails when every window is over (a real
    /// regression of 15 % stays over 10 % in each).
    #[test]
    fn the_best_retry_window_counts() {
        let idle = Settle::Idle(SETTLE_SAMPLE);
        let v = |measured| Verdict { name: "olr-sim/tick/t".into(), baseline: 100.0, measured };
        let noisy = best_window(114.0, &[(104.0, idle), (112.0, idle)]);
        assert!(!v(noisy).regressed(), "a burst of other work: {noisy}");
        let real = best_window(118.0, &[(115.0, idle), (116.0, Settle::Busy(3.0))]);
        assert!(v(real).regressed(), "a real regression: {real}");
        assert_eq!(best_window(120.0, &[]), 120.0);
        let n = window_notice(&v(104.0), 114.0, &[(104.0, idle), (112.0, Settle::Busy(2.5))]);
        assert_eq!(
            n,
            "olr-sim/tick/t: +14.0 %, then +4.0 %, +12.0 % (busy: 2.5 CPUs) in 2 later window(s) of 3 runs"
        );
    }

    /// Baselines from another machine are a notice, not a failure.
    #[test]
    fn baselines_from_another_machine_only_notice() {
        let here = Host { cpus: 12, cpu: "X".into(), adapter: "llvmpipe".into() };
        assert_eq!(host_notice(&here, &here), None);
        let other = Host { cpus: 8, ..here.clone() };
        assert!(host_notice(&other, &here).unwrap().contains("another machine"));
        assert!(host_notice(&Host::default(), &here).is_some(), "a file without a host");
    }
}
