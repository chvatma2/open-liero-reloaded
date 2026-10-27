//! The operations on a level's placed copies (issue #75): adding,
//! duplicating, moving, resizing, setting, ordering and removing copies,
//! and the copies' and the level's scripts. The level tab (#82, #84) and
//! `olr-cli level copy` share them, as `element`'s serve `level element`.
//! Each edits a copy of the pack's placement (`pack::placement`), checks it
//! (`check_placed`) and keeps it only then: a refusal (`PlacedError`,
//! naming the copy) leaves the pack unchanged. None bakes: callers do.

use std::collections::{BTreeMap, BTreeSet};

use olr_format::assets::{AssetSource, MAX_SCRIPT_BYTES, ValueSource};
use olr_format::levels::{
    AssetOrigin, EDITOR_DIR, GROUND_KEY, LevelAsset, LevelSource, MAX_COPIES, MAX_COPY_ID, MAX_LEVEL_ASSETS,
    PlacedCopy, PlacedSource, RESERVED_KEYS, SCRIPTS_DIR, check_placed, copy_script_file, gather_groups,
    gather_materials, next_key, valid_copy_id, valid_key,
};

use super::PlacedError;
use crate::asset::{asset_slug, decode, decode_own, live_refusal, posed};
use crate::pack::{Pack, Source, UNCONVERTED, ground_alone, ground_composition, placement};

/// What [`add_copy`] did: the copy's id, its asset's key, whether the
/// asset is new to the level, and the look's changes the next bake makes
/// (the glow groups and materials gathered new to the level's look, by
/// their ids there; whether a look is added to hold them).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Added {
    pub id: String,
    pub key: String,
    pub new_asset: bool,
    pub groups_added: Vec<String>,
    pub materials_added: Vec<String>,
    pub added_look: bool,
}

/// What [`remove_copy`] removed, so callers can say it and the editor can
/// undo it: the copy and its place in the stack, its script, and its
/// asset with its folder when no copy used it any more.
#[derive(Clone, Debug, PartialEq)]
pub struct Removed {
    pub copy: PlacedCopy,
    pub index: usize,
    pub script: Option<String>,
    pub asset: Option<(LevelAsset, AssetSource)>,
}

/// A copy's placed size ([`resize_copy`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// The asset's own size (`size` none).
    Natural,
    /// This width, the height following the asset's aspect.
    Width(u32),
    /// This height, the width following the asset's aspect.
    Height(u32),
    /// Both sides, the aspect free (`free_aspect` true).
    Free(u32, u32),
}

/// A copy's setting ([`set_copy`]).
#[derive(Clone, Debug, PartialEq)]
pub enum CopyProperty {
    /// Its id; its script file is renamed with it (scripts naming the old
    /// id are text, not rewritten).
    Id(String),
    Live(bool),
    /// Its starting state (none: the asset's start).
    State(Option<String>),
    /// Its value of an exposed property (none: the default).
    Value(String, Option<ValueSource>),
    DefaultScript(bool),
    /// Placed distant at this depth, 1 to 99 (none: near).
    Distant(Option<u8>),
    /// The aspect free; false snaps the height to the width's aspect.
    FreeAspect(bool),
    Locked(bool),
}

/// `size` of an asset of `asset` (world pixels) as [`Size`] makes it: `None`
/// for `Natural`; a `Width(w)` takes height `max(1, round(w × h0 / w0))`
/// (half up), a `Height` likewise, a `Free` size as it is. The level tab's
/// handles compute what `resize_copy` would with it.
pub fn keep_aspect(asset: (u32, u32), size: Size) -> Option<(u32, u32)> {
    let follow = |v: u32, num: u32, den: u32| {
        let (v, num, den) = (u64::from(v), u64::from(num), u64::from(den.max(1)));
        u32::try_from((2 * v * num + den) / (2 * den)).unwrap_or(u32::MAX).max(1)
    };
    match size {
        Size::Natural => None,
        Size::Width(w) => Some((w, follow(w, asset.1, asset.0))),
        Size::Height(h) => Some((follow(h, asset.0, asset.1), h)),
        Size::Free(w, h) => Some((w, h)),
    }
}

fn copy_err(copy: &str, message: impl Into<String>) -> PlacedError {
    PlacedError::Copy { copy: copy.into(), message: message.into() }
}

