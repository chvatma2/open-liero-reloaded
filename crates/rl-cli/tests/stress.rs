//! The engine stress test's runner (`rl_cli::stress`), short: 32 chaos
//! bots with near-instant reloads on a pool of 200 projectiles fill it at
//! once; every tick's invariants hold, the buffers stay bounded, the match
//! is the same when played again and its replay reproduces it. Needs
//! `RL_CONTENT_DIR`; skips without it. (`cargo xtask stress` plays it for
//! minutes on Castle Strike; `full` 20 s against a baseline.)

use std::path::PathBuf;

use rl_cli::stress::{self, Config};

fn content() -> Option<rl_content::Content> {
    let dir = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("RL_CONTENT_DIR not set; skipping");
    }
    dir.map(|d| rl_content::Content::open(&d).unwrap())
}

#[test]
fn stress_run_is_bounded_and_deterministic() {
    let Some(content) = content() else { return };
    let spec = stress::spec("olx-mw-1-0", "olx-level-dirt-level", 32, 200, 4.0, 3);
    let cfg = Config { spec, render_every: 0, render_size: (64, 36), verify: true };
    let mut lines = Vec::new();
    let r = stress::run(&content, &cfg, &mut |l| lines.push(l)).unwrap();
    assert_eq!(r.ticks, 400);
    assert_eq!(r.pool_capacity, 200);
    assert!(r.projectiles_peak == 200 && r.ticks_at_pool_cap > 0, "the small pool fills: {r:?}");
    assert!(r.shots > 100, "chaos bots fire all the time: {}", r.shots);
    assert_eq!(r.deterministic, Some(true));
    assert!(r.tick_us.p50 > 0.0 && r.tick_us.max >= r.tick_us.p99 && r.tick_us.p99 >= r.tick_us.p50);
    assert!(r.render.is_none());
    if cfg!(target_os = "linux") {
        assert!(r.peak_mib.is_some_and(|m| m > 1.0));
    }
    assert!(r.text().contains("projectiles: peak 200 of 200"), "{}", r.text());
    assert!(lines.iter().any(|l| l.contains("playing it again")));
}
