//! The chaos personality (spec §20 F10, notes §4.6): over a grid of
//! matches, chaos bots fire every weapon of both OLX mods, whatever the
//! restrictions, while Deathmatch rotates their loadouts on each spawn.

use crate::common;

use std::collections::BTreeSet;

use common::olx::*;
use rl_bots::{BotConfig, Personality};
use rl_modes::{ChaosCycle, DeathmatchSettings, RespawnLoadout};
use rl_sim::{EventKind, WeaponType};

const CHAOS: usize = 4;

#[test]
fn chaos_fires_every_weapon_over_a_match_grid() {
    let Some(c) = content() else { return };
    let chaos = Player::Bot(BotConfig { personality: Personality::Chaos, ..BotConfig::default() });
    for mod_id in [CLASSIC, MW] {
        let sim = c.load_mod(mod_id).unwrap().sim;
        // Specials report no `WeaponFired` (they are shot on the client only).
        let all: BTreeSet<i32> = (0..sim.weapons().len() as i32)
            .filter(|&i| sim.weapons()[i as usize].kind != WeaponType::Special)
            .collect();
        let cycle = ChaosCycle::new(&(0..CHAOS).collect::<Vec<_>>(), sim.weapons().len());
        let dm = DeathmatchSettings {
            respawn_loadout: RespawnLoadout::ChaosCycle(cycle),
            kill_limit: -1,
            ..DeathmatchSettings::default()
        };
        let (mut fired, mut tracked) = (BTreeSet::new(), BTreeSet::new());
        let mut ticks = 0;
        for seed in [1, 2] {
            let mut m = deathmatch(&c, mod_id, DIRT_LEVEL, seed, &[chaos; CHAOS], dm.clone(), |_| {});
            for _ in 0..6000 {
                m.tick();
                for e in m.out.iter() {
                    if let EventKind::WeaponFired { weapon, .. } = e.kind {
                        fired.insert(weapon);
                    }
                }
                if fired.len() == all.len() {
                    break;
                }
            }
            ticks += m.ticks;
            for b in m.bots.iter_mut().flatten() {
                // A bot reads a tick's events in its next think.
                b.think(&m.w, &m.out);
                tracked.extend(b.fired_weapons());
            }
            if fired.len() == all.len() {
                break;
            }
        }
        let missing: Vec<&str> =
            all.difference(&fired).map(|&i| sim.weapons()[i as usize].name.as_str()).collect();
        eprintln!("{mod_id}: {} of {} weapons fired in {ticks} ticks", fired.len(), all.len());
        assert!(missing.is_empty(), "{mod_id}: never fired {missing:?}");
        assert!(tracked.is_superset(&fired), "the bots' own bookkeeping saw every shot");
    }
}
