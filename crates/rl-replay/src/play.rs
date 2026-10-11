//! Rebuilding a replay's world and replaying it, checking the checksums.

use alloc::sync::Arc;
use alloc::vec::Vec;

use rl_sim::{SimMod, TickEvents, World};
use rl_terrain::{GreenMask, Stamp, Terrain};

use crate::{Mismatch, Replay, ReplayError, SetupCall};

/// The content a replay's packs resolve to: what `rl_content::match_setup`
/// takes from the mod, the level and its theme.
#[derive(Clone, Debug)]
pub struct Assets {
    pub sim_mod: Arc<SimMod>,
    pub terrain: Terrain,
    pub holes: Vec<Stamp>,
    pub green_mask: Option<GreenMask>,
}

/// Makes setup call number `index` on `world`; gives `find_spot`'s position.
pub(crate) fn apply_call(
    world: &mut World,
    index: usize,
    call: SetupCall,
) -> Result<Option<(i32, i32)>, ReplayError> {
    match call {
        SetupCall::SetHealth { worm, health } => {
            if (worm as usize) >= world.worms().len() {
                return Err(ReplayError::NoWorm { index, worm });
            }
            if !health.is_finite() {
                return Err(ReplayError::Health { index });
            }
            world.set_health(worm as usize, health);
        }
        SetupCall::PlaceBonus { x, y, item } => {
            world.place_bonus(x, y, item).map_err(|error| ReplayError::Bonus { index, error })?;
        }
        SetupCall::FindSpot => return Ok(Some(world.find_spot())),
        SetupCall::AlignPhysicsClock => world.align_physics_clock(),
    }
    Ok(None)
}

/// A replay played to its end.
#[derive(Clone, Debug)]
pub struct ReplayRun {
    /// The world after the last tick.
    pub world: World,
    /// How many ticks were played.
    pub ticks: u32,
    /// How many recorded checksums were checked (all of them).
    pub checked: usize,
}

impl Replay {
    /// The world the match started from, made of `assets` (which must be
    /// the content of [`crate::ReplaySetup::packs`]: `rl-content` checks the
    /// hashes), with the setup calls made.
    pub fn world(&self, assets: Assets) -> Result<World, ReplayError> {
        let (w, h) = (assets.terrain.width(), assets.terrain.height());
        for (worm, s) in self.setup.worms.iter().enumerate() {
            if let Some(at @ (x, y)) = s.spawn
                && !(u32::try_from(x).is_ok_and(|x| x < w) && u32::try_from(y).is_ok_and(|y| y < h))
            {
                return Err(ReplayError::Spawn { worm, at });
            }
        }
        self.mode.validate(&self.setup.settings).map_err(ReplayError::Mode)?;
        let mut world =
            World::new(self.setup.match_setup(assets), self.mode.game_mode()).map_err(ReplayError::Setup)?;
        for (i, &call) in self.setup.calls.iter().enumerate() {
            apply_call(&mut world, i, call)?;
        }
        Ok(world)
    }

    /// Plays every tick on `world` (from [`Replay::world`]), checking each
    /// recorded checksum; the first that differs is the error.
    pub fn play(&self, mut world: World) -> Result<ReplayRun, Mismatch> {
        let mut out = TickEvents::new();
        let mut player = self.player();
        while player.step(&mut world, &mut out)? {}
        Ok(ReplayRun { world, ticks: player.tick(), checked: player.checked() })
    }

    /// A player that steps through the replay one tick at a time.
    pub fn player(&self) -> Player<'_> {
        Player { replay: self, tick: 0, next: 0 }
    }
}

/// Plays a replay a tick at a time (to stop at a tick, or to draw it).
#[derive(Clone, Debug)]
pub struct Player<'r> {
    replay: &'r Replay,
    tick: u32,
    /// The next recorded checksum to check.
    next: usize,
}

impl Player<'_> {
    /// How many ticks were played.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// Whether every tick was played.
    pub fn is_done(&self) -> bool {
        self.tick >= self.replay.ticks()
    }

    /// How many recorded checksums were checked.
    pub fn checked(&self) -> usize {
        self.next
    }

    /// Plays the next tick on `world` (the replay's world, played by this
    /// player only) and checks the checksum recorded for it; gives whether
    /// a tick was played (false at the end). A [`Mismatch`] is reported
    /// once; stepping on is allowed.
    pub fn step(&mut self, world: &mut World, out: &mut TickEvents) -> Result<bool, Mismatch> {
        if self.tick == 0 {
            self.verify(world)?;
        }
        if self.is_done() {
            return Ok(false);
        }
        world.tick(self.replay.row(self.tick), out);
        self.tick += 1;
        self.verify(world)?;
        Ok(true)
    }

    /// Checks the checksums recorded up to the current tick.
    fn verify(&mut self, world: &World) -> Result<(), Mismatch> {
        while let Some(&(t, expected)) = self.replay.checksums.get(self.next)
            && t <= self.tick
        {
            self.next += 1;
            if t == self.tick {
                let actual = world.checksum();
                if actual != expected {
                    return Err(Mismatch { tick: t, expected, actual });
                }
            }
        }
        Ok(())
    }
}
