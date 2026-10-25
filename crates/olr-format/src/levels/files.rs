//! A level's files. Other paths (`pack.ron`, ...) belong to `olr-pack`
//! and are ignored here.

use olr_data::level::{DistantGlow, DistantLayer, Material};
use olr_vfs::{ContentSink, ContentSource, VPath};
use serde::{Deserialize, Serialize};

use super::{
    BACK_FILE, CLEAR_FILE, FRONT_FILE, GLASS_FILE, GLOW_BACK_FILE, GLOW_FILE, GROUPS_BACK_FILE, GROUPS_FILE,
    LEVEL_FILE, LOOK_FILES, LevelHeader, LevelObject, LevelSource, MATERIAL_FILE, MAX_DISTANT,
    MAX_THUMB_BYTES, SURFACE_BACK_FILE, SURFACE_FILE, SURFACE_FILES, SurfaceMaps, THUMB_FILE, check,
    check_distant_layers, check_header, check_thumb, distant_file, distant_glow_file,
};
use crate::FormatError;
use crate::image_io::{self, IMAGES, LEVEL_ART};
use crate::look::LookSource;
use crate::ron_io::{MAX_FILE_BYTES, from_canonical, read_text, to_canonical, write_text};
use olr_terrain::{GlassMap, GlowLayers};

fn default_scale() -> u32 {
    1
}

fn is_one(v: &u32) -> bool {
    *v == 1
}

/// `level.ron`: what the level is, besides its images.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LevelFile {
    /// Shown in menus.
    name: String,
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    /// Width and height in world pixels, each 1 to `MAX_SIDE` (4096);
    /// `material.png` is this size.
    size: (u32, u32),
    /// Art pixels per world pixel, 1 to `MAX_ART_SCALE` (8): `front.png`
    /// and `back.png` are `size` × `art_scale` (at most 64 Mi pixels).
    #[serde(default = "default_scale", skip_serializing_if = "is_one")]
    art_scale: u32,
    /// The id of the theme pack whose holes and green dirt the level uses.
    theme: String,
    /// Things placed in the level, at most `MAX_OBJECTS` (4096); an editor
    /// addresses them by index.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    objects: Vec<LevelObject>,
    /// The level's look (darkness, light settings, grade, glow groups); none
    /// draws as OLX did. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    look: Option<LookSource>,
    /// The level's distant (parallax) layers, far to near (the element
    /// order: each later one in front of the earlier ones), at most
    /// `MAX_DISTANT` (16); the `i`-th one's image is `distant/<i>.png`.
    /// They show where `back.png` is see-through (alpha 0). Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    distant: Vec<DistantEntry>,
}

/// A distant (parallax) layer (issue #11): its image `distant/<i>.png` is
/// art pixels (the level's art scale; alpha 0 uncovered, anything else
/// covered and drawn opaque), each side a multiple of `art_scale` and at
/// most `MAX_SIDE` (4096) world pixels. It shows through the level's
/// see-through back pixels, scrolling by `1 - depth` of the camera's
/// movement, lit by `1 - depth` of the light beyond the ambient. Purely
/// decoration: no collision material, never in the simulation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DistantEntry {
    /// Percent, 1 (near: moves and is lit almost like the level) to 99
    /// (far: almost still, ambient light only).
    depth: u8,
    /// Its top-left in world pixels with the camera (the view's centre) at
    /// the level's centre; with the camera at `c` it lies at `at + depth ×
    /// (c - size / 2)`. Each coordinate within `MAX_DISTANT_AT` (16384)
    /// of 0.
    at: (i32, i32),
    /// The whole layer's surface material (issue #37): the id of one of
    /// the look's `materials`. Default none. When it heats, explosions
    /// heat the layer as slices of a ball (issue #41): the layer lies
    /// `z = 14 + depth × 1.1` world pixels behind the worms' plane (the
    /// level's own back wall at 14, issue #65), and an explosion of heat
    /// radius `R` warms it in a circle of radius `sqrt(R² − z²)` behind
    /// it, less the further back (no heat at depth 46 and beyond,
    /// past the largest radius, 64). A hot layer glows but casts no light.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    surface: Option<String>,
    /// The layer's glow strength (issue #62): percent, 1 to 100, with
    /// `glow_group`; default none. Its light is baked into the level's
    /// `look/glow-back.png` and `look/groups-back.png` where it covers a
    /// see-through back pixel with the camera at the level's centre (its
    /// reference placement, `at`); its picture is lit by its own glow
    /// wherever it is drawn. A small bright feature (a lit window in a far
    /// city) lights the level as seen from the centre, so with the camera
    /// far from it its light and its drawn place part; a broad source (a
    /// sky, a sunset, moon glow) shows no difference. Its painted mask,
    /// when it has one, is `distant/<i>-glow.png` (grey, the image's size:
    /// 0 no glow to 255 full).
    #[serde(default, skip_serializing_if = "is_zero")]
    glow: u8,
    /// The id of the layer's glow group in the look's `glow_groups` (issue
    /// #62), with `glow`. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glow_group: Option<String>,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

