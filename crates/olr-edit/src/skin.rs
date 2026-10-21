//! Skin operations (issue #13), shared by `olr-cli skin …` and the skin
//! editor: a blank skin, a classic skin doubled by Scale2x, a skin pack
//! made from either or copied from another, checked, and previewed.

use std::path::Path;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::manifest::PackKind;
use olr_data::skin::{GLOW_LEVELS, MaskKind, Skin, glow_level};
use olr_format::skins::{DOUBLE_SKIN, OLX_SKIN, SkinSource};
use olr_pack::Severity;
use olr_vfs::DirFs;

use crate::new::{NewError, Template, new_pack_with};
use crate::pack::Source;

/// Why a skin was not doubled.
#[derive(Debug, thiserror::Error)]
pub enum SkinError {
    /// Only OLX's layout doubles into one the runtime draws.
    #[error(
        "only OLX's layout (art scale 2, 20 × 18 frames) doubles; this skin is art scale {art_scale}, \
         {}x{} frames",
        .frame_size.0,
        .frame_size.1
    )]
    NotClassic { art_scale: u32, frame_size: (u32, u32) },
    /// The source's images do not fit its layout.
    #[error("{0} does not fit the skin's layout")]
    Size(&'static str),
}

/// A skin of nothing: every pixel transparent, no glow or light; OLX's
/// layout, or double resolution's when `double`.
pub fn blank(name: &str, double: bool) -> SkinSource {
    let (art_scale, frame_size, aim_frames, walk_frames) = if double { DOUBLE_SKIN } else { OLX_SKIN };
    let (w, h) = (frame_size.0 * aim_frames, frame_size.1 * walk_frames);
    SkinSource {
        name: name.into(),
        notes: String::new(),
        art_scale,
        frame_size,
        aim_frames,
        walk_frames,
        image: RgbaImage::new(w, h),
        tint: vec![0; w as usize * h as usize],
        glow: None,
        light: None,
        left: None,
    }
}

/// A transparent pixel as the format lifts it: every channel 0.
fn plain(p: Rgba) -> Rgba {
    if p.a == 0 { Rgba::default() } else { p }
}

/// For each art pixel of the doubled grid (row by row), the source pixel
/// it copies: Scale2x within each `frame` of a `w` × `h` grid, comparing
/// pixels by `key`; a frame's edge counts its own pixel as the neighbour
/// beyond it, so no frame takes from another. Where the neighbours above
/// and below or left and right are alike, all four copy the source pixel.
fn scale2x<K: PartialEq>(
    w: u32,
    h: u32,
    (fw, fh): (u32, u32),
    key: impl Fn(u32, u32) -> K,
) -> Vec<(u32, u32)> {
    let (w2, h2) = (2 * w as usize, 2 * h as usize);
    let mut out = vec![(0, 0); w2 * h2];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x % fw, y % fh);
            let e = (x, y);
            let b = if fy > 0 { (x, y - 1) } else { e };
            let h_ = if fy + 1 < fh { (x, y + 1) } else { e };
            let d = if fx > 0 { (x - 1, y) } else { e };
            let f = if fx + 1 < fw { (x + 1, y) } else { e };
            let k = |p: (u32, u32)| key(p.0, p.1);
            let (kb, kh, kd, kf) = (k(b), k(h_), k(d), k(f));
            let quad = if kb != kh && kd != kf {
                [
                    if kd == kb { d } else { e },
                    if kb == kf { f } else { e },
                    if kd == kh { d } else { e },
                    if kh == kf { f } else { e },
                ]
            } else {
                [e; 4]
            };
            let (x2, y2) = (2 * x as usize, 2 * y as usize);
            out[y2 * w2 + x2] = quad[0];
            out[y2 * w2 + x2 + 1] = quad[1];
            out[(y2 + 1) * w2 + x2] = quad[2];
            out[(y2 + 1) * w2 + x2 + 1] = quad[3];
        }
    }
    out
}

