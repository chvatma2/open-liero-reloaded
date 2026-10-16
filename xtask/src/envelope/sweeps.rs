//! The headless sweeps (spec §4): each point is `olr-cli stress` in its own
//! process (its peak memory is its own), `runs` times, from the reference
//! configuration (ruling P1) with one axis changed. The map and art points'
//! first runs record their replays for the GPU scenes (ruling P4). Every
//! run is `--no-verify` (ruling P9: verifying the art-8 point would hold
//! about 7 GiB); the map-size and art-scale axes play Deathmatch bots
//! (`--personality dm`, ruling T1-A: chaos bots never use the area cache,
//! whose cost those axes measure), the others chaos bots.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::results::{self, Point, Rollback, RollbackOut, StressOut, Sweep};
use super::{Axis, Plan};
use crate::root;

pub const REPLAYS: &str = "target/envelope/replays";
const REF_MOD: &str = "olx-classic";
const REF_LEVEL: &str = "olx-level-castlestrike";
const POOL_MOD: &str = "olx-mw-1-0";
const REF_WORMS: u32 = 16;
const REF_POOL: u32 = 3000;
const REF_FX: u32 = 1024;
/// Ruling P13: past the 2 GiB budget, below this machine's memory.
const MEMORY_CEILING_MIB: u32 = 16_384;

/// `<axis>-<label>.olrp`.
pub fn replay_name(axis: &str, label: &str) -> String {
    format!("{axis}-{label}.olrp")
}

/// Builds `packages` in the release profile.
pub fn build_release(packages: &[&str]) -> Result<(), String> {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root()).args(["build", "--quiet", "--release"]);
    for p in packages {
        c.args(["-p", p]);
    }
    let out = c.output().map_err(|e| format!("cannot run cargo: {e}"))?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).into_owned()) }
}

/// The bots of the reference configuration: they fire everything.
const CHAOS: &str = "chaos";
/// OLX's bot, finding routes through the area cache (ruling T1-A).
const DEATHMATCH: &str = "dm";

/// One point's configuration.
#[derive(Clone, Debug)]
pub struct Value {
    pub label: String,
    pub n: f64,
    pub mod_id: &'static str,
    pub level: String,
    /// `olr-cli stress --personality`.
    pub personality: &'static str,
    pub worms: u32,
    pub pool: u32,
    pub fx: u32,
}

fn reference(label: String, n: f64) -> Value {
    Value {
        label,
        n,
        mod_id: REF_MOD,
        level: REF_LEVEL.into(),
        personality: CHAOS,
        worms: REF_WORMS,
        pool: REF_POOL,
        fx: REF_FX,
    }
}

/// The points of `axis`.
pub fn values(axis: Axis, plan: &Plan) -> Vec<Value> {
    let f = |x: u32| f64::from(x);
    match axis {
        Axis::Worms => {
            plan.worms.iter().map(|&w| Value { worms: w, ..reference(w.to_string(), f(w)) }).collect()
        }
        Axis::Pool => plan
            .pools
            .iter()
            .map(|&p| Value { pool: p, mod_id: POOL_MOD, ..reference(p.to_string(), f(p)) })
            .collect(),
        Axis::Fx => plan.fx.iter().map(|&x| Value { fx: x, ..reference(x.to_string(), f(x)) }).collect(),
        Axis::Map => plan
            .maps
            .iter()
            .map(|&(w, h)| Value {
                level: format!("gen-{w}x{h}-a1-s1"),
                personality: DEATHMATCH,
                ..reference(format!("{w}x{h}"), f(w) * f(h))
            })
            .collect(),
        Axis::Art => plan
            .arts
            .iter()
            .map(|&a| Value {
                level: format!("gen-2048x2048-a{a}-s1"),
                personality: DEATHMATCH,
                ..reference(format!("a{a}"), f(a))
            })
            .collect(),
        Axis::Rollback | Axis::Gpu | Axis::Olx => Vec::new(),
    }
}

