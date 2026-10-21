//! A match from its wire start (networking spec §4): the options every peer
//! builds the identical world from, the world itself, and the host's bots
//! pressing their keys (spec §3.6: bots think on the host only).

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context as _, anyhow, bail, ensure};
use olr_bots::{AreaCache, Bot, BotConfig, Difficulty, LoadoutPolicy, Personality};
use olr_content::{Content, MatchLoadout, MatchOptions, Player, WeaponChoice};
use olr_data::loadouts::{LOADING_TIME_RANGE, unique_ok};
use olr_modes::DeathmatchSettings;
use olr_netcode::{Driver, TickInfo, WorldSim};
use olr_proto::{BotWire, LoadoutWire, Look, MatchSetupWire, MatchStartWire, RulesWire};
use olr_sim::{Keys, MatchSetup, SimSettings, World};

/// The bots' wire numbers: `BotWire::difficulty` is OLX's level, 0 (Easy)
/// to 3 (Xtreme); `personality` 0 OLX's bot, 1 chaos, 2 idle; `loadout` 0 OLX's
/// presets, 1 random weapons.
const DIFFICULTIES: [Difficulty; 4] =
    [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard, Difficulty::Xtreme];
const PERSONALITIES: [Personality; 3] = [Personality::Deathmatch, Personality::Chaos, Personality::Idle];
const LOADOUTS: [LoadoutPolicy; 2] = [LoadoutPolicy::OlxPreset, LoadoutPolicy::Random];

/// A bot's configuration from its wire numbers (a number out of range is
/// refused: the start came from the network).
pub fn bot_config(b: &BotWire) -> anyhow::Result<BotConfig> {
    let pick = |n: u8, what: &str, len: usize| {
        ensure!(usize::from(n) < len, "bot {what} {n} is not 0 to {}", len - 1);
        Ok(usize::from(n))
    };
    Ok(BotConfig {
        difficulty: DIFFICULTIES[pick(b.difficulty, "difficulty", DIFFICULTIES.len())?],
        personality: PERSONALITIES[pick(b.personality, "personality", PERSONALITIES.len())?],
        loadout: LOADOUTS[pick(b.loadout, "loadout", LOADOUTS.len())?],
        ..BotConfig::default()
    })
}

/// A bot's wire numbers (what [`bot_config`] reads back); custom
/// difficulty numbers, aim and budget do not travel.
pub fn bot_wire(c: &BotConfig, look: Look) -> BotWire {
    let index = |found: Option<usize>| found.unwrap_or(0) as u8;
    BotWire {
        difficulty: index(DIFFICULTIES.iter().position(|&d| d == c.difficulty)),
        personality: index(PERSONALITIES.iter().position(|&p| p == c.personality)),
        loadout: index(LOADOUTS.iter().position(|&l| l == c.loadout)),
        look,
    }
}

/// The Deathmatch rules of the wire (every field named, so a rule added to
/// `DeathmatchSettings` does not compile until it travels).
pub fn deathmatch(r: &RulesWire) -> DeathmatchSettings {
    let RulesWire {
        lives,
        kill_limit,
        time_limit_min,
        allow_negative_score,
        death_decreases_score,
        suicide_decreases_score,
        sprees,
        dying_sprees,
    } = r.clone();
    DeathmatchSettings {
        lives,
        kill_limit,
        time_limit: time_limit_min,
        allow_negative_score,
        death_decreases_score,
        suicide_decreases_score,
        sprees,
        dying_sprees,
        respawn_loadout: Default::default(),
    }
}

/// The wire's rules of a Deathmatch (its respawn loadout is the match's
/// own: [`olr_content::build_match`] sets it).
pub fn rules_wire(d: &DeathmatchSettings) -> RulesWire {
    let DeathmatchSettings {
        lives,
        kill_limit,
        time_limit,
        allow_negative_score,
        death_decreases_score,
        suicide_decreases_score,
        sprees,
        dying_sprees,
        respawn_loadout: _,
    } = d.clone();
    RulesWire {
        lives,
        kill_limit,
        time_limit_min: time_limit,
        allow_negative_score,
        death_decreases_score,
        suicide_decreases_score,
        sprees,
        dying_sprees,
    }
}