fn write_bytes(sink: &mut dyn ContentSink, file: &str, bytes: &[u8]) -> Result<(), FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    sink.write(&VPath::new(file).map_err(|e| io(e.to_string()))?, bytes).map_err(|e| io(e.to_string()))
}

fn read_bytes(src: &dyn ContentSource, file: &str) -> Result<Vec<u8>, FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    let path = VPath::new(file).map_err(|e| io(e.to_string()))?;
    src.read_max(&path, MAX_FILE_BYTES).map_err(|e| io(e.to_string()))
}

/// The paths the writer writes for `src`, without encoding anything.
pub fn file_paths(src: &LevelSource) -> Vec<String> {
    let mut out: Vec<String> = [LEVEL_FILE, MATERIAL_FILE, FRONT_FILE, BACK_FILE].map(String::from).into();
    if src.thumb.is_some() {
        out.push(THUMB_FILE.into());
    }
    if src.glow.is_some() {
        out.extend(LOOK_FILES.map(String::from));
    }
    if src.clear.is_some() {
        out.push(CLEAR_FILE.into());
    }
    if src.glass.is_some() {
        out.push(GLASS_FILE.into());
    }
    if src.surface.is_some() {
        out.extend(SURFACE_FILES.map(String::from));
    }
    out.extend((0..src.distant.len()).map(distant_file));
    let masked =
        src.distant.iter().enumerate().filter(|(_, d)| d.glow.as_ref().is_some_and(|g| g.mask.is_some()));
    out.extend(masked.map(|(i, _)| distant_glow_file(i)));
    out
}

/// `surfaces` without the `None`s at its end.
fn trimmed(mut surfaces: Vec<Option<String>>) -> Vec<Option<String>> {
    while surfaces.last().is_some_and(Option::is_none) {
        surfaces.pop();
    }
    surfaces
}

pub fn write_level_files(sink: &mut dyn ContentSink, src: &LevelSource) -> Result<(), FormatError> {
    check(src)?;
    let file = LevelFile {
        name: src.name.clone(),
        notes: src.notes.clone(),
        size: src.size,
        art_scale: src.art_scale,
        theme: src.theme.clone(),
        objects: src.objects.clone(),
        look: src.look.clone(),
        distant: src
            .distant
            .iter()
            .enumerate()
            .map(|(i, d)| DistantEntry {
                depth: d.depth,
                at: d.at,
                surface: src.distant_surfaces.get(i).cloned().flatten(),
                glow: d.glow.as_ref().map_or(0, |g| g.strength),
                glow_group: d.glow.as_ref().and_then(|g| group_id(src, g.group)),
            })
            .collect(),
    };
    write_text(sink, LEVEL_FILE, &to_canonical(LEVEL_FILE, &file)?)?;
    write_bytes(sink, MATERIAL_FILE, &super::encode_material(src.size, &src.material))?;
    write_bytes(sink, FRONT_FILE, &image_io::encode_rgba(&src.front))?;
    write_bytes(sink, BACK_FILE, &image_io::encode_rgba(&src.back))?;
    if let Some(t) = &src.thumb {
        write_bytes(sink, THUMB_FILE, t)?;
    }
    if let Some(g) = &src.glow {
        let (art, world) = (g.art, g.world);
        for (file, size, values) in [
            (GLOW_FILE, art, &g.front),
            (GLOW_BACK_FILE, art, &g.back),
            (GROUPS_FILE, world, &g.groups),
            (GROUPS_BACK_FILE, world, &g.groups_back),
        ] {
            write_bytes(sink, file, &image_io::encode_grey(size.0, size.1, values))?;
        }
    }
    if let Some(c) = &src.clear {
        write_bytes(sink, CLEAR_FILE, &image_io::encode_grey(src.size.0, src.size.1, c))?;
    }
    if let Some(g) = &src.glass {
        let image = olr_data::image::RgbaImage::from_raw(g.art.0, g.art.1, g.texels.clone())
            .ok_or_else(|| FormatError::invalid(GLASS_FILE, "", "its texels do not fit its size"))?;
        write_bytes(sink, GLASS_FILE, &image_io::encode_rgba(&image))?;
    }
    if let Some(m) = &src.surface {
        let (w, h) = m.world;
        write_bytes(sink, SURFACE_FILE, &image_io::encode_grey(w, h, &m.front))?;
        write_bytes(sink, SURFACE_BACK_FILE, &image_io::encode_grey(w, h, &m.back))?;
    }
    for (i, d) in src.distant.iter().enumerate() {
        write_bytes(sink, &distant_file(i), &image_io::encode_rgba(&d.image))?;
        if let Some(mask) = d.glow.as_ref().and_then(|g| g.mask.as_ref()) {
            let (w, h) = (d.image.width(), d.image.height());
            write_bytes(sink, &distant_glow_file(i), &image_io::encode_grey(w, h, mask))?;
        }
    }
    Ok(())
}

/// The id of glow group value `group` (1 + its index) in `src`'s look.
fn group_id(src: &LevelSource, group: u8) -> Option<String> {
    let i = usize::from(group).checked_sub(1)?;
    src.look.as_ref()?.glow_groups.get(i).map(|g| g.id.clone())
}

