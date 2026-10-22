//! The presentation of a match (spec §8.2, §19 E1-E8): the scene (a
//! camera per view, interpolation, sprites), the cosmetic entities, the colour
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
//! - [`Presentation::build`] per drawn frame: the scene, then the entities
//!   ([`Presentation::build_views`]: a draw list per view, split screen).
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

mod look;
pub mod net;

pub use look::{BlastKey, FlashKey};

use olr_content::{CorePack, LoadedLevel, LoadedMod, LoadedSkin, LoadedTheme};
use olr_data::image::Rgba;
use olr_fx::{Fx, FxAssets, FxImages, FxTick, default_dirt_colour};
use olr_look::{LevelLook, LookFrame, ModLook};
use olr_render::{HeatView, MAX_HEAT_DEPOSITS, MAX_HEAT_STEPS, Renderer};
use olr_scene::{Camera, DrawList, Scene, SceneAssets, SceneOptions, ShadeRules, Viewport};
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

/// View `v`'s camera seed: the first view's is the setup's `seed`.
fn view_seed(seed: u32, v: usize) -> u32 {
    seed.wrapping_add((v as u32).wrapping_mul(0x9E37_79B9))
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
    /// Seeds the first camera's shake (`seed`; another view's its own,
    /// from it), the laser sights' dots
    /// (`seed + 1`) and the entities (`seed + 2`).
    pub seed: u32,
}

/// A match's scene (a camera per view), cosmetic entities, colour terrain
/// and atlas images, and what the renderer still needs of them.
pub struct Presentation {
    pub scene: Scene,
    pub fx: Fx,
    color: ColorMap,
    assets: SceneAssets,
    fx_assets: FxAssets,
    seed: u32,
    atlas_dirty: bool,
    terrain_new: bool,
    /// The level's look, when it has one.
    level_look: Option<LevelLook>,
    /// The level's distant layers (issue #11), uploaded with the terrain.
    distant: Vec<olr_data::level::DistantLayer>,
    /// The level's surface maps (issue #37), given to the renderer with
    /// its look's materials before each whole terrain upload (issue #39:
    /// its heat).
    surface: Option<olr_look::SurfaceMaps>,
    /// Each distant layer's surface material value (issue #41), given to
    /// the renderer with the surface maps.
    distant_surface: Vec<u8>,
    /// The level or the mod has a look: the renderer's look passes draw it.
    lit: bool,
    /// The frame's look, rebuilt by every lit [`Presentation::build`].
    look: LookFrame,
    /// The mod's look (projectile and weapon lights).
    mod_look: ModLook,
    /// The dynamic lights' state (end flashes) and the heat waiting.
    dynamic: look::Dynamic,
    /// Issue #40: the level's materials heat (its look has one that does,
    /// and surface maps): explosions queue heat.
    heats: bool,
    /// The match time (ms) the last built frame showed, and the time the
    /// renderer's heat has been advanced to.
    heat_now: u64,
    heat_clock: u64,
    /// The renderer's heat clears with the next upload (a loaded state or
    /// a seek).
    heat_reset: bool,
}

/// Whether `level`'s surface materials heat (issue #40): a material that
/// does in its look, and maps; or (issue #41) a distant layer's material
/// that does.
fn heats(level: &LoadedLevel) -> bool {
    let Some(look) = &level.look else { return false };
    let heating = |m: Option<&olr_look::SurfaceMaterial>| m.is_some_and(|m| m.heats().is_some());
    (level.surface.is_some() && look.materials.iter().any(|m| heating(Some(m))))
        || level.distant_surface.iter().any(|&v| heating(look.material(v)))
}

/// The most heat catch-up runs an upload makes: a box lives at most 15 000
/// steps, 60 runs of [`MAX_HEAT_STEPS`], so the renderer is caught up long
/// before.
const HEAT_RUNS: u32 = 64;

