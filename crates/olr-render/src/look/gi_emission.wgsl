// Glow light: the emission grid (see gi.wgsl).


@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var<uniform> gp: GiParams;
@group(0) @binding(2) var glow_tex: texture_2d<f32>;
@group(0) @binding(3) var groups_tex: texture_2d<u32>;
@group(0) @binding(4) var solid_tex: texture_2d<u32>;
@group(0) @binding(5) var terrain_tex: texture_2d<f32>;


@fragment
fn fs_emission(@builtin(position) p: vec4<f32>) -> GiOut {
    let cell = vec2<i32>(p.xy);
    let s = max(i32(gp.c.w), 1);
    let a = max(i32(look.level.z), 1);
    let level = vec2<i32>(look.level.xy);
    let count = u32(look.mode.y);
    var em = vec3<f32>(0.0);
    var solid = 0.0;
    var reach = 0.0;
    var n = 0.0;
    for (var dy = 0; dy < s; dy++) {
        for (var dx = 0; dx < s; dx++) {
            let w = cell * s + vec2<i32>(dx, dy);
            if any(w >= level) {
                continue;
            }
            n += 1.0;
            if textureLoad(solid_tex, clamped(w, textureDimensions(solid_tex)), 0).r != 1u {
                solid += 1.0;
            }
            let v = textureLoad(groups_tex, clamped(w, textureDimensions(groups_tex)), 0).r;
            let art = clamped(w * a + vec2<i32>(a / 2), textureDimensions(glow_tex));
            let g = textureLoad(glow_tex, art, 0).r;
            if g > 0.0 && v > 0u && v <= min(count, 255u) {
                let info = look.group_info[v - 1u];
                var colour = vec3<f32>(1.0);
                if info.y > 0.5 {
                    colour = to_linear(textureLoad(terrain_tex, clamped(art, textureDimensions(terrain_tex)), 0).rgb);
                }
                em += colour * look.groups[v - 1u].rgb * g;
                reach = max(reach, info.x);
            }
        }
    }
    var out: GiOut;
    let k = 1.0 / max(n, 1.0);
    out.emission = vec4<f32>(em * k, solid * k);
    out.reach = vec4<f32>(reach, 0.0, 0.0, 0.0);
    return out;
}