/// The distant layers `entries` name (`distant/<i>.png`), each image's
/// size checked from its header before decoding (`check_distant_layers`
/// then checks them together).
///
/// A layer's glow (issue #62) names its group by id: one the look lacks is
/// group 0, which `check_distant_glows` refuses (the lenient reader plays
/// the layers without glow); its mask, `distant/<i>-glow.png` when present,
/// is read only for a layer that glows and must be the image's size.
fn read_distant(
    src: &dyn ContentSource,
    entries: &[DistantEntry],
    art_scale: u32,
    look: Option<&LookSource>,
) -> Result<Vec<DistantLayer>, FormatError> {
    if entries.len() > MAX_DISTANT {
        return Err(FormatError::invalid(
            LEVEL_FILE,
            "distant",
            format!("{} distant layers, above {MAX_DISTANT}", entries.len()),
        ));
    }
    let mut out = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        let file = distant_file(i);
        let bytes = read_bytes(src, &file)?;
        if let Some((w, h)) = image_io::png_size(&bytes) {
            super::check_distant_size((w, h), art_scale).map_err(|m| FormatError::invalid(&file, "", m))?;
        }
        let image = image_io::decode_rgba_within(&bytes, LEVEL_ART)
            .map_err(|message| FormatError::Io { file: file.clone(), message })?;
        let glow = match (e.glow, &e.glow_group) {
            (0, None) => None,
            (strength, id) => {
                let groups = look.map_or(&[][..], |l| &l.glow_groups[..]);
                let index = id.as_ref().and_then(|id| groups.iter().position(|g| g.id == *id));
                let group = index.and_then(|i| u8::try_from(i + 1).ok()).unwrap_or(0);
                let mask = read_distant_mask(src, i, (image.width(), image.height()))?;
                Some(DistantGlow { group, strength, mask })
            }
        };
        out.push(DistantLayer { image, at: e.at, depth: e.depth, glow });
    }
    Ok(out)
}

/// The `i`-th distant layer's glow mask (`distant/<i>-glow.png`), when
/// present: grey, of the layer's image `size`.
fn read_distant_mask(
    src: &dyn ContentSource,
    i: usize,
    size: (u32, u32),
) -> Result<Option<Vec<u8>>, FormatError> {
    let file = distant_glow_file(i);
    if !VPath::new(&file).is_ok_and(|p| src.exists(&p)) {
        return Ok(None);
    }
    read_grey(src, &file, size).map(Some)
}

/// Reads an 8-bit grey look image, checking its header size first.
fn read_grey(src: &dyn ContentSource, file: &str, expected: (u32, u32)) -> Result<Vec<u8>, FormatError> {
    let bytes = read_bytes(src, file)?;
    let mismatch = |(w, h): (u32, u32)| {
        FormatError::invalid(
            file,
            "",
            format!("{file} is {w}x{h}, the level needs {}x{}", expected.0, expected.1),
        )
    };
    if let Some(size) = image_io::png_size(&bytes)
        && size != expected
    {
        return Err(mismatch(size));
    }
    let (w, h, values) = image_io::decode_grey_within(&bytes, LEVEL_ART)
        .map_err(|message| FormatError::Io { file: file.into(), message })?;
    if (w, h) != expected {
        return Err(mismatch((w, h)));
    }
    Ok(values)
}

/// The four `look/` images: all or none.
fn read_glow(
    src: &dyn ContentSource,
    art: (u32, u32),
    world: (u32, u32),
) -> Result<Option<GlowLayers>, FormatError> {
    let present = |f: &str| VPath::new(f).is_ok_and(|p| src.exists(&p));
    if !LOOK_FILES.iter().any(|f| present(f)) {
        return Ok(None);
    }
    if let Some(missing) = LOOK_FILES.iter().find(|f| !present(f)) {
        return Err(FormatError::invalid(missing, "", "missing: a level's glow needs all four look/ images"));
    }
    Ok(Some(GlowLayers {
        art,
        world,
        front: read_grey(src, GLOW_FILE, art)?,
        back: read_grey(src, GLOW_BACK_FILE, art)?,
        groups: read_grey(src, GROUPS_FILE, world)?,
        groups_back: read_grey(src, GROUPS_BACK_FILE, world)?,
    }))
}

/// The level's art size (its size × art scale, saturating).
fn art_size(level: &LevelSource) -> (u32, u32) {
    (level.size.0.saturating_mul(level.art_scale), level.size.1.saturating_mul(level.art_scale))
}

/// `look/clear.png`, when present.
fn read_clear(src: &dyn ContentSource, world: (u32, u32)) -> Result<Option<Vec<u8>>, FormatError> {
    if !VPath::new(CLEAR_FILE).is_ok_and(|p| src.exists(&p)) {
        return Ok(None);
    }
    read_grey(src, CLEAR_FILE, world).map(Some)
}

/// `look/glass.png`, when present, at art size `art`.
fn read_glass(src: &dyn ContentSource, art: (u32, u32)) -> Result<Option<GlassMap>, FormatError> {
    if !VPath::new(GLASS_FILE).is_ok_and(|p| src.exists(&p)) {
        return Ok(None);
    }
    let image = read_art(src, GLASS_FILE, art)?;
    Ok(Some(GlassMap { art, texels: image.as_raw().to_vec() }))
}

