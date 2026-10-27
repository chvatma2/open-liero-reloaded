//! The fire decision (notes §1.6): `AI_Shoot` with its line of sight,
//! clearing weapons, weapon choice, aim and trajectory check. With the
//! ballistic aim (spec §20 F2) the bot aims at the solver's angle and
//! checks the real arc through the terrain; with OLX's direct aim it aims
//! at the target and checks OLX's parabola.

use crate::ground::Ground;
use olr_math::Vec2;
use olr_sim::{WeaponType, Worm};
use olr_terrain::{PX_DIRT, PX_EMPTY, PX_ROCK};

use crate::AimModel;
use crate::aim::{Launch, aim_direction, ballistic_from, olx_aiming_angle};
use crate::bot::{Act, Bot, Ctx};
use crate::budget::{Afford, Budget};
use crate::loadout::GameType;
use crate::probe::{empty_for_worm, rock_between, trace_weapon_line};
use crate::weapons::Facts;

/// The most steps a trajectory check takes (2 px each).
const ARC_STEPS: u64 = 400;

/// `canShootRightNowWithCurWeapon` (`src/common/CWormBot.cpp:1963-1996`)
/// for slot `slot`: loaded, past its rate of fire, a projectile or beam
/// weapon (or a jetpack).
pub(crate) fn can_shoot_now(me: &Worm, slot: usize, facts: Option<&Facts>) -> bool {
    let Some(s) = me.slots.get(slot) else { return false };
    let Some(f) = facts else { return false };
    if s.reloading || s.last_fire > 0.0 {
        return false;
    }
    match f.kind {
        WeaponType::Special => f.jetpack,
        WeaponType::Projectile | WeaponType::Beam => true,
    }
}

impl Bot {
    /// `traceWeaponLine`'s steps for the weapon held (`CWormBot.cpp:2716-2752`),
    /// the worm's 5 px added to the first.
    pub(crate) fn weapon_line_steps(&self, c: &Ctx) -> (i32, i32) {
        let beam =
            c.me.slots
                .get(self.weapon)
                .and_then(|s| self.arsenal.get(s.weapon))
                .is_some_and(|f| f.kind == WeaponType::Beam);
        let (first, divs) = if beam {
            (1, 1)
        } else {
            match self.game_type {
                GameType::Rifles => (10, 6),
                GameType::Mortars => (7, 2),
                GameType::Lt100 => (6, 3),
                GameType::Other => (7, 5),
            }
        };
        (first + 5, divs)
    }

    /// `AI_Shoot` (`CWormBot.cpp:2001-2326`): whether the bot is busy
    /// aiming or firing (then it stands still). A scripted goal is a race:
    /// no shooting.
    pub(crate) fn shoot(&mut self, c: &Ctx, b: &mut Budget, act: &mut Act) -> bool {
        let me = c.me;
        let held = me.slots.get(self.weapon).and_then(|s| self.arsenal.get(s.weapon));
        if self.goal.is_some() || !can_shoot_now(me, self.weapon, held) {
            return false;
        }
        let Some(w) = self.find_target(c) else { return false };
        let trg = w.pos;
        let diff = trg - me.pos;
        let d = diff.length();
        let (first, divs) = self.weapon_line_steps(c);
        // Not affordable this tick: no shot.
        let Some((frac, ty)) = trace_weapon_line(c.t, b, me.pos, trg, first, divs) else { return false };
        let mut direct = ty & PX_ROCK == 0;
        if ty & PX_DIRT != 0 {
            direct = false;
            // Dirt above: clear it (OLX compares px with the fraction).
            if diff.y < 0.0
                && libm::fabsf(diff.y) > libm::fabsf(diff.x)
                && d - frac > 40.0
                && let Some(cw) = self.clearing_weapon(c)
                && rock_between(c.t, b, me.pos, trg).is_some_and(|r| r <= 3)
            {
                self.weapon = cw;
                if self.set_aim(c, act, trg) {
                    act.shoot = true;
                    return true;
                }
                return false;
            }
        }
        if self.game_type == GameType::Mortars && ty & PX_EMPTY == 0 && frac <= 25.0 {
            return false;
        }
        let Some(wpn) = self.best_weapon(c, b, d, direct, frac, act) else { return false };
        self.weapon = wpn;
        let Some(f) = me.slots.get(wpn).and_then(|s| self.arsenal.get(s.weapon)).cloned() else {
            return false;
        };
        let (aim, shoot) = if f.projectile() {
            self.aim_projectile(c, b, act, w, &f, direct)
        } else if f.kind == WeaponType::Beam {
            let dist = d as i32;
            if direct && f.beam_length >= dist {
                let good = self.set_aim(c, act, trg) || dist <= 25;
                (good, good)
            } else {
                if direct {
                    self.set_aim(c, act, trg);
                }
                (false, false)
            }
        } else {
            (false, false)
        };
        if !aim {
            self.mind.bad_aim_ms = self.mind.bad_aim_ms.saturating_add(10);
            if self.mind.bad_aim_ms > 4000 {
                if empty_for_worm(c.t, b, me.pos + Vec2::new(0.0, -5.0)) == Some(true) {
                    act.jump = true;
                }
                self.mind.bad_aim_ms = 0;
            }
            return shoot;
        }
        self.mind.bad_aim_ms = 0;
        self.mind.last_shoot_target = trg;
        act.shoot = true;
        self.mind.last_shoot = Some(c.now);
        true
    }

