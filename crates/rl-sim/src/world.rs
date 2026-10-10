//! The world and its two clocks (OLX `Game::frameInner`, `src/game/Game.cpp:897-959`,
//! and `CClient::Simulation`, `src/client/CClient_Game.cpp:60-222`).
//!
//! Each [`World::tick`] is one 100 Hz frame: it latches the frame's keys, runs
//! as many LX56 physics sub-steps (`1000 / LX56PhysicsFPS` whole milliseconds,
//! 11 ms by default) as fit up to the current frame time, then advances the
//! frame clock by 10 ms. Some frames run no sub-step at all.

use alloc::sync::Arc;
use alloc::vec::Vec;

use rl_math::{AbsTime, GlibcRand, Mt19937, TimeDiff, Vec2};
use rl_terrain::{GreenMask, Stamp, Terrain};

use crate::SimSettings;
use crate::beam::BeamCtx;
use crate::damage::{Harm, die_at_zero_health};
use crate::input::{Keys, Latches};
use crate::mod_rt::SimMod;
use crate::physics::{Ctx, simulate_worm};
use crate::proj_sim::{ProjCtx, simulate_projectiles};
use crate::projectile::{Projectile, ProjectilePool};
use crate::respawn::{SPAWN_HOLE, Spawner, simulate_respawns};
use crate::rng::Draws;
use crate::shots::{MAX_SHOOTINGS, ShootList, Shot, ShotCtx, encode_decode, process_shot, spawn_time};
use crate::weapons::{ShootCtx, laser_sight, player_shoot, worm_shoot};
use crate::worm::{Loadout, Worm};

/// OLX `MAX_WORMS` (`include/Consts.h:34`).
pub const MAX_WORMS: usize = 32;
/// `Game::FixedFrameTime`.
const FRAME_MS: u64 = 10;
/// The oracle pins `tLX->currentTime` to `simulationTime + K` (spec §18 D2).
pub const CLOCK_OFFSET_MS: u64 = 10;

#[derive(Clone, Debug, PartialEq)]
pub struct World {
    settings: SimSettings,
    sim_mod: Arc<SimMod>,
    terrain: Terrain,
    /// The theme's five hole stamps, smallest first.
    holes: Vec<Stamp>,
    /// The green-dirt mask (`bmpGreenMask`), if the theme has one.
    green_mask: Option<GreenMask>,
    worms: Vec<Worm>,
    latches: Vec<Latches>,
    /// `Game::simulationTime`.
    time: AbsTime,
    /// `CClient::fLastSimulationTime`: where the physics clock is.
    physics_time: AbsTime,
    /// `game.serverFrame`: 0 at the start, +1 at the start of each frame.
    server_frame: u64,
    /// The server's shot list for the local client.
    shots: ShootList,
    /// The shots delivered right before the last frame.
    last_shots: Vec<Shot>,
    /// The client's projectiles (`cProjectiles`).
    projectiles: ProjectilePool,
    /// OLX's `rand()` and `FindSpot` generators and the entity counter.
    draws: Draws,
    /// Reused each physics step so that `tick` does not allocate (spec §5.3).
    others: Scratch,
}

/// Buffers only: they hold no state, so they never make two worlds unequal.
#[derive(Clone, Debug, Default)]
struct Scratch {
    /// The `(id, alive, pos)` of every worm, as the rope needs them.
    others: Vec<(i32, bool, Vec2)>,
    /// A beam trace's `goodWidthParts`, as wide as the widest beam.
    beam_parts: Vec<bool>,
}

impl PartialEq for Scratch {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl World {
    /// `holes` are the theme's hole stamps (`hole1` to `hole5`). Like OLX, a
    /// missing hole carves nothing. `seed` is the match seed (`simSetSeed`):
    /// it seeds both the `rand()` stream and the `FindSpot` `mt19937`.
    /// Weapon and projectile ids are indices into `sim_mod`.
    pub fn new(
        settings: SimSettings,
        sim_mod: Arc<SimMod>,
        terrain: Terrain,
        holes: Vec<Stamp>,
        start: AbsTime,
        seed: u32,
    ) -> Self {
        let terrain_min = terrain.width().min(terrain.height());
        Self {
            settings,
            sim_mod,
            terrain,
            holes,
            green_mask: None,
            worms: Vec::new(),
            latches: Vec::new(),
            time: start,
            physics_time: start,
            server_frame: 0,
            shots: ShootList::new(),
            last_shots: Vec::with_capacity(MAX_SHOOTINGS),
            projectiles: ProjectilePool::new(ProjectilePool::DEFAULT_CAPACITY),
            draws: Draws::new(GlibcRand::new(seed), Mt19937::new(seed)),
            others: Scratch { others: Vec::new(), beam_parts: Vec::with_capacity(terrain_min as usize) },
        }
    }

