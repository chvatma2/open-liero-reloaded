// Glow light: the jump flood's seeds (see gi.wgsl). A cell that stops a
// ray (half or more solid, or glowing) is its own nearest stopper; others
// have none yet (-1). Coordinates are `split`, each axis in two halves.

@group(0) @binding(0) var emission: texture_2d<f32>;

@fragment
fn fs_seed(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let c = vec2<i32>(p.xy);
    if stops(textureLoad(emission, c, 0)) {
        return vec4<f32>(split(f32(c.x)), split(f32(c.y)));
    }
    return vec4<f32>(-1.0, 0.0, -1.0, 0.0);
}
