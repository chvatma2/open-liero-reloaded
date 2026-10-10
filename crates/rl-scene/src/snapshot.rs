//! The previous tick's drawable state and the interpolation between it and
//! the current world (spec §8.2, §19 E2).

use rl_math::{AbsTime, Vec2};
use rl_sim::{Bonus, MAX_BONUSES, MAX_WORMS, Projectile, ProjectilePool, World, Worm};

/// What a worm looked like at a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WormSnap {
    pub alive: bool,
    pub pos: Vec2,
    pub angle: f32,
    /// The walk frame (`fFrame`).
    pub frame: f32,
    /// Changes with every death, so a death and respawn between two
    /// snapshots is seen even when the worm is alive in both.
    pub time_of_death: AbsTime,
    /// The rope is out.
    pub rope_released: bool,
    pub hook: Vec2,
}

impl WormSnap {
    pub fn of(w: &Worm) -> WormSnap {
        WormSnap {
            alive: w.alive,
            pos: w.pos,
            angle: w.angle,
            frame: w.frame,
            time_of_death: w.time_of_death,
            rope_released: w.rope.released,
            hook: w.rope.pos,
        }
    }
}

/// What a used projectile slot held at a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProjSnap {
    pub slot: u32,
    /// [`Projectile::serial`]: a new projectile in the same slot has another.
    pub serial: u32,
    pub pos: Vec2,
}

/// What a client bonus slot held at a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BonusSnap {
    pub used: bool,
    /// `fSpawnTime`: another bonus in the same slot has another.
    pub spawn_time: AbsTime,
    pub pos: Vec2,
}

impl BonusSnap {
    pub fn of(b: &Bonus) -> BonusSnap {
        BonusSnap { used: b.used, spawn_time: b.spawn_time, pos: b.pos }
    }
}

/// The drawable state of a world at one tick: positions, angles, frames,
/// serials and alive flags. [`Snapshot::capture_into`] reuses its buffers,
/// so recapturing allocates nothing once they are big enough.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    worms: Vec<WormSnap>,
    projectiles: Vec<ProjSnap>,
    bonuses: [BonusSnap; MAX_BONUSES],
    time: Option<AbsTime>,
}

impl Default for Snapshot {
    fn default() -> Snapshot {
        Snapshot::new()
    }
}

impl Snapshot {
    /// An empty snapshot: nothing blends with it.
    pub fn new() -> Snapshot {
        Snapshot {
            worms: Vec::with_capacity(MAX_WORMS),
            projectiles: Vec::with_capacity(ProjectilePool::DEFAULT_CAPACITY),
            bonuses: [BonusSnap::default(); MAX_BONUSES],
            time: None,
        }
    }

    pub fn capture(world: &World) -> Snapshot {
        let mut s = Snapshot::new();
        s.capture_into(world);
        s
    }

    pub fn capture_into(&mut self, world: &World) {
        self.worms.clear();
        self.worms.extend(world.worms().iter().map(WormSnap::of));
        self.projectiles.clear();
        self.projectiles.extend(world.projectiles().map(|(slot, p)| ProjSnap {
            slot: slot as u32,
            serial: p.serial(),
            pos: p.pos,
        }));
        for (s, b) in self.bonuses.iter_mut().zip(world.bonuses()) {
            *s = BonusSnap::of(b);
        }
        self.time = Some(world.time());
    }

    /// Forgets everything (after a restart).
    pub fn clear(&mut self) {
        self.worms.clear();
        self.projectiles.clear();
        self.bonuses = [BonusSnap::default(); MAX_BONUSES];
        self.time = None;
    }

    pub fn worms(&self) -> &[WormSnap] {
        &self.worms
    }

    /// Used slots in slot order.
    pub fn projectiles(&self) -> &[ProjSnap] {
        &self.projectiles
    }

    /// The client's bonus slots ([`World::bonuses`]); unused when empty.
    pub fn bonuses(&self) -> &[BonusSnap; MAX_BONUSES] {
        &self.bonuses
    }

    /// The world's time when captured; `None` when empty.
    pub fn time(&self) -> Option<AbsTime> {
        self.time
    }
}

/// How positions are drawn (spec §19 E2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Interpolation {
    /// Blend the previous tick and the current state, at sub-pixel
    /// positions.
    #[default]
    Linear,
    /// Draw the latest state at OLX's whole-pixel positions, like OLX.
    Latest,
}

