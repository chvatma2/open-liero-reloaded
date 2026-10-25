//! The level authoring format: `level.ron`, `material.png`, `front.png`,
//! `back.png` and an optional `thumb.png` (spec §6), and an optional
//! composition under `editor/` that bakes into them (plan 14a). A level may
//! have distant (parallax) layers (issue #11), `distant/<i>.png`: they show
//! where `back.png` is see-through (alpha 0), scrolling slower than the
//! level, and through its glass (alpha 1 to 254: the background glass
//! design), whose tint and frost are `look/glass.png`; without them every
//! back pixel is opaque, as always. A level
//! whose look has surface materials (issue #37, `material.md`) may have
//! `look/surface.png` and `look/surface-back.png`: what its front and back
//! pixels are made of, for the look only.

mod composition;
pub use composition::*;
mod files;
pub(crate) use files::LevelFile;
mod thumb;

use olr_data::image::RgbaImage;
use olr_data::level::{DistantLayer, Level, LevelDef, Material, MaterialGrid, SpawnPoint};
use serde::{Deserialize, Serialize};

pub use files::{
    file_paths, read_level_files, read_level_files_lenient, read_level_files_without_thumb,
    read_level_header, write_level_files,
};
pub use thumb::{THUMB_SIZE, letterbox, level_thumbnail, thumbnail};

use crate::image_io::{LEVEL_ART, check_size};
use crate::look::{LookSource, compile_look_source};
use crate::{FormatError, LiftError};
pub use olr_look::SurfaceMaps;
pub use olr_terrain::{GlassMap, GlowBeamMap, GlowLayers};

/// The largest `art_scale` (a level's art pixels per world pixel).
pub const MAX_ART_SCALE: u32 = 8;
/// The most objects in `level.ron`.
pub const MAX_OBJECTS: usize = 4096;
/// The largest level side in world pixels (OLX's 12-bit positions).
pub const MAX_SIDE: u32 = 4096;
/// `material.png`'s palette, indexed by `Material as u8`: rock grey,
/// background black, dirt brown.
pub const MATERIAL_PALETTE: [u8; 9] = [128, 128, 128, 0, 0, 0, 139, 90, 43];

