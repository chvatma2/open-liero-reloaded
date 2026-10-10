//! The motor (spec §20 F3, notes §4.4): OLX's bot writes the worm directly
//! (`fAngle`, the facing side, the weapon, `bMove`/`bJump`/…); a bot here may
//! only press the 8 keys a player has, so the motor turns what the bot wants
//! ([`Intent`]) into [`Keys`] under `rl-sim`'s input rules
//! (`crates/rl-sim/src/physics.rs` `get_input`, `input.rs` latches).
//!
//! | Intent | Keys |
//! |---|---|
//! | aim angle | UP/DOWN through a speed controller, about 100°/s |
//! | face a side | that side's key, held until the worm faces it (a step of walking) |
//! | walk | hold LEFT or RIGHT |
//! | carve | while walking, press the other side's key (one tick) |
//! | jump | a JUMP press, released the next tick; never while the rope is out |
//! | weapon slot | hold CHANGE, press RIGHT or LEFT (the shorter way round) every other tick |
//! | fire | hold FIRE (not while switching weapons without `WeaponCombos`) |
//! | rope shoot | a ROPE press (fires along the aim the worm has) |
//! | rope release | a JUMP press while the rope is out |
//! | respawn | JUMP presses while dead |
//!
//! A press is a key up in the last tick and down in this one; the
//! simulation's latches keep it until the next input read, so a frame that
//! runs no physics step loses none.

use rl_sim::{Dir, Keys, SimSettings, Worm};

/// LX56 aim (`physics.rs`): acceleration while a key is held, the speed a
/// release clamps to, and the slow-down after it.
const AIM_ACCEL: f32 = 500.0;
const AIM_MAX_SPEED: f32 = 100.0;
/// The braking the controller plans with (a held opposite key gives 500).
const AIM_BRAKE: f32 = 400.0;
/// Half the speed change of one held 11 ms step.
const AIM_DEAD_BAND: f32 = AIM_ACCEL * 0.011 / 2.0;
/// Close enough to stop turning.
const AIM_DONE: f32 = 0.5;

/// What a bot wants to do this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Intent {
    /// The worm angle to turn to, or `None` to stop turning.
    pub aim: Option<f32>,
    /// The side to face (walking faces its own side).
    pub face: Option<Dir>,
    /// Walk to this side (`bMove`, with OLX's `iMoveDirectionSide`).
    pub walk: Option<Dir>,
    pub jump: bool,
    pub fire: bool,
    /// Dig ahead (`bCarve`): only while walking.
    pub carve: bool,
    pub rope_shoot: bool,
    pub rope_release: bool,
    /// The weapon slot to hold.
    pub weapon: Option<usize>,
}

/// The motor's memory: the keys of the last tick, for press edges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Motor {
    prev: Keys,
}

impl Motor {
    pub fn new() -> Self {
        Motor::default()
    }

    /// The keys of the last tick.
    pub fn last(&self) -> Keys {
        self.prev
    }

    fn pressed(&self, bit: u8, k: &mut u8) {
        if self.prev.0 & bit == 0 {
            *k |= bit;
        }
    }

    /// The keys for worm `w` (as the world shows it before this tick) to do
    /// `intent`.
    pub fn keys(&mut self, w: &Worm, s: &SimSettings, intent: &Intent) -> Keys {
        let mut k = 0u8;
        if !w.alive {
            // `CWormHuman.cpp:115-119`: the jump key asks for the respawn.
            self.pressed(Keys::JUMP, &mut k);
            return self.done(k);
        }
        let slots = w.slots.len();
        let switch = intent
            .weapon
            .filter(|&want| want < slots && want as i32 != w.current_weapon)
            .map(|want| shorter_way(w.current_weapon, want as i32, slots as i32));
        if let Some(side) = switch {
            // One slot per arrow press while CHANGE is held, which stops
            // walking (`physics.rs`: `!keys.change.is_down()`).
            k |= Keys::CHANGE;
            if self.prev.0 & (Keys::LEFT | Keys::RIGHT) == 0 {
                k |= side_key(side);
            }
        } else if let Some(side) = intent.walk {
            k |= side_key(side);
            if intent.carve {
                // Holding one side and pressing the other carves
                // (`physics.rs`: `if right_once { carve }`).
                self.pressed(side_key(opposite(side)), &mut k);
            }
        } else if let Some(side) = intent.face.filter(|&f| f != w.face) {
            k |= side_key(side);
        }
        if let Some(target) = intent.aim {
            k |= aim_key(w.angle, w.angle_speed, target);
        }
        if intent.fire && (switch.is_none() || s.weapon_combos) {
            k |= Keys::FIRE;
        }
        if w.rope.released {
            // A jump releases the rope (`physics.rs`), so it only jumps
            // when the bot lets go.
            if intent.rope_release {
                self.pressed(Keys::JUMP, &mut k);
            }
        } else if intent.jump {
            self.pressed(Keys::JUMP, &mut k);
        }
        if intent.rope_shoot {
            self.pressed(Keys::ROPE, &mut k);
        }
        self.done(k)
    }

