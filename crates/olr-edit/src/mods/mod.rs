//! Mod editing operations (issue #16): the mod editor's operations below
//! its UI, and `olr-cli mod`'s.
//!
//! Every operation works on a loaded mod [`Pack`] in memory and is
//! validated before it changes anything: a result with an error the mod
//! did not have ([`check`]'s, from `olr_format::mods::mod_errors` and the
//! missing assets) is refused with those errors, as is one that would not
//! read back from its canonical text exactly as set (a `NaN`). So what
//! LX56 cannot express (two effects in an event, a projectile cycle) is
//! refused with its path. `olr_edit::pack::save` writes the result.
//!
//! - [`get`] / [`set`] a value by [`path`]: `mod.worm.gravity`,
//!   `weapons/bazooka.fire.shot.speed`, through a spawn's shared or inline
//!   `projectile` alike; [`leaves`], [`children`], [`choices`].
//! - Weapons: [`add_weapon`], [`remove_weapon`], [`move_weapon`],
//!   [`copy_weapon`], [`rename_weapon`]; the mod: [`rename_mod`].
//! - Shared projectiles: [`used_by`], [`inline`], [`share`].
//! - [`check`], [`check_dir`]: every error and warning, located.

mod check;
mod node;
pub mod path;
mod shared;
mod walk;
mod weapons;

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use olr_format::mods::{ModSource, RESTRICTIONS_FILE, valid_id};
use olr_format::ron_io::{from_canonical, to_canonical};
use olr_vfs::VPath;
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use check::{Issue, check, check_dir, check_files};
pub use shared::{inline, share, used_by};
pub use weapons::{
    WeaponFrom, add_weapon, blank_weapon, copy_weapon, move_weapon, remove_weapon, rename_mod, rename_weapon,
};

use crate::pack::{Pack, Source};
use node::{Node, Step, ron_of};
use path::{FieldPath, File, Seg};

/// Why an operation changed nothing.
#[derive(Debug, thiserror::Error)]
pub enum ModError {
    #[error("not a mod pack")]
    NotAMod,
    /// A path that names nothing.
    #[error("{0}")]
    Path(String),
    /// A value that does not read as its field's type.
    #[error("{path}: {message}")]
    Value { path: String, message: String },
    /// An operation's own refusal.
    #[error("{0}")]
    Refused(String),
    /// The result would have these errors, which the mod did not have.
    #[error("{}", list(.0))]
    Invalid(Vec<Issue>),
}

fn list(issues: &[Issue]) -> String {
    issues.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("; ")
}

/// The mod source of `pack`.
pub fn source(pack: &Pack) -> Result<&ModSource, ModError> {
    match &pack.source {
        Source::Mod(m) => Ok(m),
        _ => Err(ModError::NotAMod),
    }
}

enum Found<'a> {
    Node(&'a dyn Node),
    Text(String),
}

fn root<'a>(src: &'a ModSource, file: &File) -> Result<&'a dyn Node, ModError> {
    Ok(match file {
        File::Mod => src,
        File::Restrictions => &src.restrictions,
        File::Weapon(id) => {
            &src.weapons
                .iter()
                .find(|(w, _)| w == id)
                .ok_or_else(|| ModError::Path(format!("no weapon {id:?}")))?
                .1
        }
        File::Shared(id) => {
            src.shared.get(id).ok_or_else(|| ModError::Path(format!("no shared projectile {id:?}")))?
        }
    })
}

fn root_mut<'a>(src: &'a mut ModSource, file: &File) -> Result<&'a mut dyn Node, ModError> {
    Ok(match file {
        File::Mod => src,
        File::Restrictions => &mut src.restrictions,
        File::Weapon(id) => {
            &mut src
                .weapons
                .iter_mut()
                .find(|(w, _)| w == id)
                .ok_or_else(|| ModError::Path(format!("no weapon {id:?}")))?
                .1
        }
        File::Shared(id) => {
            src.shared.get_mut(id).ok_or_else(|| ModError::Path(format!("no shared projectile {id:?}")))?
        }
    })
}

