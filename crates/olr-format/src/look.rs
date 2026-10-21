//! The look's authoring types (spec `2026-10-18-look-system-design.md`
//! §Data), shared by levels (`level.ron`'s `look`) and
//! mods (a projectile's or weapon's `look`), and their compilers to
//! `olr-look`'s runtime types. Every value is an integer: percent,
//! milliseconds, world pixels.

use olr_data::image::Rgba;
use serde::de::{self, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::defaults as d;
use crate::{Colour, FormatError};

/// The largest light or glow intensity, percent.
pub const MAX_INTENSITY: u16 = 1000;
/// The largest `Pixels` reach, world pixels.
pub const MAX_REACH: u16 = 4096;
/// The longest `Pulse` period, milliseconds.
pub const MAX_PERIOD_MS: u32 = 600_000;
/// The most keys of a `Keys` animation.
pub const MAX_KEYS: usize = 64;
/// The most glow groups of a level (a group index is one byte).
pub const MAX_GROUPS: usize = 255;

/// How far a light reaches: it fades to nothing there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReachSource {
    /// World pixels, 1 to 4096.
    Pixels(u16),
    /// No fade (in a level, glow light reaches across the whole level).
    Unlimited,
}

/// The wave of a `Pulse`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PulseShape {
    /// Smooth.
    #[default]
    Sine,
    /// Linear up and down.
    Triangle,
    /// `high` for the first half of the period, `low` for the second.
    Square,
}

/// One key of a `Keys` animation, written `(at_ms, level)` or
/// `(at_ms, level, "#rrggbb")`: milliseconds from the curve's start, the
/// intensity in percent (0 to 1000), and the light's colour from this key on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySource {
    pub at_ms: u32,
    pub level: u16,
    pub colour: Option<Colour>,
}

impl Serialize for KeySource {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut t = s.serialize_tuple(if self.colour.is_some() { 3 } else { 2 })?;
        t.serialize_element(&self.at_ms)?;
        t.serialize_element(&self.level)?;
        if let Some(c) = &self.colour {
            t.serialize_element(c)?;
        }
        t.end()
    }
}

impl<'de> Deserialize<'de> for KeySource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = KeySource;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a key: (at_ms, level) or (at_ms, level, \"#rrggbb\")")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<KeySource, A::Error> {
                let at_ms = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let level = seq.next_element()?.ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let colour = seq.next_element()?;
                if seq.next_element::<de::IgnoredAny>()?.is_some() {
                    return Err(de::Error::invalid_length(4, &self));
                }
                Ok(KeySource { at_ms, level, colour })
            }
        }
        d.deserialize_tuple(3, V)
    }
}

/// How a light's intensity changes over time. Levels are percent of its
/// intensity (0 to 1000), times milliseconds. The clock is the match's (a
/// mod's light: the projectile's age, or the time since its flash began).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimationSource {
    /// Constant.
    #[default]
    Steady,
    /// Between `low` and `high` over `period_ms` (1 to 600 000), starting at
    /// the midpoint and rising.
    Pulse {
        /// Milliseconds, 1 to 600 000.
        period_ms: u32,
        /// Percent, 0 to 1000.
        low: u16,
        /// Percent, 0 to 1000.
        high: u16,
        /// The wave. Default `Sine`.
        #[serde(default, skip_serializing_if = "d::is_default")]
        shape: PulseShape,
    },
    /// A flame's unsteadiness: smooth noise, the same on every machine.
    Flicker {
        /// Changes a second, 1 to 1000.
        speed: u16,
        /// How far it dims, percent, 0 to 100.
        depth: u16,
    },
    /// A hand-made curve, linear between keys.
    Keys {
        /// 1 to 64 keys, their times not decreasing.
        keys: Vec<KeySource>,
        /// Repeats after the last key; otherwise the light ends there (a
        /// flash). Default false.
        #[serde(default, skip_serializing_if = "d::is_false")]
        looped: bool,
    },
}