pub(crate) const LEVEL_FILE: &str = "level.ron";
pub(crate) const MATERIAL_FILE: &str = "material.png";
pub(crate) const FRONT_FILE: &str = "front.png";
pub(crate) const BACK_FILE: &str = "back.png";
/// The thumbnail's file name; `olr-pack` re-exports it.
pub const THUMB_FILE: &str = "thumb.png";
/// The largest `thumb.png` read, in bytes. The reader leaves a larger or
/// undecodable one out (presentation only); the writer refuses it.
/// `olr-pack` re-exports it.
pub const MAX_THUMB_BYTES: u64 = 1 << 20;
/// The widest and tallest `thumb.png` read, checked on the PNG header
/// before any decoding (a pack must not make the menus decode a huge
/// image). `olr-pack` re-exports it.
pub const MAX_THUMB_SIDE: u32 = 512;
/// The glow strength of the composite (front over solid, back elsewhere), art size, 8-bit grey.
pub const GLOW_FILE: &str = "look/glow.png";
/// The backdrop's glow strength (what digging reveals), art size, 8-bit grey.
pub const GLOW_BACK_FILE: &str = "look/glow-back.png";
/// The composite's glow group per world pixel, 8-bit grey: 0 no group (the
/// pixel's glow lights nothing), `n` the look's `glow_groups[n - 1]`.
pub const GROUPS_FILE: &str = "look/groups.png";
/// The backdrop's glow group per world pixel, 8-bit grey, as `look/groups.png`.
pub const GROUPS_BACK_FILE: &str = "look/groups-back.png";
/// The level's clear solids per world pixel, 8-bit grey: 0 opaque, any
/// other value lets light through a solid cell (spec §Clear solids).
/// Optional, on its own.
pub const CLEAR_FILE: &str = "look/clear.png";
/// The level's glass (the background glass design), RGBA at the art size:
/// the RGB the filter of the distant layers behind each pixel (white none),
/// the alpha the frost in tenths of a world pixel. Optional, on its own.
pub const GLASS_FILE: &str = "look/glass.png";
/// The glow beams of the composite per world pixel, 8-bit grey-alpha: the
/// grey the direction (see [`GlowBeamMap`]), the alpha the spread (a texel
/// with a direction needs a spread of at least 1). Both beam
/// files or none.
pub const GLOW_BEAM_FILE: &str = "look/glow-beam.png";
/// The backdrop's glow beams, as [`GLOW_BEAM_FILE`].
pub const GLOW_BEAM_BACK_FILE: &str = "look/glow-beam-back.png";
/// A level's glow beam maps: both or none.
pub const GLOW_BEAM_FILES: [&str; 2] = [GLOW_BEAM_FILE, GLOW_BEAM_BACK_FILE];
/// The haze per world pixel, 8-bit grey: 0 none to 255 the look's `haze`.
/// Optional, on its own.
pub const HAZE_FILE: &str = "look/haze.png";
/// A level's glow images: all four or none.
pub const LOOK_FILES: [&str; 4] = [GLOW_FILE, GLOW_BACK_FILE, GROUPS_FILE, GROUPS_BACK_FILE];
/// The front's surface material per world pixel (the rock and dirt), 8-bit
/// grey: 0 none, `v` the look's `materials[v - 1]` (issue #37).
pub const SURFACE_FILE: &str = "look/surface.png";
/// The back's surface material per world pixel (what shows where the front
/// is carved or background), 8-bit grey, as `look/surface.png`.
pub const SURFACE_BACK_FILE: &str = "look/surface-back.png";
/// A level's surface maps: both or none.
pub const SURFACE_FILES: [&str; 2] = [SURFACE_FILE, SURFACE_BACK_FILE];
/// The folder of a level's distant (parallax) layers' images (issue #11).
pub const DISTANT_DIR: &str = "distant";
/// The most distant layers a level has.
pub const MAX_DISTANT: usize = 16;
/// How far from the level's origin a distant layer may be placed, in world
/// pixels along each axis.
pub const MAX_DISTANT_AT: u32 = 4 * MAX_SIDE;

/// The `i`-th distant layer's image: `distant/<i>.png`.
pub fn distant_file(i: usize) -> String {
    format!("{DISTANT_DIR}/{i}.png")
}

/// The `i`-th distant layer's glow mask (issue #62): `distant/<i>-glow.png`.
pub fn distant_glow_file(i: usize) -> String {
    format!("{DISTANT_DIR}/{i}-glow.png")
}

/// A distant layer's image size (art pixels) for a level at `art_scale`:
/// each side 1 to `MAX_SIDE` world pixels, a whole number of them, and the
/// art within `LEVEL_ART`.
pub(crate) fn check_distant_size((w, h): (u32, u32), art_scale: u32) -> Result<(), String> {
    let s = art_scale.max(1);
    let side = |v: u32| v > 0 && v.is_multiple_of(s) && v / s <= MAX_SIDE;
    if !side(w) || !side(h) {
        return Err(format!(
            "{w}x{h}: each side 1 to {MAX_SIDE} world pixels, a multiple of the art scale {s}"
        ));
    }
    check_size(w, h, LEVEL_ART)
}

