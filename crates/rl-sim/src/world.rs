//! The world and its two clocks (OLX `Game::frameInner`, `src/game/Game.cpp:897-959`,
//! and `CClient::Simulation`, `src/client/CClient_Game.cpp:60-222`).
//!
//! Each [`World::tick`] is one 100 Hz frame: it latches the frame's keys, runs
//! as many LX56 physics sub-steps (`1000 / LX56PhysicsFPS` whole milliseconds,
//! 11 ms by default) as fit up to the current frame time, then advances the
//! frame clock by 10 ms. Some frames run no sub-step at all.

use alloc::vec::Vec;

use rl_math::{AbsTime, TimeDiff, Vec2};
use rl_terrain::{Stamp, Terrain};

use crate::SimSettings;
use crate::input::{Keys, Latches};
use crate::physics::{Ctx, simulate_worm};
use crate::worm::Worm;

/// OLX `SPAWN_HOLESIZE` (`include/Consts.h:36`): the size-4 (largest) hole.
const SPAWN_HOLE: usize = 4;
/// `Game::FixedFrameTime`.
const FRAME_MS: u64 = 10;

#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub settings: SimSettings,
    pub terrain: Terrain,
    /// The theme's five hole stamps, smallest first.
    holes: Vec<Stamp>,
    worms: Vec<Worm>,
    latches: Vec<Latches>,
    /// `Game::simulationTime`.
    time: AbsTime,
    /// `CClient::fLastSimulationTime`: where the physics clock is.
    physics_time: AbsTime,
}

impl World {
    /// `holes` must be the theme's five hole stamps (`hole1` to `hole5`).
    pub fn new(settings: SimSettings, terrain: Terrain, holes: Vec<Stamp>, start: AbsTime) -> Self {
        assert_eq!(holes.len(), 5, "a theme has five hole stamps");
        Self {
            settings,
            terrain,
            holes,
            worms: Vec::new(),
            latches: Vec::new(),
            time: start,
            physics_time: start,
        }
    }

    pub fn worms(&self) -> &[Worm] {
        &self.worms
    }

    pub fn worms_mut(&mut self) -> &mut [Worm] {
        &mut self.worms
    }

    pub fn time(&self) -> AbsTime {
        self.time
    }

    /// `CWorm::Spawn` at `(x, y)`, including the spawn hole. Returns the worm's index.
    pub fn spawn_worm(&mut self, x: i32, y: i32) -> usize {
        let id = self.worms.len() as i32;
        let pos = Vec2::new(x as f32, y as f32);
        self.worms.push(Worm::spawned(id, pos, self.time, self.settings.worm_speed_factor));
        self.latches.push(Latches::default());
        self.terrain.carve_hole(&self.holes[SPAWN_HOLE], pos.x as i32, pos.y as i32);
        self.worms.len() - 1
    }

    /// Puts the physics clock at the frame clock, as OLX does when a match or
    /// an oracle scenario starts (`OracleHooks::loadInputs`, `simStep`).
    pub fn align_physics_clock(&mut self) {
        self.physics_time = self.time;
    }

    fn physics_dt(&self) -> TimeDiff {
        TimeDiff((1000 / self.settings.lx56_physics_fps.max(1)) as u64)
    }

    /// One 100 Hz frame. `keys[i]` is what worm `i` holds during this frame.
    pub fn tick(&mut self, keys: &[Keys]) {
        self.gus_speed_round_trip();
        for (latch, k) in self.latches.iter_mut().zip(keys) {
            latch.set(*k);
        }
        let speed = self.settings.game_speed.clamp(0.05, 10.0);
        let step = self.physics_dt();
        let mut step_clock = step;
        let mut frame = TimeDiff(FRAME_MS);
        if !self.settings.game_speed_only_for_projs {
            step_clock = step.scaled(1.0 / speed);
            if step_clock.0 == 0 {
                step_clock = TimeDiff(1);
            }
            frame = frame.scaled(1.0 / speed);
            if frame.0 == 0 {
                frame = TimeDiff(1);
            }
        }
        while self.physics_time + step_clock <= self.time {
            self.physics_step(step.seconds());
            self.physics_time += step_clock;
        }
        self.time += frame;
    }

