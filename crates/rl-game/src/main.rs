//! `rl-game`: the menus (M7), or straight into the sandbox (spec §19 E11),
//! a Deathmatch against bots (M6) or a replay when the command line names
//! one. Run `rl-game --help`.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches};
use rl_game::app::{App, Config};
use rl_game::autoplay::Autoplay;
use rl_game::script::{self, Command, Step};
use rl_game::session::Session;
use rl_game::setup::{self, DeathmatchOptions, Game, SandboxOptions};
use rl_game::shell::{Shell, ShellConfig};
use rl_game::speed::Speed;
use rl_game::video::{self, Overrides};
use rl_game::window::{self, WindowSystem};
use rl_game::wsl;
use rl_render::{RenderSettings, UpscaleFilter};
use rl_scene::Interpolation;

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Mode {
    /// You and idle targets.
    Sandbox,
    /// You against bots (OLX's Deathmatch).
    Deathmatch,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Level {
    Easy,
    Medium,
    Hard,
    Xtreme,
}

impl Level {
    fn difficulty(self) -> rl_bots::Difficulty {
        match self {
            Level::Easy => rl_bots::Difficulty::Easy,
            Level::Medium => rl_bots::Difficulty::Medium,
            Level::Hard => rl_bots::Difficulty::Hard,
            Level::Xtreme => rl_bots::Difficulty::Xtreme,
        }
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Filter {
    /// Whole multiples of the art, centred, black bars around.
    Nearest,
    /// Fills the window keeping the aspect; texel edges blend over one pixel.
    Sharp,
}

/// rust-liero: the menus; or, when a flag below chooses a game, straight
/// into the sandbox (you and idle targets on a level, with a mod's
/// weapons), a Deathmatch against bots (`--mode deathmatch`) or a replay
/// (`--replay`). Arrows move and aim, Ctrl fires, Alt jumps, X throws the
/// rope, Shift with the arrows changes weapon; Tab scores, F2 debug, F5
/// restart (a Deathmatch's rematch), H health bars, Esc pauses (the menus)
/// or quits. With the menus, --fullscreen, --uncapped, --filter and
/// --no-interp win over the options for the run (they are not saved).
#[derive(clap::Parser)]
#[command(name = "rl-game")]
struct Args {
    /// What to play (the sandbox when another flag chooses a game).
    #[arg(long, value_enum)]
    mode: Option<Mode>,
    /// Deathmatch: bots against you (1 to 31; with --spectate, 2 to 32).
    #[arg(long, default_value_t = 3)]
    bots: usize,
    /// Deathmatch: the bots play alone and you watch (fast-forward with
    /// --speed, PageUp and PageDown).
    #[arg(long, conflicts_with = "replay")]
    spectate: bool,
    /// How fast a watched game runs: 1, 2, 4, 8, 16 or max (a replay, or
    /// --spectate; PageUp and PageDown change it). A game you play runs at 1×.
    #[arg(long, value_parser = Speed::parse)]
    speed: Option<Speed>,
    /// Deathmatch: the bots' difficulty.
    #[arg(long, value_enum, default_value = "medium")]
    difficulty: Level,
    /// Deathmatch: lives (below 0 unlimited; OLX's default -2).
    #[arg(long, default_value_t = -2, allow_negative_numbers = true)]
    lives: i32,
    /// Deathmatch: the match ends when a score reaches this (0 or less: off).
    #[arg(long, default_value_t = 15, allow_negative_numbers = true)]
    kill_limit: i32,
    /// Deathmatch: the time limit in minutes (0: off).
    #[arg(long, default_value_t = 6.0)]
    time_limit: f32,
    /// Deathmatch: save the match's replay here (rematches next to it,
    /// `<stem>-2.<ext>`, ...).
    #[arg(long)]
    record: Option<PathBuf>,
    /// Play this replay (its mod and level come from it).
    #[arg(long, conflicts_with_all = ["mode", "record"])]
    replay: Option<PathBuf>,
    /// The level: a pack id or an OLX level name ("Liero Factory", "CastleStrike", ...).
    #[arg(long, default_value = "Liero Factory")]
    level: String,
    /// The mod: a pack id or an OLX mod name ("Classic", "MW 1.0").
    #[arg(long = "mod", default_value = "Classic")]
    mod_name: String,
    /// The player's skin: a pack id or an OLX skin name.
    #[arg(long, default_value = "default")]
    skin: String,
    /// Sandbox: idle target worms.
    #[arg(long, default_value_t = 3)]
    targets: usize,
    /// The match seed.
    #[arg(long, default_value_t = 1)]
    seed: u32,
    /// Bonuses spawn and fall (OLX's Bonuses option).
    #[arg(long)]
    bonuses: bool,
    /// Imported content (packs written by `rl-cli import`).
    #[arg(long, default_value = "target/content")]
    content: PathBuf,
    /// Borderless fullscreen.
    #[arg(long)]
    fullscreen: bool,
    /// The window system (Linux); else `RL_WINDOW_SYSTEM`, else auto
    /// (Wayland when available). Try x11 if the Wayland compositor drops
    /// the game.
    #[arg(long, value_enum)]
    window_system: Option<WindowSystem>,
    /// No VSync: present as fast as the GPU draws (Immediate or Mailbox).
    #[arg(long)]
    uncapped: bool,
    /// Draw the latest tick at whole pixels like OLX, not interpolated.
    #[arg(long)]
    no_interp: bool,
    /// The upscale filter.
    #[arg(long, value_enum, default_value = "sharp")]
    filter: Filter,
    /// Quit after this many seconds.
    #[arg(long)]
    autoquit_after: Option<f64>,
    /// Play a script of timed keys and commands (see `rl-game/src/script.rs`).
    #[arg(long, conflicts_with = "demo")]
    script: Option<PathBuf>,
    /// Play the built-in demo script: walk, rope, shoot, die, respawn.
    #[arg(long)]
    demo: bool,
    /// Save a screenshot after this tick (repeatable).
    #[arg(long = "shot")]
    shots: Vec<u64>,
    /// Where screenshots go.
    #[arg(long, default_value = "target/rl-game-shots")]
    shot_dir: PathBuf,
    /// Print the debug counters (FPS, frame and tick times) every second.
    #[arg(long)]
    stats: bool,
    /// Measure input latency with a probe press every this many ms (it
    /// presses no key); the exit summary prints the numbers.
    #[arg(long)]
    latency_probe: Option<u64>,
    /// Drive the game from a script of clicks by label, keys, waits,
    /// screenshots and checks (see `rl-game/src/autoplay.rs`; logged to
    /// `<shot dir>/autoplay.jsonl`). A failing step ends the game with an error.
    #[arg(long)]
    autoplay: Option<PathBuf>,
    /// With --autoplay: every frame is 20 ms of time and runs two ticks,
    /// whatever the clock says (deterministic scripted runs; the first menu
    /// match's seed is 1). Never for a person playing: it changes the
    /// game's speed.
    #[arg(long, requires = "autoplay")]
    fake_clock: bool,
    /// The data directory (settings, profiles, replays, imported packs)
    /// instead of the platform's.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// No sound output (no audio device is opened).
    #[arg(long)]
    no_sound: bool,
}

/// The flags that choose a game: with any of them on the command line the
/// game plays it at once (scripted runs and tests); without, the menus.
const GAME_FLAGS: [&str; 17] = [
    "mode",
    "bots",
    "spectate",
    "difficulty",
    "lives",
    "kill_limit",
    "time_limit",
    "record",
    "replay",
    "level",
    "mod_name",
    "skin",
    "targets",
    "seed",
    "bonuses",
    "script",
    "demo",
];

/// Whether the command line chose a game.
fn plays_directly(m: &ArgMatches) -> bool {
    GAME_FLAGS.iter().any(|id| m.value_source(id) == Some(ValueSource::CommandLine))
}

/// The game the command line chose, loaded, and its session.
fn direct(args: &Args, audio: rl_audio::Audio, cfg: ShellConfig) -> anyhow::Result<Shell> {
    let mode = args.mode.unwrap_or(if args.spectate { Mode::Deathmatch } else { Mode::Sandbox });
    anyhow::ensure!(
        args.record.is_none() || mode == Mode::Deathmatch,
        "--record records a Deathmatch (--mode deathmatch)"
    );
    anyhow::ensure!(!args.spectate || mode == Mode::Deathmatch, "--spectate watches a Deathmatch of bots");
    let assets = if let Some(file) = &args.replay {
        setup::load_replay(&[args.content.as_path()], file, &args.skin)?
    } else if mode == Mode::Deathmatch {
        let o = DeathmatchOptions {
            seed: args.seed,
            bots: args.bots,
            difficulty: args.difficulty.difficulty(),
            lives: args.lives,
            kill_limit: args.kill_limit,
            time_limit: args.time_limit,
            bonuses: args.bonuses,
            spectate: args.spectate,
        };
        setup::load_deathmatch(&[args.content.as_path()], &args.mod_name, &args.level, &args.skin, &o)?
    } else {
        let options = SandboxOptions {
            mod_id: String::new(),
            level_id: String::new(),
            seed: args.seed,
            targets: args.targets,
            bonuses: args.bonuses,
        };
        setup::load(&[args.content.as_path()], &args.mod_name, &args.level, &args.skin, options)?
    };
    for w in &assets.warnings {
        eprintln!("warning: {w}");
    }
    anyhow::ensure!(
        args.speed.is_none_or(|s| s == Speed::X1) || !assets.human_plays(),
        "--speed is for watching (--replay, --spectate): a game you play runs at 1×"
    );
    let mut audio = audio;
    audio.load(&assets.m, assets.content.core());
    for w in audio.take_warnings() {
        eprintln!("warning: {w}");
    }
    let session = Session::new(&assets, cfg.viewport, cfg.interpolation, audio)?;
    let what = match &assets.game {
        Game::Sandbox(_) => "sandbox".to_string(),
        Game::Deathmatch { options, .. } if assets.human_plays() => {
            format!("Deathmatch seed {}", options.seed)
        }
        Game::Deathmatch { options, .. } => format!("Deathmatch of bots (watched) seed {}", options.seed),
        Game::Replay(r) => format!("replay of {:.1} s", f64::from(r.ticks()) / 100.0),
    };
    println!("rl-game: {what}: {} on {}, {} worms", assets.mod_id, assets.level_id, assets.worms());
    Ok(Shell::playing(cfg, assets, session))
}

/// The video flags given on the command line (they win over the saved
/// options for the run).
fn video_flags(m: &ArgMatches, args: &Args) -> Overrides {
    let given = |id: &str| m.value_source(id) == Some(ValueSource::CommandLine);
    Overrides {
        fullscreen: given("fullscreen").then_some(true),
        frame_rate: given("uncapped").then_some(rl_ui::FrameRate::Uncapped),
        filter: given("filter").then_some(match args.filter {
            Filter::Nearest => rl_ui::UpscaleFilter::Nearest,
            Filter::Sharp => rl_ui::UpscaleFilter::Sharp,
        }),
        interpolation: given("no_interp").then_some(false),
    }
}

/// The menus, with the settings and profiles of the data directory; the
/// video `flags` on top of the saved options (not saved).
fn menus(args: &Args, audio: rl_audio::Audio, mut cfg: ShellConfig, flags: Overrides) -> Shell {
    let (mut store, warning) = match &args.data_dir {
        Some(dir) => (rl_ui::Store::open(dir), None),
        None => rl_ui::Store::platform(),
    };
    let mut settings = store.load_settings();
    let profiles = store.load_profiles();
    for w in warning.into_iter().chain(settings.warnings).chain(profiles.warnings) {
        eprintln!("warning: {w}");
    }
    // Matches get seeds from here on (each one is in its replay); `--seed`
    // would choose a game, so the clock picks the first.
    let seed = if args.fake_clock {
        1
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32)
    };
    let data = store.dir().map_or("none (nothing is saved)".to_string(), |d| d.display().to_string());
    println!("rl-game: menus; content {}, data {data}", args.content.display());
    let file = settings.value.video.clone();
    flags.apply(&mut settings.value.video);
    cfg.interpolation = video::interpolation(settings.value.video.interpolation);
    let ui = rl_ui::UiState::new(settings.value, profiles.value, seed);
    let mut shell = Shell::new(cfg, ui, store, audio);
    shell.set_overrides(file, flags);
    shell
}

fn run(args: Args, play_now: bool, flags: Overrides) -> anyhow::Result<()> {
    let var = std::env::var(window::WINDOW_SYSTEM_VAR).ok();
    let ws = window::window_system(args.window_system, var.as_deref()).map_err(anyhow::Error::msg)?;
    let mut steps: Vec<Step> = match (&args.script, args.demo) {
        (Some(path), _) => {
            let text = std::fs::read_to_string(path).with_context(|| format!("{}", path.display()))?;
            script::parse(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?
        }
        (None, true) => script::parse(script::DEMO).map_err(anyhow::Error::msg)?,
        (None, false) => Vec::new(),
    };
    steps.extend(args.shots.iter().map(|&tick| Step { tick, command: Command::Shot }));
    steps.sort_by_key(|s| s.tick);

    let mut audio = if args.no_sound { rl_audio::Audio::silent() } else { rl_audio::Audio::new() };
    let silent = audio.is_silent();
    for w in audio.take_warnings() {
        eprintln!("warning: {w}");
    }
    let gilrs = match gilrs::Gilrs::new() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("warning: no gamepads: {e}");
            None
        }
    };
    let interpolation = if args.no_interp { Interpolation::Latest } else { Interpolation::Linear };
    let viewport = rl_scene::Viewport::GAME;
    let shell_cfg = ShellConfig { content: args.content.clone(), interpolation, viewport };
    let shell =
        if play_now { direct(&args, audio, shell_cfg)? } else { menus(&args, audio, shell_cfg, flags) };
    println!("rl-game: audio {}", if silent { "silent" } else { "opened" });
    let osrelease = wsl::osrelease();
    let backend = std::env::var("WGPU_BACKEND").ok();
    let mut filter = match args.filter {
        Filter::Nearest => UpscaleFilter::IntegerNearest,
        Filter::Sharp => UpscaleFilter::SharpBilinear,
    };
    let (mut fullscreen, mut uncapped, mut interpolation) = (args.fullscreen, args.uncapped, interpolation);
    if !play_now {
        // The options as this run shows them (the flags on top).
        let v = &shell.ui.settings().video;
        filter = video::filter(v.filter);
        fullscreen = v.fullscreen;
        uncapped = v.frame_rate == rl_ui::FrameRate::Uncapped;
        interpolation = video::interpolation(v.interpolation);
    }
    let cfg = Config {
        backends: wsl::backends(wsl::is_wsl(&osrelease), backend.as_deref()),
        render: RenderSettings { filter },
        fullscreen,
        uncapped,
        interpolation,
        autoquit_after: args.autoquit_after,
        script: steps,
        shot_dir: args.shot_dir.clone(),
        print_stats: args.stats,
        latency_probe: args.latency_probe.filter(|&ms| ms > 0).map(std::time::Duration::from_millis),
        record: args.record.clone(),
        menus: !play_now,
        speed: args.speed.unwrap_or_default(),
        fake_clock: args.fake_clock,
    };
    let autoplay = match &args.autoplay {
        Some(script) => Some(Autoplay::load(script, &args.shot_dir.join("autoplay.jsonl"))?),
        None => None,
    };
    let event_loop = event_loop(ws)?;
    let wayland = is_wayland(&event_loop);
    let mut app = App::new(cfg, shell, silent, gilrs, autoplay);
    let ran = event_loop.run_app(&mut app);
    println!("rl-game: {}", app.summary());
    if let Some(m) = app.match_summary() {
        println!("rl-game: {m}");
    }
    println!("rl-game: {}", app.latency_summary());
    app.save_recording();
    let autoplay = app.autoplay_result();
    match ran {
        // The game never sets a failing exit code; winit does, most often
        // when the display server closed the connection (a compositor crash).
        Err(winit::error::EventLoopError::ExitFailure(_)) if app.error.is_none() => {
            Err(anyhow::anyhow!(window::lost_connection(wayland)))
        }
        ran => {
            ran.context("the event loop")?;
            if let Some(e) = app.error.take() {
                return Err(e);
            }
            match autoplay {
                Some(Ok(())) => println!("rl-game: autoplay passed"),
                Some(Err(why)) => anyhow::bail!(why),
                None => {}
            }
            Ok(())
        }
    }
}

/// The event loop on window system `ws`.
fn event_loop(ws: WindowSystem) -> anyhow::Result<winit::event_loop::EventLoop<()>> {
    let mut builder = winit::event_loop::EventLoop::builder();
    #[cfg(target_os = "linux")]
    match ws {
        WindowSystem::Auto => {}
        WindowSystem::Wayland => {
            winit::platform::wayland::EventLoopBuilderExtWayland::with_wayland(&mut builder);
        }
        WindowSystem::X11 => {
            winit::platform::x11::EventLoopBuilderExtX11::with_x11(&mut builder);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = ws;
    builder.build().with_context(|| format!("opening the window system ({ws:?})"))
}

/// Whether the event loop runs on Wayland.
fn is_wayland(event_loop: &winit::event_loop::EventLoop<()>) -> bool {
    #[cfg(target_os = "linux")]
    return winit::platform::wayland::EventLoopExtWayland::is_wayland(event_loop);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = event_loop;
        false
    }
}

/// On WSL without the user's choice, runs this program again with Mesa's
/// d3d12 driver and wgpu's GL backend; returns its exit code.
fn relaunch() -> Option<ExitCode> {
    let wsl = wsl::is_wsl(&wsl::osrelease());
    let backend = std::env::var("WGPU_BACKEND").ok();
    let gallium = std::env::var("GALLIUM_DRIVER").ok();
    let adapter = std::env::var(wsl::ADAPTER_VAR).ok();
    let discrete = wsl::discrete_adapter(std::path::Path::new(wsl::WSL_LIB));
    let env = wsl::relaunch_env(wsl, backend.as_deref(), gallium.as_deref(), adapter.as_deref(), discrete)?;
    let exe = std::env::current_exe().ok()?;
    let status = std::process::Command::new(exe).args(std::env::args_os().skip(1)).envs(env).status();
    match status {
        Ok(s) => Some(ExitCode::from(s.code().unwrap_or(1).clamp(0, 255) as u8)),
        Err(e) => {
            eprintln!("warning: could not restart with the WSL GPU defaults ({e}); continuing");
            None
        }
    }
}

fn main() -> ExitCode {
    let matches = Args::command().get_matches();
    let args = Args::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    if let Some(code) = relaunch() {
        return code;
    }
    let flags = video_flags(&matches, &args);
    match run(args, plays_directly(&matches), flags) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
