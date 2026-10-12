//! OLX's Deathmatch (`CDeathMatch`, `src/server/CDeathMatch.cpp:19-26`),
//! which is the bare `CGameMode` (`src/server/CGameMode.cpp`): its kill
//! bookkeeping and messages, `CheckGameOver`, `Winner` and the results
//! order. Notes: `docs/porting/olx-notes/bots-and-deathmatch.md` §3.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt;

use rl_sim::{
    EndReason, Fnv, GameMode, KillCredit, MAX_WORMS, MatchResult, MatchSetup, Message, ModeCtx,
    ModeStateError, Score, SimSettings, TextId, World, Worm, olx_kill_credit, olx_score,
};
use serde::{Deserialize, Serialize};

use crate::respawn::RespawnLoadout;
use crate::rules::{self, WRM_OUT, WRM_UNLIM};
use crate::text;

/// The Deathmatch rules: OLX's game settings with their defaults
/// (`src/common/FeatureList.cpp:31-33`, `110-118`) and the spree
/// thresholds of its options (`src/client/Options.cpp:243-253`).
///
/// The scoring rules and the lives are the simulation's too
/// (`SimSettings`, which counts kills and lives): [`Deathmatch::prepare`]
/// copies them into the match's settings ([`DeathmatchSettings::apply`]).
#[derive(Clone, Debug, PartialEq)]
pub struct DeathmatchSettings {
    /// `Lives` (-2): below 0 unlimited; `N` lets a worm die `N + 1` times.
    pub lives: i32,
    /// `KillLimit` (15): the match ends when a score reaches it; 0 or less is off.
    pub kill_limit: i32,
    /// `TimeLimit` (6), in minutes: the match ends once the server time
    /// passes it; 0 or less is off.
    pub time_limit: f32,
    /// `AllowNegativeScore` (false): deaths and suicides count at a score of 0 or less.
    pub allow_negative_score: bool,
    /// `DeathDecreasesScore` (0): each death takes this much off the score.
    pub death_decreases_score: f32,
    /// `SuicideDecreasesScore` (false): each suicide takes one off the score.
    pub suicide_decreases_score: bool,
    /// `Game.SpreeThreshold1..5` (3, 5, 7, 9, 10): kills in a row for
    /// [`text::SPREE`]`[i]`.
    pub sprees: [i32; 5],
    /// `Game.DyingSpreeThreshold1..5` (3, 5, 7, 9, 10): deaths in a row
    /// for [`text::DYING_SPREE`]`[i]`.
    pub dying_sprees: [i32; 5],
    /// What a worm holds when it spawns ([`RespawnLoadout::Keep`]: OLX).
    pub respawn_loadout: RespawnLoadout,
}

impl Default for DeathmatchSettings {
    fn default() -> Self {
        Self {
            lives: -2,
            kill_limit: 15,
            time_limit: 6.0,
            allow_negative_score: false,
            death_decreases_score: 0.0,
            suicide_decreases_score: false,
            sprees: [3, 5, 7, 9, 10],
            dying_sprees: [3, 5, 7, 9, 10],
            respawn_loadout: RespawnLoadout::Keep,
        }
    }
}

impl DeathmatchSettings {
    /// Writes the rules the simulation applies (lives, kill limit and
    /// scoring) into `s`.
    pub fn apply(&self, s: &mut SimSettings) {
        s.lives = self.lives;
        s.kill_limit = self.kill_limit;
        s.allow_negative_score = self.allow_negative_score;
        s.death_decreases_score = self.death_decreases_score;
        s.suicide_decreases_score = self.suicide_decreases_score;
    }

    /// Whether `s` holds these rules (what [`DeathmatchSettings::apply`]
    /// writes), bit for bit: a NaN agrees with nothing.
    pub fn agrees_with(&self, s: &SimSettings) -> bool {
        s.lives == self.lives
            && s.kill_limit == self.kill_limit
            && s.allow_negative_score == self.allow_negative_score
            && s.death_decreases_score == self.death_decreases_score
            && s.suicide_decreases_score == self.suicide_decreases_score
    }

    /// `CGameMode::TimeLimit()` (`CGameMode.cpp:31-33`, minutes · 60 in
    /// `float`) as `TimeDiff(float)` milliseconds (`(Uint64)(s · 1000)`),
    /// or `None` when it is off (`TimeLimit() > 0` fails).
    fn time_limit_ms(&self) -> Option<u64> {
        let secs = self.time_limit * 60.0;
        (secs > 0.0).then_some((secs * 1000.0) as u64)
    }
}

