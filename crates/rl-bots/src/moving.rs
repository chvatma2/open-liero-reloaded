//! Moving along a path (notes §1.5): `AI_MoveToTarget` with its rope,
//! jump, dig and stuck handling and the 100lt, rifle and mortar tactics,
//! `AI_SimpleMove` without a path, `AI_Jump` and the checks before jumping
//! or walking.

use rl_math::Vec2;
use rl_sim::Dir;
use rl_terrain::{PX_DIRT, PX_EMPTY};

use crate::bot::{Act, Bot, Ctx};
use crate::brain::{State, Target, since};
use crate::budget::Budget;
use crate::loadout::GameType;
use crate::nav::{jump_loses_node, worm_line_clear};
use crate::probe::{trace_line, trace_line_dist, trace_weapon_line};

/// `AI_Jump`'s interval (OLX reads the wall clock here).
const JUMP_MS: u64 = 300;
/// `BotAngleSpeed` (`CWormBot.cpp:1646`) for one 10 ms tick.
const ANGLE_STEP: f32 = 1.0;

impl Bot {
    /// `AI_MoveToTarget` (`CWormBot.cpp:3370-3942`).
    pub(crate) fn move_to_target(&mut self, c: &Ctx, b: &mut Budget, act: &mut Act) {
        let me = c.me;
        let target_gone = match self.mind.target {
            Target::None => true,
            Target::Worm(id) => !c.w.worms().iter().any(|w| w.id == id),
            _ => false,
        };
        if target_gone {
            self.mind.state = State::Think;
            return;
        }
        act.mv = false;
        act.shoot = false;
        act.jump = false;
        self.rope_timers(me, act);
        self.mind.pos_target = self.path_target(c, b);
        let can_shoot = self.goal.is_none();
        let since_shoot = since(c.now, self.mind.last_shoot);
        let mortars = can_shoot && self.game_type == GameType::Mortars;
        if mortars {
            // Get away from where the last mortar went (0.2-1 s after it).
            let away = self.mind.last_shoot_target.x - me.pos.x;
            if libm::copysignf(1.0, me.vel.x) == libm::copysignf(1.0, away)
                && (200..=1000).contains(&since_shoot)
            {
                if me.rope.attached
                    && libm::copysignf(1.0, me.rope.force(me.pos, c.s).x) == libm::copysignf(1.0, away)
                {
                    act.rope_release = true;
                }
                self.aim.set_face(if me.pos.x < self.mind.last_shoot_target.x {
                    Dir::Left
                } else {
                    Dir::Right
                });
                act.mv = true;
            }
        }
        if can_shoot && matches!(self.game_type, GameType::Mortars | GameType::Lt100) && me.on_ground {
            // Jump when an enemy is within 50 px in the open.
            for w in c.w.worms() {
                if w.id != me.id
                    && w.alive
                    && (me.pos - w.pos).length2() <= 2500.0
                    && let Some((_, ty)) = trace_line(c.t, b, me.pos, w.pos, 5)
                    && ty & PX_EMPTY != 0
                {
                    self.ai_jump(c, b, act);
                }
            }
        }
        if mortars && since_shoot <= 1000 {
            return;
        }
        if self.mind.stuck {
            act.mv = true;
            self.ai_jump(c, b, act);
            if since(c.now, self.aim.last_face()) >= 500 {
                self.aim.turn_around(me, c.now);
            }
            if c.now.0.saturating_sub(self.mind.stuck_pause.0) > 2000 {
                self.mind.stuck = false;
            }
            return;
        }
        if mortars && since_shoot <= 200 {
            if self.rope.attached_ms >= 100 {
                act.rope_release = true;
            }
            return;
        }
        if self.follow.nodes().is_empty() {
            self.mind.state = State::Think;
            self.simple_move(c, b, act);
            return;
        }
        let step = self.follow.advance(me.pos, c.t, b, c.now, c.s);
        let Some(node) = step.node else { return };
        self.mind.jump_check = Some((c.now, node, step.jump_loses_node));
        if me.rope.attached {
            // Let go when the rope pulls away from the node.
            let pull = me.rope.force(me.pos, c.s).normalize();
            if (pull + me.pos - node).length2() > (me.pos - node).length2() {
                act.rope_release = true;
                self.rope.attached_ms = 0;
            }
        }
        let we_see_the_target = self.sees(c, b, self.mind.pos_target);
        self.dig_on(c, b, node);
        if can_shoot && self.game_type == GameType::Rifles && self.params.rifle_reload_before_engage {
            // Rifles: don't go to battle unloaded.
            let loaded = me.slots.iter().take(5).filter(|s| !s.reloading).count();
            if loaded <= 3 && !we_see_the_target {
                let left = self.follow.nodes_left();
                if left >= 3 {
                    self.reload_weapons(me);
                }
                if (3..=5).contains(&left) {
                    if self.rope.attached_ms >= 700 {
                        act.rope_release = true;
                    }
                    return;
                }
            }
        }
        let fire_ninja = self.dig_or_rope(c, b, act, node, can_shoot);
        let fire_ninja = fire_ninja
            && !(can_shoot
                && self.game_type == GameType::Rifles
                && we_see_the_target
                && (me.pos - self.mind.pos_target).length2() <= 3600.0);
        if self.rope_or_walk(c, b, act, node, fire_ninja, we_see_the_target) {
            return;
        }
        self.overshoot(c, node, act);
        if self.stuck_check(c, b, act) {
            return;
        }
        act.mv = if can_shoot { libm::fabsf(me.pos.x - node.x) > 3.0 } else { true };
    }

