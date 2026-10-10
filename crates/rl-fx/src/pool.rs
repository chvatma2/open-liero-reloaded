//! The entity pool: OLX's `FastVector<entity_t, MAX_ENTITIES>`.

use rl_data::image::Rgba;
use rl_math::{TimeDiff, Vec2};

use crate::MAX_ENTITIES;
use crate::entity::{Entity, Kind};

/// A fixed array of slots. New entities take the lowest free slot and are
/// dropped when none is free; iteration is in slot order. A freed slot keeps
/// its old contents, as OLX's does.
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
}

impl Pool {
    pub fn new() -> Pool {
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
        Pool {
            slots: vec![blank; MAX_ENTITIES],
            used: vec![false; MAX_ENTITIES],
            first_unused: 0,
            last_used: -1,
        }
    }

    /// `clear`.
    pub fn clear(&mut self) {
        self.used.iter_mut().for_each(|u| *u = false);
        self.first_unused = 0;
        self.last_used = -1;
    }

    /// `size`: the used slots up to the last used one.
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// Whether a new entity would be dropped.
    pub fn is_full(&self) -> bool {
        self.first_unused >= MAX_ENTITIES
    }

    pub fn get(&self, i: usize) -> &Entity {
        &self.slots[i]
    }

    pub fn get_mut(&mut self, i: usize) -> &mut Entity {
        &mut self.slots[i]
    }

    /// `getNewObj` and `entity_t::Spawn`: puts `e` in the lowest free slot.
    pub fn add(&mut self, e: Entity) -> Option<usize> {
        if self.first_unused >= MAX_ENTITIES {
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
                || self.first_unused >= MAX_ENTITIES
                || !self.used[self.first_unused]
            {
                break;
            }
        }
        self.slots[new] = e;
        self.used[new] = true;
        Some(new)
    }

    /// `entity_t::setUnused`, then `refreshObj`.
    pub fn remove(&mut self, i: usize) {
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
}
