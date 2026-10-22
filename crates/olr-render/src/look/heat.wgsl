// Heat's passes (issue #39): a deposit, a step, a zero, each over the
// active boxes (scissored) of the heat texture into a scratch texture that
// is copied back (`renderer/heat.rs`). Temperatures are texel units,
// HEAT_SCALE a degree.

@group(0) @binding(0) var<uniform> params: HeatParams;
@group(0) @binding(1) var heat_tex: texture_2d<u32>;
@group(0) @binding(2) var surface_tex: texture_2d<u32>;
@group(0) @binding(3) var solid_tex: texture_2d<u32>;

struct Deposits {
    // world x, y, radius, amount (degrees stone takes at the centre)
    d: array<vec4<f32>, MAX_HEAT_DEPOSITS>,
};
@group(0) @binding(4) var<uniform> deposits: Deposits;

// A triangle covering the target (the scissor picks the box).
fn covering(vi: u32) -> vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    return covering(vi);
}

struct DepositOut {
    @builtin(position) pos: vec4<f32>,
    // the pass's deposits: [x, y)
    @location(0) @interpolate(flat) range: vec2<f32>,
};

@vertex
fn vs_deposit(@builtin(vertex_index) vi: u32, @location(0) range: vec4<f32>) -> DepositOut {
    var out: DepositOut;
    out.pos = covering(vi);
    out.range = range.xy;
    return out;
}

fn temp(w: vec2<i32>) -> f32 {
    return f32(textureLoad(heat_tex, w, 0).r);
}

fn material_at(w: vec2<i32>) -> u32 {
    return heat_material(surface_tex, solid_tex, w);
}

fn written(t: f32) -> vec4<u32> {
    return vec4<u32>(u32(clamp(floor(t), 0.0, HEAT_MAX)), 0u, 0u, 0u);
}

// Each deposit of the pass's range adds amount × (1 - (d / radius)²) ×
// the material's gain; a pixel of no heating material holds none.
@fragment
fn fs_heat_deposit(in: DepositOut) -> @location(0) vec4<u32> {
    let w = vec2<i32>(floor(in.pos.xy));
    let mat = params.mats[material_at(w)].a;
    if mat.z <= 0.0 {
        return vec4<u32>(0u);
    }
    var t = temp(w);
    let c = vec2<f32>(w) + vec2<f32>(0.5);
    let end = min(u32(max(in.range.y, 0.0)), MAX_HEAT_DEPOSITS);
    for (var i = u32(max(in.range.x, 0.0)); i < end; i++) {
        let d = deposits.d[i];
        let x = length(c - d.xy) / max(d.z, 0.5);
        if x < 1.0 {
            t += max(d.w, 0.0) * (1.0 - x * x) * mat.x * HEAT_SCALE;
        }
    }
    return written(t);
}

// One step: each pixel takes a share of each neighbour's difference by the
// lesser conduction of the two over its capacity (at most MAX_COUPLING, so
// it is a convex combination: stable, never above the hottest), then loses
// heat by its cooling, fully beside the air (or as the back wall, which
// faces it), BULK of it inside. Outside the level is rock: no heat.
@fragment
fn fs_heat_step(@builtin(position) p: vec4<f32>) -> @location(0) vec4<u32> {
    let w = vec2<i32>(floor(p.xy));
    let mat = params.mats[material_at(w)].a;
    if mat.z <= 0.0 {
        return vec4<u32>(0u);
    }
    let t = temp(w);
    let size = vec2<i32>(textureDimensions(heat_tex));
    var exposed = !heat_is_solid(solid_tex, w);
    var flow = 0.0;
    let around = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
    for (var i = 0; i < 4; i++) {
        let q = w + around[i];
        if any(q < vec2<i32>(0)) || any(q >= size) {
            continue;
        }
        if !heat_is_solid(solid_tex, q) {
            exposed = true;
        }
        let k = min(mat.y, params.mats[material_at(q)].a.y);
        flow += min(k * mat.z, MAX_COUPLING) * (temp(q) - t);
    }
    var next = t + flow;
    let e = select(BULK, 1.0, exposed);
    next -= mat.w * e * (COOL_RATE * next + COOL_MIN * HEAT_SCALE);
    return written(next);
}

@fragment
fn fs_heat_zero() -> @location(0) vec4<u32> {
    return vec4<u32>(0u);
}
