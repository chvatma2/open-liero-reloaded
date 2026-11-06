//! Loose dirt (spec `docs/superpowers/specs/2026-11-05-loose-dirt-design.md`):
//! dirt of a loose material falls under gravity, rides on falling grains
//! and slides down slopes steeper than its angle of repose, once a tick at
//! the end of [`World::frame`]. No OLX counterpart.
//!
//! The terrain holds each cell's kind ([`Terrain::loose`]); the world holds
//! [`LooseState`], small: the moving grains (settled ones are not in it),
//! the chunks awake for the next tick and the live rock parts' boxes as the
//! last tick saw them. Rows run bottom-up, so a grain riding on a falling
//! one sees the pixels its carrier vacated this tick.

use alloc::vec;
use alloc::vec::Vec;

use olr_data::level::{LooseMaterial, Material};
use olr_terrain::Terrain;
use serde::{Deserialize, Serialize};

use crate::checksum::Fnv;
use crate::solid::SolidParts;
use crate::worm::Worm;

/// The most loose materials a level holds.
pub const MAX_LOOSE_MATERIALS: usize = 16;
/// The most grains a tick moves; the rest wait for the next tick.
pub const MAX_LOOSE_MOVES: u32 = 20_000;
/// Chunks of 16 × 16 pixels sleep and wake together.
const CHUNK_SHIFT: u32 = 4;
/// Sub-pixel units in a pixel.
const SUB: u32 = 256;
/// Fixed point of the slope thresholds: 1/65536.
const ONE: u32 = 1 << 16;

/// A loose material's rates in a tick's units.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rates {
    /// Sub-pixels a tick a tick.
    gravity: u32,
    /// Sub-pixels a tick.
    top: u32,
    /// The chance in 65536 that a grain able to slide slides this tick.
    slide: u32,
    /// Steeper than 45°: the side column must be open `k` pixels down;
    /// flatter: a drop within `n` pixels along the row.
    steep: bool,
    /// `k` or `n` in [`ONE`]s: its whole part and the dithered fraction.
    reach: u32,
}

impl Rates {
    fn of(m: &LooseMaterial) -> Rates {
        let per_tick = |px_s: f64| libm::round(px_s / 100.0 * f64::from(SUB)) as u32;
        let tan = libm::tan(f64::from(m.repose).to_radians());
        let t = libm::round(tan * f64::from(ONE)) as u32;
        let steep = t >= ONE;
        let reach = if steep {
            t
        } else {
            let t = t.max(ONE / 20);
            ((u64::from(ONE) * u64::from(ONE) + u64::from(t) / 2) / u64::from(t)) as u32
        };
        Rates {
            gravity: per_tick(f64::from(m.gravity) / 100.0).max(1),
            top: per_tick(f64::from(m.top_speed)).max(1),
            slide: (u32::from(m.slide_speed.min(100)) * ONE) / 100,
            steep,
            reach,
        }
    }
}

/// A level's loose materials, shared by every world of a match.
#[derive(Clone, Debug, PartialEq)]
pub struct LooseLevel {
    materials: Vec<LooseMaterial>,
    rates: Vec<Rates>,
}

impl LooseLevel {
    /// `None` for more than [`MAX_LOOSE_MATERIALS`] or a value out of its
    /// range (repose 5-85, gravity 1-2000, top speed 1-500, slide 0-100).
    pub fn new(materials: Vec<LooseMaterial>) -> Option<LooseLevel> {
        let fits = |m: &LooseMaterial| {
            (5..=85).contains(&m.repose)
                && (1..=2000).contains(&m.gravity)
                && (1..=500).contains(&m.top_speed)
                && m.slide_speed <= 100
        };
        if materials.len() > MAX_LOOSE_MATERIALS || !materials.iter().all(fits) {
            return None;
        }
        let rates = materials.iter().map(Rates::of).collect();
        Some(LooseLevel { materials, rates })
    }

    pub fn materials(&self) -> &[LooseMaterial] {
        &self.materials
    }
}

/// A moving grain: its cell, fall speed and sub-pixel remainder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Grain {
    cell: u32,
    vel: u16,
    frac: u8,
}

