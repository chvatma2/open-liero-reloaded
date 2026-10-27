//! Live copies drawn over the ground (issue #87; the design
//! `docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`): a
//! level's live copies are composited into the colour map the renderer
//! uploads, not drawn as sprites, so every lit pass (the glow light,
//! occlusion, the light soak, bloom) handles their pictures as it handles
//! the ground's.
//!
//! [`LiveArt`] holds every copy's pictures (static, made once from the
//! level's played form); [`LiveFrame`] each presented tick's placed parts,
//! copy by copy (presentation fills it from the world's live state). The
//! colour map keeps the ground as it is (its edits, `material()`,
//! `rebuild_area`) and a shown composite of the ground with the parts over
//! it ([`crate::ColorMap::set_live`]):
//! - a rock part is front art: where its rock mask is solid it hides the
//!   ground, glows as its own glow and groups, and is solid for the light;
//! - a background part is back art: it shows only where the ground is
//!   background, in the stack's order (a rock part above it covers it; it
//!   lays over a rock part below it, which is then open for the light, as
//!   a static background element above rock is), its alpha (glass included) times
//!   its opacity laid over what is there as the static bake lays a
//!   translucent background (straight alpha, never over a see-through
//!   pixel unless it is glass), its glow and groups over the ground's.
//!
//! Issue #93: the shown beam map (two bytes a world pixel, as
//! [`crate::GlowBeamMap`]'s) and the shown glass map (as
//! [`crate::GlassMap`]'s) are composited alike when the ground has one or a
//! picture does: a drawn part's pixel shows the part's beam texel (its
//! direction and spread the frame's, mirrored with the part) and glass
//! texel (its tint the frame's) or none, elsewhere the ground's.

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use olr_data::level::Material;

use crate::dirty::DirtyTiles;

/// One picture of a live copy as the colour map draws it: the played
/// form's `live/<copy>/<k>.png` and its masks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LivePicture {
    /// World pixels.
    pub size: (u32, u32),
    /// RGBA8 at `size` × the map's art scale, row-major; another length
    /// draws nothing.
    pub art: Vec<u8>,
    /// Per world pixel: 0 open, else solid (a rock part's picture).
    pub rock: Option<Vec<u8>>,
    /// Per art pixel, the unscaled glow mask (shown: mask × the part's
    /// strength, as the static bake).
    pub glow: Option<Vec<u8>>,
    /// Per world pixel, the group map value at run time (the caller maps
    /// the copy's own groups after the level's), 0 none.
    pub groups: Option<Vec<u8>>,
    /// Per world pixel, two bytes (direction / 2 + 1, 0 none; the spread),
    /// as [`crate::GlowBeamMap`]'s (issue #93).
    pub beam: Option<Vec<u8>>,
    /// Per art pixel, RGBA: the glass's filter (`mix(255, colour, tint)`
    /// at its authored tint) and frost, as [`crate::GlassMap`]'s, white
    /// and 0 where it is not glass (issue #93; a background picture's).
    pub glass: Option<Vec<u8>>,
}

/// Every live copy's pictures, by copy then picture index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveArt {
    pub copies: Vec<Vec<LivePicture>>,
}

impl LiveArt {
    /// Whether any picture glows: a level lit only by live copies then
    /// gets an empty glow channel, so its groups and glow are uploaded.
    pub fn glows(&self) -> bool {
        self.copies.iter().flatten().any(|p| p.glow.as_ref().is_some_and(|g| g.iter().any(|&v| v != 0)))
    }

    /// Whether any picture has a directional pixel (issue #93): the map
    /// then keeps a shown beam map.
    pub fn beams(&self) -> bool {
        self.copies.iter().flatten().any(|p| p.beam.as_ref().is_some_and(|b| b.iter().any(|&v| v != 0)))
    }

    /// Whether any picture has glass (issue #93): the map then keeps a
    /// shown glass map.
    pub fn glass(&self) -> bool {
        self.copies.iter().flatten().any(|p| p.glass.is_some())
    }
}

