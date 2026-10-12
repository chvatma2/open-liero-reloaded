mod common;

use common::{content, full_tier};
use rl_cli::netsim::{NetSpec, run};

fn spec(profile: &str, seed: u32) -> NetSpec {
    NetSpec {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-castlestrike".into(),
        seed,
        slots: 8,
        clients: 3,
        spectators: 1,
        join_tick: None,
        profile: profile.into(),
        drift_ppm: 2_000,
        ticks: 600,
        outages: vec![],
        net_seed: seed as u64,
        difficulty: None,
    }
}

fn pass(c: &rl_content::Content, s: &NetSpec) {
    let r = run(c, s).unwrap();
    if let Err(f) = &r.verdict {
        let path = rl_cli::netsim::write_failure(&common::failures_dir(), &r).unwrap();
        panic!(
            "{} failed at tick {}: {} (rl-cli net-sim --from {})",
            s.stem(),
            f.tick,
            f.what,
            path.display()
        );
    }
}

#[test]
fn bots_play_online_over_a_typical_and_a_bad_network() {
    let Some(c) = content() else { return };
    pass(&c, &spec("typical", 1));
    pass(&c, &spec("bad", 2));
}

#[test]
fn a_spectator_joins_a_real_match() {
    let Some(c) = content() else { return };
    let mut s = spec("typical", 3);
    s.join_tick = Some(300);
    pass(&c, &s);
}

#[test]
fn net_grid() {
    if !full_tier() {
        return;
    }
    let Some(c) = content() else { return };
    for profile in ["lan", "good", "typical", "bad", "awful"] {
        for seed in 1..=4 {
            let mut s = spec(profile, seed);
            s.ticks = 3_000;
            if seed % 2 == 0 {
                s.mod_id = "olx-mw-1-0".into();
            }
            pass(&c, &s);
        }
    }
}
