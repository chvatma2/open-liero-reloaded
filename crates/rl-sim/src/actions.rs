//! What projectile events do: `Proj_Action::applyTo` and
//! `Proj_DoActionInfo::execute` (`src/common/PhysicsLX56_Projectiles.cpp:670-945`,
//! `1115-1230`), with the v7 (LX56) defaults for the fields only v8 mods set.
//!
//! The map and health effects are in [`crate::effects`] and
//! [`crate::damage`].

use rl_data::mod_def::{ActionDef, ActionKind, AnimKind, ProjectileGfx, TimerDef};
use rl_math::{PI_F32, TimeDiff, Vec2, atan2f, normalize_vector};

use crate::damage::injure_worm;
use crate::effects::{carve, explosion, make_dirt, make_green_dirt};
use crate::mod_rt::SpawnInfo;
use crate::proj_sim::{COL_BOTTOM, COL_LEFT, COL_RIGHT, COL_TOP, Col, PJC_MAPBORDER, ProjCtx};
use crate::projectile::{Projectile, ProjectilePool, attr_set};
use crate::spawn::{SpawnCtx, SpawnParent, spawn_info_apply};
use crate::world::MAX_WORMS;

/// `Proj_EventOccurInfo` for LX56 events: no targets.
pub(crate) struct Event {
    pub timer_hit: bool,
    /// `colType`: `None` for the timer.
    pub col: Option<Col>,
    pub dt: TimeDiff,
}

impl Event {
    fn worm(&self) -> Option<i32> {
        match self.col {
            Some(Col::Worm(id)) => Some(id),
            _ => None,
        }
    }

    fn terrain(&self) -> Option<u8> {
        match self.col {
            Some(Col::Terrain(mask)) => Some(mask),
            _ => None,
        }
    }
}

/// `Proj_DoActionInfo` (`include/ProjAction.h:477-500`), without sounds.
#[derive(Debug, PartialEq)]
pub(crate) struct DoActionInfo {
    pub explode: bool,
    pub damage: i32,
    pub timer: bool,
    pub shake: i32,
    pub dirt: bool,
    pub green_dirt: bool,
    pub delete_after: bool,
    /// `trailprojspawn`.
    pub trail_spawn: bool,
    /// `spawnprojectiles`: spawn `GeneralSpawnInfo`.
    pub spawn_projectiles: bool,
}

impl Default for DoActionInfo {
    fn default() -> Self {
        Self {
            explode: false,
            damage: -1,
            timer: false,
            shake: 0,
            dirt: false,
            green_dirt: false,
            delete_after: false,
            trail_spawn: false,
            spawn_projectiles: false,
        }
    }
}

/// `Proj_Action::hasAction` (`include/ProjAction.h:249-253`).
pub(crate) fn has_action(a: &ActionDef) -> bool {
    a.kind != ActionKind::Nothing || a.spawns_projectiles
}

/// `Proj_LX56Timer::hasAction` (`ProjAction.h:461`).
pub(crate) fn timer_has_action(t: &TimerDef) -> bool {
    t.action.as_ref().is_some_and(has_action) && t.time > 0.0
}

/// `MatrixD2<float>(1.0f) * v` (`include/CVec.h:214-216`), LX56's `SpeedMult`.
fn speed_mult(v: Vec2) -> Vec2 {
    Vec2::new(1.0 * v.x + 0.0 * v.y, 0.0 * v.x + 1.0 * v.y)
}