/// A live rock part as the step last saw it: copy, picture, place, size
/// and mirroring.
type PartBox = (u16, u16, (i32, i32), (u32, u32), bool);

/// The loose dirt's state: what a snapshot, the checksum and a state
/// transfer carry.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LooseState {
    /// Sorted by cell.
    moving: Vec<Grain>,
    /// A bit per chunk, row-major: awake for the next tick.
    next: Vec<u64>,
    parts: Vec<PartBox>,
}

impl LooseState {
    pub(crate) fn hash(&self, h: &mut Fnv) {
        h.usize(self.moving.len());
        for g in &self.moving {
            h.u32(g.cell);
            h.u32(u32::from(g.vel) << 8 | u32::from(g.frac));
        }
        for w in &self.next {
            h.u64(*w);
        }
        h.usize(self.parts.len());
        for &(copy, picture, at, size, flip) in &self.parts {
            h.u32(u32::from(copy) << 16 | u32::from(picture));
            h.u32(at.0 as u32);
            h.u32(at.1 as u32);
            h.u32(size.0);
            h.u32(size.1);
            h.bool(flip);
        }
    }

    pub(crate) fn moving_len(&self) -> usize {
        self.moving.len()
    }
}

/// One cell's step scratch: its fall speed and remainder, and whether a
/// grain landed in it this tick (it is not visited again).
#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    /// The cell + 1; 0 empty.
    key: u32,
    vel: u16,
    frac: u8,
    landed: bool,
}

/// Buffers only, emptied after each step: never state, never compared.
#[derive(Debug, Default)]
struct Scratch {
    /// Open addressing, linear probing, a power of two long.
    slots: Vec<Slot>,
    used: usize,
    /// Cells whose slot may hold a moving grain at the step's end.
    cand: Vec<u32>,
    /// The chunks awake this tick.
    now: Vec<u64>,
    parts: Vec<PartBox>,
}

impl Clone for Scratch {
    fn clone(&self) -> Self {
        Scratch::default()
    }
}

impl Scratch {
    fn find(&self, cell: u32) -> Option<usize> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut i = (mix(cell) as usize) & mask;
        loop {
            match self.slots[i].key {
                0 => return None,
                k if k == cell + 1 => return Some(i),
                _ => i = (i + 1) & mask,
            }
        }
    }

    fn get(&self, cell: u32) -> Slot {
        self.find(cell).map_or_else(Slot::default, |i| self.slots[i])
    }

    fn set(&mut self, cell: u32, vel: u16, frac: u8, landed: bool) {
        if let Some(i) = self.find(cell) {
            let s = &mut self.slots[i];
            (s.vel, s.frac, s.landed) = (vel, frac, landed);
            return;
        }
        if vel == 0 && frac == 0 && !landed {
            return;
        }
        if (self.used + 1) * 2 > self.slots.len() {
            self.grow();
        }
        let mask = self.slots.len() - 1;
        let mut i = (mix(cell) as usize) & mask;
        while self.slots[i].key != 0 {
            i = (i + 1) & mask;
        }
        self.slots[i] = Slot { key: cell + 1, vel, frac, landed };
        self.used += 1;
        self.cand.push(cell);
    }

    fn grow(&mut self) {
        let old = core::mem::take(&mut self.slots);
        self.slots = vec![Slot::default(); (old.len() * 2).max(64)];
        self.used = 0;
        let mask = self.slots.len() - 1;
        for s in old.into_iter().filter(|s| s.key != 0) {
            let mut i = (mix(s.key - 1) as usize) & mask;
            while self.slots[i].key != 0 {
                i = (i + 1) & mask;
            }
            self.slots[i] = s;
            self.used += 1;
        }
    }

    fn clear(&mut self) {
        if self.used > 0 {
            self.slots.iter_mut().for_each(|s| *s = Slot::default());
        }
        self.used = 0;
        self.cand.clear();
    }
}

