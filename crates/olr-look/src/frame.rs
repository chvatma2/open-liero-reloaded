//! The per-frame look the renderer draws with (spec §Pipeline): the level's
//! settings and groups evaluated at the clock, and play's glowing discs
//! (plan 15c).

use alloc::vec::Vec;

use crate::glow_beams::{GlowBeamSource, MAX_GLOW_BEAMS};
use crate::{
    Animation, GlowColour, GlowGroup, Grade, LevelLook, Light, Occlusion, Reach, default_phase, identity_of,
};

/// The most glowing discs a frame draws (plan 15c): play's lights, in
/// order.
pub const MAX_EMITTERS: usize = 1024;

/// The deepest light soaks into solids, world pixels (spec §Light in solids).
pub const MAX_SOAK: u8 = 12;

/// The shortest ambient occlusion reach, world pixels (issue #10).
pub const MIN_OCCLUSION_REACH: u8 = 2;

/// The longest ambient occlusion reach, world pixels (issue #10): the
/// renderer reads the solid map this far past its light grid.
pub const MAX_OCCLUSION_REACH: u8 = 32;

/// The most a live copy's value sets a group's intensity to, percent (as
/// `olr_format::look::MAX_INTENSITY`).
pub const MAX_GROUP_INTENSITY: u16 = 1000;

/// The longest reach a live copy's value gives a group, world pixels (as
/// `olr_format::look::MAX_REACH`).
pub const MAX_GROUP_REACH: u16 = 4096;

/// A live copy's current values over one of its glow groups (issue #93):
/// each `None` keeps the group's own setting. `colour` is `0xRRGGBB`
/// (negative: the group's own), `intensity` percent, `reach` world pixels,
/// `animation` one of the copy's animation table.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GroupValues<'a> {
    pub colour: Option<i64>,
    pub intensity: Option<i64>,
    pub reach: Option<i64>,
    pub animation: Option<&'a Animation>,
}

/// A group's settings as a frame group is made of them.
struct Parts<'a> {
    colour: GlowColour,
    intensity: u16,
    reach: Reach,
    animation: &'a Animation,
    phase_ms: Option<u32>,
}

/// What the composite shows (the editor's toggle, spec §The level editor).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewMode {
    #[default]
    Lit,
    /// No light: the art as it is.
    FullBright,
    /// The light alone, on white art.
    LightOnly,
}

/// A glow group at this frame. Colours are linear.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameGroup {
    /// Multiplies the cast colour (the pixel's art when `own_colour`, else
    /// already the tint): intensity × animation.
    pub cast: [f32; 3],
    pub own_colour: bool,
    /// With `own_colour`: the pixel's art colour is scaled so its
    /// brightest channel is full, and `cast` holds the group's colour (a
    /// `Filtered` group, the background glass design).
    pub filtered: bool,
    /// The animation's scale of the pixels' self-light.
    pub self_scale: f32,
    /// World pixels; `f32::INFINITY` unlimited.
    pub reach: f32,
    /// A `Flicker` group's speed and depth (percent) (plan 15c): each
    /// separately drawn region of its pixels flickers on its own, so
    /// `cast` and `self_scale` leave the flicker out and the renderer
    /// applies it per region. `None` for every other animation, whose
    /// scale `cast` and `self_scale` hold.
    pub flicker: Option<(f32, f32)>,
    /// The group's clock with its phase, ms (a flicker's).
    pub t_ms: f32,
    /// Seeds a flicker's noise (the group's identity, folded).
    pub seed: u32,
}

/// A glowing disc at this frame (plan 15c): one of play's lights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameLight {
    /// World coordinates.
    pub pos: [f32; 2],
    /// The disc's radius, world pixels.
    pub radius: f32,
    /// Linear, × intensity × animation.
    pub colour: [f32; 3],
    /// World pixels; `f32::INFINITY` unlimited.
    pub reach: f32,
}

