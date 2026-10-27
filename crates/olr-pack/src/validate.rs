//! Pack validation with file and position information (spec §9.1).

use std::collections::BTreeSet;
use std::fmt;

use olr_data::manifest::PackKind;
use olr_format::FormatError;
use olr_vfs::{ContentSource, VPath, walk};

use crate::level_pack::LEVEL_FILE;
use crate::mod_pack::is_definition;
use crate::{
    LevelPack, MANIFEST, ModPack, PackError, THUMB_FILE, read_core, read_level, read_manifest, read_mod,
    read_skin, read_theme,
};

/// A mod's asset directories: a file elsewhere that nothing names is not
/// read by the format (a file in them is an unused asset).
const MOD_ASSET_DIRS: [&str; 2] = ["gfx/", "sfx/"];

/// The warning for a file the pack's format does not read.
const NOT_READ: &str = "not read by the format";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub col: Option<usize>,
    pub message: String,
}

impl Diagnostic {
    fn new(severity: Severity, file: impl Into<String>, message: impl Into<String>) -> Self {
        Self { severity, file: Some(file.into()), line: None, col: None, message: message.into() }
    }

    fn from_error(e: &PackError) -> Self {
        match e {
            PackError::Ron { file, line, col, message } => Self {
                severity: Severity::Error,
                file: Some(file.clone()),
                line: Some(*line),
                col: Some(*col),
                message: message.clone(),
            },
            PackError::Image { file, message } | PackError::Invalid { file, message } => {
                Self::new(Severity::Error, file.clone(), message.clone())
            }
            PackError::Format(FormatError::Parse { file, line, col, message }) => Self {
                severity: Severity::Error,
                file: Some(file.clone()),
                line: Some(*line),
                col: Some(*col),
                message: message.clone(),
            },
            // The field path stays in the message.
            PackError::Format(FormatError::Invalid { file, path, message }) => Self::new(
                Severity::Error,
                file.clone(),
                if path.is_empty() { message.clone() } else { format!("{path}: {message}") },
            ),
            PackError::Format(FormatError::Io { file, message }) => {
                Self::new(Severity::Error, file.clone(), message.clone())
            }
            PackError::Vfs(v) => {
                Self { severity: Severity::Error, file: None, line: None, col: None, message: v.to_string() }
            }
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{sev}: ")?;
        if let Some(file) = &self.file {
            write!(f, "{file}")?;
            if let (Some(l), Some(c)) = (self.line, self.col) {
                write!(f, ":{l}:{c}")?;
            }
            write!(f, ": ")?;
        }
        f.write_str(&self.message)
    }
}

/// Checks a pack and returns every problem found, errors first. An empty list
/// means the pack is valid.
pub fn validate(src: &dyn ContentSource) -> Vec<Diagnostic> {
    let manifest = match read_manifest(src) {
        Ok(m) => m,
        Err(e) => return vec![Diagnostic::from_error(&e)],
    };
    let mut out = Vec::new();
    let id_ok = !manifest.id.is_empty()
        && manifest.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !id_ok {
        out.push(Diagnostic::new(
            Severity::Error,
            "pack.ron",
            format!("id {:?} must be a lowercase slug", manifest.id),
        ));
    }
    let result = match manifest.kind {
        PackKind::Mod => read_mod(src).map(|p| out.extend(check_mod(&p))),
        PackKind::Level => read_level(src).map(|p| {
            // `read_level` plays a broken look without it; a check reports it.
            if let Err(e) = olr_format::levels::read_level_files_without_thumb(src) {
                out.push(Diagnostic::from_error(&PackError::Format(e)));
            }
            // Presentation only: the level plays without it.
            if let Err(e) = crate::read_level_thumb(src) {
                out.push(Diagnostic::new(Severity::Warning, THUMB_FILE, format!("{e}: not shown")));
            }
            out.extend(check_level(&p));
            out.extend(check_placement(src, &p));
        }),
        PackKind::Skin => read_skin(src).map(|_| ()),
        PackKind::Theme => read_theme(src).map(|_| ()),
        PackKind::Core => read_core(src).map(|_| ()),
    };
    match result {
        Ok(()) => out.extend(unread(src, manifest.kind)),
        Err(e) => out.push(Diagnostic::from_error(&e)),
    }
    out.sort_by_key(|d| d.severity);
    out
}

/// A warning for each file of a pack that reads cleanly which its format
/// does not read, by path: the format's files are those its `olr-format`
/// writer produces from the source read, plus `pack.ron` and a level's
/// `thumb.png`. A mod's other files are assets, which `check_mod` covers.
fn unread(src: &dyn ContentSource, kind: PackKind) -> Vec<Diagnostic> {
    use olr_format::{core_pack, levels, mods, skins, themes};
    let owned = match kind {
        // The reader reads `restrictions.ron` even when the writer would
        // leave it out (no presets).
        PackKind::Mod => mods::read_mod_files(src).map(|s| {
            let mut paths = mods::file_paths(&s);
            paths.push(mods::RESTRICTIONS_FILE.into());
            paths
        }),
        PackKind::Level => levels::read_level_files_without_thumb(src).map(|s| {
            let mut paths = levels::file_paths(&s);
            match levels::read_placed_files(src) {
                Ok(Some(p)) => paths.extend(levels::placed_paths(&p)),
                // A placement that does not read is one error
                // (`check_placement`), not a warning for every `editor/`
                // file besides.
                Err(_) => {
                    if let Ok(all) = walk(src, &VPath::root()) {
                        paths.extend(
                            all.iter()
                                .map(VPath::as_str)
                                .filter(|p| crate::is_authoring(p))
                                .map(String::from),
                        );
                    }
                }
                // An unconverted level of OLR 0.2 (issue #74): the files
                // `composition.ron` names are its (one warning, on it).
                _ => {
                    if let Ok(Some(composition)) = levels::read_composition(src) {
                        let c = levels::CompositionSource { composition, files: Default::default() };
                        paths.extend(levels::composition_paths(&c));
                        paths.push(levels::COMPOSITION_FILE.into());
                    }
                }
            }
            paths
        }),
        PackKind::Skin => skins::read_skin_files(src).map(|s| skins::file_paths(&s)),
        PackKind::Theme => themes::read_theme_files(src).map(|s| themes::file_paths(&s)),
        PackKind::Core => core_pack::read_core_files(src).map(|s| core_pack::file_paths(&s)),
    };
    // The reader above already accepted the pack; nothing to add if not.
    let (Ok(owned), Ok(paths)) = (owned, walk(src, &VPath::root())) else { return Vec::new() };
    let owned: BTreeSet<&str> = owned.iter().map(String::as_str).collect();
    paths
        .iter()
        .map(VPath::as_str)
        .filter(|p| *p != MANIFEST && !owned.contains(p))
        .filter(|p| !(kind == PackKind::Level && *p == THUMB_FILE))
        .filter(|p| !(kind == PackKind::Mod && !is_definition(p)))
        .filter(|p| !(kind == PackKind::Level && *p == olr_format::levels::COMPOSITION_FILE))
        .map(|p| {
            let script = p.strip_prefix(levels::SCRIPTS_DIR).is_some_and(|f| f.ends_with(".rhai"));
            let message = if kind == PackKind::Level && script { STRAY_SCRIPT_WARNING } else { NOT_READ };
            Diagnostic::new(Severity::Warning, p, message)
        })
        .collect()
}

/// A level of OLR 0.2 without a placement: the baked level plays, the
/// editors refuse it until it is converted (issue #74).
pub const UNCONVERTED_WARNING: &str = "a level from OLR 0.2: the editors refuse it until olr-cli level convert converts it; the baked level plays";
/// A composition left beside a placement (issue #74).
pub const LEFTOVER_WARNING: &str = "a level from OLR 0.2 left beside editor/placed.ron: not read; remove it";
/// A copy's script no copy lists (issue #74).
pub const STRAY_SCRIPT_WARNING: &str = "a script no copy lists: not read";

/// The level's placement (`editor/placed.ron`, issue #74), if it has one:
/// an error when it does not read or does not fit the level
/// (`check_placed`, metadata only: the cells are the bake's). OLR 0.2's
/// `editor/composition.ron` is a warning: unconverted without a
/// placement, a leftover beside one.
fn check_placement(src: &dyn ContentSource, pack: &LevelPack) -> Vec<Diagnostic> {
    use olr_format::levels::{COMPOSITION_FILE, PLACED_FILE};
    let def = &pack.level.def;
    let exists = |f: &str| VPath::new(f).is_ok_and(|p| src.exists(&p));
    let mut out = Vec::new();
    match (exists(COMPOSITION_FILE), exists(PLACED_FILE)) {
        (true, false) => out.push(Diagnostic::new(Severity::Warning, COMPOSITION_FILE, UNCONVERTED_WARNING)),
        (true, true) => out.push(Diagnostic::new(Severity::Warning, COMPOSITION_FILE, LEFTOVER_WARNING)),
        _ => {}
    }
    let checked = olr_format::levels::read_placed_files(src).and_then(|p| match p {
        Some(p) => olr_format::levels::check_placed(&p, (def.width, def.height), def.art_scale),
        None => Ok(()),
    });
    out.extend(checked.err().map(|e| Diagnostic::from_error(&PackError::Format(e))));
    out
}

/// Objects outside the level (warnings): the runtime places worms itself.
fn check_level(pack: &LevelPack) -> Vec<Diagnostic> {
    let def = &pack.level.def;
    let (w, h) = (def.width, def.height);
    let inside = |v: i32, side: u32| u32::try_from(v).is_ok_and(|v| v < side);
    def.spawns
        .iter()
        .enumerate()
        .filter(|(_, s)| !(inside(s.x, w) && inside(s.y, h)))
        .map(|(i, s)| {
            Diagnostic::new(
                Severity::Warning,
                LEVEL_FILE,
                format!("objects[{i}]: spawn at ({}, {}) is outside the level ({w}×{h})", s.x, s.y),
            )
        })
        .collect()
}

/// What the readers cannot see, on the compiled mod: assets the definitions
/// name but the pack lacks (error), assets nothing names (warning; outside
/// [`MOD_ASSET_DIRS`] a file not read by the format), and shared
/// projectiles (`projectiles/<name>.ron`) nothing spawns (warning).
/// `read_mod` has already refused duplicate weapons, unknown projectile
/// names and spawn cycles.
fn check_mod(pack: &ModPack) -> Vec<Diagnostic> {
    use Severity::*;
    let def = &pack.def;
    let mut out = Vec::new();

    let mut referenced: BTreeSet<&str> = BTreeSet::new();
    let mut used_assets: BTreeSet<&str> = BTreeSet::new();
    for w in &def.weapons {
        referenced.extend(w.projectile());
        used_assets.extend(w.sound());
    }
    for p in def.projectiles.values() {
        referenced.extend(p.children());
        used_assets.extend(p.assets());
    }
    for asset in &used_assets {
        let present = VPath::new(asset).map(|p| pack.assets.contains_key(&p)).unwrap_or(false);
        if !present {
            out.push(Diagnostic::new(Error, "mod.ron", format!("missing asset {asset:?}")));
        }
    }
    for name in def.projectiles.keys() {
        if !referenced.contains(name.as_str()) {
            out.push(Diagnostic::new(Warning, format!("projectiles/{name}.ron"), "projectile is never used"));
        }
    }
    for (path, bytes) in &pack.assets {
        // The mod editor's file (issue #17) is read by the editor alone.
        if path.as_str() == olr_format::mods::MOD_EDITOR_FILE {
            let text = String::from_utf8_lossy(bytes);
            if let Err(e) = olr_format::ron_io::from_canonical::<olr_format::mods::ModEditorFile>(
                olr_format::mods::MOD_EDITOR_FILE,
                &text,
            ) {
                out.push(Diagnostic::new(Warning, path.as_str(), format!("{e}: the mod editor ignores it")));
            }
            continue;
        }
        // Issue #19: a glow mask goes with its image, of its size.
        if let Some(image) = olr_format::mods::image_of_mask(path.as_str()) {
            let found = VPath::new(&image).ok().and_then(|p| pack.assets.get(&p));
            let problem = match found.map(|img| crate::image_io::png_size(img)) {
                None => Some(format!("a glow mask without its image {image:?}")),
                Some(None) => Some(format!("its image {image:?} is not a PNG")),
                Some(Some(size)) => olr_format::mods::decode_glow_mask(bytes, size)
                    .err()
                    .map(|e| format!("glow mask of {image:?}: {e}")),
            };
            if let Some(message) = problem {
                out.push(Diagnostic::new(Error, path.as_str(), message));
            } else if !used_assets.contains(image.as_str()) {
                out.push(Diagnostic::new(Warning, path.as_str(), "asset is never used"));
            }
            continue;
        }
        if !used_assets.contains(path.as_str()) {
            let asset = MOD_ASSET_DIRS.iter().any(|d| path.as_str().starts_with(d));
            out.push(Diagnostic::new(
                Warning,
                path.as_str(),
                if asset { "asset is never used" } else { NOT_READ },
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level_pack::tests::sample_level;
    use crate::mod_pack::tests::sample_mod;
    use crate::{write_level, write_mod};
    use olr_data::mod_def::SpawnDef;
    use olr_vfs::MemFs;

    #[test]
    fn a_format_error_maps_to_a_diagnostic() {
        let parse = PackError::Format(FormatError::Parse {
            file: "level.ron".into(),
            line: 3,
            col: 7,
            message: "bad".into(),
        });
        let d = Diagnostic::from_error(&parse);
        assert_eq!(
            (d.file.as_deref(), d.line, d.col, d.message.as_str()),
            (Some("level.ron"), Some(3), Some(7), "bad")
        );
        let invalid = PackError::Format(FormatError::Invalid {
            file: "core.ron".into(),
            path: "sprites".into(),
            message: "too many".into(),
        });
        let d = Diagnostic::from_error(&invalid);
        assert_eq!(
            (d.file.as_deref(), d.line, d.message.as_str()),
            (Some("core.ron"), None, "sprites: too many")
        );
        assert_eq!(invalid.file(), Some("core.ron"));
    }

    fn written(pack: &ModPack) -> MemFs {
        let mut fs = MemFs::new();
        write_mod(&mut fs, pack).unwrap();
        fs
    }

    #[test]
    fn valid_packs_have_no_diagnostics() {
        assert_eq!(validate(&written(&sample_mod())), vec![]);
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        assert_eq!(validate(&fs), vec![]);
    }

    /// Format 5 inlines every projectile, so the lift refuses an unknown
    /// projectile at write time.
    #[test]
    fn an_unknown_projectile_cannot_be_written() {
        let mut pack = sample_mod();
        pack.def.projectiles.get_mut("rocket").unwrap().event_spawn =
            Some(SpawnDef::olx_default("nope".into()));
        let e = write_mod(&mut MemFs::new(), &pack).unwrap_err().to_string();
        assert!(e.contains("nope"), "{e}");
    }

    /// The lift writes a cycle (as shared projectiles); `read_mod` refuses it
    /// when compiling, and `validate` reports that error.
    #[test]
    fn reports_cycles() {
        let mut pack = sample_mod();
        pack.def.projectiles.get_mut("bits").unwrap().event_spawn =
            Some(SpawnDef::olx_default("rocket".into()));
        let d = validate(&written(&pack));
        let msg = d.iter().find(|d| d.message.contains("cycle")).expect("cycle reported");
        assert_eq!(msg.severity, Severity::Error);
        assert!(msg.file.as_deref().is_some_and(|f| f.starts_with("projectiles/")), "{msg:?}");
    }

    #[test]
    fn an_unused_shared_projectile_is_a_warning() {
        let mut pack = sample_mod();
        let extra = pack.def.projectiles["bits"].clone();
        pack.def.projectiles.insert("spare".into(), extra);
        let d: Vec<String> = validate(&written(&pack)).iter().map(|d| d.to_string()).collect();
        assert_eq!(d, vec!["warning: projectiles/spare.ron: projectile is never used".to_string()]);
    }

    #[test]
    fn an_invalid_weapon_value_is_reported_with_its_file() {
        let fs = written(&sample_mod());
        let path = VPath::new("weapons/rocket.ron").unwrap();
        let text = String::from_utf8(fs.read(&path).unwrap()).unwrap();
        let bad = text.replace("speed: 100,", "speed: \"fast\",");
        assert_ne!(text, bad);
        let d = validate(&fs.with("weapons/rocket.ron", bad.as_str()));
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].file.as_deref(), Some("weapons/rocket.ron"));
        assert_eq!((d[0].line, d[0].col), (Some(11), Some(19)));
    }

    #[test]
    fn an_invalid_weapon_value_names_its_field_path() {
        let fs = written(&sample_mod());
        let path = VPath::new("weapons/rocket.ron").unwrap();
        let text = String::from_utf8(fs.read(&path).unwrap()).unwrap();
        let bad = text.replace("speed: 100,", "speed: 100,\n amount: 99999999,");
        assert_ne!(text, bad);
        let d = validate(&fs.with("weapons/rocket.ron", bad.as_str()));
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!((d[0].file.as_deref(), d[0].line), (Some("weapons/rocket.ron"), None));
        assert!(d[0].message.contains("fire.shot.amount"), "{}", d[0].message);
    }

    /// Issue #19: a used image's glow mask is no unused asset; a mask
    /// without its image, of another size or cut short is an error.
    #[test]
    fn glow_masks_go_with_their_images() {
        let png = |w, h| crate::image_io::encode_rgba(&olr_data::image::RgbaImage::new(w, h));
        let mask = |w, h| olr_format::mods::encode_glow_mask((w, h), &vec![255; (w * h) as usize]);
        let mut pack = sample_mod();
        pack.assets.insert(VPath::new("gfx/missile.png").unwrap(), png(8, 4));
        pack.assets.insert(VPath::new("gfx/missile.glow.png").unwrap(), mask(8, 4));
        assert_eq!(validate(&written(&pack)), vec![]);
        let check = |path: &str, bytes: Vec<u8>| {
            let mut p = pack.clone();
            p.assets.insert(VPath::new(path).unwrap(), bytes);
            validate(&written(&p)).iter().map(|d| d.to_string()).collect::<Vec<_>>()
        };
        let d = check("gfx/missile.glow.png", mask(4, 4));
        assert!(
            d.len() == 1 && d[0].starts_with("error: gfx/missile.glow.png: ") && d[0].contains("4 × 4"),
            "{d:?}"
        );
        let d = check("gfx/other.glow.png", mask(8, 4));
        assert_eq!(
            d,
            vec!["error: gfx/other.glow.png: a glow mask without its image \"gfx/other.png\"".to_string()]
        );
        let cut = mask(8, 4)[..30].to_vec();
        let d = check("gfx/missile.glow.png", cut);
        assert!(d.len() == 1 && d[0].starts_with("error: gfx/missile.glow.png"), "{d:?}");
        // An unused image's mask is as unused as it.
        let mut p = pack.clone();
        p.assets.insert(VPath::new("gfx/extra.png").unwrap(), png(2, 2));
        p.assets.insert(VPath::new("gfx/extra.glow.png").unwrap(), mask(2, 2));
        let d: Vec<String> = validate(&written(&p)).iter().map(|d| d.to_string()).collect();
        assert_eq!(
            d,
            vec![
                "warning: gfx/extra.glow.png: asset is never used".to_string(),
                "warning: gfx/extra.png: asset is never used".to_string()
            ]
        );
    }

    #[test]
    fn reports_missing_asset_and_unused_items() {
        let mut pack = sample_mod();
        pack.assets.remove(&VPath::new("sfx/boom.ogg").unwrap());
        pack.assets.insert(VPath::new("gfx/extra.png").unwrap(), b"x".to_vec());
        let d: Vec<String> = validate(&written(&pack)).iter().map(|d| d.to_string()).collect();
        assert!(d.contains(&"error: mod.ron: missing asset \"sfx/boom.ogg\"".to_string()), "{d:?}");
        assert!(d.contains(&"warning: gfx/extra.png: asset is never used".to_string()), "{d:?}");
    }

    /// Issue #17: the mod editor's file is read, not an unused asset; one
    /// that does not parse is a warning.
    #[test]
    fn the_mod_editors_file_is_read() {
        let fs = written(&sample_mod()).with("editor/mod.ron", "(base: \"olx-classic\")");
        assert_eq!(validate(&fs), vec![]);
        let fs = fs.with("editor/mod.ron", "(bsae: 1)");
        let d: Vec<String> = validate(&fs).iter().map(|d| d.to_string()).collect();
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].starts_with("warning: editor/mod.ron:") && d[0].contains("bsae"), "{d:?}");
    }

    #[test]
    fn reports_parse_errors_with_position() {
        let fs = written(&sample_mod()).with("mod.ron", "(name: \"x\",\n oops)");
        let d = validate(&fs);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].file.as_deref(), d[0].line), (Some("mod.ron"), Some(2)));
    }

    fn tiny_level_pack() -> MemFs {
        let mut fs = MemFs::new();
        write_level(&mut fs, &sample_level()).unwrap();
        fs
    }

    /// An 8×8 level with one spawn at `at`.
    fn level_with_spawn(at: (i32, i32)) -> MemFs {
        let mut pack = sample_level();
        let level = &mut pack.level;
        level.def.width = 8;
        level.def.height = 8;
        level.def.art_scale = 1;
        level.def.spawns = vec![olr_data::level::SpawnPoint { x: at.0, y: at.1 }];
        level.material =
            olr_data::level::MaterialGrid::from_cells(8, 8, vec![olr_data::level::Material::Dirt; 64])
                .unwrap();
        level.front = olr_data::image::RgbaImage::new(8, 8);
        level.back = olr_data::image::RgbaImage::new(8, 8);
        let mut fs = MemFs::new();
        write_level(&mut fs, &pack).unwrap();
        fs
    }

    /// A level whose placement is its ground alone: one raster `base`
    /// (issue #74).
    fn ground_alone() -> olr_format::levels::PlacedSource {
        let level = olr_format::levels::read_level_files(&level_with_spawn((1, 1))).unwrap();
        let indices: Vec<u8> = level.material.iter().map(|m| *m as u8).collect();
        let [m, f, b] = olr_format::levels::raster_files("base");
        let strip = |p: String| p.strip_prefix("editor/").unwrap().to_string();
        let mut files = std::collections::BTreeMap::new();
        files.insert(
            strip(m),
            crate::image_io::encode_indexed(8, 8, &indices, &olr_format::levels::MATERIAL_PALETTE),
        );
        files.insert(strip(f), crate::image_io::encode_rgba(&level.front));
        files.insert(strip(b), crate::image_io::encode_rgba(&level.back));
        let element = olr_format::levels::Element {
            id: "base".into(),
            name: String::new(),
            visible: true,
            locked: false,
            folder: None,
            kind: olr_format::levels::ElementKind::Raster,
            glow: None,
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        let ground = olr_format::assets::AssetSource::new(
            olr_format::assets::AssetFile::new("Ground", (8, 8), vec![element]),
            files,
        );
        olr_format::levels::PlacedSource {
            file: olr_format::levels::PlacedFile {
                assets: vec![olr_format::levels::LevelAsset {
                    key: "ground".into(),
                    origin: olr_format::levels::AssetOrigin::Own,
                }],
                copies: vec![olr_format::levels::PlacedCopy::new("ground", "ground", (0, 0))],
                script: false,
            },
            assets: [("ground".to_string(), ground)].into(),
            scripts: Default::default(),
            level_script: None,
        }
    }

    fn placed_level() -> MemFs {
        let mut fs = level_with_spawn((1, 1));
        olr_format::levels::write_placed_files(&mut fs, &ground_alone()).unwrap();
        fs
    }

    /// Issue #74: the placement is checked (`check_placed`); a file the
    /// placement does not read is a warning, a stray script too.
    #[test]
    fn validate_checks_the_placement() {
        assert_eq!(validate(&placed_level()), vec![]);
        let small = crate::image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 4));
        let d = validate(&placed_level().with("editor/layers/base.front.png", small));
        assert!(
            d.iter().any(|d| d.severity == Severity::Error
                && d.file.as_deref() == Some("editor/layers/base.front.png")),
            "{d:?}"
        );
        let d: Vec<String> =
            validate(&placed_level().with("editor/notes.txt", "x")).iter().map(|d| d.to_string()).collect();
        assert_eq!(d, ["warning: editor/notes.txt: not read by the format"]);
        let d: Vec<String> = validate(&placed_level().with("editor/scripts/x.rhai", ""))
            .iter()
            .map(|d| d.to_string())
            .collect();
        assert_eq!(d, [format!("warning: editor/scripts/x.rhai: {STRAY_SCRIPT_WARNING}")]);
        // A copy of an asset the placement does not hold.
        let mut p = ground_alone();
        p.file.copies.push(olr_format::levels::PlacedCopy::new("lamp", "a", (0, 0)));
        let mut fs = level_with_spawn((1, 1));
        olr_format::levels::write_placed_files(&mut fs, &p).unwrap();
        let d = validate(&fs);
        assert!(
            d.iter().any(|d| d.severity == Severity::Error && d.file.as_deref() == Some("editor/placed.ron")),
            "{d:?}"
        );
        // A placement without its ground: an error, never a panic.
        let mut fs = MemFs::new();
        for (path, bytes) in placed_level().files() {
            if path.as_str() != "editor/asset.ron" {
                olr_vfs::ContentSink::write(&mut fs, path, bytes).unwrap();
            }
        }
        assert!(validate(&fs).iter().any(|d| d.severity == Severity::Error), "{:?}", validate(&fs));
    }

    /// Issue #74: OLR 0.2's composition is a warning, unconverted or left
    /// beside a placement (the baked level plays); its files are not
    /// flagged one by one.
    #[test]
    fn a_leftover_composition_is_a_warning() {
        let p = ground_alone();
        let ground = &p.assets["ground"];
        let src = olr_format::assets::as_composition(ground);
        let mut legacy = level_with_spawn((1, 1));
        olr_format::levels::write_composition_files(&mut legacy, &src).unwrap();
        let d: Vec<String> = validate(&legacy).iter().map(|d| d.to_string()).collect();
        assert_eq!(d, [format!("warning: editor/composition.ron: {UNCONVERTED_WARNING}")]);
        let mut both = placed_level();
        olr_format::levels::write_composition_files(&mut both, &src).unwrap();
        let d: Vec<String> = validate(&both).iter().map(|d| d.to_string()).collect();
        assert_eq!(d, [format!("warning: editor/composition.ron: {LEFTOVER_WARNING}")]);
    }

    /// Issue #37: the baked surface maps are checked: their size, and
    /// values within the look's materials; an element's surface names a
    /// material of its asset.
    #[test]
    fn surface_maps_are_validated() {
        let mut fs = placed_level();
        let mut level = olr_format::levels::read_level_files(&fs).unwrap();
        let stone = olr_format::materials::MaterialSource {
            light_soak: None,
            id: "stone".into(),
            name: "Stone".into(),
            notes: String::new(),
            heat: None,
        };
        level.look =
            Some(olr_format::look::LookSource { materials: vec![stone.clone()], ..Default::default() });
        level.surface =
            Some(olr_format::levels::SurfaceMaps { world: (8, 8), front: vec![1; 64], back: vec![0; 64] });
        olr_format::levels::write_level_files(&mut fs, &level).unwrap();
        assert_eq!(validate(&fs), vec![]);
        let errors = |fs: &MemFs| -> Vec<String> {
            validate(fs).iter().filter(|d| d.severity == Severity::Error).map(|d| d.to_string()).collect()
        };
        let grey = |w, h, v: &[u8]| crate::image_io::encode_grey(w, h, v);
        let e = errors(&fs.clone().with("look/surface.png", grey(4, 4, &[0; 16])));
        assert!(e.iter().any(|e| e.contains("look/surface.png")), "{e:?}");
        let e = errors(&fs.clone().with("look/surface-back.png", grey(8, 8, &[2; 64])));
        assert!(e.iter().any(|e| e.contains("look/surface-back.png") && e.contains("material 2")), "{e:?}");
        // A painted map on the ground's raster.
        let mut p = ground_alone();
        let g = p.assets.get_mut("ground").unwrap();
        g.file.materials = vec![stone];
        g.file.elements[0].surface = Some("stone".into());
        g.file.elements[0].surface_map = true;
        g.files.insert("surface/base.png".into(), grey(8, 8, &[1; 64]));
        let mut ok = fs.clone();
        olr_format::levels::write_placed_files(&mut ok, &p).unwrap();
        assert_eq!(validate(&ok), vec![]);
        p.assets.get_mut("ground").unwrap().file.elements[0].surface = Some("lava".into());
        let mut bad = fs.clone();
        olr_format::levels::write_placed_files(&mut bad, &p).unwrap();
        let e = errors(&bad);
        assert!(e.iter().any(|e| e.contains("lava")), "{e:?}");
    }

    /// Deferred minors: a missing ground file is one error, not also
    /// a "not read" warning for every other `editor/` file.
    #[test]
    fn a_missing_ground_file_is_one_error() {
        use olr_vfs::ContentSink as _;
        let mut fs = MemFs::new();
        for (p, b) in placed_level().files() {
            if p.as_str() != "editor/layers/base.front.png" {
                fs.write(p, b).unwrap();
            }
        }
        let d: Vec<String> = validate(&fs).iter().map(|d| d.to_string()).collect();
        assert_eq!(d.len(), 1, "{d:?}");
        assert!(d[0].contains("editor/layers/base.front.png"), "{d:?}");
    }

    #[test]
    fn warns_about_files_the_format_does_not_read() {
        let mut fs = tiny_level_pack();
        fs = fs.with("level.ron~", "backup").with("art/.DS_Store", "x");
        let d = validate(&fs);
        let warned: Vec<_> =
            d.iter().filter(|d| d.severity == Severity::Warning).map(|d| d.file.clone().unwrap()).collect();
        assert_eq!(warned, ["art/.DS_Store", "level.ron~"]);
        assert!(d.iter().all(|d| d.severity == Severity::Warning));
        assert_eq!(d[1].to_string(), "warning: level.ron~: not read by the format");
    }

    #[test]
    fn a_mod_warns_once_about_a_file_outside_its_asset_directories() {
        let d: Vec<String> = validate(&written(&sample_mod()).with("README.txt", "hi"))
            .iter()
            .map(|d| d.to_string())
            .collect();
        assert_eq!(d, ["warning: README.txt: not read by the format"]);
    }

    /// Review A-9: the reader reads an empty `restrictions.ron` (the
    /// writer leaves it out): it is no foreign file.
    #[test]
    fn an_empty_restrictions_file_is_read() {
        let mut pack = sample_mod();
        pack.def.restrictions.clear();
        pack.def.loadouts.clear();
        assert_eq!(validate(&written(&pack).with("restrictions.ron", "{}\n")), vec![]);
    }

    /// A shipped loadout naming a preset the mod lacks (issue #5) is an
    /// error, never a panic.
    #[test]
    fn a_loadout_without_its_preset_is_an_error() {
        let mut pack = sample_mod();
        pack.def.restrictions.clear();
        let d = validate(&written(&pack));
        assert!(d.iter().any(|d| d.to_string().contains("names no preset")), "{d:?}");
    }

    /// Review A-5: an editor's leftovers inside a mod's folders.
    #[test]
    fn a_mod_warns_about_leftovers_in_its_folders() {
        let fs = written(&sample_mod())
            .with("weapons/rocket.ron~", "backup")
            .with("weapons/.rocket.ron.swp", "x")
            .with("projectiles/.DS_Store", "x");
        let d: Vec<String> = validate(&fs).iter().map(|d| d.to_string()).collect();
        assert_eq!(
            d,
            [
                "warning: projectiles/.DS_Store: not read by the format",
                "warning: weapons/.rocket.ron.swp: not read by the format",
                "warning: weapons/rocket.ron~: not read by the format",
            ]
        );
    }

    #[test]
    fn warns_about_a_spawn_outside_the_level() {
        assert_eq!(validate(&level_with_spawn((7, 7))), vec![]);
        let d = validate(&level_with_spawn((8, 3)));
        assert!(
            d.iter().any(|d| d.severity == Severity::Warning
                && d.message.contains("objects[0]")
                && d.message.contains("outside the level")),
            "{d:?}"
        );
        assert_eq!(
            d[0].to_string(),
            "warning: level.ron: objects[0]: spawn at (8, 3) is outside the level (8×8)"
        );
    }

    #[test]
    fn reports_missing_manifest() {
        let d = validate(&MemFs::new());
        assert_eq!(d[0].severity, Severity::Error);
        assert!(d[0].message.contains("pack.ron"), "{}", d[0].message);
    }
}
