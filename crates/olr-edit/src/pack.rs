//! A pack's authoring source, loaded and saved as a whole (plan 13d R2).
//!
//! [`load`] reads the manifest and the kind's files through `olr-format`,
//! and keeps every other file of the directory byte for byte ([`Pack::other`]:
//! foreign files such as an editor's backup, and a mod's assets). [`save`]
//! writes the complete new pack into a hidden sibling and swaps it in
//! (`olr_pack::install::replace_dir`), after checking it with
//! `olr_pack::validate`: the pack on disk is the old one or the new one,
//! never a mix, and files the source no longer has are gone.
//!
//! A level's editable source (issue #74) is its placement,
//! `editor/placed.ron` ([`Pack::placed`]); while it is the ground alone
//! (every converted, new or plain level) [`Pack::composition`] holds the
//! ground's elements, folders and files, which the element operations and
//! the editor edit, and [`files`] writes back into `editor/asset.ron`. A
//! level of OLR 0.2 (`editor/composition.ron`, no placement) loads with
//! [`Pack::legacy_composition`] set and is refused by everything that
//! edits it ([`UNCONVERTED`]) until `olr_edit::placed::from_composition`
//! converts it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use olr_data::manifest::{PackKind, PackManifest};
use olr_format::FormatError;
use olr_format::assets::{AssetFile, AssetSource, asset_paths};
use olr_format::core_pack::{CoreSource, read_core_files, write_core_files};
use olr_format::levels::{
    AssetOrigin, COMPOSITION_FILE, CompositionSource, EDITOR_DIR, GROUND_KEY, LevelAsset, LevelSource,
    PlacedCopy, PlacedFile, PlacedSource, asset_dir, placed_paths, read_level_files, read_placed_files,
    write_level_files, write_placed_files,
};
use olr_format::mods::{ModSource, read_mod_files, write_mod_files};
use olr_format::skins::{SkinSource, read_skin_files, write_skin_files};
use olr_format::themes::{ThemeSource, read_theme_files, write_theme_files};
use olr_pack::{Diagnostic, MANIFEST, MAX_FILE_BYTES, PackError, Severity};
use olr_vfs::{ContentSink, ContentSource, DirFs, MemFs, VPath, walk};

/// A pack's authoring source, by kind.
// One per open pack: boxing the level would buy nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Mod(ModSource),
    Level(LevelSource),
    Skin(SkinSource),
    Theme(ThemeSource),
    Core(CoreSource),
}

impl Source {
    pub fn kind(&self) -> PackKind {
        match self {
            Source::Mod(_) => PackKind::Mod,
            Source::Level(_) => PackKind::Level,
            Source::Skin(_) => PackKind::Skin,
            Source::Theme(_) => PackKind::Theme,
            Source::Core(_) => PackKind::Core,
        }
    }

    /// Reads the files of `kind` from `src`. A level keeps its thumbnail.
    pub fn read(kind: PackKind, src: &dyn ContentSource) -> Result<Source, FormatError> {
        Ok(match kind {
            PackKind::Mod => Source::Mod(read_mod_files(src)?),
            PackKind::Level => Source::Level(read_level_files(src)?),
            PackKind::Skin => Source::Skin(read_skin_files(src)?),
            PackKind::Theme => Source::Theme(read_theme_files(src)?),
            PackKind::Core => Source::Core(read_core_files(src)?),
        })
    }

    /// The files the kind's reader reads: what its writer writes (no image
    /// is encoded), and a mod's `restrictions.ron`, read even when the writer
    /// would leave it out.
    pub fn reads(&self) -> Vec<String> {
        use olr_format::{core_pack, levels, mods, skins, themes};
        match self {
            Source::Mod(s) => {
                let mut paths = mods::file_paths(s);
                paths.push(mods::RESTRICTIONS_FILE.into());
                paths
            }
            Source::Level(s) => levels::file_paths(s),
            Source::Skin(s) => skins::file_paths(s),
            Source::Theme(s) => themes::file_paths(s),
            Source::Core(s) => core_pack::file_paths(s),
        }
    }

