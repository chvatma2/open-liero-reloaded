// Glow light: one cascade (see gi.wgsl).


@group(0) @binding(0) var<uniform> cp: GiParams;
@group(0) @binding(1) var level_em: texture_2d<f32>;
@group(0) @binding(2) var level_reach: texture_2d<f32>;
@group(0) @binding(3) var upper: texture_2d<f32>;
@group(0) @binding(4) var seeds: texture_2d<u32>;
// Plan 15c's bounce: each cell's albedo, the last frame's fluence (the
// fluence itself, which `fs_fluence` rewrites only after the cascades;
// issue #35), and the look's globals (`frame.w`: the bounce, 0-0.9).
@group(0) @binding(5) var albedo_tex: texture_2d<f32>;
@group(0) @binding(6) var history: texture_2d<f32>;
@group(0) @binding(7) var<uniform> look: LookGlobals;
// Each cell's haze (see gi.wgsl), its reach `cp.e.z` world pixels.
@group(0) @binding(8) var level_haze: texture_2d<f32>;
// The glow beams (the directional-glow design): each world pixel's beam
// texel, direction / 2 + 1 (0 not directional) and the spread (degrees);
// 1 x 1 none (`look.beams.z` 0).
@group(1) @binding(0) var beam_tex: texture_2d<u32>;

// How much wider than its spread a directional cell's light fades out,
// radians (the spike's soft edge).
const BEAM_SOFT: f32 = 0.26;

// How much of a directional cell's light the ray from it along `-ray`
// carries: all within its spread, none past the spread and BEAM_SOFT, a
// smoothstep between; 1 for a cell that is not directional. Cell `q`'s
// middle world pixel's texel (the light comes from the patch it is in).
fn beam_cone(q: vec2<i32>, ray: vec2<f32>) -> f32 {
    let c = i32(cp.c.w);
    let w = q * c + vec2<i32>(c / 2);
    let size = vec2<i32>(textureDimensions(beam_tex));
    if any(w < vec2<i32>(0)) || any(w >= size) {
        return 1.0;
    }
    let b = textureLoad(beam_tex, w, 0).rg;
    if b.x == 0u {
        return 1.0;
    }
    let theta = radians(f32(b.x - 1u) * 2.0);
    let spread = radians(f32(max(b.y, 1u)));
    let d = vec2<f32>(cos(theta), sin(theta));
    return smoothstep(cos(min(spread + BEAM_SOFT, 3.14159265)), cos(spread), dot(-ray, d));
}

// The most steps of one ray's march (a ray grazing a wall steps one cell
// at a time); past it the ray counts as open.
const MAX_STEPS: i32 = 128;

// A cascade's longest jump, of its probes' spacing (haze is no stopper,
// so only this keeps a ray from jumping over it): finer haze its cascade
// cannot tell apart anyway. A quarter keeps about nine tenths of a hot
// patch's light in open air for 2 % more cost than a whole spacing, which
// keeps half (`hot_open_air_lights_the_air_round_it`).
const HAZE_STRIDE: f32 = 0.25;

// The most radiance a ray carries, well inside f16 (final review,
// Important 1: overlapping discs overflowed into infinity, then NaN, which
// bounce kept alive).
const LIGHT_MAX: f32 = 16384.0;

// Cascade (d.x dirs, d.y x d.z block, d.w x e.x probes) at probe q, direction k.
fn upper_at(q: vec2<i32>, k: i32) -> vec4<f32> {
    let bw = i32(cp.d.y);
    let probe = clamp(q, vec2<i32>(0), vec2<i32>(i32(cp.d.w), i32(cp.e.x)) - vec2<i32>(1));
    let t = probe * vec2<i32>(bw, i32(cp.d.z)) + vec2<i32>(k % bw, k / bw);
    return textureLoad(upper, t, 0);
}

fn inside(q: vec2<i32>) -> bool {
    return all(q >= vec2<i32>(0)) && all(q < vec2<i32>(textureDimensions(level_em)));
}

// Whether cell `q` stops a ray (outside the grid nothing does).
fn stops_at(q: vec2<i32>) -> bool {
    return inside(q) && stops(textureLoad(level_em, q, 0));
}

// How far a ray at `at` may jump: the jump flood's distance to the nearest
// stopper, less a cell and a half; 0 near one, `JFA_FREE` with none within
// the flood's reach.
fn free_at(at: vec2<f32>) -> f32 {
    let t = textureLoad(seeds, vec2<i32>(floor(at)), 0).xy;
    if t.x == NO_SEED {
        return JFA_FREE;
    }
    let s = vec2<f32>(t);
    return max(distance(s + vec2<f32>(0.5), at) - 1.5, 0.0);
}