/// A mod's light: a projectile's light or flash, a weapon's muzzle light.
/// Play draws it as a glowing disc (plan 15c).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LightSource {
    /// Opaque.
    pub colour: Colour,
    /// The light at the source, percent, 0 to 1000.
    pub intensity: u16,
    /// Where it fades to nothing.
    pub reach: ReachSource,
    /// Default `Steady`.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub animation: AnimationSource,
    /// Shifts the animation's clock, milliseconds; by default a looping
    /// animation's phase comes from the light's identity, so equal lights
    /// never pulse together. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_ms: Option<u32>,
}

/// The light everywhere, unblocked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmbientSource {
    /// Opaque. Default none: white.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
    /// Percent, 0 to 100. Default 100.
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    pub intensity: u16,
}

impl Default for AmbientSource {
    fn default() -> AmbientSource {
        AmbientSource { colour: None, intensity: 100 }
    }
}

/// Ambient occlusion (issue #10): air near opaque rock and dirt shows less
/// of the ambient light, so tunnels, corners and pockets read as deep. Only
/// the ambient light is darkened (not glow light or flashes) and only in air;
/// the worm and solid floors still hold. Outside the level is open air.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OcclusionSource {
    /// How much of the ambient light is taken away from air right against a
    /// solid (a flat wall's face; deeper in a corner or a narrow tunnel),
    /// percent, 1 to 100 (100: none of it is left there).
    pub strength: u16,
    /// How far from a solid the darkening fades out to nothing, world
    /// pixels, 2 to 32. Default 12.
    #[serde(default = "occlusion_reach", skip_serializing_if = "is_occlusion_reach")]
    pub reach: u16,
}

/// The occlusion's default reach, world pixels.
fn occlusion_reach() -> u16 {
    DEFAULT_OCCLUSION_REACH
}

fn is_occlusion_reach(v: &u16) -> bool {
    *v == DEFAULT_OCCLUSION_REACH
}

/// The occlusion's default reach, world pixels (issue #10).
pub const DEFAULT_OCCLUSION_REACH: u16 = 12;

/// The least light an opaque solid pixel (rock or dirt that is not clear)
/// shows, so deep rock still shows its art.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolidFloorSource {
    /// Opaque. Default none: white.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<Colour>,
    /// Percent, 0 to 100 (0: none). Default 30 (plan 15a Stop 1b).
    #[serde(default = "solid_floor_intensity", skip_serializing_if = "is_solid_floor_intensity")]
    pub intensity: u16,
}

impl Default for SolidFloorSource {
    fn default() -> SolidFloorSource {
        SolidFloorSource { colour: None, intensity: solid_floor_intensity() }
    }
}

/// The solid floor's default intensity, percent (plan 15a Stop 1b).
fn solid_floor_intensity() -> u16 {
    30
}

fn is_solid_floor_intensity(v: &u16) -> bool {
    *v == solid_floor_intensity()
}

/// The default light soak, world pixels (plan 15a Stop 1b).
fn light_soak() -> u8 {
    8
}

fn is_light_soak(v: &u8) -> bool {
    *v == light_soak()
}

fn hundred() -> u16 {
    100
}

fn is_hundred(v: &u16) -> bool {
    *v == 100
}

fn white() -> Colour {
    Colour(Rgba::rgb(255, 255, 255))
}

/// A level's colour grade, applied to the lit world (not the HUD).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradeSource {
    /// Hundredths of a stop, -200 to 200. Default 0.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub exposure: i16,
    /// Percent, 0 to 200. Default 100.
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    pub contrast: u16,
    /// Percent, 0 to 200. Default 100.
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    pub saturation: u16,
    /// Multiplied in; opaque. Default none: white.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<Colour>,
}

impl Default for GradeSource {
    fn default() -> GradeSource {
        GradeSource { exposure: 0, contrast: 100, saturation: 100, tint: None }
    }
}

/// What colour a glow group's pixels cast (their own glow always shows their art).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlowColourSource {
    /// Each pixel's art colour.
    #[default]
    Own,
    /// One colour; opaque.
    Tint(Colour),
}

