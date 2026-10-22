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
  resets the world targets only on a change; `lighting()` says which.
- **Several views a frame** (issue #26, split screen; `pass.rs`):
  `render_views(&[ViewDraw { list, area }], Option<&LookFrame>, &view)`
  clears the target once, then draws each view into its `area` (clipped
  to the target, the upscale scissored to it: its black bars are its
  own, nothing outside is touched); `render_views_over` is it without the
  clear (the rest of the target is left as it was). `render_lit` is
  `render_views` of one view, `render_in` the same unlit: one view draws
  exactly as before (the goldens). Each view has its own `Target` (world
  target, shade, `LitTarget`: light grid, bloom, occluder map, composite,
  lit target) at its view's size, made in order and kept while the count
  shrinks, and its own uniforms (the world pass's globals, the upscale's,
  the look's `LookGlobals` in `LitTarget::globals`), so every view is
  encoded into one command encoder and one submit: all views' world and
  occluder passes, the glow light once, all views' look passes, then
  all upscales. The glow light reads the frame's globals
  (`LookState::globals`: the first view's look with the level's occluder
  map; the lit terrain's group 2 reads them too, none of a view's fields)
  and its own occluder map (`LookTextures::gi_occluders`, `R8Uint` at the
  world size, cleared each lit frame, each view's map copied in where it
  lies in the level, `copy_occluders`: exactly one view's bytes for one
  view); one history step a frame (`glow_light_steps()`, tests). The
  instance buffer holds every view's quads (`ViewPlan`: its terrain,
  distant, sprite and occluder ranges). `view_light_grid(i)` is view
  `i`'s light grid (`light_grid()` the first's). Tests (GPU):
  `two_views_in_one_frame_match_single_views` (unlit and lit, pictures
  and light grids, a worm in one view),
  `the_glow_light_steps_once_a_frame_with_four_views` (with bounce: four
  views' light frame after frame is one view's; fails with the glow
  light run per view), `a_view_draws_only_its_area`.
- **Distant layers** (issue #11): `set_distant(&[DistantLayer])` makes
  one texture per layer (an empty one, or one past the device's texture
  side, skipped with a warning; `set_distant(&[])` drops them). Each frame
  `frame::distant_instance` places each at `DistantLayer::position` of the
  list's `camera()` (`DrawList::parallax`, else the view's centre), its
  offset rounded to whole target texels against the snapped world-target
  origin and clipped to the level and the target; the instance's shade
  carries the depth. Only a see-through terrain (`ColorMap::see_through`,
  taken at `upload_terrain`) draws them: its opaque backing first
  (`fs_terrain_backing`, `_lit`: the tile colour of see-through pixels),
  then the layers far to near (`fs_distant`, `_lit`: no blending), then
  the terrain again over them (`fs_world`; `fs_terrain_lit` discards
  alpha 0), then the sprites. Lit, a distant texel writes shade alpha
  `1 − depth`, and the composite (gated by `LookGlobals.distant`, set only
  while a layer is drawn) gives it the light beyond the ambient scaled by
  `1 − depth` (glow light, flashes and bloom alike); never a light of its
  own. A terrain that is not see-through draws exactly as before
  (`look_none_draws_todays_bytes`, the goldens). Tests (GPU):
  `distant_layers_scroll_by_their_depth`,
  `distant_layers_take_light_by_their_depth`; `frame.rs`'s
  `distant_quads_snap_to_texels_and_clip_to_the_level`.
