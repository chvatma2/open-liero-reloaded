//! Unit tests of the state transfer's number checks: a hostile state's
//! non-finite floats are refused, the receiving world unchanged.
use super::*;
use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
use crate::{BonusItem, Keys, Loadout, SimMod, SimSettings, Slot};
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_math::Vec2;
use rl_terrain::{Stamp, Terrain};

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

/// Two armed worms over a dirt floor that have fired for a few ticks, with
/// a health bonus on both sides: every kind of float a state carries is
/// in use.
fn played() -> World {
    let def = mod_def(vec![projectile_weapon("gun", "pellet")], vec![("pellet", pixel_projectile(None))]);
    let sim_mod = Arc::new(SimMod::compile(&def, &BTreeMap::new()).unwrap());
    let cells =
        (0..200 * 200).map(|i| if i / 200 < 150 { Material::Background } else { Material::Dirt }).collect();
    let terrain = Terrain::from_grid(&MaterialGrid::from_cells(200, 200, cells).unwrap());
    let settings = SimSettings { bonuses: true, ..SimSettings::lx56() };
    let mut w = World::test_new(settings, sim_mod, terrain, holes(), AbsTime(100_000), 7);
    for x in [60, 140] {
        let i = w.spawn_worm(x, 100).unwrap();
        let slot = Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
        w.set_loadout(
            i,
            &Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 },
        );
    }
    w.place_bonus(100, 50, BonusItem::Health).unwrap();
    // On until the last tick delivered shots too.
    for t in 0..200 {
        w.tick_new(&[Keys(Keys::FIRE | Keys::ROPE), Keys(Keys::FIRE)]);
        if t >= 30 && !w.last_shots.is_empty() {
            break;
        }
    }
    assert!(w.projectiles.iter().next().is_some(), "a projectile flies");
    assert!(w.bonuses.client.iter().any(|b| b.used) && w.bonuses.server.iter().any(|b| b.used));
    assert!(!w.last_shots.is_empty(), "shots were delivered");
    w
}

