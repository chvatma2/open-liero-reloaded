//! LX56 worm simulation (`src/common/PhysicsLX56.cpp:88-469`) and the human
//! keyboard input it reads (`CWormHumanInputHandler::getInput`,
//! `src/common/CWormHuman.cpp:81-560`). Every expression keeps OLX's
//! operation order: results must match the oracle bit for bit.

use rl_math::{AbsTime, TimeDiff, Vec2, clamp_direct, reduce_const, reset_small, sign};
use rl_terrain::{PX_DIRT, PX_EMPTY, Stamp, Terrain};

use crate::SimSettings;
use crate::effects::carve_px;
use crate::events::{CosmeticKind, SoundId, TerrainOp, TickEvents};
use crate::input::Latches;
use crate::mod_rt::SimMod;
use crate::rng::Draws;
use crate::worm::{Dir, Worm};

/// Everything one worm's sub-step needs besides the worm itself.
pub(crate) struct Ctx<'a> {
    pub now: AbsTime,
    pub dt: f32,
    pub terrain: &'a mut Terrain,
    pub holes: &'a [Stamp],
    pub settings: &'a SimSettings,
    pub sim_mod: &'a SimMod,
    /// `(id, alive, pos)` of every worm, for the rope.
    pub worms: &'a [(i32, bool, Vec2)],
    pub draws: &'a mut Draws,
}

/// LX56 aim (`ForceLX56Aim`): max speed, acceleration.
const AIM_MAX_SPEED: f32 = 100.0;
const AIM_ACCEL: f32 = 500.0;

/// The keyboard part of `CWormHumanInputHandler::getInput` with OLX's default
/// client options (no mouse aim, no strafing, no old-school rope).
pub(crate) fn get_input(
    w: &mut Worm,
    keys: &mut Latches,
    now: AbsTime,
    s: &SimSettings,
    ev: &mut TickEvents,
) {
    let jump = keys.jump.is_once();
    let left_once = keys.left.is_once();
    let right_once = keys.right.is_once();

    if !w.alive {
        // `CWormHuman.cpp:115-119`: a dead worm's jump asks for the respawn
        // the server allows (`bCanRespawnNow`).
        if w.can_respawn_now && jump {
            w.respawn_requested = true;
        }
        keys.reset();
        return;
    }

    let dt = if now > w.last_input_time {
        let d = now - w.last_input_time;
        w.last_input_time = now;
        d
    } else {
        TimeDiff(0)
    };

    w.state.carve = false;
    w.state.moving = false;
    w.state.shoot = false;
    w.state.jump = false;

    // Aim, LX56 style: no friction, a constant slow-down when no key is held.
    if keys.up.is_down() {
        w.angle_speed -= AIM_ACCEL * dt.seconds();
    } else if keys.down.is_down() {
        w.angle_speed += AIM_ACCEL * dt.seconds();
    } else {
        clamp_direct(&mut w.angle_speed, -AIM_MAX_SPEED, AIM_MAX_SPEED);
        reduce_const(&mut w.angle_speed, 200.0 * dt.seconds());
        reset_small(&mut w.angle_speed, 5.0);
    }
    w.angle += w.angle_speed * dt.seconds();
    let max_angle = if s.full_aim_angle { 90.0 } else { 60.0 };
    if clamp_direct(&mut w.angle, -90.0, max_angle) != 0 {
        w.angle_speed = 0.0;
    }

    // Taken before this read changes the facing direction.
    let ninja_dir = w.face_direction();

    // `src/common/CWormHuman.cpp:401-421`: with `WeaponCombos` the weapon can change while firing.
    w.state.shoot = keys.fire.is_down();
    let slots = w.slots.len() as i32;
    if slots > 0 && (!w.state.shoot || s.weapon_combos) && keys.change.is_down() {
        let change = right_once as i32 - left_once as i32;
        w.current_weapon = (w.current_weapon + change).rem_euclid(slots);
    }

    if !keys.change.is_down() {
        if keys.left.is_down() {
            w.state.moving = true;
            if !keys.right.is_down() {
                w.face = Dir::Left;
                w.move_dir = Dir::Left;
            }
            if right_once {
                w.state.carve = true;
            }
        }
        if keys.right.is_down() {
            w.state.moving = true;
            if !keys.left.is_down() {
                w.face = Dir::Right;
                w.move_dir = Dir::Right;
            }
            if left_once {
                w.state.carve = true;
            }
        }
    }

    w.state.jump |= jump;
    if jump && w.rope.released {
        w.rope.clear();
    }
    if keys.rope.is_once() {
        let (pos, vel, can_use) = (w.pos, w.vel, w.can_use_ninja);
        w.rope.shoot(pos, vel, ninja_dir, can_use, s);
        // `PlaySoundSample(sfxGame.smpNinja)` (`CWormHuman.cpp:572-574`), even without the rope.
        ev.sound(SoundId::THROW, pos);
    }

    keys.reset();
}