/// The world's loose dirt: the level's materials, the state, this tick's
/// moves and the step's buffers.
#[derive(Clone, Debug)]
pub(crate) struct LooseWorld {
    level: alloc::sync::Arc<LooseLevel>,
    pub(crate) state: LooseState,
    /// This tick's moves, `(from, to)` cell indices in order: output.
    moves: Vec<(u32, u32)>,
    scratch: Scratch,
    cols: u32,
    rows: u32,
    /// A bit per chunk that may hold a loose pixel: every chunk holding one
    /// is set (grains enter a chunk only by a move, which sets it; a
    /// restore brings the snapshot's). Not state: the step scans no other.
    holds: Vec<u64>,
    /// The chunk rows scanned grain by grain in the last step (tests).
    pub(crate) scanned: u32,
}

/// Equal worlds have equal states; the moves and buffers are output and
/// scratch.
impl PartialEq for LooseWorld {
    fn eq(&self, other: &Self) -> bool {
        self.level == other.level && self.state == other.state
    }
}

/// The splitmix32-like finaliser of the step's hashes.
fn mix(mut h: u32) -> u32 {
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// The step's fixed hash of a pixel, a tick and a salt.
fn hash(x: u32, y: u32, t: u32, salt: u32) -> u32 {
    mix(x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77) ^ t.wrapping_mul(0xC2B2_AE3D) ^ salt)
}

/// What the step reads besides the terrain: the worms and the live parts.
struct Obstacles<'a> {
    worms: &'a [Worm],
    parts: Option<SolidParts<'a>>,
}

impl Obstacles<'_> {
    /// Whether `(x, y)` is open for a grain: background on the ground, no
    /// live rock part there, no living worm's box (`|dx| ≤ 3`, `-4 ≤ dy ≤
    /// 5`, the cross `moveAndCheckWormCollision` reads, as a box).
    #[inline]
    fn open(&self, t: &Terrain, x: i64, y: i64) -> bool {
        t.material(x, y) == Some(Material::Background)
            && !self.parts.is_some_and(|p| p.rock(x, y))
            && !self.worms.iter().any(|w| {
                let (dx, dy) = (x - w.pos.x as i64, y - w.pos.y as i64);
                w.alive && dx.abs() <= 3 && (-4..=5).contains(&dy)
            })
    }
}

impl LooseWorld {
    /// The world's loose dirt on `terrain` (which has a loose layer): every
    /// chunk holding a loose pixel awake.
    pub(crate) fn new(level: alloc::sync::Arc<LooseLevel>, terrain: &Terrain) -> LooseWorld {
        let (w, h) = (terrain.width(), terrain.height());
        let (cols, rows) = (w.div_ceil(1 << CHUNK_SHIFT), h.div_ceil(1 << CHUNK_SHIFT));
        let mut l = LooseWorld {
            level,
            state: LooseState { moving: Vec::new(), next: vec![0; bits(cols * rows)], parts: Vec::new() },
            moves: Vec::new(),
            scratch: Scratch::default(),
            cols,
            rows,
            holds: Vec::new(),
            scanned: 0,
        };
        l.find_holds(terrain);
        l.state.next.clone_from(&l.holds);
        l
    }

    /// `state` on `terrain`, whose loose layer was just loaded with it (a
    /// state transfer).
    pub(crate) fn load(&mut self, state: LooseState, terrain: &Terrain) {
        self.state = state;
        self.find_holds(terrain);
    }

    /// Sets `holds` from the terrain's loose layer.
    fn find_holds(&mut self, terrain: &Terrain) {
        let w = terrain.width();
        self.holds = vec![0; bits(self.cols * self.rows)];
        for (i, _) in terrain.loose_cells().unwrap_or(&[]).iter().enumerate().filter(|(_, k)| **k != 0) {
            let (x, y) = (i as u32 % w, i as u32 / w);
            set_bit(&mut self.holds, (y >> CHUNK_SHIFT) * self.cols + (x >> CHUNK_SHIFT));
        }
    }

    /// Whether every kind of `terrain` names one of the level's materials.
    pub(crate) fn kinds_fit(level: &LooseLevel, terrain: &Terrain) -> bool {
        let n = level.materials.len();
        terrain.loose_cells().is_none_or(|k| k.iter().all(|&k| usize::from(k) <= n))
    }

