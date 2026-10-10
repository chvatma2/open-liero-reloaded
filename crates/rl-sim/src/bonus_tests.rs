//! Unit tests of the bonuses, through [`crate::World`].
use super::*;
use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
use crate::{Keys, Loadout, SimMod, SimSettings, Slot, World};
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_math::{GlibcRand, TimeDiff, Vec2};
use rl_terrain::{Stamp, Terrain};

const SEED: u32 = 7;
const START: AbsTime = AbsTime(100_000);
/// `LX56PhysicsDT` at 84 fps.
const DT: TimeDiff = TimeDiff(11);

/// Two weapons, so a weapon bonus can replace one with the other.
fn two_weapons() -> Arc<SimMod> {
    let def = mod_def(
        vec![projectile_weapon("minigun", "pellet"), projectile_weapon("cannon", "pellet")],
        vec![("pellet", pixel_projectile(None))],
    );
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap())
}

/// The Dirt theme's hole sizes (hole1..hole5) as full squares.
fn holes() -> Vec<Stamp> {
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

fn bonus_settings() -> SimSettings {
    SimSettings { bonuses: true, ..SimSettings::lx56() }
}

/// 200 × 200: open air above a dirt floor from row 150 down.
fn world_with(settings: SimSettings) -> World {
    let cells =
        (0..200 * 200).map(|i| if i / 200 < 150 { Material::Background } else { Material::Dirt }).collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(200, 200, cells).unwrap());
    World::new(settings, two_weapons(), terrain, holes(), START, SEED)
}

fn world() -> World {
    world_with(bonus_settings())
}

/// A worm at `(x, y)` holding weapon 0 in all five slots.
fn armed_worm(w: &mut World, x: i32, y: i32) -> usize {
    let i = w.spawn_worm(x, y).unwrap();
    let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    w.set_loadout(i, &Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 });
    i
}

fn ticks(w: &mut World, n: usize) {
    for _ in 0..n {
        w.tick_new(&[Keys(0)]);
    }
}

#[test]
fn bonus_falls_and_rests_on_dirt() {
    let mut w = world();
    w.place_bonus(100, 100, BonusItem::Health);
    w.place_bonus(50, 160, BonusItem::Weapon(1));
    assert!(!w.bonuses()[0].used, "placed at the next tick");
    // Frames 0 and 1 run no physics sub-step.
    ticks(&mut w, 2);
    let b = w.bonuses()[0];
    assert!(b.used && b.kind == BonusKind::Health && b.weapon == 0, "{b:?}");
    assert_eq!((b.pos, b.vel, b.life), (Vec2::new(100.0, 100.0), Vec2::ZERO, 0.0));
    // The spawn hole (size 4, 15 × 15) is carved on the client, and ENT_SPAWN takes a slot.
    assert_eq!(w.terrain().material(50, 160), Some(Material::Background));
    assert_eq!(w.terrain().material(57, 167), Some(Material::Background));
    assert_eq!(w.terrain().material(58, 160), Some(Material::Dirt));
    assert_eq!((w.entity_count(), w.rand_calls(), w.mt_calls()), (2, 0, 0));
    // One sub-step (PhysicsLX56.cpp:624-697), as OLX's `bonus-health` frame 5.
    ticks(&mut w, 1);
    let dt = DT.seconds();
    let b = w.bonuses()[0];
    let vy = 80.0 * dt;
    assert_eq!((b.vel.y.to_bits(), b.life), (vy.to_bits(), dt));
    assert_eq!(b.pos.y.to_bits(), (100.0 + vy * dt).to_bits());
    // It falls straight down until a dirt pixel is in the 5 × 5 box, then
    // stays 2 px above the row it found, at rest.
    ticks(&mut w, 300);
    let b = w.bonuses()[0];
    assert_eq!((b.pos, b.vel), (Vec2::new(100.0, 148.0), Vec2::ZERO));
    assert_eq!(w.bonuses()[1].pos, Vec2::new(50.0, 166.0), "on the spawn hole's floor");
}

#[test]
fn bonuses_off_neither_fall_nor_count() {
    let mut w = world_with(SimSettings::lx56());
    let i = armed_worm(&mut w, 100, 140);
    w.set_health(i, 50.0);
    w.place_bonus(100, 100, BonusItem::Health);
    ticks(&mut w, 50);
    let b = w.bonuses()[0];
    assert!(b.used && b.pos == Vec2::new(100.0, 100.0) && b.life == 0.0, "{b:?}");
    assert_eq!(w.worms()[0].health, 50.0);
}

#[test]
fn health_bonus_heals_with_olx_draw() {
    let mut w = world();
    let i = armed_worm(&mut w, 100, 140);
    w.set_health(i, 50.0);
    w.place_bonus(100, 140, BonusItem::Health);
    // The first sub-step (frame 2) picks it up: `GetRandomInt(40) + 10`.
    ticks(&mut w, 3);
    let heal = (GlibcRand::new(SEED).random_int(40) + 10) as f32;
    assert_eq!(w.worms()[0].health, 50.0 + heal);
    assert_eq!(w.rand_calls(), 1, "one rand(), no blood for negative damage");
    assert!(!w.bonuses()[0].used && w.server_bonuses()[0].used);

    // Clamped to 100 - health; at full health a bonus stays on the map.
    let mut w = world();
    let i = armed_worm(&mut w, 100, 140);
    w.set_health(i, 95.0);
    w.place_bonus(100, 140, BonusItem::Health);
    w.place_bonus(102, 140, BonusItem::Health);
    ticks(&mut w, 3);
    assert_eq!((w.worms()[0].health, w.rand_calls()), (100.0, 1));
    assert!(!w.bonuses()[0].used && w.bonuses()[1].used, "the second one stays: health is 100");
}

