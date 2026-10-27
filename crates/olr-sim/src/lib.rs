//! The deterministic simulation (spec §5), ported from OpenLieroX's LX56
//! physics at commit `d1ad953f` and verified against the oracle traces in
//! `fixtures/oracle` (bit-exact).
//!
//! `#![no_std]` + `alloc`: no files, threads or clock. A match starts from a
//! [`MatchSetup`] and a [`GameMode`] ([`World::new`]). Apart from setup
//! ([`World::set_health`], [`World::place_bonus`], [`World::set_live_state`],
//! [`World::set_live_value`]), [`World::tick`] is the
//! only way state changes, and it does not allocate, mode commands included.
//!
//! [`World::tick`] refills the caller's [`TickEvents`] (spec §5.7), and
//! [`World::checksum`] hashes the whole simulation state.
//!
//! Scope: worm movement and collision, keyboard input, digging, the ninja
//! rope, OLX's two clocks, and (M4) weapon slots, firing, the shot round trip,
//! projectile spawning, motion, collision and actions, explosions, dirt,
//! damage, death, bleeding and respawning, beams and the jetpack, and bonuses.
#![no_std]

extern crate alloc;

mod actions;
mod beam;
mod bonus;
mod buf;
mod checksum;
mod damage;
mod effects;
mod events;
mod input;
mod live;
mod mod_rt;
mod mode;
mod physics;
mod proj_sim;
mod projectile;
mod referee;
mod respawn;
mod rng;
mod rope;
mod settings;
mod setup;
mod shot_codec;
mod shots;
mod snapshot;
mod spawn;
mod state;
mod tally;
mod weapons;
mod world;
mod world_step;
mod worm;

pub use bonus::{Bonus, BonusItem, BonusKind, MAX_BONUSES, PlaceBonusError};
pub use checksum::Fnv;
pub use damage::{olx_kill_credit, olx_score};
pub use events::{
    BEAM_EXPLOSION, CosmeticKind, Event, EventKind, GAME_SOUND_PREFIX, GAME_SOUNDS, LostArea, MAX_BEAM_HITS,
    SoundId, StampId, TerrainOp, TickEvents,
};
pub use input::{KeyLatch, Keys, Latches};
pub use live::{
    LIVE_SUBPIXELS, LiveCopy, LiveCopySetup, LiveCopyView, LiveError, LiveHealth, LiveLevel, LivePart,
    LivePartKind, LiveProp, LiveSolid, LiveState, LiveTarget, LiveView, LiveWire,
};
pub use mod_rt::{
    MAX_AMOUNT, MAX_DEPTH, MAX_PROJECTILES, SimMod, SimModError, SimProjectile, SimWeapon, SpawnInfo,
    WeaponType,
};
pub use mode::{
    COMMAND_CAPACITY, CommandError, EndReason, GameMode, KillCredit, MatchResult, Message, ModeCtx,
    ModeStateError, OracleMode, Score, StampOp, TextId,
};
pub use projectile::{Projectile, ProjectilePool};
pub use rope::NinjaRope;
pub use settings::{SettingsError, SimSettings};
pub use setup::{MAX_SLOTS, MatchSetup, SetupError, WormSetup};
pub use shots::{MAX_SHOOTINGS, RoundTrip, Shot, encode_decode, spawn_time};
pub use snapshot::{RestoreError, Snapshot};
pub use state::{StateError, WorldState};
pub use tally::{ACTION_KINDS, ActionTally, HitAudit, projectile_actions};
pub use world::{CLOCK_OFFSET_MS, MAX_WORMS, World};
pub use worm::{Dir, Loadout, PosRecorder, Slot, Worm, WormState};
