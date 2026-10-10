//! Unit tests of the tick events and the checksum.
use super::*;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::{ActionDef, ActionKind, WeaponKind};
use rl_math::AbsTime;
use rl_terrain::{Stamp, Terrain};

use crate::damage::injure_worm;
use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
use crate::{GAME_SOUND_PREFIX, GAME_SOUNDS, Keys, Loadout, SimMod, SimSettings, Slot, World};

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

/// A gun that fires once a second, with a firing sound; its pellet explodes
/// on the map (damage 5) with a hit sound.
fn gun_mod() -> Arc<SimMod> {
    let mut weapon = projectile_weapon("gun", "pellet");
    weapon.rof = 1.0;
    if let WeaponKind::Projectile { sound, .. } = &mut weapon.kind {
        *sound = Some("sfx/gun.wav".into());
    }
    let mut pellet = pixel_projectile(None);
    pellet.gravity = Some(0);
    pellet.hit = ActionDef {
        damage: 5,
        sound: Some("sfx/boom.wav".into()),
        ..ActionDef::olx_default(ActionKind::Explode)
    };
    Arc::new(SimMod::compile(&mod_def(vec![weapon], vec![("pellet", pellet)]), &BTreeMap::new()).unwrap())
}

/// Open air left of `x = 120`, dirt from there on, and a dirt floor below `y = 150`.
fn walled_world() -> World {
    let cells = (0..200 * 200)
        .map(|i| if i % 200 >= 120 || i / 200 >= 150 { Material::Dirt } else { Material::Background })
        .collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(200, 200, cells).unwrap());
    let mut w = World::new(SimSettings::lx56(), gun_mod(), terrain, holes(), AbsTime(100_000), 1);
    w.spawn_worm(60, 140);
    let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    w.set_loadout(0, &Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 });
    w
}

#[test]
fn events_are_emitted_once_per_cause() {
    let mut w = walled_world();
    let setup: Vec<Event> = w.tick(&[Keys(Keys::FIRE)]).iter().copied().collect();
    assert!(setup.contains(&Event::Spawned { worm: 0 }), "setup events come with the first tick: {setup:?}");
    assert!(setup.contains(&Event::TerrainEdit {
        op: TerrainOp::Carve,
        stamp: StampId::Hole(4),
        x: 60,
        y: 140
    }));
    let mut all = Vec::new();
    for f in 1..100 {
        let keys = if f < 4 { Keys(Keys::FIRE) } else { Keys(0) };
        all.extend(w.tick(&[keys]).iter().copied());
    }
    let count = |f: &dyn Fn(&Event) -> bool| all.iter().filter(|e| f(e)).count();
    let sounds = w.sim_mod().sounds();
    let id = |name: &str| SoundId(sounds.iter().position(|s| s == name).unwrap() as u16);
    assert_eq!(count(&|e| matches!(e, Event::WeaponFired { worm: 0, weapon: 0 })), 1, "{all:?}");
    assert_eq!(count(&|e| matches!(e, Event::Sound { id: s, .. } if *s == id("sfx/gun.wav"))), 1);
    assert_eq!(count(&|e| matches!(e, Event::Explosion { damage: 5.0, .. })), 1, "{all:?}");
    assert_eq!(count(&|e| matches!(e, Event::Explosion { .. })), 1);
    assert_eq!(count(&|e| matches!(e, Event::Sound { id: s, .. } if *s == id("sfx/boom.wav"))), 1);
    // One hole, of size 5 (the largest, index 4), where the pellet hit the wall.
    let edits: Vec<&Event> = all.iter().filter(|e| matches!(e, Event::TerrainEdit { .. })).collect();
    assert_eq!(edits.len(), 1, "{edits:?}");
    let Event::TerrainEdit { op: TerrainOp::Carve, stamp: StampId::Hole(4), x, .. } = *edits[0] else {
        panic!("{edits:?}")
    };
    assert!((115..=125).contains(&x), "{x}");
    // The explosion entity (size 8 below damage 6) and its two dirt particles.
    assert_eq!(
        count(&|e| matches!(e, Event::SpawnCosmetic { kind: CosmeticKind::Explosion { size: 8 }, .. })),
        1
    );
    assert_eq!(count(&|e| matches!(e, Event::SpawnCosmetic { kind: CosmeticKind::Dirt, .. })), 2);
}

#[test]
fn a_death_reports_damage_the_kill_its_sound_and_gore() {
    let mut w = walled_world();
    w.spawn_worm(80, 140);
    w.tick(&[]);
    w.draws.ents = 1024; // A full entity pool still reports every cosmetic.
    w.draws.events.begin_tick();
    injure_worm(&mut w.harm(), 1, 150.0, 0);
    let ev = w.draws.events.as_slice();
    assert_eq!(ev[0], Event::Damage { worm: 1, amount: 150.0, by: 0 });
    assert_eq!(ev[1], Event::Kill { victim: 1, killer: 0 });
    assert!(matches!(ev[2], Event::Sound { id, .. } if SoundId::DEATH.contains(&id)), "{:?}", ev[2]);
    let kind = |k: CosmeticKind| {
        ev.iter().filter(|e| matches!(e, Event::SpawnCosmetic { kind, .. } if *kind == k)).count()
    };
    assert_eq!(kind(CosmeticKind::Gib), 7);
    // 50 death rounds and 100 hit rounds (clamp(150, 10, 100)), 3 entities each.
    assert_eq!(kind(CosmeticKind::BloodDropper), 150);
    assert_eq!(kind(CosmeticKind::Blood { red: 200 }) + kind(CosmeticKind::Blood { red: 128 }), 300);
    assert_eq!(ev.len(), 3 + 7 + 450);
    let pos = w.worms()[1].pos;
    assert!(ev[3..].iter().all(|e| matches!(e, Event::SpawnCosmetic { pos: p, .. } if *p == pos)));
}