/// A setup's loadout (issue #5) as the host sent it, its loading time
/// clamped into `LOADING_TIME_RANGE`; its weapons are matched to the mod's
/// by name when the match is built (unknown names enable nothing).
pub fn loadout_of(w: &LoadoutWire) -> MatchLoadout {
    MatchLoadout {
        name: w.name.clone(),
        weapons: w.weapons.clone(),
        loading_time: w.loading_time.clamp(*LOADING_TIME_RANGE.start(), *LOADING_TIME_RANGE.end()),
        unique: w.unique,
    }
}

/// The host's loadout for its setup: `l` in full. One that does not fit
/// the wire's bounds (`olr_proto::valid_loadout`: more than
/// `MAX_LOADOUT_WEAPONS` weapons, a longer weapon name) is `Default`, with
/// the reason.
pub fn loadout_wire(l: &MatchLoadout) -> (LoadoutWire, Option<String>) {
    let w = LoadoutWire {
        name: l.name.clone(),
        weapons: l.weapons.clone(),
        loading_time: l.loading_time,
        unique: l.unique,
    };
    if olr_proto::valid_loadout(&w) {
        (w, None)
    } else {
        let why = format!("loadout {:?} is too large to send: playing Default", l.name);
        (LoadoutWire::default(), Some(why))
    }
}

/// The match options of a start: slot `i` is worm `i`; a member's slot is
/// `Player::HumanWith` its chosen weapons or `Player::Human` (random
/// weapons drawn from the seed, the same on every peer), a bot's slot
/// `Player::Bot`. OLX's LX56 settings (no bonuses: issue #5) with the
/// setup's loadout ([`loadout_of`]).
pub fn options(start: &MatchStartWire) -> anyhow::Result<MatchOptions> {
    let setup = &start.setup;
    let players = start
        .slots
        .iter()
        .enumerate()
        .map(|(i, s)| match (s.member, s.bot) {
            (Some(_), None) => Ok(s.loadout.map_or(Player::Human, Player::HumanWith)),
            (None, Some(b)) => {
                let wire = setup.bots.get(usize::from(b)).with_context(|| format!("slot {i}: no bot {b}"))?;
                bot_config(wire).map(Player::Bot).with_context(|| format!("slot {i}"))
            }
            _ => Err(anyhow!("slot {i} is neither a member's nor a bot's")),
        })
        .collect::<anyhow::Result<Vec<Player>>>()?;
    Ok(MatchOptions {
        mod_id: setup.mod_id.clone(),
        level_id: setup.level_id.clone(),
        seed: start.seed,
        settings: SimSettings::lx56(),
        players,
        deathmatch: deathmatch(&setup.rules),
        weapons: WeaponChoice::Loadout(loadout_of(&setup.loadout)),
        pool_capacity: MatchSetup::DEFAULT_POOL_CAPACITY,
    })
}

/// The packs a setup names for a match of `mod_id` on `level_id`: the mod,
/// the level and its theme with their content hashes (what the host puts in
/// `MatchSetupWire::packs`).
pub fn setup_packs(content: &Content, mod_id: &str, level_id: &str) -> anyhow::Result<Vec<(String, String)>> {
    let p = content.replay_packs(mod_id, level_id)?;
    Ok([p.mod_pack, p.level, p.theme].into_iter().map(|r| (r.id, r.hash)).collect())
}

/// Checks that every pack the setup names is here with its content hash.
pub fn check_packs(content: &Content, setup: &MatchSetupWire) -> anyhow::Result<()> {
    for (id, hash) in &setup.packs {
        let Some(kind) = content.pack_kind(id) else { bail!("pack {id} is missing") };
        let ours = content.pack_hash(id, kind)?;
        ensure!(ours == *hash, "pack {id} differs from the host's");
    }
    Ok(())
}

