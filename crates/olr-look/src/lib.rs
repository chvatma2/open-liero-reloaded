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
pub mod glow_beams;
mod material;

pub use anim::{default_phase, splitmix64};
pub use engine::worm_floor_of;
pub use frame::{
    FrameGroup, FrameLight, GroupValues, LookFrame, MAX_EMITTERS, MAX_GROUP_INTENSITY, MAX_GROUP_REACH,
    MAX_OCCLUSION_REACH, MAX_SOAK, MIN_OCCLUSION_REACH, ViewMode, fade, light_at, live_identity,
    srgb_to_linear,
};
pub use material::{
    Heat, LAVA_RAMP, MAX_CAPACITY, MAX_CONDUCTIVITY, MAX_COOLING, MAX_GLOW, MAX_MATERIALS, MAX_RAMP_KEYS,
    MAX_SENSITIVITY, MAX_TEMPERATURE, MIN_CAPACITY, MIN_COOLING, RampKey, SurfaceMaps, SurfaceMaterial,
    ramp_at,
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

/// A level's ambient occlusion (issue #10): air near opaque rock and dirt
/// shows less of the ambient light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Occlusion {
    /// Percent of the ambient light taken away from air right against a
    /// solid, 1-100 (0 is none).
    pub strength: u8,
    /// World pixels over which the darkening fades out,
    /// [`MIN_OCCLUSION_REACH`]-[`MAX_OCCLUSION_REACH`].
    pub reach: u8,
}

/// What colour a glow group's pixels cast (their own glow is always their art).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlowColour {
    /// Each pixel's art colour.
    Own,
    Tint(Rgb),
    /// The colour × each pixel's art colour scaled so its brightest
    /// channel is full (the background glass design).
    Filtered(Rgb),
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

/// A level's look (`level.ron`'s `look`). Its glowing pixels are its only
/// light (plan 15c).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelLook {
    pub ambient: Ambient,
    /// Darkens the ambient light in air near solids; `None` (the default
    /// of a look) is none.
    pub occlusion: Option<Occlusion>,
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
    pub grade: Grade,
    /// A group map value `v` (1-255) is `groups[v - 1]`; 0 is no group.
    pub groups: Vec<GlowGroup>,
    /// Its surface materials (materials design): a surface map value `v`
    /// (1-255) is `materials[v - 1]`; 0 is none.
    pub materials: Vec<SurfaceMaterial>,
    /// Percent, 0-100: how much of the level's air the lights' beams
    /// scatter in (the directional-glow design). 0 is none.
    pub haze: u8,
}

impl LevelLook {
    /// The material surface map value `v` names: `None` for 0 or past the
    /// list.
    pub fn material(&self, v: u8) -> Option<&SurfaceMaterial> {
        self.materials.get(usize::from(v).checked_sub(1)?)
    }
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
    /// Replaces the engine's light of every explosion it makes (issue #15).
    pub explosion: Option<ExplosionLight>,
    /// Scales the engine's heat of every explosion it makes (issue #42).
    pub explosion_heat: Option<ExplosionHeatLook>,
    /// Heat it leaves around itself each tick it lives (issue #42).
    pub heat: Option<ProjectileHeat>,
}

/// A mod's explosion heat (issue #42, spec
/// `2026-10-22-materials-and-heat-design.md`): the engine's heat of an
/// explosion ([`engine::explosion_heat`]) at `amount` percent, over its own
/// radius when it has one. Presentation only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplosionHeatLook {
    /// Percent of the engine's amount, 0 (none) to
    /// [`engine::MAX_EXPLOSION_HEAT_AMOUNT`].
    pub amount: u16,
    /// World pixels, 1 to [`engine::MAX_EXPLOSION_HEAT_RADIUS`]; `None`
    /// keeps the engine's for the explosion.
    pub radius: Option<u8>,
}

