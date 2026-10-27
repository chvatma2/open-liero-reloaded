//! A level's library links (issue #76; the design's "The library, export
//! and import" and calls 7, 9, 17, 18): a level holds a copy of every
//! asset it uses, each of origin `Own`, `Builtin(folder)` or
//! `Yours(folder)`; this compares the copies with the libraries, takes a
//! library's version (`update`, `relink`), gives a missing one back
//! (`restore`), unlinks one (`adopt`), and exports and imports a level
//! with its assets (`.olrlevel`, `olr_pack::archive`'s level export).
//!
//! **Pure cores, path wrappers.** [`compare`], [`update`] and [`relink`]
//! work on sources in memory and a [`Libraries`] value (the library's
//! version of each linked asset), so the level editor's pure state can
//! carry it through a host round trip; [`read_libraries`] and the `_from`
//! wrappers read the libraries from disk. A `Builtin` origin is read from
//! the built-in library only and a `Yours` from the user's only: built-in
//! levels never follow the user's library (call 7).
//!
//! Two versions are the same when `asset.ron` (as read), the element
//! files, sounds and script are byte for byte (`placed::same_asset`);
//! `thumb.png` is never compared (it changes with rendering).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use olr_data::image::RgbaImage;
use olr_data::manifest::PackKind;
use olr_format::FormatError;
use olr_format::assets::{AssetSource, check_asset, check_value, property_type, read_asset_files};
use olr_format::levels::{AssetOrigin, GROUND_KEY, PLACED_FILE, PlacedSource, check_placed, distant_refusal};
use olr_pack::archive::{self, Limits};
use olr_vfs::{ContentSink, DirFs};

use crate::asset::{
    Asset, AssetError, add_source_to_library, decode, free_folders, library_folders, live_refusal, save_dir,
    valid_asset_id,
};
use crate::element::copy_name;
use crate::pack::{self, Pack, Source, UNCONVERTED, placement};
use crate::placed::{PlacedError, commit, same_asset, stale};

/// The library's version of each linked asset, by origin: `None` when the
/// library has no such folder or it does not read (or check).
pub type Libraries = BTreeMap<AssetOrigin, Option<AssetSource>>;

/// The back tile of a theme, by its id (`compose::theme_back_tile` over a
/// content directory): what `placed::stale` needs to bake a level.
pub type BackTile<'a> = &'a dyn Fn(&str) -> Result<RgbaImage, String>;

/// A level's linked asset compared with its library.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub key: String,
    pub origin: AssetOrigin,
    pub status: Status,
}

/// How a linked asset stands against its library.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// The library's version is the level's copy (its thumbnail aside).
    Same,
    /// The library's version differs; `breaks` are the copies it would
    /// break.
    Changed { breaks: Vec<Break> },
    /// The library has no folder of that name, or it does not read.
    Missing,
}

/// What a library version would break of a copy, naming the copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Break {
    /// A value naming an exposed property it no longer has, or of another
    /// type.
    Value { copy: String, name: String, message: String },
    /// A starting state it no longer has.
    State { copy: String, state: String },
    /// Live on an asset that may no longer be (`live_refusal`).
    Live { copy: String, message: String },
    /// A distant copy of an asset that may no longer be distant.
    Distant { copy: String, message: String },
    /// A script naming what the asset no longer has (reserved for #89).
    Script { copy: String, message: String },
}

impl Break {
    /// The copy it breaks.
    pub fn copy(&self) -> &str {
        match self {
            Break::Value { copy, .. }
            | Break::State { copy, .. }
            | Break::Live { copy, .. }
            | Break::Distant { copy, .. }
            | Break::Script { copy, .. } => copy,
        }
    }
}

impl fmt::Display for Break {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Break::Value { copy, name, message } => write!(f, "{copy}: value {name}: {message}"),
            Break::State { copy, state } => write!(f, "{copy}: state {state:?}: no such state"),
            Break::Live { copy, message } => write!(f, "{copy}: live: {message}"),
            Break::Distant { copy, message } => write!(f, "{copy}: distant: {message}"),
            Break::Script { copy, message } => write!(f, "{copy}: script: {message}"),
        }
    }
}

