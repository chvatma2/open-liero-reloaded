//! The world's side of game modes: the mode, its command queue, the match's
//! end (`GameServer::GameOver`, `src/server/CServer.cpp:532-625`) and the
//! commands' effects.

use alloc::boxed::Box;

use rl_math::TimeDiff;

use crate::checksum::Fnv;
use crate::effects::carve_px;
use crate::events::{EventKind, StampId, TerrainOp};
use crate::mode::{Command, Commands, GameMode, MatchResult, ModeCtx, StampOp};
use crate::setup::MAX_SLOTS;
use crate::world::{FRAME_MS, MAX_WORMS, World};
use crate::worm::{Loadout, Slot};

/// OLX `GAMEOVER_WAIT` (`include/Consts.h:47`), in seconds: the physics
/// stops this long after the end (`Game::shouldDoPhysicsFrame`).
const GAMEOVER_WAIT: f32 = 3.0;

/// The game mode and what the world keeps for it.
#[derive(Clone, Debug)]
pub(crate) struct Referee {
    pub mode: Box<dyn GameMode>,
    pub commands: Commands,
    /// `game.gameOver`, with how the match ended.
    pub over: Option<MatchResult>,
    /// `game.gameOverFrame`.
    pub over_frame: u64,
    /// `S2C_GAMEOVER` waits for the local client (spec §18 D10).
    pub game_over_sent: bool,
    /// The worms withdrawn from the match ([`World::withdraw`]), bit `i`
    /// for worm `i`: hashed only when not 0, so a match nobody left hashes
    /// as before.
    pub gone: u32,
}

impl PartialEq for Referee {
    /// Two modes are equal when their states hash alike.
    fn eq(&self, other: &Self) -> bool {
        let hash = |r: &Referee| {
            let mut h = Fnv::new();
            r.mode.checksum(&mut h);
            h.finish()
        };
        hash(self) == hash(other)
            && (self.commands == other.commands)
            && (self.over, self.over_frame, self.game_over_sent, self.gone)
                == (other.over, other.over_frame, other.game_over_sent, other.gone)
    }
}

impl Referee {
    pub fn new(mode: Box<dyn GameMode>, num_weapons: usize, num_holes: usize) -> Self {
        Self {
            mode,
            commands: Commands::new(num_weapons.min(i32::MAX as usize) as i32, num_holes),
            over: None,
            over_frame: 0,
            game_over_sent: false,
            gone: 0,
        }
    }

    /// [`crate::OracleMode`] for a mod without weapons or holes, for tests.
    #[cfg(test)]
    pub fn oracle() -> Self {
        Self::new(Box::new(crate::OracleMode), 0, 0)
    }

    /// A context for commands outside a callback, for tests.
    #[cfg(test)]
    pub fn ctx<'a>(
        &'a mut self,
        worms: &'a [crate::Worm],
        settings: &'a crate::SimSettings,
        now: rl_math::AbsTime,
    ) -> ModeCtx<'a> {
        ModeCtx { worms, settings, now, gone: self.gone, commands: &mut self.commands }
    }

    /// `Game::gameOverTime().seconds() > GAMEOVER_WAIT`
    /// (`src/game/Game.cpp:1272`): the physics no longer runs. `seconds()`
    /// is `ms * 0.001f` (`include/olx-types.h:40`), so 3000 ms (3.0000002 s)
    /// is already past the wait: the physics stops 300 frames after the end.
    pub fn frozen(&self, server_frame: u64) -> bool {
        frozen(self.over.is_some(), self.over_frame, server_frame)
    }

    pub fn hash(&self, h: &mut Fnv) {
        let Referee { mode, commands, over, over_frame, game_over_sent, gone } = self;
        mode.checksum(h);
        // Applied within the tick that queues them, so empty between ticks.
        h.usize(commands.list.len());
        match over {
            None => h.bool(false),
            Some(r) => {
                h.bool(true);
                h.i32(r.winner);
                let (tag, n) = match r.reason {
                    crate::EndReason::KillLimit => (0, 0),
                    crate::EndReason::TimeLimit => (1, 0),
                    crate::EndReason::LastStanding => (2, 0),
                    crate::EndReason::Other(n) => (3, n),
                };
                h.bytes(&[tag]);
                h.bytes(&n.to_le_bytes());
            }
        }
        h.u64(*over_frame);
        h.bool(*game_over_sent);
        // Only once someone left: a match nobody left (local play, the
        // match goldens, the oracle) hashes as it did before the mask.
        if *gone != 0 {
            h.u32(*gone);
        }
    }
}

/// [`Referee::frozen`] for a match that is `over` (or not) since `over_frame`.
pub(crate) fn frozen(over: bool, over_frame: u64, server_frame: u64) -> bool {
    let ms = server_frame.saturating_sub(over_frame).saturating_mul(FRAME_MS);
    over && server_frame > over_frame && ms as f32 * 0.001 > GAMEOVER_WAIT
}

