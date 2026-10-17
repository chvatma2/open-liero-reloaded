//! The screen state machine (spec §8.3, §21 G1): which screen shows, and
//! what the app must do when the player acts. Pure logic: the app feeds
//! [`UiAction`]s in (from the widgets, and its own reports such as a match
//! having started) and carries out the [`AppCommand`]s that come back.
//!
//! - **Menus:** Main opens LocalPlay, Profiles, Options, Replays
//!   and About; Back (Esc) returns to Main.
//! - **Online** (plan 10 task 11): Main opens Multiplayer, which opens
//!   Host (Back returns there). `Host(prefs)` and `Join`/`JoinLan` emit
//!   `HostGame`/`JoinGame` and wait for the app (`LobbyJoined` opens the
//!   Lobby, `OnlineFailed(why)` returns with the reason; Back while
//!   waiting gives up: `LeaveOnline`). In the Lobby the `Lobby*` actions
//!   become `AppCommand::Lobby(LobbyCommand)`, the weapon screen opened
//!   from it answers with `Loadout`, and Back or `LeaveLobby` leaves
//!   (`LeaveOnline`, to Multiplayer). The download prompt's Download is
//!   `ApproveDownloads`, its Leave leaves (plan 11). Finding games (plan
//!   12a): the Internet tab's Join is `JoinListed` (a `JoinGame` that
//!   expects the listed key; plan 12b: without an address too, the app
//!   joins it through the relays), its "Block this host" `BlockHost` (the app's
//!   list hides it at once; the key is saved); the Direct tab's Join takes
//!   an address or an invite code alike (the app tells them apart); the
//!   options' Online tab sets the finding settings (`SetFinding`, saved).
//! - **Online matches** (plan 10 task 12): while a lobby is up the app
//!   reports `MatchStarted` when the host's match starts (from any
//!   screen: InGame, no weapon selection, it was the lobby's). Esc opens
//!   the menu over the match, which goes on (nothing pauses online); its
//!   Quit to menu leaves the online game (`EndMatch`, `LeaveOnline`, to
//!   Multiplayer). `MatchOver` shows the results, whose Back returns to
//!   the Lobby (`EndMatch`; no rematch, no replay kept: the app saves
//!   each match's replay). `LobbyChat` works in game and on the results
//!   too. `OnlineFailed` (the connection lost, a kick) ends the match and
//!   returns to Multiplayer from any screen. `HostLost` (plan 12c: the host
//!   gone and nobody could take the match over) ends it too and opens the
//!   Host lost screen, whose Replays opens the Replays screen and whose
//!   Back returns to Multiplayer. `MatchLeft` (a late rejoiner whose match
//!   is over) ends it and shows the Lobby.
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
//! - **Options** apply and save only what changed; their Clear of the
//!   download cache is `ClearDownloadCache` (the app refuses it while an
//!   online game is on).

use std::net::SocketAddr;
use std::path::PathBuf;

use olr_content::MatchOptions;
use olr_input::Bindings;
use olr_sim::MAX_SLOTS;

use crate::match_prefs::MatchPrefs;
use crate::online::{FindingSettings, HostPrefs, LobbyCommand, Password, key_hex};
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
    /// Credits and licenses (§21 G9).
    About,
    /// Games on the network, joining by address (plan 10 task 11).
    Multiplayer,
    /// Hosting a game: its name, visibility, password, size and port.
    Host,
    /// The lobby of a hosted or joined game.
    Lobby,
    /// The match ended because its host was lost and nobody could take it
    /// over (plan 12c): why, and where its replay is.
    HostLost,
    /// The level editor's packs: edit, copy, new, recoveries (plan 14c).
    Editor,
}