/// A level's distant layers (issue #11): at most `MAX_DISTANT`, each of
/// depth `MIN_DEPTH` to `MAX_DEPTH`, placed within `MAX_DISTANT_AT` of the
/// origin and of a size `check_distant_size` takes, all within
/// `LEVEL_ART`'s pixels together.
pub(crate) fn check_distant_layers(distant: &[DistantLayer], art_scale: u32) -> Result<(), FormatError> {
    use olr_data::level::{MAX_DEPTH, MIN_DEPTH};
    if distant.len() > MAX_DISTANT {
        return Err(FormatError::invalid(
            LEVEL_FILE,
            "distant",
            format!("{} distant layers, above {MAX_DISTANT}", distant.len()),
        ));
    }
    let mut pixels = 0u64;
    for (i, d) in distant.iter().enumerate() {
        if !(MIN_DEPTH..=MAX_DEPTH).contains(&d.depth) {
            return Err(FormatError::invalid(
                LEVEL_FILE,
                &format!("distant[{i}].depth"),
                format!("{}: {MIN_DEPTH} to {MAX_DEPTH}", d.depth),
            ));
        }
        let far = |v: i32| v.unsigned_abs() > MAX_DISTANT_AT;
        if far(d.at.0) || far(d.at.1) {
            return Err(FormatError::invalid(
                LEVEL_FILE,
                &format!("distant[{i}].at"),
                format!("({}, {}): each within {MAX_DISTANT_AT} of 0", d.at.0, d.at.1),
            ));
        }
        let file = distant_file(i);
        check_distant_size((d.image.width(), d.image.height()), art_scale)
            .map_err(|m| FormatError::invalid(&file, "", m))?;
        pixels += u64::from(d.image.width()) * u64::from(d.image.height());
        if pixels > LEVEL_ART.max_pixels {
            return Err(FormatError::invalid(
                &file,
                "",
                format!(
                    "the distant layers up to this one hold {pixels} pixels, above {}",
                    LEVEL_ART.max_pixels
                ),
            ));
        }
    }
    Ok(())
}

/// `material` (`size`, row-major) as `material.png` holds it: an 8-bit
/// palette PNG of `MATERIAL_PALETTE`, indexed by `Material as u8`. The
/// writer and a raster element's material (`olr_edit::compose`) share it,
/// so a level composed of its raster base bakes back to its own bytes.
pub fn encode_material(size: (u32, u32), material: &[Material]) -> Vec<u8> {
    let indices: Vec<u8> = material.iter().map(|m| *m as u8).collect();
    crate::image_io::encode_indexed(size.0, size.1, &indices, &MATERIAL_PALETTE)
}

/// What `level.ron` alone says, read without touching any image.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelHeader {
    pub name: String,
    /// World pixels.
    pub size: (u32, u32),
    pub art_scale: u32,
    /// Theme pack id.
    pub theme: String,
    pub spawns: Vec<SpawnPoint>,
    /// `level.ron`'s objects as written (with their notes).
    pub objects: Vec<LevelObject>,
    /// `level.ron`'s look as written.
    pub look: Option<LookSource>,
}

/// A level as its files hold it.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelSource {
    pub name: String,
    pub notes: String,
    /// World pixels.
    pub size: (u32, u32),
    /// `1..=MAX_ART_SCALE`.
    pub art_scale: u32,
    /// Theme pack id.
    pub theme: String,
    pub objects: Vec<LevelObject>,
    /// `size.0 * size.1`, row-major.
    pub material: Vec<Material>,
    /// `size` × `art_scale`.
    pub front: RgbaImage,
    /// `size` × `art_scale`.
    pub back: RgbaImage,
    /// `thumb.png`'s bytes, passed through (presentation only).
    pub thumb: Option<Vec<u8>>,
    /// `level.ron`'s `look`; `None` draws as always.
    pub look: Option<LookSource>,
    /// The four `look/` images, when the level glows.
    pub glow: Option<GlowLayers>,
    /// `look/clear.png`'s flags, `size.0 * size.1`, row-major, when some
    /// solid is clear.
    pub clear: Option<Vec<u8>>,
    /// `look/glass.png` (the background glass design), the art size, when
    /// some glass tints or frosts.
    pub glass: Option<GlassMap>,
    /// `look/glow-beam.png` and `look/glow-beam-back.png` (the
    /// directional-glow design), when some glow is directional.
    pub glow_beams: Option<GlowBeamMap>,
    /// `look/haze.png`'s values, `size.0 * size.1`, row-major, when the haze
    /// varies.
    pub haze: Option<Vec<u8>>,
    /// The distant (parallax) layers, far to near (`level.ron`'s `distant`
    /// and `distant/<i>.png`; issue #11). With any, `back.png` is
    /// see-through (alpha 0) where no element gives a back.
    pub distant: Vec<DistantLayer>,
    /// The surface maps (`look/surface.png` and `look/surface-back.png`,
    /// issue #37), when some pixel has a surface material.
    pub surface: Option<SurfaceMaps>,
    /// Each distant layer's surface material by index (`level.ron`'s
    /// `distant[i].surface`): the id of one of the look's materials; past
    /// its end, none. The readers and the bake leave no `None` at its end.
    pub distant_surfaces: Vec<Option<String>>,
}

