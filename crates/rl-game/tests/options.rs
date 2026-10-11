//! The options applied by the game (plan 7 task 4, Review Focus 4): video
//! changes through the paced window requests, volumes applied at once,
//! and the saved window opened at startup with the command line's flags
//! on top (never saved). Headless: the window is the requests it gets.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rl_audio::Audio;
use rl_game::shell::{Outcome, Shell, ShellConfig};
use rl_game::video::{Opening, Overrides, Video};
use rl_game::window::{Change, MIN_CHANGE_INTERVAL, Requests};
use rl_scene::{Interpolation, Viewport};
use rl_ui::{
    AudioSettings, FrameRate, Profiles, Screen, Settings, Store, UiAction, UiState, UpscaleFilter,
    VideoSettings, WindowPreset,
};

fn shell(content: &Path, settings: Settings, store: Store) -> Shell {
    let cfg = ShellConfig {
        content: content.to_path_buf(),
        interpolation: Interpolation::Linear,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    Shell::new(cfg, UiState::new(settings, Profiles::default(), 1), store, Audio::silent())
}

fn content_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("RL_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("RL_CONTENT_DIR not set; skipping the imported content");
    }
    dir
}

const DESKTOP: (u32, u32) = (2560, 1440);
const FRAME: Duration = Duration::from_millis(16);

/// The window as the requests leave it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Window {
    size: (u32, u32),
    fullscreen: bool,
}

impl Window {
    fn apply(&mut self, c: Change) {
        if let Some(on) = c.fullscreen {
            self.fullscreen = on;
        }
        if let Some(s) = c.size {
            self.size = s;
        }
    }
}

/// The app's part: the menus' video outcomes to the window requests.
struct Game {
    sh: Shell,
    video: Video,
    requests: Requests,
    window: Window,
    now: Instant,
    /// When each change was applied.
    applied: Vec<Instant>,
    live: Vec<rl_game::video::Live>,
}

impl Game {
    fn act(&mut self, a: UiAction) -> usize {
        let out = self.sh.act(a);
        let n = out.len();
        for o in out {
            if let Outcome::Video(v) = o {
                self.live.push(self.video.change(v, Some(DESKTOP), &mut self.requests));
            }
        }
        n
    }

    /// One frame: the change due, if any, applied before drawing.
    fn frame(&mut self) {
        self.now += FRAME;
        if let Some(c) = self.requests.due(self.now) {
            self.window.apply(c);
            self.applied.push(self.now);
        }
    }

    fn set(&mut self, f: impl FnOnce(&mut VideoSettings)) -> usize {
        let mut v = self.sh.ui.settings().video.clone();
        f(&mut v);
        self.act(UiAction::SetVideo(v))
    }
}

#[test]
fn video_changes_go_through_the_paced_path() {
    let tmp = tempfile::tempdir().unwrap();
    let sh = shell(tmp.path(), Settings::default(), Store::in_memory());
    let v = sh.ui.settings().video.clone();
    let now = Instant::now();
    let mut requests = Requests::default();
    requests.opened(now);
    let window = Window { size: (1280, 720), fullscreen: false };
    let mut g =
        Game { sh, video: Video::new(v), requests, window, now, applied: Vec::new(), live: Vec::new() };
    g.act(UiAction::Open(Screen::Options));

    // Interpolation, the filter, the frame rate and the UI scale apply at
    // once and spend no window change.
    g.set(|v| v.interpolation = false);
    assert_eq!(g.sh.interpolation(), Interpolation::Latest);
    g.set(|v| v.filter = UpscaleFilter::Nearest);
    g.set(|v| v.frame_rate = FrameRate::Uncapped);
    g.set(|v| v.ui_scale = 1.5);
    let last = g.live.last().unwrap();
    assert_eq!(last.ui_scale, 1.5);
    assert_eq!(g.live[1].filter, Some(rl_render::UpscaleFilter::IntegerNearest));
    assert_eq!(g.live[2].uncapped, Some(true));
    assert!(g.live[3].filter.is_none() && g.live[3].uncapped.is_none(), "only what changed");
    for _ in 0..40 {
        g.frame();
    }
    assert!(g.applied.is_empty(), "no window change was asked for");
    // The same settings again: nothing at all.
    assert_eq!(g.set(|_| {}), 0);

    // Rapid clicks: a new size or mode every frame for 3 s.
    let presets = WindowPreset::ALL;
    for i in 0..(3000 / 16) {
        g.set(|v| {
            v.window = presets[i % presets.len()];
            v.fullscreen = i % 7 == 3;
        });
        g.frame();
    }
    // Then the last choice: 1600 × 900, windowed.
    g.set(|v| {
        v.window = WindowPreset::W1600x900;
        v.fullscreen = false;
    });
    for _ in 0..60 {
        g.frame();
    }
    let gaps: Vec<Duration> = g.applied.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.iter().all(|d| *d >= MIN_CHANGE_INTERVAL), "{gaps:?}");
    assert!(g.applied.len() <= 3000 / 250 + 2, "{} changes", g.applied.len());
    assert_eq!(g.window, Window { size: (1600, 900), fullscreen: false });
    // Nothing more is pending.
    let n = g.applied.len();
    for _ in 0..60 {
        g.frame();
    }
    assert_eq!(g.applied.len(), n);

    // The desktop's size, then fullscreen and back: one change each.
    g.set(|v| v.window = WindowPreset::Desktop);
    for _ in 0..20 {
        g.frame();
    }
    assert_eq!(g.window, Window { size: DESKTOP, fullscreen: false });
    g.set(|v| v.fullscreen = true);
    for _ in 0..20 {
        g.frame();
    }
    assert_eq!(g.window, Window { size: DESKTOP, fullscreen: true });
    assert_eq!(g.applied.len(), n + 2);
}

