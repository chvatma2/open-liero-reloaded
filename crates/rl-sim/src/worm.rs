//! Worm state (OLX `CWorm`, `src/game/CWorm.h`), only what the simulation uses.

use alloc::vec::Vec;

use rl_math::{AbsTime, PI_F32, Vec2};

use crate::rope::NinjaRope;

/// OLX `DIR_TYPE`: left is 0, right is 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Left = 0,
    Right = 1,
}

/// OLX `worm_state_t`: what the worm does this sub-step, derived from input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WormState {
    pub shoot: bool,
    pub carve: bool,
    pub moving: bool,
    pub jump: bool,
}

impl WormState {
    /// `worm_state_t::asInt` (`src/common/CWorm.cpp:62-69`): shoot 1, carve 2, move 4, jump 8.
    pub fn as_int(&self) -> u8 {
        self.shoot as u8 | (self.carve as u8) << 1 | (self.moving as u8) << 2 | (self.jump as u8) << 3
    }
}

/// OLX `wpnslot_t`. Not simulated in M3: weapons arrive with M4.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub weapon: i32,
    pub charge: f32,
    pub reloading: bool,
    pub last_fire: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Worm {
    pub id: i32,
    pub alive: bool,
    pub pos: Vec2,
    pub vel: Vec2,
    pub health: f32,
    /// Aim angle in degrees: negative is up.
    pub angle: f32,
    pub angle_speed: f32,
    pub face: Dir,
    pub move_dir: Dir,
    pub on_ground: bool,
    pub state: WormState,
    pub current_weapon: i32,
    pub slots: Vec<Slot>,
    pub rope: NinjaRope,
    pub lives: i32,
    pub kills: i32,
    pub deaths: i32,
    pub dirt_count: i32,
    /// Animation frame (0..3), only for rendering.
    pub frame: f32,
    pub speed_factor: f32,
    pub can_use_ninja: bool,
    pub can_air_jump: bool,
    pub last_input_time: AbsTime,
    pub last_air_jump_time: AbsTime,
}

impl Worm {
    /// A worm as `CWorm::Spawn` leaves it (`src/common/CWorm.cpp:591-650`),
    /// except the carving, which [`crate::World::spawn_worm`] does.
    pub fn spawned(id: i32, pos: Vec2, now: AbsTime, speed_factor: f32) -> Self {
        Self {
            id,
            alive: true,
            pos,
            vel: Vec2::ZERO,
            health: 100.0,
            angle: 0.0,
            angle_speed: 0.0,
            face: Dir::Right,
            move_dir: Dir::Right,
            on_ground: false,
            state: WormState::default(),
            current_weapon: 0,
            slots: Vec::new(),
            rope: NinjaRope::default(),
            lives: -2,
            kills: 0,
            deaths: 0,
            dirt_count: 0,
            frame: 0.0,
            speed_factor,
            can_use_ninja: true,
            can_air_jump: false,
            last_input_time: now,
            last_air_jump_time: AbsTime(0),
        }
    }

    fn direction(&self, side: Dir) -> Vec2 {
        let rad = self.angle * (PI_F32 / 180.0);
        let sx = if side == Dir::Left { -1.0 } else { 1.0 };
        Vec2::new(libm::cosf(rad) * sx, libm::sinf(rad))
    }

    /// `CWorm::getFaceDirection` (`src/game/CWorm.h:514-516`).
    pub fn face_direction(&self) -> Vec2 {
        self.direction(self.face)
    }

    /// `CWorm::getMoveDirection` (`src/game/CWorm.h:518-520`).
    pub fn move_direction(&self) -> Vec2 {
        self.direction(self.move_dir)
    }
}
