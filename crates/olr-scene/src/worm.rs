//! Worms as OLX draws them (`CWorm::Draw`): rope, health bar, crosshair,
//! skin frame and muzzle flash. Offsets are OLX's screen pixels (half world
//! pixels) relative to the worm's drawn position.

use olr_data::image::Rgba;
use olr_math::Vec2;
use olr_sim::{Dir, World, Worm};

use crate::assets::{SceneAssets, WormLook};
use crate::draw_list::{AtlasId, DrawList, Layer, Rect, Shade, Sprite, View, perform_line};

/// OLX's skin layout (`include/CGameSkin.h:36-41`), as `olr-content` has it.
pub use olr_content::{FRAME_WIDTH, SKIN_HEIGHT, SKIN_WIDTH};

/// `Options.CrosshairDistance`'s default, in screen pixels.
pub const CROSSHAIR_DISTANCE: f32 = 32.0;

const RIGHT_MUZZLE: [i32; 14] = [2, 3, 5, 3, 4, 0, 5, -8, 3, -9, 2, -13, -2, -12];
const LEFT_MUZZLE: [i32; 14] = [4, -12, -1, -12, -1, -9, -3, -8, -2, 0, -2, 4, 1, 3];

/// The skin frame for a worm aiming at `angle` (degrees, −90 up) with walk
/// frame `walk`, on a sheet of `frame_count` frames: the source rectangle,
/// and whether it is drawn mirrored (`olr_content::skin_source`, which the
/// profiles' preview draws from too).
///
/// OLX: src/common/CWorm.cpp:1225-1244, src/client/CGameSkin.cpp:584-601
pub fn skin_frame(angle: f32, face: Dir, walk: f32, frame_count: u32) -> (Rect, bool) {
    let ang = (((angle + 90.0) / 151.0 * 7.0) as i32).min(6);
    let f = (walk as i32).wrapping_mul(7).wrapping_add(ang);
    let f = f.clamp(0, frame_count.max(1) as i32 - 1) as u32;
    let s = olr_content::skin_source(f, face);
    (Rect { x: s.x, y: s.y, w: s.w, h: s.h }, s.mirrored)
}

/// The muzzle flash for `angle` and `face`: its frame in `muzzle.png` and
/// its offset in screen pixels. The angle bucket is clamped to the table.
///
/// OLX: src/common/CWorm.cpp:1272-1299, :896-897
pub fn muzzle_frame(angle: f32, face: Dir) -> (u32, i32, i32) {
    let a = (((angle + 90.0) / 151.0 * 7.0) as i32).clamp(0, 6) as usize;
    match face {
        Dir::Right => {
            let k = 6 - a;
            (k as u32, -12 + RIGHT_MUZZLE[2 * k], -10 + RIGHT_MUZZLE[2 * k + 1])
        }
        Dir::Left => (a as u32 + 7, -21 + LEFT_MUZZLE[2 * a], -10 + LEFT_MUZZLE[2 * a + 1]),
    }
}

/// The crosshair's offset from the worm in screen pixels: `(int)` of
/// `CrosshairDistance` along the whole-degree aim, mirrored when facing left.
///
/// OLX: src/common/CWorm.cpp:1050-1052, :1184-1194; src/common/MathLib.cpp:135-149
pub fn crosshair_offset(angle: f32, face: Dir) -> (i32, i32) {
    let mut a = angle as i32;
    if face == Dir::Left {
        a = 180i32.wrapping_sub(a);
    }
    let rad = a as f32 * (core::f64::consts::PI / 180.0) as f32;
    let (x, y) = (olr_math::cosf(rad) * CROSSHAIR_DISTANCE, olr_math::sinf(rad) * CROSSHAIR_DISTANCE);
    (x as i32, y as i32)
}

/// The old-style health bar's segment count: `Round((health + 15) / 20)`,
/// at most 5.
///
/// OLX: src/common/CWorm.cpp:1167-1170, src/common/MathLib.cpp:85-88
pub fn health_segments(health: f32) -> i32 {
    ((((health + 15.0) / 20.0) as f64 - 0.5).ceil() as i32).min(5)
}

/// Drawing helpers relative to a worm's drawn position `p` (world pixels).
struct At<'a> {
    out: &'a mut DrawList,
    p: Vec2,
}