/// A group of glowing pixels: how they light the world and animate. The
/// pixels name it in `look/groups.png` by 1 + its index (0 is no group).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlowGroupSource {
    /// A slug unique in the level (`[a-z0-9_.-]`); the editor's elements name it.
    pub id: String,
    /// Default `Own`.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub colour: GlowColourSource,
    /// The glowing surface's brightness, percent, 0 to 1000. Default 100.
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    pub intensity: u16,
    /// How far its light reaches; `Unlimited` lights the whole level (a sun).
    pub reach: ReachSource,
    /// Default `Steady`.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub animation: AnimationSource,
    /// Default none: from the group's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_ms: Option<u32>,
}

/// A level's look: darkness, light settings, grade and glow groups. A level
/// without one draws as it always has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookSource {
    /// Default: white at 100 (no darkness).
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub ambient: AmbientSource,
    /// Darkens the ambient light in air near rock and dirt (issue #10).
    /// Default none: no occlusion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub occlusion: Option<OcclusionSource>,
    /// The brightest light, percent, 100 to 200 (above 100, flashes brighten a
    /// lit level). Default 100.
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    pub max_light: u16,
    /// Bands the light: 0 (off) or 2 to 32. Default 0.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub light_steps: u8,
    /// Blurs the light, world pixels, 0 to 4. Default 0.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub softness: u8,
    /// How deep light soaks into solids from the air beside them, world
    /// pixels, 0 to 12, fading with depth. Default 8.
    #[serde(default = "light_soak", skip_serializing_if = "is_light_soak")]
    pub light_soak: u8,
    /// The least light of an opaque solid. Default white at 30.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub solid_floor: SolidFloorSource,
    /// Default none.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub grade: GradeSource,
    /// At most 255. Default none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub glow_groups: Vec<GlowGroupSource>,
}

impl Default for LookSource {
    /// Every field at its serde default: no darkness, nothing else.
    fn default() -> LookSource {
        LookSource {
            ambient: AmbientSource::default(),
            occlusion: None,
            max_light: 100,
            light_steps: 0,
            softness: 0,
            light_soak: light_soak(),
            solid_floor: SolidFloorSource::default(),
            grade: GradeSource::default(),
            glow_groups: Vec::new(),
        }
    }
}

/// A mod projectile's look.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectileLookSource {
    /// How bright its sprite or pixels show whatever the darkness, percent,
    /// 0 to 100. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glow: Option<u8>,
    /// A light following it; its clock is the projectile's age. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<LightSource>,
    /// A light where it ended; its clock starts there. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_flash: Option<LightSource>,
}

/// A mod weapon's look.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponLookSource {
    /// Replaces the engine's muzzle-flash light. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muzzle_light: Option<LightSource>,
}

fn invalid(file: &str, path: &str, message: impl Into<String>) -> FormatError {
    FormatError::invalid(file, path, message)
}

fn rgb(c: Colour, file: &str, path: &str) -> Result<olr_look::Rgb, FormatError> {
    if c.0.a != 255 {
        return Err(invalid(file, path, format!("{} must be opaque", c.to_hex())));
    }
    Ok([c.0.r, c.0.g, c.0.b])
}

fn within(v: u32, lo: u32, hi: u32, file: &str, path: &str) -> Result<(), FormatError> {
    if (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(invalid(file, path, format!("{v}, not within {lo} to {hi}")))
    }
}

/// A reach, checked.
pub fn compile_reach(r: ReachSource, file: &str, path: &str) -> Result<olr_look::Reach, FormatError> {
    Ok(match r {
        ReachSource::Pixels(n) => {
            within(n.into(), 1, MAX_REACH.into(), file, path)?;
            olr_look::Reach::Pixels(n)
        }
        ReachSource::Unlimited => olr_look::Reach::Unlimited,
    })
}

