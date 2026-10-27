//! The ninja rope (OLX `CNinjaRope`, `src/client/CNinjaRope.cpp`, and
//! `simulateNinjarope`, `src/common/PhysicsLX56.cpp:505-604`).

use olr_data::level::Material;
use olr_math::Vec2;

use crate::rng::Draws;
use crate::solid::Solid;

use crate::SimSettings;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NinjaRope {
    pub released: bool,
    pub shooting: bool,
    pub attached: bool,
    /// Id of the worm the hook is attached to, or -1.
    pub player_attached: i32,
    /// Hook position and velocity.
    pub pos: Vec2,
    pub vel: Vec2,
    /// How many times the rope was thrown ([`NinjaRope::throws`]).
    pub(crate) throws: u32,
}

impl Default for NinjaRope {
    /// A cleared rope (`CNinjaRope::Clear`): nothing attached, player -1.
    fn default() -> Self {
        Self {
            released: false,
            shooting: false,
            attached: false,
            player_attached: -1,
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            throws: 0,
        }
    }
}

impl NinjaRope {
    /// `CNinjaRope::Clear`.
    pub fn clear(&mut self) {
        self.released = false;
        self.shooting = false;
        self.attached = false;
        self.player_attached = -1;
    }

    /// `CNinjaRope::Shoot` from `owner_pos` towards `dir`. Like OLX, a worm
    /// that may not use the rope (`can_use`) only loses its current one.
    pub fn shoot(&mut self, owner_pos: Vec2, owner_vel: Vec2, dir: Vec2, can_use: bool, s: &SimSettings) {
        self.clear();
        if !can_use {
            return;
        }
        self.released = true;
        self.throws = self.throws.wrapping_add(1);
        self.shooting = true;
        self.pos = owner_pos;
        self.vel = dir * s.rope_speed;
        if s.rope_add_parent_speed {
            self.vel += owner_vel;
        }
    }

    /// How many times this worm's rope was thrown (it wraps after 2^32; not
    /// OLX state, the renderer's): a rope thrown again while it is out keeps
    /// `released` but gets another count, so its hook is never drawn
    /// sliding from the old anchor.
    pub fn throws(&self) -> u32 {
        self.throws
    }

    /// `CNinjaRope::Attach`.
    fn attach(&mut self) {
        self.shooting = false;
        self.attached = true;
        self.vel = Vec2::ZERO;
    }

    /// `CNinjaRope::GetForce`: the pull on the owner at `owner_pos`.
    pub fn force(&self, owner_pos: Vec2, s: &SimSettings) -> Vec2 {
        if !self.attached {
            return Vec2::ZERO;
        }
        let dir = (owner_pos - self.pos).normalize();
        let rest = s.rope_rest_length as f32;
        if (owner_pos - self.pos).length2() < rest * rest {
            return Vec2::ZERO;
        }
        dir * (s.rope_strength * -100.0)
    }