#[test]
fn the_sound_table_lists_game_sounds_then_mod_sounds_once() {
    let m = gun_mod();
    assert_eq!(&m.sounds()[..GAME_SOUNDS.len()], &GAME_SOUNDS);
    assert_eq!(&m.sounds()[GAME_SOUNDS.len()..], ["sfx/gun.wav", "sfx/boom.wav"]);
    assert_eq!(m.weapons()[0].sound, Some(SoundId(6)));
    assert_eq!(m.projectiles()[0].hit_sound, Some(SoundId(7)));
    assert_eq!(m.projectiles()[0].player_hit_sound, None);
}

#[test]
fn game_sounds_are_prefixed_and_mod_sounds_never_are() {
    assert!(GAME_SOUNDS.iter().all(|s| s.starts_with(GAME_SOUND_PREFIX)));
    let m = gun_mod();
    assert!(m.sounds()[GAME_SOUNDS.len()..].iter().all(|s| !s.starts_with(GAME_SOUND_PREFIX)));
    // A mod naming a game sound by its table path gets no sound, not OLX's.
    let mut weapon = projectile_weapon("gun", "pellet");
    if let WeaponKind::Projectile { sound, .. } = &mut weapon.kind {
        *sound = Some(GAME_SOUNDS[0].into());
    }
    let m =
        SimMod::compile(&mod_def(vec![weapon], vec![("pellet", pixel_projectile(None))]), &BTreeMap::new())
            .unwrap();
    assert_eq!(m.weapons()[0].sound, None);
    assert_eq!(m.sounds().len(), GAME_SOUNDS.len());
}

#[test]
fn cosmetics_leave_room_for_gameplay_events() {
    let mut e = TickEvents::new();
    for _ in 0..TickEvents::CAPACITY {
        e.cosmetic(CosmeticKind::Smoke, rl_math::Vec2::ZERO, rl_math::Vec2::ZERO);
    }
    assert_eq!(e.len(), TickEvents::COSMETIC_LIMIT);
    for _ in 0..TickEvents::CAPACITY {
        e.push(Event::Spawned { worm: 0 });
    }
    assert_eq!(e.len(), TickEvents::CAPACITY);
    assert_eq!(e.dropped() as usize, 2 * TickEvents::CAPACITY - TickEvents::CAPACITY);
    let c = e.clone();
    assert!(c.list.capacity() >= TickEvents::CAPACITY);
    e.begin_tick();
    assert!(e.is_empty() && e.dropped() == 0);
}

#[test]
fn checksum_changes_with_state() {
    let mut w = walled_world();
    let c0 = w.checksum();
    assert_eq!(w.clone().checksum(), c0, "a clone has the same checksum");
    assert_eq!(walled_world().checksum(), c0, "the same setup has the same checksum");
    // Every kind of state: a worm field, a slot, the rope, a latch, the terrain,
    // the generators, the projectiles, the bonuses and the clocks.
    let changes: [fn(&mut World); 10] = [
        |w| w.worms[0].health = 99.0,
        |w| w.worms[0].slots[3].charge = 0.5,
        |w| w.worms[0].rope.vel.x = -0.0,
        |w| w.latches[0].set(Keys(Keys::UP)),
        |w| {
            w.spawn_worm(150, 50);
        },
        |w| {
            w.draws.rand.random_num();
        },
        |w| {
            w.draws.mt.next_u32();
        },
        |w| w.place_bonus(10, 10, crate::BonusItem::Health),
        |w| w.time.0 += 1,
        |w| w.worms[0].pos_recorder.push(rl_math::Vec2::new(1.0, 2.0)),
    ];
    for (i, change) in changes.iter().enumerate() {
        let mut v = w.clone();
        change(&mut v);
        assert_ne!(v.checksum(), c0, "change {i}");
    }
    let mut v = w.clone();
    v.draws.events.push(Event::Spawned { worm: 3 });
    assert_eq!(v.checksum(), c0, "events are output, not state");
    w.tick(&[Keys(Keys::FIRE)]);
    let c1 = w.checksum();
    assert_ne!(c1, c0);
    let mut t = w.clone();
    for _ in 0..30 {
        w.tick(&[Keys(Keys::FIRE)]);
        t.tick(&[Keys(Keys::FIRE)]);
        assert_eq!(w.checksum(), t.checksum());
    }
    assert!(w.projectiles().count() > 0 || w.worms()[0].dirt_count != 0);
    let mut v = w.clone();
    let first = v.projectiles().next().map(|(i, _)| i);
    if let Some(i) = first {
        v.projectiles.slot_mut(i).life += 1.0;
        assert_ne!(v.checksum(), w.checksum(), "a projectile");
    }
}
