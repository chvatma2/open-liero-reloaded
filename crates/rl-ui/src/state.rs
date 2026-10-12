//! The screen state machine (spec §8.3, §21 G1): which screen shows, and
//! what the app must do when the player acts. Pure logic: the app feeds
//! [`UiAction`]s in (from the widgets, and its own reports such as a match
//! having started) and carries out the [`AppCommand`]s that come back.
//!
//! - **Menus:** Main opens LocalPlay, Profiles, Options, Replays, Import
//!   and About; Back (Esc) returns to Main.
//! - **Online** (plan 10 task 11): Main opens Multiplayer, which opens
//!   Host (Back returns there). `Host(prefs)` and `Join`/`JoinLan` emit
//!   `HostGame`/`JoinGame` and wait for the app (`LobbyJoined` opens the
//!   Lobby, `OnlineFailed(why)` returns with the reason; Back while
//!   waiting gives up: `LeaveOnline`). In the Lobby the `Lobby*` actions
//!   become `AppCommand::Lobby(LobbyCommand)`, the weapon screen opened
//!   from it answers with `Loadout`, and Back or `LeaveLobby` leaves
//!   (`LeaveOnline`, to Multiplayer).
//! - **Matches:** Start emits one `StartMatch` with the next seed; until
//!   the app reports `MatchStarted` or `MatchFailed`, starting again and
//!   Back do nothing (no double matches). A Deathmatch starts with OLX's
//!   weapon selection (§21 G4): its Done arms the player (`ArmPlayer`,
//!   into the match's options) and the match plays. In game, Back toggles
//!   the pause menu, which opens the options (Back returns to it) or
//!   leaves the match; the end shows the results, whose Start is a
//!   rematch on the next seed (the match over ends first, and the weapons
//!   are chosen again) and whose Save replay keeps the match's replay.
//!   Restart (F5) plays the game again in place: a Deathmatch's rematch
//!   (same weapons), the sandbox or a replay from the start.
//! - **Options** apply and save only what changed.
//! - **Import** (plan 7 task 7): the Import screen's Import starts one
//!   import (`StartImport`; the path remembered in the settings) until
//!   the app reports `ImportFinished`; another press does nothing. The
//!   import runs on while the player leaves the screen, but no game
//!   starts until it ends (the packs it writes are not loaded half-way).

use std::net::SocketAddr;
use std::path::PathBuf;

use rl_content::MatchOptions;
use rl_input::Bindings;
use rl_sim::MAX_SLOTS;

use crate::match_prefs::MatchPrefs;
use crate::online::{HostPrefs, LobbyCommand, Password};
use crate::profiles::Profiles;
use crate::settings::{AudioSettings, Settings, VideoSettings};

/// A screen (§21 G1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Screen {
    Main,
    /// Mod, level, Deathmatch settings, bots and difficulty.
    LocalPlay,
    Profiles,
    Options,
    /// OLX's weapon selection before the match (§21 G4).
    WeaponSelect,
    /// The match with its HUD, and the pause menu.
    InGame,
    Results,
    /// The replays saved in the data directory (and any file).
    Replays,
    /// Content import.
    Import,
    /// Credits and licenses (§21 G9).
    About,
    /// Games on the network, joining by address (plan 10 task 11).
    Multiplayer,
    /// Hosting a game: its name, visibility, password, size and port.
    Host,
    /// The lobby of a hosted or joined game.
    Lobby,
}

impl Screen {
    /// The screens the main menu opens.
    pub const MENUS: [Screen; 7] = [
        Screen::LocalPlay,
        Screen::Multiplayer,
        Screen::Profiles,
        Screen::Options,
        Screen::Replays,
        Screen::Import,
        Screen::About,
    ];
}

