//! Benchmarks with stored baselines (spec §7, §21 G8): `cargo xtask bench`
//! and `full`'s `bench` step.
//!
//! - **divan benchmarks** ([`BENCHES`]: `rl-sim`'s tick with the pool
//!   full and 32 worms, `rl-render`'s offscreen frame at 1080p and 1440p,
//!   `rl-cli`'s 8-bot tick and draw-list build on the imported content),
//!   run in the bench (release) profile [`RUNS`] times; each one's median
//!   of the runs' medians is compared with `fixtures/bench/baselines.ron`.
//! - **the short stress run** ([`STRESS`]: both mods × OLX's pool and a
//!   raised one, [`STRESS_SECONDS`] of a 32-chaos-bot match each, through
//!   `rl-cli stress`, determinism checked on the first run); the median
//!   tick time and offscreen frame time against `fixtures/bench/stress.ron`.
//!
//! **Noise guard.** A measurement fails only above the baseline by more
//! than [`TOLERANCE`] (10 %), on the median of [`RUNS`] runs; one that does
//! is measured [`RETRIES`] more times and passes if the best run is within
//! the tolerance (this machine is shared: WSL2 with noisy neighbours). A
//! measurement without a baseline, or a baseline not measured (no adapter,
//! no content), is a notice, never a failure. `cargo xtask bench --bless
//! --reason "<why>"` measures everything and writes both files, logged in
//! `fixtures/blessings.log`. Baselines are this machine's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::root;

/// The divan benchmarks as `(package, bench target)`.
pub const BENCHES: &[(&str, &str)] = &[("rl-sim", "tick"), ("rl-render", "frame"), ("rl-cli", "game")];

pub const BASELINES: &str = "fixtures/bench/baselines.ron";
pub const STRESS_BASELINE: &str = "fixtures/bench/stress.ron";

/// Runs whose median is compared.
pub const RUNS: usize = 3;
/// More runs for a measurement over the tolerance; the best counts.
pub const RETRIES: usize = 2;
/// How much slower than its baseline a measurement may be.
pub const TOLERANCE: f64 = 0.10;

/// The short stress run's configurations: `(mod, pool)` on Castle Strike
/// with 32 chaos bots.
pub const STRESS: &[(&str, usize)] =
    &[("olx-mw-1-0", 3000), ("olx-mw-1-0", 20_000), ("olx-classic", 3000), ("olx-classic", 20_000)];
pub const STRESS_LEVEL: &str = "olx-level-castlestrike";
/// Seconds of match time a configuration plays in `full`.
pub const STRESS_SECONDS: f32 = 20.0;

/// `fixtures/bench/baselines.ron`: each benchmark's median, in ns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Baselines {
    pub note: String,
    /// `<package>/<bench target>/<function>` to ns.
    pub benches: BTreeMap<String, f64>,
}

/// `fixtures/bench/stress.ron`: each configuration's medians.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StressBaselines {
    pub note: String,
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
    pub tick_us_p99: f64,
    pub projectiles_peak: usize,
    pub fx_peak: usize,
    pub events_max: usize,
    pub peak_mib: f64,
}

/// What `rl-cli stress --out` writes, the parts read here (`rl_cli::stress::Report`).
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

/// One run of one bench target (only `filter`'s function when given):
/// its medians.
fn run_bench(
    package: &str,
    bench: &str,
    filter: Option<&str>,
    envs: &[(&str, PathBuf)],
) -> Result<BTreeMap<String, f64>, String> {
    let mut c = cargo();
    c.args(["bench", "--quiet", "-p", package, "--bench", bench, "--"]);
    if let Some(f) = filter {
        c.args(["--exact", f]);
    }
    for (k, v) in envs {
        c.env(k, v);
    }
    let out = c.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(format!("{text}{}", String::from_utf8_lossy(&out.stderr)));
    }
    if let Some(skip) = text.lines().find(|l| l.starts_with("skip ")) {
        println!("     {skip}");
    }
    Ok(parse_divan(&text, &format!("{package}/{bench}")))
}

/// Every benchmark `runs` times: each one's measurements.
fn measure_benches(runs: usize, envs: &[(&str, PathBuf)]) -> Result<BTreeMap<String, Vec<f64>>, String> {
    let mut all: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for _ in 0..runs {
        for (package, bench) in BENCHES {
            for (name, ns) in run_bench(package, bench, None, envs)? {
                all.entry(name).or_default().push(ns);
            }
        }
    }
    Ok(all)
}

/// `rl-cli` in the release profile, built.
fn rl_cli() -> Result<PathBuf, String> {
    let out = cargo()
        .args(["build", "--quiet", "--release", "-p", "rl-cli"])
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(root().join("target/release/rl-cli"))
}

