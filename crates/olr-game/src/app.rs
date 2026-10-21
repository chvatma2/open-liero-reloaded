//! The winit application (spec §8.6): one thread, the event loop drives
//! the 100 Hz tick accumulator, then an interpolated frame. A Deathmatch's
//! bots think inside `Session::tick`, on this thread. What plays, and the
//! menus when the game started without a game on the command line, are
//! the [`Shell`]'s (`shell.rs`).
//!
//! Per tick: the script's steps, `input.latch_tick()`, `Session::tick`
//! (world, entities with the colour terrain, sound, scene), then the
//! terrain's dirty rectangles to the GPU. Per frame: `Session::build`
//! (scene, entities), the egui overlay, `Gpu::present`, a screenshot when
//! one is due.
//!
//! Online (plan 10 task 12): every frame first polls the online game
//! (`Shell::net_frame`); while an online match plays its peer runs the
//! ticks (the input latch is its keys' source) and the local tick clock
//! rests. While the window is hidden the loop keeps polling and ticking
//! without drawing ([`App::hidden_frame`], decision N21).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use olr_input::{Action, InputState, KeyCode};
use olr_render::UpscaleFilter;
use olr_scene::{DrawList, Interpolation};
use winit::event::{ElementState, KeyEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::PhysicalKey;
use winit::window::Window;

use crate::autoplay::{self, Autoplay, Effect, Seen};
use crate::clock::TickClock;
use crate::editor::HostOut;
use crate::editor::canvas::Canvas;
use crate::gpu::{Gpu, OverlayPaint};
use crate::hud::{MinimapTexture, ScoreKey};
use crate::latency::Latency;
use crate::overlay::{Overlay, ScreenMap, Stats};
use crate::script::{Command, Step};
use crate::session::Session;
use crate::shell::{Outcome, Shell};
use crate::speed::{self, Speed};
use crate::stats::Meter;
use crate::video::Video;
use crate::window::{self, Plan, Requests, Shape};

mod handler;
mod menus;

/// How often changed settings are written (`Shell::flush`).
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// How the game runs, from the command line.
pub struct Config {
    pub backends: wgpu::Backends,
    pub render: olr_render::RenderSettings,
    pub fullscreen: bool,
    /// No VSync: Immediate or Mailbox presentation when the surface has one.
    pub uncapped: bool,
    pub interpolation: Interpolation,
    /// Quit after this many seconds.
    pub autoquit_after: Option<f64>,
    /// Scripted input and commands, in tick order.
    pub script: Vec<Step>,
    pub shot_dir: PathBuf,
    /// Print the debug counters every second.
    pub print_stats: bool,
    /// Latency probes (`latency.rs`) this often.
    pub latency_probe: Option<Duration>,
    /// Where a Deathmatch's replay is saved (rematches next to it,
    /// `drive::record_path`).
    pub record: Option<PathBuf>,
    /// The menus run the game (no game on the command line): Esc and F5
    /// go to them, games start and end from them.
    pub menus: bool,
    /// How fast a watched game (a replay, bots alone) starts (`--speed`);
    /// a game a person plays always runs at 1×.
    pub speed: Speed,
    /// `--fake-clock`: every frame is `autoplay::FAKE_FRAME` of time and
    /// runs `FAKE_TICKS_PER_FRAME` ticks, whatever the wall clock says.
    pub fake_clock: bool,
}

/// The editor's uploads this frame (`outs`) into `canvas`'s map: a new
/// document, a size or art-scale change, or the material view turned on or
/// off rebuilds it whole (`full`: it goes up whole); an edit replaces its
/// rectangle.
fn canvas_outs(canvas: &mut Canvas, full: &mut bool, shell: &Shell, outs: Vec<HostOut>) {
    let Some(h) = &shell.editor else { return };
    let Some(doc) = h.state().document() else { return };
    let material = h.state().material_view();
    if outs.contains(&HostOut::FullUpload) || !canvas.fits(doc) || canvas.showing_material() != material {
        canvas.rebuild(doc, material);
        *full = true;
        return;
    }
    for o in outs {
        if let HostOut::Upload(r) = o {
            canvas.upload(doc, r);
        }
    }
}

/// Shows `window` (made hidden: `window::attributes`). A no-op where
/// windows have no visibility (Wayland) and once shown.
pub(crate) fn show_window(window: &Window) {
    if window.is_visible() == Some(false) {
        window.set_visible(true);
    }
}

/// The renderer's warnings remembered as logged, at most.
const MAX_LOGGED: usize = 256;

/// The frames a hidden window draws at most before it shows anyway.
pub(crate) const REVEAL_AFTER_FRAMES: u32 = 30;
/// The longest a hidden window waits for its first frame.
pub(crate) const REVEAL_AFTER: Duration = Duration::from_secs(1);

/// Whether a hidden window shows after its `attempts`-th frame, `waited`
/// after it was made: once one was presented, or by the deadline whatever
/// they gave (a surface that never presents while hidden, or whose frames
/// each time out, must not keep the window hidden).
pub(crate) fn reveal_now(presented: bool, attempts: u32, waited: Duration) -> bool {
    presented || attempts >= REVEAL_AFTER_FRAMES || waited >= REVEAL_AFTER
}

/// Whether `window` is still hidden (`window::attributes`), waiting for its
/// first frame. Never on Wayland, whose windows have no visibility.
pub(crate) fn hidden(window: &Window) -> bool {
    window.is_visible() == Some(false)
}

/// What exists once the window does.
struct Shown {
    window: Arc<Window>,
    gpu: Gpu,
    egui: egui_winit::State,
    /// The frames drawn while hidden, and when the window was made (`reveal_now`).
    hidden_frames: u32,
    made: Instant,
}

pub struct App {
    cfg: Config,
    /// The menus and what plays.
    shell: Shell,
    input: InputState,
    gilrs: Option<gilrs::Gilrs>,
    clock: TickClock,
    list: DrawList,
    /// The level editor's canvas (plan 14c): its colour map and the frame's
    /// terrain-only list.
    canvas: Canvas,
    canvas_list: Option<(DrawList, [u32; 4])>,
    /// The canvas's map goes up whole on the next draw (a new document, or
    /// the renderer's terrain was a game's).
    canvas_full: bool,
    /// The window's close waits for the editor's answer (`CloseDecision::Wait`).
    quit_after_close: bool,
    /// The renderer's warnings logged so far (each once).
    logged: std::collections::BTreeSet<String>,
    egui: egui::Context,
    win: Option<Shown>,
    debug: bool,
    /// Tab held: the scoreboard shows.
    scores: ScoreKey,
    /// The keyboard's modifiers, as the window last said.
    modifiers: winit::keyboard::ModifiersState,
    /// The HUD's minimap picture.
    minimap: MinimapTexture,
    /// The UI scale the options set (1 for the command line's games):
    /// on top of the height's (`olr_ui::theme::pixels_per_point`).
    ui_scale: f32,
    /// Replays saved (`--record`).
    saved: u32,
    /// Ticks since the start, across restarts (the script's clock).
    ticks: u64,
    script_at: usize,
    shot_due: bool,
    start: Option<Instant>,
    last: Option<Instant>,
    meter: Meter,
    latency: Latency,
    stats: Stats,
    dropped_warned: bool,
    /// The window could not show frames (minimised or covered): the game
    /// waits for window events and the sandbox is paused.
    paused: bool,
    /// Window size and mode changes asked for and not yet applied.
    requests: Requests,
    /// The window's size and mode, as last made or changed.
    shape: Shape,
    /// How changes reach the window (made again on Wayland).
    plan: Plan,
    /// The video options the window shows (the menus').
    video: Video,
    /// When the settings were last written (`Shell::flush`, once a second).
    flushed: Option<Instant>,
    /// How fast a watched game runs (PageUp, PageDown; `speed.rs`).
    speed: Speed,
    /// The speed it reached lately (the HUD says when it is short).
    rate: speed::Rate,
    /// `--autoplay`'s script, while it plays.
    autoplay: Option<Autoplay>,
    /// Frames drawn (the autoplay's and the fake clock's count).
    frames: u64,
    /// `--fake-clock`'s start: the time is this plus a frame's for each frame.
    fake_start: Option<Instant>,
    /// The autoplay's name for the screenshot due.
    shot_name: Option<String>,
    /// What stopped the game, if something went wrong.
    pub error: Option<anyhow::Error>,
    /// The surface's size at the last frame drawn (`--frame-log`).
    drawn_size: (u32, u32),
    /// F3: the network overlay (spec §7.3).
    net_overlay: bool,
    /// The in-game chat's field was open last frame (keys held are
    /// released when it opens: they go to it, not the game).
    chatting: bool,
}

/// Whether egui hears window event `event`: not a key while the options
/// capture one (`capturing`), so the captured key is the capture's alone
/// (Space would also press the focused button, Tab move the focus).
pub fn egui_hears(event: &winit::event::WindowEvent, capturing: bool) -> bool {
    use winit::event::WindowEvent as E;
    !(capturing && matches!(event, E::KeyboardInput { .. } | E::Ime(_)))
}

/// Whether egui-winit swallowed a paste (plan 14e batch C review):
/// Ctrl+V `pressed` with `command` held makes it send `Event::Paste` only
/// when the system clipboard holds text, and no key event either; the
/// level editor's own clipboard then never hears it. `v`: the key is V;
/// `pushed`: egui-winit added an event for it.
pub fn paste_swallowed(pressed: bool, command: bool, v: bool, pushed: bool) -> bool {
    pressed && command && v && !pushed
}

/// What a key does for a watched game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchKey {
    /// The next (1) or previous (-1) worm followed.
    Follow(i32),
    /// The next speed, faster or slower.
    Speed { faster: bool },
}

