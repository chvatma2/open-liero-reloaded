//! `cargo xtask bench-quick`: `olr-cli quick-bench` against
//! `fixtures/bench/quick.ron`, the issue runner's one-minute benchmark of
//! one change. Instruction counts gate at [`INSTR_TOLERANCE`], the frame's
//! wall time at the looser [`RENDER_TOLERANCE`]. A missing baseline or
//! number, or another machine's baseline, is a notice, never a failure.
//! `cargo xtask bench` stays the real measurement.

use serde::{Deserialize, Serialize};

use crate::bench::{Host, host_notice};
use crate::root;

pub const BASELINE: &str = "fixtures/bench/quick.ron";
/// Instructions may grow this much before a run fails.
pub const INSTR_TOLERANCE: f64 = 0.15;
/// The frame's fastest wall time may grow this much.
pub const RENDER_TOLERANCE: f64 = 0.50;

/// `olr_cli::quick_bench::QuickReport`, read strictly (xtask does not
/// depend on olr-cli).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuickReport {
    pub match_ticks_instructions: Option<u64>,
    pub draw_lists_instructions: Option<u64>,
    pub render_frame_ms_min: Option<f64>,
}

/// The stored baseline: this machine's numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub host: Host,
    pub report: QuickReport,
}

/// One line per number over its tolerance; notices for numbers not compared.
pub fn compare(base: &QuickReport, now: &QuickReport, notices: &mut Vec<String>) -> Vec<String> {
    let mut regressions = Vec::new();
    let mut check = |name: &str, base: Option<f64>, now: Option<f64>, tolerance: f64| match (base, now) {
        (Some(b), Some(n)) if b > 0.0 && n > b * (1.0 + tolerance) => regressions.push(format!(
            "{name}: {n:.0} against {b:.0} (+{:.1} %, tolerance {:.0} %)",
            (n / b - 1.0) * 100.0,
            tolerance * 100.0
        )),
        (Some(_), Some(_)) => {}
        _ => notices.push(format!("{name}: not compared (no baseline or not measured)")),
    };
    let instr = |v: Option<u64>| v.map(|n| n as f64);
    check(
        "match ticks (instructions)",
        instr(base.match_ticks_instructions),
        instr(now.match_ticks_instructions),
        INSTR_TOLERANCE,
    );
    check(
        "draw lists (instructions)",
        instr(base.draw_lists_instructions),
        instr(now.draw_lists_instructions),
        INSTR_TOLERANCE,
    );
    check(
        "1080p frame (µs)",
        base.render_frame_ms_min.map(|m| m * 1000.0),
        now.render_frame_ms_min.map(|m| m * 1000.0),
        RENDER_TOLERANCE,
    );
    regressions
}

/// Measures, then compares with the baseline or (with a reason) writes it.
pub fn run(bless: Option<&str>) -> anyhow::Result<bool> {
    let out = root().join("target/bench-quick.ron");
    let status = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "run",
            "--quiet",
            "--release",
            "-p",
            "olr-cli",
            "--",
            "quick-bench",
            "--content",
            "content/packs",
            "--out",
        ])
        .arg(&out)
        .current_dir(root())
        .status()?;
    anyhow::ensure!(status.success(), "olr-cli quick-bench failed");
    let now: QuickReport = ron::from_str(&std::fs::read_to_string(&out)?)?;
    let here = Host::here();
    if let Some(reason) = bless {
        let text = ron::ser::to_string_pretty(
            &Baseline { host: here, report: now },
            ron::ser::PrettyConfig::default(),
        )?;
        std::fs::write(root().join(BASELINE), text + "\n")?;
        crate::bless::log_blessing("bench-quick", reason, &[BASELINE.to_string()])?;
        return Ok(true);
    }
    let base: Baseline = match std::fs::read_to_string(root().join(BASELINE)) {
        Ok(text) => ron::from_str(&text)?,
        Err(_) => {
            println!("notice: no {BASELINE}; run `cargo xtask bench-quick --bless --reason \"<why>\"`");
            return Ok(true);
        }
    };
    if let Some(notice) = host_notice(&base.host, &here) {
        println!("notice: {notice}");
        return Ok(true);
    }
    let mut notices = Vec::new();
    let regressions = compare(&base.report, &now, &mut notices);
    for n in notices {
        println!("notice: {n}");
    }
    if regressions.is_empty() {
        println!("bench-quick passed");
        return Ok(true);
    }
    println!("FAIL bench-quick");
    for r in regressions {
        println!("  {r}");
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(ticks: u64, lists: u64, ms: f64) -> QuickReport {
        QuickReport {
            match_ticks_instructions: Some(ticks),
            draw_lists_instructions: Some(lists),
            render_frame_ms_min: Some(ms),
        }
    }

    #[test]
    fn within_tolerance_passes() {
        let mut notices = Vec::new();
        assert!(compare(&report(1000, 1000, 10.0), &report(1149, 900, 14.9), &mut notices).is_empty());
        assert!(notices.is_empty());
    }

    #[test]
    fn instructions_regression_fails() {
        let lines = compare(&report(1000, 1000, 10.0), &report(1160, 1000, 10.0), &mut Vec::new());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("match ticks"), "{lines:?}");
    }

    #[test]
    fn render_regression_fails() {
        let lines = compare(&report(1000, 1000, 10.0), &report(1000, 1000, 15.1), &mut Vec::new());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("1080p frame"), "{lines:?}");
    }

    #[test]
    fn missing_values_are_notices() {
        let mut notices = Vec::new();
        let now = QuickReport { render_frame_ms_min: None, ..report(5000, 5000, 1.0) };
        let base = QuickReport { match_ticks_instructions: None, ..report(1000, 1000, 10.0) };
        let lines = compare(&base, &now, &mut notices);
        assert_eq!(lines.len(), 1, "draw lists regressed: {lines:?}");
        assert_eq!(notices.len(), 2, "{notices:?}");
    }
}
