//! Colour terrain (spec §19 E7): the level's visible composite image
//! (OLX's `bmpDrawImage`) at art scale, kept in step with the material grid
//! by replaying the simulation's terrain edits in order, plus the stains that
//! blood and gibs leave on it (presentation note §2.1, §2.2, §7.2).
//!
//! The map keeps its own copy of the material grid and applies each edit to
//! it as well, so every colour rule reads the material **before** the edit,
//! exactly as OLX's single loop does. Replaying the same edits in the same
//! order keeps that copy equal to the simulation's grid.

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;

use crate::dirt::{Clip, flags_of};
use crate::dirty::{Dirty, DirtyTiles};
use crate::live::{Ground, Live, LiveArt, LiveFrame};
use crate::{GlassMap, GlowBeamMap};
use crate::{GreenMask, GreenPixel, PX_EMPTY, PX_ROCK, Stamp, StampPixel, Terrain, clip};

/// `PlaceGreenDirt`'s shades (`CMap.cpp:1245-1248`), picked with
/// `GetRandomInt(3)` (0 to 3) per pixel.
pub const GREEN_SHADES: [Rgba; 4] =
    [Rgba::rgb(148, 136, 0), Rgba::rgb(136, 124, 0), Rgba::rgb(124, 112, 0), Rgba::rgb(116, 100, 0)];

/// Dirt's colour when the theme's `FrontTile` is empty: `olr-content`'s
/// placeholder colour.
const NO_TILE: Rgba = Rgba::rgb(255, 0, 220);

/// What a theme gives the colour map: the stamps it edits with and the tile
/// placed dirt is drawn from.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeArt {
    /// `hole1` to `hole5`, as the simulation's.
    pub holes: Vec<Stamp>,
    /// `data/gfx/greenball.png`.
    pub green_mask: Option<GreenMask>,
    /// `FrontTile.png`, tiled from the map origin in world pixels.
    pub front_tile: RgbaImage,
}

/// A terrain edit as the colour map replays it: the simulation's
/// `TerrainEdit` (`olr-content` converts). Positions are the integer
/// position OLX passes to the map function, in world pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorEdit {
    /// `CMap::CarveHole` with hole `hole` (`TerrainOp::Carve` and `Beam`).
    Carve { hole: u8, x: i32, y: i32 },
    /// `CMap::PlaceDirt` with hole `hole`.
    Dirt { hole: u8, x: i32, y: i32 },
    /// `CMap::PlaceGreenDirt`.
    GreenDirt { x: i32, y: i32 },
}

/// A rectangle of the colour image, in art pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A map area in world pixels, `x0..x1` × `y0..y1` (exclusive ends), not
/// necessarily inside the map (such as `olr-sim`'s `LostArea`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

/// The level's colour image and the material grid it is kept in step with.
#[derive(Clone, Debug)]
pub struct ColorMap {
    scale: u32,
    /// Art pixels.
    width: u32,
    height: u32,
    /// The composite, RGBA8, opaque but where see-through.
    pixels: Vec<u8>,
    /// The level's front and back art (opaque but where see-through), for
    /// carving and rebuilding.
    front: Vec<u8>,
    back: Vec<u8>,
    /// The level has distant layers (issue #11): its art keeps alpha 0
    /// where it is see-through.
    see_through: bool,
    material: Terrain,
    theme: ThemeArt,
    /// The green shades' own generator (OLX draws them from the client's `rand()`).
    rng: u64,
    /// What changed since [`ColorMap::take_dirty`] (issue #35: tiles).
    dirty: DirtyTiles,
    /// The glow channel (spec 2026-10-18 look system §Glow), when the level has one.
    glow: Option<Box<Glow>>,
    /// The level's clear flags per world pixel (§Clear solids), as loaded.
    clear: Option<Box<[u8]>>,
    /// The level's glass map (the background glass design), as loaded.
    glass: Option<Box<GlassMap>>,
    /// Counts the glass maps set, so a renderer uploads one only anew.
    glass_generation: u32,
    /// The level's glow beams (the directional-glow design), as loaded.
    glow_beams: Option<Box<GlowBeamMap>>,
    /// The level's haze map (the directional-glow design): grey per world
    /// pixel, as loaded.
    haze: Option<Box<[u8]>>,
    /// Counts the glow beam and haze maps set, so a renderer derives and
    /// uploads them only anew.
    beams_generation: u32,
    /// The live copies drawn over the ground (issue #87), when the level
    /// has any.
    live: Option<Box<Live>>,
}

/// A level's glow (the look system's `look/` images): strength per art pixel
/// and group per world pixel, of the composite at load (`front`, which
/// equals `back` on background) and of the backdrop digging reveals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlowLayers {
    /// Art pixels (the colour map's size).
    pub art: (u32, u32),
    /// World pixels (the material's size).
    pub world: (u32, u32),
    /// Strength 0-255, art size, row-major.
    pub front: Vec<u8>,
    pub back: Vec<u8>,
    /// Glow group index per world pixel, row-major.
    pub groups: Vec<u8>,
    pub groups_back: Vec<u8>,
}

