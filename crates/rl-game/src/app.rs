//! The winit application (spec §8.6): one thread, the event loop drives
//! the 100 Hz tick accumulator, then an interpolated frame.
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
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowId};

use crate::clock::TickClock;
use crate::gpu::{Gpu, OverlayPaint};
use crate::latency::Latency;
use crate::overlay::{Overlay, ScreenMap, Stats};
use crate::script::{Command, Step};
use crate::session::Session;
use crate::setup::Assets;
use crate::stats::Meter;
use crate::window::{self, Requests};

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
}

/// What exists once the window does.
struct Shown {
    window: Arc<Window>,
    gpu: Gpu,
    egui: egui_winit::State,
}

pub struct App {
    cfg: Config,
    assets: Assets,
    session: Session,
    input: InputState,
    gilrs: Option<gilrs::Gilrs>,
    clock: TickClock,
    list: DrawList,
    egui: egui::Context,
    win: Option<Shown>,
    debug: bool,
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
    /// What stopped the game, if something went wrong.
    pub error: Option<anyhow::Error>,
}

/// The viewport for a window of `size` pixels.
fn viewport(size: (u32, u32)) -> Viewport {
    let aspect = if size.1 == 0 { MAX_ASPECT } else { size.0 as f32 / size.1 as f32 };
    Viewport::from_aspect(aspect, MAX_ASPECT)
}

impl App {
    pub fn new(cfg: Config, assets: Assets, session: Session, gilrs: Option<gilrs::Gilrs>) -> App {
        let audio =
            if session.audio.is_silent() { "audio: silent (no output device)" } else { "audio: opened" };
        let stats = Stats { audio: audio.to_string(), ..Stats::default() };
        App {
            latency: Latency::new(cfg.latency_probe),
            cfg,
            assets,
            session,
            input: InputState::default(),
            gilrs,
            clock: TickClock::new(),
            list: DrawList::new(),
            egui: egui::Context::default(),
            win: None,
            debug: false,
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
            error: None,
        }
    }

    /// The whole run, for the exit summary.
    pub fn summary(&self) -> String {
        format!(
            "{}; {} catch-up ticks dropped; player died {} times, spawned {} times",
            self.meter.summary(),
            self.clock.dropped(),
            self.session.player_deaths,
            self.session.player_spawns
        )
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
        self.session.present.invalidate_gpu();
        let size = window.inner_size();
        self.session.set_viewport(viewport((size.width, size.height)));
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
        Ok(())
    }

    /// F5: the next frame uploads the new colour map.
    fn restart(&mut self) -> anyhow::Result<()> {
        self.session.restart(&self.assets)
    }

