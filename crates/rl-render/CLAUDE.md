# rl-render

The wgpu renderer (spec §8.2, §19 E3): it draws `rl-scene`'s `DrawList`
on screen or offscreen.

- **Layer 5, uses `std` and wgpu 30.0.1.** Depends on `rl-scene` (same
  layer, `xtask/layers.ron`), `rl-terrain`, `rl-data`, `rl-pack` (PNG),
  `bytemuck`, `pollster`. `#![forbid(unsafe_code)]`: wgpu needs no unsafe
  here (instances are `[f32; 12]` cast by `bytemuck`, no derive).
- **Use** (`renderer.rs`): `Renderer::new(device, queue, surface_format,
  RenderSettings)`; `upload_atlas(&SceneAssets)` once every image is
  registered (scene, worms, `rl-fx`); `upload_terrain(&ColorMap)` for a new
  or restarted level, then `update_terrain(&mut ColorMap)` after each tick
  (only `take_dirty()`'s rectangles); `render(&DrawList, &view, size)` per
  drawn frame (`render_in(.., area)`: the picture in a part of the target,
  `dest_rect_in`; the game's playfield above its HUD bar; `rl_present::Presentation::upload`
  does the uploads in this order). **An image registered after `upload_atlas`** (a worm added
  later) draws as a magenta square until `upload_atlas` runs again.
- **World pass** (`frame.rs`, `world.wgsl`): a target covering the view
  plus `MARGIN` world pixels, at `scale()` texels per world pixel
  (`MIN_SCALE` 2, OLX's sprite art, or the level's art scale if larger). The
  terrain is one opaque RGBA texture of the `ColorMap` composite (spec §8.2's
  separate back, front and material textures are not needed: `rl-terrain`
  composites on the CPU). Sprites snap to whole texels; line pixels are
  world-pixel blocks of the atlas's white texel. Texels are fetched with
  `textureLoad`, clamped to the quad's source: nearest, no bleeding.
  Blending is on the stored bytes (gamma space, like SDL).
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
  `render_to_png`, `read_texture`. `NoAdapter` means skip, never panic.
- **Goldens** (`rl-cli/tests/render_golden.rs`, `fixtures/render/frame-*.png`):
  `rl-cli render` of one oracle scenario per level (`dig` on Dirt Level,
  `rope` on Liero Factory, `weapons/classic/bazooka` on FossilFacility,
  `castle-shoot-rope` on CastleStrike, the 1× level), drawn from the
  imported OLX content at test time (`RL_CONTENT_DIR`) and compared with
  `perceptual_diff` (tolerance 0.1, at most 0.002 % of pixels: lavapipe and llvmpipe GL both draw the four goldens with no differing pixel). They were
  blessed on lavapipe; they are accepted fixtures, the OLX packs they are
  drawn from are never committed. Re-bless with `cargo xtask bless-render
  --reason "<why>"` (without an adapter it re-draws only the CPU golden).
- **Instance buffer** (`instance_buffer_bytes()`): grows to the busiest
  frame's next power of two and never shrinks; `rl-game`'s soak test
  (`tests/soak.rs`) bounds it.
- **WSL:** under `GALLIUM_DRIVER=d3d12` with `WGPU_BACKEND=gl` (the real
  GPU) a thread that used the device segfaults when it exits (a driver
  bug), so the GPU tests, which run on test threads, crash there; the
  goldens (one `rl-cli` process each, main thread) pass. The default
  software adapter has no such problem.

Bench: `benches/frame.rs` (divan, `cargo xtask bench`): a frame of a
synthetic 504 × 350 level with 2000 sprites and 500 line pixels at 1080p
and 1440p, waited for on the device; without an adapter it prints a
notice and measures nothing.

Test: `cargo nextest run -p rl-render` (CPU: packing, quads, upscale
geometry, image diff). The GPU tests (`tests/gpu.rs`) and goldens need
`RL_GPU_TESTS=1`; `cargo xtask full` sets it and runs them, skipping with a
notice when no adapter opens (`cargo run -p rl-render --example adapter`).