/// A thing placed in a level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum LevelObject {
    /// A spawn point (world pixels). The 0.2 runtime does not read spawns yet.
    Spawn {
        /// World pixels (x, y), origin top-left, y down; outside the level
        /// is a warning of `olr-cli pack check`.
        at: (i32, i32),
        /// Free text for authors; ignored by the game.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        notes: String,
    },
}

impl LevelObject {
    /// A spawn point without notes.
    pub fn spawn(at: (i32, i32)) -> LevelObject {
        LevelObject::Spawn { at, notes: String::new() }
    }

    /// Where the object is, world pixels.
    pub fn at(&self) -> (i32, i32) {
        match self {
            LevelObject::Spawn { at, .. } => *at,
        }
    }

    /// Moves the object.
    pub fn set_at(&mut self, to: (i32, i32)) {
        match self {
            LevelObject::Spawn { at, .. } => *at = to,
        }
    }

    /// A spawn point's position; `None` for any other object.
    pub fn spawn_point(&self) -> Option<SpawnPoint> {
        match self {
            LevelObject::Spawn { at, .. } => Some(SpawnPoint { x: at.0, y: at.1 }),
        }
    }
}

/// The level's runtime look, or `None` without one (it draws as it always
/// has). Checks every value. Its glowing pixels are its only light (plan
/// 15c).
pub fn compile_look(src: &LevelSource) -> Result<Option<olr_look::LevelLook>, FormatError> {
    src.look.as_ref().map(|l| compile_look_source(l, LEVEL_FILE)).transpose()
}

/// Each distant layer's surface material as a value in the look's
/// `materials` (issue #41, the renderer's heat): `v` is `materials[v - 1]`
/// (as the surface maps' values), 0 none or an id the look lacks; one per
/// [`LevelSource::distant_surfaces`] entry.
pub fn distant_surface_values(src: &LevelSource) -> Vec<u8> {
    let materials = src.look.as_ref().map_or(&[][..], |l| &l.materials[..]);
    let value = |id: &String| {
        materials.iter().position(|m| m.id == *id).map_or(0, |i| u8::try_from(i + 1).unwrap_or(0))
    };
    src.distant_surfaces.iter().map(|id| id.as_ref().map_or(0, value)).collect()
}

/// The glow images fit the level and name only groups its look has.
pub(crate) fn check_glow(src: &LevelSource, glow: &GlowLayers) -> Result<(), FormatError> {
    let art = (src.size.0 * src.art_scale, src.size.1 * src.art_scale);
    if !glow.fits(art, src.size) {
        return Err(FormatError::invalid(
            GLOW_FILE,
            "",
            format!(
                "the look images must be {}x{} (art) and {}x{} (world)",
                art.0, art.1, src.size.0, src.size.1
            ),
        ));
    }
    let groups = src.look.as_ref().map_or(0, |l| l.glow_groups.len());
    for (file, values) in [(GROUPS_FILE, &glow.groups), (GROUPS_BACK_FILE, &glow.groups_back)] {
        if let Some(i) = values.iter().position(|v| usize::from(*v) > groups) {
            let (x, y) = (i as u32 % src.size.0, i as u32 / src.size.0);
            return Err(FormatError::invalid(
                file,
                "",
                format!("group {} at ({x}, {y}), the look has {groups} glow groups", values[i]),
            ));
        }
    }
    Ok(())
}

/// The surface maps fit the level and name only materials its look has.
pub(crate) fn check_surface(src: &LevelSource, maps: &SurfaceMaps) -> Result<(), FormatError> {
    if !maps.fits(src.size) {
        return Err(FormatError::invalid(
            SURFACE_FILE,
            "",
            format!("the surface maps must be {}x{} (world)", src.size.0, src.size.1),
        ));
    }
    let materials = src.look.as_ref().map_or(0, |l| l.materials.len());
    for (file, values) in [(SURFACE_FILE, &maps.front), (SURFACE_BACK_FILE, &maps.back)] {
        if let Some(i) = values.iter().position(|v| usize::from(*v) > materials) {
            let (x, y) = (i as u32 % src.size.0, i as u32 / src.size.0);
            return Err(FormatError::invalid(
                file,
                "",
                format!("material {} at ({x}, {y}), the look has {materials} materials", values[i]),
            ));
        }
    }
    Ok(())
}