    /// OLX runs `gusLogicFrame()` every frame, even for classic mods, inside a
    /// `GusSpeedScope` (`src/game/Game.cpp:682-691`, `945-949`) that converts
    /// every worm's and rope's velocity to Gusanos units and back
    /// (`CGameObject::ScopedGusCompatibleSpeed`, `src/common/CGameObject.cpp:105-127`):
    /// `v *= GusDT.seconds()` then `v *= 1.0f / GusDT.seconds()`, with `GusDT` = 10 ms.
    /// The round trip is lossy, and the oracle shows its effect, so it is kept.
    fn gus_speed_round_trip(&mut self) {
        let to_gus = TimeDiff(FRAME_MS).seconds();
        let from_gus = 1.0 / to_gus;
        for w in &mut self.worms {
            w.vel *= to_gus;
            w.vel *= from_gus;
            w.rope.vel *= to_gus;
            w.rope.vel *= from_gus;
        }
    }

    fn physics_step(&mut self, dt: f32) {
        for i in 0..self.worms.len() {
            let others: Vec<(i32, bool, Vec2)> = self.worms.iter().map(|w| (w.id, w.alive, w.pos)).collect();
            let mut ctx = Ctx {
                now: self.time,
                dt,
                terrain: &mut self.terrain,
                holes: &self.holes,
                settings: &self.settings,
                worms: &others,
            };
            simulate_worm(&mut self.worms[i], &mut self.latches[i], &mut ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use rl_data::image::{Rgba, RgbaImage};
    use rl_data::level::{Material, MaterialGrid};

    fn holes() -> Vec<Stamp> {
        // Same sizes as the Dirt theme's hole1..hole5.
        [5, 7, 9, 11, 15]
            .into_iter()
            .map(|size| {
                let mut img = RgbaImage::new(size, size);
                for y in 0..size {
                    for x in 0..size {
                        img.set(x, y, Rgba::MAGENTA);
                    }
                }
                Stamp::from_image(&img)
            })
            .collect()
    }

    fn world(fill: Material) -> World {
        let terrain = Terrain::from_grid(&MaterialGrid::new(200, 200, fill));
        World::new(SimSettings::lx56(), terrain, holes(), AbsTime(0))
    }

    #[test]
    fn physics_steps_follow_the_two_clocks() {
        // 11 ms physics steps against 10 ms frames: none in frames 0 and 1,
        // one per frame from frame 2, none again in frame 12 (OLX trace `fall`).
        let mut w = world(Material::Background);
        w.spawn_worm(100, 20);
        let mut stepped = vec![];
        for _ in 0..13 {
            let before = w.worms()[0].vel.y;
            w.tick(&[Keys(0)]);
            stepped.push(w.worms()[0].vel.y - before > 0.5);
        }
        let expected = [false, false, true, true, true, true, true, true, true, true, true, true, false];
        assert_eq!(stepped, expected);
    }

    #[test]
    fn gus_speed_round_trip_matches_olx() {
        // From the oracle: OLX turns 6.60000134 into 6.60000086 between frames 7 and 8 of `fall`.
        let mut w = world(Material::Background);
        w.spawn_worm(100, 20);
        w.worms_mut()[0].vel.y = f32::from_bits(0x40d3_3336);
        w.tick(&[Keys(0)]); // frame 0 runs no physics step
        assert_eq!(w.worms()[0].vel.y.to_bits(), 0x40d3_3335);
    }

    #[test]
    fn spawning_carves_the_largest_hole() {
        let mut w = world(Material::Dirt);
        w.spawn_worm(50, 50);
        assert_eq!(w.terrain.material(50, 50), Some(Material::Background));
        assert_eq!(w.terrain.material(43, 50), Some(Material::Background));
        assert_eq!(w.terrain.material(42, 50), Some(Material::Dirt));
        assert_eq!(w.worms()[0].dirt_count, 0, "spawning does not count as digging");
    }

    #[test]
    fn a_clone_evolves_identically() {
        let mut a = world(Material::Background);
        a.spawn_worm(100, 20);
        let mut b = a.clone();
        for k in [Keys(Keys::RIGHT), Keys(Keys::JUMP), Keys(0), Keys(Keys::ROPE | Keys::UP)] {
            a.tick(&[k]);
            b.tick(&[k]);
        }
        assert_eq!(a, b);
    }
}
