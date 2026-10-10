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
use crate::worm::{Loadout, Worm};

/// OLX `SPAWN_HOLESIZE` (`include/Consts.h:36`): the size-4 (largest) hole.
const SPAWN_HOLE: usize = 4;
/// OLX `MAX_WORMS` (`include/Consts.h:34`).
pub const MAX_WORMS: usize = 32;
/// `Game::FixedFrameTime`.
const FRAME_MS: u64 = 10;

#[derive(Clone, Debug, PartialEq)]
pub struct World {
    settings: SimSettings,
    terrain: Terrain,
    /// The theme's five hole stamps, smallest first.
    holes: Vec<Stamp>,
    worms: Vec<Worm>,
    latches: Vec<Latches>,
    /// `Game::simulationTime`.
    time: AbsTime,
    /// `CClient::fLastSimulationTime`: where the physics clock is.
    physics_time: AbsTime,
    /// Reused each physics step so that `tick` does not allocate (spec §5.3).
    others: Scratch,
}

/// The `(id, alive, pos)` of every worm, as the rope needs them. Only a buffer:
/// it holds no state, so it never makes two worlds unequal.
#[derive(Clone, Debug, Default)]
struct Scratch(Vec<(i32, bool, Vec2)>);

impl PartialEq for Scratch {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl World {
    /// `holes` are the theme's hole stamps (`hole1` to `hole5`). Like OLX, a
    /// missing hole carves nothing.
    pub fn new(settings: SimSettings, terrain: Terrain, holes: Vec<Stamp>, start: AbsTime) -> Self {
        Self {
            settings,
            terrain,
            holes,
            worms: Vec::new(),
            latches: Vec::new(),
            time: start,
            physics_time: start,
            others: Scratch::default(),
        }
    }

    pub fn worms(&self) -> &[Worm] {
        &self.worms
    }

    pub fn settings(&self) -> &SimSettings {
        &self.settings
    }

    pub fn terrain(&self) -> &Terrain {
        &self.terrain
    }

    /// Gives worm `i` its weapons and counters, before the match starts.
    /// Panics if there is no worm `i`.
    pub fn set_loadout(&mut self, i: usize, loadout: &Loadout) {
        let w = &mut self.worms[i];
        w.slots.clone_from(&loadout.slots);
        w.current_weapon = loadout.current_weapon;
        (w.lives, w.kills, w.deaths) = (loadout.lives, loadout.kills, loadout.deaths);
    }

    pub fn time(&self) -> AbsTime {
        self.time
    }

    /// `CWorm::Spawn` at `(x, y)`, including the spawn hole. Returns the worm's
    /// index, or `None` if the world already has [`MAX_WORMS`] worms.
    pub fn spawn_worm(&mut self, x: i32, y: i32) -> Option<usize> {
        if self.worms.len() >= MAX_WORMS {
            return None;
        }
        let id = self.worms.len() as i32;
        let pos = Vec2::new(x as f32, y as f32);
        self.worms.push(Worm::spawned(id, pos, self.time, &self.settings));
        self.latches.push(Latches::default());
        self.others.0 = Vec::with_capacity(self.worms.len());
        if let Some(hole) = self.holes.get(SPAWN_HOLE) {
            self.terrain.carve_hole(hole, pos.x as i32, pos.y as i32);
        }
        Some(self.worms.len() - 1)
    }

    /// Puts the physics clock at the frame clock, as OLX does when a match or
    /// an oracle scenario starts (`OracleHooks::loadInputs`, `simStep`).
    pub fn align_physics_clock(&mut self) {
        self.physics_time = self.time;
    }

    /// At least 1 ms, so a frame always ends even with settings outside
    /// OLX's range ([`SimSettings::validate`]).
    fn physics_dt(&self) -> TimeDiff {
        TimeDiff((1000 / self.settings.lx56_physics_fps.max(1)).max(1) as u64)
    }