/// Everything the renderer's look passes read for one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LookFrame {
    /// Linear.
    pub ambient: [f32; 3],
    /// The level's ambient occlusion (issue #10): strength 1-100, reach
    /// [`MIN_OCCLUSION_REACH`]-[`MAX_OCCLUSION_REACH`]; `None` without
    /// one, a look or a strength.
    pub occlusion: Option<Occlusion>,
    /// 1.0-2.0.
    pub max_light: f32,
    pub light_steps: u8,
    pub softness: u8,
    /// 0-[`MAX_SOAK`].
    pub light_soak: u8,
    /// Linear.
    pub solid_floor: [f32; 3],
    /// The least light of a worm, linear ([`crate::worm_floor_of`] of the
    /// level's ambient colour as set, its intensity ignored; plan 15b).
    pub worm_floor: [f32; 3],
    /// How strongly rock and dirt reflect light (plan 15c):
    /// [`crate::engine::BOUNCE`] in a lit level, 0 without a look.
    pub bounce: f32,
    pub grade: Grade,
    /// Percent, 0-100: how much of the air the glow beams scatter in (the
    /// level's `haze`, the directional-glow design); 0 without a look.
    pub haze: u8,
    /// The level's groups in order: a group map value `v` (1-255) is
    /// `groups[v - 1]`; 0 is no group.
    pub groups: Vec<FrameGroup>,
    /// Play's lights as glowing discs in the glow light (plan 15c): the
    /// engine's explosion and muzzle lights, a mod look's shot lights, end
    /// flashes and muzzle lights. `radius` is the disc's.
    pub emitters: Vec<FrameLight>,
    /// The live copies' glow beams this frame (issue #93), copy and part
    /// order, placed, turned and mirrored as the world shows them: the
    /// renderer packs them after the level's own, at most
    /// [`MAX_GLOW_BEAMS`] in all (past it the live ones drop).
    pub live_beams: Vec<GlowBeamSource>,
    pub mode: ViewMode,
    /// The world state this frame shows (the render profile: the glow
    /// light once a tick): frames with the same `Some` step show the same
    /// world, so the renderer keeps the glow light the first of them made,
    /// discs and flicker as they were then. `None` (`begin`'s: the
    /// editors, previews, tests) makes it every frame.
    pub step: Option<u64>,
}

impl LookFrame {
    /// Clears the frame and fills the level's part at clock `t_ms`: ambient,
    /// grade, settings and groups. A level without a look is full ambient.
    /// Keeps `mode` and the vectors' capacity; `step` is `None`.
    pub fn begin(&mut self, level: Option<&LevelLook>, t_ms: f64) {
        self.groups.clear();
        self.emitters.clear();
        self.live_beams.clear();
        self.step = None;
        let Some(level) = level else {
            self.ambient = [1.0; 3];
            self.occlusion = None;
            self.max_light = 1.0;
            self.light_steps = 0;
            self.softness = 0;
            self.light_soak = 0;
            self.solid_floor = [0.0; 3];
            self.worm_floor = crate::worm_floor_of([255; 3]);
            self.bounce = 0.0;
            self.grade = Grade::default();
            self.haze = 0;
            return;
        };
        let linear = |a: crate::Ambient| {
            let k = f32::from(a.intensity.min(100)) / 100.0;
            a.colour.map(|c| srgb_to_linear(c) * k)
        };
        self.ambient = linear(level.ambient);
        self.occlusion = level.occlusion.filter(|o| o.strength > 0).map(|o| Occlusion {
            strength: o.strength.min(100),
            reach: o.reach.clamp(MIN_OCCLUSION_REACH, MAX_OCCLUSION_REACH),
        });
        self.light_soak = level.light_soak.min(MAX_SOAK);
        self.solid_floor = linear(level.solid_floor);
        self.worm_floor = crate::worm_floor_of(level.ambient.colour);
        self.bounce = crate::engine::BOUNCE;
        self.max_light = (f32::from(level.max_light) / 100.0).clamp(1.0, 2.0);
        self.light_steps = level.light_steps;
        self.softness = level.softness.min(4);
        self.grade = level.grade;
        self.haze = level.haze.min(100);
        for g in &level.groups {
            self.push_group(g, identity_of(&g.id), t_ms);
        }
    }

