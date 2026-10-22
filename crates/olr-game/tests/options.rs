//! The options applied by the game (plan 7 task 4, Review Focus 4): video
//! changes through the paced window requests, volumes applied at once,
//! and the saved window opened at startup with the command line's flags
//! on top (never saved). Headless: the window is the requests it gets.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use olr_audio::Audio;
use olr_game::shell::{Outcome, Shell, ShellConfig};
use olr_game::video::{Opening, Overrides, Video};
use olr_game::window::{self, MIN_CHANGE_INTERVAL, Plan, Requests, Shape};
use olr_scene::{Interpolation, Viewport};
use olr_ui::weapon_select::WeaponSelect;
use olr_ui::{
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
    let dir = std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir());
    if dir.is_none() {
        eprintln!("OLR_CONTENT_DIR not set; skipping the imported content");
    }
    dir
}

/// Local play's Deathmatch started from the menus, past the weapon
/// selection (its first draw), in game.
fn start_match(sh: &mut Shell) {
    sh.act(UiAction::Open(Screen::LocalPlay));
    sh.act(UiAction::StartMatch);
    sh.poll();
    if sh.ui.screen() == Screen::WeaponSelect {
        let weapons = WeaponSelect::new(sh.data.weapons.as_ref().unwrap(), &[]).slots();
        sh.act(UiAction::ChooseWeapons { weapons, names: vec![] });
    }
    assert_eq!(sh.ui.screen(), Screen::InGame, "{:?}", sh.ui.message());
}

const FRAME: Duration = Duration::from_millis(16);

/// A windowed shape of `size` physical pixels, or fullscreen.
fn shape(size: (u32, u32), fullscreen: bool) -> Shape {
    Shape { size, physical: true, fullscreen }
}

/// The app's part: the menus' video outcomes to the window requests.
struct Game {
    sh: Shell,
    video: Video,
    requests: Requests,
    window: Shape,
    now: Instant,
    /// When each change was applied.
    applied: Vec<Instant>,
    /// Windows made again for them (Wayland's plan).
    recreated: usize,
    live: Vec<olr_game::video::Live>,
}

impl Game {
    fn act(&mut self, a: UiAction) -> usize {
        let out = self.sh.act(a);
        let n = out.len();
        for o in out {
            if let Outcome::Video(v) = o {
                self.live.push(self.video.change(v, &mut self.requests));
            }
        }
        n
    }

