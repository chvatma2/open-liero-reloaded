//! One viewport's presentation of a match (spec §8.2, §19 E1-E8): the
//! scene (camera, interpolation, sprites), the cosmetic entities, the colour
//! terrain and every image of the atlas, wired in the frame loop's order
//! once, for every mode that draws a world (the sandbox, matches, replays,
//! `olr-cli render`).
//!
//! - [`Presentation::new`] registers the core and mod images and starts
//!   the scene from the world; [`Presentation::add_worm`] registers a
//!   worm's skin and gibs (in `World::worms` order) and marks the atlas
//!   for upload, so a worm added mid-match is uploaded with the next frame.
//! - [`Presentation::on_tick`] after every `World::tick`: the entities
//!   replay the tick's terrain edits on the colour map (`olr_fx::Fx::step_tick`;
//!   nothing else may apply them), then the scene reads the tick.
//! - [`Presentation::build`] per drawn frame: the scene, then the entities.
//! - [`Presentation::upload`] right before the frame is drawn: the atlas
//!   when an image was added, the whole terrain when the map is new, else
//!   its dirty rectangles (merged across the ticks since the last frame).
//!
//! - Online play (spec §3.4) draws the newest predicted tick and replays
//!   rolled-back ticks: [`net::NetPresenter`] plays each sound and effect
//!   once across re-simulations and repairs the colour terrain after them.
//!
//! Pure CPU apart from [`Presentation::upload`], which takes the renderer:
//! it holds no GPU or OS handle, so it is tested headless. It only reads
//! the world (the simulation never sees presentation).

pub mod net;

use olr_content::{CorePack, LoadedLevel, LoadedMod, LoadedSkin, LoadedTheme};
use olr_data::image::Rgba;
use olr_fx::{Fx, FxAssets, FxImages, FxTick, default_dirt_colour};
use olr_render::Renderer;
use olr_scene::{Camera, DrawList, Scene, SceneAssets, SceneOptions, Viewport};
use olr_sim::{TickEvents, World};
use olr_terrain::ColorMap;

/// OLX's default worm colours for the first worms (`Profiles.cpp`): red,
/// blue, green, yellow.
pub const WORM_COLOURS: [Rgba; 4] =
    [Rgba::rgb(255, 0, 0), Rgba::rgb(0, 0, 255), Rgba::rgb(0, 255, 0), Rgba::rgb(255, 255, 0)];

/// Worm `i`'s default colour: [`WORM_COLOURS`] in turn.
pub fn default_colour(i: usize) -> Rgba {
    WORM_COLOURS[i % WORM_COLOURS.len()]
}

/// What a presentation is made from.
#[derive(Clone, Copy)]
pub struct PresentSetup<'a> {
    pub core: &'a CorePack,
    pub m: &'a LoadedMod,
    pub level: &'a LoadedLevel,
    pub theme: &'a LoadedTheme,
    pub viewport: Viewport,
    pub options: SceneOptions,
    /// Seeds the camera's shake (`seed`), the laser sights' dots
    /// (`seed + 1`) and the entities (`seed + 2`).
    pub seed: u32,
}

/// One viewport's scene, cosmetic entities, colour terrain and atlas
/// images, and what the renderer still needs of them.
pub struct Presentation {
    pub scene: Scene,
    pub fx: Fx,
    color: ColorMap,
    assets: SceneAssets,
    fx_assets: FxAssets,
    seed: u32,
    atlas_dirty: bool,
    terrain_new: bool,
}

impl Presentation {
    /// The presentation of `world`, at its start (the first tick blends
    /// from it), with the core and mod images registered and no worm yet:
    /// call [`Presentation::add_worm`] for each.
    pub fn new(s: &PresentSetup, world: &World) -> Presentation {
        let mut assets = SceneAssets::new(s.core, s.m);
        let fx_assets = FxAssets::new(FxImages::from_core(s.core), default_dirt_colour(s.theme), &mut assets);
        let camera = Camera::new(s.viewport, 0, s.seed);
        let mut scene = Scene::new(camera, s.options, s.seed.wrapping_add(1));
        scene.start(world);
        Presentation {
            scene,
            fx: Fx::for_match(s.seed.wrapping_add(2), world.settings().olx_quirks),
            color: s.level.color_map(s.theme),
            assets,
            fx_assets,
            seed: s.seed,
            atlas_dirty: true,
            terrain_new: true,
        }
    }

    /// A cosmetic-entity pool of `capacity` instead of the match's
    /// (`Fx::for_match`: play's 16384, or OLX's 1024 with `olx_quirks`), with
    /// play's death reserve, a sixteenth (bench only: the performance
    /// envelope's effects sweep, plan 8). Call it before the first
    /// [`Presentation::on_tick`]: the entities so far go.
    pub fn set_fx_capacity(&mut self, capacity: usize) {
        self.fx = Fx::with_capacity(self.seed.wrapping_add(2), capacity);
    }

