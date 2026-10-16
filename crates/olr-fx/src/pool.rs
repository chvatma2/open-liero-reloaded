//! The entity pool: OLX's `FastVector<entity_t, MAX_ENTITIES>`, of any size.

use olr_data::image::Rgba;
use olr_math::{TimeDiff, Vec2};

use crate::MAX_ENTITIES;
use crate::entity::{Entity, Kind};

/// A fixed array of slots ([`MAX_ENTITIES`], OLX's, or play's
/// [`crate::PLAY_ENTITIES`]), allocated once. New entities take the lowest
/// free slot and are dropped when none is free, or when as many are used as
/// the caller's limit allows ([`Pool::add_below`]); iteration is in slot
/// order. A freed slot keeps its old contents, as OLX's does.
///
/// OLX: include/FastVector.h:27-150
#[derive(Clone, Debug)]
pub(crate) struct Pool {
    slots: Vec<Entity>,
    used: Vec<bool>,
    /// `m_firstUnused`: the lowest free slot.
    first_unused: usize,
    /// `m_lastUsed`: no slot above it is used (-1 when empty).
    last_used: isize,
    /// How many slots are used.
    count: usize,
}

impl Pool {
    /// OLX's pool: [`MAX_ENTITIES`] slots.
    pub fn new() -> Pool {
        Pool::with_capacity(MAX_ENTITIES)
    }

    /// A pool of `capacity` slots (a bench-only size, plan 8; at least 1).
    pub fn with_capacity(capacity: usize) -> Pool {
        let blank = Entity {
            kind: Kind::Smoke,
            type2: 0,
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            colour: Rgba::BLACK,
            frame: 0.0,
            extra: TimeDiff(0),
            angl_vel: 0.0,
            rotation: 0,
        };
        let capacity = capacity.max(1);
        Pool {
            slots: vec![blank; capacity],
            used: vec![false; capacity],
            first_unused: 0,
            last_used: -1,
            count: 0,
        }
    }

    /// How many slots it has.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// `clear`.
    pub fn clear(&mut self) {
        self.used.iter_mut().for_each(|u| *u = false);
        self.first_unused = 0;
        self.last_used = -1;
        self.count = 0;
    }

    /// `size`: the used slots (counted, not scanned: play's pool is large).
    pub fn len(&self) -> usize {
        self.count
    }

    /// Whether a new entity would be dropped with `limit` used slots
    /// allowed.
    pub fn is_full_below(&self, limit: usize) -> bool {
        self.count >= limit || self.first_unused >= self.slots.len()
    }

    pub fn get(&self, i: usize) -> &Entity {
        &self.slots[i]
    }

    pub fn get_mut(&mut self, i: usize) -> &mut Entity {
        &mut self.slots[i]
    }

    /// `getNewObj` and `entity_t::Spawn`: puts `e` in the lowest free slot.
    #[cfg(test)]
    pub fn add(&mut self, e: Entity) -> Option<usize> {
        self.add_below(e, usize::MAX)
    }

    /// [`Pool::add`] while fewer than `limit` slots are used.
    pub fn add_below(&mut self, e: Entity, limit: usize) -> Option<usize> {
        if self.is_full_below(limit) {
            return None;
        }
        let new = self.first_unused;
        if new as isize > self.last_used {
            self.last_used = new as isize;
        }
        // `findNewFirstUnused`.
        loop {
            self.first_unused += 1;
            if self.first_unused as isize > self.last_used
                || self.first_unused >= self.slots.len()
                || !self.used[self.first_unused]
            {
                break;
            }
        }
        self.slots[new] = e;
        self.used[new] = true;
        self.count += 1;
        Some(new)
    }

    /// `entity_t::setUnused`, then `refreshObj`. Removing a free slot again
    /// (OLX's second switch does) only moves the bounds.
    pub fn remove(&mut self, i: usize) {
        if self.used[i] {
            self.count -= 1;
        }
        self.used[i] = false;
        if i < self.first_unused {
            self.first_unused = i;
        }
        if i as isize >= self.last_used {
            // `findNewLastUsed`.
            loop {
                self.last_used -= 1;
                if self.last_used < 0
                    || self.last_used < self.first_unused as isize
                    || self.used[self.last_used as usize]
                {
                    break;
                }
            }
        }
    }

    /// The iterator's `next`: the first used slot after `after` (from 0
    /// without one), up to the last used slot as it is now.
    pub fn next(&self, after: Option<usize>) -> Option<usize> {
        let start = after.map_or(0, |i| i + 1);
        let end = usize::try_from(self.last_used).ok()?;
        (start..=end).find(|&i| self.used[i])
    }

    /// The used slots in order.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &Entity)> {
        let end = usize::try_from(self.last_used + 1).unwrap_or(0);
        (0..end).filter(|&i| self.used[i]).map(|i| (i, &self.slots[i]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smoke() -> Entity {
        Pool::new().slots[0]
    }

    #[test]
    fn slots_are_reused_lowest_first_and_bounds_follow() {
        let mut p = Pool::new();
        for i in 0..4 {
            assert_eq!(p.add(smoke()), Some(i));
        }
        p.remove(1);
        p.remove(3);
        assert_eq!((p.first_unused, p.last_used), (1, 2));
        assert_eq!(p.add(smoke()), Some(1));
        assert_eq!(p.add(smoke()), Some(3));
        p.remove(0);
        p.remove(1);
        p.remove(2);
        p.remove(3);
        assert_eq!((p.first_unused, p.last_used, p.len()), (0, -1, 0));
        assert_eq!(p.next(None), None);
        for _ in 0..MAX_ENTITIES {
            assert!(p.add(smoke()).is_some());
        }
        assert_eq!(p.add(smoke()), None);
        assert_eq!(p.iter().count(), MAX_ENTITIES);
    }

    #[test]
    fn the_count_follows_adds_and_removes() {
        let mut p = Pool::with_capacity(8);
        for _ in 0..6 {
            p.add(smoke());
        }
        p.remove(2);
        p.remove(2);
        p.remove(5);
        assert_eq!((p.len(), p.iter().count()), (4, 4));
        assert_eq!(p.add_below(smoke(), 4), None, "4 used, a limit of 4");
        assert_eq!(p.add_below(smoke(), 5), Some(2));
        p.clear();
        assert_eq!(p.len(), 0);
    }

    #[test]
    fn a_pool_of_any_size_fills_to_it() {
        let mut p = Pool::with_capacity(3);
        assert_eq!(p.capacity(), 3);
        for i in 0..3 {
            assert_eq!(p.add(smoke()), Some(i));
        }
        assert!(p.is_full_below(usize::MAX));
        assert_eq!(p.add(smoke()), None);
        assert_eq!(Pool::new().capacity(), MAX_ENTITIES);
    }
}
