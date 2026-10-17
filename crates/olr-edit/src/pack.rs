//! A pack's authoring source, loaded and saved as a whole (plan 13d R2).
//!
//! [`load`] reads the manifest and the kind's files through `olr-format`,
//! and keeps every other file of the directory byte for byte ([`Pack::other`]:
//! foreign files such as an editor's backup, and a mod's assets). [`save`]
//! writes the complete new pack into a hidden sibling and swaps it in
//! (`olr_pack::install::replace_dir`), after checking it with
//! `olr_pack::validate`: the pack on disk is the old one or the new one,
//! never a mix, and files the source no longer has are gone.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use olr_data::manifest::{PackKind, PackManifest};
use olr_format::FormatError;
use olr_format::core_pack::{CoreSource, read_core_files, write_core_files};
use olr_format::levels::{LevelSource, read_level_files, write_level_files};
use olr_format::mods::{ModSource, read_mod_files, write_mod_files};
use olr_format::skins::{SkinSource, read_skin_files, write_skin_files};
use olr_format::themes::{ThemeSource, read_theme_files, write_theme_files};
use olr_pack::{Diagnostic, MANIFEST, MAX_FILE_BYTES, PackError, Severity};
use olr_vfs::{ContentSink, ContentSource, DirFs, MemFs, VPath, walk};

/// A pack's authoring source, by kind.
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

/// A pack as an editor holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub manifest: PackManifest,
    pub source: Source,
    /// Files the format does not read (foreign files, and a mod's
    /// gfx/sounds assets), by path, kept byte for byte.
    pub other: BTreeMap<VPath, Vec<u8>>,
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
    let manifest = olr_pack::read_manifest(&src)?;
    let source = Source::read(manifest.kind, &src)?;
    let mut format_files = MemFs::new();
    source.write(&mut format_files)?;
    let owned: BTreeSet<&VPath> = format_files.files().map(|(p, _)| p).collect();
    let mut other = BTreeMap::new();
    for path in walk(&src, &VPath::root())? {
        if path.as_str() != MANIFEST && !owned.contains(&path) {
            let bytes = src.read_max(&path, MAX_FILE_BYTES)?;
            other.insert(path, bytes);
        }
    }
    Ok(Pack { manifest, source, other })
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
/// (`pack.ron` and the kind's files), plus `other` as it is.
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
    Ok(fs.files().map(|(p, b)| (p.clone(), b.to_vec())).collect())
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
    let mut mem = MemFs::new();
    for (path, bytes) in &files {
        mem.write(path, bytes).map_err(PackError::from)?;
    }
    let errors: Vec<Diagnostic> =
        olr_pack::validate(&mem).into_iter().filter(|d| d.severity == Severity::Error).collect();
    if !errors.is_empty() {
        return Err(SaveError::Invalid(errors));
    }
    let dir = dir.canonicalize()?;
    Ok(olr_pack::install::replace_dir(&dir, |d| {
        let mut sink = DirFs::new(d);
        for (path, bytes) in &files {
            sink.write(path, bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        Ok(())
    })?)
}
