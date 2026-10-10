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
use crate::bonus::{Bonus, BonusItem, Bonuses, ClientCtx, MAX_BONUSES};
use crate::buf::Buf;
use crate::damage::{Harm, die_at_zero_health};
use crate::effects::carve_px;
use crate::events::{CosmeticKind, EventKind, TerrainOp, TickEvents};
use crate::input::{Keys, Latches};
use crate::mod_rt::SimMod;
use crate::physics::{Ctx, simulate_worm};
use crate::proj_sim::{ProjCtx, simulate_projectiles};
use crate::projectile::{Projectile, ProjectilePool, c_int};
use crate::respawn::{SPAWN_HOLE, Spawner, find_spot, simulate_respawns};
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
    pub(crate) settings: SimSettings,
    pub(crate) sim_mod: Arc<SimMod>,
    pub(crate) terrain: Terrain,
    /// The theme's five hole stamps, smallest first.
    pub(crate) holes: Vec<Stamp>,
    /// The green-dirt mask (`bmpGreenMask`), if the theme has one.
    pub(crate) green_mask: Option<GreenMask>,
    pub(crate) worms: Vec<Worm>,
    pub(crate) latches: Vec<Latches>,
    /// `Game::simulationTime`.
    pub(crate) time: AbsTime,
    /// `CClient::fLastSimulationTime`: where the physics clock is.
    pub(crate) physics_time: AbsTime,
    /// `game.serverFrame`: 0 at the start, +1 at the start of each frame.
    pub(crate) server_frame: u64,
    /// The physics sub-steps the last tick ran.
    pub(crate) substeps: u8,
    /// The server's shot list for the local client.
    pub(crate) shots: ShootList,
    /// The shots delivered right before the last frame.
    pub(crate) last_shots: Buf<Shot>,
    /// The client's projectiles (`cProjectiles`).
    pub(crate) projectiles: ProjectilePool,
    /// OLX's `rand()` and `FindSpot` generators and the entity counter.
    pub(crate) draws: Draws,
    /// The server's and the client's bonuses, and the messages between them.
    pub(crate) bonuses: Bonuses,
    /// Reused each physics step so that `tick` does not allocate (spec §5.3).
    pub(crate) others: Scratch,
}

/// Buffers only: they hold no state, so they never make two worlds unequal.
#[derive(Clone, Debug, Default)]
pub(crate) struct Scratch {
    /// The `(id, alive, pos)` of every worm, as the rope needs them.
    others: Buf<(i32, bool, Vec2)>,
    /// A beam trace's `goodWidthParts`, as wide as the widest beam.
    beam_parts: Buf<bool>,
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
        let bonuses = Bonuses::new(sim_mod.weapons().len(), start + TimeDiff(CLOCK_OFFSET_MS));
        let draws = Draws::new(GlibcRand::new(seed), Mt19937::new(seed), sim_mod.projectiles().len());
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
            substeps: 0,
            shots: ShootList::new(),
            last_shots: Buf::with_capacity(MAX_SHOOTINGS),
            projectiles: ProjectilePool::new(ProjectilePool::DEFAULT_CAPACITY),
            draws,
            bonuses,
            others: Scratch { others: Buf::default(), beam_parts: Buf::with_capacity(terrain_min as usize) },
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

    /// The client's bonuses (`cClient->cBonuses`), which fall and are picked up.
    pub fn bonuses(&self) -> &[Bonus; MAX_BONUSES] {
        &self.bonuses.client
    }

    /// The server's bonuses (`cServer->cBonuses`): a slot stays used for the
    /// bonus's whole `BonusLife`, even after a pickup.
    pub fn server_bonuses(&self) -> &[Bonus; MAX_BONUSES] {
        &self.bonuses.server
    }

