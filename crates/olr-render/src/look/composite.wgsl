// The composite (plan 15a Task 10, spec §The picture): each world-target
// texel's albedo lit by the light grid sampled at its centre, never below
// its self-light nor its floor (the solid floor for blood and gibs, as rock;
// the worm floor for worms, plan 15b), graded; a marker shows its albedo. Linear light, sRGB
// bytes in and out.

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var albedo_tex: texture_2d<f32>;
@group(0) @binding(2) var shade_tex: texture_2d<f32>;
@group(0) @binding(3) var light_tex: texture_2d<f32>;
@group(0) @binding(4) var occluder_tex: texture_2d<u32>;

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

// The light a worm's world pixel `w` (of worm `id`) takes (plan 15b, the
// lit side): the light just outside the worm on that pixel's side. The
// outward direction is the sum of `w - q` over the worm's pixels `q` in the
// 7 x 7 window (up when they cancel); the first pixel along it within 4
// that is not this worm, and the light one pixel further out. The same for
// every texel of a world pixel, so never finer than the art; `here` when
// the worm is thicker than that.
fn facing_light(w: vec2<i32>, id: u32, here: vec3<f32>) -> vec3<f32> {
    var sum = vec2<f32>(0.0);
    for (var dy = -3; dy <= 3; dy++) {
        for (var dx = -3; dx <= 3; dx++) {
            if worm_in(occluder_tex, look.occluders.xy, w + vec2<i32>(dx, dy)) == id {
                sum -= vec2<f32>(f32(dx), f32(dy));
            }
        }
    }
    var n = vec2<f32>(0.0, -1.0);
    if dot(sum, sum) > 1e-6 {
        n = normalize(sum);
    }
    let c = vec2<f32>(w) + vec2<f32>(0.5);
    let origin = vec2<i32>(look.frame.xy);
    for (var k = 1; k <= 4; k++) {
        let q = vec2<i32>(floor(c + n * f32(k)));
        if worm_in(occluder_tex, look.occluders.xy, q) != id {
            let out = vec2<i32>(floor(c + n * f32(k + 1)));
            return light_at(vec2<f32>(out - origin) + vec2<f32>(LIGHT_PAD));
        }
    }
    return here;
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
    var here = light_at(g);
    if shade.z > 0.75 {
        // A worm's pixel takes the light on its side (plan 15b).
        let w = vec2<i32>(look.frame.xy) + vec2<i32>(floor((vec2<f32>(t) + vec2<f32>(0.5)) / look.frame.z));
        let id = worm_in(occluder_tex, look.occluders.xy, w);
        if id != 0u {
            here = facing_light(w, id, here);
        }
    }
    // The floor kind, stored halved (world_lit.wgsl): 0.5 the solid floor,
    // 1 the worm floor (plan 15b).
    var least = vec3<f32>(0.0);
    if shade.z > 0.75 {
        least = look.worm_floor.rgb;
    } else if shade.z > 0.25 {
        least = look.solid.rgb;
    }
    let l = max(max(min(here, vec3<f32>(look.ambient.w)), vec3<f32>(shade.x)), least);
    var c = a * l * look.grade.x;
    c = (c - vec3<f32>(0.5)) * look.grade.y + vec3<f32>(0.5);
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = vec3<f32>(luma) + (c - vec3<f32>(luma)) * look.grade.z;
    c = c * look.tint.rgb;
    return vec4<f32>(to_srgb(c), 1.0);
}
