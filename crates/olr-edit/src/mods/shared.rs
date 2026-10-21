//! Shared projectiles (`projectiles/<id>.ron`, issue #16): who uses each,
//! a spawn's shared projectile made inline (a copy for that spawn alone),
//! a spawn's inline projectile made shared.

use std::collections::BTreeMap;

use olr_format::mods::{ModSource, valid_id};

use super::path::{FieldPath, Seg};
use super::walk::{fresh_keys, keys, walk};
use super::{ModError, commit, node_mut, parse_path, resolve, source};
use crate::pack::Pack;

/// Each shared projectile's users: the paths of the spawns naming it (a
/// weapon's shot, a projectile's event spawn or trail, in shared
/// projectiles and weapons); an empty list for one nothing uses.
pub fn used_by(src: &ModSource) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> =
        src.shared.keys().map(|id| (id.clone(), Vec::new())).collect();
    for s in walk(src).spawns {
        if let Some(id) = s.shared {
            out.entry(id.to_string()).or_default().push(s.path.to_string());
        }
    }
    out
}

/// The spawn `path` names (a spawn's path, or its `projectile`'s), as
/// [`resolve`] follows it.
fn spawn_path(src: &ModSource, path: &str) -> Result<FieldPath, ModError> {
    let mut p = parse_path(src, path)?;
    if p.segs.last() == Some(&Seg::Name("projectile".into())) {
        p.segs.pop();
    }
    let (at, _) = resolve(src, &p)?;
    Ok(at)
}

/// Makes the shared projectile the spawn at `path` names its own inline
/// copy. The shared projectile goes when no other spawn names it. The
/// copy's key is the spawn's (an `id` only where that key is taken).
pub fn inline(pack: &mut Pack, path: &str) -> Result<(), ModError> {
    let src = source(pack)?;
    let at = spawn_path(src, path)?;
    let w = walk(src);
    let s = w
        .spawns
        .iter()
        .find(|s| s.path == at)
        .ok_or_else(|| ModError::Refused(format!("{at} is not a spawn")))?;
    let Some(id) = s.shared else {
        return Err(ModError::Refused(format!("{at}'s projectile is inline already")));
    };
    let copy = src.shared.get(id).ok_or_else(|| ModError::Refused(format!("no shared projectile {id:?}")))?;
    let others = w.spawns.iter().any(|o| o.shared == Some(id) && o.path != at);
    let (id, mut copy, key) = (id.to_string(), copy.clone(), s.default_key.clone());
    let mut edited = src.clone();
    set_spawn(&mut edited, &at, None, None)?;
    if !others {
        edited.shared.remove(&id);
    }
    let mut taken = keys(&edited);
    fresh_keys(&mut copy, &key, &mut taken, false).map_err(ModError::Refused)?;
    set_spawn(&mut edited, &at, None, Some(copy))?;
    commit(pack, edited, None)
}

/// Makes the inline projectile of the spawn at `path` the shared
/// projectile `id` (by default its key), named by the spawn. Returns the
/// id.
pub fn share(pack: &mut Pack, path: &str, id: Option<&str>) -> Result<String, ModError> {
    let src = source(pack)?;
    let at = spawn_path(src, path)?;
    let w = walk(src);
    let s = w
        .spawns
        .iter()
        .find(|s| s.path == at)
        .ok_or_else(|| ModError::Refused(format!("{at} is not a spawn")))?;
    let Some(p) = s.projectile else {
        let names = s.shared.unwrap_or_default();
        return Err(ModError::Refused(format!("{at} names shared projectile {names:?} already")));
    };
    let id = id.map(String::from).unwrap_or_else(|| p.id.clone().unwrap_or_else(|| s.default_key.clone()));
    if !valid_id(&id) {
        return Err(ModError::Refused(format!("{id:?} is not a projectile id")));
    }
    if src.shared.contains_key(&id) {
        return Err(ModError::Refused(format!("the mod has a shared projectile {id:?} already")));
    }
    let mut p = p.clone();
    p.id = None;
    let mut edited = src.clone();
    set_spawn(&mut edited, &at, Some(id.clone()), None)?;
    let mut taken = keys(&edited);
    fresh_keys(&mut p, &id, &mut taken, true).map_err(|e| ModError::Refused(format!("{id:?}: {e}")))?;
    edited.shared.insert(id.clone(), p);
    commit(pack, edited, None)?;
    Ok(id)
}

fn set_spawn(
    src: &mut ModSource,
    at: &FieldPath,
    shared: Option<String>,
    projectile: Option<olr_format::mods::Projectile>,
) -> Result<(), ModError> {
    let n = node_mut(src, at)?;
    let (s, p) = n.spawn_mut().ok_or_else(|| ModError::Refused(format!("{at} is not a spawn")))?;
    *s = shared;
    *p = projectile.map(Box::new);
    Ok(())
}
