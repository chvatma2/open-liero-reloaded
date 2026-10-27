//! Loads content packs (`olr-pack`) into ready-to-run inputs: a [`SimMod`]
//! with its decoded images and sound bytes, levels, themes, skins, OLX's
//! core pack, and whole sandbox [`World`]s.
//!
//! **Never panics on content** (spec §19, Review Focus 3): a missing or odd
//! asset gets a placeholder ([`PLACEHOLDER`]-coloured images of the size the
//! renderer expects, silence for sounds) and a warning. Warnings are
//! collected, never printed: [`Content::warnings`] for the directory and the
//! core pack, and a `warnings` list on each loaded item, so the caller logs
//! each once. A missing pack or an unreadable definition is an error.
//!
//! The oracle loads mods strictly ([`Content::sim_mod`]): OLX's client
//! knows every projectile image's frame count, so a missing one is an error.

mod cache;
mod catalog;
mod color;
mod core_assets;
mod live;
mod locate;
mod maps;
mod matches;
mod mods;
mod placeholder;
mod replay;
mod sandbox;
mod skins;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Context as _;
use olr_data::manifest::PackKind;
use olr_vfs::DirFs;

pub use cache::{
    CACHE_BYTES, DownloadCache, Eviction, InstallError, Keeper, MAX_PARTIALS, MAX_SKIN_ARCHIVE, Partial,
    PendingInstall, SKIN_LIMITS, archive_limits,
};
pub use catalog::{Catalog, LevelInfo, MAX_THUMB_BYTES, MAX_THUMB_SIDE, ModInfo, PackSource, ThumbRef};
pub use color::{apply_edits, color_edit, lost_area};
pub use core_assets::{CORE_PACK_ID, CORE_SPRITES, CorePack};
pub use live::{LoadedLive, live_level};
pub use locate::{
    ASSETS_DIR, CONTENT_DIR, MATERIALS_DIR, RELOADED_DIR, assets_dir, bundled_roots, default_content_dir,
    materials_dir,
};
pub use maps::{
    DEFAULT_SKIN, LoadedLevel, LoadedSkin, LoadedTheme, MAX_ART_SCALE, MAX_COLOR_MAP_BYTES, NO_SKIN_HASH,
};
pub use matches::{
    BuiltMatch, CPU_COLOURS, MatchAssets, MatchOptions, Player, WeaponChoice, build_match,
    build_match_spawning, default_looks,
};
pub use mods::{LoadedMod, check_frames, glow_overlays};
pub use olr_pack::sounds::{MAX_SOUND_BYTES, MAX_SOUND_SECONDS, check_sound};
pub use placeholder::{PLACEHOLDER, placeholder_image};
pub use replay::replay_assets_from;
pub use sandbox::{SANDBOX_START, SandboxSetup, SpawnSpec, match_setup, new_world_from};
pub use skins::{
    FULL_LIGHT_GLOW, GLOW_LEVELS, SHEET_FRAMES, SkinFrameLight, SkinInfo, SkinLights, SkinSource, glow_level,
    resolve_id, skin_source, skin_sprite,
};

#[doc(no_inline)]
pub use olr_data::loadouts::{DEFAULT_LOADOUT, MatchLoadout};
#[doc(no_inline)]
pub use olr_data::restrictions::{DEFAULT_PRESET, WeaponRules};
#[doc(no_inline)]
pub use olr_sim::{SimMod, World};

/// The pack id `olr-cli import` gives OLX mod directory `dir` (such as `MW 1.0`).
pub fn olx_mod_id(dir: &str) -> String {
    format!("olx-{}", olr_data::slug::slug(dir))
}

/// The pack id `olr-cli import` gives OLX level file `file` (such as
/// `CastleStrike.lxl`, with or without its `levels/` directory).
pub fn olx_level_id(file: &str) -> String {
    let name = file.rsplit('/').next().unwrap_or(file);
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    format!("olx-level-{}", olr_data::slug::slug(stem))
}

/// A content directory (such as `content/packs`): its packs by id, and the
/// core pack, loaded once.
pub struct Content {
    packs: BTreeMap<String, (PackKind, PathBuf)>,
    /// Each pack's name (its manifest's), by id.
    names: BTreeMap<String, String>,
    /// Each pack's origin (its manifest's), by id.
    origins: BTreeMap<String, Option<olr_data::manifest::Origin>>,
    core: CorePack,
    warnings: Vec<String>,
    /// Content hashes by pack directory ([`Content::pack_hash`]). Content is
    /// opened per session, so a pack changed on disk meanwhile is hashed
    /// stale until the next open.
    hashes: Mutex<BTreeMap<PathBuf, String>>,
}

