//! The menus driving the game (spec §21 G1-G2), without a window: the
//! `olr_ui::UiState`, its widgets' data, the settings and profiles files,
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
//! ends, is restarted or the game quits (`olr_ui::replays::auto_save`); the
//! results' Save replay keeps it under a name of its own
//! (`olr_ui::replays::save_kept`).
//!
//! The menus list and play the bundled content (`--content`) with the
//! user data's `packs/` layered over it (spec §9.4: a user pack
//! replaces the bundled one of its id; `olr-cli import` writes them).
//! The content is listed once at start-up; a game playing keeps the packs
//! it loaded.
//!
//! **Online** (plan 10 task 12, `net.rs`): `HostGame` and `JoinGame` make
//! the `olr_online::OnlinePeer` (the identity from `Store::load_identity`,
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
//!
//! **Downloads** (plan 11): the shell opens one download cache at startup
//! (`Store::cache_dir`, else `-2` .. `-4` while other games hold it:
//! `net::open_download_cache`) and shares it with every peer it starts
//! (`--net-sim`'s host and client too). The lobby lists the downloads and
//! asks before content over the prompt; a match loads from its peer's
//! content (the downloads laid over); a replay's packs come from this
//! install, else from the cache by content hash. The options show the
//! cache's size and clear it, never while an online game is on.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;
use std::time::{Duration, Instant};

use egui::{ColorImage, TextureHandle, TextureOptions};
use olr_audio::Audio;
use olr_content::{Content, LoadedSkin, MatchOptions, Player};
use olr_discovery::defaults::{DEFAULT_RELAYS, DEFAULT_STUN};
use olr_input::Bindings;
use olr_lobby::Identity;
use olr_net::endpoint::IceConfig;
use olr_net::lan::Listener;
use olr_netcode::netsim::Profile;
use olr_online::Shutdown;
use olr_online::replay::PendingReplay;
use olr_scene::{Interpolation, Viewport};
use olr_sim::{Keys, MAX_SLOTS};
use olr_ui::screens::MenuData;
use olr_ui::skin_preview::{self, PreviewKey, PreviewTextures};
use olr_ui::weapon_select::{WeaponMenu, WeaponSelect};
use olr_ui::{
    AppCommand, AudioSettings, DownloadCacheView, Profiles, Screen, Settings, Store, UiAction, UiState,
    VideoSettings,
};

use crate::finding::Internet;
use crate::net::{self, Net, SharedCache};
use crate::session::{Driver, Session};
use crate::setup::{self, Assets, EditedPlay, Game, PlayerLook, SandboxOptions};
use crate::video::{self, Overrides};

mod editing;
mod joining;
mod online;
pub use joining::{NO_WAY_TO_JOIN, UNMAP_WAIT, Unmapping, resolve};
pub use online::late_rejoin;

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
    /// The Report screen opened: the app hands [`Shell::prepare_report`]
    /// what only the window knows (the GPU, the display).
    PrepareReport,
}

