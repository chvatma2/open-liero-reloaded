//! Level thumbnails for the menus (spec §21 G6): the level's picture
//! scaled down into [`THUMB_SIZE`], keeping its aspect. Moved here from
//! `olr-import-gfx` (plan 13d): the importer and the level operations
//! both make them.
//!
//! OLX's level list shows the level's minimap as its preview
//! (`minimapForLevel`, `src/game/Level.cpp:33-44`; `MapLoad::getMinimap`,
//! `src/common/MapLoader.cpp:79-104`): the drawn image resampled into
//! 128 × 96 (`CMap.h:86-87`), stretched (`gusUpdateMinimap`,
//! `src/gusanos/level.cpp:486-527`). The remake letterboxes it into
//! 160 × 96 instead, so wide and tall levels keep their shape
//! (`docs/porting/STATUS.md`).

use olr_data::image::RgbaImage;

/// The size thumbnails are made at: the level letterboxed into it.
/// `olr-pack` re-exports it.
pub const THUMB_SIZE: (u32, u32) = (160, 96);

/// Samples per thumbnail pixel along each axis, at most: a box filter of
/// bounded cost however large the art (OLX's default `Game.Antialiasing`
/// resamples).
const SAMPLES: u64 = 8;

/// The rectangle `(x, y, w, h)` an `aw` × `ah` picture fills inside a
/// `tw` × `th` box at the largest scale that keeps its aspect, centred.
/// Each side is at least 1 pixel.
pub fn letterbox(aw: u32, ah: u32, tw: u32, th: u32) -> (u32, u32, u32, u32) {
    if aw == 0 || ah == 0 || tw == 0 || th == 0 {
        return (0, 0, tw, th);
    }
    // Width-bound when aw / ah >= tw / th.
    let (w, h) = if u64::from(aw) * u64::from(th) >= u64::from(ah) * u64::from(tw) {
        let h = (u64::from(tw) * u64::from(ah) + u64::from(aw) / 2) / u64::from(aw);
        (tw, (h as u32).clamp(1, th))
    } else {
        let w = (u64::from(th) * u64::from(aw) + u64::from(ah) / 2) / u64::from(ah);
        ((w as u32).clamp(1, tw), th)
    };
    ((tw - w) / 2, (th - h) / 2, w, h)
}

/// The thumbnail of a level whose visible picture is `front` (which
/// equals `back` where the material is background): box-filtered into
/// [`THUMB_SIZE`], letterboxed with transparent bars. Opaque inside;
/// never panics, whatever the art's size.
pub fn thumbnail(front: &RgbaImage) -> RgbaImage {
    let (tw, th) = THUMB_SIZE;
    scaled(front, tw, th)
}

/// `art` box-filtered into the [`letterbox`] of a `tw` × `th` image.
fn scaled(art: &RgbaImage, tw: u32, th: u32) -> RgbaImage {
    let mut out = RgbaImage::new(tw, th);
    let (aw, ah) = (art.width(), art.height());
    if aw == 0 || ah == 0 || tw == 0 || th == 0 {
        return out;
    }
    let (ox, oy, w, h) = letterbox(aw, ah, tw, th);
    let (aw64, ah64, w64, h64) = (u64::from(aw), u64::from(ah), u64::from(w), u64::from(h));
    let sx = SAMPLES.min(aw64.div_ceil(w64)).max(1);
    let sy = SAMPLES.min(ah64.div_ceil(h64)).max(1);
    let raw = art.as_raw();
    for ty in 0..h64 {
        for tx in 0..w64 {
            let mut sum = [0u64; 3];
            for j in 0..sy {
                // Sample centres spread over the pixel's area.
                let y = (((ty * sy + j) * 2 + 1) * ah64 / (h64 * sy * 2)).min(ah64 - 1);
                for i in 0..sx {
                    let x = (((tx * sx + i) * 2 + 1) * aw64 / (w64 * sx * 2)).min(aw64 - 1);
                    let p = ((y * aw64 + x) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u64::from(raw[p + c]);
                    }
                }
            }
            let n = sx * sy;
            let [r, g, b] = sum.map(|s| (s / n) as u8);
            out.set(ox + tx as u32, oy + ty as u32, olr_data::image::Rgba::rgb(r, g, b));
        }
    }
    out
}

