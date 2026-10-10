//! Unit tests of where the renderer's event data comes from (spec §19 E6):
//! each source of `Explosion.shake` and of a `Dirt` probe, and the sub-step
//! of worm-loop events.
use super::*;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_data::mod_def::{ActionDef, ActionKind, WeaponDef, WeaponKind};
use rl_math::{AbsTime, GlibcRand, Mt19937, Vec2};
use rl_terrain::{PX_DIRT, Stamp, Terrain};

use crate::beam::{BeamCtx, process_shot_beam};
use crate::bonus::{Bonus, MAX_BONUSES};
use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
use crate::rng::Draws;
use crate::shots::Shot;
use crate::{Keys, Loadout, SimMod, SimSettings, Slot, World, Worm};

const T0: AbsTime = AbsTime(100_000);

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

/// A gun firing once a second.
fn gun(laser_sight: bool) -> WeaponDef {
    let mut w = projectile_weapon("gun", "pellet");
    w.rof = 1.0;
    w.laser_sight = laser_sight;
    w
}

/// [`gun`] with a gravity-free pellet that does `hit` on the map.
fn gun_mod(hit: ActionDef, laser_sight: bool) -> Arc<SimMod> {
    let mut pellet = pixel_projectile(None);
    pellet.gravity = Some(0);
    pellet.hit = hit;
    let def = mod_def(vec![gun(laser_sight)], vec![("pellet", pellet)]);
    Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap())
}

/// 200 × 200: open air left of `x = 120`, dirt from there on and below
/// `y = 150`; worm 0 at `(x, 140)` holds weapon 0.
fn world(m: Arc<SimMod>, settings: SimSettings, x: i32) -> World {
    let cells = (0..200 * 200)
        .map(|i| if i % 200 >= 120 || i / 200 >= 150 { Material::Dirt } else { Material::Background })
        .collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(200, 200, cells).unwrap());
    let mut w = World::test_new(settings, m, terrain, holes(), T0, 1);
    w.spawn_worm(x, 140);
    let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    w.set_loadout(0, &Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 });
    w
}

/// Every event of `ticks` frames with `keys(frame)` held, with the terrain
/// before its tick.
fn play(w: &mut World, ticks: usize, keys: impl Fn(usize) -> u8) -> Vec<(Event, Terrain)> {
    let mut out = TickEvents::new();
    let mut all = Vec::new();
    for f in 0..ticks {
        let before = w.terrain().clone();
        w.tick(&[Keys(keys(f))], &mut out);
        all.extend(out.iter().map(|e| (*e, before.clone())));
    }
    all
}

