// Glow light: one cascade (see gi.wgsl).


@group(0) @binding(0) var<uniform> cp: GiParams;
@group(0) @binding(1) var level_em: texture_2d<f32>;
@group(0) @binding(2) var level_reach: texture_2d<f32>;
@group(0) @binding(3) var upper: texture_2d<f32>;
@group(0) @binding(4) var seeds: texture_2d<f32>;
// Plan 15c's bounce: each cell's albedo, the last frame's fluence (the
// fluence itself, which `fs_fluence` rewrites only after the cascades;
// issue #35), and the look's globals (`frame.w`: the bounce, 0-0.9).
@group(0) @binding(5) var albedo_tex: texture_2d<f32>;
@group(0) @binding(6) var history: texture_2d<f32>;
@group(0) @binding(7) var<uniform> look: LookGlobals;

// The most steps of one ray's march (a ray grazing a wall steps one cell
// at a time); past it the ray counts as open.
const MAX_STEPS: i32 = 128;

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

// Whether a worm's cell (marked 2 + its rock share by the emission) is a
// worm to a ray `t` cells from its probe: within `worms` cells (the
// worm-shadow follow-up, 2026-10-20: a worm throws only a short shadow).
fn worm_cell(e: vec4<f32>) -> bool {
    return e.a >= 1.5;
}

// Whether cell `q` stops a ray `t` cells out: a worm only within `worms`
// cells; past that, what lies under it (its rock, its glow) does.
fn blocks(q: vec2<i32>, t: f32, worms: f32) -> bool {
    if !inside(q) {
        return false;
    }
    let e = textureLoad(level_em, q, 0);
    if worm_cell(e) {
        return t <= worms || e.a - 2.0 >= 0.5 || max(e.r, max(e.g, e.b)) > 1e-6;
    }
    return stops(e);
}

// How far a ray at `at` may jump: the jump flood's distance to the nearest
// stopper, less a cell and a half; 0 near one.
fn free_at(at: vec2<f32>) -> f32 {
    let t = textureLoad(seeds, vec2<i32>(floor(at)), 0);
    if t.x < 0.0 {
        return 1e9;
    }
    let s = vec2<f32>(joined(t.xy), joined(t.zw));
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
// false when it started in it).
struct Hit {
    hit: bool,
    // The ray met a worm within its shadow's reach: dark, no bounce.
    worm: bool,
    // The march ran out of steps before its end: counted as blocked (final
    // review, Important 2), never as open, so no light from where its tip
    // lies comes through rock.
    blocked: bool,
    q: vec2<i32>,
    t: f32,
    prev: vec2<i32>,
    has_prev: bool,
};

fn march(o: vec2<f32>, dir: vec2<f32>, t0: f32, t1: f32, skip: vec2<i32>, worms: f32) -> Hit {
    var h: Hit;
    h.hit = false;
    h.worm = false;
    h.blocked = false;
    h.has_prev = false;
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
        if any(q != skip) && blocks(q, t, worms) {
            h.hit = true;
            h.q = q;
            h.t = t;
            let e = textureLoad(level_em, q, 0);
            h.worm = worm_cell(e) && t <= worms;
            return h;
        }
        h.prev = q;
        h.has_prev = true;
        let free = free_at(at);
        if free >= 1.0 {
            t += free;
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
            if blocks(a, tc, worms) && blocks(b, tc, worms) {
                h.hit = true;
                h.q = a;
                h.t = tc;
                h.worm = worm_cell(textureLoad(level_em, a, 0)) && tc <= worms;
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
        return !blocks(vec2<i32>(floor(b)), 1e9, 0.0);
    }
    let skip = vec2<i32>(floor(a));
    // Its end's cell counts: the upper ray starts there.
    let m = march(a, d / len, 0.0, len + 1e-3, skip, 0.0);
    return !m.hit && !m.blocked && (all(vec2<i32>(floor(b)) == skip) || !blocks(vec2<i32>(floor(b)), 1e9, 0.0));
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
    // A worm throws a shadow only `WORM_SHADOW` world pixels long.
    let worms = WORM_SHADOW / max(cp.c.w, 1.0);
    let h = march(centre, dir, cp.b.z, end, select(vec2<i32>(-1), own, cp.a.x < 0.5), worms);
    if h.blocked || h.worm {
        ray = vec4<f32>(0.0);
    } else if h.hit {
        let e = textureLoad(level_em, h.q, 0);
        if max(e.r, max(e.g, e.b)) > 1e-6 {
            let reach = joined(textureLoad(level_reach, h.q, 0).xy);
            ray = vec4<f32>(e.rgb * fade(h.t * cp.c.w, reach), 0.0);
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
    if cp.e.y > 0.5 && ray.a > 0.0 {
        // The next cascade's two directions around this one, bilinear
        // between its four probes around this probe's centre, each only
        // when nothing stops the way from this ray's end to where its ray
        // starts (the bilinear fix: no light from a probe across a wall).
        let g = centre / (spacing * 2.0) - vec2<f32>(0.5);
        let g0 = floor(g);
        let f = g - g0;
        let q0 = vec2<i32>(g0);
        let tip = centre + dir * end;
        var acc = vec3<f32>(0.0);
        for (var k = 0; k < 2; k++) {
            let dk = d * 2 + k;
            let ak = (f32(dk) + 0.5) / cp.d.x * TAU;
            let dir_k = vec2<f32>(cos(ak), sin(ak));
            var sum = vec3<f32>(0.0);
            var weight = 0.0;
            for (var j = 0; j < 4; j++) {
                let o = vec2<i32>(j % 2, j / 2);
                let wj = mix(1.0 - f.x, f.x, f32(o.x)) * mix(1.0 - f.y, f.y, f32(o.y));
                if wj <= 0.0 {
                    continue;
                }
                let qj = q0 + o;
                let cj = (vec2<f32>(clamp(qj, vec2<i32>(0), vec2<i32>(i32(cp.d.w), i32(cp.e.x)) - vec2<i32>(1))) + vec2<f32>(0.5)) * spacing * 2.0;
                if clear_between(tip, cj + dir_k * end) {
                    sum += upper_at(qj, dk).rgb * wj;
                    weight += wj;
                }
            }
            if weight > 0.0 {
                acc += sum / weight;
            }
        }
        ray = vec4<f32>(ray.rgb + ray.a * acc * 0.5, ray.a);
    }
    return vec4<f32>(select(vec3<f32>(0.0), min(ray.rgb, vec3<f32>(LIGHT_MAX)), ray.rgb == ray.rgb), ray.a);
}
