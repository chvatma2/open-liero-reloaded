// olr-render's upscale pass (spec §8.2, §19 E3): one full-screen triangle
// samples the view's rectangle of the world target into the output's
// destination rectangle; outside it the output is black.

struct Upscale {
    // The view in world-target texels: x, y, w, h.
    src: vec4<f32>,
    // Where it lands in the output, in output pixels: x, y, w, h.
    dst: vec4<f32>,
    // The world target's size in texels.
    tex_size: vec2<f32>,
    // 0: integer nearest, 1: sharp bilinear.
    mode: u32,
    // 1: the output is sRGB, so convert to linear before it encodes.
    srgb: u32,
};

@group(0) @binding(0) var<uniform> up: Upscale;
@group(0) @binding(1) var world_tex: texture_2d<f32>;
@group(0) @binding(2) var smooth_sampler: sampler;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_upscale(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let rel = (frag.xy - up.dst.xy) / up.dst.zw;
    if any(rel < vec2<f32>(0.0)) || any(rel >= vec2<f32>(1.0)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let texel = up.src.xy + rel * up.src.zw;
    var c: vec4<f32>;
    if up.mode == 0u {
        let t = clamp(floor(texel), vec2<f32>(0.0), up.tex_size - vec2<f32>(1.0));
        c = textureLoad(world_tex, vec2<i32>(t), 0);
    } else {
        // Sharp bilinear: flat inside each texel, a one-output-pixel blend
        // at its edges (output pixels per texel: `scale`).
        let scale = up.dst.zw / up.src.zw;
        let region = max(vec2<f32>(0.5) - vec2<f32>(0.5) / scale, vec2<f32>(0.0));
        let centre = fract(texel) - vec2<f32>(0.5);
        let f = (centre - clamp(centre, -region, region)) * scale + vec2<f32>(0.5);
        c = textureSampleLevel(world_tex, smooth_sampler, (floor(texel) + f) / up.tex_size, 0.0);
    }
    if up.srgb == 1u {
        return vec4<f32>(to_linear(c.rgb), 1.0);
    }
    return vec4<f32>(c.rgb, 1.0);
}
