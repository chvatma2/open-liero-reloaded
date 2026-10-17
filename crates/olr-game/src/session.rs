//! One match without a window: the world and who plays it (`drive.rs`:
//! the sandbox's player, a Deathmatch's player and bots, a replay;
//! `online.rs`: an online match), its presentation
//! (`olr_present::Presentation`: scene, cosmetic entities, colour terrain,
//! atlas images), sound, the damage popups and the match HUD's state
//! (`hud.rs`). The window and GPU (`app.rs`, `gpu.rs`) drive it.
//!
//! The player plays the local slot ([`Session::local`]: any worm, not
//! always worm 0; `None` when nobody here plays: a replay, bots alone, an
//! online match watched). Its keys, name, look, camera, crosshair, HUD,
//! minimap dot, scoreboard row, kills, deaths and spawns are that slot's.
//! Every worm wears its own look ([`WormLook`]).

use std::time::Instant;

use olr_audio::Audio;
use olr_content::{LoadedSkin, MatchAssets};
use olr_math::Vec2;
use olr_modes::Deathmatch;
use olr_netcode::world_sim::ConfirmedTick;
use olr_online::OnlineEvent;
use olr_present::{PresentSetup, Presentation, default_colour};
use olr_replay::Replay;
use olr_scene::{DrawList, Interpolation, SceneOptions, Viewport};
use olr_sim::{Event, EventKind, Keys, MatchResult, Score, TickEvents, World};
use olr_terrain::Area;

use olr_ui::hud::{Log, LogLine, Names, Results, message_text};

use crate::drive::{MatchDriver, ReplayDriver};
use crate::net::PolledEvents;
use crate::online::OnlineDriver;
use crate::setup::{Assets, Game};
use crate::throw_sound::ThrowSounds;

/// How often (ticks) debug builds check that the colour terrain's material
/// is the world's (`Presentation::terrain_drift`): a terrain change
/// without its `TerrainEdit` event would leave dirt drawn on background.
pub const DRIFT_CHECK_TICKS: u64 = 100;

/// How long a damage popup shows, in ticks.
pub const POPUP_TICKS: u32 = 100;

/// Hits on one worm this close together (ticks) show as one number.
pub const MERGE_TICKS: u32 = 30;

/// How long the weapon name stays over the player's worm after a change
/// (ticks): OLX's `fForceWeapon_Time = tLX->currentTime + 0.75f`.
///
/// OLX: src/common/CWormHuman.cpp:405-428
pub const WEAPON_NAME_TICKS: u64 = 75;

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
    /// The player (its slot, if one plays) and bots.
    Match(Box<MatchDriver>),
    /// A replay's keys.
    Replay(ReplayDriver),
    /// An online match: its peer ticks it ([`Session::frame`], never
    /// [`Session::tick`]).
    Online(Box<OnlineDriver>),
}

/// How one worm looks: the name over it and in the HUD, its colour (the
/// skin's tint, the minimap's dot) and its skin.
#[derive(Clone, Debug)]
pub struct WormLook {
    pub name: String,
    pub colour: [u8; 3],
    pub skin: LoadedSkin,
}

/// Kills and time over the whole run (rematches included), for the exit
/// summary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Kills of another worm by a bot, by the player (the local slot),
    /// and suicides.
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

/// A match: the local slot is the player's (if one plays); the camera and
/// the HUD follow it, or the worm a watcher chose ([`Session::follow`]).
pub struct Session {
    /// The world. An online match's is its peer's ([`Session::world`]):
    /// this one is its start, then the one presented last, kept once the
    /// match ended there (`frozen`).
    pub world: World,
    pub present: Presentation,
    pub events: TickEvents,
    pub audio: Audio,
    pub popups: Vec<Popup>,
    pub driver: Driver,
    pub names: Names,
    /// The event log (the mode's messages: kills, deaths, the end).
    pub log: Log,
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
    /// A person plays (`Assets::human_plays`, again at each restart; an
    /// online match always counts); otherwise the game is watched (a
    /// replay, bots alone) and may run fast (`speed.rs`).
    human: bool,
    /// The slot this player controls; `None`: watching.
    local: Option<u8>,
    /// An online match ended: `world` is the one drawn, no longer the
    /// peer's.
    frozen: bool,
    /// Every worm's look, by index (a worm a mode spawns mid-match gets a
    /// default one).
    looks: Vec<WormLook>,
    /// The ticks play their sounds (the app thins them out when the game
    /// runs fast: `Speed::sound`). The world never sees it.
    pub sound: bool,
    /// The sounds the last tick or poll played ([`Session::sounds`]).
    heard: Vec<Event>,
    /// Whose rope throws are heard: the player's own (OLX's).
    throws: ThrowSounds,
    keys: Vec<Keys>,
    /// Ticks since the match (re)started.
    ticks: u64,
    /// The tick the match ended in.
    ended_at: Option<u64>,
    /// The weapon name shows over the player's worm until this tick: a
    /// change made with the change key (OLX's `bForceWeapon_Name` and
    /// `fForceWeapon_Time`, [`Session::weapon_name`]).
    weapon_name_until: Option<u64>,
    /// Worms the presentation knows (a worm spawned mid-match is added).
    shown: usize,
    /// OLX's default skin (a worm spawned mid-match wears it).
    default_skin: LoadedSkin,
}