impl GlowLayers {
    /// No glow anywhere.
    pub fn empty(art: (u32, u32), world: (u32, u32)) -> GlowLayers {
        let a = art.0 as usize * art.1 as usize;
        let w = world.0 as usize * world.1 as usize;
        GlowLayers {
            art,
            world,
            front: vec![0; a],
            back: vec![0; a],
            groups: vec![0; w],
            groups_back: vec![0; w],
        }
    }

    /// The sizes match `art` and `world` and every buffer holds them.
    pub fn fits(&self, art: (u32, u32), world: (u32, u32)) -> bool {
        let a = art.0 as usize * art.1 as usize;
        let w = world.0 as usize * world.1 as usize;
        self.art == art
            && self.world == world
            && self.front.len() == a
            && self.back.len() == a
            && self.groups.len() == w
            && self.groups_back.len() == w
    }
}

/// The colour map's glow: the layers as loaded, and the composite the edits change.
#[derive(Clone, Debug)]
struct Glow {
    layers: GlowLayers,
    /// Strength, art size.
    comp: Vec<u8>,
    /// Group, world size.
    comp_groups: Vec<u8>,
}

/// `img` as opaque RGBA8 of `w` × `h`: the part that fits is copied, the
/// rest is black (a level pack guarantees the size; this never panics).
fn fit(img: &RgbaImage, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    let (iw, ih) = (img.width().min(w) as usize, img.height().min(h));
    for y in 0..h {
        if y < ih {
            let row = y as usize * img.width() as usize * 4;
            out.extend_from_slice(&img.as_raw()[row..row + iw * 4]);
        }
        out.resize((y as usize + 1) * w as usize * 4, 0);
    }
    for p in out.as_chunks_mut::<4>().0 {
        p[3] = 255;
    }
    out
}

impl ColorMap {
    /// The side of a dirty tile, world pixels (issue #35): every change
    /// marks the tiles under it, and [`ColorMap::take_dirty`] gives each
    /// row's runs of them.
    pub const DIRTY_TILE: u32 = 16;

    /// The composite of a level (presentation note §2.1): `front` is
    /// already the visible image (it equals `back` where the material is
    /// background), `back` is revealed by carving, both `art_scale` art
    /// pixels per world pixel of `material`. The map starts clean: the
    /// caller uploads [`ColorMap::pixels`] once, then the dirty rectangles.
    pub fn new(
        material: &Terrain,
        front: &RgbaImage,
        back: &RgbaImage,
        art_scale: u32,
        theme: ThemeArt,
    ) -> Self {
        let size = |s: u32| {
            let (w, h) = (material.width().checked_mul(s)?, material.height().checked_mul(s)?);
            (w as usize).checked_mul(h as usize)?.checked_mul(4)?;
            Some((w, h))
        };
        let scale = art_scale.max(1);
        let (scale, (width, height)) = match size(scale) {
            Some(wh) => (scale, wh),
            None => (1, (material.width(), material.height())),
        };
        let front = fit(front, width, height);
        Self {
            scale,
            width,
            height,
            pixels: front.clone(),
            back: fit(back, width, height),
            front,
            material: material.clone(),
            theme,
            rng: 0x9e37_79b9_7f4a_7c15,
            dirty: DirtyTiles::new((width, height), Self::DIRTY_TILE * scale),
            glow: None,
            clear: None,
            glass: None,
            glass_generation: 0,
            glow_beams: None,
            haze: None,
            beams_generation: 0,
            see_through: false,
            live: None,
        }
    }

    /// [`ColorMap::new`] for a level with distant layers (issue #11): the
    /// back art's pixels of alpha 0 stay see-through (alpha 0) in the map's
    /// back, and so in its front and composite where the material is
    /// background, and so does glass, alpha 1 to 254 (the background glass
    /// design); every other pixel is opaque. Carving reveals them;
    /// stains never land on them.
    pub fn new_see_through(
        material: &Terrain,
        front: &RgbaImage,
        back: &RgbaImage,
        art_scale: u32,
        theme: ThemeArt,
    ) -> Self {
        let mut map = Self::new(material, front, back, art_scale, theme);
        map.see_through = true;
        let (w, h) = (map.width, map.height);
        let s = map.scale;
        let mw = map.material.width() as usize;
        for y in 0..h.min(back.height()) {
            for x in 0..w.min(back.width()) {
                let a = back.get(x, y).a;
                if a == 255 {
                    continue;
                }
                let i = (y as usize * w as usize + x as usize) * 4 + 3;
                map.back[i] = a;
                let cell = (y / s) as usize * mw + (x / s) as usize;
                if map.material.cells[cell] == Material::Background as u8 {
                    map.front[i] = a;
                    map.pixels[i] = a;
                }
            }
        }
        map
    }

    /// Whether the map keeps see-through pixels ([`ColorMap::new_see_through`]).
    pub fn see_through(&self) -> bool {
        self.see_through
    }

    /// Gives the map a glow channel (`None` removes it). Layers that do not
    /// fit this map's art and world sizes are refused (`false`) and the map
    /// keeps what it had. The composite starts as `front`; set it before the
    /// first upload, as nothing is marked dirty.
    pub fn set_glow(&mut self, glow: Option<GlowLayers>) -> bool {
        let Some(layers) = glow else {
            self.glow = None;
            self.compose_live();
            return true;
        };
        if !layers.fits((self.width, self.height), (self.material.width(), self.material.height())) {
            return false;
        }
        let (comp, comp_groups) = (layers.front.clone(), layers.groups.clone());
        self.glow = Some(Box::new(Glow { layers, comp, comp_groups }));
        self.compose_live();
        true
    }