    /// One 100 Hz frame. `keys[i]` is what worm `i` holds during this frame;
    /// a worm with no entry holds nothing.
    pub fn tick(&mut self, keys: &[Keys]) {
        self.gus_speed_round_trip();
        for (i, latch) in self.latches.iter_mut().enumerate() {
            latch.set(keys.get(i).copied().unwrap_or(Keys(0)));
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
            // Taken afresh for each worm: OLX's rope sees the worms already moved this step.
            let others = &mut self.others.0;
            others.clear();
            others.extend(self.worms.iter().map(|w| (w.id, w.alive, w.pos)));
            let mut ctx = Ctx {
                now: self.time,
                dt,
                terrain: &mut self.terrain,
                holes: &self.holes,
                settings: &self.settings,
                worms: others,
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
        w.worms[0].vel.y = f32::from_bits(0x40d3_3336);
        w.tick(&[Keys(0)]); // frame 0 runs no physics step
        assert_eq!(w.worms()[0].vel.y.to_bits(), 0x40d3_3335);
    }

    #[test]
    fn spawning_carves_the_largest_hole() {
        let mut w = world(Material::Dirt);
        w.spawn_worm(50, 50);
        assert_eq!(w.terrain().material(50, 50), Some(Material::Background));
        assert_eq!(w.terrain().material(43, 50), Some(Material::Background));
        assert_eq!(w.terrain().material(42, 50), Some(Material::Dirt));
        assert_eq!(w.worms()[0].dirt_count, 0, "spawning does not count as digging");
    }

    #[test]
    fn weapon_combos_allow_changing_weapons_while_firing() {
        // OLX getInput: `if(slots > 0 && (!bShoot || allowCombo))`, and the
        // LX56 preset has WeaponCombos on.
        let mut w = world(Material::Background);
        w.spawn_worm(100, 20);
        let slot = crate::Slot { weapon: 0, charge: 1.0, reloading: false, last_fire: 0.0 };
        let loadout =
            crate::Loadout { slots: vec![slot; 5], current_weapon: 0, lives: -2, kills: 0, deaths: 0 };
        w.set_loadout(0, &loadout);
        let held = Keys(Keys::CHANGE | Keys::FIRE);
        w.tick(&[held]);
        w.tick(&[held]);
        w.tick(&[Keys(held.0 | Keys::RIGHT)]); // frame 2: the first physics step reads the press
        assert_eq!(w.worms()[0].current_weapon, 1);
    }

    #[test]
    fn instant_air_jump_lets_a_falling_worm_jump() {
        let mut settings = SimSettings::lx56();
        settings.instant_air_jump = true;
        let terrain = Terrain::from_grid(&MaterialGrid::new(200, 200, Material::Background));
        let mut w = World::new(settings, terrain, holes(), AbsTime(0));
        w.spawn_worm(100, 20);
        w.tick(&[Keys(0)]);
        w.tick(&[Keys(0)]);
        w.tick(&[Keys(Keys::JUMP)]);
        assert!(w.worms()[0].vel.y < -60.0, "vy {}", w.worms()[0].vel.y);
    }

    #[test]
    fn a_theme_without_holes_does_not_panic() {
        // OLX's CarveHole returns 0 when the theme lacks the hole image.
        let terrain = Terrain::from_grid(&MaterialGrid::new(50, 50, Material::Dirt));
        let mut w = World::new(SimSettings::lx56(), terrain, Vec::new(), AbsTime(0));
        w.spawn_worm(25, 25);
        for _ in 0..20 {
            w.tick(&[Keys(Keys::RIGHT | Keys::LEFT)]);
        }
        assert_eq!(w.terrain().material(25, 25), Some(Material::Dirt));
    }

    /// Two worms digging, jumping and roping through dirt, varying every few frames.
    fn busy_keys(frame: usize) -> [Keys; 2] {
        const SEQ: [u8; 6] = [
            Keys::RIGHT,
            Keys::RIGHT | Keys::LEFT,
            Keys::ROPE | Keys::UP,
            Keys::JUMP | Keys::LEFT,
            Keys::DOWN | Keys::RIGHT,
            0,
        ];
        [Keys(SEQ[(frame / 7) % 6]), Keys(SEQ[(frame / 5 + 3) % 6])]
    }

    #[test]
    fn a_clone_evolves_identically() {
        let mut a = world(Material::Dirt);
        a.spawn_worm(60, 100);
        a.spawn_worm(140, 100);
        let start = a.clone();
        for f in 0..100 {
            a.tick(&busy_keys(f));
        }
        let mut b = a.clone();
        for f in 100..400 {
            a.tick(&busy_keys(f));
            b.tick(&busy_keys(f));
        }
        assert_eq!(a, b);
        assert_ne!(a.terrain().material_hash(), start.terrain().material_hash(), "the worms dug");
        assert_ne!(a.worms()[0].pos, start.worms()[0].pos, "the worms moved");
    }

    #[test]
    fn a_rope_press_without_permission_still_clears_the_rope() {
        // OLX CNinjaRope::Shoot clears the rope before it checks canUseNinja.
        let mut w = world(Material::Background);
        w.spawn_worm(100, 100);
        for k in [0, 0, Keys::ROPE] {
            w.tick(&[Keys(k)]);
        }
        assert!(w.worms()[0].rope.released);
        w.worms[0].can_use_ninja = false;
        w.tick(&[Keys(0)]);
        w.tick(&[Keys(Keys::ROPE)]);
        assert!(!w.worms()[0].rope.released);
    }

    #[test]
    fn missing_keys_mean_nothing_is_held() {
        let mut w = world(Material::Background);
        w.spawn_worm(50, 100);
        w.spawn_worm(150, 100);
        for _ in 0..5 {
            w.tick(&[Keys(Keys::RIGHT), Keys(Keys::RIGHT)]);
        }
        w.tick(&[Keys(Keys::RIGHT)]);
        w.tick(&[Keys(Keys::RIGHT)]);
        assert!(w.worms()[0].state.moving);
        assert!(!w.worms()[1].state.moving);
    }

    #[test]
    fn an_empty_map_does_not_panic() {
        let terrain = Terrain::from_grid(&MaterialGrid::new(0, 0, Material::Dirt));
        let mut w = World::new(SimSettings::lx56(), terrain, holes(), AbsTime(0));
        w.spawn_worm(0, 0);
        for k in [0, 0, Keys::ROPE, 0, Keys::RIGHT, Keys::LEFT | Keys::RIGHT, Keys::JUMP] {
            w.tick(&[Keys(k)]);
        }
    }

    #[test]
    fn at_most_32_worms() {
        // OLX MAX_WORMS (include/Consts.h:34).
        let mut w = world(Material::Background);
        for i in 0..32 {
            assert_eq!(w.spawn_worm(10 + i * 5, 50), Some(i as usize));
        }
        assert_eq!(w.spawn_worm(10, 80), None);
        assert_eq!(w.worms().len(), 32);
    }
}