/// The worm whose rope throws are heard ([`ThrowSounds`]): the player's,
/// or a replay's viewer's (the player whose game it was).
fn hears_throws_of(a: &Assets) -> Option<u8> {
    match &a.game {
        Game::Replay(r) => r.setup.viewer,
        _ => a.local_slot(),
    }
}

/// The world and driver `a` starts with, rematch `n`.
fn start(a: &Assets, n: u32) -> anyhow::Result<(World, Driver)> {
    Ok(match &a.game {
        Game::Sandbox(_) => (a.new_world()?, Driver::Sandbox),
        Game::Deathmatch { options, packs } => {
            let o = olr_content::MatchOptions { seed: options.seed.wrapping_add(n), ..(**options).clone() };
            let assets = MatchAssets { m: &a.m, level: &a.level, theme: &a.theme, packs };
            let (world, d) = MatchDriver::new(olr_content::build_match(assets, &o)?);
            (world, Driver::Match(Box::new(d)))
        }
        Game::Replay(r) => {
            let world = r.world(olr_content::replay_assets_from(&a.m, &a.level, &a.theme))?;
            (world, Driver::Replay(ReplayDriver::new(r.clone())))
        }
        Game::Online { .. } => anyhow::bail!("an online match's world is its peer's (Session::online)"),
    })
}

fn rgb(c: olr_data::image::Rgba) -> [u8; 3] {
    [c.r, c.g, c.b]
}

/// The looks of `a`'s first `n` worms. An online match's are the lobby's
/// (every member's and bot's name, colour and skin). Otherwise the player
/// (the local slot) has the profile's name ("You" without one), colour
/// (else the default first colour) and skin; the others are numbered from
/// 1 in worm order ("Target N" in the sandbox, "Bot N"), in OLX's bot
/// colours away from the player's and the default skin; a game nobody
/// plays numbers every worm from 1 ("Worm N" in a replay, whose looks are
/// not recorded) in the default colours.
fn looks(a: &Assets, n: usize) -> Vec<WormLook> {
    let default = |i: usize| WormLook {
        name: format!("Worm {}", i + 1),
        colour: rgb(default_colour(i)),
        skin: a.bot_skin.clone(),
    };
    if let Game::Online { start, .. } = &a.game {
        let lobby = start.slots.iter().enumerate().map(|(i, s)| WormLook {
            name: s.look.name.clone(),
            colour: s.look.colour,
            skin: a.skin_of_slot(i).clone(),
        });
        return lobby.chain((start.slots.len()..n).map(default)).collect();
    }
    let local = a.local_slot().map(usize::from).filter(|&i| i < n);
    let mine = a.player.as_ref().map_or_else(|| rgb(default_colour(0)), |p| p.colour);
    let others = if local.is_some() { olr_ui::bot_colours(mine, n.saturating_sub(1)) } else { Vec::new() };
    let mut other = 0;
    (0..n)
        .map(|i| {
            if Some(i) == local {
                let name = a.player.as_ref().map_or_else(|| "You".into(), |p| p.name.clone());
                return WormLook { name, colour: mine, skin: a.skin.clone() };
            }
            other += 1;
            let name = match a.game {
                Game::Replay(_) => format!("Worm {}", i + 1),
                _ if local.is_none() => format!("Bot {}", i + 1),
                Game::Sandbox(_) => format!("Target {other}"),
                _ => format!("Bot {other}"),
            };
            let colour = others.get(other - 1).copied().unwrap_or_else(|| rgb(default_colour(i)));
            WormLook { name, colour, skin: a.bot_skin.clone() }
        })
        .collect()
}

/// The Deathmatch's time left in `world`, if it plays one with a limit.
fn mode_time_left(world: &World) -> Option<u64> {
    world.mode_state().as_any().downcast_ref::<Deathmatch>()?.time_left_ms()
}

/// Where the worms were after a tick (the damage popups').
enum Positions<'a> {
    World(&'a World),
    Listed(&'a [(i32, Vec2)]),
}

impl Positions<'_> {
    fn of(&self, worm: i32) -> Option<Vec2> {
        match self {
            Positions::World(w) => w.worms().iter().find(|w| w.id == worm).map(|w| w.pos),
            Positions::Listed(l) => l.iter().find(|(id, _)| *id == worm).map(|&(_, p)| p),
        }
    }
}

