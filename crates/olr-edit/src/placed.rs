//! A level of placed copies baked (issue #73; the format is
//! `olr_format::levels`' `placed`): each copy posed in its starting state
//! with its values ([`crate::asset::posed`]), stretched to its placed size
//! and resampled to the level's art scale (nearest neighbour), every piece
//! flattened (distant pieces first), the glow groups and materials
//! gathered into the level's look, then baked with the composition's bake
//! (`compose::bake_elements_with_opacity`, the thumbnail). It works on a
//! [`PlacedSource`] in memory; the pack's integration is #74's. Live
//! copies bake as static for now (#85 takes them out).

use std::collections::BTreeMap;
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_data::level::Material;
use olr_format::FormatError;
use olr_format::assets::AssetSource;
use olr_format::image_io::{self, LEVEL_ART};
use olr_format::levels::{
    AssetOrigin, Composition, Element, ElementKind, LevelSource, PlacedCopy, PlacedSource, check_placed,
    element_box, gather_groups, gather_materials, stretch_offset, stretch_span, surface_back_file,
    surface_file,
};
use olr_format::look::{GlowGroupSource, MAX_GROUPS};
use olr_format::materials::MaterialSource;

use crate::asset::{
    ADDED_LIGHTING, Asset, AssetError, Piece, PosedPiece, decode, decode_own, live_refusal, posed,
};
use crate::compose::{Decoded, GlowMasks, PaintedSurfaces, Rect, bake_elements_with_opacity};
use crate::level::{refresh_thumb, rescale_art};

/// Why a placement does not bake.
#[derive(Debug, thiserror::Error)]
pub enum PlacedError {
    /// The placement does not check (`check_placed`).
    #[error(transparent)]
    Format(#[from] FormatError),
    /// An asset does not decode.
    #[error("asset {key:?}: {source}")]
    Asset { key: String, source: AssetError },
    /// A copy cannot be baked: a Live copy's refusal, a value posing
    /// refuses.
    #[error("copy {copy:?}: {message}")]
    Copy { copy: String, message: String },
    /// The level itself: its art beyond the bound, a gathered look that
    /// does not check.
    #[error("{0}")]
    Level(String),
}

/// A glow group or material a copy brought under another id: its id in
/// the asset and in the level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renamed {
    pub copy: String,
    pub from: String,
    pub to: String,
}

/// What the bake's gathering did to the level's look, for callers to
/// print: the glow groups and materials new to it (ids the look before
/// the bake lacked), those renamed, and whether a look was added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Baked {
    pub added_look: bool,
    pub groups_added: Vec<String>,
    pub materials_added: Vec<String>,
    pub groups_renamed: Vec<Renamed>,
    pub materials_renamed: Vec<Renamed>,
}

impl Baked {
    /// What to say of an added look ([`ADDED_LIGHTING`]), if one was.
    pub fn note(&self) -> Option<&'static str> {
        self.added_look.then_some(ADDED_LIGHTING)
    }
}

/// One flattened piece: its element (an id unique in the bake), its data
/// at the level's art scale, its glow mask, painted maps and opacity.
struct Flat {
    element: Element,
    decoded: Decoded,
    glow_mask: Option<Arc<Vec<u8>>>,
    surface_map: Option<Arc<Vec<u8>>>,
    back_surface_map: Option<Arc<Vec<u8>>>,
    opacity: u8,
}

/// `src` (`sw` × `sh` values, row-major) resampled to `dw` × `dh`, nearest
/// neighbour: value `(u, v)` is `(floor(u × sw / dw), floor(v × sh / dh))`.
/// `None` when `src` is not `sw` × `sh`.
fn resample<T: Copy>(src: &[T], (sw, sh): (u32, u32), (dw, dh): (u32, u32)) -> Option<Vec<T>> {
    if src.len() as u64 != u64::from(sw) * u64::from(sh) || sw == 0 || sh == 0 {
        return None;
    }
    let cols: Vec<usize> = (0..dw).map(|u| (u64::from(u) * u64::from(sw) / u64::from(dw)) as usize).collect();
    let mut out = Vec::with_capacity(dw as usize * dh as usize);
    for v in 0..dh {
        let row = (u64::from(v) * u64::from(sh) / u64::from(dh)) as usize * sw as usize;
        out.extend(cols.iter().map(|c| src[row + c]));
    }
    Some(out)
}

