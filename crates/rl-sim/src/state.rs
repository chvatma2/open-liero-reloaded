//! World-state transfer (networking spec §3.5, §3.6; plan 9 decision N5): a
//! world built from the same match setup (settings, mod, level, holes,
//! green mask, worms, pool size, game mode and its settings) takes another
//! world's dynamic state, for a spectator who arrives mid-match or a peer
//! repairing a desync.
//!
//! [`WorldState`] holds exactly what [`World::checksum`] hashes: the
//! terrain's cells, the worms and their latches, both clocks, the server
//! frame, the sub-step count, the shot lists, the projectile slots ever
//! taken and the pool's indices, the generators, the entity counter, the
//! `RotIncrement` bumps, both bonus arrays and their queues, the mode's
//! bytes ([`crate::GameMode::save_state`]) and the match's end. The rest is setup
//! (or not state) and must already agree. On the wire it is postcard, then
//! deflate.
//!
//! The state comes from a host that may be a stranger: [`World::load_state`]
//! checks everything that could index out of bounds, grow a buffer past
//! its capacity, overflow a clock or a counter, or keep a tick from ending,
//! before it changes anything, and is all-or-nothing.

use alloc::vec::Vec;
use core::fmt;

use rl_math::{AbsTime, GlibcRand, Mt19937};
use serde::{Deserialize, Serialize};

use crate::bonus::Bonuses;
use crate::events::TickEvents;
use crate::input::Latches;
use crate::mode::{MatchResult, ModeStateError};
use crate::proj_sim::catch_up_step;
use crate::projectile::{PoolState, Projectile};
use crate::referee::{Referee, frozen};
use crate::rng::Draws;
use crate::rope::NinjaRope;
use crate::setup::{MAX_SLOTS, Spare};
use crate::shots::{MAX_SHOOTINGS, ShootList, Shot};
use crate::world::{CLOCK_OFFSET_MS, FRAME_MS, MAX_WORMS, World};
use crate::worm::Worm;

/// The latest a clock may be (ms), or a counter that counts up once a
/// tick or draw: far beyond any match, so that adding to it cannot overflow.
const MAX_CLOCK: u64 = 1 << 62;
/// Slack on top of the furthest a moving projectile's clock gets from the
/// frame clock (ms; see `World::validate`).
const PROJECTILE_LAG_MARGIN_MS: u64 = 4 * FRAME_MS;
/// The fastest a thrown rope's hook may move (px/s, each axis): the rope
/// halves its step until the hook moves less than a few pixels, so a
/// hostile speed would keep a tick from ending. Rope speeds are at most
/// `RopeSpeed` (1000) plus the worm's: at the extremes of
/// `SimSettings::validate` (1 to 200 physics steps a second, GameSpeed 0.1
/// to 10) the tests see hooks at about 1100 px/s and worms at about 3100.
const MAX_ROPE_SPEED: f32 = 1e5;
/// The largest dirt count: digging adds to it without wrapping
/// (`w.dirt_count += carved`, `physics.rs:204`), a few pixels a sub-step.
const MAX_DIRT_COUNT: u32 = 1 << 30;

/// Another world's dynamic state, for [`World::load_state`] (networking
/// spec §3.5): [`WorldState::encode`] it to send it.
///
/// The field order is the wire layout: `tests/state.rs`'s aimed proptest
/// counts on the cells first, then the worms, latches and clocks, and the
/// generators (at least 3 KB with `Mt19937`'s 624 words), bonuses, mode and
/// end last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    width: u32,
    height: u32,
    cells: Vec<u8>,
    setup_worms: u64,
    worms: Vec<Worm>,
    latches: Vec<Latches>,
    time: AbsTime,
    physics_time: AbsTime,
    server_frame: u64,
    substeps: u8,
    shots: ShootList,
    last_shots: Vec<Shot>,
    projectiles: PoolState,
    rand: GlibcRand,
    mt: Mt19937,
    ents: u32,
    rot_bumped: Vec<bool>,
    bonuses: Bonuses,
    mode: Vec<u8>,
    over: Option<MatchResult>,
    over_frame: u64,
    game_over_sent: bool,
}

