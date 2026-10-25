// The world pass with a look (plan 15a Task 9): the same quads as
// world.wgsl into three targets, the albedo, its shade (self-light,
// marker) and its heat's light (issue #68), blended alike. The terrain's
// self-light is its glow; a hot pixel (issue #39: group 3,
// `heat_reader.wgsl`) adds its heat's light. The terrain (its opaque
// backing too) and a distant layer write the background tag in the marker
// channel (issue #54: `look.distant.z`, 0.25; a marker writes 1, a sprite
// 0), which the composite's worm halo falls on. Glass (the background
// glass design: a see-through level's terrain of alpha 1 to 254) is lit
// as the distant layer behind it, which its glass map filters and frosts.

struct Globals {
    target_size: vec2<f32>,
    _pad: vec2<f32>,
    // the view's world frame: origin x, y (world px), texels per world px, 0
    frame: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(0) var<uniform> look: LookGlobals;
@group(2) @binding(1) var glow_tex: texture_2d<f32>;
@group(2) @binding(2) var groups_tex: texture_2d<u32>;
// Each world pixel's flicker region (plan 15c).
@group(2) @binding(3) var regions_tex: texture_2d<u32>;
// The level's glass map (the background glass design): the filter, and
// the frost in tenths of a world pixel; 1 × 1 white unless see-through.
@group(2) @binding(4) var glass_tex: texture_2d<f32>;

struct Quad {
    @location(0) dst: vec4<f32>,
    @location(1) src: vec4<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) shade: vec3<f32>,
};

struct QuadOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) rect: vec4<f32>,
    @location(2) @interpolate(flat) tint: vec4<f32>,
    @location(3) @interpolate(flat) shade: vec3<f32>,
};

struct LitOut {
    @location(0) colour: vec4<f32>,
    // self-light, marker (the background tag on the background, #54),
    // floor kind, alpha
    @location(1) shade: vec4<f32>,
    // the heat's light (issue #68: linear, added after the composite's
    // cap), alpha
    @location(2) heat: vec4<f32>,
};

// How a hot pixel (issue #39) is drawn: its colour, self-light and heat
// light.
struct Hot {
    colour: vec4<f32>,
    self_light: f32,
    light: vec3<f32>,
};

// Pixel colour `c` with self-light `self_light` hot by `ramp` (its
// material's ramp colour, sRGB, and glow `g`). Lit (issue #68), it is
// drawn as it looks cold plus its heat as emitted light, `to_linear(ramp
// colour) g²`, which the composite adds after the cap: the art shows
// through lit as cold, and a cooling pixel moves steadily back to its
// cold look. Full bright and light only keep issue #39's look: the ramp's
// colour over the art by its glow, and that much self-light whatever the
// darkness.
fn hot(c: vec4<f32>, self_light: f32, ramp: vec4<f32>) -> Hot {
    if ramp.a <= 0.0 {
        return Hot(c, self_light, vec3<f32>(0.0));
    }
    if look.mode.x < 0.5 {
        return Hot(c, self_light, to_linear(ramp.rgb) * ramp.a * ramp.a);
    }
    return Hot(vec4<f32>(mix(c.rgb, ramp.rgb, ramp.a), c.a), max(self_light, ramp.a), vec3<f32>(0.0));
}

@vertex
fn vs_world(@builtin(vertex_index) vi: u32, q: Quad) -> QuadOut {
    let corner = vec2<f32>(f32(vi & 1u), f32((vi >> 1u) & 1u));
    let p = q.dst.xy + corner * q.dst.zw;
    var out: QuadOut;
    out.pos = vec4<f32>(p.x / globals.target_size.x * 2.0 - 1.0, 1.0 - p.y / globals.target_size.y * 2.0, 0.0, 1.0);
    out.uv = q.src.xy + corner * q.src.zw;
    let x0 = min(q.src.x, q.src.x + q.src.z);
    out.rect = vec4<f32>(x0, q.src.y, abs(q.src.z), q.src.w);
    out.tint = q.tint;
    out.shade = q.shade;
    return out;
}

fn texel(in: QuadOut) -> vec2<i32> {
    return vec2<i32>(clamp(floor(in.uv), in.rect.xy, in.rect.xy + in.rect.zw - vec2<f32>(1.0)));
}

@fragment
fn fs_sprite_lit(in: QuadOut) -> LitOut {
    let c = textureLoad(tex, texel(in), 0) * in.tint;
    if c.a <= 0.0 {
        discard;
    }
    var out: LitOut;
    out.colour = c;
    // The floor kind (0 none, 1 solid, 2 worm) is stored halved: 0, 0.5, 1.
    out.shade = vec4<f32>(in.shade.x, in.shade.y, in.shade.z * 0.5, c.a);
    out.heat = vec4<f32>(0.0, 0.0, 0.0, c.a);
    return out;
}