    /// Whether a received state's `kinds` and `state` fit this world with
    /// its `cells` (a state transfer): a kind a cell, each on dirt and one
    /// of the materials; the moving grains sorted, unique, loose; the
    /// chunks' bits this map's; at most as many part boxes as parts.
    pub(crate) fn wire_fits(&self, cells: &[u8], kinds: &[u8], state: &LooseState) -> bool {
        let n = self.level.materials.len();
        let kinds_ok = kinds.len() == cells.len()
            && kinds
                .iter()
                .zip(cells)
                .all(|(&k, &c)| k == 0 || (c == Material::Dirt as u8 && usize::from(k) <= n));
        kinds_ok
            && state.moving.windows(2).all(|p| p[0].cell < p[1].cell)
            && state.moving.iter().all(|g| kinds.get(g.cell as usize).is_some_and(|&k| k != 0))
            && state.next.len() == self.state.next.len()
            && state.parts.len() <= crate::solid::MAX_LIVE_SOLID_PARTS
    }

    pub(crate) fn moves(&self) -> &[(u32, u32)] {
        &self.moves
    }

    /// Wakes the chunks over world box `x0..=x1` × `y0..=y1` for the next
    /// tick (clipped to the map).
    fn wake(&mut self, x0: i64, y0: i64, x1: i64, y1: i64) {
        wake_in(&mut self.state.next, (self.cols, self.rows), (x0, y0, x1, y1));
    }

    /// The step (module docs), unless `frozen`: the tick's edits, worms and
    /// parts wake their chunks either way.
    pub(crate) fn step(
        &mut self,
        terrain: &mut Terrain,
        worms: &[Worm],
        parts: Option<SolidParts<'_>>,
        tick: u32,
        frozen: bool,
    ) {
        self.moves.clear();
        for a in terrain.take_touched() {
            let r = (i64::from(a.x0) - 1, i64::from(a.y0) - 1, i64::from(a.x1), i64::from(a.y1));
            wake_in(&mut self.state.next, (self.cols, self.rows), r);
            // A grain a live part moved there may have come into a chunk
            // that held none (the production line design): `holds` only
            // ever says "may hold", so a touched chunk is one.
            wake_in(&mut self.holds, (self.cols, self.rows), r);
        }
        for w in worms.iter().filter(|w| w.alive) {
            let (x, y) = (w.pos.x as i64, w.pos.y as i64);
            self.wake(x - 7, y - 8, x + 7, y + 9);
        }
        self.wake_parts(parts);
        if frozen {
            return;
        }
        let LooseWorld { level, state, moves, scratch, cols, rows, holds, scanned } = self;
        *scanned = 0;
        let (cols, rows) = (*cols, *rows);
        scratch.now.clear();
        scratch.now.extend_from_slice(&state.next);
        state.next.iter_mut().for_each(|b| *b = 0);
        scratch.clear();
        for g in &state.moving {
            scratch.set(g.cell, g.vel, g.frac, false);
        }
        let obstacles = Obstacles { worms, parts };
        let (w, h) = (terrain.width(), terrain.height());
        let mut capped = false;
        for y in (0..h).rev() {
            let cy = y >> CHUNK_SHIFT;
            // A row of chunks asleep, or awake without loose dirt, costs
            // one look at their bits (once capped, every awake one counts).
            let worth = if capped { &scratch.now } else { &*holds };
            if !any_in(&scratch.now, worth, cy * cols, (cy + 1) * cols) {
                continue;
            }
            let flip = (tick.wrapping_add(y)) & 1 == 1;
            for k in 0..cols {
                let cx = if flip { cols - 1 - k } else { k };
                let chunk = cy * cols + cx;
                if !bit(&scratch.now, chunk) {
                    continue;
                }
                if capped {
                    set_bit(&mut state.next, chunk);
                    continue;
                }
                // Scanning a chunk without loose dirt would do nothing.
                if !bit(holds, chunk) {
                    continue;
                }
                *scanned += 1;
                for j in 0..1u32 << CHUNK_SHIFT {
                    let x = (cx << CHUNK_SHIFT) + if flip { (1 << CHUNK_SHIFT) - 1 - j } else { j };
                    if x >= w {
                        continue;
                    }
                    let kind = terrain.loose(i64::from(x), i64::from(y));
                    if kind == 0 {
                        continue;
                    }
                    let cell = y * w + x;
                    let slot = scratch.get(cell);
                    if slot.landed {
                        continue;
                    }
                    let r = level.rates[usize::from(kind - 1)];
                    let to = grain_step(
                        terrain,
                        &obstacles,
                        scratch,
                        (x, y),
                        slot,
                        r,
                        tick,
                        &mut state.next,
                        (cols, rows),
                    );
                    let Some((tx, ty)) = to else { continue };
                    let dest = ty * w + tx;
                    let moved = scratch.get(cell);
                    terrain.move_grain(cell, dest);
                    set_bit(holds, (ty >> CHUNK_SHIFT) * cols + (tx >> CHUNK_SHIFT));
                    scratch.set(cell, 0, 0, false);
                    scratch.set(dest, moved.vel, moved.frac, true);
                    moves.push((cell, dest));
                    let (xi, yi, txi, tyi) = (i64::from(x), i64::from(y), i64::from(tx), i64::from(ty));
                    wake_in(&mut state.next, (cols, rows), (xi - 1, yi - 1, xi + 1, yi + 1));
                    wake_in(&mut state.next, (cols, rows), (txi - 1, tyi - 1, txi + 1, tyi + 1));
                    if moves.len() as u32 >= MAX_LOOSE_MOVES {
                        capped = true;
                        set_bit(&mut state.next, chunk);
                        break;
                    }
                }
            }
        }
        // The moving grains: every candidate cell still loose and moving.
        scratch.cand.sort_unstable();
        scratch.cand.dedup();
        state.moving.clear();
        for &c in &scratch.cand {
            let s = scratch.get(c);
            if (s.vel > 0 || s.frac > 0) && terrain.loose(i64::from(c % w), i64::from(c / w)) != 0 {
                state.moving.push(Grain { cell: c, vel: s.vel, frac: s.frac });
            }
        }
        scratch.clear();
    }

