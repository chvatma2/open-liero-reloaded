//! `cargo xtask net-feel`: the feel numbers of online play (networking
//! spec §9.2), measured by `rl-cli net-sim --feel` from seeded runs in
//! virtual time (the same on every host), gated against
//! `fixtures/net/feel.ron`.
//!
//! Absolute rules (always fail): no player stalls in `lan`, `good`,
//! `typical` or `bad`; the 32-worm client receives at most [`DOWN_BUDGET`]
//! (spec §3.1). Regression rules: a metric worse than its baseline by more
//! than [`TOLERANCE`] and more than its floor fails; one better by as much
//! is a notice suggesting a re-bless.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::root;

/// Where `rl-cli net-sim --feel` writes the measurement.
pub const REPORT: &str = "target/net/feel.ron";
/// The committed baseline.
pub const BASELINE: &str = "fixtures/net/feel.ron";
/// The baseline's note.
pub const NOTE: &str = "Seeded, virtual-time feel numbers (plan 9); host-independent.";
/// Spec §3.1: at most 24 KB/s received per client at 32 players, 48 bytes
/// of overhead per datagram included.
pub const DOWN_BUDGET: u32 = 24_576;
/// No player may stall on these profiles (spec §9.2).
pub const NO_STALLS: [&str; 4] = ["lan", "good", "typical", "bad"];
/// A metric fails when worse than its baseline by more than this share (and its floor).
pub const TOLERANCE: f64 = 0.10;

/// `rl_cli::feel::ProfileFeel`, read strictly: a renamed field fails
/// instead of reading as 0.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileFeel {
    pub profile: String,
    pub rollback_p50: u32,
    pub rollback_p99: u32,
    pub correction_px_p99: f32,
    pub own_corrections_per_min: f32,
    pub stalls: u64,
    pub down_bytes_per_s: u32,
    pub up_bytes_per_s: u32,
}

/// `rl_cli::feel::FeelReport`, read strictly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeelReport {
    pub note: String,
    pub profiles: Vec<ProfileFeel>,
    pub bandwidth32: ProfileFeel,
}

/// The verdict on one profile.
#[derive(Debug, Default)]
pub struct Gate {
    pub failures: Vec<String>,
    pub notices: Vec<String>,
}

/// The gated metrics: name, value, floor, unit.
fn metrics(f: &ProfileFeel) -> [(&'static str, f64, f64, &'static str); 6] {
    [
        ("rollback p50", f64::from(f.rollback_p50), 1.0, " ticks"),
        ("rollback p99", f64::from(f.rollback_p99), 1.0, " ticks"),
        ("correction p99", f64::from(f.correction_px_p99), 1.0, " px"),
        ("own corrections", f64::from(f.own_corrections_per_min), 0.5, "/min"),
        ("down", f64::from(f.down_bytes_per_s), 256.0, " B/s"),
        ("up", f64::from(f.up_bytes_per_s), 256.0, " B/s"),
    ]
}

/// The absolute stall rule and the regression rules for one profile.
pub fn compare(base: &ProfileFeel, now: &ProfileFeel) -> Gate {
    let mut g = Gate::default();
    if NO_STALLS.contains(&now.profile.as_str()) && now.stalls > 0 {
        g.failures.push(format!("stalls {} (no player may stall up to bad)", now.stalls));
    }
    for ((name, b, floor, unit), (_, n, _, _)) in metrics(base).into_iter().zip(metrics(now)) {
        let margin = (b * TOLERANCE).max(floor);
        if n - b > margin {
            g.failures.push(format!("{name} {n:.1}{unit}, baseline {b:.1}{unit}"));
        } else if b - n > margin {
            g.notices
                .push(format!("{name} improved to {n:.1}{unit} from {b:.1}{unit}: re-bless to lock it in"));
        }
    }
    g
}

/// The bandwidth rule (spec §3.1) on the 32-worm run.
pub fn bandwidth(f: &ProfileFeel) -> Option<String> {
    (f.down_bytes_per_s > DOWN_BUDGET)
        .then(|| format!("down {} B/s, over the budget of {DOWN_BUDGET} B/s (spec §3.1)", f.down_bytes_per_s))
}

