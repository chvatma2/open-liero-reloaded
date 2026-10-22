//! Surface materials (spec `2026-10-22-materials-and-heat-design.md`): what
//! a level's pixels are made of for the look (stone, wood, metal, glass,
//! …), apart from OLX's collision kind, which stays the simulation's. Heat
//! is their first effect. Presentation only: the simulation never sees them.
//!
//! `olr-format` checks every bound when it compiles a material; the
//! accessors here clamp again, so a hand-built value never misbehaves.

use alloc::string::String;
use alloc::vec::Vec;

use crate::Rgb;

/// The most materials a level holds (a map value is one byte; 0 is none).
pub const MAX_MATERIALS: usize = 255;
/// The largest `sensitivity`, percent.
pub const MAX_SENSITIVITY: u8 = 100;
/// The smallest `capacity`, percent of the engine's reference (stone's, 100).
pub const MIN_CAPACITY: u16 = 10;
/// The largest `capacity`, percent of the engine's reference.
pub const MAX_CAPACITY: u16 = 1000;
/// The largest `conductivity`, percent of the engine's reference (stone's, 100).
pub const MAX_CONDUCTIVITY: u16 = 1000;
/// The smallest `cooling`, percent of the engine's reference (stone's,
/// 100): every hot pixel beside air cools in the end.
pub const MIN_COOLING: u16 = 1;
/// The largest `cooling`, percent of the engine's reference.
pub const MAX_COOLING: u16 = 1000;
/// The hottest temperature: a fresh crater's rim at its brightest. The
/// scale is the engine's, 0 (cold) to this.
pub const MAX_TEMPERATURE: u16 = 1000;
/// The most keys of a colour ramp.
pub const MAX_RAMP_KEYS: usize = 16;
/// The largest ramp key `glow` and heat `light`, percent.
pub const MAX_GLOW: u8 = 100;

/// One key of a colour ramp: from `temperature` on, a hot pixel shows
/// `colour` over its art at `glow` percent (and glows that much, whatever
/// the darkness).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RampKey {
    /// 1 to [`MAX_TEMPERATURE`].
    pub temperature: u16,
    pub colour: Rgb,
    /// Percent, 0 to [`MAX_GLOW`].
    pub glow: u8,
}

/// The engine's colour ramp, a material's without its own: lava, from a
/// faint dull red through orange to yellow-white.
pub const LAVA_RAMP: [RampKey; 6] = [
    RampKey { temperature: 100, colour: [60, 6, 0], glow: 0 },
    RampKey { temperature: 250, colour: [140, 20, 0], glow: 35 },
    RampKey { temperature: 450, colour: [224, 74, 0], glow: 65 },
    RampKey { temperature: 650, colour: [255, 140, 26], glow: 85 },
    RampKey { temperature: 850, colour: [255, 208, 96], glow: 100 },
    RampKey { temperature: 1000, colour: [255, 244, 220], glow: 100 },
];

/// How a material takes heat, spreads it, loses it and shows it. Every
/// value but the sensitivity is percent of the engine's reference
/// material (stone: 100).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heat {
    /// Percent of the deposited heat the pixel takes, 0 to
    /// [`MAX_SENSITIVITY`] (0: it never heats).
    pub sensitivity: u8,
    /// How much heat warms a pixel: higher warms less and holds longer.
    /// [`MIN_CAPACITY`] to [`MAX_CAPACITY`].
    pub capacity: u16,
    /// How fast heat spreads to neighbouring pixels (and so how deep it
    /// soaks in), 0 to [`MAX_CONDUCTIVITY`].
    pub conductivity: u16,
    /// How fast a pixel beside air loses heat to it, [`MIN_COOLING`] to
    /// [`MAX_COOLING`].
    pub cooling: u16,
    /// Temperature to colour and glow, its keys' temperatures rising; empty
    /// is the engine's ([`LAVA_RAMP`]).
    pub ramp: Vec<RampKey>,
    /// Percent of its glow cast as light, 0 (it shows hot but lights
    /// nothing) to [`MAX_GLOW`].
    pub light: u8,
}

impl Heat {
    /// [`Heat::sensitivity`], at most [`MAX_SENSITIVITY`].
    pub fn sensitivity(&self) -> u8 {
        self.sensitivity.min(MAX_SENSITIVITY)
    }

