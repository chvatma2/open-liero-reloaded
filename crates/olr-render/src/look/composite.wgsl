// The composite (plan 15a Task 10, spec §The picture): each world-target
// texel's albedo lit by the light grid sampled at its centre, never below
// its self-light nor its floor (the solid floor for blood and gibs, as rock;
// the worm floor for worms, plan 15b), with the bloom added (plan 15c),
// graded; a worm's pixel takes the light where it is, as a rope's does
// (#53: no lit side); a marker shows its albedo. Opaque rock and dirt show
// at most their art (#1). A distant layer's pixel takes less of the light
// the farther it is (issue #11). The background round a living worm takes
// less light, the worm halo (issue #54), by how far back it lies. A hot
// pixel's heat light (issue #68) is added after the cap, before the grade.
// The glow beams (the directional-glow design) light the surfaces they
// fall on before the cap and, by the haze, the air and the sky after it.
// Linear light, sRGB bytes in and out.

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var albedo_tex: texture_2d<f32>;
@group(0) @binding(2) var shade_tex: texture_2d<f32>;
@group(0) @binding(3) var light_tex: texture_2d<f32>;
// The occluder map (plan 15b): the living worms' bodies, one texel per
// world pixel over the light grid, each its worm (index + 1), 0 none; the
// worm halo reads it (issue #54).
@group(0) @binding(4) var occluder_tex: texture_2d<u32>;
// The bloom (plan 15c): the light above 1, spread; added before the grade
// at `look.mode.w`'s strength.
@group(0) @binding(5) var bloom_tex: texture_2d<f32>;
// The solid map (#1): an opaque solid shows at most its art.
@group(0) @binding(6) var solid_tex: texture_2d<u32>;

// The view's worm boxes (issue #35): `count.x` boxes, each `[x0, y0, x1,
// y1)` in light-grid texels (the occluder map's), holding every worm texel:
// the worm halo searches the map only near one (issue #54).
struct WormBoxes {
    count: vec4<i32>,
    boxes: array<vec4<i32>, WORM_BOXES>,
};
@group(0) @binding(7) var<uniform> worm_boxes: WormBoxes;
// The heat's light (issue #68): each texel's emitted light (linear, the
// world pass's third target; 0 where nothing is hot, and in full bright
// and light only, which draw issue #39's look of a hot pixel).
@group(0) @binding(8) var heat_tex: texture_2d<f32>;

@group(1) @binding(0) var<uniform> glow_beams: GlowBeams;
// The haze map (the directional-glow design): per world pixel, 0 none to
// 1 the level's haze; 1 x 1 at 1 without a map.
@group(1) @binding(1) var haze_tex: texture_2d<f32>;
// The block map (issue #100): per BEAM_BLOCK-pixel square of the solid map
// (BEAM_BLOCK and BEAM_BLOCK_SHIFT prepended from Rust), 1 when any of its
// pixels is opaque, else 0; past it open, as `solid_px`.
@group(1) @binding(2) var solid_blocks: texture_2d<u32>;
// The beam shadow map (issue #100, `renderer/beam_shadow.rs`): per row (a
// beam's source point, `Rows`), per ray of its span, the distance x
// SHADOW_SCALE at which the ray enters the first pixel that stops it, or
// SHADOW_CLEAR.
@group(1) @binding(3) var beam_shadow: texture_2d<u32>;
@group(1) @binding(4) var<uniform> shadow_rows: Rows;

// A beam's edge widens by this much of its distance, world pixels (the
// picked look's "softest edge").
const BEAM_PENUMBRA: f32 = 0.03;
// The most pixels a shadow march visits, BEAM_MAX_STEPS (2048, given by
// `renderer/glow_beams.rs`, four times the longest reach): a walk that runs
// out counts as blocked, so each beam's reach is cut to fit (`walk_reach`).

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
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

