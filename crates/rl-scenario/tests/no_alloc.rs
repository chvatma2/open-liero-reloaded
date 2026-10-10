//! Spec §5.3 with real content: firing every weapon of the imported Classic
//! mod for 500 ticks, with its explosions, trails, beams and events, does not
//! allocate. Needs the imported OLX content (`RL_CONTENT_DIR`).

use std::path::Path;
use std::sync::Arc;

use rl_math::AbsTime;
use rl_sim::{EventKind, Keys, Loadout, SimSettings, Slot, TickEvents, WeaponType, World};

const TICKS: usize = 500;
const WORMS: usize = 4;

#[test]
fn firing_every_classic_weapon_does_not_allocate() {
    let Ok(dir) = std::env::var("RL_CONTENT_DIR") else {
        return eprintln!("RL_CONTENT_DIR not set; skipping");
    };
    let content = Path::new(&dir);
    if !content.is_dir() {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        return;
    }
    let content = rl_content::Content::open(content).unwrap();
    let sim_mod = Arc::new(content.sim_mod(&rl_content::olx_mod_id("Classic")).unwrap());
    let level = content.load_level("olx-level-dirt-level").unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    let n = sim_mod.weapons().len();
    // Dead worms respawn the next frame, so every round's worms can fire.
    let settings =
        SimSettings { respawn_time: 0.0, bonuses: true, bonus_frequency: 1.0, ..SimSettings::lx56() };
    let terrain = rl_terrain::Terrain::from_grid(&level.material);
    let mut w = World::new(settings, sim_mod.clone(), terrain, theme.stamps(), AbsTime(100_000), 7);
    if let Some(mask) = theme.green_mask() {
        w.set_green_mask(mask);
    }
    for i in 0..WORMS {
        w.spawn_worm(100 + 80 * i as i32, 100).unwrap();
    }
    // Each round gives the worms the next four weapons, in all five slots.
    let rounds = n.div_ceil(WORMS);
    let mut fired = vec![false; n];
    let mut allocations = 0;
    let mut events = TickEvents::new();
    for round in 0..rounds {
        for i in 0..WORMS {
            let weapon = ((round * WORMS + i) % n) as i32;
            let slot = Slot { weapon, charge: 1.0, reloading: false, last_fire: 0.0 };
            let loadout = Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 };
            w.set_loadout(i, &loadout);
        }
        let keys = [
            Keys::FIRE | Keys::UP,
            Keys::FIRE | Keys::LEFT,
            Keys::FIRE | Keys::DOWN,
            Keys::FIRE | Keys::RIGHT,
        ]
        .map(Keys);
        let info = allocation_counter::measure(|| {
            for _ in 0..TICKS / rounds {
                w.tick(&keys, &mut events);
                for e in &events {
                    if let EventKind::WeaponFired { weapon, .. } = e.kind {
                        fired[weapon as usize] = true;
                    }
                }
            }
        });
        allocations += info.count_total;
    }
    eprintln!("{n} weapons in {rounds} rounds, {} fired", fired.iter().filter(|f| **f).count());
    assert_eq!(allocations, 0, "tick allocated");
    let missed: Vec<&str> = (0..n)
        .filter(|&i| !fired[i] && sim_mod.weapons()[i].kind != WeaponType::Special)
        .map(|i| sim_mod.weapons()[i].name.as_str())
        .collect();
    assert!(missed.is_empty(), "never fired: {missed:?}");
}
