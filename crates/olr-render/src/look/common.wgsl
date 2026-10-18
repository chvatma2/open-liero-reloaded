// The look's per-frame globals (plan 15a): one uniform shared by every look
// pass. Every field is a vec4 (std140-safe); `groups[v - 1]` is a group map
// value v's group (0 is no group).

struct LookGlobals {
    // rgb ambient (linear), max light
    ambient: vec4<f32>,
    // exposure multiplier, contrast, saturation, light steps
    grade: vec4<f32>,
    // rgb tint (linear), softness (world pixels)
    tint: vec4<f32>,
    // world frame origin x, y (world px), world target texels per world px, placed lights
    frame: vec4<f32>,
    // level width, height (world px), art scale, glow-light cell (world px)
    level: vec4<f32>,
    // view mode (0 lit, 1 full bright, 2 light only), group count, 0, 0
    mode: vec4<f32>,
    // per group: rgb cast (linear, × intensity × animation), self-light scale
    groups: array<vec4<f32>, 255>,
    // per group: reach (world px; 1e9 unlimited), own colour (1) or tint (0), 0, 0
    group_info: array<vec4<f32>, 255>,
};
