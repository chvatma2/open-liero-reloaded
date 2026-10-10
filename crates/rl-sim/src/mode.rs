//! Game modes (spec §5.8, hook H1 of §12.1): the [`GameMode`] trait the
//! world calls back, the commands a mode gives through [`ModeCtx`], and
//! [`OracleMode`], OLX's bare `CGameMode` crediting with no end, which the
//! oracle scenarios and the sandbox play.
//!
//! The world owns the mode (`World::new`) and calls it where OLX calls its
//! `CGameMode` or where the event happens:
//! - `on_kill` inside `GameServer::killWorm`, in place of `CGameMode::Kill`;
//! - `on_damage` at the end of `CClient::InjureWorm`'s health change;
//! - `on_pickup` after `CWorm::GiveBonus` took a bonus;
//! - `on_spawn` after a worm (re)spawned (`GameServer::SpawnWorm`, which asks
//!   `CGameMode::Spawn` first; Deathmatch's always agrees);
//! - `is_over` at the start of `GameServer::SimulateGame`
//!   (`CGameMode::CheckGameOver`), every frame until the match ends;
//! - `on_tick` at the end of it (`CGameMode::Simulate`).
//!
//! Commands are queued (at most [`COMMAND_CAPACITY`] a tick, checked when
//! queued) and applied in queue order at one point of each tick: right after
//! `on_tick`, before the frame clock advances. Commands queued during setup
//! (by `on_spawn` of the starting worms) are applied at the end of `World::new`.

use alloc::boxed::Box;
use core::fmt;

use rl_math::AbsTime;

use crate::bonus::BonusItem;
use crate::checksum::Fnv;
use crate::setup::{MAX_SLOTS, WormSetup};
use crate::worm::{Slot, Worm};
use crate::{SimSettings, World};

/// The most commands a tick (or the setup) can queue.
pub const COMMAND_CAPACITY: usize = 128;

/// A game mode: Deathmatch (`rl-modes`), [`OracleMode`], or a scenario's.
///
/// It must be deterministic and must not allocate in its callbacks (they run
/// inside `World::tick`). `Clone` is [`GameMode::box_clone`], so the world
/// stays `Clone`; its state is hashed into `World::checksum` by
/// [`GameMode::checksum`].
pub trait GameMode: fmt::Debug + Send + Sync {
    /// A copy of the mode, for `World::clone`.
    fn box_clone(&self) -> Box<dyn GameMode>;

    /// Hashes every part of the mode's state that can change during a
    /// match, as `World::checksum` does the world's.
    fn checksum(&self, h: &mut Fnv);

    /// Worm `worm` (an id) spawned or respawned, at the setup, by the
    /// server's respawn loop or by a [`ModeCtx::spawn_worm`] command.
    fn on_spawn(&mut self, worm: i32, ctx: &mut ModeCtx) {
        let _ = (worm, ctx);
    }

    /// `victim` is killed by `killer` (an id; -1 for nobody, `victim` for a
    /// suicide), in place of OLX's `CGameMode::Kill`: says what to count.
    /// The world then kills the victim (`CWorm::Kill`), taking a life
    /// unless they are unlimited. [`crate::olx_kill_credit`] is OLX's rule.
    fn on_kill(&mut self, victim: i32, killer: i32, ctx: &mut ModeCtx) -> KillCredit;

    /// `victim` lost health to `by` (-1 for nobody); `dealt` is what OLX
    /// credits to the attacker (`MIN(health before, damage)`), negative for healing.
    fn on_damage(&mut self, victim: i32, by: i32, dealt: f32, ctx: &mut ModeCtx) {
        let _ = (victim, by, dealt, ctx);
    }

    /// Worm `worm` picked up a bonus holding `item`.
    fn on_pickup(&mut self, worm: i32, item: BonusItem, ctx: &mut ModeCtx) {
        let _ = (worm, item, ctx);
    }

    /// Once a tick, at the end of the server's frame.
    fn on_tick(&mut self, ctx: &mut ModeCtx) {
        let _ = ctx;
    }

    /// Whether the match is over, and how (`CGameMode::CheckGameOver` and
    /// `Winner`). Asked once a tick until it is.
    fn is_over(&self, world: &World) -> Option<MatchResult> {
        let _ = world;
        None
    }

