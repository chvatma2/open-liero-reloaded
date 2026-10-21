//! Loadouts (issue #5): a named weapon choice for one mod, which weapons
//! are on, the loading time and whether a worm's five slots must hold
//! different weapons. Not an OLX feature: OLX chose the weapons with a
//! `.wps` preset ([`crate::restrictions`]) and set `LoadingTime` apart
//! (`src/common/FeatureList.cpp:31`); a loadout puts them together.
//!
//! Every mod has [`DEFAULT_LOADOUT`] ("Default"), which is no data: every
//! weapon on, 100 %, unique off ([`MatchLoadout::default`], whose empty
//! weapon list means "every weapon of the mod"). A mod pack lists its
//! shipped loadouts by preset name ([`crate::mod_def::ModDef::loadouts`]):
//! each is the weapons its preset enables at the loading time given.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::RangeInclusive;
use serde::{Deserialize, Serialize};

pub use crate::restrictions::ShippedLoadout;
use crate::restrictions::{Preset, WeaponRules};

/// The loadout every mod has, first in its list: every weapon, 100 %,
/// unique off.
pub const DEFAULT_LOADOUT: &str = "Default";

/// OLX's `LoadingTime` range in percent (`src/common/FeatureList.cpp:31`).
pub const LOADING_TIME_RANGE: RangeInclusive<i32> = 0..=500;

/// OLX's default `LoadingTime` (`src/common/FeatureList.cpp:31`).
pub const DEFAULT_LOADING_TIME: i32 = 100;

/// The longest loadout name, in characters.
pub const MAX_LOADOUT_NAME: usize = 40;

/// A loadout: what a match is played with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MatchLoadout {
    /// Unique within its mod.
    pub name: String,
    /// The weapons on, by the mod's names, in the mod's order. Empty:
    /// every weapon of the mod (no restriction: [`DEFAULT_LOADOUT`]).
    pub weapons: Vec<String>,
    /// OLX's `LoadingTime`, percent, within [`LOADING_TIME_RANGE`].
    pub loading_time: i32,
    /// A worm's five slots may not hold the same weapon twice, unless
    /// every weapon on is already in one ([`unique_ok`]).
    pub unique: bool,
}

impl Default for MatchLoadout {
    /// [`DEFAULT_LOADOUT`]: every weapon, 100 %, unique off.
    fn default() -> Self {
        MatchLoadout {
            name: DEFAULT_LOADOUT.to_string(),
            weapons: Vec::new(),
            loading_time: DEFAULT_LOADING_TIME,
            unique: false,
        }
    }
}

impl MatchLoadout {
    /// The shipped loadout `name` of a mod with weapon names `weapons` (by
    /// id) and presets `presets`: the weapons the preset of that name
    /// enables (OLX's `CWpnRest` after `updateList`: a weapon the preset
    /// lacks is on; `Bonus` and `Banned` are off). `None` without that
    /// preset, or when it enables no weapon (bonuses are gone, so such a
    /// preset plays nothing).
    pub fn shipped(
        name: &str,
        weapons: &[&str],
        presets: &BTreeMap<String, Preset>,
        s: ShippedLoadout,
    ) -> Option<MatchLoadout> {
        presets.get(name)?;
        // The `.wps` suffix makes `loadList`'s file-name rule find any name.
        let rules = WeaponRules::from_parts("", weapons.iter().copied(), presets, &format!("{name}.wps"));
        let on: Vec<String> =
            weapons.iter().filter(|w| rules.is_enabled(w)).map(|w| (*w).to_string()).collect();
        (!on.is_empty()).then(|| MatchLoadout {
            name: name.to_string(),
            weapons: on,
            loading_time: s.loading_time.clamp(*LOADING_TIME_RANGE.start(), *LOADING_TIME_RANGE.end()),
            unique: s.unique,
        })
    }

    /// Whether each of the mod's weapons (`weapons`, by id) is on: all for
    /// an empty list, else those it names (ignoring ASCII case, as OLX's
    /// `FindWeaponId`).
    pub fn enabled<S: AsRef<str>>(&self, weapons: &[S]) -> Vec<bool> {
        weapons
            .iter()
            .map(|w| {
                self.weapons.is_empty() || self.weapons.iter().any(|n| n.eq_ignore_ascii_case(w.as_ref()))
            })
            .collect()
    }

