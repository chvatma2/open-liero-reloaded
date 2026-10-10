//! Maximal free rectangles of rock, OLX's pathfinding "areas" (notes §1.4),
//! and the cache that shares them between searches and bots (spec §20 F7).
//!
//! LX56 never creates or removes rock (`CarveHole` removes dirt,
//! `PlaceDirt` fills only empty pixels), so a rectangle free of rock stays
//! free all match: areas are grown once, lazily under the budget, and kept.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use rl_terrain::{PX_ROCK, Terrain};

use crate::budget::Budget;

/// OLX's `wormsize` for areas and passages (`CWormBot.cpp:56`): an area
/// spans at least this much more than one pixel on each axis.
pub const WORM_SIZE: i32 = 7;

/// OLX's `SquareMatrix<int>`: `(x0, y0)` top-left, `(x1, y1)` bottom-right,
/// both inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    /// `isInDefinedArea`.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.x0 <= x && x <= self.x1 && self.y0 <= y && y <= self.y1
    }

    /// `getCenter`: `(v1 + v2) / 2` in ints.
    pub fn center(&self) -> (i32, i32) {
        ((self.x0 + self.x1) / 2, (self.y0 + self.y1) / 2)
    }

    /// `getInsersectionWithArea`.
    pub fn intersection(&self, o: &Rect) -> Rect {
        Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }

    /// Big enough to be an area (`addAreaNode`, `CWormBot.cpp:671`).
    pub fn is_area(&self) -> bool {
        self.x1 - self.x0 >= WORM_SIZE && self.y1 - self.y0 >= WORM_SIZE
    }
}

const RIGHT: u8 = 1;
const DOWN: u8 = 2;
const LEFT: u8 = 4;
const UP: u8 = 8;

/// `getMaxFreeArea(p, PX_ROCK)` (`src/common/CWormBot.cpp:115-200`), one
/// pixel at a time so that it can stop when the budget runs out and go on
/// next tick. From the rectangle `p..p`, it moves one edge out by a pixel
/// at a time, in the cycle down, left, up, right, skipping the edges that
/// met rock or the map's border; the line just outside the edge is read
/// whole (each pixel one unit). `p` itself is never read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grower {
    rect: Rect,
    blocked: u8,
    dir: u8,
    /// The pixel of the line being read, if one is.
    line: Option<(i32, i32)>,
    width: i32,
    height: i32,
    done: bool,
}

impl Grower {
    /// Starts growing at `p` on a `width` × `height` map. A point outside
    /// the map gives the rectangle `p..p` at once.
    pub fn new(p: (i32, i32), width: u32, height: u32) -> Self {
        let (width, height) = (width.min(i32::MAX as u32) as i32, height.min(i32::MAX as u32) as i32);
        let outside = p.0 < 0 || p.0 >= width || p.1 < 0 || p.1 >= height;
        Grower {
            rect: Rect { x0: p.0, y0: p.1, x1: p.0, y1: p.1 },
            blocked: 0,
            dir: RIGHT,
            line: None,
            width,
            height,
            done: outside,
        }
    }

    /// Grows until done (the rectangle, clipped to the map) or until `b`
    /// is spent (`None`: call again).
    pub fn step(&mut self, t: &Terrain, b: &mut Budget) -> Option<Rect> {
        while !self.done {
            let (mut x, mut y) = match self.line {
                Some(p) => p,
                None => {
                    if self.blocked == 0xF {
                        self.done = true;
                        break;
                    }
                    loop {
                        self.dir <<= 1;
                        if self.dir > UP {
                            self.dir = RIGHT;
                        }
                        if self.blocked & self.dir == 0 {
                            break;
                        }
                    }
                    let r = self.rect;
                    let (x, y) = match self.dir {
                        RIGHT => (r.x1 + 1, r.y0),
                        DOWN => (r.x0, r.y1 + 1),
                        LEFT => (r.x0 - 1, r.y0),
                        _ => (r.x0, r.y0 - 1),
                    };
                    if x < 0 || x >= self.width || y < 0 || y >= self.height {
                        self.blocked |= self.dir;
                        continue;
                    }
                    (x, y)
                }
            };
            let across = self.dir == RIGHT || self.dir == LEFT;
            loop {
                if (across && y > self.rect.y1) || (!across && x > self.rect.x1) {
                    break;
                }
                if b.left() == 0 {
                    self.line = Some((x, y));
                    return None;
                }
                b.spend(1);
                if t.flags(i64::from(x), i64::from(y)) & PX_ROCK != 0 {
                    self.blocked |= self.dir;
                    break;
                }
                if across {
                    y += 1;
                } else {
                    x += 1;
                }
            }
            self.line = None;
            if self.blocked & self.dir == 0 {
                match self.dir {
                    RIGHT => self.rect.x1 += 1,
                    DOWN => self.rect.y1 += 1,
                    LEFT => self.rect.x0 -= 1,
                    _ => self.rect.y0 -= 1,
                }
            }
        }
        let mut r = self.rect;
        r.x0 = r.x0.max(0);
        r.y0 = r.y0.max(0);
        r.x1 = r.x1.min(self.width - 1);
        r.y1 = r.y1.min(self.height - 1);
        Some(r)
    }
}