/// [`options`] made safe to build alike on every peer (the start is the
/// network's input): a loadout enabling none of the mod's weapons (only a
/// hostile host sends one) is `Default`; a member's chosen weapons the
/// loadout refuses (a weapon it does not enable, or under its unique rule
/// a repeat while others are free: `unique_ok`) become random weapons.
pub fn resolve(content: &Content, start: &MatchStartWire) -> anyhow::Result<MatchOptions> {
    let mut o = options(start)?;
    let mut rules = content.rules(&o.mod_id, &o.weapons)?;
    if rules.enabled_weapons().is_empty() {
        o.weapons = WeaponChoice::default_loadout();
        rules = content.rules(&o.mod_id, &o.weapons)?;
    }
    let enabled = rules.enabled_weapons();
    for p in &mut o.players {
        if let Player::HumanWith(ids) = p
            && (!ids.iter().all(|id| enabled.contains(id)) || (rules.unique() && !unique_ok(ids, &enabled)))
        {
            *p = Player::Human;
        }
    }
    Ok(o)
}

/// A match ready to play: the world under rollback, and its bots by slot
/// (only the host runs them).
pub struct MatchWorld {
    pub sim: WorldSim,
    pub bots: BTreeMap<u8, Bot>,
}

/// The world of a start: the setup's packs checked, then
/// [`build_options`] of [`resolve`].
pub fn build(content: &Content, start: &MatchStartWire) -> anyhow::Result<MatchWorld> {
    check_packs(content, &start.setup)?;
    build_options(content, &resolve(content, start)?)
}

/// The world of `o` and its bots, with their own area cache
/// (`olr_content::build_match`: every call loads the packs again).
pub fn build_options(content: &Content, o: &MatchOptions) -> anyhow::Result<MatchWorld> {
    let built = content.build_match(o)?;
    let bots = built.bots.into_iter().map(|b| (b.worm() as u8, b)).collect();
    Ok(MatchWorld { sim: WorldSim::new(built.world), bots })
}

/// The bots of `start` restarted on `world` (spec §3.7, N75): a new host
/// takes them over with `Bot::with_cache` per bot slot, the match's seed
/// and a new area cache (empty: it fills as they think), so they forget
/// their routes once per migration. Only the host thinks for bots: nobody
/// else's simulation changes.
pub fn restart_bots(start: &MatchStartWire, world: &World) -> anyhow::Result<HostBots> {
    let o = options(start)?;
    let cache = AreaCache::new(world.terrain());
    let bots = o
        .players
        .iter()
        .enumerate()
        .filter_map(|(i, p)| match p {
            Player::Bot(cfg) => {
                Some((i as u8, Bot::with_cache(i, *cfg, start.seed, world, Arc::clone(&cache))))
            }
            Player::Human | Player::HumanWith(_) => None,
        })
        .collect();
    Ok(HostBots::new(bots))
}

/// The host's bots (spec §3.6): each thinks for its slot, in slot order,
/// once a tick, before the world ticks (they share one area cache).
pub struct HostBots {
    bots: BTreeMap<u8, Bot>,
}

impl HostBots {
    pub fn new(bots: BTreeMap<u8, Bot>) -> HostBots {
        HostBots { bots }
    }

    /// The slots bots play.
    pub fn slots(&self) -> impl Iterator<Item = u8> + '_ {
        self.bots.keys().copied()
    }

    /// The keys of the local `slots` (`out[i]` for `slots[i]`, slots
    /// ascending): `human`'s slot from its callback, a bot's from its
    /// thinking on the world and the last tick's events, any other `Keys(0)`.
    pub fn press(
        &mut self,
        sim: &WorldSim,
        slots: &[u8],
        out: &mut [Keys],
        mut human: Option<(u8, &mut dyn FnMut() -> Keys)>,
    ) {
        for (&slot, key) in slots.iter().zip(out.iter_mut()) {
            *key = match (&mut human, self.bots.get_mut(&slot)) {
                (Some((h, keys)), _) if *h == slot => keys(),
                (_, Some(bot)) => bot.think(sim.world(), sim.events()),
                _ => Keys(0),
            };
        }
    }
}

/// The bots alone (no human at this peer): `olr-cli`'s online matches.
impl Driver<WorldSim> for HostBots {
    fn local_inputs(&mut self, sim: &WorldSim, _tick: u32, slots: &[u8], out: &mut [Keys]) {
        self.press(sim, slots, out, None);
    }

    fn simulated(&mut self, _sim: &WorldSim, _info: TickInfo) {}
}