impl Presentation {
    /// The presentation of `world`, at its start (the first tick blends
    /// from it), with the core and mod images registered and no worm yet:
    /// call [`Presentation::add_worm`] for each.
    pub fn new(s: &PresentSetup, world: &World) -> Presentation {
        let mut assets = SceneAssets::new(s.core, s.m);
        let fx_assets = FxAssets::new(FxImages::from_core(s.core), default_dirt_colour(s.theme), &mut assets);
        let camera = Camera::new(s.viewport, 0, s.seed);
        let mut scene = Scene::new(camera, s.options, s.seed.wrapping_add(1));
        let lit = s.level.has_look() || !s.m.look.is_empty();
        // A lit match shades play objects (plan 15a R7, R13); others draw as always.
        scene.set_shades(lit.then(|| ShadeRules {
            floor: olr_look::engine::FLOOR,
            projectile_glow: s.m.look.projectiles.iter().map(|p| p.as_ref().and_then(|p| p.glow)).collect(),
        }));
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
            level_look: s.level.look.clone(),
            distant: s.level.distant.clone(),
            surface: s.level.surface.clone(),
            distant_surface: s.level.distant_surface.clone(),
            lit,
            look: LookFrame::default(),
            mod_look: s.m.look.clone(),
            dynamic: look::Dynamic::default(),
            heats: heats(s.level),
            heat_now: world.time().0,
            heat_clock: world.time().0,
            heat_reset: false,
        }
    }

    /// The level or the mod has a look (plan 15a R7): frames are drawn
    /// with [`Presentation::look`].
    pub fn has_look(&self) -> bool {
        self.lit
    }

    /// The look of the frame last built; `None` without a look.
    pub fn look(&self) -> Option<&LookFrame> {
        self.lit.then_some(&self.look)
    }

    /// Turns the skins' lights on or off for this presentation, overriding
    /// the engine's one switch (`olr_look::engine::SKIN_LIGHT`, on): for
    /// tests and tools; no player or match setting reaches it.
    pub fn set_skin_light(&mut self, on: bool) {
        self.dynamic.set_skin_light(on);
    }

    /// The end flashes the last frame started (tests and tools).
    pub fn flashes_started(&self) -> &[FlashKey] {
        self.dynamic.started()
    }

    /// The explosions whose heat the last frame queued (issue #40; tests
    /// and tools): each once, whatever the re-simulations.
    pub fn blasts_started(&self) -> &[BlastKey] {
        self.dynamic.blasts()
    }

    /// The heat waiting for the next [`Presentation::upload`], each with
    /// its tick's time (ms), in tick order (issue #40; tests and tools).
    pub fn heat_waiting(&self) -> &[(u64, olr_render::HeatDeposit)] {
        self.dynamic.heat()
    }

    /// Clears the level's heat (issue #40: a replay seek to `world`): the
    /// heat waiting goes and the renderer's clears with the next upload;
    /// explosions heat again from `world`'s time.
    pub fn reset_heat(&mut self, world: &World) {
        self.dynamic.clear_heat();
        self.heat_reset = true;
        self.heat_now = world.time().0;
        self.heat_clock = self.heat_now;
    }

    /// Rebuilds the frame's look ([`Presentation::build`] does, after the
    /// scene and the entities): the level's part at the match clock (the
    /// world's time, `alpha` of a tick on), then the projectiles', end
    /// flashes', muzzles' and explosions' lights (plan 15a R9), then in a
    /// level with a look the skins' lights (issue #12), as glowing
    /// discs, wherever they are (plan 15c: never culled to the view, so the
    /// light stays camera-independent). Allocates nothing once warm.
    pub fn build_look(&mut self, world: &World, alpha: f32) {
        if !self.lit {
            return;
        }
        let t_ms = world.time().0 as f64 + f64::from(alpha.clamp(0.0, 1.0)) * 10.0;
        self.look.begin(self.level_look.as_ref(), t_ms);
        let skins = self.level_look.is_some().then_some(&self.assets);
        let (m, heats) = (&self.mod_look, self.heats);
        self.dynamic.push(&mut self.look, m, &mut self.scene, &self.fx, skins, t_ms, heats);
        self.heat_now = world.time().0;
    }

    /// A cosmetic-entity pool of `capacity` instead of the match's
    /// (`Fx::for_match`: play's 16384, or OLX's 1024 with `olx_quirks`), with
    /// play's death reserve, a sixteenth (bench only: the performance
    /// envelope's effects sweep, plan 8). Call it before the first
    /// [`Presentation::on_tick`]: the entities so far go.
    /// The blood amount stays.
    pub fn set_fx_capacity(&mut self, capacity: usize) {
        let blood = self.fx.blood();
        self.fx = Fx::with_capacity(self.seed.wrapping_add(2), capacity);
        self.fx.set_blood(blood);
    }

    /// The player's blood amount in percent (issue #6, Options → Video;
    /// `olr_fx::BLOOD_DEFAULT` 100 when new, at most `olr_fx::BLOOD_MAX`):
    /// how much of the simulation's blood the entities show from the next
    /// tick on ([`olr_fx::Fx::set_blood`]). Presentation only: the world,
    /// replays and the netcode never see it, so every player keeps their
    /// own. It stays through restarts and loaded states.
    pub fn set_blood(&mut self, percent: u16) {
        self.fx.set_blood(percent);
    }

    /// The blood amount in percent.
    pub fn blood(&self) -> u16 {
        self.fx.blood()
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
    /// With several views it draws the first ([`Presentation::build_views`]).
    pub fn build(&mut self, world: &World, alpha: f32, list: &mut DrawList) {
        self.build_views(world, alpha, core::slice::from_mut(list));
    }

    /// [`Presentation::build`] of every view, view `v` into `lists[v]`
    /// (split screen, [`Presentation::set_views`]): each camera's scene
    /// ([`olr_scene::Scene::build_views`]), the same entities into each
    /// list, then the frame's one look, shared by every view.
    pub fn build_views(&mut self, world: &World, alpha: f32, lists: &mut [DrawList]) {
        self.scene.build_views(world, alpha, &self.assets, lists);
        for list in lists.iter_mut().take(self.scene.views()) {
            self.fx.draw(&self.fx_assets, self.lit, list);
        }
        self.build_look(world, alpha);
    }

    /// A new world on the same level with the same worms (a restart): the
    /// scene starts from it, the entities go, the colour terrain is the
    /// level's again (uploaded whole with the next frame). The atlas stays.
    pub fn restart(&mut self, world: &World, level: &LoadedLevel, theme: &LoadedTheme) {
        self.scene.start(world);
        self.fx.clear();
        self.dynamic.clear();
        self.color = level.color_map(theme);
        self.distant = level.distant.clone();
        self.surface = level.surface.clone();
        self.distant_surface = level.distant_surface.clone();
        self.heats = heats(level);
        self.terrain_new = true;
        // The heat starts afresh with the terrain upload.
        (self.heat_now, self.heat_clock, self.heat_reset) = (world.time().0, world.time().0, false);
    }

    /// The world's state was replaced by a loaded one (online play's
    /// repair or a late spectator, `olr_netcode::Event::StateLoaded`): the
    /// scene starts from it and the entities go. The colour terrain is
    /// repainted by the whole-map restored area the load reports
    /// ([`net::NetPresenter::apply`]); call [`net::NetPresenter::reset`] too.
    pub fn state_loaded(&mut self, world: &World) {
        self.scene.start(world);
        self.fx.clear();
        self.dynamic.clear();
        self.reset_heat(world);
    }

    /// The first view's camera follows worm `worm` (by id) from the next
    /// frame: the local player's, or the one a spectator watches. A new
    /// presentation follows worm 0.
    pub fn follow(&mut self, worm: i32) {
        self.scene.camera_mut().set_target(worm);
    }

    /// The worm the first view's camera follows.
    pub fn followed(&self) -> i32 {
        self.scene.camera().target()
    }

    /// How many views the presentation draws (1, or 2 to
    /// [`olr_scene::MAX_VIEWS`] in split screen).
    pub fn views(&self) -> usize {
        self.scene.views()
    }

    /// A view per worm id of `targets` (split screen: one per local
    /// player; at least one, at most [`olr_scene::MAX_VIEWS`]), each camera
    /// following its worm with the solo rules (hard lock, clamp, shake, a
    /// dead worm's view staying, the jump at its respawn). A view kept
    /// keeps its camera (its place and shake); a new one starts on its
    /// worm with the next frame. Everything else stays one: the entities,
    /// the colour terrain, the scene's history and the look.
    ///
    /// OLX: src/client/CClient.cpp:1527-1551, 1653-1744
    pub fn set_views(&mut self, targets: &[i32]) {
        let targets = &targets[..targets.len().min(olr_scene::MAX_VIEWS)];
        self.scene.truncate_views(targets.len());
        let viewport = self.scene.camera().viewport();
        for (v, &worm) in targets.iter().enumerate() {
            match self.scene.cameras_mut().get_mut(v) {
                Some(c) => c.set_target(worm),
                None => {
                    self.scene.add_view(Camera::new(viewport, worm, view_seed(self.seed, v)));
                }
            }
        }
    }

    /// View `view`'s camera follows worm `worm` (by id); no such view
    /// changes nothing.
    pub fn follow_view(&mut self, view: usize, worm: i32) {
        if let Some(c) = self.scene.cameras_mut().get_mut(view) {
            c.set_target(worm);
        }
    }

    /// The worm view `view`'s camera follows.
    pub fn followed_view(&self, view: usize) -> Option<i32> {
        self.scene.cameras().get(view).map(Camera::target)
    }

    /// Each view's sound listener, in view order: the audio hears each
    /// positional sound from the nearest (`olr_audio::Audio::play_heard`).
    pub fn listeners(&self) -> olr_scene::Listeners {
        self.scene.listeners()
    }

    /// A view of another shape: new cameras on the same targets.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        let seed = self.seed;
        for (v, c) in self.scene.cameras_mut().iter_mut().enumerate() {
            if c.viewport() != viewport {
                *c = Camera::new(viewport, c.target(), view_seed(seed, v));
            }
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
    /// (a new world or renderer) with the level's distant layers, else the
    /// rectangles the ticks since the last upload changed. A whole upload
    /// gives the renderer the level's surface materials first (issue #39:
    /// a level whose materials heat gets a fresh heat texture). Then the
    /// heat (issue #40): the explosions' deposits since the last upload,
    /// each after the match time up to its tick, then the time up to the
    /// frame built (ticks never drawn included), running the renderer's
    /// heat between (`Renderer::run_heat`) whenever its deposits fill up or
    /// its steps fall more than a frame's behind, so the heat is the same
    /// however often frames are drawn.
    pub fn upload(&mut self, renderer: &mut Renderer) {
        // A look needs the renderer's look textures, uploaded with the map.
        if renderer.lighting() != self.lit {
            renderer.set_lighting(self.lit);
            self.terrain_new = true;
        }
        if self.take_atlas_dirty() {
            renderer.upload_atlas(&self.assets);
        }
        if core::mem::take(&mut self.terrain_new) {
            self.color.take_dirty().for_each(drop);
            // The heat (issue #39) is made afresh with the terrain.
            let materials = self.level_look.as_ref().map_or(&[][..], |l| &l.materials[..]);
            renderer.set_surface(materials, self.surface.as_ref());
            renderer.set_distant_surface(&self.distant_surface);
            renderer.upload_terrain(&self.color);
            renderer.set_distant(&self.distant);
        } else {
            renderer.update_terrain(&mut self.color);
        }
        self.upload_heat(renderer);
    }

    /// [`Presentation::upload`]'s heat, after the terrain: deposits land
    /// on the craters their explosions carved.
    fn upload_heat(&mut self, r: &mut Renderer) {
        if core::mem::take(&mut self.heat_reset) {
            r.reset_heat();
        }
        if !r.has_heat() {
            self.dynamic.clear_heat();
            self.heat_clock = self.heat_clock.max(self.heat_now);
            return;
        }
        let ms = |d: u64| u32::try_from(d).unwrap_or(u32::MAX);
        // Issue #41: the distant layers' slices are placed by the views of
        // the frame built, the one first showing the explosions.
        let mut views = [HeatView::centred(olr_scene::View::default()); olr_scene::MAX_VIEWS];
        for (v, c) in views.iter_mut().zip(self.scene.cameras()) {
            *v = HeatView::centred(c.view());
        }
        let views = &views[..self.scene.views().min(olr_scene::MAX_VIEWS)];
        let mut clock = self.heat_clock;
        for &(at, deposit) in self.dynamic.heat() {
            if at > clock {
                r.advance_heat(ms(at - clock));
                clock = at;
            }
            if r.heat_waiting() >= MAX_HEAT_DEPOSITS {
                r.run_heat();
            }
            r.deposit_heat_seen(deposit, views);
        }
        self.dynamic.clear_heat();
        if self.heat_now > clock {
            r.advance_heat(ms(self.heat_now - clock));
            clock = self.heat_now;
        }
        self.heat_clock = clock;
        for _ in 0..HEAT_RUNS {
            if r.heat_behind() <= u64::from(MAX_HEAT_STEPS) {
                break;
            }
            r.run_heat();
        }
    }
}