    /// The composite glow strength, art size, row-major; `None` without glow.
    /// Sets the level's clear flags (spec §Clear solids: a solid cell whose
    /// flag is not 0 lets light through), one byte per world pixel,
    /// row-major; flags of another size are refused (`false`). They are the
    /// level's as loaded: play edits leave them, the flag only counts where
    /// the cell is solid.
    pub fn set_clear(&mut self, clear: Option<Vec<u8>>) -> bool {
        match clear {
            Some(c)
                if c.len() as u64 != u64::from(self.material.width()) * u64::from(self.material.height()) =>
            {
                false
            }
            c => {
                self.clear = c.map(Vec::into_boxed_slice);
                true
            }
        }
    }

    /// Sets the level's glass map (the background glass design): what its
    /// glass does to the distant layers behind it, per art pixel; a map of
    /// another art size is refused (`false`) and the map keeps what it had.
    /// Play never changes it.
    pub fn set_glass(&mut self, glass: Option<GlassMap>) -> bool {
        match glass {
            Some(g) if !g.fits((self.width, self.height)) => false,
            g => {
                self.glass = g.map(Box::new);
                self.glass_generation = self.glass_generation.wrapping_add(1);
                true
            }
        }
    }

    /// The level's glass map; `None` when no glass tints or frosts.
    pub fn glass(&self) -> Option<&GlassMap> {
        self.glass.as_deref()
    }

    /// Changes with each glass map [`set_glass`](Self::set_glass) takes.
    pub fn glass_generation(&self) -> u32 {
        self.glass_generation
    }

    /// Sets the level's glow beams (the directional-glow design): where its
    /// glow is directional, per world pixel of the composite and of the
    /// backdrop; a map that does not fit the world size is refused
    /// (`false`) and the map keeps what it had. Play never changes it.
    pub fn set_glow_beams(&mut self, beams: Option<GlowBeamMap>) -> bool {
        match beams {
            Some(b) if !b.fits((self.material.width(), self.material.height())) => false,
            b => {
                self.glow_beams = b.map(Box::new);
                self.beams_generation = self.beams_generation.wrapping_add(1);
                true
            }
        }
    }

    /// The level's glow beams; `None` when no glow is directional.
    pub fn glow_beams(&self) -> Option<&GlowBeamMap> {
        self.glow_beams.as_deref()
    }

    /// Sets the level's haze map (the directional-glow design): grey per
    /// world pixel, row-major, 0 none to 255 the level's haze; a map of
    /// another size is refused (`false`) and the map keeps what it had.
    /// Play never changes it.
    pub fn set_haze(&mut self, haze: Option<Vec<u8>>) -> bool {
        let cells = u64::from(self.material.width()) * u64::from(self.material.height());
        match haze {
            Some(h) if h.len() as u64 != cells => false,
            h => {
                self.haze = h.map(Vec::into_boxed_slice);
                self.beams_generation = self.beams_generation.wrapping_add(1);
                true
            }
        }
    }

    /// The level's haze map; `None` when the haze is the same everywhere.
    pub fn haze(&self) -> Option<&[u8]> {
        self.haze.as_deref()
    }

    /// Changes with each glow beam or haze map
    /// [`set_glow_beams`](Self::set_glow_beams) and
    /// [`set_haze`](Self::set_haze) take, and on a level with glow beams
    /// with each [`replace_glow_rect`](Self::replace_glow_rect) and
    /// [`replace_rect`](Self::replace_rect) (the editor's uploads change the
    /// beams' strength, group and colour).
    pub fn beams_generation(&self) -> u32 {
        self.beams_generation
    }

    /// The glow layers as loaded (and as `replace_glow_rect` changed
    /// them); `None` without glow.
    pub fn glow_layers(&self) -> Option<&GlowLayers> {
        self.glow.as_ref().map(|g| &g.layers)
    }

    /// The level's front art (the composite as loaded) and back art, RGBA8
    /// at art size, row-major (opaque but where see-through).
    pub fn art_layers(&self) -> (&[u8], &[u8]) {
        (&self.front, &self.back)
    }

    /// The level's clear flags per world pixel; `None` when nothing is clear.
    pub fn clear(&self) -> Option<&[u8]> {
        self.clear.as_deref()
    }

    /// The shown glow strength, art size, row-major (with live copies, issue
    /// #87, theirs over the ground's); `None` without glow.
    pub fn glow(&self) -> Option<&[u8]> {
        match &self.live {
            Some(l) => l.glow.as_ref().map(|(a, _)| &a[..]),
            None => self.glow.as_ref().map(|g| &g.comp[..]),
        }
    }

    /// The shown glow group per world pixel, row-major (with live copies,
    /// theirs over the ground's); `None` without glow.
    pub fn glow_groups(&self) -> Option<&[u8]> {
        match &self.live {
            Some(l) => l.glow.as_ref().map(|(_, w)| &w[..]),
            None => self.glow.as_ref().map(|g| &g.comp_groups[..]),
        }
    }

