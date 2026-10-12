//! A match from its wire start (networking spec §4): the options every peer
//! builds the identical world from, the world itself, and the host's bots
//! pressing their keys (spec §3.6: bots think on the host only).

use std::collections::BTreeMap;

use anyhow::{Context as _, anyhow, bail, ensure};
use rl_bots::{Bot, BotConfig, Difficulty, LoadoutPolicy, Personality};
use rl_content::{Content, DEFAULT_PRESET, MatchOptions, Player};
use rl_modes::DeathmatchSettings;
use rl_netcode::{Driver, TickInfo, WorldSim};
use rl_proto::{BotWire, Look, MatchSetupWire, MatchStartWire, RulesWire};
use rl_sim::{Keys, MatchSetup, SimSettings};

/// The bots' wire numbers: `BotWire::difficulty` is OLX's level, 0 (Easy)
/// to 3 (Xtreme); `personality` 0 OLX's bot, 1 chaos; `loadout` 0 OLX's
/// presets, 1 random weapons.
const DIFFICULTIES: [Difficulty; 4] =
    [Difficulty::Easy, Difficulty::Medium, Difficulty::Hard, Difficulty::Xtreme];
const PERSONALITIES: [Personality; 2] = [Personality::Deathmatch, Personality::Chaos];
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
/// own: [`rl_content::build_match`] sets it).
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

/// The match options of a start: slot `i` is worm `i`; a member's slot is
/// `Player::HumanWith` its chosen weapons or `Player::Human` (random
/// weapons drawn from the seed, the same on every peer), a bot's slot
/// `Player::Bot`. OLX's LX56 settings with the setup's bonuses and loading
/// time; an empty restrictions name is [`DEFAULT_PRESET`].
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
    let restrictions =
        if setup.restrictions.is_empty() { DEFAULT_PRESET.to_string() } else { setup.restrictions.clone() };
    Ok(MatchOptions {
        mod_id: setup.mod_id.clone(),
        level_id: setup.level_id.clone(),
        seed: start.seed,
        settings: SimSettings {
            bonuses: setup.bonuses,
            loading_time: setup.loading_time,
            ..SimSettings::lx56()
        },
        players,
        deathmatch: deathmatch(&setup.rules),
        restrictions,
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

/// [`options`] with each member's chosen weapons checked against the mod's
/// restrictions: a choice the restrictions refuse (the network's input)
/// becomes random weapons, alike on every peer.
pub fn resolve(content: &Content, start: &MatchStartWire) -> anyhow::Result<MatchOptions> {
    let mut o = options(start)?;
    let enabled = content.weapon_rules(&o.mod_id, &o.restrictions)?.enabled_weapons();
    for p in &mut o.players {
        if let Player::HumanWith(ids) = p
            && !ids.iter().all(|id| enabled.contains(id))
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
/// (`rl_content::build_match`: every call loads the packs again).
pub fn build_options(content: &Content, o: &MatchOptions) -> anyhow::Result<MatchWorld> {
    let built = content.build_match(o)?;
    let bots = built.bots.into_iter().map(|b| (b.worm() as u8, b)).collect();
    Ok(MatchWorld { sim: WorldSim::new(built.world), bots })
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

/// The bots alone (no human at this peer): `rl-cli`'s online matches.
impl Driver<WorldSim> for HostBots {
    fn local_inputs(&mut self, sim: &WorldSim, _tick: u32, slots: &[u8], out: &mut [Keys]) {
        self.press(sim, slots, out, None);
    }

    fn simulated(&mut self, _sim: &WorldSim, _info: TickInfo) {}
}