fn level(pack: &Pack) -> Result<&LevelSource, PlacedError> {
    match &pack.source {
        Source::Level(l) => Ok(l),
        _ => Err(PlacedError::Level("not a level pack".into())),
    }
}

/// Applies `edit` to a copy of `pack`'s placement and keeps it only when it
/// checks (`check_placed`). The pack then holds the placement; its
/// composition is the ground's when the result is the ground alone (and
/// the look's glow groups and materials then the ground's, as the next
/// bake gathers them), else none.
pub(crate) fn commit<T>(
    pack: &mut Pack,
    edit: impl FnOnce(&mut PlacedSource) -> Result<T, PlacedError>,
) -> Result<T, PlacedError> {
    if pack.legacy_composition {
        return Err(PlacedError::Level(UNCONVERTED.into()));
    }
    let l = level(pack)?;
    let Some(mut src) = placement(pack) else {
        return Err(PlacedError::Level(
            "a plain level: it places no copies (give it its ground first)".into(),
        ));
    };
    let out = edit(&mut src)?;
    check_placed(&src, l.size, l.art_scale)?;
    let alone = ground_alone(&src, l);
    let ground = src.assets.get(GROUND_KEY).filter(|_| alone);
    pack.composition = ground.map(ground_composition);
    if let (Some(g), Source::Level(l)) = (ground, &mut pack.source)
        && (l.look.is_some() || !g.file.glow_groups.is_empty() || !g.file.materials.is_empty())
    {
        let look = l.look.get_or_insert_with(Default::default);
        look.glow_groups = g.file.glow_groups.clone();
        look.materials = g.file.materials.clone();
    }
    pack.placed = Some(src);
    Ok(out)
}

fn index(src: &PlacedSource, id: &str) -> Result<usize, PlacedError> {
    src.file.copies.iter().position(|c| c.id == id).ok_or_else(|| copy_err(id, "no such copy"))
}

/// Copy `id`'s index when it is not locked.
fn unlocked(src: &PlacedSource, id: &str) -> Result<usize, PlacedError> {
    let i = index(src, id)?;
    if src.file.copies[i].locked {
        return Err(copy_err(id, "it is locked: unlock it first"));
    }
    Ok(i)
}

/// The pack's stray files (`other`) under `editor/`, paths without it.
fn stray(pack: &Pack) -> Vec<String> {
    let prefix = format!("{EDITOR_DIR}/");
    pack.other.keys().filter_map(|p| p.as_str().strip_prefix(&prefix).map(str::to_string)).collect()
}

/// Whether two asset folders are the same (#76's comparison): `asset.ron`
/// (as read), the element files, sounds and script, byte for byte; never
/// the thumbnail.
pub fn same_asset(a: &AssetSource, b: &AssetSource) -> bool {
    a.file == b.file && a.files == b.files && a.sounds == b.sounds && a.script == b.script
}

/// The keys taken in `src` and by stray files in `editor/` (a folder of
/// one is never written over): every asset's, the ground's and the
/// ground's folder names.
fn taken_keys(src: &PlacedSource, stray: &[String]) -> BTreeSet<String> {
    src.file
        .assets
        .iter()
        .map(|a| a.key.clone())
        .chain(src.assets.keys().cloned())
        .chain(stray.iter().filter_map(|p| p.split_once('/').map(|(d, _)| d.to_string())))
        .chain([GROUND_KEY.to_string()])
        .chain(RESERVED_KEYS.iter().map(|k| k.to_string()))
        .collect()
}

/// The first of `base`, `<base>-2`, `<base>-3`, … that is a copy id and
/// free in `taken` (the stem cut to fit its suffix). Always terminates:
/// there are fewer taken ids than candidates.
fn numbered(base: &str, taken: &BTreeSet<String>) -> String {
    let limit = taken.len() + 2;
    (1..=limit)
        .map(|k| {
            if k == 1 {
                base.to_string()
            } else {
                let suffix = format!("-{k}");
                let mut stem = base.to_string();
                stem.truncate(MAX_COPY_ID - suffix.len());
                format!("{}{suffix}", stem.trim_end_matches('-'))
            }
        })
        .find(|id| valid_copy_id(id) && !taken.contains(id))
        .unwrap_or_else(|| format!("copy-{limit}"))
}

