//! One match without a window: the world and who plays it (`drive.rs`:
//! the sandbox's player, a Deathmatch's player and bots, a replay), its
//! presentation (`rl_present::Presentation`: scene, cosmetic entities,
//! colour terrain, atlas images), sound, the damage popups and the match
//! HUD's state (`hud.rs`). The window and GPU (`app.rs`, `gpu.rs`) drive it.

use rl_audio::Audio;
use rl_content::{LoadedSkin, MatchAssets};
use rl_math::Vec2;
use rl_modes::Deathmatch;
use rl_present::{PresentSetup, Presentation, default_colour};
use rl_replay::Replay;
use rl_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use rl_sim::{EventKind, Keys, TickEvents, World};

use crate::drive::{MatchDriver, ReplayDriver};
use crate::hud::{Feed, Names, Results, message_text};
use crate::setup::{Assets, Game};

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

/// Who presses the worms' keys.
pub enum Driver {
    /// The player (worm 0) and idle targets.
    Sandbox,
    /// The player (worm 0) and bots.
    Match(Box<MatchDriver>),
    /// A replay's keys.
    Replay(ReplayDriver),
}

/// Kills and time over the whole run (rematches included), for the exit
/// summary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Kills of another worm by a bot, by the player (worm 0 when it is
    /// not a bot), and suicides.
    pub bot_kills: u64,
    pub player_kills: u64,
    pub suicides: u64,
    /// Ticks played before a match ended.
    pub match_ticks: u64,
    pub matches_ended: u32,
}

impl Tally {
    /// Bot kills per minute of match time.
    pub fn bot_kills_per_minute(&self) -> f64 {
        if self.match_ticks == 0 { 0.0 } else { self.bot_kills as f64 * 6000.0 / self.match_ticks as f64 }
    }
}

/// A match: worm 0 is the player (or, in a replay, the worm the camera
/// follows).
pub struct Session {
    pub world: World,
    pub present: Presentation,
    pub events: TickEvents,
    pub audio: Audio,
    pub popups: Vec<Popup>,
    pub driver: Driver,
    pub names: Names,
    /// The kill feed (the mode's messages).
    pub feed: Feed,
    /// The time-limit clock: the time left, frozen when the match ends
    /// (Deathmatch's own keeps counting; the HUD shows the end's).
    pub time_left_ms: Option<u64>,
    /// The results, once the match ended.
    pub results: Option<Results>,
    pub tally: Tally,
    /// The player's deaths and spawns this run (restarts included), for
    /// the exit summary.
    pub player_deaths: u32,
    pub player_spawns: u32,
    /// Rematches so far (the seed of rematch `n` is the match's plus `n`).
    pub rematches: u32,
    keys: Vec<Keys>,
    /// Ticks since the match (re)started.
    ticks: u64,
    /// The tick the match ended in.
    ended_at: Option<u64>,
    /// Worms the presentation knows (a worm spawned mid-match is added,
    /// with this skin).
    shown: usize,
    skin: LoadedSkin,
}

/// The world and driver `a` starts with, rematch `n`.
fn start(a: &Assets, n: u32) -> anyhow::Result<(World, Driver)> {
    Ok(match &a.game {
        Game::Sandbox(_) => (a.new_world()?, Driver::Sandbox),
        Game::Deathmatch { options, packs } => {
            let o = rl_content::MatchOptions { seed: options.seed.wrapping_add(n), ..(**options).clone() };
            let assets = MatchAssets { m: &a.m, level: &a.level, theme: &a.theme, packs };
            let (world, d) = MatchDriver::new(rl_content::build_match(assets, &o)?);
            (world, Driver::Match(Box::new(d)))
        }
        Game::Replay(r) => {
            let world = r.world(rl_content::replay_assets_from(&a.m, &a.level, &a.theme))?;
            (world, Driver::Replay(ReplayDriver::new(r.clone())))
        }
    })
}

/// The worms' names: the player is "You", the others are numbered from 1
/// (a replay's worms all, from 1).
fn names(a: &Assets, worms: usize) -> Names {
    let name = |i: usize| match a.game {
        Game::Replay(_) => format!("Worm {}", i + 1),
        _ if i == 0 => "You".into(),
        Game::Sandbox(_) => format!("Target {i}"),
        Game::Deathmatch { .. } => format!("Bot {i}"),
    };
    Names((0..worms).map(name).collect())
}

impl Session {
    /// A new match of `a`'s game seen through `viewport`, with its sound
    /// output `audio` (already loaded with the mod's sounds). Registers every
    /// image (scene, each worm's skin and gibs, entities); the caller's
    /// `Presentation::upload` sends them to the renderer.
    pub fn new(
        a: &Assets,
        viewport: Viewport,
        interpolation: Interpolation,
        audio: Audio,
    ) -> anyhow::Result<Session> {
        let (world, driver) = start(a, 0)?;
        let seed = match &a.game {
            Game::Sandbox(s) => s.seed,
            Game::Deathmatch { options, .. } => options.seed,
            Game::Replay(r) => r.setup.seed,
        };
        let setup = PresentSetup {
            core: a.content.core(),
            m: &a.m,
            level: &a.level,
            theme: &a.theme,
            viewport,
            options: SceneOptions { interpolation, show_health_bars: false },
            seed,
        };
        let present = Presentation::new(&setup, &world);
        let n = world.worms().len();
        let mut s = Session {
            names: names(a, n.max(a.worms())),
            world,
            present,
            events: TickEvents::new(),
            audio,
            popups: Vec::with_capacity(64),
            driver,
            feed: Feed::default(),
            time_left_ms: None,
            results: None,
            tally: Tally::default(),
            player_deaths: 0,
            player_spawns: 0,
            rematches: 0,
            keys: vec![Keys(0); n],
            ticks: 0,
            ended_at: None,
            shown: 0,
            skin: a.skin.clone(),
        };
        s.show_new_worms();
        s.time_left_ms = s.mode_time_left();
        Ok(s)
    }