/// `img` resampled to `to`, nearest neighbour ([`resample`]).
fn resample_image(img: &RgbaImage, to: (u32, u32)) -> Option<RgbaImage> {
    let (w, h) = (img.width(), img.height());
    let px: Vec<olr_data::image::Rgba> = (0..h).flat_map(|y| (0..w).map(move |x| img.get(x, y))).collect();
    let values = resample(&px, (w, h), to)?;
    let mut out = RgbaImage::new(to.0, to.1);
    for (i, p) in values.into_iter().enumerate() {
        out.set(i as u32 % to.0.max(1), i as u32 / to.0.max(1), p);
    }
    Some(out)
}

/// An image's processed grid expanded to its box's art pixels (`art`), at
/// `pixel_size`: art pixel `(x, y)` is grid pixel `(x / ps, y / ps)`.
fn expanded(grid: &RgbaImage, pixel_size: u32, art: (u32, u32)) -> Option<RgbaImage> {
    let ps = pixel_size.max(1);
    if (grid.width(), grid.height()) != (art.0.div_ceil(ps), art.1.div_ceil(ps)) {
        return None;
    }
    let mut out = RgbaImage::new(art.0, art.1);
    for y in 0..art.1 {
        for x in 0..art.0 {
            out.set(x, y, grid.get(x / ps, y / ps));
        }
    }
    Some(out)
}

/// The copy's frame: the asset's size and art scale, where it is placed,
/// its placed size and the level's art scale.
struct Frame {
    size: (u32, u32),
    art_scale: u32,
    at: (i32, i32),
    to: (u32, u32),
    s: u32,
}

impl Frame {
    /// Whether the pieces' data is kept as it is: unstretched at the
    /// level's art scale.
    fn untouched(&self) -> bool {
        self.to == self.size && self.art_scale == self.s
    }
}

/// A piece's glow mask, surface map and back surface map.
type Maps = [Option<Arc<Vec<u8>>>; 3];

