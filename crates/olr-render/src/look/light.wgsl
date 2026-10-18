// The view's light grid (plan 15a Task 10): one texel per world pixel of
// the world target. `fs_sum` gathers the light (ambient; the glow light and
// placed lights join it), `fs_blur_h` and `fs_blur_v` blur it by the
// level's softness, and `fs_blur_v` also caps it at max light and bands it
// into light steps.

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var<uniform> lights: Lights;
@group(0) @binding(3) var shadow_tex: texture_2d<f32>;
@group(0) @binding(4) var glow_light: texture_2d<f32>;

// The level's glow light at world point `c`: its cell grid (cells of
// `look.level.w` world pixels) sampled bilinearly, clamped.
fn glow_at(c: vec2<f32>) -> vec3<f32> {
    let g = c / max(look.level.w, 1.0) - vec2<f32>(0.5);
    let size = vec2<i32>(textureDimensions(glow_light));
    let g0 = floor(g);
    let f = g - g0;
    let i0 = vec2<i32>(g0);
    let lo = vec2<i32>(0);
    let hi = size - vec2<i32>(1);
    let a = textureLoad(glow_light, clamp(i0, lo, hi), 0).rgb;
    let b = textureLoad(glow_light, clamp(i0 + vec2<i32>(1, 0), lo, hi), 0).rgb;
    let d = textureLoad(glow_light, clamp(i0 + vec2<i32>(0, 1), lo, hi), 0).rgb;
    let e = textureLoad(glow_light, clamp(i0 + vec2<i32>(1, 1), lo, hi), 0).rgb;
    return mix(mix(a, b, f.x), mix(d, e, f.x), f.y);
}

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

// The placed lights at world point `c`.
fn placed(c: vec2<f32>) -> vec3<f32> {
    var acc = vec3<f32>(0.0);
    let count = u32(look.frame.w);
    for (var i = 0u; i < count; i++) {
        let a = lights.v[i * 2u];
        let d = distance(c, a.xy);
        let past = max(d - a.z, 0.0);
        if past >= a.w {
            continue;
        }
        // The ray towards c; inside the radius the light's own ray 0 says
        // whether it sits in rock.
        var ray = 0;
        if d > a.z {
            let angle = atan2(c.y - a.y, c.x - a.x);
            ray = i32(floor(fract(angle / TAU) * SHADOW_RAYS)) % i32(SHADOW_RAYS);
        }
        let shadow = joined(textureLoad(shadow_tex, vec2<i32>(ray, i32(i)), 0).xy);
        if shadow <= 0.0 || d > shadow + 1.0 {
            continue;
        }
        acc += lights.v[i * 2u + 1u].rgb * fade(past, a.w);
    }
    return acc;
}

@fragment
fn fs_sum(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    // Grid texel p is world pixel (origin - pad + p); its centre is p's.
    let c = look.frame.xy - vec2<f32>(LIGHT_PAD) + floor(p.xy) + vec2<f32>(0.5);
    return vec4<f32>(look.ambient.rgb + glow_at(c) + placed(c), 1.0);
}

fn blur(p: vec2<i32>, dir: vec2<i32>) -> vec3<f32> {
    let r = clamp(i32(look.tint.w), 0, 4);
    let size = vec2<i32>(textureDimensions(src));
    var acc = vec3<f32>(0.0);
    for (var i = -r; i <= r; i++) {
        let q = clamp(p + dir * i, vec2<i32>(0), size - vec2<i32>(1));
        acc += textureLoad(src, q, 0).rgb;
    }
    return acc / f32(2 * r + 1);
}

@fragment
fn fs_blur_h(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(blur(vec2<i32>(p.xy), vec2<i32>(1, 0)), 1.0);
}

@fragment
fn fs_blur_v(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let max_light = look.ambient.w;
    var c = min(blur(vec2<i32>(p.xy), vec2<i32>(0, 1)), vec3<f32>(max_light));
    let steps = look.grade.w;
    if steps > 0.0 {
        c = floor(c / max_light * steps + vec3<f32>(0.5)) / steps * max_light;
    }
    return vec4<f32>(max(c, vec3<f32>(0.0)), 1.0);
}