/// The two surface maps (issue #37): both or none.
fn read_surface(src: &dyn ContentSource, world: (u32, u32)) -> Result<Option<SurfaceMaps>, FormatError> {
    let present = |f: &str| VPath::new(f).is_ok_and(|p| src.exists(&p));
    if !SURFACE_FILES.iter().any(|f| present(f)) {
        return Ok(None);
    }
    if let Some(missing) = SURFACE_FILES.iter().find(|f| !present(f)) {
        return Err(FormatError::invalid(missing, "", "missing: a level's surface needs both surface maps"));
    }
    Ok(Some(SurfaceMaps {
        world,
        front: read_grey(src, SURFACE_FILE, world)?,
        back: read_grey(src, SURFACE_BACK_FILE, world)?,
    }))
}

/// Reads an art PNG, checking its header size against `level.ron`'s before
/// decoding.
fn read_art(
    src: &dyn ContentSource,
    file: &str,
    expected: (u32, u32),
) -> Result<olr_data::image::RgbaImage, FormatError> {
    let bytes = read_bytes(src, file)?;
    if let Some((w, h)) = image_io::png_size(&bytes)
        && (w, h) != expected
    {
        return Err(FormatError::invalid(
            file,
            "",
            format!("{file} is {w}x{h}, {LEVEL_FILE}'s size × art_scale is {}x{}", expected.0, expected.1),
        ));
    }
    image_io::decode_rgba_within(&bytes, LEVEL_ART)
        .map_err(|message| FormatError::Io { file: file.into(), message })
}

/// Reads and validates `level.ron` only; no image is touched.
pub fn read_level_header(src: &dyn ContentSource) -> Result<LevelHeader, FormatError> {
    let file = read_level_ron(src)?;
    check_header(file.size, file.art_scale, file.objects.len())?;
    Ok(LevelHeader {
        name: file.name,
        size: file.size,
        art_scale: file.art_scale,
        theme: file.theme,
        spawns: file.objects.iter().filter_map(LevelObject::spawn_point).collect(),
        objects: file.objects,
        look: file.look,
    })
}

fn read_level_ron(src: &dyn ContentSource) -> Result<LevelFile, FormatError> {
    from_canonical(LEVEL_FILE, &read_text(src, LEVEL_FILE)?)
}

/// The thumbnail, tolerantly: absent, above [`MAX_THUMB_BYTES`] or
/// `MAX_THUMB_SIDE` (by the PNG header) or not a decodable PNG all give
/// `None`. The bytes are passed through.
fn read_thumb(src: &dyn ContentSource) -> Option<Vec<u8>> {
    let path = VPath::new(THUMB_FILE).ok()?;
    if !src.exists(&path) {
        return None;
    }
    let bytes = src.read_max(&path, MAX_THUMB_BYTES).ok()?;
    // The header says the size: a large image is never decoded.
    check_thumb(&bytes).ok()?;
    image_io::decode_rgba_within(&bytes, IMAGES).ok()?;
    Some(bytes)
}

/// The level with its thumbnail read tolerantly (never an error).
pub fn read_level_files(src: &dyn ContentSource) -> Result<LevelSource, FormatError> {
    let mut level = read_level_files_without_thumb(src)?;
    level.thumb = read_thumb(src);
    Ok(level)
}

/// The level with `thumb` always `None`: the thumbnail is never read.
pub fn read_level_files_without_thumb(src: &dyn ContentSource) -> Result<LevelSource, FormatError> {
    let (mut level, glow, distant) = read_base(src)?;
    level.glow = glow?;
    level.clear = read_clear(src, level.size)?;
    level.glass = read_glass(src, art_size(&level))?;
    level.surface = read_surface(src, level.size)?;
    level.distant = distant?;
    check(&level)?;
    Ok(level)
}

