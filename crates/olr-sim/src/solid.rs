//! The one terrain query (issue #90): the ground and the live rock parts.
//! No OLX counterpart.
//!
//! [`Solid`] reads a pixel as [`Terrain`] does, except where a visible live
//! rock part covers it (its picture's rock mask at its whole-pixel place,
//! mirrored by its copy's `Facing`): there it reads rock. Every solidity read
//! of the simulation goes through it; the dirt reads (digging, explosions'
//! dirt probes) and every carve and dirt edit act on the ground alone, so a
//! part is never dug.
//!
//! The sites are generic over [`Solidity`], implemented by [`Terrain`]
//! (today's code, monomorphised) and by [`OnParts`] (the ground and the
//! parts); a world picks one once per call through [`with_solid!`], outside
//! the hot loops, so a world without live rock parts reads exactly what it
//! read before, as fast.
//!
//! [`SolidIndex`] is derived from the world's live copies (rebuilt when a
//! footprint changes, after a restore and a state load): the parts' places,
//! the rectangle around them all (a read outside it is one comparison) and
//! a 32-pixel tile grid over the map with a bit per part. It is in no
//! checksum and no state transfer, and a rollback snapshot does not copy it
//! (`World::save`).

use olr_data::level::Material;
use olr_terrain::{PX_ROCK, Terrain};

use crate::buf::Buf;
use crate::live::LiveLevel;

/// The most live rock parts a level's played form may show at once (each
/// copy's state with the most of them, summed): the index keeps a bit per
/// part. More are refused at setup ([`crate::SetupError::Live`]) and by the
/// level checks.
pub const MAX_LIVE_SOLID_PARTS: usize = 64;

/// The most rock parts `level` can show at once: each copy's state with
/// the most of them, summed.
pub(crate) fn most_rock_parts(level: &LiveLevel) -> usize {
    level
        .copies
        .iter()
        .map(|c| {
            let rock = |p: &&crate::LivePart| p.kind == crate::LivePartKind::Rock;
            c.states.iter().map(|s| s.parts.iter().filter(rock).count()).max().unwrap_or(0)
        })
        .sum()
}

/// The index's tiles: 32 × 32 pixels.
const TILE_SHIFT: u32 = 5;

/// A pixel's solidity: the ground's, or the ground's and the live rock
/// parts'.
pub(crate) trait Solidity {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    /// OLX's pixel flags ([`Terrain::flags`]).
    fn flags(&self, x: i64, y: i64) -> u8;
    /// The material, `None` outside the map ([`Terrain::material`]).
    fn material(&self, x: i64, y: i64) -> Option<Material>;
}

impl Solidity for Terrain {
    #[inline(always)]
    fn width(&self) -> u32 {
        Terrain::width(self)
    }

    #[inline(always)]
    fn height(&self) -> u32 {
        Terrain::height(self)
    }

    #[inline(always)]
    fn flags(&self, x: i64, y: i64) -> u8 {
        Terrain::flags(self, x, y)
    }

    #[inline(always)]
    fn material(&self, x: i64, y: i64) -> Option<Material> {
        Terrain::material(self, x, y)
    }
}

/// One shown live rock part as the index has it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SolidPart {
    /// Its copy (an index into [`LiveLevel::copies`]).
    pub copy: u16,
    /// Its picture (an index into the copy's pictures).
    pub picture: u16,
    /// Its asset layer, if a layer property names it.
    pub layer: Option<u16>,
    /// Its place in its copy, world pixels ([`crate::LivePart::at`]).
    pub part_at: (i32, i32),
    /// Its picture's top-left, world pixels, after mirroring.
    pub at: (i32, i32),
    /// Its picture's size, world pixels.
    pub size: (u32, u32),
    /// Mirrored left to right (its copy's `Facing`).
    pub flip: bool,
}

/// The live rock parts' places and a coarse index over them (module docs).
#[derive(Clone, Debug, Default)]
pub(crate) struct SolidIndex {
    parts: Buf<SolidPart>,
    /// Around every part: `(x0, y0, x1, y1)`, the ends exclusive; empty when
    /// `x0 >= x1`.
    union: (i32, i32, i32, i32),
    /// A bit per part (by its index in `parts`) for each tile it covers,
    /// row-major, `cols × rows` tiles over the map; empty for a level
    /// without rock pictures.
    tiles: Buf<u64>,
    cols: u32,
    rows: u32,
}

/// Derived from the world: two indices of equal worlds are equal.
impl PartialEq for SolidIndex {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl SolidIndex {
    /// An empty index for a map of `size` pixels; `rock` false (a level
    /// without rock pictures) keeps no tiles.
    pub(crate) fn new(size: (u32, u32), rock: bool) -> SolidIndex {
        let (cols, rows) =
            if rock { (size.0.div_ceil(1 << TILE_SHIFT), size.1.div_ceil(1 << TILE_SHIFT)) } else { (0, 0) };
        let mut tiles = Buf::with_capacity(cols as usize * rows as usize);
        tiles.resize(cols as usize * rows as usize, 0);
        SolidIndex {
            parts: Buf::with_capacity(if rock { MAX_LIVE_SOLID_PARTS } else { 0 }),
            union: (0, 0, 0, 0),
            tiles,
            cols,
            rows,
        }
    }

