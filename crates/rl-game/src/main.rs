//! `rl-game`: the sandbox (spec §19 E11), a Deathmatch against bots (M6)
//! and replays. Run `rl-game --help`.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::Parser;
use rl_game::app::{App, Config};
use rl_game::script::{self, Command, Step};
use rl_game::session::Session;
use rl_game::setup::{self, DeathmatchOptions, Game, SandboxOptions};
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

/// rust-liero: the sandbox (you and idle targets on a level, with a mod's
/// weapons), a Deathmatch against bots (`--mode deathmatch`) or a replay
/// (`--replay`). Arrows move and aim, Ctrl fires, Alt jumps, X throws the
/// rope, Shift with the arrows changes weapon; Tab scores, F2 debug, F5
/// restart (a Deathmatch's rematch), H health bars, Esc quits.
#[derive(Parser)]
#[command(name = "rl-game")]
struct Args {
    /// What to play.
    #[arg(long, value_enum, default_value = "sandbox")]
    mode: Mode,
    /// Deathmatch: bots against you (1 to 31).
    #[arg(long, default_value_t = 3)]
    bots: usize,
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
}

fn run(args: Args) -> anyhow::Result<()> {
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

    anyhow::ensure!(
        args.record.is_none() || args.mode == Mode::Deathmatch,
        "--record records a Deathmatch (--mode deathmatch)"
    );
    let assets = if let Some(file) = &args.replay {
        setup::load_replay(&args.content, file, &args.skin)?
    } else if args.mode == Mode::Deathmatch {
        let o = DeathmatchOptions {
            seed: args.seed,
            bots: args.bots,
            difficulty: args.difficulty.difficulty(),
            lives: args.lives,
            kill_limit: args.kill_limit,
            time_limit: args.time_limit,
            bonuses: args.bonuses,
        };
        setup::load_deathmatch(&args.content, &args.mod_name, &args.level, &args.skin, &o)?
    } else {
        let options = SandboxOptions {
            mod_id: String::new(),
            level_id: String::new(),
            seed: args.seed,
            targets: args.targets,
            bonuses: args.bonuses,
        };
        setup::load(&args.content, &args.mod_name, &args.level, &args.skin, options)?
    };
    for w in &assets.warnings {
        eprintln!("warning: {w}");
    }
    let mut audio = rl_audio::Audio::new();
    let opened = !audio.is_silent();
    for w in audio.take_warnings() {
        eprintln!("warning: {w}");
    }
    audio.load(&assets.m, assets.content.core());
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
    let viewport = rl_scene::Viewport::from_aspect(16.0 / 9.0, rl_game::app::MAX_ASPECT);
    let session = Session::new(&assets, viewport, interpolation, audio)?;
    let osrelease = wsl::osrelease();
    let backend = std::env::var("WGPU_BACKEND").ok();
    let filter = match args.filter {
        Filter::Nearest => UpscaleFilter::IntegerNearest,
        Filter::Sharp => UpscaleFilter::SharpBilinear,
    };
    let cfg = Config {
        backends: wsl::backends(wsl::is_wsl(&osrelease), backend.as_deref()),
        render: RenderSettings { filter },
        fullscreen: args.fullscreen,
        uncapped: args.uncapped,
        interpolation,
        autoquit_after: args.autoquit_after,
        script: steps,
        shot_dir: args.shot_dir.clone(),
        print_stats: args.stats,
        latency_probe: args.latency_probe.filter(|&ms| ms > 0).map(std::time::Duration::from_millis),
        record: args.record.clone(),
    };
    let what = match &assets.game {
        Game::Sandbox(_) => "sandbox".to_string(),
        Game::Deathmatch { options, .. } => format!("Deathmatch seed {}", options.seed),
        Game::Replay(r) => format!("replay of {:.1} s", f64::from(r.ticks()) / 100.0),
    };
    println!(
        "rl-game: {what}: {} on {}, {} worms, audio {}",
        assets.mod_id,
        assets.level_id,
        assets.worms(),
        if opened { "opened" } else { "silent" }
    );
    let event_loop = event_loop(ws)?;
    let wayland = is_wayland(&event_loop);
    let mut app = App::new(cfg, assets, session, gilrs);
    let ran = event_loop.run_app(&mut app);
    app.save_recording();
    println!("rl-game: {}", app.summary());
    if let Some(m) = app.match_summary() {
        println!("rl-game: {m}");
    }
    println!("rl-game: {}", app.latency_summary());
    match ran {
        // The game never sets a failing exit code; winit does, most often
        // when the display server closed the connection (a compositor crash).
        Err(winit::error::EventLoopError::ExitFailure(_)) if app.error.is_none() => {
            Err(anyhow::anyhow!(window::lost_connection(wayland)))
        }
        ran => {
            ran.context("the event loop")?;
            app.error.take().map_or(Ok(()), Err)
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
    let env = wsl::relaunch_env(wsl, backend.as_deref(), gallium.as_deref())?;
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
    let args = Args::parse();
    if let Some(code) = relaunch() {
        return code;
    }
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
