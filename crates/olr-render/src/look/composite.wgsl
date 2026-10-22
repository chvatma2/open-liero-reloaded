// The composite (plan 15a Task 10, spec §The picture): each world-target
// texel's albedo lit by the light grid sampled at its centre, never below
// its self-light nor its floor (the solid floor for blood and gibs, as rock;
// the worm floor for worms, plan 15b), with the bloom added (plan 15c),
// graded; a worm's pixel takes the light where it is, as a rope's does
// (#53: no lit side); a marker shows its albedo. Opaque rock and dirt show
// at most their art (#1). A distant layer's pixel takes less of the light
// the farther it is (issue #11). Linear light, sRGB bytes in and out.

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var albedo_tex: texture_2d<f32>;
@group(0) @binding(2) var shade_tex: texture_2d<f32>;
@group(0) @binding(3) var light_tex: texture_2d<f32>;
// The bloom (plan 15c): the light above 1, spread; added before the grade
// at `look.mode.w`'s strength.
@group(0) @binding(5) var bloom_tex: texture_2d<f32>;
// The solid map (#1): an opaque solid shows at most its art.
@group(0) @binding(6) var solid_tex: texture_2d<u32>;

// The view's worm boxes (issue #35): `count.x` boxes, each `[x0, y0, x1,
// y1)` in light-grid texels (the occluder map's), holding every worm texel.
// Bound for #54's halo; read by nothing yet (#53).
struct WormBoxes {
    count: vec4<i32>,
    boxes: array<vec4<i32>, WORM_BOXES>,
};
@group(0) @binding(7) var<uniform> worm_boxes: WormBoxes;

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

// Whether world pixel `w` is opaque rock or dirt (the solid map: material 1
// is background, a clear solid is written as it); outside the level is open.
fn solid_px(w: vec2<i32>) -> bool {
    let size = vec2<i32>(textureDimensions(solid_tex));
    if any(w < vec2<i32>(0)) || any(w >= size) || any(w >= vec2<i32>(look.level.xy)) {
        return false;
    }
    return textureLoad(solid_tex, w, 0).r != 1u;
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

// The bloom at light-grid coordinate `g`, bilinear, clamped.
fn bloom_at(g: vec2<f32>) -> vec3<f32> {
    let size = vec2<i32>(textureDimensions(bloom_tex));
    let g0 = floor(g);
    let f = g - g0;
    let i0 = vec2<i32>(g0);
    let a = textureLoad(bloom_tex, clamp(i0, vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
    let b = textureLoad(bloom_tex, clamp(i0 + vec2<i32>(1, 0), vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
    let c = textureLoad(bloom_tex, clamp(i0 + vec2<i32>(0, 1), vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
    let d = textureLoad(bloom_tex, clamp(i0 + vec2<i32>(1, 1), vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
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
    var here = light_at(g);
    // Texel t's world pixel.
    let w = vec2<i32>(look.frame.xy) + vec2<i32>(floor((vec2<f32>(t) + vec2<f32>(0.5)) / look.frame.z));
    // The floor kind, stored halved (world_lit.wgsl): 0.5 the solid floor,
    // 1 the worm floor (plan 15b).
    var least = vec3<f32>(0.0);
    if shade.z > 0.75 {
        least = look.worm_floor.rgb;
    } else if shade.z > 0.25 {
        least = look.solid.rgb;
    }
    // A glowing pixel shows its own art: other light lifts it to 1 at most
    // (plan 15c: a sky lights the level without looking blown out).
    // Nor does the glare touch it: the halo is the bright air's around it.
    var cap = vec3<f32>(look.ambient.w);
    var glare = look.mode.w;
    // So does opaque rock or dirt (#1: under a bright sky, rock soaked
    // above 1 and blooming read oversaturated, brighter than its art).
    if shade.x > 0.0 || solid_px(w) {
        cap = min(cap, vec3<f32>(1.0));
        glare = 0.0;
    }
    // A distant layer's pixel (issue #11) takes the ambient and 1 - depth
    // of the light beyond it, the bloom too; only in a level that has one.
    if look.distant.x > 0.5 && shade.w > 0.0 && shade.w < 0.998 {
        let d = 1.0 - shade.w;
        let base = min(here, look.ambient.rgb);
        here = base + (here - base) * (1.0 - d);
        glare = glare * (1.0 - d);
    }
    let l = max(max(min(here, cap), vec3<f32>(shade.x)), least);
    var c = (a * l + bloom_at(g) * glare) * look.grade.x;
    c = (c - vec3<f32>(0.5)) * look.grade.y + vec3<f32>(0.5);
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = vec3<f32>(luma) + (c - vec3<f32>(luma)) * look.grade.z;
    c = c * look.tint.rgb;
    return vec4<f32>(to_srgb(c), 1.0);
}
