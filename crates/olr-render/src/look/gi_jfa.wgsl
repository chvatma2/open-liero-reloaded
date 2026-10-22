// Glow light: one jump-flood pass (see gi.wgsl): each cell keeps the
// nearest stopper among its own and those of the cells `step` away in the
// eight directions.

@group(0) @binding(0) var<uniform> jp: vec4<f32>;
@group(0) @binding(1) var seeds: texture_2d<u32>;

@fragment
fn fs_jfa(@builtin(position) p: vec4<f32>) -> @location(0) vec2<u32> {
    let c = vec2<i32>(p.xy);
    let size = vec2<i32>(textureDimensions(seeds));
    let step = i32(jp.x);
    var best = vec2<f32>(-1.0);
    var best_d = 1e30;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let q = c + vec2<i32>(i, j) * step;
            if any(q < vec2<i32>(0)) || any(q >= size) {
                continue;
            }
            let t = textureLoad(seeds, q, 0).xy;
            if t.x == NO_SEED {
                continue;
            }
            let s = vec2<f32>(t);
            let d = distance(s, vec2<f32>(c));
            if d < best_d {
                best_d = d;
                best = s;
            }
        }
    }
    if best.x < 0.0 {
        return vec2<u32>(NO_SEED);
    }
    return vec2<u32>(best);
}
