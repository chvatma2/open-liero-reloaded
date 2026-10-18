//! The engine's own look (spec §The engine, plan 15a R13): code, not data, so
//! `olx-core` stays the importer's output. It acts only in a match with a
//! look.

use alloc::vec;

use crate::{Animation, Key, Light, Reach};

/// The least self-light of worms, projectiles and bonuses in a lit match,
/// percent (spec C6): a dark level never hides play.
pub const FLOOR: u8 = 50;

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