/// `CWorm::CheckOnGround` (`src/common/CWorm.cpp:1363-1379`).
fn check_on_ground(w: &Worm, t: &Terrain) -> bool {
    let (px, py) = (w.pos.x as i32 as i64, w.pos.y as i32 as i64);
    (1..=6).rev().any(|y| t.flags(px - 2, py + y) & PX_EMPTY == 0 || t.flags(px + 2, py + y) & PX_EMPTY == 0)
}

/// The one-argument `CarveHole(CVec)` (`src/common/CMap.cpp:2172-2199`): a size-3 hole,
/// or nothing if the theme has no such hole (`CMap.cpp:1014-1016`).
/// Before carving, it looks for dirt in the five pixels around `pos.x` on
/// row `pos.y` (`CMap.cpp:2181-2192`) and, if it finds some, throws three
/// cosmetic particles, each drawing `GetRandomVec()` (6 `rand()` calls).
fn carve_small(ctx: &mut Ctx, pos: Vec2) -> i32 {
    let (width, height) = (ctx.terrain.width() as i64, ctx.terrain.height() as i64);
    let y = (pos.y as i32 as i64).min(height - 1).max(0);
    let px = pos.x as i32 as i64;
    for x in px - 2..=px + 2 {
        if x < 0 {
            continue;
        }
        if x >= width {
            break;
        }
        if ctx.terrain.flags(x, y) & PX_DIRT != 0 {
            // `GetRandomVec().multPairwise(30, 10)`.
            ctx.draws.particles(3, pos, (x as i32, y as i32), |v| Vec2::new(v.x * 30.0, v.y * 10.0));
            break;
        }
    }
    let (x, y) = (pos.x as i32, pos.y as i32);
    carve_px(ctx.terrain, ctx.holes, &mut ctx.draws.events, TerrainOp::Carve, 3, x, y) as i32
}

/// Bleeding (`PhysicsLX56.cpp:317-330`): below 15 health, at most once
/// every 2 s of frame time (strictly later than `fLastBlood + 2 s`), 20 rounds
/// of two blood entities, each drawing a `GetRandomVec()`.
fn bleed(w: &mut Worm, ctx: &mut Ctx) {
    // `AbsTime + 2.0f`: `time + (Uint64)(2.0f * 1000.0f)`.
    if w.health < 15.0 && ctx.now.0 > w.last_blood.0.wrapping_add(2000) {
        w.last_blood = ctx.now;
        // `((float)iBloodAmount / 100.0f) * 20` with the default 100.
        let d = &mut *ctx.draws;
        for _ in 0..20 {
            for red in [200, 180] {
                let v = d.rand.random_vec() * 100.0;
                d.entity(CosmeticKind::Blood { red }, w.pos, v);
            }
        }
    }
}

