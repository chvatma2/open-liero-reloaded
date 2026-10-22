//! The engine's own look (spec §The engine, plan 15a R13): code, not data, so
//! `olx-core` stays the importer's output. It acts only in a match with a
//! look.

use alloc::vec;

use crate::{Animation, Key, Light, Reach};

/// The least self-light of worms, projectiles and bonuses in a lit match,
/// percent (spec C6): a dark level never hides play.
pub const FLOOR: u8 = 50;

/// The least light of a worm (with its rope and hook) in a lit match,
/// percent of the level's ambient hue at full brightness (plan 15b, the
/// user's playtest 2026-10-19: "a bit lower" than [`FLOOR`]; a starting
/// value to retune).
pub const WORM_FLOOR: u8 = 35;

/// The worm floor of an ambient colour (sRGB), linear (plan 15b): the
/// colour scaled until its brightest linear channel is 1, then taken at
/// [`WORM_FLOOR`] percent. Black (no hue) gives white. A hue darker than
/// half of the white floor's luminance is mixed towards white until it
/// reaches that half, so a near-black hue never makes a worm vanish.
pub fn worm_floor_of(ambient: [u8; 3]) -> [f32; 3] {
    let k = f32::from(WORM_FLOOR) / 100.0;
    let c = ambient.map(crate::srgb_to_linear);
    let m = c[0].max(c[1]).max(c[2]);
    if m <= 0.0 {
        return [k; 3];
    }
    let f = c.map(|v| v / m * k);
    let lum = |c: [f32; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let (have, want) = (lum(f), 0.5 * k);
    if have >= want {
        return f;
    }
    // White at k has luminance k: blend f + t × (white − f) to `want`.
    let t = (want - have) / (k - have);
    f.map(|v| v + t * (k - v))
}

fn flash(colour: [u8; 3], intensity: u16, reach: u16, ms: u32) -> Light {
    let key = |at_ms, level| Key { at_ms, level, colour: None };
    Light {
        colour,
        intensity,
        reach: Reach::Pixels(reach),
        animation: Animation::Keys { keys: vec![key(0, 100), key(ms, 0)], looped: false },
        phase_ms: Some(0),
    }
}

/// How strongly rock and dirt reflect the light reaching them, times their
/// colour (plan 15c: bounce). The renderer clamps it to 0-0.9 so the
/// feedback always converges.
pub const BOUNCE: f32 = 0.6;

/// An explosion light's keys, ms: the presentation fits them to the
/// explosion's life.
pub const EXPLOSION_MS: u32 = 200;

/// An explosion of OLX size `size`: warm, brighter and wider with the size.
pub fn explosion_light(size: i32) -> Light {
    let s = size.clamp(0, 500) as u16;
    flash([255, 200, 140], (60 + 8 * s).min(250), (24 + 8 * s).min(4096), EXPLOSION_MS)
}

/// The heat an explosion leaves (issue #40): the renderer's deposit, its
/// centre and falloff `1 - (d / radius)²` as the renderer's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExplosionHeat {
    /// World pixels.
    pub radius: f32,
    /// The temperature it gives stone at its centre, 1 to
    /// [`crate::MAX_TEMPERATURE`].
    pub amount: f32,
}

/// The widest engine explosion heat, world pixels (the renderer takes 64).
pub const MAX_EXPLOSION_HEAT_RADIUS: f32 = 48.0;

/// The radii of OLX's hole stamps by hole size (damage 0 to 4 and up): the
/// dirt theme's `holes/1-5.png`, 5, 7, 9, 11 and 15 pixels across. The
/// heat reaches past the crater the explosion carves.
///
/// OLX: src/common/CMap.cpp:1005-1105
const HOLE_RADII: [f32; 5] = [2.5, 3.5, 4.5, 5.5, 7.5];

/// The engine's heat of an explosion of `damage` (OLX's `Explode` damage,
/// which also sizes its hole; issue #40, spec
/// `2026-10-22-materials-and-heat-design.md`): `amount` 1.2 × damage⁴ up to
/// [`crate::MAX_TEMPERATURE`], `radius` twice the hole's radius plus half
/// the damage, up to [`MAX_EXPLOSION_HEAT_RADIUS`]. Calibrated on stone: a
/// minigun hit (damage 3) leaves its rim under the lava ramp's first key
/// (100) unless hits pile up; a Bazooka's (4) rim glows a few pixels deep;
/// a Big Nuke's fragments (15) leave a wide patch near white. `None` for a
/// damage of 0 or less, or not a number.
pub fn explosion_heat(damage: f32) -> Option<ExplosionHeat> {
    if damage.is_nan() || damage <= 0.0 {
        return None;
    }
    let s = damage.min(1000.0);
    let hole = HOLE_RADII[(s as usize).min(HOLE_RADII.len() - 1)];
    let amount = (1.2 * s * s * s * s).min(f32::from(crate::MAX_TEMPERATURE));
    (amount >= 1.0)
        .then_some(ExplosionHeat { radius: (2.0 * hole + s / 2.0).min(MAX_EXPLOSION_HEAT_RADIUS), amount })
}

