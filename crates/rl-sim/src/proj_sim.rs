//! Projectile simulation: `LX56_simulateProjectiles` and the step it runs
//! for each projectile (`src/common/PhysicsLX56_Projectiles.cpp:52-491`,
//! `1234-1411`). What the events do is in [`crate::actions`].

use rl_data::level::Material;
use rl_data::mod_def::{ActionKind, AnimKind, ProjectileGfx, TrailKind};
use rl_math::{AbsTime, TimeDiff, Vec2, fmod, normalize_vector};
use rl_terrain::{GreenMask, Stamp, Terrain};

use crate::SimSettings;
use crate::actions::{DoActionInfo, Event, apply_to, execute, timer_has_action};
use crate::bonus::{Bonus, MAX_BONUSES};
use crate::damage::Harm;
use crate::events::CosmeticKind;
use crate::mod_rt::{SimMod, SimProjectile};
use crate::projectile::{Projectile, ProjectilePool, attr_set, c_int, c_u64};
use crate::referee::Referee;
use crate::rng::Draws;
use crate::world::MAX_WORMS;
use crate::worm::Worm;

/// `ProjCollisionType` (`include/CProjectile.h:53-63`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Col {
    /// `Terrain(colMask)`; `Terrain(0)` is `NoCol`.
    Terrain(u8),
    /// `Worm(wormId)`.
    Worm(i32),
}

pub(crate) const PJC_TERRAIN: u8 = 1;
pub(crate) const PJC_MAPBORDER: u8 = 4;
const PJC_DIRT: u8 = 8;
const NO_COL: Col = Col::Terrain(0);

impl Col {
    /// `operator bool`.
    fn any(self) -> bool {
        self != NO_COL
    }
}

/// `COL_TOP`, `COL_RIGHT`, `COL_BOTTOM`, `COL_LEFT` (`include/CProjectile.h`).
pub(crate) const COL_TOP: i32 = 1;
pub(crate) const COL_RIGHT: i32 = 2;
pub(crate) const COL_BOTTOM: i32 = 4;
pub(crate) const COL_LEFT: i32 = 8;

/// Everything a projectile step reads or changes besides the pool.
pub(crate) struct ProjCtx<'a> {
    pub sim_mod: &'a SimMod,
    pub settings: &'a SimSettings,
    pub terrain: &'a mut Terrain,
    pub worms: &'a mut [Worm],
    pub draws: &'a mut Draws,
    /// The theme's hole stamps, smallest first.
    pub holes: &'a [Stamp],
    /// `bmpGreenMask`, if the theme has it.
    pub green_mask: Option<&'a GreenMask>,
    /// `GetPhysicsTime()`: the frame clock.
    pub now: AbsTime,
    /// `tLX->currentTime`, pinned to `now + K` (spec §18 D2).
    pub wall_clock: AbsTime,
    /// `LX56PhysicsDT`.
    pub physics_dt: TimeDiff,
    /// The client's bonuses, which explosions destroy.
    pub bonuses: &'a mut [Bonus; MAX_BONUSES],
    /// The game mode, which credits kills.
    pub referee: &'a mut Referee,
}

impl ProjCtx<'_> {
    /// What `CClient::InjureWorm` needs.
    pub fn harm(&mut self) -> Harm<'_> {
        Harm {
            settings: self.settings,
            worms: self.worms,
            draws: self.draws,
            now: self.now,
            referee: self.referee,
        }
    }
}

/// `LX56_simulateProjectiles` (`:1400-1411`) and `LX56_simulateProjectile`
/// (`:1376-1397`): each projectile in slot order catches up its own clock to
/// the frame clock in `LX56PhysicsDT / GameSpeed` steps. The iteration reads
/// the pool's last used slot live, so children spawned into higher slots run
/// in the same pass.
pub(crate) fn simulate_projectiles(pool: &mut ProjectilePool, ctx: &mut ProjCtx) {
    let speed = ctx.settings.game_speed.clamp(0.05, 10.0);
    let mut frame_dt = ctx.physics_dt.scaled(1.0 / speed);
    if frame_dt.0 == 0 {
        frame_dt = TimeDiff(1);
    }
    let mut next = pool.next_used(None);
    while let Some(i) = next {
        while let Some(p) = pool.get_mut(i) {
            if p.last_sim_time + frame_dt > ctx.now {
                break;
            }
            p.last_sim_time += frame_dt;
            if !do_frame(pool, i, ctx) {
                break;
            }
        }
        next = pool.next_used(Some(i));
    }
}

