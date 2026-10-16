//! The chaos loadout rotation (spec §20 F10, notes §4.6): Deathmatch gives
//! each chaos bot its weapons on every spawn through `give_weapon`.

use crate::common;

use common::*;
use olr_modes::{ChaosCycle, DeathmatchSettings, RespawnLoadout};
use olr_sim::{EventKind, Keys, MAX_SLOTS};

fn slots(r: &Run, i: usize) -> Vec<i32> {
    r.w.worms()[i].slots.iter().map(|s| s.weapon).collect()
}

#[test]
fn chaos_cycle_gives_the_rotation_on_every_spawn() {
    // Worms 0 and 2 are chaos bots 0 and 1 of 2; the mod has 3 weapons.
    let cycle = ChaosCycle::new(&[0, 2], 3);
    assert_eq!(cycle.weapons_for(0, 0), Some([0, 1, 2, 0, 1]));
    assert_eq!(cycle.weapons_for(2, 0), Some([2, 0, 1, 2, 0]), "(1·5 + j) mod 3");
    assert_eq!(cycle.weapons_for(0, 1), Some([1, 2, 0, 1, 2]), "(1·5·2 + j) mod 3");
    assert_eq!(cycle.weapons_for(1, 0), None, "worm 1 is no chaos bot");
    assert_eq!(cycle.weapons_for(40, 0), None);
    assert_eq!(ChaosCycle::new(&[0], 0).weapons_for(0, 3), None, "a mod without weapons");
    // A replay or the CLI rebuilds the rotation from its parts.
    let odd = ChaosCycle::new(&[5, 2, 5, 40, 9], 38);
    assert_eq!(odd.worms().collect::<Vec<_>>(), [5, 2, 9]);
    assert_eq!(odd.weapon_count(), 38);
    assert_eq!(ChaosCycle::new(&odd.worms().collect::<Vec<_>>(), odd.weapon_count()), odd);
    let huge = ChaosCycle::new(&[0, 1], 7).weapons_for(1, u32::MAX).unwrap();
    assert!(huge.iter().all(|w| (0..7).contains(w)));

    let dm = DeathmatchSettings { respawn_loadout: RespawnLoadout::ChaosCycle(cycle), ..Default::default() };
    // Worm 0 holds the bomb, worm 1 the laser, worm 2 the graze beam.
    let mut r = Run::new(world(dm, &[(50, BOMB), (300, LASER), (200, GRAZE)]));
    assert_eq!(slots(&r, 0), [0, 1, 2, 0, 1], "the first spawn is life 0");
    assert_eq!(slots(&r, 1), [LASER; MAX_SLOTS], "not a chaos bot");
    assert_eq!(slots(&r, 2), [2, 0, 1, 2, 0]);
    assert!(r.w.worms()[0].slots.iter().all(|s| s.charge == 1.0 && !s.reloading));
    r.idle(150);

    // Worm 0 bombs itself (slot 2 holds the bomb) and asks to respawn.
    let mut keys = [Keys(Keys::CHANGE | Keys::RIGHT), Keys(0), Keys(0)];
    r.tick(&keys);
    keys[0] = Keys(Keys::CHANGE);
    r.tick(&keys);
    keys[0] = Keys(Keys::CHANGE | Keys::RIGHT);
    r.tick(&keys);
    r.tick(&[Keys(0); 3]);
    assert_eq!(r.w.worms()[0].current_weapon, 2);
    r.fire(&[0], 10);
    assert!(r.kills().iter().any(|k| k.1 == 0), "worm 0 died");
    let mut spawned = false;
    for t in 0..600u32 {
        let jump = if t.is_multiple_of(2) { Keys(Keys::JUMP) } else { Keys(0) };
        r.tick(&[jump, Keys(0), Keys(0)]);
        if r.out.iter().any(|e| e.kind == EventKind::Spawned { worm: 0 }) {
            spawned = true;
            break;
        }
    }
    assert!(spawned, "worm 0 respawned");
    r.tick(&[Keys(0); 3]);
    assert_eq!(slots(&r, 0), [1, 2, 0, 1, 2], "life 1");
    assert_eq!(slots(&r, 1), [LASER; MAX_SLOTS]);
    assert_eq!(slots(&r, 2), [2, 0, 1, 2, 0], "unchanged until it respawns");
}
