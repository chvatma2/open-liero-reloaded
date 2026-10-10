//! Unit tests of the event data a renderer needs (spec §19 E6) and of the
//! caller-owned event buffer.
use super::*;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::{ActionDef, ActionKind, ProjectileGfx, WeaponKind};
use rl_math::AbsTime;
use rl_terrain::{PX_DIRT, Stamp, Terrain};

use crate::damage::injure_worm;
use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
use crate::{Keys, Loadout, SimMod, SimSettings, Slot, World};

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

/// Weapon 0 fires a pellet once a second that explodes on the map (damage
/// 5, shake 7); weapon 1 fires a rotating image projectile whose
/// `RotIncrement` is 0, with `UseRandomRot`.
fn shaky_mod() -> Arc<SimMod> {
    let mut gun = projectile_weapon("gun", "pellet");
    gun.rof = 1.0;
    let mut pellet = pixel_projectile(None);
    pellet.gravity = Some(0);
    pellet.hit = ActionDef { damage: 5, shake: 7, ..ActionDef::olx_default(ActionKind::Explode) };
    let spinner = projectile_weapon("spinner", "spin");
    assert!(matches!(&spinner.kind, WeaponKind::Projectile { spawn, .. } if spawn.use_random_rot));
    let mut spin = pixel_projectile(None);
    spin.gfx = ProjectileGfx::Image {
        file: "gfx/spin.png".into(),
        rotating: true,
        rot_increment: 0,
        rot_speed: 0,
        use_angle: false,
        use_spec_angle: false,
        angle_images: 0,
        animation: None,
    };
    let frames = [("gfx/spin.png".into(), 1)].into_iter().collect::<BTreeMap<_, _>>();
    let def = mod_def(vec![gun, spinner], vec![("pellet", pellet), ("spin", spin)]);
    Arc::new(SimMod::compile(&def, &frames).unwrap())
}

/// Open air left of `x = 120`, dirt from there on, and a dirt floor below
/// `y = 150`; worm 0 at `(60, 140)` holds `weapon` in every slot.
fn walled_world(weapon: i32) -> World {
    let cells = (0..200 * 200)
        .map(|i| if i % 200 >= 120 || i / 200 >= 150 { Material::Dirt } else { Material::Background })
        .collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(200, 200, cells).unwrap());
    let mut w = World::new(SimSettings::lx56(), shaky_mod(), terrain, holes(), AbsTime(100_000), 1);
    w.spawn_worm(60, 140);
    let slot = Slot { weapon, charge: 1.0, reloading: false, last_fire: 0.0 };
    w.set_loadout(0, &Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 });
    w
}

/// One tick into a fresh buffer.
fn tick(w: &mut World, keys: &[Keys]) -> TickEvents {
    let mut out = TickEvents::new();
    w.tick(keys, &mut out);
    out
}

