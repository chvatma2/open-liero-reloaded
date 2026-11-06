//! The live step (issue #90): once a tick, before the physics, the live
//! copies' motions step their rock parts a whole pixel at a time, pushing,
//! carrying and wedging worms and taking the ropes hooked to them; then a
//! part that appeared over a worm pushes it out or wedges it; then the
//! wedged worms are crushed. A part moving into loose dirt pushes and
//! carries its grains where they have room (the factory's production line
//! design, 2026-11-06), each a [`EventKind::GrainMoved`]; the dirt left in
//! its new footprint, plain or loose, is crushed: background, a
//! [`crate::TerrainOp::Crush`] edit a row run, counted in an
//! [`EventKind::DirtCrushed`] per step. No OLX counterpart.
//!
//! A worm's body here is the pixels `move_and_check_collision` reads, but
//! its lowest (`y + 5`), which rests inside the ground while it stands:
//! its row `x - 3..=x + 3` and its column `y - 4..=y + 4`. A place is free
//! for it when its body there is inside the level, on neither ground rock
//! nor dirt, and on no live rock part.

use olr_data::level::Material;
use olr_math::Vec2;
use olr_terrain::{PX_EMPTY, Terrain};

use crate::damage::injure_worm;
use crate::events::{EventKind, TickEvents};
use crate::live::{CRUSH_CHUNK, CRUSH_EVERY, LiveLevel, PUSH_OUT_PX, PartMover, PartStep};
use crate::solid::{MAX_LIVE_SOLID_PARTS, SolidIndex, SolidPart, covers};
use crate::world::{MAX_WORMS, World};
use crate::worm::Worm;

/// The worms wedged this tick, and by which copy (the first).
#[derive(Clone, Copy)]
struct Wedged {
    mask: u32,
    copy: [u16; MAX_WORMS],
}

impl Wedged {
    fn set(&mut self, worm: usize, copy: u16) {
        if worm < MAX_WORMS && self.mask & (1 << worm) == 0 {
            self.mask |= 1 << worm;
            self.copy[worm] = copy;
        }
    }
}

/// A worm's whole-pixel place, as `move_and_check_collision` takes it.
fn pixel(pos: Vec2) -> (i64, i64) {
    (i64::from(pos.x as i32), i64::from(pos.y as i32))
}

/// The worm's body at `(x, y)` (module docs).
fn body(x: i64, y: i64) -> impl Iterator<Item = (i64, i64)> {
    (x - 3..=x + 3).map(move |bx| (bx, y)).chain((y - 4..=y + 4).map(move |by| (x, by)))
}

/// The parts (a bit each) a body at `pos` lies on.
fn overlap(solids: &SolidIndex, level: &LiveLevel, pos: Vec2) -> u64 {
    let (x, y) = pixel(pos);
    body(x, y).fold(0, |m, (bx, by)| m | solids.covering(level, bx, by))
}

/// Whether a worm's body fits at `pos` (module docs).
fn free(t: &Terrain, solids: &SolidIndex, level: &LiveLevel, pos: Vec2) -> bool {
    let (x, y) = pixel(pos);
    let (w, h) = (i64::from(t.width()), i64::from(t.height()));
    body(x, y).all(|(bx, by)| {
        bx >= 0
            && by >= 0
            && bx < w
            && by < h
            && t.flags(bx, by) & PX_EMPTY != 0
            && !solids.rock(level, bx, by)
    })
}

fn moved(pos: Vec2, d: (i32, i32)) -> Vec2 {
    Vec2::new(pos.x + d.0 as f32, pos.y + d.1 as f32)
}

/// Whether part `p` moving from `old` to `new` would enter ground rock,
/// another shown rock part (outside `group`, the parts moving with it) or
/// the level's edge: the pixels of its new footprint its old one lacks.
fn blocked(
    t: &Terrain,
    solids: &SolidIndex,
    level: &LiveLevel,
    p: &SolidPart,
    old: (i32, i32),
    new: (i32, i32),
    group: u64,
) -> bool {
    let (w, h) = (i64::from(t.width()), i64::from(t.height()));
    for ly in 0..i64::from(p.size.1) {
        for lx in 0..i64::from(p.size.0) {
            let (x, y) = (i64::from(new.0) + lx, i64::from(new.1) + ly);
            if !covers(level, p, new, x, y) || covers(level, p, old, x, y) {
                continue;
            }
            if x < 0 || y < 0 || x >= w || y >= h {
                return true;
            }
            if t.material(x, y) == Some(Material::Rock) || solids.covering(level, x, y) & !group != 0 {
                return true;
            }
        }
    }
    false
}