    /// Sets the green-dirt mask (`data/gfx/greenball.png`) before the match
    /// starts. Like OLX, green dirt places nothing without it.
    pub fn set_green_mask(&mut self, mask: GreenMask) {
        self.green_mask = Some(mask);
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

    pub fn sim_mod(&self) -> &SimMod {
        &self.sim_mod
    }

    /// Used projectile slots in slot order.
    pub fn projectiles(&self) -> impl Iterator<Item = (usize, &Projectile)> {
        self.projectiles.iter()
    }

    /// The shots delivered and processed right before the last frame (Model R),
    /// with their spawn times and their random seeds as spawning left them.
    pub fn last_shots(&self) -> &[Shot] {
        &self.last_shots
    }

    /// `game.serverFrame`.
    pub fn server_frame(&self) -> u64 {
        self.server_frame
    }

    /// Sets worm `i`'s health before the match starts (the oracle's
    /// `simSetHealth`). Panics if there is no worm `i`.
    pub fn set_health(&mut self, i: usize, health: f32) {
        self.worms[i].health = health;
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

    /// How many `rand()` calls the simulation has made since the seed was set.
    pub fn rand_calls(&self) -> u64 {
        self.draws.rand.calls()
    }

    /// How many `FindSpot` `rnd()` draws the simulation has made.
    pub fn mt_calls(&self) -> u64 {
        self.draws.mt.calls()
    }

    /// The dedicated server's entity pool fill (`tEntities`, never freed).
    pub fn entity_count(&self) -> u32 {
        self.draws.ents
    }

    /// What `CClient::InjureWorm` and `GameServer::killWorm` need, at the frame clock.
    pub(crate) fn harm(&mut self) -> Harm<'_> {
        Harm { settings: &self.settings, worms: &mut self.worms, draws: &mut self.draws, now: self.time }
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
        self.others.others = Vec::with_capacity(self.worms.len());
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

    /// How far the frame clock moves in one frame: 10 ms, scaled by
    /// `1 / GameSpeed` unless `GameSpeedOnlyForProjs`, and at least 1 ms.
    pub fn frame_dt(&self) -> TimeDiff {
        let mut frame = TimeDiff(FRAME_MS);
        if !self.settings.game_speed_only_for_projs {
            frame = frame.scaled(1.0 / self.game_speed());
            if frame.0 == 0 {
                frame = TimeDiff(1);
            }
        }
        frame
    }

    fn game_speed(&self) -> f32 {
        self.settings.game_speed.clamp(0.05, 10.0)
    }

    /// One 100 Hz frame. `keys[i]` is what worm `i` holds during this frame;
    /// a worm with no entry holds nothing.
    pub fn tick(&mut self, keys: &[Keys]) {
        // What OLX does between the previous frame and this one (spec §18 D1, D10).
        self.deliver_shots();
        self.server_frame += 1;
        self.gus_speed_round_trip();
        // `gusLogicFrame` (`Game.cpp:945-949`): worms left at exactly 0 health die.
        let wall_clock = self.time + TimeDiff(CLOCK_OFFSET_MS);
        die_at_zero_health(&mut self.harm(), wall_clock);
        for (i, latch) in self.latches.iter_mut().enumerate() {
            latch.set(keys.get(i).copied().unwrap_or(Keys(0)));
        }
        let step = self.physics_dt();
        let mut step_clock = step;
        if !self.settings.game_speed_only_for_projs {
            step_clock = step.scaled(1.0 / self.game_speed());
            if step_clock.0 == 0 {
                step_clock = TimeDiff(1);
            }
        }
        let frame = self.frame_dt();
        while self.physics_time + step_clock <= self.time {
            self.physics_step(step.seconds());
            self.physics_time += step_clock;
        }
        // `GameServer::SimulateGame` (`src/server/CServer_Game.cpp:167-261`).
        let mut spawner = Spawner {
            settings: &self.settings,
            terrain: &mut self.terrain,
            holes: &self.holes,
            mt: &mut self.draws.mt,
            now: self.time,
            wall_clock,
        };
        simulate_respawns(&mut self.worms, &mut self.latches, &mut spawner);
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

    /// Model R (spec §18 D1, `OracleHooks::deliverShots`): between frames,
    /// once the frame clock has advanced, the shot list started before the
    /// last frame goes through OLX's encode/decode round trip, and each
    /// decoded shot is processed (`CClient::ProcessServerShotList`,
    /// `src/client/CClient_Game.cpp:813-824`) with the clock pinned to
    /// `simulationTime + K`. It runs at the start of [`World::tick`], before
    /// `serverFrame` advances, which is the same moment.
    fn deliver_shots(&mut self) {
        self.last_shots.clear();
        let Some(list) = self.shots.take_due() else { return };
        let decoded = encode_decode(list, self.sim_mod.weapons().len() as i32 - 1);
        list.clear();
        let current = self.time + TimeDiff(CLOCK_OFFSET_MS);
        let mut ctx = ShotCtx {
            sim_mod: &self.sim_mod,
            settings: &self.settings,
            pool: &mut self.projectiles,
            terrain: &mut self.terrain,
            holes: &self.holes,
            draws: &mut self.draws,
            parts: &mut self.others.beam_parts,
            now: self.time,
        };
        for mut shot in decoded {
            let spawn = spawn_time(shot.time, self.server_frame, current);
            shot.spawn = spawn;
            process_shot(&mut shot, spawn, &mut self.worms, &mut ctx);
            if self.last_shots.len() < MAX_SHOOTINGS {
                self.last_shots.push(shot);
            }
        }
    }

    /// One LX56 sub-step of `CClient::Simulation` (`src/client/CClient_Game.cpp:97-221`).
    fn physics_step(&mut self, dt: f32) {
        for i in 0..self.worms.len() {
            // Taken afresh for each worm: OLX's rope sees the worms already moved this step.
            let others = &mut self.others.others;
            others.clear();
            others.extend(self.worms.iter().map(|w| (w.id, w.alive, w.pos)));
            let mut ctx = Ctx {
                now: self.time,
                dt,
                terrain: &mut self.terrain,
                holes: &self.holes,
                settings: &self.settings,
                sim_mod: &self.sim_mod,
                worms: others,
                draws: &mut self.draws,
            };
            simulate_worm(&mut self.worms[i], &mut self.latches[i], &mut ctx);
            // `CClient_Game.cpp:155-176`: the server's fire code, then the client's.
            let w = &mut self.worms[i];
            if w.state.shoot && w.alive {
                let mut ctx = ShootCtx {
                    sim_mod: &self.sim_mod,
                    settings: &self.settings,
                    list: &mut self.shots,
                    rand: &mut self.draws.rand,
                    server_time: TimeDiff(self.server_frame.wrapping_mul(FRAME_MS)),
                };
                worm_shoot(w, &mut ctx);
            }
            if w.alive {
                if w.state.shoot {
                    let mut ctx = BeamCtx {
                        terrain: &mut self.terrain,
                        holes: &self.holes,
                        draws: &mut self.draws,
                        parts: &mut self.others.beam_parts,
                    };
                    player_shoot(&mut self.worms, i, &self.sim_mod, &mut ctx);
                }
                laser_sight(&self.worms[i], &self.sim_mod, &mut self.draws);
            }
        }
        // `CClient_Game.cpp:213-215`: projectiles after all worms.
        let physics_dt = self.physics_dt();
        let mut ctx = ProjCtx {
            sim_mod: &self.sim_mod,
            settings: &self.settings,
            terrain: &mut self.terrain,
            worms: &mut self.worms,
            draws: &mut self.draws,
            holes: &self.holes,
            green_mask: self.green_mask.as_ref(),
            now: self.time,
            wall_clock: self.time + TimeDiff(CLOCK_OFFSET_MS),
            physics_dt,
        };
        simulate_projectiles(&mut self.projectiles, &mut ctx);
    }
}

#[cfg(test)]
#[path = "world_tests.rs"]
mod tests;
