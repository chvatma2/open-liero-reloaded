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
    // world frame origin x, y (world px), world target texels per world px, bounce (0-0.9; plan 15c)
    frame: vec4<f32>,
    // level width, height (world px), art scale, glow-light cell (world px)
    level: vec4<f32>,
    // view mode (0 lit, 1 full bright, 2 light only), group count, light soak (world px), bloom strength (plan 15c)
    mode: vec4<f32>,
    // rgb solid floor (linear), ambient occlusion strength (0 none, up to 1; issue #10)
    solid: vec4<f32>,
    // rgb worm floor (linear; plan 15b), ambient occlusion reach (world px; issue #10)
    worm_floor: vec4<f32>,
    // the occluder map (plan 15b): origin x, y (world px), width, height (texels)
    occluders: vec4<f32>,
    // per group: rgb cast (linear, × intensity × animation), self-light scale
    groups: array<vec4<f32>, 255>,
    // per group: reach (world px; 1e9 unlimited), own colour (1) or tint (0), 0, 0
    group_info: array<vec4<f32>, 255>,
    // per group: a flicker's speed (0: none), depth (percent), clock (ms),
    // seed (plan 15c: each region flickers on its own)
    group_flicker: array<vec4<f32>, 255>,
};

// LIGHT_PAD (world pixels the light grid reaches past the world target on
// every side), MAX_SOAK, BLOOM_RADIUS and OCCLUSION_PAD (the occlusion's
// longest reach: its rows reach that far above and below the light grid)
// are prepended from the Rust constants.

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

// A 32-bit hash (PCG's output function).
fn hash32(x: u32) -> u32 {
    let state = x * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

// `olr_look::anim`'s flicker (plan 15c), seeded by `seed`: smooth value
// noise over buckets of 1000 / speed ms, dimming by up to `depth` percent.
fn flicker(t_ms: f32, speed: f32, depth: f32, seed: u32) -> f32 {
    let pos = max(t_ms, 0.0) / (1000.0 / max(speed, 1.0));
    let b = floor(pos);
    let f = clamp(pos - b, 0.0, 1.0);
    let k = u32(b);
    let a = f32(hash32(seed ^ hash32(k)) >> 8u) / 16777216.0;
    let c = f32(hash32(seed ^ hash32(k + 1u)) >> 8u) / 16777216.0;
    let v = a + (c - a) * (f * f * (3.0 - 2.0 * f));
    return 1.0 - clamp(depth / 100.0, 0.0, 1.0) * v;
}

// A group's flicker `f` (`LookGlobals.group_flicker`) in region `region`
// (plan 15c): 1 for a group that does not flicker.
fn region_flicker(f: vec4<f32>, region: u32) -> f32 {
    if f.x <= 0.0 {
        return 1.0;
    }
    return flicker(f.z, f.x, f.y, hash32(u32(f.w) ^ (region * 2654435769u)));
}
