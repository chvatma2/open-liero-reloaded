// The world pass with a look (plan 15a Task 9): the same quads as
// world.wgsl into two targets, the albedo and its shade (self-light,
// marker), blended alike. The terrain's self-light is its glow, or its
// heat's (issue #39: group 3, `heat_reader.wgsl`). The terrain (its opaque
// backing too) and a distant layer write the background tag in the marker
// channel (issue #54: `look.distant.z`, 0.25; a marker writes 1, a sprite
// 0), which the composite's worm halo falls on.

struct Globals {
    target_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(0) var<uniform> look: LookGlobals;
@group(2) @binding(1) var glow_tex: texture_2d<f32>;
@group(2) @binding(2) var groups_tex: texture_2d<u32>;
// Each world pixel's flicker region (plan 15c).
@group(2) @binding(3) var regions_tex: texture_2d<u32>;

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
};

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
    var self_light = clamp(glow * scale, 0.0, 1.0);
    // A hot pixel (issue #39) shows its material's ramp colour over its
    // art by its glow, and glows that much whatever the darkness.
    let heat = heat_at(t / s);
    if heat.colour.a > 0.0 {
        c = vec4<f32>(mix(c.rgb, heat.colour.rgb, heat.colour.a), c.a);
        self_light = max(self_light, heat.colour.a);
    }
    var out: LitOut;
    out.colour = c;
    out.shade = vec4<f32>(self_light, look.distant.z, 0.0, 1.0);
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

// A distant layer's pixel (issue #11): opaque where it covers; its shade
// holds its depth as 1 - alpha (the composite lights it by 1 - depth of
// the light beyond the ambient). Issue #41: hot (group 3 is the layer's
// own heat, at its world pixel), it shows its ramp colour and glows as the
// lit terrain does; cold, it holds no self-light. The glow light never
// reads it: a layer casts nothing.
@fragment
fn fs_distant_lit(in: QuadOut) -> LitOut {
    let t = texel(in);
    var c = textureLoad(tex, t, 0);
    if c.a <= 0.0 {
        discard;
    }
    var self_light = 0.0;
    let heat = heat_at(t / max(i32(look.level.z), 1));
    if heat.colour.a > 0.0 {
        c = vec4<f32>(mix(c.rgb, heat.colour.rgb, heat.colour.a), c.a);
        self_light = heat.colour.a;
    }
    var out: LitOut;
    out.colour = vec4<f32>(c.rgb, 1.0);
    out.shade = vec4<f32>(self_light, look.distant.z, 0.0, 1.0 - clamp(in.shade.x, 0.0, 0.99));
    return out;
}