/// Whether the game has the screen (its keys, the worms' names): always in
/// a command-line game (no menus; its menus' state stays on the main menu),
/// else while the menus' panels do not hold it (`Shell::names_shown`).
pub fn game_has_screen(menus: bool, names_shown: bool) -> bool {
    !menus || names_shown
}

/// Whether the mouse cursor is hidden (the user's note, 2026-10-19): while
/// a game plays with the screen its own ([`game_has_screen`]), except the
/// level editor's Play (its Stop button) and the in-game chat. Play uses no
/// mouse.
pub fn cursor_hidden(
    menus: bool,
    names_shown: bool,
    playing: bool,
    editing_play: bool,
    chatting: bool,
) -> bool {
    playing && game_has_screen(menus, names_shown) && !editing_play && !chatting
}

/// What `code` does in a game: the arrows choose the worm followed in a
/// game nobody here plays (`spectating`: a replay, bots alone, an online
/// match watched), and are the player's own keys otherwise; PageUp and
/// PageDown change the speed of a game that can fast-forward (not a
/// `human` one: an online match runs at the host's pace). Nothing while a
/// menu holds the game (`in_game` false: the arrows are the menu's).
pub fn watch_key(code: KeyCode, spectating: bool, human: bool, in_game: bool) -> Option<WatchKey> {
    match code {
        KeyCode::ArrowLeft | KeyCode::ArrowRight if spectating && in_game => {
            Some(WatchKey::Follow(if code == KeyCode::ArrowRight { 1 } else { -1 }))
        }
        KeyCode::PageUp | KeyCode::PageDown if !human => {
            Some(WatchKey::Speed { faster: code == KeyCode::PageUp })
        }
        _ => None,
    }
}

/// Whether the event loop runs on Wayland.
fn is_wayland(el: &ActiveEventLoop) -> bool {
    #[cfg(target_os = "linux")]
    return winit::platform::wayland::ActiveEventLoopExtWayland::is_wayland(el);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = el;
        false
    }
}

/// The game's playfield in a window of `size` pixels, `[x, y, w, h]`:
/// its centred 16:9 frame above the HUD bar (`olr_ui::hud::playfield`),
/// whole pixels, at least one. The world is drawn there, its view the
/// same for every window (`olr_scene::Viewport::GAME`, spec §19 E4 as
/// amended after the v0.1 playtest).
pub fn playfield(size: (u32, u32)) -> [u32; 4] {
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32));
    let r = olr_ui::hud::playfield(window);
    let (x0, y0) = (r.left().round().max(0.0) as u32, r.top().round().max(0.0) as u32);
    let (x1, y1) = (r.right().round().max(0.0) as u32, r.bottom().round().max(0.0) as u32);
    [
        x0.min(size.0.saturating_sub(1)),
        y0.min(size.1.saturating_sub(1)),
        (x1 - x0.min(x1)).max(1),
        (y1 - y0.min(y1)).max(1),
    ]
}

/// The height, in pixels, the UI's scale follows
/// (`olr_ui::theme::pixels_per_point`): in game the window's 16:9 frame's,
/// so the HUD and the game's panels take the same share of it on every
/// window; in the menus the window's.
pub fn scale_height(size: (u32, u32), in_game: bool) -> f32 {
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32));
    if in_game { olr_ui::hud::frame(window).height() } else { size.1 as f32 }
}

