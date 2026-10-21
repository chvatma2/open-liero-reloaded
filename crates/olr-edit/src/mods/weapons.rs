//! A mod's weapons (issue #16): add (blank or a copy of a weapon of
//! another mod, its shared projectiles and asset files copied along),
//! remove, move (a weapon's index is its id in the game), copy within the
//! mod, rename; and the mod's rename. A weapon's name is what
//! `restrictions.ron`'s presets match (ignoring ASCII case, trimmed, as
//! OLX's `CWpnRest`), and the shipped loadouts are their presets'
//! weapons: a rename carries through the presets, a removal drops the
//! weapon's entries, and a new weapon is in none (on in each, OLX's
//! `updateList`).

use std::collections::{BTreeMap, BTreeSet};

use olr_data::image::Rgba;
use olr_format::Colour;
use olr_format::mods::{Effect, Fire, Gfx, ModSource, Projectile, ShotSpawn, Weapon, WeaponClass, valid_id};
use olr_vfs::VPath;

use super::walk::{
    each_projectile_mut, fresh, fresh_keys, keys, projectile_assets, rename_projectile_assets,
    rename_shared_refs, shared_refs,
};
use super::{ModError, commit, source};
use crate::pack::Pack;

/// A new weapon's start: `name`'s gun, a yellow pixel that explodes on
/// terrain and injures a worm (`olr-cli pack new mod`'s weapon).
pub fn blank_weapon(name: &str) -> Weapon {
    let bullet = Projectile {
        hit_terrain: vec![Effect::Explode { damage: 2, shake: 0, sound: None }],
        hit_worm: vec![Effect::Injure { damage: 8 }],
        ..Projectile::new(Gfx::Pixels(vec![Colour(Rgba::rgb(255, 230, 64))]))
    };
    Weapon {
        name: name.into(),
        notes: String::new(),
        recharge: 0.5,
        drain: 10.0,
        between_shots_s: 0.1,
        laser_sight: false,
        fire: Fire::Projectile {
            class: WeaponClass::default(),
            recoil: 0,
            sound: None,
            shot: ShotSpawn { speed: 200, projectile: Some(Box::new(bullet)), ..ShotSpawn::default() },
        },
        look: None,
    }
}

/// What [`add_weapon`] starts from.
#[derive(Clone, Copy, Debug)]
pub enum WeaponFrom<'a> {
    /// [`blank_weapon`].
    Blank,
    /// The weapon `id` of the mod `pack` (another installed mod's).
    Copy { pack: &'a Pack, id: &'a str },
}

/// How presets compare weapon names (OLX's `CWpnRest::findWeapon`).
fn same_name(a: &str, b: &str) -> bool {
    let t = |s: &str| s.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')).to_string();
    t(a).eq_ignore_ascii_case(&t(b))
}

fn index(src: &ModSource, id: &str) -> Result<usize, ModError> {
    src.weapons
        .iter()
        .position(|(w, _)| w == id)
        .ok_or_else(|| ModError::Refused(format!("no weapon {id:?}")))
}

/// A new weapon id: a valid id no weapon has.
fn check_id(src: &ModSource, id: &str) -> Result<(), ModError> {
    if !valid_id(id) {
        return Err(ModError::Refused(format!(
            "{id:?} is not a weapon id (lowercase ASCII, digits, -, _ and ., not starting with .)"
        )));
    }
    if src.weapons.iter().any(|(w, _)| w == id) {
        return Err(ModError::Refused(format!("the mod has a weapon {id:?} already")));
    }
    Ok(())
}

/// A weapon name: not empty, no spaces around it or control characters,
/// and no other weapon's (but weapon `except`'s) as presets compare them.
fn check_name(src: &ModSource, name: &str, except: Option<usize>) -> Result<(), ModError> {
    if name.trim().is_empty() || name.trim() != name || name.chars().any(char::is_control) {
        return Err(ModError::Refused(format!(
            "{name:?} is not a weapon name (not empty, no spaces around it, no control characters)"
        )));
    }
    if let Some((id, _)) = src
        .weapons
        .iter()
        .enumerate()
        .find(|(i, (_, w))| Some(*i) != except && same_name(&w.name, name))
        .map(|(_, w)| w)
    {
        return Err(ModError::Refused(format!(
            "weapon {id:?} is named {name:?} already: presets match names"
        )));
    }
    Ok(())
}