/// How a forced update resolved a [`Break`] of a copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// A value it can no longer hold dropped.
    DroppedValue { copy: String, name: String },
    /// A starting state it lacks: the copy starts in the asset's start.
    StartState { copy: String, state: String },
    /// Live it can no longer be: static.
    Static { copy: String },
    /// Distant it can no longer be: near.
    Near { copy: String },
    /// A script break, left for the script's author (#89).
    Script { copy: String, message: String },
}

impl fmt::Display for Resolution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Resolution::DroppedValue { copy, name } => write!(f, "{copy}: dropped value {name}"),
            Resolution::StartState { copy, state } => {
                write!(f, "{copy}: starts in the asset's start (no state {state:?})")
            }
            Resolution::Static { copy } => write!(f, "{copy}: static now"),
            Resolution::Near { copy } => write!(f, "{copy}: not distant now"),
            Resolution::Script { copy, message } => write!(f, "{copy}: script left as it is: {message}"),
        }
    }
}

/// What [`update`] and [`relink`] did: the keys that took the library's
/// version, those already the same, and every resolution, by copy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Updated {
    pub keys: Vec<String>,
    pub same: Vec<String>,
    pub resolutions: Vec<Resolution>,
}

/// What became of an imported level's asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportedAsset {
    /// Identical to a library asset, now linked to it.
    Reused { key: String, origin: AssetOrigin },
    /// Added to the user's library under `name` in `folder`.
    Added { key: String, name: String, folder: String },
    /// The level's own.
    Own { key: String },
}

impl fmt::Display for ImportedAsset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportedAsset::Reused { origin, .. } => write!(f, "reused {}", origin_label(origin)),
            ImportedAsset::Added { name, folder, .. } => write!(f, "added {name:?} as yours {folder}"),
            ImportedAsset::Own { key } => write!(f, "own {key}"),
        }
    }
}

/// What [`import`] did: the level's id and name (numbered when taken),
/// where it went, and each asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Imported {
    pub id: String,
    pub name: String,
    pub dir: PathBuf,
    pub assets: Vec<ImportedAsset>,
}