impl Screen {
    /// The screens the main menu opens.
    pub const MENUS: [Screen; 7] = [
        Screen::LocalPlay,
        Screen::Multiplayer,
        Screen::Editor,
        Screen::Profiles,
        Screen::Options,
        Screen::Replays,
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
    /// The options' online chat key (a key the game keeps is refused).
    SetChatKey(olr_input::KeyCode),
    SetMatch(MatchPrefs),
    SetProfiles(Profiles),
    /// The app: the match or sandbox asked for is ready.
    MatchStarted,
    /// The app: it could not be started (missing or broken content).
    MatchFailed(String),
    /// The app: the match is over and its results are ready.
    MatchOver,
    /// The Host screen's Start hosting.
    Host(HostPrefs),
    /// The Multiplayer screen's Direct Join: `host[:port]` or an invite
    /// code (`olr1…`) as typed, and the password (none when empty).
    Join {
        addr: String,
        password: Option<Password>,
    },
    /// A game on the network was clicked.
    JoinLan(SocketAddr),
    /// The Internet tab's Join: the listed game whose host must prove it
    /// holds `key`, at `addr` (directly first, then through the relays,
    /// N46), or through the relays alone without one.
    JoinListed {
        addr: Option<SocketAddr>,
        key: [u8; 32],
        password: Option<Password>,
    },
    /// The Internet tab's "Block this host": its games are hidden from now
    /// on (remembered in `settings.online.finding.blocked`).
    BlockHost([u8; 32]),
    /// The options' Online tab: region, relays, STUN servers, the public
    /// address, blocked hosts.
    SetFinding(FindingSettings),
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
    /// The lobby's download prompt: Download (`true`) or Leave.
    LobbyDownload(bool),
    /// Back from the lobby, or its confirmed leave.
    LeaveLobby,
    /// The options' Clear of the download cache.
    ClearDownloadCache,
    /// The app: hosting or joining failed, or the lobby was lost.
    OnlineFailed(String),
    /// The app: the lobby is up (hosted, or joined and accepted).
    LobbyJoined,
    /// The app: the match's host is gone and no player could take the
    /// match over (spec §3.7), or this host stepped down: the online game
    /// is over, the Host lost screen opens with the reason (the app puts
    /// it in `MenuData::host_lost`).
    HostLost(String),
    /// The app: this player's online match view ends without results (a
    /// late rejoiner's, plan 12c N72: the match it reached is over): the
    /// Lobby shows.
    MatchLeft,
    /// The Editor screen's Edit (a pack of the data directory's).
    EditLevel(String),
    /// The Editor screen's Edit a copy: level pack `from` copied as `name`.
    CopyLevel {
        from: String,
        name: String,
    },
    /// The Editor screen's New level.
    NewLevel {
        name: String,
        size: (u32, u32),
        theme: String,
    },
    /// The Editor screen's Recover and Discard of a recovery copy.
    Recover(String),
    DiscardRecovery(String),
}

/// What the app must do.
#[derive(Clone, Debug, PartialEq)]
pub enum AppCommand {
    /// Build and start this Deathmatch (`olr_content::build_match`), then
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
    /// with these weapons (`olr_content::Player::HumanWith` in its options,
    /// so the replay holds them).
    ArmPlayer([i32; MAX_SLOTS]),
    /// Keep the finished match's replay as a file of its own (then report
    /// [`UiAction::ReplaySaved`]).
    SaveReplay,
    /// Window size, fullscreen, pacing, filter, interpolation and UI scale,
    /// through `olr-game`'s paced window changes.
    ApplyVideo(VideoSettings),
    ApplyAudio(AudioSettings),
    /// The keys the player now uses (the profile's own, else the options').
    ApplyBindings(Bindings),
    /// Write `settings.ron` (`Store::save_settings`).
    SaveSettings(Settings),
    /// Write `profiles.ron` (`Store::save_profiles`).
    SaveProfiles(Profiles),
    /// Host a game with these choices, then report [`UiAction::LobbyJoined`]
    /// or [`UiAction::OnlineFailed`].
    HostGame(HostPrefs),
    /// Join the game at `addr` (`host[:port]` as typed, the port defaulting
    /// to `settings.online.port`, or an invite code: `is_invite`), then
    /// report as for `HostGame`. With `expected_host`, a host whose key is
    /// another is refused, and the join goes on through the relays when
    /// the address never answers (a listed game); without `addr`, it goes
    /// through the relays to `expected_host` alone (N46).
    JoinGame {
        addr: Option<String>,
        password: Option<Password>,
        expected_host: Option<[u8; 32]>,
    },
    /// Hide this host's games from the Internet tab now.
    BlockHost([u8; 32]),
    /// What the player asks of the lobby they are in.
    Lobby(LobbyCommand),
    /// Leave the lobby, or give up hosting or joining.
    LeaveOnline,
    /// Empty the download cache (the options; never while an online game
    /// is on: the app refuses then).
    ClearDownloadCache,
    /// Open the editor on level pack `id` (one of `MenuData::editable`).
    EditLevel(String),
    /// Copy level pack `from` into the data directory's packs as `name`
    /// (plan 14c R5), then edit it.
    CopyLevel {
        from: String,
        name: String,
    },
    /// Make a new level pack (`olr_edit::new::Template::Level`), then edit it.
    NewLevel {
        name: String,
        size: (u32, u32),
        theme: String,
    },
    /// Reopen recovery `id` (its unsaved changes), or remove it.
    Recover(String),
    DiscardRecovery(String),
    Quit,
}

/// What plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Game {
    Match,
    Sandbox,
    Replay,
    /// A lobby's match (plan 10).
    Online,
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