/// `PhysicsEngine::simulateWorm` for a local worm (`PhysicsLX56.cpp:272-469`).
/// Input is read first, as in OLX; the weapon slot is simulated last, then
/// the position is recorded.
pub(crate) fn simulate_worm(w: &mut Worm, keys: &mut Latches, ctx: &mut Ctx) {
    let s = ctx.settings;
    get_input(w, keys, ctx.now, s, &mut ctx.draws.events);
    if !w.alive {
        return;
    }
    let dt = ctx.dt;
    bleed(w, ctx);

    if w.state.carve {
        let dir = w.move_direction();
        let carved = carve_small(ctx, w.pos + dir * 4.0);
        w.dirt_count += carved;
    }

    if w.state.moving {
        w.frame += 7.5 * dt;
    }
    if w.frame >= 3.0 || !w.state.moving {
        w.frame = 0.0;
    }
    if w.frame < 0.0 {
        w.frame = 2.99;
    }

    if w.rope.released {
        let (id, pos) = (w.id, w.pos);
        w.rope.simulate(dt, id, pos, ctx.worms, ctx.terrain, s, ctx.draws);
        w.vel += w.rope.force(w.pos, s) * dt;
    }

    if w.state.moving {
        let (speed, max) = if w.on_ground {
            (s.worm_ground_speed, s.worm_max_ground_move_speed)
        } else {
            (s.worm_air_speed, s.worm_max_air_move_speed)
        };
        if w.move_dir == Dir::Right {
            if w.vel.x < max {
                w.vel.x += speed * dt * 90.0;
            }
        } else if w.vel.x > -max {
            w.vel.x -= speed * dt * 90.0;
        }
    }

    let mut jumped = false;
    let on_ground = check_on_ground(w, ctx.terrain);
    if on_ground {
        w.last_air_jump_time = AbsTime(0);
    }
    let relative_ok = s.relative_air_jump
        && ctx.now
            > AbsTime(w.last_air_jump_time.0.saturating_add((s.relative_air_jump_delay * 1000.0) as u64));
    if w.state.jump && (on_ground || w.can_air_jump || relative_ok) {
        // Ground jump, or OLX's "instant air jump" (absolute velocity); both set vy.
        if !on_ground {
            // Three ENT_SPARKLEs (`PhysicsLX56.cpp:388-390`); their arguments draw nothing.
            for x in [0.0, 2.0, -2.0] {
                let (pos, vel) = (w.pos + Vec2::new(x, 4.0), w.vel + Vec2::new(x * 10.0, 40.0));
                ctx.draws.entity(CosmeticKind::Sparkle, pos, vel);
            }
        }
        if on_ground || (!s.jump_to_aim_dir && w.can_air_jump && w.vel.y > s.worm_jump_force) {
            w.vel.y = s.worm_jump_force;
        } else {
            let dir = if s.jump_to_aim_dir { -w.face_direction() } else { Vec2::new(0.0, 1.0) };
            w.vel += dir * s.worm_jump_force;
        }
        w.last_air_jump_time = ctx.now;
        w.on_ground = false;
        jumped = true;
    }

    if !w.on_ground {
        let drag = s.worm_air_friction;
        w.vel.x -= w.vel.x * w.vel.x * sign(w.vel.x) as f32 * drag * dt;
        w.vel.y += -(w.vel.y * w.vel.y) * sign(w.vel.y) as f32 * drag * dt;
    }

    w.vel.y += s.worm_gravity * dt;

    let mut vel = w.vel;
    let pos = w.pos;
    move_and_check_collision(dt, w, pos, &mut vel, pos, jumped, ctx.terrain, &mut ctx.draws.events);
    w.vel = vel;

    if w.on_ground {
        w.vel.x *= 1.0 - s.worm_ground_friction;
        if libm::fabsf(w.vel.x) < s.worm_ground_stop_speed && !w.state.moving {
            w.vel.x = 0.0;
        }
    }

    crate::weapons::simulate_worm_weapon(w, dt, s, ctx.sim_mod);
    // `PhysicsLX56.cpp:468`: projectiles hit the recorded positions.
    w.pos_recorder.push(w.pos);
}

