//! Mouse controls (issue #24): a client-side controller that turns the
//! mouse and the keys into the [`Keys`] a keyboard player could press each
//! tick. The mouse never reaches the simulation: the simulation, the
//! netcode, replays and the oracle see only those keys.
//!
//! - **Aim**: Up or Down toward the cursor's direction, through the bots'
//!   aim controller ([`olr_bots::motor::aim_key`]): the aim moves exactly
//!   as with keys (LX56 aim: a held key accelerates it), braked with the
//!   opposite key so it settles on the target. OLX's own mouse aim sets the
//!   angle directly (`CWormHuman.cpp:280-306`), faster off the mark than
//!   keys: not ported, on purpose (the maintainer's rule: no advantage).
//! - **Facing**: the worm faces the side of its vertical line the cursor
//!   is on, with hysteresis; the controller turns it with one Left or Right
//!   tick without Strafe, a key player's shortest tap. It never forms the
//!   dig gesture: the turn waits while the other direction has a press the
//!   simulation has not read yet, the turn's tick holds that direction
//!   alone, and it is held until a read turned the worm (a tick may run no
//!   sub-step, `World::substeps_ahead`).
//! - **A and D** move with Strafe held by the controller; the dig gesture
//!   and Dig pass through as pressed.
//! - **The wheel**: one Previous or Next weapon press per notch, queued
//!   [`WHEEL_QUEUE`] deep, each pressed once the last was read.
//! - **Look-ahead**: online the keys apply `ahead` ticks (the input delay)
//!   after the newest predicted world, so [`LocalWorm`] replays the keys
//!   the controller gave for the ticks between through a model of the
//!   simulation's input read.

use std::collections::VecDeque;

use olr_math::{AbsTime, TimeDiff, clamp_direct, reduce_const, reset_small};
use olr_sim::{Dir, Keys, Latches, World};

/// The most ticks [`LocalWorm`] looks ahead (the input delay is at most 3).
pub const MAX_AHEAD: usize = 8;

/// At most this many wheel notches wait; more are dropped.
pub const WHEEL_QUEUE: usize = 8;

/// A cursor nearer the worm than this (game pixels) aims nowhere new.
pub const NEAR: f32 = 3.0;

/// The cursor turns the worm once it is this far past the worm's vertical
/// line: a share of its distance (about 6°), and at least
/// [`FACE_MARGIN_MIN`] game pixels.
pub const FACE_MARGIN: f32 = 0.1;
pub const FACE_MARGIN_MIN: f32 = 1.5;

/// Within this many degrees of the target, and slower than the
/// simulation's own rest (5°/s), the aim is left alone.
pub const AIM_DEAD_ZONE: f32 = 1.0;

/// LX56 aim, as `olr-sim`'s `physics.rs`: a held key's acceleration, the
/// speed a release clamps to, the slow-down after it.
const AIM_ACCEL: f32 = 500.0;
const AIM_MAX_SPEED: f32 = 100.0;
const AIM_SLOW: f32 = 200.0;

const SIDES: u16 = Keys::LEFT | Keys::RIGHT;
const WHEEL: u16 = Keys::PREV_WEAPON | Keys::NEXT_WEAPON;

/// The bits the controller makes up itself: a key player's Up, Down,
/// SelectWeapon and Strafe keys are unused in mouse mode.
pub const MADE_UP: u16 = Keys::UP | Keys::DOWN | Keys::STRAFE;
const OWNED: u16 = MADE_UP | Keys::CHANGE;

/// A wheel notch: Previous (wheel up) or Next weapon (wheel down).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notch {
    Previous,
    Next,
}

impl Notch {
    fn bit(self) -> u16 {
        match self {
            Notch::Previous => Keys::PREV_WEAPON,
            Notch::Next => Keys::NEXT_WEAPON,
        }
    }
}

/// The local worm as the controller plans with it: its state before the
/// next tick, the clocks and settings its input read uses, and the
/// sub-steps (input reads) of the next ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalWorm {
    pub alive: bool,
    pub face: Dir,
    pub angle: f32,
    pub angle_speed: f32,
    pub last_input_time: AbsTime,
    pub latches: Latches,
    /// Weapon slots: none, and the wheel does nothing.
    pub slots: usize,
    /// The frame clock of the next tick ([`World::time`]) and its step.
    pub time: AbsTime,
    pub frame: TimeDiff,
    /// The aim's lowest angle: 60°, or 90° with `FullAimAngle`.
    pub max_angle: f32,
    /// `WeaponCombos`: the weapon changes while firing.
    pub weapon_combos: bool,
    /// The input reads of the next ticks (`reads[0]`: the next one).
    pub reads: [u8; MAX_AHEAD + 1],
    /// How many ticks after the next one the keys apply (the input delay).
    pub ahead: usize,
}

