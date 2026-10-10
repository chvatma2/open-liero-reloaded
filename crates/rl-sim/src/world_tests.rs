//! Unit tests of [`crate::World`].
use super::*;
use alloc::vec;
use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};

fn test_mod() -> Arc<SimMod> {
    crate::mod_rt::tests::test_mod()
}

fn holes() -> Vec<Stamp> {
    // Same sizes as the Dirt theme's hole1..hole5.
    [5, 7, 9, 11, 15]
        .into_iter()
        .map(|size| {
            let mut img = RgbaImage::new(size, size);
            for y in 0..size {
                for x in 0..size {
                    img.set(x, y, Rgba::MAGENTA);
                }
            }
            Stamp::from_image(&img)
        })
        .collect()
}

fn world(fill: Material) -> World {
    let terrain = Terrain::from_grid(&MaterialGrid::new(200, 200, fill));
    World::test_new(SimSettings::lx56(), test_mod(), terrain, holes(), AbsTime(0), 1)
}

#[test]
fn physics_steps_follow_the_two_clocks() {
    // 11 ms physics steps against 10 ms frames: none in frames 0 and 1,
    // one per frame from frame 2, none again in frame 12 (OLX trace `fall`).
    let mut w = world(Material::Background);
    w.spawn_worm(100, 20);
    let mut stepped = vec![];
    for _ in 0..13 {
        let before = w.worms()[0].vel.y;
        w.tick_new(&[Keys(0)]);
        stepped.push(w.worms()[0].vel.y - before > 0.5);
    }
    let expected = [false, false, true, true, true, true, true, true, true, true, true, true, false];
    assert_eq!(stepped, expected);
}

#[test]
fn gus_speed_round_trip_matches_olx() {
    // From the oracle: OLX turns 6.60000134 into 6.60000086 between frames 7 and 8 of `fall`.
    let mut w = world(Material::Background);
    w.spawn_worm(100, 20);
    w.worms[0].vel.y = f32::from_bits(0x40d3_3336);
    w.tick_new(&[Keys(0)]); // frame 0 runs no physics step
    assert_eq!(w.worms()[0].vel.y.to_bits(), 0x40d3_3335);
}

#[test]
fn spawning_carves_the_largest_hole() {
    let mut w = world(Material::Dirt);
    w.spawn_worm(50, 50);
    assert_eq!(w.terrain().material(50, 50), Some(Material::Background));
    assert_eq!(w.terrain().material(43, 50), Some(Material::Background));
    assert_eq!(w.terrain().material(42, 50), Some(Material::Dirt));
    assert_eq!(w.worms()[0].dirt_count, 0, "spawning does not count as digging");
}

#[test]
fn weapon_combos_allow_changing_weapons_while_firing() {
    // OLX getInput: `if(slots > 0 && (!bShoot || allowCombo))`, and the
    // LX56 preset has WeaponCombos on.
    let mut w = world(Material::Background);
    w.spawn_worm(100, 20);
    let slot = crate::Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    let loadout = crate::Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 };
    w.set_loadout(0, &loadout);
    let held = Keys(Keys::CHANGE | Keys::FIRE);
    w.tick_new(&[held]);
    w.tick_new(&[held]);
    w.tick_new(&[Keys(held.0 | Keys::RIGHT)]); // frame 2: the first physics step reads the press
    assert_eq!(w.worms()[0].current_weapon, 1);
}

#[test]
fn instant_air_jump_lets_a_falling_worm_jump() {
    let mut settings = SimSettings::lx56();
    settings.instant_air_jump = true;
    let terrain = Terrain::from_grid(&MaterialGrid::new(200, 200, Material::Background));
    let mut w = World::test_new(settings, test_mod(), terrain, holes(), AbsTime(0), 1);
    w.spawn_worm(100, 20);
    w.tick_new(&[Keys(0)]);
    w.tick_new(&[Keys(0)]);
    w.tick_new(&[Keys(Keys::JUMP)]);
    assert!(w.worms()[0].vel.y < -60.0, "vy {}", w.worms()[0].vel.y);
}

