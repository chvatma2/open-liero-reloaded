//! Entities as OLX's `DrawEntities` draws them (presentation note §7.3).

use olr_data::image::Rgba;
use olr_scene::{DrawList, Layer, Rect, Shade, Sprite};

use crate::assets::{FxAssets, FxSprite};
use crate::entity::{DOOMSDAY, Entity, Kind};

/// Where to draw: OLX's screen position of the entity relative to the
/// viewport, `int((pos - World) * 2)` (half-pixel precision), and the
/// view's corner it is measured from.
#[derive(Clone, Copy)]
struct At {
    x: i32,
    y: i32,
    view: (f32, f32),
}

impl At {
    /// Screen pixel `(sx, sy)` relative to the viewport, as world pixels.
    fn world(&self, sx: i32, sy: i32) -> [f32; 2] {
        [self.view.0 + sx as f32 / 2.0, self.view.1 + sy as f32 / 2.0]
    }

    /// `DrawRectFill2x2(x + dx, y + dy, colour)`: one world pixel.
    fn block(&self, assets: &FxAssets, out: &mut DrawList, dx: i32, dy: i32, colour: Rgba, shade: Shade) {
        let [x, y] = self.world(self.x.saturating_add(dx), self.y.saturating_add(dy));
        out.pixel_shaded(Layer::Entities, assets.white, (x, y), colour, shade);
    }

    /// `DrawImageAdv(img, sx, 0, x + d, y + d, w, w)` of a strip of `w` ×
    /// `w` frames: SDL clips the source rectangle to the image, moving the
    /// destination with it, so frames before or past the image draw nothing.
    fn blit(&self, out: &mut DrawList, img: FxSprite, (sx, d, w_frame): (i64, i32, u32), shade: Shade) {
        let (mut sx, mut w, mut dx) = (sx, w_frame as i64, d as i64);
        if sx < 0 {
            (w, dx) = (w + sx, dx - sx);
            sx = 0;
        }
        let w = w.min(img.w as i64 - sx);
        let h = (w_frame as i64).min(img.h as i64);
        if w <= 0 || h <= 0 {
            return;
        }
        let dx = i32::try_from(dx).unwrap_or(i32::MAX);
        let pos = self.world(self.x.saturating_add(dx), self.y.saturating_add(d));
        let src = Rect { x: sx as u32, y: 0, w: w as u32, h: h as u32 };
        let size = [w as f32 / 2.0, h as f32 / 2.0];
        let tint = Rgba::rgb(255, 255, 255);
        out.sprite(Layer::Entities, Sprite { atlas: img.atlas, src, pos, size, flip: false, tint, shade });
    }
}

/// One entity, unless its position is outside the view (`x < Left` or
/// `x > Left + VirtW`, likewise `y`).
///
/// OLX's view corner (`GetWorldX`, `GetWorldY`) is a whole pixel, so
/// entities sit on the world's half-pixel grid; an interpolated camera's
/// view is not, so the grid is measured from its corner truncated, and only
/// the clip uses the true view.
///
/// OLX: src/client/Entity.cpp:210-315
pub(crate) fn entity(e: &Entity, assets: &FxAssets, lit: bool, out: &mut DrawList) {
    let glow = if lit { Shade::GLOW } else { Shade::LIT };
    // Blood and gibs never show below the solid floor, as rock does (the
    // user's playtest, 2026-10-19).
    let floored = if lit { Shade::FLOORED } else { Shade::LIT };
    let lit = Shade::LIT;
    let v = out.view;
    let (ox, oy) = (v.x.trunc(), v.y.trunc());
    let at = At { x: ((e.pos.x - ox) * 2.0) as i32, y: ((e.pos.y - oy) * 2.0) as i32, view: (ox, oy) };
    let (left, top) = ((v.x - ox) * 2.0, (v.y - oy) * 2.0);
    let (x, y) = (at.x as f32, at.y as f32);
    if x < left || x > left + v.w * 2.0 || y < top || y > top + v.h * 2.0 {
        return;
    }
    let frame = e.frame as i32 as i64;
    match e.kind {
        Kind::Particle => at.block(assets, out, -1, -1, e.colour, lit),
        Kind::Blood | Kind::BloodDropper => at.block(assets, out, -1, -1, e.colour, floored),
        Kind::Explosion => at.blit(out, assets.explosion, (frame * 32, -16, 32), glow),
        Kind::Smoke => at.blit(out, assets.smoke, (frame * 14, -7, 14), lit),
        Kind::ChemSmoke => at.blit(out, assets.chemsmoke, (frame * 10, -5, 10), lit),
        Kind::Spawn => at.blit(out, assets.spawn, (frame * 32, -16, 32), glow),
        Kind::Gib { worm } => {
            let (atlas, sheet) = assets.gibs(worm);
            let img = FxSprite { atlas, w: sheet.width(), h: sheet.height() };
            at.blit(out, img, (e.rotation as i64 * 8, -2, 8), floored);
        }
        // The sim's sparkles have no colour: the image, never the two blocks.
        Kind::Sparkle => at.blit(out, assets.sparkle, (frame * 10, -5, 10), glow),
        Kind::Doomsday => {
            if let Some(&c) = usize::try_from(frame).ok().and_then(|i| DOOMSDAY.get(i)) {
                at.block(assets, out, -1, -1, c, lit);
            }
        }
        Kind::JetpackSpray => {
            let f = |k: f32| (k * (255.0 - e.frame)).min(255.0) as u8;
            at.block(assets, out, -1, -1, Rgba::rgb(f(0.314), f(0.588), f(0.784)), lit);
        }
        Kind::LaserSight => {}
    }
}
