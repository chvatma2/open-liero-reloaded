//! A level's placed copies of assets (issue #73; the design
//! `docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`, "The
//! model", calls 4, 8, 13, 16, 17 and 20): `editor/placed.ron` lists the
//! assets the level holds a copy of and the copies placed from them,
//! bottom to top. The ground (the level's own painted asset, key
//! `ground`) is an asset folder rooted at `editor/` itself (`editor/
//! asset.ron`, `editor/layers/<id>.png`, …); every other asset is one at
//! `editor/<key>/`; a copy's own script is `editor/scripts/<copy-id>.rhai`
//! and the level script `editor/level.rhai`. `olr_edit::placed` bakes the
//! placement into the level's play files. Everything here is authoring:
//! never hashed, never downloaded.
//!
//! [`check_placed`] checks what can be checked from `placed.ron`, the
//! assets' `asset.ron`s and the files' headers; the cell-level checks
//! (a Live copy's mixed dirt cells) are `olr-edit`'s.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use olr_vfs::{ContentSink, ContentSource, VPath, VfsError};
use serde::{Deserialize, Serialize};

use super::{
    EDITOR_DIR, Element, ElementKind, MAX_DISTANT, MAX_DISTANT_AT, MAX_SIDE, check_distant, element_box,
};
use crate::FormatError;
use crate::assets::{
    AssetFile, AssetSource, MAX_SCRIPT_BYTES, PropertyType, ValueSource, check_asset, check_own_asset,
    check_value, owners, property_type, read_asset_files, valid_asset_id, write_asset_files,
};
use crate::defaults as d;
use crate::image_io::LEVEL_ART;
use crate::look::{GlowGroupSource, MAX_GROUPS};
use crate::materials::{MAX_MATERIAL_ID, MAX_MATERIALS, MaterialSource};
use crate::ron_io::{from_canonical, read_text, to_canonical, write_text};

/// The placement's file in a level pack.
pub const PLACED_FILE: &str = "editor/placed.ron";
/// The level script's file in a level pack.
pub const LEVEL_SCRIPT_FILE: &str = "editor/level.rhai";
/// The folder of the copies' own scripts.
pub const SCRIPTS_DIR: &str = "editor/scripts";
/// The key of the ground: the level's own asset stored in `editor/`.
pub const GROUND_KEY: &str = "ground";
/// The most assets a level holds a copy of.
pub const MAX_LEVEL_ASSETS: usize = 256;
/// The most copies a level places.
pub const MAX_COPIES: usize = 1024;
/// The longest asset key, in bytes.
pub const MAX_KEY: usize = 16;
/// The longest copy id, in bytes.
pub const MAX_COPY_ID: usize = 32;
/// How far from the level's origin a copy is placed, world pixels along
/// each axis: `4 × MAX_SIDE`.
pub const MAX_COPY_AT: i32 = 4 * MAX_SIDE as i32;
/// The most bytes a level's `editor/` files hold together:
/// `olr_pack::archive::MAX_UNPACKED`, an unpacked pack's bound (a test
/// holds them equal).
pub const MAX_PLACED_BYTES: u64 = 512 << 20;
/// The ground's folder names in `editor/`: never an asset's key.
pub const RESERVED_KEYS: [&str; 7] = ["images", "layers", "masks", "glow", "surface", "sounds", "scripts"];

fn yes() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

fn is_origin(v: &(i32, i32)) -> bool {
    *v == (0, 0)
}

/// `editor/placed.ron`: the assets a level holds and the copies placed
/// from them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedFile {
    /// The assets the level holds a copy of, at most `MAX_LEVEL_ASSETS`
    /// (256), each named by at least one copy.
    pub assets: Vec<LevelAsset>,
    /// The placed copies, bottom to top, at most `MAX_COPIES` (1024).
    pub copies: Vec<PlacedCopy>,
    /// The level has a level script, `editor/level.rhai` (UTF-8 Rhai, at
    /// most 64 KiB). Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub script: bool,
}

/// An asset the level holds a copy of, in its own folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LevelAsset {
    /// Its folder in `editor/`, internal (scripts name copies, never
    /// keys): `ground` is the ground, stored in `editor/` itself (an `Own`
    /// asset); any other is `[a-z0-9-]`, 1 to 16 bytes, not starting with
    /// `-`, never one of the ground's folder names (`images`, `layers`,
    /// `masks`, `glow`, `surface`, `sounds`, `scripts`), stored in
    /// `editor/<key>/`. The editor gives `a`, `b`, … `z`, `aa`, `ab`, …,
    /// the first free, in the order the assets join the level. Unique.
    pub key: String,
    /// Where the asset comes from.
    pub origin: AssetOrigin,
}

/// Where a level's asset comes from (ordered so a map can key on it,
/// issue #76's `Libraries`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AssetOrigin {
    /// The level's own, in no library (the ground).
    Own,
    /// A copy of the built-in library's `content/assets/<folder>`; the
    /// folder is an asset id (`[a-z0-9_-]`, 1 to 64 bytes, not starting
    /// with `-` or `_`).
    Builtin(String),
    /// A copy of the user's library's `<data>/assets/<folder>`; the folder
    /// is an asset id.
    Yours(String),
}

/// A copy of an asset placed in the level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedCopy {
    /// `[a-z][a-z0-9_-]*`, 1 to 32 bytes, unique among the copies: scripts
    /// reach the copy by it.
    pub id: String,
    /// The key of the asset it copies (one of `assets`).
    pub asset: String,
    /// Its top-left, world pixels, each within 16384 (`4 × MAX_SIDE`) of
    /// 0. Default `(0, 0)`.
    #[serde(default, skip_serializing_if = "is_origin")]
    pub at: (i32, i32),
    /// Its placed size, world pixels, each 1 to `MAX_SIDE` (4096): the
    /// asset stretched to it, nearest neighbour. Default none: the
    /// asset's size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<(u32, u32)>,
    /// The copy panel's free resize (the aspect not kept); any `size` is
    /// taken either way. Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub free_aspect: bool,
    /// Live: the copy stays an object in the match, driven by its states,
    /// values and scripts (for now it bakes as a static copy). Only an
    /// asset with more than one state, exposed values, sounds or a
    /// script, holding no dirt, raster or distant layer, and not placed
    /// distant. Default false: static, baked into the level in its
    /// starting state.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub live: bool,
    /// The name of its starting state, one of its asset's states. Default
    /// none: the asset's `start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Its own values of the asset's exposed properties, by exposed name,
    /// each of the property's type within its `min` to `max`: the copy's
    /// defaults (a state's own value comes first). Default none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, ValueSource>,
    /// The copy has its own script, `editor/scripts/<id>.rhai` (UTF-8
    /// Rhai, at most 64 KiB). Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub script: bool,
    /// The asset's default script runs on this copy too. Default true.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub default_script: bool,
    /// Placed as a distant (parallax) layer of this depth, 1 to 99: only
    /// an asset of Background images and plain drawn layers. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distant: Option<u8>,
    /// The level tab and `olr-cli level copy` do not move, resize, set or
    /// remove it (the editor's only: no effect on the bake or play).
    /// Default false.
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub locked: bool,
}

impl PlacedCopy {
    /// A static copy `id` of asset `asset` at `at`, of the asset's size,
    /// in its starting state with its defaults.
    pub fn new(id: &str, asset: &str, at: (i32, i32)) -> PlacedCopy {
        PlacedCopy {
            id: id.into(),
            asset: asset.into(),
            at,
            size: None,
            free_aspect: false,
            live: false,
            state: None,
            values: BTreeMap::new(),
            script: false,
            default_script: true,
            distant: None,
            locked: false,
        }
    }
}