- **World pass** (`frame.rs`, `world.wgsl`): a target covering the view
  plus `MARGIN` world pixels, at `scale()` texels per world pixel
  (`MIN_SCALE` 2, OLX's sprite art, or the level's art scale if larger:
  `level_scale()`; issue #12: or the largest `px_per_world` of the atlas's
  images, capped at `MAX_SPRITE_SCALE` 8, so a double-resolution skin
  draws at 4). `dest_rect_in(src, area)` places the target with integer
  nearest in whole multiples of the level's own scale
  (`upscale::dest_rect_in_units`; a fraction of a texel a pixel when the
  target does not fit), so a level shows as large whatever its skins. The
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
  world size, plan 15c's flicker regions `R16Uint` at world size, each
  cropped like the terrain by `look_texture_sizes`) and each
  `update_terrain` rectangle rewrites them (`look_textures_follow_dirty_rects`).
  The solid map is the material, except that with the level's clear flags
  (`ColorMap::clear`, Task 14b) a clear solid is written as background,
  so the glow light and the soak pass it
  (`a_lamp_shines_through_a_clear_wall`, `glow_light_passes_a_clear_wall`,
  `a_clear_solid_takes_no_soak_or_floor`). The `LookFrame` is packed each
  frame into one uniform (`LookGlobals`, `look/common.wgsl`, every field a
  vec4; 255 groups, `groups[v - 1]` for group value v, 0 none, in three
  arrays: cast and self-light scale, reach and own colour, and plan 15c's
  flicker (speed, depth, clock, seed); `frame.w` the bounce, `mode.w` the
  bloom strength, `solid.w` the ambient occlusion's strength (0-1, 0
  none) and `worm_floor.w` its reach (issue #10); non-finite values
  become 0) and play's discs into an
  instance buffer (`pack_emitters`). `LIGHT_PAD`, `MAX_SOAK`,
  `BLOOM_RADIUS` and `OCCLUSION_PAD` are prepended to the shaders from Rust. **Plan 15c**
  (spec `docs/superpowers/specs/2026-10-19-glowing-light-design.md`): the
  glow light is the only light model (no placed lights, no shadow maps).
  A lit frame's passes, in order (`pass.rs`'s `draw`):
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
    index + 1; cleared every lit frame. `worm_in` (`common.wgsl`) reads it;
    the glow light reads the level-wide copy (issue #26, above).
    The glow light's emission marks a mostly-worm cell (2 + its rock
    share; not in a cell a disc lights, a muzzle flash inside its worm),
    keeping the glow under it; the cascades let a worm stop a ray only
    within `WORM_SHADOW` (4) world pixels of its probe, so a worm throws a
    short shadow and nothing more than 10 px from it changes (the user's
    playtest, 2026-10-20; `a_worm_throws_no_long_shadow`,
    `a_worm_changes_no_light_away_from_it`)
    (`a_worm_shadows_the_wall_behind_it`, `a_light_inside_a_worm_ignores_it`,
    `overlapping_worms_block_each_others_lights`,
    `an_off_view_worm_still_shadows_the_view`, `a_worm_blocks_glow_light`).
  - **Glow light** (`gi.rs`, `gi_*.wgsl`, Task 12; plan 15c): radiance
    cascades over the whole level, fixed to the world (never
    camera-dependent): play's discs (`gi_dynamic.wgsl`: the first
    `olr_look::MAX_EMITTERS` of `LookFrame::emitters`, instanced quads into
    a dynamic layer, never cleared whole (issue #35: each frame zeroes the
    quads of the discs last drawn, `fs_disc_clear`, from the other of two
    disc buffers, `GiState::write_discs`, then adds its own; no pass when
    neither frame has a disc; `discs_leave_nothing_behind`); a cell whose centre is in a disc and in the open takes
    its radiance whole, `olr_look::engine::disc_radiance`; a disc smaller
    than a cell lights its own by area; reach max-blended as unsplit f16,
    60000 unlimited, `R16Float`), the emission (glow × group × each pixel's flicker
    region, the discs added, solid share, albedo of rock and dirt), a
    jump-flood seed and its passes, the cascades from the top down, then the
    fluence the sum samples bilinearly, which is also the history the next
    frame's bounce reads (the cascades run before `fs_fluence` rewrites it;
    issue #35: no copy; a fresh start clears it). Rays march at full grid resolution, at most 128 steps, jumping
    by the flood's distance far from stoppers and walking cell by cell near
    them (a corner whose two side cells stop closes the ray: no light
    through a 1-px or diagonal wall,
    `a_one_pixel_wall_stops_light_right_behind_it`,
    `a_diagonal_wall_one_pixel_thin_stops_light`); a cascade merges only
    the upper probes whose ray start its ray's end can see (the bilinear
    fix). It runs once a frame however many views (issue #26). A march
    that runs out of steps counts as blocked; a disc sum, a
    ray and the history are clamped (16384) and never NaN
    (`stacked_bright_discs_never_poison_the_light`,
    `a_long_low_tunnel_takes_no_light_through_rock`). A ray meeting rock or dirt takes its albedo × the history's
    fluence of the open cell it came from × the bounce (`frame.w`, 0-0.9):
    bounced light never crosses a wall (`light_bounces_round_a_corner`,
    `bounce_tints_with_the_wall`, `bounce_never_runs_away`,
    `bounce_settles_to_steady_bytes`, `no_bounce_through_a_thin_wall`).
    `reset_light_history` (and `upload_terrain`, a frame without a look,
    one that casts nothing) starts it afresh
    (`a_new_level_starts_its_light_afresh`). `gi_cell` picks the grid's
    cell (a power of two) so the grid holds at most `GI_CELL_BUDGET` (2^20:
    Castle Strike at one cell a world pixel; a larger level's 2-px cells
    can let a 1-px wall leak, `docs/porting/STATUS.md`) cells and every cascade
    texture fits the device (`glow_light_past_the_device_crop_is_open`).
    Without groups or discs the fluence is only cleared.
  - **Flicker regions** (`regions.rs`, plan 15c): `Regions` labels each
    4-connected region of a `Flicker` group's world pixels (only the
    groups the frame flickers: `sync_regions` relabels the whole map when
    that set changes; `write` relabels the regions touching a dirty
    rectangle or its border) with 1 + its first pixel's index folded into
    16 bits, an `R16Uint` map the lit terrain and the emission read
    (`region_flicker` in `common.wgsl`: value noise on a 32-bit hash;
    `two_flames_flicker_on_their_own`, `one_flame_flickers_as_one`,
    `pulse_groups_stay_in_step`, `regions_follow_glow_edits`).
  - **Ambient occlusion** (issue #10, `fs_occlusion_h` in `light.wgsl`;
    only when the frame's `LookFrame::occlusion` has a strength, otherwise
    no pass runs): for each world pixel of the light grid and
    `OCCLUSION_PAD` (`olr_look::MAX_OCCLUSION_REACH`, 32) rows above and
    below it, the Gaussian-weighted share (sigma a third of the reach,
    taps out to the reach) of opaque solids along its row, read from the
    world-size solid map (outside the level open), into `LitTarget`'s
    occlusion rows (`Rgba16Float`, made with every lit target).
  - **Sum** (`fs_sum`): the light grid, one texel per world pixel of the
    world target plus `LIGHT_PAD` on every side (`Rgba16Float`): ambient
    and glow light. With occlusion (`ambient_share`), an air texel's
    ambient (not its glow light) is scaled by `1 - strength × s(x)`: the
    rows weighed down its column, divided by a thick flat wall's face
    share, `x` at most 1, `s(x) = 1 - (1 - x)²`; a solid keeps its
    ambient; without it the ambient is unscaled (the same bytes)
    (`occlusion_darkens_a_narrow_tunnel`, `occlusion_leaves_glow_light_alone`,
    `occlusion_keeps_the_worm_floor`, `occlusion_follows_digging`,
    `occlusion_is_camera_independent`).
  - **Soak** (`soak.wgsl`, Task 14a): `fs_soak_h` across, `fs_soak_v` down:
    an opaque solid texel takes the most light within the soak of it,
    faded by distance; `fs_soak_v` raises it to the solid floor. Air keeps
    its light (`soak_and_floor_never_change_air`). A worm's short shade
    colours only the rock face (the user's playtest, 2026-10-20): the
    across pass also writes rows into the bloom's spare texture, where a
    texel within twice `WORM_SHADOW` + 2 of a worm (the occluder map) takes
    the nearest unshaded same-kind texel's light along its row; the down
    pass uses them from 2 px into rock (half at 2 px, whole deeper)
    (`a_worm_shadow_stays_at_the_rock_face`).
  - **Blur** (`fs_blur_h`, `fs_blur_v`): a box of the level's softness (0
    to 4 world pixels) each way, an air texel gathering only the air before
    the first solid each way (no light blurs through a wall); `fs_blur_v`
    also tones the light (`tone`, the CPU twin `olr_render::tone`: the
    identity up to 1, a shoulder to max light; a white point of 1 clips),
    bands it into the light steps and writes the light above 1,
    soft-saturated, for the bloom (a glowing pixel writes none).
  - **Bloom** (`fs_bloom_h`, `fs_bloom_v`, plan 15c): a Gaussian of
    `BLOOM_RADIUS` (12) world pixels each way over the light grid, an air
    texel's stopping at rock; the composite adds it × `BLOOM_STRENGTH`
    (0.6; `set_bloom(false)` for tests) before the grade, never on rock
    or dirt
    (`bright_light_blooms`, `bloom_is_steady_and_camera_independent`,
    `a_dim_level_draws_as_before`, `glowing_pixels_feed_no_bloom`,
    `bloom_keeps_the_worm_floor_and_lit_side`).
  - **Composite** (`composite.wgsl`): each world-target texel's albedo
    times the light grid at its centre (bilinear), never below its
    self-light nor its floor: the solid floor for `Shade::FLOORED` (blood,
    droppers and gibs; the user's playtest, 2026-10-19,
    `a_floored_item_never_falls_below_the_solid_floor`), the worm floor for
    `Shade::WORM` (plan 15b, `a_worm_item_never_falls_below_the_worm_floor`).
    A glowing texel (self-light above 0) shows its own art: other light
    lifts it to 1 at most and no bloom lands on it
    (`a_glowing_pixel_shows_its_art_under_bright_light`); so does any
    texel over an opaque solid's world pixel (the solid map, binding 6;
    #1: rock under a bright sky read oversaturated,
    `rock_shows_at_most_its_art_under_bright_light`).
    A worm texel over its own worm in the occluder map takes the light just
    outside the worm on its side (`facing_light`: outward from the 7 × 7
    window's worm pixels, read one pixel past the first non-worm pixel
    within 4; per world pixel; `a_worm_is_lit_on_the_lamps_side`,
    `worm_shading_is_steady`); a rope pixel takes the light where it is;
    graded (exposure, contrast, saturation, tint) in linear light; a
    marker shows its albedo. View modes: lit, full bright (the albedo),
    light only (white albedo). The upscale pass reads the lit target.
  `LIGHT_PAD` is `MAX_SOAK` (12) + 4 + `BLOOM_RADIUS` (12): neither the
  soak, the blur nor the bloom sees the grid's edge, so the light is
  camera-independent (`light_is_camera_independent`,
  `an_off_view_flash_lights_the_view_the_same`).
  **GL limits:** GL renders into no `R32Float` or `Rg32Float` target, so
  the emission's reach splits each value into two halves exact in f16 in
  `Rg16Float` (`split`/`joined` in `common.wgsl`; Task 19's e2e found
  it; `a_lit_frame_draws_on_gl`); the flood's seeds are whole cells in
  `Rg16Uint` (`NO_SEED` none) and the disc layer's reach one `R16Float`
  (issue #35: half the bytes, or a quarter, of `Rgba16Float`).
  **CPU model** (`look/mod.rs`): `shade_texel` (one composite texel),
  `grade_linear`, `linear_to_srgb`, `LightGrid` (where the grid lies),
  references for tests (`shade_texel_table`). The light grid is rebuilt
  every lit frame; each pass group is timed with GPU timestamps when
  timing is on (below, issue #34).
- **Atlas** (`atlas.rs`): shelf packing, tallest first, ties by id
  (deterministic); texel (0, 0) is white. An image too large for the
  device is left out with a warning (`warnings()`), and any id the layout
  lacks draws as a magenta square (`MISSING`).
- **Upscale pass** (`upscale.rs`, `upscale.wgsl`): `UpscaleFilter::
  IntegerNearest` (whole multiples, centred, black bars) or `SharpBilinear`
  (the default: fills the output keeping aspect, texel edges blend over
  one output pixel). The sub-pixel camera only moves the source rectangle.
  An sRGB target gets linear values, so it shows the same bytes.
- **Counted uploads** (issue #35, `renderer/uploads.rs`): the renderer's
  queue is `uploads::Queue`, which counts every `write_texture` (its
  extent's texel bytes) and `write_buffer` made through it;
  `Renderer::uploads()` gives the totals (`Uploads`, `since` an earlier
  reading), `olr-cli quick-bench` shows them. `GpuTimes::pass_count` is
  the passes a timed frame ran.
- **No per-frame allocation** of its own once warm: instance, dirty and
  view-plan buffers are reused, the world targets and instance buffer are
  recreated only when they must grow or change size (`frame.rs`'s
  no-alloc test).
- **Offscreen** (`offscreen.rs`): `Offscreen::new()` opens a device without
  a surface, preferring a software rasteriser (lavapipe on Vulkan, then
  llvmpipe on GL) so goldens are the same everywhere; `WGPU_BACKEND` and
  the other `WGPU_*` variables narrow the choice. `render_to_image`,
  `render_to_png`, `read_texture`; `render_lit_to_image(renderer, list,
  look, size)` (plan 15a; `render_to_image` is it with `None`),
  `render_views_to_image(renderer, views, look, size)` (issue #26),
  `output_texture(size)` (a target to draw into, read back and write),
  `read_light` (the light grid as last drawn, `Renderer::light_grid`: its
  `LightGrid` and each texel's linear light, f16 decoded),
  `read_view_light(renderer, i)` (view `i`'s), `read_bytes` (any
  `COPY_SRC` texture's texels; issue #34: `Renderer::view_lit_target(i)`,
  view `i`'s lit target as last drawn, the composite before the upscale,
  for `olr-cli frame-hashes`' byte identity). `NoAdapter` means skip, never panic.
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
  `gpu_times` (`timing.rs`: slot 0 at the first view's world pass's
  start, 1 at the last view's upscale pass's end, 2 and 3 around the probe: the first pass's start
  and the last's end, the passes between asking for none, which wgpu refuses
  when empty; only the slots a frame
  wrote are resolved, the probe's only when it ran, since Vulkan resolves
  with a wait; reading waits for the frame, so offscreen only; each frame
  is read once). Issue #34: every pass of a timed frame also writes a
  pair of its own (`PASS_BASE` 32, a resolve's 256-byte alignment, to
  `PASS_SLOTS` 256 more; the first world pass's start is slot 0, the last
  upscale's end slot 1; a pass past them goes untimed), counted in its
  `PassGroup` (world and occluders; emission: the glow light's occluder
  map, discs and emission; jump flood; cascades; fluence and its
  clear; occlusion; light sum; soak; blur; bloom; composite; upscale; the
  timer's `pass` hands each pass its writes, `start_frame` clears them, the
  glow light's `encode` takes the timer): `GpuTimes::passes`, indexed as
  `PassGroup::ALL`, 0 for a group not run; the frame less their sum is the
  copies and gaps between passes (`passes_add_up_in_their_groups`).
  `olr-cli gpu-bench` prints them as a table. `set_probe(Some(Probe { passes, taps }))` (`probe.rs`,
  `probe.wgsl`, clamped to `MAX_PROBE_PASSES`/`MAX_PROBE_TAPS`): after
  each frame, an untimed seed pass (`fs_seed`) stretches the drawn frame
  (the first view's world target, at its area's size) into the first pass's source (even with 0 passes,
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
  `fixtures/look/levels/look-test` at (256, 128); `look-occlusion.png`
  and `look-occlusion-light.png` (issue #10) of its copy
  `look-test-occlusion` (ambient `#405880` at 40, occlusion 80 over 16 px);
  `look-distant-a.png` (at 213, 96), `look-distant-b.png` (at 299, 160)
  and `look-distant-full.png` (`--view full`) of the distant demo level
  `look-test-distant` (issue #11: a night sky at depth 90 and a far city at
  55 seen through two gaps); `look-rocket.png` is
  `olr-cli render-match` of chaos bots with the look test mod
  (`fixtures/look/mods`), seed 1, 300 ticks. Byte identity (issue #34):
  `fixtures/render/lit-hashes.ron`, `olr-cli frame-hashes` of a lit Castle
  Strike Reloaded match, a hash of the exact bytes of each frame's light
  grid and lit target (no tolerance; only on the adapter that recorded
  it), re-blessed like the goldens.
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
floor, softness, glow light, clear solids, discs, bounce, flicker, glare,
the disc cap, ambient occlusion, GL, several views (issue #26); issue #12:
`a_double_skin_draws_at_four_texels_per_world_pixel`,
`a_glowing_skin_pixel_is_bright_in_the_dark`,
`a_skin_light_lights_the_air_and_a_wall_stops_it`) and goldens need
`OLR_GPU_TESTS=1`; `cargo xtask full` sets it and runs them, skipping with a
notice when no adapter opens (`cargo run -p olr-render --example adapter`).