/// `LX56ProjectileHandler_doFrame` (`:1330-1375`). Returns `false` once the
/// projectile is deleted.
fn do_frame(pool: &mut ProjectilePool, i: usize, ctx: &mut ProjCtx) -> bool {
    let sim_mod = ctx.sim_mod;
    let Some(p) = pool.get_mut(i) else { return false };
    let pi = &sim_mod.projectiles()[p.proj];
    let def = &pi.def;
    let dt = ctx.physics_dt;
    let mut info = DoActionInfo::default();
    // The timer fires every step once `Time + TimeVar * f < life` (life before this step's increment).
    if let Some(timer) = &def.timer.action
        && timer_has_action(&def.timer)
        && def.timer.time + def.timer.time_var * p.time_var_random < p.life
    {
        apply_to(timer, pi.timer_sound, &Event { timer_hit: true, col: None, dt }, p, &mut info, ctx);
    }
    let res = low_level(p, pi, dt, &mut info, ctx);
    let ev = Event { timer_hit: false, col: Some(res), dt };
    match res {
        Col::Terrain(mask) if mask & PJC_TERRAIN != 0 => {
            apply_to(&def.hit, pi.hit_sound, &ev, p, &mut info, ctx)
        }
        Col::Worm(_) if !info.explode => {
            apply_to(&def.player_hit, pi.player_hit_sound, &ev, p, &mut info, ctx)
        }
        _ => {}
    }
    execute(pool, i, &mut info, ctx);
    !info.delete_after
}

/// `LX56_simulateProjectile_LowLevel` (`:1234-1327`): move, then age,
/// rotate, animate (with the image's frame count, spec §18 D3) and trail.
fn low_level(
    p: &mut Projectile,
    pi: &SimProjectile,
    dt: TimeDiff,
    info: &mut DoActionInfo,
    ctx: &mut ProjCtx,
) -> Col {
    let res = check_coll_and_move(p, pi, dt, ctx);
    let dts = dt.seconds();
    p.life += dts;
    p.extra += dts;
    if let ProjectileGfx::Image { rotating, rot_speed, animation, .. } = &pi.def.gfx {
        if *rotating {
            p.rotation += *rot_speed as f32 * dts;
            fmod(&mut p.rotation, 360.0);
        }
        if let Some(anim) = animation {
            if p.frame_delta {
                p.frame += anim.rate * dts;
            } else {
                p.frame -= anim.rate * dts;
            }
            if let Some(n) = pi.frames {
                let n = n as i32 as f32;
                if p.frame >= n {
                    match anim.kind {
                        AnimKind::Once => info.delete_after = true,
                        AnimKind::Loop => p.frame = 0.0,
                        AnimKind::PingPong => {
                            p.frame_delta = !p.frame_delta;
                            p.frame = n - 1.0;
                        }
                        AnimKind::Other(_) => {}
                    }
                } else if p.frame < 0.0 && anim.kind == AnimKind::PingPong {
                    p.frame_delta = !p.frame_delta;
                    p.frame = 0.0;
                }
            }
        }
    }
    // Smoke and explosive trails are entities: only their pool slots matter
    // to the simulation (spec §18 D4).
    let trail = &pi.def.trail;
    match trail.kind {
        TrailKind::Smoke | TrailKind::ChemSmoke if p.extra >= 0.075 => {
            p.extra = 0.0;
            let kind =
                if trail.kind == TrailKind::Smoke { CosmeticKind::Smoke } else { CosmeticKind::ChemSmoke };
            ctx.draws.entity(kind, p.pos, Vec2::ZERO);
        }
        TrailKind::Doomsday | TrailKind::Explosive if p.extra >= 0.05 => {
            p.extra = 0.0;
            if trail.kind == TrailKind::Doomsday {
                ctx.draws.entity(CosmeticKind::Doomsday, p.pos, p.vel);
            } else {
                ctx.draws.entity(CosmeticKind::Explosion { size: 10 }, p.pos, Vec2::ZERO);
            }
        }
        TrailKind::Projectile if p.last_sim_time > p.last_trail_proj => {
            // AbsTime + float: `time + (Uint64)(td * 1000.0f)`.
            let delay = trail.delay / ctx.settings.game_speed;
            p.last_trail_proj = AbsTime(p.last_sim_time.0.wrapping_add(c_u64(delay * 1000.0)));
            info.trail_spawn = true;
        }
        _ => {}
    }
    res
}