/// Why [`Deathmatch::prepare`] refused a match.
#[derive(Clone, Debug, PartialEq)]
pub enum DeathmatchError {
    /// A Deathmatch needs two worms (spec §20 F5).
    TooFewWorms(usize),
    /// The time limit is NaN or infinite.
    TimeLimit(f32),
    /// The match's settings do not hold these rules ([`Deathmatch::resume`]).
    Disagrees,
}

impl fmt::Display for DeathmatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeathmatchError::TooFewWorms(n) => write!(f, "a Deathmatch needs 2 worms, not {n}"),
            DeathmatchError::TimeLimit(t) => write!(f, "time limit {t} is not a number of minutes"),
            DeathmatchError::Disagrees => {
                write!(f, "the match's settings disagree with the Deathmatch's rules")
            }
        }
    }
}

/// The messages of one kill: at most a kill or suicide, first blood, a
/// spree, a dying spree and "out".
#[derive(Default)]
pub(crate) struct Said {
    list: [Option<Message>; 5],
}

impl Said {
    fn push(&mut self, text: TextId, args: [i32; 3]) {
        if let Some(slot) = self.list.iter_mut().find(|m| m.is_none()) {
            *slot = Some(Message { text, args });
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = Message> + '_ {
        self.list.iter().flatten().copied()
    }
}

/// How a match ends (`CheckGameOver`), and the worm that hit the kill limit.
struct End {
    result: MatchResult,
    hit_by: i32,
}

/// OLX's Deathmatch as a [`GameMode`].
///
/// - **Kills** are credited by [`rl_sim::olx_kill_credit`] (exactly what
///   [`rl_sim::OracleMode`] does). There are no teams, so no teamkills
///   (`GameTeams() == 1`).
/// - **The end** is `CGameMode::CheckGameOver` (`CGameMode.cpp:276-360`)
///   in [`GameMode::is_over`], every frame: OLX's `recheckGame` is set by
///   `BeginMatch` (`src/server/CServer.cpp:521`) and only cleared by
///   `GameServer::Clear`, so the check runs every frame of a match and a
///   kill that reaches the limit ends the match in its own frame. The kill
///   limit (the first worm by id with a score at it), then the time limit
///   (`serverFrame · 10 > TimeLimit`), then fewer than two worms not out
///   (one in a single-worm local game; a local game never allows empty
///   games). The winner is `CGameMode::Winner`.
/// - **Messages** ([`crate::text`]) are queued where OLX sends its texts:
///   the kill's in `on_kill`, the end's in the `on_tick` of the frame that
///   ended (`is_over` cannot queue them; the mode checks again with the
///   same worms and frame).
/// - **The scoreboard** ([`GameMode::scores`]) is refreshed after each
///   spawn and at the end of each frame, best first (`rules::sort`).
#[derive(Debug)]
pub struct Deathmatch {
    settings: DeathmatchSettings,
    /// `bFirstBlood`.
    first_blood: bool,
    /// `iKillsInRow`, by worm id.
    kills_in_row: [i32; MAX_WORMS],
    /// `iDeathsInRow`, by worm id.
    deaths_in_row: [i32; MAX_WORMS],
    /// How many times each worm has spawned, by worm id (the chaos
    /// rotation's lives).
    spawns: [u32; MAX_WORMS],
    /// `game.serverFrame` as of the last `on_tick` (the frames played).
    frame: u64,
    /// How the match ended, once this mode announced it.
    over: Option<MatchResult>,
    /// `frame` when `over` was set: the clock [`Deathmatch::time_left_ms`]
    /// stops at. Not in the checksum (it would change every recorded
    /// replay's): it is fixed together with `over`, which is.
    ended_at: u64,
    /// The scoreboard; its capacity is `MAX_WORMS`, so it never grows.
    board: Vec<Score>,
}

/// What a Deathmatch transfers with its world (`GameMode::save_state`):
/// every field but the settings, which the receiving mode was built with.
/// The chaos rotation is settings only (each worm's life is `spawns`).
#[derive(Serialize, Deserialize)]
struct DeathmatchState {
    first_blood: bool,
    kills_in_row: [i32; MAX_WORMS],
    deaths_in_row: [i32; MAX_WORMS],
    spawns: [u32; MAX_WORMS],
    frame: u64,
    over: Option<MatchResult>,
    ended_at: u64,
    /// Last, so tests can append another board.
    board: Vec<Score>,
}

impl Clone for Deathmatch {
    fn clone(&self) -> Self {
        let mut board = Vec::with_capacity(MAX_WORMS);
        board.extend_from_slice(&self.board);
        Self { board, settings: self.settings.clone(), ..*self }
    }
}

impl Deathmatch {
    /// A Deathmatch with `settings` (`CGameMode::PrepareGame`,
    /// `CGameMode.cpp:41-51`). Private: the match's `SimSettings` must
    /// agree with it, which [`Deathmatch::prepare`] and
    /// [`Deathmatch::resume`] see to (one source of rules).
    pub(crate) fn new(settings: DeathmatchSettings) -> Self {
        Self {
            settings,
            first_blood: true,
            kills_in_row: [0; MAX_WORMS],
            deaths_in_row: [0; MAX_WORMS],
            spawns: [0; MAX_WORMS],
            frame: 0,
            over: None,
            ended_at: 0,
            board: Vec::with_capacity(MAX_WORMS),
        }
    }

