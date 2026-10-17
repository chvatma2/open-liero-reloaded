//! Importing OLX content into packs (spec §9.3, §9.4; plan 7 ruling Q3):
//! the one pipeline `olr-cli import` (and so `cargo xtask
//! fetch-olx-content`) run. It reads an OLX
//! game directory (`share/gamedir`) through the importers (`olr-import-lgs`,
//! `olr-import-lxl`, `olr-import-gfx`) and writes one pack per item to
//! `<out>/<pack id>/`, plus `import-report.ron`.
//!
//! - **What to import** is an [`ImportPlan`]: the items `olr-cli import`'s
//!   flags name (the manifest's), or [`discover`]'s list of everything an
//!   OLX game directory holds, found where OLX's menus look for it.
//! - **One item at a time**, each reported through the progress callback,
//!   which may stop the import between items (a cancel). An item that
//!   fails is an [`ImportEntry`] with its error; the rest still import.
//!   Ids that collide within one run get `-2`, `-3`, ... in turn.
//! - **No half-written pack is ever visible:** a pack is written to a
//!   hidden directory (`.import-<id>`; `olr_content::Content::open`
//!   skips names starting with `.`) and renamed into place; the pack it
//!   replaces is first renamed aside (`.replaced-<id>`) and then removed
//!   (one that cannot be is a warning: the new pack is in place). An
//!   import killed between the two renames leaves only `.replaced-<id>`:
//!   `Content::open` still lists it as the pack, and the next import puts
//!   it back; every other leftover is removed by the next import (one
//!   that cannot be is a note, `ImportReport::notes`).
//! - **Linked directories:** a symbolic link to a mod, `levels`, `skins`,
//!   `data`, `data/themes` or a theme directory is followed, one level
//!   (nothing is walked, so a link loop cannot recur).
//! - [`discover_until`] stops when asked ([`Cancelled`]).
//! - **Bounded memory:** every file of the game directory is read at most
//!   [`MAX_SOURCE_BYTES`] (no OLX file is near it), and only regular files
//!   (`DirFs`); every image is decoded only after its header's size is
//!   checked (`olr_pack::image_io::IMAGES`, `LEVEL_ART`: at most 64 MiB, a
//!   level's art 256 MiB), and a level's data decompresses to at most
//!   its size's worth (`olr-import-lxl`). A small file declaring a huge
//!   image (a decompression bomb) is that item's error, never an
//!   allocation.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::Context as _;
use olr_data::manifest::Origin;
use olr_pack::install::{OLD_PREFIX, TMP_PREFIX, newest_aside_first, replace_dir};
use olr_vfs::{ContentSource, DirFs, VPath, VfsError, find_ci};
use serde::Serialize;

/// The import's report file in the output directory.
pub const REPORT_FILE: &str = "import-report.ron";

/// The most bytes read from any one file of an OLX game directory.
pub const MAX_SOURCE_BYTES: u64 = olr_pack::MAX_FILE_BYTES;

/// The files of the core pack `olx-core` (spec §19 E9), as
/// `content/olx-manifest.ron` lists them (`tests/import.rs` checks they
/// agree): OLX's shared game sprites, game sounds and frontend colours.
pub const OLX_CORE_FILES: [&str; 18] = [
    "data/gfx/crosshair.png",
    "data/gfx/muzzle.png",
    "data/gfx/explosion.png",
    "data/gfx/smoke.png",
    "data/gfx/chemsmoke.png",
    "data/gfx/spawn.png",
    "data/gfx/sparkle.png",
    "data/gfx/giblets.png",
    "data/gfx/hook.png",
    "data/gfx/bonus.png",
    "data/gfx/health.png",
    "data/sounds/throw.ogg",
    "data/sounds/pickup.ogg",
    "data/sounds/bump.ogg",
    "data/sounds/death1.ogg",
    "data/sounds/death2.ogg",
    "data/sounds/death3.ogg",
    "data/frontend/colours.cfg",
];

/// What to import from an OLX game directory, by paths relative to it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportPlan {
    /// Mod directories, such as `MW 1.0`.
    pub mods: Vec<String>,
    /// Level files, such as `levels/CastleStrike.lxl`.
    pub levels: Vec<String>,
    /// Theme directories, such as `data/themes/Dirt`.
    pub themes: Vec<String>,
    /// Every PNG in `skins/`.
    pub skins: bool,
    /// The core pack's files (none: no core pack).
    pub core: Vec<String>,
    /// More skins from other directories, each with its own origin
    /// (`olr-cli import --extra-skins`: OLX 0.58's release has skins its
    /// repository lacks).
    pub extra_skins: Vec<SkinSource>,
}