    /// [`ColorMap::replace_rect`]'s glow: copies art rectangle `r` of
    /// `glow`'s strengths (front into the composite too) and the groups of
    /// the world pixels under it, then marks `r` dirty. Layers of another
    /// size, or a map without glow, change nothing; `r` is clipped.
    pub fn replace_glow_rect(&mut self, r: Rect, glow: &GlowLayers) {
        let (art, world) = ((self.width, self.height), (self.material.width(), self.material.height()));
        let Some(g) = self.glow.as_deref_mut() else { return };
        if !glow.fits(art, world) {
            return;
        }
        let (x0, y0) = (r.x.min(art.0), r.y.min(art.1));
        let (x1, y1) = (r.x.saturating_add(r.w).min(art.0), r.y.saturating_add(r.h).min(art.1));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let w = art.0 as usize;
        for y in y0..y1 {
            let (a, b) = (y as usize * w + x0 as usize, y as usize * w + x1 as usize);
            g.layers.front[a..b].copy_from_slice(&glow.front[a..b]);
            g.layers.back[a..b].copy_from_slice(&glow.back[a..b]);
            g.comp[a..b].copy_from_slice(&glow.front[a..b]);
        }
        let (s, mw) = (self.scale, world.0 as usize);
        for wy in y0 / s..y1.div_ceil(s) {
            for wx in x0 / s..x1.div_ceil(s) {
                let i = wy as usize * mw + wx as usize;
                g.layers.groups[i] = glow.groups[i];
                g.layers.groups_back[i] = glow.groups_back[i];
                g.comp_groups[i] = glow.groups[i];
            }
        }
        self.push_dirty(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
        self.beams_changed();
    }

    /// The beams' inputs changed (the editor's upload of glow or art: a
    /// directional element's strength, group or colour, which leave the
    /// beam map as it is): a new beams generation when the level has beams,
    /// so the renderer derives them again (final review I1). Without beams
    /// nothing changes.
    fn beams_changed(&mut self) {
        if self.glow_beams.is_some() {
            self.beams_generation = self.beams_generation.wrapping_add(1);
        }
    }

    /// Width of the colour image in art pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height of the colour image in art pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Art pixels per world pixel.
    pub fn art_scale(&self) -> u32 {
        self.scale
    }

    /// The colour image shown, RGBA8, row-major, opaque (alpha 0 where
    /// see-through, only in a map made by [`ColorMap::new_see_through`]):
    /// the ground, with the live copies over it (issue #87) as of the last
    /// [`ColorMap::set_live`], [`ColorMap::settle_live`] or
    /// [`ColorMap::take_dirty`].
    pub fn pixels(&self) -> &[u8] {
        match &self.live {
            Some(l) => &l.pixels,
            None => &self.pixels,
        }
    }

    /// The ground's colour image, without live copies (issue #87): what
    /// the edits keep in step with [`ColorMap::material`].
    pub fn ground_pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The ground, as the live layer's composition reads it.
    fn ground(&self) -> Ground<'_> {
        Ground {
            scale: self.scale,
            width: self.width,
            pixels: &self.pixels,
            glow: self.glow.as_deref().map(|g| (&g.comp[..], &g.comp_groups[..])),
            cells: &self.material.cells,
            world: (self.material.width(), self.material.height()),
        }
    }

    /// Gives the map its live copies' pictures (issue #87; `None` removes
    /// them): nothing is drawn until [`ColorMap::set_live`]. When a
    /// picture glows and the map has no glow channel (a level lit only by
    /// live copies), it gets an empty one ([`GlowLayers::empty`]), so the
    /// renderer uploads glow and groups. Set it before the first upload.
    pub fn set_live_art(&mut self, art: Option<Arc<LiveArt>>) {
        let Some(art) = art else {
            self.live = None;
            return;
        };
        if art.glows() && self.glow.is_none() {
            let (a, w) = ((self.width, self.height), (self.material.width(), self.material.height()));
            self.set_glow(Some(GlowLayers::empty(a, w)));
        }
        let live = Live::new(art, &self.ground(), Self::DIRTY_TILE * self.scale);
        self.live = Some(Box::new(live));
    }

    /// Whether the map draws live copies ([`ColorMap::set_live_art`]).
    pub fn has_live(&self) -> bool {
        self.live.is_some()
    }

    /// Draws the live copies' parts of `frame` over the ground (issue #87):
    /// the ground's edits since the last call are settled first, then each
    /// copy whose parts changed (picture, place, facing, opacity or glow)
    /// is recomposed where it was and where it is, those rectangles marked
    /// dirty. An unchanged frame changes and marks nothing. Without
    /// [`ColorMap::set_live_art`] it does nothing. Allocates nothing once
    /// warm.
    pub fn set_live(&mut self, frame: &LiveFrame) {
        self.settle_live();
        let Some(mut live) = self.live.take() else { return };
        let s = self.scale;
        for &(x0, y0, x1, y1) in live.set_frame(frame, &self.ground()) {
            self.dirty.mark(Rect { x: x0 * s, y: y0 * s, w: (x1 - x0) * s, h: (y1 - y0) * s }, false);
        }
        self.live = Some(live);
    }