    /// The scoreboard, best first (empty for a mode without one).
    fn scores(&self, world: &World) -> &[Score] {
        let _ = world;
        &[]
    }
}

impl Clone for Box<dyn GameMode> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// What a kill counts (`CGameMode::Kill` with `CWorm::addKill`,
/// `addSuicide` and `addDeath`). The world applies it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KillCredit {
    /// The killer's kills go up by one.
    pub kill: bool,
    /// The killer's suicides go up by one (killer and victim are one worm).
    pub suicide: bool,
    /// The victim's deaths go up by one.
    pub death: bool,
}

/// Why a match ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// A worm's score reached `KillLimit`.
    KillLimit,
    /// The time limit passed.
    TimeLimit,
    /// Fewer than two worms have lives left.
    LastStanding,
    /// A mode's own reason.
    Other(u16),
}

/// How a match ended: the winner's id (-1 for none) and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchResult {
    pub winner: i32,
    pub reason: EndReason,
}

/// One row of a mode's scoreboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Score {
    pub worm: i32,
    pub score: i32,
    pub lives: i32,
    pub kills: i32,
    pub deaths: i32,
    pub suicides: i32,
    /// `fDamage`.
    pub damage: f32,
}

/// A message's text, by an id the mode defines (such as "`<killer>` killed
/// `<victim>`"); the HUD turns it into words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextId(pub u16);

/// A message for the players: a text and up to three numbers it names
/// (worm ids, a weapon id, a count), whose meaning the text gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub text: TextId,
    pub args: [i32; 3],
}

/// A terrain stamp a mode applies with [`ModeCtx::stamp`]: the theme's
/// hole `hole` (`0..=4`, `hole1` to `hole5`), carved (`CMap::CarveHole`)
/// or placed as dirt (`CMap::PlaceDirt`). It counts for nobody's dirt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StampOp {
    Carve { hole: u8 },
    Dirt { hole: u8 },
}

/// Why a command was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandError {
    /// [`COMMAND_CAPACITY`] commands are queued already.
    Full,
    /// No such weapon id in the mod.
    NoSuchWeapon(i32),
    /// A slot index of [`MAX_SLOTS`] or more.
    NoSuchSlot(usize),
    /// No such hole in the theme.
    NoSuchHole(u8),
    /// A loadout of more than [`MAX_SLOTS`] slots.
    TooManySlots(usize),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::Full => write!(f, "{COMMAND_CAPACITY} commands are queued already"),
            CommandError::NoSuchWeapon(id) => write!(f, "no weapon {id}"),
            CommandError::NoSuchSlot(i) => write!(f, "no slot {i} (at most {MAX_SLOTS})"),
            CommandError::NoSuchHole(h) => write!(f, "no hole {h}"),
            CommandError::TooManySlots(n) => write!(f, "{n} slots, at most {MAX_SLOTS}"),
        }
    }
}

/// A queued command; a spawn's loadout is copied in, so queuing allocates nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Command {
    Spawn {
        team: u8,
        slots: [Slot; MAX_SLOTS],
        len: u8,
        current_weapon: i32,
        lives: i32,
        kills: i32,
        deaths: i32,
        at: Option<(i32, i32)>,
    },
    GiveWeapon {
        worm: i32,
        slot: u8,
        weapon: i32,
    },
    Stamp {
        op: StampOp,
        x: i32,
        y: i32,
    },
    Message(Message),
    End(MatchResult),
}

/// The command queue, with what it checks commands against.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Commands {
    pub list: crate::buf::Buf<Command>,
    pub num_weapons: i32,
    pub num_holes: usize,
}

impl Commands {
    pub fn new(num_weapons: i32, num_holes: usize) -> Self {
        Self { list: crate::buf::Buf::with_capacity(COMMAND_CAPACITY), num_weapons, num_holes }
    }

    fn push(&mut self, c: Command) -> Result<(), CommandError> {
        if self.list.len() >= COMMAND_CAPACITY {
            return Err(CommandError::Full);
        }
        self.list.push(c);
        Ok(())
    }
}

/// What a mode's callback reads (the worms, the settings, the frame clock)
/// and the commands it gives (hook H1).
pub struct ModeCtx<'a> {
    pub(crate) worms: &'a [Worm],
    pub(crate) settings: &'a SimSettings,
    pub(crate) now: AbsTime,
    pub(crate) commands: &'a mut Commands,
}

