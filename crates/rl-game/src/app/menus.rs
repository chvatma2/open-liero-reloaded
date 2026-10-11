//! What the menus ask of the window (`Shell`'s `Outcome`s).

use winit::event_loop::ActiveEventLoop;

use super::{App, viewport};
use crate::shell::Outcome;
use crate::window;

impl App {
    /// Does what the menus asked of the window.
    pub(super) fn outcomes(&mut self, el: &ActiveEventLoop, out: Vec<Outcome>) {
        for o in out {
            match o {
                Outcome::Quit => el.exit(),
                Outcome::Bindings(b) => self.input.set_bindings(b),
                Outcome::Video(v) => self.apply_video(v),
                Outcome::Started => {
                    let size = self.win.as_ref().map_or(window::DEFAULT_SIZE, |w| w.gpu.size());
                    self.shell.set_viewport(viewport(size));
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
            }
        }
    }

    /// The menus' video options (`video.rs`): the window's size and mode
    /// through the paced requests when they changed, the rest at once
    /// (interpolation is the shell's).
    fn apply_video(&mut self, v: rl_ui::VideoSettings) {
        let live = self.video.change(v, &mut self.requests);
        rl_ui::theme::apply(&self.egui, live.ui_scale);
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