/// A directory of OLX skin PNGs from outside the game directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinSource {
    /// The directory holding the PNGs (OLX's `skins/` itself).
    pub dir: PathBuf,
    /// Recorded as each pack's origin source, such as `OpenLieroX 0.58 rc5`.
    pub source: String,
    /// Recorded as each pack's origin revision.
    pub revision: String,
}

impl ImportPlan {
    /// Nothing to import.
    pub fn is_empty(&self) -> bool {
        self.mods.is_empty()
            && self.levels.is_empty()
            && self.themes.is_empty()
            && !self.skins
            && self.core.is_empty()
            && self.extra_skins.is_empty()
    }
}

/// How an import writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportOptions {
    /// Recorded in each pack's origin (a git commit, or `local`).
    pub revision: String,
    /// The output directory is this import's alone: packs of earlier runs
    /// that this run did not produce are removed (`olr-cli import`, whose
    /// `content/packs` is the manifest's). Off to collect several imports
    /// in one directory, such as a user `packs/`.
    pub prune: bool,
}

/// One line of `import-report.ron`: an item, the pack it became or why
/// it failed, and its warnings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImportEntry {
    pub source: String,
    pub pack: Option<String>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

/// What an import did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Every item tried, in order.
    pub entries: Vec<ImportEntry>,
    /// The items the plan had (more than `entries` when cancelled).
    pub total: usize,
    /// The progress callback stopped it before every item was tried.
    pub cancelled: bool,
    /// Problems outside any item (a hidden pack an earlier import left
    /// that cannot be removed), to show once.
    pub notes: Vec<String>,
}

impl ImportReport {
    /// Items that failed.
    pub fn failed(&self) -> usize {
        self.entries.iter().filter(|e| e.error.is_some()).count()
    }

    /// Packs written.
    pub fn imported(&self) -> usize {
        self.entries.len() - self.failed()
    }

    /// Warnings over every item.
    pub fn warnings(&self) -> usize {
        self.entries.iter().map(|e| e.warnings.len()).sum()
    }
}

/// An import's progress, for its callback.
#[derive(Clone, Copy, Debug)]
pub enum Progress<'a> {
    /// Item `index` of `total` (from 0) starts.
    Item { index: usize, total: usize, source: &'a str },
    /// It is done: imported, or failed.
    Done { index: usize, total: usize, entry: &'a ImportEntry },
}

enum Imported {
    Mod(olr_import_lgs::ImportedMod),
    Level(olr_import_lxl::ImportedLevel),
    Skin(olr_import_gfx::ImportedSkin),
    Theme(olr_import_gfx::ImportedTheme),
    Core(olr_import_gfx::ImportedCore),
}

impl Imported {
    fn id(&self) -> &str {
        match self {
            Imported::Mod(m) => &m.pack.manifest.id,
            Imported::Level(l) => &l.pack.manifest.id,
            Imported::Skin(s) => &s.pack.manifest.id,
            Imported::Theme(t) => &t.pack.manifest.id,
            Imported::Core(c) => &c.pack.manifest.id,
        }
    }

    fn set_id(&mut self, id: String) {
        match self {
            Imported::Mod(m) => m.pack.manifest.id = id,
            Imported::Level(l) => l.pack.manifest.id = id,
            Imported::Skin(s) => s.pack.manifest.id = id,
            Imported::Theme(t) => t.pack.manifest.id = id,
            Imported::Core(c) => c.pack.manifest.id = id,
        }
    }

    fn warnings(&self) -> Vec<String> {
        match self {
            Imported::Mod(m) => m.warnings.clone(),
            Imported::Level(l) => l.warnings.clone(),
            Imported::Skin(s) => s.warnings.clone(),
            Imported::Theme(t) => t.warnings.clone(),
            Imported::Core(c) => c.warnings.clone(),
        }
    }

    /// Writes the pack into the (new, empty) directory `dir`.
    fn write_to(&self, dir: &Path) -> anyhow::Result<()> {
        let mut sink = DirFs::new(dir);
        match self {
            Imported::Mod(m) => olr_pack::write_mod(&mut sink, &m.pack)?,
            Imported::Level(l) => olr_pack::write_level(&mut sink, &l.pack)?,
            Imported::Skin(s) => olr_pack::write_skin(&mut sink, &s.pack)?,
            Imported::Theme(t) => olr_pack::write_theme(&mut sink, &t.pack)?,
            Imported::Core(c) => olr_pack::write_core(&mut sink, &c.pack)?,
        }
        Ok(())
    }

    /// Writes the pack as `<out>/<id>/`, replacing an older one: written
    /// hidden, then renamed into place (`olr_pack::install::replace_dir`).
    /// The old pack that cannot be removed once replaced is a warning
    /// (returned): the new one is in place, and the next import tries again.
    fn write(&self, out: &Path, id: &str) -> anyhow::Result<Vec<String>> {
        Ok(replace_dir(&out.join(id), |d| {
            self.write_to(d).map_err(|e| std::io::Error::other(format!("{e:#}")))
        })?)
    }
}

fn remove_dir(dir: &Path) -> anyhow::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", dir.display()))
        }
        _ => Ok(()),
    }
}