/// What the player did, or what the app reports.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    /// A main-menu button ([`Screen::MENUS`] only).
    Open(Screen),
    /// Esc or a Back button.
    Back,
    /// Local play's Start, or the results' rematch.
    StartMatch,
    /// The main menu's sandbox (the player and idle targets).
    StartSandbox,
    /// The replays screen: play this file.
    PlayReplay(PathBuf),
    /// F5: the game again from the start (a Deathmatch's rematch).
    Restart,
    /// The pause menu's Resume.
    Resume,
    /// The pause menu's Quit to menu, or the results' Back to menu.
    LeaveMatch,
    /// The weapon selection's Done: the weapons by mod id, slot by slot,
    /// and their names (remembered in the profile; none: nothing to
    /// remember, as when there was no choice).
    ChooseWeapons {
        weapons: [i32; MAX_SLOTS],
        names: Vec<String>,
    },
    /// The results' Save replay.
    SaveReplay,
    /// The app: the replay was saved as this file, or why not.
    ReplaySaved(Result<String, String>),
    Quit,
    SetVideo(VideoSettings),
    SetAudio(AudioSettings),
    /// The options' key bindings.
    SetBindings(Bindings),
    SetMatch(MatchPrefs),
    SetProfiles(Profiles),
    /// The app: the match or sandbox asked for is ready.
    MatchStarted,
    /// The app: it could not be started (missing or broken content).
    MatchFailed(String),
    /// The app: the match is over and its results are ready.
    MatchOver,
    /// The Import screen's Import: the OLX game directory typed.
    StartImport(String),
    /// The Import screen's Cancel.
    CancelImport,
    /// The app: the import has ended (its report is the screen's data).
    ImportFinished,
    /// The Host screen's Start hosting.
    Host(HostPrefs),
    /// The Multiplayer screen's Join: `host[:port]` as typed and the
    /// password (none when empty).
    Join {
        addr: String,
        password: Option<Password>,
    },
    /// A game on the network was clicked.
    JoinLan(SocketAddr),
    LobbyReady(bool),
    LobbySpectate(bool),
    LobbyChat(String),
    /// The host changed the match's setup.
    LobbySetup(MatchPrefs),
    LobbyKick {
        member: u8,
        ban: bool,
    },
    /// The host moved a member to the spectators or the players.
    LobbyMove {
        member: u8,
        spectator: bool,
    },
    LobbyStart,
    /// The host starts although players are not ready: they watch.
    LobbyForceStart,
    /// This player's weapons by mod id (none: random).
    LobbyLoadout(Option<[i32; MAX_SLOTS]>),
    /// Back from the lobby, or its confirmed leave.
    LeaveLobby,
    /// The app: hosting or joining failed, or the lobby was lost.
    OnlineFailed(String),
    /// The app: the lobby is up (hosted, or joined and accepted).
    LobbyJoined,
}

/// What the app must do.
#[derive(Clone, Debug, PartialEq)]
pub enum AppCommand {
    /// Build and start this Deathmatch (`rl_content::build_match`), then
    /// report [`UiAction::MatchStarted`] or [`UiAction::MatchFailed`].
    StartMatch(MatchOptions),
    /// Start the sandbox, then report as for `StartMatch`.
    StartSandbox,
    /// Play this replay, then report as for `StartMatch`.
    PlayReplay(PathBuf),
    /// Play the running game again from the start, in place.
    Restart,
    /// Stop the match or sandbox playing.
    EndMatch,
    /// Pause or resume the match (the pause menu).
    SetPaused(bool),
    /// The weapon selection's choice: the human (worm 0) starts the match
    /// with these weapons (`rl_content::Player::HumanWith` in its options,
    /// so the replay holds them).
    ArmPlayer([i32; MAX_SLOTS]),
    /// Keep the finished match's replay as a file of its own (then report
    /// [`UiAction::ReplaySaved`]).
    SaveReplay,
    /// Window size, fullscreen, pacing, filter, interpolation and UI scale,
    /// through `rl-game`'s paced window changes.
    ApplyVideo(VideoSettings),
    ApplyAudio(AudioSettings),
    /// The keys the player now uses (the profile's own, else the options').
    ApplyBindings(Bindings),
    /// Write `settings.ron` (`Store::save_settings`).
    SaveSettings(Settings),
    /// Write `profiles.ron` (`Store::save_profiles`).
    SaveProfiles(Profiles),
    /// Import the OLX game directory at this path (as typed, cleaned:
    /// `~` and a relative path are the app's, `import::expand_typed_path`)
    /// into the user data's packs off the UI thread, then report
    /// [`UiAction::ImportFinished`].
    StartImport(PathBuf),
    /// Stop the import after the item it is on.
    CancelImport,
    /// Host a game with these choices, then report [`UiAction::LobbyJoined`]
    /// or [`UiAction::OnlineFailed`].
    HostGame(HostPrefs),
    /// Join the game at `addr` (`host[:port]` as typed; the port defaults
    /// to `settings.online.port`), then report as for `HostGame`.
    JoinGame {
        addr: String,
        password: Option<Password>,
    },
    /// What the player asks of the lobby they are in.
    Lobby(LobbyCommand),
    /// Leave the lobby, or give up hosting or joining.
    LeaveOnline,
    Quit,
}

