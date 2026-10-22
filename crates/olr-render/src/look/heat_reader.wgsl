// Heat's look (issue #39) for the lit terrain and the glow light's
// emission, bound at group HEAT_GROUP (`heat.rs`'s `heat_reader`): a
// level without heat binds a table whose head is off.

@group(HEAT_GROUP) @binding(0) var<uniform> heat_params: HeatParams;
@group(HEAT_GROUP) @binding(1) var heat_tex: texture_2d<u32>;
@group(HEAT_GROUP) @binding(2) var surface_tex: texture_2d<u32>;
@group(HEAT_GROUP) @binding(3) var ramp_tex: texture_2d<f32>;
@group(HEAT_GROUP) @binding(4) var heat_solid_tex: texture_2d<u32>;

struct HeatLook {
    // the ramp's colour (sRGB) and glow (0: the pixel shows its art)
    colour: vec4<f32>,
    // the share of the glow cast as light
    light: f32,
};

// How world pixel `w` looks hot; nothing while nothing is hot, outside
// the heat texture or on a pixel whose material holds no heat.
fn heat_at(w: vec2<i32>) -> HeatLook {
    var out = HeatLook(vec4<f32>(0.0), 0.0);
    if heat_params.head.z < 0.5 {
        return out;
    }
    let size = vec2<i32>(textureDimensions(heat_tex));
    if any(w < vec2<i32>(0)) || any(w >= size) {
        return out;
    }
    let m = heat_material(surface_tex, heat_solid_tex, w);
    let mat = heat_params.mats[m];
    if mat.a.z <= 0.0 {
        return out;
    }
    out.colour = heat_ramp(ramp_tex, m, f32(textureLoad(heat_tex, w, 0).r) / HEAT_SCALE);
    out.light = mat.b.x;
    return out;
}
