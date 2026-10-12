//! [`MatchSetup`] and [`World::new`]: everything a match starts from (spec
//! §5.1), checked before the world exists.

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;

use rl_math::{AbsTime, GlibcRand, Mt19937, TimeDiff};
use rl_terrain::{GreenMask, Stamp, Terrain};

use crate::bonus::Bonuses;
use crate::buf::Buf;
use crate::mod_rt::SimMod;
use crate::mode::{GameMode, ModeCtx};
use crate::projectile::ProjectilePool;
use crate::referee::Referee;
use crate::rng::Draws;
use crate::settings::{SettingsError, SimSettings};
use crate::shots::{MAX_SHOOTINGS, ShootList};
use crate::world::{CLOCK_OFFSET_MS, MAX_WORMS, Scratch, World};
use crate::worm::{Loadout, Slot};

/// OLX's `WeaponSlotsNum` (`src/common/FeatureList.cpp:172`, fixed at 5):
/// the most weapon slots a worm has.
pub const MAX_SLOTS: usize = 5;

/// One worm of a [`MatchSetup`], or of a mode's spawn command.
#[derive(Clone, Debug, PartialEq)]
pub struct WormSetup {
    /// Its team (Deathmatch has none: every worm's is 0).
    pub team: u8,
    /// Its weapons (at most [`MAX_SLOTS`]; a weapon id outside the mod never
    /// fires) and counters.
    pub loadout: Loadout,
    /// Where it spawns; `None` draws a position with the world's `FindSpot`
    /// ([`World::find_spot`]) when it spawns, as OLX's server does.
    pub spawn: Option<(i32, i32)>,
}

/// Everything a match starts from: the mod, the level, the settings, the
/// seed and the worms (spec §5.1). [`MatchSetup::new`] gives the defaults.
#[derive(Clone, Debug)]
pub struct MatchSetup {
    pub settings: SimSettings,
    /// The mod, compiled to OLX's indices ([`SimMod`]).
    pub sim_mod: Arc<SimMod>,
    pub terrain: Terrain,
    /// The theme's hole stamps (`hole1` to `hole5`). Like OLX, a missing
    /// hole carves nothing.
    pub holes: Vec<Stamp>,
    /// The green-dirt mask (`data/gfx/greenball.png`); without it green
    /// dirt places nothing, as in OLX.
    pub green_mask: Option<GreenMask>,
    /// Where the frame clock starts: [`MatchSetup::T0`] by default (spec §20 F9).
    pub start: AbsTime,
    /// The match seed (`simSetSeed`): it seeds the `rand()` stream and the
    /// `FindSpot` `mt19937`.
    pub seed: u32,
    /// Spawned in order at the start, worm `i` with id `i`.
    pub worms: Vec<WormSetup>,
    /// The projectile pool's size: OLX's 3000 by default (spec §5.3), from
    /// 1 to [`MatchSetup::MAX_POOL_CAPACITY`].
    pub pool_capacity: usize,
    /// The weapons a spawned bonus may hold (`goodWpns`): the ids the weapon
    /// restrictions do not ban. `None` allows every weapon, as for a mod
    /// missing from the restriction file (`CWpnRest::updateList`). Ids
    /// outside the mod are left out.
    pub bonus_weapons: Option<Vec<i32>>,
}

impl MatchSetup {
    /// Where matches start: `simulationTime` at 100 000 ms, as the oracle
    /// restarts it (spec §20 F9). Not 0: OLX treats time 0 as "never" (a
    /// worm's last bleeding, a bonus's spawn time).
    pub const T0: AbsTime = AbsTime(100_000);
    /// OLX's `MAX_PROJECTILES` (`include/CProjectile.h:36`).
    pub const DEFAULT_POOL_CAPACITY: usize = ProjectilePool::DEFAULT_CAPACITY;
    /// The largest pool: 64 Ki slots (about 13 MB).
    pub const MAX_POOL_CAPACITY: usize = 1 << 16;

    /// A match of `sim_mod` on `terrain` with `settings`, no holes, green
    /// mask or worms, seed 0, starting at [`MatchSetup::T0`], OLX's pool size
    /// and every weapon allowed in bonuses.
    pub fn new(settings: SimSettings, sim_mod: Arc<SimMod>, terrain: Terrain) -> Self {
        Self {
            settings,
            sim_mod,
            terrain,
            holes: Vec::new(),
            green_mask: None,
            start: Self::T0,
            seed: 0,
            worms: Vec::new(),
            pool_capacity: Self::DEFAULT_POOL_CAPACITY,
            bonus_weapons: None,
        }
    }

    /// Checks what the simulation cannot run with: settings outside OLX's
    /// ranges ([`SimSettings::validate`]), more than [`MAX_WORMS`] worms, a
    /// pool size outside `1..=MAX_POOL_CAPACITY`, a loadout of more than
    /// [`MAX_SLOTS`] slots.
    pub fn validate(&self) -> Result<(), SetupError> {
        self.settings.validate().map_err(SetupError::Settings)?;
        if self.worms.len() > MAX_WORMS {
            return Err(SetupError::TooManyWorms(self.worms.len()));
        }
        if !(1..=Self::MAX_POOL_CAPACITY).contains(&self.pool_capacity) {
            return Err(SetupError::PoolCapacity(self.pool_capacity));
        }
        if let Some((worm, w)) =
            self.worms.iter().enumerate().find(|(_, w)| w.loadout.slots.len() > MAX_SLOTS)
        {
            return Err(SetupError::TooManySlots { worm, slots: w.loadout.slots.len() });
        }
        Ok(())
    }
}

