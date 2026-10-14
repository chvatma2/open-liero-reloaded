//! The winit event handler: window events to the game, a frame per redraw.

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use super::App;

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        el.set_control_flow(ControlFlow::Poll);
        if self.win.is_none()
            && let Err(e) = self.open(el)
        {
            self.fail(el, e);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if !self.current(id) {
            return;
        }
        let capturing = self.shell.data.capture.is_some();
        if let Some(w) = &mut self.win
            && super::egui_hears(&event, capturing)
        {
            let _ = w.egui.on_window_event(&w.window, &event);
        }
        // The in-game chat's field has the keys: the game hears none.
        let chatting = self.shell.data.ingame_chat.is_some();
        if !(chatting && matches!(event, WindowEvent::KeyboardInput { .. })) {
            self.input.on_window_event(&event);
        }
        // The options' "press a key" box, while open.
        if let Some(c) = &mut self.shell.data.capture {
            c.on_window_event(&event);
        }
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
            WindowEvent::Focused(focused) => {
                self.scores.focus(*focused);
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
    /// While the network is live (`Shell::network_live`: an online game,
    /// the Multiplayer screen's lists, a lookup) it goes on instead (decision N21): a
    /// frame without drawing (`App::hidden_frame`: the network, the
    /// match's ticks) every `net::HIDDEN_FRAME`, not spinning.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(g) = &mut self.gilrs {
            while let Some(ev) = g.next_event() {
                self.input.on_gamepad(&ev);
                if let Some(c) = &mut self.shell.data.capture {
                    c.on_gamepad(&ev);
                }
            }
        }
        let Some(w) = &self.win else { return };
        if w.gpu.drawable() {
            if std::mem::take(&mut self.paused) {
                self.last = None;
            }
            el.set_control_flow(ControlFlow::Poll);
            w.window.request_redraw();
        } else if self.shell.network_live() {
            self.paused = true;
            let out = self.hidden_frame();
            self.outcomes(el, out);
            let next = std::time::Instant::now() + crate::net::HIDDEN_FRAME;
            el.set_control_flow(ControlFlow::WaitUntil(next));
        } else {
            self.paused = true;
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}
