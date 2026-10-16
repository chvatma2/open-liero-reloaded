//! The game window: its attributes, the window system it opens on, and
//! the size changes the game asks for.
//!
//! The player cannot drag-resize or maximise the window (the user's ruling
//! after the plan 6 playtest): under WSLg, dragging the window's edge
//! crashed the compositor (Weston), which ended the game with a broken
//! pipe. The size comes from the command line and changes only when the
//! game asks (the `size` and `fullscreen` script commands, the options
//! menu: `video.rs`). Those requests are coalesced in [`Requests`]: the
//! latest wins, applied at the start of a frame before its surface texture
//! is acquired (so the frame is drawn at the new size), and at most once
//! every [`MIN_CHANGE_INTERVAL`], counting the window's creation as one.
//!
//! Why the interval: WSLg's Weston 9 crashes (SIGSEGV) when a window's
//! client-side decorations are resized quickly. winit draws them into one
//! `wl_shm` pool and grows it with `wl_shm_pool.resize` while the
//! compositor still uses buffers from it; every crash we traced ended right
//! after such a resize. Resizing every 50 ms crashed it in 8 of 8 runs,
//! every 250 ms or slower in none of 33 (hotfix report, plan 6). Under X11
//! (XWayland) the compositor draws the frame and nothing crashed.
//!
//! Why Wayland makes the window again ([`Plan::Recreate`], v0.1 playtest):
//! under WSLg (Weston's RDP-RAIL shell) a live window that changed size or
//! mode left pieces of its old frame on the Windows desktop: a line at the
//! old right border after 1280 × 720 → 1920 × 1080, and after a window
//! of the desktop's size → fullscreen the old title bar and 1280 × 720 outline over the
//! game, the window at -22,-22 and the taskbar still shown. winit and
//! sctk-adwaita did it right (a `WAYLAND_DEBUG` trace: `set_fullscreen`,
//! `configure(3440,1440)`, the window geometry set and the 5 frame
//! subsurfaces unmapped, all put back afterwards), so the stale pieces are
//! the compositor's or RDP's, left by moved or unmapped subsurfaces; WSLg's
//! Weston offers no `zxdg_decoration_manager_v1`, so no server-side frame.
//! A window made at its size and mode never showed any of it (`--fullscreen`
//! at 0,0 3440 × 1440, the taskbar hidden), so on Wayland each change
//! closes the window and opens a new one at the new [`Shape`]; the
//! device, the game, the sound and the menus stay (`app.rs`: `recreate`,
//! `gpu.rs`: `Gpu::rebind`). It also never grows a live frame's `wl_shm`
//! pool. The pacing stays: it costs nothing, and it bounds how often a
//! window is made. X11 and other platforms change the window in place
//! ([`apply`]): their frame is the window manager's.
//!
//! The compositor places the new window as it likes: WSLg puts it near the
//! middle of the monitor (1920 × 1080 at 1125,324 and 1280 × 720 at
//! 1021,485 of 3440 × 1440), fullscreen at exactly 0,0 with the taskbar hidden. The old window
//! closes before the new one shows its first frame, so the change may
//! blink (making the window takes ~15 ms), and the new window
//! draws its second frame only after the compositor's first frame callback:
//! 0.15 s at most on WSLg, but 0.3 to 25 s while the Windows session was
//! locked, during which the game does not tick (the time is dropped as
//! any stall's: `MAX_CATCHUP_TICKS`). The options offer no window of the
//! desktop's size (`olr_ui::WindowPreset`): with its title bar it was taller
//! than the work area, its bottom under the taskbar, and winit cannot tell
//! the work area. Fullscreen fills the screen.

use std::time::{Duration, Instant};

use winit::dpi::PhysicalSize;
use winit::window::{Fullscreen, Window, WindowAttributes, WindowButtons};

/// The window's size before the player chooses one (logical pixels).
pub const DEFAULT_SIZE: (u32, u32) = (1280, 720);

/// The least time between two applied size or mode changes.
pub const MIN_CHANGE_INTERVAL: Duration = Duration::from_millis(250);