impl LocalWorm {
    /// Worm `i` of `world`, its keys applying `ahead` ticks after the next
    /// one; `None` without that worm.
    pub fn of(world: &World, i: usize, ahead: u32) -> Option<LocalWorm> {
        let w = world.worms().get(i)?;
        let s = world.settings();
        let ahead = (ahead as usize).min(MAX_AHEAD);
        let mut reads = [0; MAX_AHEAD + 1];
        for (k, r) in reads.iter_mut().enumerate().take(ahead + 1) {
            *r = world.substeps_ahead(k as u32).min(u32::from(u8::MAX)) as u8;
        }
        Some(LocalWorm {
            alive: w.alive,
            face: w.face,
            angle: w.angle,
            angle_speed: w.angle_speed,
            last_input_time: w.last_input_time,
            latches: *world.latches(i)?,
            slots: w.slots.len(),
            time: world.time(),
            frame: world.frame_dt(),
            max_angle: if s.full_aim_angle { 90.0 } else { 60.0 },
            weapon_combos: s.weapon_combos,
            reads,
            ahead,
        })
    }

    /// The input reads of the tick the keys are for.
    fn reads_now(&self) -> u8 {
        self.reads[self.ahead]
    }

    /// Plays one tick of `keys` on the model: the latches set, each read
    /// (the aim and facing of `olr-sim`'s `get_input`), the clock. Returns
    /// whether a read dug (the dig gesture).
    /// OLX: src/common/CWormHuman.cpp:81-560 (`getInput`, aim and moves)
    fn run(&mut self, keys: Keys, reads: u8) -> bool {
        self.latches.set(keys);
        let mut dug = false;
        for _ in 0..reads {
            dug |= self.read();
        }
        self.time += self.frame;
        dug
    }

    fn read(&mut self) -> bool {
        let k = &mut self.latches;
        if !self.alive {
            k.reset();
            return false;
        }
        let (left_once, right_once) = (k.left.is_once(), k.right.is_once());
        let dt = if self.time > self.last_input_time {
            let d = self.time - self.last_input_time;
            self.last_input_time = self.time;
            d
        } else {
            TimeDiff(0)
        };
        if k.up.is_down() {
            self.angle_speed -= AIM_ACCEL * dt.seconds();
        } else if k.down.is_down() {
            self.angle_speed += AIM_ACCEL * dt.seconds();
        } else {
            clamp_direct(&mut self.angle_speed, -AIM_MAX_SPEED, AIM_MAX_SPEED);
            reduce_const(&mut self.angle_speed, AIM_SLOW * dt.seconds());
            reset_small(&mut self.angle_speed, 5.0);
        }
        self.angle += self.angle_speed * dt.seconds();
        if clamp_direct(&mut self.angle, -90.0, self.max_angle) != 0 {
            self.angle_speed = 0.0;
        }
        let mut dug = false;
        if !k.change.is_down() {
            let strafe = k.strafe.is_down();
            let (left, right) = (k.left.is_down(), k.right.is_down());
            if left && !right && !strafe {
                self.face = Dir::Left;
            }
            if right && !left && !strafe {
                self.face = Dir::Right;
            }
            dug = (left && right_once) || (right && left_once);
        }
        k.reset();
        dug
    }
}

/// The mouse controller's state between ticks.
#[derive(Clone, Debug, Default)]
pub struct MouseController {
    /// The side the cursor is on (hysteresis); `None` until it is seen.
    side: Option<Dir>,
    /// The keys given for the last ticks, newest last ([`MAX_AHEAD`]).
    sent: VecDeque<Keys>,
    wheel: VecDeque<Notch>,
}

impl MouseController {
    pub fn new() -> Self {
        MouseController::default()
    }

    /// A wheel notch, pressed at the next tick that may (dropped beyond
    /// [`WHEEL_QUEUE`]).
    pub fn notch(&mut self, n: Notch) {
        if self.wheel.len() < WHEEL_QUEUE {
            self.wheel.push_back(n);
        }
    }

    /// The notches waiting.
    pub fn notches(&self) -> usize {
        self.wheel.len()
    }