    /// `AI_Shoot`'s projectile case (`CWormBot.cpp:2125-2255`): the speed
    /// along the line with the worm's own, a lead of `0.7 · dist / v`, the
    /// feasibility test, then the aim by distance: from 50 px with the
    /// trajectory check for slow lobbed weapons, from 10 px without it,
    /// closer not at all. Gives `(bAim, bShoot)`.
    fn aim_projectile(
        &mut self,
        c: &Ctx,
        b: &mut Budget,
        act: &mut Act,
        w: &Worm,
        f: &Facts,
        direct: bool,
    ) -> (bool, bool) {
        let me = c.me;
        if f.dirty() {
            return (false, false);
        }
        let dir = (w.pos - me.pos).normalize();
        let my_speed = dir.x * me.vel.x + dir.y * me.vel.y;
        let v = f.speed + f.speed_var * 100.0 + my_speed;
        if v.is_nan() || v < 0.0 {
            return (false, false);
        }
        // y up (OLX's "real-world" coordinates), then the lead.
        let (mut x, mut y) = (w.pos.x - me.pos.x, me.pos.y - w.pos.y);
        let t = if v != 0.0 { libm::sqrtf(x * x + y * y) / v * 0.7 } else { 0.0 };
        x += t * w.vel.x;
        y -= t * w.vel.y;
        let lead = me.pos + Vec2::new(x, -y);
        let g = f.chain_gravity * c.s.proj_gravity_factor;
        let launch = Launch {
            speed: f.speed + self.noise.speed,
            gravity: f.gravity * c.s.proj_gravity_factor + self.noise.gravity,
            spawn_distance: c.s.shoot_spawn_distance,
            carried: if c.s.proj_relative_vel { me.vel * f.carry } else { Vec2::ZERO },
        };
        let solution = match self.cfg.aim {
            AimModel::Ballistic => ballistic_from(me.pos, lead, &launch, c.s),
            AimModel::OlxDirect => None,
        };
        let mut aim = match self.cfg.aim {
            AimModel::Ballistic => solution.is_some(),
            AimModel::OlxDirect => olx_aiming_angle(v, g, x, y, c.s).0,
        };
        if !aim {
            return (false, false);
        }
        // Fast or straight weapons need a free line.
        if g <= 10.0 || v >= 200.0 {
            aim = direct;
        }
        if !aim {
            return (false, false);
        }
        let set_aim = |bot: &mut Bot, act: &mut Act| match solution {
            Some(s) => bot.set_aim_at(c, act, s),
            None => bot.set_aim(c, act, w.pos),
        };
        let r2 = (w.pos - me.pos).length2();
        if r2 >= 2500.0 {
            let aim = set_aim(self, act);
            if aim && g >= 10.0 && v <= 200.0 {
                let hits = self.weapon_can_hit(c, b, &launch, lead);
                return (hits, hits);
            }
            (aim, true)
        } else if r2 >= 100.0 {
            // So close that it can hardly miss (in mortar games OLX keeps
            // `bShoot` as it was: true here too).
            (set_aim(self, act), true)
        } else {
            (false, false)
        }
    }