    /// Checks `setup` for a Deathmatch (two worms, spec §20 F5; a time
    /// limit that is a number), applies the rules to its settings
    /// ([`DeathmatchSettings::apply`]) and gives every worm the lives the
    /// game starts with (`Lives < 0 ? WRM_UNLIM : Lives`, `Game.cpp:851`).
    pub fn prepare(settings: DeathmatchSettings, setup: &mut MatchSetup) -> Result<Self, DeathmatchError> {
        if setup.worms.len() < 2 {
            return Err(DeathmatchError::TooFewWorms(setup.worms.len()));
        }
        if !settings.time_limit.is_finite() {
            return Err(DeathmatchError::TimeLimit(settings.time_limit));
        }
        settings.apply(&mut setup.settings);
        let lives = if settings.lives < 0 { WRM_UNLIM } else { settings.lives };
        for w in &mut setup.worms {
            w.loadout.lives = lives;
        }
        Ok(Self::new(settings))
    }

    /// A Deathmatch for a match whose settings already hold its rules: a
    /// replay's setup (recorded after [`Deathmatch::prepare`]) or an
    /// oracle trace's (whose worms carry the lives they had). Refuses a
    /// time limit that is not a number and `settings` that disagree with
    /// the rules ([`DeathmatchSettings::agrees_with`]), so the mode and
    /// the simulation never play by two sets of rules. The worms' lives
    /// are left as they are.
    pub fn resume(settings: DeathmatchSettings, sim: &SimSettings) -> Result<Self, DeathmatchError> {
        if !settings.time_limit.is_finite() {
            return Err(DeathmatchError::TimeLimit(settings.time_limit));
        }
        if !settings.agrees_with(sim) {
            return Err(DeathmatchError::Disagrees);
        }
        Ok(Self::new(settings))
    }

    pub fn settings(&self) -> &DeathmatchSettings {
        &self.settings
    }

    /// How the match ended, once it has (as `World::result`).
    pub fn result(&self) -> Option<MatchResult> {
        self.over
    }

    /// The time left before the time limit, in ms (0 once it passed), or
    /// `None` without one. It stops at the frame the match ended in.
    pub fn time_left_ms(&self) -> Option<u64> {
        let limit = self.settings.time_limit_ms()?;
        let frame = if self.over.is_some() { self.ended_at } else { self.frame };
        Some(limit.saturating_sub(frame.saturating_mul(10)))
    }

    /// `CGameMode::CompareWormsScore` with this match's lives and kill
    /// limit ([`crate::compare`]).
    pub fn compare(&self, a: &Score, b: &Score) -> i32 {
        rules::compare(a, b, self.settings.lives, self.settings.kill_limit)
    }

    /// `CGameMode::Kill` (`CGameMode.cpp:69-206`) for `killer` killing
    /// `victim` (ids; -1 for nobody), from the worms before the kill: the
    /// credit, the rows of kills and deaths, and the messages.
    pub(crate) fn kill(
        &mut self,
        worms: &[Worm],
        s: &SimSettings,
        victim: i32,
        killer: i32,
    ) -> (KillCredit, Said) {
        let credit = olx_kill_credit(worms, s, victim, killer);
        let mut said = Said::default();
        let Some(v) = worms.iter().find(|w| w.id == victim) else { return (credit, said) };
        let has_killer = worms.iter().any(|w| w.id == killer);
        let other = has_killer && killer != victim;
        if has_killer {
            if killer == victim {
                said.push(text::SUICIDE, [victim, 0, 0]);
            } else {
                // `getLastWeaponHitBy` is unknown: " with <weapon>" is dropped.
                said.push(text::KILLED, [killer, victim, -1]);
            }
        }
        if self.first_blood && other {
            self.first_blood = false;
            said.push(text::FIRST_BLOOD, [killer, 0, 0]);
        }
        let slot = |id: i32| usize::try_from(id).ok().filter(|&i| i < MAX_WORMS);
        if other && let Some(k) = slot(killer) {
            self.kills_in_row[k] = self.kills_in_row[k].wrapping_add(1);
            self.deaths_in_row[k] = 0;
            if let Some(i) = self.settings.sprees.iter().position(|&t| t == self.kills_in_row[k]) {
                said.push(text::SPREE[i], [killer, 0, 0]);
            }
        }
        if let Some(i) = slot(victim) {
            self.kills_in_row[i] = 0;
            self.deaths_in_row[i] = self.deaths_in_row[i].wrapping_add(1);
            if let Some(t) = self.settings.dying_sprees.iter().position(|&t| t == self.deaths_in_row[i]) {
                said.push(text::DYING_SPREE[t], [victim, 0, 0]);
            }
        }
        // `CWorm::Kill(true)` takes a life unless they are unlimited.
        if v.lives != WRM_UNLIM && v.lives.wrapping_sub(1) == WRM_OUT {
            said.push(text::PLAYER_OUT, [victim, 0, 0]);
        }
        (credit, said)
    }