enum Job {
    Mod(String),
    Level(String),
    Theme(String),
    Skin(VPath),
    /// A skin of `ImportPlan::extra_skins[source]`, by its file name.
    ExtraSkin {
        source: usize,
        name: VPath,
    },
    /// The core pack's files.
    Core(Vec<String>),
}

impl Job {
    fn source(&self) -> String {
        match self {
            Job::Mod(s) | Job::Level(s) | Job::Theme(s) => s.clone(),
            Job::Skin(p) => p.to_string(),
            Job::ExtraSkin { name, .. } => format!("skins/{name}"),
            Job::Core(_) => "data".to_string(),
        }
    }

    /// Imports the item: from `olx` (the game directory), or from its own
    /// directory in `extras` (`ImportPlan::extra_skins`, read alike).
    fn run(
        &self,
        olx: &dyn ContentSource,
        revision: &str,
        extras: &[(Bounded, &SkinSource)],
    ) -> anyhow::Result<Imported> {
        let path = self.source();
        let (src, origin): (&dyn ContentSource, Origin) = match self {
            Job::ExtraSkin { source, .. } => {
                let (dir, from) = extras.get(*source).context("no such skin source")?;
                (dir, Origin { source: from.source.clone(), path, revision: from.revision.clone() })
            }
            _ => (
                olx,
                Origin {
                    source: "OpenLieroX".to_string(),
                    path: format!("share/gamedir/{path}"),
                    revision: revision.to_string(),
                },
            ),
        };
        Ok(match self {
            Job::Mod(m) => Imported::Mod(olr_import_lgs::import_mod(src, &VPath::new(m)?, origin)?),
            Job::Level(l) => {
                let mut level = olr_import_lxl::import_lxl(src, &VPath::new(l)?, origin)?;
                // The menus' thumbnail (plan 7 G6), from the art.
                level.pack.thumb = Some(olr_import_gfx::level_thumbnail(&level.pack.level));
                Imported::Level(level)
            }
            Job::Theme(t) => Imported::Theme(olr_import_gfx::import_theme(src, &VPath::new(t)?, origin)?),
            Job::Skin(p) | Job::ExtraSkin { name: p, .. } => {
                Imported::Skin(olr_import_gfx::import_skin(src, p, origin)?)
            }
            Job::Core(files) => {
                let files = files.iter().map(|f| VPath::new(f)).collect::<Result<Vec<_>, _>>()?;
                Imported::Core(olr_import_gfx::import_core(src, &files, origin)?)
            }
        })
    }
}

/// The game directory read with every file bounded by [`MAX_SOURCE_BYTES`].
struct Bounded(DirFs);

/// How deep `Bounded::list` follows a linked directory: in the root, in
/// `data` and in `data/themes`.
const FOLLOW_DEPTH: usize = 2;

impl ContentSource for Bounded {
    fn read(&self, path: &VPath) -> Result<Vec<u8>, VfsError> {
        self.0.read_max(path, MAX_SOURCE_BYTES)
    }