type Mutation = (&'static str, StateError, fn(&mut World, f32));

fn proj(w: &mut World) -> &mut Projectile {
    let i = w.projectiles.iter().next().unwrap().0;
    w.projectiles.get_mut(i).unwrap()
}

fn client_bonus(w: &mut World) -> &mut crate::bonus::Bonus {
    w.bonuses.client.iter_mut().find(|b| b.used).unwrap()
}

fn server_bonus(w: &mut World) -> &mut crate::bonus::Bonus {
    w.bonuses.server.iter_mut().find(|b| b.used).unwrap()
}

const MUTATIONS: &[Mutation] = &[
    ("worm pos.x", StateError::Worms, |w, v| w.worms[0].pos.x = v),
    ("worm pos.y", StateError::Worms, |w, v| w.worms[1].pos.y = v),
    ("worm vel.x", StateError::Worms, |w, v| w.worms[0].vel.x = v),
    ("worm vel.y", StateError::Worms, |w, v| w.worms[0].vel.y = v),
    ("worm health", StateError::Worms, |w, v| w.worms[0].health = v),
    ("worm angle", StateError::Worms, |w, v| w.worms[0].angle = v),
    ("worm angle_speed", StateError::Worms, |w, v| w.worms[0].angle_speed = v),
    ("worm damage", StateError::Worms, |w, v| w.worms[0].damage = v),
    ("worm damage_factor", StateError::Worms, |w, v| w.worms[0].damage_factor = v),
    ("worm shield_factor", StateError::Worms, |w, v| w.worms[0].shield_factor = v),
    ("worm frame", StateError::Worms, |w, v| w.worms[0].frame = v),
    ("worm speed_factor", StateError::Worms, |w, v| w.worms[0].speed_factor = v),
    ("slot charge", StateError::Worms, |w, v| w.worms[0].slots[2].charge = v),
    ("slot last_fire", StateError::Worms, |w, v| w.worms[0].slots[0].last_fire = v),
    ("rope pos", StateError::Worms, |w, v| w.worms[0].rope.pos.x = v),
    ("rope vel", StateError::Worms, |w, v| w.worms[0].rope.vel.y = v),
    ("unthrown rope pos", StateError::Worms, |w, v| w.worms[1].rope.pos.y = v),
    ("position record", StateError::Worms, |w, v| w.worms[0].pos_recorder.push(Vec2::new(10.0, v))),
    ("projectile pos", StateError::Projectiles, |w, v| proj(w).pos.x = v),
    ("projectile vel", StateError::Projectiles, |w, v| proj(w).vel.y = v),
    ("projectile old_pos", StateError::Projectiles, |w, v| proj(w).old_pos.x = v),
    ("projectile life", StateError::Projectiles, |w, v| proj(w).life = v),
    ("projectile extra", StateError::Projectiles, |w, v| proj(w).extra = v),
    ("projectile speed", StateError::Projectiles, |w, v| proj(w).speed = v),
    ("projectile time_var_random", StateError::Projectiles, |w, v| proj(w).time_var_random = v),
    ("projectile rotation", StateError::Projectiles, |w, v| proj(w).rotation = v),
    ("projectile wallshoot_time", StateError::Projectiles, |w, v| proj(w).wallshoot_time = v),
    ("projectile frame", StateError::Projectiles, |w, v| proj(w).frame = v),
    ("projectile health", StateError::Projectiles, |w, v| proj(w).health = v),
    ("client bonus pos", StateError::Bonuses, |w, v| client_bonus(w).pos.x = v),
    ("client bonus vel", StateError::Bonuses, |w, v| client_bonus(w).vel.y = v),
    ("client bonus life", StateError::Bonuses, |w, v| client_bonus(w).life = v),
    ("client bonus flash_time", StateError::Bonuses, |w, v| client_bonus(w).flash_time = v),
    ("server bonus pos", StateError::Bonuses, |w, v| server_bonus(w).pos.y = v),
    ("server bonus vel", StateError::Bonuses, |w, v| server_bonus(w).vel.x = v),
    ("server bonus life", StateError::Bonuses, |w, v| server_bonus(w).life = v),
    ("delivered shot pos", StateError::Buffers, |w, v| w.last_shots[0].pos.x = v),
    ("delivered shot worm_vel", StateError::Buffers, |w, v| w.last_shots[0].worm_vel.y = v),
];

#[test]
fn the_played_state_loads() {
    let src = played();
    let mut dst = src.clone();
    dst.tick_new(&[Keys(0), Keys(Keys::JUMP)]);
    dst.load_state(src.state()).unwrap();
    assert_eq!(dst, src);
}

#[test]
fn non_finite_numbers_are_refused() {
    let base = played();
    let mut dst = base.clone();
    dst.tick_new(&[Keys(0), Keys(Keys::JUMP)]);
    let before = dst.clone();
    for &(what, error, mutate) in MUTATIONS {
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut src = base.clone();
            mutate(&mut src, v);
            assert_eq!(dst.load_state(src.state()), Err(error), "{what} = {v}");
            assert!(dst == before, "{what} = {v}: a refused state changes nothing");
        }
    }
}

#[test]
fn large_finite_numbers_load() {
    // No bound but the aim's and a thrown rope's speed (see `worm_finite`).
    let base = played();
    let mut src = base.clone();
    let w = &mut src.worms[0];
    (w.pos, w.vel, w.health) = (Vec2::new(-1e30, 1e30), Vec2::new(3e38, -3e38), 1e30);
    let mut dst = base.clone();
    dst.load_state(src.state()).unwrap();
    assert_eq!(dst, src);
}

#[test]
fn a_last_damager_past_the_worlds_worms_is_refused() {
    // Two worms: the killer an exact-zero death credits must be one of them.
    let base = played();
    let mut dst = base.clone();
    dst.tick_new(&[Keys(0), Keys(Keys::JUMP)]);
    let before = dst.clone();
    for (who, ok) in [(-1, true), (0, true), (1, true), (2, false), (20, false), (-2, false)] {
        let mut src = base.clone();
        src.worms[0].last_damager = who;
        let got = dst.clone().load_state(src.state());
        assert_eq!(got.is_ok(), ok, "last_damager {who}: {got:?}");
        if !ok {
            assert_eq!(dst.load_state(src.state()), Err(StateError::Worms), "last_damager {who}");
            assert!(dst == before, "last_damager {who}: a refused state changes nothing");
        }
    }
}
