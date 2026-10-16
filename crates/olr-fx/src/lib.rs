//! OLX's cosmetic entities (`src/client/Entity.cpp`, presentation note §7):
//! blood, dirt particles, gibs, explosions, smoke, sparkles, spawn effects
//! and jetpack spray, spawned from the simulation's events and stepped once
//! per physics sub-step like OLX's `SimulateEntities`.
//!
//! Pure CPU: no GPU or OS handles. The entities draw from their own
//! generator (spec §19 E8), never from the simulation's, and only read the
//! world.

use olr_data::image::Rgba;
use olr_math::{GlibcRand, TimeDiff, Vec2};
use olr_scene::DrawList;
use olr_sim::{CosmeticKind, Event, EventKind, LostArea, TickEvents, World};
use olr_terrain::{ColorMap, PX_DIRT, Terrain};

use entity::Landed;
use pool::Pool;

pub use assets::{Atlas, FxAssets, FxImages, FxSprite, colorize_gibs, default_dirt_colour};
pub use entity::{Entity, Kind};

mod assets;
mod draw;
mod entity;
mod pool;

/// `MAX_ENTITIES` (`include/Entity.h:30`): new entities are dropped beyond it.
/// The pool's size with [`olr_sim::SimSettings::olx_quirks`] (the oracle's
/// scenarios, the render goldens), [`Fx::new`].
pub const MAX_ENTITIES: usize = 1024;

/// The pool's size in play ([`Fx::for_match`]): OLX's 1024 is a baseline,
/// not a limit worth keeping (the user's ruling, spec §19 E1 amended
/// 2026-10-13). A 16-bot chaos match peaks under 10 000 (the performance
/// envelope's effects axis). Allocated once; a hostile mod still finds a
/// hard cap, beyond which new entities are dropped as in OLX.
pub const PLAY_ENTITIES: usize = 16384;

// A frame full of play's entities fits the draw list's reserved room.
const _: () = assert!(PLAY_ENTITIES <= DrawList::ENTITIES_RESERVED);

/// The slots of play's pool that only a death's gibs and blood
/// ([`olr_sim::Event::death`]) may take, so a death always shows: other
/// entities stop at `PLAY_ENTITIES - DEATH_RESERVE`. One death is 157
/// entities (7 gibs, 150 blood), so the reserve holds six at once.
pub const DEATH_RESERVE: usize = PLAY_ENTITIES / 16;

/// What one [`World::tick`] reported, as [`Fx::step_tick`] replays it.
#[derive(Clone, Copy, Debug)]
pub struct FxTick<'a> {
    /// The tick's events, in order.
    pub events: &'a [Event],
    /// How many physics sub-steps the tick ran ([`World::substeps_last_tick`]).
    pub substeps: u8,
    /// `LX56PhysicsDT`, the sub-step's length.
    pub dt: TimeDiff,
    /// The area of terrain edits the tick dropped, if any.
    pub lost: Option<LostArea>,
    /// The world's terrain after the tick, to rebuild a lost area from.
    pub material: &'a Terrain,
}

impl<'a> FxTick<'a> {
    /// The tick `world` just ran, which filled `events`.
    pub fn new(world: &'a World, events: &'a TickEvents) -> FxTick<'a> {
        FxTick {
            events: events.as_slice(),
            substeps: world.substeps_last_tick(),
            dt: world.settings().lx56_physics_dt(),
            lost: events.lost_area(),
            material: world.terrain(),
        }
    }
}

/// The cosmetic entities of one match and their generator.
#[derive(Clone, Debug)]
pub struct Fx {
    pool: Pool,
    /// The slots only a death's entities take ([`DEATH_RESERVE`] in play).
    reserve: usize,
    /// A death's entities spawned and dropped (for want of room) so far.
    death_spawns: u64,
    death_drops: u64,
    /// The client's own `rand()` (spec §19 E8): gib spins and drips.
    rand: GlibcRand,
}

impl Fx {
    /// No entities, OLX's pool of [`MAX_ENTITIES`] with no reserve; `seed`
    /// seeds the entities' own `rand()`.
    pub fn new(seed: u32) -> Fx {
        Fx { pool: Pool::new(), reserve: 0, death_spawns: 0, death_drops: 0, rand: GlibcRand::new(seed) }
    }

