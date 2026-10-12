//! The menus driving the game (spec §21 G1-G2), without a window: the
//! `rl_ui::UiState`, its widgets' data, the settings and profiles files,
//! and the game playing. `app.rs` feeds it the widgets' actions and does
//! what comes back ([`Outcome`]); everything else happens here, so it is
//! tested headless (`tests/menu.rs`).
//!
//! A start (local play's Start, the sandbox, a replay) is loaded by
//! [`Shell::poll`] after the frame that asked for it, so the menus show
//! "Loading…" meanwhile; until then `UiState` ignores another start, Back
//! and the menu buttons (plan 7 Review Focus 4). A start that fails (a
//! pack missing or broken, a file that is no replay) returns to the
//! screen it came from with the reason (Review Focus 3). A Deathmatch
//! starts with OLX's weapon selection over its level: the world waits (no
//! tick) until Done arms the player (`Player::HumanWith` in the match's
//! options, the world built again with them, so the replay holds them).
//! The pause menu and the selection stop the ticks ([`Shell::tick`]):
//! replays record ticks only, so a pause never shows in one. Every
//! Deathmatch's replay is saved in the data directory's `replays/` when it
//! ends, is restarted or the game quits (`rl_ui::replays::auto_save`); the
//! results' Save replay keeps it under a name of its own
//! (`rl_ui::replays::save_kept`).
//!
//! The menus list and play the bundled content (`--content`) with the
//! user data's `packs/` layered over it (spec §9.4: an imported pack
//! replaces the bundled one of its id). The Import screen's import runs
//! on its own thread (`rl_ui::import::ImportJob`), polled each frame by
//! [`Shell::poll`]; when it ends the content is listed again, so new and
//! re-imported packs show (their thumbnails made again) without a restart.
//! No game starts while it runs (`UiState` refuses), and a game playing
//! keeps the packs it loaded.
//!
//! **Online** (plan 10 task 12, `net.rs`): `HostGame` and `JoinGame` make
//! the `rl_online::OnlinePeer` (the identity from `Store::load_identity`,
//! the look from the profile playing, the content from the shell's roots);
//! [`Shell::net_frame`] polls it every frame (the app's loop, also while
//! the window is hidden: decision N21), through the match's session while
//! one plays (`Session::frame`), and turns its events into the menus'
//! data (the lobby, the chat, the LAN list) and their actions
//! (`LobbyJoined`, `OnlineFailed`, `MatchStarted`, `MatchOver`).
//! `MatchStarting` loads the match's assets and starts a session with
//! `Driver::Online` (the events after it in that poll are the new
//! session's); `EndMatch` gives the peer back to the lobby; a lost
//! connection, a kick or a refusal leaves the match view at once (the
//! peer dropped its match) and returns to Multiplayer; `ReplayReady` is
//! saved with the automatic replays and `DesyncBundle` in `desyncs/`.

use std::collections::{BTreeSet, VecDeque};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{ColorImage, TextureHandle, TextureOptions};
use rl_audio::Audio;
use rl_content::{Content, LoadedSkin, MatchOptions, Player};
use rl_input::Bindings;
use rl_lobby::{HostConfig, Identity};
use rl_net::endpoint::EndpointConfig;
use rl_net::lan::{Announcer, Listener, broadcast_targets};
use rl_netcode::netsim::Profile;
use rl_online::{OnlineEvent, OnlinePeer, PeerRole};
use rl_proto::{LAN_PORT, LobbyState, Look, MatchStartWire};
use rl_scene::{Interpolation, Viewport};
use rl_sim::{Keys, MAX_SLOTS};
use rl_ui::hud::{LineKind, LogLine};
use rl_ui::import::{ImportJob, ImportProgress};
use rl_ui::screens::MenuData;
use rl_ui::skin_preview::{self, PreviewKey, PreviewTextures};
use rl_ui::weapon_select::{WeaponMenu, WeaponSelect};
use rl_ui::{
    AppCommand, AudioSettings, HostPrefs, LobbyCommand, Password, Profiles, Screen, Settings, Store,
    UiAction, UiState, VideoSettings, Visibility,
};

use crate::net::{self, Net, NetView};
use crate::online::OnlineDriver;
use crate::session::{Driver, Session};
use crate::setup::{self, Assets, DEFAULT_SKIN, Game, PlayerLook, SandboxOptions};
use crate::video::{self, Overrides};

/// What the shell needs besides the menus' state.
#[derive(Clone, Debug)]
pub struct ShellConfig {
    /// The content directory (`--content`).
    pub content: PathBuf,
    pub interpolation: Interpolation,
    /// The window's view (`app::viewport`).
    pub viewport: Viewport,
}

/// A game playing: what it was loaded from, and its session.
pub struct Playing {
    pub assets: Assets,
    pub session: Session,
}

/// What the window must do.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Quit,
    /// The video options changed (window size, fullscreen, UI scale).
    Video(VideoSettings),
    /// The player's keys changed.
    Bindings(Bindings),
    /// A new game plays: its images go to the renderer, the clock starts
    /// afresh (the loading time is not caught up).
    Started,
}

/// What to load.
enum Launch {
    Match(Box<MatchOptions>),
    Sandbox,
    Replay(PathBuf),
}

/// The sandbox's idle targets when started from the menu.
pub const SANDBOX_TARGETS: usize = 3;

pub struct Shell {
    pub ui: UiState,
    pub data: MenuData,
    /// The game playing, if any.
    pub play: Option<Playing>,
    cfg: ShellConfig,
    store: Store,
    /// The sound output while no session holds it.
    audio: Option<Audio>,
    pending: Option<Launch>,
    paused: bool,
    /// `MatchOver` was reported for the game playing.
    over_reported: bool,
    launches: u32,
    unsaved_settings: Option<Settings>,
    unsaved_profiles: Option<Profiles>,
    /// Warnings logged once.
    logged: BTreeSet<String>,
    /// The content the menus list (the skin preview loads its skins).
    content: Option<Content>,
    /// The skin preview: the skin loaded last (by the id asked for) and
    /// the textures shown (dropping them frees them).
    preview_skin: Option<(String, LoadedSkin)>,
    preview_textures: Vec<TextureHandle>,
    /// The command line's video flags over the saved options, and the
    /// options as loaded (`video::Overrides`).
    overrides: Option<(VideoSettings, Overrides)>,
    /// The Import screen's import while it runs.
    import: Option<ImportJob>,
    /// The online game (its lobby and match), if any.
    net: Option<Net>,
    /// `--net-sim`'s host, in this process (spec §9.10).
    sim_host: Option<OnlinePeer>,
    /// The key this install plays online with (loaded once).
    identity: Option<Identity>,
    /// LAN announcements and the LAN list (tests turn them off: fixed port).
    lan: bool,
    /// The LAN list's listener while the Multiplayer screen shows.
    lan_listener: Option<Listener>,
    /// The listener could not be made (the port taken): tried again when
    /// the screen opens again.
    lan_failed: bool,
    /// The port to host on and join at instead of the options' (`--port`;
    /// tests host on 0).
    port: Option<u16>,
    /// Every link impaired with this profile (`--net-sim`).
    impair: Option<Profile>,
}

