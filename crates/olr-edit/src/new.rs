//! Pack templates (`olr-cli pack new`, plan 13d R6): a one-weapon mod and
//! a plain level made in code, or a copy of an existing pack of any kind
//! under a new id and name.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_data::restrictions::WeaponState;
use olr_format::Colour;
use olr_format::levels::{LevelSource, MAX_SIDE};
use olr_format::mods::{
    Effect, Fire, Gfx, ModSource, Projectile, Rope, ShotSpawn, Weapon, WeaponClass, Worm,
};
use olr_pack::PackError;

use crate::pack::{Pack, SaveError, Source, load, save};

/// What a new pack starts from.
#[derive(Clone, Copy, Debug)]
pub enum Template<'a> {
    /// The smallest mod that compiles: one weapon, `gun`.
    Mod,
    /// A level of `size` world pixels: dirt inside a one-pixel rock border,
    /// on the theme `theme`.
    Level { size: (u32, u32), theme: &'a str },
    /// A copy of the pack at this directory.
    From(&'a Path),
}

/// Why no pack was made; nothing was written.
#[derive(Debug, thiserror::Error)]
pub enum NewError {
    #[error("id {0:?} must be a lowercase slug (a-z, 0-9 and -)")]
    BadId(String),
    #[error("{}: exists and is not an empty directory", .0.display())]
    NotEmpty(PathBuf),
    #[error("a {template} template makes no {kind:?} pack")]
    WrongKind { kind: PackKind, template: String },
    #[error("level size {0}x{1} is not within 1x1 to {MAX_SIDE}x{MAX_SIDE}")]
    Size(u32, u32),
    #[error("{}: {source}", from.display())]
    Load { from: PathBuf, source: PackError },
    #[error(transparent)]
    Save(#[from] SaveError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The rule `olr_pack::validate` applies to a manifest's id.
fn is_slug(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The longest id [`pack_id`] makes.
const MAX_PACK_ID: usize = 64;

/// A pack id from `name` (the editor's new and copied packs, plan 14c R5):
/// lowercase, every run of other characters than `a-z` and `0-9` one `-`,
/// leading and trailing `-` trimmed, at most 64 bytes; then `-2`, `-3`, …
/// appended until `taken` is false. `level` when the name gives nothing.
pub fn pack_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut base = String::new();
    for c in name.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            base.push(c);
        } else if !base.ends_with('-') {
            base.push('-');
        }
    }
    base.truncate(MAX_PACK_ID);
    let base = match base.trim_matches('-') {
        "" => "level".to_string(),
        b => b.to_string(),
    };
    std::iter::once(base.clone())
        .chain((2u32..).map(|i| {
            let suffix = format!("-{i}");
            let head = base[..base.len().min(MAX_PACK_ID - suffix.len())].trim_end_matches('-');
            format!("{head}{suffix}")
        }))
        .find(|c| !taken(c))
        .unwrap_or_default()
}

/// Writes a new pack `id` (name `name`) into `dir`, which must not exist
/// or be empty. Kind `kind`; `Template::From` must name a pack of `kind`.
/// Returns the save's warnings.
pub fn new_pack(
    dir: &Path,
    kind: PackKind,
    id: &str,
    name: &str,
    template: Template<'_>,
) -> Result<Vec<String>, NewError> {
    if !is_slug(id) {
        return Err(NewError::BadId(id.into()));
    }
    let existed = dir.exists();
    if existed && (!dir.is_dir() || std::fs::read_dir(dir)?.next().is_some()) {
        return Err(NewError::NotEmpty(dir.into()));
    }
    let wrong = |template: &str| NewError::WrongKind { kind, template: template.into() };
    let mut pack = match template {
        Template::Mod if kind == PackKind::Mod => fresh(Source::Mod(mod_template(name))),
        Template::Mod => return Err(wrong("mod")),
        Template::Level { size, theme } if kind == PackKind::Level => {
            fresh(Source::Level(level_template(name, size, theme)?))
        }
        Template::Level { .. } => return Err(wrong("level")),
        Template::From(from) => {
            let pack = load(from).map_err(|source| NewError::Load { from: from.into(), source })?;
            if pack.manifest.kind != kind {
                return Err(wrong(&format!("{:?} pack's", pack.manifest.kind)));
            }
            pack
        }
    };
    pack.manifest.id = id.into();
    pack.manifest.name = name.into();
    pack.manifest.origin = None;
    rename(&mut pack.source, name);
    // `replace_dir` swaps the new pack in over an empty directory. A failed
    // save removes what this made: `dir` and the parents it lacked.
    let made = dir.ancestors().take_while(|a| !a.as_os_str().is_empty() && !a.exists()).last();
    std::fs::create_dir_all(dir)?;
    save(dir, &pack).map_err(|e| {
        if let Some(made) = made {
            let _ = std::fs::remove_dir_all(made);
        }
        e.into()
    })
}

/// A pack of `source` with a new manifest (id and name set by the caller).
fn fresh(source: Source) -> Pack {
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: String::new(),
        name: String::new(),
        version: "0.1.0".into(),
        kind: source.kind(),
        authors: Vec::new(),
        origin: None,
        license: None,
    };
    Pack { manifest, source, composition: None, other: BTreeMap::new() }
}

