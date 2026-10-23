//! Walks over a mod's spawns and projectiles with their paths and keys
//! (`olr-format`'s key rule: a shared projectile's key is its id, an
//! inline one's its `id`, else the weapon id for a shot, then
//! `<parent>.spawn` or `<parent>.trail`).

use std::collections::BTreeSet;

use olr_data::mod_def::{ChildLink, child_key};
use olr_format::mods::{
    Effect, Fire, Gfx, MAX_INLINE_DEPTH, ModSource, Projectile, Spawn, SpawnContext, Trail,
};

use super::path::{FieldPath, File, Seg};

fn name(n: &str) -> Seg {
    Seg::Name(n.into())
}

/// A spawn of the mod.
pub(crate) struct SpawnAt<'a> {
    /// The spawn's path, such as `weapons/bazooka.fire.shot`.
    pub path: FieldPath,
    pub shared: Option<&'a str>,
    pub projectile: Option<&'a Projectile>,
    /// The key an inline projectile here takes without an `id`.
    pub default_key: String,
}

/// A projectile of the mod: a shared one, or an inline one in place.
pub(crate) struct ProjectileAt<'a> {
    pub path: FieldPath,
    pub key: String,
    pub projectile: &'a Projectile,
}

#[derive(Default)]
pub(crate) struct Walk<'a> {
    pub spawns: Vec<SpawnAt<'a>>,
    pub projectiles: Vec<ProjectileAt<'a>>,
}

impl<'a> Walk<'a> {
    fn spawn<C: SpawnContext>(
        &mut self,
        s: &'a Spawn<C>,
        path: FieldPath,
        default_key: String,
        depth: usize,
    ) {
        if let Some(p) = s.projectile.as_deref() {
            let key = p.id.clone().unwrap_or_else(|| default_key.clone());
            self.projectile(p, path.join(name("projectile")), key, depth + 1);
        }
        self.spawns.push(SpawnAt {
            path,
            shared: s.shared.as_deref(),
            projectile: s.projectile.as_deref(),
            default_key,
        });
    }

    fn projectile(&mut self, p: &'a Projectile, path: FieldPath, key: String, depth: usize) {
        // Deeper than the format allows: an error `mod_errors` names.
        if depth > MAX_INLINE_DEPTH + 1 {
            return;
        }
        for (n, s) in &p.spawns {
            let at = path.join(name("spawns")).join(name(n));
            self.spawn(s, at, child_key(&key, ChildLink::Spawn), depth);
        }
        if let Trail::Projectile { spawn, .. } = &p.trail {
            self.spawn(
                spawn,
                path.join(name("trail")).join(name("spawn")),
                child_key(&key, ChildLink::Trail),
                depth,
            );
        }
        self.projectiles.push(ProjectileAt { path, key, projectile: p });
    }
}

/// Every spawn and projectile of `src`: the shared projectiles' trees,
/// then the weapons'.
pub(crate) fn walk(src: &ModSource) -> Walk<'_> {
    let mut w = Walk::default();
    for (id, p) in &src.shared {
        w.projectile(p, FieldPath::new(File::Shared(id.clone()), Vec::new()), id.clone(), 1);
    }
    for (id, weapon) in &src.weapons {
        if let Fire::Projectile { shot, .. } = &weapon.fire {
            let path = FieldPath::new(File::Weapon(id.clone()), vec![name("fire"), name("shot")]);
            w.spawn(shot, path, id.clone(), 0);
        }
    }
    w
}

/// Every projectile key of `src`.
pub(crate) fn keys(src: &ModSource) -> BTreeSet<String> {
    walk(src).projectiles.into_iter().map(|p| p.key).collect()
}

/// `base`, else `base-2`, `base-3`, … : the first `taken` lacks.
pub(crate) fn fresh(base: &str, taken: impl Fn(&str) -> bool) -> String {
    std::iter::once(base.to_string())
        .chain((2u32..).map(|i| format!("{base}-{i}")))
        .find(|c| !taken(c))
        .unwrap_or_default()
}

/// Gives the inline projectiles under `p` (`p` too, its key `key` when it
/// has no `id`) keys `taken` lacks, adding them: an `id` where the key is
/// taken. `root_fixed`: `p`'s key cannot change (a shared projectile's).
pub(crate) fn fresh_keys(
    p: &mut Projectile,
    key: &str,
    taken: &mut BTreeSet<String>,
    root_fixed: bool,
) -> Result<(), String> {
    fresh_keys_at(p, key, taken, root_fixed, 1)
}

fn fresh_keys_at(
    p: &mut Projectile,
    key: &str,
    taken: &mut BTreeSet<String>,
    fixed: bool,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_INLINE_DEPTH + 1 {
        return Ok(());
    }
    let mut key = p.id.clone().unwrap_or_else(|| key.to_string());
    if taken.contains(&key) {
        if fixed {
            return Err(format!("projectile id {key:?} is taken"));
        }
        key = fresh(&key, |k| taken.contains(k));
        p.id = Some(key.clone());
    }
    taken.insert(key.clone());
    for s in p.spawns.values_mut() {
        if let Some(c) = s.projectile.as_deref_mut() {
            fresh_keys_at(c, &child_key(&key, ChildLink::Spawn), taken, false, depth + 1)?;
        }
    }
    if let Trail::Projectile { spawn, .. } = &mut p.trail
        && let Some(c) = spawn.projectile.as_deref_mut()
    {
        fresh_keys_at(c, &child_key(&key, ChildLink::Trail), taken, false, depth + 1)?;
    }
    Ok(())
}

