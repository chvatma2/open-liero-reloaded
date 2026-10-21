//! `olr-game`: the menus (M7), or straight into the sandbox (spec §19 E11),
//! a Deathmatch against bots (M6) or a replay when the command line names
//! one. Run `olr --help`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Context as _;
use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches};
use olr_game::app::{App, Config};
use olr_game::autoplay::Autoplay;
use olr_game::script::{self, Command, Step};
use olr_game::session::Session;
use olr_game::setup::{self, DeathmatchOptions, SandboxOptions};
use olr_game::shell::{Shell, ShellConfig};
use olr_game::speed::Speed;
use olr_game::video::{self, Overrides};
use olr_game::window::{self, WindowSystem};
use olr_game::{logging, wsl};
use olr_netcode::netsim::Profile;
use olr_render::{RenderSettings, UpscaleFilter};
use olr_scene::Interpolation;

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
    fn difficulty(self) -> olr_bots::Difficulty {
        match self {
            Level::Easy => olr_bots::Difficulty::Easy,
            Level::Medium => olr_bots::Difficulty::Medium,
            Level::Hard => olr_bots::Difficulty::Hard,
            Level::Xtreme => olr_bots::Difficulty::Xtreme,
        }
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum HudLayoutFlag {
    Auto,
    Bar,
    Sides,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Filter {
    /// Whole multiples of the art, centred, black bars around.
    Nearest,
    /// Fills the window keeping the aspect; texel edges blend over one pixel.
    Sharp,
}

/// OLR: the menus; or, when a flag below chooses a game, straight
/// into the sandbox (you and idle targets on a level, with a mod's
/// weapons), a Deathmatch against bots (`--mode deathmatch`) or a replay
/// (`--replay`). Arrows move and aim, Ctrl fires, Alt jumps, X throws the
/// rope, Shift with the arrows changes weapon; Tab scores, F2 debug, F5
/// restart (a Deathmatch's rematch), H health bars, Esc pauses (the menus)
/// or quits. With the menus, --fullscreen, --uncapped, --filter and
/// --no-interp win over the options for the run (they are not saved).
#[derive(clap::Parser)]
#[command(name = "olr", version = olr_game::VERSION)]
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
    /// Imported content (packs written by `olr-cli import`): `content/packs`
    /// beside this program when there is one, else in the working directory.
    #[arg(long, default_value_os_t = default_content())]
    content: PathBuf,
    /// Borderless fullscreen.
    #[arg(long)]
    fullscreen: bool,
    /// The window system (Linux); else `OLR_WINDOW_SYSTEM`, else auto
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
    /// Where the HUD goes on a window wider than 16:9: the bar, side
    /// panels beside the 16:9 frame when there is room, or auto (side
    /// panels on 21:9 and wider).
    #[arg(long, value_enum, default_value = "auto")]
    hud_layout: HudLayoutFlag,
    /// Quit after this many seconds.
    #[arg(long)]
    autoquit_after: Option<f64>,
    /// Write the run's frame times (after --frame-log-skip seconds) here as
    /// RON, for `cargo xtask envelope`.
    #[arg(long)]
    frame_log: Option<PathBuf>,
    /// Seconds at the start the frame log leaves out (the window settling,
    /// a size change).
    #[arg(long, default_value_t = 3.0)]
    frame_log_skip: f64,
    /// Play a script of timed keys and commands (see `olr-game/src/script.rs`).
    #[arg(long, conflicts_with = "demo")]
    script: Option<PathBuf>,
    /// Play the built-in demo script: walk, rope, shoot, die, respawn.
    #[arg(long)]
    demo: bool,
    /// Save a screenshot after this tick (repeatable).
    #[arg(long = "shot")]
    shots: Vec<u64>,
    /// Where screenshots go.
    #[arg(long, default_value = "target/olr-game-shots")]
    shot_dir: PathBuf,
    /// Print the debug counters (FPS, frame and tick times) every second.
    #[arg(long)]
    stats: bool,
    /// Measure input latency with a probe press every this many ms (it
    /// presses no key); the exit summary prints the numbers.
    #[arg(long)]
    latency_probe: Option<u64>,
    /// Drive the game from a script of clicks by label, keys, waits,
    /// screenshots and checks (see `olr-game/src/autoplay.rs`; logged to
    /// `<shot dir>/autoplay.jsonl`). A failing step ends the game with an error.
    #[arg(long)]
    autoplay: Option<PathBuf>,
    /// With --autoplay: every frame is 20 ms of time and runs two ticks,
    /// whatever the clock says (deterministic scripted runs; the first menu
    /// match's seed is 1). Never for a person playing: it changes the
    /// game's speed.
    #[arg(long, requires = "autoplay")]
    fake_clock: bool,
    /// The data directory (settings, profiles, replays, user packs)
    /// instead of the platform's.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// No sound output (no audio device is opened).
    #[arg(long)]
    no_sound: bool,
    /// Host an online game at once (the Lobby opens), named NAME (else
    /// the Host screen's name), with the profile's look and the saved match.
    #[arg(long, value_name = "NAME", num_args = 0..=1, default_missing_value = "",
          conflicts_with_all = ["join", "net_sim", "mode", "replay", "spectate", "script", "demo"])]
    host: Option<String>,
    /// Join the online game at ADDR (host[:port]) at once.
    #[arg(long, value_name = "ADDR", conflicts_with_all = ["net_sim", "mode", "replay", "spectate", "script", "demo"])]
    join: Option<String>,
    /// Feel a network: a host with three bots in this process, joined over
    /// a loopback link impaired as PROFILE (lan, good, typical, bad, awful).
    #[arg(long, value_name = "PROFILE", value_parser = net_profile,
          conflicts_with_all = ["mode", "replay", "spectate", "script", "demo"])]
    net_sim: Option<Profile>,
    /// The port to host on (TCP and UDP) and to join when the address has none.
    #[arg(long)]
    port: Option<u16>,
    /// Open the level editor on the level pack in DIR (not bundled content:
    /// copy it from the Editor menu); the menus run behind it.
    #[arg(long, value_name = "DIR", conflicts_with_all = ["host", "join", "net_sim"],
          conflicts_with_all = GAME_FLAGS)]
    edit: Option<PathBuf>,
    /// The run log's level (error, warn, info, debug), in the terminal and
    /// in `<data>/logs/`; else `OLR_LOG`, else info.
    #[arg(long, value_name = "LEVEL", value_parser = log_level)]
    log: Option<olr_log::LevelFilter>,
}