/// What to load.
enum Launch {
    Match(Box<MatchOptions>),
    Sandbox,
    Replay(PathBuf),
    /// The level editor's Play (plan 14c): no replay is saved.
    Edited(Box<EditedPlay>),
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
    /// The own loadouts to write (`loadouts.ron`, issue #5).
    unsaved_loadouts: Option<olr_ui::OwnLoadouts>,
    /// The settings as the log last said them (changes are logged).
    logged_settings: Settings,
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
    /// The user packs layer (`<data>/packs`), over the bundled content.
    packs_dir: Option<PathBuf>,
    /// The online game (its lobby and match), if any.
    net: Option<Net>,
    /// `--net-sim`'s host, in this process (spec §9.10), with its pump.
    sim_host: Option<crate::net::PeerHandle>,
    /// The key this install plays online with (loaded once).
    identity: Option<Identity>,
    /// LAN announcements and the LAN list (tests turn them off: fixed port).
    lan: bool,
    /// The LAN list's listener while the Multiplayer screen shows, on
    /// `lan_port` (`LAN_PORT`; tests: another, `test_lan_port`).
    lan_listener: Option<Listener>,
    lan_port: u16,
    /// The listener could not be made (the port taken): tried again when
    /// the screen opens again.
    lan_failed: bool,
    /// The port to host on and join at instead of the options' (`--port`;
    /// tests host on 0).
    port: Option<u16>,
    /// Every link impaired with this profile (`--net-sim`).
    impair: Option<Profile>,
    /// Replays of online matches whose peer is gone, still being made.
    replays: Vec<(u64, PendingReplay)>,
    /// The download cache every peer shares (plan 11, ruling PF11-3).
    cache: SharedCache,
    /// When the Options' cache row last read the cache's sizes (`None`
    /// off the Options screen): at most every `CACHE_READ_EVERY`.
    cache_read: Option<Instant>,
    /// The relays and the game list while the Multiplayer screen shows or
    /// a hosted game is public (plan 12a).
    internet: Option<Internet>,
    /// The Internet game last pinged (the menus' selection).
    pinged: Option<[u8; 32]>,
    /// When the relays stopped being needed (they linger, `RELAY_LINGER`).
    internet_left: Option<Instant>,
    /// Relay pools opened so far.
    relay_pools: u32,
    /// The `created_at` of the last closing note this game published (a
    /// game hosted again dates its first note after it).
    closed_note: Option<u64>,
    /// A typed host name being looked up on a thread (never on the UI
    /// thread), and the join waiting for it.
    resolving: Option<joining::Resolving>,
    /// The peers let go, their router mappings being deleted on threads
    /// (ruling NT-2): the exit waits for them, after the window closed.
    unmapping: Vec<Shutdown>,
    /// Tests only: applied to the ICE settings of every endpoint the shell
    /// makes (`test_ice`, feature `test-hooks`).
    ice_hook: Option<IceHook>,
    /// Tests only: each match's load takes this much longer
    /// (`test_slow_load`, feature `test-hooks`).
    slow_load: Option<Duration>,
    /// The local player's keys held now (`set_held_keys`; all until told).
    held_keys: Keys,
    /// Clients this shell starts take part in host migration (plan 12c;
    /// tests turn it off, `test_migration`: D7).
    migration: bool,
    /// Tests only: clients this shell starts never host (no standby, N79:
    /// `test_no_standby`).
    no_standby: bool,
    /// The number the next replay handed to `replays` gets.
    next_replay: u64,
    /// The Host lost screen waits for its match's replay (its number in
    /// `replays`): when that one is saved `MenuData::host_lost_replay` is
    /// set, no other replay's saving does.
    host_lost_pending: Option<u64>,
    /// The level editor while it is open (plan 14c): it replaces the menus.
    pub(crate) editor: Option<crate::editor::EditorHost>,
    /// What the editor said while it opened, for the app's next editor frame.
    editor_out: Vec<crate::editor::HostOut>,
    /// The skin editor while it is open (issue #14): it replaces the menus.
    pub(crate) skin_editor: Option<crate::editor::skin::SkinHost>,
    /// The mod editor while it is open (issue #17): it replaces the menus.
    pub(crate) mod_editor: Option<crate::editor::mods::ModHost>,
    /// The bug relay's address (`report::endpoint`; none: Report a bug
    /// is hidden), and the report being sent.
    report_url: Option<String>,
    report_send: Option<crate::report::Sending>,
    /// The update check and install (issue #4); none: a build that does
    /// not check.
    updater: Option<crate::update::Updater>,
}

/// A change to an endpoint's ICE settings (tests: a loopback base, a
/// `FakeGateway`).
type IceHook = Box<dyn Fn(&mut IceConfig)>;

/// The Options' download cache row reads the cache's sizes (a directory
/// listing, a file per partial) at most this often (review Minor 4).
pub const CACHE_READ_EVERY: Duration = Duration::from_secs(1);

/// How long quitting waits for an online match's replay to be made.
pub const REPLAY_STOP_WAIT: Duration = Duration::from_secs(2);

