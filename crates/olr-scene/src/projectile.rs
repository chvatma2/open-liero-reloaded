//! Projectiles and bonuses as OLX draws them (`CProjectile::Draw`,
//! `CBonus::Draw`).

use olr_data::image::Rgba;
use olr_data::mod_def::ProjectileGfx;
use olr_math::Vec2;
use olr_sim::{Bonus, BonusKind, Projectile, SimProjectile};

use crate::assets::SceneAssets;
use crate::draw_list::{DrawList, Layer, Rect, Sprite, View};

/// `BONUS_LIFETIME` (`include/CBonus.h:30`): a bonus blinks for its last 3 s.
const BONUS_LIFETIME: f32 = 60.0;

/// `-atan2(x, y)` in degrees, as `CProjectile::Draw` computes its heading.
fn heading(v: Vec2) -> f32 {
    (-(v.x as f64).atan2(v.y as f64) * (180.0 / core::f64::consts::PI)) as f32
}

/// The image frame (`framestep`) of an image projectile: its animation
/// frame, else its special-angle or angle image, else its rotation (only
/// while it moves, and only with a nonzero `RotIncrement`: a zero one
/// counts as 1 once `rot_bumped`, OLX's sticky mutation). A projectile
/// whose centre is outside the view uses frame 0, as OLX does.
///
/// OLX: src/common/CProjectile.cpp:238-286
pub fn framestep(gfx: &ProjectileGfx, p: &Projectile, rot_bumped: bool, inside: bool) -> f32 {
    let ProjectileGfx::Image {
        rotating,
        rot_increment,
        use_angle,
        use_spec_angle,
        angle_images,
        animation,
        ..
    } = *gfx
    else {
        return 0.0;
    };
    if !inside {
        return 0.0;
    }
    if animation.is_some() {
        return p.frame;
    }
    if use_spec_angle {
        let mut angle = heading(p.vel);
        let mut direct = 0;
        if angle > 0.0 {
            angle = 180.0 - angle;
        }
        if angle < 0.0 {
            angle += 180.0;
            direct = 1;
        }
        if angle == 0.0 {
            direct = 0;
        }
        let num = (angle_images.wrapping_sub(1) / 2) as f32;
        return if direct == 0 { (151.0 - angle) / 151.0 * num } else { angle / 151.0 * num + (num + 1.0) };
    }
    if use_angle {
        let mut angle = heading(p.vel);
        let offset = 360.0 / angle_images as f32;
        olr_math::fmod(&mut angle, 360.0);
        return angle / offset;
    }
    let rot_increment = if rot_increment == 0 && rot_bumped { 1 } else { rot_increment };
    if rot_increment != 0 && rotating && (p.vel.x != 0.0 || p.vel.y != 0.0) {
        return p.rotation / rot_increment as f32;
    }
    0.0
}

/// The source x of `framestep`'s frame in a strip `image_w` wide of square
/// `size` frames: `MOD((int)framestep * size, w)`, never negative.
///
/// OLX: src/common/CProjectile.cpp:288-291
pub fn frame_x(framestep: f32, size: u32, image_w: u32) -> u32 {
    if image_w == 0 {
        return 0;
    }
    ((framestep as i32 as i64) * size as i64).rem_euclid(image_w as i64) as u32
}

/// One projectile at its drawn position `p` (snapped when exact).
///
/// OLX: src/common/CProjectile.cpp:227-300
pub fn draw_projectile(
    proj: &SimProjectile,
    p: &Projectile,
    at: Vec2,
    rot_bumped: bool,
    assets: &SceneAssets,
    view: View,
    out: &mut DrawList,
) {
    let inside = view.contains(at.x, at.y);
    match &proj.def.gfx {
        ProjectileGfx::Pixel { colours } => {
            // `DrawRectFill2x2` at `p - 1` screen pixels: half a world pixel up-left.
            let c = p.colour;
            let (r, g, b) = ((c >> 16) as u8, (c >> 8) as u8, c as u8);
            let a = colours.iter().find(|k| (k.r, k.g, k.b) == (r, g, b)).map_or(255, |k| k.a);
            if inside && a != 0 {
                let tint = Rgba { r, g, b, a };
                let src = Rect { x: 0, y: 0, w: 1, h: 1 };
                let pos = [at.x - 0.5, at.y - 0.5];
                let sprite = Sprite { atlas: assets.white, src, pos, size: [1.0, 1.0], flip: false, tint };
                out.sprite(Layer::Projectiles, sprite);
            }
        }
        gfx @ ProjectileGfx::Image { .. } => {
            let Some(id) = assets.projectile_image(p.proj) else { return };
            let img = assets.image(id);
            let (w, size) = (img.image.width(), img.image.height());
            let s = img.px_per_world as f32;
            let half = (size / 2) as f32 / s;
            let (pos, dim) = ([at.x - half, at.y - half], size as f32 / s);
            if !view.overlaps(pos[0], pos[1], dim, dim) {
                return;
            }
            let x = frame_x(framestep(gfx, p, rot_bumped, inside), size, w);
            let src = Rect { x, y: 0, w: size.min(w - x), h: size };
            let size = [src.w as f32 / s, dim];
            let tint = Rgba::rgb(255, 255, 255);
            out.sprite(Layer::Projectiles, Sprite { atlas: id, src, pos, size, flip: false, tint });
        }
    }
}

/// One client bonus at its drawn position `p`, unless it is blinking off.
/// Both images are drawn at `p − 5` screen pixels (the weapon crate is off
/// centre).
///
/// OLX: src/common/CBonus.cpp:49-76
pub fn draw_bonus(b: &Bonus, p: Vec2, assets: &SceneAssets, view: View, out: &mut DrawList) {
    if b.life > BONUS_LIFETIME - 3.0 && b.flash_time > 0.15 {
        return;
    }
    let id = match b.kind {
        BonusKind::Health => assets.health,
        BonusKind::Weapon => assets.bonus,
    };
    let img = assets.image(id);
    let s = img.px_per_world as f32;
    let (w, h) = (img.image.width(), img.image.height());
    let (pos, size) = ([p.x - 2.5, p.y - 2.5], [w as f32 / s, h as f32 / s]);
    if view.overlaps(pos[0], pos[1], size[0], size[1]) {
        let src = Rect { x: 0, y: 0, w, h };
        let tint = Rgba::rgb(255, 255, 255);
        out.sprite(Layer::Bonuses, Sprite { atlas: id, src, pos, size, flip: false, tint });
    }
}