/// Posed piece `p` laid in the level by copy frame `f`: its box mapped
/// (`stretch_span` of its box before the positions moved it, then the
/// move scaled by `stretch_offset`) and its data resampled to the box at
/// the level's art scale (an image's processed grid, drawn art, cells,
/// back art, glow mask, painted maps); kept as it is when `f` is
/// untouched. `None` for a piece stretched to nothing or whose data does
/// not fit it.
fn lay(p: &PosedPiece, f: &Frame) -> Option<(Element, Decoded, Maps)> {
    let Piece { element, decoded, glow_mask, surface_map, back_surface_map, .. } = &p.piece;
    let mut e = element.clone();
    let maps = [glow_mask.clone(), surface_map.clone(), back_surface_map.clone()];
    let Some((b, n)) = element_box(&e.kind, f.size) else {
        // A raster: placed at (0, 0), unstretched (`check_placed`); only
        // its art scale may change.
        let Decoded::Raster { material, front, back } = decoded else { return None };
        let d = Decoded::Raster {
            material: material.clone(),
            front: rescale_art(front, f.art_scale, f.s),
            back: rescale_art(back, f.art_scale, f.s),
        };
        return Some((e, d, maps));
    };
    let base = (i64::from(b.0) - p.offset.0, i64::from(b.1) - p.offset.1);
    let clamp32 = |v: i64| v.clamp(i32::MIN.into(), i32::MAX.into()) as i32;
    let (x, w) = stretch_span(clamp32(base.0), n.0, f.size.0, f.to.0);
    let (y, h) = stretch_span(clamp32(base.1), n.1, f.size.1, f.to.1);
    if w == 0 || h == 0 {
        return None;
    }
    let at = (
        clamp32(i64::from(f.at.0) + x + stretch_offset(p.offset.0, f.size.0, f.to.0)),
        clamp32(i64::from(f.at.1) + y + stretch_offset(p.offset.1, f.size.1, f.to.1)),
    );
    let untouched = f.untouched();
    let (sa, s) = (f.art_scale, f.s);
    let art_from = (n.0.checked_mul(sa)?, n.1.checked_mul(sa)?);
    let art_to = (w.checked_mul(s)?, h.checked_mul(s)?);
    let decoded = match (&mut e.kind, decoded) {
        (ElementKind::Image { at: a, size, pixel_size, .. }, Decoded::Image { grid, mask }) => {
            *a = at;
            *size = (w, h);
            if untouched {
                decoded.clone()
            } else {
                let grid = match grid {
                    Some(g) => Some(Arc::new(resample_image(&expanded(g, *pixel_size, art_from)?, art_to)?)),
                    None => None,
                };
                *pixel_size = 1;
                Decoded::Image { grid, mask: mask.clone() }
            }
        }
        (ElementKind::Drawn { at: a, size, .. }, Decoded::Drawn { art, cells, back }) => {
            *a = at;
            *size = Some((w, h));
            if untouched {
                decoded.clone()
            } else {
                Decoded::Drawn {
                    art: resample_image(art, art_to)?,
                    cells: match cells {
                        Some(c) => Some(resample::<Material>(c, n, (w, h))?),
                        None => None,
                    },
                    back: match back {
                        Some(b) => Some(resample_image(b, art_to)?),
                        None => None,
                    },
                }
            }
        }
        _ => return None,
    };
    if untouched {
        return Some((e, decoded, maps));
    }
    let [glow, front, back] = maps;
    let glow = match glow {
        Some(m) => Some(Arc::new(resample(&m, art_from, art_to)?)),
        None => None,
    };
    let map = |m: Option<Arc<Vec<u8>>>| match m {
        Some(m) => resample(&m, n, (w, h)).map(|v| Some(Arc::new(v))),
        None => Some(None),
    };
    Some((e, decoded, [glow, map(front)?, map(back)?]))
}

/// The asset of `key` decoded: an `Own` one as a level's own
/// ([`decode_own`]), a library copy as a library asset ([`decode`]).
fn decoded_asset(key: &str, origin: &AssetOrigin, src: &AssetSource) -> Result<Asset, PlacedError> {
    let asset = match origin {
        AssetOrigin::Own => decode_own(src),
        _ => decode(src),
    };
    asset.map_err(|source| PlacedError::Asset { key: key.into(), source })
}

/// Copy `c` of `asset` posed (its state, else the asset's start; its
/// values) with a Live copy's refusal (`live_refusal`) an error.
fn posed_copy(c: &PlacedCopy, asset: &Asset) -> Result<crate::asset::Posed, PlacedError> {
    let fail = |message: String| PlacedError::Copy { copy: c.id.clone(), message };
    if c.live
        && let Some(why) = live_refusal(asset)
    {
        return Err(fail(format!("cannot be Live: {why}")));
    }
    let state = match &c.state {
        Some(name) => {
            asset.state(name).ok_or_else(|| fail(format!("{:?} has no state {name:?}", asset.name)))?
        }
        None => asset.start,
    };
    posed(asset, state, &c.values).map_err(|e| fail(e.to_string()))
}