fn no_child(at: &FieldPath, seg: &Seg, n: &dyn Node) -> ModError {
    let have: Vec<String> = n.children().iter().map(|s| s.to_string()).collect();
    if have.is_empty() {
        ModError::Path(format!("{at}{seg}: {at} has no fields"))
    } else {
        ModError::Path(format!("{at}{seg}: no such field (has {})", have.join(" ")))
    }
}

/// Follows `path` from its file, into a shared projectile where a spawn
/// names one: the path as followed (in the shared projectile's file after
/// such a step) and what it names.
fn resolve<'a>(src: &'a ModSource, path: &FieldPath) -> Result<(FieldPath, Found<'a>), ModError> {
    let mut at = FieldPath::new(path.file.clone(), Vec::new());
    let mut found = Found::Node(root(src, &at.file)?);
    for seg in &path.segs {
        let Found::Node(n) = found else {
            return Err(ModError::Path(format!("{at}{seg}: {at} has no fields")));
        };
        match n.child(seg) {
            Some(Step::Node(c)) => {
                at.segs.push(seg.clone());
                found = Found::Node(c);
            }
            Some(Step::Text(t)) => {
                at.segs.push(seg.clone());
                found = Found::Text(t);
            }
            Some(Step::Shared(id)) => {
                at = FieldPath::new(File::Shared(id.to_string()), Vec::new());
                found = Found::Node(root(src, &at.file)?);
            }
            None => return Err(no_child(&at, seg, n)),
        }
    }
    Ok((at, found))
}

/// The node at `at`, a path as [`resolve`] gives it.
fn node_mut<'a>(src: &'a mut ModSource, at: &FieldPath) -> Result<&'a mut dyn Node, ModError> {
    let mut n = root_mut(src, &at.file)?;
    for (i, seg) in at.segs.iter().enumerate() {
        let here = FieldPath::new(at.file.clone(), at.segs[..=i].to_vec());
        n = n.child_mut(seg).ok_or_else(|| {
            ModError::Refused(format!(
                "{here} is read-only: the weapons' order is olr-cli mod weapon add|remove|move's"
            ))
        })?;
    }
    Ok(n)
}

fn parse_path(src: &ModSource, path: &str) -> Result<FieldPath, ModError> {
    path::parse(src, path).map_err(ModError::Path)
}

/// The value at `path` as canonical RON: a field (its default when the
/// file leaves it out), a list item, a map entry, or a whole file
/// (`weapons/bazooka`).
pub fn get(src: &ModSource, path: &str) -> Result<String, ModError> {
    let (at, found) = resolve(src, &parse_path(src, path)?)?;
    match found {
        Found::Node(n) => n.ron().map_err(|message| ModError::Value { path: at.to_string(), message }),
        Found::Text(t) => Ok(t),
    }
}

/// The paths of the children of `path` (fields, items, keys); empty for a
/// leaf.
pub fn children(src: &ModSource, path: &str) -> Result<Vec<String>, ModError> {
    let (at, found) = resolve(src, &parse_path(src, path)?)?;
    Ok(match found {
        Found::Node(n) => n.children().into_iter().map(|s| at.join(s).to_string()).collect(),
        Found::Text(_) => Vec::new(),
    })
}

/// The variants an enum at `path` offers (a `Fire`'s `Projectile`,
/// `Beam`, `Special`); never `Unknown`. Empty when it is no enum.
pub fn choices(src: &ModSource, path: &str) -> Result<Vec<&'static str>, ModError> {
    let (_, found) = resolve(src, &parse_path(src, path)?)?;
    Ok(match found {
        Found::Node(n) => n.choices().to_vec(),
        Found::Text(_) => Vec::new(),
    })
}

/// The variant of the enum at `path` (`Unknown` too), `None` when it is
/// no enum (or an absent option).
pub fn variant(src: &ModSource, path: &str) -> Result<Option<String>, ModError> {
    let (_, found) = resolve(src, &parse_path(src, path)?)?;
    Ok(match found {
        Found::Node(n) if !n.choices().is_empty() => {
            n.ron().ok().and_then(|r| node::variant_of(&r).map(String::from))
        }
        _ => None,
    })
}