/// `LX56Projectile_checkCollAndMove` (`:433-491`): dampening, then one move
/// or several shorter ones so that no move is longer than `AVG_CHECKSTEP`.
fn check_coll_and_move(p: &mut Projectile, pi: &SimProjectile, dt: TimeDiff, ctx: &mut ProjCtx) -> Col {
    let def = &pi.def;
    if p.changes_speed {
        let len = c_int(p.vel.length2());
        // abs() of INT_MIN stays INT_MIN on x86, as wrapping_abs.
        if len.wrapping_sub(p.check_speed_len).wrapping_abs() > 50000 {
            p.calculate_check_steps(def.hit.kind == ActionKind::Bounce);
        }
    }
    // dt is always LX56PhysicsDT here, so OLX takes the plain multiply.
    if def.dampening != 1.0 {
        let v = p.vel * def.dampening;
        attr_set(&mut p.vel, v);
    }
    let dts = dt.seconds();
    let v2 = p.vel.length2();
    if c_int(v2 * dts * dts) <= p.max_checkstep2 {
        return move_frame(p, pi, dt, ctx);
    }
    let mut checkstep = p.avg_checkstep as f32 / libm::sqrtf(v2);
    // MAX(a, b) is `a > b ? a : b`.
    checkstep = if checkstep > 0.001 { checkstep } else { 0.001 };
    let cstep = TimeDiff(c_u64(checkstep * 1000.0));
    if cstep < dt {
        let mut time = TimeDiff(0);
        while time < dt {
            let step = if time.0 + cstep.0 > dt.0 { TimeDiff(dt.0 - time.0) } else { cstep };
            let ret = move_frame(p, pi, step, ctx);
            if ret.any() {
                return ret;
            }
            time = TimeDiff(time.0 + cstep.0);
        }
        return NO_COL;
    }
    move_frame(p, pi, dt, ctx)
}

/// `CProjectile::ColInfo`.
struct ColInfo {
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
    collided: bool,
    only_dirt: bool,
}

/// `LX56Projectile_checkCollAndMove_Frame` (`:367-430`). `vOldPos` is the
/// last terrain-safe position: the wallshoot window keeps it at the spawn
/// position.
fn move_frame(p: &mut Projectile, pi: &SimProjectile, dt: TimeDiff, ctx: &mut ProjCtx) -> Col {
    let def = &pi.def;
    let dts = dt.seconds();
    let gravity = def.gravity.map_or(100.0, |g| g as f32);
    p.vel.y += ctx.settings.proj_gravity_factor * gravity * dts;
    // ProjFriction > 0 is rejected by SimSettings.
    let old_vel = p.vel;
    let frame_old_pos = p.pos;
    let v = p.pos + p.vel * dts;
    attr_set(&mut p.pos, v);
    if c_int((p.old_pos - p.pos).length2()) < p.min_checkstep2 {
        return final_worm_check(p, pi, frame_old_pos, old_vel, NO_COL, ctx);
    }
    let (px, py) = (c_int(p.pos.x), c_int(p.pos.y));
    if map_bounds_collision(p, px, py, ctx.terrain) {
        attr_set(&mut p.pos, p.old_pos);
        attr_set(&mut p.vel, old_vel);
        return final_worm_check(
            p,
            pi,
            frame_old_pos,
            old_vel,
            Col::Terrain(PJC_TERRAIN | PJC_MAPBORDER),
            ctx,
        );
    }
    // Wall shooting: LX56 ignores the terrain for the first 10-12 ms.
    let window = TimeDiff(c_u64(p.wallshoot_time * 1000.0));
    if p.last_sim_time <= AbsTime(p.spawn_time.0.wrapping_add(window.0)) {
        return final_worm_check(p, pi, frame_old_pos, old_vel, NO_COL, ctx);
    }
    let c = terrain_collision(p, px, py, ctx.terrain);
    if c.collided && handle_collision(p, pi, &c, frame_old_pos, old_vel, dt) {
        let mask = PJC_TERRAIN | if c.only_dirt { PJC_DIRT } else { 0 };
        return final_worm_check(p, pi, frame_old_pos, old_vel, Col::Terrain(mask), ctx);
    }
    p.old_pos = p.pos;
    final_worm_check(p, pi, frame_old_pos, old_vel, NO_COL, ctx)
}