/// The window as created: `size` logical pixels (`physical`: physical
/// pixels, an options preset), borderless fullscreen when asked, neither
/// resizable nor maximisable (the size is the game's).
pub fn attributes(size: (u32, u32), physical: bool, fullscreen: bool) -> WindowAttributes {
    let (w, h) = (size.0.max(1), size.1.max(1));
    let size: winit::dpi::Size =
        if physical { PhysicalSize::new(w, h).into() } else { winit::dpi::LogicalSize::new(w, h).into() };
    Window::default_attributes()
        .with_title("rust-liero")
        .with_inner_size(size)
        .with_resizable(false)
        .with_enabled_buttons(WindowButtons::CLOSE | WindowButtons::MINIMIZE)
        .with_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)))
}

/// A size or mode change for the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Change {
    /// The inner size in physical pixels (at least 1 × 1).
    pub size: Option<(u32, u32)>,
    /// Borderless fullscreen on or off.
    pub fullscreen: Option<bool>,
}

/// What the window is, or is to be: the arguments of [`attributes`], so a
/// window can be made again as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// The windowed inner size; kept while fullscreen, for when it ends.
    pub size: (u32, u32),
    /// `size` is in physical pixels (an options preset, a change), else
    /// logical (the command line's default).
    pub physical: bool,
    /// Borderless fullscreen.
    pub fullscreen: bool,
}

impl Shape {
    /// The window's attributes for this shape ([`attributes`]).
    pub fn attributes(&self) -> WindowAttributes {
        attributes(self.size, self.physical, self.fullscreen)
    }

    /// Applies `change`; returns whether what the window shows changed: the
    /// mode, or the size of a window (a fullscreen window only keeps a new
    /// size for later).
    pub fn apply(&mut self, change: Change) -> bool {
        let before = *self;
        if let Some(on) = change.fullscreen {
            self.fullscreen = on;
        }
        if let Some((w, h)) = change.size {
            self.size = (w.max(1), h.max(1));
            self.physical = true;
        }
        self.fullscreen != before.fullscreen || (!self.fullscreen && *self != before)
    }
}

/// How a size or mode change reaches the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Changed on the live window (`set_fullscreen`, `request_inner_size`:
    /// [`apply`]).
    InPlace,
    /// The window made again at its new shape; the device, the game and the
    /// menus stay (module docs: WSLg leaves a moved window's old frame on
    /// the screen).
    Recreate,
}

/// How changes reach the window on this window system: made again on
/// Wayland (its client-side frame is what WSLg leaves behind), in place
/// elsewhere (X11's frame is the window manager's: nothing is left).
pub fn plan(wayland: bool) -> Plan {
    if wayland { Plan::Recreate } else { Plan::InPlace }
}

/// The changes the game asked for and not yet applied; the latest of each
/// kind wins.
#[derive(Clone, Copy, Debug, Default)]
pub struct Requests {
    pending: Change,
    last: Option<Instant>,
}

impl Requests {
    /// The window was created at `now`: the first change waits out the
    /// interval from then, like any change after another.
    pub fn opened(&mut self, now: Instant) {
        self.last = Some(now);
    }

    /// Asks for a window of `w` × `h` physical pixels.
    pub fn size(&mut self, w: u32, h: u32) {
        self.pending.size = Some((w.max(1), h.max(1)));
    }

    /// Asks for borderless fullscreen (`true`) or a window.
    pub fn fullscreen(&mut self, on: bool) {
        self.pending.fullscreen = Some(on);
    }

    /// The change to apply at `now`, if one is pending and the last was
    /// applied at least [`MIN_CHANGE_INTERVAL`] ago; it is then no longer
    /// pending. A change that must wait stays pending, merged with later
    /// requests.
    pub fn due(&mut self, now: Instant) -> Option<Change> {
        if self.pending == Change::default() {
            return None;
        }
        if self.last.is_some_and(|last| now.saturating_duration_since(last) < MIN_CHANGE_INTERVAL) {
            return None;
        }
        self.last = Some(now);
        Some(std::mem::take(&mut self.pending))
    }
}

/// Applies `change` to `window`: the mode first, then the size. Returns
/// the new inner size when the window system applied it at once (Wayland);
/// otherwise a `Resized` event brings it (X11).
///
/// The window is not resizable, so a new size also moves its minimum and
/// maximum to it (winit's Wayland backend keeps a fixed window's old
/// limits, and the compositor would hold the window to them).
pub fn apply(window: &Window, change: Change) -> Option<PhysicalSize<u32>> {
    if let Some(on) = change.fullscreen {
        window.set_fullscreen(on.then_some(Fullscreen::Borderless(None)));
    }
    let (w, h) = change.size?;
    let size = PhysicalSize::new(w, h);
    window.set_max_inner_size(Some(size));
    window.set_min_inner_size(Some(size));
    window.request_inner_size(size)
}

