//! The [`Simulation`] of the real game: an `rl_sim::World` with its terrain
//! journal on (networking spec §3.3), checksummed with the terrain's cell
//! hash (decision N4) and transferred as rl-sim's `WorldState` (N5).

use alloc::vec::Vec;

use rl_sim::{Keys, Snapshot, TickEvents, World, WorldState};
use rl_terrain::Area;

use crate::{Session, Simulation, StateRejected};

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

    /// The map areas restores and loaded states put back since the last
    /// call ([`World::take_restored_areas`]); see
    /// [`Session::take_restored_areas`] for what the presentation does.
    pub fn take_restored_areas(&mut self) -> Vec<Area> {
        self.world.take_restored_areas()
    }
}

impl Session<WorldSim> {
    /// The map areas the session's restores and loaded states put back
    /// since the last call ([`World::take_restored_areas`]), without
    /// touching the timeline (unlike [`Session::sim_mut`]).
    ///
    /// The presentation's colour map follows the ticks' terrain edits
    /// (`EventKind::TerrainEdit`, from [`WorldSim::events`] in
    /// [`crate::Driver::simulated`]), which say nothing of an undo: a
    /// mispredicted hole would stay drawn. So after every
    /// [`Session::advance`] (the list is empty unless it reported
    /// [`crate::Event::Rollback`] or [`crate::Event::StateLoaded`] or, at
    /// a host, sent a repair, whose quiet re-simulation reports areas it
    /// did again), the game takes these areas, calls
    /// `ColorMap::rebuild_area` for each with `sim().world().terrain()`,
    /// and only then applies the edits of the ticks the advance simulated,
    /// in order (collected in `Driver::simulated`, re-simulated ones
    /// included). Every restore of an advance comes before its reported
    /// ticks. The rebuild paints from the terrain after the advance: the
    /// edits applied after it paint their rims again; blood stains and the
    /// level's own dirt art inside the areas are lost, as for
    /// `TickEvents::lost_area`. `rl-netcode`'s `tests/world_sim.rs`
    /// (`colour_maps_follow_rollbacks_and_loads`) does exactly this.
    pub fn take_restored_areas(&mut self) -> Vec<Area> {
        self.sim.take_restored_areas()
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