/// A classic (OLX-layout) skin turned double resolution as a starting
/// point to refine: every frame by Scale2x (Andrea Mazzoleni's EPX
/// rules). An art pixel takes its colour, tint and glow from the one
/// source pixel it copies, and pixels compare by all three, so a tinted
/// edge stays tinted; where Scale2x keeps the source pixel that is
/// nearest scaling. `left.png`, if any, doubles by its own colours. The
/// name, notes and light are kept.
pub fn double(src: &SkinSource) -> Result<SkinSource, SkinError> {
    if (src.art_scale, src.frame_size, src.aim_frames, src.walk_frames) != OLX_SKIN {
        return Err(SkinError::NotClassic { art_scale: src.art_scale, frame_size: src.frame_size });
    }
    let (fw, fh) = src.frame_size;
    let (w, h) = (fw * src.aim_frames, fh * src.walk_frames);
    let n = w as usize * h as usize;
    if (src.image.width(), src.image.height()) != (w, h) {
        return Err(SkinError::Size("skin.png"));
    }
    if src.tint.len() != n {
        return Err(SkinError::Size("tint.png"));
    }
    if src.glow.as_ref().is_some_and(|g| g.len() != n) {
        return Err(SkinError::Size("glow.png"));
    }
    if src.left.as_ref().is_some_and(|l| (l.width(), l.height()) != (w, h)) {
        return Err(SkinError::Size("left.png"));
    }
    let at = |x: u32, y: u32| y as usize * w as usize + x as usize;
    let glow_at = |x, y| src.glow.as_ref().map_or(0, |g| g[at(x, y)]);
    let from =
        scale2x(w, h, src.frame_size, |x, y| (plain(src.image.get(x, y)), src.tint[at(x, y)], glow_at(x, y)));
    let (w2, h2) = (2 * w, 2 * h);
    let mut out = blank(&src.name, true);
    out.notes = src.notes.clone();
    out.light = src.light;
    let mut glow = src.glow.as_ref().map(|_| vec![0u8; from.len()]);
    for (i, &(sx, sy)) in from.iter().enumerate() {
        let (x, y) = (i as u32 % w2, i as u32 / w2);
        out.image.set(x, y, plain(src.image.get(sx, sy)));
        out.tint[i] = src.tint[at(sx, sy)];
        if let Some(g) = &mut glow {
            g[i] = glow_at(sx, sy);
        }
    }
    out.glow = glow;
    if let Some(left) = &src.left {
        let mut img = RgbaImage::new(w2, h2);
        for (i, &(sx, sy)) in scale2x(w, h, src.frame_size, |x, y| plain(left.get(x, y))).iter().enumerate() {
            img.set(i as u32 % w2, i as u32 / w2, plain(left.get(sx, sy)));
        }
        out.left = Some(img);
    }
    Ok(out)
}

/// Writes a new skin pack `id` named `name` into `dir` (empty or not
/// there): a copy of the skin pack at `from`, doubled by [`double`] when
/// `doubled`, or without `from` a [`blank`] one. Returns the save's
/// warnings; on an error nothing is written.
pub fn new_skin(
    dir: &Path,
    id: &str,
    name: &str,
    from: Option<&Path>,
    doubled: bool,
) -> Result<Vec<String>, NewError> {
    let template = match from {
        Some(p) => Template::From(p),
        None => Template::Skin { double: doubled },
    };
    new_pack_with(dir, PackKind::Skin, id, name, template, |source| {
        if let (Some(_), true, Source::Skin(s)) = (from, doubled, source) {
            *s = double(s).map_err(|e| NewError::Edit(e.to_string()))?;
        }
        Ok(())
    })
}

/// A skin pack checked: the pack's errors and warnings
/// (`olr_pack::validate`, the files as they are on disk) and, when it has
/// no error, the skin as the game loads it.
#[derive(Debug)]
pub struct Checked {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub skin: Option<Skin>,
}

