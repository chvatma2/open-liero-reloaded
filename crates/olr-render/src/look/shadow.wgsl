// Placed lights' shadow maps (plan 15a Task 11): row i is light i, column
// r the distance from the light to the first rock or dirt along ray r of
// SHADOW_RAYS, marched in world-pixel steps from the light's radius out to
// its reach (or the level's far side). Outside the level is open. A light
// whose centre is solid gets 0 on every ray: it lights nothing.

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var<uniform> lights: Lights;
@group(0) @binding(2) var solid_tex: texture_2d<u32>;

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

@fragment
fn fs_shadow(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let ray = floor(p.x);
    let row = u32(p.y);
    let a = lights.v[row * 2u];
    let pos = a.xy;
    if solid_at(pos) {
        return vec4<f32>(0.0);
    }
    let angle = (ray + 0.5) / SHADOW_RAYS * 6.2831853;
    let dir = vec2<f32>(cos(angle), sin(angle));
    // As far as the reach, and never past the level's farthest corner.
    let level = look.level.xy;
    let corner = max(abs(pos), abs(pos - level));
    let far = length(corner) + 1.0;
    let end = min(a.z + a.w, far);
    var d = a.z;
    loop {
        if d >= end {
            break;
        }
        if solid_at(pos + dir * d) {
            return vec4<f32>(d);
        }
        d += 1.0;
    }
    return vec4<f32>(1e9);
}
