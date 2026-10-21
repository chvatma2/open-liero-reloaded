//! Turns a simulation [`World`] into a [`DrawList`] (spec §8.2): OLX's
//! camera, interpolation between the last two ticks, and OLX's sprite math
//! for worms, ropes, projectiles and bonuses (presentation note §1, §3-§6,
//! §11). Pure CPU: no GPU or OS handles, so it is unit-testable.
//!
//! Per tick, [`Scene::after_tick`] reads the new state and events; per
//! drawn frame, [`Scene::build`] moves the camera and fills the draw list.
//! The caller then appends cosmetic entities (`olr-fx`) to
//! [`Layer::Entities`] and hands the list to the renderer.
//!
//! **The simulation never sees presentation:** the scene only reads the
//! world, and its randomness (shake, laser dots) comes from its own
//! generators.

mod assets;
mod camera;
mod draw_list;
mod marks;
mod projectile;
mod snapshot;
mod worm;

pub use assets::{AtlasImage, SceneAssets, WormLook};
pub use camera::{Camera, FOV_HEIGHT, SHAKE_MS, Viewport};
pub use draw_list::{
    AtlasId, DrawList, Floor, Item, Layer, Line, Occluder, Rect, Shade, Sprite, View, perform_line,
};
pub use marks::Marks;
pub use projectile::{draw_bonus, draw_projectile, frame_x, framestep};
pub use snapshot::{Blend, BonusSnap, Interpolation, ProjBlend, ProjSnap, Snapshot, WormSnap, worm_blends};
pub use worm::{
    BODY_OFFSET, CROSSHAIR_DISTANCE, WormExtras, WormFrame, crosshair_offset, draw_worm, health_segments,
    muzzle_frame, skin_frame, skin_frame_index,
};

use olr_math::{AbsTime, Vec2};
use olr_sim::{TickEvents, World};

/// How a scene draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneOptions {
    /// Blend the last two ticks (the default), or draw the latest state at
    /// OLX's whole pixels (spec §19 E2).
    pub interpolation: Interpolation,
    /// OLX's `Game.ShowWormHealth` (off by default; key `h`): health bars
    /// over worms that are not local humans.
    pub show_health_bars: bool,
}

/// How a match with a look shades its items (look system, plan 15a R13):
/// projectiles and bonuses never fall below `floor`, a projectile whose look
/// glows shows at least that; worms take the worm floor (plan 15b). A scene without rules draws every item lit, as
/// always.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShadeRules {
    /// Self-light of projectiles and bonuses, percent.
    pub floor: u8,
    /// Each projectile's glow (by `SimMod` index), percent.
    pub projectile_glow: Vec<Option<u8>>,
}

impl ShadeRules {
    fn floor(&self) -> Shade {
        Shade { self_light: self.floor, ..Shade::LIT }
    }

    fn projectile(&self, proj: usize) -> Shade {
        let glow = self.projectile_glow.get(proj).copied().flatten().unwrap_or(0);
        Shade { self_light: self.floor.max(glow).min(100), ..Shade::LIT }
    }
}

/// A projectile as the last [`Scene::build`] placed it (a lit match's
/// lights follow it), view or not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawnProjectile {
    pub slot: usize,
    pub serial: u32,
    /// Its `SimMod` index.
    pub proj: usize,
    pub pos: Vec2,
    pub spawn_time: AbsTime,
}

/// A projectile that ended in a tick (its slot empty or holding another
/// serial after it), where it was at the tick before: a lit match's end
/// flash. Recorded per tick, so it never depends on the frame rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EndedProjectile {
    pub slot: usize,
    pub serial: u32,
    /// Its `SimMod` index.
    pub proj: usize,
    pub spawn_time: AbsTime,
    pub pos: Vec2,
    /// The tick it ended in.
    pub at: AbsTime,
}

/// A living worm as a lit build placed it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawnWorm {
    pub id: i32,
    /// Its index in `World::worms` (and `SceneAssets::worm`).
    pub index: usize,
    /// Its drawn position.
    pub pos: Vec2,
    /// Its occluder id: index + 1 (plan 15b).
    pub occluder: u8,
    /// The skin's sheet frame drawn (issue #12: its skin light's frame).
    pub frame: u32,
}

/// A projectile weapon's shot in a tick: a lit match's muzzle light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shot {
    pub worm: i32,
    /// The weapon (its `SimMod` index).
    pub weapon: usize,
    pub at: AbsTime,
}

/// The most [`EndedProjectile`]s and [`Shot`]s kept between two
/// [`Scene::clear_look_events`] (a host drawing no frame drops the rest).
pub const LOOK_EVENTS: usize = 8192;

