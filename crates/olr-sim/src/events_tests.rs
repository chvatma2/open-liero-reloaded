//! Unit tests of the tick events and the checksum.
use super::*;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_data::mod_def::{ActionDef, ActionKind, WeaponKind};
use olr_math::AbsTime;
use olr_terrain::{Stamp, Terrain};

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
    let mut w = World::test_new(SimSettings::lx56(), gun_mod(), terrain, holes(), AbsTime(100_000), 1);
    w.spawn_worm(60, 140);
    let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    w.set_loadout(0, &Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 });
    w
}

#[test]
fn events_are_emitted_once_per_cause() {
    let mut w = walled_world();
    let setup: Vec<EventKind> = w.tick_new(&[Keys(Keys::FIRE)]).iter().map(|e| e.kind).collect();
    assert!(
        setup.contains(&EventKind::Spawned { worm: 0 }),
        "setup events come with the first tick: {setup:?}"
    );
    assert!(setup.contains(&EventKind::TerrainEdit {
        op: TerrainOp::Carve,
        stamp: StampId::Hole(4),
        x: 60,
        y: 140
    }));
    let mut all = Vec::new();
    for f in 1..100 {
        let keys = if f < 4 { Keys(Keys::FIRE) } else { Keys(0) };
        all.extend(w.tick_new(&[keys]).iter().map(|e| e.kind));
    }
    let count = |f: &dyn Fn(&EventKind) -> bool| all.iter().filter(|e| f(e)).count();
    let sounds = w.sim_mod().sounds();
    let id = |name: &str| SoundId(sounds.iter().position(|s| s == name).unwrap() as u16);
    assert_eq!(count(&|e| matches!(e, EventKind::WeaponFired { worm: 0, weapon: 0 })), 1, "{all:?}");
    assert_eq!(count(&|e| matches!(e, EventKind::Sound { id: s, .. } if *s == id("sfx/gun.wav"))), 1);
    assert_eq!(count(&|e| matches!(e, EventKind::Explosion { damage: 5.0, .. })), 1, "{all:?}");
    assert_eq!(count(&|e| matches!(e, EventKind::Explosion { .. })), 1);
    assert_eq!(count(&|e| matches!(e, EventKind::Sound { id: s, .. } if *s == id("sfx/boom.wav"))), 1);
    // One hole, of size 5 (the largest, index 4), where the pellet hit the wall.
    let edits: Vec<&EventKind> = all.iter().filter(|e| matches!(e, EventKind::TerrainEdit { .. })).collect();
    assert_eq!(edits.len(), 1, "{edits:?}");
    let EventKind::TerrainEdit { op: TerrainOp::Carve, stamp: StampId::Hole(4), x, .. } = *edits[0] else {
        panic!("{edits:?}")
    };
    assert!((115..=125).contains(&x), "{x}");
    // The explosion entity (size 8 below damage 6) and its two dirt particles.
    assert_eq!(
        count(&|e| matches!(e, EventKind::SpawnCosmetic { kind: CosmeticKind::Explosion { size: 8 }, .. })),
        1
    );
    assert_eq!(count(&|e| matches!(e, EventKind::SpawnCosmetic { kind: CosmeticKind::Dirt { .. }, .. })), 2);
}

