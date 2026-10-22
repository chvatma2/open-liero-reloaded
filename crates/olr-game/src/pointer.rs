//! Mouse controls' pointer (issue #24): the cursor the controller aims at,
//! grabbed during a match and free elsewhere.
//!
//! While the game has the pointer ([`wanted`]: mouse controls on, a match
//! played with the screen the game's, no menu, pause, chat or weapon
//! selection, the window focused) the window's pointer is **locked** and a
//! virtual cursor moves by the mouse's motion, clamped to the drawn view
//! (so no dead zone over the HUD's bar or side panels); where locking is
//! refused it is **confined** to the window and the cursor's own position
//! is clamped to the view; where both are refused (Wayland without pointer
//! constraints, WSLg, ...) the free cursor is clamped, logged once. Losing
//! the pointer (a menu, focus lost) frees it; it is grabbed again once the
//! game has it back.
//!
//! The controller's cursor direction is in screen space, from the local
//! worm as last drawn to the cursor, in game pixels ([`Pointer::aim`]).

use winit::window::{CursorGrabMode, Window};

/// What holds the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grab {
    /// Nothing: the menus' pointer.
    Free,
    /// Locked in place; the virtual cursor follows the mouse's motion.
    Locked,
    /// Confined to the window; the cursor's position is clamped.
    Confined,
    /// Both refused: the free cursor, clamped.
    Refused,
}

/// Whether the game has the pointer: mouse controls on, the cursor hidden
/// for play (`app::cursor_hidden`: a match with the screen the game's, no
/// menu, pause or chat), no weapon selection (it takes clicks), the window
/// focused.
pub fn wanted(mouse: bool, cursor_hidden: bool, selecting: bool, focused: bool) -> bool {
    mouse && cursor_hidden && !selecting && focused
}

/// The grab to hold after `now`, when the game `want`s the pointer:
/// `apply` asks the window for a mode (`Window::set_cursor_grab`). Locked,
/// else confined, else refused; freed when not wanted.
pub fn next_grab(now: Grab, want: bool, mut apply: impl FnMut(CursorGrabMode) -> bool) -> Grab {
    match (now, want) {
        (Grab::Free, true) => {
            if apply(CursorGrabMode::Locked) {
                Grab::Locked
            } else if apply(CursorGrabMode::Confined) {
                Grab::Confined
            } else {
                Grab::Refused
            }
        }
        (Grab::Locked | Grab::Confined, false) => {
            apply(CursorGrabMode::None);
            Grab::Free
        }
        (Grab::Refused, false) => Grab::Free,
        (g, _) => g,
    }
}

/// The pointer's state across frames.
#[derive(Debug)]
pub struct Pointer {
    grab: Grab,
    /// The cursor, in the window's physical pixels: the virtual one while
    /// locked, else the system's, clamped.
    pos: Option<[f32; 2]>,
    /// The window's cursor as last reported (unclamped).
    system: Option<[f32; 2]>,
    /// The drawn view, `[x, y, w, h]` in physical pixels.
    view: [f32; 4],
    /// The local worm as last drawn (physical pixels) and physical pixels
    /// per game pixel.
    worm: Option<([f32; 2], f32)>,
    focused: bool,
    /// A refused grab was logged.
    warned: bool,
}

impl Default for Pointer {
    fn default() -> Self {
        Pointer {
            grab: Grab::Free,
            pos: None,
            system: None,
            view: [0.0; 4],
            worm: None,
            focused: true,
            warned: false,
        }
    }
}

impl Pointer {
    pub fn grab(&self) -> Grab {
        self.grab
    }