/// A level's placement in memory: `placed.ron`, each asset's folder by
/// key (the ground's included), the copies' scripts by copy id and the
/// level script.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlacedSource {
    pub file: PlacedFile,
    pub assets: BTreeMap<String, AssetSource>,
    pub scripts: BTreeMap<String, String>,
    pub level_script: Option<String>,
}

/// Whether `key` may name an asset's folder: `[a-z0-9-]`, 1 to `MAX_KEY`
/// bytes, not starting with `-`, none of [`RESERVED_KEYS`], a portable
/// name. (`ground` is one, for the ground alone: [`check_placed`].)
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY
        && !key.starts_with('-')
        && key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !RESERVED_KEYS.contains(&key)
        && olr_vfs::non_portable(key).is_none()
}

/// Whether `id` is a copy's id: `[a-z][a-z0-9_-]*`, 1 to `MAX_COPY_ID`
/// bytes, its script's file name portable.
pub fn valid_copy_id(id: &str) -> bool {
    let b = id.as_bytes();
    (1..=MAX_COPY_ID).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-'))
        && olr_vfs::non_portable(&format!("{id}.rhai")).is_none()
}

/// The next generated key: `a`, `b`, … `z`, `aa`, `ab`, …, the first not in
/// `taken`, never `ground` nor one of [`RESERVED_KEYS`].
pub fn next_key<'a>(taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: BTreeSet<&str> = taken.into_iter().collect();
    // Bijective base 26: 1 is `a`, 26 `z`, 27 `aa`.
    (1u64..)
        .map(|mut n| {
            let mut out = Vec::new();
            while n > 0 {
                n -= 1;
                out.push(b'a' + (n % 26) as u8);
                n /= 26;
            }
            out.reverse();
            String::from_utf8(out).unwrap_or_default()
        })
        .find(|k| !taken.contains(k.as_str()) && k != GROUND_KEY && valid_key(k))
        .unwrap_or_default()
}

/// The folder in the pack of asset `key`: `editor` for the ground,
/// `editor/<key>` for any other.
pub fn asset_dir(key: &str) -> String {
    if key == GROUND_KEY { EDITOR_DIR.to_string() } else { format!("{EDITOR_DIR}/{key}") }
}

/// Copy `id`'s own script: `editor/scripts/<id>.rhai`.
pub fn copy_script_file(id: &str) -> String {
    format!("{SCRIPTS_DIR}/{id}.rhai")
}

/// The id a gathered glow group or material takes (issue #73): `base`
/// when nothing gathered has it, else the first of `base`, `<base>-2`,
/// `<base>-3`, … that nothing gathered has or whose gathered one has the
/// same settings. `find(id)` says whether one is gathered under `id`
/// (`None`) and whether its settings are the same; `max` bounds an id's
/// bytes (the base is cut to fit its suffix). Returns the id and whether
/// it is shared.
pub fn gather_id(base: &str, max: usize, find: &dyn Fn(&str) -> Option<bool>) -> (String, bool) {
    for k in 1usize.. {
        let id = if k == 1 {
            base.to_string()
        } else {
            let suffix = format!("-{k}");
            let mut stem = base.to_string();
            stem.truncate(max.saturating_sub(suffix.len()));
            format!("{stem}{suffix}")
        };
        match find(&id) {
            None => return (id, false),
            Some(true) => return (id, true),
            Some(false) => {}
        }
    }
    unreachable!("an id is free within the gathered count")
}

/// Gathers `groups` (a copy's, bottom to top) into `gathered` by the
/// gathering rule ([`gather_id`]): returns each one's id there.
pub fn gather_groups(gathered: &mut Vec<GlowGroupSource>, groups: &[GlowGroupSource]) -> Vec<String> {
    groups
        .iter()
        .map(|g| {
            let find = |id: &str| {
                gathered
                    .iter()
                    .find(|x| x.id == id)
                    .map(|x| *x == GlowGroupSource { id: id.into(), ..g.clone() })
            };
            let (id, shared) = gather_id(&g.id, 64, &find);
            if !shared {
                gathered.push(GlowGroupSource { id: id.clone(), ..g.clone() });
            }
            id
        })
        .collect()
}

/// Gathers `materials` into `gathered` as [`gather_groups`] gathers glow
/// groups: returns each one's id there.
pub fn gather_materials(gathered: &mut Vec<MaterialSource>, materials: &[MaterialSource]) -> Vec<String> {
    materials
        .iter()
        .map(|m| {
            let find = |id: &str| {
                gathered
                    .iter()
                    .find(|x| x.id == id)
                    .map(|x| *x == MaterialSource { id: id.into(), ..m.clone() })
            };
            let (id, shared) = gather_id(&m.id, MAX_MATERIAL_ID, &find);
            if !shared {
                gathered.push(MaterialSource { id: id.clone(), ..m.clone() });
            }
            id
        })
        .collect()
}

/// An asset's box `[b, b + n)` along one axis (asset pixels) in a copy
/// stretched from `from` to `to` pixels: `[floor(b × to / from),
/// floor((b + n) × to / from))` (floor division, a negative `b`
/// included), relative to the copy's top-left. Returns the start and the
/// length.
pub fn stretch_span(b: i32, n: u32, from: u32, to: u32) -> (i64, u32) {
    if from == to || from == 0 {
        return (i64::from(b), n);
    }
    let map = |v: i64| (v * i64::from(to)).div_euclid(i64::from(from));
    let start = map(i64::from(b));
    let end = map(i64::from(b) + i64::from(n));
    (start, u32::try_from(end - start).unwrap_or(0))
}

/// A position `v` (asset pixels, a resolved `X`, `Y`, `OffsetX` or
/// `OffsetY`) in a copy stretched from `from` to `to` pixels: `v × to /
/// from` rounded half away from zero.
pub fn stretch_offset(v: i64, from: u32, to: u32) -> i64 {
    if from == to || from == 0 {
        return v;
    }
    let (num, den) = (v.unsigned_abs() as i128 * i128::from(to), i128::from(from));
    let r = ((2 * num + den) / (2 * den)) as i64;
    if v < 0 { -r } else { r }
}

/// A copy's placed size: its `size`, else its asset's.
pub fn placed_size(copy: &PlacedCopy, asset: &AssetFile) -> (u32, u32) {
    copy.size.unwrap_or(asset.size)
}

/// `e` of an asset of `size` as a copy placed at `at` and stretched to
/// `to` lays it in the level: its box mapped by [`stretch_span`] (a
/// drawn layer always boxed, an image of pixel size 1 when resampled);
/// `None` for a box stretched to nothing. A raster is as it is.
pub fn stretched_element(
    e: &Element,
    size: (u32, u32),
    at: (i32, i32),
    to: (u32, u32),
    resampled: bool,
) -> Option<Element> {
    let mut e = e.clone();
    let Some((b, n)) = element_box(&e.kind, size) else { return Some(e) };
    let (x, w) = stretch_span(b.0, n.0, size.0, to.0);
    let (y, h) = stretch_span(b.1, n.1, size.1, to.1);
    if w == 0 || h == 0 {
        return None;
    }
    let clamp = |v: i64| v.clamp(i32::MIN.into(), i32::MAX.into()) as i32;
    let at = (clamp(i64::from(at.0) + x), clamp(i64::from(at.1) + y));
    match &mut e.kind {
        ElementKind::Image { at: a, size: s, pixel_size, .. } => {
            *a = at;
            *s = (w, h);
            if resampled {
                *pixel_size = 1;
            }
        }
        ElementKind::Drawn { at: a, size: s, .. } => {
            *a = at;
            *s = Some((w, h));
        }
        ElementKind::Raster => {}
    }
    Some(e)
}

fn invalid(path: &str, message: impl Into<String>) -> FormatError {
    FormatError::invalid(PLACED_FILE, path, message)
}

