//! The wgpu renderer (spec §8.2, §19 E3): it draws a [`olr_scene::DrawList`]
//! on screen or offscreen.
//!
//! - **World pass** at art scale (at least [`MIN_SCALE`], OLX's 2× sprite
//!   art) into a texture covering the view plus a [`MARGIN`] of world
//!   pixels: the colour terrain, then every sprite and line pixel as one
//!   instanced quad per item from a single atlas, in the list's order. No
//!   antialiasing: lines are exact world-pixel blocks.
//! - **Upscale pass** from the view's rectangle of that texture to the
//!   output with the selected [`UpscaleFilter`]; a sub-pixel camera moves
//!   the source rectangle, never the world's pixels.
//!
//! The CPU halves (atlas packing, quads, upscale geometry, image
//! comparison) are pure and tested in the fast gate; the GPU halves run in
//! `cargo xtask full`.

#![forbid(unsafe_code)]

mod atlas;
mod diff;
mod frame;
mod look;
mod offscreen;
mod renderer;
mod upscale;

pub use atlas::{AtlasLayout, PackedAtlas, TexRect, WHITE, pack};
pub use diff::{ImageDiff, perceptual_diff};
pub use frame::{
    INSTANCE_FLOATS, Instance, LookTextureSizes, MARGIN, MAX_VIEW, MISSING, WorldFrame, look_texture_sizes,
    push_instances, terrain_instance,
};
pub use look::{LightGrid, grade_linear, linear_to_srgb, shade_texel, tone};
pub use offscreen::{
    AdapterPreference, NoAdapter, OUTPUT_FORMAT, Offscreen, OffscreenOptions, list_adapters,
};
pub use renderer::{
    GI_CELL_BUDGET, GpuTimes, MAX_PROBE_PASSES, MAX_PROBE_TAPS, MIN_SCALE, Probe, RenderSettings, Renderer,
    WORLD_FORMAT, WORM_SHADOW, gi_cell, gpu_times,
};
pub use upscale::{UpscaleFilter, dest_rect, dest_rect_in};
