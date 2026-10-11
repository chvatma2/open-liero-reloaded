//! OLX's Deathmatch brain (notes §1.2-1.3, §4.2): `getInput`'s control
//! flow, the state machine THINK ↔ MOVINGTOTARGET, `AI_Think`'s target and
//! bonus choices, `AI_CreatePath`, and `AI_SetAim` for the rest of the bot.
//! Shooting is [`crate::shoot`], moving [`crate::moving`].

use rl_math::{AbsTime, Vec2};
use rl_sim::{BonusKind, Dir, Worm};

use crate::aim::olx_direct;
use crate::bot::{Act, Bot, Ctx};
use crate::budget::Budget;
use crate::nav::{SearchStatus, worm_line_clear};
use crate::probe::good_spawn_point;

/// `AI_THINK`, `AI_MOVINGTOTARGET` (`src/common/CWormBot.cpp:69-75`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum State {
    #[default]
    Think,
    Moving,
}

/// `nAITargetType` with what it points at (`AIT_*`, `CWormBot.cpp:78-83`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Target {
    #[default]
    None,
    /// A worm, by id.
    Worm(i32),
    /// A bonus, by its slot in the client's bonuses.
    Bonus(usize),
    Position(Vec2),
}

/// `AI_Think`'s interval (`thinkInterval`, Deathmatch).
const THINK_MS: u64 = 3000;
/// Thinking again while a search runs (`getInput`).
const PENDING_THINK_MS: u64 = 500;
/// `AI_CreatePath` restarts a search that has run this long.
const SEARCH_RESTART_MS: u64 = 5000;
/// ... and starts no new one within this of the last path found.
const CREATE_MS: u64 = 500;
/// `AI_Carve`'s interval.
const CARVE_MS: u64 = 200;

/// The brain's memory (`CWormBotInputHandler`'s members, `include/CWormBot.h`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mind {
    pub state: State,
    pub target: Target,
    /// `cPosTarget`.
    pub pos_target: Vec2,
    /// `fLastThink`.
    pub last_think: Option<AbsTime>,
    /// `bPathFinished`: no search runs.
    pub path_finished: bool,
    /// `fSearchStartTime`.
    pub search_start: AbsTime,
    /// `fLastCreated`.
    pub last_created: Option<AbsTime>,
    /// `fLastShoot`.
    pub last_shoot: Option<AbsTime>,
    /// `vLastShootTargetPos`.
    pub last_shoot_target: Vec2,
    /// `fLastJump`.
    pub last_jump: Option<AbsTime>,
    /// The worm's `fLastCarve`.
    pub last_carve: Option<AbsTime>,
    /// `fLastWeaponChange`.
    pub last_weapon_change: Option<AbsTime>,
    /// `bStuck`, `cStuckPos`, `fStuckTime` (ms), `fStuckPause`.
    pub stuck: bool,
    pub stuck_pos: Vec2,
    pub stuck_ms: u64,
    pub stuck_pause: AbsTime,
    /// `fBadAimTime` (ms).
    pub bad_aim_ms: u64,
    /// This tick's jump check: the node it was for and its answer.
    pub jump_check: Option<(AbsTime, Vec2, Option<bool>)>,
}

impl Mind {
    pub fn new() -> Mind {
        Mind {
            state: State::Think,
            target: Target::None,
            pos_target: Vec2::ZERO,
            last_think: None,
            path_finished: true,
            search_start: AbsTime(0),
            last_created: None,
            last_shoot: None,
            last_shoot_target: Vec2::ZERO,
            last_jump: None,
            last_carve: None,
            last_weapon_change: None,
            stuck: false,
            stuck_pos: Vec2::ZERO,
            stuck_ms: 0,
            stuck_pause: AbsTime(0),
            bad_aim_ms: 0,
            jump_check: None,
        }
    }
}

/// Milliseconds from `t` to `now` (forever for `None`).
pub(crate) fn since(now: AbsTime, t: Option<AbsTime>) -> u64 {
    t.map_or(u64::MAX, |t| now.0.saturating_sub(t.0))
}