    /// Writes the kind's files (not `pack.ron`) with its canonical writer.
    pub fn write(&self, sink: &mut dyn ContentSink) -> Result<(), FormatError> {
        match self {
            Source::Mod(s) => write_mod_files(sink, s),
            Source::Level(s) => write_level_files(sink, s),
            Source::Skin(s) => write_skin_files(sink, s),
            Source::Theme(s) => write_theme_files(sink, s),
            Source::Core(s) => write_core_files(sink, s),
        }
    }
}

/// What refuses an edit of a level of OLR 0.2 (issue #74): its
/// `editor/composition.ron` is converted first, never flattened.
pub const UNCONVERTED: &str =
    "editor/composition.ron: a level from OLR 0.2: run olr-cli level convert <pack>";

/// What refuses an edit of the ground of a level placing more than its
/// ground (issue #74; #75 brings the commands for copies).
pub const NOT_THE_GROUND_ALONE: &str =
    "the level places assets besides its ground: only a ground placed alone is edited here (issue #75)";

/// The name a ground asset made for a level takes.
pub const GROUND_NAME: &str = "Ground";

/// A pack as an editor holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub manifest: PackManifest,
    pub source: Source,
    /// A level's ground as a composition (issue #74; plan 14a's
    /// composition): the ground asset's elements, folders and files, by
    /// pack path under `editor/`, while the placement is the ground alone
    /// ([`ground_alone`]), or the composition a plain level is given
    /// (`element::ensure_composed`, the editor) before it has a placement.
    /// `None` for a plain level, a level placing more than its ground, and
    /// the other kinds.
    pub composition: Option<CompositionSource>,
    /// A level's placement (`editor/placed.ron`, issue #74), its assets
    /// and scripts; `None` for a plain or unconverted level and the other
    /// kinds. [`files`] writes [`placement`].
    pub placed: Option<PlacedSource>,
    /// The level holds OLR 0.2's `editor/composition.ron` and no
    /// placement: it is not read (its files stay in `other`, byte for
    /// byte), and everything that edits or bakes the level refuses it
    /// ([`UNCONVERTED`]) until it is converted.
    pub legacy_composition: bool,
    /// Files the format does not read (foreign files, and a mod's
    /// gfx/sounds assets), by path, kept byte for byte.
    pub other: BTreeMap<VPath, Vec<u8>>,
    /// The placement's library copies as loaded, written back byte for
    /// byte while unedited.
    pub verbatim: Verbatim,
}

/// A level's library copies (every asset but the ground) as loaded: each
/// asset and its files' bytes by pack path. [`files`] writes an asset's
/// files as they were while it equals the one loaded, so a copy never
/// edited is never rewritten (its `asset.ron`'s own spacing kept).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Verbatim(BTreeMap<String, (AssetSource, BTreeMap<VPath, Vec<u8>>)>);

impl Pack {
    /// A pack of `source` and nothing else: no composition, placement or
    /// other files.
    pub fn new(manifest: PackManifest, source: Source) -> Pack {
        Pack {
            manifest,
            source,
            composition: None,
            placed: None,
            legacy_composition: false,
            other: BTreeMap::new(),
            verbatim: Verbatim::default(),
        }
    }

    /// Why the level's ground cannot be edited as a composition: an
    /// unconverted level of OLR 0.2 ([`UNCONVERTED`]), or a placement of
    /// more than the ground ([`NOT_THE_GROUND_ALONE`]). `None` for any
    /// other level (a plain one is composed when first edited).
    pub fn ground_refusal(&self) -> Option<&'static str> {
        if self.legacy_composition {
            Some(UNCONVERTED)
        } else if self.placed.is_some() && self.composition.is_none() {
            Some(NOT_THE_GROUND_ALONE)
        } else {
            None
        }
    }
}

