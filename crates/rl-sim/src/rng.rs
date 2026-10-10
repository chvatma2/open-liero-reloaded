//! The generators the simulation draws from, in OLX's order (spec §5.5).

use rl_math::{GlibcRand, Mt19937};

/// The dedicated server's entity pool size (`tEntities`, `FastVector<entity_t, 1024>`,
/// `src/client/Entity.cpp:31-32`).
const MAX_ENTITIES: u32 = 1024;

/// `rand()`, `FindSpot`'s `rnd()` and the entity counter.
///
/// OLX throws cosmetic entities (particles, blood, sparkles) that draw from the
/// same `rand()` stream as gameplay, so the draws are made and thrown away.
/// A dedicated server never frees an entity, so the pool only fills.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Draws {
    pub rand: GlibcRand,
    pub mt: Mt19937,
    pub ents: u32,
}

impl Draws {
    pub fn new(rand: GlibcRand, mt: Mt19937) -> Self {
        Self { rand, mt, ents: 0 }
    }

    /// `SpawnEntity` (`Entity.cpp:160-205`) for a type that draws nothing itself:
    /// it takes a pool slot while there is one. Its arguments' draws (the
    /// callers' `random_vec` calls) happen first, whether or not it does.
    pub fn spawn_entity(&mut self) {
        if self.ents < MAX_ENTITIES {
            self.ents += 1;
        }
    }

    /// `n` particles thrown with `GetRandomVec()` velocities.
    pub fn particles(&mut self, n: u32) {
        for _ in 0..n {
            self.rand.random_vec();
            self.spawn_entity();
        }
    }
}