    /// The pool for a match with these settings' `olx_quirks`: OLX's
    /// ([`Fx::new`]) with them, else play's [`PLAY_ENTITIES`] with its
    /// [`DEATH_RESERVE`]. Allocated here, once (about 1 MB in play).
    pub fn for_match(seed: u32, olx_quirks: bool) -> Fx {
        if olx_quirks { Fx::new(seed) } else { Fx::with_capacity(seed, PLAY_ENTITIES) }
    }

    /// A pool of `capacity` entities (at least 1) with play's rules: a
    /// sixteenth of it is the death reserve ([`DEATH_RESERVE`] at
    /// [`PLAY_ENTITIES`]). The performance envelope's knob (`olr-cli stress
    /// --fx-cap`, plan 8); games use [`Fx::for_match`].
    pub fn with_capacity(seed: u32, capacity: usize) -> Fx {
        let pool = Pool::with_capacity(capacity);
        let reserve = pool.capacity() / 16;
        Fx { pool, reserve, death_spawns: 0, death_drops: 0, rand: GlibcRand::new(seed) }
    }

    /// The pool's size.
    pub fn capacity(&self) -> usize {
        self.pool.capacity()
    }

    /// The slots only a death's entities take.
    pub fn death_reserve(&self) -> usize {
        self.reserve
    }

    /// A death's entities ([`olr_sim::Event::death`]) spawned so far.
    pub fn death_spawns(&self) -> u64 {
        self.death_spawns
    }

    /// A death's entities dropped so far because the pool was full: never
    /// in play unless deaths alone fill it.
    pub fn death_drops(&self) -> u64 {
        self.death_drops
    }

    /// How many entities other than a death's may be alive.
    fn ordinary_limit(&self) -> usize {
        self.pool.capacity() - self.reserve
    }

    /// Removes every entity (`ClearEntities`), for a new or restarted match.
    pub fn clear(&mut self) {
        self.pool.clear();
    }

