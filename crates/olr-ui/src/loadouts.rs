//! Loadouts in the menus (issue #5; not OLX's): a mod's `Default` and
//! shipped loadouts (`olr_content::ModInfo::loadouts`, read-only) and the
//! player's own, kept per mod in the data directory's [`LOADOUTS_FILE`]
//! ([`OwnLoadouts`]). The menus choose one by name; [`resolve`] gives the
//! loadout to play, `Default` when the name is gone.

use std::collections::BTreeMap;

use olr_content::{DEFAULT_LOADOUT, MatchLoadout, ModInfo};
use olr_data::loadouts::{LOADING_TIME_RANGE, MAX_LOADOUT_NAME, clean_name};
use serde::{Deserialize, Serialize};

/// The own loadouts' file in the data directory (`Store::save_file`).
pub const LOADOUTS_FILE: &str = "loadouts.ron";

/// The most own loadouts of one mod the editor makes (a file may hold more:
/// none is dropped).
pub const MAX_OWN_LOADOUTS: usize = 64;

/// The player's own loadouts, by mod pack id, each mod's in the order made.
/// Weapons are kept by name as saved: a weapon a mod update removed stays
/// in the file and is left out only when the loadout is played
/// ([`resolve`]), so an update never loses a loadout.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OwnLoadouts {
    pub mods: BTreeMap<String, Vec<MatchLoadout>>,
}

impl OwnLoadouts {
    /// Mod `mod_id`'s own loadouts.
    pub fn of(&self, mod_id: &str) -> &[MatchLoadout] {
        self.mods.get(mod_id).map_or(&[], Vec::as_slice)
    }

    /// Mod `mod_id`'s own loadouts to change (an empty list made when it
    /// has none; [`OwnLoadouts::sanitize`] drops it again).
    pub fn of_mut(&mut self, mod_id: &str) -> &mut Vec<MatchLoadout> {
        self.mods.entry(mod_id.to_string()).or_default()
    }

    /// Fixes what a file may hold wrong, one warning each, never dropping
    /// a loadout: a name cleaned (`clean_name`), a name `Default` or one
    /// another loadout of the mod has (ignoring case) numbered, the loading
    /// time clamped into `LOADING_TIME_RANGE`; a mod without loadouts goes.
    pub fn sanitize(&mut self) -> Vec<String> {
        let mut w = Vec::new();
        self.mods.retain(|_, list| !list.is_empty());
        for (id, list) in &mut self.mods {
            let mut names: Vec<String> = vec![DEFAULT_LOADOUT.to_string()];
            for l in list.iter_mut() {
                let mut name = clean_name(&l.name);
                if names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                    name = numbered(&name, |n| names.iter().any(|m| m.eq_ignore_ascii_case(n)));
                }
                if name != l.name {
                    w.push(format!("loadouts: {id}: {:?} is now {name:?}", l.name));
                    l.name = name.clone();
                }
                names.push(name);
                let t = l.loading_time.clamp(*LOADING_TIME_RANGE.start(), *LOADING_TIME_RANGE.end());
                if t != l.loading_time {
                    w.push(format!(
                        "loadouts: {id}: {:?}'s loading time {} is now {t}",
                        l.name, l.loading_time
                    ));
                    l.loading_time = t;
                }
            }
        }
        w
    }
}

/// `base` with the first number from 2 that `taken` does not refuse
/// ("Rifles 2"), cut to [`MAX_LOADOUT_NAME`] characters.
fn numbered(base: &str, taken: impl Fn(&str) -> bool) -> String {
    (2..)
        .map(|k| {
            let suffix = format!(" {k}");
            let keep = MAX_LOADOUT_NAME.saturating_sub(suffix.chars().count());
            let cut: String = base.chars().take(keep).collect();
            format!("{}{suffix}", cut.trim_end())
        })
        .find(|n| !taken(n))
        .unwrap_or_default()
}

/// One of a mod's loadouts as the menus list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub loadout: MatchLoadout,
    /// `Default` or shipped with the mod: read-only.
    pub shipped: bool,
}