    /// Brings the shown composite up to the ground's edits (issue #87):
    /// [`ColorMap::set_live`] and [`ColorMap::take_dirty`] do it first.
    pub fn settle_live(&mut self) {
        if let Some(mut live) = self.live.take() {
            live.settle(&self.ground());
            self.live = Some(live);
        }
    }

    /// Recomposes the whole shown composite (a new glow channel).
    fn compose_live(&mut self) {
        if let Some(mut live) = self.live.take() {
            live.compose_all(&self.ground());
            self.live = Some(live);
        }
    }

    /// The live copies' solid pixels (issue #87): per world pixel, 1 where
    /// a shown rock part is solid; `None` without live copies. The
    /// renderer's solid map counts them as rock (they occlude light and
    /// take light soak); the material never does.
    pub fn live_solid(&self) -> Option<&[u8]> {
        self.live.as_ref().map(|l| &l.solid[..])
    }

    /// The material grid as the replayed edits left it.
    pub fn material(&self) -> &Terrain {
        &self.material
    }

    /// OLX `CMap::getColorAt` (`CMap.cpp:2355-2362`): the colour of world
    /// pixel `(x, y)` (its top-left art pixel), `None` outside the map.
    pub fn color_at(&self, x: i64, y: i64) -> Option<Rgba> {
        self.material.material(x, y)?;
        let i =
            ((y as usize * self.scale as usize) * self.width as usize + x as usize * self.scale as usize) * 4;
        let p = &self.pixels[i..i + 4];
        Some(Rgba::rgb(p[0], p[1], p[2]))
    }

    /// Replays one terrain edit (after the simulation made it, in the same order).
    pub fn apply(&mut self, edit: ColorEdit) {
        match edit {
            ColorEdit::Carve { hole, x, y } => self.carve(hole as usize, x, y),
            ColorEdit::Dirt { hole, x, y } => self.place_dirt(hole as usize, x, y),
            ColorEdit::GreenDirt { x, y } => self.place_green_dirt(x, y),
        }
    }

