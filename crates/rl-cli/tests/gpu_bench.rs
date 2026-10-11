//! `rl-cli gpu-bench` (plan 8) on the software adapter: a short stress
//! match recorded, then drawn at one size with the probe. Needs
//! `RL_GPU_TESTS` (`cargo xtask full`), content and an adapter.

use std::path::PathBuf;

use rl_cli::gpu_bench::{self, BenchConfig};
use rl_cli::stress::{self, Config, Spread};
use rl_render::OffscreenOptions;

#[test]
fn a_recorded_match_is_drawn_timed_and_probed() {
    if std::env::var_os("RL_GPU_TESTS").is_none() {
        eprintln!("RL_GPU_TESTS not set; skipping");
        return;
    }
    let Some(dir) = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set; skipping");
        return;
    };
    if rl_render::Offscreen::new().is_err() {
        eprintln!("no adapter; skipping");
        return;
    }
    let content = rl_content::Content::open(&dir).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("m.rlrp");
    let spec = stress::spec("olx-classic", "olx-level-dirt-level", 8, 3000, 3.0, 1);
    stress::run(
        &content,
        &Config { verify: false, record: Some(file.clone()), ..Config::new(spec) },
        &mut |_| {},
    )
    .unwrap();
    let replay = rl_replay::Replay::decode(&std::fs::read(&file).unwrap()).unwrap();
    let cfg = BenchConfig {
        sizes: vec![(320, 180)],
        start_tick: 100,
        frames: 50,
        probe_passes: vec![0, 2],
        probe_taps: 4,
        probe_frames: 5,
        options: OffscreenOptions { timestamps: true, ..OffscreenOptions::default() },
    };
    let r = gpu_bench::run(&content, &replay, &cfg, &mut |_| {}).unwrap();
    eprintln!("gpu-bench on {} ({}, {}), timestamps {}", r.adapter, r.backend, r.device_type, r.timestamps);
    let s = &r.sizes[0];
    assert_eq!((s.size, s.frames, s.probe.len()), ((320, 180), 50, 2));
    assert!(s.total_ms.p50 >= s.frame_ms.p50 && s.frame_ms.p50 > 0.0, "{s:?}");
    assert_eq!(s.gpu_ms.is_some(), r.timestamps);
    if r.timestamps {
        assert!(s.gpu_ms.unwrap().p50 > 0.0, "{s:?}");
    }
    // The probe's passes cost GPU time when timestamps time them (the view
    // is drawn over the whole target, so they cover all of it: ruling P11).
    let probed = &s.probe[1];
    assert_eq!((probed.passes, probed.taps, probed.timed_on_gpu), (2, 4, r.timestamps), "{s:?}");
    if r.timestamps {
        assert!(probed.ms.p50 > 0.0 && s.probe[0].ms == Spread::default(), "{s:?}");
    }
    let mut tampered = replay.clone();
    *tampered.row_mut(150).first_mut().unwrap() = rl_sim::Keys(0xFF);
    let e = gpu_bench::run(&content, &tampered, &cfg, &mut |_| {}).expect_err("a different match is refused");
    assert!(format!("{e:#}").contains("diverged"), "{e:#}");
}
