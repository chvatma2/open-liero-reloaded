//! Weapon restrictions: OLX's `.wps` presets and `CWpnRest`
//! (`src/common/CWpnRest.cpp`, `include/CWpnRest.h`) at commit `d1ad953f`.
//!
//! A mod pack carries the presets OLX would find for it
//! ([`crate::mod_def::ModDef::restrictions`]); [`WeaponRules`] is the list a
//! match plays with, after OLX's `loadList` and `updateList`.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::loadouts::MatchLoadout;
use crate::mod_def::ModDef;

/// OLX's default `WeaponRestrictionsFile` (`src/common/FeatureList.cpp:41`).
pub const DEFAULT_PRESET: &str = "Standard 100lt";

/// OLX `WpnRestrictionState` (`include/CWpnRest.h:28-32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponState {
    /// `wpr_enabled` (0): selectable, and in bonuses.
    Enabled,
    /// `wpr_bonus` (1): only in bonuses.
    Bonus,
    /// `wpr_banned` (2): nowhere.
    Banned,
}

impl WeaponState {
    /// A file's state number taken `MOD 3` (`CWpnRest::addWeapon`,
    /// `CWpnRest.cpp:146-152`; `MOD` is `include/MathLib.h:85`).
    pub fn from_olx(n: i32) -> Self {
        match n.rem_euclid(3) {
            0 => WeaponState::Enabled,
            1 => WeaponState::Bonus,
            _ => WeaponState::Banned,
        }
    }
}

/// A shipped loadout as a mod pack lists it in `mod.ron`'s `loadouts`,
/// keyed by the name of the preset whose weapons it enables (issue #5, not
/// an OLX setting; the game's side is `olr_data::loadouts`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShippedLoadout {
    /// Percent, 0 to 500 (OLX's `LoadingTime` range,
    /// `src/common/FeatureList.cpp:31`): weapons reload in `100 / loading_time`
    /// times their time.
    pub loading_time: i32,
    /// A worm's slots must hold different weapons.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unique: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// One `.wps` file as `CWpnRest::loadList` holds it: weapon name to state.
/// OLX keys its map ignoring ASCII case, keeps a name's first spelling and
/// the state of its last line; names are not trimmed when stored.
pub type Preset = BTreeMap<String, WeaponState>;

/// OLX's `TrimSpaces` (`src/common/StringUtils.cpp:35-47`): C `isspace`.
fn trim_spaces(s: &str) -> &str {
    s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r'))
}

/// The name a `WeaponRestrictionsFile` value refers to among a mod's
/// presets (file names without `.wps`), as `CWpnRest::loadList`
/// (`CWpnRest.cpp:183-213`) builds the path: `.wps` is appended to a name
/// longer than 4 bytes that lacks it. `None` when no imported preset can
/// match: a `cfg/` path, or a short name without `.wps` (OLX looks for a file
/// without the extension).
fn preset_name(file: &str) -> Option<&str> {
    let b = file.as_bytes();
    if b.len() >= 4 && b[..4].eq_ignore_ascii_case(b"cfg/") {
        return None;
    }
    let has_ext = b.len() >= 4 && b[b.len() - 4..].eq_ignore_ascii_case(b".wps");
    if has_ext {
        Some(&file[..file.len() - 4])
    } else if b.len() > 4 {
        Some(file)
    } else {
        None
    }
}

/// The preset `file` (a `WeaponRestrictionsFile` value such as
/// `Standard 100lt`) names among `presets`, compared ignoring ASCII case like
/// OLX's file lookup. `None` is OLX's "couldn't load" (an empty list).
pub fn find_preset<'a>(presets: &'a BTreeMap<String, Preset>, file: &str) -> Option<&'a Preset> {
    let name = preset_name(file)?;
    presets.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, p)| p)
}

/// The restriction list a match plays with: OLX's `CWpnRest` after
/// `Game::loadWeaponRestrictions` (`src/game/Game_loadWeaponRestrictions.cpp:16-28`):
/// the preset's entries, then every mod weapon it lacks, enabled
/// (`updateList`, `CWpnRest.cpp:59-75`). It also knows the mod's name and
/// weapons, which is what OLX's weapon choices read next to it.
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponRules {
    mod_name: String,
    /// The mod's weapon names by id.
    weapons: Vec<String>,
    /// `m_psWeaponList`: unique ignoring ASCII case.
    list: Vec<(String, WeaponState)>,
    /// A loadout's unique rule (issue #5, not OLX's): a worm's slots hold
    /// different weapons ([`crate::loadouts::unique_ok`]).
    unique: bool,
}