fn io(file: &str, message: String) -> FormatError {
    FormatError::Io { file: file.into(), message }
}

/// The counts, keys and ids: what makes the placement's paths safe,
/// checked before any file is read or written.
fn check_names(f: &PlacedFile) -> Result<(), FormatError> {
    if f.assets.len() > MAX_LEVEL_ASSETS {
        return Err(invalid("assets", format!("{} assets: at most {MAX_LEVEL_ASSETS}", f.assets.len())));
    }
    if f.copies.len() > MAX_COPIES {
        return Err(invalid("copies", format!("{} copies: at most {MAX_COPIES}", f.copies.len())));
    }
    let mut keys = BTreeSet::new();
    for (i, a) in f.assets.iter().enumerate() {
        if !valid_key(&a.key) {
            return Err(invalid(
                &format!("assets[{i}].key"),
                format!(
                    "{:?} is not a key: [a-z0-9-], 1 to {MAX_KEY} bytes, not starting with -, none of {RESERVED_KEYS:?}",
                    a.key
                ),
            ));
        }
        if !keys.insert(a.key.as_str()) {
            return Err(invalid(&format!("assets[{i}].key"), format!("asset {:?} is listed twice", a.key)));
        }
    }
    let mut ids = BTreeSet::new();
    for (i, c) in f.copies.iter().enumerate() {
        if !valid_copy_id(&c.id) {
            return Err(invalid(
                &format!("copies[{i}].id"),
                format!("copy {:?}: not an id ([a-z][a-z0-9_-]*, 1 to {MAX_COPY_ID} bytes)", c.id),
            ));
        }
        if !ids.insert(c.id.as_str()) {
            return Err(invalid(
                &format!("copies[{i}].id"),
                format!("copy {:?}: the id is used twice", c.id),
            ));
        }
    }
    Ok(())
}

/// Asset `key`'s error said of its folder in the pack (`asset.ron` is
/// `editor/<key>/asset.ron`).
fn under(dir: &str, e: FormatError) -> FormatError {
    let file = |f: String| format!("{dir}/{f}");
    match e {
        FormatError::Parse { file: f, line, col, message } => {
            FormatError::Parse { file: file(f), line, col, message }
        }
        FormatError::Invalid { file: f, path, message } => {
            FormatError::Invalid { file: file(f), path, message }
        }
        FormatError::Io { file: f, message } => FormatError::Io { file: file(f), message },
    }
}

/// Why a copy of `file` placed distant at `depth` may not be, layer by
/// layer (`check_placed`'s rule): a layer `check_distant` refuses, glass,
/// a directed glow or a painted surface map; "layer <id> of <name>: <why>"
/// for the first, else `None`.
pub fn distant_layer_refusal(file: &AssetFile, depth: u8) -> Option<String> {
    file.elements.iter().find_map(|e| {
        let why = check_distant(e, depth).err().or_else(|| {
            if e.glass.is_some() {
                Some("a distant layer cannot be glass".into())
            } else if e.glow.as_ref().is_some_and(|g| g.beam.is_some()) {
                Some("a distant layer's glow stays undirected".into())
            } else if e.surface_map || e.back_surface_map {
                Some("a distant layer takes one surface for the whole layer".into())
            } else {
                None
            }
        })?;
        Some(format!("layer {:?} of {:?}: {why}", e.id, file.name))
    })
}

/// Why a copy of `file` may not be placed distant at `depth`: a raster or
/// distant layer in it (it is then placed unstretched, not distant), else
/// [`distant_layer_refusal`]'s. Issue #76 checks a library asset's new
/// version against a level's distant copies with it.
pub fn distant_refusal(file: &AssetFile, depth: u8) -> Option<String> {
    if file.elements.iter().any(|e| matches!(e.kind, ElementKind::Raster)) {
        Some(format!("{:?} holds a raster: placed at (0, 0), unstretched, not distant", file.name))
    } else if file.elements.iter().any(Element::is_distant) {
        Some(format!("{:?} holds distant layers: placed unstretched, not distant", file.name))
    } else {
        distant_layer_refusal(file, depth)
    }
}

/// Whether `asset` drives anything: more than one state, exposed values,
/// sounds or a script (what a Live copy needs).
fn drives(asset: &AssetSource) -> bool {
    let f = &asset.file;
    f.states.len() > 1 || !f.exposed.is_empty() || !f.sounds.is_empty() || asset.script.is_some()
}

/// Whether `e` is a plain Dirt image or drawn layer.
fn plain_dirt(e: &Element) -> bool {
    use super::ElementMaterial::Dirt;
    matches!(
        e.kind,
        ElementKind::Image { material: Dirt, .. } | ElementKind::Drawn { material: Dirt, mixed: false, .. }
    )
}

