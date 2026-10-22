//! A mod's images and sounds (issue #19): the mod editor's Art and Sounds
//! and `olr-cli mod gfx|sfx`. Every operation works on a loaded mod
//! [`Pack`] and changes nothing when it is refused: an image is a PNG
//! whose frames are whole (frame count = width ÷ height, at least one),
//! a glow mask (`gfx/<name>.glow.png`, `olr_format::mods::glow_mask_of`)
//! is its image's size, a sound is WAV or Ogg within the game's limits
//! (`olr_pack::sounds::check_sound`); a file in use is never removed, and a
//! rename moves the file (and its mask) and every field naming it.

use olr_format::image_io::{self, IMAGES};
use olr_format::mods::{
    Fire, GFX_DIR, ModSource, SFX_DIR, decode_glow_mask, encode_glow_mask, glow_mask_of, image_of_mask,
    is_image,
};
use olr_vfs::VPath;

use super::path::{File, Seg};
use super::walk::{assets, each_projectile_mut, rename_projectile_assets};
use super::{ModError, commit, source};
use crate::pack::Pack;

/// The most bytes an image or sound file may have (the pack reader's cap).
pub const MAX_ASSET_BYTES: usize = olr_pack::MAX_FILE_BYTES as usize;

/// A field naming an asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetUser {
    /// The field's path (`weapons/bazooka.fire.shot.projectile.gfx.file`).
    pub path: String,
    /// What it is, for people: `Bazooka's shot`, `Bazooka's fire sound`,
    /// `projectile bazooka.spawn's hit terrain sound`.
    pub label: String,
}

/// The mod's images (`gfx/*.png` but the glow masks), sorted.
pub fn images(pack: &Pack) -> Vec<String> {
    pack.other.keys().map(|p| p.to_string()).filter(|p| is_image(p)).collect()
}

/// The mod's sounds (every file in `sfx/`), sorted.
pub fn sounds(pack: &Pack) -> Vec<String> {
    pack.other.keys().map(|p| p.to_string()).filter(|p| p.starts_with(SFX_DIR)).collect()
}

/// The fields naming asset `asset`, in the mod's order (weapons, then
/// shared projectiles).
pub fn users(src: &ModSource, asset: &str) -> Vec<AssetUser> {
    let mut out: Vec<AssetUser> = assets(src)
        .into_iter()
        .filter(|(a, _)| a == asset)
        .map(|(_, path)| {
            let owner = match &path.file {
                File::Weapon(id) => src
                    .weapons
                    .iter()
                    .find(|(w, _)| w == id)
                    .map_or_else(|| id.clone(), |(_, w)| w.name.clone()),
                File::Shared(id) => format!("projectile {id}"),
                f => f.to_string(),
            };
            AssetUser { path: path.to_string(), label: format!("{owner}'s {}", what(&path.segs)) }
        })
        .collect();
    out.dedup();
    out
}

/// A field's place for people: the shot, its spawn's or trail's, and what
/// names the asset (`shot's spawn's hit terrain sound`).
fn what(segs: &[Seg]) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut last = "";
    let mut iter = segs.iter().peekable();
    while let Some(s) = iter.next() {
        let Seg::Name(n) = s else { continue };
        match n.as_str() {
            "shot" => parts.push("shot"),
            "spawns" => {
                iter.next();
                parts.push("spawn");
            }
            "trail" => {
                iter.next();
                parts.push("trail");
            }
            "hit_terrain" => last = "hit terrain sound",
            "hit_worm" => last = "hit worm sound",
            "timer" => last = "timer sound",
            "sound" if last.is_empty() => last = if parts.is_empty() { "fire sound" } else { "sound" },
            _ => {}
        }
    }
    if !last.is_empty() {
        parts.push(last);
    }
    if parts.is_empty() { "image".into() } else { parts.join("'s ") }
}

fn vpath(path: &str) -> Result<VPath, ModError> {
    VPath::new(path).map_err(|e| ModError::Refused(format!("{path:?}: {e}")))
}

/// A new asset's path in `dir` (`gfx/`, `sfx/`): a plain file name.
fn new_path(dir: &str, path: &str) -> Result<VPath, ModError> {
    let name =
        path.strip_prefix(dir).ok_or_else(|| ModError::Refused(format!("{path:?} is not in {dir}")))?;
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return Err(ModError::Refused(format!("{path:?}: a file name in {dir}, without folders")));
    }
    vpath(path)
}