    /// The keys of the last tick.
    pub fn last(&self) -> Keys {
        self.sent.back().copied().unwrap_or_default()
    }

    /// Forgets the cursor's side and the waiting notches (a new match, the
    /// mode switched).
    pub fn reset(&mut self) {
        *self = MouseController::default();
    }

    /// This tick's keys. `local`: the local worm before the next tick (its
    /// keys applying `ahead` ticks later), `None` without one (nothing is
    /// aimed or turned); `cursor`: the cursor's direction from the drawn
    /// worm, in game pixels (y down), `None` without one; `player`: the
    /// keys the player presses (A and D as Left and Right, the buttons,
    /// the dig gesture made), whose Up, Down, SelectWeapon and Strafe are
    /// ignored.
    pub fn keys(&mut self, local: Option<&LocalWorm>, cursor: Option<[f32; 2]>, player: Keys) -> Keys {
        let k = match local {
            Some(l) => self.plan(l, cursor, player.0 & !OWNED),
            None => {
                self.wheel.clear();
                let k = player.0 & !OWNED;
                if k & SIDES != 0 { k | Keys::STRAFE } else { k }
            }
        };
        if self.sent.len() == MAX_AHEAD {
            self.sent.pop_front();
        }
        self.sent.push_back(Keys(k));
        Keys(k)
    }

    fn plan(&mut self, local: &LocalWorm, cursor: Option<[f32; 2]>, mut k: u16) -> u16 {
        // The worm as the keys will find it: the keys already given for
        // the ticks before replayed.
        let mut w = local.clone();
        for t in 0..local.ahead {
            let back = local.ahead - t;
            let keys = self.sent.len().checked_sub(back).map_or(Keys(0), |i| self.sent[i]);
            w.run(keys, local.reads[t]);
        }
        if !w.alive {
            self.wheel.clear();
            self.side = None;
            return k;
        }
        let reads = local.reads_now();
        let side = *self.side.get_or_insert(w.face);
        let side = match cursor.filter(|c| c[0].is_finite() && c[1].is_finite()) {
            Some(c) if c[0].hypot(c[1]) >= NEAR => {
                let margin = (c[0].hypot(c[1]) * FACE_MARGIN).max(FACE_MARGIN_MIN);
                let side = if c[0] > margin {
                    Dir::Right
                } else if c[0] < -margin {
                    Dir::Left
                } else {
                    side
                };
                self.side = Some(side);
                let target = c[1].atan2(c[0].abs()).to_degrees();
                if reads > 0 {
                    k |= aim(w.angle, w.angle_speed, target.clamp(-90.0, w.max_angle));
                }
                side
            }
            _ => side,
        };
        let sides = k & SIDES;
        let (turn, other) = match side {
            Dir::Left => (Keys::LEFT, &w.latches.right),
            Dir::Right => (Keys::RIGHT, &w.latches.left),
        };
        // The turn: the side's key alone, no Strafe, not while the player
        // digs (both held) or the other key's press is still unread (a read
        // seeing it with the turn's press would dig).
        if w.face != side && sides != SIDES && !other.is_once() {
            k = (k & !SIDES) | turn;
        } else if sides != 0 {
            k |= Keys::STRAFE;
        }
        k | self.wheel_press(&w, k, reads)
    }

    /// A waiting notch's press, if the simulation will read it alone: the
    /// key up at the last tick (a press), the last press read, a read this
    /// tick, slots to change, and not firing without `WeaponCombos` (the
    /// read would drop it).
    fn wheel_press(&mut self, w: &LocalWorm, k: u16, reads: u8) -> u16 {
        if w.slots == 0 {
            self.wheel.clear();
            return 0;
        }
        let l = &w.latches;
        let firing = k & Keys::FIRE != 0 || l.fire.is_once();
        let free = self.last().0 & WHEEL == 0
            && k & WHEEL == 0
            && !l.prev_weapon.is_once()
            && !l.next_weapon.is_once()
            && reads > 0
            && (w.weapon_combos || !firing);
        if !free {
            return 0;
        }
        self.wheel.pop_front().map_or(0, Notch::bit)
    }
}

