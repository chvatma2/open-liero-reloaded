// Glow light: play's lights as glowing discs (plan 15c, spec "My calls"),
// drawn into the dynamic emission layer before the emission pass. One
// instance a disc, a quad over its cells; a cell whose centre is in the
// disc and in the open (not rock or dirt: a disc in rock lights nothing)
// takes the disc's radiance whole, so a ray meets it undiluted, added up
// (target 0); its reach is the most of the discs' over it (target 1,
// unsplit f16: 60000 or more is unlimited).

@group(0) @binding(0) var<uniform> gp: GiParams;
@group(0) @binding(1) var solid_tex: texture_2d<u32>;
@group(0) @binding(2) var<uniform> look: LookGlobals;

struct DiscOut {
    @builtin(position) pos: vec4<f32>,
    // world x, y, radius, reach
    @location(0) disc: vec4<f32>,
    @location(1) radiance: vec3<f32>,
};

@vertex
fn vs_disc(
    @builtin(vertex_index) vi: u32,
    @location(0) disc: vec4<f32>,
    @location(1) radiance: vec4<f32>,
) -> DiscOut {
    let s = max(gp.c.w, 1.0);
    let grid = gp.c.yz;
    // The disc's cells and one more each way, clamped to the grid.
    let lo = clamp(floor((disc.xy - vec2<f32>(disc.z)) / s) - vec2<f32>(1.0), vec2<f32>(0.0), grid);
    let hi = clamp(floor((disc.xy + vec2<f32>(disc.z)) / s) + vec2<f32>(2.0), vec2<f32>(0.0), grid);
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let c = mix(lo, hi, corner) / max(grid, vec2<f32>(1.0));
    var out: DiscOut;
    out.pos = vec4<f32>(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0, 0.0, 1.0);
    out.disc = disc;
    out.radiance = radiance.rgb;
    return out;
}

struct DynOut {
    @location(0) radiance: vec4<f32>,
    @location(1) reach: f32,
};

@fragment
fn fs_disc(in: DiscOut) -> DynOut {
    let s = max(gp.c.w, 1.0);
    let cell = floor(in.pos.xy);
    let centre = (cell + vec2<f32>(0.5)) * s;
    var k = 1.0;
    if distance(centre, in.disc.xy) > in.disc.z {
        // A disc smaller than a cell lights the cell holding it, by its
        // area (no cell centre is in it then).
        if any(floor(in.disc.xy / s) != cell) {
            discard;
        }
        k = min(3.14159265 * in.disc.z * in.disc.z / (s * s), 1.0);
    }
    let size = vec2<i32>(textureDimensions(solid_tex));
    let level = min(vec2<i32>(look.level.xy), size);
    let w = vec2<i32>(floor(select(centre, in.disc.xy, k < 1.0)));
    if all(w >= vec2<i32>(0)) && all(w < level) && textureLoad(solid_tex, w, 0).r != 1u {
        discard;
    }
    var out: DynOut;
    out.radiance = vec4<f32>(in.radiance * k, 1.0);
    out.reach = min(in.disc.w, 60000.0);
    return out;
}