/// Mod `m`'s loadouts: `Default` and the shipped ones (read-only, the
/// mod's order), then the own ones (an own one whose name a shipped one
/// has, ignoring case, is not listed: a mod update shipped that name; the
/// file keeps it).
pub fn choices(m: &ModInfo, own: &OwnLoadouts) -> Vec<Choice> {
    let mut list: Vec<Choice> =
        m.loadouts.iter().map(|l| Choice { loadout: l.clone(), shipped: true }).collect();
    if !list.iter().any(|c| c.loadout.name == DEFAULT_LOADOUT) {
        list.insert(0, Choice { loadout: MatchLoadout::default(), shipped: true });
    }
    for l in own.of(&m.id) {
        if !list.iter().any(|c| c.shipped && c.loadout.name.eq_ignore_ascii_case(&l.name)) {
            list.push(Choice { loadout: l.clone(), shipped: false });
        }
    }
    list
}

/// The loadout of mod `m` named `name` ready to play, and what was wrong
/// (each to be logged once): a shipped one as the pack gives it; an own one
/// fitted to the mod's weapons (`MatchLoadout::fit`: a weapon the mod
/// lacks left out); `Default` when the mod has none of that name, or when
/// an own one keeps no weapon of the mod.
pub fn resolve(name: &str, m: &ModInfo, own: &OwnLoadouts) -> (MatchLoadout, Vec<String>) {
    let Some(c) = choices(m, own).into_iter().find(|c| c.loadout.name == name) else {
        let w = if name == DEFAULT_LOADOUT {
            Vec::new()
        } else {
            vec![format!("{}: no loadout {name:?}: playing {DEFAULT_LOADOUT}", m.name)]
        };
        return (MatchLoadout::default(), w);
    };
    if c.shipped {
        return (c.loadout, Vec::new());
    }
    let mut l = c.loadout;
    let mut w: Vec<String> = l.fit(&m.weapons).into_iter().map(|x| format!("{}: {x}", m.name)).collect();
    if l.weapons.is_empty() {
        w.push(format!(
            "{}: loadout {name:?} has none of the mod's weapons: playing {DEFAULT_LOADOUT}",
            m.name
        ));
        return (MatchLoadout::default(), w);
    }
    (l, w)
}

/// `name` made a loadout name for mod `m` (`clean_name`), or why it cannot
/// be one: empty, or a name another of its loadouts has, ignoring case
/// (`Default` and the shipped ones included); `except` is the loadout
/// being renamed.
pub fn check_name(
    name: &str,
    m: &ModInfo,
    own: &OwnLoadouts,
    except: Option<&str>,
) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("a loadout needs a name".into());
    }
    let clean = clean_name(name);
    for c in choices(m, own) {
        if Some(c.loadout.name.as_str()) == except || !c.loadout.name.eq_ignore_ascii_case(&clean) {
            continue;
        }
        return Err(if c.shipped {
            format!("{:?} is a loadout of the mod's", c.loadout.name)
        } else {
            format!("you have a loadout {:?} already", c.loadout.name)
        });
    }
    // An own loadout hidden by a shipped one of its name keeps it too.
    if own.of(&m.id).iter().any(|l| Some(l.name.as_str()) != except && l.name.eq_ignore_ascii_case(&clean)) {
        return Err(format!("you have a loadout {clean:?} already"));
    }
    Ok(clean)
}

/// A name for a new loadout of mod `m`: `base` when free, else numbered.
pub fn free_name(base: &str, m: &ModInfo, own: &OwnLoadouts) -> String {
    let base = clean_name(base);
    if check_name(&base, m, own, None).is_ok() {
        return base;
    }
    numbered(&base, |n| check_name(n, m, own, None).is_err())
}

