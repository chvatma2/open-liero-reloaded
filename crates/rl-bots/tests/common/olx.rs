//! Matches on OLX's content (`RL_CONTENT_DIR`; tests skip with a notice
//! without it): Deathmatch worlds with bots, static targets and the events
//! the tests count.

use std::path::Path;
use std::sync::Arc;

use rl_bots::{AreaCache, Bot, BotConfig, BotRng, choose_loadout};
use rl_content::{Content, DEFAULT_PRESET, SandboxSetup, SpawnSpec, match_setup};
use rl_modes::{Deathmatch, DeathmatchSettings};
use rl_sim::{EventKind, Keys, SimSettings, TickEvents, World};

pub const CLASSIC: &str = "olx-classic";
pub const MW: &str = "olx-mw-1-0";
pub const DIRT_LEVEL: &str = "olx-level-dirt-level";

/// The imported content, or `None` (with a notice) without it.
pub fn content() -> Option<Content> {
    let Some(dir) = std::env::var("RL_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(Path::new(&dir)).expect("the content directory"))
}

/// Who plays a worm: a bot, or a target that never moves (it only asks to
/// respawn when dead).
#[derive(Clone, Copy, Debug)]
pub enum Player {
    Bot(BotConfig),
    Target,
}

/// A Deathmatch on OLX content, the bots sharing one area cache.
pub struct Match {
    pub w: World,
    pub bots: Vec<Option<Bot>>,
    pub out: TickEvents,
    pub ticks: u32,
    /// `(tick, kind)` of every gameplay event the tests count.
    pub log: Vec<(u32, EventKind)>,
}

/// A Deathmatch of `players` on `level` with mod `mod_id`, seeded `seed`
/// (spawns drawn by `FindSpot`); `tweak` may change the settings first.
pub fn deathmatch(
    c: &Content,
    mod_id: &str,
    level_id: &str,
    seed: u32,
    players: &[Player],
    dm: DeathmatchSettings,
    tweak: impl FnOnce(&mut SimSettings),
) -> Match {
    let m = c.load_mod(mod_id).unwrap();
    let level = c.load_level(level_id).unwrap();
    let theme = c.load_theme(&level.theme).unwrap();
    let mut settings = SimSettings::lx56();
    tweak(&mut settings);
    let sandbox = SandboxSetup {
        mod_id: mod_id.into(),
        level_id: level_id.into(),
        seed,
        settings: settings.clone(),
        worms: players.iter().map(|_| SpawnSpec::default()).collect(),
        restrictions: DEFAULT_PRESET.into(),
    };
    let mut setup = match_setup(&m, &level, &theme, &sandbox).unwrap();
    let rules = m.weapon_rules(DEFAULT_PRESET);
    for (i, p) in players.iter().enumerate() {
        if let Player::Bot(cfg) = p {
            let mut rng = BotRng::new(seed, i, cfg.personality);
            setup.worms[i].loadout = choose_loadout(cfg, &rules, &settings, &mut rng);
        }
    }
    let mode = Deathmatch::prepare(dm, &mut setup).unwrap();
    Match::new(World::new(setup, Box::new(mode)).unwrap(), players, seed)
}

impl Match {
    /// `players` in world `w`, the bots sharing one area cache.
    pub fn new(w: World, players: &[Player], seed: u32) -> Match {
        let cache = AreaCache::new(w.terrain());
        let bots = players
            .iter()
            .enumerate()
            .map(|(i, p)| match p {
                Player::Bot(cfg) => Some(Bot::with_cache(i, *cfg, seed, &w, Arc::clone(&cache))),
                Player::Target => None,
            })
            .collect();
        Match { w, bots, out: TickEvents::new(), ticks: 0, log: Vec::new() }
    }

    /// One tick: every bot thinks (in worm order), targets ask to respawn.
    pub fn tick(&mut self) -> Vec<Keys> {
        let jump = if self.ticks.is_multiple_of(2) { Keys(Keys::JUMP) } else { Keys(0) };
        let keys: Vec<Keys> = (0..self.bots.len())
            .map(|i| match &mut self.bots[i] {
                Some(b) => {
                    let k = b.think(&self.w, &self.out);
                    assert!(
                        b.units_used() <= b.budget(),
                        "bot {i}: {} units of {}",
                        b.units_used(),
                        b.budget()
                    );
                    k
                }
                None if !self.w.worms()[i].alive => jump,
                None => Keys(0),
            })
            .collect();
        self.w.tick(&keys, &mut self.out);
        for e in self.out.iter() {
            if matches!(
                e.kind,
                EventKind::Kill { .. }
                    | EventKind::Damage { .. }
                    | EventKind::WeaponFired { .. }
                    | EventKind::GameOver(_)
                    | EventKind::Spawned { .. }
            ) {
                self.log.push((self.ticks, e.kind));
            }
        }
        self.ticks += 1;
        keys
    }

    /// Ticks until `stop` holds or `n` ticks passed; gives whether it held.
    pub fn run_until(&mut self, n: u32, mut stop: impl FnMut(&Match) -> bool) -> bool {
        for _ in 0..n {
            self.tick();
            if stop(self) {
                return true;
            }
        }
        false
    }

    /// The first tick worm `victim` was killed by `killer`.
    pub fn first_kill(&self, killer: i32, victim: i32) -> Option<u32> {
        self.log.iter().find_map(|(t, k)| {
            matches!(k, EventKind::Kill { victim: v, killer: by } if *v == victim && *by == killer)
                .then_some(*t)
        })
    }

    /// The shots `worm` fired (`WeaponFired`) and its hits on `target`
    /// (`Damage` events: a shotgun shot can hit several times).
    pub fn shots_and_hits(&self, worm: i32, target: i32) -> (u32, u32) {
        let shots = self
            .log
            .iter()
            .filter(|(_, k)| matches!(k, EventKind::WeaponFired { worm: w, .. } if *w == worm));
        let hits = self.log.iter().filter(
            |(_, k)| matches!(k, EventKind::Damage { worm: v, by, amount, .. } if *v == target && *by == worm && *amount > 0.0),
        );
        (shots.count() as u32, hits.count() as u32)
    }
}
