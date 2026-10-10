//! The window surface's state machine (Review Focus 4): resizes, a
//! minimised window, `Outdated` (the first frame under Wayland) and a lost
//! surface never crash nor leave the surface misconfigured. It is generic
//! over [`Surface`] so a fake can test it; `gpu.rs` implements it on wgpu.

/// What acquiring the next frame gave.
#[derive(Debug, PartialEq, Eq)]
pub enum Acquire<F> {
    Frame(F),
    /// A frame, but the surface should be configured again.
    Suboptimal(F),
    /// The configuration no longer matches the window: configure, retry.
    Outdated,
    /// The surface is gone: create it again, configure, retry.
    Lost,
    /// Skip this frame (a timeout, an occluded window, a validation error).
    Skip,
}

/// A presentable surface.
pub trait Surface {
    type Frame;
    /// Configures the surface for `size` pixels (never 0).
    fn configure(&mut self, size: (u32, u32));
    fn acquire(&mut self) -> Acquire<Self::Frame>;
    /// Creates the surface again after [`Acquire::Lost`].
    fn recreate(&mut self);
}

/// How many times one frame retries after `Outdated` or `Lost`.
pub const RETRIES: u32 = 2;

/// The window's size and what the surface was last configured for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SurfaceState {
    size: (u32, u32),
    occluded: bool,
    configured: Option<(u32, u32)>,
}

impl SurfaceState {
    /// A window of `size` pixels; the surface is configured on the first frame.
    pub fn new(size: (u32, u32)) -> SurfaceState {
        SurfaceState { size, occluded: false, configured: None }
    }

    /// The window is now `size` pixels (0 when minimised).
    pub fn resize(&mut self, size: (u32, u32)) {
        self.size = size;
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The window is hidden (minimised, covered) or shown again.
    pub fn set_occluded(&mut self, occluded: bool) {
        self.occluded = occluded;
    }

    /// Whether frames can be drawn: the window has an area and is not
    /// occluded. While it cannot, the game stops drawing and waits.
    pub fn drawable(&self) -> bool {
        self.size.0 > 0 && self.size.1 > 0 && !self.occluded
    }

    /// The next frame to draw into, if there is one: none while the window
    /// has no area (minimised), or when the surface keeps failing (the next
    /// frame tries again). Configures the surface first when the window
    /// changed size; reconfigures and retries after `Outdated`, recreates
    /// after `Lost`; a suboptimal frame is used and the surface configured
    /// again next time.
    pub fn frame<S: Surface>(&mut self, surface: &mut S) -> Option<S::Frame> {
        if !self.drawable() {
            return None;
        }
        for _ in 0..=RETRIES {
            if self.configured != Some(self.size) {
                surface.configure(self.size);
                self.configured = Some(self.size);
            }
            match surface.acquire() {
                Acquire::Frame(f) => return Some(f),
                Acquire::Suboptimal(f) => {
                    self.configured = None;
                    return Some(f);
                }
                Acquire::Outdated => self.configured = None,
                Acquire::Lost => {
                    surface.recreate();
                    self.configured = None;
                }
                Acquire::Skip => return None,
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A surface that answers from a script, then with frames; it records
    /// its configurations and checks it is never acquired unconfigured.
    #[derive(Default)]
    struct Fake {
        answers: VecDeque<Acquire<u32>>,
        configured: Vec<(u32, u32)>,
        current: Option<(u32, u32)>,
        recreated: u32,
        frames: u32,
    }

    impl Surface for Fake {
        type Frame = u32;
        fn configure(&mut self, size: (u32, u32)) {
            assert!(size.0 > 0 && size.1 > 0, "configured with no area");
            self.configured.push(size);
            self.current = Some(size);
        }
        fn acquire(&mut self) -> Acquire<u32> {
            assert!(self.current.is_some(), "acquired unconfigured");
            self.answers.pop_front().unwrap_or_else(|| {
                self.frames += 1;
                Acquire::Frame(self.frames)
            })
        }
        fn recreate(&mut self) {
            self.recreated += 1;
            self.current = None;
        }
    }

    #[test]
    fn surface_survives_resize_and_outdated() {
        let mut s = SurfaceState::new((800, 600));
        // Wayland's first frame is Outdated: configure again and retry.
        let mut f = Fake { answers: [Acquire::Outdated].into(), ..Fake::default() };
        assert_eq!(s.frame(&mut f), Some(1));
        assert_eq!(f.configured, [(800, 600), (800, 600)]);
        // Steady frames do not configure again.
        assert_eq!(s.frame(&mut f), Some(2));
        assert_eq!(f.configured.len(), 2);
        // A resize configures the new size once.
        s.resize((1920, 1080));
        assert_eq!(s.frame(&mut f), Some(3));
        assert_eq!(s.frame(&mut f), Some(4));
        assert_eq!(f.configured.last(), Some(&(1920, 1080)));
        assert_eq!(f.configured.len(), 3);
        // Minimised: no frame and no zero-sized configuration; restored:
        // configured for the new size.
        s.resize((0, 0));
        assert_eq!(s.frame(&mut f), None);
        s.resize((1280, 0));
        assert_eq!(s.frame(&mut f), None);
        s.resize((1280, 720));
        assert_eq!(s.frame(&mut f), Some(5));
        assert_eq!(f.configured.last(), Some(&(1280, 720)));
        // Lost: recreated, configured and retried within the frame.
        f.answers = [Acquire::Lost].into();
        assert_eq!(s.frame(&mut f), Some(6));
        assert_eq!(f.recreated, 1);
        // Suboptimal: drawn, and configured again before the next frame.
        f.answers = [Acquire::Suboptimal(99)].into();
        let n = f.configured.len();
        assert_eq!(s.frame(&mut f), Some(99));
        assert_eq!(s.frame(&mut f), Some(7));
        assert_eq!(f.configured.len(), n + 1);
        // Outdated forever: the frame gives up (no endless loop) and the
        // next one tries again.
        f.answers = std::iter::repeat_with(|| Acquire::Outdated).take(RETRIES as usize + 1).collect();
        assert_eq!(s.frame(&mut f), None);
        assert_eq!(s.frame(&mut f), Some(8));
        // Occluded (winit's `Occluded`): no frame and no acquire, until shown.
        s.set_occluded(true);
        assert!(!s.drawable());
        f.answers = [Acquire::Skip].into();
        assert_eq!(s.frame(&mut f), None);
        assert_eq!(f.answers.len(), 1, "not acquired while occluded");
        s.set_occluded(false);
        assert!(s.drawable());
        s.resize((0, 720));
        assert!(!s.drawable(), "minimised");
        s.resize((1280, 720));
        // A timeout or an occluded window skips the frame.
        f.answers = [Acquire::Skip].into();
        assert_eq!(s.frame(&mut f), None);
        assert_eq!(s.frame(&mut f), Some(9));
    }
}
