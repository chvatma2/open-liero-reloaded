//! The winit event handler: window events to the game, a frame per redraw.

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
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
        if let WindowEvent::ModifiersChanged(m) = &event {
            self.modifiers = m.state();
        }
        let capturing = self.shell.data.capture.as_ref().map(|c| c.device());
        if let Some(w) = &mut self.win
            && super::egui_hears(&event, capturing)
        {
            let before = w.egui.egui_input().events.len();
            let _ = w.egui.on_window_event(&w.window, &event);
            // A Ctrl+V egui-winit swallowed (an empty system clipboard) is
            // still a paste: the level editor pastes its own clipboard.
            if let WindowEvent::KeyboardInput { event: key, .. } = &event {
                use winit::keyboard::{Key, KeyCode, PhysicalKey};
                let v = matches!(&key.logical_key, Key::Character(c) if c.eq_ignore_ascii_case("v"))
                    || key.physical_key == PhysicalKey::Code(KeyCode::KeyV);
                let command = if cfg!(target_os = "macos") {
                    self.modifiers.super_key()
                } else {
                    self.modifiers.control_key()
                };
                let input = w.egui.egui_input_mut();
                let pushed = input.events.len() > before;
                if super::paste_swallowed(key.state.is_pressed(), command, v, pushed) {
                    input.events.push(egui::Event::Paste(String::new()));
                }
            }
        }
        // A key is the game's unless the in-game chat's field has it
        // (`App::game_key`); repeats are not presses.
        match &event {
            WindowEvent::KeyboardInput { event: key, .. } => {
                if let winit::keyboard::PhysicalKey::Code(code) = key.physical_key
                    && !key.repeat
                {
                    self.game_key(code, key.state.is_pressed());
                }
            }
            _ => self.inputs.on_window_event(&event),
        }
        // Mouse controls (issue #24): the buttons and the wheel are the
        // game's while it has the pointer; the cursor moves the pointer's.
        if self.pointer.active() {
            self.inputs.first_mut().on_mouse(&event);
        }
        match &event {
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer.on_cursor_moved([position.x as f32, position.y as f32]);
            }
            WindowEvent::Focused(focused) => {
                self.pointer.on_focus(*focused, self.win.as_ref().map(|w| &*w.window));
            }
            _ => {}
        }
        // The options' "press a key" box, while open.
        if let Some(c) = &mut self.shell.data.capture {
            c.on_window_event(&event);
        }
        let result = match &event {
            WindowEvent::CloseRequested => {
                self.close_requested(el);
                Ok(())
            }
            WindowEvent::Resized(size) => {
                self.resized((size.width, size.height));
                Ok(())
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                log::info!("display scale factor {scale_factor}");
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

    /// The mouse's raw motion: mouse controls' virtual cursor while the
    /// pointer is locked (issue #24).
    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            self.pointer.on_motion([delta.0 as f32, delta.1 as f32]);
        }
    }

    /// Draws continuously while the window can show frames. Minimised,
    /// covered or of no size, it stops asking for frames and sleeps until a
    /// window event (`Resized`, `Occluded`) changes that: the sandbox pauses
    /// (no ticks run, and the time hidden is not caught up afterwards).
    /// While the network is live (`Shell::network_live`: an online game,
    /// the Multiplayer screen's lists, a lookup) it goes on instead (decision N21): a
    /// frame without drawing (`App::hidden_frame`: the network, the
    /// match's ticks) every `Shell::hidden_wake` (4 ms with an online
    /// game, 60 ms for the lists alone), not spinning.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(g) = &mut self.gilrs {
            while let Some(ev) = g.next_event() {
                // A pad plugged in again takes its player's seat back by its
                // UUID (issue #32).
                if let Some(pad) = g.connected_gamepad(ev.id) {
                    self.inputs.pad_seen(usize::from(ev.id), pad.uuid());
                }
                self.inputs.on_gamepad(&ev);
                // The menus by pad (issue #31; they hear it when they show).
                if let Some(p) = olr_input::PadEvent::from_gilrs(&ev.event) {
                    self.shell.data.pad.on_pad(usize::from(ev.id), p);
                }
                if let Some(c) = &mut self.shell.data.capture {
                    c.on_gamepad(&ev);
                }
            }
        }
        // The players' lineup follows a pad plugged in again (issue #32).
        for (from, to) in self.inputs.take_reseated() {
            log::info!("{} plays from {} now", olr_ui::join::seat_name(from), olr_ui::join::seat_name(to));
            let out = self.shell.act(olr_ui::UiAction::Reseat { from, to });
            self.outcomes(el, out);
        }
        // A pad's Start pauses and resumes, as Esc (issue #28); the
        // editor's Play has no pause, a game without menus none either.
        if self.inputs.take_pause() && self.cfg.menus && !self.shell.editing_play() {
            let out = self.shell.act(olr_ui::UiAction::PadPause);
            self.outcomes(el, out);
        }
        let Some(w) = &self.win else { return };
        if w.gpu.drawable() {
            if std::mem::take(&mut self.paused) {
                self.last = None;
            }
            el.set_control_flow(ControlFlow::Poll);
            if super::hidden(&w.window) {
                // A hidden window gets no redraw on Windows (no `WM_PAINT`
                // while invisible): its frames until it shows are drawn
                // from here (as eframe does).
                if let Err(e) = self.frame(el) {
                    self.fail(el, e);
                }
            } else {
                w.window.request_redraw();
            }
        } else if let Some(wake) = self.shell.hidden_wake() {
            super::show_window(&w.window);
            self.paused = true;
            let out = self.hidden_frame();
            self.outcomes(el, out);
            el.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + wake));
        } else {
            super::show_window(&w.window);
            self.paused = true;
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}
