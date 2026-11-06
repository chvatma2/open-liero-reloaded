// The production line design: grain moves as a gather, one point a
// destination world pixel (`HeatOp::Move`).

// The heat as it was before the moves (the scratch copy), and the look's
// solid map as now.
@group(0) @binding(0) var source_tex: texture_2d<u32>;
@group(0) @binding(1) var solid_tex: texture_2d<u32>;

// A source of `COLD` is none: the destination starts cold.
const COLD: u32 = 0xffffffffu;

struct MoveOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) from_px: vec2<u32>,
    @location(1) @interpolate(flat) to_px: vec2<u32>,
};

fn point(to: vec2<u32>, size: vec2<u32>, from_px: vec2<u32>) -> MoveOut {
    var out: MoveOut;
    let s = vec2<f32>(max(size, vec2<u32>(1u)));
    let c = (vec2<f32>(to) + 0.5) / s;
    out.pos = vec4<f32>(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0, 0.0, 1.0);
    out.from_px = from_px;
    out.to_px = to;
    return out;
}

// `m`: destination x, y, source x, y.
@vertex
fn vs_move_heat(@location(0) m: vec4<u32>) -> MoveOut {
    return point(m.xy, textureDimensions(source_tex), m.zw);
}

@vertex
fn vs_move_seen(@location(0) m: vec4<u32>) -> MoveOut {
    return point(m.xy, textureDimensions(solid_tex), m.zw);
}

@fragment
fn fs_move_heat(in: MoveOut) -> @location(0) vec4<u32> {
    if in.from_px.x == COLD {
        return vec4<u32>(0u);
    }
    return textureLoad(source_tex, vec2<i32>(in.from_px), 0);
}

// The solid map as now is what the heat has seen at the destination, so
// the layer reset after it keeps the grain's heat.
@fragment
fn fs_move_seen(in: MoveOut) -> @location(0) vec4<u32> {
    return textureLoad(solid_tex, vec2<i32>(in.to_px), 0);
}