/// One shown part of a live copy, where it is drawn this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LivePlaced {
    /// An index into its copy's pictures.
    pub picture: u16,
    /// A rock part (front art, solid); else a background part.
    pub rock: bool,
    /// Its picture's top-left, world pixels (after mirroring).
    pub at: (i32, i32),
    /// The picture is drawn mirrored left to right (the copy's `Facing`).
    pub flip: bool,
    /// A background part's opacity, 0-255 (255 as drawn).
    pub alpha: u8,
    /// Its glow strength, percent (shown: mask × strength).
    pub glow: u8,
    /// Its beams' direction, whole degrees 0-359 before mirroring (its
    /// layer's `BeamDirection`, issue #93); `None` its pictures' own.
    pub beam_direction: Option<u16>,
    /// Its beams' spread, degrees 1-90 (`BeamSpread`); `None` their own.
    pub beam_spread: Option<u8>,
    /// Its glass's opacity, percent 1-99 (a background part's
    /// `GlassOpacity`, issue #93); `None` as baked.
    pub glass_opacity: Option<u8>,
    /// Its glass's tint, percent 0-100 (`GlassTint`); `None` as baked.
    pub glass_tint: Option<u8>,
}

/// The live copies' shown parts, copy by copy, bottom to top: what
/// presentation hands the colour map each presented tick. Reused, it
/// allocates nothing once warm.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LiveFrame {
    parts: Vec<LivePlaced>,
    /// Each copy's end in `parts`.
    ends: Vec<u32>,
}

impl LiveFrame {
    /// No copy.
    pub fn clear(&mut self) {
        self.parts.clear();
        self.ends.clear();
    }

    /// Adds a part to the copy being filled.
    pub fn push(&mut self, part: LivePlaced) {
        self.parts.push(part);
    }

    /// Ends the copy being filled: the next part is the next copy's.
    pub fn end_copy(&mut self) {
        self.ends.push(u32::try_from(self.parts.len()).unwrap_or(u32::MAX));
    }

    /// How many copies it holds.
    pub fn copies(&self) -> usize {
        self.ends.len()
    }

    /// Copy `c`'s parts, bottom to top (none past the copies).
    pub fn copy(&self, c: usize) -> &[LivePlaced] {
        let Some(&end) = self.ends.get(c) else { return &[] };
        let start = if c == 0 { 0 } else { self.ends[c - 1] as usize };
        self.parts.get(start..end as usize).unwrap_or(&[])
    }

    fn copy_from(&mut self, other: &LiveFrame) {
        self.parts.clear();
        self.parts.extend_from_slice(&other.parts);
        self.ends.clear();
        self.ends.extend_from_slice(&other.ends);
    }
}

/// A world rectangle, `x0..x1` × `y0..y1`.
pub(crate) type WorldBox = (u32, u32, u32, u32);

/// The colour map's live layer: the pictures, the frame drawn and the
/// shown composite of the ground with the parts.
#[derive(Clone, Debug)]
pub(crate) struct Live {
    pub(crate) art: Arc<LiveArt>,
    pub(crate) frame: LiveFrame,
    /// The shown colour, RGBA8 at art size.
    pub(crate) pixels: Vec<u8>,
    /// The shown glow strength (art size) and groups (world size), when
    /// the map has a glow channel.
    pub(crate) glow: Option<(Vec<u8>, Vec<u8>)>,
    /// Per world pixel, 1 where a rock part is solid.
    pub(crate) solid: Vec<u8>,
    /// The shown beam map, two bytes a world pixel (issue #93), when the
    /// ground or a picture has beams.
    pub(crate) beams: Option<Vec<u8>>,
    /// The shown glass map, RGBA an art pixel (issue #93), when the
    /// ground or a picture has glass.
    pub(crate) glass: Option<Vec<u8>>,
    /// What ground edits changed since the composite was last settled.
    pub(crate) stale: DirtyTiles,
    /// Scratch: the boxes a frame change recomposes.
    boxes: Vec<WorldBox>,
}