    /// The dirty rectangles since the last call, in art pixels; together
    /// they cover every pixel that changed. Each is a run of
    /// [`ColorMap::DIRTY_TILE`] tiles along a row, clipped to the map
    /// (issue #35); dropped early, the iterator forgets the rest too.
    pub fn take_dirty(&mut self) -> impl Iterator<Item = Rect> + '_ {
        self.settle_live();
        self.dirty.drain().map(|d| d.rect)
    }

    /// [`ColorMap::take_dirty`] with each rectangle's kind (issue #35):
    /// `colour_only` where only blood and gib stains changed it, so its
    /// material and glow are as they were.
    pub fn take_dirty_kinds(&mut self) -> impl Iterator<Item = Dirty> + '_ {
        self.settle_live();
        self.dirty.drain()
    }

    /// Blood that hit rock or dirt: OLX `CMap::putColorTo` (`CMap.cpp:2365-2368`,
    /// `Entity.cpp:363-369`) paints world pixel `(x, y)` (OLX's 2×2 block)
    /// in `color`. Only the colour changes, never the material.
    pub fn stain(&mut self, x: i32, y: i32, color: Rgba) {
        if self.material.material(x as i64, y as i64).is_none() {
            return;
        }
        let (x, y) = (x as u32, y as u32);
        self.stain_block(x, y, color);
        self.mark_colour(x as i64, y as i64, x as i64 + 1, y as i64 + 1);
    }

    /// A gib that came to rest: OLX `CMap::putSurfaceTo` with
    /// `DrawImageStretch2` (`CMap.cpp:2370-2373`, `Entity.cpp:373-389`,
    /// `GfxPrimitives.cpp`): the `src_rect` part of `src` (the worm's
    /// colorized `giblets.png`, a 4 × 4 frame at `(rotation * 4, 8)`), one
    /// world pixel per source pixel, with its top-left at world `(x, y)`.
    /// Magenta (the colour key) and fully transparent pixels are skipped.
    /// Kept quirk: clipped at the map's left or top edge, the source is still
    /// read from its first column and row. With `on_air` (OLX, the oracle's
    /// `olx_quirks`) it paints open air too, where nothing ever clears it
    /// (the playtest's floating gib pixels); in play only rock and dirt.
    pub fn stain_image(&mut self, x: i32, y: i32, src: &RgbaImage, src_rect: Rect, on_air: bool) {
        // Source clipping first; the destination box uses the requested size.
        let (sx0, sy0) = (src_rect.x.min(src.width()), src_rect.y.min(src.height()));
        let sw = src_rect.x.saturating_add(src_rect.w).min(src.width()) - sx0;
        let sh = src_rect.y.saturating_add(src_rect.h).min(src.height()) - sy0;
        let (x, y) = (x as i64, y as i64);
        let (dx0, dy0) = (x.max(0), y.max(0));
        let dx1 = (x + src_rect.w as i64).min(self.material.width() as i64);
        let dy1 = (y + src_rect.h as i64).min(self.material.height() as i64);
        let (w, h) = ((dx1 - dx0).min(sw as i64), (dy1 - dy0).min(sh as i64));
        if w <= 0 || h <= 0 {
            return;
        }
        for row in 0..h {
            for col in 0..w {
                let c = src.get(sx0 + col as u32, sy0 + row as u32);
                let air = self.material.material(dx0 + col, dy0 + row) == Some(Material::Background);
                if !c.same_rgb(Rgba::MAGENTA) && c.a != 0 && (on_air || !air) {
                    self.stain_block((dx0 + col) as u32, (dy0 + row) as u32, c);
                }
            }
        }
        self.mark_colour(dx0, dy0, dx0 + w, dy0 + h);
    }

    /// Redraws `area` from `material` and takes its material there: the
    /// back image where it is background, the theme's `FrontTile` where it
    /// is dirt, the level's art where it is rock. This is the fallback for
    /// terrain edits the simulation could not report (`TickEvents::lost_area`):
    /// edits outside the area are exact, inside it only the stains and the
    /// original dirt art are lost.
    pub fn rebuild_area(&mut self, area: Area, material: &Terrain) {
        let mw = self.material.width().min(material.width()) as i64;
        let mh = self.material.height().min(material.height()) as i64;
        let (x0, y0) = ((area.x0 as i64).clamp(0, mw), (area.y0 as i64).clamp(0, mh));
        let (x1, y1) = ((area.x1 as i64).clamp(x0, mw), (area.y1 as i64).clamp(y0, mh));
        for y in y0..y1 {
            for x in x0..x1 {
                let m = material.material(x, y).unwrap_or(Material::Rock);
                let i = y as usize * self.material.width() as usize + x as usize;
                self.material.cells[i] = m as u8;
                let (x, y) = (x as u32, y as u32);
                match m {
                    Material::Background => {
                        self.copy_block(x, y, false);
                        glow_from(
                            &mut self.glow,
                            (self.width, self.scale, self.material.width()),
                            x,
                            y,
                            false,
                        );
                    }
                    Material::Dirt => {
                        self.put(x, y, self.tile(x, y));
                        glow_none(&mut self.glow, (self.width, self.scale, self.material.width()), x, y);
                    }
                    Material::Rock => {
                        self.copy_block(x, y, true);
                        glow_from(
                            &mut self.glow,
                            (self.width, self.scale, self.material.width()),
                            x,
                            y,
                            true,
                        );
                    }
                }
            }
        }
        self.mark(x0, y0, x1, y1);
    }

    /// Copies art rectangle `r` of `front` and `back` (images of this map's
    /// art size) into the map's own front, back and composited pixels (the
    /// composite takes `front`), and the world cells under `r` from
    /// `material` (the level's cells, row-major), then marks `r` dirty.
    /// Images or a material of another size change nothing. `r` is clipped
    /// to the map; only its pixels and cells are read. The editor's canvas
    /// uploads an edit's bake this way.
    pub fn replace_rect(&mut self, r: Rect, front: &RgbaImage, back: &RgbaImage, material: &[Material]) {
        let art = (self.width, self.height);
        let cells = self.material.width() as usize * self.material.height() as usize;
        if (front.width(), front.height()) != art
            || (back.width(), back.height()) != art
            || material.len() != cells
        {
            return;
        }
        let (x0, y0) = (r.x.min(self.width), r.y.min(self.height));
        let (x1, y1) = (r.x.saturating_add(r.w).min(self.width), r.y.saturating_add(r.h).min(self.height));
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let w = self.width as usize;
        for y in y0..y1 {
            let (a, b) = ((y as usize * w + x0 as usize) * 4, (y as usize * w + x1 as usize) * 4);
            self.front[a..b].copy_from_slice(&front.as_raw()[a..b]);
            self.back[a..b].copy_from_slice(&back.as_raw()[a..b]);
            // Opaque, but where see-through in a see-through map.
            let keep = self.see_through;
            for p in self.front[a..b]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .chain(self.back[a..b].as_chunks_mut::<4>().0)
            {
                p[3] = if keep { p[3] } else { 255 };
            }
            self.pixels[a..b].copy_from_slice(&self.front[a..b]);
        }
        let s = self.scale;
        let mw = self.material.width() as usize;
        for wy in y0 / s..y1.div_ceil(s) {
            for wx in x0 / s..x1.div_ceil(s) {
                let i = wy as usize * mw + wx as usize;
                self.material.cells[i] = material[i] as u8;
            }
        }
        self.push_dirty(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
        self.beams_changed();
    }

    /// OLX `CMap::CarveHole` (`CMap.cpp:1005-1105`), hi-res branch: under a
    /// magenta stamp pixel dirt becomes background and shows the back image
    /// (`CopyPixel2x2`); under any other non-black pixel it stays dirt,
    /// painted in that colour (the brown rim). Rock and background are left
    /// alone. Clipped like the material (the stamp is not offset).
    fn carve(&mut self, hole: usize, pos_x: i32, pos_y: i32) {
        let Some(stamp) = self.theme.holes.get(hole) else { return };
        let (w, h) = (stamp.width() as i32, stamp.height() as i32);
        let (mut x, mut y, mut cw, mut ch) = (pos_x.wrapping_sub(w / 2), pos_y.wrapping_sub(h / 2), w, h);
        let (mw, mh) = (self.material.width() as i32, self.material.height() as i32);
        if !clip(&mut x, &mut cw, mw) || !clip(&mut y, &mut ch, mh) {
            return;
        }
        let mut changed = false;
        for row in 0..ch {
            for col in 0..cw {
                let (mx, my) = ((x + col) as u32, (y + row) as u32);
                if self.material.cells[my as usize * mw as usize + mx as usize] != Material::Dirt as u8 {
                    continue;
                }
                match stamp.get(col as u32, row as u32) {
                    StampPixel::Carve => {
                        copy(&mut self.pixels, &self.back, self.width, self.scale, mx, my);
                        glow_from(
                            &mut self.glow,
                            (self.width, self.scale, self.material.width()),
                            mx,
                            my,
                            false,
                        );
                    }
                    StampPixel::Paint(c) => fill(&mut self.pixels, self.width, self.scale, mx, my, c),
                    StampPixel::Skip => continue,
                }
                changed = true;
            }
        }
        self.material.carve_hole(stamp, pos_x, pos_y);
        if changed {
            self.mark(x as i64, y as i64, (x + cw) as i64, (y + ch) as i64);
        }
    }

    /// OLX `CMap::PlaceDirt` (`CMap.cpp:1114-1224`), hi-res branch: under a
    /// non-black stamp pixel, anything but rock becomes dirt coloured from
    /// the theme's `FrontTile` at `(x % tw, y % th)`; then, where it was
    /// background, a non-magenta stamp pixel paints its own colour.
    fn place_dirt(&mut self, hole: usize, pos_x: i32, pos_y: i32) {
        let Some(stamp) = self.theme.holes.get(hole) else { return };
        let (w, h) = (stamp.width(), stamp.height());
        let (sx, sy) = (pos_x.wrapping_sub((w >> 1) as i32), pos_y.wrapping_sub((h >> 1) as i32));
        let c = Clip::new(&self.material, sx, sy, w, h);
        let mw = self.material.width();
        let mut changed = false;
        for (i, x, y) in c.cells(mw) {
            let (mx, my) = ((i % mw as usize) as u32, (i / mw as usize) as u32);
            let (p, flag) = (stamp.get(x, y), flags_of(self.material.cells[i]));
            if p != StampPixel::Skip && flag & PX_ROCK == 0 {
                let t = tile(&self.theme.front_tile, mx, my);
                fill(&mut self.pixels, self.width, self.scale, mx, my, t);
                glow_none(&mut self.glow, (self.width, self.scale, self.material.width()), mx, my);
                changed = true;
            }
            if let StampPixel::Paint(rim) = p
                && flag & PX_EMPTY != 0
            {
                fill(&mut self.pixels, self.width, self.scale, mx, my, rim);
            }
        }
        self.material.place_dirt(stamp, pos_x, pos_y);
        if changed {
            self.mark(c.x0, c.y0, c.x1, c.y1);
        }
    }

    /// OLX `CMap::PlaceGreenDirt` (`CMap.cpp:1231-1340`), hi-res branch:
    /// background under a green mask pixel becomes dirt in one of
    /// [`GREEN_SHADES`] (this map's own draw, row-major, one per pixel);
    /// under the rim colour, dirt in that colour.
    fn place_green_dirt(&mut self, pos_x: i32, pos_y: i32) {
        let Some(mask) = &self.theme.green_mask else { return };
        let (w, h) = (mask.width(), mask.height());
        let (sx, sy) = (pos_x.wrapping_sub((w >> 1) as i32), pos_y.wrapping_sub((h >> 1) as i32));
        let c = Clip::new(&self.material, sx, sy, w, h);
        let mw = self.material.width();
        let mut changed = false;
        for (i, x, y) in c.cells(mw) {
            if flags_of(self.material.cells[i]) & PX_EMPTY == 0 {
                continue;
            }
            let colour = match mask.get(x, y) {
                GreenPixel::Green => GREEN_SHADES[next_shade(&mut self.rng)],
                GreenPixel::Rim(rim) => rim,
                GreenPixel::Pink => continue,
            };
            let (mx, my) = ((i % mw as usize) as u32, (i / mw as usize) as u32);
            fill(&mut self.pixels, self.width, self.scale, mx, my, colour);
            glow_none(&mut self.glow, (self.width, self.scale, self.material.width()), mx, my);
            changed = true;
        }
        self.material.place_green_dirt(mask, pos_x, pos_y, &mut || 0);
        if changed {
            self.mark(c.x0, c.y0, c.x1, c.y1);
        }
    }

    fn tile(&self, x: u32, y: u32) -> Rgba {
        tile(&self.theme.front_tile, x, y)
    }

    fn put(&mut self, x: u32, y: u32, c: Rgba) {
        fill(&mut self.pixels, self.width, self.scale, x, y, c);
    }

    /// Stains world pixel `(x, y)`'s block in `c`, but its see-through
    /// pixels (a see-through map's alpha 0 and its glass: blood never hangs
    /// in the air before the distant layers, nor lands on glass).
    fn stain_block(&mut self, x: u32, y: u32, c: Rgba) {
        if !self.see_through {
            fill(&mut self.pixels, self.width, self.scale, x, y, c);
            return;
        }
        let rgba = [c.r, c.g, c.b, 255];
        let (w, s) = (self.width as usize, self.scale as usize);
        for row in y as usize * s..(y as usize + 1) * s {
            let start = (row * w + x as usize * s) * 4;
            for p in self.pixels[start..start + s * 4].as_chunks_mut::<4>().0 {
                if p[3] == 255 {
                    p.copy_from_slice(&rgba);
                }
            }
        }
    }

    fn copy_block(&mut self, x: u32, y: u32, front: bool) {
        let src = if front { &self.front } else { &self.back };
        copy(&mut self.pixels, src, self.width, self.scale, x, y);
    }

    /// Marks world box `x0..x1` × `y0..y1` (inside the map) dirty.
    fn mark(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) {
        self.mark_box(x0, y0, x1, y1, false);
    }

    /// [`ColorMap::mark`] where only the colour changed (a stain).
    fn mark_colour(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) {
        self.mark_box(x0, y0, x1, y1, true);
    }

    fn mark_box(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, colour_only: bool) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let s = self.scale;
        let r = Rect { x: x0 as u32 * s, y: y0 as u32 * s, w: (x1 - x0) as u32 * s, h: (y1 - y0) as u32 * s };
        self.dirty.mark(r, colour_only);
        self.mark_stale(r);
    }

    /// Marks art rectangle `r` (inside the map) dirty, more than its colour.
    fn push_dirty(&mut self, r: Rect) {
        self.dirty.mark(r, false);
        self.mark_stale(r);
    }

    /// The ground changed in art rectangle `r`: the live copies' shown
    /// composite is recomposed there when settled (issue #87).
    fn mark_stale(&mut self, r: Rect) {
        if let Some(l) = self.live.as_deref_mut() {
            l.stale.mark(r, false);
        }
    }
}