// The ray from `o` along `dir` over [t0, t1) cells: where it meets a
// stopper, `t` its distance and `q` its cell (`hit`), or open. Far from
// stoppers it jumps by the flood's distance; near them it walks cell by
// cell (an exact grid walk), and passing a corner it is stopped when both
// cells beside the corner stop: no ray slips between rock pixels touching
// at a corner, so light never passes a wall one pixel thin (the user's
// request, 2026-10-19). `skip` is a cell never stopping it (the probe's own).
// `prev` is the open cell the ray left to enter the stopper (`has_prev`
// false when it started in it). `glow` is the brightest haze's light the
// ray met on its way, taken at each step, jump or walk: haze is no seed,
// so no jump is longer than `stride` cells (`HAZE_STRIDE` of its
// cascade's probe spacing).
struct Hit {
    hit: bool,
    // The march ran out of steps before its end: counted as blocked (final
    // review, Important 2), never as open, so no light from where its tip
    // lies comes through rock.
    blocked: bool,
    q: vec2<i32>,
    t: f32,
    prev: vec2<i32>,
    has_prev: bool,
    glow: vec3<f32>,
};

fn march(o: vec2<f32>, dir: vec2<f32>, t0: f32, t1: f32, skip: vec2<i32>, stride: f32) -> Hit {
    var h: Hit;
    h.hit = false;
    h.blocked = false;
    h.has_prev = false;
    h.glow = vec3<f32>(0.0);
    var t = t0;
    let sx = select(-1, 1, dir.x > 0.0);
    let sy = select(-1, 1, dir.y > 0.0);
    for (var n = 0; n < MAX_STEPS; n++) {
        if t >= t1 {
            break;
        }
        let at = o + dir * t;
        let q = vec2<i32>(floor(at));
        if !inside(q) {
            // Outside the grid nothing stops a ray, and it never comes back.
            break;
        }
        if any(q != skip) && stops_at(q) {
            h.hit = true;
            h.q = q;
            h.t = t;
            return h;
        }
        h.prev = q;
        h.has_prev = true;
        // The brightest haze along the way.
        let haze = textureLoad(level_haze, q, 0).rgb;
        if hazy(vec4<f32>(haze, 0.0)) {
            let seen = haze * fade(t * cp.c.w, cp.e.z);
            if luma(seen) > luma(h.glow) {
                h.glow = seen;
            }
        }
        let free = free_at(at);
        if free >= 1.0 {
            t += min(free, stride);
            continue;
        }
        // The next cell boundary along each axis.
        let bx = select(f32(q.x), f32(q.x + 1), dir.x > 0.0);
        let by = select(f32(q.y), f32(q.y + 1), dir.y > 0.0);
        let tx = select(1e9, (bx - at.x) / dir.x, abs(dir.x) > 1e-6);
        let ty = select(1e9, (by - at.y) / dir.y, abs(dir.y) > 1e-6);
        if abs(tx - ty) < 1e-3 {
            // Through a corner: both cells beside it stopping close it.
            let a = q + vec2<i32>(sx, 0);
            let b = q + vec2<i32>(0, sy);
            let tc = t + min(tx, ty);
            if stops_at(a) && stops_at(b) {
                h.hit = true;
                h.q = a;
                h.t = tc;
                h.prev = q;
                h.has_prev = true;
                return h;
            }
        }
        t += max(min(tx, ty), 0.0) + 1e-3;
    }
    if t < t1 && inside(vec2<i32>(floor(o + dir * t))) {
        h.blocked = true;
    }
    return h;
}