#[test]
fn volume_changes_apply_live() {
    let tmp = tempfile::tempdir().unwrap();
    let content = content_dir();
    let dir = content.as_deref().unwrap_or(tmp.path());
    let mut sh = shell(dir, Settings::default(), Store::in_memory());
    // OLX's default volume (70 %).
    assert_eq!(sh.volume(), 0.7);
    let set = |sh: &mut Shell, master: f32, sfx: f32| {
        let a = AudioSettings { master, sfx, ..sh.ui.settings().audio.clone() };
        sh.act(UiAction::SetAudio(a));
    };
    set(&mut sh, 0.5, 0.5);
    assert_eq!(sh.volume(), 0.25, "master times effects, at once");
    let Some(_) = content else { return };
    // In a game, the game's sound output follows too, and keeps it after.
    sh.act(UiAction::StartSandbox);
    sh.poll();
    assert_eq!(sh.ui.screen(), Screen::InGame);
    assert_eq!(sh.session().unwrap().audio.volume(), 0.25);
    set(&mut sh, 1.0, 0.2);
    assert!((sh.session().unwrap().audio.volume() - 0.2).abs() < 1e-6);
    sh.act(UiAction::Back);
    sh.act(UiAction::LeaveMatch);
    assert!(sh.session().is_none());
    assert!((sh.volume() - 0.2).abs() < 1e-6);
}

#[test]
fn the_saved_window_opens_with_the_flags_on_top_unsaved() {
    // The window opens at the saved size and mode, at once (no resize).
    let file = VideoSettings { window: WindowPreset::W1600x900, ..VideoSettings::default() };
    assert_eq!(Video::opening(&file, Some(DESKTOP)), Opening { size: (1600, 900), fullscreen: false });
    let desk = VideoSettings { window: WindowPreset::Desktop, fullscreen: true, ..file.clone() };
    assert_eq!(Video::opening(&desk, Some(DESKTOP)), Opening { size: DESKTOP, fullscreen: true });
    assert_eq!(Video::opening(&desk, None).size, (1280, 720));

    // `--fullscreen` and `--filter nearest` on the command line: this run
    // shows them, the options too, but the file keeps its own values until
    // the player changes them in the options.
    let flags =
        Overrides { fullscreen: Some(true), filter: Some(UpscaleFilter::Nearest), ..Overrides::default() };
    let mut run = file.clone();
    flags.apply(&mut run);
    assert!(run.fullscreen && run.filter == UpscaleFilter::Nearest && run.window == WindowPreset::W1600x900);
    assert_eq!(Video::opening(&run, Some(DESKTOP)), Opening { size: (1600, 900), fullscreen: true });

    let tmp = tempfile::tempdir().unwrap();
    let settings = Settings { video: run.clone(), ..Settings::default() };
    let mut sh = shell(tmp.path(), settings, Store::open(tmp.path()));
    sh.set_overrides(file.clone(), flags);
    let saved = |sh: &mut Shell| {
        sh.flush();
        Store::open(tmp.path()).load_settings().value.video
    };
    sh.act(UiAction::SetVideo(VideoSettings { ui_scale: 1.5, ..run.clone() }));
    let v = saved(&mut sh);
    assert_eq!((v.fullscreen, v.filter, v.ui_scale), (false, UpscaleFilter::Sharp, 1.5));
    // The player turns fullscreen off and on again: theirs from now.
    sh.act(UiAction::SetVideo(VideoSettings { ui_scale: 1.5, fullscreen: false, ..run.clone() }));
    sh.act(UiAction::SetVideo(VideoSettings { ui_scale: 1.5, ..run.clone() }));
    let v = saved(&mut sh);
    assert_eq!((v.fullscreen, v.filter), (true, UpscaleFilter::Sharp));
}