impl At<'_> {
    /// A sprite at `(dx, dy)` screen pixels from `p`.
    #[allow(clippy::too_many_arguments)]
    fn blit(
        &mut self,
        atlas: AtlasId,
        src: Rect,
        d: (i32, i32),
        px_per_world: u32,
        flip: bool,
        shade: Shade,
    ) {
        let s = px_per_world.max(1) as f32;
        let pos = [self.p.x + d.0 as f32 / 2.0, self.p.y + d.1 as f32 / 2.0];
        let size = [src.w as f32 / s, src.h as f32 / s];
        let tint = Rgba::rgb(255, 255, 255);
        self.out.sprite(Layer::Worms, Sprite { atlas, src, pos, size, flip, tint, shade });
    }

    /// A solid rectangle covering screen pixels `[x0, x1) × [y0, y1)` from
    /// `p`: health bars, markers in a lit match.
    fn fill(&mut self, white: AtlasId, (x0, y0, x1, y1): (i32, i32, i32, i32), colour: Rgba, shade: Shade) {
        let src = Rect { x: 0, y: 0, w: 1, h: 1 };
        let pos = [self.p.x + x0 as f32 / 2.0, self.p.y + y0 as f32 / 2.0];
        let size = [(x1 - x0) as f32 / 2.0, (y1 - y0) as f32 / 2.0];
        self.out
            .sprite(Layer::Worms, Sprite { atlas: white, src, pos, size, flip: false, tint: colour, shade });
    }
}

/// What [`draw_worm`] needs about the frame.
pub struct WormFrame<'a> {
    pub world: &'a World,
    pub assets: &'a SceneAssets,
    pub view: View,
    pub show_health_bars: bool,
    /// A lit match (plan 15a R13): worms take the worm floor (plan 15b);
    /// unlit, every item is drawn lit, as always.
    pub lit: bool,
}

impl WormFrame<'_> {
    /// A worm's, its rope's and hook's shade: the worm floor in a lit match.
    fn body(&self) -> Shade {
        if self.lit { Shade::WORM } else { Shade::LIT }
    }

    /// A marker's (crosshair, health bar): its art, unlit, in a lit match.
    fn marker(&self) -> Shade {
        if self.lit { Shade::MARKER } else { Shade::LIT }
    }

    /// A glow's (the muzzle flash): full self-light in a lit match.
    fn glow(&self) -> Shade {
        if self.lit { Shade::GLOW } else { Shade::LIT }
    }
}

/// What a worm shows besides its state: the muzzle flash and the green
/// crosshair (from the last tick), and its drawn hook position.
pub struct WormExtras {
    pub muzzle: bool,
    pub target: bool,
    pub hook: Vec2,
}

/// One alive worm, drawn at `p` (its drawn position, already snapped), in
/// `CWorm::Draw`'s order. The rope is drawn even when the worm is off
/// screen; the rest only when it is near the view.
///
/// OLX: src/common/CWorm.cpp:1016-1300
pub fn draw_worm(f: &WormFrame, i: usize, w: &Worm, p: Vec2, extras: &WormExtras, out: &mut DrawList) {
    let look: WormLook = f.assets.worm(i);
    if w.rope.released {
        draw_rope(f, p, extras.hook, out);
    }
    // `x+l+10 < l || x-10 > VirtW || ...` in screen pixels.
    let (rx, ry) = (2.0 * (p.x - f.view.x), 2.0 * (p.y - f.view.y));
    if rx + 10.0 < 0.0 || rx - 10.0 > 2.0 * f.view.w || ry + 10.0 < 0.0 || ry - 10.0 > 2.0 * f.view.h {
        return;
    }
    let white = f.assets.white;
    let mut at = At { out, p };
    if f.show_health_bars && (!look.local || !look.human) {
        // The old-style bar (OLX's fallback without `worm_health.png`):
        // a border grid, then up to five segments.
        let border = Rgba::rgb(0x49, 0x50, 0x65);
        let hy = -9 - 7;
        let m = f.marker();
        at.fill(white, (-10, hy - 1, 16, hy), border, m);
        at.fill(white, (-10, hy + 5, 16, hy + 6), border, m);
        at.fill(white, (-10, hy - 1, -9, hy + 6), border, m);
        at.fill(white, (15, hy - 1, 16, hy + 6), border, m);
        for x in [-5, 0, 5, 10] {
            at.fill(white, (x, hy, x + 1, hy + 5), border, m);
        }
        const COLOURS: [Rgba; 5] = [
            Rgba::rgb(0xe3, 0x04, 0x04),
            Rgba::rgb(0xfe, 0x85, 0x03),
            Rgba::rgb(0xfe, 0xe9, 0x03),
            Rgba::rgb(0xa8, 0xfe, 0x03),
            Rgba::rgb(0x21, 0xfe, 0x03),
        ];
        for (s, &c) in COLOURS.iter().enumerate().take(health_segments(w.health).max(0) as usize) {
            let x = -10 + s as i32 * 5 + 1;
            at.fill(white, (x, hy, x + 4, hy + 5), c, m);
        }
    }
    if look.local {
        let (dx, dy) = crosshair_offset(w.angle, w.face);
        let frame = if extras.target { 6 } else { 0 };
        at.blit(
            f.assets.crosshair,
            Rect { x: frame, y: 0, w: 6, h: 6 },
            (dx - 2, dy - 2),
            2,
            false,
            f.marker(),
        );
    }
    let skin = f.assets.image(look.skin);
    let (src, flip) = skin_frame(w.angle, w.face, w.frame, skin.image.width() / FRAME_WIDTH);
    let src = clip(src, skin.image.width(), skin.image.height());
    let d = (-(SKIN_WIDTH as i32) / 2, -(SKIN_HEIGHT as i32) / 2);
    at.blit(look.skin, src, d, skin.px_per_world, flip, f.body());
    if extras.muzzle {
        let (frame, dx, dy) = muzzle_frame(w.angle, w.face);
        at.blit(f.assets.muzzle, Rect { x: frame * 16, y: 0, w: 16, h: 16 }, (dx, dy), 1, false, f.glow());
    }
}