    /// At the path's end with the target worm out of sight, head for the
    /// worm itself: the path ends at a shooting spot, which on a level of
    /// dirt (where `AI_FindShootingSpot` finds no spot in the air) is as
    /// deep in the dirt as the worm, so OLX's bot stands there unable to
    /// shoot. Digging on (paths ignore dirt) opens the line of fire. A
    /// deviation (`docs/porting/STATUS.md`).
    fn dig_on(&mut self, c: &Ctx, b: &mut Budget, node: Vec2) {
        let me = c.me;
        if self.goal.is_some() || self.follow.nodes_left() > 0 || (node - me.pos).length2() > 100.0 {
            return;
        }
        let Some(w) = self.target_worm(c) else { return };
        if (w.pos - node).length2() > 25.0 && !self.sees(c, b, w.pos) {
            self.follow.push(w.pos);
        }
    }

    /// Whether `traceWeaponLine` reaches `p` through empty pixels only.
    fn sees(&mut self, c: &Ctx, b: &mut Budget, p: Vec2) -> bool {
        let (first, divs) = self.weapon_line_steps(c);
        trace_weapon_line(c.t, b, c.me.pos, p, first, divs).is_some_and(|(_, ty)| ty & PX_EMPTY != 0)
    }

    /// Dirt above: a clearing weapon, else the rope; dirt just ahead:
    /// walk and carve through it (jumping to nodes 10-30 px up); a free way:
    /// a jetpack or the rope (`CWormBot.cpp:3686-3745`). Gives whether to
    /// throw the rope.
    fn dig_or_rope(&mut self, c: &Ctx, b: &mut Budget, act: &mut Act, node: Vec2, can_shoot: bool) -> bool {
        let me = c.me;
        let Some((length, _, ty)) = trace_line_dist(c.t, b, me.pos, node, 5) else { return false };
        let direct = (length as f32) * (length as f32) > (node - me.pos).length2();
        let mut fire_ninja = false;
        if libm::fabsf(node.x - me.pos.x) <= 50.0 && ty & PX_DIRT != 0 && node.y < me.pos.y {
            match self.clearing_weapon(c).filter(|_| can_shoot) {
                Some(w) => {
                    self.weapon = w;
                    act.shoot = self.set_aim(c, act, node);
                    if act.shoot {
                        act.mv = false;
                        act.jump = false;
                    }
                }
                None => {
                    self.set_aim(c, act, node);
                    self.ai_jump(c, b, act);
                    fire_ninja = true;
                }
            }
        }
        if !fire_ninja && !direct && ty & PX_DIRT != 0 && length < 10 {
            let up = me.pos.y - node.y;
            if (10.0..=30.0).contains(&up) {
                self.ai_jump(c, b, act);
            }
            return false;
        }
        // A free way: the rope is faster, or a jetpack (`Recoil < 0`).
        let jetpack = (0..me.slots.len()).find(|&i| {
            self.arsenal.get(me.slots[i].weapon).is_some_and(|f| f.recoil < 0) && !me.slots[i].reloading
        });
        if let Some(i) = jetpack {
            self.weapon = i;
            act.shoot = self.set_aim(c, act, node);
            if act.shoot {
                return false;
            }
        }
        true
    }

