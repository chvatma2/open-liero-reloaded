//! Unit tests of spawning (`Proj_SpawnInfo::apply`).
use super::*;
use crate::mod_rt::MAX_AMOUNT;
use crate::mod_rt::tests::{mod_def, pixel_projectile, projectile_weapon};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use rl_data::mod_def::{ProjectileDef, ProjectileGfx};

#[test]
fn spawn_info_random_index_chain() {
    // weapons-firing.md §5.2: per projectile, spread reads T[n], then
    // n = (n+1)%255, speed variation reads T[n], n = (n*5)%255 is the
    // projectile's iRandom, and the next projectile starts at (n+1)%255.
    let mut spawn = crate::mod_rt::tests::weapon_spawn("pellet");
    spawn.amount = 3;
    spawn.speed_var = 10.0;
    let mut weapon = projectile_weapon("gun", "pellet");
    if let rl_data::mod_def::WeaponKind::Projectile { spawn: s, .. } = &mut weapon.kind {
        *s = spawn;
    }
    let def = mod_def(vec![weapon], vec![("pellet", pixel_projectile(None))]);
    let m = SimMod::compile(&def, &BTreeMap::new()).unwrap();
    let mut settings = SimSettings::lx56();
    settings.lx56_wall_shooting = false;
    let mut pool = ProjectilePool::new(10);
    let mut rand = GlibcRand::new(1);
    let mut shot = Shot { random: 250, angle: 0, pos: Vec2::new(100.0, 50.0), ..Shot::default() };
    let mut ctx = SpawnCtx {
        sim_mod: &m,
        pool: &mut pool,
        rand: &mut rand,
        settings: &settings,
        now: AbsTime(500),
        work: &mut 0,
        rot_bumped: &mut [],
    };
    let info = m.weapons()[0].spawn.clone().unwrap();
    spawn_info_apply(&info, SpawnParent::Shot(&mut shot), AbsTime(490), &mut ctx);
    // n: 250 -> 251 -> 251*5%255 = 235 (proj 0) -> 236 -> 237 -> 237*5%255 = 165 (proj 1)
    //   -> 166 -> 167 -> 167*5%255 = 70 (proj 2) -> 71.
    assert_eq!(shot.random, 71);
    let randoms: Vec<_> = pool.iter().map(|(_, p)| p.random).collect();
    assert_eq!(randoms, [235, 165, 70]);
    let p0 = pool.get(0).unwrap();
    let sprd = vec_from_angle(0.0 + 0.0 + get_fixed_random_num(250) * 3.0);
    let v = sprd * 250.0 + sprd * 10.0 * get_fixed_random_num(251) + Vec2::new(0.0, 0.0);
    assert_eq!((p0.vel.x.to_bits(), p0.vel.y.to_bits()), (v.x.to_bits(), v.y.to_bits()));
    assert_eq!(p0.pos, Vec2::new(108.0, 50.0));
    assert_eq!(
        (p0.spawn_time, p0.last_sim_time, p0.ignore_worm_coll_before),
        (AbsTime(490), AbsTime(490), AbsTime(600))
    );
    assert_eq!(p0.time_var_random, get_fixed_random_num(235));
    // Pixel projectiles draw their colour: one rand() each, nothing else here.
    assert_eq!(rand.calls(), 3);
}

/// A rotating image projectile without colours (so spawning it draws only its
/// random rotation) that `spawner` spawns `amount` of.
fn rot_mod(amount: i32, parent_vel_spread: bool) -> SimMod {
    let image = ProjectileDef {
        gfx: ProjectileGfx::Image {
            file: "gfx/rot.png".into(),
            rotating: true,
            rot_increment: 7,
            rot_speed: 0,
            use_angle: false,
            use_spec_angle: false,
            angle_images: 0,
            animation: None,
        },
        ..pixel_projectile(None)
    };
    let mut spawn = crate::mod_rt::tests::weapon_spawn("rot");
    spawn.amount = amount;
    spawn.speed_var = 10.0;
    spawn.use_parent_velocity_for_spread = parent_vel_spread;
    let mut weapon = projectile_weapon("gun", "rot");
    if let rl_data::mod_def::WeaponKind::Projectile { spawn: s, .. } = &mut weapon.kind {
        *s = spawn.clone();
    }
    let spawner = pixel_projectile(Some(spawn));
    let def = mod_def(vec![weapon], vec![("rot", image), ("spawner", spawner)]);
    let frames = BTreeMap::from([("gfx/rot.png".into(), 1)]);
    SimMod::compile(&def, &frames).unwrap()
}

/// What spawning left behind: the parent's random index, the `rand()` calls
/// and the generator's next value, the projectiles spawned, and the children
/// computed in full.
#[derive(Debug, PartialEq)]
struct After {
    random: i32,
    rand_calls: u64,
    next_rand: i32,
    spawned: usize,
    work: u64,
}