    /// Places a bonus at `(x, y)` at the start of the next [`World::tick`],
    /// in the first free server slot and the same client slot, without
    /// `FindSpot` or `rand()` (the oracle's `simPlaceBonus`, spec hook H1).
    /// Nothing is placed if no server slot is free or the weapon does not exist.
    pub fn place_bonus(&mut self, x: i32, y: i32, item: BonusItem) {
        self.bonuses.place(x, y, item);
    }

    /// The weapons a spawned bonus may hold (`goodWpns`): the ids the weapon
    /// restrictions do not ban (`FT_WeaponRest`, `CWpnRest::getWeaponState`).
    /// Ids outside the mod are left out. By default every weapon is allowed,
    /// as for a mod missing from the restriction file (`CWpnRest::updateList`).
    pub fn set_bonus_weapons(&mut self, ids: &[i32]) {
        let n = self.sim_mod.weapons().len() as i32;
        self.bonuses.set_weapons(ids.iter().copied().filter(|id| (0..n).contains(id)));
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

    /// `CClient::fLastSimulationTime`: where the physics clock is, at most
    /// the frame clock ([`World::time`]). A renderer that interpolates
    /// sub-steps reads how far it lags.
    pub fn physics_time(&self) -> AbsTime {
        self.physics_time
    }

    /// How many physics sub-steps the last [`World::tick`] ran (0 in some
    /// frames: 11-ms sub-steps in 10-ms frames). Each event's
    /// [`crate::Event::substep`] is at most this.
    pub fn substeps_last_tick(&self) -> u8 {
        self.substeps
    }

    /// Whether OLX has set projectile type `proj`'s zero `RotIncrement` to 1
    /// (`Proj_SpawnInfo::apply`, `PhysicsLX56_Projectiles.cpp:590-593`): a
    /// projectile of that type was spawned with `UseRandomRot`. From then on
    /// `CProjectile::Draw` animates its rotation as if `RotIncrement` were 1
    /// (`CProjectile.cpp:291-292`). OLX keeps the change in the cached mod,
    /// across games; here it lasts as long as the world. False for unknown types.
    pub fn rot_bumped(&self, proj: usize) -> bool {
        self.draws.rot_bumped.get(proj).copied().unwrap_or(false)
    }

    /// `CMap::FindSpot` (`src/common/CMap.cpp:2381-2396`) with the world's
    /// `FindSpot` generator, as `GameServer::SpawnWorm` picks a spawn
    /// position, for setup before [`World::spawn_worm`]. It advances the
    /// generator, so call it in the same order to replay a match.
    pub fn find_spot(&mut self) -> (i32, i32) {
        let p = find_spot(&self.terrain, &mut self.draws.mt);
        (c_int(p.x), c_int(p.y))
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
    /// index, or `None` if the world already has [`MAX_WORMS`] worms. Its
    /// events (`Spawned`, the hole, the spawn sparkle) come with the next tick's.
    pub fn spawn_worm(&mut self, x: i32, y: i32) -> Option<usize> {
        if self.worms.len() >= MAX_WORMS {
            return None;
        }
        let id = self.worms.len() as i32;
        let pos = Vec2::new(x as f32, y as f32);
        self.worms.push(Worm::spawned(id, pos, self.time, &self.settings));
        self.latches.push(Latches::default());
        self.others.others = Buf::with_capacity(self.worms.len());
        let ev = &mut self.draws.events;
        ev.push(EventKind::Spawned { worm: id });
        carve_px(
            &mut self.terrain,
            &self.holes,
            ev,
            TerrainOp::Carve,
            SPAWN_HOLE,
            pos.x as i32,
            pos.y as i32,
        );
        ev.cosmetic(CosmeticKind::Spawn, pos, Vec2::ZERO);
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
        self.settings.lx56_physics_dt()
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
    /// a worm with no entry holds nothing. Refills `out` with what happened,
    /// after the events of setup calls since the last tick. `out` is the
    /// caller's, so cloning the world never copies events; one made with
    /// [`TickEvents::new`] is never reallocated.
    pub fn tick(&mut self, keys: &[Keys], out: &mut TickEvents) {
        out.start_tick(&mut self.draws.events);
        // The emitters push to `draws.events`: lend them `out` for the tick.
        core::mem::swap(&mut self.draws.events, out);
        self.frame(keys);
        core::mem::swap(&mut self.draws.events, out);
    }

    fn frame(&mut self, keys: &[Keys]) {
        // What OLX does between the previous frame and this one (spec §18 D1,
        // D10): the server's messages, the shots, then the client's messages.
        let num_weapons = self.sim_mod.weapons().len() as i32;
        let wall_clock = self.time + TimeDiff(CLOCK_OFFSET_MS);
        self.bonus_messages(wall_clock, false);
        self.deliver_shots();
        self.bonuses.deliver_to_server(&mut self.worms, num_weapons);
        self.server_frame += 1;
        // `OracleHooks::beforeFrame`: bonuses placed for this frame.
        self.bonus_messages(wall_clock, true);
        self.gus_speed_round_trip();
        // `gusLogicFrame` (`Game.cpp:945-949`): worms left at exactly 0 health die.
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
        self.substeps = 0;
        while self.physics_time + step_clock <= self.time {
            self.physics_step(step.seconds());
            self.physics_time += step_clock;
        }
        // `GameServer::SimulateGame` (`src/server/CServer_Game.cpp:167-261`).
        let mut spawner = Spawner {
            settings: &self.settings,
            terrain: &mut self.terrain,
            holes: &self.holes,
            draws: &mut self.draws,
            now: self.time,
            wall_clock,
        };
        simulate_respawns(&mut self.worms, &mut self.latches, &mut spawner);
        self.bonuses.simulate_server(&self.settings, &self.terrain, &mut self.draws, wall_clock);
        self.time += frame;
    }

    /// The client parses the server's bonus messages, or (`placed`) the
    /// bonuses placed for this frame, which the server spawns first.
    fn bonus_messages(&mut self, wall_clock: AbsTime, placed: bool) {
        let mut ctx = ClientCtx {
            terrain: &mut self.terrain,
            holes: &self.holes,
            draws: &mut self.draws,
            num_weapons: self.sim_mod.weapons().len() as i32,
            wall_clock,
        };
        if placed {
            self.bonuses.apply_placements(&mut ctx);
        } else {
            self.bonuses.deliver_to_client(&mut ctx);
        }
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
            bonuses: &mut self.bonuses.client,
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
            if self.settings.bonuses && self.worms[i].alive {
                let num_weapons = self.sim_mod.weapons().len() as i32;
                let mut h = Harm {
                    settings: &self.settings,
                    worms: &mut self.worms,
                    draws: &mut self.draws,
                    now: self.time,
                };
                self.bonuses.pick_up(&mut h, i, num_weapons);
            }
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
                laser_sight(&self.worms, i, &self.sim_mod, &self.terrain, &mut self.draws);
            }
        }
        // `CClient_Game.cpp:206-208`: the client's entity pass (cosmetic,
        // not simulated here) runs now; later events come after it.
        self.substeps = self.substeps.saturating_add(1);
        self.draws.events.set_substep(self.substeps);
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
            bonuses: &mut self.bonuses.client,
        };
        simulate_projectiles(&mut self.projectiles, &mut ctx);
        // `CClient_Game.cpp:217-218`: bonuses after the projectiles.
        if self.settings.bonuses {
            self.bonuses.simulate_client(&self.terrain, dt);
        }
    }
}

#[cfg(test)]
impl World {
    /// Worm `i`, to change between ticks in tests.
    pub(crate) fn worm_mut(&mut self, i: usize) -> &mut Worm {
        &mut self.worms[i]
    }

    /// [`World::tick`] into a new buffer, for tests that do not count allocations.
    pub(crate) fn tick_new(&mut self, keys: &[Keys]) -> TickEvents {
        let mut out = TickEvents::new();
        self.tick(keys, &mut out);
        out
    }
}

#[cfg(test)]
#[path = "world_tests.rs"]
mod tests;