/// The largest disc a mod's explosion light may ask for, world pixels
/// (issue #15): the renderer's largest disc. `olr-format` refuses more,
/// [`crate::ExplosionLight::radius`] clamps to it.
pub const MAX_EXPLOSION_RADIUS: u8 = 16;

/// The farthest a mod's explosion light may reach, world pixels (issue
/// #15): as far as the engine's largest explosion. `olr-format` refuses
/// more (and `Unlimited`), [`crate::ExplosionLight::reach`] clamps to it.
pub const MAX_EXPLOSION_REACH: u16 = 4096;

/// A mod's explosion light without an animation (issue #15): `colour` at
/// `intensity` percent (at most 1000) reaching `reach` world pixels (1 to
/// [`MAX_EXPLOSION_REACH`]), with the engine's flash keys over
/// [`EXPLOSION_MS`], which the presentation fits to the explosion's life.
pub fn explosion_flash(colour: [u8; 3], intensity: u16, reach: u16) -> Light {
    flash(colour, intensity.min(1000), reach.clamp(1, MAX_EXPLOSION_REACH), EXPLOSION_MS)
}

/// A muzzle flash's light.
pub fn muzzle_light() -> Light {
    flash([255, 220, 160], 80, 40, 50)
}

/// The engine's switch for skin light (issue #12): a skin's `light` lights
/// the worm's surroundings in a lit level. `false` turns every skin's light
/// off, everywhere (their glowing pixels still show bright); there is no
/// per-player or per-match setting.
pub const SKIN_LIGHT: bool = true;

/// The brightest a skin's light may be, percent: `olr-format` refuses more,
/// [`skin_light`] clamps to it.
pub const MAX_SKIN_LIGHT_INTENSITY: u16 = 200;

/// The farthest a skin's light may reach, world pixels, so a skin never
/// floods a level: `olr-format` refuses more, [`skin_light`] clamps to it.
pub const MAX_SKIN_LIGHT_REACH: u16 = 48;

/// A skin's light (issue #12) of `colour` at `intensity` percent reaching
/// `reach` world pixels, capped at [`MAX_SKIN_LIGHT_INTENSITY`] and
/// [`MAX_SKIN_LIGHT_REACH`]: steady, following its worm.
pub fn skin_light(colour: [u8; 3], intensity: u16, reach: u16) -> Light {
    Light {
        colour,
        intensity: intensity.min(MAX_SKIN_LIGHT_INTENSITY),
        reach: Reach::Pixels(reach.min(MAX_SKIN_LIGHT_REACH)),
        animation: Animation::Steady,
        phase_ms: Some(0),
    }
}

/// A glowing disc's radius, world pixels, for a light of `reach` (plan 15c,
/// "My calls": a muzzle flash, shot light or end flash): about an eighth of
/// the reach, 1 to 6; 1 for a reach that is not finite.
pub fn disc_radius(reach: f32) -> f32 {
    if reach.is_finite() { (reach / 8.0).clamp(1.0, 6.0) } else { 1.0 }
}

/// An explosion's disc radius, world pixels (plan 15c): it grows with the
/// OLX size (clamped to 0-500), 2 to 12.
pub fn explosion_radius(size: i32) -> f32 {
    (2 + size.clamp(0, 500) / 2).clamp(2, 12) as f32
}

/// The most a disc's radiance is of its colour.
const MAX_RADIANCE: f32 = 64.0;

/// The radiance of a glowing disc of `radius` and `reach` whose colour
/// (linear, × intensity × animation) is `colour` (plan 15c, "My calls"):
/// calibrated so its light at `d0 = max(reach / 3, 2 radius)` (at most
/// halfway from the disc's edge to its reach) in the open equals today's
/// point light there. A disc lights `L · min(1, 2r / (TAU
/// d0)) · fade(d0)` (the share of directions it covers); a point light
/// lit `colour · fade(d0 − r)`. Always finite, at most 64 × the colour; an
/// unlimited reach is calibrated at `2 radius`.
pub fn disc_radiance(colour: [f32; 3], radius: f32, reach: f32) -> [f32; 3] {
    let r = if radius.is_finite() { radius.max(0.5) } else { 1.0 };
    let reach = if reach.is_nan() { 0.0 } else { reach };
    // Within the reach: no farther than halfway from the disc's edge to it.
    let d0 = if reach.is_finite() { (reach / 3.0).max(2.0 * r).min((r + reach) / 2.0) } else { 2.0 * r };
    let share = (2.0 * r / (core::f32::consts::TAU * d0)).min(1.0);
    let (now, disc) = (fade_px(d0 - r, reach), share * fade_px(d0, reach));
    let k = if disc > 1e-12 { (now / disc).min(MAX_RADIANCE) } else { 0.0 };
    colour.map(|c| if c.is_finite() { (c.max(0.0) * k).min(c.max(0.0) * MAX_RADIANCE) } else { 0.0 })
}

/// [`crate::fade`] for a reach in world pixels (`f32::INFINITY` unlimited).
fn fade_px(d: f32, reach: f32) -> f32 {
    if reach.is_infinite() {
        return 1.0;
    }
    let x = d / reach;
    if x.is_nan() || x >= 1.0 || reach <= 0.0 {
        return 0.0;
    }
    let v = 1.0 - x * x;
    v * v
}
