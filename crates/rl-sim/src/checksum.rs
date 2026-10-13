//! [`World::checksum`]: FNV-1a 64 over the whole simulation state.
//!
//! Every type is hashed field by field in declaration order, through an
//! exhaustive destructuring, so a new field does not compile until it is
//! hashed. Integers are hashed as their little-endian bytes, floats as their
//! bits (`-0.0` and `+0.0` differ, as they do in the simulation; every NaN
//! as the one canonical quiet NaN, since targets differ in the payloads they
//! produce), `bool` and enums as one byte, and a variable-length list after
//! its length. Types with private fields hash themselves in their own module,
//! and the game mode its own state ([`crate::GameMode::checksum`]).

use rl_math::{AbsTime, TimeDiff, Vec2};

use crate::World;
use crate::bonus::{Bonus, BonusItem, BonusKind};
use crate::projectile::Projectile;
use crate::rng::Draws;
use crate::rope::NinjaRope;
use crate::shots::Shot;
use crate::worm::{Dir, Slot, Worm, WormState};

/// FNV-1a 64 (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`),
/// the hash of [`World::checksum`], which game modes feed their state into.
#[derive(Clone, Debug)]
pub struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv {
    pub fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    pub fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = (self.0 ^ x as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub fn u64(&mut self, v: u64) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.bytes(&v.to_le_bytes());
    }

    /// The bits, with every NaN as `0x7fc0_0000`.
    pub fn f32(&mut self, v: f32) {
        self.u32(if v.is_nan() { 0x7fc0_0000 } else { v.to_bits() });
    }

    pub fn bool(&mut self, v: bool) {
        self.bytes(&[v as u8]);
    }

    pub fn usize(&mut self, v: usize) {
        self.u64(v as u64);
    }

    pub fn vec2(&mut self, v: Vec2) {
        self.f32(v.x);
        self.f32(v.y);
    }

    pub fn time(&mut self, t: AbsTime) {
        self.u64(t.0);
    }

    pub fn diff(&mut self, t: TimeDiff) {
        self.u64(t.0);
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// A part of the simulation state.
pub(crate) trait Checksum {
    fn hash(&self, h: &mut Fnv);
}

impl<T: Checksum> Checksum for [T] {
    fn hash(&self, h: &mut Fnv) {
        h.usize(self.len());
        self.iter().for_each(|x| x.hash(h));
    }
}

impl World {
    /// FNV-1a 64 over all simulation state, in a fixed order: the terrain
    /// (its material hash), the worms, their key latches, both clocks, the
    /// server frame, the last tick's sub-step count, the shot list and the
    /// last delivered shots, every projectile slot ever used (with its
    /// serial) and the pool's indices and spawn counter, the generators, the
    /// entity counter and the `RotIncrement` bumps, both bonus arrays
    /// with the messages between them, then the game mode's state and the
    /// match's end. Two worlds that evolve identically
    /// have equal checksums, and any change to that state changes it
    /// (barring a hash collision).
    ///
    /// Left out: what only setup sets (the settings, the mod, the theme's
    /// holes, the green-dirt mask and the setup's worm count), events (output, not state: setup
    /// events waiting for the next tick too), the spawn work counter and
    /// scratch and spare buffers.
    pub fn checksum(&self) -> u64 {
        self.hash_state(self.terrain.material_hash())
    }

    /// [`World::checksum`] with the terrain as its cell hash
    /// ([`rl_terrain::Terrain::cell_hash`]): O(1) to read while rollback is
    /// on, so the session can compare worlds every 10 ticks (networking spec
    /// §3.5). Not equal to `checksum`; equal between equal worlds whether or
    /// not rollback is on.
    pub fn sync_checksum(&self) -> u64 {
        self.hash_state(self.terrain.cell_hash())
    }

    fn hash_state(&self, terrain_hash: u64) -> u64 {
        let World {
            settings: _,
            sim_mod: _,
            terrain,
            holes: _,
            green_mask: _,
            worms,
            latches,
            time,
            physics_time,
            server_frame,
            substeps,
            shots,
            last_shots,
            projectiles,
            draws,
            bonuses,
            referee,
            others: _,
            spare: _,
            setup_worms: _,
        } = self;
        let mut h = Fnv::new();
        h.u32(terrain.width());
        h.u32(terrain.height());
        h.u64(terrain_hash);
        worms[..].hash(&mut h);
        latches[..].hash(&mut h);
        h.time(*time);
        h.time(*physics_time);
        h.u64(*server_frame);
        h.bytes(&[*substeps]);
        shots.hash(&mut h);
        last_shots.hash(&mut h);
        projectiles.hash(&mut h);
        draws.hash(&mut h);
        bonuses.hash(&mut h);
        referee.hash(&mut h);
        h.finish()
    }
}

impl Checksum for Draws {
    fn hash(&self, h: &mut Fnv) {
        let Draws { rand, mt, ents, events: _, rot_bumped, spawn_work: _, actions: _, hits: _ } = self;
        rand.visit_state(|w| h.u64(w));
        mt.visit_state(|w| h.u64(w));
        h.u32(*ents);
        h.usize(rot_bumped.len());
        for b in rot_bumped {
            h.bool(*b);
        }
    }
}

impl Checksum for Worm {
    fn hash(&self, h: &mut Fnv) {
        let Worm {
            id,
            team,
            alive,
            pos,
            vel,
            health,
            angle,
            angle_speed,
            face,
            move_dir,
            on_ground,
            state,
            current_weapon,
            slots,
            rope,
            lives,
            kills,
            deaths,
            suicides,
            damage,
            damage_factor,
            shield_factor,
            time_of_death,
            last_blood,
            spawned_once,
            can_respawn_now,
            respawn_requested,
            dirt_count,
            frame,
            speed_factor,
            can_use_ninja,
            can_air_jump,
            last_input_time,
            last_air_jump_time,
            pos_recorder,
        } = self;
        h.i32(*id);
        h.bytes(&[*team]);
        h.bool(*alive);
        h.vec2(*pos);
        h.vec2(*vel);
        h.f32(*health);
        h.f32(*angle);
        h.f32(*angle_speed);
        face.hash(h);
        move_dir.hash(h);
        h.bool(*on_ground);
        state.hash(h);
        h.i32(*current_weapon);
        slots.hash(h);
        rope.hash(h);
        for v in [lives, kills, deaths, suicides] {
            h.i32(*v);
        }
        for v in [damage, damage_factor, shield_factor] {
            h.f32(*v);
        }
        h.time(*time_of_death);
        h.time(*last_blood);
        for v in [spawned_once, can_respawn_now, respawn_requested] {
            h.bool(*v);
        }
        h.i32(*dirt_count);
        h.f32(*frame);
        h.f32(*speed_factor);
        h.bool(*can_use_ninja);
        h.bool(*can_air_jump);
        h.time(*last_input_time);
        h.time(*last_air_jump_time);
        pos_recorder.hash(h);
    }
}

impl Checksum for Dir {
    fn hash(&self, h: &mut Fnv) {
        h.bytes(&[*self as u8]);
    }
}

impl Checksum for WormState {
    fn hash(&self, h: &mut Fnv) {
        h.bytes(&[self.as_int()]);
    }
}

impl Checksum for Slot {
    fn hash(&self, h: &mut Fnv) {
        let Slot { weapon, charge, reloading, last_fire } = self;
        h.i32(*weapon);
        h.f32(*charge);
        h.bool(*reloading);
        h.f32(*last_fire);
    }
}

impl Checksum for NinjaRope {
    fn hash(&self, h: &mut Fnv) {
        let NinjaRope { released, shooting, attached, player_attached, pos, vel, throws } = self;
        h.bool(*released);
        h.bool(*shooting);
        h.bool(*attached);
        h.i32(*player_attached);
        h.vec2(*pos);
        h.vec2(*vel);
        h.u32(*throws);
    }
}

impl Checksum for Shot {
    fn hash(&self, h: &mut Fnv) {
        let Shot { worm, weapon, pos, worm_vel, angle, random, speed, time, release, spawn } = self;
        h.i32(*worm);
        h.i32(*weapon);
        h.vec2(*pos);
        h.vec2(*worm_vel);
        h.i32(*angle);
        h.i32(*random);
        h.i32(*speed);
        h.diff(*time);
        h.bool(*release);
        h.time(*spawn);
    }
}

impl Checksum for Projectile {
    fn hash(&self, h: &mut Fnv) {
        let Projectile {
            used,
            proj,
            spawn_time,
            life,
            extra,
            owner,
            speed,
            colour,
            ignore_worm_coll_before,
            last_trail_proj,
            time_var_random,
            pos,
            vel,
            old_pos,
            rotation,
            radius,
            random,
            max_checkstep,
            min_checkstep,
            max_checkstep2,
            min_checkstep2,
            avg_checkstep,
            wallshoot_time,
            changes_speed,
            check_speed_len,
            collision_side,
            frame_delta,
            frame,
            frame_x,
            first_bounce,
            last_sim_time,
            health,
            serial,
        } = self;
        h.bool(*used);
        h.usize(*proj);
        h.time(*spawn_time);
        h.f32(*life);
        h.f32(*extra);
        h.i32(*owner);
        h.f32(*speed);
        h.u32(*colour);
        h.time(*ignore_worm_coll_before);
        h.time(*last_trail_proj);
        h.f32(*time_var_random);
        h.vec2(*pos);
        h.vec2(*vel);
        h.vec2(*old_pos);
        h.f32(*rotation);
        h.i32(radius.0);
        h.i32(radius.1);
        for v in [random, max_checkstep, min_checkstep, max_checkstep2, min_checkstep2, avg_checkstep] {
            h.i32(*v);
        }
        h.f32(*wallshoot_time);
        h.bool(*changes_speed);
        h.i32(*check_speed_len);
        h.i32(*collision_side);
        h.bool(*frame_delta);
        h.f32(*frame);
        h.i32(*frame_x);
        h.bool(*first_bounce);
        h.time(*last_sim_time);
        h.f32(*health);
        h.u32(*serial);
    }
}

impl Checksum for BonusKind {
    fn hash(&self, h: &mut Fnv) {
        h.i32(*self as i32);
    }
}

impl Checksum for BonusItem {
    fn hash(&self, h: &mut Fnv) {
        match self {
            BonusItem::Health => h.i32(-1),
            BonusItem::Weapon(w) => {
                h.i32(0);
                h.i32(*w);
            }
        }
    }
}

impl Checksum for Bonus {
    fn hash(&self, h: &mut Fnv) {
        let Bonus { used, kind, weapon, pos, vel, life, spawn_time, flash_time } = self;
        h.bool(*used);
        kind.hash(h);
        h.i32(*weapon);
        h.vec2(*pos);
        h.vec2(*vel);
        h.f32(*life);
        h.time(*spawn_time);
        h.f32(*flash_time);
    }
}
