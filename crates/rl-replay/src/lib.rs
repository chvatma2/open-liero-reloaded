//! Match replays (spec §20 F8): the setup a match started from (pack ids
//! and content hashes, settings, seed, worms, the mode), every worm's keys
//! for every tick, bots included, and the world's checksum every
//! [`CHECKSUM_EVERY`] ticks.
//!
//! `#![no_std]` + `alloc`. A [`Recorder`] follows a match; [`Replay::encode`]
//! and [`Replay::decode`] give the compact, versioned file format; a replay
//! rebuilds its world from [`Assets`] ([`Replay::world`]; `rl-content`
//! resolves the packs and checks their hashes) and replays it, checking the
//! checksums as it goes ([`Replay::play`], [`Player`]).
#![no_std]

extern crate alloc;

mod error;
mod mirror;
mod play;
mod record;
mod wire;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use rl_math::AbsTime;
use rl_modes::{Deathmatch, DeathmatchSettings};
use rl_sim::{BonusItem, GameMode, Keys, MatchSetup, OracleMode, SimSettings, WormSetup};

pub use error::{DecodeError, Mismatch, RecordError, ReplayError};
pub use play::{Assets, Cursor, Player, ReplayRun};
pub use record::Recorder;

/// The format version [`Replay::encode`] writes and the only one
/// [`Replay::decode`] reads. Bump it whenever the encoding changes (a field
/// of the setup, the settings, the mode or the key columns): while no
/// replay is published, other versions are refused, not migrated.
pub const FORMAT_VERSION: u16 = 1;

/// The first bytes of every replay file.
pub const MAGIC: [u8; 4] = *b"RLRP";

/// A checksum is recorded every this many ticks (and at the end).
pub const CHECKSUM_EVERY: u32 = 100;

/// The longest replay: two hours at 100 Hz. With [`rl_sim::MAX_WORMS`]
/// worms its keys take at most 23 MB decoded.
pub const MAX_TICKS: u32 = 2 * 60 * 60 * 100;

/// At most this many setup calls.
pub const MAX_SETUP_CALLS: usize = 1024;

/// A content pack by id and content hash (`rl_pack::content_hash`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackRef {
    pub id: String,
    pub hash: String,
}

/// The packs a match is made of: the mod, the level and the level's theme
/// (holes and green-dirt mask).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packs {
    pub mod_pack: PackRef,
    pub level: PackRef,
    pub theme: PackRef,
}

/// A change a caller made to the world between `World::new` and the first
/// tick. A replay makes the same calls in the same order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SetupCall {
    /// `World::set_health`.
    SetHealth { worm: u32, health: f32 },
    /// `World::place_bonus`.
    PlaceBonus { x: i32, y: i32, item: BonusItem },
    /// `World::find_spot` (it advances the `FindSpot` generator).
    FindSpot,
    /// `World::align_physics_clock`.
    AlignPhysicsClock,
}

/// What a replay's world starts from: a [`MatchSetup`] whose mod, terrain,
/// holes and green-dirt mask come from [`Packs`] (as
/// `rl_content::match_setup` builds them), and the setup calls made before
/// the first tick.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplaySetup {
    pub packs: Packs,
    pub settings: SimSettings,
    pub start: AbsTime,
    pub seed: u32,
    pub worms: Vec<WormSetup>,
    pub pool_capacity: usize,
    pub bonus_weapons: Option<Vec<i32>>,
    pub calls: Vec<SetupCall>,
}

impl ReplaySetup {
    /// `setup` (as `World::new` gets it: after `Deathmatch::prepare`) made
    /// of `packs`. Its content is named by `packs`, not stored.
    pub fn new(packs: Packs, setup: &MatchSetup) -> Self {
        // Exhaustive: a new field of `MatchSetup` must be recorded (or come
        // from the packs) and bump `FORMAT_VERSION`.
        let MatchSetup {
            settings,
            sim_mod: _,
            terrain: _,
            holes: _,
            green_mask: _,
            start,
            seed,
            worms,
            pool_capacity,
            bonus_weapons,
        } = setup;
        ReplaySetup {
            packs,
            settings: settings.clone(),
            start: *start,
            seed: *seed,
            worms: worms.clone(),
            pool_capacity: *pool_capacity,
            bonus_weapons: bonus_weapons.clone(),
            calls: Vec::new(),
        }
    }

    /// The [`MatchSetup`] this describes, made of `assets`.
    pub fn match_setup(&self, assets: Assets) -> MatchSetup {
        let Assets { sim_mod, terrain, holes, green_mask } = assets;
        MatchSetup {
            settings: self.settings.clone(),
            sim_mod,
            terrain,
            holes,
            green_mask,
            start: self.start,
            seed: self.seed,
            worms: self.worms.clone(),
            pool_capacity: self.pool_capacity,
            bonus_weapons: self.bonus_weapons.clone(),
        }
    }
}

/// The game mode a match plays, to make it again.
#[derive(Clone, Debug, PartialEq)]
pub enum ModeSpec {
    /// [`OracleMode`]: OLX's crediting with no end (the sandbox).
    Oracle,
    /// [`Deathmatch::new`] with these settings (the setup is recorded after
    /// `Deathmatch::prepare` applied them).
    Deathmatch(DeathmatchSettings),
}

impl ModeSpec {
    /// A new game mode as the match started with.
    pub fn game_mode(&self) -> Box<dyn GameMode> {
        match self {
            ModeSpec::Oracle => Box::new(OracleMode),
            ModeSpec::Deathmatch(s) => Box::new(Deathmatch::new(s.clone())),
        }
    }

    /// Checks the mode's numbers as `Deathmatch::prepare` and
    /// `SimSettings::validate` would: a finite time limit, and lives, kill
    /// limit and `death_decreases_score` in OLX's ranges (applied to
    /// `settings`, the match's). Spree thresholds are only compared with
    /// counts (`Deathmatch::kill`), so any value is harmless.
    pub fn validate(&self, settings: &SimSettings) -> Result<(), String> {
        let ModeSpec::Deathmatch(d) = self else { return Ok(()) };
        if !d.time_limit.is_finite() {
            return Err(alloc::format!("time limit {} is not a number of minutes", d.time_limit));
        }
        let mut s = settings.clone();
        d.apply(&mut s);
        s.validate().map_err(|e| e.0)
    }
}

/// A recorded match.
#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    pub setup: ReplaySetup,
    pub mode: ModeSpec,
    /// `(t, checksum)`: the world's checksum after `t` ticks, every
    /// [`CHECKSUM_EVERY`] ticks and after the last one, in order.
    pub checksums: Vec<(u32, u64)>,
    /// How many ticks it holds (at most [`MAX_TICKS`]).
    ticks: u32,
    /// How many keys a row holds (the widest row recorded).
    width: usize,
    /// The keys of tick `t` are `keys[t * width..(t + 1) * width]`.
    keys: Vec<Keys>,
}

impl Replay {
    /// How many ticks it holds.
    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    /// How many keys each row holds (worms that were given none press nothing).
    pub fn width(&self) -> usize {
        self.width
    }

    /// The keys of tick `t` (0-based), one per worm.
    pub fn row(&self, t: u32) -> &[Keys] {
        let t = t as usize;
        self.keys.get(t * self.width..(t + 1) * self.width).unwrap_or(&[])
    }

    /// The keys of tick `t`, to edit (for tests and tools that bisect).
    pub fn row_mut(&mut self, t: u32) -> &mut [Keys] {
        let t = t as usize;
        self.keys.get_mut(t * self.width..(t + 1) * self.width).unwrap_or(&mut [])
    }
}
