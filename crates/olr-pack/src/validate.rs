//! Pack validation with file and position information (spec §9.1).

use std::collections::BTreeSet;
use std::fmt;

use olr_data::manifest::PackKind;
use olr_format::FormatError;
use olr_vfs::{ContentSource, MemFs, VPath, walk};

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
            // Presentation only: the level plays without it.
            if let Err(e) = crate::read_level_thumb(src) {
                out.push(Diagnostic::new(Severity::Warning, THUMB_FILE, format!("{e}: not shown")));
            }
            out.extend(check_level(&p));
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
    let mut owned = MemFs::new();
    let written = match kind {
        PackKind::Mod => mods::read_mod_files(src).and_then(|s| mods::write_mod_files(&mut owned, &s)),
        PackKind::Level => levels::read_level_files_without_thumb(src)
            .and_then(|s| levels::write_level_files(&mut owned, &s)),
        PackKind::Skin => skins::read_skin_files(src).and_then(|s| skins::write_skin_files(&mut owned, &s)),
        PackKind::Theme => {
            themes::read_theme_files(src).and_then(|s| themes::write_theme_files(&mut owned, &s))
        }
        PackKind::Core => {
            core_pack::read_core_files(src).and_then(|s| core_pack::write_core_files(&mut owned, &s))
        }
    };
    // The reader above already accepted the pack; nothing to add if not.
    let (Ok(()), Ok(paths)) = (written, walk(src, &VPath::root())) else { return Vec::new() };
    let owned: BTreeSet<&str> = owned.files().map(|(p, _)| p.as_str()).collect();
    paths
        .iter()
        .map(VPath::as_str)
        .filter(|p| *p != MANIFEST && !owned.contains(p))
        .filter(|p| !(kind == PackKind::Level && *p == THUMB_FILE))
        .filter(|p| !(kind == PackKind::Mod && !is_definition(p)))
        .map(|p| Diagnostic::new(Severity::Warning, p, NOT_READ))
        .collect()
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
    for path in pack.assets.keys() {
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

    #[test]
    fn reports_missing_asset_and_unused_items() {
        let mut pack = sample_mod();
        pack.assets.remove(&VPath::new("sfx/boom.ogg").unwrap());
        pack.assets.insert(VPath::new("gfx/extra.png").unwrap(), b"x".to_vec());
        let d: Vec<String> = validate(&written(&pack)).iter().map(|d| d.to_string()).collect();
        assert!(d.contains(&"error: mod.ron: missing asset \"sfx/boom.ogg\"".to_string()), "{d:?}");
        assert!(d.contains(&"warning: gfx/extra.png: asset is never used".to_string()), "{d:?}");
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