/// One tick as the HUD's bookkeeping reads it (`Session::after_tick`): a
/// local tick's world and events, or an online match's confirmed tick
/// (`olr_netcode::world_sim::ConfirmedTick`, spec §3.4).
struct TickView<'a> {
    /// The ticks played with it (its number plus one).
    played: u64,
    events: &'a [Event],
    scores: &'a [Score],
    time_left_ms: Option<u64>,
    result: Option<MatchResult>,
    positions: Positions<'a>,
}

impl Session {
    /// A new match of `a`'s game seen through `viewport`, with its sound
    /// output `audio` (already loaded with the mod's sounds). Registers every
    /// image (scene, each worm's skin and gibs, entities); the caller's
    /// `Presentation::upload` sends them to the renderer. An online match
    /// starts with [`Session::online`].
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
            Game::Online { start, .. } => start.seed,
        };
        Ok(Session::assemble(a, world, driver, seed, viewport, interpolation, audio))
    }

    /// The online match `a` describes (`setup::load_online`), played by
    /// `driver`'s peer, which has just started it: its world is the
    /// peer's. The peer's events polled after its `MatchStarting` go to
    /// [`Session::present_events`]; from then on [`Session::frame`] polls
    /// it.
    pub fn online(
        a: &Assets,
        driver: OnlineDriver,
        viewport: Viewport,
        interpolation: Interpolation,
        audio: Audio,
    ) -> anyhow::Result<Session> {
        Session::try_online(a, driver, viewport, interpolation, audio).map_err(|(e, _)| e)
    }

    /// [`Session::online`], the driver handed back with the error (its
    /// peer still has to say goodbye).
    pub fn try_online(
        a: &Assets,
        driver: OnlineDriver,
        viewport: Viewport,
        interpolation: Interpolation,
        audio: Audio,
    ) -> Result<Session, (anyhow::Error, Box<OnlineDriver>)> {
        let checked = driver.handle().with(|peer| {
            let Game::Online { start, my_slot } = &a.game else { anyhow::bail!("not an online match") };
            let s = peer.match_session().ok_or_else(|| anyhow::anyhow!("the peer plays no match"))?;
            let world = s.sim().world();
            anyhow::ensure!(world.worms().len() == start.slots.len(), "the match's worms are not its slots");
            anyhow::ensure!(peer.my_slot() == *my_slot, "the peer plays another slot");
            Ok((world.clone(), start.seed, u64::from(s.current_tick())))
        });
        let (world, seed, ticks) = match checked {
            Ok(c) => c,
            Err(e) => return Err((e, Box::new(driver))),
        };
        let mut s = Session::assemble(
            a,
            world,
            Driver::Online(Box::new(driver)),
            seed,
            viewport,
            interpolation,
            audio,
        );
        s.ticks = ticks;
        Ok(s)
    }

    /// The session of `world` played by `driver`, its presentation made
    /// and every worm registered with its look.
    fn assemble(
        a: &Assets,
        world: World,
        driver: Driver,
        seed: u32,
        viewport: Viewport,
        interpolation: Interpolation,
        audio: Audio,
    ) -> Session {
        let setup = PresentSetup {
            core: a.content.core(),
            m: &a.m,
            level: &a.level,
            theme: &a.theme,
            viewport,
            options: SceneOptions { interpolation, show_health_bars: false },
            seed,
        };
        let mut present = Presentation::new(&setup, &world);
        let local = a.local_slot();
        let throws = ThrowSounds::new(hears_throws_of(a), &world);
        present.follow(i32::from(a.first_followed()));
        let n = world.worms().len();
        let looks = looks(a, n.max(a.worms()));
        let mut s = Session {
            names: Names(looks.iter().map(|l| l.name.clone()).collect()),
            time_left_ms: mode_time_left(&world),
            world,
            present,
            events: TickEvents::new(),
            audio,
            popups: Vec::with_capacity(64),
            driver,
            log: Log::default(),
            results: None,
            tally: Tally::default(),
            player_deaths: 0,
            player_spawns: 0,
            rematches: 0,
            human: a.human_plays(),
            local,
            frozen: false,
            looks,
            sound: true,
            heard: Vec::new(),
            throws,
            keys: vec![Keys(0); n],
            ticks: 0,
            ended_at: None,
            weapon_name_until: None,
            shown: 0,
            default_skin: a.bot_skin.clone(),
        };
        s.show_new_worms();
        s
    }

    /// Registers the worms the presentation does not know yet (all at the
    /// start; a worm a mode spawns mid-match later, in a default look,
    /// which marks the atlas for upload). The local slot's worm has the
    /// crosshair; with none, a local game's worm 0 has it (a replay's,
    /// bots alone: OLX draws the crosshair of its local worms), an online
    /// match watched's none.
    fn show_new_worms(&mut self) {
        let n = self.with_world(|w| w.worms().len());
        let online = matches!(self.driver, Driver::Online(_));
        let crosshair = self.local.or(if online { None } else { Some(0) });
        while self.shown < n {
            let i = self.shown;
            if self.looks.len() <= i {
                let skin = self.default_skin.clone();
                self.looks.push(WormLook {
                    name: format!("Worm {}", i + 1),
                    colour: rgb(default_colour(i)),
                    skin,
                });
            }
            let look = &self.looks[i];
            let [r, g, b] = look.colour;
            let mine = self.local.is_some_and(|l| usize::from(l) == i);
            let aims = crosshair.is_some_and(|l| usize::from(l) == i);
            self.present.add_worm(&look.skin, olr_data::image::Rgba::rgb(r, g, b), aims, mine);
            self.shown += 1;
        }
        if self.keys.len() < self.shown {
            self.keys.resize(self.shown, Keys(0));
        }
        while self.names.0.len() < self.shown {
            let i = self.names.0.len();
            self.names.0.push(self.looks[i].name.clone());
        }
    }

    /// `f` with the world drawn: the session's, or an online match's
    /// newest predicted one (its peer's, under the peer's lock: `f` must
    /// not use the session's peer, nor call `with_world` again) until the
    /// match ended there, then the one presented last, kept
    /// ([`Session::present_events`]).
    pub fn with_world<R>(&self, f: impl FnOnce(&World) -> R) -> R {
        with_drawn(&self.driver, &self.world, self.frozen, f)
    }

    /// The scores the HUD shows: the world's, an online match's after its
    /// last confirmed tick (spec §3.4).
    pub fn scores(&self) -> &[Score] {
        match &self.driver {
            Driver::Online(d) => d.confirmed_scores(),
            _ => self.world.scores(),
        }
    }

    /// The `Sound` events the last tick or poll played, or would have
    /// played with [`Session::sound`] off.
    pub fn sounds(&self) -> &[Event] {
        &self.heard
    }

    /// The slot this player controls; `None` when nobody here plays (a
    /// replay, bots alone, an online match watched).
    pub fn local(&self) -> Option<u8> {
        self.local
    }

    /// The worm the camera and the HUD follow: the local slot's, else the
    /// one chosen ([`Session::follow`]; worm 0 at first).
    pub fn followed(&self) -> i32 {
        self.present.followed()
    }

    /// The player's worm's current weapon index, while it is alive.
    fn local_weapon(&self) -> Option<i32> {
        let i = usize::from(self.local?);
        self.with_world(|world| world.worms().get(i).filter(|w| w.alive).map(|w| w.current_weapon))
    }

    /// After a tick (or an online frame's ticks) whose player keys were
    /// `keys`: a weapon change made with the change key held (the
    /// player's worm, alive before and after, no longer holds weapon
    /// `before`) shows its name for [`WEAPON_NAME_TICKS`] (OLX sets
    /// `bForceWeapon_Name` when SelectWeapon and left or right change the
    /// weapon).
    ///
    /// OLX: src/common/CWormHuman.cpp:405-428
    fn note_weapon_change(&mut self, before: Option<i32>, keys: Keys) {
        let now = self.local_weapon();
        // A worm that (re)spawned this tick did not change weapons.
        if keys.0 & Keys::CHANGE != 0 && before.is_some() && now.is_some() && now != before {
            self.weapon_name_until = Some(self.ticks + WEAPON_NAME_TICKS);
        }
    }

    /// The weapon name over the player's worm (OLX's `CWorm::Draw`): the
    /// worm and its current weapon's name while `held` (the change key,
    /// as the input holds it now) or for [`WEAPON_NAME_TICKS`] after a
    /// change made with it. Only the player's own worm (OLX: a local
    /// human worm), alive and with a weapon; `None` otherwise (a replay,
    /// bots alone, an online match watched).
    ///
    /// OLX: src/common/CWorm.cpp:1304-1315, src/gusanos/base_worm.cpp:678-684
    pub fn weapon_name(&self, held: bool) -> Option<(usize, String)> {
        let i = usize::from(self.local?);
        let forced = self.weapon_name_until.is_some_and(|until| self.ticks <= until);
        if !(held || forced) {
            return None;
        }
        self.with_world(|world| {
            let w = world.worms().get(i).filter(|w| w.alive && !w.slots.is_empty())?;
            let weapon = world.sim_mod().weapon(w.current_slot().weapon)?;
            Some((i, weapon.name.clone()))
        })
    }

    /// Whether nobody here plays this game (a replay, bots alone, an
    /// online match watched): the arrows choose the worm followed
    /// ([`Session::follow_next`]).
    pub fn spectating(&self) -> bool {
        self.local.is_none()
    }

    /// The camera and the HUD follow worm `slot`.
    pub fn follow(&mut self, slot: u8) {
        self.present.follow(i32::from(slot));
    }

    /// A watcher's next (`step` 1) or previous (-1) worm to follow, in
    /// worm order, round, skipping worms that left (`World::gone`); the
    /// player's camera stays on its own worm, and a watcher stays where it
    /// is when every worm is gone.
    pub fn follow_next(&mut self, step: i32) {
        let (n, gone) = self.with_world(|w| (w.worms().len() as i64, w.gone()));
        if self.local.is_some() || n == 0 {
            return;
        }
        let from = i64::from(self.followed());
        let step = if step < 0 { -1 } else { 1 };
        for k in 1..=n {
            let next = (from + step * k).rem_euclid(n);
            if gone.checked_shr(next as u32).unwrap_or(0) & 1 == 0 {
                self.present.follow(next as i32);
                return;
            }
        }
    }

    /// The Deathmatch's kill limit, if on.
    pub fn kill_limit(&self) -> Option<i32> {
        let k = self.with_world(|w| {
            Some(w.mode_state().as_any().downcast_ref::<Deathmatch>()?.settings().kill_limit)
        })?;
        (k > 0).then_some(k)
    }

    /// Worm `i`'s colour (black for one not shown yet).
    pub fn colour(&self, i: usize) -> [u8; 3] {
        self.looks.get(i).filter(|_| i < self.shown).map_or([0; 3], |l| l.colour)
    }

    /// Every worm's look, by index.
    pub fn looks(&self) -> &[WormLook] {
        &self.looks
    }

    /// F5: the sandbox or replay from the start, or a Deathmatch's rematch
    /// (the next seed), from the assets loaded once. The worms are the
    /// same, so the atlas stays; the next upload sends the new colour map.
    /// Gives the replay of the match that was playing (a Deathmatch's).
    pub fn restart(&mut self, a: &Assets) -> anyhow::Result<Option<Replay>> {
        let n = if matches!(a.game, Game::Deathmatch { .. }) { self.rematches + 1 } else { 0 };
        self.reset(a, n)
    }

    /// The same match from its start again, built from `a` as it is now:
    /// after the weapon selection changed its options (no tick played yet,
    /// so nothing is lost; the recording starts again with the weapons).
    pub fn rebuild(&mut self, a: &Assets) -> anyhow::Result<()> {
        self.reset(a, self.rematches).map(drop)
    }

    /// The world of `a`'s game, rematch `n`, from its start.
    fn reset(&mut self, a: &Assets, n: u32) -> anyhow::Result<Option<Replay>> {
        let (world, driver) = start(a, n)?;
        anyhow::ensure!(world.worms().len() == self.shown, "the restarted match has other worms");
        let old = self.take_replay();
        self.human = a.human_plays();
        self.local = a.local_slot();
        self.rematches = n;
        self.world = world;
        self.driver = driver;
        self.throws = ThrowSounds::new(hears_throws_of(a), &self.world);
        self.present.restart(&self.world, &a.level, &a.theme);
        self.audio.stop_all();
        self.popups.clear();
        self.log.clear();
        self.results = None;
        self.ended_at = None;
        self.time_left_ms = mode_time_left(&self.world);
        self.keys.fill(Keys(0));
        self.weapon_name_until = None;
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

    /// The replay of the match playing (a Deathmatch's), finished now if
    /// it was still recording; it stays to be taken (`take_replay`).
    pub fn replay(&mut self) -> Option<&Replay> {
        match &mut self.driver {
            Driver::Match(d) => d.replay(&self.world),
            _ => None,
        }
    }

    /// Whether a person plays (an online match counts, watched too: it
    /// runs at the host's pace); a game nobody plays (a replay, bots
    /// alone) may run fast (`speed.rs`).
    pub fn human(&self) -> bool {
        self.human
    }

    /// Ticks since the match (re)started (an online match's: its newest
    /// predicted tick).
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// The results show: `RESULTS_AFTER` ticks after the end (OLX's
    /// `GAMEOVER_WAIT`; "Game Over" until then), or a replay's at its end.
    pub fn results_shown(&self) -> bool {
        let replay_done = matches!(&self.driver, Driver::Replay(d) if d.is_done());
        self.results.is_some()
            && (replay_done || self.since_end().is_some_and(|t| t > crate::hud::RESULTS_AFTER))
    }

    /// Ticks since the match ended, once it has.
    pub fn since_end(&self) -> Option<u64> {
        self.ended_at.map(|t| self.ticks.saturating_sub(t))
    }

    /// A window of another shape: the camera's viewport follows.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.present.set_viewport(viewport);
    }

    /// One 100 Hz tick with the player (the local slot) holding `player`
    /// (the sandbox's targets hold nothing; a Deathmatch's bots think
    /// first; bots alone and a replay ignore `player`; a replay plays its
    /// keys and stops at its end; an online match never ticks here:
    /// [`Session::frame`]): the simulation, then the cosmetic entities
    /// (which replay the tick's terrain edits on the colour map and
    /// rebuild any area the tick lost), the sounds at the view centre
    /// (unless [`Session::sound`] is off), the scene's history and marks,
    /// the damage popups and the HUD. Debug builds check every
    /// [`DRIFT_CHECK_TICKS`] that the colour terrain has not drifted.
    /// Whether the world ticked (not after a replay's end, nor online).
    pub fn tick(&mut self, player: Keys) -> bool {
        let ended = self.since_end();
        let weapon = self.local_weapon();
        if let Some(k) = self.local.and_then(|i| self.keys.get_mut(usize::from(i))) {
            *k = player;
        }
        match &mut self.driver {
            Driver::Sandbox => self.world.tick(&self.keys, &mut self.events),
            Driver::Match(d) => {
                d.think(&self.world, &self.events, &mut self.keys, ended);
                self.world.tick(&self.keys, &mut self.events);
                d.record(&self.keys, &self.world);
            }
            Driver::Replay(d) => {
                if !d.step(&mut self.world, &mut self.events) {
                    return false;
                }
            }
            Driver::Online(_) => return false,
        }
        self.show_new_worms();
        self.heard.clear();
        self.heard.extend(self.events.iter().filter(|e| matches!(e.kind, EventKind::Sound { .. })));
        self.throws.retain(&self.world, &mut self.heard);
        if self.sound {
            self.audio.play(&self.heard, self.present.scene.camera.listener());
        }
        self.present.on_tick(&self.world, &self.events);
        self.ticks += 1;
        self.note_weapon_change(weapon, player);
        if cfg!(any(debug_assertions, test)) && self.ticks.is_multiple_of(DRIFT_CHECK_TICKS) {
            assert!(
                !self.present.terrain_drift(&self.world),
                "tick {}: the colour terrain's material is not the world's (a terrain change without its TerrainEdit?)",
                self.ticks
            );
        }
        self.after_tick(None);
        true
    }

    /// A tick's bookkeeping: the clock, the tally, the event log, the
    /// end and its results, the damage popups. A local tick's (`None`:
    /// the world and its events), or an online match's confirmed tick.
    fn after_tick(&mut self, confirmed: Option<&ConfirmedTick>) {
        let followed = self.present.followed();
        let Session {
            world,
            events,
            driver,
            names,
            log,
            popups,
            tally,
            results,
            ended_at,
            time_left_ms,
            player_deaths,
            player_spawns,
            local,
            ticks,
            ..
        } = self;
        let v = match confirmed {
            Some(c) => TickView {
                played: u64::from(c.tick) + 1,
                events: &c.events,
                scores: &c.scores,
                time_left_ms: c.time_left_ms,
                result: c.result,
                positions: Positions::Listed(&c.positions),
            },
            None => TickView {
                played: *ticks,
                events: events.as_slice(),
                scores: world.scores(),
                time_left_ms: mode_time_left(world),
                result: world.result(),
                positions: Positions::World(world),
            },
        };
        popups.retain_mut(|p| {
            p.age += 1;
            p.age < POPUP_TICKS
        });
        // The mode's clock stops at the game over (`Deathmatch::time_left_ms`).
        *time_left_ms = v.time_left_ms;
        if results.is_none() {
            tally.match_ticks += 1;
        }
        let me = local.map(i32::from);
        let bot = |w: i32| match &*driver {
            Driver::Match(d) => d.bots.iter().any(|b| b.worm() as i32 == w),
            Driver::Online(d) => d.is_bot(w),
            _ => false,
        };
        for e in v.events {
            match e.kind {
                EventKind::Kill { victim, killer } => {
                    if Some(victim) == me {
                        *player_deaths += 1;
                    }
                    if killer == victim {
                        tally.suicides += 1;
                    } else if bot(killer) {
                        tally.bot_kills += 1;
                    } else if Some(killer) == me {
                        tally.player_kills += 1;
                    }
                }
                EventKind::Spawned { worm } if Some(worm) == me => *player_spawns += 1,
                EventKind::Message(m) => {
                    if let Some(line) = message_text(&m, names) {
                        log.push(LogLine::event(line));
                    }
                }
                EventKind::GameOver(_) => {
                    *ended_at = Some(v.played);
                    tally.matches_ended += 1;
                    *results =
                        v.result.map(|r| Results::new(r, v.scores, names, *time_left_ms, Some(followed)));
                }
                _ => {}
            }
            let EventKind::Damage { worm, dealt, .. } = e.kind else { continue };
            let Some(pos) = v.positions.of(worm) else { continue };
            if dealt <= 0.0 {
                continue;
            }
            // Hits in quick succession sum into one number, as OLX's do.
            if let Some(p) = popups.iter_mut().find(|p| p.worm == worm && p.age < MERGE_TICKS) {
                p.amount += dealt;
                p.pos = pos;
                p.age = 0;
            } else if popups.len() < popups.capacity() {
                popups.push(Popup { worm, amount: dealt, pos, age: 0 });
            }
        }
        // A watcher's camera moves on from a worm that left (N89).
        let f = self.followed();
        if self.local.is_none()
            && f >= 0
            && self.with_world(|w| w.gone().checked_shr(f as u32).unwrap_or(0) & 1 != 0)
        {
            self.follow_next(1);
        }
    }

    /// One frame of an online match at `now` (`Driver::Online`; nothing
    /// otherwise): the peer polled once (the local player's keys from
    /// `local_keys`, called once per new tick: the input latch), its
    /// events presented (`online.rs`; sounds at the camera unless
    /// [`Session::sound`] is off) and its confirmed ticks kept as a local
    /// tick's (the log, the tally, the scoreboard, the results, the
    /// popups). Returns the events that are not the match's presentation
    /// (the lobby's, chat, the match's end, the connection's) for the
    /// shell.
    ///
    /// The poll is the peer's handle's (`net::pump`): the pump's events
    /// queued since the UI's last poll come first, and when the pump ran
    /// ticks the presentation starts again from the world
    /// ([`PolledEvents::pumped`], handed back with the shell's events: a
    /// next match's session starts again too).
    pub fn frame(&mut self, now: Instant, local_keys: &mut dyn FnMut() -> Keys) -> PolledEvents {
        let weapon = self.local_weapon();
        let Driver::Online(d) = &mut self.driver else { return PolledEvents::default() };
        let mut pressed = Keys(0);
        let mut keys = || {
            let k = local_keys();
            pressed.0 |= k.0;
            k
        };
        let PolledEvents { events, pumped } = d.poll(now, &mut keys, &mut self.present);
        let rest = self.present_polled(events, pumped);
        self.note_weapon_change(weapon, pressed);
        PolledEvents { events: rest, pumped }
    }

    /// An online match's polled `events` presented, as [`Session::frame`]
    /// does (the shell's for the events of the poll that started the
    /// match, after its `MatchStarting`); the others handed back.
    /// A next match's `MatchStarting` and what follows it are not this
    /// match's: they are handed back as they are (the shell starts that
    /// match's session with them).
    pub fn present_events(&mut self, events: Vec<OnlineEvent>) -> Vec<OnlineEvent> {
        self.present_polled(events, None)
    }

    /// [`Session::present_events`] of a poll whose `pumped` says the pump
    /// ran ticks before it (`PolledEvents::pumped`): the presentation
    /// starts again from the world, as after a loaded state, then the
    /// poll's ticks follow (`OnlineDriver::present`).
    pub fn present_polled(&mut self, mut events: Vec<OnlineEvent>, pumped: Option<u32>) -> Vec<OnlineEvent> {
        let next = match events.iter().position(|e| matches!(e, OnlineEvent::MatchStarting { .. })) {
            Some(i) => events.split_off(i),
            None => Vec::new(),
        };
        let Driver::Online(d) = &mut self.driver else {
            events.extend(next);
            return events;
        };
        let (sound, audio, heard) = (self.sound, &mut self.audio, &mut self.heard);
        heard.clear();
        let mut play = |sounds: &[Event], p: &Presentation| {
            heard.extend_from_slice(sounds);
            if sound {
                audio.play(sounds, p.scene.camera.listener());
            }
        };
        let shown = d.present(events, pumped, self.frozen, &mut self.present, &mut self.throws, &mut play);
        let (confirmed, rest, resynced) = (shown.confirmed, shown.rest, shown.resynced);
        if let Some(t) = shown.current_tick {
            self.ticks = u64::from(t);
        }
        self.show_new_worms();
        for c in &confirmed {
            self.after_tick(Some(c));
        }
        // At the end the peer stops presenting (its session goes on to
        // confirm and drain): the world presented last is drawn from then
        // on, taken once, until the shell leaves the match. So too for a
        // player back after a migration's resume (a late rejoiner, plan 12c
        // N72): its peer dropped the match it played, and it watches once
        // the next start comes (the match drawn as it last was meanwhile,
        // never its start world).
        let ended = rest.iter().any(|e| matches!(e, OnlineEvent::MatchEnded(_)));
        let late = rest.iter().any(|e| matches!(e, OnlineEvent::Migrated { me: false, .. }));
        if !self.frozen
            && (ended || late)
            && let Driver::Online(d) = &mut self.driver
            && let Some(taken) = d.handle().with(|p| {
                if ended || p.match_session().is_none() { Some(p.take_ended_world()) } else { None }
            })
        {
            if let Some(w) = taken {
                self.world = w;
            }
            self.frozen = true;
            if resynced || pumped.is_some() {
                // The pump ran ticks this presentation never saw: the
                // world drawn from now on gets its own colour terrain.
                let t = self.world.terrain();
                let whole = Area { x0: 0, y0: 0, x1: t.width() as i32, y1: t.height() as i32 };
                self.present.state_loaded(&self.world);
                self.present.color_mut().rebuild_area(whole, self.world.terrain());
            }
        }
        let mut rest = rest;
        rest.extend(next);
        rest
    }

    /// The new ticks an online match's last poll ran itself (its pump's
    /// excepted): the F2 meter's count; 0 for a local game.
    pub fn polled_ticks(&self) -> u32 {
        match &self.driver {
            Driver::Online(d) => d.polled_ticks(),
            _ => 0,
        }
    }

    /// An online match's alpha for the frame at `now` (`OnlineDriver::alpha`;
    /// 1 once the match ended here: its world is frozen); `None` for a
    /// local game (the app's tick clock gives its alpha).
    pub fn online_alpha(&self, now: Instant) -> Option<f32> {
        match &self.driver {
            Driver::Online(_) if self.frozen => Some(1.0),
            Driver::Online(d) => Some(d.alpha(now)),
            _ => None,
        }
    }

    /// One drawn frame at `alpha` between the last two ticks: the scene,
    /// then the cosmetic entities.
    pub fn build(&mut self, alpha: f32, list: &mut DrawList) {
        let Session { driver, world, frozen, present, .. } = self;
        with_drawn(driver, world, *frozen, |w| present.build(w, alpha, list));
    }
}

