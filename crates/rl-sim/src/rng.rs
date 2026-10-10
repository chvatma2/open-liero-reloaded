//! The generators the simulation draws from, in OLX's order (spec §5.5).

use rl_math::{GlibcRand, Mt19937, Vec2};

use crate::events::{CosmeticKind, TickEvents};

/// The dedicated server's entity pool size (`tEntities`, `FastVector<entity_t, 1024>`,
/// `src/client/Entity.cpp:31-32`).
const MAX_ENTITIES: u32 = 1024;

/// `rand()`, `FindSpot`'s `rnd()` and the entity counter, and the tick's
/// events, which are emitted where these draws happen.
///
/// OLX throws cosmetic entities (particles, blood, sparkles) that draw from the
/// same `rand()` stream as gameplay, so the draws are made, and only reported
/// as events. A dedicated server never frees an entity, so the pool only fills.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Draws {
    pub rand: GlibcRand,
    pub mt: Mt19937,
    pub ents: u32,
    pub events: TickEvents,
    /// Spawned children `Proj_SpawnInfo::apply` computed in full (aim, speed,
    /// position), counted for tests that bound the work per tick. Not state.
    pub spawn_work: u64,
}

impl Draws {
    pub fn new(rand: GlibcRand, mt: Mt19937) -> Self {
        Self { rand, mt, ents: 0, events: TickEvents::new(), spawn_work: 0 }
    }

    /// `SpawnEntity` (`Entity.cpp:160-205`) for a type that draws nothing itself:
    /// it takes a pool slot while there is one. Its arguments' draws (the
    /// callers' `random_vec` calls) happen first, whether or not it does.
    /// Returns whether it took one.
    pub fn spawn_entity(&mut self) -> bool {
        let room = self.ents < MAX_ENTITIES;
        if room {
            self.ents += 1;
        }
        room
    }

    /// `SpawnEntity(kind, .., pos, vel, ..)`: a pool slot as
    /// [`spawn_entity`](Self::spawn_entity) takes one, and the event, which is
    /// emitted even when the pool is full. Returns whether it took a slot.
    pub fn entity(&mut self, kind: CosmeticKind, pos: Vec2, vel: Vec2) -> bool {
        self.events.cosmetic(kind, pos, vel);
        self.spawn_entity()
    }

    /// `n` dirt particles at `pos`, each thrown with `vel(GetRandomVec())`.
    pub fn particles(&mut self, n: u32, pos: Vec2, vel: impl Fn(Vec2) -> Vec2) {
        for _ in 0..n {
            let v = self.rand.random_vec();
            self.entity(CosmeticKind::Dirt, pos, vel(v));
        }
    }
}