    /// [`Heat::capacity`], clamped to its range.
    pub fn capacity(&self) -> u16 {
        self.capacity.clamp(MIN_CAPACITY, MAX_CAPACITY)
    }

    /// [`Heat::conductivity`], at most [`MAX_CONDUCTIVITY`].
    pub fn conductivity(&self) -> u16 {
        self.conductivity.min(MAX_CONDUCTIVITY)
    }

    /// [`Heat::cooling`], clamped to its range.
    pub fn cooling(&self) -> u16 {
        self.cooling.clamp(MIN_COOLING, MAX_COOLING)
    }

    /// [`Heat::light`], at most [`MAX_GLOW`].
    pub fn light(&self) -> u8 {
        self.light.min(MAX_GLOW)
    }

    /// The ramp drawn with: its own (at most [`MAX_RAMP_KEYS`] keys), else
    /// [`LAVA_RAMP`].
    pub fn ramp(&self) -> &[RampKey] {
        match self.ramp.len() {
            0 => &LAVA_RAMP,
            n => &self.ramp[..n.min(MAX_RAMP_KEYS)],
        }
    }

    /// How a pixel at `temperature` shows: [`ramp_at`] of [`Heat::ramp`].
    pub fn look_at(&self, temperature: u16) -> Option<(Rgb, u8)> {
        ramp_at(self.ramp(), temperature)
    }
}

/// A surface material: what a level's map values name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceMaterial {
    /// Unique in its level.
    pub id: String,
    /// `None`: it never heats.
    pub heat: Option<Heat>,
}

impl SurfaceMaterial {
    /// Its heat when it takes any (a sensitivity above 0).
    pub fn heats(&self) -> Option<&Heat> {
        self.heat.as_ref().filter(|h| h.sensitivity() > 0)
    }
}

/// The colour and glow (percent) of `ramp` at `temperature`: `None` below
/// its first key (the pixel shows its art), linear between keys, the last
/// key's above it. Keys out of order never panic: each segment is read
/// as it stands, a key not above the one before taken at once. Integer
/// only.
pub fn ramp_at(ramp: &[RampKey], temperature: u16) -> Option<(Rgb, u8)> {
    let first = ramp.first()?;
    if temperature < first.temperature {
        return None;
    }
    let mut at = (first.colour, first.glow.min(MAX_GLOW));
    for pair in ramp.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if temperature >= b.temperature {
            at = (b.colour, b.glow.min(MAX_GLOW));
            continue;
        }
        if temperature < a.temperature || b.temperature <= a.temperature {
            break;
        }
        let (t, span) = (i32::from(temperature - a.temperature), i32::from(b.temperature - a.temperature));
        let mix = |x: u8, y: u8| (i32::from(x) + (i32::from(y) - i32::from(x)) * t / span) as u8;
        let colour = [mix(a.colour[0], b.colour[0]), mix(a.colour[1], b.colour[1]), mix(a.colour[2], b.colour[2])];
        return Some((colour, mix(a.glow.min(MAX_GLOW), b.glow.min(MAX_GLOW))));
    }
    Some(at)
}

/// A level's surface maps (`look/surface.png`, `look/surface-back.png`): a
/// byte per world pixel, row-major. A value `v` (1-255) is the look's
/// `materials[v - 1]`; 0 is none.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SurfaceMaps {
    /// World pixels.
    pub world: (u32, u32),
    /// The front: rock and dirt.
    pub front: Vec<u8>,
    /// The back: what shows where the front is carved or background.
    pub back: Vec<u8>,
}

impl SurfaceMaps {
    /// Maps of `world` naming no material.
    pub fn empty(world: (u32, u32)) -> SurfaceMaps {
        let n = world.0 as usize * world.1 as usize;
        SurfaceMaps { world, front: alloc::vec![0; n], back: alloc::vec![0; n] }
    }

    /// Whether both maps are `world`'s size.
    pub fn fits(&self, world: (u32, u32)) -> bool {
        let n = u64::from(world.0) * u64::from(world.1);
        self.world == world && self.front.len() as u64 == n && self.back.len() as u64 == n
    }

    /// Whether no pixel names a material.
    pub fn is_empty(&self) -> bool {
        self.front.iter().chain(&self.back).all(|v| *v == 0)
    }

    /// The largest value of either map (0 when none names a material).
    pub fn highest(&self) -> u8 {
        self.front.iter().chain(&self.back).copied().max().unwrap_or(0)
    }
}