/// `--net-sim`'s bots (spec §9.10: a host with three bots).
pub const NET_SIM_BOTS: u8 = 3;
/// Spectators a hosted lobby takes (spec §7.2).
pub const MAX_SPECTATORS: u8 = 16;

/// How long quitting waits for an import to stop after its item.
pub const IMPORT_STOP_WAIT: Duration = Duration::from_secs(2);

impl Shell {
    /// The main menu over content directory `cfg.content` with the data
    /// directory's imported packs over it (listed for the menus now), with
    /// `audio` for every game.
    pub fn new(cfg: ShellConfig, ui: UiState, store: Store, audio: Audio) -> Shell {
        let mut data = MenuData::new(Default::default(), store.replays_dir());
        data.packs_dir = store.packs_dir();
        data.notices = store.notices();
        let mut shell = Shell {
            ui,
            data,
            play: None,
            cfg,
            store,
            audio: Some(audio),
            pending: None,
            paused: false,
            over_reported: false,
            launches: 0,
            unsaved_settings: None,
            unsaved_profiles: None,
            logged: BTreeSet::new(),
            content: None,
            preview_skin: None,
            preview_textures: Vec::new(),
            overrides: None,
            import: None,
            net: None,
            sim_host: None,
            identity: None,
            lan: true,
            lan_listener: None,
            lan_failed: false,
            port: None,
            impair: None,
        };
        shell.list_content();
        let audio = shell.ui.settings().audio.clone();
        shell.set_volume(&audio);
        shell
    }

    /// A game started without the menus (the command line's): `play`
    /// plays; nothing is saved.
    pub fn playing(cfg: ShellConfig, assets: Assets, session: Session) -> Shell {
        let ui = UiState::new(Settings::default(), Profiles::default(), 0);
        let data = MenuData::new(Default::default(), None);
        Shell {
            ui,
            data,
            play: Some(Playing { assets, session }),
            cfg,
            store: Store::in_memory(),
            audio: None,
            pending: None,
            paused: false,
            over_reported: false,
            launches: 1,
            unsaved_settings: None,
            unsaved_profiles: None,
            logged: BTreeSet::new(),
            content: None,
            preview_skin: None,
            preview_textures: Vec::new(),
            overrides: None,
            import: None,
            net: None,
            sim_host: None,
            identity: None,
            lan: false,
            lan_listener: None,
            lan_failed: false,
            port: None,
            impair: None,
        }
    }

    /// The content directories, highest priority first: the data
    /// directory's imported packs, then the bundled content.
    fn roots(&self) -> Vec<PathBuf> {
        self.data.packs_dir.iter().cloned().chain([self.cfg.content.clone()]).collect()
    }

