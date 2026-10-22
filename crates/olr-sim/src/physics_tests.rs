//! Unit tests of [`get_input`]'s Strafe and Previous/Next weapon keys
//! (`CWormHuman.cpp:443-456`, `:465-503`), through [`crate::World`].
use alloc::vec;
use alloc::vec::Vec;

use olr_data::level::{Material, MaterialGrid};
use olr_math::AbsTime;
use olr_terrain::Terrain;

use crate::input::Keys;
use crate::worm::Dir;
use crate::{Loadout, SimSettings, Slot, World};

fn world(settings: SimSettings) -> World {
    let terrain = Terrain::from_grid(&MaterialGrid::new(400, 200, Material::Background));
    let mut w =
        World::test_new(settings, crate::mod_rt::tests::test_mod(), terrain, Vec::new(), AbsTime(0), 1);
    w.spawn_worm(200, 100);
    let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    w.set_loadout(0, &Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 });
    // Frames 0 and 1 run no physics step (`physics_steps_follow_the_two_clocks`).
    w.tick_new(&[Keys(0)]);
    w.tick_new(&[Keys(0)]);
    w
}

/// Ticks `n` frames holding `k`.
fn hold(w: &mut World, k: u16, n: usize) {
    for _ in 0..n {
        w.tick_new(&[Keys(k)]);
    }
}

/// Presses `k` (on top of `held`) for one frame, then holds `held` until
/// a physics step has read it.
fn press(w: &mut World, held: u16, k: u16) {
    w.tick_new(&[Keys(held | k)]);
    hold(w, held, 2);
}

#[test]
fn strafe_moves_both_ways_without_turning() {
    let mut w = world(SimSettings::lx56());
    assert_eq!(w.worms()[0].face, Dir::Right);
    let x0 = w.worms()[0].pos.x;
    hold(&mut w, Keys::STRAFE | Keys::LEFT, 30);
    let worm = &w.worms()[0];
    assert_eq!((worm.face, worm.move_dir), (Dir::Right, Dir::Left));
    assert!(worm.pos.x < x0 - 1.0, "moved left: {} -> {}", x0, worm.pos.x);

    // Turned left without Strafe, then strafing right keeps facing left.
    hold(&mut w, Keys::LEFT, 3);
    assert_eq!(w.worms()[0].face, Dir::Left);
    let x1 = w.worms()[0].pos.x;
    hold(&mut w, Keys::STRAFE | Keys::RIGHT, 30);
    let worm = &w.worms()[0];
    assert_eq!((worm.face, worm.move_dir), (Dir::Left, Dir::Right));
    assert!(worm.pos.x > x1 + 1.0, "moved right: {} -> {}", x1, worm.pos.x);

    // Released, Right turns the worm again.
    hold(&mut w, Keys::RIGHT, 3);
    assert_eq!(w.worms()[0].face, Dir::Right);
}

#[test]
fn strafe_alone_changes_nothing() {
    let mut a = world(SimSettings::lx56());
    let mut b = world(SimSettings::lx56());
    for f in 0..60u16 {
        let k = if f % 7 < 4 { Keys::LEFT | Keys::UP } else { Keys::RIGHT | Keys::DOWN };
        a.tick_new(&[Keys(k)]);
        b.tick_new(&[Keys(k)]);
    }
    hold(&mut a, Keys::STRAFE, 1);
    hold(&mut b, 0, 1);
    hold(&mut a, 0, 3);
    hold(&mut b, 0, 3);
    assert_eq!(a.checksum(), b.checksum(), "Strafe pressed and released, read: back at rest");
}

#[test]
fn previous_and_next_weapon_wrap() {
    let mut w = world(SimSettings::lx56());
    let slot = |w: &World| w.worms()[0].current_weapon;
    press(&mut w, 0, Keys::PREV_WEAPON);
    assert_eq!(slot(&w), 4, "0 - 1 wraps to the last slot");
    press(&mut w, 0, Keys::NEXT_WEAPON);
    assert_eq!(slot(&w), 0, "4 + 1 wraps to the first");
    press(&mut w, 0, Keys::NEXT_WEAPON);
    assert_eq!(slot(&w), 1);
    press(&mut w, 0, Keys::PREV_WEAPON | Keys::NEXT_WEAPON);
    assert_eq!(slot(&w), 1, "both at once: next - prev = 0");
    // Held, they do not repeat: `isDownOnce`.
    hold(&mut w, Keys::NEXT_WEAPON, 20);
    assert_eq!(slot(&w), 2);
}

#[test]
fn previous_and_next_weapon_stack_with_select_weapon() {
    let mut w = world(SimSettings::lx56());
    hold(&mut w, Keys::CHANGE, 1);
    press(&mut w, Keys::CHANGE, Keys::RIGHT | Keys::NEXT_WEAPON);
    assert_eq!(w.worms()[0].current_weapon, 2, "SelectWeapon + Right, then Next, in one read");
    press(&mut w, Keys::CHANGE, Keys::RIGHT | Keys::PREV_WEAPON);
    assert_eq!(w.worms()[0].current_weapon, 2, "+1 - 1");
    press(&mut w, Keys::CHANGE, Keys::LEFT | Keys::PREV_WEAPON);
    assert_eq!(w.worms()[0].current_weapon, 0);
}

#[test]
fn previous_and_next_weapon_wait_while_firing_without_combos() {
    let mut settings = SimSettings::lx56();
    settings.weapon_combos = false;
    let mut w = world(settings);
    press(&mut w, Keys::FIRE, Keys::NEXT_WEAPON);
    assert_eq!(w.worms()[0].current_weapon, 0, "not while firing");
    press(&mut w, 0, Keys::NEXT_WEAPON);
    assert_eq!(w.worms()[0].current_weapon, 1);

    let mut w = world(SimSettings::lx56());
    press(&mut w, Keys::FIRE, Keys::NEXT_WEAPON);
    assert_eq!(w.worms()[0].current_weapon, 1, "WeaponCombos: while firing too");
}