    /// The game has the pointer: mouse events are the controller's.
    pub fn active(&self) -> bool {
        self.grab != Grab::Free
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    /// The window gained or lost the focus. Lost, the grab is freed at
    /// once (the next [`Pointer::update`] grabs again once focused).
    pub fn on_focus(&mut self, focused: bool, window: Option<&Window>) {
        self.focused = focused;
        if !focused {
            self.release(window);
        }
    }

    /// The window's cursor moved to `p` (physical pixels): the cursor,
    /// clamped, unless locked (the virtual one follows the motion).
    pub fn on_cursor_moved(&mut self, p: [f32; 2]) {
        self.system = Some(p);
        if self.grab != Grab::Locked {
            self.pos = Some(self.clamp(p));
        }
    }

    /// The mouse moved by `d` (raw motion, about physical pixels): the
    /// virtual cursor while locked.
    pub fn on_motion(&mut self, d: [f32; 2]) {
        if self.grab != Grab::Locked || !d[0].is_finite() || !d[1].is_finite() {
            return;
        }
        let at = self.pos.unwrap_or_else(|| self.centre());
        self.pos = Some(self.clamp([at[0] + d[0], at[1] + d[1]]));
    }

    /// This frame's drawn view (`[x, y, w, h]`, physical pixels) and the
    /// local worm in it (`None`: no local worm alive), with the physical
    /// pixels per game pixel.
    pub fn set_view(&mut self, view: [f32; 4], worm: Option<([f32; 2], f32)>) {
        self.view = view;
        self.worm = worm;
        if let Some(p) = self.pos {
            self.pos = Some(self.clamp(p));
        }
    }

    /// Grabs or frees the pointer as the game `want`s it ([`wanted`]),
    /// falling back from locked to confined to none (logged once).
    pub fn update(&mut self, window: &Window, want: bool) {
        let before = self.grab;
        self.grab = next_grab(before, want, |mode| window.set_cursor_grab(mode).is_ok());
        if before == Grab::Free && self.grab != Grab::Free {
            // The cursor starts where the system's is, else a little ahead
            // of the worm.
            self.pos = Some(match (self.system, self.worm) {
                (Some(p), _) => self.clamp(p),
                (None, Some((w, k))) => self.clamp([w[0] + 40.0 * k, w[1]]),
                (None, None) => self.centre(),
            });
            log::info!("mouse controls: pointer {:?}", self.grab);
        }
        if self.grab == Grab::Refused && !self.warned {
            self.warned = true;
            log::warn!(
                "mouse controls: the pointer cannot be grabbed here; the free cursor is clamped to the view"
            );
        }
        if before != Grab::Free && self.grab == Grab::Free {
            // Back where the system cursor was: the menus' pointer.
            self.pos = self.system.map(|p| self.clamp(p));
        }
    }

    /// The window was made again (`App::recreate`): the old one's grab
    /// went with it; the next [`Pointer::update`] grabs the new one.
    pub fn window_replaced(&mut self) {
        self.grab = Grab::Free;
    }

    fn release(&mut self, window: Option<&Window>) {
        if matches!(self.grab, Grab::Locked | Grab::Confined)
            && let Some(w) = window
        {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
        }
        self.grab = Grab::Free;
    }

    /// The cursor's direction from the drawn local worm, in game pixels
    /// (y down): what the controller aims at. `None` unless the game has
    /// the pointer and a local worm is drawn.
    pub fn aim(&self) -> Option<[f32; 2]> {
        if !self.active() {
            return None;
        }
        let (p, (w, k)) = (self.pos?, self.worm?);
        let k = if k.is_finite() && k > 0.0 { k } else { 1.0 };
        Some([(p[0] - w[0]) / k, (p[1] - w[1]) / k])
    }

    /// Where the target marker goes (physical pixels), while the game has
    /// the pointer.
    pub fn marker(&self) -> Option<[f32; 2]> {
        self.pos.filter(|_| self.active())
    }

    fn centre(&self) -> [f32; 2] {
        let [x, y, w, h] = self.view;
        [x + w / 2.0, y + h / 2.0]
    }

    fn clamp(&self, p: [f32; 2]) -> [f32; 2] {
        let [x, y, w, h] = self.view;
        if w <= 0.0 || h <= 0.0 {
            return p;
        }
        [p[0].clamp(x, x + w - 1.0), p[1].clamp(y, y + h - 1.0)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grab_falls_back_and_frees() {
        let all = |_| true;
        assert_eq!(next_grab(Grab::Free, true, all), Grab::Locked);
        let no_lock = |m| m != CursorGrabMode::Locked;
        assert_eq!(next_grab(Grab::Free, true, no_lock), Grab::Confined);
        assert_eq!(next_grab(Grab::Free, true, |m| m == CursorGrabMode::None), Grab::Refused);
        let mut asked = vec![];
        assert_eq!(
            next_grab(Grab::Locked, false, |m| {
                asked.push(m);
                true
            }),
            Grab::Free
        );
        assert_eq!(asked, [CursorGrabMode::None]);
        assert_eq!(next_grab(Grab::Refused, false, |_| panic!("nothing to free")), Grab::Free);
        assert_eq!(next_grab(Grab::Confined, true, |_| panic!("held already")), Grab::Confined);
    }

    #[test]
    fn the_pointer_is_wanted_only_in_play() {
        assert!(wanted(true, true, false, true));
        assert!(!wanted(false, true, false, true), "mouse controls off");
        assert!(!wanted(true, false, false, true), "a menu, the pause, chat");
        assert!(!wanted(true, true, true, true), "the weapon selection");
        assert!(!wanted(true, true, false, false), "focus lost");
    }

    #[test]
    fn the_virtual_cursor_is_clamped_to_the_view_and_aims_from_the_worm() {
        let mut p = Pointer::default();
        p.set_view([100.0, 50.0, 640.0, 360.0], Some(([420.0, 230.0], 2.0)));
        assert_eq!(p.aim(), None, "free: no aim");
        p.grab = Grab::Locked;
        p.pos = Some([420.0, 230.0]);
        p.on_motion([60.0, -20.0]);
        assert_eq!(p.aim(), Some([30.0, -10.0]));
        // Far past the view's edge (over the HUD's bar): clamped.
        p.on_motion([0.0, 10_000.0]);
        assert_eq!(p.marker(), Some([480.0, 409.0]));
        // The system cursor does not move a locked one.
        p.on_cursor_moved([0.0, 0.0]);
        assert_eq!(p.marker(), Some([480.0, 409.0]));
        // Confined: the system cursor, clamped.
        p.grab = Grab::Confined;
        p.on_cursor_moved([5.0, 5.0]);
        assert_eq!(p.marker(), Some([100.0, 50.0]));
        // Focus lost: freed.
        p.on_focus(false, None);
        assert!(!p.active() && p.aim().is_none() && p.marker().is_none());
        // No worm drawn: no aim.
        p.grab = Grab::Locked;
        p.set_view([100.0, 50.0, 640.0, 360.0], None);
        assert_eq!(p.aim(), None);
    }
}
