//! One sandbox match without a window: the world, its presentation
//! (`rl_present::Presentation`: scene, cosmetic entities, colour terrain,
//! atlas images), sound and the damage popups. The window and GPU
//! (`app.rs`, `gpu.rs`) drive it.

use rl_audio::Audio;
use rl_math::Vec2;
use rl_present::{PresentSetup, Presentation, default_colour};
use rl_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use rl_sim::{EventKind, Keys, TickEvents, World};

use crate::setup::Assets;

/// How often (ticks) debug builds check that the colour terrain's material
/// is the world's (`Presentation::terrain_drift`): a terrain change
/// without its `TerrainEdit` event would leave dirt drawn on background.
pub const DRIFT_CHECK_TICKS: u64 = 100;

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
    pub present: Presentation,
    pub events: TickEvents,
    pub audio: Audio,
    pub popups: Vec<Popup>,
    /// The player's deaths and spawns this run (restarts included), for
    /// the exit summary.
    pub player_deaths: u32,
    pub player_spawns: u32,
    keys: Vec<Keys>,
    /// Ticks since the match (re)started.
    ticks: u64,
}

impl Session {
    /// A new match of `a`'s sandbox seen through `viewport`, with its sound
    /// output `audio` (already loaded with the mod's sounds). Registers every
    /// image (scene, each worm's skin and gibs, entities); the caller's
    /// `Presentation::upload` sends them to the renderer.
    pub fn new(
        a: &Assets,
        viewport: Viewport,
        interpolation: Interpolation,
        audio: Audio,
    ) -> anyhow::Result<Session> {
        let world = a.new_world()?;
        let setup = PresentSetup {
            core: a.content.core(),
            m: &a.m,
            level: &a.level,
            theme: &a.theme,
            viewport,
            options: SceneOptions { interpolation, show_health_bars: false },
            seed: a.setup.seed,
        };
        let mut present = Presentation::new(&setup, &world);
        for i in 0..world.worms().len() {
            present.add_worm(&a.skin, default_colour(i), i == 0, i == 0);
        }
        let n = world.worms().len();
        Ok(Session {
            world,
            present,
            events: TickEvents::new(),
            audio,
            popups: Vec::with_capacity(64),
            player_deaths: 0,
            player_spawns: 0,
            keys: vec![Keys(0); n],
            ticks: 0,
        })
    }

    /// F5: the same sandbox from the start, from the assets loaded once.
    /// The worms are the same, so the atlas stays; the next upload sends
    /// the new colour map.
    pub fn restart(&mut self, a: &Assets) -> anyhow::Result<()> {
        let world = a.new_world()?;
        anyhow::ensure!(world.worms().len() == self.keys.len(), "the restarted sandbox has other worms");
        self.world = world;
        self.present.restart(&self.world, &a.level, &a.theme);
        self.audio.stop_all();
        self.popups.clear();
        self.ticks = 0;
        Ok(())
    }

    /// A window of another shape: the camera's viewport follows.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.present.set_viewport(viewport);
    }

    /// One 100 Hz tick with the player holding `player` (the targets hold
    /// nothing): the simulation, then the cosmetic entities (which replay
    /// the tick's terrain edits on the colour map and rebuild any area the
    /// tick lost), the sounds at the view centre, the scene's history and
    /// marks, and the damage popups. Debug builds check every
    /// [`DRIFT_CHECK_TICKS`] that the colour terrain has not drifted.
    pub fn tick(&mut self, player: Keys) {
        self.keys[0] = player;
        self.world.tick(&self.keys, &mut self.events);
        self.audio.play_events(&self.events, self.present.scene.camera.listener());
        self.present.on_tick(&self.world, &self.events);
        self.ticks += 1;
        if cfg!(any(debug_assertions, test)) && self.ticks.is_multiple_of(DRIFT_CHECK_TICKS) {
            assert!(
                !self.present.terrain_drift(&self.world),
                "tick {}: the colour terrain's material is not the world's (a terrain change without its TerrainEdit?)",
                self.ticks
            );
        }
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
        self.present.build(&self.world, alpha, list);
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
            busiest = busiest.max(s.present.fx.len());
        }
        assert!(busiest > 0, "explosions and dirt made entities");
        assert!(
            !s.popups.is_empty() || s.player_deaths > 0 || s.world.worms()[0].health < 100.0,
            "self-damage"
        );
        assert_ne!(s.world, start);
        s.restart(&a).unwrap();
        assert_eq!(s.world, start, "F5 is the same start");
        assert!(s.present.fx.is_empty() && s.popups.is_empty());
        assert!(
            s.present.color().pixels() == a.level.color_map(&a.theme).pixels(),
            "the colour terrain starts again"
        );
    }

    /// Debug builds catch a colour terrain that drifted from the world's
    /// material (a future terrain change without its `TerrainEdit`).
    #[test]
    fn session_catches_terrain_drift() {
        let Some(dir) = std::env::var_os("RL_CONTENT_DIR").filter(|d| std::path::Path::new(d).is_dir())
        else {
            eprintln!("RL_CONTENT_DIR not set; skipping");
            return;
        };
        let o = SandboxOptions {
            mod_id: String::new(),
            level_id: String::new(),
            seed: 4,
            targets: 1,
            bonuses: false,
        };
        let a = load(std::path::Path::new(&dir), "Classic", "Liero Factory", "default", o).unwrap();
        let vp = Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
        let mut s = Session::new(&a, vp, Interpolation::Linear, Audio::silent()).unwrap();
        for _ in 0..DRIFT_CHECK_TICKS {
            s.tick(Keys(Keys::FIRE));
        }
        // Dirt where the player stands, which the world never placed.
        let at = s.world.worms()[0].pos;
        s.present.color_mut().apply(rl_terrain::ColorEdit::Dirt { hole: 0, x: at.x as i32, y: at.y as i32 });
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for _ in 0..DRIFT_CHECK_TICKS {
                s.tick(Keys(0));
            }
        }));
        let msg = caught.expect_err("the drift is caught within one check period");
        let msg = msg.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(msg.contains("colour terrain"), "{msg}");
    }
}
