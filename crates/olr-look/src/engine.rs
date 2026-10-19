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

/// An explosion light's keys, ms: the presentation fits them to the
/// explosion's life.
pub const EXPLOSION_MS: u32 = 200;

/// An explosion of OLX size `size`: warm, brighter and wider with the size.
pub fn explosion_light(size: i32) -> Light {
    let s = size.clamp(0, 500) as u16;
    flash([255, 200, 140], (60 + 8 * s).min(250), (24 + 8 * s).min(4096), EXPLOSION_MS)
}

/// A muzzle flash's light.
pub fn muzzle_light() -> Light {
    flash([255, 220, 160], 80, 40, 50)
}