    /// A symbolic link to a directory is listed as a directory in the game
    /// directory's top levels (mods, `levels`, `skins`, `data`,
    /// `data/themes`, the themes): one level, nothing is walked, so a link
    /// loop cannot recur.
    fn list(&self, dir: &VPath) -> Result<Vec<String>, VfsError> {
        let mut names = self.0.list(dir)?;
        if dir.components().count() <= FOLLOW_DEPTH {
            let mut native = self.0.root().to_path_buf();
            native.extend(dir.components());
            for name in names.iter_mut().filter(|n| !n.ends_with('/')) {
                let linked = std::fs::symlink_metadata(native.join(&**name)).is_ok_and(|m| m.is_symlink());
                if linked && std::fs::metadata(native.join(&**name)).is_ok_and(|m| m.is_dir()) {
                    name.push('/');
                }
            }
            names.sort();
        }
        Ok(names)
    }

    fn exists(&self, path: &VPath) -> bool {
        self.0.exists(path)
    }

    fn read_max(&self, path: &VPath, limit: u64) -> Result<Vec<u8>, VfsError> {
        self.0.read_max(path, limit.min(MAX_SOURCE_BYTES))
    }

    fn read_prefix(&self, path: &VPath, n: usize) -> Result<Vec<u8>, VfsError> {
        self.0.read_prefix(path, n)
    }

    fn file_len(&self, path: &VPath) -> Result<u64, VfsError> {
        self.0.file_len(path)
    }
}

/// The jobs `plan` asks for, in the report's order: mods, levels, themes,
/// the core pack, then the skins.
fn jobs(
    olx: &dyn ContentSource,
    plan: &ImportPlan,
    extras: &[(Bounded, &SkinSource)],
) -> anyhow::Result<Vec<Job>> {
    let mut jobs: Vec<Job> = Vec::new();
    jobs.extend(plan.mods.iter().cloned().map(Job::Mod));
    jobs.extend(plan.levels.iter().cloned().map(Job::Level));
    jobs.extend(plan.themes.iter().cloned().map(Job::Theme));
    if !plan.core.is_empty() {
        jobs.push(Job::Core(plan.core.clone()));
    }
    if plan.skins {
        let skins = VPath::new("skins")?;
        for name in olx.list(&skins).context("listing skins/")? {
            if name.to_ascii_lowercase().ends_with(".png") {
                jobs.push(Job::Skin(skins.join(&name)?));
            }
        }
    }
    for (source, extra) in extras.iter().enumerate() {
        let names =
            extra.0.list(&VPath::root()).with_context(|| format!("listing {}", extra.1.dir.display()))?;
        for name in names {
            if name.to_ascii_lowercase().ends_with(".png") {
                jobs.push(Job::ExtraSkin { source, name: VPath::new(&name)? });
            }
        }
    }
    Ok(jobs)
}

