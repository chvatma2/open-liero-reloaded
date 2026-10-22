//! Unit tests of [`get_input`]'s Strafe, Previous/Next weapon and
//! Weapon1-5 keys (`CWormHuman.cpp:430-456`, `:465-503`), through
//! [`crate::World`].
use alloc::vec;
use alloc::vec::Vec;

use olr_data::level::{Material, MaterialGrid};
use olr_math::AbsTime;
use olr_terrain::Terrain;

use crate::input::Keys;
use crate::worm::Dir;
use crate::{Loadout, SimSettings, Slot, World};

fn world(settings: SimSettings) -> World {
    world_of(settings, 5)
}

/// [`world`] with a worm of `slots` weapon slots.
fn world_of(settings: SimSettings, slots: usize) -> World {
    let terrain = Terrain::from_grid(&MaterialGrid::new(400, 200, Material::Background));
    let mut w =
        World::test_new(settings, crate::mod_rt::tests::test_mod(), terrain, Vec::new(), AbsTime(0), 1);
    w.spawn_worm(200, 100);
    let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    w.set_loadout(
        0,
        &Loadout { slots: vec![slot; slots], current_weapon: 0, lives: -2, kills: 0, deaths: 0 },
    );
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

#[test]
fn each_weapon_key_selects_its_slot() {
    let mut w = world(SimSettings::lx56());
    for (i, k) in Keys::WEAPONS.into_iter().enumerate().rev() {
        press(&mut w, 0, k);
        assert_eq!(w.worms()[0].current_weapon, i as i32, "Weapon{}", i + 1);
    }
}

#[test]
fn a_held_weapon_key_selects_again_at_every_read() {
    let mut w = world(SimSettings::lx56());
    hold(&mut w, Keys::WEAPON3, 3);
    assert_eq!(w.worms()[0].current_weapon, 2);
    // Next steps from Weapon3's slot in the read it is pressed in; the
    // next read selects Weapon3's slot again (`isDown`, not once).
    press(&mut w, Keys::WEAPON3, Keys::NEXT_WEAPON);
    hold(&mut w, 0, 2);
    assert_eq!(w.worms()[0].current_weapon, 2, "held through: selected again after Next");
    while w.substeps_ahead(0) == 0 {
        hold(&mut w, Keys::WEAPON3, 1);
    }
    w.tick_new(&[Keys(Keys::WEAPON3 | Keys::NEXT_WEAPON)]);
    assert_eq!(w.worms()[0].current_weapon, 3, "Weapon3, then Next, in one read");
    while w.substeps_ahead(0) == 0 {
        hold(&mut w, Keys::WEAPON3, 1);
    }
    hold(&mut w, Keys::WEAPON3, 1);
    assert_eq!(w.worms()[0].current_weapon, 2, "still held: selected again");
}

#[test]
fn a_tapped_weapon_key_is_seen_by_the_next_read() {
    let mut w = world(SimSettings::lx56());
    while w.substeps_ahead(0) != 0 {
        hold(&mut w, 0, 1);
    }
    // Pressed and released between two reads.
    w.tick_new(&[Keys(Keys::WEAPON4)]);
    assert_eq!(w.substeps_last_tick(), 0, "a tick without a read");
    hold(&mut w, 0, 2);
    assert_eq!(w.worms()[0].current_weapon, 3);
}

#[test]
fn with_two_weapon_keys_held_the_highest_wins() {
    let mut w = world(SimSettings::lx56());
    press(&mut w, 0, Keys::WEAPON2 | Keys::WEAPON4);
    assert_eq!(w.worms()[0].current_weapon, 3);
    press(&mut w, 0, Keys::WEAPON5 | Keys::WEAPON1);
    assert_eq!(w.worms()[0].current_weapon, 4);
}

#[test]
fn a_weapon_key_beyond_the_slots_does_nothing_in_play_and_wraps_with_olx_quirks() {
    let mut w = world_of(SimSettings::lx56(), 3);
    press(&mut w, 0, Keys::WEAPON3);
    assert_eq!(w.worms()[0].current_weapon, 2);
    press(&mut w, 0, Keys::WEAPON5);
    assert_eq!(w.worms()[0].current_weapon, 2, "no fifth slot: nothing");
    press(&mut w, 0, Keys::WEAPON4);
    assert_eq!(w.worms()[0].current_weapon, 2, "no fourth slot: nothing");
    press(&mut w, 0, Keys::WEAPON2 | Keys::WEAPON5);
    assert_eq!(w.worms()[0].current_weapon, 1, "the keys in range still select");

    let mut settings = SimSettings::lx56();
    settings.olx_quirks = true;
    let mut w = world_of(settings, 3);
    press(&mut w, 0, Keys::WEAPON5);
    assert_eq!(w.worms()[0].current_weapon, 1, "OLX's MOD: 4 % 3");
    press(&mut w, 0, Keys::WEAPON4);
    assert_eq!(w.worms()[0].current_weapon, 0, "3 % 3");
}

#[test]
fn weapon_keys_wait_while_firing_without_combos() {
    let mut settings = SimSettings::lx56();
    settings.weapon_combos = false;
    let mut w = world(settings);
    press(&mut w, Keys::FIRE, Keys::WEAPON3);
    assert_eq!(w.worms()[0].current_weapon, 0, "not while firing");
    press(&mut w, 0, Keys::WEAPON3);
    assert_eq!(w.worms()[0].current_weapon, 2);

    let mut w = world(SimSettings::lx56());
    press(&mut w, Keys::FIRE, Keys::WEAPON3);
    assert_eq!(w.worms()[0].current_weapon, 2, "WeaponCombos: while firing too");
}

#[test]
fn weapon_keys_come_after_select_weapon_and_before_previous_and_next() {
    let mut w = world(SimSettings::lx56());
    hold(&mut w, Keys::CHANGE, 1);
    press(&mut w, Keys::CHANGE, Keys::RIGHT | Keys::WEAPON3);
    assert_eq!(w.worms()[0].current_weapon, 2, "SelectWeapon + Right, then Weapon3: Weapon3's slot");
    press(&mut w, 0, Keys::WEAPON2 | Keys::NEXT_WEAPON);
    assert_eq!(w.worms()[0].current_weapon, 2, "Weapon2, then Next");
    press(&mut w, 0, Keys::WEAPON5 | Keys::NEXT_WEAPON);
    assert_eq!(w.worms()[0].current_weapon, 0, "Weapon5, then Next, wrapping");
    press(&mut w, 0, Keys::WEAPON1 | Keys::PREV_WEAPON);
    assert_eq!(w.worms()[0].current_weapon, 4, "Weapon1, then Previous, wrapping");
}

#[test]
fn a_dead_worm_ignores_the_weapon_keys() {
    let mut w = world(SimSettings::lx56());
    w.worms[0].alive = false;
    hold(&mut w, Keys::WEAPON4, 10);
    assert!(!w.worms()[0].alive);
    assert_eq!(w.worms()[0].current_weapon, 0);
}

#[test]
fn weapon_keys_pressed_and_released_hash_as_never_pressed() {
    let mut a = world(SimSettings::lx56());
    let mut b = world(SimSettings::lx56());
    hold(&mut a, Keys::WEAPON1, 1);
    hold(&mut b, 0, 1);
    hold(&mut a, 0, 3);
    hold(&mut b, 0, 3);
    assert_eq!(a.checksum(), b.checksum(), "the first slot again, read: back at rest");
}