    fn done(&mut self, k: u8) -> Keys {
        self.prev = Keys(k);
        self.prev
    }
}

fn side_key(d: Dir) -> u8 {
    match d {
        Dir::Left => Keys::LEFT,
        Dir::Right => Keys::RIGHT,
    }
}

fn opposite(d: Dir) -> Dir {
    match d {
        Dir::Left => Dir::Right,
        Dir::Right => Dir::Left,
    }
}

/// RIGHT goes up a slot and LEFT down, both wrapping: the side with fewer
/// presses (RIGHT on a tie).
fn shorter_way(from: i32, to: i32, slots: i32) -> Dir {
    if (to - from).rem_euclid(slots) <= (from - to).rem_euclid(slots) { Dir::Right } else { Dir::Left }
}

/// The aim controller: the angle speed the worm should have is
/// `±min(100, √(2·400·|error|))` (OLX's bots turn at `BotAngleSpeed`,
/// 100°/s, `CWormBot.cpp:1646`), and the key held pushes the speed towards
/// it at 500°/s² (UP lowers the speed, DOWN raises it). With no key held
/// the simulation clamps the speed to ±100 and slows it by 200°/s².
pub fn aim_key(angle: f32, speed: f32, target: f32) -> u8 {
    let e = target - angle;
    if !e.is_finite() || !speed.is_finite() {
        return 0;
    }
    if libm::fabsf(e) < AIM_DONE && libm::fabsf(speed) < 5.0 {
        return 0;
    }
    let want = libm::copysignf(AIM_MAX_SPEED.min(libm::sqrtf(2.0 * AIM_BRAKE * libm::fabsf(e))), e);
    if speed < want - AIM_DEAD_BAND {
        Keys::DOWN
    } else if speed > want + AIM_DEAD_BAND {
        Keys::UP
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_math::{AbsTime, Vec2};
    use rl_sim::Slot;

    fn worm() -> Worm {
        let s = SimSettings::lx56();
        let mut w = Worm::spawned(0, Vec2::new(50.0, 50.0), AbsTime(100_000), &s);
        w.slots = alloc::vec![Slot::ready(0); 5];
        w
    }

    #[test]
    fn intents_become_the_keys_of_the_table() {
        let s = SimSettings::lx56();
        let mut w = worm();
        let mut m = Motor::new();
        let walk_carve = Intent { walk: Some(Dir::Right), carve: true, ..Intent::default() };
        assert_eq!(m.keys(&w, &s, &walk_carve), Keys(Keys::RIGHT | Keys::LEFT));
        assert_eq!(m.keys(&w, &s, &walk_carve), Keys(Keys::RIGHT), "the carve key is pressed, not held");
        let jump = Intent { jump: true, rope_shoot: true, ..Intent::default() };
        assert_eq!(m.keys(&w, &s, &jump), Keys(Keys::JUMP | Keys::ROPE));
        assert_eq!(m.keys(&w, &s, &jump), Keys(0));
        assert_eq!(m.keys(&w, &s, &jump), Keys(Keys::JUMP | Keys::ROPE));
        w.rope.released = true;
        assert_eq!(m.keys(&w, &s, &jump), Keys(0), "no jump while the rope is out");
        let release = Intent { rope_release: true, ..Intent::default() };
        assert_eq!(m.keys(&w, &s, &release), Keys(Keys::JUMP));
        w.rope.released = false;
        w.face = Dir::Right;
        let face = Intent { face: Some(Dir::Left), ..Intent::default() };
        assert_eq!(m.keys(&w, &s, &face), Keys(Keys::LEFT));
        w.face = Dir::Left;
        assert_eq!(m.keys(&w, &s, &face), Keys(0));
        // Slot 0 to 4: one LEFT press with CHANGE; firing waits unless combos.
        let switch = Intent { weapon: Some(4), fire: true, walk: Some(Dir::Right), ..Intent::default() };
        let no_combos = SimSettings { weapon_combos: false, ..SimSettings::lx56() };
        assert_eq!(m.keys(&w, &no_combos, &switch), Keys(Keys::CHANGE | Keys::LEFT));
        assert_eq!(m.keys(&w, &s, &switch), Keys(Keys::CHANGE | Keys::FIRE));
        w.alive = false;
        assert_eq!(m.keys(&w, &s, &Intent::default()), Keys(Keys::JUMP));
        assert_eq!(m.keys(&w, &s, &Intent::default()), Keys(0));
    }

    #[test]
    fn the_aim_controller_brakes_before_the_target() {
        assert_eq!(aim_key(0.0, 0.0, -30.0), Keys::UP);
        assert_eq!(aim_key(0.0, 0.0, 30.0), Keys::DOWN);
        assert_eq!(aim_key(0.0, -100.0, -60.0), 0, "cruising at 100°/s");
        assert_eq!(aim_key(-58.0, -100.0, -60.0), Keys::DOWN, "braking");
        assert_eq!(aim_key(-60.2, 2.0, -60.0), 0, "there");
        assert_eq!(aim_key(f32::NAN, 0.0, 1.0), 0);
    }
}