/// `GetRandomInt(3)` with the map's own xorshift64* generator: 0 to 3.
fn next_shade(state: &mut u64) -> usize {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    (state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 62) as usize
}

/// The theme's `FrontTile` at world pixel `(x, y)`, tiled from the origin.
fn tile(t: &RgbaImage, x: u32, y: u32) -> Rgba {
    if t.width() == 0 || t.height() == 0 {
        return NO_TILE;
    }
    t.get(x % t.width(), y % t.height())
}

/// Paints world pixel `(x, y)`'s `s` × `s` block of an image `w` art pixels wide, opaque.
fn fill(dst: &mut [u8], w: u32, s: u32, x: u32, y: u32, c: Rgba) {
    let c = [c.r, c.g, c.b, 255];
    let (w, s) = (w as usize, s as usize);
    for row in y as usize * s..(y as usize + 1) * s {
        let start = (row * w + x as usize * s) * 4;
        for p in dst[start..start + s * 4].as_chunks_mut::<4>().0 {
            p.copy_from_slice(&c);
        }
    }
}

/// Copies world pixel `(x, y)`'s block from `src` to `dst` (OLX's `CopyPixel2x2`).
fn copy(dst: &mut [u8], src: &[u8], w: u32, s: u32, x: u32, y: u32) {
    let (w, s) = (w as usize, s as usize);
    for row in y as usize * s..(y as usize + 1) * s {
        let start = (row * w + x as usize * s) * 4;
        dst[start..start + s * 4].copy_from_slice(&src[start..start + s * 4]);
    }
}