#[test]
fn a_death_reports_damage_the_kill_its_sound_and_gore() {
    let mut w = walled_world();
    w.spawn_worm(80, 140);
    w.tick_new(&[]);
    w.draws.ents = 1024; // A full entity pool still reports every cosmetic.
    injure_worm(&mut w.harm(), 1, 150.0, 0);
    let ev: Vec<EventKind> = w.draws.events.iter().map(|e| e.kind).collect();
    assert_eq!(ev[0], EventKind::Damage { worm: 1, amount: 150.0, by: 0, dealt: 100.0 });
    assert_eq!(ev[1], EventKind::Kill { victim: 1, killer: 0 });
    assert!(matches!(ev[2], EventKind::Sound { id, .. } if SoundId::DEATH.contains(&id)), "{:?}", ev[2]);
    let kind = |k: CosmeticKind| {
        ev.iter().filter(|e| matches!(e, EventKind::SpawnCosmetic { kind, .. } if *kind == k)).count()
    };
    assert_eq!(kind(CosmeticKind::Gib { worm: 1 }), 7);
    // 50 death rounds and 100 hit rounds (clamp(150, 10, 100)), 3 entities each.
    assert_eq!(kind(CosmeticKind::BloodDropper), 150);
    assert_eq!(kind(CosmeticKind::Blood { red: 200 }) + kind(CosmeticKind::Blood { red: 128 }), 300);
    assert_eq!(ev.len(), 3 + 7 + 450);
    let pos = w.worms()[1].pos;
    assert!(ev[3..].iter().all(|e| matches!(e, EventKind::SpawnCosmetic { pos: p, .. } if *p == pos)));
}

