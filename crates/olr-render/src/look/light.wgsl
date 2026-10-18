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

// Light i's shadow map along ray `ray`: whether a point `d` world pixels
// out is lit (1) or in shadow (0).
fn lit_on(i: u32, ray: i32, d: f32) -> f32 {
    let r = ((ray % i32(SHADOW_RAYS)) + i32(SHADOW_RAYS)) % i32(SHADOW_RAYS);
    let s = joined(textureLoad(shadow_tex, vec2<i32>(r, i32(i)), 0).xy);
    return select(0.0, 1.0, s > 0.0 && d <= s + 1.0);
}

// The ray of light `a` towards `c` (ray 0 inside its radius: the light's
// own place says whether it sits in rock).
fn ray_to(a: vec4<f32>, c: vec2<f32>) -> i32 {
    if distance(c, a.xy) <= a.z {
        return 0;
    }
    let angle = atan2(c.y - a.y, c.x - a.x);
    return i32(floor(fract(angle / TAU) * SHADOW_RAYS)) % i32(SHADOW_RAYS);
}

// How much of light i (its first vec4 `a`) reaches world point `c`, by the
// shadow filter (`look.mode.w`): 0 hard (one test, a world pixel's edge),
// 1 anti-aliased (four tests within the world pixel), 2 soft (a penumbra
// as wide as the light's radius casts behind the occluder, filtered over
// the neighbouring rays).
fn visible(i: u32, a: vec4<f32>, c: vec2<f32>) -> f32 {
    let d = distance(c, a.xy);
    let shadows = look.mode.w;
    if shadows < 0.5 || d <= a.z {
        return lit_on(i, ray_to(a, c), d);
    }
    if shadows < 1.5 {
        var sum = 0.0;
        for (var k = 0; k < 4; k++) {
            let q = c + vec2<f32>(f32(k % 2) - 0.5, f32(k / 2) - 0.5) * 0.5;
            sum += lit_on(i, ray_to(a, q), distance(q, a.xy));
        }
        return sum * 0.25;
    }
    // Soft: the occluders among the rays a penumbra could reach, then the
    // share of rays lit over the penumbra they cast here.
    let step = TAU / SHADOW_RAYS;
    let r0 = ray_to(a, c);
    let reach = clamp(i32(ceil(a.z * 2.0 / max(d * step, 1e-3))), 1, 12);
    var blockers = 0.0;
    var count = 0.0;
    for (var k = -reach; k <= reach; k++) {
        let r = ((r0 + k) % i32(SHADOW_RAYS) + i32(SHADOW_RAYS)) % i32(SHADOW_RAYS);
        let s = joined(textureLoad(shadow_tex, vec2<i32>(r, i32(i)), 0).xy);
        if s > 0.0 && d > s + 1.0 {
            blockers += s;
            count += 1.0;
        }
    }
    if count == 0.0 {
        return lit_on(i, r0, d);
    }
    let s = blockers / count;
    let width = a.z * (d - s) / max(s, 1.0);
    let n = clamp(i32(round(width / max(d * step, 1e-3))), 0, reach);
    var sum = 0.0;
    for (var k = -n; k <= n; k++) {
        sum += lit_on(i, r0 + k, d);
    }
    return sum / f32(2 * n + 1);
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
        let v = visible(i, a, c);
        if v <= 0.0 {
            continue;
        }
        acc += lights.v[i * 2u + 1u].rgb * fade(past, a.w) * v;
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