/// An image's frame count from its PNG `bytes`, checked: a PNG within the
/// decoder's bounds, at least as wide as it is tall.
pub fn image_frames(bytes: &[u8]) -> Result<u32, ModError> {
    let refused = |m: String| ModError::Refused(m);
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(refused(format!("{} bytes, more than {MAX_ASSET_BYTES}", bytes.len())));
    }
    let img =
        image_io::decode_rgba_within(bytes, IMAGES).map_err(|e| refused(format!("not a PNG image: {e}")))?;
    let (w, h) = (img.width(), img.height());
    if h == 0 || w < h {
        return Err(refused(format!(
            "{w} × {h}: an image is a strip of square frames (its height a frame's side), so at least as wide as tall"
        )));
    }
    Ok(w / h)
}

/// A sound's `bytes`, checked as the game reads them: WAV or Ogg, within
/// its size and length limits.
pub fn check_sound(bytes: &[u8]) -> Result<(), ModError> {
    if !olr_pack::sounds::is_sound(bytes) {
        return Err(ModError::Refused("not a WAV or Ogg sound".into()));
    }
    olr_pack::sounds::check_sound(bytes).map_err(ModError::Refused)
}

/// Adds image `path` (`gfx/<name>.png`, new) of PNG `bytes`.
pub fn import_image(pack: &mut Pack, path: &str, bytes: &[u8]) -> Result<(), ModError> {
    source(pack)?;
    if !is_image(path) {
        return Err(ModError::Refused(format!("{path:?}: an image is gfx/<name>.png (not .glow.png)")));
    }
    let p = new_path(GFX_DIR, path)?;
    if pack.other.contains_key(&p) {
        return Err(ModError::Refused(format!("{path} exists already")));
    }
    image_frames(bytes)?;
    pack.other.insert(p, bytes.to_vec());
    Ok(())
}

/// Adds sound `path` (`sfx/<name>`, new) of `bytes`.
pub fn import_sound(pack: &mut Pack, path: &str, bytes: &[u8]) -> Result<(), ModError> {
    source(pack)?;
    let p = new_path(SFX_DIR, path)?;
    if pack.other.contains_key(&p) {
        return Err(ModError::Refused(format!("{path} exists already")));
    }
    check_sound(bytes)?;
    pack.other.insert(p, bytes.to_vec());
    Ok(())
}

/// What replacing an image did to the simulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageChange {
    pub frames_before: u32,
    pub frames_after: u32,
}

impl ImageChange {
    /// Whether play changes: only the frame count reaches the simulation.
    pub fn changes_play(&self) -> bool {
        self.frames_before != self.frames_after
    }
}

/// Replaces existing image `path` with PNG `bytes` (painted, or a new
/// file over it). Its glow mask goes when the size changed (it would no
/// longer fit). The frame count before and after: a change of it changes
/// play.
pub fn set_image(pack: &mut Pack, path: &str, bytes: &[u8]) -> Result<ImageChange, ModError> {
    source(pack)?;
    let p = vpath(path)?;
    let old = pack
        .other
        .get(&p)
        .filter(|_| is_image(path))
        .ok_or_else(|| ModError::Refused(format!("no image {path}")))?;
    let frames_before = image_frames(old).unwrap_or(0);
    let frames_after = image_frames(bytes)?;
    let size = image_io::png_size(bytes);
    if let Some(mask) = glow_mask_of(path).and_then(|m| VPath::new(&m).ok())
        && pack.other.get(&mask).is_some_and(|m| image_io::png_size(m) != size)
    {
        pack.other.remove(&mask);
    }
    pack.other.insert(p, bytes.to_vec());
    Ok(ImageChange { frames_before, frames_after })
}

/// Image `path`'s glow values (row by row), all 0 without a mask; `None`
/// when the image does not read, or its mask does not fit.
pub fn glow(pack: &Pack, path: &str) -> Option<Vec<u8>> {
    let img = pack.other.get(&VPath::new(path).ok()?)?;
    let (w, h) = image_io::png_size(img)?;
    match glow_mask_of(path).and_then(|m| pack.other.get(&VPath::new(&m).ok()?)) {
        Some(mask) => decode_glow_mask(mask, (w, h)).ok(),
        None => Some(vec![0; w as usize * h as usize]),
    }
}

/// Sets image `path`'s glow mask to `values` (row by row, of its size);
/// all 0 (or `None`) removes the mask.
pub fn set_glow(pack: &mut Pack, path: &str, values: Option<&[u8]>) -> Result<(), ModError> {
    source(pack)?;
    let img = vpath(path).ok().and_then(|p| pack.other.get(&p)).filter(|_| is_image(path));
    let img = img.ok_or_else(|| ModError::Refused(format!("no image {path}")))?;
    let (w, h) = image_io::png_size(img).ok_or_else(|| ModError::Refused(format!("{path} is not a PNG")))?;
    let mask = vpath(&glow_mask_of(path).unwrap_or_default())?;
    match values.filter(|v| v.iter().any(|&g| g != 0)) {
        None => {
            pack.other.remove(&mask);
        }
        Some(v) => {
            if v.len() != w as usize * h as usize {
                return Err(ModError::Refused(format!(
                    "a glow mask of {} values for a {w} × {h} image ({} pixels)",
                    v.len(),
                    w * h
                )));
            }
            pack.other.insert(mask, encode_glow_mask((w, h), v));
        }
    }
    Ok(())
}