impl Content {
    /// Lists the packs in `root`'s subdirectories (each with a `pack.ron`;
    /// a name starting with `.` is skipped: an import's pack being
    /// written) and loads the core pack [`CORE_PACK_ID`]. A pack this
    /// build cannot read, a second pack with the same id or a missing core
    /// pack is a warning; only an unreadable `root` is an error.
    pub fn open(root: &Path) -> anyhow::Result<Content> {
        std::fs::read_dir(root).with_context(|| format!("content directory {}", root.display()))?;
        Content::open_layers(&[root])
    }

    /// The bundled content of content directory `content`: its packs under
    /// OLR's own ([`bundled_roots`]: `content/reloaded` over
    /// `content/packs`), what the programs open by default. An unreadable
    /// `content` is an error, as in [`Content::open`]; a missing
    /// `reloaded` directory is skipped.
    pub fn open_bundled(content: &Path) -> anyhow::Result<Content> {
        std::fs::read_dir(content).with_context(|| format!("content directory {}", content.display()))?;
        let roots = bundled_roots(content);
        Content::open_layers(&roots.iter().map(PathBuf::as_path).collect::<Vec<_>>())
    }

    /// The packs of several content directories, layered (spec §9.4):
    /// `roots` from the highest priority down, such as the user data's
    /// `packs/` (written by `olr-cli import`) before the bundled content. A pack id
    /// found in a higher layer hides the same id below it, whatever its
    /// kind, so an imported pack replaces the bundled one (the core pack
    /// too); a pack of a higher layer that cannot be read hides nothing
    /// (a warning). Within one layer a second pack with an id is a warning,
    /// as in [`Content::open`]. A root that does not exist is skipped (no
    /// imports yet), one that cannot be read is a warning; an error only
    /// when no root can be read.
    pub fn open_layers(roots: &[&Path]) -> anyhow::Result<Content> {
        let mut packs: BTreeMap<String, (PackKind, PathBuf)> = BTreeMap::new();
        let mut layer_of: BTreeMap<String, usize> = BTreeMap::new();
        let mut names = BTreeMap::new();
        let mut origins = BTreeMap::new();
        let mut warnings = Vec::new();
        let mut unreadable = Vec::new();
        let mut read_any = false;
        for (layer, root) in roots.iter().enumerate() {
            let entries = match std::fs::read_dir(root) {
                Ok(e) => e,
                Err(e) => {
                    let why = format!("content directory {}: {e}", root.display());
                    if e.kind() != std::io::ErrorKind::NotFound {
                        warnings.push(format!("{why}: skipped"));
                    }
                    unreadable.push(why);
                    continue;
                }
            };
            read_any = true;
            let (mut dirs, mut aside): (Vec<PathBuf>, Vec<PathBuf>) = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.join(olr_pack::MANIFEST).is_file())
                .filter(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                    !name.starts_with('.') || name.starts_with(olr_pack::install::OLD_PREFIX)
                })
                .partition(|p| !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')));
            dirs.sort();
            // The newest of an id's, as the next import puts back.
            olr_pack::install::newest_aside_first(&mut aside);
            for dir in dirs {
                match olr_pack::read_manifest(&DirFs::new(&dir)) {
                    Ok(m) => match (packs.get(&m.id), layer_of.get(&m.id)) {
                        (Some((_, first)), Some(&l)) if l == layer => warnings.push(format!(
                            "{}: pack {} is also {}, skipped",
                            dir.display(),
                            m.id,
                            first.display()
                        )),
                        // Hidden by a higher layer's pack.
                        (Some(_), _) => {}
                        _ => {
                            names.insert(m.id.clone(), m.name);
                            origins.insert(m.id.clone(), m.origin);
                            layer_of.insert(m.id.clone(), layer);
                            packs.insert(m.id, (m.kind, dir));
                        }
                    },
                    Err(e) => warnings.push(format!("{}: {e}, skipped", dir.display())),
                }
            }
            // A pack an import moved aside and was killed before its new
            // one came (`import`'s `.replaced-<id>`): still the pack, until
            // the next import puts it back.
            for dir in aside {
                if let Ok(m) = olr_pack::read_manifest(&DirFs::new(&dir))
                    && !packs.contains_key(&m.id)
                {
                    names.insert(m.id.clone(), m.name);
                    origins.insert(m.id.clone(), m.origin);
                    layer_of.insert(m.id.clone(), layer);
                    packs.insert(m.id, (m.kind, dir));
                }
            }
        }
        if !read_any {
            anyhow::bail!(
                "{}",
                if unreadable.is_empty() { "no content directory".into() } else { unreadable.join("; ") }
            );
        }
        let core = match packs.get(CORE_PACK_ID) {
            Some((PackKind::Core, dir)) => match olr_pack::read_core_tolerant(&DirFs::new(dir)) {
                Ok((pack, skipped)) => {
                    for e in skipped {
                        warnings.push(format!("core pack {CORE_PACK_ID}: {e}: left out"));
                    }
                    CorePack::from_pack(pack, &mut warnings)
                }
                Err(e) => {
                    warnings.push(format!("core pack {CORE_PACK_ID}: {e}: placeholders and silence instead"));
                    CorePack::empty()
                }
            },
            _ => {
                warnings.push(format!("no core pack {CORE_PACK_ID}: placeholders and silence instead"));
                CorePack::empty()
            }
        };
        Ok(Content { packs, names, origins, core, warnings, hashes: Mutex::default() })
    }

    /// Problems found when opening: unreadable packs, duplicate ids, and the
    /// core pack's missing or odd assets.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// OLX's shared game assets (spec §19 E9).
    pub fn core(&self) -> &CorePack {
        &self.core
    }

    /// The ids of the packs of `kind`, sorted.
    pub fn ids(&self, kind: PackKind) -> Vec<String> {
        self.packs.iter().filter(|(_, (k, _))| *k == kind).map(|(id, _)| id.clone()).collect()
    }

    /// Pack `id`'s name (its manifest's), if there is such a pack.
    pub fn pack_name(&self, id: &str) -> Option<&str> {
        self.names.get(id).map(String::as_str)
    }

    /// Pack `id`'s origin (its manifest's), if there is such a pack and it
    /// records one.
    pub fn pack_origin(&self, id: &str) -> Option<&olr_data::manifest::Origin> {
        self.origins.get(id)?.as_ref()
    }

    /// Pack `id`'s kind, if there is such a pack.
    pub fn pack_kind(&self, id: &str) -> Option<PackKind> {
        self.packs.get(id).map(|(k, _)| *k)
    }

    /// Pack `id`'s directory, if there is such a pack.
    pub fn pack_dir(&self, id: &str) -> Option<&Path> {
        self.packs.get(id).map(|(_, dir)| dir.as_path())
    }

    /// The pack `id`, which must be of `kind`.
    fn pack(&self, id: &str, kind: PackKind) -> anyhow::Result<DirFs> {
        match self.packs.get(id) {
            Some((k, dir)) if *k == kind => Ok(DirFs::new(dir)),
            Some((k, _)) => anyhow::bail!("pack {id} is a {k:?} pack, not {kind:?}"),
            None => anyhow::bail!("no {kind:?} pack {id}"),
        }
    }
}

