//! OLX's cosmetic entities (`src/client/Entity.cpp`, presentation note §7):
//! blood, dirt particles, gibs, explosions, smoke, sparkles, spawn effects
//! and jetpack spray, spawned from the simulation's events and stepped once
//! per physics sub-step like OLX's `SimulateEntities`.
//!
//! Pure CPU: no GPU or OS handles. The entities draw from their own
//! generator (spec §19 E8), never from the simulation's, and only read the
//! world.

use rl_data::image::Rgba;
use rl_math::{GlibcRand, TimeDiff, Vec2};
use rl_scene::DrawList;
use rl_sim::{CosmeticKind, Event, EventKind, LostArea, TickEvents, World};
use rl_terrain::{ColorMap, PX_DIRT, Terrain};

use entity::Landed;
use pool::Pool;

pub use assets::{Atlas, FxAssets, FxImages, FxSprite, colorize_gibs, default_dirt_colour};
pub use entity::{Entity, Kind};

mod assets;
mod draw;
mod entity;
mod pool;

/// `MAX_ENTITIES` (`include/Entity.h:30`): new entities are dropped beyond it.
pub const MAX_ENTITIES: usize = 1024;

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
    /// The client's own `rand()` (spec §19 E8): gib spins and drips.
    rand: GlibcRand,
}

impl Fx {
    /// No entities; `seed` seeds the entities' own `rand()`.
    pub fn new(seed: u32) -> Fx {
        Fx { pool: Pool::new(), rand: GlibcRand::new(seed) }
    }

    /// Removes every entity (`ClearEntities`), for a new or restarted match.
    pub fn clear(&mut self) {
        self.pool.clear();
    }

    /// How many entities are alive.
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
    /// slot (none for a beam, which `rl-scene` draws, or when all are used).
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
        if self.pool.is_full() {
            return false;
        }
        if let Kind::Gib { .. } = e.kind {
            e.angl_vel = self.rand.random_num().abs() * 20.0;
            e.rotation = (self.rand.random_num().abs() * 3.0) as i32;
        }
        self.pool.add(e).is_some()
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
                    let src = rl_terrain::Rect { x: (rotation as u32).wrapping_mul(4), y: 8, w: 4, h: 4 };
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
                self.pool.add(blood);
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
    /// edits: the caller must not also replay them (`rl_content::apply_edits`).
    pub fn step_tick(&mut self, tick: &FxTick, color: &mut ColorMap, assets: &FxAssets) {
        let mut passes = 0u8;
        for ev in tick.events {
            while passes < ev.substep.min(tick.substeps) {
                self.simulate(tick.dt, color, assets);
                passes += 1;
            }
            match &ev.kind {
                EventKind::SpawnCosmetic { kind, pos, vel } => {
                    self.spawn(*kind, *pos, *vel, color, assets);
                }
                kind => {
                    if let Some(edit) = rl_content::color_edit(kind) {
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
            color.rebuild_area(rl_content::lost_area(area), tick.material);
        }
    }

    /// Appends the entities to [`rl_scene::Layer::Entities`] in slot order,
    /// as OLX draws them in the view `out.view`.
    pub fn draw(&self, assets: &FxAssets, out: &mut DrawList) {
        for (_, e) in self.pool.iter() {
            draw::entity(e, assets, out);
        }
    }
}