/// `Proj_Action::applyTo` (`:740-945`).
pub(crate) fn apply_to(
    a: &ActionDef,
    ev: &Event,
    p: &mut Projectile,
    info: &mut DoActionInfo,
    ctx: &mut ProjCtx,
) {
    let mut push_worm = true;
    let mut spawn = a.spawns_projectiles;
    match a.kind {
        ActionKind::Explode => {
            info.explode = true;
            info.damage = a.damage;
            if ev.timer_hit {
                info.timer = true;
            }
            if a.shake > info.shake {
                info.shake = a.shake;
            }
        }
        ActionKind::Bounce => {
            if ev.timer_hit {
                spawn = false;
            } else {
                push_worm = false;
                bounce(p, a.bounce_coeff);
                if a.bounce_explode > 0 {
                    explosion(ctx, p.last_sim_time, p.pos, a.bounce_explode as f32, p.owner);
                }
            }
        }
        ActionKind::Carve => {
            if ev.timer_hit || ev.terrain().is_some() {
                carve(ctx, a.damage, p.pos, p.owner);
                info.delete_after = true;
            }
        }
        ActionKind::Dirt | ActionKind::GreenDirt => {
            if a.kind == ActionKind::Dirt {
                info.dirt = true;
            } else {
                info.green_dirt = true;
            }
            if ev.timer_hit && a.shake > info.shake {
                info.shake = a.shake;
            }
        }
        ActionKind::Disappear2 => info.delete_after = true,
        ActionKind::InjureWorm => {
            if let Some(id) = ev.worm() {
                injure_worm(&mut ctx.harm(), id, a.damage as f32, p.owner);
            }
        }
        // Injure falls through to Disappear, which falls through to Nothing.
        ActionKind::Injure if let Some(id) = ev.worm() => {
            info.delete_after = true;
            injure_worm(&mut ctx.harm(), id, a.damage as f32, p.owner);
        }
        ActionKind::Disappear if ev.worm().is_some() => {}
        ActionKind::Injure | ActionKind::Disappear | ActionKind::GoThrough | ActionKind::Nothing => {
            if ev.terrain().is_some_and(|m| m & PJC_MAPBORDER != 0) {
                info.delete_after = true;
            }
            push_worm = false;
            if ev.timer_hit {
                spawn = false;
            }
        }
        // ChangeRadius adds (0, 0) and clamps at 0: radii are never negative after Spawn.
        ActionKind::ChangeRadius => {
            p.radius = (p.radius.0.max(0), p.radius.1.max(0));
        }
        ActionKind::OverwriteOwnSpeed => attr_set(&mut p.vel, Vec2::ZERO),
        ActionKind::MultiplyOwnSpeed => {
            let v = speed_mult(p.vel);
            attr_set(&mut p.vel, v);
        }
        ActionKind::DiffOwnSpeed => {
            let v = p.vel + Vec2::ZERO;
            attr_set(&mut p.vel, v);
        }
        ActionKind::HeadingToNextWorm => {
            let target = nearest_worm(ctx, p.pos, |_| true);
            let v = p.vel + speed_mult(heading_vel_to(target, p));
            attr_set(&mut p.vel, v);
        }
        ActionKind::HeadingToOwner => {
            if (0..MAX_WORMS as i32).contains(&p.owner) {
                let target = ctx.worms.iter().find(|w| w.id == p.owner).map(|w| w.pos);
                if target.is_some() {
                    let v = p.vel + speed_mult(heading_vel_to(target, p));
                    attr_set(&mut p.vel, v);
                }
            }
        }
        // Deathmatch only: no team game, so an enemy is any other worm and there are no team mates.
        ActionKind::HeadingToNextOtherWorm | ActionKind::HeadingToNextEnemyWorm => {
            let owner = p.owner;
            let target = nearest_worm(ctx, p.pos, |id| id != owner);
            let v = p.vel + speed_mult(heading_vel_to(target, p));
            attr_set(&mut p.vel, v);
        }
        ActionKind::HeadingToNextTeamMate => {
            let v = p.vel + speed_mult(heading_vel_to(None, p));
            attr_set(&mut p.vel, v);
        }
        // LX56 events have no targets; sounds are not simulated.
        ActionKind::InjureProj
        | ActionKind::PlaySound
        | ActionKind::OverwriteTargetSpeed
        | ActionKind::MultiplyTargetSpeed
        | ActionKind::DiffTargetSpeed
        | ActionKind::HeadTargetToUs
        | ActionKind::Other(_) => {}
    }
    if push_worm && let Some(id) = ev.worm() {
        let mut d = p.vel;
        normalize_vector(&mut d);
        if let Some(w) = ctx.worms.iter_mut().find(|w| w.id == id) {
            let v = w.vel + (d * 100.0) * ev.dt.seconds();
            attr_set(&mut w.vel, v);
        }
    }
    if spawn {
        info.spawn_projectiles = true;
    }
}

/// `CProjectile::Bounce` (`src/common/CProjectile.cpp:376-429`), LX56's
/// "wrong" version: each side sets both factors, and left/right override
/// top/bottom.
pub(crate) fn bounce(p: &mut Projectile, coeff: f32) {
    let (mut x, mut y) = (1.0, 1.0);
    if p.collision_side & COL_TOP != 0 {
        (x, y) = (coeff, -coeff);
    }
    if p.collision_side & COL_BOTTOM != 0 {
        (x, y) = (coeff, -coeff);
    }
    if p.collision_side & COL_LEFT != 0 {
        (x, y) = (-coeff, coeff);
    }
    if p.collision_side & COL_RIGHT != 0 {
        (x, y) = (-coeff, coeff);
    }
    p.vel.x *= x;
    p.vel.y *= y;
}

