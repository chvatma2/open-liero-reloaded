//! One sandbox match without a window: the world, its presentation state
//! (scene, cosmetic entities, colour terrain), sound and the damage popups.
//! The window and GPU (`app.rs`, `gpu.rs`) drive it.

use rl_audio::Audio;
use rl_data::image::Rgba;
use rl_fx::{Fx, FxAssets, FxImages, FxTick, default_dirt_colour};
use rl_math::Vec2;
use rl_scene::{Camera, DrawList, Interpolation, Scene, SceneAssets, SceneOptions, Viewport};
use rl_sim::{EventKind, Keys, TickEvents, World};
use rl_terrain::ColorMap;

use crate::setup::Assets;

/// OLX's default worm colours for the first worms (`Profiles.cpp`): red,
/// blue, green, yellow.
pub const COLOURS: [Rgba; 4] =
    [Rgba::rgb(255, 0, 0), Rgba::rgb(0, 0, 255), Rgba::rgb(0, 255, 0), Rgba::rgb(255, 255, 0)];

/// How long a damage popup shows, in ticks.
pub const POPUP_TICKS: u32 = 100;

/// Hits on one worm this close together (ticks) show as one number.
pub const MERGE_TICKS: u32 = 30;

/// A damage number floating up from a worm (spec §19 E10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Popup {
    pub worm: i32,
    /// The health it took (`Damage.dealt`).
    pub amount: f32,
    /// Where the worm was hit, in world pixels.
    pub pos: Vec2,
    pub age: u32,
}

/// The sandbox's match: worm 0 is the player, the others idle targets.
pub struct Session {
    pub world: World,
    pub scene: Scene,
    pub fx: Fx,
    pub color: ColorMap,
    pub events: TickEvents,
    /// Every image the renderer's atlas holds (scene, worms, entities).
    pub scene_assets: SceneAssets,
    pub fx_assets: FxAssets,
    pub audio: Audio,
    pub popups: Vec<Popup>,
    /// The player's deaths and spawns this run (restarts included), for
    /// the exit summary.
    pub player_deaths: u32,
    pub player_spawns: u32,
    keys: Vec<Keys>,
    seed: u32,
}

impl Session {
    /// A new match of `a`'s sandbox seen through `viewport`, with its sound
    /// output `audio` (already loaded with the mod's sounds). Registers every
    /// image (scene, each worm's skin and gibs, entities) before the caller
    /// uploads the atlas.
    pub fn new(
        a: &Assets,
        viewport: Viewport,
        interpolation: Interpolation,
        audio: Audio,
    ) -> anyhow::Result<Session> {
        let world = a.new_world()?;
        let core = a.content.core();
        let mut scene_assets = SceneAssets::new(core, &a.m);
        let mut fx_assets =
            FxAssets::new(FxImages::from_core(core), default_dirt_colour(&a.theme), &mut scene_assets);
        for i in 0..world.worms().len() {
            let colour = COLOURS[i % COLOURS.len()];
            scene_assets.add_worm(&a.skin, colour, i == 0, i == 0);
            fx_assets.add_worm(colour, &mut scene_assets);
        }
        let seed = a.setup.seed;
        let options = SceneOptions { interpolation, show_health_bars: false };
        let n = world.worms().len();
        let mut s = Session {
            world,
            scene: Scene::new(Camera::new(viewport, 0, seed), options, seed.wrapping_add(1)),
            fx: Fx::new(seed.wrapping_add(2)),
            color: a.level.color_map(&a.theme),
            events: TickEvents::new(),
            scene_assets,
            fx_assets,
            audio,
            popups: Vec::with_capacity(64),
            player_deaths: 0,
            player_spawns: 0,
            keys: vec![Keys(0); n],
            seed,
        };
        s.scene.after_tick(&s.world, &s.events);
        Ok(s)
    }

    /// F5: the same sandbox from the start, from the assets loaded once.
    /// The worms are the same, so the atlas stays; the caller uploads the
    /// new colour map.
    pub fn restart(&mut self, a: &Assets) -> anyhow::Result<()> {
        let world = a.new_world()?;
        anyhow::ensure!(world.worms().len() == self.keys.len(), "the restarted sandbox has other worms");
        self.world = world;
        self.scene.reset();
        self.events = TickEvents::new();
        self.scene.after_tick(&self.world, &self.events);
        self.fx.clear();
        self.color = a.level.color_map(&a.theme);
        self.audio.stop_all();
        self.popups.clear();
        Ok(())
    }