    /// `CGameMode::CheckGameOver` (`CGameMode.cpp:276-360`, without teams)
    /// at server frame `server_frame`, with `Winner` for the result.
    fn check(&self, worms: &[Worm], s: &SimSettings, server_frame: u64) -> Option<End> {
        let (lives, kill_limit) = (self.settings.lives, self.settings.kill_limit);
        let end = |reason, hit_by| {
            let winner = rules::winner(worms, s, lives, kill_limit);
            Some(End { result: MatchResult { winner, reason }, hit_by })
        };
        if kill_limit > 0
            && let Some(w) = worms.iter().find(|w| olx_score(w, s) >= kill_limit)
        {
            return end(EndReason::KillLimit, w.id);
        }
        // `game.serverTime()` is `serverFrame * 10` ms (`Game.h:111`).
        if let Some(limit) = self.settings.time_limit_ms()
            && server_frame.saturating_mul(10) > limit
        {
            return end(EndReason::TimeLimit, -1);
        }
        let in_game = worms.iter().filter(|w| w.lives != WRM_OUT).count();
        let needed = if worms.len() == 1 { 1 } else { 2 };
        if in_game < needed {
            return end(EndReason::LastStanding, -1);
        }
        None
    }

    /// The scoreboard from `worms`, best first.
    fn refresh(&mut self, worms: &[Worm], s: &SimSettings) {
        self.board.clear();
        self.board.extend(worms.iter().take(MAX_WORMS).map(|w| rules::row(w, s)));
        rules::sort(&mut self.board, self.settings.lives, self.settings.kill_limit);
    }
}

impl GameMode for Deathmatch {
    fn box_clone(&self) -> Box<dyn GameMode> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn checksum(&self, h: &mut Fnv) {
        // `ended_at` is left out: it is set with `over`, which is hashed.
        let Deathmatch {
            settings,
            first_blood,
            kills_in_row,
            deaths_in_row,
            spawns,
            frame,
            over,
            ended_at: _,
            board,
        } = self;
        let DeathmatchSettings {
            lives,
            kill_limit,
            time_limit,
            allow_negative_score,
            death_decreases_score,
            suicide_decreases_score,
            sprees,
            dying_sprees,
            respawn_loadout,
        } = settings;
        h.i32(*lives);
        h.i32(*kill_limit);
        h.f32(*time_limit);
        h.bool(*allow_negative_score);
        h.f32(*death_decreases_score);
        h.bool(*suicide_decreases_score);
        sprees.iter().chain(dying_sprees).chain(kills_in_row).chain(deaths_in_row).for_each(|&n| h.i32(n));
        respawn_loadout.hash(h);
        spawns.iter().for_each(|&n| h.u32(n));
        h.bool(*first_blood);
        h.u64(*frame);
        match over {
            None => h.i32(-2),
            Some(r) => {
                h.i32(r.winner);
                let (tag, n) = match r.reason {
                    EndReason::KillLimit => (0, 0),
                    EndReason::TimeLimit => (1, 0),
                    EndReason::LastStanding => (2, 0),
                    EndReason::Other(n) => (3, n),
                };
                h.u32(tag);
                h.u32(n as u32);
            }
        }
        h.usize(board.len());
        for r in board {
            let Score { worm, score, lives, kills, deaths, suicides, damage } = *r;
            [worm, score, lives, kills, deaths, suicides].into_iter().for_each(|n| h.i32(n));
            h.f32(damage);
        }
    }

