//! Matches of humans and bots (plan 6 ruling R5): the one place a Deathmatch
//! is assembled, so `olr-cli match` and `olr-game --mode deathmatch` cannot
//! drift. A [`MatchOptions`] names the packs, the seed, the rules and who
//! plays each worm; [`build_match`] gives the world, its bots (one shared
//! area cache, thinking in worm order) and a started [`Recorder`].

use std::sync::Arc;

use anyhow::{Context as _, anyhow, ensure};
use olr_bots::{AreaCache, Bot, BotConfig, BotRng, LoadoutPolicy, Personality, choose_loadout};
use olr_modes::{ChaosCycle, Deathmatch, DeathmatchSettings, RespawnLoadout};
use olr_replay::{ModeSpec, Packs, Recorder};
use olr_sim::{Loadout, MAX_SLOTS, MAX_WORMS, MatchSetup, SimSettings, Slot, World};

use crate::{Content, LoadedLevel, LoadedMod, LoadedTheme, SandboxSetup, SpawnSpec, match_setup};

/// Who plays a worm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Player {
    /// A person at the keyboard with random weapons the restrictions
    /// enable, as OLX's weapon dialog's "Random" button gives
    /// (`CWorm::GetRandomWeapons`, then the empty slots filled), drawn from
    /// the worm's own `BotRng`: the command line's games.
    Human,
    /// A person at the keyboard with the weapons chosen in the weapon
    /// selection (spec §21 G4), by mod weapon id, slot by slot; each must be
    /// one the restrictions enable.
    HumanWith([i32; MAX_SLOTS]),
    /// A bot.
    Bot(BotConfig),
}

/// A Deathmatch to build: everything that decides how it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchOptions {
    /// A mod pack id, such as `olx-classic`.
    pub mod_id: String,
    /// A level pack id, such as `olx-level-dirt-level`.
    pub level_id: String,
    /// The match seed; every bot's generator is seeded from it too.
    pub seed: u32,
    /// The settings before the Deathmatch's rules are applied (OLX's LX56
    /// with the bonus options, as the command line set them).
    pub settings: SimSettings,
    /// Worm `i` is played by `players[i]`: 2 to [`MAX_WORMS`].
    pub players: Vec<Player>,
    /// The rules. `respawn_loadout` is replaced: [`RespawnLoadout::ChaosCycle`]
    /// over the chaos bots when there are any, else `Keep`.
    pub deathmatch: DeathmatchSettings,
    /// OLX's `WeaponRestrictionsFile`, such as [`crate::DEFAULT_PRESET`].
    pub restrictions: String,
    /// The projectile pool's size: OLX's `MAX_PROJECTILES`
    /// ([`MatchSetup::DEFAULT_POOL_CAPACITY`], 3000) for every game; the
    /// engine stress test raises it (`olr-cli stress`). The replay records it.
    pub pool_capacity: usize,
}

impl MatchOptions {
    /// A Deathmatch of `players` on these packs with the defaults: OLX's
    /// LX56 settings, the default rules, [`crate::DEFAULT_PRESET`] and
    /// OLX's projectile pool.
    pub fn deathmatch(mod_id: &str, level_id: &str, seed: u32, players: Vec<Player>) -> MatchOptions {
        MatchOptions {
            mod_id: mod_id.into(),
            level_id: level_id.into(),
            seed,
            settings: SimSettings::lx56(),
            players,
            deathmatch: DeathmatchSettings::default(),
            restrictions: crate::DEFAULT_PRESET.into(),
            pool_capacity: MatchSetup::DEFAULT_POOL_CAPACITY,
        }
    }
}

/// The assets a match is made of, loaded once (a rematch decodes nothing
/// again), and their packs for the replay.
#[derive(Clone, Copy)]
pub struct MatchAssets<'a> {
    pub m: &'a LoadedMod,
    pub level: &'a LoadedLevel,
    pub theme: &'a LoadedTheme,
    pub packs: &'a Packs,
}

/// A match ready for its first tick.
pub struct BuiltMatch {
    /// The setup as `World::new` got it (after `Deathmatch::prepare`), to
    /// make the world again.
    pub setup: MatchSetup,
    /// The mode, to make it again (`ModeSpec::game_mode`).
    pub mode: ModeSpec,
    pub world: World,
    /// The bots in worm order (`Bot::worm`), sharing one area cache: they
    /// must think in this order, once per tick, before `World::tick`.
    pub bots: Vec<Bot>,
    /// Recording since the world was made (`Recorder::start` done).
    pub recorder: Recorder,
}

impl BuiltMatch {
    /// Which worms bots play, by worm index.
    pub fn bot_worms(&self) -> impl Iterator<Item = usize> + '_ {
        self.bots.iter().map(Bot::worm)
    }
}