/// `--net-sim`'s bots (spec §9.10: a host with three bots).
pub const NET_SIM_BOTS: u8 = 3;
/// Spectators a hosted lobby takes (spec §7.2).
pub const MAX_SPECTATORS: u8 = 16;

impl Shell {
    /// The main menu over content directory `cfg.content` with the data
    /// directory's user packs over it (listed for the menus now), with
    /// `audio` for every game.
    pub fn new(cfg: ShellConfig, ui: UiState, store: Store, audio: Audio) -> Shell {
        let mut data = MenuData::new(Default::default(), store.replays_dir());
        let packs_dir = store.packs_dir();
        data.notices = store.notices();
        data.version = crate::VERSION.to_string();
        let logged_settings = ui.settings().clone();
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
            unsaved_loadouts: None,
            logged_settings,
            logged: BTreeSet::new(),
            content: None,
            preview_skin: None,
            preview_textures: Vec::new(),
            overrides: None,
            packs_dir,
            net: None,
            sim_host: None,
            identity: None,
            lan: true,
            lan_listener: None,
            lan_port: olr_proto::LAN_PORT,
            lan_failed: false,
            port: None,
            impair: None,
            replays: Vec::new(),
            cache: Err(String::new()),
            cache_read: None,
            internet: None,
            pinged: None,
            internet_left: None,
            relay_pools: 0,
            closed_note: None,
            resolving: None,
            unmapping: Vec::new(),
            ice_hook: None,
            slow_load: None,
            held_keys: Keys(u8::MAX),
            migration: true,
            no_standby: false,
            next_replay: 0,
            host_lost_pending: None,
            editor: None,
            editor_out: Vec::new(),
            skin_editor: None,
            mod_editor: None,
            report_url: None,
            report_send: None,
            updater: None,
        };
        let loaded = shell.store.load_file::<olr_ui::OwnLoadouts>(olr_ui::LOADOUTS_FILE);
        let fixed = shell.ui.set_loadouts(loaded.value.unwrap_or_default());
        shell.log(loaded.warnings.into_iter().chain(fixed).collect::<Vec<_>>());
        shell.set_report_url(crate::report::endpoint(std::env::var(crate::report::URL_VAR).ok()));
        shell.set_updater(crate::update::Updater::of_build());
        shell.data.default_relays = DEFAULT_RELAYS.iter().map(|r| r.to_string()).collect();
        shell.data.default_stun = DEFAULT_STUN.iter().map(|s| s.to_string()).collect();
        shell.cache = net::open_download_cache(shell.store.cache_dir());
        if let Err(why) = &shell.cache {
            shell.log([format!("content downloads are off: {why}")]);
        }
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
            unsaved_loadouts: None,
            logged_settings: Settings::default(),
            logged: BTreeSet::new(),
            content: None,
            preview_skin: None,
            preview_textures: Vec::new(),
            overrides: None,
            packs_dir: None,
            net: None,
            sim_host: None,
            identity: None,
            lan: false,
            lan_listener: None,
            lan_port: olr_proto::LAN_PORT,
            lan_failed: false,
            port: None,
            impair: None,
            replays: Vec::new(),
            cache: Err("a game from the command line downloads nothing".into()),
            cache_read: None,
            internet: None,
            pinged: None,
            internet_left: None,
            relay_pools: 0,
            closed_note: None,
            resolving: None,
            unmapping: Vec::new(),
            ice_hook: None,
            slow_load: None,
            held_keys: Keys(u8::MAX),
            migration: true,
            no_standby: false,
            next_replay: 0,
            host_lost_pending: None,
            editor: None,
            editor_out: Vec::new(),
            skin_editor: None,
            mod_editor: None,
            report_url: None,
            report_send: None,
            updater: None,
        }
    }

    /// The content directories, highest priority first: the data
    /// directory's user packs, then the bundled content (OLR's own packs,
    /// then the imported ones: `olr_content::bundled_roots`).
    fn roots(&self) -> Vec<PathBuf> {
        self.packs_dir.iter().cloned().chain(olr_content::bundled_roots(&self.cfg.content)).collect()
    }

    /// Lists the content for the menus: the
    /// catalog, its thumbnails made again (`MenuData::set_catalog`), the
    /// skin preview loaded again.
    fn list_content(&mut self) {
        let roots = self.roots();
        let roots: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        self.content = match Content::open_layers(&roots) {
            Ok(c) => Some(c),
            Err(e) => {
                log::warn!("{e:#}: no mods or levels to play");
                None
            }
        };
        let catalog = self.content.as_ref().map(Content::catalog).unwrap_or_default();
        let dirs: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
        log::info!(
            "content: {} mods, {} levels, {} skins from {}",
            catalog.mods.len(),
            catalog.levels.len(),
            catalog.skins.len(),
            dirs.join(", ")
        );
        self.log(self.content.as_ref().map(|c| c.warnings().to_vec()).unwrap_or_default());
        self.log(catalog.warnings.clone());
        self.data.set_catalog(catalog);
        self.preview_skin = None;
        self.data.preview = None;
        self.preview_textures.clear();
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
        let right = (0..n).map(|f| upload(olr_sim::Dir::Right, f)).collect();
        let left = (0..n).map(|f| upload(olr_sim::Dir::Left, f)).collect();
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
        self.play.is_some() && (self.ui.screen() == Screen::InGame && !self.paused || self.editing_play())
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
        let before = self.ui.screen();
        for c in self.ui.update(a) {
            self.carry(c, &mut out);
        }
        // The Editor screen lists the packs and recoveries as they are now.
        if self.ui.screen() == Screen::Editor && before != Screen::Editor {
            self.data.editor_message = None;
            self.refresh_editor_packs();
        }
        out
    }

    /// The level editor's Play: loaded by the next [`Shell::poll`], as a
    /// menu start is.
    pub fn play_edited(&mut self, play: EditedPlay) {
        self.pending = Some(Launch::Edited(Box::new(play)));
    }

    fn carry(&mut self, c: AppCommand, out: &mut Vec<Outcome>) {
        match c {
            AppCommand::StartMatch(o) => self.pending = Some(Launch::Match(Box::new(o))),
            AppCommand::StartSandbox => self.pending = Some(Launch::Sandbox),
            AppCommand::PlayReplay(path) => self.pending = Some(Launch::Replay(path)),
            AppCommand::EndMatch => self.end(),
            AppCommand::EditLevel(_)
            | AppCommand::CopyLevel { .. }
            | AppCommand::NewLevel { .. }
            | AppCommand::Recover(_)
            | AppCommand::DiscardRecovery(_) => self.editor_command(c),
            AppCommand::EditSkin(_) | AppCommand::NewSkin(_) => self.skin_command(c),
            AppCommand::EditMod(_)
            | AppCommand::NewMod { .. }
            | AppCommand::RecoverMod(_)
            | AppCommand::DiscardModRecovery(_) => self.mod_command(c),
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
                if let Some(s) = self.session_mut() {
                    s.present.set_blood(v.blood);
                }
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
            AppCommand::SaveLoadouts(l) => self.unsaved_loadouts = Some(l),
            AppCommand::HostGame(prefs) => {
                let started = self.start_host(&prefs);
                self.connected(started.map(|()| true), out);
            }
            AppCommand::JoinGame { addr, password, expected_host } => {
                let started = self.start_client(addr, password, expected_host);
                self.connected(started.map(|()| false), out);
            }
            AppCommand::BlockHost(key) => self.block_host(key),
            AppCommand::Lobby(c) => self.lobby_command(c),
            AppCommand::LeaveOnline => self.leave_online(),
            AppCommand::ClearDownloadCache => self.clear_download_cache(),
            AppCommand::PrepareReport => out.push(Outcome::PrepareReport),
            AppCommand::SendReport(r) => self.send_report(*r),
            AppCommand::InstallUpdate => {
                if let Some(u) = &mut self.updater {
                    u.install(&mut self.data.update);
                }
            }
            AppCommand::RestartGame => match self.updater.as_ref().map(|u| u.restart()) {
                Some(Ok(())) => {
                    log::info!("update: restarting into the new build");
                    out.push(Outcome::Quit);
                }
                Some(Err(why)) => {
                    log::warn!("update: {why}");
                    self.data.update.status = olr_ui::update::UpdateStatus::Failed(format!(
                        "The new version could not be started ({why}). Quit and start the game again."
                    ));
                }
                None => {}
            },
            AppCommand::OpenUrl(url) => {
                if let Err(why) = crate::update::open_url(&url) {
                    log::warn!("{why}");
                }
            }
            AppCommand::Quit => out.push(Outcome::Quit),
        }
    }

    /// Loads the game asked for in an earlier frame, if any,
    /// and reports to the menus whether it started.
    pub fn poll(&mut self) -> Vec<Outcome> {
        self.poll_report();
        if let Some(u) = &mut self.updater {
            u.poll(self.ui.settings().updates.include_prereleases, &mut self.data.update);
        }
        if self.ui.screen() != Screen::Options {
            self.cache_read = None;
        } else if self.cache_read.is_none_or(|t| t.elapsed() >= CACHE_READ_EVERY) {
            self.show_download_cache();
        } else {
            // Whether Clear may run needs no disk: current every frame.
            let clear = self.clear_refused().map_or(Ok(()), Err);
            if let Ok(view) = &mut self.data.download_cache {
                view.clear = clear;
            }
        }
        let Some(launch) = self.pending.take() else { return Vec::new() };
        self.launch_reported(launch)
    }

    fn launch_reported(&mut self, launch: Launch) -> Vec<Outcome> {
        // The editor's Play: the menus' state is not the game's.
        if matches!(launch, Launch::Edited(_)) {
            return match self.launch(launch) {
                Ok(()) => vec![Outcome::Started],
                Err(e) => {
                    let why = format!("{e:#}");
                    log::warn!("could not play the level: {why}");
                    self.stop_edited();
                    if let Some(h) = &mut self.editor {
                        h.act(vec![olr_editor::EditorAction::Failed(format!(
                            "The level could not be played: {why}"
                        ))]);
                    }
                    Vec::new()
                }
            };
        }
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
                log::warn!("could not start: {why}");
                self.act(UiAction::MatchFailed(why))
            }
        }
    }

    /// The download cache every peer shares, or why there is none.
    pub fn download_cache(&self) -> &SharedCache {
        &self.cache
    }

    /// Why the download cache may not be cleared now: an online game (its
    /// peers download into it) is on.
    fn clear_refused(&self) -> Option<String> {
        (self.net.is_some() || self.sim_host.is_some()).then(|| "leave the online game first".to_string())
    }

    /// The options' download cache row: its size (packs and partial
    /// downloads) and whether Clear may run. A cache busy on a peer's
    /// thread keeps the last row (and is read again next poll).
    fn show_download_cache(&mut self) {
        let clear = self.clear_refused().map_or(Ok(()), Err);
        self.data.download_cache = match &self.cache {
            Ok(c) => match c.try_lock() {
                Ok(c) => {
                    self.cache_read = Some(Instant::now());
                    Ok(DownloadCacheView { bytes: c.total_bytes() + c.partial_bytes(), clear })
                }
                Err(_) => return,
            },
            Err(why) => Err(why.clone()),
        };
    }

    /// The options' Clear: every download goes (the menus have no match
    /// to keep); refused while an online game is on.
    fn clear_download_cache(&mut self) {
        if let Some(why) = self.clear_refused() {
            self.log([format!("the download cache is not cleared: {why}")]);
            return;
        }
        let Ok(cache) = &self.cache else { return };
        let cleared = cache.lock().unwrap_or_else(PoisonError::into_inner).clear(&[]);
        match cleared {
            Ok(()) => log::info!("download cache cleared"),
            Err(e) => log::warn!("clearing the download cache: {e}"),
        }
        self.show_download_cache();
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
                log::warn!("{w}");
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
                    bonuses: false,
                };
                setup::load(&root, &m.mod_id, &m.level_id, &profile.skin_id, o)?
            }
            Launch::Replay(path) => {
                log::info!("playing the replay {}", path.display());
                setup::load_replay(&root, &path, &net::cache_dirs(self.store.cache_dir()))?
            }
            Launch::Edited(play) => {
                let prefs = &self.ui.settings().gameplay.match_prefs;
                setup::load_edited(&root, &play, &profile.skin_id, prefs)?
            }
        };
        assets.player = Some(PlayerLook { name: profile.name, colour: profile.colour });
        self.log(std::mem::take(&mut assets.warnings));
        log::info!("game start: {}", assets.describe());
        let mut session = Session::new(&assets, self.cfg.viewport, self.cfg.interpolation, Audio::silent())?;
        session.present.set_blood(self.ui.settings().video.blood);
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
            log::warn!("arming the player: {e:#}");
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
        let r = self
            .play
            .as_mut()
            .filter(|p| !p.assets.edited())
            .and_then(|p| p.session.replay())
            .ok_or("there is no replay")?;
        let path = olr_ui::replays::save_kept(&dir, &r.encode()).map_err(|e| e.to_string())?;
        log::info!("kept replay {}", path.display());
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
            log::info!("game end: {}", p.assets.describe());
            if let Some(r) = p.session.take_replay().filter(|_| !p.assets.edited()) {
                self.save_replay(&r);
            }
            // An online match's peer goes back to the lobby.
            if let Driver::Online(d) = std::mem::replace(&mut p.session.driver, Driver::Sandbox)
                && let Some(n) = &mut self.net
            {
                n.peer = Some(d.into_handle());
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
        log::info!("restart: {}", p.assets.describe());
        match p.session.restart(&p.assets) {
            Ok(Some(r)) if !p.assets.edited() => self.save_replay(&r),
            Ok(_) => {}
            Err(e) => log::warn!("restarting: {e:#}"),
        }
        self.over_reported = false;
    }

    /// Saves `r` with the automatic replays (one of no tick is not kept).
    fn save_replay(&mut self, r: &olr_replay::Replay) {
        let Some(dir) = self.store.replays_dir().filter(|_| r.ticks() > 0) else { return };
        match olr_ui::replays::auto_save(&dir, &r.encode()) {
            Ok(path) => log::info!("saved the replay {} ({} ticks)", path.display(), r.ticks()),
            Err(e) => log::warn!("saving the replay in {}: {e}", dir.display()),
        }
    }

    /// Where bug reports go (`report::endpoint`; tests: a local mock);
    /// `None` hides Report a bug.
    pub fn set_report_url(&mut self, url: Option<String>) {
        self.data.report.available = url.is_some();
        self.report_url = url;
    }

    /// The update check (issue #4: `update::Updater::of_build`; tests: one
    /// on a mock); `None` hides every update notice and option.
    pub fn set_updater(&mut self, updater: Option<crate::update::Updater>) {
        self.data.update = Default::default();
        if let Some(u) = &updater {
            u.fill(&mut self.data.update);
        }
        self.updater = updater;
    }

    /// The Report screen's context from `env` (the window's): the
    /// system, the settings in effect (the command line's flags marked),
    /// this run's and the previous run's logs. Gathers only: nothing is
    /// sent but on Send.
    pub fn prepare_report(&mut self, env: crate::report::Environment) {
        let overrides =
            self.overrides.as_ref().map(|(_, o)| crate::report::overridden(o)).unwrap_or_default();
        let facts = crate::report::facts(env, self.ui.settings(), overrides);
        let context =
            crate::report::context(&facts, olr_log::path().as_deref(), std::env::home_dir().as_deref());
        for w in &context.warnings {
            log::info!("bug report: {w}");
        }
        self.data.report.context = Some(context);
    }

    /// The Report screen's Send: on a thread of its own, one at a time.
    fn send_report(&mut self, report: olr_ui::report::Report) {
        if self.report_send.is_some() {
            return;
        }
        let Some(url) = self.report_url.clone() else {
            self.data.report.finished(Err("This build has no report server to send to.".into()));
            return;
        };
        log::info!(
            "bug report: sending ({} bytes of report, {} of replay)",
            report.json().len(),
            report.replay_bytes.as_ref().map_or(0, |b| b.0.len())
        );
        self.data.report.status = olr_ui::report::SendStatus::Sending;
        self.report_send = Some(crate::report::Sending::start(url, report, crate::report::TIMEOUTS));
    }

    /// A send that ended: its outcome on the Report screen, and the log.
    fn poll_report(&mut self) {
        let Some(result) = self.report_send.as_ref().and_then(|s| s.poll()) else { return };
        self.report_send = None;
        match &result {
            Ok(url) => log::info!("bug report: filed as {url}"),
            Err(why) => log::warn!("bug report: not sent: {why}"),
        }
        self.data.report.finished(result);
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
        // An online match goes on while its menu's options are open: the
        // results show when the player is back in game.
        if !shown || self.over_reported || self.ui.screen() != Screen::InGame {
            return Vec::new();
        }
        self.report_over()
    }

    /// The results to the menus (`MatchOver`), once.
    fn report_over(&mut self) -> Vec<Outcome> {
        self.data.results = self.session().and_then(|s| s.results.clone());
        log::info!("match over: the results show");
        let out = self.act(UiAction::MatchOver);
        self.over_reported = self.ui.screen() == Screen::Results;
        out
    }

    /// Writes the settings, profiles and own loadouts changed since the last call (the
    /// app calls it about once a second, so a dragged slider is not saved
    /// every frame); a failure is a warning.
    pub fn flush(&mut self) {
        if let Some(s) = self.unsaved_settings.take() {
            for change in crate::logging::settings_changes(&self.logged_settings, &s) {
                log::info!("settings: {change}");
            }
            self.logged_settings.clone_from(&s);
            if let Err(e) = self.store.save_settings(&s) {
                log::warn!("saving the settings: {e:#}");
            }
        }
        if let Some(p) = self.unsaved_profiles.take()
            && let Err(e) = self.store.save_profiles(&p)
        {
            log::warn!("saving the profiles: {e:#}");
        }
        if let Some(l) = self.unsaved_loadouts.take()
            && let Err(e) = self.store.save_file(olr_ui::LOADOUTS_FILE, &l)
        {
            log::warn!("saving the loadouts: {e:#}");
        }
    }

    /// At exit: the playing game's replay saved, the settings written, an
    /// online game left (its router mapping's deletion started: the
    /// caller waits for it with [`Shell::take_unmapping`] once the window
    /// closed).
    pub fn finish(&mut self) {
        // A quit that did not ask (an autoplay's, `--autoquit-after`, an
        // error) keeps the editor's unsaved changes in a recovery copy.
        if let Some(h) = &mut self.editor {
            for m in h.flush_recovery() {
                if let crate::editor::HostOut::Message(m) = m {
                    log::warn!("editor: {m}");
                }
            }
        }
        if let Some(h) = &mut self.mod_editor {
            h.flush_recovery();
            for w in h.take_warnings() {
                log::warn!("mod editor: {w}");
            }
        }
        self.leave_online();
        self.end();
        self.close_internet();
        self.wait_for_replays(REPLAY_STOP_WAIT);
        self.flush();
    }
}

/// The weapon selection's menu for `a`'s game: a Deathmatch's mod weapons
/// with its loadout's (`None` for another game).
pub fn weapon_menu(a: &Assets) -> Option<WeaponMenu> {
    let Game::Deathmatch { options, .. } = &a.game else { return None };
    let rules = a.m.rules(&options.weapons);
    let names: Vec<String> = a.m.sim.weapons().iter().map(|w| w.name.clone()).collect();
    let enabled = names.iter().map(|n| rules.is_enabled(n)).collect();
    let (fallback, unique) = (rules.find_enabled_weapon(), rules.unique());
    Some(WeaponMenu { names, enabled, fallback, seed: options.seed, unique })
}
