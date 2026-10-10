//! A bot: what it remembers between ticks and its `think`, once per 100 Hz
//! tick (spec §20 F7), from a read-only [`World`] to the [`Keys`] its worm
//! presses (F3).
//!
//! The brain here is a placeholder that walks to a point ([`Bot::set_goal`]):
//! the frame of OLX's `getInput` (`src/common/CWormBot.cpp:1022-1122`:
//! respawn, the 0.4 s spawn pause, the carve timer) around the pathfinding
//! (`AI_CreatePath`) and the path following (`AI_MoveToTarget`'s node
//! advance, aim and jumps). Plan 6 task 5 replaces it with OLX's brain.

use alloc::sync::Arc;

use rl_math::{AbsTime, Vec2};
use rl_sim::{EventKind, Keys, SimSettings, TickEvents, World, Worm};
use rl_terrain::Terrain;

use crate::aim::{AimState, olx_direct};
use crate::budget::Budget;
use crate::motor::{Intent, Motor};
use crate::nav::{AreaCache, Follower, Search, SearchStatus, jump_loses_node};
use crate::{BotConfig, BotRng, DifficultyParams};

/// `getInput`'s pause after a spawn (`CWormBot.cpp:1033`).
const SPAWN_PAUSE_MS: u64 = 400;
/// `AI_Think`'s interval (`think_interval`, Deathmatch).
const THINK_MS: u64 = 3000;
/// `AI_CreatePath` restarts a search that has run this long.
const SEARCH_RESTART_MS: u64 = 5000;
/// `AI_Carve`'s interval.
const CARVE_MS: u64 = 200;
/// `AI_Jump`'s interval.
const JUMP_MS: u64 = 300;

/// One bot, playing worm `worm`.
#[derive(Clone, Debug)]
pub struct Bot {
    worm: usize,
    cfg: BotConfig,
    params: DifficultyParams,
    rng: BotRng,
    budget: u32,
    units: u32,
    search: Search,
    follow: Follower,
    motor: Motor,
    aim: AimState,
    goal: Option<Vec2>,
    /// The goal the running or last search was for.
    planned: Option<(i32, i32)>,
    /// `fLastThink`.
    last_think: Option<AbsTime>,
    /// `fSearchStartTime`.
    search_start: AbsTime,
    /// `fSpawnTime`.
    spawn_time: Option<AbsTime>,
    /// `fLastCarve`.
    last_carve: Option<AbsTime>,
    /// `fLastJump`.
    last_jump: Option<AbsTime>,
}

/// The least budget a bot gets on a level: following a path takes at most
/// half of a `think`, and the other half must fit the longest worm line
/// (three traces across the map) and a checkpoint.
pub fn min_budget(terrain: &Terrain) -> u32 {
    let longest = terrain.width().max(terrain.height()).saturating_add(2);
    longest.saturating_mul(3).saturating_add(54).saturating_mul(2)
}

impl Bot {
    /// A bot for worm `worm` in a match seeded `seed`, with an area cache
    /// of its own.
    pub fn new(worm: usize, cfg: BotConfig, seed: u32, world: &World) -> Bot {
        Self::with_cache(worm, cfg, seed, world, AreaCache::new(world.terrain()))
    }

    /// A bot sharing `cache` with the other bots of its match (a cache for
    /// another level is replaced by one of its own).
    pub fn with_cache(worm: usize, cfg: BotConfig, seed: u32, world: &World, cache: Arc<AreaCache>) -> Bot {
        let cache = if cache.fits(world.terrain()) { cache } else { AreaCache::new(world.terrain()) };
        let alive = world.worms().get(worm).is_some_and(|w| w.alive);
        Bot {
            worm,
            params: cfg.params(),
            rng: BotRng::new(seed, worm, cfg.personality),
            budget: cfg.budget.max(min_budget(world.terrain())),
            cfg,
            units: 0,
            search: Search::new(cache),
            follow: Follower::new(),
            motor: Motor::new(),
            aim: AimState::default(),
            goal: None,
            planned: None,
            last_think: None,
            search_start: AbsTime(0),
            spawn_time: alive.then(|| world.time()),
            last_carve: None,
            last_jump: None,
        }
    }

    pub fn worm(&self) -> usize {
        self.worm
    }

    pub fn config(&self) -> &BotConfig {
        &self.cfg
    }

    /// The work units one `think` may spend.
    pub fn budget(&self) -> u32 {
        self.budget
    }

    /// The work units the last `think` spent (the test hook of notes §4.8).
    pub fn units_used(&self) -> u32 {
        self.units
    }

    pub fn cache(&self) -> &Arc<AreaCache> {
        self.search.cache()
    }

    /// The path being followed.
    pub fn path(&self) -> &[Vec2] {
        self.follow.nodes()
    }

    pub fn search_status(&self) -> SearchStatus {
        self.search.status()
    }