/// A `--log` level by name.
fn log_level(name: &str) -> Result<olr_log::LevelFilter, String> {
    olr_log::parse_level(name).ok_or_else(|| format!("no level {name:?}: error, warn, info or debug"))
}

/// `--edit`'s directory (plan 14c R6): the pack at `edit`, unless it lies
/// inside the `content` root (bundled content is never edited in place).
fn edit_target(edit: &Path, content: &Path) -> Result<PathBuf, String> {
    let dir = edit.canonicalize().map_err(|e| format!("{}: {e}", edit.display()))?;
    if !dir.is_dir() {
        return Err(format!("{}: not a directory", edit.display()));
    }
    if content.canonicalize().is_ok_and(|c| dir.starts_with(c)) {
        return Err(olr_game::editor::BUNDLED.to_string());
    }
    Ok(dir)
}

/// `content/packs` beside the executable (an unpacked release), else in
/// the working directory (the repository).
fn default_content() -> PathBuf {
    olr_content::default_content_dir(std::env::current_exe().ok().as_deref())
}

/// A `--net-sim` profile by name.
fn net_profile(name: &str) -> Result<Profile, String> {
    Profile::by_name(name).ok_or_else(|| {
        let names: Vec<&str> = Profile::ALL.iter().map(|p| p.name).collect();
        format!("no profile {name:?}: one of {}", names.join(", "))
    })
}

/// The flags that choose a game: with any of them on the command line the
/// game plays it at once (scripted runs and tests); without, the menus.
const GAME_FLAGS: [&str; 16] = [
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
    "script",
    "demo",
];

/// Whether the command line chose a game.
fn plays_directly(m: &ArgMatches) -> bool {
    GAME_FLAGS.iter().any(|id| m.value_source(id) == Some(ValueSource::CommandLine))
}