/// Everything checkable without decoding a cell (issue #73): the counts,
/// keys and ids ([`valid_key`], [`valid_copy_id`]; `ground` only for an
/// `Own` asset, stored in `editor/`); every asset listed in `src.assets`
/// and named by a copy, every copy's asset known; each asset by
/// `check_own_asset` (`Own`) or `check_asset` (a library's copy, its
/// folder a `valid_asset_id`), its art scale free; each copy's place
/// (`at` within `MAX_COPY_AT`, `size` each 1 to `MAX_SIDE`, depth 1-99),
/// state (one of its asset's) and values (exposed, of the type, within
/// the limits); a Live copy's asset drives something and holds no plain
/// Dirt element, raster or distant layer, the copy not distant (the
/// mixed layers' dirt cells are `olr_edit::placed`'s); a distant copy's
/// elements what a distant element may be, no glass, beam or painted
/// surface map; an asset holding a raster placed at `(0, 0)`, unstretched,
/// not distant, not Live, of the level's size; one holding distant layers
/// unstretched, not distant, not Live; every script a copy or the level
/// says it has present, within `MAX_SCRIPT_BYTES`, and no other; the
/// copies' pixels after stretching (stacked state pictures counted)
/// within `MAX_COMPOSED_PIXELS`; the glow groups gathered from the
/// assets' own settings within `MAX_GROUPS` (a copy's values may change a
/// group's settings: the bake gathers the posed ones and checks again),
/// the materials within `MAX_MATERIALS`; the distant layers within
/// `MAX_DISTANT`, `MAX_DISTANT_AT` and `LEVEL_ART`'s pixels. Each error
/// names the copy or the key.
pub fn check_placed(src: &PlacedSource, level_size: (u32, u32), art_scale: u32) -> Result<(), FormatError> {
    use super::MAX_COMPOSED_PIXELS;
    let f = &src.file;
    check_names(f)?;
    let s = art_scale.max(1);
    let mut used = BTreeSet::new();
    for (i, c) in f.copies.iter().enumerate() {
        if !f.assets.iter().any(|a| a.key == c.asset) {
            return Err(invalid(
                &format!("copies[{i}].asset"),
                format!("copy {:?}: no asset {:?}", c.id, c.asset),
            ));
        }
        used.insert(c.asset.as_str());
    }
    for (i, a) in f.assets.iter().enumerate() {
        let at = |field: &str| format!("assets[{i}].{field}");
        let key = &a.key;
        match &a.origin {
            AssetOrigin::Own => {}
            AssetOrigin::Builtin(_) | AssetOrigin::Yours(_) if key == GROUND_KEY => {
                return Err(invalid(&at("key"), "asset \"ground\": the key is the ground's, an Own asset's"));
            }
            AssetOrigin::Builtin(folder) | AssetOrigin::Yours(folder) => {
                if !valid_asset_id(folder) {
                    return Err(invalid(
                        &at("origin"),
                        format!("asset {key:?}: {folder:?} is not an asset folder's name"),
                    ));
                }
            }
        }
        if !used.contains(key.as_str()) {
            return Err(invalid(&at("key"), format!("asset {key:?}: no copy places it")));
        }
        let Some(asset) = src.assets.get(key) else {
            return Err(io(&format!("{}/{}", asset_dir(key), crate::assets::ASSET_FILE), "missing".into()));
        };
        let checked = match a.origin {
            AssetOrigin::Own => check_own_asset(asset),
            _ => check_asset(asset),
        };
        checked.map_err(|e| under(&asset_dir(key), e))?;
    }
    if let Some(key) = src.assets.keys().find(|k| !f.assets.iter().any(|a| a.key == **k)) {
        return Err(invalid("assets", format!("asset {key:?}: its folder is not listed")));
    }
    let mut pixels = 0u64;
    let mut groups: Vec<GlowGroupSource> = Vec::new();
    let mut materials: Vec<MaterialSource> = Vec::new();
    let (mut distant, mut distant_pixels) = (0usize, 0u64);
    for (i, c) in f.copies.iter().enumerate() {
        let at = |field: &str| format!("copies[{i}].{field}");
        let bad = |field: &str, m: String| invalid(&at(field), format!("copy {:?}: {m}", c.id));
        let Some(asset) = src.assets.get(&c.asset).filter(|_| f.assets.iter().any(|a| a.key == c.asset))
        else {
            return Err(bad("asset", format!("no asset {:?}", c.asset)));
        };
        let file = &asset.file;
        let far = |v: i32| v.unsigned_abs() > MAX_COPY_AT as u32;
        if far(c.at.0) || far(c.at.1) {
            return Err(bad("at", format!("{:?}: each within {MAX_COPY_AT} of 0", c.at)));
        }
        if let Some((w, h)) = c.size
            && (!(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h))
        {
            return Err(bad("size", format!("{w} × {h}: each side 1 to {MAX_SIDE}")));
        }
        if let Some(depth) = c.distant
            && !(olr_data::level::MIN_DEPTH..=olr_data::level::MAX_DEPTH).contains(&depth)
        {
            return Err(bad("distant", format!("depth {depth}: 1 to 99")));
        }
        if let Some(state) = &c.state
            && !file.states.iter().any(|x| x.name == *state)
        {
            return Err(bad("state", format!("{:?} has no state {state:?}", file.name)));
        }
        for (name, v) in &c.values {
            let Some(x) = file.exposed.iter().find(|x| x.name == *name) else {
                return Err(bad(
                    &format!("values[{name:?}]"),
                    format!("{:?} exposes no value {name:?}", file.name),
                ));
            };
            let t: PropertyType =
                property_type(file, &x.property).map_err(|m| bad(&format!("values[{name:?}]"), m))?;
            let range = t.range.map(|(lo, hi)| (x.min.unwrap_or(lo), x.max.unwrap_or(hi)));
            check_value(t, range, v, PLACED_FILE, &at(&format!("values[{name:?}]"))).map_err(
                |e| match e {
                    FormatError::Invalid { file, path, message } => {
                        FormatError::Invalid { file, path, message: format!("copy {:?}: {message}", c.id) }
                    }
                    e => e,
                },
            )?;
        }
        let raster = file.elements.iter().any(|e| matches!(e.kind, ElementKind::Raster));
        let has_distant = file.elements.iter().any(Element::is_distant);
        let stretched = c.size.is_some_and(|z| z != file.size);
        if c.live {
            if !drives(asset) {
                return Err(bad(
                    "live",
                    format!(
                        "{:?} has a single state, no exposed values, no sounds and no script: nothing to drive",
                        file.name
                    ),
                ));
            }
            if let Some(e) = file.elements.iter().find(|e| plain_dirt(e)) {
                return Err(bad("live", format!("{:?} holds dirt (layer {:?})", file.name, e.id)));
            }
            if raster || has_distant {
                return Err(bad("live", format!("{:?} holds a raster or distant layer", file.name)));
            }
            if c.distant.is_some() {
                return Err(bad("live", "a distant copy cannot be Live".into()));
            }
        }
        if raster {
            if c.at != (0, 0) || stretched || c.distant.is_some() {
                return Err(bad(
                    "at",
                    format!("{:?} holds a raster: placed at (0, 0), unstretched, not distant", file.name),
                ));
            }
            if file.size != level_size {
                return Err(bad(
                    "asset",
                    format!(
                        "{:?} holds a raster of {} × {}: the level is {} × {}",
                        file.name, file.size.0, file.size.1, level_size.0, level_size.1
                    ),
                ));
            }
        }
        if has_distant && (stretched || c.distant.is_some()) {
            return Err(bad(
                "distant",
                format!("{:?} holds distant layers: placed unstretched, not distant", file.name),
            ));
        }
        if let Some(depth) = c.distant
            && let Some(why) = distant_layer_refusal(file, depth)
        {
            return Err(bad("distant", why));
        }
        if c.script {
            match src.scripts.get(&c.id) {
                None => return Err(io(&copy_script_file(&c.id), format!("copy {:?}: missing", c.id))),
                Some(t) if t.len() as u64 > MAX_SCRIPT_BYTES => {
                    return Err(io(
                        &copy_script_file(&c.id),
                        format!("copy {:?}: {} bytes, more than {MAX_SCRIPT_BYTES}", c.id, t.len()),
                    ));
                }
                Some(_) => {}
            }
        }
        // The copy's pixels as the bake lays them, every stacked picture.
        let to = placed_size(c, file);
        let resampled = stretched || file.art_scale != s;
        let pictures = owners(file);
        let mut copy_distant = 0u64;
        for e in &file.elements {
            let Some(placed) = stretched_element(e, file.size, c.at, to, resampled) else { continue };
            let n = 1 + pictures.get(e.id.as_str()).copied().unwrap_or(0);
            pixels = pixels.saturating_add(super::composition::decoded_pixels(&placed, level_size, s, n));
            if c.distant.is_some() || e.is_distant() {
                distant += 1;
                if let Some((p, b)) = element_box(&placed.kind, level_size) {
                    let far = |v: i32| v.unsigned_abs() > MAX_DISTANT_AT;
                    if far(p.0) || far(p.1) {
                        return Err(bad(
                            "at",
                            format!(
                                "its distant layer {:?} lies beyond {MAX_DISTANT_AT} of the origin",
                                e.id
                            ),
                        ));
                    }
                    copy_distant += u64::from(b.0) * u64::from(b.1) * u64::from(s) * u64::from(s);
                }
            }
        }
        if pixels > MAX_COMPOSED_PIXELS {
            return Err(bad(
                "size",
                format!(
                    "the copies up to this one decode to {pixels} pixels, more than {MAX_COMPOSED_PIXELS}"
                ),
            ));
        }
        distant_pixels = distant_pixels.saturating_add(copy_distant);
        if distant > MAX_DISTANT {
            return Err(bad("distant", format!("more than {MAX_DISTANT} distant layers in the level")));
        }
        if distant_pixels > LEVEL_ART.max_pixels {
            return Err(bad(
                "distant",
                format!(
                    "the distant layers up to this copy hold {distant_pixels} art pixels, above {}",
                    LEVEL_ART.max_pixels
                ),
            ));
        }
        gather_groups(&mut groups, &file.glow_groups);
        if groups.len() > MAX_GROUPS {
            return Err(bad("asset", format!("the level's gathered glow groups are more than {MAX_GROUPS}")));
        }
        gather_materials(&mut materials, &file.materials);
        if materials.len() > MAX_MATERIALS {
            return Err(bad(
                "asset",
                format!("the level's gathered materials are more than {MAX_MATERIALS}"),
            ));
        }
    }
    if let Some(id) = src.scripts.keys().find(|id| !f.copies.iter().any(|c| c.script && c.id == **id)) {
        return Err(io(&copy_script_file(id), format!("no copy {id:?} with a script")));
    }
    match (&src.level_script, f.script) {
        (None, true) => return Err(io(LEVEL_SCRIPT_FILE, "missing".into())),
        (Some(_), false) => return Err(invalid("script", "the level script is not listed (script: false)")),
        (Some(t), true) if t.len() as u64 > MAX_SCRIPT_BYTES => {
            return Err(io(LEVEL_SCRIPT_FILE, format!("{} bytes, more than {MAX_SCRIPT_BYTES}", t.len())));
        }
        _ => {}
    }
    Ok(())
}