/// Sets the name the menus show inside the source (core packs have none).
fn rename(source: &mut Source, name: &str) {
    match source {
        Source::Mod(s) => s.name = name.into(),
        Source::Level(s) => s.name = name.into(),
        Source::Skin(s) => s.name = name.into(),
        Source::Theme(s) => s.name = name.into(),
        Source::Core(_) => {}
    }
}

/// One weapon, `gun` ("Gun"): a yellow pixel that explodes on terrain and
/// injures a worm; the default worm and rope; a preset, `Standard`,
/// enabling it.
fn mod_template(name: &str) -> ModSource {
    let bullet = Projectile {
        hit_terrain: vec![Effect::Explode { damage: 2, shake: 0, sound: None }],
        hit_worm: vec![Effect::Injure { damage: 8 }],
        ..Projectile::new(Gfx::Pixels(vec![Colour(Rgba::rgb(255, 230, 64))]))
    };
    let gun = Weapon {
        name: "Gun".into(),
        notes: String::new(),
        recharge: 0.5,
        drain: 10.0,
        between_shots_s: 0.1,
        laser_sight: false,
        fire: Fire::Projectile {
            class: WeaponClass::default(),
            recoil: 0,
            sound: None,
            shot: ShotSpawn { speed: 200, projectile: Some(Box::new(bullet)), ..ShotSpawn::default() },
        },
    };
    let standard = BTreeMap::from([("Gun".to_string(), WeaponState::Enabled)]);
    ModSource {
        name: name.into(),
        notes: String::new(),
        worm: Worm::default(),
        rope: Rope::default(),
        weapons: vec![("gun".into(), gun)],
        shared: BTreeMap::new(),
        restrictions: BTreeMap::from([("Standard".to_string(), standard)]),
    }
}

/// Dirt inside a one-pixel rock border, art scale 1: front dirt-brown
/// (rock grey on the border), back near-black; with its thumbnail.
fn level_template(name: &str, (w, h): (u32, u32), theme: &str) -> Result<LevelSource, NewError> {
    if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
        return Err(NewError::Size(w, h));
    }
    let (dirt, rock, back) = (Rgba::rgb(139, 90, 43), Rgba::rgb(128, 128, 128), Rgba::rgb(16, 12, 8));
    let mut material = Vec::with_capacity(w as usize * h as usize);
    let mut front = RgbaImage::new(w, h);
    let mut back_art = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let border = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            material.push(if border { Material::Rock } else { Material::Dirt });
            front.set(x, y, if border { rock } else { dirt });
            back_art.set(x, y, back);
        }
    }
    let mut level = LevelSource {
        name: name.into(),
        notes: String::new(),
        size: (w, h),
        art_scale: 1,
        theme: theme.into(),
        objects: Vec::new(),
        material,
        front,
        back: back_art,
        thumb: None,
    };
    crate::level::refresh_thumb(&mut level);
    Ok(level)
}