impl ExplosionHeatLook {
    /// The heat an explosion of `damage` leaves: the engine's amount at
    /// `amount` percent (clamped to the cap) up to [`MAX_TEMPERATURE`], over
    /// the radius (clamped to 1 to the cap) or the engine's. `None` when the
    /// engine's is none (no damage) or it gives less than a degree.
    pub fn heat(&self, damage: f32) -> Option<engine::ExplosionHeat> {
        let h = engine::explosion_heat(damage)?;
        let share = f32::from(self.amount.min(engine::MAX_EXPLOSION_HEAT_AMOUNT)) / 100.0;
        let amount = (h.amount * share).min(f32::from(MAX_TEMPERATURE));
        let radius = match self.radius {
            Some(r) => f32::from(r.clamp(1, engine::MAX_EXPLOSION_HEAT_RADIUS as u8)),
            None => h.radius,
        };
        (amount >= 1.0).then_some(engine::ExplosionHeat { radius, amount })
    }
}

/// The heat a mod projectile leaves around itself each tick it lives
/// (issue #42: flames, napalm, a hot slug), where it is that tick, its
/// falloff the renderer's. Presentation only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectileHeat {
    /// The temperature a tick gives stone at its centre, 1 to
    /// [`engine::MAX_PROJECTILE_HEAT`].
    pub amount: u16,
    /// World pixels, 1 to [`engine::MAX_PROJECTILE_HEAT_RADIUS`].
    pub radius: u8,
}

impl ProjectileHeat {
    /// A tick's deposit, clamped to the caps.
    pub fn tick(&self) -> engine::ExplosionHeat {
        engine::ExplosionHeat {
            radius: f32::from(self.radius.clamp(1, engine::MAX_PROJECTILE_HEAT_RADIUS)),
            amount: f32::from(self.amount.clamp(1, engine::MAX_PROJECTILE_HEAT)),
        }
    }
}

/// A mod's explosion light (issue #15): replaces the engine's
/// ([`engine::explosion_light`], [`engine::explosion_radius`]) for a
/// projectile's explosions. It shows while the explosion does (its size ×
/// 25 ms).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplosionLight {
    /// Its colour, intensity, reach (`Pixels`, at most
    /// [`engine::MAX_EXPLOSION_REACH`]) and animation.
    pub light: Light,
    /// The glowing disc's radius, world pixels, 1 to
    /// [`engine::MAX_EXPLOSION_RADIUS`].
    pub radius: u8,
    /// The animation's keys span [`engine::EXPLOSION_MS`], fitted to the
    /// explosion's life, as the engine's ([`engine::explosion_flash`]: no
    /// animation given); otherwise its clock is the explosion's age in ms.
    pub fitted: bool,
}

impl ExplosionLight {
    /// The disc's radius, world pixels, clamped to 1 to
    /// [`engine::MAX_EXPLOSION_RADIUS`].
    pub fn radius(&self) -> f32 {
        f32::from(self.radius.clamp(1, engine::MAX_EXPLOSION_RADIUS))
    }

    /// The light's reach, world pixels, at most
    /// [`engine::MAX_EXPLOSION_REACH`] (an `Unlimited` one too).
    pub fn reach(&self) -> f32 {
        self.light.reach.pixels().min(f32::from(engine::MAX_EXPLOSION_REACH))
    }
}

/// A mod weapon's look.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WeaponLook {
    /// Replaces the engine's muzzle light.
    pub muzzle_light: Option<Light>,
    /// A beam weapon's own beam look (issue #69); `None` keeps the
    /// engine's.
    pub beam: Option<BeamLook>,
}

/// A beam weapon's look (issue #69): each part `None` keeps the engine's
/// ([`engine::beam_light`], [`engine::beam_tip_light`],
/// [`engine::BEAM_HEAT`]). Presentation only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BeamLook {
    /// Each glowing disc along the beam (an intensity of 0: none).
    pub light: Option<Light>,
    /// The disc where it stopped on rock, dirt or a worm (an intensity of
    /// 0: none).
    pub tip_light: Option<Light>,
    pub heat: Option<BeamHeat>,
}