    /// Registers the worms the presentation does not know yet (all at the
    /// start; a worm a mode spawns mid-match later, which marks the atlas
    /// for upload).
    fn show_new_worms(&mut self) {
        let human = !matches!(self.driver, Driver::Replay(_));
        while self.shown < self.world.worms().len() {
            let i = self.shown;
            self.present.add_worm(&self.skin, default_colour(i), i == 0, i == 0 && human);
            self.shown += 1;
        }
        if self.keys.len() < self.shown {
            self.keys.resize(self.shown, Keys(0));
        }
        while self.names.0.len() < self.shown {
            let i = self.names.0.len();
            self.names.0.push(format!("Worm {}", i + 1));
        }
    }

    /// The Deathmatch's time left, if the world plays one with a limit.
    fn mode_time_left(&self) -> Option<u64> {
        self.world.mode_state().as_any().downcast_ref::<Deathmatch>()?.time_left_ms()
    }

    /// F5: the sandbox or replay from the start, or a Deathmatch's rematch
    /// (the next seed), from the assets loaded once. The worms are the
    /// same, so the atlas stays; the next upload sends the new colour map.
    /// Gives the replay of the match that was playing (a Deathmatch's).
    pub fn restart(&mut self, a: &Assets) -> anyhow::Result<Option<Replay>> {
        let n = if matches!(a.game, Game::Deathmatch { .. }) { self.rematches + 1 } else { 0 };
        let (world, driver) = start(a, n)?;
        anyhow::ensure!(world.worms().len() == self.shown, "the restarted match has other worms");
        let old = self.take_replay();
        self.rematches = n;
        self.world = world;
        self.driver = driver;
        self.present.restart(&self.world, &a.level, &a.theme);
        self.audio.stop_all();
        self.popups.clear();
        self.feed.clear();
        self.results = None;
        self.ended_at = None;
        self.time_left_ms = self.mode_time_left();
        self.keys.fill(Keys(0));
        // The rematch's bots think first: the last match's events are not theirs.
        self.events = TickEvents::new();
        self.ticks = 0;
        Ok(old)
    }

    /// The replay of the match playing (a Deathmatch's), finished now if
    /// it was still recording; `None` once taken, or for another game.
    pub fn take_replay(&mut self) -> Option<Replay> {
        match &mut self.driver {
            Driver::Match(d) => d.take_replay(&self.world),
            _ => None,
        }
    }

    /// Ticks since the match (re)started.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Ticks since the match ended, once it has.
    pub fn since_end(&self) -> Option<u64> {
        self.ended_at.map(|t| self.ticks - t)
    }

    /// A window of another shape: the camera's viewport follows.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.present.set_viewport(viewport);
    }

    /// One 100 Hz tick with the player holding `player` (the sandbox's
    /// targets hold nothing; a Deathmatch's bots think first; a replay
    /// plays its keys and ignores `player`, and stops at its end): the
    /// simulation, then the cosmetic entities (which replay the tick's
    /// terrain edits on the colour map and rebuild any area the tick
    /// lost), the sounds at the view centre, the scene's history and
    /// marks, the damage popups and the HUD. Debug builds check every
    /// [`DRIFT_CHECK_TICKS`] that the colour terrain has not drifted.
    pub fn tick(&mut self, player: Keys) {
        let ended = self.since_end();
        match &mut self.driver {
            Driver::Sandbox => {
                self.keys[0] = player;
                self.world.tick(&self.keys, &mut self.events);
            }
            Driver::Match(d) => {
                self.keys[0] = player;
                d.think(&self.world, &self.events, &mut self.keys, ended);
                self.world.tick(&self.keys, &mut self.events);
                d.record(&self.keys, &self.world);
            }
            Driver::Replay(d) => {
                if !d.step(&mut self.world, &mut self.events) {
                    return;
                }
            }
        }
        self.show_new_worms();
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
        self.after_tick();
    }

    /// The tick's events: popups, the kill feed, the tally, the end.
    fn after_tick(&mut self) {
        // The mode's clock stops at the game over (`Deathmatch::time_left_ms`).
        self.time_left_ms = self.mode_time_left();
        if self.results.is_none() {
            self.tally.match_ticks += 1;
        }
        let bot = |w: i32| match &self.driver {
            Driver::Match(d) => d.bots.iter().any(|b| b.worm() as i32 == w),
            _ => false,
        };
        for e in self.events.iter() {
            match e.kind {
                EventKind::Kill { victim, killer } => {
                    if victim == 0 {
                        self.player_deaths += 1;
                    }
                    if killer == victim {
                        self.tally.suicides += 1;
                    } else if bot(killer) {
                        self.tally.bot_kills += 1;
                    } else if killer == 0 {
                        self.tally.player_kills += 1;
                    }
                }
                EventKind::Spawned { worm: 0 } => self.player_spawns += 1,
                EventKind::Message(m) => {
                    if let Some(line) = message_text(&m, &self.names) {
                        self.feed.push(self.ticks, line);
                    }
                }
                EventKind::GameOver(_) => {
                    self.ended_at = Some(self.ticks);
                    self.tally.matches_ended += 1;
                    self.results = Results::of(&self.world, &self.names, self.time_left_ms);
                }
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
