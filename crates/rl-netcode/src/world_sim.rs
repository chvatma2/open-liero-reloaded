//! The [`Simulation`] of the real game: an `rl_sim::World` with its terrain
//! journal on (networking spec §3.3), checksummed with the terrain's cell
//! hash (decision N4) and transferred as rl-sim's `WorldState` (N5).

use alloc::vec::Vec;

use rl_sim::{Keys, Snapshot, TickEvents, World, WorldState};

use crate::{Simulation, StateRejected};

/// A world under rollback.
pub struct WorldSim {
    world: World,
    events: TickEvents,
}

impl WorldSim {
    /// Takes `world` and turns on its rollback (the terrain journal).
    pub fn new(mut world: World) -> Self {
        world.enable_rollback();
        WorldSim { world, events: TickEvents::new() }
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// The world, to change outside the session's timeline (a test's planted
    /// fault): through `Session::sim_mut`, which knows it cannot roll back
    /// across the change.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The events of the last simulated tick. After a restore or a loaded
    /// state they are stale (the tick before it) until the next tick.
    pub fn events(&self) -> &TickEvents {
        &self.events
    }
}

impl Simulation for WorldSim {
    type Snapshot = Snapshot;

    fn tick(&mut self, keys: &[Keys]) {
        self.world.tick(keys, &mut self.events);
    }

    fn save(&mut self) -> Snapshot {
        self.world.save()
    }

    fn restore(&mut self, s: &Snapshot) {
        // The session restores only snapshots it kept and has not forgotten:
        // a refusal is a bug of the session, not of the input.
        self.world.restore(s).expect("a kept snapshot restores");
    }

    fn forget_before(&mut self, s: &Snapshot) {
        self.world.forget_before(s);
    }

    fn checksum(&self) -> u64 {
        self.world.sync_checksum()
    }

    fn encode_state(&self) -> Vec<u8> {
        self.world.state().encode()
    }

    /// Every `StateError` (one that does not decode, does not fit this
    /// world's setup, or holds values out of range) is a refusal; the world
    /// is unchanged then.
    fn decode_state(&mut self, bytes: &[u8]) -> Result<(), StateRejected> {
        let s = WorldState::decode(bytes).map_err(|_| StateRejected)?;
        self.world.load_state(s).map_err(|_| StateRejected)
    }
}
