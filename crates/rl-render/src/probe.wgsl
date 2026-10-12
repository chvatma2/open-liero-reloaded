// rl-render's shader-headroom probe (bench only, plan 8): a box blur of
// `taps` texels of the last pass's output, over the whole target. Its GPU
// time per pass says how many full-screen effects fit in a frame. The
// first pass reads the seed: the drawn frame (`fs_seed`), so the passes
// blur a real picture, never a blank texture a GPU would compress.

struct Probe {
    size: vec2<f32>,
    taps: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> probe: Probe;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_probe(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    var sum = vec4<f32>(0.0);
    for (var i = 0u; i < probe.taps; i = i + 1u) {
        let o = vec2<f32>(f32(i % 8u) - 3.5, f32(i / 8u) - 3.5);
        sum = sum + textureSampleLevel(src, smp, (frag.xy + o) / probe.size, 0.0);
    }
    return sum / f32(probe.taps);
}

// The seed (untimed): the drawn frame, the world target, stretched over the
// probe's target, one tap a pixel.
@fragment
fn fs_seed(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    return textureSampleLevel(src, smp, frag.xy / probe.size, 0.0);
}
