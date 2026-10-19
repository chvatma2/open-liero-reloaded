// The view's light grid (plan 15a Task 10): one texel per world pixel of
// the world target. `fs_sum` gathers the light (the ambient and the glow
// light, the only light model since plan 15c), `fs_blur_h` and `fs_blur_v`
// blur it by the level's softness, and `fs_blur_v` also tones it towards
// max light and bands it into light steps, and writes the light the tone
// took off (above 1) for the bloom, which `fs_bloom_h` and `fs_bloom_v`
// spread (plan 15c's glare).

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var glow_light: texture_2d<f32>;
@group(0) @binding(3) var solid_tex: texture_2d<u32>;
// The glow groups per world pixel (plan 15c: a glowing pixel feeds no
// bloom: the bright air around it does).
@group(0) @binding(4) var groups_tex: texture_2d<u32>;

// Whether world pixel `w` is opaque rock or dirt (the solid map: material 1
// is background, a clear solid is written as it); outside the level is open.
fn solid_px(w: vec2<i32>) -> bool {
    let size = vec2<i32>(textureDimensions(solid_tex));
    if any(w < vec2<i32>(0)) || any(w >= size) || any(w >= vec2<i32>(look.level.xy)) {
        return false;
    }
    return textureLoad(solid_tex, w, 0).r != 1u;
}

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

@fragment
fn fs_sum(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    // Grid texel p is world pixel (origin - pad + p); its centre is p's.
    let c = look.frame.xy - vec2<f32>(LIGHT_PAD) + floor(p.xy) + vec2<f32>(0.5);
    return vec4<f32>(look.ambient.rgb + glow_at(c), 1.0);
}

// Grid texel `p`'s world pixel.
fn world_of(p: vec2<i32>) -> vec2<i32> {
    return vec2<i32>(look.frame.xy) - vec2<i32>(i32(LIGHT_PAD)) + p;
}

// A box of the level's softness along `dir`. An air texel gathers only the
// air before the first rock each way, so no light blurs through a wall
// however thin (the user's request, 2026-10-19).
fn blur(p: vec2<i32>, dir: vec2<i32>) -> vec3<f32> {
    let r = clamp(i32(look.tint.w), 0, 4);
    let size = vec2<i32>(textureDimensions(src));
    var acc = textureLoad(src, p, 0).rgb;
    var n = 1.0;
    let air = !solid_px(world_of(p));
    for (var side = -1; side <= 1; side += 2) {
        for (var i = 1; i <= r; i++) {
            let q = clamp(p + dir * (i * side), vec2<i32>(0), size - vec2<i32>(1));
            if air && solid_px(world_of(q)) {
                break;
            }
            acc += textureLoad(src, q, 0).rgb;
            n += 1.0;
        }
    }
    return acc / n;
}

@fragment
fn fs_blur_h(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(blur(vec2<i32>(p.xy), vec2<i32>(1, 0)), 1.0);
}

// The tone curve (plan 15c, `olr_render::tone`): the identity up to 1,
// then a shoulder towards the white point `m`; a white point of 1 clips.
fn tone(v: f32, m: f32) -> f32 {
    if v <= 1.0 {
        return v;
    }
    let k = m - 1.0;
    if k <= 0.0 {
        return 1.0;
    }
    return 1.0 + k * (1.0 - exp(-(v - 1.0) / k));
}

struct Toned {
    @location(0) light: vec4<f32>,
    // the light above 1 (soft-saturated), the bloom's source
    @location(1) excess: vec4<f32>,
};

@fragment
fn fs_blur_v(@builtin(position) p: vec4<f32>) -> Toned {
    let max_light = look.ambient.w;
    let u = max(blur(vec2<i32>(p.xy), vec2<i32>(0, 1)), vec3<f32>(0.0));
    var c = vec3<f32>(tone(u.r, max_light), tone(u.g, max_light), tone(u.b, max_light));
    let steps = look.grade.w;
    if steps > 0.0 {
        c = floor(c / max_light * steps + vec3<f32>(0.5)) / steps * max_light;
    }
    var out: Toned;
    out.light = vec4<f32>(max(c, vec3<f32>(0.0)), 1.0);
    let w = world_of(vec2<i32>(p.xy));
    let gs = vec2<i32>(textureDimensions(groups_tex));
    if all(w >= vec2<i32>(0)) && all(w < gs) && textureLoad(groups_tex, w, 0).r != 0u {
        out.excess = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    // Soft-saturated: a source far above 1 (a disc's own cells) glows, not
    // a blot.
    out.excess = vec4<f32>(vec3<f32>(1.0) - exp(-max(u - vec3<f32>(1.0), vec3<f32>(0.0))), 1.0);
    return out;
}

// The bloom's spread along `dir`: a Gaussian of BLOOM_RADIUS world pixels;
// an air texel gathers only the air before the first rock each way, so no
// glare crosses a wall however thin.
fn bloom(p: vec2<i32>, dir: vec2<i32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(src));
    let sigma = f32(BLOOM_RADIUS) / 3.0;
    var acc = textureLoad(src, p, 0).rgb;
    var n = 1.0;
    let air = !solid_px(world_of(p));
    for (var side = -1; side <= 1; side += 2) {
        for (var i = 1; i <= BLOOM_RADIUS; i++) {
            let q = clamp(p + dir * (i * side), vec2<i32>(0), size - vec2<i32>(1));
            if air && solid_px(world_of(q)) {
                break;
            }
            let w = exp(-f32(i * i) / (2.0 * sigma * sigma));
            acc += textureLoad(src, q, 0).rgb * w;
        }
    }
    // Normalised as the whole kernel, so a lone bright texel spreads.
    for (var i = 1; i <= BLOOM_RADIUS; i++) {
        n += 2.0 * exp(-f32(i * i) / (2.0 * sigma * sigma));
    }
    return acc / n;
}

@fragment
fn fs_bloom_h(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(bloom(vec2<i32>(p.xy), vec2<i32>(1, 0)), 1.0);
}

@fragment
fn fs_bloom_v(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(bloom(vec2<i32>(p.xy), vec2<i32>(0, 1)), 1.0);
}