/// Why a [`WorldState`] was refused: it did not decode, or which part of it
/// does not fit the receiving world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateError {
    /// Not a deflated postcard `WorldState`.
    Decode,
    /// It inflates past [`WorldState::MAX_INFLATED`].
    TooLarge,
    /// Another map size, or a cell that is no material.
    Terrain,
    /// Another worm count at setup, too many worms, or a worm whose ids,
    /// slots, weapons or position record a match never reaches.
    Worms,
    /// The clocks or the server frame are out of step or out of range.
    Clocks,
    /// Another pool size, pool indices outside it, or a projectile of no
    /// type, owner or clock a match reaches.
    Projectiles,
    /// A shot list, or the `RotIncrement` bumps, that do not fit.
    Buffers,
    /// Bonus queues past their capacity, another setup's bonus weapons, or
    /// a weapon outside the mod.
    Bonuses,
    /// A generator whose table index is out of range.
    Rng,
    /// The game mode refused its part.
    Mode(ModeStateError),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::Decode => write!(f, "not a world state"),
            StateError::TooLarge => {
                write!(f, "a world state larger than {} bytes", WorldState::MAX_INFLATED)
            }
            StateError::Terrain => write!(f, "a world state for another map"),
            StateError::Worms => write!(f, "a world state with worms this match cannot have"),
            StateError::Clocks => write!(f, "a world state with clocks out of step"),
            StateError::Projectiles => write!(f, "a world state with projectiles this match cannot have"),
            StateError::Buffers => write!(f, "a world state with shots this match cannot have"),
            StateError::Bonuses => write!(f, "a world state with bonuses this match cannot have"),
            StateError::Rng => write!(f, "a world state with broken generators"),
            StateError::Mode(e) => write!(f, "the game mode refused its state: {e:?}"),
        }
    }
}

impl WorldState {
    /// The most bytes a state may inflate to: a 4096 × 4096 map (16 MiB)
    /// and the largest projectile pool fit several times over.
    pub const MAX_INFLATED: usize = 64 * 1024 * 1024;

    /// The state for the wire: postcard, then deflate.
    pub fn encode(&self) -> Vec<u8> {
        miniz_oxide::deflate::compress_to_vec(&self.to_raw(), 6)
    }

    /// Reads what [`WorldState::encode`] wrote. Bytes that inflate past
    /// [`WorldState::MAX_INFLATED`] are refused before they are read.
    pub fn decode(bytes: &[u8]) -> Result<WorldState, StateError> {
        let raw = miniz_oxide::inflate::decompress_to_vec_with_limit(bytes, Self::MAX_INFLATED).map_err(
            |e| match e.status {
                miniz_oxide::inflate::TINFLStatus::HasMoreOutput => StateError::TooLarge,
                _ => StateError::Decode,
            },
        )?;
        Self::from_raw(&raw)
    }

    /// The postcard encoding alone, for tests.
    #[doc(hidden)]
    pub fn to_raw(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("postcard encodes every world state")
    }

    /// Reads [`WorldState::to_raw`]'s bytes, all of them.
    #[doc(hidden)]
    pub fn from_raw(bytes: &[u8]) -> Result<WorldState, StateError> {
        match postcard::take_from_bytes(bytes) {
            Ok((s, [])) => Ok(s),
            _ => Err(StateError::Decode),
        }
    }
}