/// An animation, checked.
pub fn compile_animation(
    a: &AnimationSource,
    file: &str,
    path: &str,
) -> Result<olr_look::Animation, FormatError> {
    let level = |v: u16, p: &str| within(v.into(), 0, MAX_INTENSITY.into(), file, &format!("{path}.{p}"));
    Ok(match a {
        AnimationSource::Steady => olr_look::Animation::Steady,
        AnimationSource::Pulse { period_ms, low, high, shape } => {
            within(*period_ms, 1, MAX_PERIOD_MS, file, &format!("{path}.period_ms"))?;
            level(*low, "low")?;
            level(*high, "high")?;
            let shape = match shape {
                PulseShape::Sine => olr_look::PulseShape::Sine,
                PulseShape::Triangle => olr_look::PulseShape::Triangle,
                PulseShape::Square => olr_look::PulseShape::Square,
            };
            olr_look::Animation::Pulse { period_ms: *period_ms, low: *low, high: *high, shape }
        }
        AnimationSource::Flicker { speed, depth } => {
            within((*speed).into(), 1, 1000, file, &format!("{path}.speed"))?;
            within((*depth).into(), 0, 100, file, &format!("{path}.depth"))?;
            olr_look::Animation::Flicker { speed: *speed, depth: *depth }
        }
        AnimationSource::Keys { keys, looped } => {
            if keys.is_empty() || keys.len() > MAX_KEYS {
                return Err(invalid(
                    file,
                    &format!("{path}.keys"),
                    format!("{} keys, not within 1 to {MAX_KEYS}", keys.len()),
                ));
            }
            let mut out = Vec::with_capacity(keys.len());
            for (i, k) in keys.iter().enumerate() {
                let p = format!("{path}.keys[{i}]");
                within(k.level.into(), 0, MAX_INTENSITY.into(), file, &p)?;
                if i > 0 && k.at_ms < keys[i - 1].at_ms {
                    return Err(invalid(file, &p, format!("{} ms comes before the key before it", k.at_ms)));
                }
                let colour = k.colour.map(|c| rgb(c, file, &p)).transpose()?;
                out.push(olr_look::Key { at_ms: k.at_ms, level: k.level, colour });
            }
            olr_look::Animation::Keys { keys: out, looped: *looped }
        }
    })
}

/// A light, checked.
pub fn compile_light(l: &LightSource, file: &str, path: &str) -> Result<olr_look::Light, FormatError> {
    within(l.intensity.into(), 0, MAX_INTENSITY.into(), file, &format!("{path}.intensity"))?;
    Ok(olr_look::Light {
        colour: rgb(l.colour, file, &format!("{path}.colour"))?,
        intensity: l.intensity,
        reach: compile_reach(l.reach, file, &format!("{path}.reach"))?,
        animation: compile_animation(&l.animation, file, &format!("{path}.animation"))?,
        phase_ms: l.phase_ms,
    })
}

/// A glow group's id: a slug of `[a-z0-9_.-]`, 1 to 64 bytes, not starting with `.`.
pub fn valid_group_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('.')
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-'))
}

