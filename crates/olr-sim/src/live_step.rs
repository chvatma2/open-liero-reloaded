//! The live step (issue #90): once a tick, before the physics, the live
//! copies' motions step their rock parts a whole pixel at a time, pushing,
//! carrying and wedging worms and taking the ropes hooked to them; then a
//! part that appeared over a worm pushes it out or wedges it; then the
//! wedged worms are crushed. No OLX counterpart.
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
use crate::events::EventKind;
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

/// The world's side of a motion's step ([`PartMover`]).
struct Mover<'a> {
    terrain: &'a Terrain,
    worms: &'a mut [Worm],
    wedged: Wedged,
}

impl PartMover for Mover<'_> {
    /// The step's rules, in order: rock stops it; it pushes the worms its
    /// new footprint meets one pixel its way, and a worm with nowhere to
    /// go is wedged and holds it; it carries the worms standing on it (one
    /// of `check_on_ground`'s pixels is the part's) and not pushed, where
    /// free; a rope hooked to the ground in its old footprint moves with
    /// it. Velocities stay.
    fn step(
        &mut self,
        solids: &mut SolidIndex,
        level: &LiveLevel,
        copy: u16,
        moves: &[(usize, (i32, i32))],
    ) -> PartStep {
        let t = self.terrain;
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
            terrain: &self.terrain,
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