/// The distant layers' glows (issue #62): each of strength 1-100, naming
/// a glow group of the look, its mask (if any) one value per art pixel of
/// its image.
pub(crate) fn check_distant_glows(src: &LevelSource) -> Result<(), FormatError> {
    let groups = src.look.as_ref().map_or(0, |l| l.glow_groups.len());
    for (i, d) in src.distant.iter().enumerate() {
        let Some(g) = &d.glow else { continue };
        if !(1..=100).contains(&g.strength) {
            return Err(FormatError::invalid(
                LEVEL_FILE,
                &format!("distant[{i}].glow"),
                format!("{}: 1 to 100, with a glow_group", g.strength),
            ));
        }
        if g.group == 0 || usize::from(g.group) > groups {
            return Err(FormatError::invalid(
                LEVEL_FILE,
                &format!("distant[{i}].glow_group"),
                format!("names no glow group of the look's {groups}"),
            ));
        }
        let pixels = u64::from(d.image.width()) * u64::from(d.image.height());
        if let Some(m) = &g.mask
            && m.len() as u64 != pixels
        {
            return Err(FormatError::invalid(
                &distant_glow_file(i),
                "",
                format!("{} values, the layer's image has {pixels} pixels", m.len()),
            ));
        }
    }
    Ok(())
}

/// The distant layers' surfaces: at most one per layer, each naming a
/// material of the look.
pub(crate) fn check_distant_surfaces(src: &LevelSource) -> Result<(), FormatError> {
    if src.distant_surfaces.len() > src.distant.len() {
        return Err(FormatError::invalid(
            LEVEL_FILE,
            "distant",
            format!(
                "{} distant surfaces for {} distant layers",
                src.distant_surfaces.len(),
                src.distant.len()
            ),
        ));
    }
    for (i, id) in src.distant_surfaces.iter().enumerate() {
        let Some(id) = id else { continue };
        if !src.look.as_ref().is_some_and(|l| l.materials.iter().any(|m| m.id == *id)) {
            return Err(FormatError::invalid(
                LEVEL_FILE,
                &format!("distant[{i}].surface"),
                format!("no material {id:?} in the look"),
            ));
        }
    }
    Ok(())
}

/// What `level.ron` alone says: size, art scale, objects, and the art size
/// they imply, which must be within what the reader decodes
/// ([`LEVEL_ART`]). Returns the art size.
pub(crate) fn check_header(
    size: (u32, u32),
    art_scale: u32,
    objects: usize,
) -> Result<(u32, u32), FormatError> {
    let (w, h) = size;
    if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
        return Err(FormatError::invalid(
            LEVEL_FILE,
            "size",
            format!("{w}x{h}, not within 1x1 to {MAX_SIDE}x{MAX_SIDE}"),
        ));
    }
    if !(1..=MAX_ART_SCALE).contains(&art_scale) {
        return Err(FormatError::invalid(
            LEVEL_FILE,
            "art_scale",
            format!("{art_scale}, not within 1 to {MAX_ART_SCALE}"),
        ));
    }
    if objects > MAX_OBJECTS {
        return Err(FormatError::invalid(
            LEVEL_FILE,
            "objects",
            format!("{objects} objects, above {MAX_OBJECTS}"),
        ));
    }
    // Both are bounded above, so this cannot overflow.
    let (aw, ah) = (w * art_scale, h * art_scale);
    check_size(aw, ah, LEVEL_ART).map_err(|e| {
        FormatError::invalid(
            LEVEL_FILE,
            "size",
            format!("size {w}x{h} at art_scale {art_scale} makes art of {aw}x{ah}: {e}"),
        )
    })?;
    Ok((aw, ah))
}

