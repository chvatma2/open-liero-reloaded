//! The look's runtime data (spec `2026-10-18-look-system-design.md`): what a
//! level's and a mod's look files compile to, animation sampling, the
//! per-frame [`LookFrame`] the renderer draws with, and the engine's own look
//! ([`engine`]). The simulation never sees any of it.
//!
//! `#![no_std]` + `alloc`, and wasm-clean (CLAUDE.md rule 5).
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

mod anim;
pub mod engine;
mod frame;

pub use anim::{default_phase, splitmix64};
pub use engine::worm_floor_of;
pub use frame::{
    FrameGroup, FrameLight, LookFrame, MAX_EMITTERS, MAX_LIGHTS, MAX_SOAK, ViewMode, fade, light_at,
    srgb_to_linear,
};

/// An sRGB colour, as the files and the art store it.
pub type Rgb = [u8; 3];

/// How far a light reaches: it fades to nothing there (spec §Light sources).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// World pixels.
    Pixels(u16),
    /// No fade.
    Unlimited,
}

impl Reach {
    /// World pixels; `f32::INFINITY` for [`Reach::Unlimited`].
    pub fn pixels(self) -> f32 {
        match self {
            Reach::Pixels(n) => f32::from(n),
            Reach::Unlimited => f32::INFINITY,
        }
    }
}

/// The wave of a [`Animation::Pulse`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PulseShape {
    Sine,
    Triangle,
    Square,
}

/// One key of a [`Animation::Keys`] curve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    /// Milliseconds from the curve's start.
    pub at_ms: u32,
    /// Percent of the light's intensity.
    pub level: u16,
    /// The light's colour from this key on, when given.
    pub colour: Option<Rgb>,
}

/// How a light's intensity changes over time (spec §Animation). Levels are
/// percent, times milliseconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Animation {
    Steady,
    /// Between `low` and `high` over `period_ms`, starting at the midpoint
    /// and rising.
    Pulse {
        period_ms: u32,
        low: u16,
        high: u16,
        shape: PulseShape,
    },
    /// Smooth value noise: `speed` changes a second, dimming by up to `depth` percent.
    Flicker {
        speed: u16,
        depth: u16,
    },
    /// A hand-made curve, linear between keys.
    Keys {
        keys: Vec<Key>,
        looped: bool,
    },
}

/// One light source's fields (spec §Light sources: one shape everywhere).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Light {
    pub colour: Rgb,
    /// Percent, 0-1000.
    pub intensity: u16,
    pub reach: Reach,
    pub animation: Animation,
    /// Shifts the clock; by default derived from the source's identity
    /// (looping animations only: a flash starts at its start).
    pub phase_ms: Option<u32>,
}

/// A level's colour grade: exposure in 1/100 stop, contrast and saturation in
/// percent, a tint multiplied in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grade {
    pub exposure: i16,
    pub contrast: u16,
    pub saturation: u16,
    pub tint: Rgb,
}

impl Default for Grade {
    fn default() -> Grade {
        Grade { exposure: 0, contrast: 100, saturation: 100, tint: [255, 255, 255] }
    }
}

/// The light everywhere, unblocked: a colour at `intensity` percent (0-100).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ambient {
    pub colour: Rgb,
    pub intensity: u16,
}

/// What colour a glow group's pixels cast (their own glow is always their art).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlowColour {
    /// Each pixel's art colour.
    Own,
    Tint(Rgb),
}

/// A level's group of glowing pixels (spec §Glow): how they light the world
/// and how they animate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlowGroup {
    pub id: String,
    pub colour: GlowColour,
    /// Percent of the glowing surface's brightness, 0-1000.
    pub intensity: u16,
    pub reach: Reach,
    pub animation: Animation,
    pub phase_ms: Option<u32>,
}

/// A light object placed in a level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlacedLight {
    /// World pixel.
    pub at: (i32, i32),
    /// The source's size in world pixels, 1-16.
    pub radius: u8,
    pub light: Light,
}

/// How placed lights' shadows meet the light (the user's choice after the
/// playtest, 2026-10-19: soft by default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shadows {
    /// Cut at whole world pixels.
    Hard,
    /// Anti-aliased: four tests a world pixel.
    Smooth,
    /// A penumbra from the light's radius, wider farther behind the occluder.
    #[default]
    Soft,
}

/// A level's look (`level.ron`'s `look` and its light objects).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelLook {
    pub ambient: Ambient,
    /// Percent, 100-200.
    pub max_light: u16,
    /// 0 off, else 2-32 bands.
    pub light_steps: u8,
    /// World pixels of blur, 0-4.
    pub softness: u8,
    /// World pixels light soaks into solids, 0-[`MAX_SOAK`].
    pub light_soak: u8,
    /// The least light an opaque solid pixel shows.
    pub solid_floor: Ambient,
    pub shadows: Shadows,
    pub grade: Grade,
    /// A group map value `v` (1-255) is `groups[v - 1]`; 0 is no group.
    pub groups: Vec<GlowGroup>,
    pub lights: Vec<PlacedLight>,
}

/// A mod projectile's look.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectileLook {
    /// Self-light of its sprite or pixels, percent.
    pub glow: Option<u8>,
    /// Follows the drawn projectile; its clock is the projectile's age.
    pub light: Option<Light>,
    /// Where the projectile ended; its clock starts there.
    pub end_flash: Option<Light>,
}

/// A mod weapon's look.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WeaponLook {
    /// Replaces the engine's muzzle light.
    pub muzzle_light: Option<Light>,
}

/// A mod's look, indexed like `SimMod::projectiles` and `SimMod::weapons`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModLook {
    pub projectiles: Vec<Option<ProjectileLook>>,
    pub weapons: Vec<Option<WeaponLook>>,
}

impl ModLook {
    /// No projectile or weapon has a look.
    pub fn is_empty(&self) -> bool {
        self.projectiles.iter().all(Option::is_none) && self.weapons.iter().all(Option::is_none)
    }
}

/// FNV-1a 64 of a string: a glow group's identity for its default phase.
pub fn identity_of(id: &str) -> u64 {
    id.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}
