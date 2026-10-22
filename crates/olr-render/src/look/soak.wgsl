// Light in solids (plan 15a Task 14a, spec §Light in solids): an opaque
// solid texel of the light grid takes the most light within the soak of it,
// faded by distance, `fs_soak_h` across, then `fs_soak_v` down, which also
// raises it to the solid floor. Air keeps its light, and so does a clear
// solid (the solid map holds it as background). The worm-shadow follow-up
// (the user's playtest, 2026-10-20): from 2 px into rock a texel also takes
// the air light along each row (the across pass spreads air's light along
// air, stopping at rock), so a worm's short shade over a face colours only
// the face, not a column into the rock.

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var solid_tex: texture_2d<u32>;
// The light sum: what air keeps.
@group(0) @binding(3) var sum_tex: texture_2d<f32>;
// The occluder map (plan 15b): where the worms are.
@group(0) @binding(4) var occluder_tex: texture_2d<u32>;
// The rows the across soak wrote, for the deeper rock (read down only).
@group(0) @binding(5) var rows_tex: texture_2d<f32>;

// The view's worm boxes (issue #35): `count.x` boxes, each `[x0, y0, x1,
// y1)` in light-grid texels (the occluder map's), holding every worm texel.
struct WormBoxes {
    count: vec4<i32>,
    boxes: array<vec4<i32>, WORM_BOXES>,
};
@group(0) @binding(6) var<uniform> worm_boxes: WormBoxes;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

// Whether grid texel `p` is an opaque solid; outside the level is open.
fn opaque(p: vec2<i32>) -> bool {
    let w = vec2<i32>(floor(look.frame.xy)) - vec2<i32>(i32(LIGHT_PAD)) + p;
    let size = min(vec2<i32>(look.level.xy), vec2<i32>(textureDimensions(solid_tex)));
    if any(w < vec2<i32>(0)) || any(w >= size) {
        return false;
    }
    return textureLoad(solid_tex, w, 0).r != 1u;
}

fn soak(p: vec2<i32>, dir: vec2<i32>) -> vec3<f32> {
    let own = textureLoad(src, p, 0).rgb;
    let r = clamp(i32(look.mode.z), 0, MAX_SOAK);
    if r == 0 || !opaque(p) {
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

// Grid texel `p`'s world pixel.
fn world_at(p: vec2<i32>) -> vec2<i32> {
    return vec2<i32>(floor(look.frame.xy)) - vec2<i32>(i32(LIGHT_PAD)) + p;
}

// Whether grid texel `p` is within `n` texels of a worm box: no worm
// texel lies farther (issue #35: the occluder map is read only near one).
fn near_box(p: vec2<i32>, n: i32) -> bool {
    for (var i = 0; i < min(worm_boxes.count.x, WORM_BOXES); i++) {
        let b = worm_boxes.boxes[i];
        if all(p >= b.xy - vec2<i32>(n)) && all(p < b.zw + vec2<i32>(n)) {
            return true;
        }
    }
    return false;
}

// Whether air texel `p` may lie in a worm's short shade: a worm pixel
// within twice `WORM_SHADOW` + 2 of it (the cascades' merging carries the
// shade that far; sampled every 2 px: a worm is wider).
fn near_worm(p: vec2<i32>) -> bool {
    let n = 2 * i32(WORM_SHADOW) + 2;
    if !near_box(p, n) {
        return false;
    }
    let w = world_at(p);
    for (var dy = -n; dy <= n; dy += 2) {
        for (var dx = -n; dx <= n; dx += 2) {
            if worm_in(occluder_tex, look.occluders.xy, w + vec2<i32>(dx, dy)) != 0u {
                return true;
            }
        }
    }
    return false;
}

// Texel `q`'s across-soaked light: an opaque texel's soak along the row,
// air's own.
fn across(q: vec2<i32>) -> vec3<f32> {
    if !opaque(q) {
        return textureLoad(src, q, 0).rgb;
    }
    return soak(q, vec2<i32>(1, 0));
}

// For the rock deeper than its face, texel `p`'s light (`own` its across
// soak): its own, or, in a worm's shade, the nearest unshaded texel's of
// the same kind (air or solid) along its row each way, within four times
// the shade and more (the worm-shadow follow-up: the shade colours the
// face, not the rock under it).
fn row_light(p: vec2<i32>, own: vec3<f32>) -> vec3<f32> {
    if !near_worm(p) {
        return own;
    }
    let n = 4 * i32(WORM_SHADOW) + 8;
    let size = vec2<i32>(textureDimensions(src));
    let kind = opaque(p);
    var best = own;
    for (var side = -1; side <= 1; side += 2) {
        for (var i = 1; i <= n; i++) {
            let q = p + vec2<i32>(i * side, 0);
            if any(q < vec2<i32>(0)) || any(q >= size) || opaque(q) != kind {
                break;
            }
            if !near_worm(q) {
                best = max(best, across(q));
                break;
            }
        }
    }
    return best;
}

struct Across {
    @location(0) soaked: vec4<f32>,
    // the rows for the deeper rock
    @location(1) rows: vec4<f32>,
};

@fragment
fn fs_soak_h(@builtin(position) p: vec4<f32>) -> Across {
    let t = vec2<i32>(p.xy);
    let a = across(t);
    var out: Across;
    out.soaked = vec4<f32>(a, 1.0);
    out.rows = vec4<f32>(row_light(t, a), 1.0);
    return out;
}

@fragment
fn fs_soak_v(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let t = vec2<i32>(p.xy);
    if !opaque(t) {
        return vec4<f32>(textureLoad(sum_tex, t, 0).rgb, 1.0);
    }
    let r = clamp(i32(look.mode.z), 0, MAX_SOAK);
    let size = vec2<i32>(textureDimensions(src));
    // Down the column: the face light as before (air's own light), and the
    // rows' air light; the nearest air's distance chooses between them.
    var face = textureLoad(src, t, 0).rgb;
    var rows = textureLoad(rows_tex, t, 0).rgb;
    var depth = r + 1;
    for (var i = 1; i <= r; i++) {
        let k = fade(f32(i), f32(r) + 1.0);
        for (var side = -1; side <= 1; side += 2) {
            let q = clamp(t + vec2<i32>(0, i * side), vec2<i32>(0), size - vec2<i32>(1));
            let h = textureLoad(src, q, 0).rgb;
            if opaque(q) {
                face = max(face, h * k);
            } else {
                depth = min(depth, i);
                face = max(face, h * k);
            }
            rows = max(rows, textureLoad(rows_tex, q, 0).rgb * k);
        }
    }
    // The face (1 px in) keeps the shade; 2 px in, half; deeper, the rows.
    var c = mix(face, rows, clamp(f32(depth - 1), 0.0, 2.0) * 0.5);
    c = max(c, look.solid.rgb);
    return vec4<f32>(c, 1.0);
}