    /// Appends live copy `copy`'s glow group `g` after the groups so far
    /// (issue #87, call 13: each live copy's groups follow the level's, in
    /// copy order, then in the copy's group order, so a group map value is
    /// `n + offset(copy) + g`), made as the level's are at clock `t_ms`.
    /// Its identity (flicker seed, default phase) is
    /// `identity_of("<copy>/<group id>")`, so two copies of one asset
    /// flicker and pulse apart. A group past the 255th (no group map value
    /// names it) is left out. Allocates nothing once the vector is warm.
    pub fn push_live_group(&mut self, g: &GlowGroup, copy: &str, t_ms: f64) {
        self.push_live_group_with(g, copy, t_ms, GroupValues::default());
    }

    /// [`LookFrame::push_live_group`] with the copy's current values `v`
    /// over the group (issue #93, as `olr_edit`'s `posed` applies them to
    /// a static copy): a colour casts it as a tint (a `Filtered` group
    /// stays filtered, in it; a negative colour is the group's own), an
    /// intensity (clamped to 0-1000) and a reach (1-4096, only over a
    /// reach in pixels) replace the group's, and an animation replaces its
    /// animation at the copy's clock and phase (its live identity's, so a
    /// switch never brings two copies into step). Allocates nothing once
    /// the vector is warm.
    pub fn push_live_group_with(&mut self, g: &GlowGroup, copy: &str, t_ms: f64, v: GroupValues<'_>) {
        if self.groups.len() >= usize::from(u8::MAX) {
            return;
        }
        let colour = match (v.colour, g.colour) {
            (Some(c), own) if c >= 0 => {
                let c = c.min(0xff_ffff);
                let rgb = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                match own {
                    GlowColour::Filtered(_) => GlowColour::Filtered(rgb),
                    _ => GlowColour::Tint(rgb),
                }
            }
            _ => g.colour,
        };
        let intensity =
            v.intensity.map_or(g.intensity, |i| i.clamp(0, i64::from(MAX_GROUP_INTENSITY)) as u16);
        let reach = match (v.reach, g.reach) {
            (Some(r), Reach::Pixels(_)) => Reach::Pixels(r.clamp(1, i64::from(MAX_GROUP_REACH)) as u16),
            _ => g.reach,
        };
        let animation = v.animation.unwrap_or(&g.animation);
        let identity = live_identity(copy, &g.id);
        self.push_parts(Parts { colour, intensity, reach, animation, phase_ms: g.phase_ms }, identity, t_ms);
    }

    /// Pushes group `g` of identity `identity` at clock `t_ms`.
    fn push_group(&mut self, g: &GlowGroup, identity: u64, t_ms: f64) {
        let parts = Parts {
            colour: g.colour,
            intensity: g.intensity,
            reach: g.reach,
            animation: &g.animation,
            phase_ms: g.phase_ms,
        };
        self.push_parts(parts, identity, t_ms);
    }

