// The look's per-frame globals (plan 15a): one uniform shared by every look
// pass. Every field is a vec4 (std140-safe); `groups[v - 1]` is a group map
// value v's group (0 is no group).

struct LookGlobals {
    // rgb ambient (linear), max light
    ambient: vec4<f32>,
    // exposure multiplier, contrast, saturation, light steps
    grade: vec4<f32>,
    // rgb tint (linear), softness (world pixels)
    tint: vec4<f32>,
    // world frame origin x, y (world px), world target texels per world px, placed lights
    frame: vec4<f32>,
    // level width, height (world px), art scale, glow-light cell (world px)
    level: vec4<f32>,
    // view mode (0 lit, 1 full bright, 2 light only), group count, 0, 0
    mode: vec4<f32>,
    // per group: rgb cast (linear, × intensity × animation), self-light scale
    groups: array<vec4<f32>, 255>,
    // per group: reach (world px; 1e9 unlimited), own colour (1) or tint (0), 0, 0
    group_info: array<vec4<f32>, 255>,
};

// Placed lights: per light two vec4s, (x, y, radius, reach; 1e9 unlimited)
// and (r, g, b linear × intensity × animation, 0).
struct Lights {
    v: array<vec4<f32>, 512>,
};

// The shadow maps' rays per light.
const SHADOW_RAYS: f32 = 512.0;

// World pixels the light grid reaches past the world target on every side,
// so a blur never sees the grid's edge in the view.
const LIGHT_PAD: f32 = 4.0;

const TAU: f32 = 6.2831853;

// How much of a light reaches `d` world pixels: (1 - (d/r)^2)^2, nothing at
// or past r; no fade for an unlimited reach (r >= 1e8).
fn fade(d: f32, r: f32) -> f32 {
    if r >= 1e8 {
        return 1.0;
    }
    let x = d / max(r, 1e-6);
    if x >= 1.0 {
        return 0.0;
    }
    let v = 1.0 - x * x;
    return v * v;
}