    /// `weaponCanHit` (`CWormBot.cpp:1823-1893`) with its noise on the
    /// target: whether the shot stays clear of rock and dirt until it gets
    /// there. Ballistic aim flies the real arc (notes §4.2; 2 px steps,
    /// the centre pixel only); OLX's aim checks its parabola through the
    /// worm and the target with the worm's angle as tangent.
    fn weapon_can_hit(&mut self, c: &Ctx, b: &mut Budget, l: &Launch, lead: Vec2) -> bool {
        if !matches!(self.mind.target, crate::brain::Target::Worm(_)) {
            return false;
        }
        let trg = lead + self.noise.target;
        let me = c.me;
        match self.cfg.aim {
            AimModel::Ballistic => {
                let dir = aim_direction(me.face, me.angle);
                fly_clear(c.t, b, me.pos + dir * l.spawn_distance, dir * l.speed + l.carried, l.gravity, trg)
            }
            AimModel::OlxDirect => {
                let mut alpha = me.angle.to_radians();
                if trg.y > me.pos.y {
                    alpha = -alpha;
                }
                parabola_clear(c.t, b, me.pos, alpha, trg)
            }
        }
    }
}

/// Flies a shot from `p` with velocity `v` under gravity `g` in steps of at
/// most 2 px: false at the first rock or dirt pixel (or the map's edge)
/// before it comes within 5 px of `trg` or passes it.
fn fly_clear(t: &impl Ground, b: &mut Budget, mut p: Vec2, mut v: Vec2, g: f32, trg: Vec2) -> bool {
    if b.afford(ARC_STEPS) != Afford::Yes
        || !(p.x.is_finite() && p.y.is_finite() && v.x.is_finite() && v.y.is_finite())
    {
        return false;
    }
    let side = libm::copysignf(1.0, trg.x - p.x);
    let mut probes = 0u32;
    let mut clear = false;
    for _ in 0..ARC_STEPS {
        if (p - trg).length2() <= 25.0 || (trg.x - p.x) * side < 0.0 {
            clear = true;
            break;
        }
        probes += 1;
        if t.flags(p.x as i64, p.y as i64) & (PX_DIRT | PX_ROCK) != 0 {
            break;
        }
        let speed = v.length().max(1.0);
        let dt = 2.0 / speed;
        p += v * dt;
        v.y += g * dt;
    }
    b.spend(probes);
    clear
}

/// OLX's `Parabola(p1, angle, p2)` (`src/common/MathLib.cpp:204-225`) and
/// `weaponCanHit`'s walk along it, 1 px at a time from 5 px past the left
/// end to the right one (the centre pixel only).
fn parabola_clear(t: &impl Ground, b: &mut Budget, me: Vec2, alpha: f32, trg: Vec2) -> bool {
    let (from, to) = if me.x > trg.x { (trg, me) } else { (me, trg) };
    if me.x == trg.x {
        return true;
    }
    let (p, r, s, tt) = (me.x, me.y, trg.x, trg.y);
    let (cos_f, sin_f) = (libm::cosf(alpha), libm::sinf(alpha));
    let denom = (p - s) * (p - s) * cos_f;
    let (a, bb, cc) = if denom == 0.0 {
        (0.0, 0.0, 0.0)
    } else {
        (
            -((r - tt) * cos_f + (s - p) * sin_f) / denom,
            (2.0 * p * (r - tt) * cos_f + (p + s) * (s - p) * sin_f) / denom,
            ((p * p * tt - 2.0 * p * r * s + r * s * s) * cos_f + p * s * (p - s) * sin_f) / denom,
        )
    };
    if a > 0.1 {
        return false;
    }
    let span = to.x - from.x;
    let steps = if span.is_finite() && span > 0.0 { span as u64 + 1 } else { 0 };
    if b.afford(steps) != Afford::Yes {
        return false;
    }
    let mut probes = 0u32;
    let mut clear = true;
    for i in 0..steps {
        let x = from.x + 5.0 + i as f32;
        if x >= to.x {
            break;
        }
        let y = a * x * x + bb * x + cc;
        probes += 1;
        if t.flags(x as i64, y as i64) & (PX_DIRT | PX_ROCK) != 0 {
            clear = false;
            break;
        }
    }
    b.spend(probes);
    clear
}