/// The heat a beam leaves where it stops on rock or dirt, each physics
/// sub-step (issue #69): falling off as a heating projectile's. Presentation
/// only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeamHeat {
    /// The temperature a sub-step gives stone at its centre, 0 (none) to
    /// [`engine::MAX_PROJECTILE_HEAT`].
    pub amount: u16,
    /// World pixels, 1 to [`engine::MAX_PROJECTILE_HEAT_RADIUS`].
    pub radius: u8,
}

impl BeamHeat {
    /// One of `parts` hits' deposit (a wide beam splits the sub-step's
    /// heat over the parts that stopped on terrain), clamped to the caps;
    /// `None` when it gives less than a degree.
    pub fn part(&self, parts: u8) -> Option<engine::ExplosionHeat> {
        let amount = f32::from(self.amount.min(engine::MAX_PROJECTILE_HEAT)) / f32::from(parts.max(1));
        let radius = f32::from(self.radius.clamp(1, engine::MAX_PROJECTILE_HEAT_RADIUS));
        (amount >= 1.0).then_some(engine::ExplosionHeat { radius, amount })
    }
}

/// A mod's look, indexed like `SimMod::projectiles` and `SimMod::weapons`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModLook {
    pub projectiles: Vec<Option<ProjectileLook>>,
    pub weapons: Vec<Option<WeaponLook>>,
    /// The explosion light of every projectile without its own (issue #15).
    pub explosion: Option<ExplosionLight>,
    /// The explosion heat of every projectile without its own (issue #42).
    pub explosion_heat: Option<ExplosionHeatLook>,
}

impl ModLook {
    /// No projectile or weapon has a look, and there is no mod-wide one.
    pub fn is_empty(&self) -> bool {
        self.projectiles.iter().all(Option::is_none)
            && self.weapons.iter().all(Option::is_none)
            && self.explosion.is_none()
            && self.explosion_heat.is_none()
    }

    /// The explosion light of projectile `proj` (a `SimMod` index; -1 is
    /// none): its own, else the mod's; `None` for the engine's.
    pub fn explosion(&self, proj: i32) -> Option<&ExplosionLight> {
        let i = usize::try_from(proj).ok()?;
        let own = self.projectiles.get(i).and_then(Option::as_ref).and_then(|p| p.explosion.as_ref());
        own.or(self.explosion.as_ref())
    }

    /// The explosion heat of projectile `proj` (a `SimMod` index; -1 is
    /// none: a bonus blown up keeps the engine's): its own, else the
    /// mod's; `None` for the engine's (issue #42).
    pub fn explosion_heat(&self, proj: i32) -> Option<&ExplosionHeatLook> {
        let i = usize::try_from(proj).ok()?;
        let own = self.projectiles.get(i).and_then(Option::as_ref).and_then(|p| p.explosion_heat.as_ref());
        own.or(self.explosion_heat.as_ref())
    }

    /// The heat projectile `proj` (a `SimMod` index) leaves each tick it
    /// lives (issue #42); `None` for most.
    pub fn heat(&self, proj: usize) -> Option<&ProjectileHeat> {
        self.projectiles.get(proj).and_then(Option::as_ref).and_then(|p| p.heat.as_ref())
    }

    /// Weapon `weapon`'s (a `SimMod` index) own beam look (issue #69);
    /// `None` for the engine's.
    pub fn beam(&self, weapon: usize) -> Option<&BeamLook> {
        self.weapons.get(weapon).and_then(Option::as_ref).and_then(|w| w.beam.as_ref())
    }

    /// Weapon `weapon`'s beam heat (issue #69): its own, else the engine's.
    pub fn beam_heat(&self, weapon: usize) -> BeamHeat {
        self.beam(weapon).and_then(|b| b.heat).unwrap_or(engine::BEAM_HEAT)
    }
}

/// FNV-1a 64 of a string: a glow group's identity for its default phase.
pub fn identity_of(id: &str) -> u64 {
    id.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}