    /// Wakes the chunks a live rock part left or entered since the last
    /// tick (its old and new box, grown by a pixel).
    fn wake_parts(&mut self, parts: Option<SolidParts<'_>>) {
        let mut now = core::mem::take(&mut self.scratch.parts);
        now.clear();
        if let Some(p) = parts {
            now.extend(p.index.parts().iter().map(|s| (s.copy, s.picture, s.at, s.size, s.flip)));
        }
        if now != self.state.parts {
            let (old, new) = (core::mem::take(&mut self.state.parts), &now);
            for i in 0..old.len().max(new.len()) {
                let (a, b) = (old.get(i), new.get(i));
                if a == b {
                    continue;
                }
                for &(_, _, at, size, _) in a.into_iter().chain(b) {
                    let (x0, y0) = (i64::from(at.0), i64::from(at.1));
                    self.wake(x0 - 1, y0 - 1, x0 + i64::from(size.0), y0 + i64::from(size.1));
                }
            }
            // The buffers swap places, so neither is ever dropped.
            self.state.parts = now;
            self.scratch.parts = old;
        } else {
            self.scratch.parts = now;
        }
    }
}

/// One grain's step (module docs): where it moves, if anywhere. Writes its
/// new speed and remainder to `scratch` and keeps its chunk awake while it
/// moves or could slide.
#[allow(clippy::too_many_arguments)]
fn grain_step(
    t: &Terrain,
    o: &Obstacles<'_>,
    scratch: &mut Scratch,
    (x, y): (u32, u32),
    slot: Slot,
    r: Rates,
    tick: u32,
    next: &mut [u64],
    grid: (u32, u32),
) -> Option<(u32, u32)> {
    let (xi, yi) = (i64::from(x), i64::from(y));
    let cell = y * t.width() + x;
    let falling = |s: &Scratch, x: i64, y: i64| {
        t.loose(x, y) != 0 && {
            let g = s.get(y as u32 * t.width() + x as u32);
            g.vel > 0 || g.frac > 0
        }
    };
    if o.open(t, xi, yi + 1) || falling(scratch, xi, yi + 1) {
        // Falling, or riding on a falling grain: speed up, move the whole
        // pixels fallen that are open.
        let v = (u32::from(slot.vel) + r.gravity).min(r.top);
        let f = u32::from(slot.frac) + v;
        let want = i64::from(f / SUB);
        let mut d = 0;
        while d < want && o.open(t, xi, yi + d + 1) {
            d += 1;
        }
        let blocked = d < want && !falling(scratch, xi, yi + d + 1);
        if blocked {
            scratch.set(cell, 0, 0, false);
        } else {
            scratch.set(cell, v as u16, (f % SUB) as u8, false);
        }
        wake_in(next, grid, (xi, yi, xi, yi));
        return (d > 0).then_some((x, y + d as u32));
    }
    // Resting: slide if the slope is steeper than the repose.
    scratch.set(cell, 0, 0, false);
    let sides: [i64; 2] = if hash(x, y, tick, 1) & 1 == 0 { [-1, 1] } else { [1, -1] };
    let target = sides.into_iter().find_map(|s| slide_to(t, o, r, xi, yi, s))?;
    wake_in(next, grid, (xi, yi, xi, yi));
    if hash(x, y, tick, 2) & 0xffff >= r.slide {
        return None;
    }
    Some((target.0 as u32, target.1 as u32))
}