/// Whether `placed` is its ground alone as `level` holds it (issue #74,
/// the editors' scope until #82): one asset, the ground (`Own`), of the
/// level's size and art scale with one state, and one static copy of it
/// at `(0, 0)`, unstretched, not distant, with no state, values or script.
pub fn ground_alone(placed: &PlacedSource, level: &LevelSource) -> bool {
    let ([a], [c]) = (placed.file.assets.as_slice(), placed.file.copies.as_slice()) else { return false };
    a.key == GROUND_KEY
        && a.origin == AssetOrigin::Own
        && c.asset == GROUND_KEY
        && c.at == (0, 0)
        && c.size.is_none_or(|s| s == level.size)
        && c.distant.is_none()
        && !c.live
        && c.state.is_none()
        && c.values.is_empty()
        && !c.script
        && placed.assets.get(GROUND_KEY).is_some_and(|g| {
            g.file.size == level.size && g.file.art_scale == level.art_scale && g.file.states.len() == 1
        })
}

/// A ground asset's composition: its elements and folders, its files
/// under `editor/`.
pub fn ground_composition(ground: &AssetSource) -> CompositionSource {
    olr_format::assets::as_composition(ground)
}

/// A level's placement of a ground alone (issue #74): the asset `ground`
/// (`Own`, [`GROUND_NAME`], the level's size and art scale) of `src`'s
/// elements, folders and files, the look's glow groups and materials, and
/// one locked copy `ground` of it at `(0, 0)`.
pub fn ground_placement(level: &LevelSource, src: &CompositionSource) -> PlacedSource {
    let mut ground = AssetSource::new(AssetFile::new(GROUND_NAME, level.size, Vec::new()), BTreeMap::new());
    set_ground(&mut ground, level, src, true);
    let copy = PlacedCopy { locked: true, ..PlacedCopy::new(GROUND_KEY, GROUND_KEY, (0, 0)) };
    PlacedSource {
        file: PlacedFile {
            assets: vec![LevelAsset { key: GROUND_KEY.into(), origin: AssetOrigin::Own }],
            copies: vec![copy],
            script: false,
        },
        assets: [(GROUND_KEY.to_string(), ground)].into(),
        scripts: BTreeMap::new(),
        level_script: None,
    }
}

/// `ground` given `src`'s elements, folders and files (paths without
/// `editor/`) and the level's size and art scale; with `look`, the
/// level's look's glow groups and materials too. Its other fields stay.
fn set_ground(ground: &mut AssetSource, level: &LevelSource, src: &CompositionSource, look: bool) {
    let f = &mut ground.file;
    f.size = level.size;
    f.art_scale = level.art_scale;
    f.elements = src.composition.elements.clone();
    f.folders = src.composition.folders.clone();
    let prefix = format!("{EDITOR_DIR}/");
    ground.files = src
        .files
        .iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(&prefix)?.to_string(), v.clone())))
        .collect();
    if look {
        f.glow_groups = level.look.as_ref().map(|l| l.glow_groups.clone()).unwrap_or_default();
        f.materials = level.look.as_ref().map(|l| l.materials.clone()).unwrap_or_default();
    }
}

/// The placement [`files`] writes for `pack` (issue #74): `placed` with
/// the ground's elements, folders, files, size and art scale from
/// `composition` (its other fields kept), and, while it is the ground
/// alone, the glow groups and materials of `level.ron`'s look (where the
/// look panel and `olr_edit::look` edit them); a level given a
/// composition and no placement yet is its [`ground_placement`]. `None`
/// for a plain or unconverted level and the other kinds.
pub fn placement(pack: &Pack) -> Option<PlacedSource> {
    placement_with(pack, true)
}

/// [`placement`]; with `look` false the ground keeps the glow groups and
/// materials it holds (`placed::stale` compares them with the look).
pub(crate) fn placement_with(pack: &Pack, look: bool) -> Option<PlacedSource> {
    let Source::Level(level) = &pack.source else { return pack.placed.clone() };
    let Some(src) = &pack.composition else { return pack.placed.clone() };
    let Some(mut placed) = pack.placed.clone() else { return Some(ground_placement(level, src)) };
    let mut ground = placed.assets.remove(GROUND_KEY).unwrap_or_else(|| {
        AssetSource::new(AssetFile::new(GROUND_NAME, level.size, Vec::new()), BTreeMap::new())
    });
    set_ground(&mut ground, level, src, false);
    placed.assets.insert(GROUND_KEY.into(), ground);
    if look
        && ground_alone(&placed, level)
        && let Some(g) = placed.assets.get_mut(GROUND_KEY)
    {
        set_ground(g, level, src, true);
    }
    Some(placed)
}