/// Every profile's verdict (and `bandwidth32`'s), against `base` if there is one.
pub fn gate(base: Option<&FeelReport>, now: &FeelReport) -> Vec<(String, Gate)> {
    let mut out = Vec::new();
    for p in &now.profiles {
        let b = base.and_then(|b| b.profiles.iter().find(|x| x.profile == p.profile));
        let mut g = compare(b.unwrap_or(p), p);
        if base.is_some() && b.is_none() {
            g.notices.push("no baseline".into());
        }
        out.push((p.profile.clone(), g));
    }
    if let Some(b) = base {
        for missing in b.profiles.iter().filter(|x| !now.profiles.iter().any(|p| p.profile == x.profile)) {
            let g = Gate { failures: vec!["not measured".into()], notices: vec![] };
            out.push((missing.profile.clone(), g));
        }
    }
    let mut g = compare(base.map_or(&now.bandwidth32, |b| &b.bandwidth32), &now.bandwidth32);
    g.failures.extend(bandwidth(&now.bandwidth32));
    out.push(("bandwidth32".into(), g));
    out
}

/// `ok   <profile>` or `FAIL <profile>: <why>` lines, notices, and whether all passed.
fn verdict_lines(results: &[(String, Gate)]) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut ok = true;
    for (name, g) in results {
        if g.failures.is_empty() {
            lines.push(format!("ok   {name}"));
        } else {
            ok = false;
            lines.push(format!("FAIL {name}: {}", g.failures.join("; ")));
        }
        lines.extend(g.notices.iter().map(|n| format!("     notice: {name}: {n}")));
    }
    (lines, ok)
}