/// The game the command line chose, loaded, and its session.
fn direct(
    args: &Args,
    audio: olr_audio::Audio,
    cfg: ShellConfig,
    flags: &Overrides,
) -> anyhow::Result<Shell> {
    let mode = args.mode.unwrap_or(if args.spectate { Mode::Deathmatch } else { Mode::Sandbox });
    anyhow::ensure!(
        args.record.is_none() || mode == Mode::Deathmatch,
        "--record records a Deathmatch (--mode deathmatch)"
    );
    anyhow::ensure!(!args.spectate || mode == Mode::Deathmatch, "--spectate watches a Deathmatch of bots");
    let assets = if let Some(file) = &args.replay {
        // Its packs from `--content`, else from the download caches by
        // content hash (a match on downloaded content, plan 11), read
        // without opening them: a game running meanwhile keeps its cache,
        // and this run creates, locks and writes none.
        let caches = olr_game::net::cache_dirs(store(args).0.cache_dir());
        setup::load_replay(&[args.content.as_path()], file, &caches)?
    } else if mode == Mode::Deathmatch {
        let o = DeathmatchOptions {
            seed: args.seed,
            bots: args.bots,
            difficulty: args.difficulty.difficulty(),
            lives: args.lives,
            kill_limit: args.kill_limit,
            time_limit: args.time_limit,
            spectate: args.spectate,
        };
        setup::load_deathmatch(&[args.content.as_path()], &args.mod_name, &args.level, &args.skin, &o)?
    } else {
        let options = SandboxOptions {
            mod_id: String::new(),
            level_id: String::new(),
            seed: args.seed,
            targets: args.targets,
            bonuses: false,
        };
        setup::load(&[args.content.as_path()], &args.mod_name, &args.level, &args.skin, options)?
    };
    for w in &assets.warnings {
        log::warn!("{w}");
    }
    anyhow::ensure!(
        args.speed.is_none_or(|s| s == Speed::X1) || !assets.human_plays(),
        "--speed is for watching (--replay, --spectate): a game you play runs at 1×"
    );
    let mut audio = audio;
    audio.load(&assets.m, assets.content.core());
    for w in audio.take_warnings() {
        log::warn!("{w}");
    }
    let session = Session::new(&assets, cfg.viewport, cfg.interpolation, audio)?;
    log::info!("game start: {}", assets.describe());
    let mut shell = Shell::playing(cfg, assets, session);
    // The HUD's layout flag (no file: the other video flags are the
    // config's).
    let mut v = shell.ui.settings().video.clone();
    flags.apply(&mut v);
    shell.ui.update(olr_ui::UiAction::SetVideo(v));
    Ok(shell)
}

/// The video flags given on the command line (they win over the saved
/// options for the run).
fn video_flags(m: &ArgMatches, args: &Args) -> Overrides {
    let given = |id: &str| m.value_source(id) == Some(ValueSource::CommandLine);
    Overrides {
        fullscreen: given("fullscreen").then_some(true),
        frame_rate: given("uncapped").then_some(olr_ui::FrameRate::Uncapped),
        filter: given("filter").then_some(match args.filter {
            Filter::Nearest => olr_ui::UpscaleFilter::Nearest,
            Filter::Sharp => olr_ui::UpscaleFilter::Sharp,
        }),
        interpolation: given("no_interp").then_some(false),
        hud_layout: given("hud_layout").then_some(match args.hud_layout {
            HudLayoutFlag::Auto => olr_ui::HudArrangement::Auto,
            HudLayoutFlag::Bar => olr_ui::HudArrangement::Bar,
            HudLayoutFlag::Sides => olr_ui::HudArrangement::Sides,
        }),
    }
}

/// The data directory's files (`--data-dir`, else the platform's), and a
/// warning when there is none.
fn store(args: &Args) -> (olr_ui::Store, Option<String>) {
    match &args.data_dir {
        Some(dir) => (olr_ui::Store::open(dir), None),
        None => olr_ui::Store::platform(),
    }
}