/// Why a save wrote nothing.
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error(transparent)]
    Pack(#[from] PackError),
    /// The saved pack would not validate; its errors (never warnings).
    #[error("the result is not a valid pack: {}", list(.0))]
    Invalid(Vec<Diagnostic>),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

fn list(diagnostics: &[Diagnostic]) -> String {
    diagnostics.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("; ")
}

/// Reads the pack at `dir`: its manifest, its kind's files, and every other
/// file as it is.
pub fn load(dir: &Path) -> Result<Pack, PackError> {
    if let Some(name) = unaddressable(dir, dir)? {
        return Err(PackError::Invalid {
            file: name,
            message: "a name a pack path cannot hold: a save would lose the file; rename or remove it".into(),
        });
    }
    let src = DirFs::new(dir);
    bounded(&src)?;
    let manifest = olr_pack::read_manifest(&src)?;
    let source = Source::read(manifest.kind, &src)?;
    let (placed, legacy_composition) = match &source {
        Source::Level(_) => {
            let placed = read_placed_files(&src)?;
            let legacy = placed.is_none() && src.exists(&VPath::new(COMPOSITION_FILE)?);
            (placed, legacy)
        }
        _ => (None, false),
    };
    let composition = match (&source, &placed) {
        (Source::Level(level), Some(p)) if ground_alone(p, level) => {
            p.assets.get(GROUND_KEY).map(ground_composition)
        }
        _ => None,
    };
    let mut reads = source.reads();
    reads.extend(placed.iter().flat_map(placed_paths));
    let owned: BTreeSet<&str> = reads.iter().map(String::as_str).collect();
    let mut other = BTreeMap::new();
    for path in walk(&src, &VPath::root())? {
        if path.as_str() != MANIFEST && !owned.contains(path.as_str()) {
            let bytes = src.read_max(&path, MAX_FILE_BYTES)?;
            other.insert(path, bytes);
        }
    }
    let mut verbatim = BTreeMap::new();
    for (key, asset) in placed.iter().flat_map(|p| &p.assets).filter(|(k, _)| k.as_str() != GROUND_KEY) {
        let dir = asset_dir(key);
        let mut raw = BTreeMap::new();
        for f in asset_paths(asset) {
            let path = VPath::new(&format!("{dir}/{f}"))?;
            let bytes = src.read_max(&path, MAX_FILE_BYTES)?;
            raw.insert(path, bytes);
        }
        verbatim.insert(key.clone(), (asset.clone(), raw));
    }
    Ok(Pack {
        manifest,
        source,
        composition,
        placed,
        legacy_composition,
        other,
        verbatim: Verbatim(verbatim),
    })
}

/// A pack is bounded as a whole as its archive would be: at most
/// `MAX_ARCHIVE_FILES` files and `MAX_UNPACKED` bytes, from the file
/// lengths alone, before anything is read.
fn bounded(src: &DirFs) -> Result<(), PackError> {
    use olr_pack::archive::{MAX_ARCHIVE_FILES, MAX_UNPACKED};
    let paths = walk(src, &VPath::root())?;
    let too_large = |message: String| PackError::Invalid { file: String::new(), message };
    if paths.len() > MAX_ARCHIVE_FILES as usize {
        return Err(too_large(format!("{} files, above {MAX_ARCHIVE_FILES}", paths.len())));
    }
    let mut bytes = 0u64;
    for p in &paths {
        bytes = bytes.saturating_add(src.file_len(p)?);
    }
    if bytes > MAX_UNPACKED {
        return Err(too_large(format!("{bytes} bytes, above {MAX_UNPACKED}")));
    }
    Ok(())
}

/// The first entry under `dir` (by path, relative to `root`) whose name
/// `DirFs` cannot address (not UTF-8, or refused by `VPath`): `walk` skips
/// such a name, so the whole-pack save would drop it.
fn unaddressable(root: &Path, dir: &Path) -> Result<Option<String>, PackError> {
    let io =
        |e: std::io::Error| PackError::Invalid { file: dir.display().to_string(), message: e.to_string() };
    let mut entries: Vec<_> = std::fs::read_dir(dir).map_err(io)?.collect::<Result<_, _>>().map_err(io)?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let rel = e.path().strip_prefix(root).unwrap_or(&e.path()).to_string_lossy().into_owned();
        if e.file_name().to_str().is_none_or(|n| VPath::new(n).is_err()) {
            return Ok(Some(rel));
        }
        // As `DirFs::list`: a symbolic link is not followed.
        if e.file_type().map_err(io)?.is_dir()
            && let Some(found) = unaddressable(root, &e.path())?
        {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// Every file of `pack` as its canonical writer produces it
/// (`pack.ron`, the kind's files and a level's [`placement`], an
/// unedited library copy as loaded), plus `other` as it is.
pub fn files(pack: &Pack) -> Result<BTreeMap<VPath, Vec<u8>>, PackError> {
    if pack.manifest.kind != pack.source.kind() {
        return Err(PackError::Invalid {
            file: MANIFEST.into(),
            message: format!(
                "manifest kind {:?} is not the source's {:?}",
                pack.manifest.kind,
                pack.source.kind()
            ),
        });
    }
    let mut fs = MemFs::new();
    // The format's files win over a foreign file of the same path.
    for (path, bytes) in &pack.other {
        fs.write(path, bytes)?;
    }
    olr_pack::write_manifest(&mut fs, &pack.manifest)?;
    pack.source.write(&mut fs)?;
    if (pack.composition.is_some() || pack.placed.is_some()) && pack.source.kind() != PackKind::Level {
        return Err(PackError::Invalid {
            file: olr_format::levels::PLACED_FILE.into(),
            message: "only a level pack has a composition or a placement".into(),
        });
    }
    if let Some(p) = placement(pack) {
        write_placed_files(&mut fs, &p)?;
        // A library copy as loaded is written as it was read.
        for (key, (asset, raw)) in &pack.verbatim.0 {
            if p.assets.get(key) == Some(asset) && p.file.assets.iter().any(|a| a.key == *key) {
                for (path, bytes) in raw {
                    fs.write(path, bytes)?;
                }
            }
        }
    }
    Ok(fs.files().map(|(p, b)| (p.clone(), b.to_vec())).collect())
}

/// Whether a pack of `files` would check: its `olr_pack::validate` errors
/// (never warnings) as `SaveError::Invalid`.
pub fn check(files: &BTreeMap<VPath, Vec<u8>>) -> Result<(), SaveError> {
    let mut mem = MemFs::new();
    for (path, bytes) in files {
        mem.write(path, bytes).map_err(PackError::from)?;
    }
    let errors: Vec<Diagnostic> =
        olr_pack::validate(&mem).into_iter().filter(|d| d.severity == Severity::Error).collect();
    if errors.is_empty() { Ok(()) } else { Err(SaveError::Invalid(errors)) }
}

/// Mod pack `pack` as the game reads a saved one (issue #18, the mod
/// editor's Play): `files(pack)` checked as [`save`] checks them (an
/// error is `SaveError::Invalid`), then read by `olr_pack::read_mod` from
/// memory. Nothing is written.
pub fn mod_pack(pack: &Pack) -> Result<olr_pack::ModPack, SaveError> {
    let files = files(pack)?;
    check(&files)?;
    let mut mem = MemFs::new();
    for (path, bytes) in &files {
        mem.write(path, bytes).map_err(PackError::from)?;
    }
    Ok(olr_pack::read_mod(&mem)?)
}

/// `files(pack)` written through `replace_dir`; the result is read
/// back with `olr_pack::validate` before the swap, and an error there
/// aborts the save (the old pack stays). Returns `replace_dir`'s warnings.
///
/// The directory is resolved first, so `.` or `pack/sub/..` names the pack
/// itself, and a symbolic link to a pack is saved where it points (the
/// link stays).
pub fn save(dir: &Path, pack: &Pack) -> Result<Vec<String>, SaveError> {
    let files = files(pack)?;
    check(&files)?;
    let dir = dir.canonicalize()?;
    Ok(olr_pack::install::replace_dir(&dir, |d| {
        let mut sink = DirFs::new(d);
        for (path, bytes) in &files {
            sink.write(path, bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        Ok(())
    })?)
}
