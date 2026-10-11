//! The screen state machine (spec §8.3, §21 G1): which screen shows, and
//! what the app must do when the player acts. Pure logic: the app feeds
//! [`UiAction`]s in (from the widgets, and its own reports such as a match
//! having started) and carries out the [`AppCommand`]s that come back.
//!
//! - **Menus:** Main opens LocalPlay, Profiles, Options, Import and About;
//!   Back (Esc) returns to Main.
//! - **Matches:** Start emits one `StartMatch` with the next seed; until
//!   the app reports `MatchStarted` or `MatchFailed`, starting again and
//!   Back do nothing (no double matches). In game, Back toggles the pause
//!   menu, which can leave the match; the end shows the results, whose
//!   Start is a rematch on the next seed (the match over ends first).
//! - **Options** apply and save only what changed.

use rl_content::MatchOptions;
use rl_input::Bindings;

use crate::profiles::Profiles;
use crate::settings::{AudioSettings, HudStyle, MatchPrefs, Settings, VideoSettings};

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
    /// Content import.
    Import,
    /// Credits and licenses (§21 G9).
    About,
}

impl Screen {
    /// The screens the main menu opens.
    pub const MENUS: [Screen; 5] =
        [Screen::LocalPlay, Screen::Profiles, Screen::Options, Screen::Import, Screen::About];
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
    /// The pause menu's Resume.
    Resume,
    /// The pause menu's or the results' Leave.
    LeaveMatch,
    Quit,
    SetVideo(VideoSettings),
    SetAudio(AudioSettings),
    /// The options' key bindings.
    SetBindings(Bindings),
    SetMatch(MatchPrefs),
    SetHud(HudStyle),
    SetProfiles(Profiles),
    /// The app: the match or sandbox asked for is ready.
    MatchStarted,
    /// The app: it could not be started (missing or broken content).
    MatchFailed(String),
    /// The app: the match is over and its results are ready.
    MatchOver,
}

/// What the app must do.
#[derive(Clone, Debug, PartialEq)]
pub enum AppCommand {
    /// Build and start this Deathmatch (`rl_content::build_match`), then
    /// report [`UiAction::MatchStarted`] or [`UiAction::MatchFailed`].
    StartMatch(MatchOptions),
    /// Start the sandbox, then report as for `StartMatch`.
    StartSandbox,
    /// Stop the match or sandbox playing.
    EndMatch,
    /// Pause or resume the match (the pause menu).
    SetPaused(bool),
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
    Quit,
}

/// What is running behind the menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Play {
    /// Nothing.
    None,
    /// Asked for; the app has not answered yet. `from` is the screen to
    /// return to if it fails.
    Starting { from: Screen },
    /// A match or the sandbox.
    Running,
}

/// The menus' state.
#[derive(Clone, Debug)]
pub struct UiState {
    screen: Screen,
    settings: Settings,
    profiles: Profiles,
    play: Play,
    paused: bool,
    /// The seed of the next match.
    seed: u32,
    /// Shown on the menu: why the last start failed.
    message: Option<String>,
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
            seed,
            message: None,
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

    /// The pause menu is up.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Why the last start failed, until the next screen change or start.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// Applies `action`; returns what the app must do, in order.
    pub fn update(&mut self, action: UiAction) -> Vec<AppCommand> {
        match action {
            UiAction::Open(s) => {
                if self.screen == Screen::Main && Screen::MENUS.contains(&s) && !self.is_starting() {
                    self.go(s);
                }
                Vec::new()
            }
            UiAction::Back => self.back(),
            UiAction::StartMatch => self.start_match(),
            UiAction::StartSandbox => {
                if self.screen != Screen::Main || self.play != Play::None {
                    return Vec::new();
                }
                self.message = None;
                self.play = Play::Starting { from: Screen::Main };
                vec![AppCommand::StartSandbox]
            }
            UiAction::Resume => self.set_paused(false),
            UiAction::LeaveMatch => self.leave(),
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
            UiAction::SetBindings(b) => {
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
            UiAction::SetHud(h) => {
                if h == self.settings.gameplay.hud {
                    return Vec::new();
                }
                self.settings.gameplay.hud = h;
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
                if self.is_starting() {
                    self.play = Play::Running;
                    self.paused = false;
                    self.go(Screen::InGame);
                }
                Vec::new()
            }
            UiAction::MatchFailed(why) => {
                if let Play::Starting { from } = self.play {
                    self.play = Play::None;
                    self.go(from);
                    self.message = Some(why);
                }
                Vec::new()
            }
            UiAction::MatchOver => {
                if self.screen != Screen::InGame || self.play != Play::Running {
                    return Vec::new();
                }
                self.go(Screen::Results);
                // The app's pause and this state never disagree.
                if std::mem::take(&mut self.paused) { vec![AppCommand::SetPaused(false)] } else { Vec::new() }
            }
        }
    }

    fn go(&mut self, s: Screen) {
        self.screen = s;
        self.message = None;
    }

    fn back(&mut self) -> Vec<AppCommand> {
        if self.is_starting() {
            return Vec::new();
        }
        match self.screen {
            Screen::Main => Vec::new(),
            Screen::InGame => self.set_paused(!self.paused),
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
        if !can || self.play != Play::Running {
            return Vec::new();
        }
        self.play = Play::None;
        self.paused = false;
        self.go(Screen::Main);
        vec![AppCommand::EndMatch]
    }

    fn start_match(&mut self) -> Vec<AppCommand> {
        let mut out = match (self.screen, self.play) {
            (Screen::LocalPlay, Play::None) => Vec::new(),
            // A rematch: the match over ends first.
            (Screen::Results, Play::Running) => vec![AppCommand::EndMatch],
            _ => return Vec::new(),
        };
        let seed = self.seed;
        self.seed = self.seed.wrapping_add(1);
        self.message = None;
        self.play = Play::Starting { from: Screen::LocalPlay };
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