/// The ids taken in `src`: its copies', and those whose script a stray
/// file in `editor/scripts/` would be.
fn taken_ids(src: &PlacedSource, stray: &[String]) -> BTreeSet<String> {
    let dir = SCRIPTS_DIR.strip_prefix(&format!("{EDITOR_DIR}/")).unwrap_or(SCRIPTS_DIR).to_string();
    src.file
        .copies
        .iter()
        .map(|c| c.id.clone())
        .chain(stray.iter().filter_map(|p| {
            p.strip_prefix(&format!("{dir}/")).and_then(|f| f.strip_suffix(".rhai")).map(str::to_string)
        }))
        .collect()
}

/// A new copy id for an asset named `name` in `pack` (issue #75): its slug
/// (`asset_slug`; `copy-<slug>` when it does not start with a letter; at
/// most 32 bytes), else `<slug>-2`, `-3`, …, the first free (neither a
/// copy's id nor a stray script's).
pub fn new_copy_id(pack: &Pack, name: &str) -> String {
    let src = placement(pack).unwrap_or_default();
    new_id_in(&src, &stray(pack), name)
}

fn new_id_in(src: &PlacedSource, stray: &[String], name: &str) -> String {
    let mut base = asset_slug(name);
    if !base.starts_with(|c: char| c.is_ascii_lowercase()) {
        base = format!("copy-{base}");
    }
    base.truncate(MAX_COPY_ID);
    let base = base.trim_end_matches('-').to_string();
    numbered(&base, &taken_ids(src, stray))
}

/// The id a duplicate of copy `id` takes: `id`'s stem (a trailing `-<n>`
/// dropped), then `-2`, `-3`, …, the first free (`element::copy_name`'s
/// numbering).
fn duplicate_id(src: &PlacedSource, stray: &[String], id: &str) -> String {
    let base = match id.rsplit_once('-') {
        Some((b, n)) if !b.is_empty() && !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => b,
        _ => id,
    };
    let mut taken = taken_ids(src, stray);
    taken.insert(base.to_string());
    numbered(base, &taken)
}

/// Adds a copy of `asset` (from `origin`) to `pack`'s placement with its
/// top-left at world pixel `at`, at the asset's size, on top (issue #75).
/// An asset already in the level with the same origin and the same files
/// ([`same_asset`]; never the ground) is reused, and then `key` is refused;
/// else it is copied in under `key` (`valid_key`, not taken, not the
/// ground's) or the first free generated key (`next_key`: `a`, `b`, …).
/// The copy's id is `id` (a free `valid_copy_id`), else [`new_copy_id`].
/// `Added` says the look's changes the next bake makes: the asset's glow
/// groups and materials (posed in its start with its defaults) gathered
/// after the level's look's (`gather_groups`), the new ids listed.
pub fn add_copy(
    pack: &mut Pack,
    asset: &AssetSource,
    origin: AssetOrigin,
    at: (i32, i32),
    id: Option<&str>,
    key: Option<&str>,
) -> Result<Added, PlacedError> {
    let stray = stray(pack);
    let look = level(pack)?.look.clone();
    let decoded = match origin {
        AssetOrigin::Own => decode_own(asset),
        _ => decode(asset),
    }
    .map_err(|e| PlacedError::Level(format!("asset {:?}: {e}", asset.file.name)))?;
    commit(pack, |src| {
        let reused = src
            .file
            .assets
            .iter()
            .filter(|a| a.key != GROUND_KEY && a.origin == origin)
            .find(|a| src.assets.get(&a.key).is_some_and(|b| same_asset(asset, b)))
            .map(|a| a.key.clone());
        let new_asset = reused.is_none();
        let key = match (reused, key) {
            (Some(k), Some(chosen)) => {
                return Err(PlacedError::Level(format!(
                    "key {chosen:?}: the asset is in the level already, as {k:?}: a key names a new one"
                )));
            }
            (Some(k), None) => k,
            (None, Some(chosen)) => {
                if !valid_key(chosen) || chosen == GROUND_KEY {
                    return Err(PlacedError::Level(format!(
                        "key {chosen:?}: not a key ([a-z0-9-], 1 to 16 bytes, not starting with -, \
                         none of the ground's: ground, {})",
                        RESERVED_KEYS.join(", ")
                    )));
                }
                if taken_keys(src, &stray).contains(chosen) {
                    return Err(PlacedError::Level(format!("key {chosen:?}: taken")));
                }
                chosen.to_string()
            }
            (None, None) => next_key(taken_keys(src, &stray).iter().map(String::as_str)),
        };
        if new_asset && src.file.assets.len() >= MAX_LEVEL_ASSETS {
            return Err(PlacedError::Level(format!("a level holds at most {MAX_LEVEL_ASSETS} assets")));
        }
        if src.file.copies.len() >= MAX_COPIES {
            return Err(PlacedError::Level(format!("a level places at most {MAX_COPIES} copies")));
        }
        let id = match id {
            Some(id) => {
                if !valid_copy_id(id) {
                    return Err(copy_err(
                        id,
                        format!("not an id ([a-z][a-z0-9_-]*, 1 to {MAX_COPY_ID} bytes)"),
                    ));
                }
                if taken_ids(src, &stray).contains(id) {
                    return Err(copy_err(id, "the id is taken"));
                }
                id.to_string()
            }
            None => new_id_in(src, &stray, &asset.file.name),
        };
        if new_asset {
            src.file.assets.push(LevelAsset { key: key.clone(), origin: origin.clone() });
            src.assets.insert(key.clone(), asset.clone());
        }
        src.file.copies.push(PlacedCopy::new(&id, &key, at));
        // The look's changes: the gathering after the level's look.
        let p = posed(&decoded, decoded.start, &BTreeMap::new()).map_err(|e| copy_err(&id, e.to_string()))?;
        let mut groups = look.as_ref().map(|l| l.glow_groups.clone()).unwrap_or_default();
        let mut materials = look.as_ref().map(|l| l.materials.clone()).unwrap_or_default();
        let (g0, m0) = (groups.len(), materials.len());
        gather_groups(&mut groups, &p.groups);
        gather_materials(&mut materials, &p.materials);
        let groups_added: Vec<String> = groups[g0..].iter().map(|g| g.id.clone()).collect();
        let materials_added: Vec<String> = materials[m0..].iter().map(|m| m.id.clone()).collect();
        let added_look = look.is_none() && !(groups_added.is_empty() && materials_added.is_empty());
        Ok(Added { id, key, new_asset, groups_added, materials_added, added_look })
    })
}