/// Loads and validates the skin pack at `dir`: a pack of another kind is
/// an error, and so is anything `olr_pack::validate` refuses (the files,
/// their canonical form aside, and the compiled skin).
pub fn check_dir(dir: &Path) -> Checked {
    let src = DirFs::new(dir);
    let mut out = Checked { errors: Vec::new(), warnings: Vec::new(), skin: None };
    match olr_pack::read_manifest(&src) {
        Ok(m) if m.kind != PackKind::Skin => {
            out.errors.push(format!("error: a {:?} pack, not a skin", m.kind));
            return out;
        }
        Ok(_) => {}
        Err(e) => {
            out.errors.push(format!("error: {e}"));
            return out;
        }
    }
    for d in olr_pack::validate(&src) {
        match d.severity {
            Severity::Error => out.errors.push(d.to_string()),
            Severity::Warning => out.warnings.push(d.to_string()),
        }
    }
    if out.errors.is_empty() {
        match olr_pack::read_skin(&src) {
            Ok(p) => out.skin = Some(p.skin),
            Err(e) => out.errors.push(format!("error: {e}")),
        }
    }
    out
}

/// One preview cell in world pixels: the 10 × 9 worm with room around it
/// for its light.
pub const PREVIEW_CELL: (u32, u32) = (16, 14);
/// Where the worm sits in its cell, world pixels.
const PREVIEW_WORM: (u32, u32) = (3, 2);
/// The largest preview scale, output pixels per world pixel.
pub const MAX_PREVIEW_SCALE: u32 = 16;
/// The light background's colour.
pub const PREVIEW_LIGHT: Rgba = Rgba::rgb(214, 214, 206);
/// The dark background's colour, before its darkness.
const PREVIEW_DARK: Rgba = Rgba::rgb(110, 118, 134);
/// The dark half's ambient light: the share of a pixel's colour it shows
/// without glow or light.
const PREVIEW_AMBIENT: f32 = 0.2;

/// The aim columns and walk rows a runtime sheet holds.
const AIM: u32 = OLX_SKIN.2;
const WALK: u32 = OLX_SKIN.3;

/// [`preview`]'s size at `scale`: 14 cells across (seven aim frames facing
/// right, then the same facing left) and 6 down (three walk rows on the
/// light background, then three on the dark).
pub fn preview_size(scale: u32) -> (u32, u32) {
    let s = scale.clamp(1, MAX_PREVIEW_SCALE);
    (2 * AIM * PREVIEW_CELL.0 * s, 2 * WALK * PREVIEW_CELL.1 * s)
}

/// One frame's glow as the preview draws its light: the glowing pixels'
/// colour weighted by their glow, their centre (art pixels in the frame)
/// and the light's strength, 0-2 (`olr_content`'s `frame_lights`).
struct FrameGlow {
    colour: [f32; 3],
    centre: (f32, f32),
    strength: f32,
}

