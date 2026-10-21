//! The live preview's worm (issue #14): it walks while its aim sweeps up
//! and down, turning round after each sweep, so every frame shows facing
//! both ways.

/// Degrees the aim sweeps a second, and the walk frames a second (OLX's
/// player menu: `src/client/DeprecatedGUI/Menu_Player.cpp:746, :756-774`).
const AIM_SPEED: f32 = 200.0;
const WALK_SPEED: f32 = 7.5;
/// The aim's ends, degrees (−90 straight up; the game's range).
const UP: f32 = -90.0;
const DOWN: f32 = 60.0;

/// The preview worm's pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anim {
    /// Degrees, −90 up to 60 down.
    angle: f32,
    /// The walk frame, 0 to 3.
    walk: f32,
    /// The aim goes down.
    down: bool,
    /// Facing left.
    pub left: bool,
}

impl Default for Anim {
    fn default() -> Self {
        Anim { angle: 0.0, walk: 0.0, down: true, left: false }
    }
}

impl Anim {
    /// Advances by `dt` seconds (at most a quarter second). The worm turns
    /// round each time its aim comes back up.
    pub fn step(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.clamp(0.0, 0.25) } else { 0.0 };
        self.angle += if self.down { dt * AIM_SPEED } else { -dt * AIM_SPEED };
        if self.angle >= DOWN {
            self.angle = DOWN;
            self.down = false;
        } else if self.angle <= UP {
            self.angle = UP;
            self.down = true;
            self.left = !self.left;
        }
        self.walk = (self.walk + dt * WALK_SPEED) % 3.0;
    }

    /// The sheet frame shown (the game's choice for the aim and walk).
    ///
    /// OLX: src/common/CWorm.cpp:1225-1244
    pub fn frame(&self) -> usize {
        let aim = (((self.angle + 90.0) / 151.0 * 7.0) as i32).clamp(0, 6);
        (self.walk as i32).clamp(0, 2) as usize * 7 + aim as usize
    }
}