// Whether block `k` of the block map holds no opaque pixel
// (`block_map_open`).
fn block_open(k: vec2<i32>) -> bool {
    return block_map_open(solid_blocks, vec2<i32>(textureDimensions(solid_tex)), k);
}

// Whether nothing opaque lies between `o` and source point `e` of beam
// `b`: an exact grid walk (Amanatides-Woo) over every world pixel the
// segment crosses (both pixels beside a corner it passes exactly through
// too), never `o`'s own. A front source's walk ends at the first pixel
// wholly behind the source's face (`behind_face`), so its own rock never
// shadows it at any angle, however long the walk; a
// backdrop source's (`back`) goes into its source's pixel, so rock or dirt
// over the patch hides it. At most BEAM_MAX_STEPS steps, then blocked;
// issue #100: an open block of the block map is one step, so a walk
// through open air costs a step a block, not a pixel.
fn beam_sees(o: vec2<f32>, e: vec2<f32>, b: GlowBeam, back: bool) -> bool {
    let d = e - o;
    let start = vec2<i32>(floor(o));
    let end = vec2<i32>(floor(e));
    let step = vec2<i32>(select(-1, 1, d.x > 0.0), select(-1, 1, d.y > 0.0));
    let far = 1e30;
    let delta = vec2<f32>(
        select(far, abs(1.0 / d.x), d.x != 0.0),
        select(far, abs(1.0 / d.y), d.y != 0.0),
    );
    let edge = vec2<f32>(
        select(f32(start.x), f32(start.x + 1), d.x > 0.0),
        select(f32(start.y), f32(start.y + 1), d.y > 0.0),
    );
    // Where the walk crosses its first pixel edge each way; it has crossed
    // `n` edges each way, so it is in pixel `start + step * n` and crosses
    // the next at `first + n * delta`.
    let first = vec2<f32>(
        select(far, (edge.x - o.x) / d.x, d.x != 0.0),
        select(far, (edge.y - o.y) / d.y, d.y != 0.0),
    );
    var n = vec2<i32>(0);
    for (var i = 0; i < BEAM_MAX_STEPS; i++) {
        let cell = start + step * n;
        if all(cell == end) {
            return true;
        }
        let next = first + vec2<f32>(n) * delta;
        if min(next.x, next.y) > 1.0 {
            return true;
        }
        // Issue #100: a block with no opaque pixel (none behind a front
        // source's face) is crossed at once, to the last pixel before the
        // edge it leaves by, which the pixel step below crosses.
        let k = cell >> vec2<u32>(BEAM_BLOCK_SHIFT);
        if block_open(k) && (back || !block_behind_face(k, b)) {
            let lo = k * BEAM_BLOCK;
            let left = abs(select(lo, lo + vec2<i32>(BEAM_BLOCK - 1), step > vec2<i32>(0)) - cell);
            let leave = first + vec2<f32>(n + left) * delta;
            let out = min(leave.x, leave.y);
            if out > 1.0 {
                return true;
            }
            let to = clamp(vec2<i32>(ceil((vec2<f32>(out) - first) / delta)), n, n + left);
            if any(to != n) {
                n = to;
                continue;
            }
        }
        if abs(next.x - next.y) < 1e-6 {
            // Exactly through a corner: both pixels beside it (but those of
            // the source behind its face), then the one across.
            let a = cell + vec2<i32>(step.x, 0);
            let c = cell + vec2<i32>(0, step.y);
            if (solid_px(a) && (back || !behind_face(a, b))) || (solid_px(c) && (back || !behind_face(c, b))) {
                return false;
            }
            n += vec2<i32>(1);
        } else if next.x < next.y {
            n.x += 1;
        } else {
            n.y += 1;
        }
        let now = start + step * n;
        if !back && behind_face(now, b) {
            return true;
        }
        if solid_px(now) {
            return false;
        }
    }
    return false;
}