/// The thumbnail of `level` as it plays (the user's note, 2026-10-19): a
/// level with a look ([`super::compile_look`]) is drawn lit, else it is
/// [`thumbnail`] of its front. A look that does not compile gives the
/// plain thumbnail too.
pub fn level_thumbnail(level: &super::LevelSource) -> RgbaImage {
    match super::compile_look(level) {
        Ok(Some(look)) => lit_thumbnail(level, &look),
        _ if level.distant.is_empty() => thumbnail(&level.front),
        _ => thumbnail(&seen(level).0),
    }
}

/// `level`'s front as seen with the camera at its centre (issue #11):
/// each see-through pixel (alpha 0) shows the nearest distant layer
/// covering it, else keeps its colour (the back tile's); with each art
/// pixel's depth (0 for the level's own art, else the layer's, 0-1).
/// Every pixel opaque.
fn seen(level: &super::LevelSource) -> (RgbaImage, Vec<f32>) {
    let s = i64::from(level.art_scale.max(1));
    let mut art = level.front.clone();
    let (aw, ah) = (art.width(), art.height());
    let mut depth = vec![0.0f32; aw as usize * ah as usize];
    for y in 0..ah {
        for x in 0..aw {
            let p = art.get(x, y);
            if p.a != 0 {
                continue;
            }
            let hit = level.distant.iter().rev().find_map(|d| {
                let (lx, ly) = (i64::from(x) - i64::from(d.at.0) * s, i64::from(y) - i64::from(d.at.1) * s);
                let inside = (0..i64::from(d.image.width())).contains(&lx)
                    && (0..i64::from(d.image.height())).contains(&ly);
                let q = if inside { d.image.get(lx as u32, ly as u32) } else { return None };
                (q.a != 0).then_some((q, d.depth))
            });
            let (c, d) = hit.unwrap_or((p, 0));
            art.set(x, y, olr_data::image::Rgba { a: 255, ..c });
            depth[(y * aw + x) as usize] = f32::from(d.min(100)) / 100.0;
        }
    }
    (art, depth)
}

/// `fade` of the look shaders (`common.wgsl`): `(1 - (d/r)²)²`, nothing at
/// or past `r`, none for an unlimited reach.
fn fade(d: f32, r: f32) -> f32 {
    if r >= 1e8 {
        return 1.0;
    }
    let x = d / r.max(1e-6);
    if x >= 1.0 {
        return 0.0;
    }
    let v = 1.0 - x * x;
    v * v
}