    /// `simulateNinjarope` for one sub-step of `dt` seconds. `owner_pos` is the
    /// owner's position at the start of the step; `worms` is `(id, alive, pos)`
    /// of every worm, for hooking onto other worms. Hooking dirt for the first
    /// time throws particles, which consume `draws`. Issue #90: the hook
    /// reads the one terrain query, so it holds on a live rock part.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn simulate(
        &mut self,
        dt: f32,
        owner_id: i32,
        owner_pos: Vec2,
        worms: &[(i32, bool, Vec2)],
        terrain: Solid<'_>,
        s: &SimSettings,
        draws: &mut Draws,
    ) {
        if !self.released {
            return;
        }
        let first_hit = !self.attached;
        let mut force = Vec2::new(0.0, if self.shooting { s.rope_gravity } else { s.rope_falling_gravity });

        if (self.vel + force * dt).length2() * dt * dt * s.ninjarope_precision > 5.0 {
            self.simulate(dt / 2.0, owner_id, owner_pos, worms, terrain, s, draws);
            self.simulate(dt / 2.0, owner_id, owner_pos, worms, terrain, s, draws);
            return;
        }

        if self.shooting {
            self.vel += force * dt;
            self.pos += self.vel * dt;
            let length2 = (owner_pos - self.pos).length2();
            let max = s.rope_max_length as f32;
            if length2 > max * max {
                self.vel = Vec2::ZERO;
                self.shooting = false;
            }
        } else if !self.attached {
            let length2 = (owner_pos - self.pos).length2();
            let rest = s.rope_rest_length as f32;
            if length2 > rest * rest {
                let mut d = owner_pos - self.pos;
                if length2 != 0.0 {
                    d *= 1.0 / libm::sqrtf(length2);
                }
                force += (d * 10000.0) * dt;
            }
            self.vel += force * dt;
            self.pos += self.vel * dt;
        }

        let player_attached = self.player_attached_alive(worms);
        let mut outside_map = false;
        let (w, h) = (terrain.width() as f32, terrain.height() as f32);
        if !player_attached
            && (self.pos.x <= 0.0 || self.pos.y <= 0.0 || self.pos.x >= w - 1.0 || self.pos.y >= h - 1.0)
        {
            self.attach();
            self.pos.x = if 0.0 > self.pos.x { 0.0 } else { self.pos.x };
            self.pos.y = if 0.0 > self.pos.y { 0.0 } else { self.pos.y };
            self.pos.x = if w - 1.0 < self.pos.x { w - 1.0 } else { self.pos.x };
            self.pos.y = if h - 1.0 < self.pos.y { h - 1.0 } else { self.pos.y };
            outside_map = true;
        }

        if !player_attached {
            self.attached = false;
            let wx = (self.pos.x as i64).rem_euclid(terrain.width().max(1) as i64);
            let wy = (self.pos.y as i64).rem_euclid(terrain.height().max(1) as i64);
            let m = terrain.material(wx, wy).unwrap_or(Material::Rock);
            // Rock and dirt stop particles and can be hooked; background lets them pass.
            let particle_pass = m == Material::Background;
            let can_hook = m != Material::Background;
            if !particle_pass || outside_map {
                if !outside_map && !can_hook {
                    self.vel = Vec2::ZERO;
                    self.shooting = false;
                } else {
                    self.attach();
                    // `px.destroyable && firsthit`: five particles, each `GetRandomVec()`.
                    if m == Material::Dirt && first_hit {
                        let probe = (wx as i32, wy as i32);
                        draws.particles(5, self.pos + Vec2::new(0.0, 2.0), probe, |v| v * 40.0);
                    }
                }
            }
        }

        self.check_worm_attachment(owner_id, worms, s);
    }

    fn player_attached_alive(&self, worms: &[(i32, bool, Vec2)]) -> bool {
        self.player_attached >= 0 && worms.iter().any(|(id, _, _)| *id == self.player_attached)
    }

    /// `CNinjaRope::checkForWormAttachment`.
    fn check_worm_attachment(&mut self, owner_id: i32, worms: &[(i32, bool, Vec2)], s: &SimSettings) {
        if s.rope_can_attach_worm && !self.attached && !self.player_attached_alive(worms) {
            for &(id, alive, pos) in worms {
                if id == owner_id || !alive {
                    continue;
                }
                if (pos - self.pos).length2() < 25.0 {
                    self.attach();
                    self.player_attached = id;
                    break;
                }
            }
        } else if self.attached && self.player_attached_alive(worms) {
            match worms.iter().find(|(id, _, _)| *id == self.player_attached) {
                Some(&(_, true, pos)) => self.pos = pos,
                _ => {
                    // UnAttachPlayer
                    self.vel = Vec2::ZERO;
                    self.shooting = false;
                    self.attached = false;
                    self.player_attached = -1;
                }
            }
        }
    }
}