/// The most loose grains a push shifts at once, or a carry lifts in a
/// column: past it the grain in the part's way is crushed.
pub const MAX_PUSH_CHAIN: usize = 64;

/// Whether a loose grain may move to `(x, y)`: inside the level,
/// background, and under no shown rock part.
fn grain_free(t: &Terrain, solids: &SolidIndex, level: &LiveLevel, x: i64, y: i64) -> bool {
    t.material(x, y) == Some(Material::Background) && solids.covering(level, x, y) == 0
}

/// Moves the loose grain at `from` to `to`, reported.
fn shift(t: &mut Terrain, events: &mut TickEvents, from: (i64, i64), to: (i64, i64)) {
    let w = i64::from(t.width());
    t.shove_grain((from.1 * w + from.0) as u32, (to.1 * w + to.0) as u32);
    events.grain_moved((from.0 as i32, from.1 as i32), (to.0 as i32, to.1 as i32));
}

/// Pushes the run of loose grains from `(x, y)` one pixel by `d` when the
/// cell past it (at most [`MAX_PUSH_CHAIN`] grains) is free, farthest
/// first.
fn push_run(
    t: &mut Terrain,
    solids: &SolidIndex,
    level: &LiveLevel,
    events: &mut TickEvents,
    x: i64,
    y: i64,
    d: (i64, i64),
) {
    let at = |j: i64| (x + j * d.0, y + j * d.1);
    let mut n = 1;
    while n < MAX_PUSH_CHAIN as i64 && t.loose(at(n).0, at(n).1) != 0 {
        n += 1;
    }
    if !grain_free(t, solids, level, at(n).0, at(n).1) {
        return;
    }
    for j in (0..n).rev() {
        shift(t, events, at(j), at(j + 1));
    }
}

/// Pushes the loose grains in part `p`'s fresh cells (those of its new
/// place its `old` one lacks; it moved by `d`) out of its way where they
/// have room: row by row (the last row first moving down), each row
/// farthest along `d` first.
fn push_grains(
    t: &mut Terrain,
    solids: &SolidIndex,
    level: &LiveLevel,
    p: &SolidPart,
    old: (i32, i32),
    d: (i64, i64),
    events: &mut TickEvents,
) {
    let (w, h) = (i64::from(p.size.0), i64::from(p.size.1));
    let new = p.at;
    for r in 0..h {
        let ly = if d.1 > 0 { h - 1 - r } else { r };
        let y = i64::from(new.1) + ly;
        for k in 0..w {
            let lx = if d.0 > 0 { w - 1 - k } else { k };
            let x = i64::from(new.0) + lx;
            if t.loose(x, y) != 0 && covers(level, p, new, x, y) && !covers(level, p, old, x, y) {
                push_run(t, solids, level, events, x, y, d);
            }
        }
    }
}

/// Carries the loose grains resting on part `p` at its `old` place (and
/// the column stacked on each) by its move `d`, where free; a part
/// rising pushes them instead ([`push_grains`]).
fn carry_grains(
    t: &mut Terrain,
    solids: &SolidIndex,
    level: &LiveLevel,
    p: &SolidPart,
    old: (i32, i32),
    d: (i64, i64),
    events: &mut TickEvents,
) {
    if d.1 < 0 {
        return;
    }
    let (w, h) = (i64::from(p.size.0), i64::from(p.size.1));
    for k in 0..w {
        let lx = if d.0 > 0 { w - 1 - k } else { k };
        let x = i64::from(old.0) + lx;
        for ly in 0..h {
            let y = i64::from(old.1) + ly;
            // A top cell of the old footprint with a grain on it, the grain
            // not in the new footprint.
            if !covers(level, p, old, x, y) || covers(level, p, old, x, y - 1) {
                continue;
            }
            if t.loose(x, y - 1) == 0 || covers(level, p, p.at, x, y - 1) {
                continue;
            }
            for j in 1..=MAX_PUSH_CHAIN as i64 {
                let from = (x, y - j);
                if t.loose(from.0, from.1) == 0 {
                    break;
                }
                let to = (from.0 + d.0, from.1 + d.1);
                if !grain_free(t, solids, level, to.0, to.1) {
                    break;
                }
                shift(t, events, from, to);
            }
        }
    }
}

