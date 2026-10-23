// Heat (issue #39): the materials table and what the heat passes and their
// readers share. HEAT_SCALE (texel units a degree), HEAT_MAX, RAMP_BINS,
// HEAT_CAST, HEAT_REACH, MAX_COUPLING, COOL_RATE, COOL_MIN, BULK, BACK_HEAT,
// HEAT_SKIN, HEAT_SKIN_GAIN, HEAT_SKIN_WEIGHTS, HEAT_WALK_STEPS and
// MAX_HEAT_DEPOSITS are prepended from the Rust constants (`heat.rs`).

// One material value's: `a` its gain (degrees a deposit's degree gives it),
// conduction, 1 / capacity (0: it holds no heat), cooling; `b.x` the share
// of its glow cast as light.
struct HeatMaterial {
    a: vec4<f32>,
    b: vec4<f32>,
};

struct HeatParams {
    // heat texture width, height, 1 while anything may be hot (else no
    // reader looks), 0
    head: vec4<f32>,
    // a surface map value's (0 none)
    mats: array<HeatMaterial, 256>,
};

// Whether world pixel `w` of the solid map is opaque rock or dirt (1 is
// background, a clear solid written as it).
fn heat_is_solid(solid: texture_2d<u32>, w: vec2<i32>) -> bool {
    return textureLoad(solid, w, 0).r != 1u;
}

// The visible surface's material value at world pixel `w`: the front's
// where rock or dirt stands, else the back's.
fn heat_material(surface: texture_2d<u32>, solid: texture_2d<u32>, w: vec2<i32>) -> u32 {
    let s = textureLoad(surface, w, 0);
    return select(s.g, s.r, heat_is_solid(solid, w));
}

// Material `m`'s ramp at `t` degrees: its colour (sRGB) and glow (0 shows
// the art), linear between the bins.
fn heat_ramp(ramp: texture_2d<f32>, m: u32, t: f32) -> vec4<f32> {
    let x = clamp(t, 0.0, 1000.0) / 1000.0 * f32(RAMP_BINS - 1);
    let i = min(i32(floor(x)), RAMP_BINS - 2);
    let f = x - f32(i);
    let a = textureLoad(ramp, vec2<i32>(i, i32(m)), 0);
    let b = textureLoad(ramp, vec2<i32>(i + 1, i32(m)), 0);
    return mix(a, b, f);
}