/// `l`'s weapons written out by name in the mod's order (`Default`'s empty
/// list is every weapon), for an own loadout made from it.
pub fn explicit(l: &MatchLoadout, m: &ModInfo) -> MatchLoadout {
    let on = l.enabled(&m.weapons);
    let weapons = m.weapons.iter().zip(on).filter(|(_, on)| *on).map(|(w, _)| w.clone()).collect();
    MatchLoadout { weapons, ..l.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classic() -> ModInfo {
        let weapons: Vec<String> = ["Bazooka", "Rifle", "Napalm"].map(String::from).to_vec();
        let rifles = MatchLoadout {
            name: "Rifle only".into(),
            weapons: vec!["Rifle".into()],
            loading_time: 20,
            unique: false,
        };
        ModInfo {
            id: "olx-classic".into(),
            name: "Classic".into(),
            weapons,
            loadouts: vec![MatchLoadout::default(), rifles],
            ..ModInfo::default()
        }
    }

    fn own(l: &[(&str, &[&str])]) -> OwnLoadouts {
        let list = l
            .iter()
            .map(|(n, w)| MatchLoadout {
                name: (*n).into(),
                weapons: w.iter().map(|s| (*s).to_string()).collect(),
                loading_time: 70,
                unique: true,
            })
            .collect();
        OwnLoadouts { mods: [("olx-classic".to_string(), list)].into() }
    }

    #[test]
    fn default_first_then_shipped_then_own() {
        let m = classic();
        let o = own(&[("Mine", &["Napalm"]), ("rifle ONLY", &["Rifle"])]);
        let names: Vec<(String, bool)> =
            choices(&m, &o).into_iter().map(|c| (c.loadout.name, c.shipped)).collect();
        assert_eq!(
            names,
            [("Default".into(), true), ("Rifle only".into(), true), ("Mine".into(), false)],
            "an own one under a shipped name is not listed"
        );
    }

    #[test]
    fn a_missing_loadout_plays_default_and_unknown_weapons_go() {
        let m = classic();
        let o = own(&[("Mine", &["Napalm", "Laser"]), ("Gone", &["Laser"])]);
        let (l, w) = resolve("Mine", &m, &o);
        assert_eq!((l.weapons, l.loading_time, l.unique), (vec!["Napalm".to_string()], 70, true));
        assert_eq!(w.len(), 1, "{w:?}");
        let (l, w) = resolve("Gone", &m, &o);
        assert_eq!(l, MatchLoadout::default());
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(resolve("Nothing", &m, &o).0, MatchLoadout::default());
        assert_eq!(resolve("Default", &m, &o), (MatchLoadout::default(), vec![]));
        assert_eq!(resolve("Rifle only", &m, &o).0.loading_time, 20);
        // The file is untouched: the unknown weapon is still there.
        assert_eq!(o.of("olx-classic")[0].weapons.len(), 2);
    }

    #[test]
    fn names_are_unique_per_mod() {
        let m = classic();
        let o = own(&[("Mine", &["Napalm"])]);
        assert!(check_name("  ", &m, &o, None).is_err());
        assert!(check_name("default", &m, &o, None).unwrap_err().contains("the mod's"));
        assert!(check_name("RIFLE ONLY", &m, &o, None).unwrap_err().contains("the mod's"));
        assert!(check_name("mine", &m, &o, None).unwrap_err().contains("already"));
        assert_eq!(check_name("mine", &m, &o, Some("Mine")), Ok("mine".into()));
        assert_eq!(check_name("  New\tone ", &m, &o, None), Ok("Newone".into()));
        assert_eq!(free_name("Mine", &m, &o), "Mine 2");
        assert_eq!(free_name("Fresh", &m, &o), "Fresh");
        let long = "x".repeat(MAX_LOADOUT_NAME);
        let o = own(&[(&long, &["Napalm"])]);
        let n = free_name(&long, &m, &o);
        assert!(n.ends_with(" 2") && n.chars().count() == MAX_LOADOUT_NAME, "{n}");
    }

    #[test]
    fn a_damaged_file_keeps_every_loadout() {
        let mut o = own(&[("Default", &["Napalm"]), ("A", &[]), ("a", &[]), ("", &[])]);
        o.of_mut("olx-classic")[1].loading_time = 9000;
        o.of_mut("olx-mw-1-0");
        let w = o.sanitize();
        let names: Vec<&str> = o.of("olx-classic").iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Default 2", "A", "a 2", "Default 3"]);
        assert_eq!(o.of("olx-classic")[1].loading_time, 500);
        assert!(!o.mods.contains_key("olx-mw-1-0"));
        assert_eq!(w.len(), 4, "{w:?}");
    }

    #[test]
    fn explicit_writes_default_out() {
        let m = classic();
        let l = explicit(&MatchLoadout::default(), &m);
        assert_eq!(l.weapons, m.weapons);
        assert_eq!(l.loading_time, 100);
    }
}
