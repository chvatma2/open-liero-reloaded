//! One LX56 physics sub-step of [`World::tick`].

use crate::beam::BeamCtx;
use crate::damage::Harm;
use crate::physics::{Ctx, simulate_worm};
use crate::proj_sim::{ProjCtx, simulate_projectiles};
use crate::weapons::{ShootCtx, laser_sight, player_shoot, worm_shoot};
use crate::world::{CLOCK_OFFSET_MS, FRAME_MS, World};

use rl_math::TimeDiff;

impl World {
    /// One LX56 sub-step of `CClient::Simulation` (`src/client/CClient_Game.cpp:97-221`).
    pub(crate) fn physics_step(&mut self, dt: f32) {
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
