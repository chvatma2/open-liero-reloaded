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
    // world frame origin x, y (world px), world target texels per world px, 0
    frame: vec4<f32>,
    // level width, height (world px), art scale, glow-light cell (world px)
    level: vec4<f32>,
    // view mode (0 lit, 1 full bright, 2 light only), group count, light soak (world px), 0
    mode: vec4<f32>,
    // rgb solid floor (linear), 0
    solid: vec4<f32>,
    // rgb worm floor (linear), 0 (plan 15b)
    worm_floor: vec4<f32>,
    // the occluder map (plan 15b): origin x, y (world px), width, height (texels)
    occluders: vec4<f32>,
    // per group: rgb cast (linear, × intensity × animation), self-light scale
    groups: array<vec4<f32>, 255>,
    // per group: reach (world px; 1e9 unlimited), own colour (1) or tint (0), 0, 0
    group_info: array<vec4<f32>, 255>,
};

// LIGHT_PAD (world pixels the light grid reaches past the world target on
// every side) and MAX_SOAK are prepended from the Rust constants.

const TAU: f32 = 6.2831853;

// A value kept in a 16-bit float target, split in two halves each exact in
// f16: (floor(v / 64), v - 64 floor(v / 64)). GL draws into no R32Float
// target (plan 15a Task 19's e2e); Rgba16Float it does. A value of 1e8 or
// more (unlimited, or none) is (-1, 0).
fn split(v: f32) -> vec2<f32> {
    if v >= 1e8 {
        return vec2<f32>(-1.0, 0.0);
    }
    let h = floor(v / 64.0);
    return vec2<f32>(h, v - h * 64.0);
}

// `split`'s value again: 1e9 for unlimited or none.
fn joined(s: vec2<f32>) -> f32 {
    if s.x < 0.0 {
        return 1e9;
    }
    return s.x * 64.0 + s.y;
}

// The worm (index + 1) at world pixel `w` of the occluder map `map` whose
// texel (0, 0) is world pixel `origin` (`LookGlobals.occluders.xy`), 0 for
// none or outside it (plan 15b).
fn worm_in(map: texture_2d<u32>, origin: vec2<f32>, w: vec2<i32>) -> u32 {
    let t = w - vec2<i32>(origin);
    let size = vec2<i32>(textureDimensions(map));
    if any(t < vec2<i32>(0)) || any(t >= size) {
        return 0u;
    }
    return textureLoad(map, t, 0).r;
}

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