    /// How many entities are alive (a count: no scan).
    pub fn len(&self) -> usize {
        self.pool.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The alive entities with their pool slots, in slot order (OLX's
    /// simulation and drawing order).
    pub fn entities(&self) -> impl Iterator<Item = (usize, &Entity)> {
        self.pool.iter()
    }

    /// `SpawnEntity` for a cosmetic event: dirt takes its colour from
    /// `color` at its probe, a gib draws its spin. Returns whether it took a
    /// slot (none for a beam, which `olr-scene` draws, or when all are used;
    /// a `death`'s entity, [`olr_sim::Event::death`], may use the death
    /// reserve, any other stops short of it).
    ///
    /// Dirt whose probe is no longer dirt takes the theme's default colour:
    /// OLX reads an explosion's colour before the bonuses it blows up
    /// explode, but the simulation reports those chained explosions (and
    /// their carves) before the particles (`docs/porting/STATUS.md`).
    ///
    /// OLX: src/client/Entity.cpp:161-205, src/client/CClient_Game.cpp:229-251
    pub fn spawn(
        &mut self,
        kind: CosmeticKind,
        pos: Vec2,
        vel: Vec2,
        death: bool,
        color: &ColorMap,
        assets: &FxAssets,
    ) -> bool {
        let colour = match kind {
            CosmeticKind::Dirt { probe: (x, y) } => {
                let (x, y) = (x as i64, y as i64);
                match color.material().flags(x, y) & PX_DIRT {
                    0 => assets.dirt_default(),
                    _ => color.color_at(x, y).unwrap_or(assets.dirt_default()),
                }
            }
            _ => Rgba::BLACK,
        };
        let Some(mut e) = Entity::new(kind, pos, vel, colour) else { return false };
        let limit = if death { self.pool.capacity() } else { self.ordinary_limit() };
        if self.pool.is_full_below(limit) {
            self.death_drops += u64::from(death);
            return false;
        }
        self.death_spawns += u64::from(death);
        if let Kind::Gib { .. } = e.kind {
            e.angl_vel = self.rand.random_num().abs() * 20.0;
            e.rotation = (self.rand.random_num().abs() * 3.0) as i32;
        }
        self.pool.add_below(e, limit).is_some()
    }

    /// One `SimulateEntities(dt)` pass over the entities in slot order,
    /// including those spawned during it into higher slots. Blood and gibs
    /// that land stain `color`; collisions read its material, which is in
    /// step with the edits replayed so far.
    ///
    /// OLX: src/client/Entity.cpp:320-468
    pub fn simulate(&mut self, dt: TimeDiff, color: &mut ColorMap, assets: &FxAssets) {
        let mut cur = self.pool.next(None);
        while let Some(i) = cur {
            let e = self.pool.get_mut(i);
            match entity::fall(e, dt, color.material()) {
                Landed::Nothing => {}
                Landed::Gone => self.pool.remove(i),
                Landed::Stain(x, y) => {
                    color.stain(x, y, e.colour);
                    self.pool.remove(i);
                }
                Landed::Stamp { x, y, worm, rotation } => {
                    let src = olr_terrain::Rect { x: (rotation as u32).wrapping_mul(4), y: 8, w: 4, h: 4 };
                    color.stain_image(x, y, assets.gibs(worm).1, src);
                    self.pool.remove(i);
                }
            }
            // OLX's second switch runs even for an entity the first removed.
            let e = self.pool.get_mut(i);
            if e.kind == Kind::BloodDropper {
                self.drip(i, dt);
            } else if entity::animate(e, dt) {
                self.pool.remove(i);
            }
            cur = self.pool.next(Some(i));
        }
    }

    /// A blood dropper's drop every 0.1 s (`fExtra` in whole milliseconds),
    /// from the entities' own `rand()`. `ent` still points at slot `i`
    /// afterwards, whatever took it, as in OLX.
    ///
    /// OLX: src/client/Entity.cpp:453-462
    fn drip(&mut self, i: usize, dt: TimeDiff) {
        if self.pool.get(i).extra > TimeDiff(100) {
            const RED: [u8; 2] = [128, 200];
            let col = self.rand.random_int(1);
            let pos = self.pool.get(i).pos;
            let vel = self.rand.random_vec();
            let red = RED[col.clamp(0, 1) as usize];
            if let Some(blood) = Entity::new(CosmeticKind::Blood { red }, pos, vel, Rgba::BLACK) {
                let limit = self.ordinary_limit();
                self.pool.add_below(blood, limit);
            }
            self.pool.get_mut(i).extra = TimeDiff(0);
        }
        let e = self.pool.get_mut(i);
        e.extra = TimeDiff(e.extra.0.wrapping_add(dt.0));
    }

    /// Replays one tick: walks its events in order, applying terrain edits
    /// to `color` and spawning cosmetics, and runs entity pass `k` right
    /// before the first event of a later sub-step (spec §19), then the
    /// passes left; finally rebuilds the area of edits the tick dropped.
    /// This is the only place the colour map follows the simulation's
    /// edits: the caller must not also replay them (`olr_content::apply_edits`).
    pub fn step_tick(&mut self, tick: &FxTick, color: &mut ColorMap, assets: &FxAssets) {
        let mut passes = 0u8;
        for ev in tick.events {
            while passes < ev.substep.min(tick.substeps) {
                self.simulate(tick.dt, color, assets);
                passes += 1;
            }
            match &ev.kind {
                EventKind::SpawnCosmetic { kind, pos, vel } => {
                    self.spawn(*kind, *pos, *vel, ev.death, color, assets);
                }
                kind => {
                    if let Some(edit) = olr_content::color_edit(kind) {
                        color.apply(edit);
                    }
                }
            }
        }
        while passes < tick.substeps {
            self.simulate(tick.dt, color, assets);
            passes += 1;
        }
        if let Some(area) = tick.lost {
            color.rebuild_area(olr_content::lost_area(area), tick.material);
        }
    }

    /// Appends the entities to [`olr_scene::Layer::Entities`] in slot order,
    /// as OLX draws them in the view `out.view`.
    pub fn draw(&self, assets: &FxAssets, out: &mut DrawList) {
        for (_, e) in self.pool.iter() {
            draw::entity(e, assets, out);
        }
    }
}