impl ModeCtx<'_> {
    /// The worms as they are now (in a kill, before it counts).
    pub fn worms(&self) -> &[Worm] {
        self.worms
    }

    pub fn settings(&self) -> &SimSettings {
        self.settings
    }

    /// The frame clock (`GetPhysicsTime()`).
    pub fn time(&self) -> AbsTime {
        self.now
    }

    /// Spawns a worm with `setup`'s team and loadout, at its position or
    /// at a `FindSpot` one. Skipped when the world has `MAX_WORMS` worms.
    pub fn spawn_worm(&mut self, setup: &WormSetup) -> Result<(), CommandError> {
        let l = &setup.loadout;
        if l.slots.len() > MAX_SLOTS {
            return Err(CommandError::TooManySlots(l.slots.len()));
        }
        let mut slots = [Slot::ready(-1); MAX_SLOTS];
        slots[..l.slots.len()].copy_from_slice(&l.slots);
        self.commands.push(Command::Spawn {
            team: setup.team,
            slots,
            len: l.slots.len() as u8,
            current_weapon: l.current_weapon,
            lives: l.lives,
            kills: l.kills,
            deaths: l.deaths,
            at: setup.spawn,
        })
    }

    /// Puts `weapon` in worm `worm`'s slot `slot`, full and ready
    /// ([`Slot::ready`]: unlike a weapon bonus, `CWorm::GiveBonus`, the
    /// slot's `LastFire` is reset too, so the weapon can fire at once).
    /// Skipped if the worm or the slot does not exist then.
    pub fn give_weapon(&mut self, worm: i32, slot: usize, weapon: i32) -> Result<(), CommandError> {
        if !(0..self.commands.num_weapons).contains(&weapon) {
            return Err(CommandError::NoSuchWeapon(weapon));
        }
        if slot >= MAX_SLOTS {
            return Err(CommandError::NoSuchSlot(slot));
        }
        self.commands.push(Command::GiveWeapon { worm, slot: slot as u8, weapon })
    }

    /// Applies a terrain stamp at `(x, y)`, reported as a `TerrainEdit`.
    pub fn stamp(&mut self, op: StampOp, x: i32, y: i32) -> Result<(), CommandError> {
        let (StampOp::Carve { hole } | StampOp::Dirt { hole }) = op;
        if hole as usize >= self.commands.num_holes {
            return Err(CommandError::NoSuchHole(hole));
        }
        self.commands.push(Command::Stamp { op, x, y })
    }

    /// Emits `EventKind::Message`.
    pub fn message(&mut self, m: Message) -> Result<(), CommandError> {
        self.commands.push(Command::Message(m))
    }

    /// Ends the match with `result` (OLX's `GameServer::GameOver`), unless
    /// it is over already.
    pub fn end(&mut self, result: MatchResult) -> Result<(), CommandError> {
        self.commands.push(Command::End(result))
    }
}

/// OLX's bare `CGameMode` crediting ([`crate::olx_kill_credit`]), with no
/// end, no scoreboard and no commands: what `rl-sim` hard-coded before game
/// modes, which the oracle scenarios and the sandbox play. (The oracle's
/// dedicated server allows empty games with unlimited lives, and no scenario
/// reaches the kill or time limit, so OLX never ends a recorded match.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OracleMode;

impl GameMode for OracleMode {
    fn box_clone(&self) -> Box<dyn GameMode> {
        Box::new(*self)
    }

    fn checksum(&self, _: &mut Fnv) {}

    fn on_kill(&mut self, victim: i32, killer: i32, ctx: &mut ModeCtx) -> KillCredit {
        crate::damage::olx_kill_credit(ctx.worms(), ctx.settings(), victim, killer)
    }
}

/// A placeholder while the world lends itself to [`GameMode::is_over`]: a
/// zero-sized mode, so boxing it does not allocate.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Vacant;

impl GameMode for Vacant {
    fn box_clone(&self) -> Box<dyn GameMode> {
        Box::new(Vacant)
    }

    fn checksum(&self, _: &mut Fnv) {}

    fn on_kill(&mut self, _: i32, _: i32, _: &mut ModeCtx) -> KillCredit {
        KillCredit::default()
    }
}

#[cfg(test)]
#[path = "mode_tests.rs"]
mod tests;