    /// Puts the online settings back to `s`, saving nothing: the app's
    /// command line flags host and join through the menus' own actions,
    /// which remember their choices, and a flag is not remembered (a later
    /// save, from the options, would write it).
    pub fn restore_online(&mut self, s: crate::online::OnlineSettings) {
        self.settings.online = s;
    }

    /// The seed of the next match, taken (a lobby's start: the host picks
    /// it as local play does).
    pub fn take_seed(&mut self) -> u32 {
        let seed = self.seed;
        self.seed = self.seed.wrapping_add(1);
        seed
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
                // The Host lost screen's Replays (the match's replay is there).
                let to_replays = self.screen == Screen::HostLost && s == Screen::Replays;
                if from_main || to_host || to_weapons || to_replays {
                    self.go(s);
                } else if self.screen == Screen::InGame && self.paused && s == Screen::Options {
                    self.options_in_game = true;
                    self.go(s);
                }
                Vec::new()
            }
            UiAction::EditLevel(_)
            | UiAction::CopyLevel { .. }
            | UiAction::NewLevel { .. }
            | UiAction::Recover(_)
            | UiAction::DiscardRecovery(_) => self.editor(action),
            UiAction::Back => self.back(),
            UiAction::StartMatch => self.start_match(),
            UiAction::StartSandbox => {
                if self.screen != Screen::Main || self.play != Play::None {
                    return Vec::new();
                }
                self.message = None;
                self.play = Play::Starting { from: Screen::Main, game: Game::Sandbox };
                vec![AppCommand::StartSandbox]
            }
            UiAction::PlayReplay(path) => {
                if self.screen != Screen::Replays || self.play != Play::None {
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
                if !can || !matches!(self.play, Play::Running(_)) || self.play == Play::Running(Game::Online)
                {
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
            UiAction::SetChatKey(k) => {
                let reserved = crate::settings::GAME_KEYS.iter().any(|(g, _)| *g == k);
                if reserved || k == self.settings.online.chat_key {
                    return Vec::new();
                }
                self.settings.online.chat_key = k;
                vec![self.save_settings()]
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
            // The lobby's match started (the host's start).
            UiAction::MatchStarted if self.online && !self.is_starting() => {
                self.play = Play::Running(Game::Online);
                self.paused = false;
                self.replay_saved = false;
                self.go(Screen::InGame);
                Vec::new()
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
            UiAction::Host(prefs) => self.host(prefs),
            UiAction::Join { addr, password } => self.join_at(Some(addr), password, true, None),
            UiAction::JoinLan(addr) => self.join_at(Some(addr.to_string()), None, false, None),
            UiAction::JoinListed { addr, key, password } => {
                self.join_at(addr.map(|a| a.to_string()), password, false, Some(key))
            }
            UiAction::BlockHost(key) => {
                if self.screen != Screen::Multiplayer {
                    return Vec::new();
                }
                let hex = key_hex(&key);
                let mut out = vec![AppCommand::BlockHost(key)];
                if !self.settings.online.finding.blocked.contains(&hex) {
                    let mut f = self.settings.online.finding.clone();
                    f.blocked.push(hex);
                    out.extend(self.set_finding(f));
                }
                out
            }
            UiAction::SetFinding(f) => self.set_finding(f),
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
                    (None, _) if self.online => {
                        self.online = false;
                        let mut out = Vec::new();
                        if self.play == Play::Running(Game::Online) {
                            self.play = Play::None;
                            out.push(AppCommand::EndMatch);
                        }
                        self.paused = false;
                        self.go(Screen::Multiplayer);
                        self.message = Some(why);
                        return out;
                    }
                    _ => {}
                }
                Vec::new()
            }
            UiAction::HostLost(_) if self.online && self.connecting.is_none() => {
                self.online = false;
                self.paused = false;
                let ended = std::mem::replace(&mut self.play, Play::None) == Play::Running(Game::Online);
                self.go(Screen::HostLost);
                if ended { vec![AppCommand::EndMatch] } else { Vec::new() }
            }
            UiAction::HostLost(_) => Vec::new(),
            UiAction::MatchLeft if self.online && self.play == Play::Running(Game::Online) => {
                self.play = Play::None;
                self.paused = false;
                self.go(Screen::Lobby);
                vec![AppCommand::EndMatch]
            }
            UiAction::MatchLeft => Vec::new(),
            UiAction::LeaveLobby => self.leave_online(),
            UiAction::LobbyReady(on) => self.lobby(LobbyCommand::Ready(on)),
            UiAction::LobbySpectate(on) => self.lobby(LobbyCommand::Spectate(on)),
            UiAction::LobbyChat(text) => {
                let text = text.trim();
                let here = matches!(self.screen, Screen::Lobby | Screen::InGame | Screen::Results);
                if text.is_empty() || !self.online || !here {
                    return Vec::new();
                }
                let text: String = text.chars().take(crate::online::CHAT_CHARS).collect();
                vec![AppCommand::Lobby(LobbyCommand::Chat(text))]
            }
            UiAction::LobbySetup(m) => self.lobby(LobbyCommand::Setup(m)),
            UiAction::LobbyKick { member, ban } => self.lobby(LobbyCommand::Kick { member, ban }),
            UiAction::LobbyMove { member, spectator } => self.lobby(LobbyCommand::Move { member, spectator }),
            UiAction::LobbyStart => self.lobby(LobbyCommand::Start),
            UiAction::LobbyForceStart => self.lobby(LobbyCommand::ForceStart),
            UiAction::LobbyLoadout(w) => self.lobby(LobbyCommand::Loadout(w)),
            UiAction::LobbyDownload(true) => self.lobby(LobbyCommand::ApproveDownloads),
            UiAction::LobbyDownload(false) if self.screen == Screen::Lobby => self.leave_online(),
            UiAction::LobbyDownload(false) => Vec::new(),
            UiAction::ClearDownloadCache if self.screen == Screen::Options => {
                vec![AppCommand::ClearDownloadCache]
            }
            UiAction::ClearDownloadCache => Vec::new(),
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

    /// The finding settings, sanitized; saved when they changed.
    fn set_finding(&mut self, mut f: FindingSettings) -> Vec<AppCommand> {
        f.sanitize();
        if self.settings.online.finding == f {
            return Vec::new();
        }
        self.settings.online.finding = f;
        vec![self.save_settings()]
    }

    /// Join a game (the Multiplayer screen): an address or invite typed is
    /// remembered, a game clicked in a list is not. A join without an
    /// address needs the host's key (a listed game's, through the relays).
    fn join_at(
        &mut self,
        addr: Option<String>,
        password: Option<Password>,
        remember: bool,
        expected_host: Option<[u8; 32]>,
    ) -> Vec<AppCommand> {
        let addr: Option<String> =
            addr.map(|a| a.trim().chars().take(crate::online::ADDRESS_CHARS).collect());
        let reachable = match &addr {
            Some(a) => !a.is_empty(),
            None => expected_host.is_some(),
        };
        if self.screen != Screen::Multiplayer || self.is_connecting() || !reachable {
            return Vec::new();
        }
        self.message = None;
        self.connecting = Some(Screen::Multiplayer);
        let password = password.filter(|p| !p.is_empty());
        let mut out = vec![AppCommand::JoinGame { addr: addr.clone(), password, expected_host }];
        if let Some(addr) = addr.filter(|a| remember && self.settings.online.last_address != *a) {
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

    fn go(&mut self, s: Screen) {
        self.screen = s;
        self.message = None;
        self.options_in_game &= s == Screen::Options;
    }

    /// The Editor screen's commands, only from it and with nothing playing.
    fn editor(&mut self, a: UiAction) -> Vec<AppCommand> {
        if self.screen != Screen::Editor || self.play != Play::None {
            return Vec::new();
        }
        vec![match a {
            UiAction::EditLevel(id) => AppCommand::EditLevel(id),
            UiAction::CopyLevel { from, name } => AppCommand::CopyLevel { from, name },
            UiAction::NewLevel { name, size, theme } => AppCommand::NewLevel { name, size, theme },
            UiAction::Recover(id) => AppCommand::Recover(id),
            UiAction::DiscardRecovery(id) => AppCommand::DiscardRecovery(id),
            _ => return Vec::new(),
        }]
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
            Screen::HostLost => {
                self.go(Screen::Multiplayer);
                Vec::new()
            }
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
    /// or the results. An online match's results return to the lobby; its
    /// menu's Quit leaves the online game.
    fn leave(&mut self) -> Vec<AppCommand> {
        let can = match self.screen {
            Screen::InGame => self.paused,
            Screen::WeaponSelect | Screen::Results => true,
            _ => false,
        };
        if !can || !matches!(self.play, Play::Running(_)) {
            return Vec::new();
        }
        let online = self.play == Play::Running(Game::Online);
        let results = self.screen == Screen::Results;
        self.play = Play::None;
        self.paused = false;
        match (online, results) {
            (true, true) => {
                self.go(Screen::Lobby);
                vec![AppCommand::EndMatch]
            }
            (true, false) => {
                self.online = false;
                self.go(Screen::Multiplayer);
                vec![AppCommand::EndMatch, AppCommand::LeaveOnline]
            }
            (false, _) => {
                self.go(Screen::Main);
                vec![AppCommand::EndMatch]
            }
        }
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