/// The menus, with the settings and profiles of the data directory; the
/// video `flags` on top of the saved options (not saved).
fn menus(args: &Args, audio: olr_audio::Audio, mut cfg: ShellConfig, flags: Overrides) -> Shell {
    let (mut store, warning) = store(args);
    let mut settings = store.load_settings();
    let profiles = store.load_profiles();
    for w in warning.into_iter().chain(settings.warnings).chain(profiles.warnings) {
        log::warn!("{w}");
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
    log::info!("menus; content {}, data {data}", args.content.display());
    let file = settings.value.video.clone();
    flags.apply(&mut settings.value.video);
    cfg.interpolation = video::interpolation(settings.value.video.interpolation);
    let ui = olr_ui::UiState::new(settings.value, profiles.value, seed);
    let mut shell = Shell::new(cfg, ui, store, audio);
    shell.set_overrides(file, flags);
    // The online flags: through the menus' own actions, nothing saved.
    shell.set_port(args.port);
    if let Some(profile) = args.net_sim {
        if let Err(e) = shell.start_net_sim(profile) {
            log::warn!("--net-sim: {e:#}");
        }
    } else if let Some(name) = &args.host {
        shell.host_from_flag(Some(name.clone()));
    } else if let Some(addr) = &args.join {
        shell.join_from_flag(addr);
    }
    // A flag's failure (hosting on a port taken, an address unknown).
    if (args.host.is_some() || args.join.is_some())
        && let Some(why) = shell.ui.message()
    {
        log::warn!("{why}");
    }
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

    let mut audio = if args.no_sound { olr_audio::Audio::silent() } else { olr_audio::Audio::new() };
    let silent = audio.is_silent();
    for w in audio.take_warnings() {
        log::warn!("{w}");
    }
    let gilrs = match gilrs::Gilrs::new() {
        Ok(g) => Some(g),
        Err(e) => {
            log::warn!("no gamepads: {e}");
            None
        }
    };
    let interpolation = if args.no_interp { Interpolation::Latest } else { Interpolation::Linear };
    let viewport = olr_scene::Viewport::GAME;
    let shell_cfg = ShellConfig { content: args.content.clone(), interpolation, viewport };
    let mut shell = if play_now {
        direct(&args, audio, shell_cfg, &flags)?
    } else {
        menus(&args, audio, shell_cfg, flags)
    };
    // `--edit`: the editor before the first frame.
    if let Some(dir) = &args.edit {
        let dir = edit_target(dir, &args.content).map_err(anyhow::Error::msg)?;
        shell.open_editor_dir(&dir).map_err(anyhow::Error::msg)?;
    }
    log::info!("audio {}", if silent { "silent" } else { "opened" });
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
        uncapped = v.frame_rate == olr_ui::FrameRate::Uncapped;
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
    log::info!("window system: {}", olr_game::window::system_name(wayland));
    let mut app = App::new(cfg, shell, silent, gilrs, autoplay);
    let ran = event_loop.run_app(&mut app);
    log::info!("{}", app.summary());
    if let Some(m) = app.match_summary() {
        log::info!("{m}");
    }
    log::info!("{}", app.latency_summary());
    if let Some(path) = &args.frame_log {
        let log = app.frame_log(args.frame_log_skip);
        let text = ron::ser::to_string_pretty(&log, ron::ser::PrettyConfig::default())?;
        std::fs::write(path, text + "\n").with_context(|| format!("{}", path.display()))?;
    }
    app.save_recording();
    let autoplay = app.autoplay_result();
    let error = app.error.take();
    // The window closes with the app; then, off any frame, the router
    // deletes the online games' port mappings (N51, ruling NT-2).
    let unmapping = app.take_unmapping();
    drop(app);
    let wait = olr_game::shell::UNMAP_WAIT;
    if !unmapping.wait(wait) {
        log::warn!("the router did not confirm deleting the port mapping within {wait:?}");
    }
    match ran {
        // The game never sets a failing exit code; winit does, most often
        // when the display server closed the connection (a compositor crash).
        Err(winit::error::EventLoopError::ExitFailure(_)) if error.is_none() => {
            Err(anyhow::anyhow!(window::lost_connection(wayland)))
        }
        ran => {
            ran.context("the event loop")?;
            if let Some(e) = error {
                return Err(e);
            }
            match autoplay {
                Some(Ok(())) => log::info!("autoplay passed"),
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
/// d3d12 driver and wgpu's GL backend; returns its exit code. A failure
/// to is a warning for the log, which starts after this.
fn relaunch(warnings: &mut Vec<String>) -> Option<ExitCode> {
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
            warnings.push(format!("could not restart with the WSL GPU defaults ({e}); continuing"));
            None
        }
    }
}

/// The data directory: `--data-dir`, else the platform's (none when the
/// platform has none).
fn data_dir(args: &Args) -> Option<PathBuf> {
    match &args.data_dir {
        Some(dir) => Some(dir.clone()),
        None => olr_ui::Store::platform().0.dir().map(Path::to_path_buf),
    }
}

fn main() -> ExitCode {
    let matches = Args::command().get_matches();
    let args = Args::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let mut warnings = Vec::new();
    // The relaunching parent writes no log: its child does.
    if let Some(code) = relaunch(&mut warnings) {
        return code;
    }
    // The terminal's first line (the log's is in its header).
    println!("olr {}", olr_game::VERSION);
    let (level, level_warning) = logging::level(args.log, std::env::var(olr_log::LEVEL_VAR).ok().as_deref());
    logging::start(data_dir(&args).as_deref(), level);
    for w in warnings.into_iter().chain(level_warning) {
        log::warn!("{w}");
    }
    // An update an interruption cut short is finished first (issue #4);
    // when this is the old build, the new one starts in its place.
    if olr_game::update::finish_interrupted_update() {
        olr_log::end();
        return ExitCode::SUCCESS;
    }
    let code = main_logged(&matches, args);
    olr_log::end();
    code
}

/// The run, with the log started.
fn main_logged(matches: &ArgMatches, args: Args) -> ExitCode {
    let flags = video_flags(matches, &args);
    // R6: bundled content is refused before any window opens.
    if let Some(dir) = &args.edit
        && let Err(why) = edit_target(dir, &args.content)
    {
        log::error!("{why}");
        return ExitCode::from(2);
    }
    match run(args, plays_directly(matches), flags) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> (Args, ArgMatches) {
        let m = Args::command().try_get_matches_from(args).unwrap();
        (Args::from_arg_matches(&m).unwrap(), m)
    }

    /// `olr --version` names the build: a release's (`dist`) or a local one.
    #[test]
    fn the_version_names_the_build() {
        let e = Args::command().try_get_matches_from(["olr", "--version"]).unwrap_err();
        assert_eq!(e.kind(), clap::error::ErrorKind::DisplayVersion);
        assert_eq!(e.to_string().trim(), format!("olr {}", olr_game::VERSION));
        if option_env!("OLR_BUILD_VERSION").is_none() {
            assert_eq!(olr_game::VERSION, concat!(env!("CARGO_PKG_VERSION"), "-dev (local build)"));
        }
    }

    #[test]
    fn edit_takes_a_directory_and_keeps_the_menus() {
        let (args, m) = parse(&["olr", "--edit", "p"]);
        assert_eq!(args.edit, Some(PathBuf::from("p")));
        assert!(!plays_directly(&m), "the menus run behind the editor");
        assert!(
            Args::command().try_get_matches_from(["olr", "--edit", "p", "--mode", "deathmatch"]).is_err()
        );
        // Batch C review: every flag that chooses a game is refused with it
        // (a command-line game under the editor quit on Esc, unasked).
        for flag in GAME_FLAGS {
            let arg =
                format!("--{}", if flag == "mod_name" { "mod".to_string() } else { flag.replace('_', "-") });
            let mut argv = vec!["olr".to_string(), "--edit".into(), "p".into(), arg.clone()];
            if !matches!(flag, "spectate" | "demo") {
                argv.push(if flag == "difficulty" { "easy".into() } else { "1".into() });
            }
            assert!(Args::command().try_get_matches_from(&argv).is_err(), "--edit with {arg}");
        }
    }

    #[test]
    fn edit_refuses_the_bundled_content() {
        let content = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/packs");
        if content.is_dir() {
            let bundled = content.join("olx-level-dirt-level");
            assert_eq!(edit_target(&bundled, &content), Err(olr_game::editor::BUNDLED.to_string()));
        }
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(edit_target(tmp.path(), &content), Ok(tmp.path().canonicalize().unwrap()));
        assert!(edit_target(&tmp.path().join("missing"), &content).is_err());
    }
}