    /// The rope towards the best spot for `node`, or walking and jumping
    /// (`CWormBot.cpp:3770-3851`). Gives whether `AI_MoveToTarget` returns
    /// (aiming at a rope spot that walking would lose).
    fn rope_or_walk(
        &mut self,
        c: &Ctx,
        b: &mut Budget,
        act: &mut Act,
        node: Vec2,
        want_rope: bool,
        we_see_the_target: bool,
    ) -> bool {
        let me = c.me;
        let (mut fire_ninja, mut still_aiming, mut spot) = (false, false, node);
        if want_rope {
            spot = self.best_rope_spot(c, b, node);
            let aim = self.set_aim(c, act, spot);
            still_aiming = !aim;
            if aim || !me.on_ground {
                fire_ninja = !me.rope.released || (me.rope.attached && self.rope.attached_ms > 500);
                if me.pos.y - node.y > 10.0 {
                    self.ai_jump(c, b, act);
                }
            }
        }
        if fire_ninja {
            act.rope_shoot = true;
            self.rope.falling_ms = 0;
            self.rope.attached_ms = 0;
            return false;
        }
        if !we_see_the_target && !still_aiming {
            self.set_aim(c, act, node);
        }
        let up = me.pos.y - node.y;
        if me.can_air_jump {
            if up > 10.0 && libm::fabsf(up) > libm::fabsf(me.pos.x - node.x) {
                self.ai_jump(c, b, act);
            }
            act.mv = true;
        } else if up <= 30.0 && up > 10.0 && !self.ai_jump(c, b, act) {
            act.mv = true;
        }
        if still_aiming && self.move_loses(c, b, me.pos + (spot - me.pos) * 0.8) {
            act.mv = false;
            return true;
        }
        false
    }

    /// Let go of the rope before flying past a node above
    /// (`CWormBot.cpp:3853-3866`): OLX's test divides by a zero force, so it
    /// lets go whenever the hook is below the worm, within 50 px sideways,
    /// the node above and the worm not falling (and gravity pulls down).
    fn overshoot(&mut self, c: &Ctx, node: Vec2, act: &mut Act) {
        let me = c.me;
        if me.rope.attached
            && !me.on_ground
            && me.rope.pos.y > me.pos.y
            && node.y < me.pos.y
            && libm::fabsf(me.rope.pos.x - me.pos.x) <= 50.0
            && me.vel.y <= 0.0
            && (node - me.pos).length() > 0.0
            && c.s.worm_gravity > 0.0
        {
            act.rope_release = true;
        }
    }

    /// The stuck check (`CWormBot.cpp:3870-3910`): 3 s within 5 px makes
    /// the bot stuck (jump, walk, carve and turn the aim up, then 2 s of
    /// flailing about); at 5 s it heads back a node. Gives whether
    /// `AI_MoveToTarget` returns.
    fn stuck_check(&mut self, c: &Ctx, b: &mut Budget, act: &mut Act) -> bool {
        let me = c.me;
        let m = &mut self.mind;
        if libm::fabsf(m.stuck_pos.x - me.pos.x) < 5.0 && libm::fabsf(m.stuck_pos.y - me.pos.y) < 5.0 {
            m.stuck_ms = m.stuck_ms.saturating_add(10);
            if m.stuck_ms > 3000 {
                self.ai_jump(c, b, act);
                let m = &mut self.mind;
                m.stuck = true;
                m.stuck_pause = c.now;
                let top = if c.s.full_aim_angle { 90.0 } else { 60.0 };
                act.aim = Some((me.angle - ANGLE_STEP).clamp(-90.0, top));
                act.mv = true;
                act.carve = true;
                if m.stuck_ms >= 5000 {
                    m.stuck_ms = 0;
                    self.follow.back();
                }
                self.create_path(c, b, false);
                return true;
            }
        } else {
            m.stuck = false;
            m.stuck_ms = 0;
            m.stuck_pos = me.pos;
        }
        false
    }