/// A source seen from its folder `dir`, counting every byte read in
/// `total` against `MAX_PLACED_BYTES`.
struct Under<'a> {
    src: &'a dyn ContentSource,
    dir: String,
    total: &'a Cell<u64>,
}

impl Under<'_> {
    fn path(&self, path: &VPath) -> Result<VPath, VfsError> {
        VPath::new(&format!("{}/{}", self.dir, path.as_str()))
    }
}

impl ContentSource for Under<'_> {
    fn read(&self, path: &VPath) -> Result<Vec<u8>, VfsError> {
        self.read_max(path, u64::MAX)
    }

    fn list(&self, dir: &VPath) -> Result<Vec<String>, VfsError> {
        let full = if dir.as_str().is_empty() { VPath::new(&self.dir)? } else { self.path(dir)? };
        self.src.list(&full)
    }

    fn exists(&self, path: &VPath) -> bool {
        self.path(path).is_ok_and(|p| self.src.exists(&p))
    }

    fn read_max(&self, path: &VPath, limit: u64) -> Result<Vec<u8>, VfsError> {
        let left = MAX_PLACED_BYTES.saturating_sub(self.total.get());
        let bytes = self.src.read_max(&self.path(path)?, limit.min(left))?;
        self.total.set(self.total.get().saturating_add(bytes.len() as u64));
        Ok(bytes)
    }
}

/// A sink writing into its folder `dir`.
struct IntoDir<'a> {
    sink: &'a mut dyn ContentSink,
    dir: String,
}

impl ContentSink for IntoDir<'_> {
    fn write(&mut self, path: &VPath, data: &[u8]) -> Result<(), VfsError> {
        self.sink.write(&VPath::new(&format!("{}/{}", self.dir, path.as_str()))?, data)
    }
}

/// The script at `file` (UTF-8, at most `MAX_SCRIPT_BYTES`).
fn read_script(src: &Under, file: &str) -> Result<String, FormatError> {
    let path = VPath::new(file).map_err(|e| io(file, e.to_string()))?;
    let bytes = src.read_max(&path, MAX_SCRIPT_BYTES).map_err(|e| io(file, e.to_string()))?;
    String::from_utf8(bytes).map_err(|_| io(file, "not UTF-8".into()))
}

/// A level's placement: `None` without `editor/placed.ron`; else
/// `placed.ron` (its counts, keys and ids checked before any other file
/// is read), each listed asset's folder (`read_asset_files`, each within
/// `MAX_ASSET_BYTES`; the ground's `editor/`), each copy's script and the
/// level script it lists (UTF-8, at most `MAX_SCRIPT_BYTES`), every byte
/// within `MAX_PLACED_BYTES` together. Not checked further:
/// [`check_placed`].
pub fn read_placed_files(src: &dyn ContentSource) -> Result<Option<PlacedSource>, FormatError> {
    let path = VPath::new(PLACED_FILE).map_err(|e| io(PLACED_FILE, e.to_string()))?;
    if !src.exists(&path) {
        return Ok(None);
    }
    let text = read_text(src, PLACED_FILE)?;
    let file: PlacedFile = from_canonical(PLACED_FILE, &text)?;
    check_names(&file)?;
    let total = Cell::new(text.len() as u64);
    let mut assets = BTreeMap::new();
    for a in &file.assets {
        let dir = asset_dir(&a.key);
        let folder = Under { src, dir: dir.clone(), total: &total };
        assets.insert(a.key.clone(), read_asset_files(&folder).map_err(|e| under(&dir, e))?);
    }
    let read = |file: &str| {
        let root = Under { src, dir: EDITOR_DIR.into(), total: &total };
        let rel = file.strip_prefix(EDITOR_DIR).and_then(|f| f.strip_prefix('/')).unwrap_or(file);
        read_script(&root, rel).map_err(|e| under(EDITOR_DIR, e))
    };
    let mut scripts = BTreeMap::new();
    for c in file.copies.iter().filter(|c| c.script) {
        scripts.insert(c.id.clone(), read(&copy_script_file(&c.id))?);
    }
    let level_script = if file.script { Some(read(LEVEL_SCRIPT_FILE)?) } else { None };
    if total.get() > MAX_PLACED_BYTES {
        return Err(io(EDITOR_DIR, format!("more than {MAX_PLACED_BYTES} bytes")));
    }
    Ok(Some(PlacedSource { file, assets, scripts, level_script }))
}

/// The files [`write_placed_files`] writes for `src`, by pack path:
/// `placed.ron`, each listed asset's files in its folder
/// (`assets::asset_paths`), the copies' scripts and the level script.
/// Nothing is encoded.
pub fn placed_paths(src: &PlacedSource) -> Vec<String> {
    let mut out = vec![PLACED_FILE.to_string()];
    for a in &src.file.assets {
        if let Some(asset) = src.assets.get(&a.key) {
            let dir = asset_dir(&a.key);
            out.extend(crate::assets::asset_paths(asset).into_iter().map(|f| format!("{dir}/{f}")));
        }
    }
    for c in src.file.copies.iter().filter(|c| c.script && src.scripts.contains_key(&c.id)) {
        out.push(copy_script_file(&c.id));
    }
    if src.file.script && src.level_script.is_some() {
        out.push(LEVEL_SCRIPT_FILE.into());
    }
    out
}