/// `CProjectile::MapBoundsCollision` (`:176-196`), without wrap-around.
fn map_bounds_collision(p: &mut Projectile, px: i32, py: i32, t: &Terrain) -> bool {
    let (w, h) = (t.width() as i32, t.height() as i32);
    let (rx, ry) = p.radius;
    p.collision_side = 0;
    if px < 0 || px.wrapping_sub(rx) < 0 {
        p.collision_side |= COL_LEFT;
    }
    if px >= w || px.wrapping_add(rx) >= w {
        p.collision_side |= COL_RIGHT;
    }
    if py < 0 || py.wrapping_sub(ry) < 0 {
        p.collision_side |= COL_TOP;
    }
    if py >= h || py.wrapping_add(ry) >= h {
        p.collision_side |= COL_BOTTOM;
    }
    p.collision_side != 0
}

/// `CProjectile::TerrainCollision` (`:220-280`) for rectangles without
/// wrap-around: every solid pixel in the projectile's box.
fn terrain_collision(p: &Projectile, px: i32, py: i32, t: &Terrain) -> ColInfo {
    let mut res = ColInfo { left: 0, right: 0, top: 0, bottom: 0, collided: false, only_dirt: true };
    let (rx, ry) = p.radius;
    for y in py - ry..=py + ry {
        for x in px - rx..=px + rx {
            // Bounds were checked by map_bounds_collision; outside counts as rock.
            let m = t.material(x as i64, y as i64).unwrap_or(Material::Rock);
            if m != Material::Background {
                if y < py {
                    res.top += 1;
                } else if y > py {
                    res.bottom += 1;
                }
                if x < px {
                    res.left += 1;
                } else if x > px {
                    res.right += 1;
                }
                if m != Material::Dirt {
                    res.only_dirt = false;
                }
                res.collided = true;
            }
        }
    }
    res
}

/// `CProjectile::HandleCollision` (`:285-357`): returns `false` if the
/// collision is ignored (`GoThrough`).
fn handle_collision(
    p: &mut Projectile,
    pi: &SimProjectile,
    c: &ColInfo,
    old_pos: Vec2,
    old_vel: Vec2,
    dt: TimeDiff,
) -> bool {
    let kind = pi.def.hit.kind;
    if kind == ActionKind::Explode && c.only_dirt {
        // Keep the position: the explosion happens inside the dirt.
        return true;
    }
    let dts = dt.seconds();
    let bounce = match kind {
        ActionKind::Bounce => true,
        ActionKind::Nothing => {
            attr_set(&mut p.pos, old_pos + (p.vel * dts) * 0.5);
            p.old_pos = p.pos;
            false
        }
        ActionKind::GoThrough => {
            // GoThroughSpeed is 1 for LX56 mods.
            attr_set(&mut p.pos, old_pos + (p.vel * dts) * 1.0);
            return false;
        }
        _ => {
            attr_set(&mut p.pos, p.old_pos);
            attr_set(&mut p.vel, old_vel);
            return true;
        }
    };
    let (vx, vy) = (c_int(p.vel.x), c_int(p.vel.y));
    if (c.left > c.right || c.left > 2) && c.left > 1 && vx <= 0 {
        if bounce {
            p.pos.x = old_pos.x;
        }
        if vx != 0 {
            p.collision_side |= COL_LEFT;
        }
    }
    if (c.right > c.left || c.right > 2) && c.right > 1 && vx >= 0 {
        if bounce {
            p.pos.x = old_pos.x;
        }
        if vx != 0 {
            p.collision_side |= COL_RIGHT;
        }
    }
    if c.top > 1 && vy <= 0 {
        if bounce {
            p.pos.y = old_pos.y;
        }
        if vy != 0 {
            p.collision_side |= COL_TOP;
        }
    }
    if c.bottom > 1 && vy >= 0 {
        if bounce {
            p.pos.y = old_pos.y;
        }
        if vy != 0 {
            p.collision_side |= COL_BOTTOM;
        }
    }
    // abs(INT_MIN) is INT_MIN on x86, which is < 2.
    if vx.wrapping_abs() < 2 {
        p.vel.x = 0.0;
    }
    if vy.wrapping_abs() < 2 {
        p.vel.y = 0.0;
    }
    true
}