/// Why [`World::new`] refused a [`MatchSetup`].
#[derive(Clone, Debug, PartialEq)]
pub enum SetupError {
    Settings(SettingsError),
    TooManyWorms(usize),
    PoolCapacity(usize),
    TooManySlots { worm: usize, slots: usize },
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupError::Settings(e) => write!(f, "settings: {}", e.0),
            SetupError::TooManyWorms(n) => write!(f, "{n} worms, at most {MAX_WORMS}"),
            SetupError::PoolCapacity(n) => {
                write!(f, "a pool of {n} projectiles, from 1 to {}", MatchSetup::MAX_POOL_CAPACITY)
            }
            SetupError::TooManySlots { worm, slots } => {
                write!(f, "worm {worm}: {slots} weapon slots, at most {MAX_SLOTS}")
            }
        }
    }
}

/// Slot vectors for the worms a mode may spawn mid-match, made at setup so
/// a spawn does not allocate. Not state: always equal, and a clone makes
/// as many again.
#[derive(Debug, Default)]
pub(crate) struct Spare(Vec<Vec<Slot>>);

impl Spare {
    pub(crate) fn new(n: usize) -> Self {
        let mut v = Vec::with_capacity(n);
        v.resize_with(n, || Vec::with_capacity(MAX_SLOTS));
        Spare(v)
    }

    pub fn take(&mut self) -> Option<Vec<Slot>> {
        self.0.pop()
    }
}

impl Clone for Spare {
    fn clone(&self) -> Self {
        Spare::new(self.0.len())
    }
}

impl PartialEq for Spare {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl World {
    /// The world `setup` describes, playing `mode`: the terrain with the
    /// worms spawned in order (each at its position or a `FindSpot` one,
    /// carving its spawn hole, with its loadout; `mode.on_spawn` for each),
    /// both clocks at `setup.start`, the generators seeded with
    /// `setup.seed`. The commands `on_spawn` queued are applied before it
    /// returns. Spawn events come with the first tick's. Fails if
    /// [`MatchSetup::validate`] does.
    pub fn new(setup: MatchSetup, mode: Box<dyn GameMode>) -> Result<World, SetupError> {
        setup.validate()?;
        Ok(Self::build(setup, mode))
    }

    /// [`World::new`] without the checks (tests build odd worlds with it).
    pub(crate) fn build(setup: MatchSetup, mode: Box<dyn GameMode>) -> World {
        let MatchSetup {
            settings,
            sim_mod,
            terrain,
            holes,
            green_mask,
            start,
            seed,
            worms,
            pool_capacity,
            bonus_weapons,
        } = setup;
        let terrain_min = terrain.width().min(terrain.height());
        let bonuses = Bonuses::new(sim_mod.weapons().len(), start + TimeDiff(CLOCK_OFFSET_MS));
        let draws = Draws::new(GlibcRand::new(seed), Mt19937::new(seed), sim_mod.projectiles().len());
        let referee = Referee::new(mode, sim_mod.weapons().len(), holes.len());
        let mut world = World {
            settings,
            sim_mod,
            terrain,
            holes,
            green_mask,
            worms: Buf::with_capacity(MAX_WORMS),
            latches: Buf::with_capacity(MAX_WORMS),
            time: start,
            physics_time: start,
            server_frame: 0,
            substeps: 0,
            shots: ShootList::new(),
            last_shots: Buf::with_capacity(MAX_SHOOTINGS),
            projectiles: ProjectilePool::new(pool_capacity),
            draws,
            bonuses,
            referee,
            others: Scratch {
                others: Buf::with_capacity(MAX_WORMS),
                beam_parts: Buf::with_capacity(terrain_min as usize),
            },
            spare: Spare::new(MAX_WORMS.saturating_sub(worms.len())),
            setup_worms: 0,
        };
        if let Some(ids) = bonus_weapons {
            world.set_bonus_weapons(&ids);
        }
        for w in worms {
            world.add_worm(w.team, w.loadout, w.spawn);
        }
        world.apply_commands();
        world.setup_worms = world.worms.len();
        world
    }

    /// Spawns a worm (at `at` or a `FindSpot` position) with `loadout`
    /// (moved in) and `team`, then tells the mode. Returns its index, or
    /// `None` if the world has [`MAX_WORMS`] worms.
    pub(crate) fn add_worm(&mut self, team: u8, loadout: Loadout, at: Option<(i32, i32)>) -> Option<usize> {
        if self.worms.len() >= MAX_WORMS {
            return None;
        }
        let (x, y) = at.unwrap_or_else(|| self.find_spot());
        let i = self.spawn_worm(x, y)?;
        let w = &mut self.worms[i];
        w.team = team;
        w.slots = loadout.slots;
        w.current_weapon = loadout.current_weapon;
        (w.lives, w.kills, w.deaths) = (loadout.lives, loadout.kills, loadout.deaths);
        let id = w.id;
        let now = self.time;
        let r = &mut self.referee;
        r.mode.on_spawn(
            id,
            &mut ModeCtx { worms: &self.worms, settings: &self.settings, now, commands: &mut r.commands },
        );
        Some(i)
    }
}