/// Writes `placed.ron` in canonical form, each listed asset's folder with
/// `write_asset_files` (its files byte for byte as given), the scripts of
/// the copies that list one and the level script when listed; counts,
/// keys and ids checked before anything is written.
pub fn write_placed_files(sink: &mut dyn ContentSink, src: &PlacedSource) -> Result<(), FormatError> {
    check_names(&src.file)?;
    write_text(sink, PLACED_FILE, &to_canonical(PLACED_FILE, &src.file)?)?;
    for a in &src.file.assets {
        if let Some(asset) = src.assets.get(&a.key) {
            let dir = asset_dir(&a.key);
            let mut into = IntoDir { sink, dir: dir.clone() };
            write_asset_files(&mut into, asset).map_err(|e| under(&dir, e))?;
        }
    }
    let mut write = |file: &str, text: &str| {
        let path = VPath::new(file).map_err(|e| io(file, e.to_string()))?;
        sink.write(&path, text.as_bytes()).map_err(|e| io(file, e.to_string()))
    };
    for c in src.file.copies.iter().filter(|c| c.script) {
        if let Some(text) = src.scripts.get(&c.id) {
            write(&copy_script_file(&c.id), text)?;
        }
    }
    if src.file.script
        && let Some(text) = &src.level_script
    {
        write(LEVEL_SCRIPT_FILE, text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"(
    assets: [
        (key: "ground", origin: Own),
        (key: "a", origin: Builtin("strip-light")),
        (key: "b", origin: Yours("crane")),
        (key: "c", origin: Own),
    ],
    copies: [
        (id: "ground", asset: "ground", locked: true),
        (id: "lamp-1", asset: "a", at: (120, 80)),
        (id: "lamp-2", asset: "a", at: (200, 80), size: (58, 4), free_aspect: true),
        (
            id: "crane",
            asset: "b",
            at: (400, 60),
            size: (64, 40),
            live: true,
            state: "Moving",
            values: {"claw_x": Int(20)},
            script: true,
            default_script: false,
        ),
        (id: "hills", asset: "c", at: (-200, 0), distant: 80),
    ],
    script: true,
)
"#;

    /// The example's canonical text: each entry on its own lines.
    fn canonical() -> String {
        let f: PlacedFile = from_canonical(PLACED_FILE, EXAMPLE).unwrap();
        to_canonical(PLACED_FILE, &f).unwrap()
    }

    #[test]
    fn a_placement_round_trips_in_canonical_form() {
        let text = canonical();
        let f: PlacedFile = from_canonical(PLACED_FILE, &text).unwrap();
        assert_eq!(to_canonical(PLACED_FILE, &f).unwrap(), text);
        assert_eq!(f, from_canonical::<PlacedFile>(PLACED_FILE, EXAMPLE).unwrap());
        for default in
            ["free_aspect: false", "default_script: true", "locked: false", "at: (0, 0)", "live: false"]
        {
            assert!(!text.contains(default), "{default}: {text}");
        }
        assert!(text.contains("\"claw_x\": Int(20)") && text.contains("distant: 80"), "{text}");
        let crane = &f.copies[3];
        assert!(crane.live && crane.script && !crane.default_script && crane.size == Some((64, 40)));
        assert_eq!(f.copies[0], PlacedCopy { locked: true, ..PlacedCopy::new("ground", "ground", (0, 0)) });
        // A file holding only what it needs reads with every default.
        let small: PlacedFile = from_canonical(PLACED_FILE, "(assets: [], copies: [])").unwrap();
        assert_eq!(small, PlacedFile::default());
    }

    #[test]
    fn generated_keys_run_a_to_z_then_aa() {
        assert_eq!(next_key([]), "a");
        assert_eq!(next_key(["a", "b"]), "c");
        assert_eq!(next_key(["a", "c"]), "b");
        let letters: Vec<String> = (b'a'..=b'z').map(|c| (c as char).to_string()).collect();
        assert_eq!(next_key(letters.iter().map(String::as_str)), "aa");
        let mut taken = letters.clone();
        taken.push("aa".into());
        assert_eq!(next_key(taken.iter().map(String::as_str)), "ab");
        // Every generated key is valid and never the ground's or reserved.
        let mut taken: Vec<String> = Vec::new();
        for _ in 0..MAX_LEVEL_ASSETS {
            let k = next_key(taken.iter().map(String::as_str));
            assert!(valid_key(&k) && k != GROUND_KEY && !RESERVED_KEYS.contains(&k.as_str()), "{k}");
            assert!(!taken.contains(&k));
            taken.push(k);
        }
        assert_eq!((taken[25].as_str(), taken[26].as_str(), taken[52].as_str()), ("z", "aa", "ba"));
    }

    #[test]
    fn keys_and_ids_follow_their_rules() {
        for k in ["a", "zz", "my-crane", "ground", "a1", "0"] {
            assert!(valid_key(k), "{k}");
        }
        for k in ["", "-a", "A", "a_b", "a.b", "layers", "scripts", "con", &"a".repeat(17)] {
            assert!(!valid_key(k), "{k}");
        }
        for id in ["a", "lamp-1", "crane_2", &"a".repeat(32)] {
            assert!(valid_copy_id(id), "{id}");
        }
        for id in ["", "1lamp", "-a", "Lamp", "a.b", "a/b", "con", &"a".repeat(33)] {
            assert!(!valid_copy_id(id), "{id}");
        }
    }

    #[test]
    fn spans_floor_and_offsets_round_half_away() {
        // A 3-wide asset at 7: [0,1) [1,2) [2,3) become [0,2) [2,4) [4,7).
        assert_eq!(stretch_span(0, 1, 3, 7), (0, 2));
        assert_eq!(stretch_span(1, 1, 3, 7), (2, 2));
        assert_eq!(stretch_span(2, 1, 3, 7), (4, 3));
        // Floor division below 0.
        assert_eq!(stretch_span(-1, 2, 3, 7), (-3, 5));
        assert_eq!(stretch_span(0, 1, 3, 2), (0, 0));
        assert_eq!(stretch_span(5, 4, 9, 9), (5, 4));
        assert_eq!(stretch_offset(1, 2, 3), 2);
        assert_eq!(stretch_offset(-1, 2, 3), -2);
        assert_eq!(stretch_offset(1, 3, 4), 1);
        assert_eq!(stretch_offset(-5, 4, 2), -3);
        assert_eq!(stretch_offset(7, 7, 7), 7);
    }

    #[test]
    fn gathering_shares_equal_and_renames_others() {
        use crate::look::{AnimationSource, GlowColourSource, ReachSource};
        let g = |id: &str, intensity: u16| GlowGroupSource {
            id: id.into(),
            colour: GlowColourSource::Own,
            intensity,
            reach: ReachSource::Pixels(40),
            animation: AnimationSource::Steady,
            phase_ms: None,
        };
        let mut all = Vec::new();
        assert_eq!(gather_groups(&mut all, &[g("lamps", 100)]), ["lamps"]);
        assert_eq!(gather_groups(&mut all, &[g("lamps", 100)]), ["lamps"]);
        assert_eq!(gather_groups(&mut all, &[g("lamps", 200)]), ["lamps-2"]);
        assert_eq!(gather_groups(&mut all, &[g("lamps", 300), g("sky", 1)]), ["lamps-3", "sky"]);
        assert_eq!(gather_groups(&mut all, &[g("lamps", 200)]), ["lamps-2"]);
        assert_eq!(all.len(), 4);
        // A long id is cut to fit its suffix.
        let long = "x".repeat(64);
        let mut all = vec![g(&long, 1)];
        let id = gather_groups(&mut all, &[g(&long, 2)]).remove(0);
        assert!(id.len() == 64 && id.ends_with("-2") && crate::look::valid_group_id(&id), "{id}");
    }

    #[test]
    fn the_byte_bound_is_an_unpacked_packs() {
        assert_eq!(MAX_PLACED_BYTES, olr_pack::archive::MAX_UNPACKED);
    }

    // Fixtures: a level of 8 × 6 at art scale 1.

    use crate::assets::{ExposedSource, PropertySource, StateSource};
    use crate::image_io;
    use crate::levels::{ElementGlow, ElementMaterial, MATERIAL_PALETTE, raster_files};
    use crate::look::{AnimationSource, GlowColourSource, ReachSource};
    use olr_data::image::RgbaImage;
    use olr_vfs::MemFs;

    const LEVEL: (u32, u32) = (8, 6);

    fn group(id: &str) -> GlowGroupSource {
        GlowGroupSource {
            id: id.into(),
            colour: GlowColourSource::Own,
            intensity: 300,
            reach: ReachSource::Pixels(40),
            animation: AnimationSource::Steady,
            phase_ms: None,
        }
    }

    fn element(id: &str, kind: ElementKind) -> Element {
        Element {
            id: id.into(),
            name: String::new(),
            visible: true,
            locked: false,
            folder: None,
            kind,
            glow: None,
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        }
    }

    fn drawn(id: &str, material: ElementMaterial, size: (u32, u32)) -> Element {
        element(id, ElementKind::Drawn { material, at: (0, 0), size: Some(size), mixed: false, back: false })
    }

    /// An asset of one drawn layer `body` of `material` filling `size`.
    fn asset(name: &str, material: ElementMaterial, size: (u32, u32)) -> AssetSource {
        let mut files = BTreeMap::new();
        files.insert("layers/body.png".into(), image_io::encode_rgba(&RgbaImage::new(size.0, size.1)));
        AssetSource::new(AssetFile::new(name, size, vec![drawn("body", material, size)]), files)
    }

    /// A 2 × 2 lamp glowing in `lamps`, with states Off and On (On owning
    /// the body) and an exposed glow strength: something to drive.
    fn lamp() -> AssetSource {
        let mut a = asset("Lamp", ElementMaterial::Background, (2, 2));
        a.file.elements[0].glow =
            Some(ElementGlow { strength: 50, group: "lamps".into(), mask: false, beam: None });
        a.file.glow_groups = vec![group("lamps")];
        a.file.states =
            vec![StateSource::new("Off"), StateSource { own: vec!["body".into()], ..StateSource::new("On") }];
        a.files.insert("layers/body.png".into(), image_io::encode_rgba(&RgbaImage::new(2, 4)));
        a.file.exposed = vec![ExposedSource {
            name: "strength".into(),
            property: PropertySource::GlowStrength("body".into()),
            default: None,
            min: None,
            max: None,
        }];
        a
    }

    /// The level's own ground: a raster of the level's size under a
    /// distant sky.
    fn ground() -> AssetSource {
        let mut sky = drawn("sky", ElementMaterial::Background, (4, 2));
        sky.distant = Some(50);
        let mut file = AssetFile::new("Ground", LEVEL, vec![sky, element("base", ElementKind::Raster)]);
        file.glow_groups = vec![group("unused")];
        let mut files = BTreeMap::new();
        files.insert("layers/sky.png".into(), image_io::encode_rgba(&RgbaImage::new(4, 2)));
        for f in raster_files("base") {
            let f = f.strip_prefix("editor/").unwrap().to_string();
            let bytes = if f.contains("material") {
                image_io::encode_indexed(LEVEL.0, LEVEL.1, &[2; 48], &MATERIAL_PALETTE)
            } else {
                image_io::encode_rgba(&RgbaImage::new(LEVEL.0, LEVEL.1))
            };
            files.insert(f, bytes);
        }
        AssetSource::new(file, files)
    }

    fn level_asset(key: &str, origin: AssetOrigin) -> LevelAsset {
        LevelAsset { key: key.into(), origin }
    }

    /// The ground, two lamps (one Live, with its own script) and a rock,
    /// and a level script.
    fn sample() -> PlacedSource {
        let mut src = PlacedSource::default();
        src.file.assets = vec![
            level_asset(GROUND_KEY, AssetOrigin::Own),
            level_asset("a", AssetOrigin::Builtin("lamp".into())),
            level_asset("b", AssetOrigin::Yours("rock".into())),
        ];
        let mut live = PlacedCopy::new("lamp-2", "a", (4, 1));
        (live.live, live.script, live.state) = (true, true, Some("On".into()));
        live.values.insert("strength".into(), ValueSource::Int(80));
        src.file.copies = vec![
            PlacedCopy { locked: true, ..PlacedCopy::new(GROUND_KEY, GROUND_KEY, (0, 0)) },
            PlacedCopy::new("lamp-1", "a", (1, 1)),
            live,
            PlacedCopy { size: Some((6, 2)), ..PlacedCopy::new("rock", "b", (1, 3)) },
        ];
        src.file.script = true;
        src.assets.insert(GROUND_KEY.into(), ground());
        src.assets.insert("a".into(), lamp());
        src.assets.insert("b".into(), asset("Rock", ElementMaterial::Rock, (3, 1)));
        src.scripts.insert("lamp-2".into(), "fn on_start() {}\n".into());
        src.level_script = Some("// the level\n".into());
        src
    }

    fn refused(src: &PlacedSource) -> String {
        check_placed(src, LEVEL, 1).unwrap_err().to_string()
    }

    #[test]
    fn a_placement_round_trips_through_its_files() {
        let src = sample();
        check_placed(&src, LEVEL, 1).unwrap();
        let mut fs = MemFs::new();
        write_placed_files(&mut fs, &src).unwrap();
        for f in [
            PLACED_FILE,
            "editor/asset.ron",
            "editor/layers/base.front.png",
            "editor/a/asset.ron",
            "editor/a/layers/body.png",
            "editor/b/asset.ron",
            "editor/scripts/lamp-2.rhai",
            LEVEL_SCRIPT_FILE,
        ] {
            assert!(fs.exists(&VPath::new(f).unwrap()), "{f}");
        }
        assert_eq!(read_placed_files(&fs).unwrap(), Some(src.clone()));
        // `placed_paths` lists exactly what was written.
        let mut paths = placed_paths(&src);
        paths.sort();
        let written: Vec<String> = fs.files().map(|(p, _)| p.as_str().to_string()).collect();
        assert_eq!(paths, written);
        // The element files are written byte for byte as given.
        let odd = b"\x89PNG not re-encoded".to_vec();
        let mut s = src;
        s.assets.get_mut("b").unwrap().files.insert("layers/body.png".into(), odd.clone());
        let mut fs = MemFs::new();
        write_placed_files(&mut fs, &s).unwrap();
        assert_eq!(fs.read(&VPath::new("editor/b/layers/body.png").unwrap()).unwrap(), odd);
        // A level without placed.ron has no placement.
        assert_eq!(read_placed_files(&MemFs::new()).unwrap(), None);
    }

    #[test]
    fn placement_rules_are_refused_naming_the_copy() {
        let with = |edit: &dyn Fn(&mut PlacedSource)| {
            let mut s = sample();
            edit(&mut s);
            refused(&s)
        };
        let has = |e: String, parts: &[&str]| {
            for p in parts {
                assert!(e.contains(p), "{p:?} not in {e}");
            }
        };
        // Ids and keys.
        has(with(&|s| s.file.copies[2].id = "lamp-1".into()), &["copies[2].id", "lamp-1", "twice"]);
        has(with(&|s| s.file.copies[1].id = "Lamp".into()), &["copies[1].id", "Lamp"]);
        has(with(&|s| s.file.assets[2].key = "a".into()), &["assets[2].key", "\"a\"", "twice"]);
        has(with(&|s| s.file.assets[1].key = "A b".into()), &["assets[1].key", "A b"]);
        has(
            with(&|s| {
                s.file.assets[1].key = "layers".into();
                for c in &mut s.file.copies[1..3] {
                    c.asset = "layers".into();
                }
                let a = s.assets.remove("a").unwrap();
                s.assets.insert("layers".into(), a);
            }),
            &["assets[1].key", "layers"],
        );
        has(
            with(&|s| s.file.assets[0].origin = AssetOrigin::Builtin("ground".into())),
            &["assets[0].key", "ground", "Own"],
        );
        has(
            with(&|s| s.file.assets[1].origin = AssetOrigin::Yours("../x".into())),
            &["assets[1].origin", "../x"],
        );
        has(with(&|s| s.file.copies[3].asset = "z".into()), &["copies[3].asset", "copy \"rock\"", "\"z\""]);
        has(with(&|s| s.file.copies[3].asset = "a".into()), &["assets[2].key", "\"b\"", "no copy"]);
        has(with(&|s| drop(s.assets.remove("b"))), &["editor/b/asset.ron", "missing"]);
        // An asset's own errors name its folder.
        has(
            with(&|s| s.assets.get_mut("a").unwrap().file.glow_groups.clear()),
            &["editor/a/asset.ron", "lamps"],
        );
        // A library copy is a library asset: no unused group.
        has(
            with(&|s| s.assets.get_mut("a").unwrap().file.glow_groups.push(group("spare"))),
            &["editor/a/asset.ron", "spare"],
        );
        // States and values.
        has(
            with(&|s| s.file.copies[2].state = Some("Broken".into())),
            &["copies[2].state", "lamp-2", "Broken"],
        );
        has(
            with(&|s| drop(s.file.copies[2].values.insert("colour".into(), ValueSource::Int(1)))),
            &["copies[2].values", "lamp-2", "colour"],
        );
        has(
            with(&|s| drop(s.file.copies[2].values.insert("strength".into(), ValueSource::Bool(true)))),
            &["copies[2].values", "lamp-2", "Int"],
        );
        has(
            with(&|s| drop(s.file.copies[2].values.insert("strength".into(), ValueSource::Int(101)))),
            &["copies[2].values", "lamp-2", "0 to 100"],
        );
        // Live.
        has(
            with(&|s| {
                s.assets.insert("a".into(), {
                    let mut a = lamp();
                    a.file.elements[0].kind = ElementKind::Drawn {
                        material: ElementMaterial::Dirt,
                        at: (0, 0),
                        size: Some((2, 2)),
                        mixed: false,
                        back: false,
                    };
                    a.file.exposed.clear();
                    a.file.elements[0].glow = None;
                    a.file.glow_groups.clear();
                    a
                });
                s.file.copies[2].values.clear();
            }),
            &["copies[2].live", "lamp-2", "dirt"],
        );
        has(with(&|s| s.file.copies[3].live = true), &["copies[3].live", "rock", "nothing to drive"]);
        has(
            with(&|s| {
                s.file.copies[0].live = true;
                s.assets.get_mut(GROUND_KEY).unwrap().script = Some(String::new());
            }),
            &["copies[0].live", "raster or distant"],
        );
        has(with(&|s| s.file.copies[2].distant = Some(40)), &["copies[2].live", "distant"]);
        // Distant copies.
        has(with(&|s| s.file.copies[3].distant = Some(40)), &["copies[3].distant", "rock", "background"]);
        has(with(&|s| s.file.copies[1].distant = Some(0)), &["copies[1].distant", "depth 0"]);
        let ok = |s: &PlacedSource| check_placed(s, LEVEL, 1).unwrap();
        let mut s = sample();
        s.file.copies[1].distant = Some(40);
        ok(&s);
        // Rasters and distant layers.
        has(with(&|s| s.file.copies[0].at = (1, 0)), &["copies[0].at", "raster", "(0, 0)"]);
        has(with(&|s| s.file.copies[0].size = Some((8, 7))), &["copies[0]", "raster", "unstretched"]);
        let mut s = sample();
        s.file.copies[0].size = Some(LEVEL);
        ok(&s);
        let e = check_placed(&sample(), (9, 6), 1).unwrap_err().to_string();
        has(e, &["copies[0].asset", "raster", "9 × 6"]);
        // Scripts.
        has(with(&|s| drop(s.scripts.remove("lamp-2"))), &["editor/scripts/lamp-2.rhai", "missing"]);
        has(
            with(&|s| drop(s.scripts.insert("lamp-2".into(), " ".repeat(MAX_SCRIPT_BYTES as usize + 1)))),
            &["editor/scripts/lamp-2.rhai", "lamp-2", "bytes"],
        );
        has(with(&|s| drop(s.scripts.insert("rock".into(), String::new()))), &["editor/scripts/rock.rhai"]);
        has(with(&|s| s.level_script = None), &[LEVEL_SCRIPT_FILE, "missing"]);
        // Counts.
        has(
            with(&|s| {
                let c = s.file.copies[1].clone();
                s.file.copies =
                    (0..=MAX_COPIES).map(|i| PlacedCopy { id: format!("c{i}"), ..c.clone() }).collect();
            }),
            &["copies", "1025"],
        );
        // Placement.
        has(with(&|s| s.file.copies[1].at = (MAX_COPY_AT + 1, 0)), &["copies[1].at", "lamp-1"]);
        has(with(&|s| s.file.copies[1].size = Some((0, 2))), &["copies[1].size", "lamp-1"]);
    }

    /// Two assets of 128 glowing layers each, of 128 groups each: their
    /// ids differ, so the level would gather 256.
    #[test]
    fn too_many_gathered_groups_are_refused() {
        let many = |prefix: &str| {
            let mut file = AssetFile::new("Many", (1, 1), Vec::new());
            let mut files = BTreeMap::new();
            for i in 0..128 {
                let mut e = drawn(&format!("l{i}"), ElementMaterial::Background, (1, 1));
                e.glow = Some(ElementGlow {
                    strength: 50,
                    group: format!("{prefix}{i}"),
                    mask: false,
                    beam: None,
                });
                file.elements.push(e);
                file.glow_groups.push(group(&format!("{prefix}{i}")));
                files.insert(format!("layers/l{i}.png"), image_io::encode_rgba(&RgbaImage::new(1, 1)));
            }
            AssetSource::new(file, files)
        };
        let mut s = PlacedSource::default();
        s.file.assets = vec![level_asset("a", AssetOrigin::Own), level_asset("b", AssetOrigin::Own)];
        s.file.copies = vec![PlacedCopy::new("one", "a", (0, 0)), PlacedCopy::new("two", "b", (0, 0))];
        s.assets.insert("a".into(), many("g"));
        s.assets.insert("b".into(), many("g"));
        // The same groups are shared.
        check_placed(&s, LEVEL, 1).unwrap();
        s.assets.insert("b".into(), many("h"));
        let e = refused(&s);
        assert!(e.contains("copies[1]") && e.contains("two") && e.contains("glow groups"), "{e}");
    }

    /// A 1 × 1 layer owned by three states stacks four pictures: five
    /// copies stretched to 4096 × 4096 decode to 320 Mi pixels, over the
    /// budget; without the states, to 80 Mi.
    #[test]
    fn stacked_state_pictures_count_in_the_budget() {
        let mut a = asset("Speck", ElementMaterial::Rock, (1, 1));
        let mut s = PlacedSource::default();
        s.file.assets = vec![level_asset("a", AssetOrigin::Own)];
        s.file.copies = (0..5)
            .map(|i| PlacedCopy {
                size: Some((4096, 4096)),
                ..PlacedCopy::new(&format!("c{i}"), "a", (0, 0))
            })
            .collect();
        s.assets.insert("a".into(), a.clone());
        check_placed(&s, LEVEL, 1).unwrap();
        a.file.states = ["A", "B", "C", "D"].map(StateSource::new).to_vec();
        for st in &mut a.file.states[1..] {
            st.own = vec!["body".into()];
        }
        a.files.insert("layers/body.png".into(), image_io::encode_rgba(&RgbaImage::new(1, 4)));
        s.assets.insert("a".into(), a);
        let e = refused(&s);
        assert!(e.contains("copies[4]") && e.contains("c4") && e.contains("pixels"), "{e}");
    }

    /// Reading refuses a bad key before any path is made of it.
    #[test]
    fn a_bad_key_or_id_is_refused_before_any_file() {
        let mut s = sample();
        s.file.assets[1].key = "../up".into();
        let mut fs = MemFs::new();
        assert!(write_placed_files(&mut fs, &s).is_err());
        assert!(!fs.exists(&VPath::new(PLACED_FILE).unwrap()));
        let text = to_canonical(PLACED_FILE, &s.file).unwrap();
        let fs = MemFs::new().with(PLACED_FILE, text);
        assert!(read_placed_files(&fs).unwrap_err().to_string().contains("assets[1].key"));
        let mut s = sample();
        s.file.copies[2].id = "../x".into();
        let fs = MemFs::new().with(PLACED_FILE, to_canonical(PLACED_FILE, &s.file).unwrap());
        assert!(read_placed_files(&fs).unwrap_err().to_string().contains("copies[2].id"));
        // A script that is not UTF-8.
        let mut fs = MemFs::new();
        write_placed_files(&mut fs, &sample()).unwrap();
        fs.write(&VPath::new("editor/scripts/lamp-2.rhai").unwrap(), &[0xff, 0xfe]).unwrap();
        let e = read_placed_files(&fs).unwrap_err().to_string();
        assert!(e.contains("editor/scripts/lamp-2.rhai") && e.contains("UTF-8"), "{e}");
    }
}