/// Duplicates copy `id` (issue #75; the level tab's Copy and Paste): the
/// same asset, size, aspect, Live, state, values, scripts, default script
/// and depth, unlocked, at `at + offset`, directly above it, under a new
/// id (`id`'s stem numbered, `-2`, `-3`, …). Returns the new id.
pub fn duplicate_copy(pack: &mut Pack, id: &str, offset: (i32, i32)) -> Result<String, PlacedError> {
    let stray = stray(pack);
    commit(pack, |src| {
        let i = index(src, id)?;
        if src.file.copies.len() >= MAX_COPIES {
            return Err(PlacedError::Level(format!("a level places at most {MAX_COPIES} copies")));
        }
        let new = duplicate_id(src, &stray, id);
        let c = &src.file.copies[i];
        let at = (c.at.0.saturating_add(offset.0), c.at.1.saturating_add(offset.1));
        let dup = PlacedCopy { id: new.clone(), at, locked: false, ..c.clone() };
        if dup.script
            && let Some(s) = src.scripts.get(id).cloned()
        {
            src.scripts.insert(new.clone(), s);
        }
        src.file.copies.insert(i + 1, dup);
        Ok(new)
    })
}

/// Moves copy `id`'s top-left to world pixel `at`; a locked copy refuses.
pub fn move_copy(pack: &mut Pack, id: &str, at: (i32, i32)) -> Result<(), PlacedError> {
    commit(pack, |src| {
        let i = unlocked(src, id)?;
        src.file.copies[i].at = at;
        Ok(())
    })
}

/// Resizes copy `id` ([`keep_aspect`] over its asset's size; `Width`
/// and `Height` keep the aspect, `Free` frees it); a locked copy refuses.
/// Returns its placed size.
pub fn resize_copy(pack: &mut Pack, id: &str, size: Size) -> Result<(u32, u32), PlacedError> {
    commit(pack, |src| {
        let i = unlocked(src, id)?;
        let natural = src.assets.get(&src.file.copies[i].asset).map(|a| a.file.size).unwrap_or((1, 1));
        let c = &mut src.file.copies[i];
        c.size = keep_aspect(natural, size);
        match size {
            Size::Natural => {}
            Size::Width(_) | Size::Height(_) => c.free_aspect = false,
            Size::Free(..) => c.free_aspect = true,
        }
        Ok(c.size.unwrap_or(natural))
    })
}