impl WeaponRules {
    /// Mod `def` played with restriction file `file` (a `WeaponRestrictionsFile`
    /// value, such as [`DEFAULT_PRESET`]), looked up in `def.restrictions`.
    pub fn new(def: &ModDef, file: &str) -> Self {
        Self::from_parts(&def.name, def.weapons.iter().map(|w| w.name.as_str()), &def.restrictions, file)
    }

    /// Like [`WeaponRules::new`], from the parts of a mod: its name, its
    /// weapon names in id order and its presets.
    pub fn from_parts<'a>(
        mod_name: &str,
        weapons: impl IntoIterator<Item = &'a str>,
        presets: &BTreeMap<String, Preset>,
        file: &str,
    ) -> Self {
        let weapons: Vec<String> = weapons.into_iter().map(ToString::to_string).collect();
        let mut rules =
            WeaponRules { mod_name: mod_name.to_string(), weapons, list: Vec::new(), unique: false };
        // loadList: a preset map can hold names equal but for case only if
        // it was written by hand; the first in map order wins, as a later
        // `addWeapon` of the same key would keep the first spelling.
        for (name, &state) in find_preset(presets, file).into_iter().flatten() {
            if !name.is_empty() && !rules.list.iter().any(|(k, _)| k.eq_ignore_ascii_case(name)) {
                rules.list.push((name.clone(), state));
            }
        }
        // updateList: findWeapon trims the query; addWeapon stores the name as
        // is (overwriting an untrimmed key no trimmed query can reach).
        for i in 0..rules.weapons.len() {
            let name = &rules.weapons[i];
            if name.is_empty() || rules.find(name).is_some() {
                continue;
            }
            match rules.list.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
                Some((_, s)) => *s = WeaponState::Enabled,
                None => rules.list.push((name.clone(), WeaponState::Enabled)),
            }
        }
        rules
    }

    /// The list a match with `loadout` plays with (issue #5): each of the
    /// mod's weapons (names by id) enabled when the loadout has it, else
    /// banned (bonuses are not played with loadouts), stored trimmed so
    /// [`WeaponRules::is_enabled`] finds each; the loadout's unique rule.
    pub fn from_loadout<'a>(
        mod_name: &str,
        weapons: impl IntoIterator<Item = &'a str>,
        loadout: &MatchLoadout,
    ) -> Self {
        let weapons: Vec<String> = weapons.into_iter().map(ToString::to_string).collect();
        let on = loadout.enabled(&weapons);
        let mut list: Vec<(String, WeaponState)> = Vec::new();
        for (name, on) in weapons.iter().zip(on) {
            let name = trim_spaces(name);
            if name.is_empty() || list.iter().any(|(k, _)| k.eq_ignore_ascii_case(name)) {
                continue;
            }
            list.push((name.to_string(), if on { WeaponState::Enabled } else { WeaponState::Banned }));
        }
        WeaponRules { mod_name: mod_name.to_string(), weapons, list, unique: loadout.unique }
    }

    /// Whether a worm's slots must hold different weapons (a loadout's
    /// unique rule; never for an OLX preset).
    pub fn unique(&self) -> bool {
        self.unique
    }

    /// `CWpnRest::findWeapon` (`CWpnRest.cpp:122-136`): `""` finds nothing,
    /// otherwise the trimmed name, ignoring ASCII case.
    fn find(&self, name: &str) -> Option<WeaponState> {
        if name.is_empty() {
            return None;
        }
        let name = trim_spaces(name);
        self.list.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|&(_, s)| s)
    }

    /// `CWpnRest::isEnabled` (`CWpnRest.cpp:216-226`): a name not in the
    /// list counts as banned.
    pub fn is_enabled(&self, name: &str) -> bool {
        self.find(name) == Some(WeaponState::Enabled)
    }

    /// `CWpnRest::getWeaponState` (`CWpnRest.cpp:268-277`): banned if missing.
    pub fn state(&self, name: &str) -> WeaponState {
        self.find(name).unwrap_or(WeaponState::Banned)
    }

    /// Every mod weapon's state, by id.
    pub fn weapon_states(&self) -> Vec<WeaponState> {
        self.weapons.iter().map(|n| self.state(n)).collect()
    }

    /// The ids of the enabled mod weapons, in id order: what
    /// `Game::getRandomEnabledWpn` (`src/game/Game.cpp:1405-1420`) chooses
    /// from, and what `CWorm::GetRandomWeapons` accepts.
    pub fn enabled_weapons(&self) -> Vec<i32> {
        self.ids(|s| s == WeaponState::Enabled)
    }

    /// The ids of the mod weapons that are not banned, in id order: the
    /// weapons a spawned bonus may hold (`goodWpns`,
    /// `src/server/CServer_Game.cpp:292-297`).
    pub fn bonus_weapons(&self) -> Vec<i32> {
        self.ids(|s| s != WeaponState::Banned)
    }

    fn ids(&self, keep: impl Fn(WeaponState) -> bool) -> Vec<i32> {
        (0..self.weapons.len()).filter(|&i| keep(self.state(&self.weapons[i]))).map(|i| i as i32).collect()
    }

    /// `CGameScript::FindWeaponId` (`src/common/CGameScript.cpp:1211-1231`):
    /// the first mod weapon named `name`, ignoring ASCII case; -1 if none.
    pub fn find_weapon_id(&self, name: &str) -> i32 {
        if name.is_empty() {
            return -1;
        }
        self.weapons.iter().position(|w| w.eq_ignore_ascii_case(name)).map_or(-1, |i| i as i32)
    }

    /// `CWpnRest::findEnabledWeapon` with the mod's weapon list
    /// (`CWpnRest.cpp:241-260`): the id of the first enabled weapon of the
    /// list in its map order (`stringcasecmp`: bytes compared ignoring ASCII
    /// case) that the mod has; -1 if none. The weapon selection gives it to
    /// a slot whose remembered weapon is not enabled
    /// (`CWormHumanInputHandler::initWeaponSelection`, `CWormHuman.cpp:722-730`).
    pub fn find_enabled_weapon(&self) -> i32 {
        let mut enabled: Vec<&str> =
            self.list.iter().filter(|(_, s)| *s == WeaponState::Enabled).map(|(k, _)| k.as_str()).collect();
        enabled.sort_by(|a, b| {
            a.bytes().map(|c| c.to_ascii_lowercase()).cmp(b.bytes().map(|c| c.to_ascii_lowercase()))
        });
        enabled.iter().map(|n| self.find_weapon_id(n)).find(|&id| id >= 0).unwrap_or(-1)
    }

    /// The mod's name (OLX's `ModInfo::name`, the script's name, such as
    /// `Liero v1.0`).
    pub fn mod_name(&self) -> &str {
        &self.mod_name
    }

    /// The number of mod weapons (`GetNumWeapons`).
    pub fn weapon_count(&self) -> usize {
        self.weapons.len()
    }
}