fn explosions(events: &[(Event, Terrain)]) -> Vec<(f32, i32)> {
    events
        .iter()
        .filter_map(|(e, _)| match e.kind {
            EventKind::Explosion { damage, shake, .. } => Some((damage, shake)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_bounce_explosion_does_not_shake() {
    let hit = ActionDef { bounce_explode: 3, shake: 7, ..ActionDef::olx_default(ActionKind::Bounce) };
    let mut w = world(gun_mod(hit, false), SimSettings::lx56(), 60);
    let ev = play(&mut w, 60, |f| if f < 2 { Keys::FIRE } else { 0 });
    let ex = explosions(&ev);
    assert!(!ex.is_empty(), "the pellet bounced off the wall");
    assert!(ex.iter().all(|&e| e == (3.0, 0)), "{ex:?}");
}

#[test]
fn a_bonus_blown_up_by_an_explosion_shakes_5() {
    let hit = ActionDef { damage: 5, shake: 7, ..ActionDef::olx_default(ActionKind::Explode) };
    let settings = SimSettings { bonuses: true, ..SimSettings::lx56() };
    let mut w = world(gun_mod(hit, false), settings, 60);
    w.place_bonus(114, 140, crate::BonusItem::Health).unwrap();
    let ev = play(&mut w, 60, |f| if (2..4).contains(&f) { Keys::FIRE } else { 0 });
    // The chained explosion is reported first: OLX recurses before its own spawn.
    assert_eq!(explosions(&ev), [(15.0, 5), (5.0, 7)]);
}

#[test]
fn a_bonus_hit_by_a_beam_shakes_5() {
    let laser = WeaponDef {
        id: "laser".into(),
        name: "Laser".into(),
        recharge: 0.18,
        drain: 1.25,
        rof: 0.009,
        laser_sight: false,
        kind: WeaponKind::Beam {
            recoil: 0,
            beam: rl_data::mod_def::BeamDef {
                colour: Rgba::rgb(255, 0, 0),
                damage: 0,
                player_damage: 1,
                length: 999_999,
                init_width: 1,
                width_increase: 0.0,
                distribute_damage_over_width: false,
            },
        },
    };
    let def =
        mod_def(vec![projectile_weapon("gun", "pellet"), laser], vec![("pellet", pixel_projectile(None))]);
    let m = SimMod::compile(&def, &BTreeMap::new()).unwrap();
    let s = SimSettings { bonuses: true, ..SimSettings::lx56() };
    let mut ws = vec![Worm::spawned(0, Vec2::new(20.0, 25.0), T0, &s)];
    ws[0].slots = vec![Slot { weapon: 1, charge: 1.0, reloading: false, last_fire: 0.0 }];
    let shot = Shot { worm: 0, weapon: 1, pos: Vec2::new(20.0, 25.0), spawn: T0, ..Shot::default() };
    let mut bonuses = [Bonus::default(); MAX_BONUSES];
    bonuses[3] = Bonus { used: true, pos: Vec2::new(40.0, 27.0), ..Bonus::default() };
    let cells =
        (0..100 * 50).map(|i| if i % 100 >= 60 { Material::Dirt } else { Material::Background }).collect();
    let mut t = Terrain::from_grid(&MaterialGrid::from_cells(100, 50, cells).unwrap());
    let (holes, mut d, mut parts) = (holes(), Draws::new(GlibcRand::new(1), Mt19937::new(1), 0), Vec::new());
    let mut ctx = BeamCtx { terrain: &mut t, holes: &holes, draws: &mut d, parts: &mut parts };
    process_shot_beam(
        &shot,
        &m.weapons()[1],
        &mut ws,
        &mut ctx,
        &s,
        T0,
        &mut bonuses,
        &mut crate::referee::Referee::oracle(),
    );
    let shakes: Vec<(f32, i32)> = d
        .events
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::Explosion { damage, shake, .. } => Some((damage, shake)),
            _ => None,
        })
        .collect();
    assert_eq!(shakes, [(15.0, 5), (0.0, 5)]);
}

/// The dirt events of `ev`: their index, position and probe.
fn dirt(ev: &[(Event, Terrain)]) -> Vec<(usize, Vec2, (i32, i32))> {
    ev.iter()
        .enumerate()
        .filter_map(|(i, (e, _))| match e.kind {
            EventKind::SpawnCosmetic { kind: CosmeticKind::Dirt { probe }, pos, .. } => Some((i, pos, probe)),
            _ => None,
        })
        .collect()
}

#[test]
fn digging_probes_the_first_dirt_pixel_on_its_row() {
    let mut w = world(gun_mod(ActionDef::olx_default(ActionKind::Explode), false), SimSettings::lx56(), 100);
    // Walk right to the wall, then add left: a dig into fresh dirt (a dig
    // right after another finds the dirt ahead already carved).
    let ev = play(&mut w, 260, |f| if f == 250 { Keys::RIGHT | Keys::LEFT } else { Keys::RIGHT });
    let d = dirt(&ev);
    assert_eq!(d.len(), 3, "three particles a dig");
    let (i, pos, probe) = d[0];
    assert!(d.iter().all(|&(_, p, q)| p == pos && q == probe));
    let t = &ev[i].1;
    let (px, y) = (pos.x as i32, (pos.y as i32).clamp(0, 199));
    assert_eq!(probe.1, y, "the probe is on the dig's row");
    assert!((px - 2..=px + 2).contains(&probe.0));
    assert!(t.flags(probe.0 as i64, y as i64) & PX_DIRT != 0, "the probe is dirt");
    for x in px - 2..probe.0 {
        assert!(t.flags(x as i64, y as i64) & PX_DIRT == 0, "{x} before the probe is not dirt");
    }
    // The particles come right before their hole.
    let after = &ev[d[2].0 + 1].0.kind;
    assert!(
        matches!(after, EventKind::TerrainEdit { op: TerrainOp::Carve, stamp: StampId::Hole(3), .. }),
        "{after:?}"
    );
}

#[test]
fn a_rope_hook_probes_where_it_lands() {
    let mut w = world(gun_mod(ActionDef::olx_default(ActionKind::Explode), false), SimSettings::lx56(), 100);
    let ev = play(&mut w, 50, |f| if f == 2 { Keys::ROPE } else { 0 });
    let d = dirt(&ev);
    assert_eq!(d.len(), 5, "five particles when the hook lands in dirt");
    let (i, pos, probe) = d[0];
    assert!(d.iter().all(|&(_, p, q)| p == pos && q == probe));
    // `hook + (0, 2)`, coloured like the hook's (wrapped) pixel.
    assert_eq!(probe, (pos.x as i32, (pos.y - 2.0) as i32));
    assert!(ev[i].1.flags(probe.0 as i64, probe.1 as i64) & PX_DIRT != 0);
}

#[test]
fn worm_loop_events_of_sub_step_k_carry_k() {
    let mut w = world(gun_mod(ActionDef::olx_default(ActionKind::Explode), true), SimSettings::lx56(), 60);
    let mut out = TickEvents::new();
    let mut with_substeps = 0;
    for _ in 0..100 {
        w.tick(&[Keys(0)], &mut out);
        // A laser sight is spawned in every sub-step's worm loop.
        let sights: Vec<u8> = out
            .iter()
            .filter(|e| {
                matches!(e.kind, EventKind::SpawnCosmetic { kind: CosmeticKind::LaserSight { .. }, .. })
            })
            .map(|e| e.substep)
            .collect();
        let n = w.substeps_last_tick();
        assert_eq!(sights, (0..n).collect::<Vec<u8>>());
        with_substeps += (n > 0) as u32;
    }
    assert!(with_substeps > 50);
}

#[test]
fn laser_sights_name_their_worm() {
    // Worm 1 stands nearer worm 0's sight than worm 0 itself does at times;
    // the event names the worm, so nobody guesses.
    let mut w = world(gun_mod(ActionDef::olx_default(ActionKind::Explode), true), SimSettings::lx56(), 60);
    w.spawn_worm(64, 140);
    let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    w.set_loadout(1, &Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 });
    let ev = play(&mut w, 30, |_| 0);
    let mut seen = [0; 2];
    for (e, _) in &ev {
        if let EventKind::SpawnCosmetic { kind: CosmeticKind::LaserSight { worm, .. }, pos, .. } = e.kind {
            let i = worm as usize;
            assert_eq!(pos.x, w.worms()[i].pos.x, "worm {worm}'s sight starts at it");
            seen[i] += 1;
        }
    }
    assert!(seen[0] > 10 && seen[0] == seen[1], "{seen:?}");
}