/// `moveAndCheckWormCollision` (`PhysicsLX56.cpp:88-269`) without wrap-around.
/// It writes `w.pos` and `w.on_ground` directly and updates `vel`. A fast
/// collision makes the bump sound (`:261-266`).
#[allow(clippy::too_many_arguments)]
fn move_and_check_collision(
    mut dt: f32,
    w: &mut Worm,
    mut pos: Vec2,
    vel: &mut Vec2,
    old_pos: Vec2,
    jump: bool,
    t: &Terrain,
    ev: &mut TickEvents,
) -> bool {
    const MAX_SPEED2: f32 = 10.0;
    let (map_w, map_h) = (t.width(), t.height());

    if (*vel * dt * w.speed_factor).length2() > map_w as f32 * map_h as f32 {
        return true;
    }
    if (*vel * dt * w.speed_factor).length2() > MAX_SPEED2 && dt > 0.001 {
        dt /= 2.0;
        if move_and_check_collision(dt, w, pos, vel, old_pos, jump, t, ev) {
            return true;
        }
        let pos2 = w.pos;
        return move_and_check_collision(dt, w, pos2, vel, old_pos, jump, t, ev);
    }

    pos += *vel * dt * w.speed_factor;
    w.pos = pos;

    let mut clip = 0u8; // 1 left, 2 right, 4 top, 8 bottom
    let mut coll = false;
    let y = pos.y as i32;
    if y >= 0 && (y as u32) < map_h {
        for x in -3..4 {
            if pos.x + x as f32 <= 2.0 {
                clip |= 0x01;
                w.pos.x = 5.0;
                coll = true;
                bounce(&mut vel.x);
                continue;
            }
            if pos.x + x as f32 >= map_w as f32 {
                w.pos.x = map_w as f32 - 5.0;
                coll = true;
                clip |= 0x02;
                bounce(&mut vel.x);
                continue;
            }
            let posx = pos.x as i32 + x;
            if t.flags(posx as i64, y as i64) & PX_EMPTY == 0 {
                coll = true;
                if x < 0 {
                    clip |= 0x01;
                    w.pos.x = pos.x + x as f32 + 4.0;
                } else {
                    clip |= 0x02;
                    w.pos.x = pos.x + x as f32 - 4.0;
                }
                bounce(&mut vel.x);
            }
        }
    }

    w.on_ground = false;
    let mut hit = false;
    let x = pos.x as i32;
    if x >= 0 && (x as u32) < map_w {
        let mut y = 5;
        while y > -5 {
            if pos.y + y as f32 <= 1.0 {
                w.pos.y = 6.0;
                coll = true;
                clip |= 0x04;
                bounce(&mut vel.y);
                break;
            }
            if pos.y + y as f32 >= map_h as f32 {
                w.pos.y = map_h as f32 - 5.0;
                clip |= 0x08;
                coll = true;
                w.on_ground = true;
                bounce(&mut vel.y);
                y -= 1;
                continue;
            }
            let posy = pos.y as i32 + y;
            if t.flags(x as i64, posy as i64) & PX_EMPTY == 0 {
                coll = true;
                if !hit && !jump {
                    if libm::fabsf(vel.y) > 40.0 && ((vel.y > 0.0 && y > 0) || (vel.y < 0.0 && y < 0)) {
                        vel.y *= -0.4;
                    } else {
                        vel.y = 0.0;
                    }
                }
                hit = true;
                w.on_ground = true;
                if y < 0 {
                    clip |= 0x04;
                    w.pos.y = pos.y + y as f32 + 5.0;
                } else {
                    clip |= 0x08;
                    w.pos.y = pos.y + y as f32 - 5.0;
                }
            }
            y -= 1;
        }
    }

    if clip & 0x01 != 0 && clip & 0x02 != 0 {
        w.pos.x = old_pos.x;
    }
    if clip & 0x04 != 0 && clip & 0x08 != 0 {
        w.pos.y = old_pos.y;
        if !w.state.jump {
            w.pos.x = old_pos.x;
        }
    }
    if coll
        && ((libm::fabsf(vel.x) > 30.0 && clip & 0x03 != 0)
            || (libm::fabsf(vel.y) > 30.0 && clip & 0x0c != 0))
    {
        ev.sound(SoundId::BUMP, w.pos);
    }
    coll
}

/// OLX's wall bounce: fast hits rebound at 40 %, slow ones stop.
fn bounce(v: &mut f32) {
    if libm::fabsf(*v) > 40.0 {
        *v *= -0.4;
    } else {
        *v = 0.0;
    }
}
