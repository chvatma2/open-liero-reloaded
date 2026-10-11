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

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rl_input::{Action, InputState, KeyCode};
use rl_scene::{DrawList, Interpolation, Viewport};
use winit::event::{ElementState, KeyEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::PhysicalKey;
use winit::window::Window;

use crate::clock::TickClock;
use crate::gpu::{Gpu, OverlayPaint};
use crate::hud::ScoreKey;
use crate::latency::Latency;
use crate::overlay::{Overlay, ScreenMap, Stats};
use crate::script::{Command, Step};
use crate::shell::Shell;
use crate::stats::Meter;
use crate::window::{self, Requests};

mod handler;
mod menus;

/// How often changed settings are written (`Shell::flush`).
const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// The widest view (spec §19 E4: 16:9).
pub const MAX_ASPECT: f32 = 16.0 / 9.0;

/// How the game runs, from the command line.
pub struct Config {
    pub backends: wgpu::Backends,
    pub render: rl_render::RenderSettings,
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
}

/// What exists once the window does.
struct Shown {
    window: Arc<Window>,
    gpu: Gpu,
    egui: egui_winit::State,
}

pub struct App {
    cfg: Config,
    /// The menus and what plays.
    shell: Shell,
    input: InputState,
    gilrs: Option<gilrs::Gilrs>,
    clock: TickClock,
    list: DrawList,
    egui: egui::Context,
    win: Option<Shown>,
    debug: bool,
    /// Tab held: the scoreboard shows.
    scores: ScoreKey,
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
    /// The video options last applied (the menus' window preset).
    video: Option<rl_ui::VideoSettings>,
    /// When the settings were last written (`Shell::flush`, once a second).
    flushed: Option<Instant>,
    /// What stopped the game, if something went wrong.
    pub error: Option<anyhow::Error>,
}

/// The viewport for a window of `size` pixels.
pub(crate) fn viewport(size: (u32, u32)) -> Viewport {
    let aspect = if size.1 == 0 { MAX_ASPECT } else { size.0 as f32 / size.1 as f32 };
    Viewport::from_aspect(aspect, MAX_ASPECT)
}

impl App {
    /// The game `shell` plays (or its menus, with `cfg.menus`); `silent`:
    /// no sound output device.
    pub fn new(cfg: Config, shell: Shell, silent: bool, gilrs: Option<gilrs::Gilrs>) -> App {
        let audio = if silent { "audio: silent (no output device)" } else { "audio: opened" };
        let stats = Stats { audio: audio.to_string(), ..Stats::default() };
        let egui = egui::Context::default();
        let mut input = InputState::default();
        if cfg.menus {
            rl_ui::theme::apply(&egui, shell.ui.settings().video.ui_scale);
            input.set_bindings(shell.ui.bindings().clone());
        }
        App {
            latency: Latency::new(cfg.latency_probe),
            cfg,
            shell,
            input,
            gilrs,
            clock: TickClock::new(),
            list: DrawList::new(),
            egui,
            win: None,
            debug: false,
            scores: ScoreKey::default(),
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
            video: None,
            flushed: None,
            error: None,
        }
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

    /// Writes `r` to the next `--record` path; a failure is a warning.
    fn save_replay(&mut self, r: &rl_replay::Replay) {
        let Some(base) = &self.cfg.record else { return };
        let path = crate::drive::record_path(base, self.saved);
        self.saved += 1;
        match std::fs::write(&path, r.encode()) {
            Ok(()) => println!("rl-game: recorded {} ({} ticks)", path.display(), r.ticks()),
            Err(e) => eprintln!("warning: recording {}: {e}", path.display()),
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
        let attrs = window::attributes(window::DEFAULT_SIZE, self.cfg.fullscreen);
        let window = Arc::new(el.create_window(attrs)?);
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
        let size = window.inner_size();
        self.shell.set_viewport(viewport((size.width, size.height)));
        let max_side = gpu.renderer.device().limits().max_texture_dimension_2d as usize;
        let scale = Some(window.scale_factor() as f32);
        let egui = egui_winit::State::new(
            self.egui.clone(),
            egui::ViewportId::ROOT,
            &window,
            scale,
            None,
            Some(max_side),
        );
        println!("rl-game: GPU {}", gpu.info);
        self.stats.gpu = gpu.info.clone();
        self.win = Some(Shown { window, gpu, egui });
        // Creating the window counts as a change: a scripted `size` right
        // away waits out the interval (`window::MIN_CHANGE_INTERVAL`).
        self.requests.opened(Instant::now());
        Ok(())
    }

    /// F5 (a Deathmatch's rematch; its replay is saved): the next frame
    /// uploads the new colour map.
    fn restart(&mut self, el: &ActiveEventLoop) -> anyhow::Result<()> {
        if self.cfg.menus {
            let out = self.shell.act(rl_ui::UiAction::Restart);
            self.outcomes(el, out);
            return Ok(());
        }
        let Some(p) = &mut self.shell.play else { return Ok(()) };
        if let Some(r) = p.session.restart(&p.assets)? {
            self.save_replay(&r);
        }
        Ok(())
    }

    /// The window is now `size` pixels: the surface and the view follow.
    fn resized(&mut self, size: (u32, u32)) {
        if let Some(w) = &mut self.win {
            w.gpu.resize(size);
        }
        self.shell.set_viewport(viewport(size));
    }

    /// The window size and mode changes asked for, at most once a frame
    /// (and once every `window::MIN_CHANGE_INTERVAL`), before the frame's
    /// surface texture is acquired.
    fn apply_requests(&mut self) {
        let Some(change) = self.requests.due(Instant::now()) else { return };
        // Wayland applies the size at once and sends no `Resized`.
        if let Some(size) = self.win.as_ref().and_then(|w| window::apply(&w.window, change)) {
            self.resized((size.width, size.height));
        }
    }

    /// The key bound to `action` (the script presses keys as a player would).
    fn key_of(&self, action: Action) -> Option<KeyCode> {
        self.input.bindings().keys.iter().find(|(a, _)| *a == action).map(|&(_, k)| k)
    }

    /// One tick: the script's steps for it, then the game (none in the
    /// menus or the pause menu; the script's clock runs on).
    fn tick(&mut self, el: &ActiveEventLoop) -> anyhow::Result<()> {
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
        let paused = self.shell.paused();
        let Some(s) = self.shell.session_mut().filter(|_| !paused) else { return Ok(()) };
        let t0 = Instant::now();
        s.tick(keys);
        self.meter.tick(t0.elapsed().as_secs_f64());
        if s.events.gameplay_dropped() > 0 && !self.dropped_warned {
            eprintln!("warning: a tick dropped gameplay events; its terrain area was rebuilt");
            self.dropped_warned = true;
        }
        Ok(())
    }

    /// One drawn frame: the surface texture first (any wait for VSync
    /// comes before the input is read, not between reading and drawing
    /// it), the ticks due, then the picture: the game and its overlay, or
    /// the menus. A game the menus asked for in an earlier frame is loaded
    /// after this one is shown (its "Loading…" with it).
    fn frame(&mut self, el: &ActiveEventLoop) -> anyhow::Result<()> {
        let begin = Instant::now();
        self.apply_requests();
        let frame = self.win.as_mut().and_then(|w| w.gpu.acquire());
        let now = Instant::now();
        self.latency.frame_start(now);
        let start = *self.start.get_or_insert(now);
        let elapsed = self.last.map_or(0.0, |l| (now - l).as_secs_f64());
        self.last = Some(now);
        for _ in 0..self.clock.advance(elapsed) {
            self.tick(el)?;
        }
        if self.cfg.menus {
            let out = self.shell.after_ticks();
            self.outcomes(el, out);
        }
        if self.cfg.autoquit_after.is_some_and(|s| (now - start).as_secs_f64() >= s) {
            el.exit();
        }
        let interpolate = self.clock.alpha();
        let mut alpha = 1.0;
        if let Some(s) = self.shell.session_mut() {
            if s.present.scene.options.interpolation != Interpolation::Latest {
                alpha = interpolate;
            }
            s.build(alpha, &mut self.list);
            self.meter.peak_entities = self.meter.peak_entities.max(s.present.fx.len());
            self.meter.peak_projectiles = self.meter.peak_projectiles.max(s.world.projectiles().count());
        }
        if self.meter.frame(elapsed, &mut self.stats) {
            self.stats.dropped = self.clock.dropped();
            if self.cfg.print_stats {
                crate::stats::print(&self.stats, self.shell.session());
            }
        }
        let Some(w) = &mut self.win else { return Ok(()) };
        let size = w.gpu.size();
        let ppp = w.window.scale_factor() as f32;
        let src = (
            self.list.view.w * w.gpu.renderer.scale() as f32,
            self.list.view.h * w.gpu.renderer.scale() as f32,
        );
        let dest = rl_render::dest_rect(w.gpu.renderer.settings.filter, src, size);
        let map = ScreenMap { view: self.list.view, dest, pixels_per_point: ppp };
        let input = w.egui.take_egui_input(&w.window);
        let overlay = self.shell.play.as_ref().map(|p| Overlay {
            session: &p.session,
            alpha,
            map,
            debug: self.debug,
            show_scores: self.scores.shown(),
            stats: &self.stats,
            menus: self.cfg.menus,
        });
        let (ui_state, data) = (&self.shell.ui, &mut self.shell.data);
        let menus = self.cfg.menus;
        let mut actions = Vec::new();
        let mut out = self.egui.run_ui(input, |ui| {
            if let Some(o) = &overlay {
                crate::overlay::paint(ui, o);
            }
            if menus {
                actions = rl_ui::screens::show(ui, ui_state, data);
            }
        });
        w.egui.handle_platform_output(&w.window, out.platform_output);
        let jobs = self.egui.tessellate(out.shapes, out.pixels_per_point);
        let paint = OverlayPaint { jobs: &jobs, pixels_per_point: out.pixels_per_point };
        // The images and terrain the ticks since the last frame changed
        // (the colour map merges their dirty rectangles), then the frame.
        let world = self.shell.play.as_mut().map(|p| {
            p.session.present.upload(&mut w.gpu.renderer);
            &self.list
        });
        for warning in w.gpu.renderer.warnings().drain(..) {
            eprintln!("warning: {warning}");
        }
        let timing = w.gpu.present(frame, world, &paint, &mut out.textures_delta);
        if let Some(t) = timing {
            self.latency.presented(Instant::now(), t);
        }
        let wait = timing.map_or(Duration::ZERO, |t| t.acquire + t.present);
        self.meter.busy(begin.elapsed().saturating_sub(wait).as_secs_f64());
        if std::mem::take(&mut self.shot_due) {
            let path = self.cfg.shot_dir.join(format!("shot-{:05}.png", self.ticks));
            // A screenshot that cannot be written is not worth the game.
            match w.gpu.screenshot(world, &paint, &path) {
                Ok(()) => println!("rl-game: wrote {} (tick {})", path.display(), self.ticks),
                Err(e) => eprintln!("warning: screenshot {}: {e:#}", path.display()),
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
        Ok(())
    }

    fn on_key(&mut self, el: &ActiveEventLoop, ev: &KeyEvent) -> anyhow::Result<()> {
        // OLX's ShowScore: the scoreboard while Tab is held.
        if ev.physical_key == PhysicalKey::Code(KeyCode::Tab) {
            self.scores.key(ev.state == ElementState::Pressed);
            return Ok(());
        }
        if ev.state != ElementState::Pressed || ev.repeat {
            return Ok(());
        }
        self.latency.press(Instant::now());
        match ev.physical_key {
            // With the menus, Esc and F5 are theirs (`rl_ui::screens::show`).
            PhysicalKey::Code(KeyCode::Escape) if !self.cfg.menus => el.exit(),
            PhysicalKey::Code(KeyCode::F5) if !self.cfg.menus => self.restart(el)?,
            PhysicalKey::Code(KeyCode::F2) => self.debug = !self.debug,
            // OLX's Game.ShowWormHealth toggle.
            PhysicalKey::Code(KeyCode::KeyH) => {
                if let Some(s) = self.shell.session_mut() {
                    let o = &mut s.present.scene.options;
                    o.show_health_bars = !o.show_health_bars;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