    /// `AI_SimpleMove` (`CWormBot.cpp:1719-1768`): without a path, walk at
    /// the target; when the way is blocked at once or the target is above,
    /// turn around every second and throw the rope at a random spot below
    /// the worm (OLX's "look up" aims down), else jump and walk.
    fn simple_move(&mut self, c: &Ctx, b: &mut Budget, act: &mut Act) {
        let me = c.me;
        act.mv = true;
        act.shoot = false;
        act.jump = false;
        let have_target = matches!(self.mind.target, Target::Worm(_));
        self.mind.pos_target = self.path_target(c, b);
        let trg = self.mind.pos_target;
        self.set_aim(c, act, trg);
        let Some((_, dist, _)) = trace_line_dist(c.t, b, me.pos, trg, 1) else { return };
        if dist < 0.75 || trg.y < me.pos.y {
            if have_target && since(c.now, self.aim.last_face()) > 1000 {
                self.aim.turn_around(me, c.now);
            }
            let (rx, ry) = (self.rng.random_num(), self.rng.random_num());
            if self.set_aim(c, act, me.pos + Vec2::new(rx * 10.0, ry * 10.0 + 10.0)) {
                act.rope_shoot = true;
            } else {
                self.ai_jump(c, b, act);
                act.mv = true;
                act.rope_release = true;
            }
            return;
        }
        act.rope_release = true;
    }

    /// `AI_Jump` (`CWormBot.cpp:3353-3366`): at most every 0.3 s, on the
    /// ground or with an air jump, unless the jump loses the node
    /// (`isJumpingGivingDisadvantage`). Gives whether the bot jumps.
    pub(crate) fn ai_jump(&mut self, c: &Ctx, b: &mut Budget, act: &mut Act) -> bool {
        let me = c.me;
        if since(c.now, self.mind.last_jump) > JUMP_MS
            && (me.on_ground || me.can_air_jump)
            && !self.jump_loses(c, b)
        {
            self.mind.last_jump = Some(c.now);
            act.jump = true;
        }
        act.jump
    }

    /// `isJumpingGivingDisadvantage` for the node headed for, once a tick
    /// (the node advance checks it first; unchecked counts as losing it).
    fn jump_loses(&mut self, c: &Ctx, b: &mut Budget) -> bool {
        let Some(node) = self.follow.current() else { return false };
        let answer = match self.mind.jump_check {
            Some((t, n, answer)) if t == c.now && n == node => answer,
            _ => {
                let answer = jump_loses_node(node, c.me.pos, c.s, c.t, b);
                self.mind.jump_check = Some((c.now, node, answer));
                answer
            }
        };
        answer != Some(false)
    }

    /// `isMoveGivingDisadvantage` (`CWormBot.cpp:3335-3349`): whether a
    /// step (0.3 s of walking on the side faced) loses sight of `target`.
    fn move_loses(&mut self, c: &Ctx, b: &mut Budget, target: Vec2) -> bool {
        let me = c.me;
        let mut speed = if me.on_ground { c.s.worm_ground_speed } else { c.s.worm_air_speed };
        if self.aim.face(me) == Dir::Left {
            speed = -speed;
        }
        let dx = (me.vel.x + speed * 90.0).clamp(-30.0, 30.0) * 0.3;
        worm_line_clear(c.t, b, target, me.pos + Vec2::new(dx, 0.0)) == Some(false)
    }
}