// The terrain lit (`backing`: opaque, a see-through pixel showing its
// back tile, under a level's distant layers; else a see-through pixel,
// alpha 0, is left to them: issue #11).
fn terrain_lit(in: QuadOut, backing: bool) -> LitOut {
    let t = texel(in);
    var c = textureLoad(tex, t, 0);
    if backing {
        c.a = 1.0;
    } else if c.a <= 0.0 {
        discard;
    }
    let gsize = vec2<i32>(textureDimensions(glow_tex));
    let glow = textureLoad(glow_tex, clamp(t, vec2<i32>(0), gsize - vec2<i32>(1)), 0).r;
    let s = max(i32(look.level.z), 1);
    let wsize = vec2<i32>(textureDimensions(groups_tex));
    let w = clamp(t / s, vec2<i32>(0), wsize - vec2<i32>(1));
    let v = textureLoad(groups_tex, w, 0).r;
    // A pixel of no group still glows; it only casts no light.
    var scale = 1.0;
    if v > 0u && v <= min(u32(look.mode.y), 255u) {
        let region = textureLoad(regions_tex, clamp(w, vec2<i32>(0), vec2<i32>(textureDimensions(regions_tex)) - vec2<i32>(1)), 0).r;
        scale = look.groups[v - 1u].light.w * region_flicker(look.groups[v - 1u].flicker, region);
    }
    let h = hot(c, clamp(glow * scale, 0.0, 1.0), heat_at(t / s).colour);
    var out: LitOut;
    out.colour = h.colour;
    out.shade = vec4<f32>(h.self_light, look.distant.z, 0.0, 1.0);
    // Glass leaves the shade to the layer behind it: it is lit as that
    // layer, and its glow shows none (it still casts); hot, it glows.
    if !backing && look.distant.w > 0.5 && c.a < 1.0 {
        out.shade.a = 0.0;
    }
    out.heat = vec4<f32>(h.light, h.colour.a);
    return out;
}

@fragment
fn fs_terrain_lit(in: QuadOut) -> LitOut {
    return terrain_lit(in, false);
}

@fragment
fn fs_terrain_backing_lit(in: QuadOut) -> LitOut {
    return terrain_lit(in, true);
}

// A distant layer's glow mask (issue #62): its image's size, or 1 × 1 full.
@group(1) @binding(1) var distant_mask: texture_2d<f32>;

// A hash of a world pixel, 0-1 (the frost's scatter).
fn glass_hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

// What the glass in front of a distant layer's fragment does to it: its
// filter (`tint`), and the layer texel shown instead (frost: the world
// pixel's own scatter, up to the frost's reach either way, rounded to
// whole layer texels, fixed to the glass).
struct Glass {
    tint: vec3<f32>,
    t: vec2<i32>,
};

fn glass_at(in: QuadOut) -> Glass {
    var g = Glass(vec3<f32>(1.0), texel(in));
    if look.distant.w < 0.5 {
        return g;
    }
    let wp = globals.frame.xy + in.pos.xy / max(globals.frame.z, 1.0);
    let s = max(look.level.z, 1.0);
    let a = vec2<i32>(floor(wp * s));
    let size = vec2<i32>(textureDimensions(glass_tex));
    if any(a < vec2<i32>(0)) || any(a >= size) {
        return g;
    }
    let m = textureLoad(glass_tex, a, 0);
    g.tint = m.rgb;
    let frost = m.a * 25.5;
    if frost > 0.0 {
        let cell = floor(wp);
        let o = frost * (vec2<f32>(glass_hash(cell), glass_hash(cell + vec2<f32>(17.0, 3.0))) * 2.0 - 1.0);
        // Clamped to the layer, not to the part of it this view draws, so
        // a view's edge frosts as another view's inside.
        let uv = vec2<f32>(g.t) + floor(o * s + 0.5);
        g.t = vec2<i32>(clamp(uv, vec2<f32>(0.0), vec2<f32>(textureDimensions(tex)) - vec2<f32>(1.0)));
    }
    return g;
}

// A distant layer's pixel (issue #11): opaque where it covers; its shade
// holds its depth as 1 - alpha (the composite lights it by 1 - depth of
// the light beyond the ambient). Issue #62: glowing (the instance's shade
// y its group, z its strength), its self-light is strength × mask × the
// group's self-light scale, with the group's flicker (the layer one
// region), where it is drawn. Issue #41: hot (group 3 is the layer's own
// heat, at its world pixel), it looks as the lit terrain does (`hot`);
// cold and not glowing, it holds none. The glow light never reads it: a
// layer casts nothing (its light is the level's, baked at its reference
// placement). The background glass design: behind glass, the layer is
// frosted and filtered by the level's glass map (`glass_at`).
@fragment
fn fs_distant_lit(in: QuadOut) -> LitOut {
    let glass = glass_at(in);
    let t = glass.t;
    let c = textureLoad(tex, t, 0);
    if c.a <= 0.0 {
        discard;
    }
    var self_light = 0.0;
    let v = u32(max(in.shade.y, 0.0) + 0.5);
    if v > 0u && v <= min(u32(look.mode.y), 255u) && in.shade.z > 0.0 {
        let msize = vec2<i32>(textureDimensions(distant_mask));
        let m = textureLoad(distant_mask, clamp(t, vec2<i32>(0), msize - vec2<i32>(1)), 0).r;
        let g = look.groups[v - 1u];
        self_light = clamp(in.shade.z * m * g.light.w * region_flicker(g.flicker, 0u), 0.0, 1.0);
    }
    let h = hot(c, self_light, heat_at(t / max(i32(look.level.z), 1)).colour);
    var out: LitOut;
    out.colour = vec4<f32>(h.colour.rgb * glass.tint, 1.0);
    out.shade = vec4<f32>(h.self_light, look.distant.z, 0.0, 1.0 - clamp(in.shade.x, 0.0, 0.99));
    out.heat = vec4<f32>(h.light * glass.tint, 1.0);
    return out;
}
