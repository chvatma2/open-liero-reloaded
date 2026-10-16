//! The OLX comparison (spec §6, ruling P8): each scenario timed in OLX
//! (Docker, `crate::olx_timing`) and in olr-sim (`olr-cli scenario --time`),
//! `runs` times each, medians, and the same work checked frame by frame.

use std::path::Path;
use std::process::Command;

use anyhow::Context as _;
use olr_oracle::Scenario;

use super::Plan;
use super::results::{self, OlxComparison, OlxRow, ScenarioTimingOut, TimingReportOut};
use crate::olx_timing::{Olx, PERF_DIR};
use crate::root;

const TIMED_DIR: &str = "target/envelope/olx";

fn olr_time(cli: &Path, content: &Path, name: &str, fixtures: &Path) -> anyhow::Result<ScenarioTimingOut> {
    let out = root().join("target/envelope/olx-time.ron");
    let _ = std::fs::remove_file(&out);
    let o = Command::new(cli)
        .current_dir(root())
        .args(["scenario", name, "--fixtures"])
        .arg(fixtures)
        .arg("--content")
        .arg(content)
        .arg("--time")
        .arg(&out)
        .output()
        .context("running olr-cli scenario")?;
    let text = std::fs::read_to_string(&out).with_context(|| {
        format!("olr-cli scenario wrote no times: {}", String::from_utf8_lossy(&o.stderr))
    })?;
    let r: TimingReportOut = ron::from_str(&text)?;
    let t = r.scenarios.into_iter().next().context("no scenario timed")?;
    anyhow::ensure!(t.name == name, "olr-cli scenario timed {:?}, not {name:?}", t.name);
    Ok(t)
}

pub fn run(cli: &Path, content: &Path, plan: &Plan) -> anyhow::Result<OlxComparison> {
    let olx = Olx::open()?;
    let timed = root().join(TIMED_DIR);
    std::fs::create_dir_all(&timed)?;
    let all: Vec<(&str, bool)> =
        plan.olx_timed.iter().map(|n| (*n, false)).chain(plan.olx_heavy.iter().map(|n| (*n, true))).collect();
    let (rows, notes) = table(&all, |name, heavy| {
        let src = root().join(if heavy { PERF_DIR } else { "fixtures/oracle" }).join(format!("{name}.ron"));
        let sc =
            Scenario::from_ron(&std::fs::read_to_string(&src)?).with_context(|| src.display().to_string())?;
        let (mut o_runs, mut r_runs) = (Vec::new(), Vec::new());
        for run in 0..plan.runs {
            println!("  olx {name}, run {}", run + 1);
            let gz = (heavy && run == 0).then(|| timed.join(format!("{name}.jsonl.gz")));
            o_runs.push(olx.time(name, &sc, gz.as_deref())?);
            if heavy && run == 0 {
                std::fs::copy(&src, timed.join(format!("{name}.ron")))?;
            }
            let fixtures = if heavy { timed.clone() } else { root().join("fixtures/oracle") };
            r_runs.push(olr_time(cli, content, name, &fixtures)?);
        }
        Ok(results::olx_row(name, &o_runs, &r_runs))
    });
    // Ruling T6-A: the build type is Release, set as a plain variable (the
    // cache's CMAKE_BUILD_TYPE is empty, so it is never read), with -O3.
    let build = format!("Release, -O3 (CMakeOlxCommon.cmake:43-47, :369); {}", olx.build_info());
    Ok(OlxComparison { image: olx.image().into(), build, rows, notes })
}

/// Each scenario's row from `time`; one that fails is a note (ruling
/// T7-A) and the rest go on.
fn table(
    scenarios: &[(&str, bool)],
    mut time: impl FnMut(&str, bool) -> anyhow::Result<OlxRow>,
) -> (Vec<OlxRow>, Vec<String>) {
    let (mut rows, mut notes) = (Vec::new(), Vec::new());
    for &(name, heavy) in scenarios {
        match time(name, heavy) {
            Ok(row) => {
                println!(
                    "  {name}: OLX p50 {:.1} µs, olr-sim p50 {:.1} µs (×{:.2}), peak {} projectiles, same work: {}",
                    row.olx_us.p50, row.olr_us.p50, row.ratio_p50, row.projectiles_peak, row.same_work
                );
                rows.push(row);
            }
            Err(e) => {
                println!("  skip olx {name} ({e:#})");
                notes.push(format!("{name}: {e:#}"));
            }
        }
    }
    (rows, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ruling T7-A: one scenario failing leaves the others' rows.
    #[test]
    fn a_failed_scenario_is_a_note() {
        let (rows, notes) = table(&[("a", false), ("b", false), ("c", true)], |name, _| {
            anyhow::ensure!(name != "b", "OLX timed 3 frames of 200");
            Ok(OlxRow { name: name.into(), ..OlxRow::default() })
        });
        assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["a", "c"]);
        assert_eq!(notes, ["b: OLX timed 3 frames of 200"]);
    }
}