#[test]
fn a_theme_without_holes_does_not_panic() {
    // OLX's CarveHole returns 0 when the theme lacks the hole image.
    let terrain = Terrain::from_grid(&MaterialGrid::new(50, 50, Material::Dirt));
    let mut w = World::test_new(SimSettings::lx56(), test_mod(), terrain, Vec::new(), AbsTime(0), 1);
    w.spawn_worm(25, 25);
    for _ in 0..20 {
        w.tick_new(&[Keys(Keys::RIGHT | Keys::LEFT)]);
    }
    assert_eq!(w.terrain().material(25, 25), Some(Material::Dirt));
}

/// Two worms digging, jumping and roping through dirt, varying every few frames.
fn busy_keys(frame: usize) -> [Keys; 2] {
    const SEQ: [u8; 6] = [
        Keys::RIGHT,
        Keys::RIGHT | Keys::LEFT,
        Keys::ROPE | Keys::UP,
        Keys::JUMP | Keys::LEFT,
        Keys::DOWN | Keys::RIGHT,
        0,
    ];
    [Keys(SEQ[(frame / 7) % 6]), Keys(SEQ[(frame / 5 + 3) % 6])]
}

#[test]
fn a_clone_evolves_identically() {
    let mut a = world(Material::Dirt);
    a.spawn_worm(60, 100);
    a.spawn_worm(140, 100);
    let start = a.clone();
    for f in 0..100 {
        a.tick_new(&busy_keys(f));
    }
    let mut b = a.clone();
    for f in 100..400 {
        a.tick_new(&busy_keys(f));
        b.tick_new(&busy_keys(f));
    }
    assert_eq!(a, b);
    assert_ne!(a.terrain().material_hash(), start.terrain().material_hash(), "the worms dug");
    assert_ne!(a.worms()[0].pos, start.worms()[0].pos, "the worms moved");
}

#[test]
fn a_rope_press_without_permission_still_clears_the_rope() {
    // OLX CNinjaRope::Shoot clears the rope before it checks canUseNinja.
    let mut w = world(Material::Background);
    w.spawn_worm(100, 100);
    for k in [0, 0, Keys::ROPE] {
        w.tick_new(&[Keys(k)]);
    }
    assert!(w.worms()[0].rope.released);
    w.worms[0].can_use_ninja = false;
    w.tick_new(&[Keys(0)]);
    w.tick_new(&[Keys(Keys::ROPE)]);
    assert!(!w.worms()[0].rope.released);
}

#[test]
fn every_rope_throw_counts_even_while_the_rope_is_out() {
    let mut w = world(Material::Background);
    w.spawn_worm(100, 100);
    let mut throws = vec![];
    // Each press is followed by idle frames: a frame may run no sub-step,
    // and the latched press is read by the next one that does.
    for k in [Keys::ROPE, Keys::ROPE, Keys::JUMP | Keys::ROPE, Keys::JUMP] {
        for k in [k, 0, 0] {
            w.tick_new(&[Keys(k)]);
        }
        throws.push((w.worms()[0].rope.released, w.worms()[0].rope.throws()));
    }
    assert_eq!(throws, [(true, 1), (true, 2), (true, 3), (false, 3)]);
    // A press that may not throw only clears the rope: no new throw.
    w.worms[0].can_use_ninja = false;
    w.tick_new(&[Keys(Keys::ROPE)]);
    assert_eq!(w.worms()[0].rope.throws(), 3);
}

#[test]
fn missing_keys_mean_nothing_is_held() {
    let mut w = world(Material::Background);
    w.spawn_worm(50, 100);
    w.spawn_worm(150, 100);
    for _ in 0..5 {
        w.tick_new(&[Keys(Keys::RIGHT), Keys(Keys::RIGHT)]);
    }
    w.tick_new(&[Keys(Keys::RIGHT)]);
    w.tick_new(&[Keys(Keys::RIGHT)]);
    assert!(w.worms()[0].state.moving);
    assert!(!w.worms()[1].state.moving);
}

#[test]
fn an_empty_map_does_not_panic() {
    let terrain = Terrain::from_grid(&MaterialGrid::new(0, 0, Material::Dirt));
    let mut w = World::test_new(SimSettings::lx56(), test_mod(), terrain, holes(), AbsTime(0), 1);
    w.spawn_worm(0, 0);
    for k in [0, 0, Keys::ROPE, 0, Keys::RIGHT, Keys::LEFT | Keys::RIGHT, Keys::JUMP] {
        w.tick_new(&[Keys(k)]);
    }
}