/// `src` cut to a `w` × `h` image (an odd skin sheet never reads outside).
pub(crate) fn clip(src: Rect, w: u32, h: u32) -> Rect {
    let x = src.x.min(w);
    let y = src.y.min(h);
    Rect { x, y, w: src.w.min(w - x), h: src.h.min(h - y) }
}

/// The ninja rope from the hook to the worm, then the hook if it is in
/// view. At world resolution (spec §19 E3) the non-antialiased rope is a
/// 1-pixel line alternating `RopeColour2`, `RopeColour1` per pixel from the
/// hook (`DrawRope` resets the phase each call). `hook` is the drawn hook:
/// a hook attached to a worm is at that worm as drawn (`Scene::build`).
///
/// OLX: src/client/CNinjaRope.cpp:132-180, src/client/GfxPrimitives.cpp:1828-1846, :2458-2477
fn draw_rope(f: &WormFrame, p: Vec2, hook: Vec2, out: &mut DrawList) {
    let h = Vec2::new((hook.x as i32) as f32, (hook.y as i32) as f32);
    let (a, b) = ((h.x as i32, h.y as i32), (p.x as i32, p.y as i32));
    let (white, colours) = (f.assets.white, f.assets.rope_colours);
    let (v, mut n) = (f.view, 0usize);
    perform_line(a, b, |x, y| {
        n += 1;
        if v.overlaps(x as f32, y as f32, 1.0, 1.0) {
            out.pixel_shaded(Layer::Worms, white, (x as f32, y as f32), colours[n % 2], f.body());
        }
    });
    if v.contains(h.x, h.y) {
        let src = Rect { x: 0, y: 0, w: 6, h: 6 };
        let tint = Rgba::rgb(255, 255, 255);
        let sprite = Sprite {
            atlas: f.assets.hook,
            src,
            pos: [h.x - 1.0, h.y - 1.0],
            size: [3.0, 3.0],
            flip: false,
            tint,
            shade: f.body(),
        };
        out.sprite(Layer::Worms, sprite);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crosshair_and_muzzle_follow_the_aim() {
        // Straight ahead: 32 screen px to the right (or left), level.
        assert_eq!(crosshair_offset(0.0, Dir::Right), (32, 0));
        assert_eq!(crosshair_offset(0.0, Dir::Left), (-32, 0));
        // Up: (int)(32 cos -90°) is 0 after float rounding, -32 up.
        assert_eq!(crosshair_offset(-90.0, Dir::Right), (0, -32));
        // 45° down-right: 32 · 0.7071 = 22.6 → 22.
        assert_eq!(crosshair_offset(45.0, Dir::Right), (22, 22));
        // Aim 0 is bucket (90/151·7) = 4: right-facing frame 2, left 11.
        assert_eq!(muzzle_frame(0.0, Dir::Right), (2, -12 + 4, -10));
        assert_eq!(muzzle_frame(0.0, Dir::Left), (11, -21 - 2, -10));
        // Straight up is frame 6 (right) or 7 (left); out-of-range aims clamp.
        assert_eq!(muzzle_frame(-90.0, Dir::Right), (6, -12 - 2, -10 - 12));
        assert_eq!(muzzle_frame(90.0, Dir::Left), (13, -21 + 1, -10 + 3));
    }

    #[test]
    fn health_bar_rounds_like_olx() {
        assert_eq!(health_segments(100.0), 5);
        assert_eq!(health_segments(64.0), 4); // Round(3.95) = 4
        assert_eq!(health_segments(15.0), 1); // Round(1.5) = ceil(1.0) = 1
        assert_eq!(health_segments(0.0), 1); // Round(0.75) = 1
    }
}