/// The window system the game opens its window on (Linux).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum WindowSystem {
    /// winit's choice: Wayland when `WAYLAND_DISPLAY` is set, else X11.
    Auto,
    Wayland,
    X11,
}

/// The variable that picks the window system when the command line does not.
pub const WINDOW_SYSTEM_VAR: &str = "OLR_WINDOW_SYSTEM";

/// The window system: the command line's (`cli`), else [`WINDOW_SYSTEM_VAR`]'s
/// (`var`: `auto`, `wayland` or `x11`, any case), else [`WindowSystem::Auto`].
/// Wayland stays the default on WSL too: it paces frames at the display's
/// rate, where WSLg's X11 path draws uncapped, and the window can no longer
/// be resized quickly (module docs).
pub fn window_system(cli: Option<WindowSystem>, var: Option<&str>) -> Result<WindowSystem, String> {
    if let Some(ws) = cli {
        return Ok(ws);
    }
    let Some(v) = var.map(str::trim).filter(|v| !v.is_empty()) else { return Ok(WindowSystem::Auto) };
    match v.to_ascii_lowercase().as_str() {
        "auto" => Ok(WindowSystem::Auto),
        "wayland" => Ok(WindowSystem::Wayland),
        "x11" => Ok(WindowSystem::X11),
        _ => Err(format!("{WINDOW_SYSTEM_VAR}={v:?} is not auto, wayland or x11")),
    }
}

