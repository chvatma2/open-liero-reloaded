// rl-render's world pass (spec §8.2, §19 E3): textured quads (terrain,
// sprites, line pixels) into the art-scale target. Texels are fetched
// with textureLoad at the floored source coordinate, clamped to the
// quad's source rectangle: nearest sampling without atlas bleeding, the
// same on every adapter.

struct Globals {
    target_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;

struct Quad {
    @location(0) dst: vec4<f32>,
    @location(1) src: vec4<f32>,
    @location(2) tint: vec4<f32>,
};

struct QuadOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) rect: vec4<f32>,
    @location(2) @interpolate(flat) tint: vec4<f32>,
};

@vertex
fn vs_world(@builtin(vertex_index) vi: u32, q: Quad) -> QuadOut {
    // A triangle strip: (0,0), (1,0), (0,1), (1,1).
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let p = q.dst.xy + corner * q.dst.zw;
    var out: QuadOut;
    out.pos = vec4<f32>(p.x / globals.target_size.x * 2.0 - 1.0, 1.0 - p.y / globals.target_size.y * 2.0, 0.0, 1.0);
    out.uv = q.src.xy + corner * q.src.zw;
    // The source rectangle with a positive width (a flip has a negative one).
    let x0 = min(q.src.x, q.src.x + q.src.z);
    out.rect = vec4<f32>(x0, q.src.y, abs(q.src.z), q.src.w);
    out.tint = q.tint;
    return out;
}

@fragment
fn fs_world(in: QuadOut) -> @location(0) vec4<f32> {
    let t = clamp(floor(in.uv), in.rect.xy, in.rect.xy + in.rect.zw - vec2<f32>(1.0));
    let c = textureLoad(tex, vec2<i32>(t), 0) * in.tint;
    if c.a <= 0.0 {
        discard;
    }
    return c;
}