// Whether ray `ray` of row `row` reaches `dist` world pixels from its
// point: 0 or 1; -1 for a ray outside the row's span (no open texel
// beyond the near zone looks along one, but a solid pixel's origin, up to
// a pixel nearer the source, may: final review I-1).
fn ray_reaches(row: u32, ray: u32, dist: f32) -> f32 {
    let r = shadow_rows.r[row];
    let rays = ray_count(r);
    let j = (ray + rays - r.x) % rays;
    if j >= r.y {
        return -1.0;
    }
    let stored = textureLoad(beam_shadow, vec2<u32>(j, row), 0).r;
    return select(0.0, 1.0, stored == SHADOW_CLEAR || dist * SHADOW_SCALE < f32(stored));
}

// How much of source point `e` (point `k` of beam `i`, `b`) `o` sees past
// opaque rock and dirt, 0 to 1. Within the near zone (`near` in
// `renderer/beam_shadow.rs`) the exact walk (`beam_sees`); beyond it, the
// shadow map: the two rays from `e` either side of `o`'s angle, each seen
// when it runs past `o` before anything stops it, blended by where the
// angle falls between them (smooth edges; the accepted inexactness of up
// to about a pixel far from the source); either outside the row's span,
// the exact walk.
fn shadow_seen(i: i32, k: i32, o: vec2<f32>, e: vec2<f32>, b: GlowBeam, back: bool) -> f32 {
    let v = o - e;
    let dist = length(v);
    if dist < beam_near(b.half, b.tan_spread) {
        return select(0.0, 1.0, beam_sees(o, e, b, back));
    }
    var a = atan2(v.y, v.x);
    if a < 0.0 {
        a += 6.2831855;
    }
    let row = u32(i * BEAM_POINTS + k);
    let rays = ray_count(shadow_rows.r[row]);
    let f = a * (f32(rays) / 6.2831855);
    let lo = floor(f);
    let r0 = u32(lo) % rays;
    let a0 = ray_reaches(row, r0, dist);
    let a1 = ray_reaches(row, (r0 + 1u) % rays, dist);
    if min(a0, a1) < 0.0 {
        return select(0.0, 1.0, beam_sees(o, e, b, back));
    }
    return mix(a0, a1, f - lo);
}

// Where a solid world pixel `w` (its centre `c`) looks for beam source
// point `to` from: its open 4-neighbour's centre that faces the source
// most; `found` false when no open one faces it (it shows no face to the
// beam).
struct Start {
    at: vec2<f32>,
    found: bool,
};

fn face_start(w: vec2<i32>, c: vec2<f32>, to: vec2<f32>) -> Start {
    var s: Start;
    s.at = c;
    s.found = false;
    var best = 0.0;
    var sides = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
    for (var k = 0; k < 4; k++) {
        let n = sides[k];
        let facing = dot(vec2<f32>(n), to - c);
        if facing > best && !solid_px(w + n) {
            best = facing;
            s.at = c + vec2<f32>(n);
            s.found = true;
        }
    }
    return s;
}

