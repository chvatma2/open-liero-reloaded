//! Look operations (look system, plan 15a Task 7): a level's look
//! settings, its glow groups and its light objects. Each edits a copy and
//! keeps it only when the level still checks (`compile_level`, which checks
//! the look and the glow); a refused edit changes nothing. Group removal and
//! renaming act on the pack: the composition's elements name groups.

use std::collections::BTreeSet;

use olr_format::levels::{LevelObject, LevelSource, MAX_OBJECTS, compile_level, composition_paths};
use olr_format::look::{GlowGroupSource, LightSource, LookSource, valid_group_id};

use crate::level::EditError;
use crate::pack::{Pack, Source};

/// `edit` on a copy of `level`, kept when the copy checks.
fn checked<T>(
    level: &mut LevelSource,
    edit: impl FnOnce(&mut LevelSource) -> Result<T, EditError>,
) -> Result<T, EditError> {
    let mut copy = level.clone();
    let out = edit(&mut copy)?;
    compile_level(&copy).map_err(|e| EditError::Look(e.to_string()))?;
    *level = copy;
    Ok(out)
}

/// Replaces the level's look (its settings and groups); `None` removes it.
/// The glow and the elements keep naming groups by index and id: removing
/// a look whose groups are named is refused by the check.
pub fn set_look(level: &mut LevelSource, look: Option<LookSource>) -> Result<(), EditError> {
    checked(level, |l| {
        l.look = look;
        Ok(())
    })
}

/// Adds a glow group at the end; returns its index. Its id must be new.
pub fn add_group(level: &mut LevelSource, group: GlowGroupSource) -> Result<usize, EditError> {
    checked(level, |l| {
        let look = l.look.get_or_insert_with(LookSource::default);
        if look.glow_groups.iter().any(|g| g.id == group.id) {
            return Err(EditError::Look(format!("a glow group {:?} exists already", group.id)));
        }
        look.glow_groups.push(group);
        Ok(look.glow_groups.len() - 1)
    })
}

/// Replaces glow group `id`'s fields; `group.id` must be `id` (see
/// [`rename_group`]).
pub fn set_group(level: &mut LevelSource, id: &str, group: GlowGroupSource) -> Result<(), EditError> {
    checked(level, |l| {
        let g = find_group(l, id)?;
        if group.id != id {
            return Err(EditError::Look(format!("set keeps the id {id:?}; rename it apart")));
        }
        *g = group;
        Ok(())
    })
}

fn find_group<'a>(l: &'a mut LevelSource, id: &str) -> Result<&'a mut GlowGroupSource, EditError> {
    l.look
        .as_mut()
        .and_then(|look| look.glow_groups.iter_mut().find(|g| g.id == id))
        .ok_or_else(|| EditError::Look(format!("no glow group {id:?}")))
}

fn level_mut(pack: &mut Pack) -> Result<&mut LevelSource, EditError> {
    match &mut pack.source {
        Source::Level(l) => Ok(l),
        _ => Err(EditError::Look("not a level pack".into())),
    }
}

/// Renames glow group `from` to `to` (a new slug), in the look and in every
/// element naming it.
pub fn rename_group(pack: &mut Pack, from: &str, to: &str) -> Result<(), EditError> {
    if !valid_group_id(to) {
        return Err(EditError::Look(format!("{to:?} is not a glow group id (a slug of [a-z0-9_.-])")));
    }
    checked(level_mut(pack)?, |l| {
        if l.look.as_ref().is_some_and(|look| look.glow_groups.iter().any(|g| g.id == to)) {
            return Err(EditError::Look(format!("a glow group {to:?} exists already")));
        }
        find_group(l, from)?.id = to.into();
        Ok(())
    })?;
    if let Some(src) = &mut pack.composition {
        for g in src.composition.elements.iter_mut().filter_map(|e| e.glow.as_mut()) {
            if g.group == from {
                g.group = to.into();
            }
        }
    }
    Ok(())
}

/// Removes glow group `id`: the level's glow loses its pixels (strength 0,
/// group 0) and the groups above it move down one; every element naming it
/// loses its glow (and mask), locked or not. Returns those elements' ids.
/// A composed level is rebaked by the caller.
pub fn remove_group(pack: &mut Pack, id: &str) -> Result<Vec<String>, EditError> {
    checked(level_mut(pack)?, |l| {
        let look = l.look.as_mut().ok_or_else(|| EditError::Look(format!("no glow group {id:?}")))?;
        let k = look
            .glow_groups
            .iter()
            .position(|g| g.id == id)
            .ok_or_else(|| EditError::Look(format!("no glow group {id:?}")))?;
        look.glow_groups.remove(k);
        let (s, w) = (l.art_scale as usize, l.size.0 as usize);
        if let Some(g) = l.glow.as_mut() {
            let aw = g.art.0 as usize;
            for (groups, strength) in [(&mut g.groups, &mut g.front), (&mut g.groups_back, &mut g.back)] {
                for (i, v) in groups.iter_mut().enumerate() {
                    match usize::from(*v) {
                        x if x == k => {
                            *v = 0;
                            let (x, y) = (i % w, i / w);
                            for ay in y * s..(y + 1) * s {
                                strength[ay * aw + x * s..ay * aw + (x + 1) * s].fill(0);
                            }
                        }
                        x if x > k => *v -= 1,
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    })?;
    let mut unglowed = Vec::new();
    if let Some(src) = &mut pack.composition {
        for e in &mut src.composition.elements {
            if e.glow.as_ref().is_some_and(|g| g.group == id) {
                e.glow = None;
                unglowed.push(e.id.clone());
            }
        }
        let named: BTreeSet<String> = composition_paths(src).into_iter().collect();
        src.files.retain(|f, _| named.contains(f));
    }
    Ok(unglowed)
}

/// Adds a light object at `at` (world pixels); returns its index.
pub fn add_light(
    level: &mut LevelSource,
    at: (i32, i32),
    light: LightSource,
    radius: u8,
) -> Result<usize, EditError> {
    checked(level, |l| {
        if l.objects.len() >= MAX_OBJECTS {
            return Err(EditError::TooManyObjects);
        }
        l.objects.push(LevelObject::light(at, radius, light));
        Ok(l.objects.len() - 1)
    })
}

/// Replaces light object `index` (its place, light and radius; its notes kept).
pub fn set_light(
    level: &mut LevelSource,
    index: usize,
    at: (i32, i32),
    light: LightSource,
    radius: u8,
) -> Result<(), EditError> {
    checked(level, |l| {
        let len = l.objects.len();
        let o = l.objects.get_mut(index).ok_or(EditError::NoObject { index, len })?;
        let LevelObject::Light { notes, .. } = o else {
            return Err(EditError::Look(format!("object {index} is not a light")));
        };
        let notes = std::mem::take(notes);
        let mut new = LevelObject::light(at, radius, light);
        if let LevelObject::Light { notes: n, .. } = &mut new {
            *n = notes;
        }
        *o = new;
        Ok(())
    })
}
