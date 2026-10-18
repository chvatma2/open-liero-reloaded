// Glow light: the fluence per cell (see gi.wgsl).


@group(0) @binding(0) var cascade0: texture_2d<f32>;

@fragment
fn fs_fluence(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    // Cascade 0: four directions in a 2 x 2 block per cell.
    let t = vec2<i32>(p.xy) * 2;
    var acc = vec3<f32>(0.0);
    for (var i = 0; i < 4; i++) {
        acc += textureLoad(cascade0, t + vec2<i32>(i % 2, i / 2), 0).rgb;
    }
    return vec4<f32>(acc * 0.25, 1.0);
}