#[test]
fn a_death_keeps_its_sound_gibs_and_blood_when_the_cosmetics_are_full() {
    // Blood of earlier hits must not crowd out a death's effects: its sound,
    // gibs and blood are pushed as gameplay, and marked as the death's.
    let mut w = walled_world();
    w.spawn_worm(80, 140);
    w.tick_new(&[]);
    let zero = olr_math::Vec2::ZERO;
    for _ in 0..TickEvents::COSMETIC_LIMIT {
        w.draws.events.cosmetic(CosmeticKind::Smoke, zero, zero);
    }
    injure_worm(&mut w.harm(), 1, 150.0, 0);
    let e = &w.draws.events;
    let death: Vec<EventKind> = e.iter().filter(|e| e.death).map(|e| e.kind).collect();
    assert!(
        matches!(death[0], EventKind::Sound { id, .. } if SoundId::DEATH.contains(&id)),
        "{:?}",
        death[0]
    );
    let gib =
        |k: &EventKind| matches!(k, EventKind::SpawnCosmetic { kind: CosmeticKind::Gib { worm: 1 }, .. });
    assert!(death[1..8].iter().all(gib), "then its 7 gibs");
    let blood = |k: &EventKind| {
        matches!(
            k,
            EventKind::SpawnCosmetic { kind: CosmeticKind::Blood { .. } | CosmeticKind::BloodDropper, .. }
        )
    };
    assert_eq!(death[8..].iter().filter(|k| blood(k)).count(), 150, "then its 50 rounds of blood");
    assert_eq!(death.len(), 1 + 7 + 150);
    assert_eq!(e.cosmetic_dropped(), 300, "only the hit's blood");
    assert_eq!(e.gameplay_dropped(), 0);
    let hit_blood = e.iter().filter(|e| !e.death && blood(&e.kind)).count();
    assert_eq!(hit_blood, 0, "the hit's blood found the cosmetics full");
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
fn each_class_of_events_has_its_own_quota() {
    let mut e = TickEvents::new();
    let zero = olr_math::Vec2::ZERO;
    for i in 0..2 * TickEvents::CAPACITY as i32 {
        e.cosmetic(CosmeticKind::Smoke, zero, zero);
        e.push(EventKind::Spawned { worm: 0 });
        e.edit(TerrainOp::Carve, StampId::Hole(0), i, -i, (5, 7));
    }
    assert_eq!(e.len(), TickEvents::CAPACITY);
    let count = |f: fn(&EventKind) -> bool| e.iter().filter(|x| f(&x.kind)).count();
    assert_eq!(count(|x| matches!(x, EventKind::SpawnCosmetic { .. })), TickEvents::COSMETIC_LIMIT);
    assert_eq!(count(|x| matches!(x, EventKind::TerrainEdit { .. })), TickEvents::EDIT_LIMIT);
    assert_eq!(count(|x| matches!(x, EventKind::Spawned { .. })), TickEvents::GAMEPLAY_LIMIT);
    let n = 2 * TickEvents::CAPACITY;
    assert_eq!(e.cosmetic_dropped() as usize, n - TickEvents::COSMETIC_LIMIT);
    let gameplay = (n - TickEvents::EDIT_LIMIT) + (n - TickEvents::GAMEPLAY_LIMIT);
    assert_eq!(e.gameplay_dropped() as usize, gameplay);
    // The dropped edits' stamps: x from EDIT_LIMIT on, y down to -(n - 1); a
    // 5 × 7 stamp covers x - 2..x + 3 and y - 3..y + 4.
    let (first, last) = (TickEvents::EDIT_LIMIT as i32, n as i32 - 1);
    let want = LostArea { x0: first - 2, y0: -last - 3, x1: last + 3, y1: -first + 4 };
    assert_eq!(e.lost_area(), Some(want));
    let c = e.clone();
    assert!(c.list.capacity() >= TickEvents::CAPACITY);
    assert_eq!(c, e);
    e.start_tick(&mut TickEvents::pending());
    assert!(e.is_empty() && e.cosmetic_dropped() == 0 && e.gameplay_dropped() == 0);
    assert_eq!(e.lost_area(), None);
    e.edit(TerrainOp::Dirt, StampId::Hole(4), 1, 1, (15, 15));
    assert_eq!(e.len(), 1, "the quotas start again");
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
        |w| w.place_bonus(10, 10, crate::BonusItem::Health).unwrap(),
        |w| w.time.0 += 1,
        |w| w.worms[0].pos_recorder.push(olr_math::Vec2::new(1.0, 2.0)),
    ];
    for (i, change) in changes.iter().enumerate() {
        let mut v = w.clone();
        change(&mut v);
        assert_ne!(v.checksum(), c0, "change {i}");
    }
    let mut v = w.clone();
    v.draws.events.push(EventKind::Spawned { worm: 3 });
    assert_eq!(v.checksum(), c0, "events are output, not state");
    w.tick_new(&[Keys(Keys::FIRE)]);
    let c1 = w.checksum();
    assert_ne!(c1, c0);
    let mut t = w.clone();
    for _ in 0..30 {
        w.tick_new(&[Keys(Keys::FIRE)]);
        t.tick_new(&[Keys(Keys::FIRE)]);
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

/// Replays `events`' terrain edits on `t` with the world's stamps, as a
/// renderer does on its colour images (spec §5.4).
fn replay(t: &mut Terrain, holes: &[Stamp], events: &[Event]) -> usize {
    let mut n = 0;
    for e in events {
        if let EventKind::TerrainEdit { op, stamp: StampId::Hole(i), x, y } = e.kind {
            let hole = &holes[i as usize];
            match op {
                TerrainOp::Carve | TerrainOp::Beam => t.carve_hole(hole, x, y),
                TerrainOp::Dirt => t.place_dirt(hole, x, y),
                TerrainOp::GreenDirt => unreachable!("no green-dirt mask here"),
            };
            n += 1;
        }
    }
    n
}

#[test]
fn every_terrain_edit_survives_a_flood_of_other_events() {
    // A dropped TerrainEdit would desync a renderer's colour images for good:
    // flood a tick with more cosmetics and other gameplay events than fit,
    // then make edits (four spawn holes in dirt), and replay them.
    let terrain = Terrain::from_grid(&MaterialGrid::new(200, 200, Material::Dirt));
    let mut w = World::test_new(SimSettings::lx56(), gun_mod(), terrain, holes(), AbsTime(0), 1);
    let mut shadow = w.terrain().clone();
    for _ in 0..20_000 {
        w.draws.events.cosmetic(CosmeticKind::Smoke, olr_math::Vec2::ZERO, olr_math::Vec2::ZERO);
        w.draws.events.push(EventKind::Damage { worm: 0, amount: 0.0, by: -1, dealt: 0.0 });
    }
    for i in 0..4 {
        w.spawn_worm(30 + 40 * i, 100).unwrap();
    }
    let events = w.tick_new(&[]).as_slice().to_vec();
    assert_eq!(replay(&mut shadow, &holes(), &events), 4, "every spawn hole is reported");
    assert_eq!(shadow.material_hash(), w.terrain().material_hash());
}