#[test]
fn events_carry_e6_data() {
    let mut w = walled_world(0);
    let step = w.settings().lx56_physics_dt();
    let (mut explosions, mut probes, mut zero_substep_ticks, mut late) = (0, 0, 0, 0);
    let mut out = TickEvents::new();
    for f in 0..150 {
        let before = w.terrain().clone();
        let physics_before = w.physics_time();
        let keys = if f < 4 { Keys(Keys::FIRE) } else { Keys(0) };
        w.tick(&[keys], &mut out);
        let n = w.substeps_last_tick();
        zero_substep_ticks += (n == 0) as u32;
        // The physics clock moves one sub-step per sub-step run.
        assert_eq!(w.physics_time().0 - physics_before.0, n as u64 * step.0, "frame {f}");
        assert!(w.physics_time() <= w.time());
        for e in &out {
            // An event's sub-step is the number of OLX entity passes before it.
            assert!(e.substep <= n, "frame {f}: {e:?} after {n} sub-steps");
            late += (e.substep > 0) as u32;
            match e.kind {
                EventKind::Explosion { shake, damage, .. } => {
                    assert_eq!((shake, damage), (7, 5.0), "the action's Shake");
                    // A projectile explodes after the sub-step's entity pass.
                    assert!(e.substep >= 1, "{e:?}");
                    explosions += 1;
                }
                EventKind::SpawnCosmetic { kind: CosmeticKind::Dirt { probe: (x, y) }, pos, .. } => {
                    assert!(before.flags(x as i64, y as i64) & PX_DIRT != 0, "probe {x},{y} is dirt");
                    assert!((x - pos.x as i32).abs() <= 2 && y == pos.y as i32, "{e:?}");
                    probes += 1;
                }
                _ => {}
            }
        }
    }
    assert_eq!(explosions, 1);
    assert_eq!(probes, 2, "the explosion's two dirt particles");
    assert!(zero_substep_ticks > 0 && late > 0);

    // A kill: the damage dealt is clamped to the health before; gibs name the victim.
    w.spawn_worm(80, 140);
    tick(&mut w, &[]);
    w.set_health(1, 30.0);
    injure_worm(&mut w.harm(), 1, 150.0, 0);
    let ev: Vec<Event> = w.draws.events.iter().copied().collect();
    assert_eq!(ev[0].kind, EventKind::Damage { worm: 1, amount: 150.0, by: 0, dealt: 30.0 });
    let gibs = ev
        .iter()
        .filter(|e| matches!(e.kind, EventKind::SpawnCosmetic { kind: CosmeticKind::Gib { .. }, .. }));
    assert_eq!(gibs.clone().count(), 7);
    assert!(
        gibs.into_iter()
            .all(|e| matches!(e.kind, EventKind::SpawnCosmetic { kind: CosmeticKind::Gib { worm: 1 }, .. }))
    );

    // `RotIncrement` 0 becomes 1 at the first random-rotation spawn, for good.
    let mut w = walled_world(1);
    let proj = |weapon: usize| w.sim_mod().weapons()[weapon].spawn.as_ref().unwrap().proj;
    let (pellet, spin) = (proj(0), proj(1));
    assert!(!w.rot_bumped(spin));
    for _ in 0..10 {
        tick(&mut w, &[Keys(Keys::FIRE)]);
    }
    assert!(w.projectiles().any(|(_, p)| p.proj == spin), "the spinner fired");
    assert!(w.rot_bumped(spin) && !w.rot_bumped(pellet));
    assert!(!w.rot_bumped(usize::MAX), "an unknown type is never bumped");
}

#[test]
fn projectile_serials_differ_across_slot_reuse() {
    let mut w = walled_world(0);
    let mut seen: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for _ in 0..400 {
        tick(&mut w, &[Keys(Keys::FIRE)]);
        for (i, p) in w.projectiles() {
            let serials = seen.entry(i).or_default();
            if serials.last() != Some(&p.serial()) {
                serials.push(p.serial());
            }
        }
    }
    let slot0 = &seen[&0];
    assert!(slot0.len() >= 3, "slot 0 is reused by each shot: {slot0:?}");
    let mut all: Vec<u32> = seen.values().flatten().copied().collect();
    let n = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), n, "no two projectiles share a serial: {seen:?}");
}

#[test]
fn the_caller_owns_the_event_buffer() {
    let mut w = walled_world(0);
    let mut out = TickEvents::new();
    w.tick(&[Keys(Keys::FIRE)], &mut out);
    assert!(out.iter().any(|e| e.kind == EventKind::Spawned { worm: 0 }), "setup events come first");
    assert!(out.iter().all(|e| e.substep == 0 || e.kind != EventKind::Spawned { worm: 0 }));
    // A clone copies no events: it ticks into its own buffer, and the same keys give the same events.
    let mut c = w.clone();
    assert_eq!(c, w);
    let mut out2 = TickEvents::new();
    for _ in 0..120 {
        w.tick(&[Keys(Keys::FIRE)], &mut out);
        c.tick(&[Keys(Keys::FIRE)], &mut out2);
        assert_eq!(out, out2);
    }
    assert_eq!(c, w);
    // The work counter is not state.
    c.draws.spawn_work.0 += 1;
    assert_eq!(c, w);
}