impl App {
    /// The game `shell` plays (or its menus, with `cfg.menus`); `silent`:
    /// no sound output device.
    pub fn new(
        cfg: Config,
        shell: Shell,
        silent: bool,
        gilrs: Option<gilrs::Gilrs>,
        autoplay: Option<Autoplay>,
    ) -> App {
        let audio = if silent { "audio: silent (no output device)" } else { "audio: opened" };
        let stats = Stats { audio: audio.to_string(), ..Stats::default() };
        let egui = egui::Context::default();
        let fake = cfg.fake_clock && autoplay.is_some();
        if autoplay.is_some() {
            // The autoplay finds widgets by label in the AccessKit tree.
            egui.enable_accesskit();
        }
        let video = Video::new(shell.ui.settings().video.clone());
        let ui_scale = if cfg.menus { shell.ui.settings().video.ui_scale } else { 1.0 };
        let mut input = InputState::default();
        if cfg.menus {
            olr_ui::theme::apply(&egui);
            input.set_bindings(shell.ui.bindings().clone());
        }
        App {
            speed: cfg.speed,
            autoplay,
            frames: 0,
            // The fake clock runs a fixed number of ticks a frame: only for
            // an autoplay, never for a person (it would change the speed).
            fake_start: fake.then(Instant::now),
            rate: speed::Rate::default(),
            shot_name: None,
            latency: Latency::new(cfg.latency_probe),
            cfg,
            shell,
            input,
            gilrs,
            clock: TickClock::new(),
            list: DrawList::new(),
            canvas: Canvas::new(),
            canvas_list: None,
            canvas_full: true,
            quit_after_close: false,
            logged: std::collections::BTreeSet::new(),
            egui,
            win: None,
            debug: false,
            scores: ScoreKey::default(),
            modifiers: winit::keyboard::ModifiersState::default(),
            minimap: MinimapTexture::default(),
            ui_scale,
            saved: 0,
            ticks: 0,
            script_at: 0,
            shot_due: false,
            start: None,
            last: None,
            meter: Meter::new(),
            stats,
            dropped_warned: false,
            paused: false,
            requests: Requests::default(),
            shape: Shape { size: window::DEFAULT_SIZE, physical: false, fullscreen: false },
            plan: Plan::InPlace,
            video,
            flushed: None,
            error: None,
            drawn_size: (0, 0),
            net_overlay: false,
            chatting: false,
        }
    }

    /// The menus and what plays.
    pub fn shell(&self) -> &Shell {
        &self.shell
    }

    pub fn shell_mut(&mut self) -> &mut Shell {
        &mut self.shell
    }

    /// An online game exists (decision N21).
    pub fn online_active(&self) -> bool {
        self.shell.online_active()
    }

    /// The network is live (`Shell::network_live`): the loop runs its
    /// frame, the window hidden or not.
    pub fn network_live(&self) -> bool {
        self.shell.network_live()
    }

    /// The network's frame at `now`, as every drawn and hidden frame runs
    /// it, while anything network-related is live (`Shell::network_live`:
    /// the Multiplayer screen's lists, a lookup, an online game): the
    /// lists and the relays polled, the peer polled (an online match's
    /// ticks run inside, the input latch giving the player's keys for
    /// each new tick), its events carried out.
    pub fn network_frame(&mut self, now: Instant) -> Vec<Outcome> {
        if !self.shell.network_live() {
            return Vec::new();
        }
        self.net_step(now)
    }

    fn net_step(&mut self, now: Instant) -> Vec<Outcome> {
        // What is held as the poll latches (no input arrives meanwhile).
        self.shell.set_held_keys(self.input.held());
        let (input, latency) = (&mut self.input, &mut self.latency);
        let mut keys = || {
            latency.latched();
            input.latch_tick()
        };
        let t0 = Instant::now();
        let mut out = self.shell.net_frame(now, &mut keys);
        // The F2 panel's and the summary's ticks: the new ones the frame's
        // poll ran (never the network pump's, minor 9), its time shared
        // among them (re-simulated ticks included in it).
        let ran = self.shell.session().filter(|_| self.shell.plays_online()).map_or(0, Session::polled_ticks);
        if ran > 0 {
            let each = t0.elapsed().as_secs_f64() / f64::from(ran);
            for _ in 0..ran {
                self.meter.tick(each);
            }
        }
        out.extend(self.shell.after_ticks());
        out
    }

    /// A frame while the window cannot show one (minimised or covered)
    /// and the network is live (decision N21): the network polled and
    /// the match ticked, nothing drawn. `about_to_wait` runs it about
    /// every `Shell::hidden_wake` and does the outcomes.
    pub fn hidden_frame(&mut self) -> Vec<Outcome> {
        let now = self.now();
        let mut out = self.network_frame(now);
        out.extend(self.shell.poll());
        if self.cfg.menus && self.flushed.is_none_or(|t| now.saturating_duration_since(t) >= FLUSH_INTERVAL) {
            self.flushed = Some(now);
            self.shell.flush();
        }
        out
    }

    /// The whole run, for the exit summary.
    pub fn summary(&self) -> String {
        let (deaths, spawns) = self.shell.session().map_or((0, 0), |s| (s.player_deaths, s.player_spawns));
        format!(
            "{}; {} catch-up ticks dropped; player died {deaths} times, spawned {spawns} times",
            self.meter.summary(),
            self.clock.dropped(),
        )
    }

    /// `--frame-log`: the run's frames after `skip_s` seconds (plan 8).
    pub fn frame_log(&self, skip_s: f64) -> crate::stats::FrameLog {
        self.meter.frame_log(skip_s, self.drawn_size, &self.stats.gpu)
    }

    /// A Deathmatch's bots and kills over the run, for the exit summary
    /// (`None` for another game).
    pub fn match_summary(&self) -> Option<String> {
        let s = self.shell.session()?;
        let crate::session::Driver::Match(d) = &s.driver else { return None };
        let t = &s.tally;
        Some(format!(
            "{} bots think per tick p50 {:.3} ms p99 {:.3} ms max {:.3} ms, units max {} of budget {}, {} over budget; \
             match time {:.1} s ({} ended, {} rematches): bot kills {} ({:.2}/min), player kills {}, suicides {}",
            d.bots.len(),
            d.think_ms(0.5),
            d.think_ms(0.99),
            f64::from(d.think_ns_max) / 1e6,
            d.units_max,
            d.budget_min,
            d.over_budget,
            t.match_ticks as f64 / 100.0,
            t.matches_ended,
            s.rematches,
            t.bot_kills,
            t.bot_kills_per_minute(),
            t.player_kills,
            t.suicides,
        ))
    }

    /// At the end of the run: the playing match's replay saved (`--record`;
    /// with the menus, in the data directory), the settings written.
    pub fn save_recording(&mut self) {
        if self.cfg.menus {
            self.shell.finish();
        } else if let Some(r) = self.shell.session_mut().and_then(|s| s.take_replay()) {
            self.save_replay(&r);
        }
    }

    /// The router mappings being deleted (ruling NT-2): the caller waits
    /// for them once the window closed (`Unmapping::wait`).
    pub fn take_unmapping(&mut self) -> crate::shell::Unmapping {
        self.shell.take_unmapping()
    }

    /// Writes `r` to the next `--record` path; a failure is a warning.
    fn save_replay(&mut self, r: &olr_replay::Replay) {
        let Some(base) = &self.cfg.record else { return };
        let path = crate::drive::record_path(base, self.saved);
        self.saved += 1;
        match std::fs::write(&path, r.encode()) {
            Ok(()) => log::info!("recorded {} ({} ticks)", path.display(), r.ticks()),
            Err(e) => log::warn!("recording {}: {e}", path.display()),
        }
    }

    /// The autoplay's outcome at the end of the run (`None` without one):
    /// a script the game ended before it finished failed.
    pub fn autoplay_result(&mut self) -> Option<Result<(), String>> {
        let frames = self.frames;
        self.autoplay.as_mut().map(|a| a.finish(frames))
    }