/// A level's look without its lights, checked.
pub fn compile_look_source(l: &LookSource, file: &str) -> Result<olr_look::LevelLook, FormatError> {
    within(l.ambient.intensity.into(), 0, 100, file, "look.ambient.intensity")?;
    if let Some(o) = &l.occlusion {
        within(o.strength.into(), 1, 100, file, "look.occlusion.strength")?;
        within(
            o.reach.into(),
            olr_look::MIN_OCCLUSION_REACH.into(),
            olr_look::MAX_OCCLUSION_REACH.into(),
            file,
            "look.occlusion.reach",
        )?;
    }
    within(l.max_light.into(), 100, 200, file, "look.max_light")?;
    if l.light_steps == 1 || l.light_steps > 32 {
        return Err(invalid(file, "look.light_steps", format!("{}, not 0 or within 2 to 32", l.light_steps)));
    }
    within(l.softness.into(), 0, 4, file, "look.softness")?;
    within(l.light_soak.into(), 0, olr_look::MAX_SOAK.into(), file, "look.light_soak")?;
    within(l.solid_floor.intensity.into(), 0, 100, file, "look.solid_floor.intensity")?;
    let g = &l.grade;
    if !(-200..=200).contains(&g.exposure) {
        return Err(invalid(file, "look.grade.exposure", format!("{}, not within -200 to 200", g.exposure)));
    }
    within(g.contrast.into(), 0, 200, file, "look.grade.contrast")?;
    within(g.saturation.into(), 0, 200, file, "look.grade.saturation")?;
    if l.glow_groups.len() > MAX_GROUPS {
        return Err(invalid(
            file,
            "look.glow_groups",
            format!("{} groups, above {MAX_GROUPS}", l.glow_groups.len()),
        ));
    }
    let mut groups = Vec::with_capacity(l.glow_groups.len());
    for (i, gr) in l.glow_groups.iter().enumerate() {
        let p = format!("look.glow_groups[{i}]");
        if !valid_group_id(&gr.id) {
            return Err(invalid(
                file,
                &format!("{p}.id"),
                format!("{:?} is not a slug of [a-z0-9_.-]", gr.id),
            ));
        }
        if l.glow_groups[..i].iter().any(|o| o.id == gr.id) {
            return Err(invalid(file, &format!("{p}.id"), format!("{:?} is used twice", gr.id)));
        }
        within(gr.intensity.into(), 0, MAX_INTENSITY.into(), file, &format!("{p}.intensity"))?;
        let colour = match gr.colour {
            GlowColourSource::Own => olr_look::GlowColour::Own,
            GlowColourSource::Tint(c) => olr_look::GlowColour::Tint(rgb(c, file, &format!("{p}.colour"))?),
        };
        groups.push(olr_look::GlowGroup {
            id: gr.id.clone(),
            colour,
            intensity: gr.intensity,
            reach: compile_reach(gr.reach, file, &format!("{p}.reach"))?,
            animation: compile_animation(&gr.animation, file, &format!("{p}.animation"))?,
            phase_ms: gr.phase_ms,
        });
    }
    Ok(olr_look::LevelLook {
        ambient: olr_look::Ambient {
            colour: rgb(l.ambient.colour.unwrap_or_else(white), file, "look.ambient.colour")?,
            intensity: l.ambient.intensity,
        },
        occlusion: l
            .occlusion
            .map(|o| olr_look::Occlusion { strength: o.strength as u8, reach: o.reach as u8 }),
        max_light: l.max_light,
        light_steps: l.light_steps,
        softness: l.softness,
        light_soak: l.light_soak,
        solid_floor: olr_look::Ambient {
            colour: rgb(l.solid_floor.colour.unwrap_or_else(white), file, "look.solid_floor.colour")?,
            intensity: l.solid_floor.intensity,
        },
        grade: olr_look::Grade {
            exposure: g.exposure,
            contrast: g.contrast,
            saturation: g.saturation,
            tint: rgb(g.tint.unwrap_or_else(white), file, "look.grade.tint")?,
        },
        groups,
    })
}

/// A projectile's look, checked; `path` names it in errors.
pub fn compile_projectile_look(
    l: &ProjectileLookSource,
    file: &str,
    path: &str,
) -> Result<olr_look::ProjectileLook, FormatError> {
    if let Some(g) = l.glow {
        within(g.into(), 0, 100, file, &format!("{path}.glow"))?;
    }
    Ok(olr_look::ProjectileLook {
        glow: l.glow,
        light: l.light.as_ref().map(|x| compile_light(x, file, &format!("{path}.light"))).transpose()?,
        end_flash: l
            .end_flash
            .as_ref()
            .map(|x| compile_light(x, file, &format!("{path}.end_flash")))
            .transpose()?,
    })
}

/// A weapon's look, checked.
pub fn compile_weapon_look(
    l: &WeaponLookSource,
    file: &str,
    path: &str,
) -> Result<olr_look::WeaponLook, FormatError> {
    Ok(olr_look::WeaponLook {
        muzzle_light: l
            .muzzle_light
            .as_ref()
            .map(|x| compile_light(x, file, &format!("{path}.muzzle_light")))
            .transpose()?,
    })
}