impl World {
    /// The world's dynamic state, for another world of the same setup
    /// ([`World::load_state`]). Mode commands are not part of it: they are
    /// applied within the tick that queues them, so none wait between ticks.
    pub fn state(&self) -> WorldState {
        // Exhaustive, like the checksum's: a new field does not compile
        // until it is transferred here or listed as setup only.
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
            setup_worms,
        } = self;
        let Draws { rand, mt, ents, events: _, rot_bumped, spawn_work: _, actions: _ } = draws;
        let Referee { mode, commands: _, over, over_frame, game_over_sent } = referee;
        WorldState {
            width: terrain.width(),
            height: terrain.height(),
            cells: terrain.cells().to_vec(),
            setup_worms: *setup_worms as u64,
            worms: worms.to_vec(),
            latches: latches.to_vec(),
            time: *time,
            physics_time: *physics_time,
            server_frame: *server_frame,
            substeps: *substeps,
            shots: shots.clone(),
            last_shots: last_shots.to_vec(),
            projectiles: projectiles.state(),
            rand: rand.clone(),
            mt: mt.clone(),
            ents: *ents,
            rot_bumped: rot_bumped.clone(),
            bonuses: bonuses.clone(),
            mode: mode.save_state(),
            over: *over,
            over_frame: *over_frame,
            game_over_sent: *game_over_sent,
        }
    }

    /// Takes `s`, another world's [`World::state`], so this world equals
    /// it (checksum included) and evolves as it would. This world must be
    /// built from the same setup. Everything is checked first: on an error
    /// nothing changes. Events waiting for the next tick (setup's) are
    /// dropped, and the terrain journal cannot undo the load: snapshots
    /// taken before it are refused afterwards.
    pub fn load_state(&mut self, s: WorldState) -> Result<(), StateError> {
        self.validate(&s)?;
        let mut mode = self.referee.mode.box_clone();
        mode.load_state(&s.mode).map_err(StateError::Mode)?;
        let WorldState {
            width: _,
            height: _,
            cells,
            setup_worms: _,
            worms,
            latches,
            time,
            physics_time,
            server_frame,
            substeps,
            shots,
            last_shots,
            projectiles,
            rand,
            mt,
            ents,
            rot_bumped,
            bonuses,
            mode: _,
            over,
            over_frame,
            game_over_sent,
        } = s;
        if !self.terrain.load_cells(&cells) {
            return Err(StateError::Terrain);
        }
        // From here on nothing fails; every buffer keeps its capacity.
        self.spare = Spare::new(MAX_WORMS - worms.len());
        self.worms.clear();
        self.worms.extend(worms);
        self.latches.clear();
        self.latches.extend(latches);
        (self.time, self.physics_time, self.server_frame, self.substeps) =
            (time, physics_time, server_frame, substeps);
        self.shots.load(shots);
        self.last_shots.clear();
        self.last_shots.extend(last_shots);
        self.projectiles.load(projectiles);
        let d = &mut self.draws;
        (d.rand, d.mt, d.ents) = (rand, mt, ents);
        d.rot_bumped.copy_from_slice(&rot_bumped);
        d.events = TickEvents::pending();
        self.bonuses.load(bonuses);
        let r = &mut self.referee;
        r.mode = mode;
        r.commands.list.clear();
        (r.over, r.over_frame, r.game_over_sent) = (over, over_frame, game_over_sent);
        Ok(())
    }

    /// Checks `s` against this world, part by part, changing nothing.
    fn validate(&self, s: &WorldState) -> Result<(), StateError> {
        let num_weapons = i32::try_from(self.sim_mod.weapons().len()).unwrap_or(i32::MAX);
        let check = |ok: bool, e: StateError| if ok { Ok(()) } else { Err(e) };

        let t = &self.terrain;
        check(s.width == t.width() && s.height == t.height() && t.can_load(&s.cells), StateError::Terrain)?;

        let n = s.worms.len();
        check(
            s.setup_worms == self.setup_worms as u64
                && (self.setup_worms..=MAX_WORMS.min(self.worms.capacity())).contains(&n)
                && s.latches.len() == n
                && s.worms.iter().enumerate().all(|(i, w)| worm_ok(i, w, n, num_weapons)),
            StateError::Worms,
        )?;

        let lag = self.step_clock().0.saturating_add(self.frame_dt().0);
        check(
            s.time.0 <= MAX_CLOCK
                && s.physics_time <= s.time
                && s.time.0 - s.physics_time.0 <= lag
                && s.server_frame <= MAX_CLOCK,
            StateError::Clocks,
        )?;

        // A moving projectile's clock is caught up to the frame clock in
        // steps of `catch_up_step`, only in sub-steps, which may be a whole
        // `step_clock` (plus the frame) apart; a shot spawns up to
        // `CLOCK_OFFSET_MS` ahead. Further away, it would run thousands of
        // steps in one tick to catch up.
        let max_lag = catch_up_step(&self.settings, self.physics_dt())
            .0
            .saturating_add(lag)
            .saturating_add(CLOCK_OFFSET_MS + PROJECTILE_LAG_MARGIN_MS);
        let projs = self.sim_mod.projectiles();
        let frozen = frozen(s.over.is_some(), s.over_frame, s.server_frame);
        let projectile_ok = |p: &Projectile| {
            let Some(info) = projs.get(p.proj) else { return false };
            let radius_ok = |r: i32, size: i32| r == size / 2 || r == (size / 2).max(0);
            // Frozen after the end, projectile clocks only skip ahead by 10 ms
            // a frame and never run.
            let clock_ok = if frozen {
                p.last_sim_time.0 <= MAX_CLOCK
            } else {
                p.last_sim_time.0.abs_diff(s.time.0) <= max_lag
            };
            radius_ok(p.radius.0, info.def.width)
                && radius_ok(p.radius.1, info.def.height)
                && (-1..MAX_WORMS as i32).contains(&p.owner)
                && clock_ok
        };
        check(self.projectiles.can_load(&s.projectiles, projectile_ok), StateError::Projectiles)?;

        let last_shots_room = MAX_SHOOTINGS.min(self.last_shots.capacity());
        check(
            self.shots.can_load(&s.shots, num_weapons)
                && s.last_shots.len() <= last_shots_room
                && s.last_shots.iter().all(|x| (0..num_weapons).contains(&x.weapon))
                && s.rot_bumped.len() == self.draws.rot_bumped.len(),
            StateError::Buffers,
        )?;

        check(self.bonuses.can_load(&s.bonuses, num_weapons), StateError::Bonuses)?;

        check(
            s.rand.is_valid() && s.mt.is_valid() && s.rand.calls() <= MAX_CLOCK && s.mt.calls() <= MAX_CLOCK,
            StateError::Rng,
        )
    }
}