    /// The parts, in copy and part order.
    pub(crate) fn parts(&self) -> &[SolidPart] {
        &self.parts
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// Forgets every part.
    pub(crate) fn clear(&mut self) {
        for i in 0..self.parts.len() {
            self.mark(i, false);
        }
        self.parts.clear();
        self.union = (0, 0, 0, 0);
    }

    /// Adds `p`; refused past [`MAX_LIVE_SOLID_PARTS`] or without tiles.
    pub(crate) fn push(&mut self, p: SolidPart) -> bool {
        if self.parts.len() >= MAX_LIVE_SOLID_PARTS || self.tiles.is_empty() {
            return false;
        }
        self.parts.push(p);
        self.mark(self.parts.len() - 1, true);
        self.grow(p);
        true
    }

    /// Moves part `i` to `at`.
    pub(crate) fn move_part(&mut self, i: usize, at: (i32, i32)) {
        if i >= self.parts.len() {
            return;
        }
        self.mark(i, false);
        self.parts[i].at = at;
        self.mark(i, true);
        self.union = (0, 0, 0, 0);
        for k in 0..self.parts.len() {
            self.grow(self.parts[k]);
        }
    }

    fn grow(&mut self, p: SolidPart) {
        let r = rect(p);
        let u = self.union;
        self.union = if u.0 >= u.2 { r } else { (u.0.min(r.0), u.1.min(r.1), u.2.max(r.2), u.3.max(r.3)) };
    }

    /// Sets or clears part `i`'s bit in the tiles it covers.
    fn mark(&mut self, i: usize, on: bool) {
        let Some(&p) = self.parts.get(i) else { return };
        let (x0, y0, x1, y1) = rect(p);
        if x1 <= 0 || y1 <= 0 || x0 >= x1 || y0 >= y1 {
            return;
        }
        // The tiles from the first covered to the last, within the map.
        let span = |a: i32, b: i32, n: u32| {
            let lo = (a.max(0) >> TILE_SHIFT) as u32;
            let hi = (((b - 1) >> TILE_SHIFT) as u32 + 1).min(n);
            lo..hi
        };
        let (xs, ys) = (span(x0, x1, self.cols), span(y0, y1, self.rows));
        let bit = 1u64 << i;
        for ty in ys {
            for tx in xs.clone() {
                let t = &mut self.tiles[ty as usize * self.cols as usize + tx as usize];
                if on {
                    *t |= bit;
                } else {
                    *t &= !bit;
                }
            }
        }
    }

    /// The parts whose picture is rock at world pixel `(x, y)`, a bit each.
    #[inline]
    pub(crate) fn covering(&self, level: &LiveLevel, x: i64, y: i64) -> u64 {
        let (x0, y0, x1, y1) = self.union;
        if x < i64::from(x0) || y < i64::from(y0) || x >= i64::from(x1) || y >= i64::from(y1) {
            return 0;
        }
        self.covering_near(level, x, y)
    }

    #[inline(never)]
    fn covering_near(&self, level: &LiveLevel, x: i64, y: i64) -> u64 {
        if x < 0 || y < 0 {
            return 0;
        }
        let (tx, ty) = (x >> TILE_SHIFT, y >> TILE_SHIFT);
        if tx >= i64::from(self.cols) || ty >= i64::from(self.rows) {
            return 0;
        }
        let mut bits = self.tiles[ty as usize * self.cols as usize + tx as usize];
        let mut out = 0;
        while bits != 0 {
            let i = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            if let Some(p) = self.parts.get(i)
                && covers(level, p, p.at, x, y)
            {
                out |= 1 << i;
            }
        }
        out
    }

    /// Whether a live rock part is rock at `(x, y)`.
    #[inline]
    pub(crate) fn rock(&self, level: &LiveLevel, x: i64, y: i64) -> bool {
        self.covering(level, x, y) != 0
    }
}

/// A part's picture's rectangle at its place, `(x0, y0, x1, y1)`.
fn rect(p: SolidPart) -> (i32, i32, i32, i32) {
    let w = i32::try_from(p.size.0).unwrap_or(i32::MAX);
    let h = i32::try_from(p.size.1).unwrap_or(i32::MAX);
    (p.at.0, p.at.1, p.at.0.saturating_add(w), p.at.1.saturating_add(h))
}

/// Whether part `p`, its picture's top-left at `at`, is rock at `(x, y)`.
#[inline]
pub(crate) fn covers(level: &LiveLevel, p: &SolidPart, at: (i32, i32), x: i64, y: i64) -> bool {
    let (lx, ly) = (x - i64::from(at.0), y - i64::from(at.1));
    if lx < 0 || ly < 0 || lx >= i64::from(p.size.0) || ly >= i64::from(p.size.1) {
        return false;
    }
    let lx = if p.flip { i64::from(p.size.0) - 1 - lx } else { lx };
    level
        .copies
        .get(usize::from(p.copy))
        .and_then(|c| c.pictures.get(usize::from(p.picture)))
        .is_some_and(|s| s.solid(lx as u32, ly as u32))
}

/// The live rock parts of a world, to read ([`Solid`]).
#[derive(Clone, Copy, Debug)]
pub struct SolidParts<'a> {
    pub(crate) index: &'a SolidIndex,
    pub(crate) level: &'a LiveLevel,
}

impl SolidParts<'_> {
    /// Whether a live rock part is rock at `(x, y)`.
    #[inline]
    pub fn rock(&self, x: i64, y: i64) -> bool {
        self.index.rock(self.level, x, y)
    }
}

