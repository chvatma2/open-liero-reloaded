// The composite (plan 15a Task 10, spec §The picture): each world-target
// texel's albedo lit by the light grid sampled at its centre, never below
// its self-light, graded; a marker shows its albedo. Linear light, sRGB
// bytes in and out.

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var albedo_tex: texture_2d<f32>;
@group(0) @binding(2) var shade_tex: texture_2d<f32>;
@group(0) @binding(3) var light_tex: texture_2d<f32>;

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

fn to_srgb(v: vec3<f32>) -> vec3<f32> {
    let c = clamp(v, vec3<f32>(0.0), vec3<f32>(1.0));
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

// The light at light-grid coordinate `g` (texel centres at whole numbers),
// bilinear, clamped to the grid.
fn light_at(g: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(light_tex));
    let g0 = floor(g);
    let f = g - g0;
    let i0 = vec2<i32>(g0);
    let a = textureLoad(light_tex, clamp(i0, vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
    let b = textureLoad(light_tex, clamp(i0 + vec2<i32>(1, 0), vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
    let c = textureLoad(light_tex, clamp(i0 + vec2<i32>(0, 1), vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
    let d = textureLoad(light_tex, clamp(i0 + vec2<i32>(1, 1), vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

@fragment
fn fs_composite(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let t = vec2<i32>(p.xy);
    let albedo = textureLoad(albedo_tex, t, 0).rgb;
    let shade = textureLoad(shade_tex, t, 0);
    if shade.y >= 0.5 {
        return vec4<f32>(albedo, 1.0);
    }
    let mode = look.mode.x;
    if mode > 0.5 && mode < 1.5 {
        return vec4<f32>(albedo, 1.0);
    }
    var a = to_linear(albedo);
    if mode > 1.5 {
        a = vec3<f32>(1.0);
    }
    // Texel t's centre in light-grid coordinates: world pixel (origin + i)'s
    // light sits at grid coordinate i.
    let g = (vec2<f32>(t) + vec2<f32>(0.5)) / look.frame.z - vec2<f32>(0.5) + vec2<f32>(LIGHT_PAD);
    let l = max(min(light_at(g), vec3<f32>(look.ambient.w)), vec3<f32>(shade.x));
    var c = a * l * look.grade.x;
    c = (c - vec3<f32>(0.5)) * look.grade.y + vec3<f32>(0.5);
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = vec3<f32>(luma) + (c - vec3<f32>(luma)) * look.grade.z;
    c = c * look.tint.rgb;
    return vec4<f32>(to_srgb(c), 1.0);
}