/// Sets one of copy `id`'s settings ([`set_copies`] of one).
pub fn set_copy(pack: &mut Pack, id: &str, property: CopyProperty) -> Result<(), PlacedError> {
    set_copies(pack, id, &[property])
}

/// Sets copy `id`'s settings together, checked once: an unlocking first,
/// a locking last, the rest in order (an `Id` renames the copy for those
/// after it). A locked copy takes only `Locked`. `Id`: a free
/// `valid_copy_id`, its script renamed with it. `Live(true)`: refused with
/// `live_refusal`'s reason (the cell-level check `check_placed` cannot
/// make), or (issue #85) when the level's played form would pass its
/// file or byte cap (`live::check_budget`). `FreeAspect(false)` snaps the height to the width's aspect.
/// The rest are checked by `check_placed`.
pub fn set_copies(pack: &mut Pack, id: &str, properties: &[CopyProperty]) -> Result<(), PlacedError> {
    let stray = stray(pack);
    let unlock = properties.iter().filter(|p| matches!(p, CopyProperty::Locked(false)));
    let rest = properties.iter().filter(|p| !matches!(p, CopyProperty::Locked(_)));
    let lock = properties.iter().filter(|p| matches!(p, CopyProperty::Locked(true)));
    let ordered: Vec<&CopyProperty> = unlock.chain(rest).chain(lock).collect();
    let s = level(pack)?.art_scale;
    let going_live = properties.iter().any(|p| matches!(p, CopyProperty::Live(true)));
    commit(pack, |src| {
        let mut id = id.to_string();
        for p in ordered {
            let i = match p {
                CopyProperty::Locked(_) => index(src, &id)?,
                _ => unlocked(src, &id)?,
            };
            set_one(src, &stray, i, p)?;
            id = src.file.copies[i].id.clone();
        }
        if going_live {
            live_budget(src, s).map_err(|e| copy_err(&id, format!("cannot be Live: {e}")))?;
        }
        Ok(())
    })
}

/// Whether `src`'s Live copies' played form keeps within its caps at art
/// scale `s` (issue #85, `live::check_budget`), their assets decoded.
fn live_budget(src: &PlacedSource, s: u32) -> Result<(), PlacedError> {
    let mut assets = BTreeMap::new();
    for c in src.file.copies.iter().filter(|c| c.live) {
        if assets.contains_key(c.asset.as_str()) {
            continue;
        }
        let Some(a) = src.file.assets.iter().find(|a| a.key == c.asset) else { continue };
        let Some(asset) = src.assets.get(&a.key) else { continue };
        assets.insert(a.key.as_str(), super::decoded_asset(&a.key, &a.origin, asset)?);
    }
    crate::live::check_budget(src, &assets, s)
}

fn set_one(src: &mut PlacedSource, stray: &[String], i: usize, p: &CopyProperty) -> Result<(), PlacedError> {
    let id = src.file.copies[i].id.clone();
    match p {
        CopyProperty::Id(new) => {
            if *new == id {
                return Ok(());
            }
            if !valid_copy_id(new) {
                return Err(copy_err(
                    &id,
                    format!("{new:?}: not an id ([a-z][a-z0-9_-]*, 1 to {MAX_COPY_ID} bytes)"),
                ));
            }
            if taken_ids(src, stray).contains(new) {
                return Err(copy_err(&id, format!("{new:?}: the id is taken")));
            }
            if let Some(s) = src.scripts.remove(&id) {
                src.scripts.insert(new.clone(), s);
            }
            src.file.copies[i].id = new.clone();
        }
        CopyProperty::Live(live) => {
            if *live {
                let key = src.file.copies[i].asset.clone();
                let origin = src.file.assets.iter().find(|a| a.key == key).map(|a| a.origin.clone());
                let asset = src.assets.get(&key).ok_or_else(|| copy_err(&id, format!("no asset {key:?}")))?;
                let decoded = match origin {
                    Some(AssetOrigin::Own) => decode_own(asset),
                    _ => decode(asset),
                }
                .map_err(|source| PlacedError::Asset { key: key.clone(), source })?;
                if let Some(why) = live_refusal(&decoded) {
                    return Err(copy_err(&id, format!("cannot be Live: {why}")));
                }
            }
            src.file.copies[i].live = *live;
        }
        CopyProperty::State(state) => src.file.copies[i].state = state.clone(),
        CopyProperty::Value(name, value) => {
            let values = &mut src.file.copies[i].values;
            match value {
                Some(v) => {
                    values.insert(name.clone(), v.clone());
                }
                None => {
                    values.remove(name);
                }
            }
        }
        CopyProperty::DefaultScript(on) => src.file.copies[i].default_script = *on,
        CopyProperty::Distant(depth) => src.file.copies[i].distant = *depth,
        CopyProperty::FreeAspect(free) => {
            let natural = src.assets.get(&src.file.copies[i].asset).map(|a| a.file.size).unwrap_or((1, 1));
            let c = &mut src.file.copies[i];
            c.free_aspect = *free;
            if !*free && let Some((w, _)) = c.size {
                c.size = keep_aspect(natural, Size::Width(w));
            }
        }
        CopyProperty::Locked(locked) => src.file.copies[i].locked = *locked,
    }
    Ok(())
}