/// The ground and the live rock parts over it: what the sites read when a
/// world has parts.
#[derive(Clone, Copy)]
pub(crate) struct OnParts<'a> {
    pub terrain: &'a Terrain,
    pub parts: SolidParts<'a>,
}

impl Solidity for OnParts<'_> {
    #[inline(always)]
    fn width(&self) -> u32 {
        self.terrain.width()
    }

    #[inline(always)]
    fn height(&self) -> u32 {
        self.terrain.height()
    }

    #[inline]
    fn flags(&self, x: i64, y: i64) -> u8 {
        if self.parts.rock(x, y) { PX_ROCK } else { self.terrain.flags(x, y) }
    }

    #[inline]
    fn material(&self, x: i64, y: i64) -> Option<Material> {
        if self.parts.rock(x, y) { Some(Material::Rock) } else { self.terrain.material(x, y) }
    }
}

/// The one terrain query ([`crate::World::solid`]): the ground, and the
/// world's live rock parts (`live`, none without them) over it.
#[derive(Clone, Copy)]
pub struct Solid<'a> {
    pub terrain: &'a Terrain,
    pub live: Option<SolidParts<'a>>,
}

impl<'a> Solid<'a> {
    #[inline]
    pub fn new(terrain: &'a Terrain, live: Option<SolidParts<'a>>) -> Solid<'a> {
        Solid { terrain, live }
    }

    #[inline]
    pub fn width(&self) -> u32 {
        self.terrain.width()
    }

    #[inline]
    pub fn height(&self) -> u32 {
        self.terrain.height()
    }

    /// [`Terrain::flags`], but `PX_ROCK` where a live rock part is.
    #[inline]
    pub fn flags(&self, x: i64, y: i64) -> u8 {
        match self.live {
            Some(parts) => OnParts { terrain: self.terrain, parts }.flags(x, y),
            None => self.terrain.flags(x, y),
        }
    }

    /// [`Terrain::material`], but rock where a live rock part is.
    #[inline]
    pub fn material(&self, x: i64, y: i64) -> Option<Material> {
        match self.live {
            Some(parts) => OnParts { terrain: self.terrain, parts }.material(x, y),
            None => self.terrain.material(x, y),
        }
    }

    /// Whether a live rock part (not the ground) is rock at `(x, y)`.
    #[inline]
    pub fn live_rock(&self, x: i64, y: i64) -> bool {
        self.live.is_some_and(|p| p.rock(x, y))
    }

    /// Whether no live rock part lies in the spawn square of `(x, y)`
    /// (`CMap::IsGoodSpawnPoint`'s 6 × 6, `respawn.rs`): a fixed spawn
    /// position falls back to `FindSpot` otherwise (issue #90).
    pub(crate) fn spawn_clear_of_parts(&self, x: i32, y: i32) -> bool {
        let Some(p) = self.live else { return true };
        let (x, y) = (i64::from(x), i64::from(y));
        (y - 3..y + 3).all(|yy| (x - 3..x + 3).all(|xx| !p.rock(xx, yy)))
    }
}

impl Solidity for Solid<'_> {
    fn width(&self) -> u32 {
        Solid::width(self)
    }

    fn height(&self) -> u32 {
        Solid::height(self)
    }

    fn flags(&self, x: i64, y: i64) -> u8 {
        Solid::flags(self, x, y)
    }

    fn material(&self, x: i64, y: i64) -> Option<Material> {
        Solid::material(self, x, y)
    }
}

/// Runs `$body` with `$s` the ground (`&Terrain`) when `$live` is `None`,
/// else the ground and the parts (`&OnParts`): the choice made once, each
/// arm its own monomorphised code.
macro_rules! with_solid {
    ($terrain:expr, $live:expr, |$s:ident| $body:expr) => {
        match $live {
            None => {
                let $s: &olr_terrain::Terrain = $terrain;
                $body
            }
            Some(parts) => {
                let $s = &$crate::solid::OnParts { terrain: $terrain, parts };
                $body
            }
        }
    };
}
pub(crate) use with_solid;

#[cfg(test)]
#[path = "solid_tests.rs"]
mod tests;