// The glow beams' light at world point `p` of world pixel `w` (the
// directional-glow design; the spike's `beams_at`): each a crisp wedge as
// wide as its source where it starts, `half + along tan(spread)`, its edge
// a smoothstep over a penumbra of `0.5 + along BEAM_PENUMBRA`, fading as
// `(1 - along/reach)^3`, flickering with its patch's region, shadowed by
// the share of BEAM_POINTS points across its source (half a pixel
// behind its face) that `p` sees past opaque rock and dirt (the live solid
// map, `shadow_seen`). A `solid` pixel (`p` its centre: every texel alike)
// looks from its open neighbour facing the source, never from inside its
// own rock. Cheapest rejects first: `along` outside (0, reach], then the
// wedge; the shadow march last.
fn glow_beams_at(p: vec2<f32>, w: vec2<i32>, solid: bool) -> vec3<f32> {
    var acc = vec3<f32>(0.0);
    let n = min(i32(look.beams.x), MAX_GLOW_BEAMS);
    for (var i = 0; i < n; i++) {
        let b = glow_beams.beams[i];
        let v = p - b.at;
        let along = dot(v, b.dir);
        if along <= 0.0 || along > b.reach {
            continue;
        }
        let perp = vec2<f32>(-b.dir.y, b.dir.x);
        let side = abs(dot(v, perp));
        let width = b.half + along * b.tan_spread;
        let pen = 0.5 + along * BEAM_PENUMBRA;
        if side >= width + pen {
            continue;
        }
        let edge = 1.0 - smoothstep(width - pen * 0.5, width + pen, side);
        let f = 1.0 - along / b.reach;
        var light = b.light * (edge * f * f * f);
        let back = b.group < 0.0;
        let g = u32(abs(b.group));
        if g >= 1u && g <= u32(look.mode.y) {
            light = light * region_flicker(look.groups[g - 1u].flicker, u32(b.region));
        }
        var origin = p;
        if solid {
            let start = face_start(w, p, b.at);
            if !start.found {
                continue;
            }
            origin = start.at;
        }
        var seen = 0.0;
        for (var k = 0; k < BEAM_POINTS; k++) {
            let u = (f32(k) + 0.5) / f32(BEAM_POINTS) * 2.0 - 1.0;
            let src = b.at + perp * (u * b.half) - b.dir * 0.5;
            seen += shadow_seen(i, k, origin, src, b, back);
        }
        acc += light * (seen / f32(BEAM_POINTS));
    }
    return acc;
}

