//! Turns a simulation [`World`] into a [`DrawList`] (spec §8.2): OLX's
//! camera, interpolation between the last two ticks, and OLX's sprite math
//! for worms, ropes, projectiles and bonuses (presentation note §1, §3-§6,
//! §11). Pure CPU: no GPU or OS handles, so it is unit-testable.
//!
//! Per tick, [`Scene::after_tick`] reads the new state and events; per
//! drawn frame, [`Scene::build`] moves the camera and fills the draw list.
//! The caller then appends cosmetic entities (`rl-fx`) to
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
pub use draw_list::{AtlasId, DrawList, Item, Layer, Line, Rect, Sprite, View, perform_line};
pub use marks::Marks;
pub use projectile::{draw_bonus, draw_projectile, frame_x, framestep};
pub use snapshot::{Blend, Interpolation, ProjBlend, ProjSnap, Snapshot, WormSnap, worm_blends};
pub use worm::{
    CROSSHAIR_DISTANCE, FRAME_WIDTH, SKIN_HEIGHT, SKIN_WIDTH, WormExtras, WormFrame, crosshair_offset,
    draw_worm, health_segments, muzzle_frame, skin_frame,
};

use rl_sim::{TickEvents, World};

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
}

impl Scene {
    /// `seed` seeds the laser sights' dots (the camera has its own seed).
    pub fn new(camera: Camera, options: SceneOptions, seed: u32) -> Scene {
        Scene { camera, options, prev: Snapshot::new(), cur: Snapshot::new(), marks: Marks::new(seed) }
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
    }

    /// Forgets the history, for a new or restarted world.
    pub fn reset(&mut self) {
        self.prev.clear();
        self.cur.clear();
        self.marks.clear();
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
        self.marks.draw(assets, view, out);
        let m = world.sim_mod();
        let mut projectiles = blend.projectiles();
        for (slot, p) in world.projectiles() {
            if let Some(def) = m.projectiles().get(p.proj) {
                let at = blend.snap(projectiles.pos(slot, p));
                draw_projectile(def, p, at, world.rot_bumped(p.proj), assets, view, out);
            }
        }
        for b in world.bonuses().iter().filter(|b| b.used) {
            draw_bonus(b, blend.snap(b.pos), assets, view, out);
        }
        let frame = WormFrame { world, assets, view, show_health_bars: self.options.show_health_bars };
        for (i, w) in worms.iter().enumerate().filter(|(_, w)| w.alive) {
            let p = blend.snap(blend.worm(i, w));
            let extras = WormExtras {
                muzzle: self.marks.muzzle(w.id),
                target: self.marks.target(w.id),
                hook: blend.hook(i, w),
            };
            draw_worm(&frame, i, w, p, &extras, out);
        }
    }
}
