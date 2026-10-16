//! Work units (spec §20 F7, notes §4.3): a bot's per-tick work is bounded by
//! the number of terrain probes it makes, never by time, so bot matches are
//! reproducible. One unit is one pixel read from the terrain (and one per
//! 64 areas a search scans in its cache).
//!
//! Every terrain primitive checks its worst case before it starts
//! ([`Budget::afford`]) and spends what it really probed, so a `think` never
//! spends more than its budget. Work that does not fit waits for the next
//! tick (pathfinding is resumable); a primitive whose worst case exceeds a
//! whole budget is never started and counts as blocked.

/// The units one `think` may spend, and what it has spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    limit: u32,
    used: u32,
}

/// What a primitive that may not run says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Afford {
    /// It fits in what is left.
    Yes,
    /// It fits in a whole budget, but not in what is left: wait a tick.
    Later,
    /// It exceeds a whole budget: it never runs.
    Never,
}

impl Budget {
    pub fn new(limit: u32) -> Self {
        Budget { limit, used: 0 }
    }

    /// The units this budget started with.
    pub fn limit(&self) -> u32 {
        self.limit
    }

    /// The units spent so far.
    pub fn used(&self) -> u32 {
        self.used
    }

    /// The units left.
    pub fn left(&self) -> u32 {
        self.limit.saturating_sub(self.used)
    }

    /// Whether `worst` units can be spent now, later, or never.
    pub(crate) fn afford(&self, worst: u64) -> Afford {
        if worst <= u64::from(self.left()) {
            Afford::Yes
        } else if worst <= u64::from(self.limit) {
            Afford::Later
        } else {
            Afford::Never
        }
    }

    /// Counts `n` units spent.
    pub(crate) fn spend(&mut self, n: u32) {
        self.used = self.used.saturating_add(n);
    }

    /// A budget of at most `share` of what is left, to be given back with
    /// [`Budget::absorb`].
    pub(crate) fn split(&self, share: u32) -> Budget {
        Budget::new(self.left().min(share))
    }

    /// Counts what a budget from [`Budget::split`] spent.
    pub(crate) fn absorb(&mut self, part: Budget) {
        self.spend(part.used);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn afford_tells_now_later_and_never() {
        let mut b = Budget::new(100);
        b.spend(60);
        assert_eq!(
            (b.afford(40), b.afford(41), b.afford(100), b.afford(101)),
            (Afford::Yes, Afford::Later, Afford::Later, Afford::Never)
        );
        let mut part = b.split(30);
        part.spend(30);
        b.absorb(part);
        assert_eq!((b.used(), b.left()), (90, 10));
    }
}
