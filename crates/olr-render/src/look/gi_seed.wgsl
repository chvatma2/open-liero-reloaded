// Glow light: the jump flood's seeds (see gi.wgsl). A cell that stops a
// ray (half or more solid, or glowing) is its own nearest stopper; others
// have none yet (`NO_SEED`).

@group(0) @binding(0) var emission: texture_2d<f32>;

@fragment
fn fs_seed(@builtin(position) p: vec4<f32>) -> @location(0) vec2<u32> {
    let c = vec2<i32>(p.xy);
    if stops(textureLoad(emission, c, 0)) {
        return vec2<u32>(c);
    }
    return vec2<u32>(NO_SEED);
}