    /// The time the game's logic sees: the wall clock, or under
    /// `--fake-clock` the start plus a frame's time for each frame drawn.
    fn now(&self) -> Instant {
        match self.fake_start {
            Some(t) => {
                t + autoplay::FAKE_FRAME.saturating_mul(u32::try_from(self.frames).unwrap_or(u32::MAX))
            }
            None => Instant::now(),
        }
    }

    /// Input latency and swap-chain waits over the run (`latency.rs`).
    pub fn latency_summary(&self) -> String {
        self.latency.summary()
    }

    fn fail(&mut self, el: &ActiveEventLoop, e: anyhow::Error) {
        self.error.get_or_insert(e);
        el.exit();
    }

    fn open(&mut self, el: &ActiveEventLoop) -> anyhow::Result<()> {
        self.shape = if self.cfg.menus {
            // The options' window (the command line's flags on top), made
            // at its size and mode: no change follows the creation.
            let o = Video::opening(&self.shell.ui.settings().video);
            Shape { size: o.size, physical: true, fullscreen: o.fullscreen }
        } else {
            Shape { size: window::DEFAULT_SIZE, physical: false, fullscreen: self.cfg.fullscreen }
        };
        self.plan = window::plan(is_wayland(el));
        let window = Arc::new(el.create_window(self.shape.attributes())?);
        let gpu = Gpu::new(
            window.clone(),
            el.owned_display_handle(),
            self.cfg.backends,
            self.cfg.render,
            self.cfg.uncapped,
        )?;
        // A new renderer: the next frame uploads the atlas and the terrain.
        if let Some(s) = self.shell.session_mut() {
            s.present.invalidate_gpu();
        }
        let egui = self.egui_state(&window, &gpu);
        log::info!("GPU {}", gpu.info);
        log::info!(
            "window {:?}, scale factor {}; video: {}, filter {:?}, interpolation {:?}, UI scale {}",
            self.shape,
            window.scale_factor(),
            if self.cfg.uncapped { "uncapped" } else { "VSync" },
            self.cfg.render.filter,
            self.cfg.interpolation,
            self.ui_scale,
        );
        self.stats.gpu = gpu.info.clone();
        self.win = Some(Shown { window, gpu, egui, hidden_frames: 0, made: Instant::now() });
        // Creating the window counts as a change: a scripted `size` right
        // away waits out the interval (`window::MIN_CHANGE_INTERVAL`).
        self.requests.opened(self.now());
        Ok(())
    }

    /// egui's window state for `window`, on the one egui context (its
    /// textures, fonts and the menus' memory stay with the context).
    fn egui_state(&self, window: &Window, gpu: &Gpu) -> egui_winit::State {
        let max_side = gpu.renderer.device().limits().max_texture_dimension_2d as usize;
        let scale = Some(window.scale_factor() as f32);
        egui_winit::State::new(self.egui.clone(), egui::ViewportId::ROOT, window, scale, None, Some(max_side))
    }

    /// Makes the window again at [`App::shape`] (`window::Plan::Recreate`):
    /// a new winit window, its wgpu surface (`Gpu::rebind`: the device and
    /// every GPU resource stay) and egui's window state on the same
    /// context; the game, the menus, the sound and the clock never notice.
    /// Keys held are released (the old window's releases never come). The
    /// old window is closed as soon as the new one exists, its surface
    /// first (in `Gpu::rebind`); if the new one cannot be made, the old one
    /// stays and takes the change in place.
    fn recreate(&mut self, el: &ActiveEventLoop, change: window::Change) {
        let Some(old) = self.win.take() else { return };
        let Shown { window: old_window, mut gpu, egui: old_egui, .. } = old;
        let made = el
            .create_window(self.shape.attributes())
            .map_err(anyhow::Error::from)
            .map(Arc::new)
            .and_then(|w| gpu.rebind(w.clone()).map(|()| w));
        let window = match made {
            Ok(w) => w,
            Err(e) => {
                log::warn!("making the window again: {e:#}; changing it in place");
                let size = window::apply(&old_window, change);
                self.win = Some(Shown {
                    window: old_window,
                    gpu,
                    egui: old_egui,
                    hidden_frames: 0,
                    made: Instant::now(),
                });
                if let Some(size) = size {
                    self.resized((size.width, size.height));
                }
                return;
            }
        };
        let egui = self.egui_state(&window, &gpu);
        log::info!("the window was made again: {:?}", self.shape);
        self.win = Some(Shown { window, gpu, egui, hidden_frames: 0, made: Instant::now() });
        drop((old_egui, old_window));
        self.input.release_keys();
        self.scores.focus(false);
        // The interval runs from the new window (`window::MIN_CHANGE_INTERVAL`).
        self.requests.opened(self.now());
    }

    /// F5 (a Deathmatch's rematch; its replay is saved): the next frame
    /// uploads the new colour map.
    fn restart(&mut self, el: &ActiveEventLoop) -> anyhow::Result<()> {
        if self.cfg.menus {
            let out = self.shell.act(olr_ui::UiAction::Restart);
            self.outcomes(el, out);
            return Ok(());
        }
        let Some(p) = &mut self.shell.play else { return Ok(()) };
        if let Some(r) = p.session.restart(&p.assets)? {
            self.save_replay(&r);
        }
        Ok(())
    }

    /// The window is now `size` pixels: the surface follows (the view
    /// does not: it is the same for every window).
    fn resized(&mut self, size: (u32, u32)) {
        log::debug!("window resized to {} × {}", size.0, size.1);
        if let Some(w) = &mut self.win {
            w.gpu.resize(size);
        }
    }

    /// The window size and mode changes asked for, at most once a frame
    /// (and once every `window::MIN_CHANGE_INTERVAL`), before the frame's
    /// surface texture is acquired.
    fn apply_requests(&mut self, el: &ActiveEventLoop) {
        let Some(change) = self.requests.due(self.now()) else { return };
        log::info!("window change: {change:?}");
        let shown = self.shape.apply(change);
        match self.plan {
            Plan::Recreate if shown => self.recreate(el, change),
            Plan::Recreate => {}
            Plan::InPlace => {
                // Wayland applies the size at once and sends no `Resized`.
                if let Some(size) = self.win.as_ref().and_then(|w| window::apply(&w.window, change)) {
                    self.resized((size.width, size.height));
                }
            }
        }
    }

    /// Whether window event of window `id` is the game's: not one of a
    /// window made again and closed (its last `Resized` or `Focused`).
    fn current(&self, id: winit::window::WindowId) -> bool {
        self.win.as_ref().is_none_or(|w| w.window.id() == id)
    }

    /// Presses (or releases) the key bound to `action` through the input
    /// path, as a player would (autoplay's `Press`, tests).
    pub fn press(&mut self, action: Action, pressed: bool) {
        if let Some(k) = self.key_of(action) {
            self.input.on_key(k, pressed);
        }
    }

    /// The ticks the F2 panel and the exit summary counted so far.
    pub fn ticks_measured(&self) -> u64 {
        self.meter.run_ticks()
    }

