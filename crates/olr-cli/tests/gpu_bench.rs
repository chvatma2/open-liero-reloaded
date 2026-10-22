//! `olr-cli gpu-bench` (plan 8) on the software adapter: a short stress
//! match recorded, then drawn at one size with the probe. Needs
//! `OLR_GPU_TESTS` (`cargo xtask full`), content and an adapter.

use std::path::PathBuf;

use olr_cli::gpu_bench::{self, BenchConfig};
use olr_cli::stress::{self, Config, Spread};
use olr_render::OffscreenOptions;

#[test]
fn a_recorded_match_is_drawn_timed_and_probed() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        eprintln!("OLR_GPU_TESTS not set; skipping");
        return;
    }
    let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set; skipping");
        return;
    };
    if olr_render::Offscreen::new().is_err() {
        eprintln!("no adapter; skipping");
        return;
    }
    let content = olr_content::Content::open(&dir).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("m.olrp");
    let spec = stress::spec("olx-classic", "olx-level-dirt-level", 8, 3000, 3.0, 1);
    stress::run(
        &content,
        &Config { verify: false, record: Some(file.clone()), ..Config::new(spec) },
        &mut |_| {},
    )
    .unwrap();
    let replay = olr_replay::Replay::decode(&std::fs::read(&file).unwrap()).unwrap();
    let cfg = BenchConfig {
        sizes: vec![(320, 180)],
        start_tick: 100,
        frames: 50,
        probe_passes: vec![0, 2],
        probe_taps: 4,
        probe_frames: 5,
        options: OffscreenOptions { timestamps: true, ..OffscreenOptions::default() },
        look: false,
        occlusion: None,
        views: 1,
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
    // `--look` (plan 15a Task 20): the same match drawn lit, said so.
    let lit =
        gpu_bench::run(&content, &replay, &BenchConfig { look: true, ..cfg.clone() }, &mut |_| {}).unwrap();
    assert!(lit.look && !r.look);
    assert_eq!(lit.sizes[0].frames, 50);
    if r.timestamps {
        assert!(lit.sizes[0].gpu_ms.unwrap().p50 > s.gpu_ms.unwrap().p50, "the look passes cost GPU time");
    }
    // `--occlusion` (issue #10): the bench look with ambient occlusion, said so.
    let occlusion = Some(olr_look::Occlusion { strength: 60, reach: 12 });
    let occluded = BenchConfig { look: true, occlusion, ..cfg.clone() };
    let occluded = gpu_bench::run(&content, &replay, &occluded, &mut |_| {}).unwrap();
    assert!(occluded.look && occluded.scene.contains("occlusion 60 % over 12 px"), "{}", occluded.scene);
    let unlit = BenchConfig { occlusion, ..cfg.clone() };
    assert!(gpu_bench::run(&content, &replay, &unlit, &mut |_| {}).is_err(), "occlusion needs the look");
    // `--views` (issue #26): four lit views a frame, said so; 0 or 5 refused.
    let split = BenchConfig { look: true, views: 4, ..cfg.clone() };
    let split = gpu_bench::run(&content, &replay, &split, &mut |_| {}).unwrap();
    assert!(split.scene.ends_with(", 4 views") && split.sizes[0].frames == 50, "{}", split.scene);
    if r.timestamps {
        let (four, one) = (split.sizes[0].gpu_ms.unwrap().p50, lit.sizes[0].gpu_ms.unwrap().p50);
        assert!(four > one, "four views cost more GPU time than one: {four} {one}");
    }
    for views in [0, 5] {
        let bad = BenchConfig { views, ..cfg.clone() };
        assert!(gpu_bench::run(&content, &replay, &bad, &mut |_| {}).is_err(), "{views} views");
    }
    let mut tampered = replay.clone();
    *tampered.row_mut(150).first_mut().unwrap() = olr_sim::Keys(0xFF);
    let e = gpu_bench::run(&content, &tampered, &cfg, &mut |_| {}).expect_err("a different match is refused");
    assert!(format!("{e:#}").contains("diverged"), "{e:#}");
}