/// Whether worm `i` of `worms` is one a match reaches: its id is its
/// index, at most [`MAX_SLOTS`] slots of the mod's weapons (or none, -1),
/// the current one among them, a rope hooked to a worm or none, a position
/// record within its buffer, and no value that keeps a tick from ending
/// or a counter from counting.
fn worm_ok(i: usize, w: &Worm, worms: usize, num_weapons: i32) -> bool {
    let slots = w.slots.len();
    let NinjaRope { released, player_attached, vel, .. } = w.rope;
    let rope_ok = !released || !(libm::fabsf(vel.x) > MAX_ROPE_SPEED || libm::fabsf(vel.y) > MAX_ROPE_SPEED);
    // The laser sight turns a finite aim into (-180, 180] 360° at a time.
    let angle_ok = !w.angle.is_finite() || libm::fabsf(w.angle) <= 360.0;
    w.id as i64 == i as i64
        && slots <= MAX_SLOTS
        && w.slots.iter().all(|s| s.weapon == -1 || (0..num_weapons).contains(&s.weapon))
        && (0..slots.max(1) as i64).contains(&(w.current_weapon as i64))
        && (-1..worms as i64).contains(&(player_attached as i64))
        && w.pos_recorder.is_valid()
        && rope_ok
        && angle_ok
        && w.dirt_count.unsigned_abs() <= MAX_DIRT_COUNT
}