    /// This loadout fitted to a mod with weapon names `weapons` (by id): its
    /// weapons in the mod's spelling and order, each once; a name the mod
    /// lacks dropped; the loading time clamped into
    /// [`LOADING_TIME_RANGE`]; the name cleaned ([`clean_name`]). One
    /// warning per change. A list left empty means every weapon: the
    /// caller decides whether that is [`DEFAULT_LOADOUT`] or a fault.
    pub fn fit<S: AsRef<str>>(&mut self, weapons: &[S]) -> Vec<String> {
        let mut w = Vec::new();
        let name = clean_name(&self.name);
        if name != self.name {
            w.push(format!("loadout name {:?} is not usable: {name:?}", self.name));
            self.name = name;
        }
        for n in &self.weapons {
            if !weapons.iter().any(|m| m.as_ref().eq_ignore_ascii_case(n)) {
                w.push(format!("loadout {:?}: the mod has no weapon {n:?}: dropped", self.name));
            }
        }
        let had = !self.weapons.is_empty();
        let on = self.enabled(weapons);
        self.weapons = if had {
            weapons.iter().zip(on).filter(|(_, on)| *on).map(|(m, _)| m.as_ref().to_string()).collect()
        } else {
            Vec::new()
        };
        // Two mod weapons of one name ignoring case: the first stands.
        let mut kept: Vec<String> = Vec::new();
        for n in core::mem::take(&mut self.weapons) {
            if !kept.iter().any(|k| k.eq_ignore_ascii_case(&n)) {
                kept.push(n);
            }
        }
        self.weapons = kept;
        if !LOADING_TIME_RANGE.contains(&self.loading_time) {
            let fixed = self.loading_time.clamp(*LOADING_TIME_RANGE.start(), *LOADING_TIME_RANGE.end());
            w.push(format!(
                "loadout {:?}: loading time {} is not within {LOADING_TIME_RANGE:?}: {fixed}",
                self.name, self.loading_time
            ));
            self.loading_time = fixed;
        }
        w
    }
}

/// `name` as a loadout's name: trimmed, control characters dropped, at
/// most [`MAX_LOADOUT_NAME`] characters; [`DEFAULT_LOADOUT`] when nothing
/// is left.
pub fn clean_name(name: &str) -> String {
    let s: String = name.chars().filter(|c| !c.is_control()).collect();
    let s: String = s.trim().chars().take(MAX_LOADOUT_NAME).collect();
    let s = s.trim_end().to_string();
    if s.is_empty() { DEFAULT_LOADOUT.to_string() } else { s }
}

/// A mod's own loadouts: [`DEFAULT_LOADOUT`] first, then each of
/// `shipped` (by name) whose preset is in `presets` and enables a weapon
/// ([`MatchLoadout::shipped`]), in name order. `weapons` are the mod's
/// weapon names by id.
pub fn mod_loadouts(
    weapons: &[&str],
    presets: &BTreeMap<String, Preset>,
    shipped: &BTreeMap<String, ShippedLoadout>,
) -> Vec<MatchLoadout> {
    let mut out = alloc::vec![MatchLoadout::default()];
    for (name, s) in shipped {
        if name.as_str() == DEFAULT_LOADOUT {
            continue;
        }
        if let Some(l) = MatchLoadout::shipped(name, weapons, presets, *s) {
            out.push(l);
        }
    }
    out
}

