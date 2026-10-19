//! The RON `cargo xtask envelope` reads from `olr-cli` (plan 8): `stress
//! --out`, `gpu-bench --out` and `scenario --time`, and the RON `cargo xtask
//! net-feel` reads (`net-sim --feel`, plan 9) and `rollback-cost --out` (plan 9), written exactly as the
//! commands write them, must equal the committed samples in
//! `fixtures/bench/tool-output/`. `xtask`'s `tool_outputs_parse` and
//! `the_tool_output_sample_parses_strictly` read the same samples with
//! their strict structs, so a field renamed here
//! fails one test or the other instead of reading as a measured 0. After
//! an intended change, rewrite the samples with `OLR_BLESS_TOOL_OUTPUT=1
//! cargo nextest run -p olr-cli --test integration tool_output::` and fix
//! `xtask`.

use std::path::PathBuf;

use olr_cli::feel::{FeelReport, NOTE, ProfileFeel};
use olr_cli::gpu_bench::{GpuReport, ProbePoint, SizeReport};
use olr_cli::rollback_cost::RollbackCost;
use olr_cli::stress::{Report, Spread};
use olr_cli::timing::{ScenarioTiming, TimingReport};

fn spread(base: f64) -> Spread {
    Spread { p50: base, p99: base * 2.0, max: base * 3.0 }
}

/// Checks `value`, written as the commands write it, against the sample `name`.
fn sample<T: serde::Serialize>(name: &str, value: &T) {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/bench/tool-output").join(name);
    let text = ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::default()).unwrap() + "\n";
    if std::env::var_os("OLR_BLESS_TOOL_OUTPUT").is_some() {
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &text).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        committed == text,
        "{} differs from what olr-cli writes now; if the change is intended, rewrite it \
         (OLR_BLESS_TOOL_OUTPUT=1) and make xtask's envelope read the new fields:\n{text}",
        file.display()
    );
}

#[test]
fn stress_report_sample() {
    let r = Report {
        name: "classic-castlestrike-w16".into(),
        tick_us: spread(60.0),
        sim_us: spread(50.0),
        think_us: spread(10.0),
        present_us: spread(20.0),
        build_us: spread(30.0),
        pool_capacity: 3000,
        projectiles_peak: 993,
        ticks_at_pool_cap: 4,
        fx_peak: 1024,
        ticks_at_fx_cap: 1250,
        fx_capacity: 1024,
        events_max: 40,
        peak_mib: Some(28.0),
        area_cache: (12, 4096),
        caps: vec!["the bots' area cache is full (4096 areas)".into()],
        ..Report::default()
    };
    sample("stress.ron", &r);
}

#[test]
fn gpu_bench_report_sample() {
    let probe = |passes: u32| ProbePoint {
        passes,
        taps: 9,
        ms: spread(0.05 * f64::from(passes.max(1))),
        timed_on_gpu: true,
        gpu_dropped: 1,
    };
    let r = GpuReport {
        adapter: "NVIDIA GeForce RTX 5070 Ti".into(),
        backend: "Vulkan".into(),
        device_type: "DiscreteGpu".into(),
        timestamps: true,
        scene: "olx-mw-1-0 on olx-level-castlestrike".into(),
        start_tick: 1000,
        look: false,
        sizes: vec![SizeReport {
            size: (2560, 1440),
            frames: 600,
            tick_ms: spread(0.6),
            cpu_ms: spread(0.3),
            frame_ms: spread(0.4),
            total_ms: spread(1.0),
            gpu_ms: Some(spread(0.02)),
            gpu_dropped: 2,
            probe: vec![probe(0), probe(4)],
        }],
    };
    sample("gpu-bench.ron", &r);
}

#[test]
fn scenario_timing_sample() {
    let r = TimingReport {
        scenarios: vec![ScenarioTiming {
            name: "heavy-spikeballs".into(),
            tick_ns: vec![150_000, 160_000],
            projectiles: vec![2990, 3000],
            rand_calls: vec![10, 20],
            matches_olx: Some(true),
        }],
    };
    sample("scenario-time.ron", &r);
}

#[test]
fn feel_report_sample() {
    let feel = |profile: &str, p50: u32, down: u32| ProfileFeel {
        profile: profile.into(),
        rollback_p50: p50,
        rollback_p99: p50 + 2,
        correction_px_p99: 9.5,
        own_corrections_per_min: 0.5,
        stalls: 0,
        down_bytes_per_s: down,
        up_bytes_per_s: 7_700,
    };
    let r = FeelReport {
        note: NOTE.into(),
        profiles: vec![feel("lan", 3, 9_200), feel("typical", 8, 13_000)],
        bandwidth32: feel("typical", 8, 16_700),
    };
    sample("feel.ron", &r);
}

#[test]
fn rollback_cost_sample() {
    let r = RollbackCost::of("olx-level-castlestrike", 16, 12, &[40.0, 50.0], &[300.0, 350.0], 21_000, 9_000);
    sample("rollback.ron", &r);
}