/// Crushes the dirt part `p` (at its new place) covers and covered not at
/// `old`, a [`crate::TerrainOp::Crush`] edit for each row run that held
/// some; how many cells.
fn crush_dirt(
    t: &mut Terrain,
    level: &LiveLevel,
    p: &SolidPart,
    old: (i32, i32),
    events: &mut TickEvents,
) -> u32 {
    let mut crushed = 0;
    let new = p.at;
    let w = i64::from(p.size.0);
    for ly in 0..i64::from(p.size.1) {
        let y = i64::from(new.1) + ly;
        let mut run = None;
        for lx in 0..=w {
            let x = i64::from(new.0) + lx;
            let fresh = lx < w && covers(level, p, new, x, y) && !covers(level, p, old, x, y);
            match (fresh, run) {
                (true, None) => run = Some(x),
                (false, Some(x0)) => {
                    run = None;
                    let mut x0 = x0;
                    while x0 < x {
                        let n = (x - x0).min(i64::from(u16::MAX));
                        let c = t.crush_row(x0 as i32, y as i32, n as u32);
                        if c > 0 {
                            events.crush(x0 as i32, y as i32, n as u16);
                            crushed += c;
                        }
                        x0 += n;
                    }
                }
                _ => {}
            }
        }
    }
    crushed
}

/// The world's side of a motion's step ([`PartMover`]).
struct Mover<'a> {
    terrain: &'a mut Terrain,
    worms: &'a mut [Worm],
    wedged: Wedged,
}

impl PartMover for Mover<'_> {
    /// The step's rules, in order: rock stops it; it pushes the worms its
    /// new footprint meets one pixel its way, and a worm with nowhere to
    /// go is wedged and holds it; it carries the worms standing on it (one
    /// of `check_on_ground`'s pixels is the part's) and not pushed, where
    /// free; a rope hooked to the ground in its old footprint moves with
    /// it; last, it crushes the dirt it moved into. Velocities stay.
    fn step(
        &mut self,
        solids: &mut SolidIndex,
        level: &LiveLevel,
        copy: u16,
        moves: &[(usize, (i32, i32))],
        events: &mut TickEvents,
    ) -> PartStep {
        let t = &*self.terrain;
        let group = moves.iter().fold(0u64, |m, &(i, _)| m | 1 << i);
        // Each moving part's old place and its move, by its index.
        let mut old = [(0i32, 0i32); MAX_LIVE_SOLID_PARTS];
        let mut delta = [(0i32, 0i32); MAX_LIVE_SOLID_PARTS];
        for &(i, new) in moves {
            let at = solids.parts()[i].at;
            old[i] = at;
            delta[i] = (new.0.wrapping_sub(at.0), new.1.wrapping_sub(at.1));
            if blocked(t, solids, level, &solids.parts()[i], at, new, group) {
                return PartStep::Blocked;
            }
        }
        for &(i, new) in moves {
            solids.move_part(i, new);
        }
        // Pushes: every living worm the new footprints meet, by the move of
        // the first part it meets.
        let mut pushed = 0u32;
        let mut push = [(0i32, 0i32); MAX_WORMS];
        let mut stuck = false;
        for (j, w) in self.worms.iter().enumerate().take(MAX_WORMS) {
            if !w.alive {
                continue;
            }
            let bits = overlap(solids, level, w.pos) & group;
            if bits == 0 {
                continue;
            }
            let d = delta[bits.trailing_zeros() as usize];
            pushed |= 1 << j;
            push[j] = d;
            if !free(t, solids, level, moved(w.pos, d)) {
                self.wedged.set(j, copy);
                stuck = true;
            }
        }
        if stuck {
            for &(i, _) in moves {
                solids.move_part(i, old[i]);
            }
            return PartStep::Wait;
        }
        for (j, w) in self.worms.iter_mut().enumerate().take(MAX_WORMS) {
            if pushed & (1 << j) != 0 {
                w.pos = moved(w.pos, push[j]);
            }
        }
        // Carrying: `check_on_ground`'s pixels on a part's old footprint.
        for (j, w) in self.worms.iter_mut().enumerate().take(MAX_WORMS) {
            if !w.alive || pushed & (1 << j) != 0 {
                continue;
            }
            let (px, py) = pixel(w.pos);
            let under = |i: usize| {
                let p = &solids.parts()[i];
                (1..=6).any(|y| {
                    covers(level, p, old[i], px - 2, py + y) || covers(level, p, old[i], px + 2, py + y)
                })
            };
            if let Some(&(i, _)) = moves.iter().find(|&&(i, _)| under(i)) {
                let to = moved(w.pos, delta[i]);
                if free(t, solids, level, to) {
                    w.pos = to;
                }
            }
        }
        // The ropes hooked to the ground in a part.
        for w in self.worms.iter_mut() {
            let r = &mut w.rope;
            if !r.released || !r.attached || r.player_attached >= 0 {
                continue;
            }
            let (hx, hy) = (i64::from(r.pos.x as i32), i64::from(r.pos.y as i32));
            if let Some(&(i, _)) =
                moves.iter().find(|&&(i, _)| covers(level, &solids.parts()[i], old[i], hx, hy))
            {
                r.pos = moved(r.pos, delta[i]);
            }
        }
        // Loose grains: carried, then pushed; what is left in the new
        // footprints is crushed.
        let unit = |i: usize| (i64::from(delta[i].0.signum()), i64::from(delta[i].1.signum()));
        if self.terrain.has_loose() {
            for &(i, _) in moves {
                carry_grains(self.terrain, solids, level, &solids.parts()[i], old[i], unit(i), events);
            }
            for &(i, _) in moves {
                push_grains(self.terrain, solids, level, &solids.parts()[i], old[i], unit(i), events);
            }
        }
        let mut crushed = 0;
        for &(i, _) in moves {
            crushed += crush_dirt(self.terrain, level, &solids.parts()[i], old[i], events);
        }
        if crushed > 0 {
            events.push(EventKind::DirtCrushed { copy, cells: crushed });
        }
        PartStep::Taken
    }
}

