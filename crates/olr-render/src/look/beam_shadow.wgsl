// The beam shadow map's pass (issue #100, `renderer/beam_shadow.rs`): one
// instanced quad a row, a row per (beam, source point), a texel per ray of
// the row's span; each texel marches its ray outward from the point by the
// composite's rules and writes the distance at which the ray enters the
// first pixel that stops it, x SHADOW_SCALE, or SHADOW_CLEAR when nothing
// does within the row's limit. BEAM_RAYS, SHADOW_WIDTH, MAX_SHADOW_ROWS,
// BEAM_POINTS, SHADOW_SCALE, SHADOW_CLEAR, BEAM_MAX_STEPS and BEAM_BLOCK(_SHIFT)
// are prepended from Rust; `beam_walk.wgsl` before this.

@group(0) @binding(0) var<uniform> glow_beams: GlowBeams;
@group(0) @binding(1) var<uniform> rows: Rows;
// The solid map (material 1 background; a clear solid written as it) and
// the block map, as the composite reads them.
@group(0) @binding(2) var solid_tex: texture_2d<u32>;
@group(0) @binding(3) var solid_blocks: texture_2d<u32>;

// Whether world pixel `w` is opaque; outside the map is open.
fn solid_px(w: vec2<i32>) -> bool {
    let size = vec2<i32>(textureDimensions(solid_tex));
    if any(w < vec2<i32>(0)) || any(w >= size) {
        return false;
    }
    return textureLoad(solid_tex, w, 0).r != 1u;
}

// Whether block `k` holds no opaque pixel (`block_map_open`).
fn block_open(k: vec2<i32>) -> bool {
    return block_map_open(solid_blocks, vec2<i32>(textureDimensions(solid_tex)), k);
}

// Where, as a share of `limit`, the ray from `s` along unit `v` enters the
// first pixel that stops it; above 1 when none does within `limit`. The
// composite's walk run outward: a front source's own rock (pixels wholly
// behind its face) never stops it, a backdrop source's pixel does when it
// is opaque, an exact corner stops it when either pixel beside it does, an
// open block is crossed in one step.
fn march(s: vec2<f32>, v: vec2<f32>, limit: f32, b: GlowBeam, back: bool) -> f32 {
    let start = vec2<i32>(floor(s));
    if back && solid_px(start) {
        return 0.0;
    }
    let d = v * limit;
    let step = vec2<i32>(select(-1, 1, d.x > 0.0), select(-1, 1, d.y > 0.0));
    let far = 1e30;
    let delta = vec2<f32>(
        select(far, abs(1.0 / d.x), d.x != 0.0),
        select(far, abs(1.0 / d.y), d.y != 0.0),
    );
    let edge = vec2<f32>(
        select(f32(start.x), f32(start.x + 1), d.x > 0.0),
        select(f32(start.y), f32(start.y + 1), d.y > 0.0),
    );
    let first = vec2<f32>(
        select(far, (edge.x - s.x) / d.x, d.x != 0.0),
        select(far, (edge.y - s.y) / d.y, d.y != 0.0),
    );
    var n = vec2<i32>(0);
    for (var i = 0; i < BEAM_MAX_STEPS; i++) {
        let cell = start + step * n;
        let next = first + vec2<f32>(n) * delta;
        let t = min(next.x, next.y);
        if t > 1.0 {
            return 2.0;
        }
        let k = cell >> vec2<u32>(BEAM_BLOCK_SHIFT);
        if block_open(k) {
            let lo = k * BEAM_BLOCK;
            let left = abs(select(lo, lo + vec2<i32>(BEAM_BLOCK - 1), step > vec2<i32>(0)) - cell);
            let leave = first + vec2<f32>(n + left) * delta;
            if min(leave.x, leave.y) > 1.0 {
                return 2.0;
            }
            let to = clamp(vec2<i32>(ceil((vec2<f32>(min(leave.x, leave.y)) - first) / delta)), n, n + left);
            if any(to != n) {
                n = to;
                continue;
            }
        }
        if abs(next.x - next.y) < 1e-6 {
            let a = cell + vec2<i32>(step.x, 0);
            let c = cell + vec2<i32>(0, step.y);
            if (solid_px(a) && (back || !behind_face(a, b))) || (solid_px(c) && (back || !behind_face(c, b))) {
                return t;
            }
            n += vec2<i32>(1);
        } else if next.x < next.y {
            n.x += 1;
        } else {
            n.y += 1;
        }
        let now = start + step * n;
        if solid_px(now) && (back || !behind_face(now, b)) {
            return t;
        }
    }
    return 0.0;
}

struct RowOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) row: u32,
};

// Row `row`'s quad: its span's texels of the map's row `row`.
@vertex
fn vs_row(@builtin(vertex_index) vi: u32, @builtin(instance_index) row: u32) -> RowOut {
    let x = select(0.0, f32(rows.r[row].y), (vi & 1u) == 1u);
    let y = f32(row) + select(0.0, 1.0, vi >= 2u);
    var o: RowOut;
    o.pos = vec4<f32>(x / f32(SHADOW_WIDTH) * 2.0 - 1.0, 1.0 - y / f32(MAX_SHADOW_ROWS) * 2.0, 0.0, 1.0);
    o.row = row;
    return o;
}

@fragment
fn fs_march(in: RowOut) -> @location(0) vec4<u32> {
    let r = rows.r[in.row];
    let b = glow_beams.beams[r.w];
    // Source point k, as the composite's.
    let k = in.row % u32(BEAM_POINTS);
    let u = (f32(k) + 0.5) / f32(BEAM_POINTS) * 2.0 - 1.0;
    let s = b.at + vec2<f32>(-b.dir.y, b.dir.x) * (u * b.half) - b.dir * 0.5;
    let ray = (r.x + u32(in.pos.x)) % BEAM_RAYS;
    let a = f32(ray) * (6.2831855 / f32(BEAM_RAYS));
    let limit = bitcast<f32>(r.z);
    let t = march(s, vec2<f32>(cos(a), sin(a)), limit, b, b.group < 0.0);
    if t > 1.0 {
        return vec4<u32>(SHADOW_CLEAR, 0u, 0u, 0u);
    }
    return vec4<u32>(min(u32(t * limit * SHADOW_SCALE), SHADOW_CLEAR - 1u), 0u, 0u, 0u);
}