/// Why a library operation, an export or an import did nothing.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error(transparent)]
    Placed(#[from] PlacedError),
    #[error(transparent)]
    Asset(#[from] AssetError),
    /// The library's version breaks copies; `force` resolves them.
    #[error("asset {key}: the library's version breaks {}: update with force to resolve them", list(.breaks))]
    Breaks { key: String, breaks: Vec<Break> },
    #[error("{0}")]
    Bad(String),
    #[error("{0}")]
    Io(String),
}

fn list(breaks: &[Break]) -> String {
    breaks.iter().map(|b| b.to_string()).collect::<Vec<_>>().join("; ")
}

/// `own`, `built-in <folder>` or `yours <folder>`.
pub fn origin_label(o: &AssetOrigin) -> String {
    match o {
        AssetOrigin::Own => "own".into(),
        AssetOrigin::Builtin(f) => format!("built-in {f}"),
        AssetOrigin::Yours(f) => format!("yours {f}"),
    }
}

fn bad(message: impl Into<String>) -> LibraryError {
    LibraryError::Bad(message.into())
}

/// Library asset folder `dir` read whole (`read_asset_files`, within
/// `MAX_ASSET_BYTES`) and checked (`check_asset`); `None` when it does not.
fn read_library_asset(dir: &Path) -> Option<AssetSource> {
    if !dir.is_dir() {
        return None;
    }
    let src = read_asset_files(&DirFs::new(dir)).ok()?;
    check_asset(&src).ok()?;
    Some(src)
}

/// The library folder of `origin`: a `Builtin` one in `builtin` only, a
/// `Yours` one in `user` only; none for `Own`, an invalid folder name or
/// an absent library.
fn library_dir(origin: &AssetOrigin, builtin: Option<&Path>, user: Option<&Path>) -> Option<PathBuf> {
    let (root, folder) = match origin {
        AssetOrigin::Own => return None,
        AssetOrigin::Builtin(f) => (builtin?, f),
        AssetOrigin::Yours(f) => (user?, f),
    };
    valid_asset_id(folder).then(|| root.join(folder))
}

/// The libraries' version of every linked asset of `placed`, read from
/// the built-in library `builtin` and the user's `user` ([`Libraries`]).
pub fn read_libraries(placed: &PlacedSource, builtin: Option<&Path>, user: Option<&Path>) -> Libraries {
    let mut out = Libraries::new();
    for a in placed.file.assets.iter().filter(|a| a.origin != AssetOrigin::Own) {
        if !out.contains_key(&a.origin) {
            let src = library_dir(&a.origin, builtin, user).and_then(|d| read_library_asset(&d));
            out.insert(a.origin.clone(), src);
        }
    }
    out
}

/// One [`Link`] per asset of `placed` whose origin is not `Own`, in
/// placement order: [`Status::Same`] when the library's version is the
/// level's copy, [`Status::Changed`] with the copies it breaks, else
/// [`Status::Missing`] (none in `libs`, or it does not decode).
pub fn compare(placed: &PlacedSource, libs: &Libraries) -> Vec<Link> {
    placed
        .file
        .assets
        .iter()
        .filter(|a| a.origin != AssetOrigin::Own)
        .filter_map(|a| {
            let ours = placed.assets.get(&a.key)?;
            let status = match libs.get(&a.origin).and_then(Option::as_ref) {
                None => Status::Missing,
                Some(lib) if same_asset(ours, lib) => Status::Same,
                Some(lib) => match decode(lib) {
                    Ok(decoded) => Status::Changed { breaks: breaks(placed, &a.key, lib, &decoded) },
                    Err(_) => Status::Missing,
                },
            };
            Some(Link { key: a.key.clone(), origin: a.origin.clone(), status })
        })
        .collect()
}

/// [`compare`] over the libraries on disk ([`read_libraries`]).
pub fn compare_from(placed: &PlacedSource, builtin: Option<&Path>, user: Option<&Path>) -> Vec<Link> {
    compare(placed, &read_libraries(placed, builtin, user))
}

/// Why value `v` of exposed name `name` does not fit `file`.
fn value_problem(
    file: &olr_format::assets::AssetFile,
    name: &str,
    v: &olr_format::assets::ValueSource,
) -> Option<String> {
    let Some(x) = file.exposed.iter().find(|x| x.name == name) else {
        return Some(format!("no exposed property {name}"));
    };
    let t = match property_type(file, &x.property) {
        Ok(t) => t,
        Err(m) => return Some(m),
    };
    let range = t.range.map(|(lo, hi)| (x.min.unwrap_or(lo), x.max.unwrap_or(hi)));
    check_value(t, range, v, PLACED_FILE, name).err().map(|e| match e {
        FormatError::Invalid { message, .. } => message,
        e => e.to_string(),
    })
}

/// What `new` (decoded `decoded`), taking the place of asset `key`, would
/// break of `placed`'s copies of it: #73's `check_placed` rules (values,
/// states, distant) and #71's `live_refusal`, copy by copy. A size change
/// is no break.
fn breaks(placed: &PlacedSource, key: &str, new: &AssetSource, decoded: &Asset) -> Vec<Break> {
    let file = &new.file;
    let mut out = Vec::new();
    for c in placed.file.copies.iter().filter(|c| c.asset == key) {
        let copy = c.id.clone();
        for (name, v) in &c.values {
            if let Some(message) = value_problem(file, name, v) {
                out.push(Break::Value { copy: copy.clone(), name: name.clone(), message });
            }
        }
        if let Some(state) = &c.state
            && !file.states.iter().any(|s| s.name == *state)
        {
            out.push(Break::State { copy: copy.clone(), state: state.clone() });
        }
        if c.live
            && let Some(why) = live_refusal(decoded)
        {
            out.push(Break::Live { copy: copy.clone(), message: format!("cannot be Live: {why}") });
        }
        if let Some(depth) = c.distant
            && let Some(message) = distant_refusal(file, depth)
        {
            out.push(Break::Distant { copy: copy.clone(), message });
        }
    }
    out
}

/// Puts `new` in place of asset `key` in `src` (its origin `origin` when
/// given), resolving each of `breaks` and listing it.
fn swap_in(
    src: &mut PlacedSource,
    key: &str,
    new: &AssetSource,
    origin: Option<&AssetOrigin>,
    breaks: &[Break],
) -> Vec<Resolution> {
    let mut out = Vec::new();
    for b in breaks {
        let Some(c) = src.file.copies.iter_mut().find(|c| c.id == b.copy()) else { continue };
        let copy = c.id.clone();
        out.push(match b {
            Break::Value { name, .. } => {
                c.values.remove(name);
                Resolution::DroppedValue { copy, name: name.clone() }
            }
            Break::State { state, .. } => {
                c.state = None;
                Resolution::StartState { copy, state: state.clone() }
            }
            Break::Live { .. } => {
                c.live = false;
                Resolution::Static { copy }
            }
            Break::Distant { .. } => {
                c.distant = None;
                Resolution::Near { copy }
            }
            Break::Script { message, .. } => Resolution::Script { copy, message: message.clone() },
        });
    }
    src.assets.insert(key.to_string(), new.clone());
    if let Some(o) = origin
        && let Some(a) = src.file.assets.iter_mut().find(|a| a.key == key)
    {
        a.origin = o.clone();
    }
    out
}

/// `pack`'s placement, refusing an unconverted or a plain level and the
/// other kinds.
fn placement_of(pack: &Pack) -> Result<PlacedSource, LibraryError> {
    if pack.legacy_composition {
        return Err(bad(UNCONVERTED));
    }
    if !matches!(pack.source, Source::Level(_)) {
        return Err(bad("not a level pack"));
    }
    placement(pack).ok_or_else(|| bad("a plain level: it places no assets"))
}

/// Replaces each of `keys`' level copies with the library's version in
/// `libs`, its files as the library has them (call 7's one click). A key
/// whose library version is the same is listed in `same`; one the library
/// lacks, an `Own` one or an unknown one is refused. A version that breaks
/// copies is refused ([`LibraryError::Breaks`]) unless `force`; forced,
/// each break is resolved ([`Resolution`]: a value dropped, the start
/// state, static, near) and listed. Everything is checked first: on a
/// refusal `pack` is unchanged. Nothing is baked or saved.
pub fn update(
    pack: &mut Pack,
    keys: &[String],
    force: bool,
    libs: &Libraries,
) -> Result<Updated, LibraryError> {
    let src = placement_of(pack)?;
    let links = compare(&src, libs);
    let mut out = Updated::default();
    let mut todo: Vec<(String, AssetSource, Vec<Break>)> = Vec::new();
    for key in keys {
        let Some(link) = links.iter().find(|l| l.key == *key) else {
            return Err(match src.file.assets.iter().any(|a| a.key == *key) {
                true => bad(format!("asset {key}: the level's own, linked to no library")),
                false => bad(format!("asset {key}: no such asset in the level")),
            });
        };
        match &link.status {
            Status::Same => out.same.push(key.clone()),
            Status::Missing => {
                return Err(bad(format!(
                    "asset {key}: {} is not in the library: restore it from the level's copy",
                    origin_label(&link.origin)
                )));
            }
            Status::Changed { breaks } => {
                if !breaks.is_empty() && !force {
                    return Err(LibraryError::Breaks { key: key.clone(), breaks: breaks.clone() });
                }
                let new = libs.get(&link.origin).cloned().flatten().ok_or_else(|| bad("unreachable"))?;
                if !todo.iter().any(|(k, ..)| k == key) {
                    todo.push((key.clone(), new, breaks.clone()));
                }
            }
        }
    }
    let resolutions = commit(pack, |src| {
        let mut all = Vec::new();
        for (key, new, breaks) in &todo {
            all.extend(swap_in(src, key, new, None, breaks));
        }
        Ok(all)
    })?;
    out.keys = todo.into_iter().map(|(k, ..)| k).collect();
    out.resolutions = resolutions;
    Ok(out)
}

/// [`update`] with the libraries on disk.
pub fn update_from(
    pack: &mut Pack,
    keys: &[String],
    force: bool,
    builtin: Option<&Path>,
    user: Option<&Path>,
) -> Result<Updated, LibraryError> {
    let src = placement_of(pack)?;
    update(pack, keys, force, &read_libraries(&src, builtin, user))
}

/// Points the level's asset `key` at library asset `origin` (not `Own`),
/// taking `asset`'s files as [`update`] takes a library version: breaks
/// refused unless `force`, then resolved and listed. How a level follows
/// the user's edited copy of a built-in asset (call 18). The ground is
/// never relinked; on a refusal `pack` is unchanged.
pub fn relink(
    pack: &mut Pack,
    key: &str,
    origin: AssetOrigin,
    asset: &AssetSource,
    force: bool,
) -> Result<Updated, LibraryError> {
    let src = placement_of(pack)?;
    if origin == AssetOrigin::Own {
        return Err(bad(format!(
            "asset {key}: relink names a library asset (adopt makes it the level's own)"
        )));
    }
    if key == GROUND_KEY {
        return Err(bad("the ground is the level's own: it follows no library"));
    }
    if !src.file.assets.iter().any(|a| a.key == key) {
        return Err(bad(format!("asset {key}: no such asset in the level")));
    }
    let decoded = decode(asset)?;
    let breaks = breaks(&src, key, asset, &decoded);
    if !breaks.is_empty() && !force {
        return Err(LibraryError::Breaks { key: key.into(), breaks });
    }
    let resolutions = commit(pack, |src| Ok(swap_in(src, key, asset, Some(&origin), &breaks)))?;
    Ok(Updated { keys: vec![key.into()], same: Vec::new(), resolutions })
}

/// [`relink`] to `origin`'s folder in the libraries on disk.
pub fn relink_from(
    pack: &mut Pack,
    key: &str,
    origin: AssetOrigin,
    builtin: Option<&Path>,
    user: Option<&Path>,
    force: bool,
) -> Result<Updated, LibraryError> {
    let asset =
        library_dir(&origin, builtin, user).and_then(|d| read_library_asset(&d)).ok_or_else(|| {
            bad(format!("{}: no such asset in the library, or it does not read", origin_label(&origin)))
        })?;
    relink(pack, key, origin, &asset, force)
}

/// Writes the level's copy of linked asset `key` into the user's library
/// `user` as a new asset (call 17: a missing library asset offered back
/// from the level's copy), its files as the level holds them, in a new
/// folder named after it; a name one of the user's assets has numbered
/// (`copy_name`, as `import_into_library` does, the level's copy renamed
/// alike). The origin then points at it: `Yours(<folder>)`, whether it was
/// `Yours` or `Builtin`. Returns the folder. Nothing is written when the
/// result would not check; `pack` is unchanged on a failure.
pub fn restore(pack: &mut Pack, key: &str, user: &Path) -> Result<String, LibraryError> {
    let src = placement_of(pack)?;
    let a = src
        .file
        .assets
        .iter()
        .find(|a| a.key == key)
        .ok_or_else(|| bad(format!("asset {key}: no such asset in the level")))?;
    if a.origin == AssetOrigin::Own {
        return Err(bad(format!("asset {key}: the level's own, linked to no library")));
    }
    let mut copy =
        src.assets.get(key).cloned().ok_or_else(|| bad(format!("asset {key}: its folder is missing")))?;
    let names = user_names(user);
    if names.contains(&copy.file.name) {
        copy.file.name = copy_name(&copy.file.name, names.iter().map(String::as_str));
    }
    let folder = free_folders(user, &[copy.file.name.as_str()])?.remove(0);
    let before = pack.clone();
    let origin = AssetOrigin::Yours(folder.clone());
    commit(pack, |src| {
        src.assets.insert(key.to_string(), copy.clone());
        if let Some(a) = src.file.assets.iter_mut().find(|a| a.key == key) {
            a.origin = origin;
        }
        Ok(())
    })?;
    if let Err(e) = add_source_to_library(user, &copy, Some(&folder)) {
        *pack = before;
        return Err(e.into());
    }
    Ok(folder)
}

/// The names of the user's assets in library `dir`.
fn user_names(dir: &Path) -> Vec<String> {
    crate::asset::list_library(dir, usize::MAX).0.into_iter().map(|(_, s)| s.name).collect()
}

/// Makes the level's asset `key` its own (`Own`): unlinked from its
/// library, the level keeps its version and hears of no change to it. An
/// asset already its own stays so.
pub fn adopt(pack: &mut Pack, key: &str) -> Result<(), LibraryError> {
    let src = placement_of(pack)?;
    if !src.file.assets.iter().any(|a| a.key == key) {
        return Err(bad(format!("asset {key}: no such asset in the level")));
    }
    commit(pack, |src| {
        if let Some(a) = src.file.assets.iter_mut().find(|a| a.key == key) {
            a.origin = AssetOrigin::Own;
        }
        Ok(())
    })?;
    Ok(())
}

/// Why level `pack` may not travel: it does not check (`olr_pack::validate`
/// errors), or its bake is stale (`placed::stale`, the theme's back tile
/// from `back_tile`).
fn refusal(pack: &Pack, back_tile: BackTile) -> Result<(), LibraryError> {
    let Source::Level(level) = &pack.source else { return Err(bad("not a level pack")) };
    if pack.legacy_composition {
        return Err(bad(UNCONVERTED));
    }
    let files = pack::files(pack).map_err(|e| bad(e.to_string()))?;
    pack::check(&files).map_err(|e| bad(format!("the level does not check: {e}")))?;
    if let Some(src) = &pack.placed {
        check_placed(src, level.size, level.art_scale).map_err(PlacedError::from)?;
    }
    if placement(pack).is_some() {
        let tile = back_tile(&level.theme).map_err(LibraryError::Bad)?;
        let stale = stale(pack, &tile)?;
        if !stale.is_empty() {
            return Err(bad(format!(
                "the baked files differ from the placement ({}): save it in the editor first, or run olr-cli level bake",
                stale.join(", ")
            )));
        }
    }
    Ok(())
}

/// The level pack at `dir` as one `.olrlevel` archive (issue #76): every
/// file of it (manifest, play files, `editor/` with its placement, every
/// asset copy and the scripts) as it is on disk. Refused for a pack that
/// does not load, a pack that is not a level, does not check, or whose
/// bake is stale. The libraries are not read: the pack's copies travel,
/// so an export is what its author plays.
pub fn export(dir: &Path, back_tile: BackTile) -> Result<Vec<u8>, LibraryError> {
    let pack = pack::load(dir).map_err(|e| bad(format!("the level does not load: {e}")))?;
    refusal(&pack, back_tile)?;
    archive::encode_level_export(&DirFs::new(dir)).map_err(|e| bad(e.to_string()))
}

/// The number of library assets (origin not `Own`) and own assets of
/// `pack`'s placement but the ground: what an export carries.
pub fn asset_counts(pack: &Pack) -> (usize, usize) {
    let Some(p) = placement(pack) else { return (0, 0) };
    let linked = p.file.assets.iter().filter(|a| a.origin != AssetOrigin::Own).count();
    (linked, p.file.assets.len() - linked)
}

/// A temporary folder of its own, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Scratch, LibraryError> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let base = std::env::temp_dir();
        for _ in 0..1000 {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = base.join(format!("olr-import-{}-{n}", std::process::id()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Scratch(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(LibraryError::Io(format!("{}: {e}", dir.display()))),
            }
        }
        Err(LibraryError::Io("no temporary folder for the import".into()))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The pack ids and names among the packs of `dir` (each folder's name
/// and its manifest's id, lower-cased).
fn packs_in(dir: &Path) -> (BTreeSet<String>, Vec<String>) {
    let (mut ids, mut names) = (BTreeSet::new(), Vec::new());
    let Ok(rd) = std::fs::read_dir(dir) else { return (ids, names) };
    for e in rd.filter_map(Result::ok) {
        if let Some(n) = e.file_name().to_str() {
            ids.insert(n.to_lowercase());
        }
        if let Ok(m) = olr_pack::read_manifest(&DirFs::new(e.path())) {
            ids.insert(m.id.to_lowercase());
            names.push(m.name);
        }
    }
    (ids, names)
}

/// Where [`import`] puts things: the user's packs (`<data>/packs`), the
/// user's asset library (`<data>/assets`), the built-in asset library
/// (`content/assets`, if any) and the bundled packs' ids (never taken by
/// an import).
pub struct Places<'a> {
    pub packs: &'a Path,
    pub user: &'a Path,
    pub builtin: Option<&'a Path>,
    pub bundled: &'a BTreeSet<String>,
}

/// Imports a `.olrlevel` archive (issue #76): unpacked into a temporary
/// folder within `Limits::PACK` (hostile input fails here), then checked
/// whole (a level pack that validates, whose placement checks and whose
/// bake is not stale; anything else refused and nothing written). The
/// level goes among the user's packs as `<packs>/<id>`: an id taken by
/// one of them or a bundled pack gets a new one (`<id>-2`, …) and a
/// numbered name (`copy_name`). Each linked copy is matched (call 9):
/// identical to a built-in asset, linked to it; else identical to one of
/// the user's, linked to it; else added to the user's library as a new
/// asset (a name the user has numbered, "Console 2"; a new folder), the
/// level's copy renamed alike. `Own` assets stay the level's. Nothing is
/// ever written over: past `MAX_USER_ASSETS` it refuses before writing.
/// The level is written, then the assets (each through `replace_dir`); a
/// failure midway removes what this import wrote.
pub fn import(bytes: &[u8], places: &Places, back_tile: BackTile) -> Result<Imported, LibraryError> {
    let scratch = Scratch::new()?;
    let root = scratch.0.join("level");
    std::fs::create_dir(&root).map_err(|e| LibraryError::Io(e.to_string()))?;
    archive::decode_level_export(&mut &bytes[..], &mut DirFs::new(&root), &Limits::PACK)
        .map_err(|e| bad(format!("not a level export: {e}")))?;
    let mut pack = pack::load(&root).map_err(|e| bad(format!("the level does not load: {e}")))?;
    if pack.manifest.kind != PackKind::Level {
        return Err(bad("not a level pack"));
    }
    refusal(&pack, back_tile)?;

    // The id and name.
    let (mut taken, names) = packs_in(places.packs);
    taken.extend(places.bundled.iter().map(|s| s.to_lowercase()));
    let own = crate::new::pack_id(&pack.manifest.id, |_| false);
    let id = crate::new::pack_id(&pack.manifest.id, |c| taken.contains(c));
    if id != own {
        let mut all = names.clone();
        all.push(pack.manifest.name.clone());
        let name = copy_name(&pack.manifest.name, all.iter().map(String::as_str));
        if let Source::Level(l) = &mut pack.source
            && l.name == pack.manifest.name
        {
            l.name = name.clone();
        }
        pack.manifest.name = name;
    }
    pack.manifest.id = id.clone();
    let dir = places.packs.join(&id);
    if dir.exists() {
        return Err(bad(format!("{}: there already", dir.display())));
    }

    // The assets: each linked copy reused from a library or added to the
    // user's (two copies of one new asset added once).
    enum Plan {
        Own,
        Linked(AssetOrigin),
        New(usize),
    }
    let mut assets = Vec::new();
    let mut added: Vec<(AssetSource, AssetSource)> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    if let Some(src) = placement(&pack) {
        let read_all = |d: &Path| -> Vec<(String, AssetSource)> {
            library_folders(d)
                .into_iter()
                .filter_map(|f| Some((f.clone(), read_library_asset(&d.join(&f))?)))
                .collect()
        };
        let builtin = places.builtin.map(read_all).unwrap_or_default();
        let yours = read_all(places.user);
        let mut user_names = user_names(places.user);
        let mut plans: Vec<(String, Plan)> = Vec::new();
        for a in &src.file.assets {
            let Some(ours) = src.assets.get(&a.key) else { continue };
            let folder = match &a.origin {
                AssetOrigin::Own => {
                    plans.push((a.key.clone(), Plan::Own));
                    continue;
                }
                AssetOrigin::Builtin(f) | AssetOrigin::Yours(f) => f.as_str(),
            };
            // The origin's own folder first, then the rest in order.
            let find = |lib: &[(String, AssetSource)]| {
                lib.iter()
                    .find(|(f, s)| f == folder && same_asset(s, ours))
                    .or_else(|| lib.iter().find(|(_, s)| same_asset(s, ours)))
                    .map(|(f, _)| f.clone())
            };
            let plan = if let Some(f) = find(&builtin) {
                Plan::Linked(AssetOrigin::Builtin(f))
            } else if let Some(f) = find(&yours) {
                Plan::Linked(AssetOrigin::Yours(f))
            } else if let Some(i) = added.iter().position(|(o, _)| same_asset(o, ours)) {
                Plan::New(i)
            } else {
                let mut copy = ours.clone();
                if user_names.contains(&copy.file.name) {
                    copy.file.name = copy_name(&copy.file.name, user_names.iter().map(String::as_str));
                }
                user_names.push(copy.file.name.clone());
                added.push((ours.clone(), copy));
                Plan::New(added.len() - 1)
            };
            plans.push((a.key.clone(), plan));
        }
        // Their folders, all at once: refused past `MAX_USER_ASSETS`.
        let names: Vec<&str> = added.iter().map(|(_, c)| c.file.name.as_str()).collect();
        folders = free_folders(places.user, &names)?;
        let mut said = BTreeSet::new();
        for (key, plan) in &plans {
            assets.push(match plan {
                Plan::Own => ImportedAsset::Own { key: key.clone() },
                Plan::Linked(o) => ImportedAsset::Reused { key: key.clone(), origin: o.clone() },
                Plan::New(i) if said.insert(*i) => ImportedAsset::Added {
                    key: key.clone(),
                    name: added[*i].1.file.name.clone(),
                    folder: folders[*i].clone(),
                },
                Plan::New(i) => ImportedAsset::Reused {
                    key: key.clone(),
                    origin: AssetOrigin::Yours(folders[*i].clone()),
                },
            });
        }
        // The level's copies point at their library assets, renamed alike.
        commit(&mut pack, |src| {
            for (key, plan) in &plans {
                let (origin, name) = match plan {
                    Plan::Own => continue,
                    Plan::Linked(o) => (o.clone(), None),
                    Plan::New(i) => (AssetOrigin::Yours(folders[*i].clone()), Some(&added[*i].1.file.name)),
                };
                if let Some(a) = src.file.assets.iter_mut().find(|a| a.key == *key) {
                    a.origin = origin;
                }
                if let (Some(n), Some(s)) = (name, src.assets.get_mut(key)) {
                    s.file.name = n.clone();
                }
            }
            Ok(())
        })?;
    }
    let files = pack::files(&pack).map_err(|e| bad(e.to_string()))?;
    pack::check(&files).map_err(|e| bad(format!("the imported level does not check: {e}")))?;

    // Written: the level, then the assets; undone on a failure.
    std::fs::create_dir_all(places.packs)
        .map_err(|e| LibraryError::Io(format!("{}: {e}", places.packs.display())))?;
    olr_pack::install::replace_dir(&dir, |d| {
        let mut sink = DirFs::new(d);
        for (path, bytes) in &files {
            sink.write(path, bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        Ok(())
    })
    .map_err(|e| LibraryError::Io(format!("{}: {e}", dir.display())))?;
    let mut written: Vec<PathBuf> = Vec::new();
    let undo = |written: &[PathBuf]| {
        let _ = std::fs::remove_dir_all(&dir);
        for w in written {
            let _ = std::fs::remove_dir_all(w);
        }
    };
    if !added.is_empty()
        && let Err(e) = std::fs::create_dir_all(places.user)
    {
        undo(&written);
        return Err(LibraryError::Io(format!("{}: {e}, nothing imported", places.user.display())));
    }
    for ((_, src), folder) in added.iter().zip(&folders) {
        let to = places.user.join(folder);
        if let Err(e) = save_dir(&to, src) {
            undo(&written);
            return Err(LibraryError::Io(format!("{e}: nothing imported")));
        }
        written.push(to);
    }
    Ok(Imported { id, name: pack.manifest.name.clone(), dir, assets })
}