/// Areas found so far on one level, shared by every search of every bot of
/// a match through an `Arc` (spec §20 F7). It holds at most a fixed number
/// of areas, allocated when it is made, and only ever grows.
///
/// It is lock-free (atomics), so it is `Send + Sync`, but the order in which
/// bots add areas decides what later searches see: for reproducible
/// matches, the bots sharing a cache must think in a fixed order on one
/// thread, and every match needs its own cache.
#[derive(Debug)]
pub struct AreaCache {
    width: u32,
    height: u32,
    rects: Vec<AtomicU64>,
    len: AtomicUsize,
}

impl AreaCache {
    /// The default number of areas a cache holds.
    pub const DEFAULT_CAPACITY: usize = 4096;

    /// An empty cache for a level of `terrain`'s size.
    pub fn new(terrain: &Terrain) -> Arc<Self> {
        Self::with_capacity(terrain, Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(terrain: &Terrain, capacity: usize) -> Arc<Self> {
        let rects = (0..capacity).map(|_| AtomicU64::new(0)).collect();
        Arc::new(AreaCache {
            width: terrain.width(),
            height: terrain.height(),
            rects,
            len: AtomicUsize::new(0),
        })
    }

    /// Whether this cache is for a level of `terrain`'s size.
    pub fn fits(&self, terrain: &Terrain) -> bool {
        (self.width, self.height) == (terrain.width(), terrain.height())
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn capacity(&self) -> usize {
        self.rects.len()
    }

    /// The number of areas held.
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire).min(self.rects.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Area `i`.
    pub fn get(&self, i: usize) -> Option<Rect> {
        unpack(self.rects.get(i)?.load(Ordering::Acquire))
    }

    /// The area containing `(x, y)`: like OLX's `getArea` (which scans its
    /// areas in `v1` order, `y` first), the one with the smallest `(y0, x0)`,
    /// then the oldest.
    pub fn find(&self, x: i32, y: i32) -> Option<usize> {
        let mut best: Option<(i32, i32, usize)> = None;
        for i in 0..self.len() {
            let Some(r) = self.get(i) else { continue };
            if r.contains(x, y) && best.is_none_or(|(by, bx, _)| (r.y0, r.x0) < (by, bx)) {
                best = Some((r.y0, r.x0, i));
            }
        }
        best.map(|(_, _, i)| i)
    }

    /// Adds `r` and gives its index, or `None` when the cache is full or
    /// `r` is not an area inside a map of at most 65 535 px a side.
    pub fn insert(&self, r: Rect) -> Option<usize> {
        let packed = pack(r)?;
        let i = self
            .len
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| (n < self.rects.len()).then_some(n + 1))
            .ok()?;
        self.rects[i].store(packed, Ordering::Release);
        Some(i)
    }
}

/// Four `u16`s; an area's `x1` is at least 7, so no area packs to 0 (an
/// unwritten slot).
fn pack(r: Rect) -> Option<u64> {
    if !r.is_area() || r.x0 < 0 || r.y0 < 0 {
        return None;
    }
    let f = |v: i32| u16::try_from(v).ok().map(u64::from);
    Some(f(r.x0)? | f(r.y0)? << 16 | f(r.x1)? << 32 | f(r.y1)? << 48)
}

fn unpack(v: u64) -> Option<Rect> {
    (v != 0).then(|| {
        let f = |s: u32| ((v >> s) & 0xFFFF) as i32;
        Rect { x0: f(0), y0: f(16), x1: f(32), y1: f(48) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_round_trips_and_refuses_what_is_not_an_area() {
        let r = Rect { x0: 3, y0: 65000, x1: 65535, y1: 65007 };
        assert_eq!(unpack(pack(r).unwrap()), Some(r));
        assert_eq!(pack(Rect { x0: 0, y0: 0, x1: 6, y1: 9 }), None);
        assert_eq!(pack(Rect { x0: 0, y0: 0, x1: 70000, y1: 9 }), None);
        assert_eq!(pack(Rect { x0: -1, y0: 0, x1: 9, y1: 9 }), None);
    }
}