fn point(p: Vec2) -> (i32, i32) {
    (p.x as i32, p.y as i32)
}

fn vec((x, y): (i32, i32)) -> Vec2 {
    Vec2::new(x as f32, y as f32)
}

impl Bot {
    /// `getInput` (`CWormBot.cpp:1022-1122`) for a living worm after the
    /// spawn pause. Shooting gets at most a quarter of the budget and the
    /// moving the rest of half of it; the search (OLX's thread) runs on
    /// with what is left.
    pub(crate) fn get_input(&mut self, c: &Ctx, b: &mut Budget) -> Act {
        let mut act = Act::default();
        self.noise.refresh(c.now, &self.params, &mut self.rng);
        let since_think = since(c.now, self.mind.last_think);
        if since_think > THINK_MS && self.mind.state != State::Think {
            self.mind.state = State::Think;
        }
        if since_think > PENDING_THINK_MS && !self.mind.path_finished {
            self.mind.state = State::Think;
        }
        if let Target::Worm(id) = self.mind.target
            && !c.w.worms().iter().any(|w| w.id == id && w.alive)
        {
            self.mind.state = State::Think;
        }
        // `AI_Carve`: always; it makes no problems and can only help.
        if since(c.now, self.mind.last_carve) > CARVE_MS {
            self.mind.last_carve = Some(c.now);
            act.carve = true;
        }
        let mut half = b.split(b.left() / 2);
        let mut part = half.split(half.left() / 2);
        let shooting = self.shoot(c, &mut part, &mut act);
        half.absorb(part);
        if shooting {
            // Don't move along the rope when it pulls where we shoot.
            let me = c.me;
            if me.rope.attached {
                let force = me.rope.force(me.pos, c.s);
                let mut rope_angle = libm::atanf(force.x / force.y);
                if force.x < 0.0 || force.y > 0.0 {
                    rope_angle = -rope_angle;
                }
                if libm::fabsf(me.angle - rope_angle.to_degrees()) <= 50.0 {
                    act.rope_release = true;
                }
            }
            act.mv = false;
        } else {
            self.reload_weapons(c.me);
            match self.mind.state {
                State::Think => self.ai_think(c, &mut half),
                State::Moving => self.move_to_target(c, &mut half, &mut act),
            }
        }
        b.absorb(half);
        self.step_search(c, b);
        act
    }

    /// The search goes on with `b` (OLX's pathfinding thread).
    pub(crate) fn step_search(&mut self, c: &Ctx, b: &mut Budget) {
        if self.search.status() == SearchStatus::Pending {
            self.search.step(c.t, b);
        }
    }

    /// `AI_Think` (`CWormBot.cpp:1430-1514`), Deathmatch: critical health
    /// first, a weapon bonus for an empty slot, the nearest enemy, else
    /// health, else a random high spot. A scripted goal is a race: no
    /// bonuses.
    fn ai_think(&mut self, c: &Ctx, b: &mut Budget) {
        let me = c.me;
        self.mind.target = Target::None;
        self.mind.last_think = Some(c.now);
        self.reload_weapons(me);
        if self.goal.is_none() {
            if me.health < 15.0 && self.find_bonus(c, b, BonusKind::Health) {
                return;
            }
            let empty = me.slots.iter().position(|s| self.arsenal.get(s.weapon).is_none());
            if let Some(i) = empty
                && self.find_bonus(c, b, BonusKind::Weapon)
            {
                // The empty slot is what the pickup fills.
                self.weapon = i;
                return;
            }
        }
        if self.find_new_target(c) {
            self.create_path(c, b, false);
            return;
        }
        self.mind.last_shoot = None;
        if me.health < 80.0 && self.find_bonus(c, b, BonusKind::Health) {
            return;
        }
        // High ground is safer; already there, look for health. (OLX
        // checks `nAIState == AI_MOVINGTOTARGET`, never true here.)
        if self.mind.pos_target.y < 75.0 && self.mind.state == State::Moving {
            if me.health < 100.0 {
                self.find_bonus(c, b, BonusKind::Health);
            }
            return;
        }
        if self.find_random_spot(c, b) {
            self.create_path(c, b, false);
        }
    }

