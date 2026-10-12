//! State transfer in real matches (networking spec §3.5): every state a
//! chaos match passes through loads, so `World::load_state`'s checks (its
//! finite numbers among them) are never stricter than play. Needs
//! `RL_CONTENT_DIR`; skips without it.

use std::path::PathBuf;

use rl_cli::assets::LevelAssets;
use rl_cli::stress;
use rl_content::BuiltMatch;
use rl_sim::{Keys, TickEvents};

fn content() -> Option<rl_content::Content> {
    let dir = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("RL_CONTENT_DIR not set; skipping");
    }
    dir.map(|d| rl_content::Content::open(&d).unwrap())
}

/// 16 chaos bots (every weapon, fired all the time, bonuses) for 40 s on
/// Castle Strike, MW 1.0 and Classic; every 5th tick's state goes into
/// another world of the same setup.
#[test]
fn every_state_of_a_chaos_match_loads() {
    let Some(content) = content() else { return };
    for mod_id in ["olx-mw-1-0", "olx-classic"] {
        let level = "olx-level-castlestrike";
        let assets = LevelAssets::load(&content, mod_id, level).unwrap();
        let spec = stress::spec(mod_id, level, 16, 3000, 40.0, 5);
        let BuiltMatch { mut world, mut bots, .. } =
            rl_content::build_match(assets.match_assets(), &spec.match_options()).unwrap();
        let mut dst = world.clone();
        let (mut events, mut keys) = (TickEvents::new(), vec![Keys(0); bots.len()]);
        let (mut projectiles, mut transfers) = (0, 0);
        for t in 0..spec.ticks {
            for (i, b) in bots.iter_mut().enumerate() {
                keys[i] = b.think(&world, &events);
            }
            world.tick(&keys, &mut events);
            projectiles = world.projectiles().count().max(projectiles);
            if t % 5 == 0 {
                dst.load_state(world.state()).unwrap_or_else(|e| panic!("{mod_id}, tick {t}: {e}"));
                assert!(dst == world, "{mod_id}, tick {t}: the loaded world differs");
                transfers += 1;
            }
        }
        assert!(projectiles > 100, "{mod_id}: the bots fire ({projectiles} projectiles at most)");
        eprintln!("{mod_id}: {transfers} states loaded, up to {projectiles} projectiles");
    }
}