    /// A window of another shape: the camera's viewport follows.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        if self.scene.camera.viewport() != viewport {
            let target = self.scene.camera.target();
            self.scene.camera = Camera::new(viewport, target, self.seed);
        }
    }

    /// One 100 Hz tick with the player holding `player` (the targets hold
    /// nothing): the simulation, then the cosmetic entities (which replay
    /// the tick's terrain edits on the colour map and rebuild any area the
    /// tick lost), the sounds at the view centre, the scene's history and
    /// marks, and the damage popups. The caller then uploads the terrain.
    pub fn tick(&mut self, player: Keys) {
        self.keys[0] = player;
        self.world.tick(&self.keys, &mut self.events);
        self.fx.step_tick(&FxTick::new(&self.world, &self.events), &mut self.color, &self.fx_assets);
        self.audio.play_events(&self.events, self.scene.camera.listener());
        self.scene.after_tick(&self.world, &self.events);
        self.popups.retain_mut(|p| {
            p.age += 1;
            p.age < POPUP_TICKS
        });
        for e in self.events.iter() {
            match e.kind {
                EventKind::Kill { victim: 0, .. } => self.player_deaths += 1,
                EventKind::Spawned { worm: 0 } => self.player_spawns += 1,
                _ => {}
            }
            let EventKind::Damage { worm, dealt, .. } = e.kind else { continue };
            let Some(w) = self.world.worms().iter().find(|w| w.id == worm) else { continue };
            if dealt <= 0.0 {
                continue;
            }
            // Hits in quick succession sum into one number, as OLX's do.
            if let Some(p) = self.popups.iter_mut().find(|p| p.worm == worm && p.age < MERGE_TICKS) {
                p.amount += dealt;
                p.pos = w.pos;
                p.age = 0;
            } else if self.popups.len() < self.popups.capacity() {
                self.popups.push(Popup { worm, amount: dealt, pos: w.pos, age: 0 });
            }
        }
    }

    /// One drawn frame at `alpha` between the last two ticks: the scene,
    /// then the cosmetic entities.
    pub fn build(&mut self, alpha: f32, list: &mut DrawList) {
        self.scene.build(&self.world, alpha, &self.scene_assets, list);
        self.fx.draw(&self.fx_assets, list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{SandboxOptions, load};

    /// A match plays headless (silent), and F5 gives back the starting
    /// world without loading anything again.
    #[test]
    fn session_plays_and_restarts_from_loaded_assets() {
        let Some(dir) = std::env::var_os("RL_CONTENT_DIR").filter(|d| std::path::Path::new(d).is_dir())
        else {
            eprintln!("RL_CONTENT_DIR not set; skipping");
            return;
        };
        let o = SandboxOptions {
            mod_id: String::new(),
            level_id: String::new(),
            seed: 4,
            targets: 2,
            bonuses: true,
        };
        let a = load(std::path::Path::new(&dir), "MW 1.0", "Dirt Level", "default", o).unwrap();
        let vp = Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
        let mut s = Session::new(&a, vp, Interpolation::Linear, Audio::silent()).unwrap();
        let start = s.world.clone();
        let mut list = DrawList::new();
        let mut busiest = 0;
        for t in 0..600 {
            // Aim down and fire, then dig right.
            let keys = if t < 300 {
                Keys::DOWN | Keys::FIRE
            } else {
                Keys::RIGHT | if t % 10 == 0 { Keys::LEFT } else { 0 }
            };
            s.tick(Keys(keys));
            s.build(0.5, &mut list);
            busiest = busiest.max(s.fx.len());
        }
        assert!(busiest > 0, "explosions and dirt made entities");
        assert!(
            !s.popups.is_empty() || s.player_deaths > 0 || s.world.worms()[0].health < 100.0,
            "self-damage"
        );
        assert_ne!(s.world, start);
        s.restart(&a).unwrap();
        assert_eq!(s.world, start, "F5 is the same start");
        assert!(s.fx.is_empty() && s.popups.is_empty());
        assert!(s.color.pixels() == a.level.color_map(&a.theme).pixels(), "the colour terrain starts again");
    }
}
