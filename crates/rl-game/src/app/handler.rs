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

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let capturing = self.shell.data.capture.is_some();
        if let Some(w) = &mut self.win
            && super::egui_hears(&event, capturing)
        {
            let _ = w.egui.on_window_event(&w.window, &event);
        }
        self.input.on_window_event(&event);
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
        } else {
            self.paused = true;
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}