/// `CWorm::clearInput` (`src/common/CWorm.cpp:493-501`): the state, the
/// input time and (for a local worm) the key latches.
pub(crate) fn clear_input(w: &mut World) {
    let now = w.time;
    for (worm, latch) in w.worms.iter_mut().zip(w.latches.iter_mut()) {
        worm.last_input_time = now;
        worm.state = Default::default();
        latch.reset();
    }
}

impl World {
    /// `CGameMode::CheckGameOver`, then `GameServer::GameOver`, at the start
    /// of `GameServer::SimulateGame` (`CServer_Game.cpp:169-176`).
    pub(crate) fn check_game_over(&mut self) {
        if self.referee.over.is_some() {
            return;
        }
        // The mode reads the whole world, itself included (`World::scores`).
        if let Some(r) = self.referee.mode.is_over(self) {
            self.game_over(r);
        }
    }

    /// `GameServer::GameOver` (`src/server/CServer.cpp:532-625`): once, it
    /// records the result and the frame, reports it, and clears every
    /// worm's input. The local client clears its projectiles when it parses
    /// the message, at the next delivery (`ParseGameOver`,
    /// `src/client/CClient_Parse.cpp:1436-1451`).
    pub(crate) fn game_over(&mut self, result: MatchResult) {
        if self.referee.over.is_some() {
            return;
        }
        self.referee.over = Some(result);
        self.referee.over_frame = self.server_frame;
        self.referee.game_over_sent = true;
        self.draws.events.push(EventKind::GameOver(result));
        clear_input(self);
    }

    /// The client parses `S2C_GAMEOVER` (`ParseGameOver`): `cProjectiles.clear()`.
    pub(crate) fn parse_game_over(&mut self) {
        if core::mem::take(&mut self.referee.game_over_sent) {
            self.projectiles.clear();
        }
    }

    /// While the physics is stopped after the end (`CClient::Simulation`,
    /// `src/client/CClient_Game.cpp:83-91`): input cleared, projectiles'
    /// clocks moved by the frame's real time (`skipProjectiles`), and the
    /// physics clock set to the frame clock.
    pub(crate) fn skip_physics(&mut self) {
        clear_input(self);
        self.projectiles.skip(TimeDiff(FRAME_MS));
        self.physics_time = self.time;
    }

    /// `on_tick`, then the queued commands in order (also those queued by
    /// the callbacks during the tick), then the queue is emptied.
    pub(crate) fn mode_tick(&mut self) {
        let now = self.time;
        let r = &mut self.referee;
        r.mode.on_tick(&mut ModeCtx {
            worms: &self.worms,
            settings: &self.settings,
            now,
            gone: r.gone,
            commands: &mut r.commands,
        });
        self.apply_commands();
    }

    /// Applies the queued commands in order. A spawn's `on_spawn` may queue
    /// more, which run in the same pass; the queue's capacity bounds it.
    pub(crate) fn apply_commands(&mut self) {
        let mut i = 0;
        while i < self.referee.commands.list.len() {
            let c = self.referee.commands.list[i];
            self.apply(c);
            i += 1;
        }
        self.referee.commands.list.clear();
    }

    fn apply(&mut self, c: Command) {
        match c {
            Command::Spawn { team, slots, len, current_weapon, lives, kills, deaths, at } => {
                // `GameServer::SpawnWorm` returns while `gameOver` (`CServer_Game.cpp:40`).
                if self.worms.len() >= MAX_WORMS || self.referee.over.is_some() {
                    return;
                }
                let Some(mut vec) = self.spare.take() else { return };
                vec.extend_from_slice(&slots[..(len as usize).min(MAX_SLOTS)]);
                let l = Loadout { slots: vec, current_weapon, lives, kills, deaths };
                self.add_worm(team, l, at);
            }
            Command::GiveWeapon { worm, slot, weapon } => {
                let Some(w) = self.worms.iter_mut().find(|w| w.id == worm) else { return };
                if let Some(s) = w.slots.get_mut(slot as usize) {
                    *s = Slot::ready(weapon);
                }
            }
            Command::Stamp { op, x, y } => {
                let ev = &mut self.draws.events;
                match op {
                    StampOp::Carve { hole } => {
                        carve_px(&mut self.terrain, &self.holes, ev, TerrainOp::Carve, hole as usize, x, y);
                    }
                    StampOp::Dirt { hole } => {
                        let Some(stamp) = self.holes.get(hole as usize) else { return };
                        ev.edit(TerrainOp::Dirt, StampId::Hole(hole), x, y, (stamp.width(), stamp.height()));
                        self.terrain.place_dirt(stamp, x, y);
                    }
                }
            }
            Command::Message(m) => {
                self.draws.events.push(EventKind::Message(m));
            }
            Command::End(r) => self.game_over(r),
        }
    }
}