/// Bakes the placement `src` into `level`'s play files (issue #73):
/// checked first (`check_placed`, then each asset decoded: `Own` ones by
/// `check_own_asset`, library copies by `check_asset`); each copy posed
/// in its state (none: the asset's start) with its values (a Live copy as a static one, its
/// `live_refusal` an error naming it); placed in level coordinates (art
/// resampled nearest from the asset's art scale to the level's; a box
/// `[b, b + n)` of an asset `w` wide placed `W` wide becoming `[at +
/// floor(b × W / w), at + floor((b + n) × W / w))`, its data resampled
/// nearest to it; positions scaled by `W / w` rounded half away from
/// zero; a distant copy's pieces distant at its depth); flattened, every
/// distant piece first, then the others, in copy order then piece order,
/// each element's id `<copy id>.<element id>`; the glow groups and
/// materials gathered in copy order ([`gather_groups`]: an id kept, shared
/// with an equal gathered one, or renamed `<id>-2`, …; the pieces and
/// painted maps follow) into the level's look (its own settings, the
/// gathered groups and materials; `LookSource::default()` for a level
/// without one when a copy brings any); then baked as a composition is
/// (`bake_elements_with_opacity`, a piece's opacity mixing its back
/// pixels) with the thumbnail. A ground alone, unstretched at `(0, 0)`
/// with the groups and materials of the level's look, bakes byte for byte
/// as `compose::bake` of its composition.
pub fn bake(
    level: &mut LevelSource,
    src: &PlacedSource,
    back_tile: &RgbaImage,
) -> Result<Baked, PlacedError> {
    let (size, s) = (level.size, level.art_scale);
    let art = size.0.checked_mul(s).zip(size.1.checked_mul(s));
    if !art.is_some_and(|(w, h)| image_io::check_size(w, h, LEVEL_ART).is_ok()) {
        return Err(PlacedError::Level(format!(
            "level.ron: {}x{} at art scale {s}: art beyond the level bound",
            size.0, size.1
        )));
    }
    check_placed(src, size, s)?;
    let mut assets = BTreeMap::new();
    for a in &src.file.assets {
        let Some(asset) = src.assets.get(&a.key) else { continue };
        assets.insert(a.key.as_str(), decoded_asset(&a.key, &a.origin, asset)?);
    }
    let mut groups: Vec<GlowGroupSource> = Vec::new();
    let mut materials: Vec<MaterialSource> = Vec::new();
    let mut baked = Baked::default();
    let (mut distant, mut near): (Vec<Flat>, Vec<Flat>) = (Vec::new(), Vec::new());
    for c in &src.file.copies {
        let Some(asset) = assets.get(c.asset.as_str()) else { continue };
        let p = posed_copy(c, asset)?;
        // The gathering: the copy's ids in the level's look.
        let group_ids = gather_groups(&mut groups, &p.groups);
        let material_ids = gather_materials(&mut materials, &p.materials);
        let renamed = |ids: &[String], olds: Vec<&String>| -> Vec<Renamed> {
            olds.into_iter()
                .zip(ids)
                .filter(|(a, b)| a != b)
                .map(|(a, b)| Renamed { copy: c.id.clone(), from: a.clone(), to: b.clone() })
                .collect()
        };
        baked.groups_renamed.extend(renamed(&group_ids, p.groups.iter().map(|g| &g.id).collect()));
        baked.materials_renamed.extend(renamed(&material_ids, p.materials.iter().map(|m| &m.id).collect()));
        let group_of: BTreeMap<&str, &str> =
            p.groups.iter().map(|g| g.id.as_str()).zip(group_ids.iter().map(String::as_str)).collect();
        let material_of: BTreeMap<&str, &str> =
            p.materials.iter().map(|m| m.id.as_str()).zip(material_ids.iter().map(String::as_str)).collect();
        // A painted value `v` names the asset's `materials[v - 1]`: in the
        // level, the gathered one of its id.
        let mut renumber = [0u8; 256];
        for (i, id) in material_ids.iter().enumerate().take(255) {
            let at = materials.iter().position(|m| m.id == *id);
            renumber[i + 1] = at.and_then(|j| u8::try_from(j + 1).ok()).unwrap_or(0);
        }
        let frame = Frame {
            size: asset.size,
            art_scale: asset.art_scale,
            at: c.at,
            to: c.size.unwrap_or(asset.size),
            s,
        };
        let folders = Composition { elements: Vec::new(), folders: asset.folders.clone() };
        for pp in &p.pieces {
            let Some((mut e, decoded, [glow_mask, front, back])) = lay(pp, &frame) else { continue };
            e = crate::folders::fold(&folders, &e);
            e.folder = None;
            e.id = format!("{}.{}", c.id, e.id);
            if let Some(g) = e.glow.as_mut() {
                g.group = group_of.get(g.group.as_str()).map_or(g.group.clone(), |x| x.to_string());
            }
            for m in [&mut e.surface, &mut e.back_surface].into_iter().flatten() {
                *m = material_of.get(m.as_str()).map_or(m.clone(), |x| x.to_string());
            }
            let remap = |m: Option<Arc<Vec<u8>>>| {
                m.map(|m| Arc::new(m.iter().map(|v| renumber[usize::from(*v)]).collect::<Vec<u8>>()))
            };
            if let Some(depth) = c.distant {
                e.distant = Some(depth);
            }
            let flat = Flat {
                element: e,
                decoded,
                glow_mask,
                surface_map: remap(front),
                back_surface_map: remap(back),
                opacity: pp.opacity,
            };
            if flat.element.is_distant() { distant.push(flat) } else { near.push(flat) }
        }
    }
    if groups.len() > MAX_GROUPS {
        return Err(PlacedError::Level(format!(
            "the copies' glow groups gather to {}, more than {MAX_GROUPS}",
            groups.len()
        )));
    }
    // The look: the level's own settings, the gathered groups and
    // materials.
    let before = level.look.clone();
    if before.is_some() || !groups.is_empty() || !materials.is_empty() {
        baked.added_look = before.is_none();
        let had_group =
            |id: &String| before.as_ref().is_some_and(|l| l.glow_groups.iter().any(|g| g.id == *id));
        let had_material =
            |id: &String| before.as_ref().is_some_and(|l| l.materials.iter().any(|m| m.id == *id));
        baked.groups_added = groups.iter().map(|g| g.id.clone()).filter(|id| !had_group(id)).collect();
        baked.materials_added =
            materials.iter().map(|m| m.id.clone()).filter(|id| !had_material(id)).collect();
        let mut look = before.unwrap_or_default();
        look.glow_groups = groups;
        look.materials = materials;
        olr_format::look::compile_look_source(&look, "level.ron")
            .map_err(|e| PlacedError::Level(e.to_string()))?;
        level.look = Some(look);
    }
    // The level's own files of its size (the bake writes every pixel).
    let cells = size.0 as usize * size.1 as usize;
    let (aw, ah) = (size.0 * s, size.1 * s);
    if level.material.len() != cells {
        level.material = vec![Material::Background; cells];
    }
    for img in [&mut level.front, &mut level.back] {
        if (img.width(), img.height()) != (aw, ah) {
            *img = RgbaImage::new(aw, ah);
        }
    }
    let flat: Vec<Flat> = distant.into_iter().chain(near).collect();
    let mut masks = GlowMasks::new();
    let mut painted = PaintedSurfaces::new();
    for f in &flat {
        let id = &f.element.id;
        if let Some(m) = &f.glow_mask {
            masks.insert(id.clone(), m.clone());
        }
        if let Some(m) = &f.surface_map {
            painted.insert(surface_file(id), m.clone());
        }
        if let Some(m) = &f.back_surface_map {
            painted.insert(surface_back_file(id), m.clone());
        }
    }
    let items: Vec<(&Element, &Decoded)> = flat.iter().map(|f| (&f.element, &f.decoded)).collect();
    let opacity: Vec<u8> = flat.iter().map(|f| f.opacity).collect();
    let all = Rect { x: 0, y: 0, w: size.0, h: size.1 };
    bake_elements_with_opacity(level, &items, &opacity, &masks, &painted, back_tile, all);
    refresh_thumb(level);
    Ok(baked)
}