/// `nearestWorm` and its variants (`:670-724`): the first alive worm, in id
/// order, with the strictly smallest squared distance to `pos`.
fn nearest_worm(ctx: &ProjCtx, pos: Vec2, ok: impl Fn(i32) -> bool) -> Option<Vec2> {
    let mut best: Option<Vec2> = None;
    for w in ctx.worms.iter().filter(|w| w.alive && ok(w.id)) {
        // `best.GetLength2() > w.GetLength2()`: a NaN distance never replaces.
        let replace = best.is_none_or(|b| (b - pos).length2() > (w.pos - pos).length2());
        if replace {
            best = Some(w.pos);
        }
    }
    best
}

/// `getHeadingVelTo(target, src)` (`:726-738`).
fn heading_vel_to(target: Option<Vec2>, src: &Projectile) -> Vec2 {
    let Some(target) = target else { return Vec2::ZERO };
    let mut ret = Vec2::new(src.vel.y, -src.vel.x);
    normalize_vector(&mut ret);
    let mut diff = target - src.pos;
    normalize_vector(&mut diff);
    // MatrixD2::Rotation(diff.x, -diff.y) * vel: v1 = (x, -y), v2 = (y, x).
    let (rx, ry) = (diff.x, -diff.y);
    let v = src.vel;
    let a = Vec2::new(rx * v.x + -ry * v.y, ry * v.x + rx * v.y);
    let mut angle = atan2f(a.y, a.x);
    if angle >= PI_F32 {
        angle -= PI_F32 * 2.0;
    }
    ret * (angle / PI_F32)
}

/// `Proj_DoActionInfo::execute` (`:1165-1230`): explosion, dirt, then the
/// children (trail first), then the junk-projectile rule, and only then
/// the projectile frees its slot, so children never take it.
pub(crate) fn execute(pool: &mut ProjectilePool, i: usize, info: &mut DoActionInfo, ctx: &mut ProjCtx) {
    let sim_mod = ctx.sim_mod;
    let Some(p) = pool.get(i) else { return };
    let pi = &sim_mod.projectiles()[p.proj];
    let def = &pi.def;
    let (time, pos, owner) = (p.last_sim_time, p.pos, p.owner);
    if info.explode {
        let damage = if !info.timer {
            info.damage
        } else if def.player_hit.kind == ActionKind::Explode {
            // projectile_doTimerExplode's quirk: PlyHit's damage wins.
            def.player_hit.damage
        } else {
            def.timer.action.as_ref().map_or(0, |a| a.damage)
        };
        if damage != -1 {
            explosion(ctx, time, pos, damage as f32, owner);
        }
        info.delete_after = true;
    }
    if info.dirt {
        make_dirt(ctx, pos, owner);
        info.delete_after = true;
    }
    if info.green_dirt {
        make_green_dirt(ctx, pos, owner);
        info.delete_after = true;
    }
    if info.trail_spawn
        && let Some(s) = &pi.trail_spawn
    {
        spawn_children(pool, i, s, ctx);
    }
    if info.spawn_projectiles
        && let Some(s) = &pi.event_spawn
    {
        spawn_children(pool, i, s, ctx);
    }
    let has_any_action = has_action(&def.hit) || has_action(&def.player_hit) || timer_has_action(&def.timer);
    let ends_by_animation = matches!(
        &def.gfx,
        ProjectileGfx::Image { animation: Some(a), .. } if a.kind == AnimKind::Once
    ) && pi.frames.is_some();
    if !has_any_action && !ends_by_animation {
        info.delete_after = true;
    }
    if info.delete_after {
        pool.set_unused(i);
    }
}

/// `Proj_SpawnInfo::apply(prj, prj->fLastSimulationTime)`.
fn spawn_children(pool: &mut ProjectilePool, i: usize, s: &SpawnInfo, ctx: &mut ProjCtx) {
    let Some(mut parent) = pool.get(i).cloned() else { return };
    let time = parent.last_sim_time;
    let mut sctx = SpawnCtx {
        sim_mod: ctx.sim_mod,
        pool,
        rand: &mut ctx.draws.rand,
        settings: ctx.settings,
        now: ctx.now,
    };
    spawn_info_apply(s, SpawnParent::Proj(&mut parent), time, &mut sctx);
    if let Some(p) = pool.get_mut(i) {
        p.random = parent.random;
    }
}