    /// The window is now `size` pixels: the surface and the view follow.
    fn resized(&mut self, size: (u32, u32)) {
        if let Some(w) = &mut self.win {
            w.gpu.resize(size);
        }
        self.session.set_viewport(viewport(size));
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

    /// One tick: the script's steps for it, then the game.
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
                Command::Restart => self.restart()?,
                Command::Debug => self.debug = !self.debug,
                Command::Quit => el.exit(),
                // Applied at the start of the next frame (`apply_requests`).
                Command::Size(w, h) => self.requests.size(w, h),
                Command::Fullscreen(on) => self.requests.fullscreen(on),
            }
        }
        let keys = self.input.latch_tick();
        self.latency.latched();
        let t0 = Instant::now();
        self.session.tick(keys);
        self.meter.tick(t0.elapsed().as_secs_f64());
        self.ticks += 1;
        if self.session.events.gameplay_dropped() > 0 && !self.dropped_warned {
            eprintln!("warning: a tick dropped gameplay events; its terrain area was rebuilt");
            self.dropped_warned = true;
        }
        Ok(())
    }

    /// One drawn frame: the surface texture first (any wait for VSync
    /// comes before the input is read, not between reading and drawing
    /// it), the ticks due, then the picture.
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
        if self.cfg.autoquit_after.is_some_and(|s| (now - start).as_secs_f64() >= s) {
            el.exit();
        }
        let s = &mut self.session;
        let alpha = if s.present.scene.options.interpolation == Interpolation::Latest {
            1.0
        } else {
            self.clock.alpha()
        };
        s.build(alpha, &mut self.list);
        self.meter.peak_entities = self.meter.peak_entities.max(s.present.fx.len());
        self.meter.peak_projectiles = self.meter.peak_projectiles.max(s.world.projectiles().count());
        if self.meter.frame(elapsed, &mut self.stats) {
            self.stats.dropped = self.clock.dropped();
            if self.cfg.print_stats {
                let st = &self.stats;
                println!(
                    "stats: {:.0} fps, frame {:.2} ms (max {:.2}), busy {:.2} ms, tick {:.3} ms (max {:.3}), {} ticks/s, entities {}, projectiles {}, voices {}",
                    st.fps,
                    st.frame_ms,
                    st.frame_ms_max,
                    st.busy_ms,
                    st.tick_ms,
                    st.tick_ms_max,
                    st.ticks_per_second,
                    s.present.fx.len(),
                    s.world.projectiles().count(),
                    s.audio.playing()
                );
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
        let overlay = Overlay { session: &self.session, alpha, map, debug: self.debug, stats: &self.stats };
        let mut out = self.egui.run_ui(input, |ui| crate::overlay::paint(ui, &overlay));
        w.egui.handle_platform_output(&w.window, out.platform_output);
        let jobs = self.egui.tessellate(out.shapes, out.pixels_per_point);
        let paint = OverlayPaint { jobs: &jobs, pixels_per_point: out.pixels_per_point };
        // The images and terrain the ticks since the last frame changed
        // (the colour map merges their dirty rectangles), then the frame.
        self.session.present.upload(&mut w.gpu.renderer);
        for warning in w.gpu.renderer.warnings().drain(..) {
            eprintln!("warning: {warning}");
        }
        let timing = w.gpu.present(frame, &self.list, &paint, &mut out.textures_delta);
        if let Some(t) = timing {
            self.latency.presented(Instant::now(), t);
        }
        let wait = timing.map_or(Duration::ZERO, |t| t.acquire + t.present);
        self.meter.busy(begin.elapsed().saturating_sub(wait).as_secs_f64());
        if std::mem::take(&mut self.shot_due) {
            let path = self.cfg.shot_dir.join(format!("shot-{:05}.png", self.ticks));
            // A screenshot that cannot be written is not worth the game.
            match w.gpu.screenshot(&self.list, &paint, &path) {
                Ok(()) => println!("rl-game: wrote {} (tick {})", path.display(), self.ticks),
                Err(e) => eprintln!("warning: screenshot {}: {e:#}", path.display()),
            }
        }
        Ok(())
    }

    fn on_key(&mut self, el: &ActiveEventLoop, ev: &KeyEvent) -> anyhow::Result<()> {
        if ev.state != ElementState::Pressed || ev.repeat {
            return Ok(());
        }
        self.latency.press(Instant::now());
        match ev.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => el.exit(),
            PhysicalKey::Code(KeyCode::F2) => self.debug = !self.debug,
            PhysicalKey::Code(KeyCode::F5) => self.restart()?,
            // OLX's Game.ShowWormHealth toggle.
            PhysicalKey::Code(KeyCode::KeyH) => {
                let o = &mut self.session.present.scene.options;
                o.show_health_bars = !o.show_health_bars;
            }
            _ => {}
        }
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        el.set_control_flow(ControlFlow::Poll);
        if self.win.is_none()
            && let Err(e) = self.open(el)
        {
            self.fail(el, e);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let Some(w) = &mut self.win {
            let _ = w.egui.on_window_event(&w.window, &event);
        }
        self.input.on_window_event(&event);
        let result = match &event {
            WindowEvent::CloseRequested => {
                el.exit();
                Ok(())
            }
            WindowEvent::Resized(size) => {
                self.resized((size.width, size.height));
                Ok(())
            }
            WindowEvent::Occluded(occluded) => {
                if let Some(w) = &mut self.win {
                    w.gpu.set_occluded(*occluded);
                }
                Ok(())
            }
            WindowEvent::KeyboardInput { event, .. } => self.on_key(el, event),
            WindowEvent::RedrawRequested => self.frame(el),
            _ => Ok(()),
        };
        if let Err(e) = result {
            self.fail(el, e);
        }
    }

    /// Draws continuously while the window can show frames. Minimised,
    /// covered or of no size, it stops asking for frames and sleeps until a
    /// window event (`Resized`, `Occluded`) changes that: the sandbox pauses
    /// (no ticks run, and the time hidden is not caught up afterwards).
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(g) = &mut self.gilrs {
            while let Some(ev) = g.next_event() {
                self.input.on_gamepad(&ev);
            }
        }
        let Some(w) = &self.win else { return };
        if w.gpu.drawable() {
            if std::mem::take(&mut self.paused) {
                self.last = None;
            }
            el.set_control_flow(ControlFlow::Poll);
            w.window.request_redraw();
        } else {
            self.paused = true;
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}
