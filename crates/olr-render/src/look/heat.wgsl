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

// Whether world pixel `q` of the solid map is rock or dirt for a
// deposit's walk: outside the map is open.
fn walk_solid(q: vec2<i32>, size: vec2<i32>) -> bool {
    return all(q >= vec2<i32>(0)) && all(q < size) && heat_is_solid(solid_tex, q);
}

// The share of a deposit's gain world pixel `w` takes by what the deposit
// at `o` reaches (issue #56): a walk from `o` to `w`'s centre through the
// pixels the segment crosses, `o`'s own and `w` included. Before the first
// rock or dirt pixel all is in sight: a back pixel takes BACK_HEAT. The
// first HEAT_SKIN solid pixels in a row are the skin: the k-th takes
// HEAT_SKIN_GAIN × HEAT_SKIN_WEIGHTS[k - 1]. Past them (deeper rock, or
// the air past it) a pixel is shielded and takes 0. Passing a corner whose
// two side pixels are solid counts as meeting a solid pixel (the glow
// light's rule), so nothing slips through a diagonal wall one pixel thin.
// Each boundary's crossing is divided afresh (no sum drifts), so a segment
// through a corner meets it within the tolerance.
fn heat_reach(o: vec2<f32>, w: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(solid_tex));
    let dir = vec2<f32>(w) + vec2<f32>(0.5) - o;
    let step = vec2<i32>(select(-1, 1, dir.x > 0.0), select(-1, 1, dir.y > 0.0));
    let ahead = max(step, vec2<i32>(0));
    var q = vec2<i32>(floor(o));
    // the solid pixels met in a row since the first
    var k = 0u;
    for (var n = 0; n < HEAT_WALK_STEPS; n++) {
        let solid = walk_solid(q, size);
        if solid {
            k += 1u;
            if k > HEAT_SKIN {
                return 0.0;
            }
        } else if k > 0u {
            return 0.0;
        }
        if all(q == w) {
            if !solid {
                return BACK_HEAT;
            }
            let weights = HEAT_SKIN_WEIGHTS;
            return HEAT_SKIN_GAIN * weights[k - 1u];
        }
        // The segment's parameter at the next pixel boundary along each axis.
        let b = vec2<f32>(q + ahead);
        let tx = select(1e9, (b.x - o.x) / dir.x, dir.x != 0.0);
        let ty = select(1e9, (b.y - o.y) / dir.y, dir.y != 0.0);
        if abs(tx - ty) < 1e-5 {
            // Through a corner: both pixels beside it solid close it.
            if k == 0u && walk_solid(q + vec2<i32>(step.x, 0), size) && walk_solid(q + vec2<i32>(0, step.y), size) {
                k = 1u;
            }
            q += step;
        } else if tx < ty {
            q.x += step.x;
        } else {
            q.y += step.y;
        }
    }
    return 0.0;
}

// Each deposit of the pass's range adds amount × (1 - (d / radius)²) ×
// the material's gain × the share its walk gives (`heat_reach`, issue
// #56), in whole texel units (issue #40: integer sums below 2^24 are
// exact, so deposits landing in one pass or in several give the same
// texel); a pixel of no heating material holds none.
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
            let gain = mat.x * heat_reach(d.xy, w);
            t += floor(max(d.w, 0.0) * (1.0 - x * x) * gain * HEAT_SCALE);
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
