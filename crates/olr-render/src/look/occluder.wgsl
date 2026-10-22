// The occluder map (plan 15b): each living worm's body drawn at one texel
// per world pixel over the light grid, each opaque texel its worm (index +
// 1); 0 is no worm. Only the composite's worm halo reads it (issue #54;
// #53: worms take no part in the light).

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(1) @binding(0) var tex: texture_2d<f32>;

struct Quad {
    @location(0) dst: vec4<f32>,
    @location(1) src: vec4<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) shade: vec3<f32>,
};

struct QuadOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) rect: vec4<f32>,
    @location(2) @interpolate(flat) worm: f32,
};

@vertex
fn vs_occluder(@builtin(vertex_index) vi: u32, q: Quad) -> QuadOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let p = q.dst.xy + corner * q.dst.zw;
    let size = max(look.occluders.zw, vec2<f32>(1.0));
    var out: QuadOut;
    out.pos = vec4<f32>(p.x / size.x * 2.0 - 1.0, 1.0 - p.y / size.y * 2.0, 0.0, 1.0);
    out.uv = q.src.xy + corner * q.src.zw;
    let x0 = min(q.src.x, q.src.x + q.src.z);
    out.rect = vec4<f32>(x0, q.src.y, abs(q.src.z), q.src.w);
    out.worm = q.shade.x;
    return out;
}

@fragment
fn fs_occluder(in: QuadOut) -> @location(0) vec4<u32> {
    let t = vec2<i32>(clamp(floor(in.uv), in.rect.xy, in.rect.xy + in.rect.zw - vec2<f32>(1.0)));
    if textureLoad(tex, t, 0).a <= 0.0 {
        discard;
    }
    return vec4<u32>(u32(round(in.worm)), 0u, 0u, 0u);
}
