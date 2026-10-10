//! OLX's camera for a local player (`CViewport` in `VW_FOLLOW` mode): a
//! hard lock on the followed worm, clamped to the map, with screen shake.

use rl_math::{AbsTime, GlibcRand, Vec2};
use rl_sim::{EventKind, TickEvents};

use crate::draw_list::View;

/// The field of view's height in world pixels (spec §19 E4: OLX's
/// 480-screen-pixel-high viewport, the fork's layout).
pub const FOV_HEIGHT: i32 = 240;

/// How long a shake lasts after the last explosion that started it, in ms
/// (`CViewport::Process`: `currentTime - fShakestart > 0.2f`).
pub const SHAKE_MS: u64 = 200;

/// A viewport's size in world pixels (`CViewport::Width`, `Height`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub width: i32,
    pub height: i32,
}

impl Viewport {
    /// The single-player viewport for a window of aspect ratio `aspect`
    /// (width / height), clamped to `max_aspect` (spec §19 E4, 16:9 by
    /// default): OLX's screen width `round_even(480 · aspect)`, at least
    /// 320, halved into world pixels, by [`FOV_HEIGHT`].
    ///
    /// OLX: src/client/AuxLib.cpp:450-469 (`displayScreenWidth`),
    /// src/client/CClient.cpp:1685-1744 and src/client/CViewport.cpp:63-85
    pub fn from_aspect(aspect: f32, max_aspect: f32) -> Viewport {
        let mut a = aspect as f64;
        if !(a.is_finite() && a > 0.0) {
            a = 4.0 / 3.0;
        }
        if max_aspect.is_finite() && max_aspect > 0.0 {
            a = a.min(max_aspect as f64);
        }
        let w = 2.0 * FOV_HEIGHT as f64 * a;
        let rounded = (((w + 0.5) as i32).saturating_add(1) & !1).max(320);
        Viewport { width: rounded / 2, height: FOV_HEIGHT }
    }
}

/// A local player's camera. Each drawn frame, [`Camera::process`] puts the
/// followed worm in the middle (even when it is dead, so the view stays
/// where it died and jumps when it respawns), shakes, and clamps to the map.
/// The shake uses the camera's own generator, never the simulation's.
#[derive(Clone, Debug)]
pub struct Camera {
    viewport: Viewport,
    target: i32,
    x: f32,
    y: f32,
    shaking: bool,
    shake_start: AbsTime,
    shake_amount: i32,
    rng: GlibcRand,
}

impl Camera {
    /// A camera of `viewport` size following worm id `target`; `seed`
    /// seeds its shake.
    pub fn new(viewport: Viewport, target: i32, seed: u32) -> Camera {
        Camera {
            viewport,
            target,
            x: 0.0,
            y: 0.0,
            shaking: false,
            shake_start: AbsTime(0),
            shake_amount: 0,
            rng: GlibcRand::new(seed),
        }
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// The id of the worm followed.
    pub fn target(&self) -> i32 {
        self.target
    }

    pub fn set_target(&mut self, worm: i32) {
        self.target = worm;
    }

    /// What the camera shows after the last [`Camera::process`].
    pub fn view(&self) -> View {
        View { x: self.x, y: self.y, w: self.viewport.width as f32, h: self.viewport.height as f32 }
    }

    /// The sound listener: the view's centre (`m_listener->pos`).
    ///
    /// OLX: src/client/CViewport.cpp:365
    pub fn listener(&self) -> Vec2 {
        Vec2::new(self.x + (self.viewport.width / 2) as f32, self.y + (self.viewport.height / 2) as f32)
    }

    /// Whether the shake is running and how strong it is.
    pub fn shake_amount(&self) -> i32 {
        if self.shaking { self.shake_amount } else { 0 }
    }

    /// `CViewport::inView`: `pos`, truncated, is in the view (edges included).
    ///
    /// OLX: src/client/CViewport.cpp:466-472
    pub fn in_view(&self, pos: Vec2) -> bool {
        let (x, y) = ((pos.x as i32) as f32, (pos.y as i32) as f32);
        x >= self.x
            && y >= self.y
            && x <= self.x + self.viewport.width as f32
            && y <= self.y + self.viewport.height as f32
    }

    /// `CViewport::Shake`: (re)starts the 0.2 s window at `now`; the amount
    /// only grows.
    ///
    /// OLX: src/client/CViewport.cpp:453-461
    pub fn shake(&mut self, amount: i32, now: AbsTime) {
        self.shake_start = now;
        self.shaking = true;
        self.shake_amount = self.shake_amount.max(amount);
    }

    /// Ends any shake (for a new or restarted world).
    pub fn stop_shake(&mut self) {
        self.shaking = false;
        self.shake_amount = 0;
    }

    /// The viewport part of `CClient::Explosion`: every explosion with a
    /// nonzero shake inside the view (as the last frame showed it) shakes.
    /// `now` is the world's time after the tick.
    ///
    /// OLX: src/client/CClient_Game.cpp:307-317
    pub fn on_events(&mut self, events: &TickEvents, now: AbsTime) {
        for e in events {
            if let EventKind::Explosion { pos, shake, .. } = e.kind
                && shake != 0
                && self.in_view(pos)
            {
                self.shake(shake, now);
            }
        }
    }

    /// `CViewport::Process` for `VW_FOLLOW`, once per drawn frame: `follow`
    /// is the followed worm's drawn position (`None` keeps the view), `map`
    /// the map's size in world pixels and `now` the world's time. `exact`
    /// is OLX's integer camera; otherwise the view keeps sub-pixel
    /// positions (interpolated drawing, spec §8.2), shaking by whole pixels.
    ///
    /// OLX: src/client/CViewport.cpp:118-170 (the hard lock), :336-362
    pub fn process(&mut self, follow: Option<Vec2>, exact: bool, map: (u32, u32), now: AbsTime) {
        let (hx, hy) = ((self.viewport.width / 2) as f32, (self.viewport.height / 2) as f32);
        if let Some(p) = follow {
            (self.x, self.y) = (p.x - hx, p.y - hy);
            if exact {
                (self.x, self.y) = ((self.x as i32) as f32, (self.y as i32) as f32);
            }
        }
        if self.shaking {
            // A clock behind the start (a restarted world) ends it too.
            if now < self.shake_start || now.0 - self.shake_start.0 > SHAKE_MS {
                self.stop_shake();
            } else {
                // Clamp first, so a camera at an edge shakes inward only.
                self.clamp(map);
                self.x += (self.rng.random_num() * self.shake_amount as f32) as i32 as f32;
                self.y += (self.rng.random_num() * self.shake_amount as f32) as i32 as f32;
            }
        }
        self.clamp(map);
    }

    /// `CViewport::Clamp`: keep the view inside the map; a map smaller than
    /// the view sits at the top-left.
    ///
    /// OLX: src/client/CViewport.cpp:417-431
    fn clamp(&mut self, map: (u32, u32)) {
        let clamp = |v: f32, map: u32, size: i32| {
            let max = map as f32 - size as f32;
            if max >= 0.0 && !v.is_nan() { v.clamp(0.0, max) } else { 0.0 }
        };
        self.x = clamp(self.x, map.0, self.viewport.width);
        self.y = clamp(self.y, map.1, self.viewport.height);
    }
}