    /// Lists the content for the menus (again, after an import): the
    /// catalog, its thumbnails made again (`MenuData::set_catalog`), the
    /// skin preview loaded again.
    fn list_content(&mut self) {
        let roots = self.roots();
        let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        self.content = match Content::open_layers(&roots) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("warning: {e:#}: no mods or levels to play");
                None
            }
        };
        let catalog = self.content.as_ref().map(Content::catalog).unwrap_or_default();
        self.log(self.content.as_ref().map(|c| c.warnings().to_vec()).unwrap_or_default());
        self.log(catalog.warnings.clone());
        self.data.set_catalog(catalog);
        self.preview_skin = None;
        self.data.preview = None;
        self.preview_textures.clear();
    }

    /// The import running, if any.
    pub fn importing(&self) -> bool {
        self.import.is_some()
    }

    /// The Import screen's Import: the job started on its own thread
    /// (one at a time; without a data directory, the reason shown).
    fn start_import(&mut self, path: PathBuf, out: &mut Vec<Outcome>) {
        if self.import.is_some() {
            return;
        }
        let Some(packs) = self.data.packs_dir.clone() else {
            self.data.import = ImportProgress::failed("there is no data directory to import into".into());
            for c in self.ui.update(UiAction::ImportFinished) {
                self.carry(c, out);
            }
            return;
        };
        let path = rl_ui::import::expand_typed_path(&path.to_string_lossy());
        println!("rl-game: importing {} into {}", path.display(), packs.display());
        self.data.import = ImportProgress::started();
        self.import = Some(ImportJob::spawn(path, packs));
    }

    /// The import's reports since the last frame; when it has ended, the
    /// content listed again and the menus told.
    fn poll_import(&mut self) -> Vec<Outcome> {
        let Some(job) = &mut self.import else { return Vec::new() };
        if !job.poll(&mut self.data.import) {
            return Vec::new();
        }
        self.import = None;
        match &self.data.import.result {
            Some(rl_ui::import::ImportResult::Failed(why)) => eprintln!("warning: import: {why}"),
            Some(r) => println!("rl-game: import done: {r:?}"),
            None => {}
        }
        self.list_content();
        self.act(UiAction::ImportFinished)
    }

    /// The profiles screen's skin preview (plan 7 ruling Q2: the menus
    /// hold no GPU handle, the app uploads): after the menus were drawn,
    /// the frames of the skin and colour they asked for
    /// (`MenuData::preview_wanted`) as egui textures, shown from the next
    /// frame; none while no screen asks.
    pub fn update_preview(&mut self, ctx: &egui::Context) {
        let wanted = self.data.preview_wanted.clone();
        if wanted.as_ref() == self.data.preview.as_ref().map(|t| &t.key) {
            return;
        }
        let Some(key) = wanted else {
            self.data.preview = None;
            self.preview_textures.clear();
            return;
        };
        let skin = self.preview_skin(&key);
        let frames = skin_preview::frames(&skin, key.colour);
        let mut textures = Vec::with_capacity(frames.len() * 2);
        let mut upload = |face, f: u32| {
            let img = frames.get(f, face);
            let size = [img.width() as usize, img.height() as usize];
            let image = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            let t = ctx.load_texture(format!("skin preview {f} {face:?}"), image, TextureOptions::NEAREST);
            let id = t.id();
            textures.push(t);
            id
        };
        let n = frames.len() as u32;
        let right = (0..n).map(|f| upload(rl_sim::Dir::Right, f)).collect();
        let left = (0..n).map(|f| upload(rl_sim::Dir::Left, f)).collect();
        self.preview_textures = textures;
        self.data.preview = Some(PreviewTextures { key, right, left });
    }

    /// The skin `key` names (as a match would load it: else the default
    /// skin, else a placeholder), loaded once while it stays the same.
    fn preview_skin(&mut self, key: &PreviewKey) -> LoadedSkin {
        if let Some((id, skin)) = &self.preview_skin
            && *id == key.skin_id
        {
            return skin.clone();
        }
        let skin = match &self.content {
            Some(c) => c.skin_or_default(&c.skin_id(&key.skin_id).unwrap_or_else(|| key.skin_id.clone())),
            None => LoadedSkin::placeholder(),
        };
        self.log(skin.warnings.clone());
        self.preview_skin = Some((key.skin_id.clone(), skin.clone()));
        skin
    }

    /// Games loaded so far (started or failed).
    pub fn launches(&self) -> u32 {
        self.launches
    }

    /// The pause menu holds the game.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// The weapon selection is open: the player's weapons are not chosen
    /// yet (the HUD shows none).
    pub fn selecting(&self) -> bool {
        self.play.is_some() && self.ui.screen() == Screen::WeaponSelect
    }

    /// Worm names show over the game: in game and not paused, so never
    /// through the weapon selection, the pause menu or the results.
    pub fn names_shown(&self) -> bool {
        self.play.is_some() && self.ui.screen() == Screen::InGame && !self.paused
    }

    pub fn session(&self) -> Option<&Session> {
        self.play.as_ref().map(|p| &p.session)
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        self.play.as_mut().map(|p| &mut p.session)
    }

    /// The window's view changed.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.cfg.viewport = viewport;
        if let Some(s) = self.session_mut() {
            s.set_viewport(viewport);
        }
    }

    /// The player did `a` (or the app reports it): the menus' commands
    /// carried out; what the window must do.
    pub fn act(&mut self, a: UiAction) -> Vec<Outcome> {
        let mut out = Vec::new();
        for c in self.ui.update(a) {
            self.carry(c, &mut out);
        }
        out
    }

    fn carry(&mut self, c: AppCommand, out: &mut Vec<Outcome>) {
        match c {
            AppCommand::StartMatch(o) => self.pending = Some(Launch::Match(Box::new(o))),
            AppCommand::StartSandbox => self.pending = Some(Launch::Sandbox),
            AppCommand::PlayReplay(path) => self.pending = Some(Launch::Replay(path)),
            AppCommand::EndMatch => self.end(),
            AppCommand::SetPaused(on) => self.paused = on,
            AppCommand::ArmPlayer(weapons) => self.arm(weapons),
            AppCommand::SaveReplay => {
                let saved = self.keep_replay();
                for c in self.ui.update(UiAction::ReplaySaved(saved)) {
                    self.carry(c, out);
                }
            }
            AppCommand::Restart => self.restart(),
            AppCommand::ApplyVideo(v) => {
                if let Some((_, o)) = &mut self.overrides {
                    o.follow(&v);
                }
                self.set_interpolation(video::interpolation(v.interpolation));
                out.push(Outcome::Video(v));
            }
            AppCommand::ApplyAudio(a) => self.set_volume(&a),
            AppCommand::ApplyBindings(b) => out.push(Outcome::Bindings(b)),
            AppCommand::SaveSettings(mut s) => {
                if let Some((file, o)) = &self.overrides {
                    o.unsaved(file, &mut s.video);
                }
                self.unsaved_settings = Some(s);
            }
            AppCommand::SaveProfiles(p) => self.unsaved_profiles = Some(p),
            AppCommand::StartImport(path) => self.start_import(path, out),
            AppCommand::CancelImport => {
                if let Some(job) = &self.import {
                    job.cancel();
                }
            }
            AppCommand::HostGame(prefs) => {
                let started = self.start_host(&prefs);
                self.connected(started.map(|()| true), out);
            }
            AppCommand::JoinGame { addr, password } => {
                let started = self.start_client(&addr, password);
                self.connected(started.map(|()| false), out);
            }
            AppCommand::Lobby(c) => self.lobby_command(c),
            AppCommand::LeaveOnline => self.leave_online(),
            AppCommand::Quit => out.push(Outcome::Quit),
        }
    }

    /// The import's progress (and, when it ends, the content listed
    /// again); then loads the game asked for in an earlier frame, if any,
    /// and reports to the menus whether it started.
    pub fn poll(&mut self) -> Vec<Outcome> {
        let mut out = self.poll_import();
        let Some(launch) = self.pending.take() else { return out };
        out.extend(self.launch_reported(launch));
        out
    }

    fn launch_reported(&mut self, launch: Launch) -> Vec<Outcome> {
        match self.launch(launch) {
            Ok(()) => {
                let mut out = self.act(UiAction::MatchStarted);
                // OLX skips the selection when there is no choice to make.
                if self.ui.screen() == Screen::WeaponSelect
                    && let Some(menu) = self.data.weapons.as_ref().filter(|m| !m.needs_choice())
                {
                    let weapons = WeaponSelect::new(menu, &[]).slots();
                    out.extend(self.act(UiAction::ChooseWeapons { weapons, names: Vec::new() }));
                }
                out.push(Outcome::Started);
                out
            }
            Err(e) => {
                let why = format!("{e:#}");
                eprintln!("warning: could not start: {why}");
                self.act(UiAction::MatchFailed(why))
            }
        }
    }

    /// Logs, once each, what the menus found wrong this frame (an
    /// unreadable level thumbnail).
    pub fn log_menu_warnings(&mut self) {
        let w = self.data.take_warnings();
        self.log(w);
    }

    fn log(&mut self, warnings: impl IntoIterator<Item = String>) {
        for w in warnings {
            if self.logged.insert(w.clone()) {
                eprintln!("warning: {w}");
            }
        }
    }

    fn launch(&mut self, launch: Launch) -> anyhow::Result<()> {
        self.launches += 1;
        let profile = self.ui.profiles().selected().cloned().unwrap_or_default();
        let roots = self.roots();
        let root: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        let mut assets = match launch {
            Launch::Match(o) => setup::load_match(&root, &o, &profile.skin_id)?,
            Launch::Sandbox => {
                let m = &self.ui.settings().gameplay.match_prefs;
                let o = SandboxOptions {
                    mod_id: String::new(),
                    level_id: String::new(),
                    seed: 1,
                    targets: SANDBOX_TARGETS,
                    bonuses: m.bonuses,
                };
                setup::load(&root, &m.mod_id, &m.level_id, &profile.skin_id, o)?
            }
            Launch::Replay(path) => setup::load_replay(&root, &path, &profile.skin_id)?,
        };
        assets.player = Some(PlayerLook { name: profile.name, colour: profile.colour });
        self.log(std::mem::take(&mut assets.warnings));
        let mut session = Session::new(&assets, self.cfg.viewport, self.cfg.interpolation, Audio::silent())?;
        let mut audio = self.audio.take().unwrap_or_else(Audio::silent);
        audio.stop_all();
        audio.load(&assets.m, assets.content.core());
        self.log(audio.take_warnings());
        session.audio = audio;
        self.data.weapons = weapon_menu(&assets);
        self.data.results = None;
        self.play = Some(Playing { assets, session });
        self.paused = false;
        self.over_reported = false;
        Ok(())
    }

    /// The weapon selection's choice: the human (the local slot) starts
    /// with `weapons` (none enabled: -1s, the random loadout stays). The
    /// world is built again from the options with them; no tick has
    /// played. Weapons the match cannot be built with leave its options as
    /// they were.
    fn arm(&mut self, weapons: [i32; MAX_SLOTS]) {
        let Some(p) = &mut self.play else { return };
        let Some(slot) = p.session.local().map(usize::from) else { return };
        let Game::Deathmatch { options, .. } = &mut p.assets.game else { return };
        if weapons.iter().any(|&w| w < 0) {
            return;
        }
        let Some(mine) = options.players.get_mut(slot) else { return };
        let before = std::mem::replace(mine, Player::HumanWith(weapons));
        if let Err(e) = p.session.rebuild(&p.assets) {
            // Options `build_match` refuses would fail every rematch too:
            // the match keeps the ones it was built with.
            eprintln!("warning: arming the player: {e:#}");
            if let Game::Deathmatch { options, .. } = &mut p.assets.game
                && let Some(mine) = options.players.get_mut(slot)
            {
                *mine = before;
            }
        }
    }

    /// The results' Save replay: the match's replay as a file of its own
    /// in the data directory's `replays/` (never removed automatically).
    fn keep_replay(&mut self) -> Result<String, String> {
        let dir = self.store.replays_dir().ok_or("there is no data directory")?;
        let r = self.play.as_mut().and_then(|p| p.session.replay()).ok_or("there is no replay")?;
        let path = rl_ui::replays::save_kept(&dir, &r.encode()).map_err(|e| e.to_string())?;
        println!("rl-game: kept replay {}", path.display());
        Ok(path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()))
    }

    /// One tick of the game playing, with the player holding `keys`,
    /// unless the pause menu or the weapon selection holds it (the
    /// replay records ticks only: a pause is not in it). Whether the
    /// world ticked (not held, nor a replay at its end).
    pub fn tick(&mut self, keys: Keys) -> bool {
        let held = self.paused || self.ui.screen() == Screen::WeaponSelect;
        self.session_mut().filter(|_| !held).is_some_and(|s| s.tick(keys))
    }

    /// The game ends: its replay saved, the sound output kept.
    fn end(&mut self) {
        if let Some(mut p) = self.play.take() {
            if let Some(r) = p.session.take_replay() {
                self.save_replay(&r);
            }
            // An online match's peer goes back to the lobby.
            if let Driver::Online(d) = std::mem::replace(&mut p.session.driver, Driver::Sandbox)
                && let Some(n) = &mut self.net
            {
                n.peer = Some(d.into_peer());
                n.lagging = false;
                n.meter.clear();
            }
            let mut audio = std::mem::replace(&mut p.session.audio, Audio::silent());
            audio.stop_all();
            self.audio = Some(audio);
        }
        // The lobby's weapon choice stays (its menu is the lobby's mod's).
        self.data.weapons = self.net.as_ref().and_then(|n| n.weapons.as_ref()).and_then(|w| w.1.clone());
        self.data.results = None;
        self.paused = false;
        self.over_reported = false;
    }

    /// F5: the game again in place (a Deathmatch's rematch, its replay saved).
    fn restart(&mut self) {
        let Some(p) = &mut self.play else { return };
        match p.session.restart(&p.assets) {
            Ok(Some(r)) => self.save_replay(&r),
            Ok(None) => {}
            Err(e) => eprintln!("warning: restarting: {e:#}"),
        }
        self.over_reported = false;
    }

    /// Saves `r` with the automatic replays (one of no tick is not kept).
    fn save_replay(&mut self, r: &rl_replay::Replay) {
        let Some(dir) = self.store.replays_dir().filter(|_| r.ticks() > 0) else { return };
        match rl_ui::replays::auto_save(&dir, &r.encode()) {
            Ok(path) => println!("rl-game: saved replay {} ({} ticks)", path.display(), r.ticks()),
            Err(e) => eprintln!("warning: saving the replay in {}: {e}", dir.display()),
        }
    }

    /// The command line's video flags (`overrides`) win over `file`, the
    /// options as loaded: the menus show them (the caller made the
    /// `UiState` with them applied), and the file keeps its own values
    /// until the player changes those options.
    pub fn set_overrides(&mut self, file: VideoSettings, overrides: Overrides) {
        self.overrides = Some((file, overrides));
    }

    /// How games are drawn between ticks (new games, and the one playing).
    pub fn interpolation(&self) -> Interpolation {
        self.cfg.interpolation
    }

    fn set_interpolation(&mut self, i: Interpolation) {
        self.cfg.interpolation = i;
        if let Some(s) = self.session_mut() {
            s.present.scene.options.interpolation = i;
        }
    }

    /// The sound output's volume now (the game's, else the menus').
    pub fn volume(&self) -> f32 {
        match (self.session(), &self.audio) {
            (Some(s), _) => s.audio.volume(),
            (None, Some(a)) => a.volume(),
            (None, None) => 0.0,
        }
    }

    /// The sound volume: master times effects (music has none yet).
    fn set_volume(&mut self, a: &AudioSettings) {
        let v = a.master * a.sfx;
        if let Some(audio) = &mut self.audio {
            audio.set_volume(v);
        }
        if let Some(s) = self.session_mut() {
            s.audio.set_volume(v);
        }
    }

    /// After the ticks of a frame: tells the menus once when the results
    /// show (`Session::results_shown`).
    pub fn after_ticks(&mut self) -> Vec<Outcome> {
        let shown = self.session().is_some_and(Session::results_shown);
        if !shown || self.over_reported {
            return Vec::new();
        }
        self.report_over()
    }

    /// The results to the menus (`MatchOver`), reported once they show
    /// (an online match goes on while its menu's options are open: then
    /// they show when the player is back in game).
    fn report_over(&mut self) -> Vec<Outcome> {
        self.data.results = self.session().and_then(|s| s.results.clone());
        let out = self.act(UiAction::MatchOver);
        self.over_reported = self.ui.screen() == Screen::Results;
        out
    }

    /// Writes the settings and profiles changed since the last call (the
    /// app calls it about once a second, so a dragged slider is not saved
    /// every frame); a failure is a warning.
    pub fn flush(&mut self) {
        if let Some(s) = self.unsaved_settings.take()
            && let Err(e) = self.store.save_settings(&s)
        {
            eprintln!("warning: saving the settings: {e:#}");
        }
        if let Some(p) = self.unsaved_profiles.take()
            && let Err(e) = self.store.save_profiles(&p)
        {
            eprintln!("warning: saving the profiles: {e:#}");
        }
    }

    /// At exit: the playing game's replay saved, the settings written, an
    /// import stopped after its item (waited for at most
    /// [`IMPORT_STOP_WAIT`]; a pack it was writing stays hidden).
    pub fn finish(&mut self) {
        if let Some(mut job) = self.import.take()
            && !job.stop(IMPORT_STOP_WAIT)
        {
            eprintln!("warning: quitting during an import: its last pack is not imported");
        }
        self.leave_online();
        self.end();
        self.flush();
    }
}

