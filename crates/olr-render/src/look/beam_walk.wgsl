// The glow beams' shared pieces (issue #100): the composite
// (`composite.wgsl`) and the beam shadow map's pass (`beam_shadow.wgsl`)
// both read the frame's beams and walk the grid by the same rules.
// MAX_GLOW_BEAMS and BEAM_BLOCK are prepended from Rust
// (`renderer/glow_beams.rs`' `wgsl`).

// One glow beam (the directional-glow design; `renderer/glow_beams.rs`
// packs them): its source is a segment across it, `half` world pixels each
// side of `at`, facing `dir`; it widens by `tan_spread` and reaches `reach`
// world pixels. `light` is its strength (linear), `group` its glow group's
// value, negative for a backdrop patch's beam, `region` its patch's
// flicker region.
struct GlowBeam {
    at: vec2<f32>,
    dir: vec2<f32>,
    half: f32,
    tan_spread: f32,
    reach: f32,
    group: f32,
    light: vec3<f32>,
    region: f32,
};
// The frame's beams, `look.beams.x` of them (MAX_GLOW_BEAMS is prepended
// from Rust).
struct GlowBeams {
    beams: array<GlowBeam, MAX_GLOW_BEAMS>,
};

// The beam shadow map's rows (`renderer/beam_shadow.rs`), BEAM_POINTS a
// beam in point order: per row, the span's first ray, its rays, the
// march's limit (f32 bits) and the beam's index | its rays a circle << 8
// (`ray_count`).
struct Rows {
    r: array<vec4<u32>, MAX_SHADOW_ROWS>,
};

// The near zone's radius round a source point of a beam `half` wide each
// side spreading `tan` (`near` in `renderer/beam_shadow.rs`, the same):
// within it the composite walks exactly, beyond it looks the map up.
fn beam_near(half: f32, tan: f32) -> f32 {
    let t = max(tan, 0.0) + 0.03;
    let h = max(half, 0.0);
    let width = 2.0 * h + 0.5;
    return min(width * (1.0 + (1.0 + t) / t), NEAR_MAX);
}

// Row `r`'s rays a circle: ray `i` leaves at the world angle `i 2pi /
// rays`.
fn ray_count(r: vec4<u32>) -> u32 {
    return max(r.w >> 8u, 1u);
}

// Whether block `k` of block map `blocks` holds no opaque pixel of a solid
// map `solid` pixels wide and high. Past the block map, a block is open
// only when it lies wholly past the solid map too: a level without a beam
// map has a 1 x 1 block map, which knows nothing of its rock.
fn block_map_open(blocks: texture_2d<u32>, solid: vec2<i32>, k: vec2<i32>) -> bool {
    if any(k < vec2<i32>(0)) || any(k * BEAM_BLOCK >= solid) {
        return true;
    }
    if any(k >= vec2<i32>(textureDimensions(blocks))) {
        return false;
    }
    return textureLoad(blocks, k, 0).r == 0u;
}

// Whether world pixel `c` lies wholly behind beam `b`'s face: its farthest
// corner along the beam (as `derive` finds the face from its patch's
// corners) not past it. Exact for the pixel, unlike a point summed along a
// long walk (a thousandth of a pixel of slack).
fn behind_face(c: vec2<i32>, b: GlowBeam) -> bool {
    let far = dot(vec2<f32>(c) - b.at, b.dir) + max(b.dir.x, 0.0) + max(b.dir.y, 0.0);
    return far <= 1e-3;
}

// Whether any pixel of block `k` lies wholly behind beam `b`'s face
// (`behind_face`, linear in the pixel: its least corner decides).
fn block_behind_face(k: vec2<i32>, b: GlowBeam) -> bool {
    let lo = k * BEAM_BLOCK;
    let c = select(lo + vec2<i32>(BEAM_BLOCK - 1), lo, b.dir >= vec2<f32>(0.0));
    return behind_face(c, b);
}
