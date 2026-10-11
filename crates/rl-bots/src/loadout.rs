//! The weapons a bot starts a match with (spec §20 F1; notes §2.2, §2.3, §4.7).

use alloc::vec::Vec;
use rl_data::restrictions::{WeaponRules, WeaponState};
use rl_sim::{Loadout, MAX_SLOTS, SimSettings, Slot};

use crate::{BotConfig, BotRng, LoadoutPolicy, Personality};

/// The tactics OLX's bot picks with its loadout (`AI_SetGameType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameType {
    /// `GAM_RIFLES`: every slot a Rifle.
    Rifles,
    /// `GAM_100LT`: Super Shotgun, Napalm, Cannon, Doomsday, Chaingun.
    Lt100,
    /// `GAM_MORTARS`: every slot a Mortar Launcher.
    Mortars,
    /// `GAM_OTHER`: random weapons.
    Other,
}

/// A bot's weapons, one per slot (-1 for an empty slot), and its tactics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BotWeapons {
    pub weapons: [i32; MAX_SLOTS],
    pub game_type: GameType,
}

/// `CWorm::GetRandomWeapons` (`src/common/CWorm.cpp:788-844`): for each slot,
/// a random start weapon (never 0, which only the wrap-around reaches), then
/// the first weapon from there that `enabled` allows and no earlier slot
/// holds. When every allowed weapon is taken, the slot repeats the last
/// allowed one the scan passed; with none allowed it stays empty (-1).
/// `enabled` has one entry per mod weapon, by id. One draw per slot.
pub fn get_random_weapons(enabled: &[bool], rng: &mut BotRng) -> [i32; MAX_SLOTS] {
    let mut slots = [-1; MAX_SLOTS];
    let count = enabled.len() as i32;
    if count <= 0 {
        return slots; // "gamescript is not loaded": no draws
    }
    for i in 0..MAX_SLOTS {
        // `num >= 1`, so the scan below always ends when it comes back to it.
        let num = rng.random_int(count - 1).max(1);
        let mut n = num;
        let mut last_enabled = -1;
        loop {
            if n >= count {
                n = 0;
            }
            let selected = slots[..i].contains(&n);
            if enabled[n as usize] {
                if !selected {
                    break;
                }
                last_enabled = n;
            }
            n += 1;
            if n == num {
                n = last_enabled;
                break;
            }
        }
        slots[i] = if (0..count).contains(&n) { n } else { -1 };
    }
    slots
}

/// `Game::getRandomEnabledWpn` (`src/game/Game.cpp:1405-1420`): one of the
/// `enabled` ids (`randomChoiceFrom`, one draw), or -1 without a draw when
/// there is none.
pub fn random_enabled_weapon(enabled: &[i32], rng: &mut BotRng) -> i32 {
    match enabled.len() {
        0 => -1,
        n => enabled[rng.random_long(n as u64 - 1) as usize],
    }
}

/// `CWormBotInputHandler::initWeaponSelection` (`src/common/CWormBot.cpp:4129-4192`):
/// OLX's preset loadouts by mod name and `LoadingTime` (percent), checked
/// against the restrictions, else random weapons; any slot left without a
/// weapon of the mod gets a random enabled one.
pub fn olx_bot_weapons(rules: &WeaponRules, loading_time: i32, rng: &mut BotRng) -> BotWeapons {
    let name = rules.mod_name();
    let lt = loading_time;
    let mut preset = None;
    if lt > 15 && lt < 26 && (name.contains("Classic") || name.contains("Liero v1.0")) {
        if rules.is_enabled("Rifle") {
            preset = Some(([rules.find_weapon_id("Rifle"); MAX_SLOTS], GameType::Rifles));
        }
    } else if (name.contains("Liero") || name.contains("Classic")) && lt == 100 {
        // `MAX_SLOTS` is OLX's five `WeaponSlotsNum`, so `slots >= 5` holds.
        if LT100.iter().all(|w| rules.is_enabled(w)) {
            preset = Some((LT100.map(|w| rules.find_weapon_id(w)), GameType::Lt100));
        }
    } else if (name.contains("MW 1.0") || name.contains("Modern Warfare1.0"))
        && lt < 50
        && rules.is_enabled("Mortar Launcher")
    {
        // The last branch, so folding its inner `if` changes nothing.
        preset = Some(([rules.find_weapon_id("Mortar Launcher"); MAX_SLOTS], GameType::Mortars));
    }
    let (weapons, game_type) = match preset {
        Some((weapons, game_type)) => (fill_empty(weapons, rules, rng), game_type),
        None => (random_loadout(rules, rng), GameType::Other),
    };
    BotWeapons { weapons, game_type }
}

/// The 100lt loadout, slot by slot.
pub(crate) const LT100: [&str; MAX_SLOTS] = ["Super Shotgun", "Napalm", "Cannon", "Doomsday", "Chaingun"];

/// The end of `initWeaponSelection`: a slot without a weapon of the mod
/// (`weapon() == NULL`) gets `getRandomEnabledWpn`.
fn fill_empty(mut weapons: [i32; MAX_SLOTS], rules: &WeaponRules, rng: &mut BotRng) -> [i32; MAX_SLOTS] {
    let count = rules.weapon_count() as i32;
    if weapons.iter().all(|w| (0..count).contains(w)) {
        return weapons;
    }
    let enabled = rules.enabled_weapons();
    for w in &mut weapons {
        if !(0..count).contains(w) {
            *w = random_enabled_weapon(&enabled, rng);
        }
    }
    weapons
}

/// The loadout bot `cfg` starts a match with (notes §4.7), for the mod and
/// restrictions of `rules` and the match's `settings`:
/// - a Deathmatch bot with [`LoadoutPolicy::OlxPreset`]: [`olx_bot_weapons`];
/// - with [`LoadoutPolicy::Random`]: [`get_random_weapons`] under the
///   restrictions, empty slots filled as OLX's bot does;
/// - a chaos bot: [`get_random_weapons`] ignoring the restrictions (F10; its
///   mode changes the weapons on every spawn).
///
/// Every slot starts full and ready; `rules` must be for the match's mod.
pub fn choose_loadout(
    cfg: &BotConfig,
    rules: &WeaponRules,
    settings: &SimSettings,
    rng: &mut BotRng,
) -> Loadout {
    let weapons = match (cfg.personality, cfg.loadout) {
        (Personality::Chaos, _) => {
            let all: Vec<bool> = alloc::vec![true; rules.weapon_count()];
            get_random_weapons(&all, rng)
        }
        (Personality::Deathmatch, LoadoutPolicy::OlxPreset) => {
            olx_bot_weapons(rules, settings.loading_time, rng).weapons
        }
        (Personality::Deathmatch, LoadoutPolicy::Random) => random_loadout(rules, rng),
    };
    Loadout::new(weapons.iter().map(|&w| Slot::ready(w)).collect(), settings)
}

/// The random branch of `initWeaponSelection`: `GetRandomWeapons`, then the
/// fill of empty slots.
fn random_loadout(rules: &WeaponRules, rng: &mut BotRng) -> [i32; MAX_SLOTS] {
    let enabled: Vec<bool> = rules.weapon_states().into_iter().map(|s| s == WeaponState::Enabled).collect();
    let weapons = get_random_weapons(&enabled, rng);
    fill_empty(weapons, rules, rng)
}

#[cfg(test)]
#[path = "loadout_tests.rs"]
mod tests;