/// Online play (plan 10 task 12): hosting, joining, the lobby's commands,
/// the peer polled every frame and its events.
impl Shell {
    /// LAN announcements and the Multiplayer screen's LAN list (on by
    /// default; tests turn them off: they use the fixed LAN port).
    pub fn set_lan(&mut self, on: bool) {
        self.lan = on;
    }

    /// Hosts on, and joins an address without a port at, `port` instead of
    /// the options' (`--port`; 0 hosts on a free port).
    pub fn set_port(&mut self, port: Option<u16>) {
        self.port = port;
    }

    /// An online game (a lobby, its match, or `--net-sim`'s) exists: the
    /// app polls it every frame, and keeps doing so while the window is
    /// hidden (decision N21).
    pub fn online_active(&self) -> bool {
        self.net.is_some() || self.sim_host.is_some()
    }

    /// An online match plays (its session's `Driver::Online`): the app
    /// runs no local tick.
    pub fn plays_online(&self) -> bool {
        self.session().is_some_and(|s| matches!(s.driver, Driver::Online(_)))
    }

    /// The online game's port (a host's: the one to join).
    pub fn local_port(&self) -> Option<u16> {
        self.peer().map(OnlinePeer::local_port)
    }

    /// The network overlay's numbers while an online match plays (F3;
    /// spec §7.3).
    pub fn net_view(&self) -> Option<NetView> {
        let net = self.net.as_ref()?;
        let peer = self.peer()?;
        let s = peer.match_session()?;
        Some(NetView {
            stats: peer.stats(),
            rollback_last: net.meter.last_depth,
            rollback_p99: net.meter.p99(),
            frontier_lag: s.current_tick().saturating_sub(s.frontier()),
            lagging: net.lagging,
        })
    }