/// Whether `thumb.png`'s bytes are within [`MAX_THUMB_BYTES`] and
/// [`MAX_THUMB_SIDE`], from the PNG header alone (nothing is decoded).
pub(crate) fn check_thumb(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() as u64 > MAX_THUMB_BYTES {
        return Err(format!("{} bytes, above {MAX_THUMB_BYTES}", bytes.len()));
    }
    let (w, h) = crate::image_io::png_size(bytes).ok_or("not a PNG")?;
    if w == 0 || h == 0 || w > MAX_THUMB_SIDE || h > MAX_THUMB_SIDE {
        return Err(format!("{w}x{h}, not within 1x1 to {MAX_THUMB_SIDE}x{MAX_THUMB_SIDE}"));
    }
    Ok(())
}

/// The checks `compile_level` and the writer share.
pub(crate) fn check(src: &LevelSource) -> Result<(), FormatError> {
    let (aw, ah) = check_header(src.size, src.art_scale, src.objects.len())?;
    let (w, h) = src.size;
    let cells = w as usize * h as usize;
    if src.material.len() != cells {
        return Err(FormatError::invalid(
            MATERIAL_FILE,
            "material",
            format!("{} cells, the level needs {cells} ({w}x{h})", src.material.len()),
        ));
    }
    if let Some(t) = &src.thumb {
        check_thumb(t).map_err(|m| FormatError::invalid(THUMB_FILE, "", m))?;
    }
    if let Some(c) = &src.clear
        && c.len() != cells
    {
        return Err(FormatError::invalid(
            CLEAR_FILE,
            "",
            format!("{} flags, the level needs {cells} ({w}x{h})", c.len()),
        ));
    }
    if let Some(b) = &src.glow_beams
        && !b.fits(src.size)
    {
        return Err(FormatError::invalid(
            GLOW_BEAM_FILE,
            "",
            format!(
                "the beam maps do not fit the level's {}x{} world pixels (sizes, byte 0 up to 180, byte 1 up to 90)",
                src.size.0, src.size.1
            ),
        ));
    }
    if let Some(values) = &src.haze
        && values.len() != cells
    {
        return Err(FormatError::invalid(
            HAZE_FILE,
            "",
            format!("{} values, the level needs {cells} ({w}x{h})", values.len()),
        ));
    }
    if let Some(g) = &src.glass
        && !g.fits((aw, ah))
    {
        return Err(FormatError::invalid(
            GLASS_FILE,
            "",
            format!("{}x{}, the level's art is {aw}x{ah}", g.art.0, g.art.1),
        ));
    }
    compile_look(src)?;
    if let Some(g) = &src.glow {
        check_glow(src, g)?;
    }
    if let Some(m) = &src.surface {
        check_surface(src, m)?;
    }
    check_distant_layers(&src.distant, src.art_scale)?;
    check_distant_surfaces(src)?;
    check_distant_glows(src)?;
    for (file, img) in [(FRONT_FILE, &src.front), (BACK_FILE, &src.back)] {
        if (img.width(), img.height()) != (aw, ah) {
            return Err(FormatError::invalid(
                file,
                "",
                format!(
                    "{file} is {}x{}, the level needs {aw}x{ah} (size × art_scale)",
                    img.width(),
                    img.height()
                ),
            ));
        }
    }
    Ok(())
}

/// Makes the runtime level.
pub fn compile_level(src: &LevelSource) -> Result<Level, FormatError> {
    check(src)?;
    let spawns = src.objects.iter().filter_map(LevelObject::spawn_point).collect();
    let def = LevelDef {
        name: src.name.clone(),
        width: src.size.0,
        height: src.size.1,
        art_scale: src.art_scale,
        theme: src.theme.clone(),
        spawns,
    };
    let material = MaterialGrid::from_cells(src.size.0, src.size.1, src.material.clone())
        .ok_or_else(|| FormatError::invalid(MATERIAL_FILE, "material", "wrong cell count"))?;
    Ok(Level { def, material, front: src.front.clone(), back: src.back.clone() })
}