    /// The key bound to `action` (the script presses keys as a player would).
    fn key_of(&self, action: Action) -> Option<KeyCode> {
        self.input.bindings().keys.iter().find(|(a, _)| *a == action).map(|&(_, k)| k)
    }

    /// How fast the game playing runs now: [`App::speed`] when it is
    /// watched (nobody plays it), else 1× (`Speed::for_game`).
    pub fn speed(&self) -> Speed {
        self.speed.for_game(self.shell.session().is_none_or(|s| s.human()))
    }

    /// The ticks of one frame: the `due` ones at 1×, more when a watched
    /// game runs fast, their sounds thinned out (`speed::run_frame`).
    fn run_ticks(&mut self, el: &ActiveEventLoop, due: u32) -> anyhow::Result<()> {
        let speed = self.speed();
        let start = Instant::now();
        let fake = self.fake_start.is_some();
        // Under the fake clock max runs a fixed number of ticks a frame.
        let spent = |n| if fake { n >= speed::FAKE_MAX_TICKS } else { start.elapsed() >= speed::MAX_BUDGET };
        let ran = speed::run_frame(speed, due, spent, |sound| {
            self.set_sound(sound);
            self.tick(el)
        })?;
        self.set_sound(true);
        if speed == Speed::X1 {
            self.rate.clear();
        } else {
            self.rate.push(ran, due);
        }
        Ok(())
    }

    fn set_sound(&mut self, on: bool) {
        if let Some(s) = self.shell.session_mut() {
            s.sound = on;
        }
    }

    /// One tick: the script's steps for it, then the game (none in the
    /// menus, the pause or the weapon selection; the script's clock still
    /// counts the tick, so at 1× it keeps real time: `speed::run_frame`).
    /// Whether the world ticked.
    fn tick(&mut self, el: &ActiveEventLoop) -> anyhow::Result<bool> {
        while let Some(step) = self.cfg.script.get(self.script_at).filter(|s| s.tick <= self.ticks).copied() {
            self.script_at += 1;
            match step.command {
                Command::Press(a) | Command::Release(a) => {
                    if let Some(k) = self.key_of(a) {
                        self.input.on_key(k, matches!(step.command, Command::Press(_)));
                    }
                }
                Command::Shot => self.shot_due = true,
                Command::Restart => self.restart(el)?,
                Command::Debug => self.debug = !self.debug,
                Command::Scores(on) => self.scores.set(on),
                Command::Quit => el.exit(),
                // Applied at the start of the next frame (`apply_requests`).
                Command::Size(w, h) => self.requests.size(w, h),
                Command::Fullscreen(on) => self.requests.fullscreen(on),
            }
        }
        let keys = self.input.latch_tick();
        self.latency.latched();
        self.ticks += 1;
        let t0 = Instant::now();
        // Not while the pause menu or the weapon selection holds the game.
        if !self.shell.tick(keys) {
            return Ok(false);
        }
        self.meter.tick(t0.elapsed().as_secs_f64());
        let Some(s) = self.shell.session() else { return Ok(true) };
        if s.events.gameplay_dropped() > 0 && !self.dropped_warned {
            log::warn!("a tick dropped gameplay events; its terrain area was rebuilt");
            self.dropped_warned = true;
        }
        Ok(true)
    }