/// One viewport's presentation state: its camera, the previous tick's
/// snapshot, the last tick's marks (muzzle flashes, laser sights, beams),
/// and the options.
#[derive(Clone, Debug)]
pub struct Scene {
    pub camera: Camera,
    pub options: SceneOptions,
    prev: Snapshot,
    cur: Snapshot,
    marks: Marks,
    /// A lit match's rules; `None` draws as always.
    shades: Option<ShadeRules>,
    /// With rules, what the last build placed (reused, grown once).
    drawn: Vec<DrawnProjectile>,
    worms: Vec<DrawnWorm>,
    /// With rules, the ticks' ended projectiles and shots since the last
    /// [`Scene::clear_look_events`].
    ended: Vec<EndedProjectile>,
    shots: Vec<Shot>,
}

impl Scene {
    /// `seed` seeds the laser sights' dots (the camera has its own seed).
    pub fn new(camera: Camera, options: SceneOptions, seed: u32) -> Scene {
        Scene {
            camera,
            options,
            prev: Snapshot::new(),
            cur: Snapshot::new(),
            marks: Marks::new(seed),
            shades: None,
            drawn: Vec::new(),
            worms: Vec::new(),
            ended: Vec::new(),
            shots: Vec::new(),
        }
    }

    /// A lit match's shade rules (`None`: every item lit, as always). With
    /// rules, each build also records where its projectiles and worms are,
    /// and each tick the projectiles that ended and the shots.
    pub fn set_shades(&mut self, shades: Option<ShadeRules>) {
        self.shades = shades;
        self.drawn.clear();
        self.worms.clear();
        self.clear_look_events();
    }

    pub fn shades(&self) -> Option<&ShadeRules> {
        self.shades.as_ref()
    }

    /// Every projectile the last build placed, in slot order (only with
    /// shade rules).
    pub fn drawn_projectiles(&self) -> &[DrawnProjectile] {
        &self.drawn
    }

    /// The alive worms the last build placed, in worm order (only with
    /// shade rules).
    pub fn drawn_worms(&self) -> &[DrawnWorm] {
        &self.worms
    }

    /// The projectiles that ended in the ticks since the last
    /// [`Scene::clear_look_events`], in tick then slot order (only with
    /// shade rules). A re-simulated tick reports its ends again.
    pub fn ended_projectiles(&self) -> &[EndedProjectile] {
        &self.ended
    }

    /// The projectile weapons' shots in those ticks, in tick then event order.
    pub fn shots(&self) -> &[Shot] {
        &self.shots
    }

    /// Forgets the ended projectiles and shots (the presentation took them).
    pub fn clear_look_events(&mut self) {
        self.ended.clear();
        self.shots.clear();
    }

    /// With shade rules, records a tick's ended projectiles (in the
    /// previous snapshot, not in the new one) and shots. The first tick
    /// re-simulated after a rollback follows a snapshot of the old
    /// timeline (a later time): no end is read from that pair.
    fn record_look_events(&mut self, world: &World, events: &TickEvents) {
        if self.shades.is_none() {
            return;
        }
        let at = world.time();
        let follows = self.prev.time().is_some_and(|t| t < at);
        let now = self.cur.projectiles();
        let mut i = 0;
        for q in self.prev.projectiles().iter().filter(|_| follows) {
            while now.get(i).is_some_and(|n| n.slot < q.slot) {
                i += 1;
            }
            let still = now.get(i).is_some_and(|n| (n.slot, n.serial) == (q.slot, q.serial));
            if !still && self.ended.len() < LOOK_EVENTS {
                let (slot, serial, proj, spawn_time, pos) =
                    (q.slot as usize, q.serial, q.proj, q.spawn_time, q.pos);
                self.ended.push(EndedProjectile { slot, serial, proj, spawn_time, pos, at });
            }
        }
        let m = world.sim_mod();
        for e in events {
            if let olr_sim::EventKind::WeaponFired { worm, weapon } = e.kind
                && m.weapon(weapon).is_some_and(|w| w.kind == olr_sim::WeaponType::Projectile)
                && self.shots.len() < LOOK_EVENTS
            {
                self.shots.push(Shot { worm, weapon: usize::try_from(weapon).unwrap_or(0), at });
            }
        }
    }

    /// Reads the world and the events after a [`World::tick`]: the state
    /// before it becomes the previous snapshot, explosions shake the
    /// camera, and the tick's marks replace the last ones. Call it also
    /// once before the first tick (with an empty buffer), so the first
    /// tick blends from the starting state.
    pub fn after_tick(&mut self, world: &World, events: &TickEvents) {
        core::mem::swap(&mut self.prev, &mut self.cur);
        self.cur.capture_into(world);
        self.camera.on_events(events, world.time());
        self.marks.observe(world, events);
        self.record_look_events(world, events);
    }

    /// [`Scene::after_tick`] for a tick simulated again (a rollback in
    /// online play, spec §3.4): the history and the marks move on as for a
    /// new tick, so the frames blend between states of the corrected
    /// timeline, but explosions do not shake the camera again (it shook
    /// when the tick was first simulated; one first seen in the
    /// re-simulation shakes through [`Camera::shake_for`]).
    pub fn after_tick_resimulated(&mut self, world: &World, events: &TickEvents) {
        core::mem::swap(&mut self.prev, &mut self.cur);
        self.cur.capture_into(world);
        self.marks.observe(world, events);
        self.record_look_events(world, events);
    }

