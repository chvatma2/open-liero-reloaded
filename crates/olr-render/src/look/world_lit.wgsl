// The world pass with a look (plan 15a Task 9): the same quads as
// world.wgsl into two targets, the albedo and its shade (self-light,
// marker), blended alike. The terrain's self-light is its glow.

struct Globals {
    target_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(0) var<uniform> look: LookGlobals;
@group(2) @binding(1) var glow_tex: texture_2d<f32>;
@group(2) @binding(2) var groups_tex: texture_2d<u32>;

struct Quad {
    @location(0) dst: vec4<f32>,
    @location(1) src: vec4<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) shade: vec2<f32>,
};

struct QuadOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) rect: vec4<f32>,
    @location(2) @interpolate(flat) tint: vec4<f32>,
    @location(3) @interpolate(flat) shade: vec2<f32>,
};

struct LitOut {
    @location(0) colour: vec4<f32>,
    // self-light, marker, 0, alpha
    @location(1) shade: vec4<f32>,
};

@vertex
fn vs_world(@builtin(vertex_index) vi: u32, q: Quad) -> QuadOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let p = q.dst.xy + corner * q.dst.zw;
    var out: QuadOut;
    out.pos = vec4<f32>(p.x / globals.target_size.x * 2.0 - 1.0, 1.0 - p.y / globals.target_size.y * 2.0, 0.0, 1.0);
    out.uv = q.src.xy + corner * q.src.zw;
    let x0 = min(q.src.x, q.src.x + q.src.z);
    out.rect = vec4<f32>(x0, q.src.y, abs(q.src.z), q.src.w);
    out.tint = q.tint;
    out.shade = q.shade;
    return out;
}

fn texel(in: QuadOut) -> vec2<i32> {
    return vec2<i32>(clamp(floor(in.uv), in.rect.xy, in.rect.xy + in.rect.zw - vec2<f32>(1.0)));
}

@fragment
fn fs_sprite_lit(in: QuadOut) -> LitOut {
    let c = textureLoad(tex, texel(in), 0) * in.tint;
    if c.a <= 0.0 {
        discard;
    }
    var out: LitOut;
    out.colour = c;
    out.shade = vec4<f32>(in.shade.x, in.shade.y, 0.0, c.a);
    return out;
}

@fragment
fn fs_terrain_lit(in: QuadOut) -> LitOut {
    let t = texel(in);
    let c = textureLoad(tex, t, 0);
    let gsize = vec2<i32>(textureDimensions(glow_tex));
    let glow = textureLoad(glow_tex, clamp(t, vec2<i32>(0), gsize - vec2<i32>(1)), 0).r;
    let s = max(i32(look.level.z), 1);
    let wsize = vec2<i32>(textureDimensions(groups_tex));
    let v = textureLoad(groups_tex, clamp(t / s, vec2<i32>(0), wsize - vec2<i32>(1)), 0).r;
    // A pixel of no group still glows; it only casts no light.
    var scale = 1.0;
    if v > 0u && v <= min(u32(look.mode.y), 255u) {
        scale = look.groups[v - 1u].w;
    }
    var out: LitOut;
    out.colour = c;
    out.shade = vec4<f32>(clamp(glow * scale, 0.0, 1.0), 0.0, 0.0, 1.0);
    return out;
}
