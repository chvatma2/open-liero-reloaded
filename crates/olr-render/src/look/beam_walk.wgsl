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
