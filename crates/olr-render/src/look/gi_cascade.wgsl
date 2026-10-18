// Glow light: one cascade (see gi.wgsl).


@group(0) @binding(0) var<uniform> cp: GiParams;
@group(0) @binding(1) var level_em: texture_2d<f32>;
@group(0) @binding(2) var level_reach: texture_2d<f32>;
@group(0) @binding(3) var upper: texture_2d<f32>;
@group(0) @binding(4) var seeds: texture_2d<f32>;

// The most steps of one ray's sphere trace (a ray grazing a wall steps one
// cell at a time); past it the ray counts as open.
const MAX_STEPS: i32 = 96;

// Cascade (d.x dirs, d.y x d.z block, d.w x e.x probes) at probe q, direction k.
fn upper_at(q: vec2<i32>, k: i32) -> vec4<f32> {
    let bw = i32(cp.d.y);
    let probe = clamp(q, vec2<i32>(0), vec2<i32>(i32(cp.d.w), i32(cp.e.x)) - vec2<i32>(1));
    let t = probe * vec2<i32>(bw, i32(cp.d.z)) + vec2<i32>(k % bw, k / bw);
    return textureLoad(upper, t, 0);
}

@fragment
fn fs_cascade(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let t = vec2<i32>(p.xy);
    let bw = i32(cp.a.z);
    let bh = i32(cp.a.w);
    let probe = t / vec2<i32>(bw, bh);
    if probe.x >= i32(cp.b.x) || probe.y >= i32(cp.b.y) {
        return vec4<f32>(0.0);
    }
    let within = t - probe * vec2<i32>(bw, bh);
    let d = within.y * bw + within.x;
    let spacing = exp2(cp.a.x);
    // Cells: probe centres at (probe + 0.5) * spacing.
    let centre = (vec2<f32>(probe) + vec2<f32>(0.5)) * spacing;
    let angle = (f32(d) + 0.5) / cp.a.y * TAU;
    let dir = vec2<f32>(cos(angle), sin(angle));
    let size = vec2<i32>(textureDimensions(level_em));
    var ray = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    // A probe never stops on its own cell, so a wall's face gathers the
    // light in front of it (cascade 0's rays leave the cell first).
    var dist = max(cp.b.z, 0.75 * spacing);
    let end = cp.b.z + cp.b.w;
    for (var n = 0; n < MAX_STEPS; n++) {
        if dist >= end {
            break;
        }
        let at = centre + dir * dist;
        let q = vec2<i32>(floor(at));
        // Outside the grid nothing stops a ray, and it never comes back.
        if any(q < vec2<i32>(0)) || any(q >= size) {
            break;
        }
        let e = textureLoad(level_em, q, 0);
        if stops(e) {
            if max(e.r, max(e.g, e.b)) > 1e-6 {
                let reach = textureLoad(level_reach, q, 0).r;
                ray = vec4<f32>(e.rgb * fade(dist * cp.c.w, reach), 0.0);
            } else {
                ray = vec4<f32>(0.0);
            }
            break;
        }
        // As far as the nearest stopper allows, at least a cell.
        let s = textureLoad(seeds, q, 0).xy;
        var free = 1e9;
        if s.x >= 0.0 {
            free = distance(s + vec2<f32>(0.5), at) - 1.5;
        }
        dist += max(free, 1.0);
    }
    if cp.e.y > 0.5 && ray.a > 0.0 {
        // The next cascade's two directions around this one, bilinear
        // between its four probes around this probe's centre.
        let g = centre / (spacing * 2.0) - vec2<f32>(0.5);
        let g0 = floor(g);
        let f = g - g0;
        let q0 = vec2<i32>(g0);
        var acc = vec3<f32>(0.0);
        for (var k = 0; k < 2; k++) {
            let dk = d * 2 + k;
            let a = upper_at(q0, dk).rgb;
            let b = upper_at(q0 + vec2<i32>(1, 0), dk).rgb;
            let c = upper_at(q0 + vec2<i32>(0, 1), dk).rgb;
            let e = upper_at(q0 + vec2<i32>(1, 1), dk).rgb;
            acc += mix(mix(a, b, f.x), mix(c, e, f.x), f.y);
        }
        ray = vec4<f32>(ray.rgb + ray.a * acc * 0.5, ray.a);
    }
    return ray;
}