    /// The host says this client's inputs are late (spec §3.2): the HUD
    /// warns "Connection problem".
    pub fn lagging(&self) -> bool {
        self.net.as_ref().is_some_and(|n| n.lagging)
    }

    fn peer(&self) -> Option<&OnlinePeer> {
        if let Some(p) = self.net.as_ref().and_then(|n| n.peer.as_ref()) {
            return Some(p);
        }
        match &self.play.as_ref()?.session.driver {
            Driver::Online(d) => Some(d.peer()),
            _ => None,
        }
    }

    fn peer_mut(&mut self) -> Option<&mut OnlinePeer> {
        if self.net.as_ref().is_some_and(|n| n.peer.is_some()) {
            return self.net.as_mut().and_then(|n| n.peer.as_mut());
        }
        match &mut self.play.as_mut()?.session.driver {
            Driver::Online(d) => Some(d.peer_mut()),
            _ => None,
        }
    }

    /// `--host [name]`: hosts at once with the profile's look and the saved
    /// match (the Lobby opens). Nothing the flags change is saved.
    pub fn host_from_flag(&mut self, name: Option<String>) -> Vec<Outcome> {
        let mut prefs = self.ui.settings().online.host.clone();
        if let Some(name) = name.filter(|n| !n.trim().is_empty()) {
            prefs.name = name;
        }
        self.without_saving(|sh| {
            let mut out = sh.act(UiAction::Open(Screen::Multiplayer));
            out.extend(sh.act(UiAction::Open(Screen::Host)));
            out.extend(sh.act(UiAction::Host(prefs)));
            out
        })
    }

    /// `--join <addr>`: joins at once (the Lobby opens once the host
    /// welcomes this player). The address is not remembered.
    pub fn join_from_flag(&mut self, addr: &str) -> Vec<Outcome> {
        let addr = addr.to_string();
        self.without_saving(|sh| {
            let mut out = sh.act(UiAction::Open(Screen::Multiplayer));
            out.extend(sh.act(UiAction::Join { addr, password: None }));
            out
        })
    }