/// Every leaf of every file of `src`, by path: each value without fields
/// (a number, a string, a unit variant, an absent option, an empty list).
/// A spawn's `projectile` naming a shared projectile is left to its file.
pub fn leaves(src: &ModSource) -> Vec<String> {
    let mut files = vec![File::Mod, File::Restrictions];
    files.extend(src.weapons.iter().map(|(id, _)| File::Weapon(id.clone())));
    files.extend(src.shared.keys().map(|id| File::Shared(id.clone())));
    let mut out = Vec::new();
    for f in files {
        if let Ok(n) = root(src, &f) {
            leaves_of(n, FieldPath::new(f, Vec::new()), &mut out);
        }
    }
    out
}

fn leaves_of(n: &dyn Node, at: FieldPath, out: &mut Vec<String>) {
    let kids = n.children();
    if kids.is_empty() {
        out.push(at.to_string());
    }
    for k in kids {
        match n.child(&k) {
            Some(Step::Node(c)) => leaves_of(c, at.join(k), out),
            Some(Step::Text(_)) => out.push(at.join(k).to_string()),
            Some(Step::Shared(_)) | None => {}
        }
    }
}

/// Sets the value at `path` to `value`, RON read as the field's type (a
/// bare string is taken as one: `Bazooka` for `"Bazooka"`). An enum given
/// one of its [`choices`] by name becomes that variant with its defaults
/// (`Beam`, `Smoke`). A missing map key is added (a new preset,
/// `restrictions["Mine"]`). A weapon's `name` and `mod.name` rename
/// ([`rename_weapon`], [`rename_mod`]); `mod.weapons` is read-only.
pub fn set(pack: &mut Pack, path: &str, value: &str) -> Result<(), ModError> {
    let src = source(pack)?;
    let wanted = parse_path(src, path)?;
    let name = || Seg::Name("name".into());
    let mut edited = src.clone();
    match resolve(src, &wanted) {
        Ok((at, found)) => {
            if at.segs == [name()] {
                match &at.file {
                    File::Weapon(id) => {
                        let id = id.clone();
                        let n = read_string(value)
                            .map_err(|message| ModError::Value { path: at.to_string(), message })?;
                        return rename_weapon(pack, &id, Some(&n), None);
                    }
                    File::Mod => {
                        let n = read_string(value)
                            .map_err(|message| ModError::Value { path: at.to_string(), message })?;
                        return rename_mod(pack, &n);
                    }
                    _ => {}
                }
            }
            if let Found::Text(t) = found {
                let same = node::parse_as::<Vec<String>>(value).ok().and_then(|v| ron_of(&v).ok());
                return if same.as_deref() == Some(t.as_str()) {
                    Ok(())
                } else {
                    Err(ModError::Refused(format!(
                        "{at} is read-only: the weapons' order is olr-cli mod weapon add|remove|move's"
                    )))
                };
            }
            let n = node_mut(&mut edited, &at)?;
            assign(n, value).map_err(|message| ModError::Value { path: at.to_string(), message })?;
            if let (File::Weapon(id), []) = (&at.file, at.segs.as_slice()) {
                weapons::renamed_whole(src, &mut edited, id)?;
            }
        }
        Err(e) => {
            // A new key of a map: its parent resolves.
            let (Some((Seg::Name(key), parent)), ModError::Path(_)) = (wanted.segs.split_last(), &e) else {
                return Err(e);
            };
            let parent = FieldPath::new(wanted.file.clone(), parent.to_vec());
            let Ok((at, Found::Node(_))) = resolve(src, &parent) else { return Err(e) };
            let n = node_mut(&mut edited, &at)?;
            let here = at.join(Seg::Name(key.clone())).to_string();
            match n.insert(key, value) {
                Some(Ok(())) => {}
                Some(Err(message)) => return Err(ModError::Value { path: here, message }),
                None => return Err(e),
            }
        }
    }
    commit(pack, edited, None)
}

/// `value` as a string: RON text (`"Big Nuke"`), else as it is.
fn read_string(value: &str) -> Result<String, String> {
    match node::parse_as::<String>(value.trim()) {
        Ok(s) => Ok(s),
        Err(_) if !value.trim_start().starts_with('"') => Ok(value.trim().to_string()),
        Err(e) => Err(e),
    }
}