/// Holds right and taps left every 20 frames, as the `dig` scenario does.
fn dig_world(fill: Material) -> World {
    let mut w = world(fill);
    w.spawn_worm(100, 100);
    w
}

fn tap_frames(w: &mut World, n: usize) {
    for f in 0..n {
        let tap = f > 20 && f % 20 == 0;
        w.tick_new(&[Keys(if tap { Keys::RIGHT | Keys::LEFT } else { Keys::RIGHT })]);
    }
}

#[test]
fn digging_into_dirt_draws_six_and_fills_three_entities() {
    // OLX `CarveHole(CVec)` (CMap.cpp:2172-2199): 3 particles, each `GetRandomVec()`.
    let mut w = dig_world(Material::Dirt);
    tap_frames(&mut w, 200);
    let calls = w.rand_calls();
    assert!(calls > 0 && calls.is_multiple_of(6), "{calls} draws");
    assert_eq!(w.entity_count() as u64 * 2, calls);
}

#[test]
fn digging_in_the_air_draws_nothing() {
    let mut w = dig_world(Material::Background);
    tap_frames(&mut w, 40);
    assert_eq!((w.rand_calls(), w.entity_count()), (0, 0));
}

#[test]
fn the_rope_hooking_dirt_draws_ten_once() {
    // `simulateNinjarope` (PhysicsLX56.cpp:590-608): 5 particles on the first hit.
    let mut w = dig_world(Material::Dirt);
    for k in [0, 0, Keys::ROPE] {
        w.tick_new(&[Keys(k)]);
    }
    for _ in 0..30 {
        w.tick_new(&[Keys(0)]);
    }
    assert!(w.worms()[0].rope.attached);
    assert_eq!((w.rand_calls(), w.entity_count()), (10, 5));
}

#[test]
fn generators_are_seeded_from_the_match_seed() {
    let terrain = Terrain::from_grid(&MaterialGrid::new(10, 10, Material::Background));
    let a = World::test_new(SimSettings::lx56(), test_mod(), terrain.clone(), holes(), AbsTime(0), 7);
    let b = World::test_new(SimSettings::lx56(), test_mod(), terrain, holes(), AbsTime(0), 8);
    assert_ne!(a, b);
    assert_eq!((a.rand_calls(), a.mt_calls()), (0, 0));
}

#[test]
fn at_most_32_worms() {
    // OLX MAX_WORMS (include/Consts.h:34).
    let mut w = world(Material::Background);
    for i in 0..32 {
        assert_eq!(w.spawn_worm(10 + i * 5, 50), Some(i as usize));
    }
    assert_eq!(w.spawn_worm(10, 80), None);
    assert_eq!(w.worms().len(), 32);
}

#[test]
fn frame_dt_follows_game_speed() {
    // `Game::frameInner`: the frame clock moves 10 ms / GameSpeed, unless
    // GameSpeedOnlyForProjs; rl-scenario's `st` reads it.
    for (speed, only_projs, ms) in [(1.0, false, 10), (2.0, false, 5), (0.5, false, 20), (2.0, true, 10)] {
        let mut s = SimSettings::lx56();
        (s.game_speed, s.game_speed_only_for_projs) = (speed, only_projs);
        let terrain = Terrain::from_grid(&MaterialGrid::new(10, 10, Material::Background));
        let mut w = World::test_new(s, test_mod(), terrain, holes(), AbsTime(1000), 1);
        assert_eq!(w.frame_dt(), TimeDiff(ms));
        w.tick_new(&[]);
        assert_eq!(w.time(), AbsTime(1000 + ms));
    }
}

#[test]
fn find_spot_draws_a_position_without_rock() {
    // Rock everywhere but a 20 × 20 hole at (100..120, 50..70).
    let cells = (0..200 * 200)
        .map(|i| {
            let (x, y) = (i % 200, i / 200);
            if (100..120).contains(&x) && (50..70).contains(&y) {
                Material::Background
            } else {
                Material::Rock
            }
        })
        .collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(200, 200, cells).unwrap());
    let mut w = World::test_new(SimSettings::lx56(), test_mod(), terrain, holes(), AbsTime(0), 1);
    let (x, y) = w.find_spot();
    assert!((103..117).contains(&x) && (53..67).contains(&y), "({x}, {y})");
    assert!(w.mt_calls() >= 2 && w.rand_calls() == 0, "two FindSpot draws per try");
}