    /// `--net-sim <profile>` (spec §9.10): a host in this process with
    /// [`NET_SIM_BOTS`] bots on the saved match (the host watches), joined
    /// as a client; every datagram both ways passes `profile`'s impaired
    /// network (`EndpointConfig::impair`), so one player feels it. The
    /// client readies itself and the host starts whenever every player is
    /// ready.
    pub fn start_net_sim(&mut self, profile: Profile) -> anyhow::Result<()> {
        self.leave_online();
        let content = self.open_content()?;
        let mut m = self.ui.settings().gameplay.match_prefs.clone();
        m.bots = NET_SIM_BOTS;
        m.per_bot = None;
        let (_, me) = self.me()?;
        let setup = net::setup_of(&m, &content, me.colour)?;
        let cfg = HostConfig {
            name: format!("net-sim {}", profile.name),
            password: None,
            max_players: 8,
            max_spectators: 4,
            admins: Vec::new(),
        };
        let endpoint =
            EndpointConfig { port: 0, impair: Some(profile), impair_seed: 1, ..EndpointConfig::default() };
        let look = Look { name: "net-sim host".into(), colour: [128, 128, 128], skin: DEFAULT_SKIN.into() };
        let mut host =
            OnlinePeer::start(PeerRole::Host(cfg), endpoint, fresh_identity()?, look, content, Some(setup))?;
        host.lobby().set_spectate(true);
        let port = host.local_port();
        println!("rl-game: net-sim {}: a host on port {port} with {NET_SIM_BOTS} bots", profile.name);
        self.sim_host = Some(host);
        self.impair = Some(profile);
        self.join_from_flag(&format!("127.0.0.1:{port}"));
        let net = self
            .net
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("{}", self.ui.message().unwrap_or("not joined")))?;
        net.auto_ready = true;
        Ok(())
    }

    /// `f` without the settings it changes being saved (the command
    /// line's online flags).
    fn without_saving(&mut self, f: impl FnOnce(&mut Shell) -> Vec<Outcome>) -> Vec<Outcome> {
        let before = self.unsaved_settings.take();
        let out = f(self);
        self.unsaved_settings = before;
        out
    }

    /// The content for a peer: the menus' roots, opened again (the peer
    /// keeps its own).
    fn open_content(&self) -> anyhow::Result<Content> {
        let roots = self.roots();
        let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        Content::open_layers(&roots)
    }

    /// This install's identity (loaded once; `identity.ron`) and the
    /// profile playing's look (its skin by pack id).
    fn me(&mut self) -> anyhow::Result<(Identity, Look)> {
        let id = match &self.identity {
            Some(id) => id.clone(),
            None => {
                let id = self.load_identity()?;
                self.identity = Some(id.clone());
                id
            }
        };
        let p = self.ui.profiles().selected().cloned().unwrap_or_default();
        let skin =
            self.content.as_ref().and_then(|c| c.skin_id(&p.skin_id)).unwrap_or_else(|| DEFAULT_SKIN.into());
        Ok((id, Look { name: p.name, colour: p.colour, skin }))
    }

    /// The identity file's key; a key the file holds that is no valid one
    /// gives a fresh key for this run (with a warning).
    fn load_identity(&mut self) -> anyhow::Result<Identity> {
        let fresh = rl_net::random_bytes::<32>()?;
        let loaded = self.store.load_identity(|| fresh);
        self.log(loaded.warnings);
        if let Some(id) = Identity::from_secret(loaded.value.0) {
            return Ok(id);
        }
        self.log(["the identity file's key is not a valid key: another one for this run".to_string()]);
        fresh_identity()
    }

    /// Hosting from the Host screen: the lobby for the saved match (the
    /// setup's bots may be none: the host adds them in the lobby).
    fn start_host(&mut self, prefs: &HostPrefs) -> anyhow::Result<()> {
        self.leave_net();
        let (id, look) = self.me()?;
        let content = self.open_content()?;
        let setup = net::setup_of(&self.ui.settings().gameplay.match_prefs, &content, look.colour)?;
        let name = if prefs.name.is_empty() { look.name.clone() } else { prefs.name.clone() };
        let cfg = HostConfig {
            name,
            password: (!prefs.password.is_empty()).then(|| prefs.password.as_str().to_string()),
            max_players: prefs.max_players,
            max_spectators: MAX_SPECTATORS,
            admins: Vec::new(),
        };
        let port = self.port.unwrap_or(prefs.port);
        let endpoint = EndpointConfig { port, impair: self.impair, ..EndpointConfig::default() };
        let peer = OnlinePeer::start(PeerRole::Host(cfg), endpoint, id, look, content, Some(setup))
            .map_err(|e| anyhow::anyhow!("cannot host on port {port}: {e}"))?;
        println!("rl-game: hosting on port {}", peer.local_port());
        let mut n = Net::new(peer, true);
        n.password = !prefs.password.is_empty();
        n.max_players = prefs.max_players;
        if self.lan && prefs.visibility == Visibility::Lan {
            match broadcast_targets().first().map(|t| Announcer::new(*t)) {
                Some(Ok(a)) => n.announcer = Some(a),
                Some(Err(e)) => self.log([format!("cannot announce the game on the LAN: {e}")]),
                None => {}
            }
        }
        self.net = Some(n);
        Ok(())
    }

    /// Joining from the Multiplayer screen: `addr` is `host[:port]` as
    /// typed (the port defaults to the options' or `--port`).
    fn start_client(&mut self, addr: &str, password: Option<Password>) -> anyhow::Result<()> {
        self.leave_net();
        let default_port = self.port.filter(|&p| p != 0).unwrap_or(self.ui.settings().online.port);
        let to = resolve(addr, default_port)?;
        let (id, look) = self.me()?;
        let content = self.open_content()?;
        let role = PeerRole::Client { addr: to, password: password.map(|p| p.0) };
        let endpoint = EndpointConfig { port: 0, impair: self.impair, ..EndpointConfig::default() };
        let peer = OnlinePeer::start(role, endpoint, id, look, content, None)?;
        println!("rl-game: joining {to}");
        self.net = Some(Net::new(peer, false));
        Ok(())
    }

    /// The menus told: a host's lobby is up at once (`LobbyJoined`); a
    /// client waits for the host's welcome (`net_frame`); a failure
    /// returns with the reason (`OnlineFailed`).
    fn connected(&mut self, started: anyhow::Result<bool>, out: &mut Vec<Outcome>) {
        let action = match started {
            Ok(false) => return,
            Ok(true) => {
                if let Some(n) = &mut self.net {
                    n.joined = true;
                }
                self.data.chat.clear();
                UiAction::LobbyJoined
            }
            Err(e) => {
                let why = format!("{e:#}");
                eprintln!("warning: online: {why}");
                UiAction::OnlineFailed(why)
            }
        };
        for c in self.ui.update(action) {
            self.carry(c, out);
        }
    }

    /// What the lobby's player asked (`LobbyCommand`); a refusal shows as
    /// the lobby's status.
    fn lobby_command(&mut self, c: LobbyCommand) {
        let seed = match c {
            LobbyCommand::Start | LobbyCommand::ForceStart => self.ui.take_seed(),
            _ => 0,
        };
        let setup = match &c {
            LobbyCommand::Setup(p) => {
                let colour = self.ui.profiles().selected().map_or([255, 255, 255], |p| p.colour);
                Some(match &self.content {
                    Some(content) => net::setup_of(p, content, colour),
                    None => Err(anyhow::anyhow!("there is no content")),
                })
            }
            _ => None,
        };
        let Some(peer) = self.peer_mut() else { return };
        let mut lobby = peer.lobby();
        let state = lobby.state().cloned();
        let why = |e: rl_online::StartError| match &state {
            Some(s) => net::start_reason(&e, s),
            None => e.to_string(),
        };
        let result = match c {
            LobbyCommand::Ready(on) => {
                lobby.set_ready(on);
                Ok(())
            }
            LobbyCommand::Spectate(on) => {
                lobby.set_spectate(on);
                Ok(())
            }
            LobbyCommand::Chat(text) => {
                lobby.chat(&text);
                Ok(())
            }
            LobbyCommand::Setup(_) => match setup {
                Some(Ok(s)) => {
                    lobby.set_setup(s).map_err(|e| format!("the lobby refuses that setup ({e:?})"))
                }
                Some(Err(e)) => Err(format!("{e:#}")),
                None => Ok(()),
            },
            LobbyCommand::Kick { member, ban } => {
                lobby.kick(member, ban);
                Ok(())
            }
            LobbyCommand::Move { member, spectator } => {
                lobby.move_seat(member, spectator);
                Ok(())
            }
            LobbyCommand::Start => lobby.start(seed).map_err(why),
            LobbyCommand::ForceStart => lobby.force_start(seed).map_err(why),
            LobbyCommand::Loadout(w) => {
                lobby.set_loadout(w);
                Ok(())
            }
        };
        self.data.online_status = result.err();
    }

    /// Leaves the online game (the lobby's Back, the match's Leave):
    /// [`Shell::leave_net`], and `--net-sim`'s host goes too.
    fn leave_online(&mut self) {
        self.leave_net();
        if let Some(mut h) = self.sim_host.take() {
            h.leave();
        }
        self.impair = None;
    }

    /// Leaves the lobby this player is in (also before hosting or joining
    /// another): the match ends, the peer says goodbye.
    fn leave_net(&mut self) {
        if self.plays_online() {
            self.end();
        }
        if let Some(mut n) = self.net.take()
            && let Some(p) = &mut n.peer
        {
            p.leave();
        }
        self.data.lobby = None;
        self.data.online_status = None;
        self.data.weapons = None;
    }

    /// The connection is gone (`Disconnected`, `Kicked`, `Refused`): the
    /// match view goes at once (the peer dropped its match: the session
    /// would draw its start world), then the menus return to Multiplayer
    /// with `why`.
    fn lost(&mut self, why: String, out: &mut Vec<Outcome>) {
        eprintln!("rl-game: online: {why}");
        self.end();
        self.net = None;
        if let Some(mut h) = self.sim_host.take() {
            h.leave();
        }
        self.impair = None;
        self.data.lobby = None;
        self.data.weapons = None;
        for c in self.ui.update(UiAction::OnlineFailed(why)) {
            self.carry(c, out);
        }
    }

    /// One frame of online play at `now` (the app's loop, and its hidden
    /// window's frames: decision N21): the LAN list while the Multiplayer
    /// screen shows, `--net-sim`'s host, then the peer polled once (through
    /// the match's session while one plays, `Session::frame`, the player's
    /// keys from `keys`: none while the match's menu, the chat or another
    /// screen has them) and its events carried out.
    pub fn net_frame(&mut self, now: Instant, keys: &mut dyn FnMut() -> Keys) -> Vec<Outcome> {
        let mut out = Vec::new();
        self.poll_lan(now);
        self.poll_sim_host(now);
        if self.net.is_none() {
            return out;
        }
        let held = self.paused || self.ui.screen() != Screen::InGame || self.data.ingame_chat.is_some();
        let mut none = || Keys(0);
        let keys: &mut dyn FnMut() -> Keys = if held { &mut none } else { keys };
        let events = match &mut self.play {
            Some(p) if matches!(p.session.driver, Driver::Online(_)) => p.session.frame(now, keys),
            _ => match self.net.as_mut().and_then(|n| n.peer.as_mut()) {
                Some(peer) => peer.poll(now, &mut || Keys(0), &mut |_, _, _| {}),
                None => Vec::new(),
            },
        };
        self.on_net_events(events, &mut out);
        let Some(n) = &mut self.net else { return out };
        if let Some((a, info)) = n.announcer.as_mut().zip(n.announce.as_ref()) {
            a.tick(now, info);
        }
        let hist = match &self.play {
            Some(p) => match &p.session.driver {
                Driver::Online(d) => d.peer().match_session().map(|s| s.stats().depth_hist),
                _ => None,
            },
            None => None,
        };
        if let Some(h) = hist {
            n.meter.sample(now, &h);
        }
        out
    }

    /// The poll's events, in order.
    fn on_net_events(&mut self, events: Vec<OnlineEvent>, out: &mut Vec<Outcome>) {
        let mut queue: VecDeque<OnlineEvent> = events.into();
        while let Some(e) = queue.pop_front() {
            match e {
                OnlineEvent::Lobby(state) => self.show_lobby(&state, out),
                OnlineEvent::Chat { from, text } => {
                    self.chat_line(LogLine { text: format!("{from}: {text}"), kind: LineKind::Chat });
                }
                OnlineEvent::Notice(text) => self.chat_line(LogLine::event(text)),
                OnlineEvent::Left(name) => self.chat_line(LogLine::event(format!("{name} left"))),
                OnlineEvent::MatchStarting { start, my_slot } => {
                    let after: Vec<OnlineEvent> = queue.drain(..).collect();
                    let rest = self.start_online(start, my_slot, after, out);
                    queue.extend(rest);
                }
                OnlineEvent::MatchEnded(_) => {
                    // The host's end: the results show now if they did not
                    // already (the confirmed end's).
                    if self.plays_online() && !self.over_reported && self.ui.screen() == Screen::InGame {
                        let o = self.report_over();
                        out.extend(o);
                    }
                }
                OnlineEvent::Lagging(on) => {
                    if let Some(n) = &mut self.net {
                        n.lagging = on;
                    }
                }
                OnlineEvent::Kicked { banned } => {
                    let why = if banned { "The host banned you." } else { "The host kicked you." };
                    return self.lost(why.into(), out);
                }
                OnlineEvent::Refused(why) | OnlineEvent::Disconnected(why) => return self.lost(why, out),
                OnlineEvent::ReplayReady(bytes) => self.save_online_replay(&bytes),
                OnlineEvent::DesyncBundle(bytes) => self.save_bundle(&bytes),
                // The match's presentation, polled without a match view (a
                // view that failed to start): nothing to show it on.
                OnlineEvent::StateLoaded { .. }
                | OnlineEvent::Restored(_)
                | OnlineEvent::Ticked(_)
                | OnlineEvent::Confirmed(_) => {}
            }
        }
    }

    /// The lobby changed: its view (and the client's welcome: the Lobby
    /// opens), the packs a client lacks told to the host, the weapon
    /// screen's menu for its mod, the LAN announcement.
    fn show_lobby(&mut self, state: &LobbyState, out: &mut Vec<Outcome>) {
        let Some(peer) = self.peer_mut() else { return };
        let port = peer.local_port();
        let lobby = peer.lobby();
        let (me, can) = (lobby.me(), lobby.can_start());
        let Some(n) = &mut self.net else { return };
        let missing = match &self.content {
            Some(c) if !n.host && n.checked_packs.as_ref() != Some(&state.setup.packs) => {
                n.checked_packs = Some(state.setup.packs.clone());
                Some(net::missing_packs(c, &state.setup))
            }
            _ => None,
        };
        let key = (state.setup.mod_id.clone(), state.setup.restrictions.clone());
        if n.weapons.as_ref().is_none_or(|w| w.0 != key) {
            let menu = self.content.as_ref().and_then(|c| net::weapon_menu(c, &key.0, &key.1));
            n.weapons = Some((key, menu));
        }
        let host = n.host;
        if host {
            n.announce = Some(net::announcement(state, port, n.max_players, n.password));
        }
        let can_start = if host {
            can.map_err(|e| net::start_reason(&e, state))
        } else {
            Err("Waiting for the host".into())
        };
        let weapons = n.weapons.as_ref().and_then(|w| w.1.clone());
        let welcomed = !n.joined && me.is_some();
        n.joined |= welcomed;
        let auto_ready = welcomed && n.auto_ready;
        if let Some(missing) = missing
            && let Some(peer) = self.peer_mut()
        {
            peer.lobby().have(missing);
        }
        if auto_ready && let Some(peer) = self.peer_mut() {
            peer.lobby().set_ready(true);
        }
        if self.play.is_none() {
            self.data.weapons = weapons;
        }
        self.data.lobby = me.map(|me| net::lobby_view(state, me, host, can_start));
        if welcomed {
            self.data.chat.clear();
            for c in self.ui.update(UiAction::LobbyJoined) {
                self.carry(c, out);
            }
        }
    }

    /// A line of the lobby's chat: in the lobby's log and, in game, the
    /// HUD's (spec §7.2).
    fn chat_line(&mut self, line: LogLine) {
        if let Some(s) = self.session_mut() {
            s.log.push(line.clone());
        }
        self.data.chat.push(line);
        let excess = self.data.chat.len().saturating_sub(net::CHAT_KEEP);
        self.data.chat.drain(..excess);
    }

    /// The host started a match (`MatchStarting`; the events after it in
    /// that poll are its): the last match's view goes, the match's assets
    /// load (mod, level, theme, every slot's skin) and its session starts
    /// with `Driver::Online`; the menus go in game. Returns the events
    /// that are not the match's presentation.
    fn start_online(
        &mut self,
        start: MatchStartWire,
        my_slot: Option<u8>,
        after: Vec<OnlineEvent>,
        out: &mut Vec<Outcome>,
    ) -> Vec<OnlineEvent> {
        if self.play.is_some() {
            self.end();
        }
        let Some(mut peer) = self.net.as_mut().and_then(|n| n.peer.take()) else { return after };
        self.launches += 1;
        let roots = self.roots();
        let root: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        let mut assets = match setup::load_online(&root, &start, my_slot) {
            Ok(a) => a,
            Err(e) => {
                peer.leave();
                self.lost(format!("cannot show the match: {e:#}"), out);
                return Vec::new();
            }
        };
        self.log(std::mem::take(&mut assets.warnings));
        let driver = OnlineDriver::new(peer, &start);
        let made =
            Session::online(&assets, driver, self.cfg.viewport, self.cfg.interpolation, Audio::silent());
        let mut session = match made {
            Ok(s) => s,
            Err(e) => {
                self.lost(format!("cannot show the match: {e:#}"), out);
                return Vec::new();
            }
        };
        let mut audio = self.audio.take().unwrap_or_else(Audio::silent);
        audio.stop_all();
        audio.load(&assets.m, assets.content.core());
        self.log(audio.take_warnings());
        session.audio = audio;
        let rest = session.present_events(after);
        self.play = Some(Playing { assets, session });
        self.paused = false;
        self.over_reported = false;
        self.data.results = None;
        if let Some(n) = &mut self.net {
            n.lagging = false;
            n.meter.clear();
        }
        let watch = if my_slot.is_some() { "plays" } else { "watches" };
        println!("rl-game: online match seed {}: this player {watch}", start.seed);
        out.extend(self.act(UiAction::MatchStarted));
        out.push(Outcome::Started);
        rest
    }

    /// A finished online match's replay, with the automatic ones.
    fn save_online_replay(&mut self, bytes: &[u8]) {
        let Some(dir) = self.store.replays_dir() else { return };
        match rl_ui::replays::auto_save(&dir, bytes) {
            Ok(path) => println!("rl-game: saved replay {}", path.display()),
            Err(e) => eprintln!("warning: saving the replay in {}: {e}", dir.display()),
        }
    }

    /// A desync's bundle (spec §9.10), in the data directory's `desyncs/`.
    fn save_bundle(&mut self, bytes: &[u8]) {
        let Some(dir) = self.store.dir().map(|d| d.join(net::DESYNCS_DIR)) else {
            eprintln!("warning: a desync happened; without a data directory its bundle is not saved");
            return;
        };
        let secs =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        match net::save_bundle(&dir, bytes, secs) {
            Ok(path) => eprintln!("warning: a desync happened: its bundle is {}", path.display()),
            Err(e) => eprintln!("warning: saving a desync bundle in {}: {e}", dir.display()),
        }
    }

    /// The Multiplayer screen's LAN list, while it shows.
    fn poll_lan(&mut self, now: Instant) {
        if !self.lan || self.ui.screen() != Screen::Multiplayer {
            self.lan_listener = None;
            self.lan_failed = false;
            return;
        }
        if self.lan_listener.is_none() && !self.lan_failed {
            match Listener::new(LAN_PORT) {
                Ok(l) => self.lan_listener = Some(l),
                Err(e) => {
                    self.lan_failed = true;
                    self.log([format!("cannot listen for LAN games on port {LAN_PORT}: {e}")]);
                }
            }
        }
        let Some(l) = &mut self.lan_listener else { return };
        let catalog = &self.data.catalog;
        let level = |id: &str| catalog.level(id).map(|l| l.name.clone());
        self.data.lan_games = l
            .poll(now)
            .iter()
            .filter(|g| g.info.fingerprint == rl_proto::FINGERPRINT)
            .map(|g| net::lan_entry(g, level))
            .collect();
    }

    /// `--net-sim`'s host: polled; it starts whenever every player is ready.
    fn poll_sim_host(&mut self, now: Instant) {
        let Some(h) = &mut self.sim_host else { return };
        for e in h.poll(now, &mut || Keys(0), &mut |_, _, _| {}) {
            if let OnlineEvent::DesyncBundle(b) = e {
                self.save_bundle(&b);
            }
        }
        if self.sim_host.as_mut().is_some_and(|h| h.lobby().can_start().is_ok()) {
            let seed = self.ui.take_seed();
            if let Some(Err(e)) = self.sim_host.as_mut().map(|h| h.lobby().start(seed)) {
                self.log([format!("net-sim: the host cannot start: {e}")]);
            }
        }
    }
}