    /// Registers the next worm's look (its index in `World::worms`): the
    /// colorized skin and gibs. Returns the index. The atlas is uploaded
    /// again with the next [`Presentation::upload`].
    pub fn add_worm(&mut self, skin: &LoadedSkin, colour: Rgba, local: bool, human: bool) -> usize {
        let i = self.assets.add_worm(skin, colour, local, human);
        let j = self.fx_assets.add_worm(colour, &mut self.assets);
        debug_assert_eq!(i, j, "the scene and the entities number worms alike");
        self.atlas_dirty = true;
        i
    }

    /// After every `world.tick(.., events)`: the entities (which replay the
    /// tick's terrain edits on the colour map, and rebuild an area the tick
    /// lost), then the scene (history, shake, marks).
    pub fn on_tick(&mut self, world: &World, events: &TickEvents) {
        self.step_fx(&FxTick::new(world, events));
        self.scene.after_tick(world, events);
    }

    /// The entities' and the colour terrain's part of
    /// [`Presentation::on_tick`], for a tick described by hand: online
    /// play's [`net::NetPresenter`] replays recorded ticks after the
    /// advance, filtered when they are re-simulated.
    pub fn step_fx(&mut self, tick: &FxTick) {
        self.fx.step_tick(tick, &mut self.color, &self.fx_assets);
    }

    /// One drawn frame at `alpha` between the last two ticks (0 the
    /// previous, 1 the latest): the scene, then the entities into `list`.
    pub fn build(&mut self, world: &World, alpha: f32, list: &mut DrawList) {
        self.scene.build(world, alpha, &self.assets, list);
        self.fx.draw(&self.fx_assets, list);
    }

    /// A new world on the same level with the same worms (a restart): the
    /// scene starts from it, the entities go, the colour terrain is the
    /// level's again (uploaded whole with the next frame). The atlas stays.
    pub fn restart(&mut self, world: &World, level: &LoadedLevel, theme: &LoadedTheme) {
        self.scene.start(world);
        self.fx.clear();
        self.color = level.color_map(theme);
        self.terrain_new = true;
    }

    /// The world's state was replaced by a loaded one (online play's
    /// repair or a late spectator, `olr_netcode::Event::StateLoaded`): the
    /// scene starts from it and the entities go. The colour terrain is
    /// repainted by the whole-map restored area the load reports
    /// ([`net::NetPresenter::apply`]); call [`net::NetPresenter::reset`] too.
    pub fn state_loaded(&mut self, world: &World) {
        self.scene.start(world);
        self.fx.clear();
    }

    /// The camera follows worm `worm` (by id) from the next frame: the
    /// local player's, or the one a spectator watches. A new presentation
    /// follows worm 0.
    pub fn follow(&mut self, worm: i32) {
        self.scene.camera.set_target(worm);
    }

    /// The worm the camera follows.
    pub fn followed(&self) -> i32 {
        self.scene.camera.target()
    }

    /// A view of another shape: a new camera on the same target.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        if self.scene.camera.viewport() != viewport {
            let target = self.scene.camera.target();
            self.scene.camera = Camera::new(viewport, target, self.seed);
        }
    }

    /// The colour terrain, as the edits replayed so far left it.
    pub fn color(&self) -> &ColorMap {
        &self.color
    }

    /// The colour terrain to change by hand (tools and tests). An edit the
    /// world did not make is drift ([`Presentation::terrain_drift`]).
    pub fn color_mut(&mut self) -> &mut ColorMap {
        &mut self.color
    }

    /// Every image the atlas holds (scene, worms, entities).
    pub fn assets(&self) -> &SceneAssets {
        &self.assets
    }

    pub fn fx_assets(&self) -> &FxAssets {
        &self.fx_assets
    }

    /// Whether the colour terrain's material differs from the world's: an
    /// edit of the world without its `TerrainEdit` event (or one replayed
    /// twice). It compares the whole grid, so callers check it now and
    /// then, in debug builds.
    pub fn terrain_drift(&self, world: &World) -> bool {
        self.color.material() != world.terrain()
    }

    /// Whether an image was added since the atlas was last uploaded, and
    /// forgets it ([`Presentation::upload`] does this itself).
    pub fn take_atlas_dirty(&mut self) -> bool {
        core::mem::take(&mut self.atlas_dirty)
    }

    /// A new renderer (a new window or device): the next
    /// [`Presentation::upload`] uploads everything.
    pub fn invalidate_gpu(&mut self) {
        self.atlas_dirty = true;
        self.terrain_new = true;
    }

    /// Brings `renderer` up to date, right before a frame is drawn: the
    /// atlas when an image was added, the whole colour map when it is new
    /// (a new world or renderer), else the rectangles the ticks since the
    /// last upload changed.
    pub fn upload(&mut self, renderer: &mut Renderer) {
        if self.take_atlas_dirty() {
            renderer.upload_atlas(&self.assets);
        }
        if core::mem::take(&mut self.terrain_new) {
            self.color.take_dirty().for_each(drop);
            renderer.upload_terrain(&self.color);
        } else {
            renderer.update_terrain(&mut self.color);
        }
    }
}