/// `base`, else `base 2`, `base 3`, …: a name no weapon has.
fn unique_name(src: &ModSource, base: &str) -> String {
    std::iter::once(base.to_string())
        .chain((2u32..).map(|i| format!("{base} {i}")))
        .find(|n| !src.weapons.iter().any(|(_, w)| same_name(&w.name, n)))
        .unwrap_or_default()
}

/// Gives the inline projectiles of `w`'s shot keys the rest of `src` does
/// not use (`w` is to be weapon `id`).
fn fresh_weapon_keys(src: &ModSource, id: &str, w: &mut Weapon) -> Result<(), ModError> {
    if let Fire::Projectile { shot, .. } = &mut w.fire
        && let Some(p) = shot.projectile.as_deref_mut()
    {
        let mut taken = keys(src);
        fresh_keys(p, id, &mut taken, false).map_err(ModError::Refused)?;
    }
    Ok(())
}

/// Adds weapon `id` at index `at` (the end by default) from `from`, named
/// `name` (by default the copied weapon's, or "New weapon", numbered when
/// taken). A copy brings the shared projectiles it uses (the same
/// projectile under its id, else under a new one) and its asset files
/// (the same file kept, else a new name); inline projectile ids that clash
/// are renumbered.
pub fn add_weapon(
    pack: &mut Pack,
    id: &str,
    name: Option<&str>,
    from: WeaponFrom<'_>,
    at: Option<usize>,
) -> Result<(), ModError> {
    let src = source(pack)?;
    check_id(src, id)?;
    let at = at.unwrap_or(src.weapons.len());
    if at > src.weapons.len() {
        return Err(ModError::Refused(format!("index {at} is past the end ({} weapons)", src.weapons.len())));
    }
    let mut edited = src.clone();
    let mut other = None;
    let mut weapon = match from {
        WeaponFrom::Blank => blank_weapon(&unique_name(src, "New weapon")),
        WeaponFrom::Copy { pack: from, id: from_id } => {
            let (w, shared, files) = bring(pack, from, from_id)?;
            edited.shared.extend(shared);
            other = Some(files);
            w
        }
    };
    if let Some(n) = name {
        weapon.name = n.to_string();
    } else if matches!(from, WeaponFrom::Copy { .. }) {
        weapon.name = unique_name(src, &weapon.name);
    }
    check_name(src, &weapon.name, None)?;
    fresh_weapon_keys(&edited, id, &mut weapon)?;
    edited.weapons.insert(at, (id.to_string(), weapon));
    commit(pack, edited, other)
}