/// The ids of `def`'s weapons that restriction file `file` enables (state 0
/// after `updateList`), in id order.
pub fn enabled_weapons(def: &ModDef, file: &str) -> Vec<i32> {
    WeaponRules::new(def, file).enabled_weapons()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn presets(entries: &[(&str, &[(&str, WeaponState)])]) -> BTreeMap<String, Preset> {
        entries
            .iter()
            .map(|(p, list)| (p.to_string(), list.iter().map(|(n, s)| (n.to_string(), *s)).collect()))
            .collect()
    }

    use WeaponState::{Banned, Bonus, Enabled};

    #[test]
    fn states_are_taken_mod_3() {
        assert_eq!(WeaponState::from_olx(0), Enabled);
        assert_eq!(WeaponState::from_olx(4), Bonus);
        assert_eq!(WeaponState::from_olx(-1), Banned);
        assert_eq!(WeaponState::from_olx(i32::MAX), Bonus);
        assert_eq!(WeaponState::from_olx(i32::MIN), Bonus);
    }

    #[test]
    fn preset_names_resolve_like_load_list() {
        let p = presets(&[("Standard 100lt", &[]), ("abc", &[])]);
        assert!(find_preset(&p, "Standard 100lt").is_some());
        assert!(find_preset(&p, "standard 100LT.WPS").is_some());
        assert!(find_preset(&p, "cfg/presets/Classic/Standard 100lt.wps").is_none());
        // OLX appends `.wps` only to names longer than 4 bytes.
        assert!(find_preset(&p, "abc").is_none());
        assert!(find_preset(&p, "abc.wps").is_some());
        assert!(find_preset(&p, "Missing").is_none());
    }

    #[test]
    fn lookups_follow_cwpnrest() {
        let p = presets(&[("Preset", &[("Rifle", Banned), ("Mine", Bonus), (" Laser", Enabled)])]);
        let r = WeaponRules::from_parts(
            "Liero v1.0",
            ["rifle", "Mine", "Laser", "Bazooka", " Nuke ", ""],
            &p,
            "Preset",
        );
        // Case-insensitive, banned and bonus kept, a missing weapon added enabled.
        assert_eq!(r.state("RIFLE"), Banned);
        assert_eq!(r.state("Mine"), Bonus);
        assert!(r.is_enabled("Bazooka"));
        // The file's " Laser" never matches a trimmed query; updateList adds "Laser".
        assert!(r.is_enabled("Laser"));
        // A mod weapon with spaces is stored as is but looked up trimmed: banned.
        assert_eq!(r.state(" Nuke "), Banned);
        // The empty name is never found.
        assert_eq!(r.state(""), Banned);
        // A name of another mod is banned unless listed.
        assert!(!r.is_enabled("Zimm"));
        assert_eq!(r.weapon_states(), vec![Banned, Bonus, Enabled, Enabled, Banned, Banned]);
        assert_eq!(r.enabled_weapons(), vec![2, 3]);
        assert_eq!(r.bonus_weapons(), vec![1, 2, 3]);
        assert_eq!(r.find_weapon_id("RIFLE"), 0);
        assert_eq!(r.find_weapon_id("Zimm"), -1);
        assert_eq!(r.find_weapon_id(""), -1);
    }

    #[test]
    fn a_missing_preset_enables_every_weapon() {
        let r = WeaponRules::from_parts("m", ["A", "B"], &BTreeMap::new(), DEFAULT_PRESET);
        assert_eq!(r.enabled_weapons(), vec![0, 1]);
        assert_eq!(r.bonus_weapons(), vec![0, 1]);
        assert_eq!(r.weapon_count(), 2);
        assert_eq!(r.mod_name(), "m");
    }

    #[test]
    fn a_loadout_enables_its_weapons_and_bans_the_rest() {
        let l = MatchLoadout {
            weapons: vec!["laser".into(), " Nuke ".into()],
            unique: true,
            ..Default::default()
        };
        let r = WeaponRules::from_loadout("m", ["Rifle", "Laser", " Nuke ", "Mine"], &l);
        // Stored trimmed: a mod weapon with spaces can be on.
        assert_eq!(r.enabled_weapons(), vec![1, 2]);
        assert_eq!(r.bonus_weapons(), vec![1, 2]);
        assert!(r.unique());
        let all = WeaponRules::from_loadout("m", ["Rifle", "Laser"], &MatchLoadout::default());
        assert_eq!(all.enabled_weapons(), vec![0, 1]);
        assert!(!all.unique());
        assert_eq!(all.find_enabled_weapon(), 1);
    }

    /// `findEnabledWeapon`: the first enabled name in the map's
    /// case-insensitive order that the mod has, not the first by id.
    #[test]
    fn the_first_enabled_weapon_is_by_name() {
        let p = presets(&[("Preset", &[("cannon", Banned), ("Zimm", Enabled), ("blaster", Enabled)])]);
        let r = WeaponRules::from_parts("m", ["Cannon", "Shotgun", "Blaster"], &p, "Preset");
        // "blaster" < "Shotgun" < "Zimm" ignoring case; "Zimm" is not the mod's.
        assert_eq!(r.find_enabled_weapon(), 2);
        let none = presets(&[("Preset", &[("A", Banned)])]);
        assert_eq!(WeaponRules::from_parts("m", ["A"], &none, "Preset").find_enabled_weapon(), -1);
    }
}
