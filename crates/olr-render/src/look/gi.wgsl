// Glow light (plan 15a Task 12, spec §Glow light): 2D global illumination
// with radiance cascades over the whole level's grid of cells (`s` world
// pixels each). `fs_disc` draws play's glowing discs (plan 15c);
// `fs_emission` averages each cell's glowing pixels (what they cast), solid
// share, albedo and haze; `fs_cascade` gathers cascade i's radiance (rays
// over its interval, a glowing or solid cell stopping a ray; a solid one
// gives back its albedo times the last frame's light in front of it, plan
// 15c's bounce; a hazy cell adding its light along the ray, stopping
// nothing) and merges cascade i + 1 beyond it; `fs_fluence` averages
// cascade 0's directions per cell.
//
// Haze is hot open background (a hot back wall): it glows, yet the light
// behind it passes, so it never shadows what lies beyond it. A ray takes
// the brightest haze it meets, as it would a glowing cell it stopped at,
// and keeps going: a wide hot patch lights no more than one hot cell.
//
// Cascade i: probes every 2^i cells, 4 * 2^i directions laid in a block of
// `bw` x `bh` texels per probe, rays over [2 (2^i - 1), 2 (2^(i+1) - 1))
// cells, sphere-traced at full resolution with a jump-flood distance to the
// nearest cell that stops a ray (`fs_seed`, `fs_jfa`): no wall or opening
// is ever stepped over. Haze is no stopper: a ray takes it at each step,
// its steps no longer than a quarter of its cascade's probe spacing.

struct GiParams {
    // cascade (or level) index, directions, block width, block height
    a: vec4<f32>,
    // probes across, down, interval start (cells), interval length (cells)
    b: vec4<f32>,
    // step (cells), grid width, height (cells), cell size (world px)
    c: vec4<f32>,
    // the next cascade's directions, block width, block height, probes across
    d: vec4<f32>,
    // the next cascade's probes down, 1 when there is one, the haze's
    // reach (world px), 0
    e: vec4<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn clamped(t: vec2<i32>, size: vec2<u32>) -> vec2<i32> {
    return clamp(t, vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
}

struct GiOut {
    // rgb cast, solid share
    @location(0) emission: vec4<f32>,
    // reach (world px, `split`; unlimited at 1e9)
    @location(1) reach: vec2<f32>,
    // rgb albedo of the cell's rock and dirt (linear; plan 15c), 1
    @location(2) albedo: vec4<f32>,
    // rgb haze: the light the cell gives a ray that meets it (linear), 0
    @location(3) haze: vec4<f32>,
};


// The jump flood's seed of a cell that has no stopper yet (`Rg16Uint`:
// cells are whole, below it; issue #35).
const NO_SEED: u32 = 65535u;

// A cell stops a ray when it is half or more solid, or glows.
fn stops(e: vec4<f32>) -> bool {
    return e.a >= 0.5 || max(e.r, max(e.g, e.b)) > 1e-6;
}

// Light `c`'s luminance (linear).
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// Whether a cell's haze `h` gives a ray any light.
fn hazy(h: vec4<f32>) -> bool {
    return max(h.r, max(h.g, h.b)) > 1e-9;
}
