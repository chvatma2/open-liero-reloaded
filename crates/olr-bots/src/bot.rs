//! A bot: what it remembers between ticks and its `think`, once per 100 Hz
//! tick (spec §20 F7), from a read-only [`World`] to the [`Keys`] its worm
//! presses (F3): OLX's `getInput` (`src/common/CWormBot.cpp:1022-1122`) for
//! a Deathmatch bot ([`crate::brain`]), or the chaos personality
//! ([`crate::chaos`]).

use alloc::sync::Arc;

use olr_math::{AbsTime, Vec2};
use olr_sim::{Dir, EventKind, Keys, SimSettings, TickEvents, World, Worm};
use olr_terrain::Terrain;

use crate::aim::{AimState, ShotNoise};
use crate::brain::Mind;
use crate::budget::Budget;
use crate::chaos::Chaos;
use crate::loadout::GameType;
use crate::motor::{Intent, Motor};
use crate::nav::{AreaCache, Follower, Search, SearchStatus};
use crate::rope::RopeMind;
use crate::spot::SpotFinder;
use crate::weapons::{Arsenal, game_type};
use crate::{BotConfig, BotRng, DifficultyParams, Personality};

/// `getInput`'s pause after a spawn (`CWormBot.cpp:1033`).
const SPAWN_PAUSE_MS: u64 = 400;

/// What one tick reads: the world before it ticks and the bot's worm.
pub(crate) struct Ctx<'a> {
    pub w: &'a World,
    pub me: &'a Worm,
    pub now: AbsTime,
    pub s: &'a SimSettings,
    pub t: &'a Terrain,
}

/// What the bot does this tick, as OLX writes it into the worm
/// (`worm_state_t`, `fAngle`, the rope): the motor turns it into keys.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Act {
    /// `bMove`, on the side the bot faces.
    pub mv: bool,
    pub shoot: bool,
    pub jump: bool,
    pub carve: bool,
    pub rope_shoot: bool,
    pub rope_release: bool,
    /// The angle `AI_SetAim` turns towards (`None`: it was not called).
    pub aim: Option<f32>,
}

/// One bot, playing worm `worm`.
#[derive(Clone, Debug)]
pub struct Bot {
    pub(crate) worm: usize,
    pub(crate) cfg: BotConfig,
    pub(crate) params: DifficultyParams,
    pub(crate) rng: BotRng,
    budget: u32,
    units: u32,
    pub(crate) arsenal: Arsenal,
    pub(crate) game_type: GameType,
    pub(crate) search: Search,
    pub(crate) follow: Follower,
    motor: Motor,
    pub(crate) aim: AimState,
    pub(crate) noise: ShotNoise,
    pub(crate) mind: Mind,
    pub(crate) rope: RopeMind,
    pub(crate) spot: SpotFinder,
    pub(crate) chaos: Chaos,
    /// A scripted target ([`Bot::set_goal`]).
    pub(crate) goal: Option<Vec2>,
    /// `fSpawnTime`.
    spawn_time: Option<AbsTime>,
    /// `iCurrentWeapon` as the bot set it (the motor switches to it).
    pub(crate) weapon: usize,
}