/// The ground as the composition reads it.
pub(crate) struct Ground<'a> {
    pub(crate) scale: u32,
    /// Art pixels.
    pub(crate) width: u32,
    pub(crate) pixels: &'a [u8],
    /// The ground's glow composite (art) and groups (world).
    pub(crate) glow: Option<(&'a [u8], &'a [u8])>,
    pub(crate) cells: &'a [u8],
    /// World pixels.
    pub(crate) world: (u32, u32),
    /// The ground's beam maps (composite, backdrop) when they fit.
    pub(crate) beams: Option<(&'a [u8], &'a [u8])>,
    /// The ground's glass map when it fits.
    pub(crate) glass: Option<&'a [u8]>,
}

/// A glass map's texel where there is no glass.
const NO_GLASS: [u8; 4] = [255, 255, 255, 0];

impl Ground<'_> {
    /// The ground's beam texel at world pixel `cell`: the backdrop's on
    /// background, the composite's elsewhere (as the renderer uploads it).
    fn beam(&self, cell: usize) -> [u8; 2] {
        let Some((front, back)) = self.beams else { return [0, 0] };
        let layer = if self.cells[cell] == Material::Background as u8 { back } else { front };
        [layer[2 * cell], layer[2 * cell + 1]]
    }
}

/// A picture's beam texel `t` as part `p` shows it: its direction the
/// part's value or its own, mirrored when the part is, in the map's 2°
/// steps, its spread the part's or its own; none stays none.
fn shown_beam(t: [u8; 2], p: &LivePlaced) -> [u8; 2] {
    if t[0] == 0 {
        return [0, 0];
    }
    let own = i64::from(t[0] - 1) * 2;
    let d = p.beam_direction.map_or(own, |d| i64::from(d) % 360);
    let d = if p.flip { (180 - d).rem_euclid(360) } else { d };
    let spread = p.beam_spread.map_or(t[1], |s| s.clamp(1, 90));
    [(d / 2 + 1) as u8, spread.max(1)]
}

/// The glow a part shows at a mask value `mask` at `strength` percent:
/// the level bake's (`olr_format::levels::glow_value`, `(round(strength ×
/// 2.55) × mask + 127) / 255`), so a live copy glows as the same copy
/// static.
pub fn live_glow(strength: u8, mask: u8) -> u8 {
    let s = (u32::from(strength.min(100)) * 255 + 50) / 100;
    ((s * u32::from(mask) + 127) / 255) as u8
}

/// A background part's colour `src` at alpha `a` over `dst` (straight
/// alpha, rounded; the bake's `glass_over`): a see-through destination
/// gives no colour of its own.
fn over(src: [u8; 4], a: u8, dst: [u8; 4]) -> [u8; 4] {
    let (a32, da) = (u32::from(a), u32::from(dst[3]));
    if da == 0 {
        return [src[0], src[1], src[2], a];
    }
    let out = a32 * 255 + da * (255 - a32);
    let mix = |s: u8, d: u8| {
        let n = u32::from(s) * a32 * 255 + u32::from(d) * da * (255 - a32);
        ((n + out / 2) / out) as u8
    };
    [mix(src[0], dst[0]), mix(src[1], dst[1]), mix(src[2], dst[2]), ((out + 127) / 255) as u8]
}

/// `m` when it holds `n` bytes (a misfit mask is left out, never indexed).
fn fits(m: &Option<Vec<u8>>, n: usize) -> Option<&[u8]> {
    m.as_deref().filter(|v| v.len() == n)
}

