// Light in solids (plan 15a Task 14a, spec §Light in solids): an opaque
// solid texel of the light grid takes the most light within the soak of it,
// faded by distance, `fs_soak_h` across, then `fs_soak_v` down, which also
// raises it to the solid floor. Air keeps its light, and so does a clear
// solid (the solid map holds it as background).
//
// Issue #57: the soak of an opaque texel is `soak_radius` of its world
// pixel, which the Rust side appends: the level's alone (the plain
// pipelines, today's bytes), or its front material's own, else the
// level's (the material pipelines, group 1). Each pass uses the receiving
// texel's radius; at a border between materials the down pass reads its
// column's across results, made with those texels' radii (accepted).

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var solid_tex: texture_2d<u32>;
// The light sum: what air keeps.
@group(0) @binding(3) var sum_tex: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

// The world pixel of grid texel `p`.
fn world_of(p: vec2<i32>) -> vec2<i32> {
    return vec2<i32>(floor(look.frame.xy)) - vec2<i32>(i32(LIGHT_PAD)) + p;
}

// The level's soak.
fn level_soak() -> i32 {
    return clamp(i32(look.mode.z), 0, MAX_SOAK);
}

// Whether grid texel `p` is an opaque solid; outside the level is open.
fn opaque(p: vec2<i32>) -> bool {
    let w = world_of(p);
    let size = min(vec2<i32>(look.level.xy), vec2<i32>(textureDimensions(solid_tex)));
    if any(w < vec2<i32>(0)) || any(w >= size) {
        return false;
    }
    return textureLoad(solid_tex, w, 0).r != 1u;
}

fn soak(p: vec2<i32>, dir: vec2<i32>) -> vec3<f32> {
    let own = textureLoad(src, p, 0).rgb;
    if !opaque(p) {
        return own;
    }
    let r = soak_radius(world_of(p));
    if r == 0 {
        return own;
    }
    let size = vec2<i32>(textureDimensions(src));
    var best = own;
    for (var i = 1; i <= r; i++) {
        let k = fade(f32(i), f32(r) + 1.0);
        let a = textureLoad(src, clamp(p + dir * i, vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
        let b = textureLoad(src, clamp(p - dir * i, vec2<i32>(0), size - vec2<i32>(1)), 0).rgb;
        best = max(best, max(a, b) * k);
    }
    return best;
}

@fragment
fn fs_soak_h(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(soak(vec2<i32>(p.xy), vec2<i32>(1, 0)), 1.0);
}

@fragment
fn fs_soak_v(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let t = vec2<i32>(p.xy);
    if !opaque(t) {
        return vec4<f32>(textureLoad(sum_tex, t, 0).rgb, 1.0);
    }
    let r = soak_radius(world_of(t));
    let size = vec2<i32>(textureDimensions(src));
    // Down the column: the most across-soaked light, faded by distance.
    var c = textureLoad(src, t, 0).rgb;
    for (var i = 1; i <= r; i++) {
        let k = fade(f32(i), f32(r) + 1.0);
        for (var side = -1; side <= 1; side += 2) {
            let q = clamp(t + vec2<i32>(0, i * side), vec2<i32>(0), size - vec2<i32>(1));
            c = max(c, textureLoad(src, q, 0).rgb * k);
        }
    }
    c = max(c, look.solid.rgb);
    return vec4<f32>(c, 1.0);
}