    /// `findNewTarget` (`CWormBot.cpp:1180-1293`): the scripted goal (a
    /// race's flag), else the nearest enemy.
    pub(crate) fn find_new_target(&mut self, c: &Ctx) -> bool {
        if let Some(goal) = self.goal {
            self.mind.target = Target::Position(goal);
            self.mind.pos_target = goal;
            self.mind.state = State::Moving;
            return true;
        }
        match self.find_target(c) {
            Some(w) => {
                self.mind.target = Target::Worm(w.id);
                self.mind.state = State::Moving;
                true
            }
            None => false,
        }
    }

    /// `findTarget` (`CWormBot.cpp:1297-1364`): the nearest living enemy.
    /// OLX prefers one in sight only when the squared distances differ by
    /// at most 50 (`fSightDistance - fDistance > 50` compares squares),
    /// so in effect it is the nearest; its line traces are dropped (notes
    /// §4.2). There are no teams in Deathmatch.
    pub(crate) fn find_target<'a>(&self, c: &Ctx<'a>) -> Option<&'a Worm> {
        let mut best: Option<(&Worm, f32)> = None;
        for w in c.w.worms() {
            if !w.alive || w.id == c.me.id {
                continue;
            }
            let l = (w.pos - c.me.pos).length2();
            if best.is_none_or(|(_, d)| l < d) {
                best = Some((w, l));
            }
        }
        best.map(|(w, _)| w)
    }

    /// The worm `nAITargetWormId` names, if it lives.
    pub(crate) fn target_worm<'a>(&self, c: &Ctx<'a>) -> Option<&'a Worm> {
        let Target::Worm(id) = self.mind.target else { return None };
        c.w.worms().iter().find(|w| w.id == id && w.alive)
    }

    /// `AI_FindBonus` (`CWormBot.cpp:1551-1589`): with bonuses on, head for
    /// the nearest bonus of `kind`.
    fn find_bonus(&mut self, c: &Ctx, b: &mut Budget, kind: BonusKind) -> bool {
        if !c.s.bonuses {
            return false;
        }
        let mut best: Option<(usize, f32)> = None;
        for (i, bonus) in c.w.bonuses().iter().enumerate() {
            if bonus.used && bonus.kind == kind {
                let d2 = (bonus.pos - c.me.pos).length2();
                if best.is_none_or(|(_, d)| d2 < d) {
                    best = Some((i, d2));
                }
            }
        }
        let Some((i, _)) = best else { return false };
        self.mind.target = Target::Bonus(i);
        self.mind.state = State::Moving;
        self.create_path(c, b, false);
        true
    }

    /// `findRandomSpot(highSpot = true)` (`CWormBot.cpp:1516-1541`): up to
    /// 10 tries in the top fifth of the map for a spot without rock.
    fn find_random_spot(&mut self, c: &Ctx, b: &mut Budget) -> bool {
        let (w, h) = (c.t.width() as f32, c.t.height() as f32 / 5.0);
        for _ in 0..10 {
            let x = (libm::fabsf(self.rng.random_num()) * w) as i64;
            let y = (libm::fabsf(self.rng.random_num()) * h) as i64;
            if good_spawn_point(c.t, b, x, y) != Some(true) {
                continue;
            }
            self.mind.pos_target = Vec2::new(x as f32, y as f32);
            self.mind.target = Target::Position(self.mind.pos_target);
            self.mind.state = State::Moving;
            return true;
        }
        false
    }

    /// `AI_GetTargetPos` (`CWormBot.cpp:1609-1644`): where the target is;
    /// back to THINK when it is gone.
    pub(crate) fn target_pos(&mut self, c: &Ctx) -> Vec2 {
        match self.mind.target {
            Target::Bonus(i) => {
                if let Some(bonus) = c.w.bonuses().get(i) {
                    if !bonus.used {
                        self.mind.state = State::Think;
                    }
                    return bonus.pos;
                }
            }
            Target::Position(p) => return p,
            Target::Worm(_) => {
                if let Some(w) = self.target_worm(c) {
                    return w.pos;
                }
            }
            Target::None => {}
        }
        self.mind.state = State::Think;
        Vec2::ZERO
    }

    /// Where the path goes: the target, or a spot to shoot a worm from
    /// (`AI_FindShootingSpot`).
    pub(crate) fn path_target(&mut self, c: &Ctx, b: &mut Budget) -> Vec2 {
        let trg = self.target_pos(c);
        match self.target_worm(c) {
            Some(w) => self.shooting_spot(c, b, w),
            None => trg,
        }
    }

    /// `AI_CreatePath(force_break)` (`CWormBot.cpp:2869-2955`): installs a
    /// path the search found; restarts a running search after 5 s, or when
    /// no worm line joins the worm to where the search started or the
    /// target to where it was (a moving target alone does not restart it);
    /// extends the path when its last node sees the target; and starts a
    /// new search at most every 0.5 s after a path was found. Gives whether
    /// a path is there.
    pub(crate) fn create_path(&mut self, c: &Ctx, b: &mut Budget, force: bool) -> bool {
        let trg = self.path_target(c, b);
        let me = c.me;
        let start = |bot: &mut Bot| {
            bot.mind.path_finished = false;
            bot.mind.search_start = c.now;
            bot.search.start(point(me.pos), point(trg));
        };
        if force {
            start(self);
            return false;
        }
        if !self.mind.path_finished {
            match self.search.status() {
                SearchStatus::Pending => {
                    let (from, to) = self.search.endpoints();
                    let restart = since(c.now, Some(self.mind.search_start)) >= SEARCH_RESTART_MS
                        || worm_line_clear(c.t, b, me.pos, vec(from)) == Some(false)
                        || worm_line_clear(c.t, b, trg, vec(to)) == Some(false);
                    if restart {
                        start(self);
                    }
                    return false;
                }
                status => {
                    self.mind.path_finished = true;
                    if status == SearchStatus::Found {
                        self.follow.set(self.search.path());
                        self.search.cancel();
                        self.mind.last_created = Some(c.now);
                        return true;
                    }
                    self.search.cancel();
                }
            }
        }
        // Don't search again while the path's end still sees the target.
        if let Some(last) = self.follow.last()
            && worm_line_clear(c.t, b, last, trg) == Some(true)
        {
            for i in 0..self.follow.nodes().len() {
                match worm_line_clear(c.t, b, self.follow.nodes()[i], me.pos) {
                    Some(true) => {
                        self.follow.head_for(i);
                        if self.follow.last() != Some(trg) {
                            self.follow.push(trg);
                        }
                        return true;
                    }
                    Some(false) => {}
                    None => break,
                }
            }
        }
        if since(c.now, self.mind.last_created) <= CREATE_MS {
            return !self.follow.nodes().is_empty();
        }
        start(self);
        false
    }

    /// `onRespawn` and `AI_Respawn` (`CWormBot.cpp:4120-4126`, `993-997`):
    /// think again, and search a path to the nearest enemy at once. The old
    /// path is dropped (OLX follows it until the new one is found).
    pub(crate) fn respawned(&mut self, c: &Ctx, b: &mut Budget) {
        self.mind.state = State::Think;
        self.mind.last_shoot = None;
        self.mind.stuck = false;
        self.mind.stuck_ms = 0;
        self.follow.clear();
        if self.find_new_target(c) {
            self.create_path(c, b, true);
        }
    }

    /// `AI_SetAim(pos)` (`CWormBot.cpp:1651-1713`): turn towards `pos`;
    /// whether the aim is good enough to fire.
    pub(crate) fn set_aim(&mut self, c: &Ctx, act: &mut Act, pos: Vec2) -> bool {
        self.set_aim_at(c, act, olx_direct(c.me.pos, pos))
    }

    /// `AI_SetAim` for a side and angle (the ballistic solver's).
    pub(crate) fn set_aim_at(&mut self, c: &Ctx, act: &mut Act, wanted: (Dir, f32)) -> bool {
        let aim = self.aim.set_aim(c.me, wanted, c.now, &self.params, &mut self.rng, c.s);
        act.aim = Some(aim.angle);
        aim.good
    }
}