impl Live {
    /// The layer of `art` over a ground of `g`'s sizes, nothing drawn yet
    /// (the shown composite is the ground).
    pub(crate) fn new(art: Arc<LiveArt>, g: &Ground<'_>, tile: u32) -> Live {
        let art_size = (g.width, g.pixels.len() as u32 / 4 / g.width.max(1));
        let mut live = Live {
            art,
            frame: LiveFrame::default(),
            pixels: g.pixels.to_vec(),
            glow: g.glow.map(|(a, w)| (a.to_vec(), w.to_vec())),
            solid: vec![0; g.cells.len()],
            beams: None,
            glass: None,
            stale: DirtyTiles::new(art_size, tile),
            boxes: Vec::new(),
        };
        live.base(g);
        live
    }

    /// The shown beam and glass maps afresh as the ground's (none where
    /// neither the ground nor a picture has any).
    fn base(&mut self, g: &Ground<'_>) {
        let cells = g.cells.len();
        self.beams = (g.beams.is_some() || self.art.beams()).then(|| {
            let mut b = vec![0; 2 * cells];
            for (i, t) in b.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                *t = g.beam(i);
            }
            b
        });
        self.glass = (g.glass.is_some() || self.art.glass()).then(|| match g.glass {
            Some(t) => t.to_vec(),
            None => NO_GLASS.repeat(g.pixels.len() / 4),
        });
    }

    /// Copy `copy`'s part `p`'s world box, clipped to the level (`None`
    /// when it shows nothing there or its picture is missing).
    fn part_box(&self, copy: usize, p: &LivePlaced, world: (u32, u32)) -> Option<WorldBox> {
        let pic = self.art.copies.get(copy)?.get(usize::from(p.picture))?;
        let clip = |a: i32, n: u32, max: u32| {
            let (a0, a1) = (i64::from(a), i64::from(a) + i64::from(n));
            let (c0, c1) = (a0.clamp(0, i64::from(max)), a1.clamp(0, i64::from(max)));
            (c1 > c0).then_some((c0 as u32, c1 as u32))
        };
        let (x0, x1) = clip(p.at.0, pic.size.0, world.0)?;
        let (y0, y1) = clip(p.at.1, pic.size.1, world.1)?;
        Some((x0, y0, x1, y1))
    }

    /// Takes `frame`, recomposing the boxes of every copy whose parts
    /// changed (theirs before and after); gives them, to mark dirty.
    pub(crate) fn set_frame(&mut self, frame: &LiveFrame, g: &Ground<'_>) -> &[WorldBox] {
        let mut boxes = core::mem::take(&mut self.boxes);
        boxes.clear();
        for c in 0..self.frame.copies().max(frame.copies()) {
            let (old, new) = (self.frame.copy(c), frame.copy(c));
            if old == new {
                continue;
            }
            boxes.extend(old.iter().chain(new).filter_map(|p| self.part_box(c, p, g.world)));
        }
        self.frame.copy_from(frame);
        for &b in &boxes {
            self.compose(g, b);
        }
        self.boxes = boxes;
        &self.boxes
    }

    /// Recomposes what ground edits changed since the last call.
    pub(crate) fn settle(&mut self, g: &Ground<'_>) {
        let mut stale = core::mem::replace(&mut self.stale, DirtyTiles::new((0, 0), 1));
        let s = g.scale.max(1);
        for d in stale.drain() {
            let r = d.rect;
            self.compose(g, (r.x / s, r.y / s, (r.x + r.w).div_ceil(s), (r.y + r.h).div_ceil(s)));
        }
        self.stale = stale;
    }

    /// Recomposes everything (a new glow channel or ground).
    pub(crate) fn compose_all(&mut self, g: &Ground<'_>) {
        self.pixels.clear();
        self.pixels.extend_from_slice(g.pixels);
        self.glow = g.glow.map(|(a, w)| (a.to_vec(), w.to_vec()));
        self.solid.clear();
        self.solid.resize(g.cells.len(), 0);
        self.base(g);
        self.compose(g, (0, 0, g.world.0, g.world.1));
    }

    /// The ground in world box `b`, then every part over it, copy by copy,
    /// bottom to top.
    fn compose(&mut self, g: &Ground<'_>, b: WorldBox) {
        let (x0, y0) = (b.0.min(g.world.0), b.1.min(g.world.1));
        let (x1, y1) = (b.2.min(g.world.0), b.3.min(g.world.1));
        if x1 <= x0 || y1 <= y0 || self.pixels.len() != g.pixels.len() {
            return;
        }
        let (s, w, mw) = (g.scale as usize, g.width as usize, g.world.0 as usize);
        for ay in y0 as usize * s..y1 as usize * s {
            let (a, e) = (ay * w + x0 as usize * s, ay * w + x1 as usize * s);
            self.pixels[a * 4..e * 4].copy_from_slice(&g.pixels[a * 4..e * 4]);
            if let (Some((glow, _)), Some((ground, _))) = (&mut self.glow, g.glow) {
                glow[a..e].copy_from_slice(&ground[a..e]);
            }
            if let Some(glass) = &mut self.glass {
                match g.glass {
                    Some(ground) => glass[a * 4..e * 4].copy_from_slice(&ground[a * 4..e * 4]),
                    None => glass[a * 4..e * 4].as_chunks_mut::<4>().0.fill(NO_GLASS),
                }
            }
        }
        for wy in y0 as usize..y1 as usize {
            let (a, e) = (wy * mw + x0 as usize, wy * mw + x1 as usize);
            self.solid[a..e].fill(0);
            if let (Some((_, groups)), Some((_, ground))) = (&mut self.glow, g.glow) {
                groups[a..e].copy_from_slice(&ground[a..e]);
            }
            if let Some(beams) = &mut self.beams {
                for i in a..e {
                    beams[2 * i..2 * i + 2].copy_from_slice(&g.beam(i));
                }
            }
        }
        let art = Arc::clone(&self.art);
        for c in 0..self.frame.copies() {
            let Some(pictures) = art.copies.get(c) else { continue };
            for i in 0..self.frame.copy(c).len() {
                let p = self.frame.copy(c)[i];
                let Some(pic) = pictures.get(usize::from(p.picture)) else { continue };
                let Some(pb) = self.part_box(c, &p, g.world) else { continue };
                let (px0, py0, px1, py1) = (pb.0.max(x0), pb.1.max(y0), pb.2.min(x1), pb.3.min(y1));
                if px1 > px0 && py1 > py0 {
                    self.draw(g, pic, &p, (px0, py0, px1, py1));
                }
            }
        }
    }

    /// Draws part `p` of picture `pic` over world box `b` (inside it).
    fn draw(&mut self, g: &Ground<'_>, pic: &LivePicture, p: &LivePlaced, b: WorldBox) {
        let s = g.scale as usize;
        let (pw, ph) = (pic.size.0 as usize, pic.size.1 as usize);
        let (aw, mw, w) = (pw * s, g.world.0 as usize, g.width as usize);
        if pic.art.len() != aw * ph * s * 4 {
            return;
        }
        let (rock, mask, groups) =
            (fits(&pic.rock, pw * ph), fits(&pic.glow, aw * ph * s), fits(&pic.groups, pw * ph));
        let (beam, glass) = (fits(&pic.beam, 2 * pw * ph), fits(&pic.glass, aw * ph * s * 4));
        let glass = glass.filter(|_| !p.rock);
        // The glass's opacity as the frame sets it (the bake's rounding).
        let glass_alpha = p.glass_opacity.map(|o| ((u32::from(o.clamp(1, 99)) * 255 + 50) / 100) as u8);
        if p.rock && rock.is_none() {
            return;
        }
        for wy in b.1 as usize..b.3 as usize {
            let ly = (wy as i64 - i64::from(p.at.1)) as usize;
            for wx in b.0 as usize..b.2 as usize {
                let lx = (wx as i64 - i64::from(p.at.0)) as usize;
                let lx = if p.flip { pw - 1 - lx } else { lx };
                let (cell, local) = (wy * mw + wx, ly * pw + lx);
                if p.rock {
                    if rock.is_some_and(|r| r[local] == 0) {
                        continue;
                    }
                    self.solid[cell] = 1;
                } else if g.cells[cell] != Material::Background as u8 {
                    continue;
                }
                let mut centre = false;
                for sy in 0..s {
                    let src_row = (ly * s + sy) * aw;
                    for sx in 0..s {
                        let ax = lx * s + if p.flip { s - 1 - sx } else { sx };
                        let src_i = src_row + ax;
                        let src = [
                            pic.art[src_i * 4],
                            pic.art[src_i * 4 + 1],
                            pic.art[src_i * 4 + 2],
                            pic.art[src_i * 4 + 3],
                        ];
                        let dst_i = (wy * s + sy) * w + wx * s + sx;
                        let dst = &mut self.pixels[dst_i * 4..dst_i * 4 + 4];
                        // Issue #93: a glass pixel (a glass map, alpha
                        // 1-254) at the frame's opacity, filtered at its tint.
                        let glassy = glass.is_some() && (1..255).contains(&src[3]);
                        let src_a = match glass_alpha {
                            Some(a) if glassy => a,
                            _ => src[3],
                        };
                        if p.rock {
                            dst.copy_from_slice(&[src[0], src[1], src[2], 255]);
                            if let Some(gl) = &mut self.glass {
                                gl[dst_i * 4..dst_i * 4 + 4].copy_from_slice(&NO_GLASS);
                            }
                        } else {
                            let a = ((u32::from(src_a) * u32::from(p.alpha) + 127) / 255) as u8;
                            if a == 0 {
                                continue;
                            }
                            let d = [dst[0], dst[1], dst[2], dst[3]];
                            // Opacity alone never lays over a see-through
                            // pixel (the bake's rule); glass does.
                            if a < 255 && d[3] == 0 && src[3] == 255 {
                                continue;
                            }
                            dst.copy_from_slice(&if a == 255 {
                                [src[0], src[1], src[2], 255]
                            } else {
                                over(src, a, d)
                            });
                            if let Some(gl) = &mut self.glass {
                                let texel = match glass {
                                    Some(m) if glassy => {
                                        let t = &m[src_i * 4..src_i * 4 + 4];
                                        match p.glass_tint {
                                            Some(tint) => {
                                                let tint = u32::from(tint.min(100));
                                                let f = |v: u8| {
                                                    ((255 * (100 - tint) + u32::from(v) * tint + 50) / 100)
                                                        as u8
                                                };
                                                [f(src[0]), f(src[1]), f(src[2]), t[3]]
                                            }
                                            None => [t[0], t[1], t[2], t[3]],
                                        }
                                    }
                                    _ => NO_GLASS,
                                };
                                gl[dst_i * 4..dst_i * 4 + 4].copy_from_slice(&texel);
                            }
                        }
                        centre |= sy == s / 2 && sx == s / 2;
                        if let Some((glow, _)) = &mut self.glow {
                            glow[dst_i] = mask.map_or(0, |m| live_glow(p.glow, m[src_row + ax]));
                        }
                    }
                }
                // A background part over a rock part below it is the
                // topmost there, background as in the static bake: the
                // light passes where its centre art pixel shows.
                if !p.rock && centre {
                    self.solid[cell] = 0;
                }
                // A world pixel's group follows its centre art pixel (a
                // rock part's every solid pixel).
                if let Some((_, gr)) = &mut self.glow
                    && (p.rock || centre)
                {
                    gr[cell] = groups.map_or(0, |v| v[local]);
                }
                // Its beam texel likewise (issue #93), as the frame turns it.
                if let Some(bm) = &mut self.beams
                    && (p.rock || centre)
                {
                    let t = beam.map_or([0, 0], |b| shown_beam([b[2 * local], b[2 * local + 1]], p));
                    bm[2 * cell..2 * cell + 2].copy_from_slice(&t);
                }
            }
        }
    }
}
