// Placed lights' shadow maps (plan 15a Task 11): row i is light i, column
// r (`split`) the distance from the light to where ray r of SHADOW_RAYS
// enters the first rock or dirt, walked pixel by pixel from the light's
// radius out to its reach, within the level's box. Outside the level is open. A light
// whose centre is solid gets 0 on every ray: it lights nothing. A living
// worm (the occluder map, plan 15b) stops a ray too, except the worm the
// light's centre is in, or the worm it belongs to.

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var<uniform> lights: Lights;
@group(0) @binding(2) var solid_tex: texture_2d<u32>;
@group(1) @binding(0) var occluder_tex: texture_2d<u32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn solid_at(p: vec2<f32>) -> bool {
    let c = vec2<i32>(floor(p));
    let size = vec2<i32>(textureDimensions(solid_tex));
    if any(c < vec2<i32>(0)) || any(c >= size) {
        return false;
    }
    // Material 1 is background; rock (0) and dirt (2) block light.
    return textureLoad(solid_tex, c, 0).r != 1u;
}

// A worm other than `own` at `p` (plan 15b).
fn worm_blocks(p: vec2<f32>, own: u32) -> bool {
    let id = worm_in(occluder_tex, look.occluders.xy, vec2<i32>(floor(p)));
    return id != 0u && id != own;
}

@fragment
fn fs_shadow(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let ray = floor(p.x);
    let row = u32(p.y);
    let a = lights.v[row * 2u];
    let pos = a.xy;
    if solid_at(pos) {
        return vec4<f32>(split(0.0), 0.0, 0.0);
    }
    // The light's own worm: its owner (a muzzle flash, whatever the art at
    // its centre), else the worm its centre is in (an explosion).
    var own = u32(round(lights.v[row * 2u + 1u].w));
    if own == 0u {
        own = worm_in(occluder_tex, look.occluders.xy, vec2<i32>(floor(pos)));
    }
    let angle = (ray + 0.5) / SHADOW_RAYS * 6.2831853;
    let dir = vec2<f32>(cos(angle), sin(angle));
    // Only the part of the ray inside the level can meet rock (outside is
    // open): clip it to the level's box (a slab test), so a light however
    // far away marches at most across the level. A light inside starts at
    // its radius as before.
    let size = min(look.level.xy, vec2<f32>(textureDimensions(solid_tex)));
    var t0 = a.z;
    var t1 = a.z + a.w;
    for (var k = 0; k < 2; k++) {
        if abs(dir[k]) < 1e-6 {
            if pos[k] < 0.0 || pos[k] > size[k] {
                return vec4<f32>(split(1e9), 0.0, 0.0);
            }
        } else {
            let ta = (0.0 - pos[k]) / dir[k];
            let tb = (size[k] - pos[k]) / dir[k];
            t0 = max(t0, min(ta, tb));
            t1 = min(t1, max(ta, tb) + 1.0);
        }
    }
    if !(t1 > t0) {
        return vec4<f32>(split(1e9), 0.0, 0.0);
    }
    // Every world pixel the ray passes through, in order (a grid walk, the
    // user's request 2026-10-19: rock however thin stops light; a 1-pixel
    // step could pass between rock pixels touching at a corner), from the
    // start of its part in the level; the distance stored is where it
    // enters the first rock, dirt or other worm. Never more cells than the
    // box's perimeter twice (f32 stops counting far away).
    let start = max(t0, a.z);
    var t = start;
    let p0 = pos + dir * start;
    var cell = vec2<i32>(floor(p0));
    let stepv = vec2<i32>(select(-1, 1, dir.x >= 0.0), select(-1, 1, dir.y >= 0.0));
    let inv = vec2<f32>(1.0 / max(abs(dir.x), 1e-9), 1.0 / max(abs(dir.y), 1e-9));
    let first = vec2<f32>(
        select(p0.x - floor(p0.x), floor(p0.x) + 1.0 - p0.x, dir.x >= 0.0),
        select(p0.y - floor(p0.y), floor(p0.y) + 1.0 - p0.y, dir.y >= 0.0),
    );
    var next = vec2<f32>(start) + first * inv;
    let steps = i32(4.0 * (size.x + size.y)) + 4;
    for (var i = 0; i < steps; i++) {
        if t >= t1 {
            break;
        }
        let centre = vec2<f32>(cell) + vec2<f32>(0.5);
        if solid_at(centre) || worm_blocks(centre, own) {
            return vec4<f32>(split(t), 0.0, 0.0);
        }
        if next.x < next.y {
            t = next.x;
            next.x += inv.x;
            cell.x += stepv.x;
        } else {
            t = next.y;
            next.y += inv.y;
            cell.y += stepv.y;
        }
    }
    return vec4<f32>(split(1e9), 0.0, 0.0);
}