/// GLSL's `smoothstep`; 0 for an empty range.
fn smoothstep(lo: f32, hi: f32, v: f32) -> f32 {
    if hi <= lo {
        return if v >= hi { 1.0 } else { 0.0 };
    }
    let t = ((v - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn linear_to_srgb(v: f32) -> u8 {
    let c = v.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

/// [`thumbnail`] with `look` at clock 0, a CPU model of the look (the
/// renderer's is on the GPU): each thumbnail pixel's light is the ambient
/// and the glow light (each of
/// a coarse grid of cells casting its glow by the angle it fills, faded by
/// its reach); each art sample is lit by it, never below its glow nor, for
/// a solid, the solid floor, capped at max light; the mean is graded. No
/// shadows, soak, softness or light steps: a thumbnail is too small for
/// them.
fn lit_thumbnail(level: &super::LevelSource, look: &olr_look::LevelLook) -> RgbaImage {
    use olr_data::level::Material;
    let (tw, th) = THUMB_SIZE;
    let mut out = RgbaImage::new(tw, th);
    // A distant layer's pixel is lit by the ambient and `1 - depth` of the
    // light beyond it.
    let (seen_art, depths) = if level.distant.is_empty() {
        (None, Vec::new())
    } else {
        let (a, d) = seen(level);
        (Some(a), d)
    };
    let art = seen_art.as_ref().unwrap_or(&level.front);
    let (aw, ah) = (art.width(), art.height());
    let (ww, wh) = level.size;
    let s = level.art_scale.max(1);
    if aw == 0 || ah == 0 || ww == 0 || wh == 0 || aw != ww * s || ah != wh * s {
        return thumbnail(art);
    }
    let mut frame = olr_look::LookFrame::default();
    frame.begin(Some(look), 0.0);
    let glow = level.glow.as_ref().filter(|g| g.art == (aw, ah) && g.world == (ww, wh));
    let solid = |wx: u32, wy: u32| {
        level.material.get((wy * ww + wx) as usize).is_some_and(|m| *m != Material::Background)
    };
    // A world pixel's group and an art pixel's glow strength, front where
    // solid, back where not (as the colour map shows them).
    let group = |wx: u32, wy: u32| -> Option<&olr_look::FrameGroup> {
        let g = glow?;
        let i = (wy * ww + wx) as usize;
        let v = if solid(wx, wy) { g.groups.get(i) } else { g.groups_back.get(i) }.copied().unwrap_or(0);
        if v == 0 { None } else { frame.groups.get(usize::from(v) - 1) }
    };
    let strength = |x: u32, y: u32| -> f32 {
        let Some(g) = glow else { return 0.0 };
        let i = (y * aw + x) as usize;
        let v = if solid(x / s, y / s) { g.front.get(i) } else { g.back.get(i) }.copied().unwrap_or(0);
        f32::from(v) / 255.0
    };
    let albedo = |x: u32, y: u32| {
        let p = art.get(x, y);
        [p.r, p.g, p.b].map(olr_look::srgb_to_linear)
    };
    let (ox, oy, w, h) = letterbox(aw, ah, tw, th);
    let (aw64, ah64, w64, h64) = (u64::from(aw), u64::from(ah), u64::from(w), u64::from(h));
    let sx = SAMPLES.min(aw64.div_ceil(w64)).max(1);
    let sy = SAMPLES.min(ah64.div_ceil(h64)).max(1);
    // Art sample (i, j) of thumbnail pixel (tx, ty).
    let sample = |tx: u64, ty: u64, i: u64, j: u64| {
        let y = (((ty * sy + j) * 2 + 1) * ah64 / (h64 * sy * 2)).min(ah64 - 1);
        let x = (((tx * sx + i) * 2 + 1) * aw64 / (w64 * sx * 2)).min(aw64 - 1);
        (x as u32, y as u32)
    };
    // The glow light's cells: two thumbnail pixels a side.
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    // Directional pixels (the directional-glow design) cast within their
    // cone only: `Some((direction as a unit vector, spread in radians))`.
    type Cone = Option<([f32; 2], f32)>;
    let mut cells: Vec<([f32; 3], f32, [f32; 2], Cone)> = Vec::new();
    let beams = level.glow_beams.as_ref().filter(|b| b.fits(level.size));
    if glow.is_some() {
        for cy in 0..u64::from(ch) {
            for cx in 0..u64::from(cw) {
                let (mut sum, mut reach, mut n) = ([0.0f32; 3], 0.0f32, 0.0f32);
                // The cell's directional light by (direction, spread).
                let mut cones: Vec<((u16, u8), [f32; 3], f32)> = Vec::new();
                for ty in (cy * 2)..(cy * 2 + 2).min(h64) {
                    for tx in (cx * 2)..(cx * 2 + 2).min(w64) {
                        for j in 0..sy {
                            for i in 0..sx {
                                let (x, y) = sample(tx, ty, i, j);
                                n += 1.0;
                                let g = strength(x, y);
                                let Some(grp) = group(x / s, y / s).filter(|_| g > 0.0) else { continue };
                                let a = match (grp.own_colour, grp.filtered) {
                                    (true, true) => olr_look::engine::filtered(albedo(x, y)),
                                    (true, false) => albedo(x, y),
                                    _ => [1.0; 3],
                                };
                                let beam = beams.and_then(|b| b.get(!solid(x / s, y / s), x / s, y / s));
                                let (sum, reach) = match beam {
                                    None => (&mut sum, &mut reach),
                                    Some(key) => {
                                        let at = match cones.iter().position(|c| c.0 == key) {
                                            Some(at) => at,
                                            None => {
                                                cones.push((key, [0.0; 3], 0.0));
                                                cones.len() - 1
                                            }
                                        };
                                        let c = &mut cones[at];
                                        (&mut c.1, &mut c.2)
                                    }
                                };
                                for ((s, k), a) in sum.iter_mut().zip(grp.cast).zip(a) {
                                    *s += k * a * g;
                                }
                                *reach = reach.max(grp.reach);
                            }
                        }
                    }
                }
                // The cell's centre in world pixels.
                let at = [
                    ((cx * 2 + 1) as f32) * ww as f32 / w as f32,
                    ((cy * 2 + 1) as f32) * wh as f32 / h as f32,
                ];
                if n > 0.0 && sum.iter().any(|v| *v > 0.0) {
                    cells.push((sum.map(|v| v / n), reach, at, None));
                }
                for ((direction, spread), sum, reach) in cones {
                    if n > 0.0 && sum.iter().any(|v| *v > 0.0) {
                        let a = f32::from(direction).to_radians();
                        let cone = Some(([a.cos(), a.sin()], f32::from(spread).to_radians()));
                        cells.push((sum.map(|v| v / n), reach, at, cone));
                    }
                }
            }
        }
    }
    // A cell's side in world pixels: the angle it fills from `d` away.
    let side = 2.0 * (ww as f32 / w as f32).max(wh as f32 / h as f32);
    let max = frame.max_light;
    let g = &frame.grade;
    let exposure = 2f32.powf(f32::from(g.exposure) / 100.0);
    let (contrast, saturation) = (f32::from(g.contrast) / 100.0, f32::from(g.saturation) / 100.0);
    let tint = g.tint.map(olr_look::srgb_to_linear);
    for ty in 0..h64 {
        for tx in 0..w64 {
            let at = [(tx as f32 + 0.5) * ww as f32 / w as f32, (ty as f32 + 0.5) * wh as f32 / h as f32];
            let mut light = frame.ambient;
            for (e, reach, p, cone) in &cells {
                let d = ((p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2)).sqrt();
                let mut k = (side / (std::f32::consts::TAU * d.max(1e-3))).min(1.0) * fade(d, *reach);
                if let Some((dir, spread)) = cone {
                    // The cascade cone: the cast direction runs from the
                    // cell to the pixel.
                    let dot =
                        if d > 1e-3 { ((at[0] - p[0]) * dir[0] + (at[1] - p[1]) * dir[1]) / d } else { 1.0 };
                    k *= smoothstep((spread + 0.26).cos(), spread.cos(), dot);
                }
                for (v, c) in light.iter_mut().zip(e) {
                    *v += c * k;
                }
            }
            let light = light.map(|v| if v.is_finite() { v.min(max) } else { 0.0 });
            let mut sum = [0.0f32; 3];
            for j in 0..sy {
                for i in 0..sx {
                    let (x, y) = sample(tx, ty, i, j);
                    let (wx, wy) = (x / s, y / s);
                    let own = strength(x, y) * group(wx, wy).map_or(1.0, |g| g.self_scale);
                    let floor = if solid(wx, wy) { frame.solid_floor } else { [0.0; 3] };
                    let a = albedo(x, y);
                    let d = depths.get((y * aw + x) as usize).copied().unwrap_or(0.0);
                    for ((((s, a), l), f), amb) in
                        sum.iter_mut().zip(a).zip(light).zip(floor).zip(frame.ambient)
                    {
                        let l =
                            if d > 0.0 { amb.min(max) + (1.0 - d) * (l - amb.min(max)).max(0.0) } else { l };
                        *s += a * l.max(f).max(own);
                    }
                }
            }
            let n = (sx * sy) as f32;
            let mut v = sum.map(|c| c / n * exposure);
            v = v.map(|c| (c - 0.5) * contrast + 0.5);
            let luma = 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
            let [r, g, b] = [0, 1, 2].map(|c| linear_to_srgb((luma + (v[c] - luma) * saturation) * tint[c]));
            out.set(ox + tx as u32, oy + ty as u32, olr_data::image::Rgba::rgb(r, g, b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::image::Rgba;

    /// The front art of a `w` × `h` level at `art_scale`.
    fn front(w: u32, h: u32, art_scale: u32, paint: impl Fn(u32, u32) -> Rgba) -> RgbaImage {
        let mut front = RgbaImage::new(w * art_scale, h * art_scale);
        for y in 0..h * art_scale {
            for x in 0..w * art_scale {
                front.set(x, y, paint(x, y));
            }
        }
        front
    }

    /// The opaque area of `img`: `(x0, y0, x1, y1)`, exclusive.
    fn opaque(img: &RgbaImage) -> (u32, u32, u32, u32) {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..img.height() {
            for x in 0..img.width() {
                if img.get(x, y).a == 255 {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        (x0, y0, x1, y1)
    }

    #[test]
    fn letterbox_keeps_the_aspect() {
        assert_eq!(letterbox(160, 96, 160, 96), (0, 0, 160, 96));
        assert_eq!(letterbox(504, 350, 160, 96), (11, 0, 138, 96), "Dirt Level");
        assert_eq!(letterbox(1350, 550, 160, 96), (0, 15, 160, 65), "Castle Strike");
        assert_eq!(letterbox(1, 1000, 160, 96), (79, 0, 1, 96), "a thread stays a pixel wide");
        assert_eq!(letterbox(1000, 1, 160, 96), (0, 47, 160, 1));
        assert_eq!(letterbox(0, 5, 160, 96), (0, 0, 160, 96));
    }

    /// Wide levels get bars above and below, tall ones beside, all of the
    /// thumbnail's size; the picture inside is the level's colours.
    #[test]
    fn thumbnails_are_letterboxed_to_a_fixed_size() {
        let red = Rgba::rgb(200, 10, 10);
        for (w, h, scale) in [(1350, 550, 1), (504, 350, 2), (200, 900, 1), (1, 1, 1), (3, 2, 8)] {
            let t = thumbnail(&front(w, h, scale, |_, _| red));
            assert_eq!((t.width(), t.height()), THUMB_SIZE);
            let (x, y, lw, lh) = letterbox(w * scale, h * scale, 160, 96);
            assert_eq!(opaque(&t), (x, y, x + lw, y + lh), "{w}x{h} x{scale}");
            assert_eq!(t.get(x + lw / 2, y + lh / 2), red);
            if lw < 160 {
                assert_eq!(t.get(0, 48).a, 0, "{w}x{h}: bars beside");
            }
            if lh < 96 {
                assert_eq!(t.get(80, 0).a, 0, "{w}x{h}: bars above");
            }
        }
    }

    /// Each pixel is the mean of its area: a fine checkerboard turns grey,
    /// and the left and right halves keep their colours.
    #[test]
    fn thumbnails_average_the_art() {
        let t = thumbnail(&front(640, 384, 1, |x, y| {
            if (x + y) % 2 == 0 { Rgba::rgb(200, 200, 200) } else { Rgba::rgb(0, 0, 0) }
        }));
        assert_eq!(t.get(80, 48), Rgba::rgb(100, 100, 100));
        let t = thumbnail(&front(640, 384, 1, |x, _| {
            if x < 320 { Rgba::rgb(0, 0, 255) } else { Rgba::rgb(255, 255, 255) }
        }));
        assert_eq!((t.get(10, 10), t.get(150, 90)), (Rgba::rgb(0, 0, 255), Rgba::rgb(255, 255, 255)));
    }

    /// Issue #11: a see-through pixel shows the nearest distant layer
    /// covering it from the level's centre, else its own (tile) colour.
    #[test]
    fn the_thumbnail_shows_distant_layers() {
        let mut level = crate::levels::tests::tiny(1);
        let (w, h) = level.size;
        let tile = Rgba { a: 0, ..Rgba::rgb(9, 9, 9) };
        level.front = front(w, h, 1, |_, _| tile);
        let mut near = RgbaImage::new(1, 2);
        near.set(0, 0, Rgba::rgb(0, 0, 200));
        near.set(0, 1, Rgba::rgb(0, 0, 200));
        let mut far = RgbaImage::new(2, 2);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            far.set(x, y, Rgba::rgb(200, 0, 0));
        }
        level.distant = vec![
            olr_data::level::DistantLayer { image: far, at: (0, 0), depth: 90, glow: None },
            olr_data::level::DistantLayer { image: near, at: (0, 0), depth: 10, glow: None },
        ];
        let (art, depth) = seen(&level);
        assert_eq!(art.get(0, 0), Rgba::rgb(0, 0, 200));
        assert_eq!(art.get(1, 0), Rgba::rgb(200, 0, 0));
        assert_eq!(art.get(2, 0), Rgba::rgb(9, 9, 9));
        assert_eq!(depth[..3], [0.1, 0.9, 0.0]);
        let t = level_thumbnail(&level);
        assert!(t.as_raw().chunks(4).any(|p| p[..3] == [0, 0, 200]));
        assert!(t.as_raw().chunks(4).any(|p| p[..3] == [200, 0, 0]));
    }
}