/// What to tell the player when winit ended its loop with an exit failure
/// the game did not ask for. That is almost always the display server
/// closing the connection, but winit does not say why, so the text says
/// "probably".
pub fn lost_connection(wayland: bool) -> String {
    if wayland {
        format!(
            "the window system stopped the game, probably by closing the connection to the \
             display server (e.g. the Wayland compositor crashed: under WSLg, Weston can crash \
             when a window is resized quickly); try --window-system x11 or \
             {WINDOW_SYSTEM_VAR}=x11"
        )
    } else {
        format!(
            "the window system stopped the game, probably by closing the connection to the \
             display server (e.g. the X server exited); try --window-system wayland or \
             {WINDOW_SYSTEM_VAR}=wayland"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_fixed_size_and_cannot_be_maximised() {
        let a = attributes(DEFAULT_SIZE, false, false);
        assert!(!a.resizable);
        assert!(!a.enabled_buttons.contains(WindowButtons::MAXIMIZE));
        assert!(a.enabled_buttons.contains(WindowButtons::CLOSE | WindowButtons::MINIMIZE));
        assert_eq!(a.fullscreen, None);
        assert_eq!(a.inner_size, Some(winit::dpi::LogicalSize::new(1280u32, 720).into()));
        let a = attributes((800, 0), false, true);
        assert!(!a.resizable);
        assert_eq!(a.fullscreen, Some(Fullscreen::Borderless(None)));
        assert_eq!(a.inner_size, Some(winit::dpi::LogicalSize::new(800u32, 1).into()));
        // An options preset is physical pixels.
        let a = attributes((1600, 900), true, false);
        assert_eq!(a.inner_size, Some(PhysicalSize::new(1600u32, 900).into()));
    }

    #[test]
    fn a_shape_says_when_the_window_shows_a_change() {
        let mut s = Shape { size: DEFAULT_SIZE, physical: false, fullscreen: false };
        assert_eq!(s.attributes().inner_size, attributes(DEFAULT_SIZE, false, false).inner_size);
        // The same size, now physical: a new window (the scale may differ).
        assert!(s.apply(Change { size: Some((1280, 720)), fullscreen: None }));
        assert_eq!(s, Shape { size: (1280, 720), physical: true, fullscreen: false });
        // Nothing new: nothing to do.
        assert!(!s.apply(Change::default()));
        assert!(!s.apply(Change { size: Some((1280, 720)), fullscreen: Some(false) }));
        // Fullscreen keeps the windowed size; a size meanwhile is kept for later.
        assert!(s.apply(Change { size: None, fullscreen: Some(true) }));
        assert!(!s.apply(Change { size: Some((1920, 1080)), fullscreen: None }));
        assert!(!s.apply(Change { size: None, fullscreen: Some(true) }));
        assert_eq!(s, Shape { size: (1920, 1080), physical: true, fullscreen: true });
        let a = s.attributes();
        assert_eq!(a.fullscreen, Some(Fullscreen::Borderless(None)));
        assert!(!a.resizable && !a.enabled_buttons.contains(WindowButtons::MAXIMIZE));
        // Out of fullscreen: the kept size.
        assert!(s.apply(Change { size: None, fullscreen: Some(false) }));
        assert_eq!(s.attributes().inner_size, Some(PhysicalSize::new(1920u32, 1080).into()));
        assert!(s.apply(Change { size: Some((0, 600)), fullscreen: None }));
        assert_eq!(s.size, (1, 600));
    }

    #[test]
    fn wayland_makes_the_window_again() {
        assert_eq!(plan(true), Plan::Recreate);
        assert_eq!(plan(false), Plan::InPlace);
    }

    #[test]
    fn requests_coalesce_to_the_latest_once() {
        let t0 = Instant::now();
        let mut r = Requests::default();
        assert_eq!(r.due(t0), None);
        // Many resizes in one frame (a script, a burst of menu clicks)
        // become one, at the latest size.
        r.size(640, 480);
        r.size(1920, 1080);
        r.fullscreen(true);
        r.size(0, 600);
        r.fullscreen(false);
        assert_eq!(r.due(t0), Some(Change { size: Some((1, 600)), fullscreen: Some(false) }));
        assert_eq!(r.due(t0), None, "applied once");
    }

    #[test]
    fn changes_are_at_least_the_interval_apart() {
        let t0 = Instant::now();
        let mut r = Requests::default();
        r.size(800, 600);
        assert!(r.due(t0).is_some());
        // Asked again within the interval: it waits, merged with what
        // comes after, and applies once the interval has passed.
        r.size(1024, 768);
        assert_eq!(r.due(t0 + Duration::from_millis(16)), None);
        r.fullscreen(true);
        r.size(1280, 720);
        let early = t0 + MIN_CHANGE_INTERVAL - Duration::from_millis(1);
        assert_eq!(r.due(early), None);
        let later = t0 + MIN_CHANGE_INTERVAL;
        assert_eq!(r.due(later), Some(Change { size: Some((1280, 720)), fullscreen: Some(true) }));
        // The interval runs from the last applied change.
        r.size(640, 480);
        assert_eq!(r.due(later + Duration::from_millis(100)), None);
        assert!(r.due(later + MIN_CHANGE_INTERVAL).is_some());
        // Nothing pending: nothing due, and the clock is not reset.
        assert_eq!(r.due(later + 10 * MIN_CHANGE_INTERVAL), None);
    }

    #[test]
    fn creating_the_window_starts_the_interval() {
        // A script's `size` at tick 0 must not resize the window right
        // after it was created (two changes in quick succession).
        let t0 = Instant::now();
        let mut r = Requests::default();
        r.opened(t0);
        r.size(800, 600);
        assert_eq!(r.due(t0 + Duration::from_millis(10)), None);
        let due = r.due(t0 + MIN_CHANGE_INTERVAL);
        assert_eq!(due, Some(Change { size: Some((800, 600)), fullscreen: None }));
    }

    #[test]
    fn the_window_system_is_the_command_lines_then_the_variables() {
        use WindowSystem::*;
        assert_eq!(window_system(None, None), Ok(Auto));
        assert_eq!(window_system(None, Some("")), Ok(Auto));
        assert_eq!(window_system(None, Some("X11")), Ok(X11));
        assert_eq!(window_system(None, Some(" wayland\n")), Ok(Wayland));
        assert_eq!(window_system(None, Some("auto")), Ok(Auto));
        assert_eq!(window_system(Some(Wayland), Some("x11")), Ok(Wayland));
        assert_eq!(window_system(Some(Auto), Some("bogus")), Ok(Auto));
        let e = window_system(None, Some("xorg")).unwrap_err();
        assert!(e.contains(WINDOW_SYSTEM_VAR) && e.contains("xorg"), "{e}");
    }

    #[test]
    fn a_lost_connection_names_the_way_out() {
        let w = lost_connection(true);
        assert!(w.starts_with("the window system stopped the game, probably"), "{w}");
        assert!(w.contains("--window-system x11") && w.contains("OLR_WINDOW_SYSTEM=x11"), "{w}");
        assert!(lost_connection(false).contains("--window-system wayland"));
    }
}