/// The match `o` describes, made of `a`:
/// - each bot's loadout is `choose_loadout` from its own `BotRng`
///   (OLX's presets for a Deathmatch bot, random for a chaos bot), each
///   human's random weapons ([`Player::Human`]) or chosen ones
///   ([`Player::HumanWith`], refused unless the restrictions enable them);
/// - chaos bots' loadouts rotate on every spawn (spec §20 F10);
/// - natural bonuses and the bots' choices follow `o.restrictions`.
pub fn build_match(a: MatchAssets<'_>, o: &MatchOptions) -> anyhow::Result<BuiltMatch> {
    let n = o.players.len();
    ensure!((2..=MAX_WORMS).contains(&n), "a Deathmatch of 2 to {MAX_WORMS} worms, not {n}");
    for (i, p) in o.players.iter().enumerate() {
        if let Player::Bot(cfg) = p {
            cfg.validate().map_err(|e| anyhow!("worm {i}: {e}"))?;
        }
    }
    let sandbox = SandboxSetup {
        mod_id: o.mod_id.clone(),
        level_id: o.level_id.clone(),
        seed: o.seed,
        settings: o.settings.clone(),
        worms: vec![SpawnSpec::default(); n],
        restrictions: o.restrictions.clone(),
    };
    let mut setup = match_setup(a.m, a.level, a.theme, &sandbox)?;
    setup.pool_capacity = o.pool_capacity;
    let rules = a.m.weapon_rules(&o.restrictions);
    for (i, (w, p)) in setup.worms.iter_mut().zip(&o.players).enumerate() {
        let cfg = match p {
            Player::Bot(cfg) => *cfg,
            Player::Human => BotConfig { loadout: LoadoutPolicy::Random, ..BotConfig::default() },
            Player::HumanWith(ids) => {
                let enabled = rules.enabled_weapons();
                if let Some(bad) = ids.iter().find(|id| !enabled.contains(id)) {
                    anyhow::bail!(
                        "worm {i}: weapon {bad} is not one {:?} enables in {}",
                        o.restrictions,
                        o.mod_id
                    );
                }
                w.loadout = Loadout::new(ids.iter().map(|&id| Slot::ready(id)).collect(), &o.settings);
                continue;
            }
        };
        let mut rng = BotRng::new(o.seed, i, cfg.personality);
        w.loadout = choose_loadout(&cfg, &rules, &o.settings, &mut rng);
    }
    let chaos: Vec<usize> = (0..n)
        .filter(|&i| matches!(o.players[i], Player::Bot(c) if c.personality == Personality::Chaos))
        .collect();
    let respawn_loadout = if chaos.is_empty() {
        RespawnLoadout::Keep
    } else {
        RespawnLoadout::ChaosCycle(ChaosCycle::new(&chaos, a.m.sim.weapons().len()))
    };
    let dm = DeathmatchSettings { respawn_loadout, ..o.deathmatch.clone() };
    let mode = ModeSpec::Deathmatch(dm.clone());
    let game = Deathmatch::prepare(dm, &mut setup).map_err(|e| anyhow!("{e}"))?;
    let world = World::new(setup.clone(), Box::new(game)).map_err(|e| anyhow!("{e}"))?;
    let mut recorder = Recorder::new(a.packs.clone(), &setup, &mode);
    // The first human's worm: where a watcher of the replay starts.
    recorder.set_viewer(
        o.players.iter().position(|p| !matches!(p, Player::Bot(_))).and_then(|i| u8::try_from(i).ok()),
    );
    recorder.start(&world).map_err(|e| anyhow!("{e}"))?;
    let cache = AreaCache::new(world.terrain());
    let bots = (0..n)
        .filter_map(|i| match o.players[i] {
            Player::Bot(cfg) => Some(Bot::with_cache(i, cfg, o.seed, &world, Arc::clone(&cache))),
            Player::Human | Player::HumanWith(_) => None,
        })
        .collect();
    Ok(BuiltMatch { setup, mode, world, bots, recorder })
}

impl Content {
    /// [`build_match`] on this content's packs `o.mod_id` and `o.level_id`
    /// (and the level's theme), loaded for this match.
    pub fn build_match(&self, o: &MatchOptions) -> anyhow::Result<BuiltMatch> {
        let m = self.load_mod(&o.mod_id)?;
        let level = self.load_level(&o.level_id)?;
        let theme = self.load_theme(&level.theme).with_context(|| format!("level {}", o.level_id))?;
        let packs = self.replay_packs(&o.mod_id, &o.level_id)?;
        build_match(MatchAssets { m: &m, level: &level, theme: &theme, packs: &packs }, o)
    }
}