/// Imports `plan`'s items from OLX game directory `olx` into `out` (made
/// if missing) and writes `import-report.ron` there. `progress` hears of
/// each item before and after it; returning `false` stops the import
/// before the next one (the report says `cancelled`). An item that fails
/// is in the report; only a failure to list the skins, to make `out` or
/// to write a pack (a full disk, no permission) stops it with an error.
pub fn import(
    olx: &Path,
    plan: &ImportPlan,
    out: &Path,
    options: &ImportOptions,
    progress: &mut dyn FnMut(Progress<'_>) -> bool,
) -> anyhow::Result<ImportReport> {
    let src = Bounded(DirFs::new(olx));
    let extras: Vec<(Bounded, &SkinSource)> =
        plan.extra_skins.iter().map(|e| (Bounded(DirFs::new(&e.dir)), e)).collect();
    let jobs = jobs(&src, plan, &extras)?;
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let notes = restore_leftovers(out)?;
    let total = jobs.len();
    let mut report = ImportReport { entries: Vec::new(), total, cancelled: false, notes };
    let mut ids = BTreeSet::new();
    for (index, job) in jobs.iter().enumerate() {
        let source = job.source();
        if !progress(Progress::Item { index, total, source: &source }) {
            report.cancelled = true;
            break;
        }
        let entry = match job.run(&src, &options.revision, &extras) {
            Ok(mut item) => {
                let base = item.id().to_string();
                let id = (1..)
                    .map(|i| if i == 1 { base.clone() } else { format!("{base}-{i}") })
                    .find(|c| !ids.contains(c))
                    .expect("unbounded");
                item.set_id(id.clone());
                ids.insert(id.clone());
                let left = item.write(out, &id)?;
                let warnings = item.warnings().into_iter().chain(left).collect();
                ImportEntry { source, pack: Some(id), error: None, warnings }
            }
            Err(e) => ImportEntry { source, pack: None, error: Some(format!("{e:#}")), warnings: vec![] },
        };
        report.entries.push(entry);
        let entry = report.entries.last().expect("pushed");
        if !progress(Progress::Done { index, total, entry }) {
            report.cancelled = index + 1 < total;
            break;
        }
    }
    // Packs from earlier runs that this run did not produce (dropped from the
    // manifest, failed this time, or renamed) must not linger as current content.
    if options.prune && !report.cancelled {
        for entry in std::fs::read_dir(out)? {
            let path = entry?.path();
            let id = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            if path.join(olr_pack::MANIFEST).is_file() && !ids.contains(&id) {
                std::fs::remove_dir_all(&path)
                    .with_context(|| format!("removing stale {}", path.display()))?;
            }
        }
    }
    let text = ron::ser::to_string_pretty(&report.entries, ron::ser::PrettyConfig::default())? + "\n";
    let file = out.join(REPORT_FILE);
    std::fs::write(&file, text).with_context(|| format!("writing {}", file.display()))?;
    Ok(report)
}

/// Repairs what an import killed mid-pack left behind: a pack moved
/// aside (`.replaced-<id>`) whose new one never came is put back; every
/// other hidden pack (half-written, or replaced) is removed. One that
/// cannot be removed is a note.
fn restore_leftovers(out: &Path) -> anyhow::Result<Vec<String>> {
    let mut notes = Vec::new();
    let mut hidden: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(out).with_context(|| format!("reading {}", out.display()))? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if (name.starts_with(TMP_PREFIX) || name.starts_with(OLD_PREFIX)) && path.is_dir() {
            hidden.push(path);
        }
    }
    hidden.sort();
    // For each id the newest pack moved aside is put back; the rest go.
    newest_aside_first(&mut hidden);
    for path in hidden {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if let Some(rest) = name.strip_prefix(OLD_PREFIX) {
            let id = rest.split('~').next().unwrap_or(rest);
            let home = out.join(id);
            if !id.is_empty() && !home.exists() && path.join(olr_pack::MANIFEST).is_file() {
                std::fs::rename(&path, &home).with_context(|| format!("restoring {}", home.display()))?;
                continue;
            }
        }
        if let Err(e) = remove_dir(&path) {
            notes.push(format!("{e:#}"));
        }
    }
    Ok(notes)
}

/// What [`discover`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovered {
    /// The game directory: the path given, or its `share/gamedir` (an
    /// OpenLieroX checkout or install root).
    pub gamedir: PathBuf,
    pub plan: ImportPlan,
    /// What was found but is not imported, for the player.
    pub notes: Vec<String>,
}

/// Everything importable in the OLX game directory `path` (or in
/// `path/share/gamedir`), found where OLX's menus look:
///
/// - mods: the directories with a `script.lgs` (`Menu_Local_FillModList`
///   lists every directory and keeps those `CGameScript::CheckFile` takes,
///   `checkLXBinMod` reading `<dir>/script.lgs`);
/// - levels: the `.lxl` files in `levels/` (OLX's map list);
/// - themes: the directories in `data/themes/` (`CMap::findRandomTheme`);
/// - skins: `skins/` (OLX's skin list; the PNGs are imported);
/// - the core pack when every one of [`OLX_CORE_FILES`] is there (a
///   partial one would replace the bundled core with placeholders).
///
/// Names match case-insensitively, as OLX's file lookup does. A path that
/// is missing, unreadable or holds none of these is an error saying so.
///
/// OLX: src/client/DeprecatedGUI/Menu_Local.cpp:895-922 (`ModAdder`,
/// `Menu_Local_FillModList`), src/common/CGameScript.cpp:472-514,561-577
/// (`checkLXBinMod`, `CGameScript::CheckFile`), src/common/CMap.cpp:2458-2466
/// (the map list), src/common/CMap.cpp:389-395 (`findRandomTheme`),
/// src/main.cpp:1003-1021 (the skin list)
pub fn discover(path: &Path) -> anyhow::Result<Discovered> {
    discover_until(path, &|| false)
}

/// The error of a [`discover_until`] that was stopped.
#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct Cancelled;