/// `f` with the world a session draws: its own, or an online match's
/// peer's (under its lock) while it plays and the session is not `frozen`
/// (the match over there; its own, the one presented last, then, or its
/// start if the peer has none).
fn with_drawn<R>(driver: &Driver, world: &World, frozen: bool, f: impl FnOnce(&World) -> R) -> R {
    match driver {
        Driver::Online(d) if !frozen => d.with_world(|w| f(w.unwrap_or(world))),
        _ => f(world),
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
        let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| std::path::Path::new(d).is_dir())
        else {
            eprintln!("OLR_CONTENT_DIR not set; skipping");
            return;
        };
        let o = SandboxOptions {
            mod_id: String::new(),
            level_id: String::new(),
            seed: 4,
            targets: 2,
            bonuses: true,
        };
        let a = load(&[std::path::Path::new(&dir)], "MW 1.0", "Dirt Level", "default", o).unwrap();
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

    /// Kill credit, deaths and spawns go by the local slot (2 here, two
    /// bots before it), never worm 0: each event counted exactly once.
    #[test]
    fn credit_goes_by_the_local_slot() {
        let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| std::path::Path::new(d).is_dir())
        else {
            eprintln!("OLR_CONTENT_DIR not set; skipping");
            return;
        };
        let bot = olr_content::Player::Bot(olr_bots::BotConfig::default());
        let players = vec![bot, bot, olr_content::Player::Human];
        let o = olr_content::MatchOptions::deathmatch("olx-classic", "olx-level-dirt-level", 3, players);
        let a = crate::setup::load_match(&[std::path::Path::new(&dir)], &o, "default").unwrap();
        let vp = Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
        let mut s = Session::new(&a, vp, Interpolation::Linear, Audio::silent()).unwrap();
        assert_eq!(s.local(), Some(2));
        let kind = |kind| Event { substep: 0, death: false, kind };
        let c = ConfirmedTick {
            tick: 0,
            events: vec![
                kind(EventKind::Kill { victim: 0, killer: 2 }),
                kind(EventKind::Kill { victim: 2, killer: 1 }),
                kind(EventKind::Spawned { worm: 2 }),
                kind(EventKind::Kill { victim: 1, killer: 0 }),
                kind(EventKind::Kill { victim: 1, killer: 1 }),
                kind(EventKind::Spawned { worm: 0 }),
            ],
            scores: s.world.scores().to_vec(),
            time_left_ms: None,
            result: None,
            positions: Vec::new(),
        };
        s.after_tick(Some(&c));
        let t = &s.tally;
        assert_eq!((t.player_kills, t.bot_kills, t.suicides), (1, 2, 1), "kills by slot");
        assert_eq!((s.player_deaths, s.player_spawns), (1, 1), "the local slot's death and spawn");
    }

    /// Debug builds catch a colour terrain that drifted from the world's
    /// material (a future terrain change without its `TerrainEdit`).
    #[test]
    fn session_catches_terrain_drift() {
        let Some(dir) = std::env::var_os("OLR_CONTENT_DIR").filter(|d| std::path::Path::new(d).is_dir())
        else {
            eprintln!("OLR_CONTENT_DIR not set; skipping");
            return;
        };
        let o = SandboxOptions {
            mod_id: String::new(),
            level_id: String::new(),
            seed: 4,
            targets: 1,
            bonuses: false,
        };
        let a = load(&[std::path::Path::new(&dir)], "Classic", "Liero Factory", "default", o).unwrap();
        let vp = Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
        let mut s = Session::new(&a, vp, Interpolation::Linear, Audio::silent()).unwrap();
        for _ in 0..DRIFT_CHECK_TICKS {
            s.tick(Keys(Keys::FIRE));
        }
        // Dirt where the player stands, which the world never placed.
        let at = s.world.worms()[0].pos;
        s.present.color_mut().apply(olr_terrain::ColorEdit::Dirt { hole: 0, x: at.x as i32, y: at.y as i32 });
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
