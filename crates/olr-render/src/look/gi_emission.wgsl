// Glow light: the emission grid (see gi.wgsl).


@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var<uniform> gp: GiParams;
@group(0) @binding(2) var glow_tex: texture_2d<f32>;
@group(0) @binding(3) var groups_tex: texture_2d<u32>;
@group(0) @binding(4) var solid_tex: texture_2d<u32>;
@group(0) @binding(5) var terrain_tex: texture_2d<f32>;
// Play's glowing discs this frame (plan 15c, `gi_dynamic.wgsl`): radiance
// per cell, and their reach (unsplit; 60000 or more unlimited).
@group(0) @binding(6) var dynamic_tex: texture_2d<f32>;
@group(0) @binding(7) var dynamic_reach: texture_2d<f32>;
// Each world pixel's flicker region (plan 15c).
@group(0) @binding(8) var regions_tex: texture_2d<u32>;
// The occluder map (plan 15b): a living worm's world pixel is solid.
@group(1) @binding(0) var occluder_tex: texture_2d<u32>;


@fragment
fn fs_emission(@builtin(position) p: vec4<f32>) -> GiOut {
    let cell = vec2<i32>(p.xy);
    let s = max(i32(gp.c.w), 1);
    let a = max(i32(look.level.z), 1);
    // The level the device keeps: past a crop is open.
    let level = min(vec2<i32>(look.level.xy), vec2<i32>(textureDimensions(solid_tex)));
    let count = u32(look.mode.y);
    // At most a ray's radiance, never NaN (final review, Important 1).
    let summed = textureLoad(dynamic_tex, cell, 0).rgb;
    let disc = select(vec3<f32>(0.0), min(summed, vec3<f32>(16384.0)), summed == summed);
    // A disc's cells never count as a worm: a muzzle flash shines from
    // inside its worm's outline.
    let lit = max(disc.r, max(disc.g, disc.b)) > 0.0;
    var em = vec3<f32>(0.0);
    var solid = 0.0;
    var reach = 0.0;
    var n = 0.0;
    var albedo = vec3<f32>(0.0);
    for (var dy = 0; dy < s; dy++) {
        for (var dx = 0; dx < s; dx++) {
            let w = cell * s + vec2<i32>(dx, dy);
            if any(w >= level) {
                continue;
            }
            n += 1.0;
            let worm = !lit && worm_in(occluder_tex, look.occluders.xy, w) != 0u;
            let art = clamped(w * a + vec2<i32>(a / 2), textureDimensions(glow_tex));
            if worm {
                solid += 1.0;
            } else if textureLoad(solid_tex, clamped(w, textureDimensions(solid_tex)), 0).r != 1u {
                // Rock and dirt reflect by their colour; a worm reflects nothing.
                solid += 1.0;
                albedo += to_linear(textureLoad(terrain_tex, clamped(art, textureDimensions(terrain_tex)), 0).rgb);
            }
            let v = textureLoad(groups_tex, clamped(w, textureDimensions(groups_tex)), 0).r;
            let g = textureLoad(glow_tex, art, 0).r;
            if g > 0.0 && v > 0u && v <= min(count, 255u) {
                let info = look.group_info[v - 1u];
                var colour = vec3<f32>(1.0);
                if info.y > 0.5 {
                    colour = to_linear(textureLoad(terrain_tex, clamped(art, textureDimensions(terrain_tex)), 0).rgb);
                }
                let region = textureLoad(regions_tex, clamped(w, textureDimensions(regions_tex)), 0).r;
                em += colour * look.groups[v - 1u].rgb * g * region_flicker(look.group_flicker[v - 1u], region);
                reach = max(reach, info.x);
            }
        }
    }
    var out: GiOut;
    let k = 1.0 / max(n, 1.0);
    if lit {
        let d = textureLoad(dynamic_reach, cell, 0).r;
        reach = max(reach, select(d, 1e9, d >= 59999.0));
    }
    out.emission = vec4<f32>(em * k + disc, solid * k);
    out.reach = vec4<f32>(split(reach), 0.0, 0.0);
    out.albedo = vec4<f32>(albedo / max(solid, 1.0), 1.0);
    return out;
}