    /// Refreshes the scoreboard; with [`RespawnLoadout::ChaosCycle`] a
    /// chaos bot gets its life's weapons (`give_weapon`, applied after this
    /// tick's `on_tick`, or at the end of the setup for the first spawn).
    fn on_spawn(&mut self, worm: i32, ctx: &mut ModeCtx) {
        if let Some(n) = usize::try_from(worm).ok().and_then(|i| self.spawns.get_mut(i)) {
            if let RespawnLoadout::ChaosCycle(cycle) = self.settings.respawn_loadout
                && let Some(weapons) = cycle.weapons_for(worm, *n)
            {
                for (slot, weapon) in weapons.into_iter().enumerate() {
                    // A full command queue keeps the old weapon, never fails.
                    let _ = ctx.give_weapon(worm, slot, weapon);
                }
            }
            *n = n.wrapping_add(1);
        }
        self.refresh(ctx.worms(), ctx.settings());
    }

    fn on_kill(&mut self, victim: i32, killer: i32, ctx: &mut ModeCtx) -> KillCredit {
        let (credit, said) = self.kill(ctx.worms(), ctx.settings(), victim, killer);
        for m in said.iter() {
            // A full command queue drops the message, never the kill.
            let _ = ctx.message(m);
        }
        credit
    }

    fn is_over(&self, world: &World) -> Option<MatchResult> {
        self.check(world.worms(), world.settings(), world.server_frame()).map(|e| e.result)
    }

    /// `CGameMode::Simulate` plays only sounds; this sends the end's texts
    /// (`CheckGameOver`'s, then `GameServer::GameOver`'s "has won") in the
    /// frame that ended, and refreshes the scoreboard.
    fn on_tick(&mut self, ctx: &mut ModeCtx) {
        self.frame = self.frame.wrapping_add(1);
        if self.over.is_none()
            && let Some(end) = self.check(ctx.worms(), ctx.settings(), self.frame)
        {
            self.over = Some(end.result);
            self.ended_at = self.frame;
            let _ = match end.result.reason {
                EndReason::KillLimit => ctx.message(Message {
                    text: text::KILL_LIMIT,
                    args: [self.settings.kill_limit, end.hit_by, 0],
                }),
                EndReason::TimeLimit => ctx.message(Message { text: text::TIME_LIMIT, args: [0; 3] }),
                _ => Ok(()),
            };
            if end.result.winner >= 0 {
                let _ = ctx.message(Message { text: text::PLAYER_HAS_WON, args: [end.result.winner, 0, 0] });
            }
        }
        self.refresh(ctx.worms(), ctx.settings());
    }

    fn scores(&self, _world: &World) -> &[Score] {
        &self.board
    }

    fn save_state(&self) -> Vec<u8> {
        let Deathmatch {
            settings: _,
            first_blood,
            kills_in_row,
            deaths_in_row,
            spawns,
            frame,
            over,
            ended_at,
            board,
        } = self;
        let s = DeathmatchState {
            first_blood: *first_blood,
            kills_in_row: *kills_in_row,
            deaths_in_row: *deaths_in_row,
            spawns: *spawns,
            frame: *frame,
            over: *over,
            ended_at: *ended_at,
            board: board.clone(),
        };
        postcard::to_allocvec(&s).expect("postcard encodes every Deathmatch state")
    }

    /// Refuses a board longer than [`MAX_WORMS`] (its capacity) or with a
    /// worm id outside `0..MAX_WORMS`, a winner neither -1 nor such an id,
    /// and bytes left over.
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), ModeStateError> {
        let s: DeathmatchState = match postcard::take_from_bytes(bytes) {
            Ok((s, [])) => s,
            _ => return Err(ModeStateError::Decode),
        };
        let worm = |id: i32| (0..MAX_WORMS as i32).contains(&id);
        let winner_ok = s.over.is_none_or(|r| r.winner == -1 || worm(r.winner));
        if s.board.len() > MAX_WORMS || !s.board.iter().all(|r| worm(r.worm)) || !winner_ok {
            return Err(ModeStateError::Invalid);
        }
        let DeathmatchState {
            first_blood,
            kills_in_row,
            deaths_in_row,
            spawns,
            frame,
            over,
            ended_at,
            board,
        } = s;
        (self.first_blood, self.kills_in_row, self.deaths_in_row, self.spawns) =
            (first_blood, kills_in_row, deaths_in_row, spawns);
        (self.frame, self.over, self.ended_at) = (frame, over, ended_at);
        // `clear` + `extend` keeps the board's capacity: callbacks never allocate.
        self.board.clear();
        self.board.extend_from_slice(&board);
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