fn read_report(path: &Path) -> Result<FeelReport, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    ron::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Builds `rl-cli` in release and measures: the report and what it printed.
fn measure(content: Option<&Path>) -> Result<(FeelReport, String), String> {
    let cli = crate::bench::rl_cli()?;
    let out_file = root().join(REPORT);
    let mut c = Command::new(&cli);
    c.args(["net-sim", "--feel"]).arg(&out_file).current_dir(root());
    if let Some(dir) = content {
        c.arg("--content").arg(dir);
    }
    let out = c.output().map_err(|e| format!("cannot run {}: {e}", cli.display()))?;
    let printed = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        return Err(format!("{printed}{}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok((read_report(&out_file)?, printed))
}

/// The baseline, `None` when there is none yet.
fn baseline() -> Result<Option<FeelReport>, String> {
    let path = root().join(BASELINE);
    if !path.exists() {
        return Ok(None);
    }
    read_report(&path).map(Some)
}

/// `full`'s `net-feel` step: quiet but for notices, the lines on failure.
pub fn feel_step(content: Option<&Path>) -> Result<(), String> {
    let (now, printed) = measure(content)?;
    let base = baseline()?;
    let (lines, ok) = verdict_lines(&gate(base.as_ref(), &now));
    let mut lines = lines;
    if base.is_none() {
        lines.push(format!(
            "     notice: no baseline in {BASELINE}: cargo xtask net-feel --bless --reason \"<why>\""
        ));
    }
    if ok {
        for l in lines.iter().filter(|l| l.starts_with("     notice")) {
            println!("{l}");
        }
        Ok(())
    } else {
        Err(format!("{printed}{}", lines.join("\n")))
    }
}

/// `cargo xtask net-feel [--bless --reason R]`: measures, prints the
/// numbers and the verdicts; with a reason, writes the baseline (when the
/// absolute rules pass) and logs it in `fixtures/blessings.log`.
pub fn run(bless: Option<&str>) -> anyhow::Result<bool> {
    let (now, printed) = measure(None).map_err(anyhow::Error::msg)?;
    print!("{printed}");
    let base = if bless.is_some() { None } else { baseline().map_err(anyhow::Error::msg)? };
    let (lines, ok) = verdict_lines(&gate(base.as_ref(), &now));
    for l in &lines {
        println!("{l}");
    }
    if base.is_none() && bless.is_none() {
        println!("     notice: no baseline in {BASELINE}: cargo xtask net-feel --bless --reason \"<why>\"");
    }
    let Some(reason) = bless else { return Ok(ok) };
    if !ok {
        println!("not blessed: an absolute rule failed (the netcode has a problem to fix, not the gate)");
        return Ok(false);
    }
    let report = FeelReport { note: NOTE.into(), ..now };
    let text = ron::ser::to_string_pretty(&report, ron::ser::PrettyConfig::default())? + "\n";
    let path = root().join(BASELINE);
    let old = std::fs::read_to_string(&path).ok();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, &text)?;
    let changed = if old.as_deref() == Some(text.as_str()) { vec![] } else { vec![BASELINE.to_string()] };
    crate::bless::log_blessing("net-feel", reason, &changed)?;
    Ok(true)
}

/// `rl-cli net-soak`'s command line for `cargo xtask net-soak`: the
/// packs in `content/packs` and the failures in `target/failures`
/// (relative: the command runs in the workspace root).
pub fn soak_args(minutes: f64, seed: u64) -> Vec<String> {
    let mut args: Vec<String> = ["net-soak", "--minutes"].map(String::from).into();
    args.extend([
        minutes.to_string(),
        "--seed".into(),
        seed.to_string(),
        "--content".into(),
        "content/packs".into(),
        "--failures".into(),
        "target/failures".into(),
    ]);
    args
}

/// `cargo xtask net-soak [--minutes N] [--seed S] [--offline]` (plan 12c,
/// N77; not a `full` step): builds `rl-cli` in release and runs `net-soak`
/// on `content/packs`, its output streaming; failures are kept as cases in
/// `target/failures` (`rl-cli net-sim --from <case>` replays one, `cargo
/// xtask bless-regression --from` keeps it). Nothing leaves the machine.
pub fn soak(minutes: f64, seed: u64, offline: bool) -> anyhow::Result<bool> {
    anyhow::ensure!(minutes.is_finite() && minutes >= 0.0, "--minutes {minutes} is not a duration");
    let cli = crate::bench::rl_cli_with(offline).map_err(anyhow::Error::msg)?;
    let status = Command::new(&cli)
        .args(soak_args(minutes, seed))
        .current_dir(root())
        .status()
        .map_err(|e| anyhow::anyhow!("cannot run {}: {e}", cli.display()))?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feel(profile: &str, p99: u32, stalls: u64, down: u32) -> ProfileFeel {
        ProfileFeel {
            profile: profile.into(),
            rollback_p50: 3,
            rollback_p99: p99,
            correction_px_p99: 4.0,
            own_corrections_per_min: 0.0,
            stalls,
            down_bytes_per_s: down,
            up_bytes_per_s: 900,
        }
    }

    #[test]
    fn net_soak_args_are_parsed() {
        let args = soak_args(0.5, 9);
        let at = |flag: &str| args.iter().position(|a| a == flag).map(|i| args[i + 1].as_str());
        assert_eq!(args[0], "net-soak");
        assert_eq!((at("--minutes"), at("--seed")), (Some("0.5"), Some("9")));
        assert_eq!((at("--content"), at("--failures")), (Some("content/packs"), Some("target/failures")));
        use clap::Parser as _;
        let cli =
            crate::Cli::try_parse_from(["xtask", "net-soak", "--minutes", "0.5", "--seed", "9", "--offline"]);
        assert!(matches!(
            cli.map(|c| c.command),
            Ok(crate::Command::NetSoak { minutes, seed: 9, offline: true }) if minutes == 0.5
        ));
        assert!(crate::Cli::try_parse_from(["xtask", "net-soak", "--minutes", "x"]).is_err());
    }

    #[test]
    fn stalls_up_to_bad_always_fail() {
        let base = feel("bad", 10, 0, 9_000);
        let now = feel("bad", 10, 1, 9_000);
        assert!(compare(&base, &now).failures.iter().any(|f| f.contains("stalls")));
        let awful = feel("awful", 10, 5, 9_000);
        assert!(compare(&awful, &awful).failures.is_empty());
    }

    #[test]
    fn small_changes_pass_and_large_ones_fail() {
        let base = feel("typical", 10, 0, 9_000);
        assert!(compare(&base, &feel("typical", 11, 0, 9_100)).failures.is_empty());
        assert!(!compare(&base, &feel("typical", 13, 0, 9_000)).failures.is_empty());
        assert!(
            !compare(&base, &feel("typical", 6, 0, 9_000)).notices.is_empty(),
            "improvements are noticed"
        );
    }

    #[test]
    fn the_bandwidth_budget_is_absolute() {
        let now = FeelReport {
            note: String::new(),
            profiles: vec![feel("typical", 10, 0, 9_000)],
            bandwidth32: feel("typical", 10, 0, DOWN_BUDGET + 1),
        };
        let results = gate(Some(&now), &now);
        let (name, g) = results.last().unwrap();
        assert_eq!(name, "bandwidth32");
        assert!(g.failures.iter().any(|f| f.contains("budget")), "{g:?}");
        // Without a baseline the absolute rules still apply.
        assert!(!gate(None, &now).last().unwrap().1.failures.is_empty());
    }

    #[test]
    fn the_tool_output_sample_parses_strictly() {
        let text =
            std::fs::read_to_string(crate::root().join("fixtures/bench/tool-output/feel.ron")).unwrap();
        let r: FeelReport = ron::from_str(&text).unwrap();
        assert!(!r.profiles.is_empty() && r.bandwidth32.down_bytes_per_s > 0, "{r:?}");
        let extra = text.replacen("rollback_p50", "rollback_p50_ticks", 1);
        assert!(ron::from_str::<FeelReport>(&extra).is_err(), "unknown fields are refused");
    }
}