    /// Forgets the history and stops the camera's shake, for a new or
    /// restarted world.
    pub fn reset(&mut self) {
        self.camera.stop_shake();
        self.prev.clear();
        self.cur.clear();
        self.marks.clear();
        self.drawn.clear();
        self.worms.clear();
        self.clear_look_events();
    }

    /// A new or restarted world: [`Scene::reset`], then `world` as the
    /// starting state the first tick blends from (what [`Scene::after_tick`]
    /// with an empty buffer does, without needing one).
    pub fn start(&mut self, world: &World) {
        self.reset();
        self.cur.capture_into(world);
    }

    /// The state before the last tick, which [`Scene::build`] blends from.
    pub fn prev(&self) -> &Snapshot {
        &self.prev
    }

    pub fn marks(&self) -> &Marks {
        &self.marks
    }

    /// One drawn frame at `alpha` between the previous tick (0) and the
    /// world (1): `CViewport::Process` (follow, shake, clamp), then the
    /// layers in OLX's order (presentation note §11.2). Clears `out` first
    /// and reuses its buffers.
    ///
    /// OLX: src/gusanos/viewport.cpp:105-253, src/client/CClient_Draw.cpp:1001-1015
    pub fn build(&mut self, world: &World, alpha: f32, assets: &SceneAssets, out: &mut DrawList) {
        let blend = Blend::new(&self.prev, alpha, self.options.interpolation);
        let worms = world.worms();
        let target = worms.iter().position(|w| w.id == self.camera.target());
        let follow = target.map(|i| blend.worm(i, &worms[i]));
        let map = (world.terrain().width(), world.terrain().height());
        self.camera.process(follow, blend.exact(), map, world.time());
        let view = self.camera.view();
        out.clear();
        out.view = view;
        out.map = map;
        // Linear: marks start at the worms as drawn, not where the sub-step left them.
        let drawn = |id: i32| {
            let i = worms.iter().position(|w| w.id == id)?;
            (!blend.exact()).then(|| blend.worm(i, &worms[i]))
        };
        let lit = self.shades.is_some();
        self.marks.draw(assets, view, drawn, lit, out);
        self.drawn.clear();
        self.worms.clear();
        let m = world.sim_mod();
        let mut projectiles = blend.projectiles();
        for (slot, p) in world.projectiles() {
            if let Some(def) = m.projectiles().get(p.proj) {
                let at = blend.snap(projectiles.pos(slot, p));
                let shade = self.shades.as_ref().map_or(Shade::LIT, |r| r.projectile(p.proj));
                draw_projectile(def, p, at, world.rot_bumped(p.proj), assets, view, shade, out);
                if lit {
                    let (serial, spawn_time) = (p.serial(), p.spawn_time);
                    self.drawn.push(DrawnProjectile { slot, serial, proj: p.proj, pos: at, spawn_time });
                }
            }
        }
        let floor = self.shades.as_ref().map_or(Shade::LIT, ShadeRules::floor);
        for (slot, b) in world.bonuses().iter().enumerate().filter(|(_, b)| b.used) {
            draw_bonus(b, blend.snap(blend.bonus(slot, b)), assets, view, floor, out);
        }
        let frame = WormFrame { world, assets, view, show_health_bars: self.options.show_health_bars, lit };
        for (i, w) in worms.iter().enumerate().filter(|(_, w)| w.alive) {
            let p = blend.snap(blend.worm(i, w));
            if lit {
                // Plan 15b: living worms block light, whatever the view.
                let body = worm::body(&frame, i, w, p);
                let at = worm::body_frame(assets, i, w);
                self.worms.push(DrawnWorm { id: w.id, index: i, pos: p, occluder: body.worm, frame: at });
                out.occluders.push(body);
            }
            let extras = WormExtras {
                muzzle: self.marks.muzzle(w.id),
                target: self.marks.target(w.id),
                hook: hook(&blend, worms, i, w),
            };
            draw_worm(&frame, i, w, p, &extras, out);
        }
    }
}

/// Worm `i`'s drawn hook: on the worm it is attached to, as that worm is
/// drawn (OLX draws it at that worm's current position, which keeps it on
/// the worm; interpolated, the worm's current position is not where it is
/// drawn), else the blended hook.
///
/// OLX: src/client/CNinjaRope.cpp:139-146
fn hook(blend: &Blend, worms: &[olr_sim::Worm], i: usize, w: &olr_sim::Worm) -> olr_math::Vec2 {
    let on = w.rope.attached && w.rope.player_attached >= 0;
    match worms.iter().position(|o| on && o.id == w.rope.player_attached) {
        Some(j) => blend.worm(j, &worms[j]),
        None => blend.hook(i, w),
    }
}