/// World pixel `(x, y)` takes the loaded front's or back's glow and group
/// (art `w` wide at scale `s`, world `mw` wide).
fn glow_from(glow: &mut Option<Box<Glow>>, (w, s, mw): (u32, u32, u32), x: u32, y: u32, front: bool) {
    if let Some(g) = glow.as_deref_mut() {
        let (src, groups) =
            if front { (&g.layers.front, &g.layers.groups) } else { (&g.layers.back, &g.layers.groups_back) };
        copy_bytes(&mut g.comp, src, w, s, x, y);
        let i = y as usize * mw as usize + x as usize;
        g.comp_groups[i] = groups[i];
    }
}

/// World pixel `(x, y)` glows no more (dirt the simulation placed).
fn glow_none(glow: &mut Option<Box<Glow>>, (w, s, mw): (u32, u32, u32), x: u32, y: u32) {
    if let Some(g) = glow.as_deref_mut() {
        let (w, s) = (w as usize, s as usize);
        for row in y as usize * s..(y as usize + 1) * s {
            let start = row * w + x as usize * s;
            g.comp[start..start + s].fill(0);
        }
        g.comp_groups[y as usize * mw as usize + x as usize] = 0;
    }
}

/// Copies world pixel `(x, y)`'s block of a one-byte-per-art-pixel image.
fn copy_bytes(dst: &mut [u8], src: &[u8], w: u32, s: u32, x: u32, y: u32) {
    let (w, s) = (w as usize, s as usize);
    for row in y as usize * s..(y as usize + 1) * s {
        let start = row * w + x as usize * s;
        dst[start..start + s].copy_from_slice(&src[start..start + s]);
    }
}

#[cfg(test)]
#[path = "color_tests.rs"]
mod tests;