/// Applies `info` once from a fresh parent, into a pool with room for `room`
/// projectiles; `shot` picks a shot parent over a projectile parent.
fn apply(m: &SimMod, info: &SpawnInfo, room: usize, shot: bool) -> After {
    let mut settings = SimSettings::lx56();
    settings.lx56_wall_shooting = false;
    let mut pool = ProjectilePool::new(room);
    let mut rand = GlibcRand::new(1);
    let mut work = 0;
    let mut ctx = SpawnCtx {
        sim_mod: m,
        pool: &mut pool,
        rand: &mut rand,
        settings: &settings,
        now: AbsTime(500),
        work: &mut work,
        rot_bumped: &mut [],
    };
    let random = if shot {
        let mut s = Shot { random: 250, angle: 30, pos: Vec2::new(100.0, 50.0), ..Shot::default() };
        spawn_info_apply(info, SpawnParent::Shot(&mut s), AbsTime(490), &mut ctx);
        s.random
    } else {
        let mut p = Projectile { random: 200, vel: Vec2::new(30.0, -40.0), ..Projectile::default() };
        spawn_info_apply(info, SpawnParent::Proj(&mut p), AbsTime(490), &mut ctx);
        p.random
    };
    let spawned = pool.iter().count();
    let rand_calls = rand.calls();
    After { random, rand_calls, next_rand: rand.next(), spawned, work }
}

#[test]
fn a_full_pool_skips_the_childrens_work_but_not_their_draws() {
    // Review Focus 2: with the pool full, a spawn of `amount` children must
    // not compute them, yet advance what OLX advances per child before
    // `SpawnProjectile` finds no slot: the parent's fixed random index and
    // the rotation's `rand()` draw (`Proj_SpawnInfo::apply`,
    // `PhysicsLX56_Projectiles.cpp:577-638`). A pool with room for all of
    // them gives the reference, since spawning these children draws nothing.
    let amount = MAX_AMOUNT;
    for parent_vel_spread in [false, true] {
        let m = rot_mod(amount, parent_vel_spread);
        let weapon = m.weapons()[0].spawn.clone().unwrap();
        for (info, shot) in [(&weapon, true), (&weapon, false)] {
            let all = apply(&m, info, amount as usize, shot);
            assert_eq!(
                (all.spawned, all.work, all.rand_calls),
                (amount as usize, amount as u64, amount as u64)
            );
            for room in [0, 1, 37] {
                let some = apply(&m, info, room, shot);
                let want = After { spawned: room, work: room as u64, ..all };
                assert_eq!(
                    some, want,
                    "room {room}, shot {shot}, parent velocity spread {parent_vel_spread}"
                );
            }
        }
    }
}

#[test]
fn a_self_trailing_projectile_costs_at_most_one_full_child_per_free_slot() {
    // Review Focus 2, "never a hang": a projectile that trails `MAX_AMOUNT`
    // copies of itself every step (OLX's text mods can express the cycle)
    // fills the pool at once. From then on every step still spawns, but no
    // child is computed in full: the work per tick is bounded by the pool,
    // not by pool × amount.
    use rl_data::mod_def::{ActionDef, ActionKind, SpawnDef, TrailDef, TrailKind};
    let spawn = SpawnDef { amount: MAX_AMOUNT, ..SpawnDef::olx_default("self".into()) };
    let selfp = ProjectileDef {
        trail: TrailDef { kind: TrailKind::Projectile, delay: 0.0, spawn: Some(spawn) },
        hit: ActionDef::olx_default(ActionKind::Bounce),
        player_hit: ActionDef::olx_default(ActionKind::Nothing),
        ..pixel_projectile(None)
    };
    let mut weapon = projectile_weapon("gun", "self");
    if let rl_data::mod_def::WeaponKind::Projectile { spawn: s, .. } = &mut weapon.kind {
        s.amount = 1;
    }
    let m = SimMod::compile(&mod_def(vec![weapon], vec![("self", selfp)]), &BTreeMap::new()).unwrap();
    let grid = rl_data::level::MaterialGrid::new(200, 200, rl_data::level::Material::Background);
    let terrain = rl_terrain::Terrain::from_grid(&grid);
    let mut w =
        crate::World::new(SimSettings::lx56(), alloc::sync::Arc::new(m), terrain, vec![], AbsTime(0), 1);
    let i = w.spawn_worm(100, 100).unwrap();
    let slot = crate::Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
    let loadout = crate::Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 };
    w.set_loadout(i, &loadout);
    w.align_physics_clock();
    let mut per_tick = Vec::new();
    let mut events = crate::TickEvents::new();
    for _ in 0..20 {
        let before = w.draws.spawn_work.0;
        w.tick(&[crate::Keys(crate::Keys::FIRE)], &mut events);
        per_tick.push(w.draws.spawn_work.0 - before);
    }
    assert_eq!(w.projectiles().count(), ProjectilePool::DEFAULT_CAPACITY, "the pool is full");
    let total: u64 = per_tick.iter().sum();
    assert_eq!(total, ProjectilePool::DEFAULT_CAPACITY as u64, "one full child per slot: {per_tick:?}");
}