/// [`read_level_files_without_thumb`] for playing: a look that does not
/// check, or glow or clear images that do not read or fit, are left out
/// with a warning each instead of failing the level (a broken look never keeps a
/// level from being played; `pack check` still reports it).
pub fn read_level_files_lenient(src: &dyn ContentSource) -> Result<(LevelSource, Vec<String>), FormatError> {
    let (mut level, glow, distant) = read_base(src)?;
    let mut warnings = Vec::new();
    level.distant =
        distant.and_then(|d| check_distant_layers(&d, level.art_scale).map(|()| d)).unwrap_or_else(|e| {
            warnings.push(format!("{e}: the level plays without its distant layers"));
            Vec::new()
        });
    if level.distant.is_empty() {
        level.distant_surfaces.clear();
    }
    level.glow = glow.unwrap_or_else(|e| {
        warnings.push(format!("{e}: the level plays without glow"));
        None
    });
    level.clear = read_clear(src, level.size).unwrap_or_else(|e| {
        warnings.push(format!("{e}: the level plays without clear solids"));
        None
    });
    level.glass = read_glass(src, art_size(&level)).unwrap_or_else(|e| {
        warnings.push(format!("{e}: the level plays without its glass"));
        None
    });
    level.surface = read_surface(src, level.size).unwrap_or_else(|e| {
        warnings.push(format!("{e}: the level plays without surface materials"));
        None
    });
    if let Err(e) = super::compile_look(&level) {
        warnings.push(format!("{e}: the level plays without its look"));
        level.look = None;
        level.objects.retain(|o| matches!(o, LevelObject::Spawn { .. }));
        level.glow = None;
        level.surface = None;
        level.distant_surfaces.clear();
        level.distant.iter_mut().for_each(|d| d.glow = None);
    }
    if let Some(g) = &level.glow
        && let Err(e) = super::check_glow(&level, g)
    {
        warnings.push(format!("{e}: the level plays without glow"));
        level.glow = None;
    }
    if let Some(m) = &level.surface
        && let Err(e) = super::check_surface(&level, m)
    {
        warnings.push(format!("{e}: the level plays without surface materials"));
        level.surface = None;
    }
    if let Err(e) = super::check_distant_glows(&level) {
        warnings.push(format!("{e}: the level's distant layers play without glow"));
        level.distant.iter_mut().for_each(|d| d.glow = None);
    }
    if let Err(e) = super::check_distant_surfaces(&level) {
        warnings.push(format!("{e}: the level's distant layers play without surface materials"));
        level.distant_surfaces.clear();
    }
    check(&level)?;
    Ok((level, warnings))
}

/// `level.ron`, the material and the art, checked; the glow images' and
/// the distant layers' reads apart, so a caller decides what a bad one
/// means.
#[allow(clippy::type_complexity)]
fn read_base(
    src: &dyn ContentSource,
) -> Result<
    (LevelSource, Result<Option<GlowLayers>, FormatError>, Result<Vec<DistantLayer>, FormatError>),
    FormatError,