impl Content {
    /// A copy whose packs `packs` (id → pack directory) hide this content's
    /// packs of the same id, as a new top layer. Each directory's manifest
    /// must be pack `id`, of the same kind as this content's pack `id` if
    /// it has one (a skin never stands in for a level); the core pack
    /// cannot be overlaid. Nothing on disk
    /// changes, and `self` stays as it is.
    pub fn with_packs(&self, packs: &[(String, PathBuf)]) -> anyhow::Result<Content> {
        let mut out = Content {
            packs: self.packs.clone(),
            names: self.names.clone(),
            origins: self.origins.clone(),
            core: self.core.clone(),
            warnings: self.warnings.clone(),
            hashes: Mutex::new(self.hashes.lock().map(|m| m.clone()).unwrap_or_default()),
        };
        for (id, dir) in packs {
            let m = olr_pack::read_manifest(&DirFs::new(dir))
                .with_context(|| format!("pack {id} in {}", dir.display()))?;
            anyhow::ensure!(m.id == *id, "{} is pack {}, not {id}", dir.display(), m.id);
            anyhow::ensure!(m.kind != PackKind::Core, "the core pack cannot be overlaid");
            if let Some(&(kind, _)) = self.packs.get(id) {
                anyhow::ensure!(kind == m.kind, "pack {id} is a {kind:?} here, not a {:?}", m.kind);
            }
            out.names.insert(m.id.clone(), m.name);
            out.origins.insert(m.id.clone(), m.origin);
            out.packs.insert(m.id, (m.kind, dir.clone()));
        }
        Ok(out)
    }

    /// Whether this content has pack `id` with content hash `hash`
    /// ([`Content::pack_hash`], memoised per pack directory).
    pub fn has(&self, id: &str, hash: &str) -> bool {
        self.packs.get(id).is_some_and(|(kind, _)| self.pack_hash(id, *kind).is_ok_and(|h| h == hash))
    }

    /// Pack `id`'s content hash if it is known already ([`Content::pack_hash`]
    /// memoised it, or [`Content::remember_hash`] was told it): never
    /// hashes, so a caller that must not block can ask.
    pub fn known_hash(&self, id: &str) -> Option<String> {
        let dir = self.pack_dir(id)?;
        self.hashes.lock().ok()?.get(dir).cloned()
    }

    /// Records `hash` as the content hash of pack `id`, computed elsewhere
    /// (such as on a worker thread, `olr_pack::content_hash` of `dir`):
    /// ignored unless `dir` is pack `id`'s directory. [`Content::pack_hash`]
    /// and [`Content::has`] then use it.
    pub fn remember_hash(&self, id: &str, dir: &Path, hash: &str) {
        if self.pack_dir(id) == Some(dir)
            && let Ok(mut m) = self.hashes.lock()
        {
            m.insert(dir.to_path_buf(), hash.to_string());
        }
    }
}