#[test]
fn server_slot_stays_busy_after_pickup() {
    let mut w = world_with(SimSettings { bonus_life: 1.0, ..bonus_settings() });
    armed_worm(&mut w, 100, 140);
    w.place_bonus(100, 140, BonusItem::Weapon(1));
    ticks(&mut w, 3);
    // Picked up in frame 2: the current slot holds the Cannon, full.
    assert_eq!(w.worms()[0].slots[0], Slot { weapon: 1, charge: 1.0, reloading: false, last_fire: 0.0 });
    assert!(!w.bonuses()[0].used && w.server_bonuses()[0].used);
    // The grab reaches the server before the next frame, which sets the
    // same slot (the current one at pickup) again: full and not reloading.
    w.worm_mut(0).slots[0] = Slot { weapon: 0, charge: 0.25, reloading: true, last_fire: 0.1 };
    w.worm_mut(0).current_weapon = 2;
    ticks(&mut w, 1);
    let s = w.worms()[0].slots[0];
    assert_eq!((s.weapon, s.charge, s.reloading), (1, 1.0, false), "{s:?}");
    // The server's slot is still busy, so the next bonus takes slot 1 on both sides.
    w.place_bonus(150, 100, BonusItem::Health);
    ticks(&mut w, 1); // frame 4
    assert!(w.server_bonuses()[0].used && w.server_bonuses()[1].used && w.bonuses()[1].used);
    // `currentTime - spawnTime > BonusLife`: placed at frame 0 (spawn time
    // START + 10), freed in frame 101 (currentTime START + 1020).
    ticks(&mut w, 96); // frames 5..=100
    assert!(w.server_bonuses()[0].used);
    ticks(&mut w, 1);
    assert!(!w.server_bonuses()[0].used);
    // Slot 1 (placed at frame 4) frees in frame 105; S2C_DESTROYBONUS
    // reaches the client before frame 106.
    ticks(&mut w, 4);
    assert!(!w.server_bonuses()[1].used && w.bonuses()[1].used);
    ticks(&mut w, 1);
    assert!(!w.bonuses()[1].used);
}

#[test]
fn natural_bonuses_spawn_by_frequency_at_find_spot() {
    let mut w = world_with(SimSettings { bonus_frequency: 1.0, ..bonus_settings() });
    // `currentTime - fLastBonusTime > BonusFrequency`, the timer starting at
    // START + 10: frame 101 (currentTime START + 1020).
    ticks(&mut w, 101);
    assert!(w.server_bonuses().iter().all(|b| !b.used));
    ticks(&mut w, 1);
    // Expected values computed outside rl-math (glibc's `srandom(7)`, and boost's
    // `mt19937(7)` in Python): rand() 1045618677 gives `GetRandomInt(999)` 486,
    // below 0.5 * 1000, so a weapon bonus; rand() 1863967299 gives
    // `GetRandomLong(1)` 1; FindSpot's first try is good, at the first two
    // `rnd()` draws times 200. The `bonus-natural` oracle scenario checks the
    // same path against OLX.
    let s = w.server_bonuses()[0];
    let pos = Vec2::new(f32::from_bits(1098133441), f32::from_bits(1110826763));
    assert_eq!((pos.x, pos.y), (15.261659, 45.467815));
    assert!(s.used && (s.pos, s.kind, s.weapon) == (pos, BonusKind::Weapon, 1), "{s:?}");
    assert_eq!(s.spawn_time, START + TimeDiff(1020));
    assert_eq!((w.rand_calls(), w.mt_calls()), (2, 2));
    // S2C_SPAWNBONUS arrives before the next frame, at the integer position.
    assert!(!w.bonuses()[0].used);
    ticks(&mut w, 1);
    let c = w.bonuses()[0];
    assert!(c.used && c.pos.x == 15.0 && c.kind == BonusKind::Weapon, "{c:?}");
    // The next one 1 s after the last spawn: frame 202.
    ticks(&mut w, 99);
    assert!(!w.server_bonuses()[1].used);
    ticks(&mut w, 1);
    assert!(w.server_bonuses()[1].used);
}

#[test]
fn without_bonus_weapons_only_health_bonuses_spawn() {
    let mut w = world_with(SimSettings { bonus_frequency: 1.0, ..bonus_settings() });
    w.set_bonus_weapons(&[]);
    ticks(&mut w, 102);
    let mut r = GlibcRand::new(SEED);
    let health = r.random_int(999) >= 500;
    // No weapon to choose: no second draw, and a weapon bonus is not spawned.
    assert_eq!((w.server_bonuses()[0].used, w.rand_calls()), (health, 1));
    if health {
        assert_eq!(w.server_bonuses()[0].weapon, -1);
    }
}