    /// One drawn frame: the surface texture first (any wait for VSync
    /// comes before the input is read, not between reading and drawing
    /// it), the ticks due, then the picture: the game and its overlay, or
    /// the menus. A game the menus asked for in an earlier frame is loaded
    /// after this one is shown (its "Loading…" with it).
    fn frame(&mut self, el: &ActiveEventLoop) -> anyhow::Result<()> {
        let begin = Instant::now();
        self.apply_requests(el);
        let frame = self.win.as_mut().and_then(|w| w.gpu.acquire());
        self.latency.frame_start(Instant::now());
        let now = self.now();
        let start = *self.start.get_or_insert(now);
        let elapsed = self.last.map_or(0.0, |l| (now - l).as_secs_f64());
        self.last = Some(now);
        let out = self.network_frame(now);
        self.outcomes(el, out);
        // An online match's peer runs its ticks (`net_step`): the local
        // clock rests meanwhile (nothing to catch up afterwards).
        let online = self.shell.plays_online();
        let due = if online {
            0
        } else if self.fake_start.is_some() {
            autoplay::FAKE_TICKS_PER_FRAME
        } else {
            self.clock.advance(elapsed)
        };
        if !online {
            self.run_ticks(el, due)?;
        }
        // Online, `net_step` reported the results already.
        if self.cfg.menus && !self.shell.online_active() {
            let out = self.shell.after_ticks();
            self.outcomes(el, out);
        }
        if self.cfg.autoquit_after.is_some_and(|s| (now - start).as_secs_f64() >= s) {
            el.exit();
        }
        let interpolate = self.clock.alpha();
        let mut alpha = 1.0;
        if let Some(s) = self.shell.session_mut() {
            // Online the pace's own tick phase (the local clock's is not the
            // netcode's): 1 while it stalls, catches up or rolls back, and
            // while the pace is held (a migration's resume hold, a jump
            // back: `tick_phase` is `None`, plan 12c Task 8b).
            if s.present.scene.options.interpolation != Interpolation::Latest {
                alpha = s.online_alpha(now).unwrap_or(interpolate);
            }
            s.build(alpha, &mut self.list);
            self.meter.peak_entities = self.meter.peak_entities.max(s.present.fx.len());
            let projectiles = s.with_world(|w| w.projectiles().count());
            self.meter.peak_projectiles = self.meter.peak_projectiles.max(projectiles);
        }
        if self.meter.frame(elapsed, &mut self.stats) {
            self.stats.dropped = self.clock.dropped();
            if self.cfg.print_stats {
                crate::stats::print(&self.stats, self.shell.session());
            }
        }
        let speed = self.speed();
        let speed_shown = (speed != Speed::X1).then(|| speed::shown(speed, self.rate.effective()));
        let Some(w) = &mut self.win else { return Ok(()) };
        let size = w.gpu.size();
        self.drawn_size = size;
        let native = w.window.scale_factor() as f32;
        // The UI's scale follows the height (in game the 16:9 frame's), not
        // the display's DPI; a change shows from the next frame.
        let height = scale_height(size, self.shell.play.is_some());
        olr_ui::theme::set_scale(&self.egui, native, height, self.ui_scale);
        let ppp = native * self.egui.zoom_factor();
        let src = (
            self.list.view.w * w.gpu.renderer.scale() as f32,
            self.list.view.h * w.gpu.renderer.scale() as f32,
        );
        // The world in the playfield: the 16:9 frame above the HUD's bar.
        let dest = olr_render::dest_rect_in(w.gpu.renderer.settings.filter, src, playfield(size));
        let map = ScreenMap { view: self.list.view, dest, pixels_per_point: ppp };
        let mut input = w.egui.take_egui_input(&w.window);
        if self.fake_start.is_some() {
            input.time = Some((now - start).as_secs_f64());
            input.predicted_dt = autoplay::FAKE_FRAME.as_secs_f32();
        } else {
            egui_frame_time(&mut input, elapsed);
        }
        if let Some(a) = &mut self.autoplay {
            a.input(&mut input);
        }
        let minimap = match self.shell.session() {
            Some(s) => Some(self.minimap.update(&self.egui, s)),
            None => {
                self.minimap.clear();
                None
            }
        };
        let time = (now - start).as_secs_f64();
        // The weapon selection reads the game's keys, keyboard or gamepad.
        self.shell.data.held = self.input.held();
        let edited_play = self.shell.editing_play();
        let overlay = self.shell.play.as_ref().map(|p| Overlay {
            session: &p.session,
            edited: edited_play,
            alpha,
            map,
            debug: self.debug,
            show_scores: self.scores.shown(),
            stats: &self.stats,
            menus: self.cfg.menus,
            minimap,
            time,
            speed: speed_shown,
            selecting: self.shell.selecting(),
            names: game_has_screen(self.cfg.menus, self.shell.names_shown()),
            changing: self.shell.data.held.0 & olr_sim::Keys::CHANGE != 0,
            arrangement: self.shell.ui.settings().video.hud_layout,
            net: self.net_overlay.then(|| self.shell.net_view()).flatten(),
            lagging: self.shell.lagging(),
            migrating: self.shell.migration_line().map(str::to_string),
        });
        let editing = self.shell.editor_open() && !self.shell.editing_play();
        let playing_edited = self.shell.editing_play();
        let stop_at = {
            let [x, y, _, h] = playfield(size);
            egui::pos2(x as f32 / ppp, (y + h) as f32 / ppp)
        };
        let (ui_state, data, mut editor) = (&self.shell.ui, &mut self.shell.data, self.shell.editor.as_mut());
        let menus = self.cfg.menus;
        let mut actions = Vec::new();
        let mut editor_actions = Vec::new();
        let mut stop = false;
        let mut out = self.egui.run_ui(input, |ui| {
            if let Some(o) = &overlay {
                crate::overlay::paint(ui, o);
            }
            if let Some(h) = editor.as_deref_mut().filter(|_| editing) {
                let (state, data) = h.parts();
                editor_actions = olr_editor::ui::show(ui, state, data);
            } else if playing_edited {
                stop = crate::overlay::stop_button(ui, stop_at);
            } else if menus {
                actions = olr_ui::screens::show(ui, ui_state, data);
            }
        });
        if editing {
            let out = self.shell.editor_frame(editor_actions, Instant::now());
            canvas_outs(&mut self.canvas, &mut self.canvas_full, &self.shell, out);
        }
        // The canvas's map is not kept while the editor is closed or its Play
        // runs (the session has its own); it is made again on return.
        if !self.shell.editor_open() || self.shell.editing_play() {
            self.canvas.clear();
        }
        // An edited Deathmatch over: back to the editor when its results
        // would show (no results screen there).
        let over = self.shell.editing_play() && self.shell.session().is_some_and(Session::results_shown);
        if menus {
            // The profiles' skin preview this frame asked for, from the next.
            self.shell.update_preview(&self.egui);
            self.shell.log_menu_warnings();
            // The chat's field takes the keys: those held go up.
            let chatting = self.shell.data.ingame_chat.is_some();
            if chatting && !self.chatting {
                self.input.release_keys();
            }
            self.chatting = chatting;
        }
        let tree = out.platform_output.accesskit_update.take();
        let hide = cursor_hidden(
            menus,
            self.shell.names_shown(),
            self.shell.play.is_some(),
            self.shell.editing_play(),
            self.shell.data.ingame_chat.is_some(),
        );
        if hide {
            // egui-winit hides the cursor for this icon and shows it again
            // when egui next asks for another.
            out.platform_output.cursor_icon = egui::CursorIcon::None;
        }
        w.egui.handle_platform_output(&w.window, out.platform_output);
        // The autoplay's steps, on what this frame shows.
        let mut effects = Vec::new();
        if let Some(a) = &mut self.autoplay {
            let seen = Seen {
                frame: self.frames,
                tree: tree.as_ref(),
                screen: menus.then(|| self.shell.ui.screen()),
                window: w.gpu.size(),
                speed,
                ticks: self.shell.session().map(crate::session::Session::ticks),
                members: self.shell.data.lobby.as_ref().map(|l| l.members.len()),
                shown: !hidden(&w.window),
            };
            for e in a.step(&seen) {
                match e {
                    Effect::Shot(name) => {
                        self.shot_due = true;
                        self.shot_name = Some(name);
                    }
                    e => effects.push(e),
                }
            }
        }
        let jobs = self.egui.tessellate(out.shapes, out.pixels_per_point);
        let paint = OverlayPaint { jobs: &jobs, pixels_per_point: out.pixels_per_point };
        // The images and terrain the ticks since the last frame changed
        // (the colour map merges their dirty rectangles), then the frame.
        let world = self.shell.play.as_mut().map(|p| {
            p.session.present.upload(&mut w.gpu.renderer);
            (&self.list, p.session.present.look(), playfield(size))
        });
        // The editor's canvas: its map's changes, then its part of the window
        // (drawn sharp: the overlays sit on it at any zoom).
        let editor_view = self
            .shell
            .editor
            .as_ref()
            .filter(|_| editing)
            .map(|h| (h.state().view(), h.data_ref().canvas_rect()));
        let mut filter = None;
        let world = match editor_view {
            Some((view, Some(rect))) => {
                // A level with a look draws lit (plan 15a Task 19): the
                // renderer's look textures come with a whole upload.
                let state = self.shell.editor.as_ref().map(|h| h.state());
                let lit = state.and_then(|s| s.document()).is_some_and(|d| self.canvas.lit(d));
                if w.gpu.renderer.lighting() != lit {
                    w.gpu.renderer.set_lighting(lit);
                    self.canvas_full = true;
                }
                if let Some(map) = self.canvas.map_mut() {
                    if std::mem::take(&mut self.canvas_full) {
                        map.take_dirty().for_each(drop);
                        w.gpu.renderer.upload_terrain(map);
                    } else {
                        w.gpu.renderer.update_terrain(map);
                    }
                }
                let max = w.gpu.renderer.device().limits().max_texture_dimension_2d;
                let max_view = Canvas::max_view_for(max, w.gpu.renderer.scale());
                // egui's own scale this pass (the frame's `ppp` may be the
                // last one's: a Play's end changes the UI's height).
                self.canvas_list = self.canvas.draw(&view, rect, out.pixels_per_point, max_view);
                filter = Some(std::mem::replace(
                    &mut w.gpu.renderer.settings.filter,
                    UpscaleFilter::SharpBilinear,
                ));
                let look = match (state, &self.canvas_list) {
                    (Some(st), Some(_)) => {
                        st.document().and_then(|d| self.canvas.look_frame(d, st.clock_ms(), st.view_mode()))
                    }
                    _ => None,
                };
                self.canvas_list.as_ref().map(|(l, a)| (l, look, *a))
            }
            Some((_, None)) => None,
            None => world,
        };
        for warning in w.gpu.renderer.warnings().drain(..) {
            // Each once (at most `MAX_LOGGED` remembered: a text that varies
            // never grows the set without bound).
            if self.logged.len() < MAX_LOGGED && self.logged.insert(warning.clone()) {
                log::warn!("{warning}");
            }
            // The editor says it too (a level whose art the GPU crops).
            if let Some(h) = self.shell.editor.as_mut().filter(|_| editing) {
                h.act(vec![olr_editor::EditorAction::Note(warning)]);
            }
        }
        let timing = w.gpu.present(frame, world, &paint, &mut out.textures_delta);
        if hidden(&w.window) {
            w.hidden_frames += 1;
            if reveal_now(timing.is_some(), w.hidden_frames, w.made.elapsed()) {
                show_window(&w.window);
            }
        }
        if let Some(t) = timing {
            self.latency.presented(Instant::now(), t);
        }
        let wait = timing.map_or(Duration::ZERO, |t| t.acquire + t.present);
        // `Meter`'s busy times are indexed beside its frame times (`frame`
        // above): no early return may come between the two calls.
        self.meter.busy(begin.elapsed().saturating_sub(wait).as_secs_f64());
        if std::mem::take(&mut self.shot_due) {
            let name = self.shot_name.take();
            let file =
                name.as_ref().map_or_else(|| format!("shot-{:05}.png", self.ticks), |n| format!("{n}.png"));
            let path = self.cfg.shot_dir.join(file);
            // A screenshot that cannot be written is not worth the game
            // (an autoplay's is: it checks what the picture shows).
            let shot = w.gpu.screenshot(world, &paint, &path);
            match &shot {
                Ok(_) => log::info!("wrote {} (tick {})", path.display(), self.ticks),
                Err(e) => log::warn!("screenshot {}: {e:#}", path.display()),
            }
            if let (Some(name), Some(a)) = (&name, &mut self.autoplay) {
                let check = shot.map_err(|e| format!("{e:#}")).and_then(|img| autoplay::sanity(&img));
                effects.extend(a.shot_taken(self.frames, name, check));
            }
        }
        if let Some(f) = filter {
            w.gpu.renderer.settings.filter = f;
        }
        // The window's close waits for the editor: it closed, or its
        // question was answered otherwise (Cancel).
        if self.quit_after_close {
            if !self.shell.editor_open() {
                el.exit();
            } else if self.shell.editor.as_ref().is_some_and(|h| h.state().dialog().is_none()) {
                self.quit_after_close = false;
            }
        }
        if stop || over {
            self.stop_edited();
            if over && let Some(h) = self.shell.editor() {
                h.act(vec![olr_editor::EditorAction::Note("the match is over".into())]);
            }
        }
        if menus {
            // The game asked for in an earlier frame (this one showed its
            // "Loading…"), then this frame's actions.
            let out = self.shell.poll();
            self.outcomes(el, out);
            for a in actions {
                let out = self.shell.act(a);
                self.outcomes(el, out);
            }
            if self.flushed.is_none_or(|t| now.saturating_duration_since(t) >= FLUSH_INTERVAL) {
                self.flushed = Some(now);
                self.shell.flush();
            }
        }
        for e in effects {
            self.autoplay_effect(el, e)?;
        }
        self.frames += 1;
        Ok(())
    }

