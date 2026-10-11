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
                    self.input.release_keys();
                }
            }
        }
    }

    /// The menus' video options: the UI scale now, the window's size and
    /// mode through the paced requests when they changed.
    fn apply_video(&mut self, v: rl_ui::VideoSettings) {
        rl_ui::theme::apply(&self.egui, v.ui_scale);
        let old = self.video.replace(v.clone());
        if old.as_ref().is_some_and(|o| (o.window, o.fullscreen) == (v.window, v.fullscreen)) {
            return;
        }
        if v.fullscreen {
            self.requests.fullscreen(true);
        } else {
            let desktop = self.win.as_ref().and_then(|w| w.window.current_monitor()).map(|m| m.size());
            let (w, h) = v.window.size(desktop.map(|s| (s.width, s.height)));
            self.requests.fullscreen(false);
            self.requests.size(w, h);
        }
    }
}
