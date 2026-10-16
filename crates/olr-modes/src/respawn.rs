//! What a worm holds when it spawns (spec §20 F10, notes §4.6): the weapons
//! it has (OLX), or the chaos bots' rotation through every weapon of the
//! mod, which the mode gives on each spawn with `ModeCtx::give_weapon`, so
//! bots only press keys and replays stay keys only.

use olr_sim::{Fnv, MAX_SLOTS, MAX_WORMS};

/// A Deathmatch setting fixed at the setup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RespawnLoadout {
    /// A worm keeps its weapons (OLX).
    #[default]
    Keep,
    /// Chaos bots get the next weapons of the rotation on every spawn.
    ChaosCycle(ChaosCycle),
}

/// The chaos rotation: chaos bot `i` of `n` takes the weapons
/// `(i·5 + r·5·n + j) mod W`, `j` in `0..5`, on its `r`-th life (counting
/// the first spawn as life 0), `W` being the mod's weapon count. Over a few
/// lives every weapon comes round, whatever the restrictions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChaosCycle {
    /// Each worm's index among the chaos bots, by worm id (`None` for the
    /// others).
    index: [Option<u8>; MAX_WORMS],
    /// How many chaos bots there are (`n`).
    count: u8,
    /// The mod's weapon count (`W`).
    weapons: u32,
}

impl ChaosCycle {
    /// The rotation for the chaos bots `chaos` (worm ids, in their chaos
    /// order; ids from [`MAX_WORMS`] on are ignored) in a mod of `weapons`
    /// weapons.
    pub fn new(chaos: &[usize], weapons: usize) -> Self {
        let mut index = [None; MAX_WORMS];
        let mut count = 0u8;
        for &w in chaos {
            if let Some(slot) = index.get_mut(w)
                && slot.is_none()
            {
                *slot = Some(count);
                count += 1;
            }
        }
        ChaosCycle { index, count, weapons: u32::try_from(weapons).unwrap_or(u32::MAX) }
    }

    /// The chaos bots' worm ids in their chaos order (what [`ChaosCycle::new`]
    /// was given, without duplicates or ids from [`MAX_WORMS`] on).
    pub fn worms(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.count).filter_map(|i| self.index.iter().position(|x| *x == Some(i)))
    }

    /// The mod's weapon count (`W`).
    pub fn weapon_count(&self) -> usize {
        usize::try_from(self.weapons).unwrap_or(usize::MAX)
    }

    /// The weapons worm `worm` gets on its life `life`, or `None` when it is
    /// no chaos bot or the mod has no weapons.
    pub fn weapons_for(&self, worm: i32, life: u32) -> Option<[i32; MAX_SLOTS]> {
        let i = u64::from(*self.index.get(usize::try_from(worm).ok()?)?.as_ref()?);
        let w = u64::from(self.weapons);
        if w == 0 {
            return None;
        }
        let (slots, n) = (MAX_SLOTS as u64, u64::from(self.count));
        // `(i·5 + r·5·n + j) mod W` without overflow: every term reduced first.
        let base = (i * slots % w + (u64::from(life) % w) * (slots * n % w) % w) % w;
        Some(core::array::from_fn(|j| ((base + j as u64) % w) as i32))
    }

    pub(crate) fn hash(&self, h: &mut Fnv) {
        self.index.iter().for_each(|i| h.i32(i.map_or(-1, i32::from)));
        h.u32(u32::from(self.count));
        h.u32(self.weapons);
    }
}

impl RespawnLoadout {
    pub(crate) fn hash(&self, h: &mut Fnv) {
        match self {
            RespawnLoadout::Keep => h.u32(0),
            RespawnLoadout::ChaosCycle(c) => {
                h.u32(1);
                c.hash(h);
            }
        }
    }
}