/// The least budget a bot gets on a level: the shooting and moving take at
/// most half of a `think`, and the other half must fit the longest worm
/// line (three traces across the map) and a checkpoint of the search.
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
        let me = world.worms().get(worm);
        let m = world.sim_mod();
        Bot {
            worm,
            params: cfg.params(),
            rng: BotRng::new(seed, worm, cfg.personality),
            budget: cfg.budget.max(min_budget(world.terrain())),
            arsenal: Arsenal::new(m),
            game_type: me.map_or(GameType::Other, |w| game_type(m, w, cfg.personality, cfg.loadout)),
            cfg,
            units: 0,
            search: Search::new(cache),
            follow: Follower::new(),
            motor: Motor::new(),
            aim: AimState::default(),
            noise: ShotNoise::default(),
            mind: Mind::new(),
            rope: RopeMind::default(),
            spot: SpotFinder::default(),
            chaos: Chaos::new(m.weapons().len()),
            goal: None,
            spawn_time: me.filter(|w| w.alive).map(|_| world.time()),
            weapon: me.map_or(0, |w| usize::try_from(w.current_weapon).unwrap_or(0)),
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

    /// The tactics the bot plays (from its loadout, as OLX's
    /// `initWeaponSelection` sets them).
    pub fn game_type(&self) -> GameType {
        self.game_type
    }

    /// A scripted target: the bot heads for `goal` and does not shoot (as
    /// OLX's bot does in a race to a flag), or plays Deathmatch again with
    /// `None`.
    pub fn set_goal(&mut self, goal: Option<Vec2>) {
        self.goal = goal.filter(|g| g.x.is_finite() && g.y.is_finite());
        self.mind.state = crate::brain::State::Think;
    }

    pub fn goal(&self) -> Option<Vec2> {
        self.goal
    }

    /// The weapon ids this bot has fired (a chaos bot tracks them for
    /// coverage, spec §20 F10; other bots too), in id order.
    pub fn fired_weapons(&self) -> impl Iterator<Item = i32> + '_ {
        self.chaos.fired()
    }

    /// One tick: reads `world` (before it ticks) and the events of its last
    /// tick, and gives the keys the worm holds in the next.
    pub fn think(&mut self, world: &World, events: &TickEvents) -> Keys {
        let now = world.time();
        let mut b = Budget::new(self.budget);
        for e in events.iter() {
            match e.kind {
                EventKind::Spawned { worm } if usize::try_from(worm) == Ok(self.worm) => {
                    self.spawn_time = Some(now);
                    self.on_respawn(world, &mut b);
                }
                EventKind::WeaponFired { worm, weapon } if usize::try_from(worm) == Ok(self.worm) => {
                    self.chaos.record(weapon);
                }
                _ => {}
            }
        }
        let keys = match world.worms().get(self.worm) {
            None => Keys(0),
            Some(me) => {
                let paused = self.spawn_time.is_some_and(|t| now.0.saturating_sub(t.0) < SPAWN_PAUSE_MS);
                let c = Ctx { w: world, me, now, s: world.settings(), t: world.terrain() };
                let intent = if me.alive && !paused && world.result().is_none() {
                    let act = match self.cfg.personality {
                        Personality::Deathmatch => self.get_input(&c, &mut b),
                        Personality::Chaos => self.chaos_input(&c, &mut b),
                    };
                    self.intent(me, &act)
                } else {
                    // The search goes on meanwhile (OLX's thread).
                    self.step_search(&c, &mut b);
                    Intent::default()
                };
                self.motor.keys(me, world.settings(), &intent)
            }
        };
        self.units = b.used();
        keys
    }

    /// The motor's intent for `act`: walk on the side the bot faces (OLX
    /// sets `iMoveDirectionSide = iFaceDirectionSide`), hold the bot's
    /// weapon, and fire only once the worm holds it.
    fn intent(&self, me: &Worm, act: &Act) -> Intent {
        let face: Dir = self.aim.face(me);
        let holds = me.current_weapon == self.weapon as i32;
        Intent {
            aim: act.aim,
            face: Some(face),
            walk: act.mv.then_some(face),
            jump: act.jump,
            fire: act.shoot && (holds || self.cfg.personality == Personality::Chaos),
            carve: act.carve,
            rope_shoot: act.rope_shoot,
            rope_release: act.rope_release,
            weapon: Some(self.weapon),
        }
    }

    /// `onRespawn` (`CWormBot.cpp:4120-4126`, `993-997`).
    fn on_respawn(&mut self, world: &World, b: &mut Budget) {
        self.aim.reset();
        self.rope = RopeMind::default();
        self.spot = SpotFinder::default();
        if let Some(me) = world.worms().get(self.worm) {
            self.weapon = usize::try_from(me.current_weapon).unwrap_or(0);
        }
        match self.cfg.personality {
            Personality::Deathmatch => {
                let c = world.worms().get(self.worm).map(|me| Ctx {
                    w: world,
                    me,
                    now: world.time(),
                    s: world.settings(),
                    t: world.terrain(),
                });
                if let Some(c) = c {
                    self.respawned(&c, b);
                }
            }
            Personality::Chaos => self.chaos.respawned(),
        }
    }
}