> {
    let file = read_level_ron(src)?;
    // level.ron is validated before any image is read.
    let art = check_header(file.size, file.art_scale, file.objects.len())?;
    let bytes = read_bytes(src, MATERIAL_FILE)?;
    if let Some((w, h)) = image_io::png_size(&bytes)
        && (w, h) != file.size
    {
        return Err(FormatError::invalid(
            MATERIAL_FILE,
            "",
            format!("{MATERIAL_FILE} is {w}x{h}, {LEVEL_FILE}'s size is {}x{}", file.size.0, file.size.1),
        ));
    }
    let (w, h, indices) = image_io::decode_indexed_within(&bytes, LEVEL_ART)
        .map_err(|message| FormatError::Io { file: MATERIAL_FILE.into(), message })?;
    if (w, h) != file.size {
        return Err(FormatError::invalid(
            MATERIAL_FILE,
            "",
            format!("{MATERIAL_FILE} is {w}x{h}, {LEVEL_FILE}'s size is {}x{}", file.size.0, file.size.1),
        ));
    }
    let mut material = Vec::with_capacity(indices.len());
    for (i, &v) in indices.iter().enumerate() {
        let m = Material::from_index(v).ok_or_else(|| {
            let (x, y) = (i as u32 % w.max(1), i as u32 / w.max(1));
            FormatError::invalid(MATERIAL_FILE, "", format!("unknown material index {v} at ({x}, {y})"))
        })?;
        material.push(m);
    }
    let front = read_art(src, FRONT_FILE, art)?;
    let back = read_art(src, BACK_FILE, art)?;
    let glow = read_glow(src, art, file.size);
    let distant = read_distant(src, &file.distant, file.art_scale, file.look.as_ref());
    let distant_surfaces = trimmed(file.distant.iter().map(|e| e.surface.clone()).collect());
    let level = LevelSource {
        name: file.name,
        notes: file.notes,
        size: file.size,
        art_scale: file.art_scale,
        theme: file.theme,
        objects: file.objects,
        material,
        front,
        back,
        thumb: None,
        look: file.look,
        glow: None,
        clear: None,
        glass: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces,
    };
    Ok((level, glow, distant))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levels::MAX_THUMB_SIDE;
    use olr_data::level::SpawnPoint;
    use olr_vfs::MemFs;

    #[test]
    fn files_round_trip_byte_stably() {
        let mut src = crate::levels::tests::tiny(2);
        src.thumb = Some(image_io::encode_rgba(&olr_data::image::RgbaImage::new(2, 2)));
        let mut a = MemFs::new();
        write_level_files(&mut a, &src).unwrap();
        let names: Vec<String> = a.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(names, ["back.png", "front.png", "level.ron", "material.png", "thumb.png"]);
        let back = read_level_files(&a).unwrap();
        assert_eq!(back, src);
        let mut b = MemFs::new();
        write_level_files(&mut b, &back).unwrap();
        assert_eq!(a, b);
    }

    fn with_thumb(bytes: Vec<u8>) -> MemFs {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &crate::levels::tests::tiny(1)).unwrap();
        fs.with("thumb.png", bytes)
    }

    #[test]
    fn a_thumbnail_that_is_too_big_or_undecodable_is_left_out() {
        let good = image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 4));
        assert_eq!(read_level_files(&with_thumb(good.clone())).unwrap().thumb, Some(good));
        let big = vec![0u8; 2 << 20];
        assert_eq!(read_level_files(&with_thumb(big)).unwrap().thumb, None);
        // A header past the side bound is refused without decoding (the
        // body is not even valid PNG data).
        let mut wide = image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 4));
        wide[16..20].copy_from_slice(&(MAX_THUMB_SIDE + 1).to_be_bytes());
        assert!(check_thumb(&wide).is_err());
        assert_eq!(read_level_files(&with_thumb(wide)).unwrap().thumb, None);
        // The writer refuses what the reader would leave out.
        let mut src = crate::levels::tests::tiny(1);
        src.thumb = Some(b"not a png".to_vec());
        assert!(write_level_files(&mut MemFs::new(), &src).is_err());
        assert_eq!(read_level_files(&with_thumb(b"not a png".to_vec())).unwrap().thumb, None);
    }

    #[test]
    fn the_read_without_thumb_ignores_the_thumbnail() {
        let good = image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 4));
        assert_eq!(read_level_files_without_thumb(&with_thumb(good)).unwrap().thumb, None);
    }

    #[test]
    fn the_header_read_touches_no_image() {
        let fs = MemFs::new().with(
            "level.ron",
            b"(name: \"x\", size: (8, 6), art_scale: 2, theme: \"t\", objects: [Spawn(at: (1, 2))])\n"
                .to_vec(),
        );
        let h = read_level_header(&fs).unwrap();
        assert_eq!((h.name.as_str(), h.size, h.art_scale, h.theme.as_str()), ("x", (8, 6), 2, "t"));
        assert_eq!(h.spawns, [SpawnPoint { x: 1, y: 2 }]);
        // It validates level.ron all the same.
        let bad = MemFs::new().with("level.ron", b"(name: \"x\", size: (0, 6), theme: \"t\")\n".to_vec());
        assert!(read_level_header(&bad).unwrap_err().to_string().contains("size"));
    }

    #[test]
    fn a_bad_material_index_names_the_pixel() {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &crate::levels::tests::tiny(1)).unwrap();
        let bad = image_io::encode_indexed(3, 2, &[0, 1, 2, 7, 1, 0], &[0; 24]);
        let fs = fs.with("material.png", bad);
        let e = read_level_files(&fs).unwrap_err().to_string();
        assert!(e.contains("material.png") && e.contains("index 7 at (0, 1)"), "{e}");
    }

    #[test]
    fn level_ron_is_validated_and_sizes_compared_before_decoding() {
        // An absurd size in level.ron is refused with no image present.
        let fs = MemFs::new()
            .with("level.ron", b"(name: \"x\", size: (4096, 4096), art_scale: 8, theme: \"t\")\n".to_vec());
        let e = read_level_files(&fs).unwrap_err().to_string();
        assert!(e.contains("level.ron") && e.contains("art_scale 8"), "{e}");
        // A bomb front.png (huge header) is refused naming both files.
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &crate::levels::tests::tiny(1)).unwrap();
        let fs = fs.with("front.png", image_io::declared_png(8, 8, false));
        let e = read_level_files(&fs).unwrap_err().to_string();
        assert!(e.contains("front.png") && e.contains("level.ron") && e.contains("8x8"), "{e}");
    }

    #[test]
    fn level_ron_is_short() {
        let mut fs = MemFs::new();
        let mut src = crate::levels::tests::tiny(1);
        src.objects.clear();
        write_level_files(&mut fs, &src).unwrap();
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "level.ron").unwrap().1.to_vec())
            .unwrap();
        assert!(
            !text.contains("art_scale") && !text.contains("objects") && !text.contains("notes"),
            "{text}"
        );
    }

    fn with_distant(art_scale: u32) -> LevelSource {
        let mut src = crate::levels::tests::tiny(art_scale);
        let mut image = olr_data::image::RgbaImage::new(2 * art_scale, art_scale);
        image.set(0, 0, olr_data::image::Rgba::rgb(9, 8, 7));
        src.distant = vec![
            DistantLayer { image: image.clone(), at: (-3, 1), depth: 40, glow: None },
            DistantLayer { image, at: (0, 0), depth: 99, glow: None },
        ];
        src
    }

    #[test]
    fn distant_layers_round_trip() {
        let src = with_distant(2);
        let mut a = MemFs::new();
        write_level_files(&mut a, &src).unwrap();
        let names: Vec<String> = a.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(
            names,
            ["back.png", "distant/0.png", "distant/1.png", "front.png", "level.ron", "material.png"]
        );
        let mut paths = file_paths(&src);
        paths.sort();
        assert_eq!(paths, names);
        let back = read_level_files(&a).unwrap();
        assert_eq!(back, src);
        let (lenient, warnings) = read_level_files_lenient(&a).unwrap();
        assert_eq!((lenient, warnings), (src, Vec::new()));
    }

    #[test]
    fn bad_distant_layers_are_errors_and_played_without() {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &with_distant(2)).unwrap();
        // An image that is not a multiple of the art scale.
        let odd =
            fs.clone().with("distant/1.png", image_io::encode_rgba(&olr_data::image::RgbaImage::new(3, 2)));
        let e = read_level_files(&odd).unwrap_err().to_string();
        assert!(e.contains("distant/1.png") && e.contains("multiple of the art scale 2"), "{e}");
        let (level, warnings) = read_level_files_lenient(&odd).unwrap();
        assert!(level.distant.is_empty());
        assert!(warnings[0].contains("without its distant layers"), "{warnings:?}");
        // A missing image.
        let mut missing = MemFs::new();
        for (p, b) in fs.files().filter(|(p, _)| p.as_str() != "distant/0.png") {
            missing = missing.with(p.as_str(), b.to_vec());
        }
        assert!(read_level_files(&missing).unwrap_err().to_string().contains("distant/0.png"));
        assert!(read_level_files_lenient(&missing).unwrap().0.distant.is_empty());
        // A depth out of range, and a place too far, are refused by the
        // writer and the reader alike.
        for (depth, at, field) in [
            (0, (0, 0), "distant[0].depth"),
            (100, (0, 0), "distant[0].depth"),
            (50, (0, -16385), "distant[0].at"),
        ] {
            let mut src = with_distant(1);
            src.distant[0].depth = depth;
            src.distant[0].at = at;
            let e = write_level_files(&mut MemFs::new(), &src).unwrap_err().to_string();
            assert!(e.contains(field), "{e}");
            let text = format!(
                "(name: \"x\", size: (3, 2), art_scale: 2, theme: \"t\", distant: [(depth: {depth}, at: ({}, {}))])\n",
                at.0, at.1
            );
            let bad = fs.clone().with("level.ron", text.into_bytes());
            let e = read_level_files(&bad).unwrap_err().to_string();
            assert!(e.contains(field) || e.contains("distant"), "{e}");
        }
        // Too many.
        let mut src = with_distant(1);
        src.distant = vec![src.distant[0].clone(); MAX_DISTANT + 1];
        assert!(write_level_files(&mut MemFs::new(), &src).unwrap_err().to_string().contains("above 16"));
    }

    /// Issue #62: a distant layer's glow (strength, group by id, painted
    /// mask) round-trips; a group the look lacks or a mask of the wrong
    /// size is refused, and played without.
    #[test]
    fn distant_glows_round_trip_and_name_the_looks_groups() {
        let mut src = with_distant(2);
        let group = |id: &str| crate::look::GlowGroupSource {
            id: id.into(),
            colour: Default::default(),
            intensity: 100,
            reach: crate::look::ReachSource::Unlimited,
            animation: Default::default(),
            phase_ms: None,
        };
        src.look = Some(crate::look::LookSource {
            glow_groups: vec![group("lamp"), group("sky")],
            ..crate::look::LookSource::default()
        });
        let (w, h) = (src.distant[1].image.width(), src.distant[1].image.height());
        src.distant[0].glow = Some(DistantGlow { group: 2, strength: 60, mask: None });
        src.distant[1].glow = Some(DistantGlow {
            group: 1,
            strength: 100,
            mask: Some((0..w * h).map(|v| (v * 50) as u8).collect()),
        });
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &src).unwrap();
        let mut names: Vec<String> = fs.files().map(|(p, _)| p.to_string()).collect();
        assert!(
            names.contains(&"distant/1-glow.png".to_string())
                && !names.contains(&"distant/0-glow.png".to_string())
        );
        let mut paths = file_paths(&src);
        paths.sort();
        names.sort();
        assert_eq!(paths, names);
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "level.ron").unwrap().1.to_vec())
            .unwrap();
        assert!(text.contains("glow: 60") && text.contains("glow_group: \"sky\""), "{text}");
        assert_eq!(read_level_files(&fs).unwrap(), src);
        assert_eq!(read_level_files_lenient(&fs).unwrap(), (src.clone(), Vec::new()));
        // A group the look lacks.
        let unknown = text.replace("glow_group: \"sky\"", "glow_group: \"moon\"");
        let bad = fs.clone().with("level.ron", unknown.into_bytes());
        let e = read_level_files(&bad).unwrap_err().to_string();
        assert!(e.contains("distant[0].glow_group"), "{e}");
        let (level, warnings) = read_level_files_lenient(&bad).unwrap();
        assert!(level.distant.len() == 2 && level.distant.iter().all(|d| d.glow.is_none()));
        assert!(warnings[0].contains("without glow"), "{warnings:?}");
        // A strength out of range, a mask of the wrong size.
        let mut wrong = src.clone();
        wrong.distant[0].glow.as_mut().unwrap().strength = 101;
        assert!(
            write_level_files(&mut MemFs::new(), &wrong).unwrap_err().to_string().contains("distant[0].glow")
        );
        let small = fs.clone().with("distant/1-glow.png", image_io::encode_grey(1, 1, &[255]));
        assert!(read_level_files(&small).unwrap_err().to_string().contains("distant/1-glow.png"));
    }

    #[test]
    fn a_level_without_distant_layers_writes_none() {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &crate::levels::tests::tiny(1)).unwrap();
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "level.ron").unwrap().1.to_vec())
            .unwrap();
        assert!(!text.contains("distant"), "{text}");
    }

    /// `tiny` with a look of two materials and maps naming both.
    fn with_surface() -> LevelSource {
        let mut src = crate::levels::tests::tiny(1);
        let material = |id: &str| crate::materials::MaterialSource {
            light_soak: None,
            id: id.into(),
            name: id.into(),
            notes: String::new(),
            heat: None,
        };
        src.look = Some(crate::look::LookSource {
            materials: vec![material("stone"), material("wood")],
            ..crate::look::LookSource::default()
        });
        src.surface = Some(SurfaceMaps { world: (3, 2), front: vec![1, 0, 2, 2, 0, 1], back: vec![2; 6] });
        src
    }

    #[test]
    fn surface_maps_round_trip() {
        let src = with_surface();
        let mut a = MemFs::new();
        write_level_files(&mut a, &src).unwrap();
        let names: Vec<String> = a.files().map(|(p, _)| p.to_string()).collect();
        assert_eq!(
            names,
            [
                "back.png",
                "front.png",
                "level.ron",
                "look/surface-back.png",
                "look/surface.png",
                "material.png"
            ]
        );
        let mut paths = file_paths(&src);
        paths.sort();
        assert_eq!(paths, names);
        assert_eq!(read_level_files(&a).unwrap(), src);
        assert_eq!(read_level_files_lenient(&a).unwrap(), (src, Vec::new()));
    }

    #[test]
    fn bad_surface_maps_are_errors_and_played_without() {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &with_surface()).unwrap();
        let grey = |w, h, v: &[u8]| image_io::encode_grey(w, h, v);
        let cases = [
            // The wrong size.
            (fs.clone().with(SURFACE_FILE, grey(2, 2, &[0; 4])), "look/surface.png"),
            // A value past the look's two materials.
            (fs.clone().with(SURFACE_BACK_FILE, grey(3, 2, &[0, 0, 0, 0, 3, 0])), "material 3 at (1, 1)"),
        ];
        for (bad, want) in cases {
            let e = read_level_files(&bad).unwrap_err().to_string();
            assert!(e.contains(want), "{e}");
            let (level, warnings) = read_level_files_lenient(&bad).unwrap();
            assert_eq!(level.surface, None);
            assert!(warnings[0].contains("without surface materials"), "{warnings:?}");
        }
        // One map without the other.
        let mut one = MemFs::new();
        for (p, b) in fs.files().filter(|(p, _)| p.as_str() != SURFACE_BACK_FILE) {
            one = one.with(p.as_str(), b.to_vec());
        }
        let e = read_level_files(&one).unwrap_err().to_string();
        assert!(e.contains("surface-back.png") && e.contains("both"), "{e}");
        // The writer refuses what the reader would.
        let mut src = with_surface();
        src.surface.as_mut().unwrap().front[0] = 3;
        assert!(write_level_files(&mut MemFs::new(), &src).is_err());
        src.surface = Some(SurfaceMaps::empty((2, 2)));
        assert!(write_level_files(&mut MemFs::new(), &src).is_err());
        // Without a look the maps name nothing, and play without.
        let mut src = with_surface();
        src.look = None;
        assert!(write_level_files(&mut MemFs::new(), &src).is_err());
    }

    #[test]
    fn distant_surfaces_round_trip_and_name_the_looks_materials() {
        let mut src = with_distant(1);
        src.look = with_surface().look;
        src.distant_surfaces = vec![None, Some("wood".into())];
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &src).unwrap();
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "level.ron").unwrap().1.to_vec())
            .unwrap();
        assert!(text.contains("surface: \"wood\"") && text.matches("surface").count() == 1, "{text}");
        assert_eq!(read_level_files(&fs).unwrap(), src);
        // Issue #41: as values in the look's materials (wood is the second).
        let ids: Vec<_> = src.look.as_ref().unwrap().materials.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.iter().position(|&i| i == "wood"), Some(1), "{ids:?}");
        assert_eq!(super::super::distant_surface_values(&src), [0, 2]);
        // An id the look lacks: refused, and played without.
        let unknown = text.replace("surface: \"wood\"", "surface: \"lava\"");
        let bad = fs.clone().with("level.ron", unknown.into_bytes());
        let e = read_level_files(&bad).unwrap_err().to_string();
        assert!(e.contains("distant[1].surface") && e.contains("lava"), "{e}");
        let (level, warnings) = read_level_files_lenient(&bad).unwrap();
        assert!(level.distant_surfaces.is_empty() && level.distant.len() == 2);
        assert!(warnings[0].contains("without surface materials"), "{warnings:?}");
        // More surfaces than layers.
        src.distant_surfaces.push(Some("stone".into()));
        assert!(write_level_files(&mut MemFs::new(), &src).is_err());
    }

    #[test]
    fn a_level_without_materials_writes_no_surface() {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, &with_distant(1)).unwrap();
        assert!(fs.files().all(|(p, _)| !p.as_str().contains("surface")));
        let text = String::from_utf8(fs.files().find(|(p, _)| p.as_str() == "level.ron").unwrap().1.to_vec())
            .unwrap();
        assert!(!text.contains("surface") && !text.contains("materials"), "{text}");
    }
}