/// `FinalWormCollisionCheck` (`:130-171`): sweeps the move in 2 px steps
/// from `frame_old_pos` for a worm. A worm overrides the terrain result.
fn final_worm_check(
    p: &mut Projectile,
    pi: &SimProjectile,
    frame_old_pos: Vec2,
    frame_old_vel: Vec2,
    cur: Col,
    ctx: &ProjCtx,
) -> Col {
    let kind = pi.def.player_hit.kind;
    if kind == ActionKind::Nothing {
        return cur;
    }
    let mut dif = p.pos - frame_old_pos;
    let mut len = normalize_vector(&mut dif);
    // MIN(a, b) is `a < b ? a : b`.
    let max = ctx.terrain.width() as f32 + ctx.terrain.height() as f32;
    len = if len < max { len } else { max };
    let mut q = 0.0f32;
    while q <= len {
        let curpos = frame_old_pos + dif * q;
        let id = proj_worm_coll(p, curpos, ctx);
        if id >= 0 {
            if kind != ActionKind::GoThrough {
                attr_set(&mut p.pos, curpos);
                p.old_pos = curpos;
                attr_set(&mut p.vel, frame_old_vel);
            }
            return Col::Worm(id);
        }
        q += 2.0;
    }
    cur
}

/// `CProjectile::ProjWormColl` (`:52-111`): the first alive worm (in id
/// order) whose recorded position (`getBest`, with the `get(0)` quirk) is
/// within 4 px. LX56 projectiles have no radius here.
fn proj_worm_coll(p: &mut Projectile, pos: Vec2, ctx: &ProjCtx) -> i32 {
    let (sx, sy) = (c_int(pos.x), c_int(pos.y));
    let (mut rx, mut ry) = p.radius;
    if rx <= 2 {
        rx = 0;
    }
    if ry <= 2 {
        ry = 0;
    }
    let has_owner = (0..MAX_WORMS as i32).contains(&p.owner);
    let prevent_self_shooting = p.ignore_worm_coll_before > p.last_sim_time;
    // `AbsTime - AbsTime` wraps in OLX (its assert is compiled out).
    let ms_back = ctx.wall_clock.0.wrapping_sub(p.last_sim_time.0);
    for w in ctx.worms.iter() {
        if !w.alive {
            continue;
        }
        let is_owner = has_owner && w.id == p.owner;
        if (prevent_self_shooting || !ctx.settings.self_hit) && is_owner {
            continue;
        }
        let wp = w.pos_recorder.get_best(ctx.physics_dt.0, ms_back);
        let (wx, wy) = (c_int(wp.x), c_int(wp.y));
        const WORM_SIZE: i32 = 4;
        if sx.wrapping_sub(wx).wrapping_abs() < rx.wrapping_add(WORM_SIZE)
            && sy.wrapping_sub(wy).wrapping_abs() < ry.wrapping_add(WORM_SIZE)
        {
            p.collision_side = 0;
            if sx < wx.wrapping_sub(2) {
                p.collision_side |= COL_LEFT;
            } else if sx > wx.wrapping_add(2) {
                p.collision_side |= COL_RIGHT;
            }
            if sy < wy.wrapping_sub(2) {
                p.collision_side |= COL_TOP;
            } else if sy > wy.wrapping_add(2) {
                p.collision_side |= COL_BOTTOM;
            }
            return w.id;
        }
    }
    -1
}

#[cfg(test)]
#[path = "proj_sim_tests.rs"]
pub(crate) mod tests;