/// Where a resting grain at `(x, y)` slides on side `s`, if the slope there
/// is steeper than its repose: steeper than 45°, the side column open `k`
/// pixels down; flatter, a drop within `n` pixels along the row, the grain
/// stepping sideways towards it. `k` and `n` are dithered by position.
fn slide_to(t: &Terrain, o: &Obstacles<'_>, r: Rates, x: i64, y: i64, s: i64) -> Option<(i64, i64)> {
    let dither = hash(x as u32, y as u32, 0, 7) & 0xffff;
    let reach = i64::from(r.reach >> 16) + i64::from(dither < (r.reach & 0xffff));
    if r.steep {
        let clear = o.open(t, x + s, y) && (1..=reach).all(|d| o.open(t, x + s, y + d));
        return clear.then_some((x + s, y + 1));
    }
    for j in 1..=reach {
        if !o.open(t, x + s * j, y) {
            return None;
        }
        if o.open(t, x + s * j, y + 1) {
            return Some(if j == 1 { (x + s, y + 1) } else { (x + s, y) });
        }
    }
    None
}

#[cfg(test)]
#[path = "loose_tests.rs"]
mod tests;

fn bits(n: u32) -> usize {
    (n as usize).div_ceil(64)
}

fn bit(b: &[u64], i: u32) -> bool {
    b[(i >> 6) as usize] >> (i & 63) & 1 == 1
}

/// Whether any of bits `from..to` is set in both `a` and `b`.
fn any_in(a: &[u64], b: &[u64], from: u32, to: u32) -> bool {
    let (mut i, end) = (from, to);
    while i < end {
        let k = (i >> 6) as usize;
        let w = (a[k] & b[k]) >> (i & 63);
        let n = (64 - (i & 63)).min(end - i);
        if w & (u64::MAX >> (64 - n)) != 0 {
            return true;
        }
        i += n;
    }
    false
}

fn set_bit(b: &mut [u64], i: u32) {
    b[(i >> 6) as usize] |= 1 << (i & 63);
}

/// Sets the chunks over world box `x0..=x1` × `y0..=y1` in `b`, clipped.
fn wake_in(b: &mut [u64], (cols, rows): (u32, u32), (x0, y0, x1, y1): (i64, i64, i64, i64)) {
    if cols == 0 || rows == 0 || x1 < 0 || y1 < 0 || x0 > x1 || y0 > y1 {
        return;
    }
    let c = |v: i64, n: u32| ((v.max(0) >> CHUNK_SHIFT) as u32).min(n - 1);
    for cy in c(y0, rows)..=c(y1, rows) {
        for cx in c(x0, cols)..=c(x1, cols) {
            set_bit(b, cy * cols + cx);
        }
    }
}