    /// The editor's Play ends (Esc, Stop): back to the editor as it was;
    /// the renderer's terrain was the match's, so the canvas goes up whole.
    fn stop_edited(&mut self) {
        self.shell.stop_edited();
        self.canvas_full = true;
        self.input.release_keys();
    }

    /// The window's close button: with the editor open it asks about
    /// unsaved changes first (Review Focus 1), and the window closes when
    /// the editor does.
    pub(super) fn close_requested(&mut self, el: &ActiveEventLoop) {
        match self.shell.request_close() {
            crate::editor::CloseDecision::Quit => el.exit(),
            crate::editor::CloseDecision::Wait => {
                self.quit_after_close = true;
                self.canvas_full = true;
            }
        }
    }

    /// PageUp (`faster`) or PageDown: the watched game's next speed (the
    /// caller, `watch_key`, leaves out a game a person plays: it stays at 1×).
    fn change_speed(&mut self, faster: bool) {
        if self.shell.session().is_none_or(|s| s.human()) {
            return;
        }
        self.speed = if faster { self.speed.faster() } else { self.speed.slower() };
        self.rate.clear();
        log::info!("speed {}", self.speed.label());
    }

    fn on_key(&mut self, el: &ActiveEventLoop, ev: &KeyEvent) -> anyhow::Result<()> {
        let PhysicalKey::Code(code) = ev.physical_key else { return Ok(()) };
        self.key(el, code, ev.state == ElementState::Pressed, ev.repeat)
    }