/// Weapon `id` of `from` for `to`: the weapon with its shared projectiles
/// and asset paths renamed where `to` has others of those names, the
/// shared projectiles `to` lacks, and `to`'s other files with the asset
/// files it lacks.
#[allow(clippy::type_complexity)]
fn bring(
    to: &Pack,
    from: &Pack,
    id: &str,
) -> Result<(Weapon, BTreeMap<String, Projectile>, BTreeMap<VPath, Vec<u8>>), ModError> {
    let dest = source(to)?;
    let other = source(from)?;
    let mut weapon = other.weapons[index(other, id)?].1.clone();
    // The shared projectiles it uses, each with those it uses itself.
    let mut uses: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut todo: Vec<String> = match &weapon.fire {
        Fire::Projectile { shot, .. } => {
            let mut first: BTreeSet<String> = shot.shared.iter().cloned().collect();
            if let Some(p) = &shot.projectile {
                first.extend(shared_refs(p));
            }
            first.into_iter().collect()
        }
        _ => Vec::new(),
    };
    while let Some(s) = todo.pop() {
        if uses.contains_key(&s) {
            continue;
        }
        let Some(p) = other.shared.get(&s) else { continue }; // dangling: the check names it
        let d = shared_refs(p);
        todo.extend(d.iter().cloned());
        uses.insert(s, d);
    }
    let mut shared: BTreeMap<String, Projectile> =
        uses.keys().map(|s| (s.clone(), other.shared[s].clone())).collect();

    // Asset files: the same file kept, a missing one copied, another of
    // the same name copied under a new name (`gfx/flame-2.png`, or one
    // holding the same bytes already).
    let mut paths: BTreeSet<String> = BTreeSet::new();
    if let Fire::Projectile { sound: Some(s), .. } = &weapon.fire {
        paths.insert(s.clone());
    }
    let mut note = |p: &mut Projectile| paths.extend(projectile_assets(p).into_iter().map(|(a, _)| a));
    if let Fire::Projectile { shot, .. } = &mut weapon.fire
        && let Some(p) = shot.projectile.as_deref_mut()
    {
        each_projectile_mut(p, &mut note);
    }
    for p in shared.values_mut() {
        each_projectile_mut(p, &mut note);
    }
    let mut files = to.other.clone();
    let mut moved: BTreeMap<String, String> = BTreeMap::new();
    for a in &paths {
        let Ok(path) = VPath::new(a) else { continue };
        let Some(bytes) = from.other.get(&path) else { continue }; // missing: the check names it
        match files.get(&path) {
            Some(b) if b == bytes => {}
            None => {
                files.insert(path, bytes.clone());
            }
            Some(_) => {
                let (stem, ext) = match a.rfind('.').filter(|&d| d > a.rfind('/').map_or(0, |s| s + 1)) {
                    Some(d) => (&a[..d], &a[d..]),
                    None => (a.as_str(), ""),
                };
                for i in 2u32.. {
                    let c = format!("{stem}-{i}{ext}");
                    let Ok(cp) = VPath::new(&c) else { break };
                    match files.get(&cp) {
                        Some(b) if b == bytes => {}
                        Some(_) => continue,
                        None => {
                            files.insert(cp, bytes.clone());
                        }
                    }
                    moved.insert(a.clone(), c);
                    break;
                }
            }
        }
    }
    let rename_asset = |s: &str| moved.get(s).cloned();
    if let Fire::Projectile { sound, shot, .. } = &mut weapon.fire {
        if let Some(new) = sound.as_deref().and_then(rename_asset) {
            *sound = Some(new);
        }
        if let Some(p) = shot.projectile.as_deref_mut() {
            each_projectile_mut(p, &mut |q| rename_projectile_assets(q, &rename_asset));
        }
    }
    for p in shared.values_mut() {
        each_projectile_mut(p, &mut |q| rename_projectile_assets(q, &rename_asset));
    }

    // Kept: the same projectile under the same id, using only kept ones.
    let mut kept: BTreeSet<String> =
        shared.iter().filter(|(s, p)| dest.shared.get(*s) == Some(p)).map(|(s, _)| s.clone()).collect();
    loop {
        let drop: Vec<String> = kept
            .iter()
            .filter(|s| uses[*s].iter().any(|u| uses.contains_key(u) && !kept.contains(u)))
            .cloned()
            .collect();
        if drop.is_empty() {
            break;
        }
        for s in drop {
            kept.remove(&s);
        }
    }
    let taken = keys(dest);
    let mut renamed: BTreeMap<String, String> = BTreeMap::new();
    for s in uses.keys().filter(|s| !kept.contains(*s)) {
        let new = fresh(s, |k| {
            dest.shared.contains_key(k) || taken.contains(k) || renamed.values().any(|v| v == k)
        });
        renamed.insert(s.clone(), new);
    }
    let rename = |s: &str| renamed.get(s).cloned();
    let mut copied: BTreeMap<String, Projectile> = BTreeMap::new();
    for (old, new) in &renamed {
        let mut p = shared.remove(old).unwrap_or_else(|| other.shared[old].clone());
        each_projectile_mut(&mut p, &mut |q| rename_shared_refs(q, &rename));
        copied.insert(new.clone(), p);
    }
    if let Fire::Projectile { shot, .. } = &mut weapon.fire {
        if let Some(new) = shot.shared.as_deref().and_then(rename) {
            shot.shared = Some(new);
        }
        if let Some(p) = shot.projectile.as_deref_mut() {
            each_projectile_mut(p, &mut |q| rename_shared_refs(q, &rename));
        }
    }
    // The copied shared projectiles' inline children take free keys.
    let mut all_taken = keys(dest);
    all_taken.extend(copied.keys().cloned());
    for (id, p) in &mut copied {
        all_taken.remove(id);
        fresh_keys(p, id, &mut all_taken, true).map_err(ModError::Refused)?;
    }
    Ok((weapon, copied, files))
}