/// Pushes each living worm a shown rock part lies on (one that appeared
/// over it) to the nearest free place within [`PUSH_OUT_PX`]: straight up,
/// then left, right and down, the shortest first. One with none is wedged.
fn push_out(t: &Terrain, solids: &SolidIndex, level: &LiveLevel, worms: &mut [Worm], wedged: &mut Wedged) {
    for (j, w) in worms.iter_mut().enumerate().take(MAX_WORMS) {
        if !w.alive {
            continue;
        }
        let bits = overlap(solids, level, w.pos);
        if bits == 0 {
            continue;
        }
        let place = (1..=PUSH_OUT_PX)
            .flat_map(|d| [(0, -d), (-d, 0), (d, 0), (0, d)])
            .map(|d| moved(w.pos, d))
            .find(|&p| free(t, solids, level, p));
        match place {
            Some(p) => w.pos = p,
            None => wedged.set(j, solids.parts()[bits.trailing_zeros() as usize].copy),
        }
    }
}

impl World {
    /// The live step (module docs), once a tick in [`World::tick`] after
    /// the keys are latched and before the first sub-step; none while the
    /// world is frozen. A world without live copies does nothing here.
    pub(crate) fn live_step(&mut self) {
        let Some(live) = &mut self.live else { return };
        let mut mover = Mover {
            terrain: &mut self.terrain,
            worms: &mut self.worms,
            wedged: Wedged { mask: 0, copy: [0; MAX_WORMS] },
        };
        live.step_motions(&mut self.draws.events, &mut mover);
        let mut wedged = mover.wedged;
        if !live.solids.is_empty() {
            push_out(&self.terrain, &live.solids, &live.level, &mut self.worms, &mut wedged);
        }
        self.crush(&wedged);
    }

    /// Crushing (issue #90): a wedged worm's count goes up, any other's
    /// starts again; a chunk of [`CRUSH_CHUNK`] on its first wedged tick and
    /// every [`CRUSH_EVERY`] after, each after its [`EventKind::Crushed`],
    /// dealt by nobody (owner -1) as a normal hit, blood included.
    fn crush(&mut self, wedged: &Wedged) {
        let Some(live) = &mut self.live else { return };
        let mut chunks = 0u32;
        for (i, c) in live.crush_mut().iter_mut().enumerate().take(MAX_WORMS) {
            let alive = self.worms.get(i).is_some_and(|w| w.alive);
            if alive && wedged.mask & (1 << i) != 0 {
                *c = c.saturating_add(1);
                if (*c - 1) % CRUSH_EVERY == 0 {
                    chunks |= 1 << i;
                }
            } else {
                *c = 0;
            }
        }
        while chunks != 0 {
            let i = chunks.trailing_zeros() as usize;
            chunks &= chunks - 1;
            let id = self.worms[i].id;
            self.draws.events.push(EventKind::Crushed { worm: id, copy: wedged.copy[i] });
            injure_worm(&mut self.harm(), id, CRUSH_CHUNK, -1);
        }
    }

    /// Worm slot `i`'s consecutive wedged ticks (issue #90: crushing); 0
    /// in a world without live copies.
    pub fn crush_count(&self, i: usize) -> u16 {
        self.live.as_ref().and_then(|l| l.crush().get(i).copied()).unwrap_or(0)
    }
}

#[cfg(test)]
#[path = "live_step_tests.rs"]
mod tests;