    /// Where the placeholder brain walks to.
    pub fn set_goal(&mut self, goal: Option<Vec2>) {
        self.goal = goal;
    }

    pub fn goal(&self) -> Option<Vec2> {
        self.goal
    }

    /// One tick: reads `world` (before it ticks) and the events of its last
    /// tick, and gives the keys the worm holds in the next.
    pub fn think(&mut self, world: &World, events: &TickEvents) -> Keys {
        let now = world.time();
        for e in events.iter() {
            if let EventKind::Spawned { worm } = e.kind
                && usize::try_from(worm) == Ok(self.worm)
            {
                self.spawn_time = Some(now);
                self.follow.clear();
                self.search.cancel();
                self.last_think = None;
            }
        }
        let mut b = Budget::new(self.budget);
        let keys = match world.worms().get(self.worm) {
            None => Keys(0),
            Some(w) => {
                let paused = self.spawn_time.is_some_and(|t| now.0.saturating_sub(t.0) < SPAWN_PAUSE_MS);
                let intent =
                    if w.alive && !paused { self.go_to_goal(world, w, &mut b) } else { Intent::default() };
                self.motor.keys(w, world.settings(), &intent)
            }
        };
        self.units = b.used();
        keys
    }

    /// The placeholder brain: follow the path to the goal, or walk at the
    /// goal while there is none (`AI_SimpleMove`).
    fn go_to_goal(&mut self, world: &World, w: &Worm, b: &mut Budget) -> Intent {
        let Some(goal) = self.goal else { return Intent::default() };
        let (now, t, s) = (world.time(), world.terrain(), world.settings());
        // Following and steering first, with at most half the budget; the
        // search gets the rest (a path found now is followed from the next tick).
        let mut part = b.split(b.left() / 2);
        self.follow.advance(w.pos, t, &mut part, now);
        let node = self.follow.current().unwrap_or(goal);
        let intent = self.steer(w, node, now, s, t, &mut part);
        b.absorb(part);
        self.plan(w, goal, now);
        match self.search.step(t, b) {
            SearchStatus::Found => {
                self.follow.set(self.search.path());
                self.search.cancel();
            }
            SearchStatus::Failed(_) => self.search.cancel(),
            SearchStatus::Idle | SearchStatus::Pending => {}
        }
        intent
    }

    /// `AI_Think`'s rhythm for paths (`AI_CreatePath`, `CWormBot.cpp:2869-2955`):
    /// a new search every 3 s or when the goal moves; a running search is
    /// restarted after 5 s.
    fn plan(&mut self, w: &Worm, goal: Vec2, now: AbsTime) {
        let from = (w.pos.x as i32, w.pos.y as i32);
        let to = (goal.x as i32, goal.y as i32);
        let since = |t: AbsTime| now.0.saturating_sub(t.0);
        let restart = match self.search.status() {
            SearchStatus::Pending => {
                since(self.search_start) >= SEARCH_RESTART_MS || self.planned != Some(to)
            }
            _ => self.last_think.is_none_or(|t| since(t) > THINK_MS) || self.planned != Some(to),
        };
        if restart {
            self.search.start(from, to);
            self.planned = Some(to);
            self.last_think = Some(now);
            self.search_start = now;
        }
    }

    /// Heads for `node` (`AI_MoveToTarget`, `CWormBot.cpp:3640-3942`, in
    /// part): aim at it (`AI_SetAim`), walk while more than 3 px away
    /// sideways (`bMove`, the facing side), carve every 0.2 s (`AI_Carve`),
    /// and jump when it is more than 10 px above (`AI_Jump`; OLX only jumps
    /// up to 30 px and ropes higher, which task 5 adds).
    fn steer(
        &mut self,
        w: &Worm,
        node: Vec2,
        now: AbsTime,
        s: &SimSettings,
        t: &Terrain,
        b: &mut Budget,
    ) -> Intent {
        let aim = self.aim.set_aim(w, olx_direct(w.pos, node), now, &self.params, &mut self.rng, s);
        let mut intent = Intent { aim: Some(aim.angle), face: Some(aim.face), ..Intent::default() };
        if libm::fabsf(w.pos.x - node.x) > 3.0 {
            intent.walk = Some(aim.face);
        }
        let since = |t: Option<AbsTime>| t.map_or(u64::MAX, |t| now.0.saturating_sub(t.0));
        if since(self.last_carve) > CARVE_MS {
            self.last_carve = Some(now);
            intent.carve = true;
        }
        if w.pos.y - node.y > 10.0
            && since(self.last_jump) > JUMP_MS
            && (w.on_ground || w.can_air_jump)
            && jump_loses_node(node, w.pos, s, t, b) == Some(false)
        {
            self.last_jump = Some(now);
            intent.jump = true;
        }
        intent
    }
}