    /// Pushes a group of `g`'s settings and identity `identity` at clock
    /// `t_ms`.
    fn push_parts(&mut self, g: Parts<'_>, identity: u64, t_ms: f64) {
        let phase = g.phase_ms.unwrap_or_else(|| phase_for(g.animation, identity));
        // A flicker is the renderer's, per region (plan 15c).
        let flicker = match *g.animation {
            Animation::Flicker { speed, depth } => Some((f32::from(speed.max(1)), f32::from(depth.min(100)))),
            _ => None,
        };
        let s = match flicker {
            Some(_) => crate::anim::STEADY_SAMPLE,
            None => g.animation.sample(t_ms, phase, identity),
        };
        let k = f32::from(g.intensity) / 100.0 * s.scale;
        // The flicker's clock, kept within f32's whole milliseconds.
        let clock = t_ms + f64::from(phase);
        let wrap = 16_777_216.0;
        let clock = if clock.is_finite() { (clock - libm::floor(clock / wrap) * wrap) as f32 } else { 0.0 };
        let (own, filtered, base) = match (s.colour, g.colour) {
            (Some(c), _) | (None, crate::GlowColour::Tint(c)) => (false, false, c.map(srgb_to_linear)),
            (None, crate::GlowColour::Own) => (true, false, [1.0; 3]),
            (None, crate::GlowColour::Filtered(c)) => (true, true, c.map(srgb_to_linear)),
        };
        self.groups.push(FrameGroup {
            cast: base.map(|c| c * k),
            own_colour: own,
            filtered,
            self_scale: s.scale,
            reach: g.reach.pixels(),
            flicker,
            t_ms: clock,
            seed: (identity ^ (identity >> 32)) as u32,
        });
    }

    /// Adds a live copy's glow beam after the others (issue #93), while
    /// there are fewer than [`MAX_GLOW_BEAMS`] (no more can be packed).
    pub fn push_live_beam(&mut self, beam: GlowBeamSource) {
        if self.live_beams.len() < MAX_GLOW_BEAMS {
            self.live_beams.push(beam);
        }
    }

    /// Adds a glowing disc after the others (plan 15c; the callers' order),
    /// while there are fewer than [`MAX_EMITTERS`]. Discs are never culled
    /// to a view: light bounced off a wall may reach the view from past a
    /// disc's reach, so the light stays camera-independent.
    pub fn push_emitter(&mut self, light: FrameLight) {
        if self.emitters.len() < MAX_EMITTERS {
            self.emitters.push(light);
        }
    }
}

/// [`identity_of`] of `"<copy>/<group>"`, without making the string.
pub fn live_identity(copy: &str, group: &str) -> u64 {
    let fold = |h: u64, b: u8| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    let h = copy.bytes().fold(0xcbf2_9ce4_8422_2325, fold);
    group.bytes().fold(fold(h, b'/'), fold)
}

/// A looping animation's default phase; a curve that plays once starts at its start.
fn phase_for(animation: &Animation, identity: u64) -> u32 {
    if animation.loops() { default_phase(identity) } else { 0 }
}

/// `light` at clock `t_ms` (its own clock) as a frame light at `pos`, or
/// `None` when it has ended or gives nothing.
pub fn light_at(light: &Light, t_ms: f64, identity: u64, pos: [f32; 2], radius: f32) -> Option<FrameLight> {
    let phase = light.phase_ms.unwrap_or_else(|| phase_for(&light.animation, identity));
    let t = t_ms + f64::from(phase);
    if light.animation.ended(t) {
        return None;
    }
    let s = light.animation.sample(t_ms, phase, identity);
    let k = f32::from(light.intensity.min(1000)) / 100.0 * s.scale;
    let colour = s.colour.unwrap_or(light.colour).map(|c| srgb_to_linear(c) * k);
    let lit = colour.iter().any(|c| *c > 0.0) && colour.iter().all(|c| c.is_finite());
    (lit && pos.iter().all(|p| p.is_finite())).then_some(FrameLight {
        pos,
        radius,
        colour,
        reach: light.reach.pixels(),
    })
}

/// How much of a light reaches `distance` world pixels: `(1 - (d/r)²)²`,
/// nothing at or past the reach; [`Reach::Unlimited`] never fades.
pub fn fade(distance: f32, reach: Reach) -> f32 {
    match reach {
        Reach::Unlimited => 1.0,
        Reach::Pixels(0) => 0.0,
        Reach::Pixels(r) => {
            let x = distance / f32::from(r);
            if x.is_nan() || x >= 1.0 {
                return 0.0;
            }
            let v = 1.0 - x * x;
            v * v
        }
    }
}

/// An sRGB byte as linear light.
pub fn srgb_to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { libm::powf((c + 0.055) / 1.055, 2.4) }
}