fn check_script(script: &Option<String>) -> Result<(), PlacedError> {
    match script {
        Some(s) if s.len() as u64 > MAX_SCRIPT_BYTES => {
            Err(PlacedError::Level(format!("the script is {} bytes, more than {MAX_SCRIPT_BYTES}", s.len())))
        }
        _ => Ok(()),
    }
}

/// Sets (`Some`) or clears copy `id`'s own script (`editor/scripts/<id>
/// .rhai`): at most `MAX_SCRIPT_BYTES`, not parsed; a locked copy refuses.
pub fn set_copy_script(pack: &mut Pack, id: &str, script: Option<String>) -> Result<(), PlacedError> {
    check_script(&script)?;
    let stray = stray(pack);
    commit(pack, |src| {
        let i = unlocked(src, id)?;
        let file = copy_script_file(id);
        let file = file.strip_prefix(&format!("{EDITOR_DIR}/")).unwrap_or(&file);
        if script.is_some() && !src.file.copies[i].script && stray.iter().any(|p| p == file) {
            return Err(copy_err(id, format!("{EDITOR_DIR}/{file}: a file of that name is in the pack")));
        }
        src.file.copies[i].script = script.is_some();
        match script {
            Some(s) => {
                src.scripts.insert(id.to_string(), s);
            }
            None => {
                src.scripts.remove(id);
            }
        }
        Ok(())
    })
}

/// Sets (`Some`) or clears the level script (`editor/level.rhai`): at most
/// `MAX_SCRIPT_BYTES`, not parsed.
pub fn set_level_script(pack: &mut Pack, script: Option<String>) -> Result<(), PlacedError> {
    check_script(&script)?;
    commit(pack, |src| {
        src.file.script = script.is_some();
        src.level_script = script;
        Ok(())
    })
}

/// Raises copy `id` one place in the copies' stack (above the next one);
/// refused on top. A distant copy's pieces bake below the rest wherever it
/// is, so the order matters among distant and among near copies.
pub fn raise_copy(pack: &mut Pack, id: &str) -> Result<(), PlacedError> {
    commit(pack, |src| {
        let i = index(src, id)?;
        if i + 1 >= src.file.copies.len() {
            return Err(copy_err(id, "it is on top already"));
        }
        src.file.copies.swap(i, i + 1);
        Ok(())
    })
}

/// Lowers copy `id` one place in the copies' stack; refused at the bottom.
pub fn lower_copy(pack: &mut Pack, id: &str) -> Result<(), PlacedError> {
    commit(pack, |src| {
        let i = index(src, id)?;
        if i == 0 {
            return Err(copy_err(id, "it is at the bottom already"));
        }
        src.file.copies.swap(i, i - 1);
        Ok(())
    })
}

/// Removes copy `id` and its script; when no copy uses its asset any more
/// the asset leaves the level too (an `Own` one included: the ground goes
/// with its only copy). A locked copy refuses.
pub fn remove_copy(pack: &mut Pack, id: &str) -> Result<Removed, PlacedError> {
    commit(pack, |src| {
        let index = unlocked(src, id)?;
        let copy = src.file.copies.remove(index);
        let script = src.scripts.remove(id);
        let asset = if src.file.copies.iter().any(|c| c.asset == copy.asset) {
            None
        } else {
            let at = src.file.assets.iter().position(|a| a.key == copy.asset);
            match (at, src.assets.remove(&copy.asset)) {
                (Some(at), Some(folder)) => Some((src.file.assets.remove(at), folder)),
                _ => None,
            }
        };
        Ok(Removed { copy, index, script, asset })
    })
}