/// Calls `f` on `p` and every inline projectile under it.
pub(crate) fn each_projectile_mut(p: &mut Projectile, f: &mut dyn FnMut(&mut Projectile)) {
    each_at(p, f, 1)
}

fn each_at(p: &mut Projectile, f: &mut dyn FnMut(&mut Projectile), depth: usize) {
    if depth > MAX_INLINE_DEPTH + 1 {
        return;
    }
    f(p);
    for s in p.spawns.values_mut() {
        if let Some(c) = s.projectile.as_deref_mut() {
            each_at(c, f, depth + 1);
        }
    }
    if let Trail::Projectile { spawn, .. } = &mut p.trail
        && let Some(c) = spawn.projectile.as_deref_mut()
    {
        each_at(c, f, depth + 1);
    }
}

/// The shared projectiles the spawns of `p` and its inline projectiles name.
pub(crate) fn shared_refs(p: &Projectile) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    refs_at(p, &mut out, 1);
    out
}

fn refs_at(p: &Projectile, out: &mut BTreeSet<String>, depth: usize) {
    if depth > MAX_INLINE_DEPTH + 1 {
        return;
    }
    let mut visit = |shared: &Option<String>, inline: &Option<Box<Projectile>>| {
        out.extend(shared.iter().cloned());
        if let Some(c) = inline {
            refs_at(c, out, depth + 1);
        }
    };
    for s in p.spawns.values() {
        visit(&s.shared, &s.projectile);
    }
    if let Trail::Projectile { spawn, .. } = &p.trail {
        visit(&spawn.shared, &spawn.projectile);
    }
}

/// Renames the shared projectiles `p`'s own spawns name (not its inline
/// children's) by `rename`.
pub(crate) fn rename_shared_refs(p: &mut Projectile, rename: &dyn Fn(&str) -> Option<String>) {
    let fix = |shared: &mut Option<String>| {
        if let Some(new) = shared.as_deref().and_then(rename) {
            *shared = Some(new);
        }
    };
    for s in p.spawns.values_mut() {
        fix(&mut s.shared);
    }
    if let Trail::Projectile { spawn, .. } = &mut p.trail {
        fix(&mut spawn.shared);
    }
}

/// The pack paths of assets `p` names itself (its image, its effects'
/// sounds) with each field's steps from `p`.
pub(crate) fn projectile_assets(p: &Projectile) -> Vec<(String, Vec<Seg>)> {
    let mut out = Vec::new();
    if let Gfx::Image { file, .. } = &p.gfx {
        out.push((file.clone(), vec![name("gfx"), name("file")]));
    }
    let mut events: Vec<(Vec<Seg>, &Vec<Effect>)> =
        vec![(vec![name("hit_terrain")], &p.hit_terrain), (vec![name("hit_worm")], &p.hit_worm)];
    if let Some(t) = &p.timer {
        events.push((vec![name("timer"), name("effects")], &t.effects));
    }
    for (at, list) in events {
        for (i, e) in list.iter().enumerate() {
            let here = |field: &str| {
                let mut segs = at.clone();
                segs.extend([Seg::Index(i), name(field)]);
                segs
            };
            match e {
                Effect::Explode { sound: Some(s), .. } => out.push((s.clone(), here("sound"))),
                Effect::Sound { file: Some(f) } => out.push((f.clone(), here("file"))),
                _ => {}
            }
        }
    }
    out
}

/// Renames the assets `p` names itself by `rename`.
pub(crate) fn rename_projectile_assets(p: &mut Projectile, rename: &dyn Fn(&str) -> Option<String>) {
    let fix = |s: &mut String| {
        if let Some(new) = rename(s) {
            *s = new;
        }
    };
    if let Gfx::Image { file, .. } = &mut p.gfx {
        fix(file);
    }
    let timer = p.timer.as_mut().map(|t| &mut t.effects);
    for list in [Some(&mut p.hit_terrain), Some(&mut p.hit_worm), timer].into_iter().flatten() {
        for e in list {
            match e {
                Effect::Explode { sound: Some(s), .. } | Effect::Sound { file: Some(s) } => fix(s),
                _ => {}
            }
        }
    }
}

/// Every asset `src` names, with the path of the field naming it: the
/// weapons' firing sounds and every projectile's own.
pub(crate) fn assets(src: &ModSource) -> Vec<(String, FieldPath)> {
    let mut out = Vec::new();
    for (id, w) in &src.weapons {
        if let Fire::Projectile { sound: Some(s), .. } = &w.fire {
            out.push((
                s.clone(),
                FieldPath::new(File::Weapon(id.clone()), vec![name("fire"), name("sound")]),
            ));
        }
    }
    for p in walk(src).projectiles {
        for (asset, segs) in projectile_assets(p.projectile) {
            let mut path = p.path.clone();
            path.segs.extend(segs);
            out.push((asset, path));
        }
    }
    out
}