/// [`discover`], stopping with [`Cancelled`] as soon as `stop` says so
/// (checked before each directory it reads).
pub fn discover_until(path: &Path, stop: &dyn Fn() -> bool) -> anyhow::Result<Discovered> {
    let meta = std::fs::metadata(path).with_context(|| format!("{}", path.display()))?;
    anyhow::ensure!(meta.is_dir(), "{} is not a directory", path.display());
    let nested = path.join("share").join("gamedir");
    for gamedir in [path.to_path_buf(), nested] {
        if !gamedir.is_dir() {
            continue;
        }
        let found = match discover_in(&gamedir, stop) {
            Err(e) if e.is::<Cancelled>() => return Err(e),
            r => r.with_context(|| format!("{}", gamedir.display()))?,
        };
        if !found.plan.is_empty() {
            return Ok(found);
        }
    }
    anyhow::bail!(
        "{} is not an OpenLieroX game directory: no mods (directories with a script.lgs), \
         levels/*.lxl, data/themes/ or skins/ in it or in its share/gamedir",
        path.display()
    )
}

fn discover_in(gamedir: &Path, stop: &dyn Fn() -> bool) -> anyhow::Result<Discovered> {
    let check = || if stop() { Err(Cancelled) } else { Ok(()) };
    // As the import reads it: linked directories followed.
    let src = Bounded(DirFs::new(gamedir));
    check()?;
    let root = src.list(&VPath::root())?;
    let mut plan = ImportPlan::default();
    let mut notes = Vec::new();
    let dirs = |list: &[String]| -> Vec<String> {
        list.iter().filter_map(|e| e.strip_suffix('/')).map(str::to_string).collect()
    };
    // A directory's entry in `list` by name, ignoring case (exact first).
    let dir_ci = |list: &[String], want: &str| -> Option<String> {
        let names = dirs(list);
        names
            .iter()
            .find(|n| *n == want)
            .or_else(|| names.iter().find(|n| n.eq_ignore_ascii_case(want)))
            .cloned()
    };
    for dir in dirs(&root) {
        check()?;
        let script = VPath::new(&dir).and_then(|d| d.join("script.lgs"));
        if script.is_ok_and(|s| find_ci(&src, &s).is_some()) {
            plan.mods.push(dir);
        }
    }
    if let Some(levels) = dir_ci(&root, "levels") {
        check()?;
        let mut other = 0;
        for name in src.list(&VPath::new(&levels)?)? {
            if name.ends_with('/') {
                other += 1;
            } else if name.to_ascii_lowercase().ends_with(".lxl") {
                plan.levels.push(format!("{levels}/{name}"));
            } else if name.to_ascii_lowercase().ends_with(".lev") {
                other += 1;
            }
        }
        if other > 0 {
            notes.push(format!(
                "{other} levels in {levels}/ are not LieroX .lxl files (Liero .lev or directory levels): not imported"
            ));
        }
    }
    if let Some(data) = dir_ci(&root, "data") {
        check()?;
        let list = src.list(&VPath::new(&data)?)?;
        if let Some(themes) = dir_ci(&list, "themes") {
            let at = format!("{data}/{themes}");
            plan.themes.extend(dirs(&src.list(&VPath::new(&at)?)?).into_iter().map(|t| format!("{at}/{t}")));
        }
    }
    if let Some(skins) = dir_ci(&root, "skins") {
        check()?;
        // `jobs` lists `skins/` as named; another case would not be found.
        let any = src.list(&VPath::new(&skins)?)?.iter().any(|n| n.to_ascii_lowercase().ends_with(".png"));
        if any && skins == "skins" {
            plan.skins = true;
        } else if any {
            notes.push(format!("{skins}/ is not named skins/: its skins are not imported"));
        }
    }
    check()?;
    let core_found =
        OLX_CORE_FILES.iter().filter(|f| VPath::new(f).is_ok_and(|p| find_ci(&src, &p).is_some()));
    let core_found = core_found.count();
    if core_found == OLX_CORE_FILES.len() {
        plan.core = OLX_CORE_FILES.iter().map(|f| f.to_string()).collect();
    } else if core_found > 0 {
        notes.push(format!(
            "{core_found} of OLX's {} core files (data/gfx, data/sounds, colours.cfg): the game's own are kept",
            OLX_CORE_FILES.len()
        ));
    }
    Ok(Discovered { gamedir: gamedir.to_path_buf(), plan, notes })
}