/// Whether a worm's `slots` (weapon ids) keep the unique rule among the
/// `enabled` ids: no weapon twice, unless every enabled weapon is in a
/// slot already (fewer than five on: then repeats are allowed).
pub fn unique_ok(slots: &[i32], enabled: &[i32]) -> bool {
    let repeats = slots.iter().enumerate().any(|(i, w)| slots[..i].contains(w));
    !repeats || enabled.iter().all(|e| slots.contains(e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::restrictions::WeaponState::{Banned, Bonus, Enabled};
    use alloc::vec;

    fn presets() -> BTreeMap<String, Preset> {
        let mut p = BTreeMap::new();
        p.insert(
            "Rifle only".to_string(),
            [("Rifle", Enabled), ("Laser", Banned), ("Mine", Bonus)]
                .into_iter()
                .map(|(n, s)| (n.to_string(), s))
                .collect::<Preset>(),
        );
        p.insert(
            "Bonus only".to_string(),
            [("Rifle", Bonus), ("Laser", Bonus), ("Mine", Bonus), ("Zimm", Bonus)]
                .into_iter()
                .map(|(n, s)| (n.to_string(), s))
                .collect::<Preset>(),
        );
        p
    }

    const WEAPONS: [&str; 4] = ["Rifle", "Laser", "Mine", "Zimm"];

    #[test]
    fn shipped_loadouts_take_their_presets_weapons() {
        let s = ShippedLoadout { loading_time: 20, unique: false };
        let l = MatchLoadout::shipped("Rifle only", &WEAPONS, &presets(), s).unwrap();
        // Bonus is off; a weapon the preset lacks is on (updateList).
        assert_eq!(l.weapons, ["Rifle", "Zimm"]);
        assert_eq!(l.loading_time, 20);
        assert!(MatchLoadout::shipped("Bonus only", &WEAPONS, &presets(), s).is_none());
        assert!(MatchLoadout::shipped("Missing", &WEAPONS, &presets(), s).is_none());
    }

    #[test]
    fn default_comes_first_and_empty_presets_are_left_out() {
        let shipped: BTreeMap<String, ShippedLoadout> = [
            ("Rifle only".to_string(), ShippedLoadout { loading_time: 20, unique: false }),
            ("Bonus only".to_string(), ShippedLoadout { loading_time: 100, unique: false }),
            ("Gone".to_string(), ShippedLoadout { loading_time: 100, unique: false }),
        ]
        .into_iter()
        .collect();
        let names: Vec<String> =
            mod_loadouts(&WEAPONS, &presets(), &shipped).into_iter().map(|l| l.name).collect();
        assert_eq!(names, [DEFAULT_LOADOUT, "Rifle only"]);
        let d = MatchLoadout::default();
        assert_eq!(d.enabled(&WEAPONS), [true; 4]);
        assert_eq!((d.loading_time, d.unique), (100, false));
    }

    #[test]
    fn fitting_drops_unknown_weapons_and_clamps() {
        let mut l = MatchLoadout {
            name: "  Mine\u{7}s  ".into(),
            weapons: vec!["zimm".into(), "Nuke".into(), "RIFLE".into(), "Rifle".into()],
            loading_time: 900,
            unique: true,
        };
        let w = l.fit(&WEAPONS);
        assert_eq!(l.name, "Mines");
        assert_eq!(l.weapons, ["Rifle", "Zimm"]);
        assert_eq!(l.loading_time, 500);
        assert_eq!(w.len(), 3, "{w:?}");
        // Fitted again: nothing to say.
        assert!(l.fit(&WEAPONS).is_empty());
        let mut gone = MatchLoadout { weapons: vec!["Nuke".into()], ..MatchLoadout::default() };
        gone.fit(&WEAPONS);
        assert!(gone.weapons.is_empty());
        assert_eq!(clean_name(" \t "), DEFAULT_LOADOUT);
        assert_eq!(clean_name(&"x".repeat(99)).len(), MAX_LOADOUT_NAME);
    }

    #[test]
    fn unique_allows_repeats_only_once_every_weapon_is_used() {
        assert!(unique_ok(&[1, 2, 3, 4, 5], &[1, 2, 3, 4, 5, 6]));
        assert!(!unique_ok(&[1, 1, 3, 4, 5], &[1, 2, 3, 4, 5, 6]));
        // Three on: every one used, repeats allowed.
        assert!(unique_ok(&[1, 2, 3, 1, 1], &[1, 2, 3]));
        assert!(!unique_ok(&[1, 2, 1, 1, 1], &[1, 2, 3]));
    }
}