    /// The game's own keys (the window's, or the autoplay's taps).
    fn key(
        &mut self,
        el: &ActiveEventLoop,
        code: KeyCode,
        pressed: bool,
        repeat: bool,
    ) -> anyhow::Result<()> {
        // The options capture this key, or the chat's field has it: it is
        // theirs alone (no scores, debug panel or health bars toggling).
        if self.shell.data.capture.is_some() || self.shell.data.ingame_chat.is_some() {
            return Ok(());
        }
        // OLX's ShowScore: the scoreboard while Tab is held.
        if code == KeyCode::Tab {
            self.scores.key(pressed);
            return Ok(());
        }
        if !pressed || repeat {
            return Ok(());
        }
        self.latency.press(Instant::now());
        // The chat key online (the options' `online.chat_key`): its field
        // takes the keys from now on.
        if code == self.shell.ui.settings().online.chat_key && self.shell.open_chat() {
            self.input.release_keys();
            return Ok(());
        }
        match code {
            // With the menus, Esc and F5 are theirs (`olr_ui::screens::show`).
            // The editor's Play: Esc returns to the editor at once.
            KeyCode::Escape if self.shell.editing_play() => self.stop_edited(),
            // F5 plays the edited level again from its start (the HUD says so).
            KeyCode::F5 if self.shell.editing_play() => self.shell.restart_edited(),
            KeyCode::Escape if !self.cfg.menus => el.exit(),
            KeyCode::F5 if !self.cfg.menus => self.restart(el)?,
            KeyCode::F2 => self.debug = !self.debug,
            // The network overlay (spec §7.3).
            KeyCode::F3 => self.net_overlay = !self.net_overlay,
            // A watched game: the arrows choose the worm followed, PageUp
            // and PageDown the speed (`watch_key`).
            KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::PageUp | KeyCode::PageDown => {
                let in_game = game_has_screen(self.cfg.menus, self.shell.names_shown());
                let Some(s) = self.shell.session_mut() else { return Ok(()) };
                match watch_key(code, s.spectating(), s.human(), in_game) {
                    Some(WatchKey::Follow(step)) => s.follow_next(step),
                    Some(WatchKey::Speed { faster }) => self.change_speed(faster),
                    None => {}
                }
            }
            // OLX's Game.ShowWormHealth toggle.
            KeyCode::KeyH => {
                if let Some(s) = self.shell.session_mut() {
                    let o = &mut s.present.scene.options;
                    o.show_health_bars = !o.show_health_bars;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Does what the autoplay asked (a screenshot was taken already).
    fn autoplay_effect(&mut self, el: &ActiveEventLoop, e: Effect) -> anyhow::Result<()> {
        match e {
            Effect::Shot(_) => {}
            // As the window delivers a key: the input path, then the game's keys.
            Effect::Key(code, pressed) => {
                self.input.on_key(code, pressed);
                self.key(el, code, pressed, false)?;
            }
            Effect::Action(a, pressed) => self.press(a, pressed),
            Effect::Quit => {
                log::info!("autoplay passed ({} frames)", self.frames);
                el.exit();
            }
            Effect::Fail(why) => self.fail(el, anyhow::anyhow!(why)),
        }
        Ok(())
    }
}

/// Tells egui how long this frame took (`elapsed`, the game's clock). The
/// game draws every frame without asking egui for a repaint, so egui's
/// `stable_dt` (its animations' time step, and the profiles' skin
/// preview's) is `predicted_dt`, 1/60 s unless set: on a faster display
/// every animation ran that much too fast. The first frame keeps egui's.
fn egui_frame_time(input: &mut egui::RawInput, elapsed: f64) {
    if elapsed > 0.0 {
        input.predicted_dt = elapsed as f32;
    }
}

#[cfg(test)]
mod tests {
    use winit::event::{Ime, WindowEvent};

    use super::{REVEAL_AFTER, REVEAL_AFTER_FRAMES, reveal_now};

    /// Review of the window fix (Critical 1): a hidden window shows with its
    /// first presented frame, and after `REVEAL_AFTER_FRAMES` attempts
    /// whatever they gave, so a surface that never presents while hidden
    /// cannot keep it hidden.
    #[test]
    fn a_hidden_window_shows_by_its_deadline() {
        use std::time::Duration;
        let soon = Duration::from_millis(5);
        assert!(reveal_now(true, 1, soon));
        assert!(!reveal_now(false, 1, soon));
        assert!(!reveal_now(false, REVEAL_AFTER_FRAMES - 1, soon));
        assert!(reveal_now(false, REVEAL_AFTER_FRAMES, soon));
        // Frames that each wait long (a surface timing out while hidden):
        // the time's deadline (deferred minor of batch B).
        assert!(!reveal_now(false, 2, REVEAL_AFTER - soon));
        assert!(reveal_now(false, 2, REVEAL_AFTER));
    }

    /// The lobby's Copy (the invite code) and pasting into a text field go
    /// through egui-winit's clipboard, a feature its default features
    /// carry: with them off, the workspace must name it, or both silently
    /// do nothing (the user's report on Windows).
    #[test]
    fn egui_winit_has_its_clipboard() {
        let manifest = include_str!("../../../Cargo.toml");
        let line =
            manifest.lines().find(|l| l.starts_with("egui-winit")).expect("egui-winit in the workspace");
        assert!(line.contains("\"clipboard\""), "{line}");
    }

    /// The game draws every frame without asking egui for a repaint, so
    /// egui takes `predicted_dt` as each frame's time: it must be the
    /// frame's real time, or menu animations (the profiles' skin preview)
    /// run at 60 Hz's pace however fast the display is.
    #[test]
    fn egui_animates_by_the_frames_real_time() {
        let ctx = egui::Context::default();
        let frame = 1.0 / 144.0;
        let mut dt = 0.0;
        for n in 1..=30 {
            let mut input = egui::RawInput { time: Some(n as f64 * frame), ..Default::default() };
            super::egui_frame_time(&mut input, frame);
            ctx.run_ui(input, |ui| dt = ui.input(|i| i.stable_dt)).textures_delta.clear();
        }
        assert!((dt - frame as f32).abs() < 1e-5, "stable_dt {dt}, the frame {frame}");
    }

    /// While the options capture a key, egui hears no key or text: the
    /// capture has it alone. Other events still reach egui.
    #[test]
    fn a_swallowed_paste_is_given_back() {
        // Ctrl+V pressed and egui-winit added nothing: its clipboard had no text.
        assert!(super::paste_swallowed(true, true, true, false));
        // It sent its own Paste, a release, no Ctrl, another key: nothing to add.
        assert!(!super::paste_swallowed(true, true, true, true));
        assert!(!super::paste_swallowed(false, true, true, false));
        assert!(!super::paste_swallowed(true, false, true, false));
        assert!(!super::paste_swallowed(true, true, false, false));
    }

    #[test]
    fn egui_hears_no_key_while_capturing() {
        let text = WindowEvent::Ime(Ime::Commit(" ".into()));
        assert!(super::egui_hears(&text, false));
        assert!(!super::egui_hears(&text, true));
        assert!(super::egui_hears(&WindowEvent::Focused(true), true));
    }

    /// The game is drawn in the window's 16:9 frame above the HUD bar: on
    /// a 16:9 window all its width (no black strips), letterboxed on a
    /// narrower one, pillarboxed on a wider one; the view in it is the
    /// same world area at every size, so the picture fills the playfield.
    #[test]
    fn the_playfield_is_the_frame_above_the_bar() {
        use super::{playfield, scale_height};
        use olr_render::{UpscaleFilter, dest_rect_in};
        assert_eq!(playfield((1280, 720)), [0, 0, 1280, 576]);
        assert_eq!(playfield((1920, 1080)), [0, 0, 1920, 864]);
        assert_eq!(playfield((3440, 1440)), [440, 0, 2560, 1152], "21:9: the frame centred");
        assert_eq!(playfield((2560, 1600)), [0, 80, 2560, 1152], "16:10: black above and below");
        let view = olr_scene::Viewport::GAME;
        for size in
            [(1280, 720), (1600, 900), (1920, 1080), (2560, 1440), (3840, 2160), (3440, 1440), (2560, 1600)]
        {
            let area = playfield(size);
            let src = (view.width as f32 * 2.0, view.height as f32 * 2.0);
            let [x, y, w, h] = dest_rect_in(UpscaleFilter::SharpBilinear, src, area);
            let fills = (x - area[0] as f32).abs() <= 1.0
                && (y - area[1] as f32).abs() <= 1.0
                && (w - area[2] as f32).abs() <= 2.0
                && (h - area[3] as f32).abs() <= 2.0;
            assert!(fills, "{size:?}: the picture {:?} fills the playfield {area:?}", [x, y, w, h]);
            assert_eq!(scale_height(size, true), (size.0 as f32 * 9.0 / 16.0).min(size.1 as f32), "{size:?}");
            assert_eq!(scale_height(size, false), size.1 as f32);
        }
        for size in [(0, 0), (1, 1), (5, 3000), (3000, 5)] {
            let [x, y, w, h] = playfield(size);
            assert!(w >= 1 && h >= 1 && x <= size.0 && y <= size.1, "{size:?}");
        }
    }
}