// Whether the segment from `a` to `b` (cells) meets no stopper but in `a`'s
// own cell.
fn clear_between(a: vec2<f32>, b: vec2<f32>) -> bool {
    let d = b - a;
    let len = length(d);
    if len < 1e-4 {
        return !stops_at(vec2<i32>(floor(b)));
    }
    let skip = vec2<i32>(floor(a));
    // Its end's cell counts: the upper ray starts there.
    let m = march(a, d / len, 0.0, len + 1e-3, skip, 1e9);
    return !m.hit && !m.blocked && (all(vec2<i32>(floor(b)) == skip) || !stops_at(vec2<i32>(floor(b))));
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
    var ray = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    // Cascade 0 marches from its probe's centre, never stopping in its own
    // cell, so a wall's face gathers the light in front of it.
    let own = vec2<i32>(floor(centre));
    let end = cp.b.z + cp.b.w;
    let h = march(centre, dir, cp.b.z, end, select(vec2<i32>(-1), own, cp.a.x < 0.5), max(spacing * HAZE_STRIDE, 1.0));
    if h.blocked {
        ray = vec4<f32>(0.0);
    } else if h.hit {
        let e = textureLoad(level_em, h.q, 0);
        if max(e.r, max(e.g, e.b)) > 1e-6 {
            let reach = joined(textureLoad(level_reach, h.q, 0).xy);
            var emitted = e.rgb * fade(h.t * cp.c.w, reach);
            // A directional cell casts along its cone only (the
            // directional-glow design): no light falls behind it.
            if look.beams.z > 0.5 {
                emitted = emitted * beam_cone(h.q, dir);
            }
            ray = vec4<f32>(emitted, 0.0);
        } else if h.has_prev {
            // Bounce (plan 15c): the light that reached the face the ray
            // meets, taken in front of it (never through the wall), times
            // the wall's colour.
            let b = clamp(look.frame.w, 0.0, 0.9);
            let last = textureLoad(history, h.prev, 0).rgb;
            // Never NaN or infinity from history (final review, Important 1).
            let lit = select(vec3<f32>(0.0), min(last, vec3<f32>(LIGHT_MAX)), last == last);
            ray = vec4<f32>(textureLoad(albedo_tex, h.q, 0).rgb * lit * b, 0.0);
        } else {
            ray = vec4<f32>(0.0);
        }
    }
    // The haze's light (see gi.wgsl): the brightest the ray met before
    // what stopped it, or the interval's end; `haze` its luminance, which
    // the ray's alpha carries out (no pass reads a cascade's alpha but
    // this one's merge). The ray keeps the brighter of its own haze and
    // the next cascade's, never both: a wide hot patch lights as one hot
    // cell does, however many intervals it spans.
    var haze = 0.0;
    if !h.blocked {
        haze = luma(h.glow);
    }
    var through = 0.0;
    if cp.e.y > 0.5 && ray.a > 0.0 {
        // The next cascade's two directions around this one, bilinear
        // between its four probes around this probe's centre, each probe
        // only when nothing stops the way from this ray's end to where a
        // ray of its starts (the bilinear fix: no light from a probe
        // across a wall). The way is checked once a probe, along this
        // ray's own direction, which lies between its two: half the
        // marches of a check for each, which cost a third of the frame
        // (the render profile), for small differences near bright light.
        let g = centre / (spacing * 2.0) - vec2<f32>(0.5);
        let g0 = floor(g);
        let f = g - g0;
        let q0 = vec2<i32>(g0);
        let tip = centre + dir * end;
        var w = vec4<f32>(0.0);
        for (var j = 0; j < 4; j++) {
            let o = vec2<i32>(j % 2, j / 2);
            let wj = mix(1.0 - f.x, f.x, f32(o.x)) * mix(1.0 - f.y, f.y, f32(o.y));
            let cj = (vec2<f32>(clamp(q0 + o, vec2<i32>(0), vec2<i32>(i32(cp.d.w), i32(cp.e.x)) - vec2<i32>(1))) + vec2<f32>(0.5)) * spacing * 2.0;
            if wj > 0.0 && clear_between(tip, cj + dir * end) {
                w[j] = wj;
            }
        }
        var acc = vec4<f32>(0.0);
        for (var k = 0; k < 2; k++) {
            let dk = d * 2 + k;
            var sum = vec4<f32>(0.0);
            var weight = 0.0;
            for (var j = 0; j < 4; j++) {
                if w[j] > 0.0 {
                    sum += upper_at(q0 + vec2<i32>(j % 2, j / 2), dk) * w[j];
                    weight += w[j];
                }
            }
            if weight > 0.0 {
                acc += sum / weight;
            }
        }
        let upper = acc * 0.5;
        if upper.a > haze {
            // The haze beyond is the brighter: the ray's own gives way.
            ray = vec4<f32>(ray.rgb + upper.rgb, ray.a);
            through = upper.a;
            haze = 0.0;
        } else if haze > 0.0 {
            // Its own is: the haze beyond comes off, taken in its colour.
            let off = h.glow * (upper.a / max(haze, 1e-9));
            ray = vec4<f32>(ray.rgb + max(upper.rgb - off, vec3<f32>(0.0)), ray.a);
        } else {
            ray = vec4<f32>(ray.rgb + upper.rgb, ray.a);
            through = upper.a;
        }
    }
    if !h.blocked {
        ray = vec4<f32>(ray.rgb + h.glow * select(0.0, 1.0, haze > 0.0), ray.a);
    }
    let light = select(vec3<f32>(0.0), min(ray.rgb, vec3<f32>(LIGHT_MAX)), ray.rgb == ray.rgb);
    let carried = max(haze, through);
    return vec4<f32>(light, select(0.0, min(carried, LIGHT_MAX), carried == carried));
}
