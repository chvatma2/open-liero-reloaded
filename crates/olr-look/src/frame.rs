//! The per-frame look the renderer draws with (spec §Pipeline): the level's
//! settings and groups evaluated at the clock, and the placed lights.

use alloc::vec::Vec;

use crate::{Grade, LevelLook, Light, Reach, default_phase, identity_of};

/// The most placed lights a frame draws (plan 15a R13).
pub const MAX_LIGHTS: usize = 256;

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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameGroup {
    /// Multiplies the cast colour (the pixel's art when `own_colour`, else
    /// already the tint): intensity × animation.
    pub cast: [f32; 3],
    pub own_colour: bool,
    /// The animation's scale of the pixels' self-light.
    pub self_scale: f32,
    /// World pixels; `f32::INFINITY` unlimited.
    pub reach: f32,
}

/// A placed light at this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameLight {
    /// World coordinates.
    pub pos: [f32; 2],
    /// World pixels: full intensity within it; shadow rays start at its edge.
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
    /// 1.0-2.0.
    pub max_light: f32,
    pub light_steps: u8,
    pub softness: u8,
    pub grade: Grade,
    /// The level's groups in order: a group map value `v` (1-255) is
    /// `groups[v - 1]`; 0 is no group.
    pub groups: Vec<FrameGroup>,
    pub lights: Vec<FrameLight>,
    pub mode: ViewMode,
}

impl LookFrame {
    /// Clears the frame and fills the level's part at clock `t_ms`: ambient,
    /// grade, settings, groups and the level's lights. A level without a look
    /// is full ambient. Keeps `mode` and the vectors' capacity.
    pub fn begin(&mut self, level: Option<&LevelLook>, t_ms: f64) {
        self.groups.clear();
        self.lights.clear();
        let Some(level) = level else {
            self.ambient = [1.0; 3];
            self.max_light = 1.0;
            self.light_steps = 0;
            self.softness = 0;
            self.grade = Grade::default();
            return;
        };
        let a = level.ambient;
        let k = f32::from(a.intensity.min(100)) / 100.0;
        self.ambient = a.colour.map(|c| srgb_to_linear(c) * k);
        self.max_light = (f32::from(level.max_light) / 100.0).clamp(1.0, 2.0);
        self.light_steps = level.light_steps;
        self.softness = level.softness.min(4);
        self.grade = level.grade;
        for g in &level.groups {
            let identity = identity_of(&g.id);
            let phase = g.phase_ms.unwrap_or_else(|| phase_for(&g.animation, identity));
            let s = g.animation.sample(t_ms, phase, identity);
            let k = f32::from(g.intensity) / 100.0 * s.scale;
            let (own, base) = match (s.colour, g.colour) {
                (Some(c), _) | (None, crate::GlowColour::Tint(c)) => (false, c.map(srgb_to_linear)),
                (None, crate::GlowColour::Own) => (true, [1.0; 3]),
            };
            self.groups.push(FrameGroup {
                cast: base.map(|c| c * k),
                own_colour: own,
                self_scale: s.scale,
                reach: g.reach.pixels(),
            });
        }
        for (i, l) in level.lights.iter().enumerate() {
            let pos = [l.at.0 as f32 + 0.5, l.at.1 as f32 + 0.5];
            if let Some(f) = light_at(&l.light, t_ms, i as u64, pos, f32::from(l.radius.clamp(1, 16))) {
                self.lights.push(f);
            }
        }
    }

    /// Adds a light after the level's (plan 15a R9's order is the callers').
    pub fn push(&mut self, light: FrameLight) {
        self.lights.push(light);
    }

    /// Keeps the lights whose reach touches `view` (world `x, y, w, h`), then
    /// the first [`MAX_LIGHTS`], in order.
    pub fn cull(&mut self, view: [f32; 4]) {
        let [x0, y0, w, h] = view;
        let (x1, y1) = (x0 + w, y0 + h);
        self.lights.retain(|l| {
            let dx = (x0 - l.pos[0]).max(l.pos[0] - x1).max(0.0);
            let dy = (y0 - l.pos[1]).max(l.pos[1] - y1).max(0.0);
            libm::sqrtf(dx * dx + dy * dy) <= l.reach + l.radius
        });
        self.lights.truncate(MAX_LIGHTS);
    }
}

/// A looping animation's default phase; a curve that plays once starts at its start.
fn phase_for(animation: &crate::Animation, identity: u64) -> u32 {
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
