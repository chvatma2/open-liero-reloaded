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
  (only `take_dirty_kinds()`'s rectangles; issue #35: a colour-only one,
  stains, writes the terrain alone, not the look textures nor the
  flicker regions); `render(&DrawList, &view, size)` per
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
  (`LookState::globals`: the first view's look; the lit terrain's group 2
  reads them too, none of a view's fields) and no occluder map (#53:
  worms take no part in the light, so no view's worms reach another's
  light); one history step a frame (`glow_light_steps()`, tests). The
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
  `distant_quads_snap_to_texels_and_clip_to_the_level` (and
  `distant_origin`, where the quad's top-left lies in the world: issue
  #41's heat placement). Issue #41: a layer may heat (Heat, below).
  **Glowing layers** (issue #62; STATUS.md "Glowing distant layers
  (issue #62)"): a layer's light is not its own: the bake writes it into
  the level's back glow and groups at its reference placement (the camera
  at the level's centre), so the emission, unchanged, casts it once a
  frame for every view (it reads the glow maps whatever the back's
  alpha). Its picture: `distant_instance` takes `glow` (group, strength
  percent) into the shade's `y` (the group's value) and `z` (the strength
  as the bake's byte, 0-1), `INSTANCE_FLOATS` unchanged; `set_distant`
  makes each layer's mask an `R8Unorm` texture (`no_mask`, 1 × 1 full,
  without one) and a `lit_bind` (`distant_layout`: the image and the
  mask, group 1 of the lit distant pipeline; unlit frames bind the image
  alone and draw as before); `fs_distant_lit` gives a texel self-light
  `strength × mask × the group's self-light scale × its flicker` (the
  layer one region, 0), the larger of it and a hot layer's, so the
  composite shows a glowing sky's art. Its drawn pixels feed no light and
  no bloom. Tests (GPU, `tests/gpu/distant_glow.rs`):
  `a_glowing_distant_layer_shows_its_art_in_the_dark`,
  `a_distant_layers_light_stays_put_as_the_camera_pans`,
  `a_glowing_layers_drawn_pixels_feed_no_bloom`,
  `a_see_through_glow_lights_as_an_opaque_one`.
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
  vec4; 255 groups, `groups[v - 1]` for group value v, 0 none, each a
  `Group` of three vec4s: `light` (the cast, `cast` being a WGSL keyword)
  and self-light scale, `info` (reach and own colour), and plan 15c's
  `flicker` (speed, depth, clock, seed); issue #35: only the frame's
  groups are packed and written, `mode.y` of them, the shaders reading
  none past it; `frame.w` the bounce, `mode.w` the
  bloom strength, `solid.w` the ambient occlusion's strength (0-1, 0
  none) and `worm_floor.w` its reach (issue #10); non-finite values
  become 0) and play's discs into an
  instance buffer (`pack_emitters`); issue #54: `distant.y` the worm
  halo's strength scale (1; 0 with `set_worm_halo(false)`, tests) and
  `distant.z` the background tag (0.25; 0 with
  `set_background_tag(false)`, tests). `LIGHT_PAD`, `MAX_SOAK`,
  `BLOOM_RADIUS`, `OCCLUSION_PAD`, `WORM_BOXES` and the worm halo's
  `WORM_HALO_RADIUS`, `WORM_HALO_RADIUS_MAX`, `WORM_HALO_STRENGTH` and
  `WORM_HALO_FADE` (`olr_look::engine`) are prepended to the shaders from Rust. **Plan 15c**
  (spec `docs/superpowers/specs/2026-10-19-glowing-light-design.md`): the
  glow light is the only light model (no placed lights, no shadow maps).
  A lit frame's passes, in order (`pass.rs`'s `draw`):
  - **World lit** (`world_lit.wgsl`): the world pass into three targets,
    the albedo, the shade target (`SHADE_FORMAT`, `Rgba8Unorm`: self-light,
    marker, floor kind halved (0 none, 0.5 solid, 1 worm), alpha) and the
    heat light (issue #68, `HEAT_LIGHT_FORMAT`, `Rgba8UnormSrgb`: a hot
    pixel's emitted light, linear, kept as sRGB bytes and blended in
    linear light; 0 where nothing is hot, a sprite blending it down), each
    item's from its `olr_scene::Shade` (three floats of the instance,
    `INSTANCE_FLOATS` 15; the floor kind 0, 1 or 2 there); the lit terrain's
    self-light is its glow × its group's self-light scale (1 for no group:
    such a pixel glows, but casts nothing). **The background tag** (issue
    #54): an opaque `Shade::LIT` sprite writes the terrain's shade, so
    `terrain_lit` (its opaque backing too) and `fs_distant_lit` write
    0.25 in the marker channel (`LookGlobals.distant.z`), which no other
    pipeline writes and which changes no other byte
    (`the_tag_changes_no_other_byte`); the composite reads a texel as
    background when the channel lies in (0.125, 0.375) and as a marker
    from 0.5, as before. A see-through sprite over the background blends
    the tag down (little or no halo there). The floor kind is left alone
    (0.25 there would round to 64/255, the solid floor).
  - **Occluders** (`occluder.wgsl`, plan 15b): the list's `occluders` (the
    living worms' bodies, `frame.rs`'s `occluder_instances`) drawn into an
    `R8Uint` occluder map, one texel per world pixel over the light grid
    (`LookGlobals.occluders`: origin and size), each opaque texel the worm's
    index + 1; cleared every lit frame. `worm_in` (`common.wgsl`) reads it.
    No light pass reads it since #53 (the maintainer's call, 2026-10-23: worm
    shading and worms blocking light caused more glitches than they were
    worth): worms take no part in the light, which is byte for byte the
    same with or without them (`worms_change_no_light`: one view and two,
    bounce on). The composite's worm halo reads it (binding 4) near the
    view's worm boxes (Soak's `look::worm_boxes`, binding 7; issue #54).
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
    region, the discs added, solid share, albedo of rock and dirt; no
    worm, #53), a
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
    its light (`soak_and_floor_never_change_air`). It reads no worm (#53:
    plan 15c's worm-shade rows are gone). Each view's
    `LitTarget::worm_boxes` uniform (issue #35: `look::worm_boxes` of its
    occluder quads, a texel to spare, at most `WORM_BOXES` 32, the rest
    joined into the last; `worm_boxes_hold_every_quad`) is still written
    each frame and bound to the composite (binding 7), where the worm halo
    reads it (issue #54). With a soak of 0
    the across pass, which would give back the sum texel for texel, does
    not run: the down pass reads the sum as its source (issue #35,
    `LitTarget::soak_binds[0]`, the across pass's;
    `no_soak_or_softness_runs_no_across_pass`, a level soaking 0 with no
    material soak). **By material** (issue #57, `renderer/soak.rs`): an
    opaque texel soaks by its world pixel's front surface material's
    `light_soak` (`SurfaceMaterial::light_soak()`), else by the level's;
    `soak.wgsl`'s `soak_radius` is appended from Rust: `PLAIN_PRELUDE`
    (the level's, today's shader and bytes) or `MATERIAL_PRELUDE` (group
    1: `soak_table`, 256 `i32`s by material value, −1 the level's, and
    `front_tex`, the front map), each its own pipelines
    (`soak_material_h_pipeline`, `soak_material_v_pipeline`). Each pass
    takes the receiving texel's radius and fades with `fade(i, r + 1)`;
    `MAX_SOAK` stays the bound. At a border between materials the down
    pass reads its column's across results, made with those texels' radii
    (an accepted approximation). `LookTextures::soak` (`SoakTextures`) is
    made by `set_surface` (with lighting, maps of the level's size and a
    material with its own soak; `Renderer::has_material_soak`), heat or
    none: it binds the heat's own surface texture (`Rg8Uint`, front in
    red) when there is one, else an `R8Uint` front map of its own at the
    solid map's size: one copy of the map. The map is static (the level's
    `look/surface.png`); the solid map it is read beside follows every
    dirty rectangle, so digging and new dirt change the soak at once.
    `set_surface_materials` rewrites the table (the material editor's
    preview). A material soak runs the across pass whatever the level's
    soak. Without one nothing changes: no table, no map, the plain
    pipelines (`no_material_soak_draws_todays_bytes`). Tests (GPU,
    `tests/gpu/soak.rs`): `a_materials_soak_overrides_the_levels`,
    `no_material_soak_draws_todays_bytes`,
    `a_soak_material_without_heat_still_works`,
    `material_soak_follows_digging`; `soak.rs`' unit test.
  - **Blur** (`fs_blur_h`, `fs_blur_v`): a box of the level's softness (0
    to 4 world pixels) each way, an air texel gathering only the air before
    the first solid each way (no light blurs through a wall); `fs_blur_v`
    also tones the light (`tone`, the CPU twin `olr_render::tone`: the
    identity up to 1, a shoulder to max light; a white point of 1 clips),
    bands it into the light steps and writes the light above 1,
    soft-saturated, for the bloom (a glowing pixel writes none). With a
    softness of 0 `fs_blur_h` (its source again, texel for texel) does not
    run and `fs_blur_v` reads the soaked grid itself (issue #35).
  - **Bloom** (`fs_bloom_h`, `fs_bloom_v`, plan 15c): a Gaussian of
    `BLOOM_RADIUS` (12) world pixels each way over the light grid, an air
    texel's stopping at rock; the composite adds it × `BLOOM_STRENGTH`
    (0.6; `set_bloom(false)` for tests) before the grade, never on rock
    or dirt
    (`bright_light_blooms`, `bloom_is_steady_and_camera_independent`,
    `a_dim_level_draws_as_before`, `glowing_pixels_feed_no_bloom`,
    `bloom_keeps_the_worm_floor`).
  - **Composite** (`composite.wgsl`): each world-target texel's albedo
    times the light grid at its centre (bilinear), never below its
    self-light nor its floor: the solid floor for `Shade::FLOORED` (blood,
    droppers and gibs; the user's playtest, 2026-10-19,
    `a_floored_item_never_falls_below_the_solid_floor`), the worm floor for
    `Shade::WORM` (plan 15b, `a_worm_item_never_falls_below_the_worm_floor`).
    A glowing texel (self-light above 0: the level's glow maps, a glowing
    distant layer; never heat, issue #68) shows its own art: other light
    lifts it to 1 at most and no bloom lands on it
    (`a_glowing_pixel_shows_its_art_under_bright_light`); so does any
    texel over an opaque solid's world pixel (the solid map, binding 6;
    #1: rock under a bright sky read oversaturated,
    `rock_shows_at_most_its_art_under_bright_light`).
    A worm texel takes the light where it is, as a rope pixel does (#53:
    plan 15b's lit side is gone; `a_worm_takes_the_light_where_it_stands`,
    `a_rope_pixel_has_no_side`), at least the worm floor
    (`a_worm_against_a_wall_keeps_its_floor`,
    `overlapping_worms_in_the_dark_keep_their_floor`).
    **The worm halo** (issue #54, `worm_halo` in `composite.wgsl`, the CPU
    twin `olr_look::engine::worm_halo`): a soft dark outline on the
    background behind a living worm, centred (no direction: it cannot
    drift, pop or flip), only in lit frames. It falls only on tagged
    background texels (above) whose world pixel is not rock or dirt
    (`solid_px`) and that do not glow (self-light 0: a lamp on the back
    wall stays lit); never on a sprite or the worm. With `d` the distance
    from the world pixel's centre to the nearest occluder-map texel's of
    any worm (1 right next to the silhouette) and `x = (d − 1) / r`, the
    light the texel takes (its ambient and glow light, capped, and the
    bloom on it; not its self-light, floor or grade) is scaled by `1 − s ×
    (1 − x)²`, nothing from `x = 1`: the same for every texel of a world
    pixel. `r` and `s` go linearly with the background's depth (0 for the
    level's back; `1 − shade.w` for a distant layer, read only with
    `distant.x`) from `WORM_HALO_RADIUS` 2 and `WORM_HALO_STRENGTH` 0.5 to
    `WORM_HALO_RADIUS_MAX` 5 and 0 at `WORM_HALO_FADE` 0.6 (half a byte's
    step of tolerance), none deeper (a night sky at depth 90). A texel
    outside every worm box grown by `ceil(r)` searches nothing; the
    window is at most 11 × 11. Overlapping halos take the nearest worm
    (the strongest, never the sum); each view draws its own from its own
    occluder map, which covers its light grid, so a worm just outside a
    view darkens its edge as a wider view shows it. The light grid is
    untouched (`light_is_camera_independent`, `worms_change_no_light`
    hold). Tests (GPU, `tests/gpu/halo.rs`):
    `the_halo_darkens_the_back_round_a_worm`,
    `a_deeper_layer_takes_a_wider_fainter_halo`,
    `the_halo_spares_rock_glow_and_sprites`,
    `overlapping_halos_take_the_strongest`, `the_halo_is_steady`,
    `an_off_view_worm_halos_the_view_edge`,
    `the_tag_changes_no_other_byte`.
    Graded (exposure, contrast, saturation, tint) in linear light; a
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
- **Heat** (issue #39, spec `docs/superpowers/specs/2026-10-22-materials-and-heat-design.md`;
  `renderer/heat.rs`, `look/heat*.wgsl`; no OLX counterpart). Behind a
  direct API (issue #40: `olr-present` deposits each explosion's heat):
  `set_surface(&[SurfaceMaterial], Option<&SurfaceMaps>)` before
  `upload_terrain` (`olr_present::Presentation::upload` does it on a
  whole upload); with lighting on, maps of the level's size and a
  material that heats (`SurfaceMaterial::heats`: `sensitivity` above 0) make
  `LookTextures::heat` (`HeatTextures`), made afresh by every
  `upload_terrain` (a new level, a restart); otherwise there is none and
  the level draws exactly as before (`no_materials_draws_todays_bytes`;
  the lit terrain and the emission bind `HeatPipelines::none`'s dummy
  group). `deposit_heat(HeatDeposit { pos, radius, amount })` (at most
  `MAX_HEAT_DEPOSITS` 64 a frame, radius to `MAX_HEAT_RADIUS` 64, amount
  to `MAX_HEAT_AMOUNT` 1000, a NaN dropped), `advance_heat(elapsed_ms)`
  (match time, ticks never drawn included; a step each `HEAT_STEP_MS` 20,
  at most `MAX_HEAT_STEPS` 250 a frame, the rest carried; each deposit
  lands before the step it was made in, so N steps at once draw N single
  steps' bytes), `reset_heat` (a replay seek), `has_heat`,
  `heat_active`, `heat_texture`. Issue #45 (the material editor's live
  preview): `set_surface_materials(&[SurfaceMaterial])` gives the same
  list new values and keeps the heat (temperatures, boxes, clock; the
  table, the ramps and the bounds rewritten, `HeatTextures::set_tables`),
  `false` when there is none to keep (then `set_surface` is the call;
  `new_material_values_keep_the_heat`). Issue #40: `run_heat()` runs what the
  next lit frame would (the waiting deposits, at most `MAX_HEAT_STEPS`
  steps) in a submit of its own, so a host that deposits more than
  `MAX_HEAT_DEPOSITS` or lets more steps fall due between frames
  (`heat_waiting()`, `heat_behind()`) drops and delays nothing
  (`running_heat_between_frames_equals_a_frame_a_tick`). The temperature is an `R16Uint` texture
  at world size (degrees × `HEAT_SCALE` 64: GL renders into no
  `R32Float`), with a scratch twin; a lit frame's first passes (once a
  frame, however many views: `split_screen_steps_once_per_frame`) are
  the frame's ops in order: deposits (falloff `1 − (d/R)²`, × the
  material's sensitivity over its capacity, × what the deposit reaches;
  issue #56, `heat_reach`: a grid walk from the centre to the pixel's
  centre over the solid map, the centre's pixel and the pixel included,
  a corner closed when both pixels beside it are rock or dirt (the glow
  light's rule), outside the map open; before the first rock or dirt
  pixel a back pixel takes the ball's slice at the back's depth, `1 −
  (d² + z²)/R²` (issue #65: the level's wall lies
  `olr_look::engine::BACK_DEPTH_PX` 14 behind the plane, read from the
  table's head `w`; 0 for a distant layer, whose deposits come sliced);
  the solid pixels met in a row are the skin: while every one is dirt,
  the k-th of the first `DIRT_SKIN` 6 takes `DIRT_HEAT_GAIN` 2.5 ×
  `DIRT_SKIN_WEIGHTS[k − 1]` (1, 1, 0.8, 0.6, 0.4, 0.2) with no falloff
  (issue #65: dirt takes the blast whole); once the walk has met rock
  only the first `HEAT_SKIN` 2 take any, a rock pixel `HEAT_SKIN_GAIN`
  1.0 × `HEAT_SKIN_WEIGHTS[k − 1]` (1, 0.5) by the falloff (rock shields
  the dirt behind it); past the skin, deeper or the air behind it,
  nothing, so conduction carries the heat in; a centre in rock is its
  own skin's first pixel; a walk visits at most `HEAT_WALK_STEPS`
  pixels; each deposit added in whole texel units, so deposits landing
  in one pass or several give the same bytes), steps (into the scratch over
  the active boxes, scissored, copied back), layer resets (issue #67,
  below) and zeroes. A step: each
  4-neighbour of the same layer (issue #67: both solid, rock or dirt by
  `heat_is_solid`, or both background; one of the other layer gives
  nothing, so no heat crosses between the dirt and the wall behind it,
  while rock and dirt warm each other) gives `min(MAX_COUPLING 0.24,
  CONDUCT 0.05 × min(k_p, k_q) / c_p)` of its difference (conductivity,
  capacity as shares of stone's; a convex combination, stable for every
  value), then
  a pixel loses `q × (COOL_RATE 0.006 × T + COOL_MIN 0.05)` (q its
  cooling share), a quarter of it (`BULK`) when neither it nor a
  4-neighbour is open (the solid map); outside the level is rock that
  conducts nothing; a pixel without a heating material holds none. The
  visible surface's material rules (`surface_texels`: the front's where
  the solid map is rock or dirt, else the back's). **A pixel that
  changes layer starts cold** (issue #67): `HeatTextures::seen` (`R8Uint`)
  is the solid map as the heat last saw it; each `update_terrain`
  rectangle that rewrites the solid map (`LookTextures::write` →
  `HeatTextures::relayer`) is, while `HeatSim::active`, a layer reset
  queued among the deposits in call order (`HeatSim::relayer`: after the
  deposits and steps queued before it, before those after, so play's
  `update_terrain` then deposits leaves a fresh crater's back its slice;
  one right after another joined, at most `MAX_HEAT_LAYERS` 64 waiting,
  then joined to the last), run as `HeatOp::Layer`: `fs_heat_layer`
  zeroes, over the boxes within the rectangle, each pixel whose class
  (solid or background) differs from `seen`'s, then the rectangle of the
  solid map is copied into `seen`; rock ↔ dirt is no change. With
  nothing hot no pass runs: `seen` takes the same bytes as the solid map
  (an upload). A new `HeatTextures` copies the solid map into `seen` in
  a submit of its own; a `HeatOp::Clear` copies it whole
  (`a_dug_pixel_loses_its_heat`, `new_dirt_starts_cold`,
  `a_fresh_craters_back_takes_its_slice`, `heat_stays_in_its_layer`,
  `layer_resets_land_in_call_order`). The CPU
  (`HeatSim`) keeps at most 8 boxes: a deposit's square, grown a pixel a
  step to `HEAT_SPREAD` (48) past it, overlapping ones joined; a box is
  zeroed and dropped once an upper bound of its temperature
  (`hot × (1 − α) − β`, the slowest cooling; `hot` counts dirt's 2.5,
  issue #65) is under the level's visible
  threshold (its ramps' first key) and 100 steps passed since its last
  deposit, or after 5 min of match time (a material that never cools);
  when every box will cool before the next deposit it is zeroed at once.
  No box, no pass (`nothing_hot_runs_no_pass`), and the materials
  uniform's `on` is 0, so the look reads none. Look (issue #68): a hot
  pixel is drawn as it looks cold plus its heat as emitted light. The
  cold part is all a texel shows without heat (its art lit as before:
  the light, the cap, the bloom, the soak, the worm halo, its own glow);
  heat changes none of it and is no self-light. The heat part, `E =
  to_linear(ramp colour) × g²` (`hot` in `world_lit.wgsl`; `HeatLook`, a
  256-bin `Rgba8Unorm` row per material from `olr_look`'s ramp, alpha
  `g` the glow), goes to the world pass's heat light target, which the
  composite (binding 8) adds to the linear light after the cap, before
  the grade: before the grade a hot pixel is never darker than cold, and
  with a ramp whose colour and glow do not fall as it heats (the lava
  ramp) it moves steadily, channel by channel, back to exactly its cold
  bytes as it cools (`a_cooling_pixel_fades_back_to_its_cold_look`,
  `a_cooling_distant_layer_fades_back_too`); in the dark it shows about
  as before (`a_hot_rim_still_glows_in_the_dark`,
  `a_hot_pixel_shows_its_colour_in_every_view`). Full bright and light
  only keep issue #39's look: the ramp colour mixed over the art by the
  glow, the glow taken as self-light, no heat light (the material
  editor's full bright); the glow light's
  emission adds its linear colour × glow × the material's `light` ×
  `HEAT_CAST` (reach `HEAT_REACH` 200; `a_hot_pixel_adds_glow_light`).
  Timed as `PassGroup::Heat`. Tests (GPU, `tests/gpu/heat.rs`): those
  named and `a_deposit_heats_only_material_pixels_and_spreads_only_into_conducting_ones`,
  issue #56's `a_deposit_heats_only_the_rock_face`,
  `heat_soaks_inward_from_the_face`, `a_wall_shields_what_is_behind_it`,
  `a_diagonal_wall_shields_too`, `a_deposit_inside_rock_heats_round_its_centre`,
  `the_back_in_sight_takes_its_share`, issue #65's
  `the_back_takes_the_balls_slice_14_px_back`,
  `dirt_takes_the_blast_whole_six_pixels_deep` and
  `rock_shields_the_dirt_behind_it` (issue #56's tests run on rock
  levels), issue #67's named above,
  `sensitivity_zero_never_heats`, `heat_cools_to_nothing`,
  `new_material_values_keep_the_heat`,
  `stepping_n_ticks_at_once_equals_n_single_steps`,
  `hostile_values_stay_bounded`, `a_reset_or_a_new_level_clears_the_heat`;
  `heat.rs`'s unit tests (boxes, bounds, tables).
  **Distant layers** (issue #41, the spec's "Distant layers: the ball"):
  `set_distant_surface(&[u8])` (each layer's material value in
  `set_surface`'s materials, 0 none) before `set_distant`, which, lit with
  the terrain up, gives a layer whose material heats and that
  `MAX_HEAT_RADIUS` reaches at its `z` (`layer_heats`: `depth ×
  olr_look::engine::layer_z` < 64; depth 45 at most) a
  `HeatTextures` of its own at its world size (art ÷ the level's art
  scale): the level's table, surface texels its material front and back
  where its image covers (`layer_texels`), a solid map all background, so
  it takes the CPU's slice whole (issue #65; a walk there never meets
  rock, issue #56) and cools as faced
  by the air; no conduction to the level or between layers (`set_lighting`
  remakes them, `upload_terrain` resets them; `set_surface` with maps
  naming nothing keeps the table for them). `deposit_heat_seen(deposit,
  &[HeatView { view, camera }])` (`HeatView::of(&DrawList)`) is
  `deposit_heat` plus each heated layer's `olr_look::engine::heat_slice`
  (radius `sqrt(R² − z²)`, amount `× (1 − z²/R²)`, none when `z ≥ R`),
  centred on the layer pixel drawn behind the deposit by the view nearest
  it (`nearest`: the distance to its rectangle, the first of those as
  near): the deposit less `frame::distant_origin` (the rounded texel
  placement `distant_instance` draws) of that view's `WorldFrame` and the
  layer's `position(camera)`; no views, no slices. The heat then stays in
  the layer's texture and moves with it in every view. Each layer steps
  on the same clock (`advance_heat`, `reset_heat`, `run_heat`, the
  frame's heat passes after the level's; `heat_waiting` and
  `heat_behind` the most of any; `has_heat` and `heat_active` any;
  `distant_heat_texture(i)`, `i` in `set_distant`'s list). Look: the lit
  distant pipeline binds the look (group 2) and the layer's heat look
  (group 3, or `none`); `fs_distant_lit` looks hot as the lit terrain
  does (`hot`, issue #68: its heat light added over its cold look; full
  bright and light only, the ramp colour over its art and the larger of
  the glow and the layer's own glow, issue #62, as self-light; cold: the
  same bytes).
  It casts no light: the emission reads only the level's heat (a layer's
  placement differs per view, the glow light is one a frame). Tests
  (GPU): `a_near_layer_takes_a_smaller_circle_than_the_plane`,
  `a_layer_beyond_the_largest_radius_has_no_texture`,
  `the_spot_moves_with_the_layer_as_the_camera_pans`,
  `with_two_views_the_nearest_views_camera_places_it`,
  `a_hot_layer_adds_no_glow_light`; unit:
  `distant_layers_pick_their_view_reach_and_texels`.
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
  view-plan buffers are reused (issue #35: the glow light's passes keep
  their attachments on the stack), the world targets and instance buffer are
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
  `PassGroup` (heat, issue #39; world and occluders; emission: the glow
  light's discs and emission; jump flood; cascades; fluence and its
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
  (`fixtures/look/mods`), seed 1, 300 ticks; issue #40's `heat-fresh.png`
  and `heat-cooled.png` are `render-match` of the heat test level and mod
  (`look-test-heat`, `look-test-heat-mod`): rims glowing, then cooled.
  Byte identity (issue #34):
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
`a_skin_light_lights_the_air_and_a_wall_stops_it`; issue #39: heat, `tests/gpu/heat.rs`; issue #54: the
worm halo, `tests/gpu/halo.rs`) and goldens need
`OLR_GPU_TESTS=1`; `cargo xtask full` sets it and runs them, skipping with a
notice when no adapter opens (`cargo run -p olr-render --example adapter`).