/// Whether worm `prev` may be blended into `cur`: the same life (no death
/// and no respawn between them; a new worm has no previous state).
pub fn worm_blends(prev: &WormSnap, cur: &Worm) -> bool {
    prev.alive == cur.alive && prev.time_of_death == cur.time_of_death
}

fn lerp(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    a + (b - a) * t
}

/// Draw positions between a snapshot and the current world.
#[derive(Clone, Copy, Debug)]
pub struct Blend<'a> {
    prev: &'a Snapshot,
    alpha: f32,
    mode: Interpolation,
}

impl<'a> Blend<'a> {
    /// `alpha` 0 draws `prev`, 1 the current state (clamped to that range).
    pub fn new(prev: &'a Snapshot, alpha: f32, mode: Interpolation) -> Blend<'a> {
        let alpha = if alpha.is_nan() { 1.0 } else { alpha.clamp(0.0, 1.0) };
        Blend { prev, alpha, mode }
    }

    /// OLX's whole-pixel drawing of the latest state.
    pub fn exact(&self) -> bool {
        self.mode == Interpolation::Latest
    }

    /// A position as drawn: OLX's `(int)` truncation when exact.
    pub fn snap(&self, p: Vec2) -> Vec2 {
        if self.exact() { Vec2::new((p.x as i32) as f32, (p.y as i32) as f32) } else { p }
    }

    fn prev_worm(&self, i: usize, w: &Worm) -> Option<&WormSnap> {
        if self.exact() {
            return None;
        }
        self.prev.worms.get(i).filter(|p| worm_blends(p, w))
    }

    /// Worm `i`'s drawn position (unsnapped).
    pub fn worm(&self, i: usize, w: &Worm) -> Vec2 {
        match self.prev_worm(i, w) {
            Some(p) => lerp(p.pos, w.pos, self.alpha),
            None => w.pos,
        }
    }

    /// Worm `i`'s hook position (unsnapped); a hook thrown since the
    /// snapshot is not blended.
    pub fn hook(&self, i: usize, w: &Worm) -> Vec2 {
        match self.prev_worm(i, w).filter(|p| p.rope_released && w.rope.released) {
            Some(p) => lerp(p.hook, w.rope.pos, self.alpha),
            None => w.rope.pos,
        }
    }

    /// The bonus in `slot`'s drawn position (unsnapped): blended only with
    /// the snapshot's bonus of the same slot and spawn time, so a bonus
    /// picked up and another spawned in its slot never slides.
    pub fn bonus(&self, slot: usize, b: &Bonus) -> Vec2 {
        if self.exact() {
            return b.pos;
        }
        match self.prev.bonuses.get(slot) {
            Some(p) if p.used && b.used && p.spawn_time == b.spawn_time => lerp(p.pos, b.pos, self.alpha),
            _ => b.pos,
        }
    }

    /// A cursor for the projectiles, which must be asked in slot order.
    pub fn projectiles(&self) -> ProjBlend<'a> {
        let prev = if self.exact() { &[][..] } else { &self.prev.projectiles[..] };
        ProjBlend { prev, at: 0, alpha: self.alpha }
    }
}

/// Blends projectiles walked in slot order with the snapshot's.
#[derive(Clone, Debug)]
pub struct ProjBlend<'a> {
    prev: &'a [ProjSnap],
    at: usize,
    alpha: f32,
}

impl ProjBlend<'_> {
    /// The projectile in `slot`'s drawn position (unsnapped). It is blended
    /// only with the snapshot's projectile of the same slot and serial, so
    /// a projectile freed and another spawned in its slot never slides.
    pub fn pos(&mut self, slot: usize, p: &Projectile) -> Vec2 {
        while self.prev.get(self.at).is_some_and(|q| (q.slot as usize) < slot) {
            self.at += 1;
        }
        match self.prev.get(self.at) {
            Some(q) if q.slot as usize == slot && q.serial == p.serial() => lerp(q.pos, p.pos, self.alpha),
            _ => p.pos,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worms_blend_only_within_one_life() {
        let mut w = Worm::spawned(0, Vec2::new(10.0, 10.0), AbsTime(1000), &rl_sim::SimSettings::lx56());
        let snap = WormSnap::of(&w);
        assert!(worm_blends(&snap, &w));
        // Died since: no blend, whether dead now or already respawned.
        w.time_of_death = AbsTime(1500);
        assert!(!worm_blends(&snap, &w));
        w.alive = false;
        assert!(!worm_blends(&snap, &w));
        // Dead in both (the same death): the frozen position blends with itself.
        assert!(worm_blends(&WormSnap::of(&w), &w));
    }
}