/// Removes weapon `id` and its name's entries from the presets (when no
/// other weapon has that name). The mod keeps at least one weapon. The
/// shared projectiles only it used stay (`check` warns that nothing uses
/// them).
pub fn remove_weapon(pack: &mut Pack, id: &str) -> Result<(), ModError> {
    let src = source(pack)?;
    let i = index(src, id)?;
    if src.weapons.len() == 1 {
        return Err(ModError::Refused("a mod keeps at least one weapon".into()));
    }
    let mut edited = src.clone();
    let (_, gone) = edited.weapons.remove(i);
    if !edited.weapons.iter().any(|(_, w)| same_name(&w.name, &gone.name)) {
        for preset in edited.restrictions.values_mut() {
            preset.retain(|k, _| !same_name(k, &gone.name));
        }
    }
    commit(pack, edited, None)
}

/// Moves weapon `id` to index `to` (its id in the game), the others
/// keeping their order.
pub fn move_weapon(pack: &mut Pack, id: &str, to: usize) -> Result<(), ModError> {
    let src = source(pack)?;
    let i = index(src, id)?;
    if to >= src.weapons.len() {
        return Err(ModError::Refused(format!("index {to} is past the last ({})", src.weapons.len() - 1)));
    }
    let mut edited = src.clone();
    let w = edited.weapons.remove(i);
    edited.weapons.insert(to, w);
    commit(pack, edited, None)
}

/// Copies weapon `id` as `new_id` right after it, named `name` (by default
/// its name, numbered). Shared projectiles stay shared; inline projectile
/// ids are renumbered.
pub fn copy_weapon(pack: &mut Pack, id: &str, new_id: &str, name: Option<&str>) -> Result<(), ModError> {
    let src = source(pack)?;
    let i = index(src, id)?;
    check_id(src, new_id)?;
    let mut w = src.weapons[i].1.clone();
    w.name = match name {
        Some(n) => n.to_string(),
        None => unique_name(src, &w.name),
    };
    check_name(src, &w.name, None)?;
    let mut edited = src.clone();
    fresh_weapon_keys(&edited, new_id, &mut w)?;
    edited.weapons.insert(i + 1, (new_id.to_string(), w));
    commit(pack, edited, None)
}

/// Renames weapon `id`: its name to `name` (every preset's entry for the
/// old name follows; the shipped loadouts are their presets' weapons)
/// and its id to `new_id` (its file; the index, its id in the game, stays).
pub fn rename_weapon(
    pack: &mut Pack,
    id: &str,
    name: Option<&str>,
    new_id: Option<&str>,
) -> Result<(), ModError> {
    let src = source(pack)?;
    let i = index(src, id)?;
    let mut edited = src.clone();
    if let Some(name) = name {
        check_name(src, name, Some(i))?;
        let old = std::mem::replace(&mut edited.weapons[i].1.name, name.to_string());
        carry_name(&mut edited, &old, name);
    }
    if let Some(new_id) = new_id.filter(|n| *n != id) {
        check_id(src, new_id)?;
        edited.weapons[i].0 = new_id.to_string();
    }
    commit(pack, edited, None)
}

/// The presets' entries for weapon name `old` (as presets compare names)
/// become one entry for `new`, with the first one's state.
fn carry_name(src: &mut ModSource, old: &str, new: &str) {
    if old == new {
        return;
    }
    for preset in src.restrictions.values_mut() {
        let entries: Vec<String> = preset.keys().filter(|k| same_name(k, old)).cloned().collect();
        if let Some(state) = entries.first().and_then(|k| preset.get(k)).copied() {
            for k in &entries {
                preset.remove(k);
            }
            preset.insert(new.to_string(), state);
        }
    }
}

/// After weapon `id` of `edited` was set whole: a new name is checked and
/// carried through the presets as [`rename_weapon`] does.
pub(crate) fn renamed_whole(src: &ModSource, edited: &mut ModSource, id: &str) -> Result<(), ModError> {
    let i = index(src, id)?;
    let (old, new) = (src.weapons[i].1.name.clone(), edited.weapons[i].1.name.clone());
    if old != new {
        check_name(src, &new, Some(i))?;
        carry_name(edited, &old, &new);
    }
    Ok(())
}

/// Renames the mod: `mod.ron`'s name and the pack's.
pub fn rename_mod(pack: &mut Pack, name: &str) -> Result<(), ModError> {
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        return Err(ModError::Refused(format!("{name:?} is not a mod name")));
    }
    let mut edited = source(pack)?.clone();
    edited.name = name.to_string();
    commit(pack, edited, None)?;
    pack.manifest.name = name.to_string();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_as_presets_do() {
        assert!(same_name(" Big nuke\t", "Big Nuke"));
        assert!(!same_name("Big Nuke", "Big Nuke 2"));
    }
}