/// Up, Down or neither, to turn the aim from `angle` at `speed` to
/// `target`: the bots' controller, left alone in a small dead zone.
fn aim(angle: f32, speed: f32, target: f32) -> u16 {
    if (target - angle).abs() < AIM_DEAD_ZONE && speed.abs() < 5.0 {
        return 0;
    }
    olr_bots::motor::aim_key(angle, speed, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A worm facing right, aiming level, at rest; one read every tick.
    pub(crate) fn local() -> LocalWorm {
        LocalWorm {
            alive: true,
            face: Dir::Right,
            angle: 0.0,
            angle_speed: 0.0,
            last_input_time: AbsTime(1000),
            latches: Latches::default(),
            slots: 5,
            time: AbsTime(1010),
            frame: TimeDiff(10),
            max_angle: 60.0,
            weapon_combos: true,
            reads: [1; MAX_AHEAD + 1],
            ahead: 0,
        }
    }

    #[test]
    fn the_model_aims_as_the_simulation() {
        let mut w = local();
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        w.run(Keys(Keys::UP), 1);
        assert!(near(w.angle_speed, -5.0) && near(w.angle, -0.05));
        // No read, then one 20 ms after the last: under 5°/s it rests.
        w.run(Keys(0), 0);
        w.run(Keys(0), 1);
        assert!(w.angle_speed == 0.0 && near(w.angle, -0.05));
        // A press in a tick with no read is read at the next.
        w.run(Keys(Keys::DOWN), 0);
        w.run(Keys(0), 1);
        assert!(near(w.angle_speed, 10.0), "{}", w.angle_speed);
    }

    #[test]
    fn the_model_turns_and_digs_as_the_simulation() {
        let mut w = local();
        assert!(!w.run(Keys(Keys::LEFT | Keys::STRAFE), 1));
        assert_eq!(w.face, Dir::Right, "strafing");
        assert!(w.run(Keys(Keys::LEFT | Keys::RIGHT | Keys::STRAFE), 1), "the gesture digs");
        assert!(!w.run(Keys(Keys::LEFT), 1));
        assert_eq!(w.face, Dir::Left);
        // Right pressed in a tick with no read, then Left alone: one read
        // sees both, digs, and turns nothing.
        w.run(Keys(Keys::RIGHT | Keys::STRAFE), 0);
        assert!(w.run(Keys(Keys::LEFT), 1));
    }

    /// The two clocks of a world: 11 ms sub-steps in 10 ms ticks.
    #[derive(Clone, Copy)]
    struct Clock {
        physics: u64,
        time: u64,
    }

    impl Clock {
        fn new() -> Clock {
            Clock { physics: 0, time: 0 }
        }

        /// The reads of the tick `k` after the next.
        fn reads(&self, k: usize) -> u8 {
            let mut c = *self;
            let mut n = 0;
            for _ in 0..=k {
                n = c.step();
            }
            n
        }

        fn step(&mut self) -> u8 {
            let mut n = 0;
            while self.physics + 11 <= self.time {
                self.physics += 11;
                n += 1;
            }
            self.time += 10;
            n
        }
    }

    /// A worm and the controller playing `ticks` ticks with keys applying
    /// `ahead` ticks late (the keys between start as none), `player(t)`
    /// and `cursor(t)` what the player does; each tick's keys, the worm
    /// after it, and whether it dug.
    struct Run {
        keys: Vec<Keys>,
        worms: Vec<LocalWorm>,
        dug: Vec<bool>,
    }

    fn run(
        start: &LocalWorm,
        ahead: usize,
        ticks: usize,
        player: impl Fn(usize) -> Keys,
        cursor: impl Fn(usize) -> Option<[f32; 2]>,
    ) -> Run {
        let mut c = MouseController::new();
        let mut w = start.clone();
        let mut clock = Clock::new();
        // The model's reads in step with the clock.
        w.time = AbsTime(clock.time);
        w.last_input_time = AbsTime(0);
        let mut flight: VecDeque<Keys> = (0..ahead).map(|_| Keys(0)).collect();
        let mut out = Run { keys: vec![], worms: vec![], dug: vec![] };
        for t in 0..ticks {
            let mut seen = w.clone();
            seen.ahead = ahead;
            for k in 0..=ahead {
                seen.reads[k] = clock.reads(k);
            }
            let k = c.keys(Some(&seen), cursor(t), player(t));
            flight.push_back(k);
            let apply = flight.pop_front().unwrap();
            let reads = clock.step();
            out.dug.push(w.run(apply, reads));
            out.keys.push(k);
            out.worms.push(w.clone());
        }
        out
    }

    /// The cursor `deg` degrees below level on side `side`, 100 px away.
    fn at(deg: f32, side: Dir) -> Option<[f32; 2]> {
        let (s, c) = deg.to_radians().sin_cos();
        let x = if side == Dir::Right { 100.0 * c } else { -100.0 * c };
        Some([x, 100.0 * s])
    }

    /// The first tick the worm's aim is within 1° of `target`.
    fn reached(r: &Run, target: f32) -> Option<usize> {
        r.worms.iter().position(|w| (w.angle - target).abs() < 1.0)
    }

    #[test]
    fn aim_is_no_faster_than_holding_the_key_and_settles() {
        for (target, key) in [(-60.0, Keys::UP), (55.0, Keys::DOWN), (-10.0, Keys::UP)] {
            for ahead in [0, 2, 3] {
                let mouse = run(&local(), ahead, 400, |_| Keys(0), |_| at(target, Dir::Right));
                let mut w = local();
                let mut clock = Clock::new();
                w.time = AbsTime(0);
                w.last_input_time = AbsTime(0);
                let held_at = (0..400)
                    .position(|_| {
                        w.run(Keys(key), clock.step());
                        (w.angle - target).abs() < 1.0 || w.angle.abs() > target.abs()
                    })
                    .unwrap();
                let mouse_at = reached(&mouse, target).expect("the aim gets there");
                assert!(mouse_at >= held_at, "{target}° ahead {ahead}: {mouse_at} < {held_at}");
                // Settled: no swinging through the target afterwards, and at rest.
                let after = &mouse.worms[mouse_at..];
                let crossings =
                    after.windows(2).filter(|p| (p[0].angle - target) * (p[1].angle - target) < 0.0).count();
                assert!(crossings <= 1, "{target}° ahead {ahead}: {crossings} crossings");
                assert!(after.iter().all(|w| (w.angle - target).abs() < 3.0), "{target}° ahead {ahead}");
                let last = mouse.worms.last().unwrap();
                assert!((last.angle - target).abs() < AIM_DEAD_ZONE + 0.5 && last.angle_speed == 0.0);
                let tail = &mouse.keys[mouse.keys.len() - 100..];
                assert!(tail.iter().all(|k| k.0 & (Keys::UP | Keys::DOWN) == 0), "at rest, no key");
            }
        }
    }

    #[test]
    fn facing_follows_the_cursor_with_hysteresis() {
        // The cursor straight above, wobbling inside the margin: no turn.
        let wobble =
            run(&local(), 0, 200, |_| Keys(0), |t| Some([if t % 2 == 0 { 4.0 } else { -4.0 }, -80.0]));
        assert!(wobble.worms.iter().all(|w| w.face == Dir::Right));
        assert!(wobble.keys.iter().all(|k| k.0 & SIDES == 0));
        // Past it: Left alone until a read (the first two ticks have none).
        let turn = run(&local(), 0, 50, |_| Keys(0), |_| Some([-20.0, -80.0]));
        assert_eq!(turn.worms.last().unwrap().face, Dir::Left);
        let lefts = turn.keys.iter().filter(|k| k.0 & Keys::LEFT != 0).count();
        assert!((1..=3).contains(&lefts), "{lefts} Left ticks: until a read");
        assert!(turn.keys.iter().all(|k| k.0 & (Keys::RIGHT | Keys::STRAFE) == 0));
    }

    #[test]
    fn a_and_d_strafe_and_the_gesture_passes_through() {
        let r = run(&local(), 0, 30, |_| Keys(Keys::LEFT), |_| at(0.0, Dir::Right));
        assert!(r.keys.iter().all(|k| *k == Keys(Keys::LEFT | Keys::STRAFE)));
        assert!(r.worms.iter().all(|w| w.face == Dir::Right), "strafing keeps the facing");
        let both = |t: usize| Keys(if t == 10 { Keys::LEFT | Keys::RIGHT } else { Keys::LEFT });
        let r = run(&local(), 0, 30, both, |_| at(0.0, Dir::Right));
        assert_eq!(r.keys[10], Keys(Keys::LEFT | Keys::RIGHT | Keys::STRAFE));
        assert_eq!(r.dug.iter().filter(|d| **d).count(), 1, "the gesture digs once");
        // The player's Up, Down, SelectWeapon and Strafe are the controller's.
        let r = run(&local(), 0, 5, |_| Keys(Keys::UP | Keys::CHANGE | Keys::STRAFE | Keys::JUMP), |_| None);
        assert!(r.keys.iter().all(|k| *k == Keys(Keys::JUMP)));
    }

    /// A small xorshift for the random runs.
    fn rng(seed: &mut u64) -> u64 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        *seed
    }

    #[test]
    fn turning_never_digs() {
        // Random walking (never both keys), a cursor jumping from side to
        // side, keys applying 0 to 3 ticks late: no read ever digs, and the
        // worm ends facing the cursor.
        for seed in 1..40u64 {
            let mut s = seed * 0x9e37_79b9;
            let plan: Vec<(u16, f32)> = (0..600)
                .map(|_| {
                    let k = [0, Keys::LEFT, Keys::RIGHT][(rng(&mut s) % 3) as usize];
                    let x = (rng(&mut s) % 200) as f32 - 100.0;
                    (k, x)
                })
                .collect();
            // Each choice held a few ticks.
            let hold = 1 + (seed % 4) as usize;
            let ahead = (seed % 4) as usize;
            // A switch from one side to the other rests a tick: the
            // player's own switch with no read between digs, as with keys.
            let side = |t: usize| {
                let (now, before) = (plan[t / hold].0, plan[t.saturating_sub(1) / hold].0);
                if now != 0 && before != 0 && now != before { 0 } else { now }
            };
            let player = |t: usize| Keys(side(t) | if t.is_multiple_of(7) { Keys::JUMP } else { 0 });
            let cursor = |t: usize| Some([plan[t / (hold + 1)].1, -30.0]);
            let r = run(&local(), ahead, 600, player, cursor);
            if let Some(t) = r.dug.iter().position(|d| *d) {
                for u in t.saturating_sub(6)..=t {
                    eprintln!(
                        "{u}: player {:?} cursor {:?} keys {:?} face {:?}",
                        player(u),
                        cursor(u),
                        r.keys[u],
                        r.worms[u].face
                    );
                }
                panic!("seed {seed}: a turn dug at {t}");
            }
            let mut end = local();
            end.face = r.worms[599].face;
            let still = run(&end, ahead, 30, |_| Keys(0), |_| Some([50.0, -30.0]));
            assert_eq!(still.worms.last().unwrap().face, Dir::Right);
        }
    }

    #[test]
    fn a_turn_waits_for_an_unread_press_of_the_other_key() {
        let mut c = MouseController::new();
        let mut w = local();
        w.latches.set(Keys(Keys::RIGHT | Keys::STRAFE)); // pressed, no read yet
        let k = c.keys(Some(&w), Some([-50.0, 0.0]), Keys(Keys::RIGHT));
        assert_eq!(k, Keys(Keys::RIGHT | Keys::STRAFE), "the turn waits");
        w.latches.reset();
        let k = c.keys(Some(&w), Some([-50.0, 0.0]), Keys(Keys::RIGHT));
        assert_eq!(k, Keys(Keys::LEFT), "then turns, Right released for the tick");
    }

    #[test]
    fn wheel_notches_press_one_each() {
        let mut c = MouseController::new();
        for n in [Notch::Next, Notch::Next, Notch::Previous] {
            c.notch(n);
        }
        let mut w = local();
        let mut clock = Clock::new();
        w.time = AbsTime(0);
        w.last_input_time = AbsTime(0);
        let (mut next, mut prev) = (0, 0);
        for _ in 0..20 {
            let mut seen = w.clone();
            seen.reads[0] = clock.reads(0);
            let k = c.keys(Some(&seen), None, Keys(0));
            let reads = clock.step();
            w.latches.set(k);
            // What a read sees (the latches' presses), counted once a read.
            if reads > 0 {
                next += w.latches.next_weapon.is_once() as u32;
                prev += w.latches.prev_weapon.is_once() as u32;
            }
            for _ in 0..reads {
                w.latches.reset();
            }
            w.time += w.frame;
        }
        assert_eq!((next, prev), (2, 1));
        assert_eq!(c.notches(), 0);
        // Firing without combos: the notch waits.
        let mut c = MouseController::new();
        c.notch(Notch::Next);
        let mut w = local();
        w.weapon_combos = false;
        assert_eq!(c.keys(Some(&w), None, Keys(Keys::FIRE)), Keys(Keys::FIRE));
        assert_eq!(c.keys(Some(&w), None, Keys(0)), Keys(Keys::NEXT_WEAPON));
        // Beyond the queue: dropped.
        for _ in 0..WHEEL_QUEUE + 3 {
            c.notch(Notch::Previous);
        }
        assert_eq!(c.notches(), WHEEL_QUEUE);
    }
}