/// Sets image `path`'s glow mask to the PNG `bytes` (greyscale, of the
/// image's size), checked.
pub fn set_glow_png(pack: &mut Pack, path: &str, bytes: &[u8]) -> Result<(), ModError> {
    let img = vpath(path).ok().and_then(|p| pack.other.get(&p)).filter(|_| is_image(path));
    let img = img.ok_or_else(|| ModError::Refused(format!("no image {path}")))?;
    let size = image_io::png_size(img).ok_or_else(|| ModError::Refused(format!("{path} is not a PNG")))?;
    let values = decode_glow_mask(bytes, size).map_err(|e| ModError::Refused(format!("glow mask: {e}")))?;
    set_glow(pack, path, Some(&values))
}

/// Why asset `path` cannot go: the fields naming it, listed.
fn in_use(src: &ModSource, path: &str) -> Option<ModError> {
    let u = users(src, path);
    (!u.is_empty()).then(|| {
        let list: Vec<String> = u.iter().map(|u| format!("{} ({})", u.label, u.path)).collect();
        ModError::Refused(format!("{path} is in use by {}", list.join(", ")))
    })
}

/// Removes image or sound `path` (an image with its glow mask); refused
/// while a field names it, listing them.
pub fn remove(pack: &mut Pack, path: &str) -> Result<(), ModError> {
    let src = source(pack)?;
    if let Some(e) = in_use(src, path) {
        return Err(e);
    }
    let p = vpath(path)?;
    let asset = (is_image(path) || path.starts_with(SFX_DIR)) && pack.other.contains_key(&p);
    if !asset {
        return Err(ModError::Refused(format!("no image or sound {path}")));
    }
    pack.other.remove(&p);
    if let Some(mask) = glow_mask_of(path).and_then(|m| VPath::new(&m).ok()) {
        pack.other.remove(&mask);
    }
    Ok(())
}

/// Renames image or sound `from` to `to` (the same folder, a new name):
/// the file, an image's glow mask, and every field naming it.
pub fn rename(pack: &mut Pack, from: &str, to: &str) -> Result<(), ModError> {
    let src = source(pack)?;
    let (image, sound) = (is_image(from), from.starts_with(SFX_DIR));
    let old = vpath(from)?;
    if !(image || sound) || !pack.other.contains_key(&old) {
        return Err(ModError::Refused(format!("no image or sound {from}")));
    }
    if from == to {
        return Ok(());
    }
    let new = new_path(if image { GFX_DIR } else { SFX_DIR }, to)?;
    if image && !is_image(to) {
        return Err(ModError::Refused(format!("{to:?}: an image is gfx/<name>.png (not .glow.png)")));
    }
    if pack.other.contains_key(&new) || image_of_mask(to).is_some() {
        return Err(ModError::Refused(format!("{to} exists already")));
    }
    let mut other = pack.other.clone();
    let bytes = other.remove(&old).unwrap_or_default();
    other.insert(new, bytes);
    if image
        && let (Some(a), Some(b)) = (glow_mask_of(from), glow_mask_of(to))
        && let Some(mask) = other.remove(&vpath(&a)?)
    {
        let b = vpath(&b)?;
        if pack.other.contains_key(&b) {
            return Err(ModError::Refused(format!("{b} exists already")));
        }
        other.insert(b, mask);
    }
    let mut edited = src.clone();
    let rename_asset = |s: &str| (s == from).then(|| to.to_string());
    for (_, w) in &mut edited.weapons {
        if let Fire::Projectile { sound, shot, .. } = &mut w.fire {
            if let Some(n) = sound.as_deref().and_then(rename_asset) {
                *sound = Some(n);
            }
            if let Some(p) = shot.projectile.as_deref_mut() {
                each_projectile_mut(p, &mut |q| rename_projectile_assets(q, &rename_asset));
            }
        }
    }
    for p in edited.shared.values_mut() {
        each_projectile_mut(p, &mut |q| rename_projectile_assets(q, &rename_asset));
    }
    commit(pack, edited, Some(other))
}

/// A blank image of `frames` square frames of `side` pixels, as PNG.
pub fn blank_image(side: u32, frames: u32) -> Result<Vec<u8>, ModError> {
    let (w, h) = (side.saturating_mul(frames), side);
    if side == 0 || frames == 0 {
        return Err(ModError::Refused("an image has at least one frame of at least 1 pixel".into()));
    }
    image_io::check_size(w, h, IMAGES).map_err(ModError::Refused)?;
    Ok(image_io::encode_rgba(&olr_data::image::RgbaImage::new(w, h)))
}
