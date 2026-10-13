//! The generators the simulation draws from, in OLX's order (spec §5.5).

use alloc::vec;
use alloc::vec::Vec;

use rl_math::{GlibcRand, Mt19937, Vec2};

use crate::events::{CosmeticKind, TickEvents};
use crate::tally::{ActionTally, HitAudit};

/// The dedicated server's entity pool size (`tEntities`, `FastVector<entity_t, 1024>`,
/// `src/client/Entity.cpp:31-32`).
const MAX_ENTITIES: u32 = 1024;

/// `rand()`, `FindSpot`'s `rnd()` and the entity counter, the projectile
/// types whose `RotIncrement` a rotation draw bumped, and the tick's events,
/// which are emitted where these draws happen.
///
/// OLX throws cosmetic entities (particles, blood, sparkles) that draw from the
/// same `rand()` stream as gameplay, so the draws are made, and only reported
/// as events. A dedicated server never frees an entity, so the pool only fills.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Draws {
    pub rand: GlibcRand,
    pub mt: Mt19937,
    pub ents: u32,
    /// During [`crate::World::tick`], the caller's buffer; between ticks,
    /// the setup events for the next tick (`TickEvents::start_tick`).
    pub events: TickEvents,
    /// Per projectile type: `Proj_SpawnInfo::apply` set its zero
    /// `RotIncrement` to 1 (`PhysicsLX56_Projectiles.cpp:590-593`), which
    /// OLX keeps in the loaded mod and `CProjectile::Draw` reads.
    pub rot_bumped: Vec<bool>,
    pub spawn_work: WorkCounter,
    /// The projectile actions applied, for coverage reports (not state).
    pub actions: ActionTally,
    /// Where worm hits landed, for the hit invariant (not state).
    pub hits: HitAudit,
}

/// Spawned children `Proj_SpawnInfo::apply` computed in full (aim, speed,
/// position), counted for tests that bound the work per tick. Not state: it
/// never makes two worlds unequal, and the checksum leaves it out.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WorkCounter(pub u64);

impl PartialEq for WorkCounter {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Draws {
    /// `projectile_types` sizes [`Draws::rot_bumped`].
    pub fn new(rand: GlibcRand, mt: Mt19937, projectile_types: usize) -> Self {
        Self {
            rand,
            mt,
            ents: 0,
            events: TickEvents::pending(),
            rot_bumped: vec![false; projectile_types],
            spawn_work: WorkCounter(0),
            actions: ActionTally::default(),
            hits: HitAudit::default(),
        }
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

    /// `n` dirt particles at `pos`, coloured like the map at `probe`, each
    /// thrown with `vel(GetRandomVec())`.
    pub fn particles(&mut self, n: u32, pos: Vec2, probe: (i32, i32), vel: impl Fn(Vec2) -> Vec2) {
        for _ in 0..n {
            let v = self.rand.random_vec();
            self.entity(CosmeticKind::Dirt { probe }, pos, vel(v));
        }
    }
}
