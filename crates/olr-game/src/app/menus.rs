//! What the menus ask of the window (`Shell`'s `Outcome`s).

use winit::event_loop::ActiveEventLoop;

use super::App;
use crate::shell::Outcome;

impl App {
    /// Does what the menus asked of the window.
    pub(super) fn outcomes(&mut self, el: &ActiveEventLoop, out: Vec<Outcome>) {
        for o in out {
            match o {
                Outcome::Quit => el.exit(),
                Outcome::Bindings(b) => self.input.set_bindings(b),
                Outcome::MouseControls(on) => self.input.set_mouse_controls(on),
                Outcome::Video(v) => self.apply_video(v),
                Outcome::Started => {
                    if let Some(s) = self.shell.session_mut() {
                        s.present.invalidate_gpu();
                    }
                    // The loading time is not caught up.
                    self.last = None;
                    // A watched game starts at `--speed`.
                    self.speed = self.cfg.speed;
                    self.rate.clear();
                    self.input.release_keys();
                }
                Outcome::PrepareReport => {
                    let env = self.report_environment(el);
                    self.shell.prepare_report(env);
                }
            }
        }
    }

    /// What a bug report needs of the window (issue #3): the GPU, the
    /// display as winit reports it, the window system.
    fn report_environment(&self, el: &ActiveEventLoop) -> crate::report::Environment {
        let window_system = Some(crate::window::system_name(super::is_wayland(el)).to_string());
        let Some(w) = &self.win else {
            return crate::report::Environment { window_system, ..Default::default() };
        };
        let monitor = w.window.current_monitor();
        let size = w.window.inner_size();
        crate::report::Environment {
            gpu: Some(w.gpu.facts()),
            display: Some(olr_ui::report::DisplayFacts {
                scale_factor: w.window.scale_factor(),
                window: (size.width, size.height),
                monitor: monitor.as_ref().map(|m| (m.size().width, m.size().height)),
                refresh_mhz: monitor.as_ref().and_then(|m| m.refresh_rate_millihertz()),
            }),
            window_system,
        }
    }

    /// The menus' video options (`video.rs`): the window's size and mode
    /// through the paced requests when they changed, the rest at once
    /// (interpolation is the shell's).
    fn apply_video(&mut self, v: olr_ui::VideoSettings) {
        let live = self.video.change(v, &mut self.requests);
        self.ui_scale = live.ui_scale;
        if let Some(f) = live.filter {
            self.cfg.render.filter = f;
            if let Some(w) = &mut self.win {
                w.gpu.renderer.settings.filter = f;
            }
        }
        if let Some(u) = live.uncapped {
            self.cfg.uncapped = u;
            if let Some(w) = &mut self.win {
                w.gpu.set_uncapped(u);
            }
        }
    }
}