/// A fresh identity (a key the store does not keep).
fn fresh_identity() -> anyhow::Result<Identity> {
    for _ in 0..8 {
        if let Some(id) = Identity::from_secret(rl_net::random_bytes::<32>()?) {
            return Ok(id);
        }
    }
    anyhow::bail!("no valid key from the system's randomness")
}

/// The address `typed` (`host[:port]`, an IPv6 address in brackets with a
/// port), `default_port` when it has none; a name is looked up.
pub fn resolve(typed: &str, default_port: u16) -> anyhow::Result<SocketAddr> {
    let typed = typed.trim();
    if let Ok(a) = typed.parse::<SocketAddr>() {
        return Ok(a);
    }
    if let Ok(ip) = typed.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, default_port));
    }
    let with_port = if typed.contains(':') { typed.to_string() } else { format!("{typed}:{default_port}") };
    with_port
        .to_socket_addrs()
        .map_err(|e| anyhow::anyhow!("{typed}: {e}"))?
        .next()
        .ok_or_else(|| anyhow::anyhow!("{typed}: no address"))
}

/// The weapon selection's menu for `a`'s game: a Deathmatch's mod weapons
/// with its restrictions (`None` for another game).
pub fn weapon_menu(a: &Assets) -> Option<WeaponMenu> {
    let Game::Deathmatch { options, .. } = &a.game else { return None };
    let rules = a.m.weapon_rules(&options.restrictions);
    let names: Vec<String> = a.m.sim.weapons().iter().map(|w| w.name.clone()).collect();
    let enabled = names.iter().map(|n| rules.is_enabled(n)).collect();
    Some(WeaponMenu { names, enabled, fallback: rules.find_enabled_weapon(), seed: options.seed })
}
