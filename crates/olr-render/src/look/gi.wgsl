// Glow light (plan 15a Task 12, spec §Glow light): 2D global illumination
// with radiance cascades over the whole level's grid of cells (`s` world
// pixels each). `fs_disc` draws play's glowing discs (plan 15c);
// `fs_emission` averages each cell's glowing pixels (what they cast), solid
// share and albedo; `fs_cascade` gathers cascade i's radiance (rays over
// its interval, a glowing or solid cell stopping a ray; a solid one gives
// back its albedo times the last frame's light in front of it, plan 15c's
// bounce) and merges cascade i + 1 beyond it; `fs_fluence` averages
// cascade 0's directions per cell.
//
// Cascade i: probes every 2^i cells, 4 * 2^i directions laid in a block of
// `bw` x `bh` texels per probe, rays over [2 (2^i - 1), 2 (2^(i+1) - 1))
// cells, sphere-traced at full resolution with a jump-flood distance to the
// nearest cell that stops a ray (`fs_seed`, `fs_jfa`): no wall or opening
// is ever stepped over.

struct GiParams {
    // cascade (or level) index, directions, block width, block height
    a: vec4<f32>,
    // probes across, down, interval start (cells), interval length (cells)
    b: vec4<f32>,
    // step (cells), grid width, height (cells), cell size (world px)
    c: vec4<f32>,
    // the next cascade's directions, block width, block height, probes across
    d: vec4<f32>,
    // the next cascade's probes down, 1 when there is one, 0, 0
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
};


// The jump flood's seed of a cell that has no stopper yet (`Rg16Uint`:
// cells are whole, below it; issue #35).
const NO_SEED: u32 = 65535u;

// A cell stops a ray when it is half or more solid, or glows.
fn stops(e: vec4<f32>) -> bool {
    return e.a >= 0.5 || max(e.r, max(e.g, e.b)) > 1e-6;
}