/// What plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Game {
    Match,
    Sandbox,
    Replay,
}

/// What is running behind the menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Play {
    /// Nothing.
    None,
    /// Asked for; the app has not answered yet. `from` is the screen to
    /// return to if it fails.
    Starting {
        from: Screen,
        game: Game,
    },
    Running(Game),
}

/// The menus' state.
#[derive(Clone, Debug)]
pub struct UiState {
    screen: Screen,
    settings: Settings,
    profiles: Profiles,
    play: Play,
    paused: bool,
    /// The options were opened from the pause menu: Back returns there.
    options_in_game: bool,
    /// The match playing has had its replay kept (Save replay, once).
    replay_saved: bool,
    /// The seed of the next match.
    seed: u32,
    /// Shown on the screen: why the last start failed, or a replay saved.
    message: Option<String>,
    /// An import runs (`StartImport` until `ImportFinished`).
    importing: bool,
    /// Hosting or joining was asked for from this screen and the app has
    /// not answered (`LobbyJoined` or `OnlineFailed`).
    connecting: Option<Screen>,
    /// A lobby is up (`LobbyJoined` until the player leaves it).
    online: bool,
}

impl UiState {
    /// The main menu with the loaded settings and profiles; matches get
    /// seeds `seed`, `seed + 1`, ... (the app picks the first).
    pub fn new(settings: Settings, profiles: Profiles, seed: u32) -> UiState {
        UiState {
            screen: Screen::Main,
            settings,
            profiles,
            play: Play::None,
            paused: false,
            options_in_game: false,
            replay_saved: false,
            seed,
            message: None,
            importing: false,
            connecting: None,
            online: false,
        }
    }