/// `n` set to `value`: a choice by name, RON, or a bare string quoted.
fn assign(n: &mut dyn Node, value: &str) -> Result<(), String> {
    let v = value.trim();
    if n.choices().contains(&v) && n.set_variant(v) {
        return Ok(());
    }
    match n.parse(v) {
        Ok(()) => Ok(()),
        Err(e) => {
            if !v.starts_with('"') && ron_of(&v).is_ok_and(|q| n.parse(&q).is_ok()) {
                return Ok(());
            }
            match n.choices() {
                [] => Err(e),
                c => Err(format!("{e} (variants: {})", c.join(", "))),
            }
        }
    }
}

/// Whether each file of `edited` that differs from `old`'s reads back
/// from its canonical text as it is (`NaN` does not equal itself).
fn reads_back(old: &ModSource, edited: &ModSource) -> Result<(), ModError> {
    fn again<T: Serialize + DeserializeOwned + PartialEq>(file: &str, v: &T) -> Result<(), ModError> {
        let text = to_canonical(file, v).map_err(|e| ModError::Refused(e.to_string()))?;
        let back: T = from_canonical(file, &text).map_err(|e| ModError::Refused(e.to_string()))?;
        if back == *v {
            Ok(())
        } else {
            Err(ModError::Refused(format!(
                "{file}: the result would not read back as set (a value such as NaN does not equal itself)"
            )))
        }
    }
    let ids = |m: &ModSource| m.weapons.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
    if (&old.name, &old.notes, &old.worm, &old.rope, &old.loadouts, &old.look, ids(old))
        != (
            &edited.name,
            &edited.notes,
            &edited.worm,
            &edited.rope,
            &edited.loadouts,
            &edited.look,
            ids(edited),
        )
    {
        // As `mod.ron` holds them.
        let fields = (
            edited.name.clone(),
            edited.notes.clone(),
            edited.worm.clone(),
            edited.rope.clone(),
            edited.loadouts.clone(),
            edited.look.clone(),
        );
        again("mod.ron", &fields)?;
        if edited.weapons.iter().any(|(id, _)| !valid_id(id)) {
            return Err(ModError::Refused("an invalid weapon id".into()));
        }
    }
    for (id, w) in &edited.weapons {
        if !old.weapons.iter().any(|(i, v)| i == id && v == w) {
            again(&format!("weapons/{id}.ron"), w)?;
        }
    }
    for (id, p) in &edited.shared {
        if old.shared.get(id) != Some(p) {
            again(&format!("projectiles/{id}.ron"), p)?;
        }
    }
    if old.restrictions != edited.restrictions {
        again(RESTRICTIONS_FILE, &edited.restrictions)?;
    }
    Ok(())
}

/// An issue's identity, to compare a mod's errors before and after an edit.
fn identity(i: &Issue) -> (Option<String>, Option<String>, String) {
    (i.file.clone(), i.path.clone(), i.message.clone())
}

/// Makes `edited` (and `other`, the pack's other files, when given)
/// `pack`'s, unless it has an error `pack` does not, or does not read back
/// from its canonical files exactly.
pub(crate) fn commit(
    pack: &mut Pack,
    edited: ModSource,
    other: Option<BTreeMap<VPath, Vec<u8>>>,
) -> Result<(), ModError> {
    let old = source(pack)?;
    if *old == edited && other.as_ref().is_none_or(|o| *o == pack.other) {
        return Ok(());
    }
    let files = other.as_ref().unwrap_or(&pack.other);
    let assets: BTreeSet<String> = files.keys().map(|p| p.to_string()).collect();
    let after = check::source_issues(&edited, &assets);
    if !after.is_empty() {
        let before_assets: BTreeSet<String> = pack.other.keys().map(|p| p.to_string()).collect();
        let before: BTreeSet<_> = check::source_issues(old, &before_assets).iter().map(identity).collect();
        let new: Vec<Issue> = after.into_iter().filter(|i| !before.contains(&identity(i))).collect();
        if !new.is_empty() {
            return Err(ModError::Invalid(new));
        }
    }
    reads_back(old, &edited)?;
    pack.source = Source::Mod(edited);
    if let Some(other) = other {
        pack.other = other;
    }
    Ok(())
}
