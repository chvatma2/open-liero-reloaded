# olr-render

The wgpu renderer (spec §8.2, §19 E3): it draws `olr-scene`'s `DrawList`
on screen or offscreen.

- **Layer 5, uses `std` and wgpu 30.0.1.** Depends on `olr-scene` (same
  layer, `xtask/layers.ron`), `olr-terrain`, `olr-data`, `olr-pack` (PNG),
  `olr-look` (the `LookFrame` a lit frame draws, plan 15a), `bytemuck`,
  `pollster`. `#![forbid(unsafe_code)]`: wgpu needs no unsafe
  here (instances are `[f32; INSTANCE_FLOATS]`, 15 since plan 15a's shade,
  cast by `bytemuck`, no derive).
- **Use** (`renderer.rs`): `Renderer::new(device, queue, surface_format,
  RenderSettings)`; `upload_atlas(&SceneAssets)` once every image is
  registered (scene, worms, `olr-fx`); `upload_terrain(&ColorMap)` for a new
  or restarted level, then `update_terrain(&mut ColorMap)` after each tick
  (only `take_dirty()`'s rectangles); `render(&DrawList, &view, size)` per
  drawn frame (`render_in(.., area)`: the picture in a part of the target,
  `dest_rect_in`; the game's playfield above its HUD bar; `olr_present::Presentation::upload`
  does the uploads in this order). **An image registered after `upload_atlas`** (a worm added
  later) draws as a magenta square until `upload_atlas` runs again.
  Lit frames (plan 15a): `set_lighting(true)` before `upload_terrain`,
  then `render_lit(&DrawList, Option<&LookFrame>, &view, area)` beside the
  unchanged `render`/`render_in` (Task 9 ruling: their callers keep their
  signatures); `None`, or lighting off (the default), draws exactly
  `render_in`'s bytes (`look_none_draws_todays_bytes`). `set_lighting`
  resets the world target only on a change; `lighting()` says which.
- **World pass** (`frame.rs`, `world.wgsl`): a target covering the view
  plus `MARGIN` world pixels, at `scale()` texels per world pixel
  (`MIN_SCALE` 2, OLX's sprite art, or the level's art scale if larger). The
  terrain is one opaque RGBA texture of the `ColorMap` composite (spec §8.2's
  separate back, front and material textures are not needed: `olr-terrain`
  composites on the CPU). Sprites snap to whole texels; line pixels are
  world-pixel blocks of the atlas's white texel. Texels are fetched with
  `textureLoad`, clamped to the quad's source: nearest, no bleeding.
  Blending is on the stored bytes (gamma space, like SDL).
- **Lighting** (plan 15a, spec `docs/superpowers/specs/2026-10-18-look-system-design.md`;
  `renderer/look.rs`, `renderer/gi.rs`, `look/*.wgsl`, the CPU side in
  `look/mod.rs`). `set_lighting(true)` makes a `LookState`; each
  `upload_terrain` then makes the level's look textures (`LookTextures`:
  glow `R8Unorm` at art size, glow groups and the solid map `R8Uint` at
  world size, each cropped like the terrain by `look_texture_sizes`) and
  each `update_terrain` rectangle rewrites them (`look_textures_follow_dirty_rects`).
  The solid map is the material, except that with the level's clear flags
  (`ColorMap::clear`, Task 14b) a clear solid is written as background,
  so placed light, glow light and the soak pass it
  (`a_lamp_shines_through_a_clear_wall`, `glow_light_passes_a_clear_wall`,
  `a_clear_solid_takes_no_soak_or_floor`). The `LookFrame` is packed each
  frame into one uniform (`LookGlobals`, `look/common.wgsl`, every field a
  vec4; 255 groups, `groups[v - 1]` for group value v, 0 none) and the
  lights' (two vec4s each, the first `olr_look::MAX_LIGHTS` (256) only,
  `lights_past_the_cap_change_nothing`; non-finite values become 0).
  `LIGHT_PAD` and `MAX_SOAK` are prepended to the shaders from Rust. A
  lit frame's passes, in order (`pass.rs`'s `draw`):
  - **World lit** (`world_lit.wgsl`): the world pass into two targets, the
    albedo and the shade target (`SHADE_FORMAT`, `Rgba8Unorm`: self-light,
    marker, floor kind halved (0 none, 0.5 solid, 1 worm), alpha), each
    item's from its `olr_scene::Shade` (three floats of the instance,
    `INSTANCE_FLOATS` 15; the floor kind 0, 1 or 2 there); the lit terrain's
    self-light is its glow × its group's self-light scale (1 for no group:
    such a pixel glows, but casts nothing).
  - **Occluders** (`occluder.wgsl`, plan 15b): the list's `occluders` (the
    living worms' bodies, `frame.rs`'s `occluder_instances`) drawn into an
    `R8Uint` occluder map, one texel per world pixel over the light grid
    (`LookGlobals.occluders`: origin and size), each opaque texel the worm's
    index + 1; cleared every lit frame. `worm_in` (`common.wgsl`) reads it.
    The shadow march and the glow light's emission treat a worm texel as
    solid, except the shadow march of a light owned by that worm (the
    light's second vec4's w, `FrameLight::owner`) or, unowned, whose centre
    is in it
    (`a_worm_shadows_the_wall_behind_it`, `a_light_inside_a_worm_ignores_it`,
    `overlapping_worms_block_each_others_lights`,
    `an_off_view_worm_still_shadows_the_view`, `a_worm_blocks_glow_light`).
  - **Glow light** (`gi.rs`, `gi_*.wgsl`, Task 12): radiance cascades over
    the whole level, fixed to the world (never camera-dependent): emission,
    a jump-flood seed and its passes (distance to the nearest solid or
    glowing cell), the cascades from the top down, then the fluence the sum
    samples bilinearly. Rays are sphere-traced at full grid resolution, at
    most 96 steps (a coarse pyramid closed thin shafts and leaked through
    1-px walls). `gi_cell` picks the grid's cell (a power of two) so the
    grid holds at most `GI_CELL_BUDGET` (262 144) cells and every cascade
    texture fits the device (`glow_light_past_the_device_crop_is_open`).
    Without groups the fluence is only cleared.
  - **Shadow maps** (`shadow.wgsl`, Task 11): one row per placed light,
    `SHADOW_RAYS` (512) columns, each the distance to the first rock or
    dirt marched from the light's radius out to its reach within the
    level's box (outside is open; a light centred in a solid lights
    nothing; a far Unlimited light's march ends,
    `a_far_unlimited_light_ends_its_shadow_march`). Skipped with no
    lights. The shadow filter (`LookFrame::shadow_filter`, from
    `olr_look::Shadows`) is applied when the sum reads them (`light.wgsl`'s
    `visible`): 0 hard (one test), 1 smooth (four tests a world pixel), 2
    soft (a penumbra from the light's radius, wider farther behind the
    occluder, over the neighbouring rays), soft the default (the user's
    playtest, 2026-10-19; `soft_shadows_have_a_penumbra`).
  - **Sum** (`fs_sum`): the light grid, one texel per world pixel of the
    world target plus `LIGHT_PAD` on every side (`Rgba16Float`): ambient,
    glow light and the placed lights (fade from the radius' edge to radius
    + reach).
  - **Soak** (`soak.wgsl`, Task 14a): `fs_soak_h` across, `fs_soak_v` down:
    an opaque solid texel takes the most light within the soak of it,
    faded by distance; `fs_soak_v` raises it to the solid floor. Air keeps
    its light (`soak_and_floor_never_change_air`).
  - **Blur** (`fs_blur_h`, `fs_blur_v`): a box of the level's softness (0
    to 4 world pixels) each way; `fs_blur_v` also caps the light at max
    light and bands it into the light steps.
  - **Composite** (`composite.wgsl`): each world-target texel's albedo
    times the light grid at its centre (bilinear), never below its
    self-light nor its floor: the solid floor for `Shade::FLOORED` (blood,
    droppers and gibs; the user's playtest, 2026-10-19,
    `a_floored_item_never_falls_below_the_solid_floor`), the worm floor for
    `Shade::WORM` (plan 15b, `a_worm_item_never_falls_below_the_worm_floor`).
    A worm texel over its own worm in the occluder map takes the light just
    outside the worm on its side (`facing_light`: outward from the 7 × 7
    window's worm pixels, read one pixel past the first non-worm pixel
    within 4; per world pixel; `a_worm_is_lit_on_the_lamps_side`,
    `worm_shading_is_steady`); a rope pixel takes the light where it is;
    graded (exposure, contrast, saturation, tint) in linear light; a
    marker shows its albedo. View modes: lit, full bright (the albedo),
    light only (white albedo). The upscale pass reads the lit target.
  `LIGHT_PAD` is `MAX_SOAK` (12) + 4: neither the soak nor the blur sees
  the grid's edge, so the light is camera-independent
  (`light_is_camera_independent`). `LIGHT_CULL_PAD` (`MARGIN` + `LIGHT_PAD`
  + 1, 18) is the padding a caller culls lights with (`LookFrame::cull`:
  `olr-present`, `olr-cli look-preview`; `culling_with_the_cull_pad_changes_no_light`).
  **GL limits:** GL renders into no `R32Float` or `Rg32Float` target, so
  the shadow maps, the flood's seeds and the reach split each value into
  two halves exact in f16 in `Rgba16Float` (`split`/`joined` in
  `common.wgsl`; Task 19's e2e found it; `a_lit_frame_draws_on_gl`).
  **CPU model** (`look/mod.rs`): `shade_texel` (one composite texel),
  `grade_linear`, `linear_to_srgb`, `LightGrid` (where the grid lies),
  references for tests (`shade_texel_table`). The light grid is rebuilt
  every lit frame; no GPU timestamps around the look passes yet.
- **Atlas** (`atlas.rs`): shelf packing, tallest first, ties by id
  (deterministic); texel (0, 0) is white. An image too large for the
  device is left out with a warning (`warnings()`), and any id the layout
  lacks draws as a magenta square (`MISSING`).
- **Upscale pass** (`upscale.rs`, `upscale.wgsl`): `UpscaleFilter::
  IntegerNearest` (whole multiples, centred, black bars) or `SharpBilinear`
  (the default: fills the output keeping aspect, texel edges blend over
  one output pixel). The sub-pixel camera only moves the source rectangle.
  An sRGB target gets linear values, so it shows the same bytes.
- **No per-frame allocation** of its own once warm: instance and dirty
  buffers are reused, the world target and instance buffer are recreated
  only when they must grow or change size (`frame.rs`'s no-alloc test).
- **Offscreen** (`offscreen.rs`): `Offscreen::new()` opens a device without
  a surface, preferring a software rasteriser (lavapipe on Vulkan, then
  llvmpipe on GL) so goldens are the same everywhere; `WGPU_BACKEND` and
  the other `WGPU_*` variables narrow the choice. `render_to_image`,
  `render_to_png`, `read_texture`; `render_lit_to_image(renderer, list,
  look, size)` (plan 15a; `render_to_image` is it with `None`), `read_light`
  (the light grid as last drawn, `Renderer::light_grid`: its `LightGrid`
  and each texel's linear light, f16 decoded), `read_bytes` (any
  `COPY_SRC` texture's texels). `NoAdapter` means skip, never panic.
  `Offscreen::open(&OffscreenOptions)` chooses otherwise (`new()` is
  `open` of the default, unchanged): `prefer: AdapterPreference::Hardware`
  ranks a discrete GPU, then an integrated one, software last; `backends`
  replaces what `WGPU_BACKEND` allows; `timestamps` asks for
  `Features::TIMESTAMP_QUERY` when the adapter has it; `max_texture_side`
  asks for a smaller texture limit (at least 2048: tests of a level the
  device crops). `list_adapters`
  lists every adapter found (`olr-cli gpu-bench --list-adapters`).
- **Bench only (plan 8)**, off by default so goldens cannot move:
  `enable_gpu_timing` (true when the device has timestamps) and
  `gpu_times` (`timing.rs`: slot 0 at the world pass's start, 1 at the
  upscale pass's end, 2 and 3 around the probe: the first pass's start
  and the last's end, the passes between asking for none, which wgpu refuses
  when empty; only the slots a frame
  wrote are resolved, the probe's only when it ran, since Vulkan resolves
  with a wait; reading waits for the frame, so offscreen only; each frame
  is read once). `set_probe(Some(Probe { passes, taps }))` (`probe.rs`,
  `probe.wgsl`, clamped to `MAX_PROBE_PASSES`/`MAX_PROBE_TAPS`): after
  each frame, an untimed seed pass (`fs_seed`) stretches the drawn frame
  (the world target) into the first pass's source (even with 0 passes,
  so a baseline's wall time holds it), then `passes` ping-pong blur
  passes of `taps` taps at the output's size into the probe's own two
  textures: they blur a real picture, never a blank one a GPU would
  compress; the picture never changes. `probe_output` gives the last
  pass's texture (tests read it back). `None`, the default, runs none.
- **Goldens** (`olr-cli/tests/render_golden.rs`, `fixtures/render/frame-*.png`):
  `olr-cli render` of one oracle scenario per level (`dig` on Dirt Level,
  `rope` on Liero Factory, `weapons/classic/bazooka` on FossilFacility,
  `castle-shoot-rope` on CastleStrike, the 1× level), drawn from the
  imported OLX content at test time (`OLR_CONTENT_DIR`) and compared with
  `perceptual_diff` (tolerance 0.1, at most 0.002 % of pixels: lavapipe and llvmpipe GL both draw the four goldens with no differing pixel). They were
  blessed on lavapipe; they are accepted fixtures, the OLX packs they are
  drawn from are never committed. Re-bless with `cargo xtask bless-render
  --reason "<why>"` (without an adapter it re-draws only the CPU golden).
  The look goldens (plan 15a Tasks 14, 16): `look-lit.png`,
  `look-light.png` (`--view light`), `look-full.png` (`--view full`) and
  `look-t500.png` (`--time 500`) are `olr-cli look-preview` of
  `fixtures/look/levels/look-test` at (256, 128); `look-rocket.png` is
  `olr-cli render-match` of chaos bots with the look test mod
  (`fixtures/look/mods`), seed 1, 300 ticks.
- **Instance buffer** (`instance_buffer_bytes()`): grows to the busiest
  frame's next power of two and never shrinks; `olr-game`'s soak test
  (`tests/soak.rs`) bounds it.
- **WSL:** under `GALLIUM_DRIVER=d3d12` with `WGPU_BACKEND=gl` (the real
  GPU) a thread that used the device segfaults when it exits (a driver
  bug), so the GPU tests, which run on test threads, crash there; the
  goldens (one `olr-cli` process each, main thread) pass. The default
  software adapter has no such problem.

Bench: `benches/frame.rs` (divan, `cargo xtask bench`): a frame of a
synthetic 504 × 350 level with 2000 sprites and 500 line pixels at 1080p
and 1440p, waited for on the device; without an adapter it prints a
notice and measures nothing.

Test: `cargo nextest run -p olr-render` (CPU: packing, quads, upscale
geometry, image diff, `shade_texel_table`, the cascades' layout). The GPU
tests (`tests/gpu.rs`; the look's read the light grid back with
`read_light`: ambient, steps, view modes, walls, reach, edges, soak and
floor, softness, glow light, clear solids, the cull pad, the light cap,
GL, soft shadows) and goldens need
`OLR_GPU_TESTS=1`; `cargo xtask full` sets it and runs them, skipping with a
notice when no adapter opens (`cargo run -p olr-render --example adapter`).