fn stress(
    cli: &Path,
    content: &Path,
    v: &Value,
    seconds: f32,
    record: Option<&Path>,
) -> Result<StressOut, String> {
    let out = root().join("target/envelope/stress.ron");
    let _ = std::fs::remove_file(&out);
    let mut c = Command::new(cli);
    c.current_dir(root())
        .args(["stress", "--mod", v.mod_id, "--level", &v.level])
        .args(["--personality", v.personality])
        .args(["--worms", &v.worms.to_string(), "--pool", &v.pool.to_string(), "--fx-cap", &v.fx.to_string()])
        .args(["--seconds", &seconds.to_string(), "--seed", "1", "--render-every", "0", "--build-every", "1"])
        .args(["--no-verify", "--memory-ceiling-mib", &MEMORY_CEILING_MIB.to_string()])
        .arg("--content")
        .arg(content)
        .arg("--out")
        .arg(&out);
    if let Some(r) = record {
        c.arg("--record").arg(r);
    }
    let o = c.output().map_err(|e| format!("cannot run olr-cli: {e}"))?;
    if !o.status.success() {
        return Err(failure("olr-cli stress", &o));
    }
    let text = std::fs::read_to_string(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    ron::from_str(&text).map_err(|e| format!("{}: {e}", out.display()))
}

/// One line for a tool that failed: its exit status (a signal, such as
/// the OOM killer's 9, has no output of its own) and its output's last line.
pub fn failure(tool: &str, o: &std::process::Output) -> String {
    let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    match text.lines().rfind(|l| !l.trim().is_empty()) {
        Some(last) => format!("{tool} ({}): {}", o.status, last.trim()),
        None => format!("{tool} ({}), no output", o.status),
    }
}

/// Every point of `axis`; a failed run makes a failed point (Review Focus
/// 1) and the sweep goes on.
pub fn run_axis(cli: &Path, content: &Path, axis: Axis, plan: &Plan) -> Sweep {
    let vals = values(axis, plan);
    let mut points: Vec<Point> = Vec::new();
    for v in &vals {
        let load = results::load_average().unwrap_or(0.0);
        if load > 1.5 {
            println!("  notice: load average {load:.2} before {} {}: is the PC idle?", axis.name(), v.label);
        }
        let mut runs = Vec::new();
        let mut failure = None;
        for run in 0..plan.runs {
            let record: Option<PathBuf> = (run == 0 && matches!(axis, Axis::Map | Axis::Art))
                .then(|| root().join(REPLAYS).join(replay_name(axis.name(), &v.label)));
            if let Some(dir) = record.as_ref().and_then(|r| r.parent()) {
                let _ = std::fs::create_dir_all(dir);
            }
            match stress(cli, content, v, plan.seconds, record.as_deref()) {
                Ok(r) => runs.push(r),
                Err(why) => {
                    failure = Some(why);
                    break;
                }
            }
        }
        let p = match failure {
            Some(why) => results::failed_point(&v.label, v.n, &why, load),
            None => results::point(&v.label, v.n, &runs, load),
        };
        println!("  {} {}: {}", axis.name(), v.label, p.line());
        points.push(p);
    }
    let first = vals.first();
    let bots = match first.map_or(CHAOS, |v| v.personality) {
        DEATHMATCH => "Deathmatch bots (OLX's bot: routes through the area cache)",
        _ => "chaos bots",
    };
    Sweep {
        axis: axis.name().into(),
        mod_id: first.map_or(REF_MOD, |v| v.mod_id).into(),
        level: first.map_or(REF_LEVEL.into(), |v| v.level.clone()),
        what: format!(
            "{} varies; otherwise {REF_WORMS} {bots} of mixed difficulties, pool {REF_POOL}, effects {REF_FX}, \
             {} s, seed 1, LoadingTime 0, bonuses every 5 s",
            axis.name(),
            plan.seconds
        ),
        points,
        ..Sweep::default()
    }
}

/// One rollback point: the level, the depth and the total's budget (µs).
pub struct RollbackSpec {
    pub label: &'static str,
    pub level: &'static str,
    pub depth: u32,
    pub budget_us: Option<f64>,
}

/// Spec §3.3 and ruling T7-A: the reference at the depth of the plan (12)
/// and at `MAX_PREDICTION` (28), and the largest map at 12.
pub const ROLLBACK_POINTS: [RollbackSpec; 3] = [
    RollbackSpec { label: "castlestrike-12", level: REF_LEVEL, depth: 12, budget_us: Some(2000.0) },
    RollbackSpec { label: "castlestrike-28", level: REF_LEVEL, depth: 28, budget_us: None },
    RollbackSpec { label: "4096x4096-12", level: "gen-4096x4096-a1-s1", depth: 12, budget_us: Some(4000.0) },
];

fn rollback_cost(
    cli: &Path,
    content: &Path,
    p: &RollbackSpec,
    samples: usize,
) -> Result<RollbackOut, String> {
    let out = root().join("target/envelope/rollback.ron");
    let _ = std::fs::remove_file(&out);
    let o = Command::new(cli)
        .current_dir(root())
        .args(["rollback-cost", "--mod", REF_MOD, "--level", p.level])
        .args(["--worms", &REF_WORMS.to_string(), "--depth", &p.depth.to_string()])
        .args(["--samples", &samples.to_string()])
        .arg("--content")
        .arg(content)
        .arg("--out")
        .arg(&out)
        .output()
        .map_err(|e| format!("cannot run olr-cli: {e}"))?;
    if !o.status.success() {
        return Err(failure("olr-cli rollback-cost", &o));
    }
    let text = std::fs::read_to_string(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    ron::from_str(&text).map_err(|e| format!("{}: {e}", out.display()))
}

/// The rollback axis: each point `runs` times; a failed run is a failed point.
pub fn run_rollback(cli: &Path, content: &Path, plan: &Plan) -> Rollback {
    let mut points = Vec::new();
    for p in &ROLLBACK_POINTS {
        let load = results::load_average().unwrap_or(0.0);
        if load > 1.5 {
            println!("  notice: load average {load:.2} before rollback {}: is the PC idle?", p.label);
        }
        let mut runs = Vec::new();
        let mut failed = None;
        for _ in 0..plan.runs {
            match rollback_cost(cli, content, p, plan.rollback_samples) {
                Ok(r) => runs.push(r),
                Err(why) => {
                    failed = Some(why);
                    break;
                }
            }
        }
        let point = match failed {
            Some(why) => results::failed_rollback_point(p.label, p.budget_us, &why),
            None => results::rollback_point(p.label, p.budget_us, &runs),
        };
        println!("  rollback {}: {}", p.label, point.verdict_line());
        points.push(point);
    }
    Rollback {
        what: format!(
            "Restoring the world and re-simulating `depth` ticks (the bots' keys recorded, not thought again), \
             {REF_WORMS} chaos bots of mixed difficulties, Classic, pool {REF_POOL}, {} samples a run, seed 1.",
            plan.rollback_samples
        ),
        points,
        ..Rollback::default()
    }
}