/// The inverse of [`compile_level`]; `notes` stay empty.
pub fn lift_level(level: &Level, thumb: Option<Vec<u8>>) -> Result<LevelSource, LiftError> {
    let d = &level.def;
    let src = LevelSource {
        name: d.name.clone(),
        notes: String::new(),
        size: (d.width, d.height),
        art_scale: d.art_scale,
        theme: d.theme.clone(),
        objects: d
            .spawns
            .iter()
            .map(|s| LevelObject::Spawn { at: (s.x, s.y), notes: String::new() })
            .collect(),
        material: level.material.cells().to_vec(),
        front: level.front.clone(),
        back: level.back.clone(),
        thumb,
        look: None,
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
    };
    if (level.material.width(), level.material.height()) != src.size {
        return Err(LiftError {
            item: format!("level {:?}", d.name),
            message: "the material grid is not the level's size".into(),
        });
    }
    check(&src).map_err(|e| LiftError { item: format!("level {:?}", d.name), message: e.to_string() })?;
    Ok(src)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use olr_data::image::Rgba;

    pub(crate) fn tiny(art_scale: u32) -> LevelSource {
        let (w, h) = (3, 2);
        let mut front = RgbaImage::new(w * art_scale, h * art_scale);
        front.set(0, 0, Rgba::rgb(1, 2, 3));
        LevelSource {
            name: "Tiny".into(),
            notes: String::new(),
            size: (w, h),
            art_scale,
            theme: "olx-theme-dirt".into(),
            objects: vec![LevelObject::Spawn { at: (1, 1), notes: "here".into() }],
            material: vec![
                Material::Rock,
                Material::Background,
                Material::Dirt,
                Material::Dirt,
                Material::Background,
                Material::Rock,
            ],
            front,
            back: RgbaImage::new(w * art_scale, h * art_scale),
            thumb: None,
            look: None,
            glow: None,
            clear: None,
            glass: None,
            glow_beams: None,
            haze: None,
            distant: Vec::new(),
            surface: None,
            distant_surfaces: Vec::new(),
        }
    }

    #[test]
    fn compile_and_lift_are_inverse() {
        for scale in [1, 2] {
            let src = tiny(scale);
            let level = compile_level(&src).unwrap();
            assert_eq!(level.def.spawns, vec![olr_data::level::SpawnPoint { x: 1, y: 1 }]);
            assert_eq!(level.material.get(2, 0), Material::Dirt);
            let mut back = lift_level(&level, None).unwrap();
            assert_eq!(back.objects, vec![LevelObject::Spawn { at: (1, 1), notes: String::new() }]);
            back.objects = src.objects.clone(); // notes are authoring-only
            assert_eq!(back, src);
        }
    }

    #[test]
    fn sizes_must_agree() {
        let mut s = tiny(1);
        s.material.pop();
        assert!(compile_level(&s).unwrap_err().to_string().contains("material"));
        let mut s = tiny(2);
        s.front = RgbaImage::new(6, 3);
        let e = compile_level(&s).unwrap_err().to_string();
        assert!(e.contains("front.png") && e.contains("6x4"), "{e}");
        let mut s = tiny(1);
        s.art_scale = 9;
        assert!(compile_level(&s).is_err());
        let mut s = tiny(1);
        s.size = (5000, 1);
        assert!(compile_level(&s).is_err());
    }

    /// A saved file must reopen: the reader decodes art within `LEVEL_ART`,
    /// so the check refuses what it would refuse (the art is a mismatch
    /// here, but the bound is reported first, so no big image is built).
    #[test]
    fn art_above_the_decode_bound_is_refused() {
        let mut s = tiny(8);
        s.size = (1100, 1000);
        s.material = vec![Material::Rock; 1_100_000];
        let e = compile_level(&s).unwrap_err().to_string();
        assert!(e.contains("level.ron") && e.contains("1100x1000") && e.contains("art_scale 8"), "{e}");
        assert!(e.contains("pixels"), "{e}");
        let mut out = olr_vfs::MemFs::new();
        assert!(write_level_files(&mut out, &s).is_err());
        // Just inside: 1000 x 1000 at 8 is 64 Mpx, exactly the bound.
        assert!(check_header((1000, 1000), 8, 0).is_ok());
    }
}