    pub fn screen(&self) -> Screen {
        self.screen
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn profiles(&self) -> &Profiles {
        &self.profiles
    }

    /// The keys the player uses: the selected profile's, else the options'.
    pub fn bindings(&self) -> &Bindings {
        self.profiles.selected().and_then(|p| p.bindings.as_ref()).unwrap_or(&self.settings.input)
    }

    /// A start was asked for and the app has not answered.
    pub fn is_starting(&self) -> bool {
        matches!(self.play, Play::Starting { .. })
    }

    /// What plays (or is starting), if anything.
    pub fn game(&self) -> Option<Game> {
        match self.play {
            Play::None => None,
            Play::Starting { game, .. } | Play::Running(game) => Some(game),
        }
    }

    /// An import runs: no game starts.
    pub fn is_importing(&self) -> bool {
        self.importing
    }

    /// Hosting or joining was asked for and the app has not answered.
    pub fn is_connecting(&self) -> bool {
        self.connecting.is_some()
    }

    /// A lobby is up.
    pub fn is_online(&self) -> bool {
        self.online
    }

    /// The pause menu is up.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// The results' Save replay has kept this match's replay.
    pub fn replay_saved(&self) -> bool {
        self.replay_saved
    }

    /// Why the last start failed, or where the replay was saved, until
    /// the next screen change or start.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Applies `action`; returns what the app must do, in order.
    pub fn update(&mut self, action: UiAction) -> Vec<AppCommand> {
        match action {
            UiAction::Open(s) => {
                let from_main =
                    self.screen == Screen::Main && Screen::MENUS.contains(&s) && !self.is_starting();
                let to_host =
                    self.screen == Screen::Multiplayer && s == Screen::Host && !self.is_connecting();
                // The lobby's weapon choice (plan 10 ruling N19).
                let to_weapons = self.screen == Screen::Lobby && s == Screen::WeaponSelect;
                if from_main || to_host || to_weapons {
                    self.go(s);
                } else if self.screen == Screen::InGame && self.paused && s == Screen::Options {
                    self.options_in_game = true;
                    self.go(s);
                }
                Vec::new()
            }
            UiAction::Back => self.back(),
            UiAction::StartMatch => self.start_match(),
            UiAction::StartSandbox => {
                if self.screen != Screen::Main || self.play != Play::None || self.refused_while_importing() {
                    return Vec::new();
                }
                self.message = None;
                self.play = Play::Starting { from: Screen::Main, game: Game::Sandbox };
                vec![AppCommand::StartSandbox]
            }
            UiAction::PlayReplay(path) => {
                if self.screen != Screen::Replays || self.play != Play::None || self.refused_while_importing()
                {
                    return Vec::new();
                }
                self.message = None;
                self.play = Play::Starting { from: Screen::Replays, game: Game::Replay };
                vec![AppCommand::PlayReplay(path)]
            }
            UiAction::Restart => {
                let can = match self.screen {
                    Screen::InGame => !self.paused,
                    Screen::Results => true,
                    _ => false,
                };
                if !can || !matches!(self.play, Play::Running(_)) {
                    return Vec::new();
                }
                self.go(Screen::InGame);
                // A rematch in place is another match to save.
                self.replay_saved = false;
                vec![AppCommand::Restart]
            }
            UiAction::Resume => self.set_paused(false),
            UiAction::LeaveMatch => self.leave(),
            UiAction::ChooseWeapons { weapons, names } => self.choose_weapons(weapons, names),
            UiAction::SaveReplay => {
                if self.screen == Screen::Results
                    && self.play == Play::Running(Game::Match)
                    && !self.replay_saved
                {
                    self.message = None;
                    vec![AppCommand::SaveReplay]
                } else {
                    Vec::new()
                }
            }
            UiAction::ReplaySaved(r) => {
                if self.screen == Screen::Results {
                    self.replay_saved = r.is_ok();
                    self.message = Some(match r {
                        Ok(name) => format!("Saved as {name}"),
                        Err(e) => format!("Not saved: {e}"),
                    });
                }
                Vec::new()
            }
            UiAction::Quit => vec![AppCommand::Quit],
            UiAction::SetVideo(mut v) => {
                v.sanitize();
                if v == self.settings.video {
                    return Vec::new();
                }
                self.settings.video = v.clone();
                vec![AppCommand::ApplyVideo(v), self.save_settings()]
            }
            UiAction::SetAudio(mut a) => {
                a.sanitize();
                if a == self.settings.audio {
                    return Vec::new();
                }
                self.settings.audio = a.clone();
                vec![AppCommand::ApplyAudio(a), self.save_settings()]
            }
            UiAction::SetBindings(mut b) => {
                crate::settings::sanitize_bindings(&mut b);
                if b == self.settings.input {
                    return Vec::new();
                }
                let before = self.bindings().clone();
                self.settings.input = b;
                let mut out = self.apply_bindings_if_changed(&before);
                out.push(self.save_settings());
                out
            }
            UiAction::SetMatch(mut m) => {
                m.sanitize();
                if m == self.settings.gameplay.match_prefs {
                    return Vec::new();
                }
                self.settings.gameplay.match_prefs = m;
                vec![self.save_settings()]
            }
            UiAction::SetProfiles(mut p) => {
                p.sanitize();
                if p == self.profiles {
                    return Vec::new();
                }
                let before = self.bindings().clone();
                self.profiles = p;
                let mut out = vec![AppCommand::SaveProfiles(self.profiles.clone())];
                out.extend(self.apply_bindings_if_changed(&before));
                out
            }
            UiAction::MatchStarted => {
                if let Play::Starting { game, .. } = self.play {
                    self.play = Play::Running(game);
                    self.paused = false;
                    self.replay_saved = false;
                    // OLX's weapon selection before a Deathmatch (§21 G4).
                    self.go(if game == Game::Match { Screen::WeaponSelect } else { Screen::InGame });
                }
                Vec::new()
            }
            UiAction::MatchFailed(why) => {
                if let Play::Starting { from, .. } = self.play {
                    self.play = Play::None;
                    self.go(from);
                    self.message = Some(why);
                }
                Vec::new()
            }
            UiAction::MatchOver => {
                if self.screen != Screen::InGame || !matches!(self.play, Play::Running(_)) {
                    return Vec::new();
                }
                self.go(Screen::Results);
                // The app's pause and this state never disagree.
                if std::mem::take(&mut self.paused) { vec![AppCommand::SetPaused(false)] } else { Vec::new() }
            }
            UiAction::StartImport(path) => self.start_import(path),
            UiAction::CancelImport => {
                if self.importing {
                    vec![AppCommand::CancelImport]
                } else {
                    Vec::new()
                }
            }
            UiAction::ImportFinished => {
                self.importing = false;
                Vec::new()
            }
            UiAction::Host(prefs) => self.host(prefs),
            UiAction::Join { addr, password } => self.join_at(addr, password, true),
            UiAction::JoinLan(addr) => self.join_at(addr.to_string(), None, false),
            UiAction::LobbyJoined => {
                if self.connecting.take().is_some() {
                    self.online = true;
                    self.go(Screen::Lobby);
                }
                Vec::new()
            }
            UiAction::OnlineFailed(why) => {
                match (self.connecting.take(), self.screen) {
                    (Some(from), _) => {
                        self.go(from);
                        self.message = Some(why);
                    }
                    (None, Screen::Lobby | Screen::WeaponSelect) if self.online => {
                        self.online = false;
                        self.go(Screen::Multiplayer);
                        self.message = Some(why);
                    }
                    _ => {}
                }
                Vec::new()
            }
            UiAction::LeaveLobby => self.leave_online(),
            UiAction::LobbyReady(on) => self.lobby(LobbyCommand::Ready(on)),
            UiAction::LobbySpectate(on) => self.lobby(LobbyCommand::Spectate(on)),
            UiAction::LobbyChat(text) => {
                let text = text.trim();
                if text.is_empty() {
                    return Vec::new();
                }
                let text: String = text.chars().take(crate::online::CHAT_CHARS).collect();
                self.lobby(LobbyCommand::Chat(text))
            }
            UiAction::LobbySetup(m) => self.lobby(LobbyCommand::Setup(m)),
            UiAction::LobbyKick { member, ban } => self.lobby(LobbyCommand::Kick { member, ban }),
            UiAction::LobbyMove { member, spectator } => self.lobby(LobbyCommand::Move { member, spectator }),
            UiAction::LobbyStart => self.lobby(LobbyCommand::Start),
            UiAction::LobbyForceStart => self.lobby(LobbyCommand::ForceStart),
            UiAction::LobbyLoadout(w) => self.lobby(LobbyCommand::Loadout(w)),
        }
    }

    /// A lobby command, from the lobby screen only.
    fn lobby(&mut self, c: LobbyCommand) -> Vec<AppCommand> {
        if self.screen == Screen::Lobby && self.online { vec![AppCommand::Lobby(c)] } else { Vec::new() }
    }

    /// Start hosting (the Host screen): the choices fixed and remembered.
    fn host(&mut self, mut prefs: HostPrefs) -> Vec<AppCommand> {
        if self.screen != Screen::Host || self.is_connecting() {
            return Vec::new();
        }
        prefs.sanitize();
        if prefs.name.is_empty() {
            prefs.name = self.profiles.selected().map(|p| p.name.clone()).unwrap_or_default();
        }
        self.message = None;
        self.connecting = Some(Screen::Host);
        let mut out = vec![AppCommand::HostGame(prefs.clone())];
        // The password is never saved: it alone changing saves nothing.
        let saved = HostPrefs { password: Password::default(), ..prefs.clone() };
        if self.settings.online.host != saved {
            self.settings.online.host = saved;
            out.push(self.save_settings());
        }
        out
    }

    /// Join a game (the Multiplayer screen): an address typed is remembered,
    /// one clicked in the LAN list is not.
    fn join_at(&mut self, addr: String, password: Option<Password>, remember: bool) -> Vec<AppCommand> {
        let addr: String = addr.trim().chars().take(crate::online::ADDRESS_CHARS).collect();
        if self.screen != Screen::Multiplayer || self.is_connecting() || addr.is_empty() {
            return Vec::new();
        }
        self.message = None;
        self.connecting = Some(Screen::Multiplayer);
        let password = password.filter(|p| !p.is_empty());
        let mut out = vec![AppCommand::JoinGame { addr: addr.clone(), password }];
        if remember && self.settings.online.last_address != addr {
            self.settings.online.last_address = addr;
            out.push(self.save_settings());
        }
        out
    }

    /// Leaves the lobby, or gives up hosting or joining.
    fn leave_online(&mut self) -> Vec<AppCommand> {
        if let Some(from) = self.connecting.take() {
            self.go(from);
            return vec![AppCommand::LeaveOnline];
        }
        if !self.online || !matches!(self.screen, Screen::Lobby | Screen::WeaponSelect) {
            return Vec::new();
        }
        self.online = false;
        self.go(Screen::Multiplayer);
        vec![AppCommand::LeaveOnline]
    }

    /// Import's button: one import at a time, from the Import screen, with
    /// no game playing; the path is remembered.
    fn start_import(&mut self, path: String) -> Vec<AppCommand> {
        let path = crate::import::clean_typed_path(&path).to_string();
        if self.screen != Screen::Import || self.importing || self.play != Play::None || path.is_empty() {
            return Vec::new();
        }
        self.importing = true;
        self.message = None;
        let mut out = vec![AppCommand::StartImport(PathBuf::from(&path))];
        if self.settings.import.last_path != path {
            self.settings.import.last_path = path;
            out.push(self.save_settings());
        }
        out
    }

    /// A start while an import runs is refused with a message (its packs
    /// are being written).
    fn refused_while_importing(&mut self) -> bool {
        if self.importing {
            self.message = Some("An import is running: start when it is done.".into());
        }
        self.importing
    }

    fn go(&mut self, s: Screen) {
        self.screen = s;
        self.message = None;
        self.options_in_game &= s == Screen::Options;
    }

    fn back(&mut self) -> Vec<AppCommand> {
        if self.is_starting() {
            return Vec::new();
        }
        match self.screen {
            Screen::Main => Vec::new(),
            Screen::Host | Screen::Multiplayer if self.is_connecting() => self.leave_online(),
            Screen::Host => {
                self.go(Screen::Multiplayer);
                Vec::new()
            }
            Screen::Lobby => self.leave_online(),
            // The lobby's weapon choice returns to the lobby.
            Screen::WeaponSelect if self.online => {
                self.go(Screen::Lobby);
                Vec::new()
            }
            Screen::InGame => self.set_paused(!self.paused),
            Screen::Options if self.options_in_game => {
                self.options_in_game = false;
                self.go(Screen::InGame);
                Vec::new()
            }
            Screen::WeaponSelect | Screen::Results => self.leave(),
            _ => {
                self.go(Screen::Main);
                Vec::new()
            }
        }
    }

    fn set_paused(&mut self, on: bool) -> Vec<AppCommand> {
        if self.screen != Screen::InGame || self.paused == on {
            return Vec::new();
        }
        self.paused = on;
        vec![AppCommand::SetPaused(on)]
    }

    /// Leaves a running match: from the pause menu, the weapon selection
    /// or the results.
    fn leave(&mut self) -> Vec<AppCommand> {
        let can = match self.screen {
            Screen::InGame => self.paused,
            Screen::WeaponSelect | Screen::Results => true,
            _ => false,
        };
        if !can || !matches!(self.play, Play::Running(_)) {
            return Vec::new();
        }
        self.play = Play::None;
        self.paused = false;
        self.go(Screen::Main);
        vec![AppCommand::EndMatch]
    }

    /// The weapon selection's Done: the player armed, the match on; the
    /// names (if any) remembered in the profile playing, as OLX's profile
    /// keeps its weapon slots (`CWormHuman.cpp:924-926`, saved on Done: `:939`, `:948`).
    fn choose_weapons(&mut self, weapons: [i32; MAX_SLOTS], names: Vec<String>) -> Vec<AppCommand> {
        let lobby = self.online && self.screen == Screen::WeaponSelect && self.play == Play::None;
        if !lobby && (self.screen != Screen::WeaponSelect || self.play != Play::Running(Game::Match)) {
            return Vec::new();
        }
        // The lobby's choice goes to the lobby, not into a local match.
        let mut out = if lobby {
            self.go(Screen::Lobby);
            vec![AppCommand::Lobby(LobbyCommand::Loadout(Some(weapons)))]
        } else {
            self.go(Screen::InGame);
            vec![AppCommand::ArmPlayer(weapons)]
        };
        // The profile `Profiles::selected` gives.
        let selected =
            if self.profiles.selected < self.profiles.list.len() { self.profiles.selected } else { 0 };
        if let Some(p) = self.profiles.list.get_mut(selected)
            && !names.is_empty()
            && p.weapons != names
        {
            p.weapons = names;
            out.push(AppCommand::SaveProfiles(self.profiles.clone()));
        }
        out
    }

    fn start_match(&mut self) -> Vec<AppCommand> {
        if self.refused_while_importing() {
            return Vec::new();
        }
        let mut out = match (self.screen, self.play) {
            (Screen::LocalPlay, Play::None) => Vec::new(),
            // A rematch: the match over ends first.
            (Screen::Results, Play::Running(Game::Match)) => vec![AppCommand::EndMatch],
            _ => return Vec::new(),
        };
        let seed = self.seed;
        self.seed = self.seed.wrapping_add(1);
        self.message = None;
        self.play = Play::Starting { from: Screen::LocalPlay, game: Game::Match };
        out.push(AppCommand::StartMatch(self.settings.gameplay.match_prefs.options(seed)));
        out
    }

    fn save_settings(&self) -> AppCommand {
        AppCommand::SaveSettings(self.settings.clone())
    }

    fn apply_bindings_if_changed(&self, before: &Bindings) -> Vec<AppCommand> {
        let now = self.bindings();
        if now == before { Vec::new() } else { vec![AppCommand::ApplyBindings(now.clone())] }
    }
}
