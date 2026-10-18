// The view's light grid (plan 15a Task 10): one texel per world pixel of
// the world target. `fs_sum` gathers the light (ambient; the glow light and
// placed lights join it), `fs_blur_h` and `fs_blur_v` blur it by the
// level's softness, and `fs_blur_v` also caps it at max light and bands it
// into light steps.

@group(0) @binding(0) var<uniform> look: LookGlobals;
@group(0) @binding(1) var src: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_sum(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(look.ambient.rgb, 1.0);
}

fn blur(p: vec2<i32>, dir: vec2<i32>) -> vec3<f32> {
    let r = clamp(i32(look.tint.w), 0, 4);
    let size = vec2<i32>(textureDimensions(src));
    var acc = vec3<f32>(0.0);
    for (var i = -r; i <= r; i++) {
        let q = clamp(p + dir * i, vec2<i32>(0), size - vec2<i32>(1));
        acc += textureLoad(src, q, 0).rgb;
    }
    return acc / f32(2 * r + 1);
}

@fragment
fn fs_blur_h(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(blur(vec2<i32>(p.xy), vec2<i32>(1, 0)), 1.0);
}

@fragment
fn fs_blur_v(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let max_light = look.ambient.w;
    var c = min(blur(vec2<i32>(p.xy), vec2<i32>(0, 1)), vec3<f32>(max_light));
    let steps = look.grade.w;
    if steps > 0.0 {
        c = floor(c / max_light * steps + vec3<f32>(0.5)) / steps * max_light;
    }
    return vec4<f32>(max(c, vec3<f32>(0.0)), 1.0);
}