    /// One frame: the change due, if any, applied before drawing.
    fn frame(&mut self) {
        self.now += FRAME;
        if let Some(c) = self.requests.due(self.now) {
            if self.window.apply(c) && window::plan(true) == Plan::Recreate {
                self.recreated += 1;
            }
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
    let window = shape((1280, 720), false);
    let mut g = Game {
        sh,
        video: Video::new(v),
        requests,
        window,
        now,
        applied: Vec::new(),
        recreated: 0,
        live: Vec::new(),
    };
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
    assert_eq!(g.live[1].filter, Some(olr_render::UpscaleFilter::IntegerNearest));
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
    assert_eq!(g.window, shape((1600, 900), false));
    assert!(g.recreated <= g.applied.len(), "at most one window made per change");
    // Nothing more is pending.
    let n = g.applied.len();
    for _ in 0..60 {
        g.frame();
    }
    assert_eq!(g.applied.len(), n);

    // Fullscreen and back: one change each, the window's size kept for
    // after; on Wayland each made the window again, once.
    let made = g.recreated;
    g.set(|v| v.fullscreen = true);
    for _ in 0..20 {
        g.frame();
    }
    assert_eq!(g.window, shape((1600, 900), true));
    assert_eq!(g.applied.len(), n + 1);
    assert_eq!(g.recreated, made + 1);
    g.set(|v| v.fullscreen = false);
    for _ in 0..20 {
        g.frame();
    }
    assert_eq!(g.window, shape((1600, 900), false));
    assert_eq!(g.applied.len(), n + 2);
    assert_eq!(g.recreated, made + 2);
}

/// A window made again for each size or mode change (Wayland's plan) never
/// touches the game: the same ticks run and the world ends as one that
/// never changed its window (the app's part: the viewport follows the new
/// window).
#[test]
fn a_window_made_again_keeps_the_game_ticking() {
    let Some(content) = content_dir() else { return };
    let start = || {
        let mut sh = shell(&content, Settings::default(), Store::in_memory());
        start_match(&mut sh);
        sh
    };
    let (mut changed, mut still) = (start(), start());
    let mut requests = Requests::default();
    let mut window = shape((1280, 720), false);
    let mut now = Instant::now();
    requests.opened(now);
    // At frame N: a size, or fullscreen on or off.
    let script = [
        (10, Some((1920, 1080)), None),
        (40, Some((3440, 1440)), None),
        (70, None, Some(true)),
        (100, None, Some(false)),
    ];
    let (mut recreated, mut ticked) = (0, 0u64);
    let keys = olr_sim::Keys(0b0101);
    for frame in 0..160u32 {
        now += FRAME;
        for &(at, size, fullscreen) in &script {
            if at == frame {
                if let Some((w, h)) = size {
                    requests.size(w, h);
                }
                if let Some(on) = fullscreen {
                    requests.fullscreen(on);
                }
            }
        }
        if let Some(c) = requests.due(now)
            && window.apply(c)
            && window::plan(true) == Plan::Recreate
        {
            recreated += 1;
            changed.set_viewport(olr_scene::Viewport::GAME);
        }
        for _ in 0..2 {
            assert!(changed.tick(keys) && still.tick(keys));
            ticked += 1;
        }
    }
    assert_eq!(recreated, 4);
    assert_eq!(window, shape((3440, 1440), false));
    let (a, b) = (changed.session().unwrap(), still.session().unwrap());
    assert_eq!(a.ticks(), ticked, "no tick lost or doubled");
    assert_eq!(a.ticks(), b.ticks());
    assert_eq!(a.world.checksum(), b.world.checksum(), "the same world");
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
    start_match(&mut sh);
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
    assert_eq!(Video::opening(&file), Opening { size: (1600, 900), fullscreen: false });
    let full = VideoSettings { fullscreen: true, ..file.clone() };
    assert_eq!(Video::opening(&full), Opening { size: (1600, 900), fullscreen: true });

    // `--fullscreen` and `--filter nearest` on the command line: this run
    // shows them, the options too, but the file keeps its own values until
    // the player changes them in the options.
    let flags =
        Overrides { fullscreen: Some(true), filter: Some(UpscaleFilter::Nearest), ..Overrides::default() };
    let mut run = file.clone();
    flags.apply(&mut run);
    assert!(run.fullscreen && run.filter == UpscaleFilter::Nearest && run.window == WindowPreset::W1600x900);
    assert_eq!(Video::opening(&run), Opening { size: (1600, 900), fullscreen: true });

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

/// The blood amount (issue #6) reaches a game started from the menus and
/// the one playing at once, and changes nothing of the world: a match
/// at 0 % and one at 500 % tick to the same checksums.
#[test]
fn the_blood_amount_applies_at_once_and_never_to_the_world() {
    let Some(content) = content_dir() else { return };
    let play = |blood: u16| {
        let settings =
            Settings { video: VideoSettings { blood, ..VideoSettings::default() }, ..Settings::default() };
        let mut sh = shell(&content, settings, Store::in_memory());
        start_match(&mut sh);
        assert_eq!(sh.session().unwrap().present.blood(), blood, "the saved amount");
        let (mut sums, mut bloody) = (Vec::new(), 0);
        for i in 0..600u32 {
            // Fire now and then: shots, hits and blood.
            sh.tick(olr_sim::Keys(if i % 50 < 10 { olr_sim::Keys::FIRE } else { 0 }));
            let s = sh.session().unwrap();
            sums.push(s.world.checksum());
            bloody += s.present.fx.entities().filter(|(_, e)| e.kind == olr_fx::Kind::Blood).count();
        }
        (sh, sums, bloody)
    };
    let (mut sh, none, shown) = play(0);
    assert_eq!(shown, 0, "no blood at 0 %");
    let (_, lots, shown) = play(500);
    assert!(shown > 0, "the shots drew blood");
    assert_eq!(none, lots, "the world never sees the blood amount");
    let v = VideoSettings { blood: 250, ..sh.ui.settings().video.clone() };
    sh.act(UiAction::SetVideo(v));
    assert_eq!(sh.session().unwrap().present.blood(), 250, "the game playing follows at once");
}