/// Every frame of `skin` in the player's `colour`, facing right and left,
/// on a light background and in the dark, at `scale` output pixels per
/// world pixel (1 to [`MAX_PREVIEW_SCALE`]), so skins of either art scale
/// preview at the same size. In the dark half the worm shows at a fifth
/// of its colour but its glowing pixels at their glow (in quarters, as the
/// game draws them), and its light, if it has one, lights the cell around
/// the frame's glowing pixels over its reach (a preview of the game's
/// light, not its exact pixels: `olr-cli render-match --skin` draws that).
pub fn preview(skin: &Skin, colour: Rgba, scale: u32) -> RgbaImage {
    let s = scale.clamp(1, MAX_PREVIEW_SCALE);
    let (w, h) = preview_size(s);
    let mut out = RgbaImage::new(w, h);
    let layout = skin.def.layout;
    let sheet = skin.colorize(colour);
    let k = layout.art_scale();
    let (sw, sh) = (sheet.width(), sheet.height());
    // A frame's sprite on the sheet (OLX: src/client/CGameSkin.cpp:584-601,
    // as `olr_content::skin_source`); off the sheet is transparent.
    let pixel = |f: u32, x: u32, y: u32| -> Option<(Rgba, u8)> {
        let sx = f.checked_mul(layout.frame_width)?.checked_add(layout.frame_spacing)?.checked_add(x)?;
        if sx >= sw || y >= sh || skin.mask_at(sx, y) == MaskKind::Transparent {
            return None;
        }
        Some((sheet.get(sx, y), skin.glow_at(sx, y)))
    };
    let glows: Vec<Option<FrameGlow>> = (0..AIM * WALK)
        .map(|f| {
            let light = skin.def.light?;
            let (mut sum, mut weight, mut cx, mut cy) = ([0.0f32; 3], 0.0f32, 0.0f32, 0.0f32);
            for y in 0..layout.worm_height {
                for x in 0..layout.worm_width {
                    let Some((p, g)) = pixel(f, x, y) else { continue };
                    let g = f32::from(g) / 255.0;
                    for (c, v) in sum.iter_mut().zip([p.r, p.g, p.b]) {
                        *c += g * f32::from(v) / 255.0;
                    }
                    weight += g;
                    cx += g * (x as f32 + 0.5);
                    cy += g * (y as f32 + 0.5);
                }
            }
            (weight > 0.0).then(|| {
                let share = (weight / (k * k) as f32 / 3.0).min(1.0);
                FrameGlow {
                    colour: sum.map(|c| c / weight),
                    centre: (cx / weight, cy / weight),
                    strength: f32::from(light.intensity) / 100.0 * share,
                }
            })
        })
        .collect();
    let reach = skin.def.light.map_or(1.0, |l| f32::from(l.reach).max(1.0));
    let to_u8 = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    for oy in 0..h {
        let (row, cy) = (oy / (PREVIEW_CELL.1 * s), oy % (PREVIEW_CELL.1 * s));
        let (dark, walk) = (row >= WALK, row % WALK);
        for ox in 0..w {
            let (col, cx) = (ox / (PREVIEW_CELL.0 * s), ox % (PREVIEW_CELL.0 * s));
            let (left, aim) = (col >= AIM, col % AIM);
            let f = walk * AIM + aim;
            // The cell's pixel centre and the worm's art pixel under it.
            let (wx, wy) = ((cx as f32 + 0.5) / s as f32, (cy as f32 + 0.5) / s as f32);
            let (rx, ry) = (cx as i64 * k as i64 / s as i64, cy as i64 * k as i64 / s as i64);
            let (ax, ay) = (rx - i64::from(PREVIEW_WORM.0 * k), ry - i64::from(PREVIEW_WORM.1 * k));
            let inside = (0..i64::from(layout.worm_width)).contains(&ax)
                && (0..i64::from(layout.worm_height)).contains(&ay);
            let skin_px = inside
                .then(|| {
                    let ax = if left { layout.worm_width - 1 - ax as u32 } else { ax as u32 };
                    pixel(f, ax, ay as u32)
                })
                .flatten();
            let bg = if dark { PREVIEW_DARK } else { PREVIEW_LIGHT };
            let rgb = |p: Rgba| [p.r, p.g, p.b].map(|c| f32::from(c) / 255.0);
            let mut lit = [if dark { PREVIEW_AMBIENT } else { 1.0 }; 3];
            if let (true, Some(g)) = (dark, &glows[f as usize]) {
                let gx = if left { layout.worm_width as f32 - g.centre.0 } else { g.centre.0 };
                let (lx, ly) =
                    (PREVIEW_WORM.0 as f32 + gx / k as f32, PREVIEW_WORM.1 as f32 + g.centre.1 / k as f32);
                let d = ((wx - lx).powi(2) + (wy - ly).powi(2)).sqrt();
                let fall = (1.0 - d / reach).max(0.0).powi(2);
                for (l, c) in lit.iter_mut().zip(g.colour) {
                    *l = (*l + c * g.strength * fall).min(1.0);
                }
            }
            let back = rgb(bg);
            let mut c: [f32; 3] = std::array::from_fn(|i| back[i] * lit[i]);
            if let Some((p, glow)) = skin_px {
                let self_lit = if dark { glow_level(glow) as f32 / GLOW_LEVELS as f32 } else { 0.0 };
                let a = f32::from(p.a) / 255.0;
                let px = rgb(p);
                for i in 0..3 {
                    c[i] = c[i] * (1.0 - a) + px[i] * lit[i].max(self_lit) * a;
                }
            }
            out.set(ox, oy, Rgba { r: to_u8(c[0]), g: to_u8(c[1]), b: to_u8(c[2]), a: 255 });
        }
    }
    out
}
