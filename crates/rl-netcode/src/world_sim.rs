//! The [`Simulation`] of the real game: an `rl_sim::World` with its terrain
//! journal on (networking spec §3.3), checksummed with the terrain's cell
//! hash (decision N4) and transferred as rl-sim's `WorldState` (N5).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use rl_math::Vec2;
use rl_modes::Deathmatch;
use rl_sim::{Event, EventKind, Keys, LostArea, MatchResult, Score, Snapshot, TickEvents, World, WorldState};
use rl_terrain::Area;

use crate::{Session, Simulation, StateRejected, TickInfo};

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

/// One simulated tick for the presentation (plan 10's sounds, effects and
/// colour map): every tick [`crate::Driver::simulated`] saw, re-simulated
/// ones included.
#[derive(Clone, Debug, PartialEq)]
pub struct TickedTick {
    pub tick: u32,
    /// Re-simulated by a rollback (or after a loaded state).
    pub resim: bool,
    /// The physics sub-steps it ran (`World::substeps_last_tick`).
    pub substeps: u8,
    /// The area of terrain edits it dropped (`TickEvents::lost_area`).
    pub lost: Option<LostArea>,
    /// Its events, in order.
    pub events: Vec<Event>,
}

/// One confirmed tick for the HUD, the log and the results (spec §3.4):
/// what its last simulation left, kept until it was confirmed.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmedTick {
    pub tick: u32,
    /// Its gameplay events only (`WeaponFired`, `Damage`, `Kill`,
    /// `BonusPickup`, `Spawned`, `Message`, `GameOver`), in order.
    pub events: Vec<Event>,
    /// `World::scores` after it.
    pub scores: Vec<Score>,
    /// The deathmatch's time left after it (`Deathmatch::time_left_ms`).
    pub time_left_ms: Option<u64>,
    /// `World::result` after it.
    pub result: Option<MatchResult>,
    /// Every worm's id and position after it (for damage popups).
    pub positions: Vec<(i32, Vec2)>,
}

/// Turns the ticks a session simulates into records for the game (spec
/// §3.4: sounds and effects at once, the scoreboard and messages from
/// confirmed ticks only). Fed from [`crate::Driver::simulated`]; after each
/// `advance` the game takes the ticked records, then the confirmed ones
/// below `Session::confirmed`.
#[derive(Debug, Default)]
pub struct TickRecorder {
    ticked: Vec<TickedTick>,
    /// One candidate per simulated tick not taken yet, the last simulation's.
    pending: BTreeMap<u32, ConfirmedTick>,
    /// Ticks below it were taken as confirmed.
    taken: u32,
}

impl TickRecorder {
    /// Records the tick `sim` just simulated: a [`TickedTick`], and its
    /// [`ConfirmedTick`] candidate, which replaces an earlier simulation's.
    /// A tick already taken as confirmed (simulated again after a loaded
    /// state) gets no new candidate: each tick is confirmed once.
    pub fn simulated(&mut self, sim: &WorldSim, info: TickInfo) {
        let (world, events) = (sim.world(), sim.events());
        self.ticked.push(TickedTick {
            tick: info.tick,
            resim: info.resim,
            substeps: world.substeps_last_tick(),
            lost: events.lost_area(),
            events: events.as_slice().to_vec(),
        });
        if info.tick < self.taken {
            return;
        }
        let time_left_ms =
            world.mode_state().as_any().downcast_ref::<Deathmatch>().and_then(Deathmatch::time_left_ms);
        self.pending.insert(
            info.tick,
            ConfirmedTick {
                tick: info.tick,
                events: events.iter().filter(|e| gameplay(&e.kind)).copied().collect(),
                scores: world.scores().to_vec(),
                time_left_ms,
                result: world.result(),
                positions: world.worms().iter().map(|w| (w.id, w.pos)).collect(),
            },
        );
    }

    /// Every tick simulated since the last call, in simulation order.
    pub fn take_ticked(&mut self) -> Vec<TickedTick> {
        core::mem::take(&mut self.ticked)
    }

    /// The candidates of ticks below `confirmed` (`Session::confirmed`),
    /// in tick order; each tick comes out once.
    pub fn take_confirmed(&mut self, confirmed: u32) -> Vec<ConfirmedTick> {
        let rest = self.pending.split_off(&confirmed);
        let done = core::mem::replace(&mut self.pending, rest);
        self.taken = self.taken.max(confirmed);
        done.into_values().collect()
    }
}

/// The kinds a confirmed tick keeps: what the HUD, the log and the results read.
fn gameplay(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::WeaponFired { .. }
            | EventKind::Damage { .. }
            | EventKind::Kill { .. }
            | EventKind::BonusPickup { .. }
            | EventKind::Spawned { .. }
            | EventKind::Message(_)
            | EventKind::GameOver(_)
    )
}