// The haze map at world pixel `w` (clamped: a 1 x 1 map is the same
// everywhere).
fn haze_at(w: vec2<i32>) -> f32 {
    let size = vec2<i32>(textureDimensions(haze_tex));
    return textureLoad(haze_tex, clamp(w, vec2<i32>(0), size - vec2<i32>(1)), 0).r;
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

// Whether occluder-map texel `p` lies within `n` texels of a worm box: no
// worm texel lies farther (issue #35).
fn near_box(p: vec2<i32>, n: i32) -> bool {
    for (var i = 0; i < min(worm_boxes.count.x, WORM_BOXES); i++) {
        let b = worm_boxes.boxes[i];
        if all(p >= b.xy - vec2<i32>(n)) && all(p < b.zw + vec2<i32>(n)) {
            return true;
        }
    }
    return false;
}

// How much of its light background world pixel `w`, `depth` (0-1) back,
// loses to the worm halo (issue #54; `olr_look::engine::worm_halo`): with
// `d` the distance from its centre to the nearest living worm pixel's (1
// right next to one) and `x = (d - 1) / r`, `s (1 - x)^2`, nothing from
// `x = 1` on; `r` and `s` go linearly from WORM_HALO_RADIUS and
// WORM_HALO_STRENGTH at depth 0 to WORM_HALO_RADIUS_MAX and 0 at
// WORM_HALO_FADE (half a byte's step of tolerance: the depth is 8-bit),
// none there or deeper. The same for every texel of a world pixel: never
// finer than the art. Overlapping halos take the nearest worm, the
// strongest, never the sum.
fn worm_halo(w: vec2<i32>, depth: f32) -> f32 {
    if depth >= WORM_HALO_FADE - 0.5 / 255.0 {
        return 0.0;
    }
    let k = clamp(depth / WORM_HALO_FADE, 0.0, 1.0);
    let r = WORM_HALO_RADIUS + (WORM_HALO_RADIUS_MAX - WORM_HALO_RADIUS) * k;
    let s = WORM_HALO_STRENGTH * (1.0 - k) * look.distant.y;
    if s <= 0.0 {
        return 0.0;
    }
    // d < r + 1 only within ceil(r) pixels each way: a window of at most
    // (2 WORM_HALO_RADIUS_MAX + 1)^2, searched only near a worm box.
    let n = min(i32(ceil(r)), i32(ceil(WORM_HALO_RADIUS_MAX)));
    if !near_box(w - vec2<i32>(look.occluders.xy), n) {
        return 0.0;
    }
    let none = 2 * n * n + 1;
    var best = none;
    for (var dy = -n; dy <= n; dy++) {
        for (var dx = -n; dx <= n; dx++) {
            let d2 = dx * dx + dy * dy;
            if d2 < best && worm_in(occluder_tex, look.occluders.xy, w + vec2<i32>(dx, dy)) != 0u {
                best = d2;
            }
        }
    }
    if best >= none {
        return 0.0;
    }
    let x = max((sqrt(f32(best)) - 1.0) / r, 0.0);
    if x >= 1.0 {
        return 0.0;
    }
    return s * (1.0 - x) * (1.0 - x);
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
    let solid = solid_px(w);
    if shade.x > 0.0 || solid {
        cap = min(cap, vec3<f32>(1.0));
        glare = 0.0;
    }
    // A distant layer's pixel (issue #11) takes the ambient and 1 - depth
    // of the light beyond it, the bloom too; only in a level that has one.
    let sky = look.distant.x > 0.5 && shade.w > 0.0 && shade.w < 0.998;
    // The glow beams here (the directional-glow design): on a pixel that
    // does not glow, or over the sky, in the level.
    var beam = vec3<f32>(0.0);
    let in_level = all(w >= vec2<i32>(0)) && all(w < vec2<i32>(look.level.xy));
    let beams_on = look.beams.x > 0.5;
    if beams_on && in_level && (shade.x <= 0.0 || sky) {
        // A solid pixel's beam once for the world pixel (its centre).
        let texel = look.frame.xy + (vec2<f32>(t) + vec2<f32>(0.5)) / look.frame.z;
        beam = glow_beams_at(select(texel, vec2<f32>(w) + vec2<f32>(0.5), solid), w, solid);
    }
    if sky {
        let d = 1.0 - shade.w;
        let base = min(here, look.ambient.rgb);
        here = base + (here - base) * (1.0 - d);
        glare = glare * (1.0 - d);
    }
    // The worm halo (issue #54) on the background only: the terrain's
    // texels (its backing too) and a distant layer's, tagged in the marker
    // channel (a see-through sprite over them blends the tag down: little
    // or no halo); never rock or dirt, a glowing texel, a sprite or a
    // worm. It darkens the light the texel takes and the bloom on it, not
    // its self-light, floor or grade.
    var lit = min(here, cap);
    // The surface term: a beam lights what it falls on as other light does
    // (never the sky, whose share is the air term below).
    if beams_on && !sky {
        lit = min(here + beam, cap);
    }
    if shade.y > 0.125 && shade.y < 0.375 && shade.x <= 0.0 && !solid {
        var depth = 0.0;
        if look.distant.x > 0.5 {
            depth = 1.0 - shade.w;
        }
        let halo = worm_halo(w, depth);
        if halo > 0.0 {
            lit = lit * (1.0 - halo);
            glare = glare * (1.0 - halo);
        }
    }
    let l = max(max(lit, vec3<f32>(shade.x)), least);
    // A hot pixel (issue #68) is its cold look plus its heat's light: never
    // darker than cold, back to exactly its cold look once its glow is 0.
    let heat = textureLoad(heat_tex, t, 0).rgb;
    var c = a * l + bloom_at(g) * glare + heat;
    // The air term: the haze shows a beam in the open air and over the sky,
    // as light of its own (haze 50 the beam itself, 100 twice it).
    if beams_on && look.beams.y > 0.0 && ((!solid && shade.x <= 0.0) || sky) {
        c = c + beam * (look.beams.y * haze_at(w));
    }
    c = c * look.grade.x;
    c = (c - vec3<f32>(0.5)) * look.grade.y + vec3<f32>(0.5);
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = vec3<f32>(luma) + (c - vec3<f32>(luma)) * look.grade.z;
    c = c * look.tint.rgb;
    return vec4<f32>(to_srgb(c), 1.0);
}