/// One stress configuration through `rl-cli stress`: its report. With
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
        c.status().map_err(|e| format!("cannot run rl-cli: {e}"))?.success()
    } else {
        let out = c.output().map_err(|e| format!("cannot run rl-cli: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "rl-cli stress {mod_id} pool {pool} failed:\n{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        true
    };
    if !ok {
        return Err(format!("rl-cli stress {mod_id} pool {pool} failed"));
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

/// The gated medians of `configs`, by `<configuration>/<measurement>`.
fn gated(configs: &BTreeMap<String, StressBaseline>) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    for (k, b) in configs {
        m.insert(format!("{k}/tick_us_p50"), b.tick_us_p50);
        if b.render_ms_p50 > 0.0 {
            m.insert(format!("{k}/render_ms_p50"), b.render_ms_p50);
        }
    }
    m
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
        self.change() > TOLERANCE
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

/// The verdicts of `measured` (each a median) against `baseline`, and the
/// notices: measurements without a baseline, baselines not measured.
pub fn compare(
    baseline: &BTreeMap<String, f64>,
    measured: &BTreeMap<String, f64>,
) -> (Vec<Verdict>, Vec<String>) {
    let mut verdicts = Vec::new();
    let mut notices = Vec::new();
    for (name, &b) in baseline {
        match measured.get(name) {
            Some(&m) => verdicts.push(Verdict { name: name.clone(), baseline: b, measured: m }),
            None => notices.push(format!("{name}: not measured here (no adapter or content?)")),
        }
    }
    for name in measured.keys().filter(|n| !baseline.contains_key(*n)) {
        notices.push(format!("{name}: no baseline (`cargo xtask bench --bless --reason ...`)"));
    }
    (verdicts, notices)
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

/// `full`'s `bench` step: the divan benchmarks against their baselines.
pub fn bench_step(envs: &[(&str, PathBuf)], verbose: bool) -> Result<(), String> {
    let base: Baselines = read_ron(BASELINES)?;
    let runs = measure_benches(RUNS, envs)?;
    let mut medians: BTreeMap<String, f64> = runs.iter().map(|(k, v)| (k.clone(), median(v))).collect();
    let (mut verdicts, mut notices) = compare(&base.benches, &medians);
    for v in verdicts.iter_mut().filter(|v| v.regressed()) {
        let first = v.change();
        // The noise guard: the best of more runs.
        let (package, bench) = split_name(&v.name);
        let function = v.name.rsplit('/').next().unwrap_or_default();
        for _ in 0..RETRIES {
            let again = run_bench(package, bench, Some(function), envs)?;
            if let Some(&ns) = again.get(&v.name) {
                v.measured = v.measured.min(ns);
            }
        }
        medians.insert(v.name.clone(), v.measured);
        notices.push(format!(
            "{}: {:+.1} %, then {:+.1} % at best of {RETRIES} more runs",
            v.name,
            first * 100.0,
            v.change() * 100.0
        ));
    }
    report("bench", &verdicts, &notices, "µs", 1e3, verbose)
}

fn split_name(name: &str) -> (&str, &str) {
    let mut parts = name.split('/');
    (parts.next().unwrap_or_default(), parts.next().unwrap_or_default())
}

/// Prints the verdicts (every one when `verbose`, else a summary line)
/// and the notices; `Err` lists them all with the regressions.
fn report(
    what: &str,
    verdicts: &[Verdict],
    notices: &[String],
    unit: &str,
    scale: f64,
    verbose: bool,
) -> Result<(), String> {
    let table: Vec<String> = verdicts.iter().map(|v| format!("     {}", v.line(unit, scale))).collect();
    if verbose {
        println!("{}", table.join("\n"));
    }
    for n in notices {
        println!("     notice: {n}");
    }
    let bad: Vec<String> = verdicts.iter().filter(|v| v.regressed()).map(|v| v.line(unit, scale)).collect();
    if !bad.is_empty() {
        let table = if verbose { String::new() } else { table.join("\n") + "\n" };
        return Err(format!(
            "{table}over the baseline by more than {:.0} % (median of {RUNS} runs, then the best of {RETRIES} more):\n{}",
            TOLERANCE * 100.0,
            bad.join("\n")
        ));
    }
    if !verbose && let Some(worst) = verdicts.iter().max_by(|a, b| a.change().total_cmp(&b.change())) {
        println!(
            "     {what}: {} within {:.0} % of the baselines (worst {:+.1} %, {})",
            verdicts.len(),
            TOLERANCE * 100.0,
            worst.change() * 100.0,
            worst.name
        );
    }
    Ok(())
}

/// `full`'s `stress` step: the short stress run against its baseline (the
/// tick and frame medians), every configuration deterministic.
pub fn stress_step(content: Option<&Path>, verbose: bool) -> Result<(), String> {
    let Some(content) = content else {
        println!("skip stress (no OLX content: --offline)");
        return Ok(());
    };
    let base: StressBaselines = read_ron(STRESS_BASELINE)?;
    let cli = rl_cli()?;
    let runs = measure_stress(&cli, content, RUNS)?;
    let measured: BTreeMap<String, StressBaseline> =
        runs.iter().map(|(k, (_, r))| (k.clone(), stress_medians(r))).collect();
    let (mut verdicts, mut notices) = compare(&gated(&base.configs), &gated(&measured));
    for v in verdicts.iter_mut().filter(|v| v.regressed()) {
        let first = v.change();
        // The noise guard: the best of more runs of that configuration.
        let Some((config, metric)) = v.name.rsplit_once('/') else { continue };
        let Some(&(cfg, _)) = runs.get(config) else { continue };
        for _ in 0..RETRIES {
            let r = stress_run(&cli, content, cfg, STRESS_SECONDS, false, false)?;
            let x = match metric {
                "tick_us_p50" => r.tick_us.p50,
                _ => r.render.map_or(0.0, |x| x.frame_ms.p50),
            };
            if x > 0.0 {
                v.measured = v.measured.min(x);
            }
        }
        notices.push(format!(
            "{}: {:+.1} %, then {:+.1} % at best of {RETRIES} more runs",
            v.name,
            first * 100.0,
            v.change() * 100.0
        ));
    }
    if let Some(r) = runs.values().next().and_then(|(_, r)| r.first()).and_then(|r| r.render.as_ref()) {
        println!("     stress adapter: {}", r.adapter);
    }
    report("stress", &verdicts, &notices, "", 1.0, verbose)
}

/// `cargo xtask stress`: every configuration for `seconds` of match time
/// (determinism checked), each report as it comes, then a summary.
pub fn stress_command(seconds: f32) -> anyhow::Result<bool> {
    let paths = crate::fetch::run(true)?;
    let cli = rl_cli().map_err(anyhow::Error::msg)?;
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
    let paths = if offline { None } else { Some(crate::fetch::run(true)?) };
    let envs: Vec<(&str, PathBuf)> = paths.iter().map(|p| ("RL_CONTENT_DIR", p.content.clone())).collect();
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
    println!("measuring the benchmarks ({RUNS} runs)...");
    let runs = measure_benches(RUNS, &envs).map_err(anyhow::Error::msg)?;
    let benches = runs.iter().map(|(k, v)| (k.clone(), median(v))).collect();
    let note =
        "Medians in ns (`cargo xtask bench --bless --reason`); this machine's: WSL2, 12 cores, lavapipe.";
    write_ron(BASELINES, &Baselines { note: note.into(), benches })?;
    if let Some(content) = content {
        println!("measuring the stress runs ({RUNS} runs)...");
        let cli = rl_cli().map_err(anyhow::Error::msg)?;
        let runs = measure_stress(&cli, content, RUNS).map_err(anyhow::Error::msg)?;
        let configs = runs.iter().map(|(k, (_, r))| (k.clone(), stress_medians(r))).collect();
        let note = "`full`'s short stress run: medians of 3 runs (tick µs, frame ms gated at +10 %; the rest for a reader)."
            .to_string();
        write_ron(STRESS_BASELINE, &StressBaselines { note, seconds: STRESS_SECONDS, configs })?;
    } else {
        println!("skip the stress baseline (no OLX content: --offline)");
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
        let m = parse_divan(DIVAN, "rl-sim/tick");
        assert_eq!(m.len(), 2);
        assert!((m["rl-sim/tick/tick_32_worms"] - 3135.0).abs() < 1e-6);
        assert!((m["rl-sim/tick/tick_full_pool"] - 226_100.0).abs() < 1e-6);
        assert_eq!(parse_time("4.414 ms"), Some(4_414_000.0));
        assert_eq!(parse_time("12 ns"), Some(12.0));
        assert_eq!(parse_time("x"), None);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.0);
    }

    #[test]
    fn regressions_need_more_than_the_tolerance() {
        let base: BTreeMap<String, f64> =
            [("a".into(), 100.0), ("b".into(), 100.0), ("gone".into(), 1.0)].into();
        let now: BTreeMap<String, f64> =
            [("a".into(), 109.0), ("b".into(), 111.0), ("new".into(), 5.0)].into();
        let (v, notices) = compare(&base, &now);
        let bad: Vec<&str> = v.iter().filter(|v| v.regressed()).map(|v| v.name.as_str()).collect();
        assert_eq!(bad, ["b"]);
        assert_eq!(notices.len(), 2, "{notices:?}");
        assert!(report("t", &v, &notices, "ns", 1.0, false).is_err());
    }
}
