//! The look's GPU state (plan 15a): its textures (glow, glow groups, solid
//! map), its per-frame globals and the lit world pass's pipelines. Made by
//! [`Renderer::set_lighting`]; without it nothing here exists and every
//! frame is drawn as before.

use olr_data::level::Material;
use olr_look::{LookFrame, ViewMode};
use olr_terrain::{ColorMap, Rect};

use super::{Renderer, WORLD_FORMAT, entry, pipeline};
use crate::frame::{WorldFrame, look_texture_sizes};

/// The shared WGSL of every look pass.
const COMMON: &str = include_str!("../look/common.wgsl");

/// [`COMMON`] after the Rust constants the shaders share (issue #54: the
/// worm halo's, `olr_look::engine`'s).
pub(super) fn common() -> String {
    use olr_look::engine::{WORM_HALO_FADE, WORM_HALO_RADIUS, WORM_HALO_RADIUS_MAX, WORM_HALO_STRENGTH};
    format!(
        "const LIGHT_PAD: f32 = {LIGHT_PAD}.0;\nconst MAX_SOAK: i32 = {};\nconst BLOOM_RADIUS: i32 = {BLOOM_RADIUS};\nconst OCCLUSION_PAD: i32 = {OCCLUSION_PAD};\nconst WORM_BOXES: i32 = {WORM_BOXES};\nconst WORM_HALO_RADIUS: f32 = {WORM_HALO_RADIUS:?};\nconst WORM_HALO_RADIUS_MAX: f32 = {WORM_HALO_RADIUS_MAX:?};\nconst WORM_HALO_STRENGTH: f32 = {WORM_HALO_STRENGTH:?};\nconst WORM_HALO_FADE: f32 = {WORM_HALO_FADE:?};\n{COMMON}",
        olr_look::MAX_SOAK
    )
}

/// The background tag (issue #54): the shade's marker channel of the lit
/// terrain (its opaque backing too) and of a distant layer, so the
/// composite tells the background from an opaque sprite, whose shade is
/// otherwise the terrain's. A marker writes 1, anything else 0.
const BACKGROUND_TAG: f32 = 0.25;

/// Groups in the globals (a group map value is one byte, 0 none).
pub(super) const GROUPS: usize = 255;

/// The globals' floats: ten vec4s, then [`GROUPS`] groups of three vec4s
/// (cast, info, flicker). A frame packs and writes only its groups
/// ([`GLOBAL_HEAD`] and 12 floats a group; issue #35).
pub(super) const GLOBAL_FLOATS: usize = GLOBAL_HEAD + 12 * GROUPS;

/// The globals' floats before the groups.
const GLOBAL_HEAD: usize = 4 * 10;

/// The occluder map's format (plan 15b): each texel a worm's index + 1, 0
/// none (GL renders into `R8Uint`).
const OCCLUDER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Uint;

/// The format of the world pass's second target: self-light, marker, solid floor, alpha.
pub(super) const SHADE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The look's textures for one colour map.
pub(super) struct LookTextures {
    pub(super) glow: wgpu::Texture,
    pub(super) groups: wgpu::Texture,
    pub(super) solid: wgpu::Texture,
    pub(super) solid_view: wgpu::TextureView,
    /// The solid map as written when the level has clear solids: its
    /// material with each clear solid as background (spec §Clear solids).
    /// Empty otherwise; the level's size once made.
    light_cells: Vec<u8>,
    pub(super) art_scale: u32,
    /// The level's size in world pixels (uncropped).
    pub(super) level: (u32, u32),
    /// The lit terrain's group 2: globals, glow, groups, flicker regions.
    pub(super) terrain_bind: wgpu::BindGroup,
    /// Each world pixel's flicker region (plan 15c), `R16Uint`.
    pub(super) region_map: wgpu::Texture,
    regions: super::regions::Regions,
    /// The level's glow light.
    pub(super) gi: super::gi::GiState,
    /// The level's heat (issue #39), only when its materials heat.
    pub(super) heat: Option<super::heat::HeatTextures>,
    /// The level's material soak (issue #57), only when a material sets
    /// its own soak.
    pub(super) soak: Option<super::soak::SoakTextures>,
}

impl LookTextures {
    /// The heat look's bind group: the level's, or `none`'s (off).
    pub(super) fn heat_bind<'a>(&'a self, none: &'a wgpu::BindGroup) -> &'a wgpu::BindGroup {
        self.heat.as_ref().map_or(none, |h| &h.look_bind)
    }

    /// Makes the level's heat and material soak from `surface` (its maps
    /// and materials, issues #39 and #57), or none.
    pub(super) fn set_surface(
        &mut self,
        device: &wgpu::Device,
        queue: &super::uploads::Queue,
        (pipes, soak_layout): (&super::heat::HeatPipelines, &wgpu::BindGroupLayout),
        surface: Option<&super::Surface>,
    ) {
        self.soak = None;
        self.heat = surface.and_then(|s| {
            let (world, texels) = s.maps.as_ref().filter(|(w, _)| *w == self.level)?;
            let size = (self.solid.width(), self.solid.height());
            Some(super::heat::HeatTextures::new(
                device,
                queue,
                pipes,
                *world,
                size,
                texels,
                s.tables.clone()?,
                &self.solid_view,
            ))
        });
        self.set_soak(device, queue, soak_layout, surface);
    }

    /// Makes the level's material soak (issue #57) from `surface`, or
    /// none: it reads the heat's surface texture when there is one (so
    /// after the heat), else a front map of its own.
    pub(super) fn set_soak(
        &mut self,
        device: &wgpu::Device,
        queue: &super::uploads::Queue,
        layout: &wgpu::BindGroupLayout,
        surface: Option<&super::Surface>,
    ) {
        let made = surface.and_then(|s| Some((s.soak.as_ref()?, s.maps.as_ref()?)));
        let Some((table, (world, texels))) = made.filter(|(_, (w, _))| *w == self.level) else {
            self.soak = None;
            return;
        };
        if let Some(soak) = &self.soak {
            soak.set_table(queue, table);
            return;
        }
        let size = (self.solid.width(), self.solid.height());
        let heat_surface = self.heat.as_ref().map(|h| &h.surface_view);
        self.soak = Some(super::soak::SoakTextures::new(
            device,
            queue,
            layout,
            table,
            (*world, size),
            heat_surface,
            texels,
        ));
    }
}

/// The light grid's format.
pub(super) const LIGHT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// World pixels the light grid reaches past the world target on every side
/// (`LIGHT_PAD` in the shaders): the soak (up to [`olr_look::MAX_SOAK`]), a
/// blur of up to 4 and the bloom ([`BLOOM_RADIUS`]) never see its edge, so
/// the light never depends on the camera.
pub(super) const LIGHT_PAD: u32 = olr_look::MAX_SOAK as u32 + 4 + BLOOM_RADIUS as u32;

/// The bloom's reach, world pixels (plan 15c's glare): a Gaussian of a
/// third of it each way.
pub(super) const BLOOM_RADIUS: i32 = 12;

/// Rows the ambient occlusion's first pass covers above and below the light
/// grid (issue #10): its longest reach, so the second pass of every grid
/// texel sees each solid within the reach, wherever the view is.
pub(super) const OCCLUSION_PAD: u32 = olr_look::MAX_OCCLUSION_REACH as u32;

/// How much of the light above 1 the bloom adds back, spread (plan 15c; a
/// starting value judged on the day Castle Strike).
pub(super) const BLOOM_STRENGTH: f32 = 0.6;

/// One world target's look passes (one a view, issue #26): the light grid
/// (gathered into 0, soaked into solids across into 1 and down into 2,
/// blurred across into 0 and down, capped and banded, into 1) and the lit
/// target the upscale reads.
pub(super) struct LitTarget {
    /// World pixels: the light grid's size.
    pub(super) grid: (u32, u32),
    /// The view's look globals (`LookGlobals`, [`GLOBAL_FLOATS`] floats:
    /// where its world target and occluder map lie), which its passes read.
    pub(super) globals: wgpu::Buffer,
    pub(super) light: [wgpu::Texture; 3],
    pub(super) views: [wgpu::TextureView; 3],
    /// The sum pass's, the across blur's (reads 2) and the down blur's
    /// (reads 0); with no softness the down blur takes the across blur's
    /// (issue #35: no across pass; it reads 2).
    pub(super) binds: [wgpu::BindGroup; 3],
    /// The ambient occlusion's rows (issue #10): the light grid's width,
    /// its height and [`OCCLUSION_PAD`] above and below.
    pub(super) occlusion_view: wgpu::TextureView,
    /// The occlusion's first pass's (binds a dark texture for the rows it writes).
    pub(super) occlusion_bind: wgpu::BindGroup,
    /// The bloom (plan 15c): the down blur writes the light above 1 into 0,
    /// spread across into 1 and down into 0, which the composite adds.
    pub(super) bloom_views: [wgpu::TextureView; 2],
    /// The across spread's (reads 0) and the down spread's (reads 1).
    pub(super) bloom_binds: [wgpu::BindGroup; 2],
    /// Reading 0 (the across soak's, and with no soak the down soak's:
    /// issue #35, no across pass) and reading 1 (the down soak's).
    pub(super) soak_binds: [wgpu::BindGroup; 2],
    /// The view's worm boxes ([`WormBoxes`], issue #35): where the
    /// composite's worm halo searches the occluder map (issue #54).
    pub(super) worm_boxes: wgpu::Buffer,
    pub(super) composite_bind: wgpu::BindGroup,
    /// The occluder map (plan 15b): one texel per world pixel of the grid,
    /// the living worms' bodies; it takes no part in the light (#53), the
    /// composite's worm halo reads it (issue #54).
    pub(super) occluder_view: wgpu::TextureView,
    /// The occluder pass's group 0: [`Self::globals`].
    pub(super) occluder_globals_bind: wgpu::BindGroup,
    pub(super) lit_view: wgpu::TextureView,
    /// The upscale pass's view of the lit target.
    pub(super) upscale_bind: wgpu::BindGroup,
}

pub(super) struct LookState {
    pub(super) terrain_pipeline: wgpu::RenderPipeline,
    pub(super) sprite_pipeline: wgpu::RenderPipeline,
    pub(super) sum_pipeline: wgpu::RenderPipeline,
    /// The ambient occlusion's first pass (issue #10).
    pub(super) occlusion_pipeline: wgpu::RenderPipeline,
    pub(super) blur_h_pipeline: wgpu::RenderPipeline,
    pub(super) blur_v_pipeline: wgpu::RenderPipeline,
    pub(super) bloom_h_pipeline: wgpu::RenderPipeline,
    pub(super) bloom_v_pipeline: wgpu::RenderPipeline,
    pub(super) composite_pipeline: wgpu::RenderPipeline,
    pub(super) soak_h_pipeline: wgpu::RenderPipeline,
    pub(super) soak_v_pipeline: wgpu::RenderPipeline,
    pub(super) soak_layout: wgpu::BindGroupLayout,
    /// The soak by material (issue #57): across and down, with group 1
    /// (`soak_material_layout`) the table and the front map.
    pub(super) soak_material_h_pipeline: wgpu::RenderPipeline,
    pub(super) soak_material_v_pipeline: wgpu::RenderPipeline,
    pub(super) soak_material_layout: wgpu::BindGroupLayout,
    /// Draws the occluder map (plan 15b): group 0 the view's globals, group 1 the atlas.
    pub(super) occluder_pipeline: wgpu::RenderPipeline,
    occluder_globals_layout: wgpu::BindGroupLayout,
    pub(super) light_layout: wgpu::BindGroupLayout,
    pub(super) composite_layout: wgpu::BindGroupLayout,
    pub(super) layout: wgpu::BindGroupLayout,
    /// The frame's globals (issue #26): the glow light's and the lit
    /// terrain's, which read none of a view's fields (the first view's).
    pub(super) globals: wgpu::Buffer,
    pub(super) gi: super::gi::GiPipelines,
    /// A 1 × 1 dark glow light, bound before a level's exists.
    no_glow: wgpu::TextureView,
    /// A 1 × 1 solid map, bound before a level's exists (the soak reads
    /// nothing outside the level, whose size is then 0).
    no_solid: wgpu::TextureView,
    pub(super) textures: Option<LookTextures>,
    /// The packed globals, reused every frame.
    pub(super) packed: Vec<f32>,
    /// The packed discs, reused every frame: [`super::gi::EMITTER_FLOATS`]
    /// each, the first [`olr_look::MAX_EMITTERS`] of the frame's (plan
    /// 15c; the level's glow light keeps their buffers).
    pub(super) packed_emitters: Vec<f32>,
    /// The discs this frame draws.
    pub(super) emitter_count: u32,
    /// Whether the bloom is added (tests turn it off: `Renderer::set_bloom`).
    pub(super) bloom: bool,
    /// Whether the worm halo darkens the background (issue #54; tests turn
    /// it off: `Renderer::set_worm_halo`).
    pub(super) halo: bool,
    /// Whether the background is tagged (issue #54; tests turn it off:
    /// `Renderer::set_background_tag`, part 1's shade bytes).
    pub(super) tag: bool,
    /// The terrain opaque under a level's distant layers, and the layers
    /// (issue #11).
    pub(super) backing_pipeline: wgpu::RenderPipeline,
    pub(super) distant_pipeline: wgpu::RenderPipeline,
    /// Heat's passes and its look's layout (issue #39).
    pub(super) heat: super::heat::HeatPipelines,
}

const UINT_TEXTURE: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Uint,
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};

const FLOAT_TEXTURE: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Float { filterable: false },
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};

fn r8(device: &wgpu::Device, label: &str, size: (u32, u32), format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Writes rectangle `r` (texels) of a `width`-texel-wide one-byte-per-texel
/// image into `texture`, clipped to it.
pub(super) fn write_bytes(
    queue: &super::uploads::Queue,
    texture: &wgpu::Texture,
    (data, width): (&[u8], u32),
    r: Rect,
) {
    let (tw, th) = (texture.width(), texture.height());
    let (w, h) = (r.w.min(tw.saturating_sub(r.x)), r.h.min(th.saturating_sub(r.y)));
    let end =
        (u64::from(r.y) + u64::from(h).saturating_sub(1)) * u64::from(width) + u64::from(r.x) + u64::from(w);
    if w == 0 || h == 0 || r.x.saturating_add(w) > width || end > data.len() as u64 {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x: r.x, y: r.y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::TexelCopyBufferLayout {
            offset: u64::from(r.y) * u64::from(width) + u64::from(r.x),
            bytes_per_row: Some(width),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

/// [`write_bytes`] for two bytes a texel (`R16Uint`).
fn write_u16(queue: &super::uploads::Queue, texture: &wgpu::Texture, (data, width): (&[u16], u32), r: Rect) {
    let (tw, th) = (texture.width(), texture.height());
    let (w, h) = (r.w.min(tw.saturating_sub(r.x)), r.h.min(th.saturating_sub(r.y)));
    let end =
        (u64::from(r.y) + u64::from(h).saturating_sub(1)) * u64::from(width) + u64::from(r.x) + u64::from(w);
    if w == 0 || h == 0 || r.x.saturating_add(w) > width || end > data.len() as u64 {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x: r.x, y: r.y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(data),
        wgpu::TexelCopyBufferLayout {
            offset: 2 * (u64::from(r.y) * u64::from(width) + u64::from(r.x)),
            bytes_per_row: Some(2 * width),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

/// The most worm boxes a view's uniform holds (issue #35); quads past the
/// last box's are joined into it.
pub(super) const WORM_BOXES: usize = 32;

/// The worm boxes' uniform (`WormBoxes` in `composite.wgsl`): the count, then
/// [`WORM_BOXES`] boxes, four `i32`s each.
pub(super) type WormBoxes = [[i32; 4]; 1 + WORM_BOXES];

/// The boxes of a view's occluder quads (`occluders`, their `dst` in the
/// occluder map's texels, which are the light grid's; issue #35, the worm
/// halo's since #54): each `[x0, y0, x1, y1)`, a texel wider every way than
/// the texels the quad can cover, so no worm texel lies outside them; past
/// [`WORM_BOXES`] quads the last box holds the rest. `out[0][0]` is the
/// count.
pub(super) fn worm_boxes(occluders: &[crate::frame::Instance], out: &mut WormBoxes) {
    let mut n = 0;
    for o in occluders {
        let [x, y, w, h] = o.dst;
        let b = [
            x.floor() as i32 - 1,
            y.floor() as i32 - 1,
            (x + w).ceil() as i32 + 1,
            (y + h).ceil() as i32 + 1,
        ];
        if n < WORM_BOXES {
            n += 1;
            out[n] = b;
        } else {
            let last = &mut out[n];
            *last = [last[0].min(b[0]), last[1].min(b[1]), last[2].max(b[2]), last[3].max(b[3])];
        }
    }
    out[0] = [n as i32, 0, 0, 0];
}

/// Art rectangle `r` as the world rectangle holding it at `s`.
fn world_rect(r: Rect, s: u32) -> Rect {
    let s = s.max(1);
    let (x0, y0) = (r.x / s, r.y / s);
    let (x1, y1) = (r.x.saturating_add(r.w).div_ceil(s), r.y.saturating_add(r.h).div_ceil(s));
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// Where a view's occluder map lies (plan 15b: over its light grid): origin
/// x, y (world pixels), width, height.
fn occluder_rect(frame: &WorldFrame) -> [f32; 4] {
    let pad = LIGHT_PAD as i32;
    [
        (frame.origin.0 - pad) as f32,
        (frame.origin.1 - pad) as f32,
        (frame.size.0 + 2 * LIGHT_PAD) as f32,
        (frame.size.1 + 2 * LIGHT_PAD) as f32,
    ]
}

impl LookState {
    pub(super) fn new(
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
        texture_layout: &wgpu::BindGroupLayout,
        distant_layout: &wgpu::BindGroupLayout,
    ) -> LookState {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render look"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, FLOAT_TEXTURE),
                entry(2, frag, UINT_TEXTURE),
                entry(3, frag, UINT_TEXTURE),
            ],
        });
        let heat = super::heat::HeatPipelines::new(device);
        let source = format!(
            "{}\n{}\n{}",
            common(),
            super::heat::heat_reader(3),
            include_str!("../look/world_lit.wgsl")
        );
        let attributes =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x3];
        let quads = || wgpu::VertexBufferLayout {
            array_stride: super::INSTANCE_STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &attributes,
        };
        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        let targets = [(WORLD_FORMAT, blend), (SHADE_FORMAT, blend)];
        let terrain_pipeline = pipeline(
            device,
            "olr-render terrain lit",
            &source,
            ("vs_world", "fs_terrain_lit"),
            &[Some(globals_layout), Some(texture_layout), Some(&layout), Some(&heat.look_layout)],
            &[Some(quads())],
            &targets,
            wgpu::PrimitiveTopology::TriangleStrip,
        );
        let sprite_pipeline = pipeline(
            device,
            "olr-render sprites lit",
            &source,
            ("vs_world", "fs_sprite_lit"),
            &[Some(globals_layout), Some(texture_layout)],
            &[Some(quads())],
            &targets,
            wgpu::PrimitiveTopology::TriangleStrip,
        );
        let backing_pipeline = pipeline(
            device,
            "olr-render terrain backing lit",
            &source,
            ("vs_world", "fs_terrain_backing_lit"),
            &[Some(globals_layout), Some(texture_layout), Some(&layout), Some(&heat.look_layout)],
            &[Some(quads())],
            &targets,
            wgpu::PrimitiveTopology::TriangleStrip,
        );
        // Unblended: the shade's alpha is the layer's depth.
        let distant_pipeline = pipeline(
            device,
            "olr-render distant lit",
            &source,
            ("vs_world", "fs_distant_lit"),
            &[Some(globals_layout), Some(distant_layout), Some(&layout), Some(&heat.look_layout)],
            &[Some(quads())],
            &[(WORLD_FORMAT, None), (SHADE_FORMAT, None)],
            wgpu::PrimitiveTopology::TriangleStrip,
        );
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render light"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, FLOAT_TEXTURE),
                entry(2, frag, FLOAT_TEXTURE),
                entry(3, frag, UINT_TEXTURE),
                entry(4, frag, UINT_TEXTURE),
                entry(5, frag, FLOAT_TEXTURE),
            ],
        });
        let no_glow = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("olr-render no glow light"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: LIGHT_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        let no_solid = r8(device, "olr-render no solid map", (1, 1), wgpu::TextureFormat::R8Uint)
            .create_view(&wgpu::TextureViewDescriptor::default());
        let soak_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render soak"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, FLOAT_TEXTURE),
                entry(2, frag, UINT_TEXTURE),
                entry(3, frag, FLOAT_TEXTURE),
            ],
        });
        // Issue #57: the plain soak (the level's, today's) and the soak by
        // material, which also reads group 1.
        let soak_material_layout = super::soak::layout(device);
        let soak_source =
            |prelude: &str| format!("{}\n{}\n{prelude}", common(), include_str!("../look/soak.wgsl"));
        let (plain, by_material) =
            (soak_source(super::soak::PLAIN_PRELUDE), soak_source(super::soak::MATERIAL_PRELUDE));
        let soak = |source: &str, fs: &str, layouts: &[Option<&wgpu::BindGroupLayout>]| {
            pipeline(
                device,
                "olr-render soak",
                source,
                ("vs_full", fs),
                layouts,
                &[],
                &[(LIGHT_FORMAT, None)],
                wgpu::PrimitiveTopology::TriangleList,
            )
        };
        let soak_h_pipeline = soak(&plain, "fs_soak_h", &[Some(&soak_layout)]);
        let soak_v_pipeline = soak(&plain, "fs_soak_v", &[Some(&soak_layout)]);
        let both = [Some(&soak_layout), Some(&soak_material_layout)];
        let soak_material_h_pipeline = soak(&by_material, "fs_soak_h", &both);
        let soak_material_v_pipeline = soak(&by_material, "fs_soak_v", &both);
        let gi = super::gi::GiPipelines::new(device, &heat.look_layout);
        let emitter_floats = olr_look::MAX_EMITTERS * super::gi::EMITTER_FLOATS;
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render composite"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, FLOAT_TEXTURE),
                entry(2, frag, FLOAT_TEXTURE),
                entry(3, frag, FLOAT_TEXTURE),
                entry(4, frag, UINT_TEXTURE),
                entry(5, frag, FLOAT_TEXTURE),
                entry(6, frag, UINT_TEXTURE),
                entry(7, frag, super::UNIFORM),
            ],
        });
        let light_source = format!("{}\n{}", common(), include_str!("../look/light.wgsl"));
        let light_to = |fs: &str, targets: &[(wgpu::TextureFormat, Option<wgpu::BlendState>)]| {
            pipeline(
                device,
                "olr-render light",
                &light_source,
                ("vs_full", fs),
                &[Some(&light_layout)],
                &[],
                targets,
                wgpu::PrimitiveTopology::TriangleList,
            )
        };
        let light = |fs: &str| light_to(fs, &[(LIGHT_FORMAT, None)]);
        let (sum_pipeline, blur_h_pipeline) = (light("fs_sum"), light("fs_blur_h"));
        let occlusion_pipeline = light("fs_occlusion_h");
        let blur_v_pipeline = light_to("fs_blur_v", &[(LIGHT_FORMAT, None), (LIGHT_FORMAT, None)]);
        let (bloom_h_pipeline, bloom_v_pipeline) = (light("fs_bloom_h"), light("fs_bloom_v"));
        let composite_pipeline = pipeline(
            device,
            "olr-render composite",
            &format!("{}\n{}", common(), include_str!("../look/composite.wgsl")),
            ("vs_full", "fs_composite"),
            &[Some(&composite_layout)],
            &[],
            &[(WORLD_FORMAT, None)],
            wgpu::PrimitiveTopology::TriangleList,
        );
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render look globals"),
            size: (GLOBAL_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let occluder_globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render occluder globals"),
            entries: &[entry(0, wgpu::ShaderStages::VERTEX, super::UNIFORM)],
        });
        let occluder_pipeline = pipeline(
            device,
            "olr-render occluders",
            &format!("{}\n{}", common(), include_str!("../look/occluder.wgsl")),
            ("vs_occluder", "fs_occluder"),
            &[Some(&occluder_globals_layout), Some(texture_layout)],
            &[Some(quads())],
            &[(OCCLUDER_FORMAT, None)],
            wgpu::PrimitiveTopology::TriangleStrip,
        );
        LookState {
            terrain_pipeline,
            sprite_pipeline,
            sum_pipeline,
            occlusion_pipeline,
            blur_h_pipeline,
            blur_v_pipeline,
            bloom_h_pipeline,
            bloom_v_pipeline,
            composite_pipeline,
            soak_h_pipeline,
            soak_v_pipeline,
            soak_layout,
            soak_material_h_pipeline,
            soak_material_v_pipeline,
            soak_material_layout,
            occluder_pipeline,
            occluder_globals_layout,
            light_layout,
            composite_layout,
            layout,
            globals,
            gi,
            no_glow,
            no_solid,
            textures: None,
            packed: Vec::with_capacity(GLOBAL_FLOATS),
            packed_emitters: Vec::with_capacity(emitter_floats),
            emitter_count: 0,
            bloom: true,
            halo: true,
            tag: true,
            backing_pipeline,
            distant_pipeline,
            heat,
        }
    }

    /// The look passes' textures for a world target of `texels` (`grid`
    /// world pixels): its light grid and lit target, bound to `albedo` and
    /// `shade` (the world pass's targets) and to the upscale, and its own
    /// globals.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn target(
        &self,
        device: &wgpu::Device,
        grid: (u32, u32),
        texels: (u32, u32),
        albedo: &wgpu::TextureView,
        shade: &wgpu::TextureView,
        (upscale_layout, upscale_uniform, sampler): (&wgpu::BindGroupLayout, &wgpu::Buffer, &wgpu::Sampler),
    ) -> LitTarget {
        let make = |label: &str, size: (u32, u32), format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0.max(1),
                    height: size.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let light = [0, 1, 2].map(|_| make("olr-render light grid", grid, LIGHT_FORMAT));
        let views = light.each_ref().map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        fn tex(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(view) }
        }
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render view look globals"),
            size: (GLOBAL_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = || wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() };
        let solid = self.textures.as_ref().map_or(&self.no_solid, |t| &t.solid_view);
        let groups =
            self.textures.as_ref().map(|t| t.groups.create_view(&wgpu::TextureViewDescriptor::default()));
        let groups = groups.as_ref().unwrap_or(&self.no_solid);
        let rows = make("olr-render occlusion", (grid.0, grid.1 + 2 * OCCLUSION_PAD), LIGHT_FORMAT);
        let occlusion_view = rows.create_view(&wgpu::TextureViewDescriptor::default());
        let light_bind_with = |src: &wgpu::TextureView, rows: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("olr-render light"),
                layout: &self.light_layout,
                entries: &[
                    uniform(),
                    tex(1, src),
                    tex(2, self.textures.as_ref().map_or(&self.no_glow, |t| &t.gi.fluence)),
                    tex(3, solid),
                    tex(4, groups),
                    tex(5, rows),
                ],
            })
        };
        let light_bind = |src: &wgpu::TextureView| light_bind_with(src, &occlusion_view);
        // The sum reads only the occlusion's rows; it is bound to a grid it does not write.
        let binds = [light_bind(&views[1]), light_bind(&views[2]), light_bind(&views[0])];
        let occlusion_bind = light_bind_with(&views[1], &self.no_glow);
        let bloom = [0, 1].map(|_| make("olr-render bloom", grid, LIGHT_FORMAT));
        let bloom_views = bloom.each_ref().map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let bloom_binds = [light_bind(&bloom_views[0]), light_bind(&bloom_views[1])];
        let occluders = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("olr-render occluders"),
            size: wgpu::Extent3d { width: grid.0.max(1), height: grid.1.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OCCLUDER_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let occluder_view = occluders.create_view(&wgpu::TextureViewDescriptor::default());
        let occluder_globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render occluder globals"),
            layout: &self.occluder_globals_layout,
            entries: &[uniform()],
        });
        let worm_boxes = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render worm boxes"),
            size: std::mem::size_of::<WormBoxes>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // The across soak (reads 0) writes 1; the down soak reads 1 and the
        // sum.
        let soak_bind = |src: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("olr-render soak"),
                layout: &self.soak_layout,
                entries: &[uniform(), tex(1, src), tex(2, solid), tex(3, &views[0])],
            })
        };
        let soak_binds = [soak_bind(&views[0]), soak_bind(&views[1])];
        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render composite"),
            layout: &self.composite_layout,
            entries: &[
                uniform(),
                tex(1, albedo),
                tex(2, shade),
                tex(3, &views[1]),
                tex(4, &occluder_view),
                tex(5, &bloom_views[0]),
                tex(6, solid),
                wgpu::BindGroupEntry { binding: 7, resource: worm_boxes.as_entire_binding() },
            ],
        });
        let lit = make("olr-render lit", texels, WORLD_FORMAT);
        let lit_view = lit.create_view(&wgpu::TextureViewDescriptor::default());
        let upscale_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render upscale lit"),
            layout: upscale_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: upscale_uniform.as_entire_binding() },
                tex(1, &lit_view),
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        });
        LitTarget {
            grid,
            globals,
            light,
            views,
            binds,
            occlusion_view,
            occlusion_bind,
            bloom_views,
            bloom_binds,
            soak_binds,
            worm_boxes,
            composite_bind,
            occluder_view,
            occluder_globals_bind,
            lit_view,
            upscale_bind,
        }
    }

    /// New textures for `map` (whose colour is `terrain`), uploaded whole
    /// (zeros where it has no glow), its glow light and, with `surface`
    /// of its size, its heat (issue #39).
    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &super::uploads::Queue,
        map: &ColorMap,
        terrain: &wgpu::Texture,
        max_dim: u32,
        surface: Option<&super::Surface>,
    ) {
        let level = (map.material().width(), map.material().height());
        let sizes = look_texture_sizes(level, map.art_scale(), max_dim);
        let glow = r8(device, "olr-render glow", sizes.art, wgpu::TextureFormat::R8Unorm);
        let groups = r8(device, "olr-render glow groups", sizes.world, wgpu::TextureFormat::R8Uint);
        let solid = r8(device, "olr-render solid", sizes.world, wgpu::TextureFormat::R8Uint);
        let region_map = r8(device, "olr-render flicker regions", sizes.world, wgpu::TextureFormat::R16Uint);
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let terrain_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render look terrain"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view(&glow)),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view(&groups)),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&view(&region_map)),
                },
            ],
        });
        let inputs = super::gi::GiInputs {
            globals: &self.globals,
            glow: &view(&glow),
            groups: &view(&groups),
            solid: &view(&solid),
            terrain: &view(terrain),
            regions: &view(&region_map),
        };
        // Over the level the device keeps (cropped like the terrain): past it
        // is open, as for the soak.
        let gi = super::gi::GiState::new(device, &self.gi, sizes.world, max_dim, &inputs);
        let t = LookTextures {
            glow,
            groups,
            solid_view: view(&solid),
            solid,
            light_cells: Vec::new(),
            art_scale: map.art_scale(),
            level,
            terrain_bind,
            regions: super::regions::Regions::new(level, map.glow_groups()),
            region_map,
            gi,
            heat: None,
            soak: None,
        };
        let mut t = t;
        t.set_surface(device, queue, (&self.heat, &self.soak_material_layout), surface);
        let whole = Rect { x: 0, y: 0, w: map.width(), h: map.height() };
        if map.glow().is_none() {
            let zeros = vec![0u8; map.width() as usize * map.height() as usize];
            write_bytes(queue, &t.glow, (&zeros, map.width()), whole);
            write_bytes(
                queue,
                &t.groups,
                (&zeros[..(level.0 * level.1) as usize], level.0),
                world_rect(whole, t.art_scale),
            );
        }
        self.textures = Some(t);
        self.write(queue, map, whole);
    }

    /// Writes art rectangle `r` of `map`'s glow, and the world rectangle
    /// holding it of its groups and material (a clear solid as background,
    /// so light passes it).
    pub(super) fn write(&mut self, queue: &super::uploads::Queue, map: &ColorMap, r: Rect) {
        let Some(t) = &mut self.textures else { return };
        let wr = world_rect(r, t.art_scale);
        if let (Some(glow), Some(groups)) = (map.glow(), map.glow_groups()) {
            write_bytes(queue, &t.glow, (glow, map.width()), r);
            write_bytes(queue, &t.groups, (groups, t.level.0), wr);
            if let Some((x, y, w, h)) = t.regions.update(groups, (wr.x, wr.y, wr.w, wr.h)) {
                write_u16(queue, &t.region_map, (&t.regions.ids, t.level.0), Rect { x, y, w, h });
            }
        }
        let cells = map.material().cells();
        let Some(clear) = map.clear().filter(|c| c.len() == cells.len()) else {
            write_bytes(queue, &t.solid, (cells, t.level.0), wr);
            return;
        };
        if t.light_cells.len() != cells.len() {
            t.light_cells = cells.to_vec();
        }
        let w = t.level.0 as usize;
        let (x0, x1) = ((wr.x as usize).min(w), (wr.x.saturating_add(wr.w) as usize).min(w));
        let rows = (wr.y as usize)..(wr.y.saturating_add(wr.h) as usize).min(cells.len() / w.max(1));
        for y in rows {
            let i = y * w;
            for ((out, &cell), &flag) in t.light_cells[i + x0..i + x1]
                .iter_mut()
                .zip(&cells[i + x0..i + x1])
                .zip(&clear[i + x0..i + x1])
            {
                *out = if flag != 0 { Material::Background as u8 } else { cell };
            }
        }
        write_bytes(queue, &t.solid, (&t.light_cells, t.level.0), wr);
    }

    /// Packs the first [`olr_look::MAX_EMITTERS`] of `look`'s emitters as
    /// discs (plan 15c): position, radius (0.5-16), reach (`f32::INFINITY`
    /// unlimited) and radiance ([`olr_look::engine::disc_radiance`]). A disc
    /// whose position or reach is not a number, or that gives no light, is
    /// left out.
    pub(super) fn pack_emitters(&mut self, look: &LookFrame) {
        let e = &mut self.packed_emitters;
        e.clear();
        for f in look.emitters.iter().take(olr_look::MAX_EMITTERS) {
            let radius = if f.radius.is_finite() { f.radius.clamp(0.5, 16.0) } else { 1.0 };
            let l = olr_look::engine::disc_radiance(f.colour, radius, f.reach);
            let placed = f.pos.iter().all(|p| p.is_finite() && p.abs() < 1e6) && !f.reach.is_nan();
            if !placed || l.iter().all(|&v| v <= 0.0) {
                continue;
            }
            e.extend([f.pos[0], f.pos[1], radius, f.reach.clamp(0.0, 1e9), l[0], l[1], l[2], 0.0]);
        }
        self.emitter_count = (e.len() / super::gi::EMITTER_FLOATS) as u32;
    }

    /// Labels the flicker regions for the groups `look` flickers, when they
    /// are not the ones labelled, and uploads the whole map (plan 15c).
    pub(super) fn sync_regions(&mut self, queue: &super::uploads::Queue, look: &LookFrame) {
        let Some(t) = &mut self.textures else { return };
        let mut mask = [false; GROUPS];
        for (m, g) in mask.iter_mut().zip(&look.groups) {
            *m = g.flicker.is_some();
        }
        if t.regions.set_mask(&mask) {
            let whole = Rect { x: 0, y: 0, w: t.level.0, h: t.level.1 };
            write_u16(queue, &t.region_map, (&t.regions.ids, t.level.0), whole);
        }
    }

    /// Packs `look` into the globals' floats for a view drawn at `frame`
    /// (the head and the frame's groups only: what is written each frame),
    /// with distant layers when `distant` (issue #11: the composite then
    /// reads their depth from the shade's alpha), its occluder map at its
    /// [`occluder_rect`].
    pub(super) fn pack(&mut self, look: &LookFrame, frame: &WorldFrame, distant: bool) {
        let p = &mut self.packed;
        p.clear();
        let (level, art_scale, cell) =
            self.textures.as_ref().map_or(((0, 0), 1, 1), |t| (t.level, t.art_scale, t.gi.cell));
        let [r, g, b] = look.ambient;
        p.extend([r, g, b, look.max_light]);
        let gr = &look.grade;
        let lin = |c: u8| olr_look::srgb_to_linear(c);
        p.extend([
            2f32.powf(f32::from(gr.exposure) / 100.0),
            f32::from(gr.contrast) / 100.0,
            f32::from(gr.saturation) / 100.0,
            f32::from(look.light_steps),
        ]);
        p.extend([lin(gr.tint[0]), lin(gr.tint[1]), lin(gr.tint[2]), f32::from(look.softness)]);
        // The bounce (plan 15c), clamped so the feedback converges.
        let bounce = if look.bounce.is_finite() { look.bounce.clamp(0.0, 0.9) } else { 0.0 };
        p.extend([frame.origin.0 as f32, frame.origin.1 as f32, frame.scale as f32, bounce]);
        p.extend([level.0 as f32, level.1 as f32, art_scale as f32, cell as f32]);
        let mode = match look.mode {
            ViewMode::Lit => 0.0,
            ViewMode::FullBright => 1.0,
            ViewMode::LightOnly => 2.0,
        };
        p.extend([
            mode,
            look.groups.len().min(GROUPS) as f32,
            f32::from(look.light_soak.min(olr_look::MAX_SOAK)),
            if self.bloom { BLOOM_STRENGTH } else { 0.0 },
        ]);
        // The ambient occlusion (issue #10): its strength (0 none) and reach.
        let (strength, reach) = look.occlusion.filter(|o| o.strength > 0).map_or((0.0, 0.0), |o| {
            let reach = o.reach.clamp(olr_look::MIN_OCCLUSION_REACH, olr_look::MAX_OCCLUSION_REACH);
            (f32::from(o.strength.min(100)) / 100.0, f32::from(reach))
        });
        let [r, g, b] = look.solid_floor;
        p.extend([r, g, b, strength]);
        let [r, g, b] = look.worm_floor;
        p.extend([r, g, b, reach]);
        p.extend(occluder_rect(frame));
        // Whether the level draws distant layers (issue #11); the worm
        // halo's strength scale and the background tag (issue #54).
        let tag = if self.tag { BACKGROUND_TAG } else { 0.0 };
        p.extend([f32::from(u8::from(distant)), f32::from(u8::from(self.halo)), tag, 0.0]);
        debug_assert_eq!(p.len(), GLOBAL_HEAD);
        // Only the frame's groups (issue #35: the shaders read no group
        // past `mode.y`): cast and self-light scale, reach and own colour,
        // then the flicker (plan 15c): speed, depth, clock, seed (24 bits,
        // whole in f32); a speed of 0 is no flicker.
        for g in look.groups.iter().take(GROUPS) {
            p.extend([g.cast[0], g.cast[1], g.cast[2], g.self_scale]);
            p.extend([g.reach.min(1e9), f32::from(u8::from(g.own_colour)), 0.0, 0.0]);
            match g.flicker {
                Some((speed, depth)) => {
                    p.extend([speed.max(1.0), depth, g.t_ms, (g.seed & 0xff_ffff) as f32])
                }
                None => p.extend([0.0; 4]),
            }
        }
        // A NaN never reaches a shader.
        for v in p.iter_mut() {
            if !v.is_finite() {
                *v = 0.0;
            }
        }
    }
}

impl Renderer {
    /// With `on`, the renderer keeps the look's textures (glow, glow groups,
    /// solid map) with the terrain from the next [`Renderer::upload_terrain`]
    /// and can draw a [`LookFrame`] ([`Renderer::render_lit`]); off (the
    /// default), none exist and every frame is drawn as before.
    pub fn set_lighting(&mut self, on: bool) {
        match (on, self.look.is_some()) {
            (true, false) => {
                self.look = Some(LookState::new(
                    &self.device,
                    &self.globals_layout,
                    &self.texture_layout,
                    &self.distant_layout,
                ));
            }
            (false, true) => self.look = None,
            _ => return,
        }
        // The world targets gain or lose their shade targets.
        self.targets.clear();
        // The distant layers' heat is the look's (issue #41).
        self.make_distant_heat();
    }

    /// Forgets the light's history (plan 15c: bounce reads the last frame's
    /// light): the next lit frame starts afresh. `upload_terrain` and a
    /// frame drawn without a look do it too.
    pub fn reset_light_history(&mut self) {
        if let Some(t) = self.look.as_ref().and_then(|l| l.textures.as_ref()) {
            t.gi.reset();
        }
    }

    /// Turns the bloom off or on (tests: a picture with and without it).
    #[doc(hidden)]
    pub fn set_bloom(&mut self, on: bool) {
        if let Some(l) = self.look.as_mut() {
            l.bloom = on;
        }
    }

    /// Turns the worm halo off or on (issue #54; tests: a lit frame with
    /// the halo's strength 0).
    #[doc(hidden)]
    pub fn set_worm_halo(&mut self, on: bool) {
        if let Some(l) = self.look.as_mut() {
            l.halo = on;
        }
    }

    /// Turns the background tag off or on (issue #54; tests: off, the lit
    /// world pass writes part 1's shade bytes).
    #[doc(hidden)]
    pub fn set_background_tag(&mut self, on: bool) {
        if let Some(l) = self.look.as_mut() {
            l.tag = on;
        }
    }

    /// Whether lighting is on ([`Renderer::set_lighting`]).
    pub fn lighting(&self) -> bool {
        self.look.is_some()
    }

    /// The first view's light grid as last drawn (tests): the texture
    /// (RGBA16Float) and where it lies; `None` before a lit frame.
    pub fn light_grid(&self) -> Option<(&wgpu::Texture, crate::LightGrid)> {
        self.view_light_grid(0)
    }

    /// View `view`'s light grid as last drawn ([`Renderer::render_views`]):
    /// as [`Renderer::light_grid`]; `None` for a view never drawn lit.
    pub fn view_light_grid(&self, view: usize) -> Option<(&wgpu::Texture, crate::LightGrid)> {
        let t = self.targets.get(view)?;
        let lit = t.lit.as_ref()?;
        let pad = LIGHT_PAD as i32;
        Some((
            &lit.light[1],
            crate::LightGrid { origin: (t.origin.0 - pad, t.origin.1 - pad), size: lit.grid },
        ))
    }

    /// View `view`'s lit target as last drawn (`Rgba8Unorm`, `COPY_SRC`:
    /// the composite before the upscale; `olr-cli frame-hashes`' byte
    /// identity, issue #34); `None` for a view never drawn lit.
    pub fn view_lit_target(&self, view: usize) -> Option<&wgpu::Texture> {
        Some(self.targets.get(view)?.lit.as_ref()?.lit_view.texture())
    }

    /// How many history steps the glow light has taken (tests, issue #26:
    /// one a lit frame that casts light, however many views it draws).
    #[doc(hidden)]
    pub fn glow_light_steps(&self) -> u64 {
        self.glow_light_steps
    }

    /// The glow texture (tests); `None` without lighting or a terrain.
    pub fn glow_texture(&self) -> Option<&wgpu::Texture> {
        self.look.as_ref()?.textures.as_ref().map(|t| &t.glow)
    }

    /// The glow groups texture (tests).
    pub fn groups_texture(&self) -> Option<&wgpu::Texture> {
        self.look.as_ref()?.textures.as_ref().map(|t| &t.groups)
    }

    /// The solid map, the colour map's material cells (tests).
    pub fn solid_texture(&self) -> Option<&wgpu::Texture> {
        self.look.as_ref()?.textures.as_ref().map(|t| &t.solid)
    }

    /// Whether the level soaks by material (issue #57): lighting on, the
    /// terrain up, surface maps of its size and a material setting its own
    /// `light_soak` ([`Renderer::set_surface`]).
    pub fn has_material_soak(&self) -> bool {
        self.look.as_ref().and_then(|l| l.textures.as_ref()).is_some_and(|t| t.soak.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Instance;

    fn quad(dst: [f32; 4]) -> Instance {
        Instance { dst, src: [0.0; 4], tint: [1.0; 4], shade: [1.0, 0.0, 0.0] }
    }

    /// Issue #35: each box holds its quad's texels with a texel to spare;
    /// past the last box the quads are joined into it.
    #[test]
    fn worm_boxes_hold_every_quad() {
        let mut out = [[0; 4]; 1 + WORM_BOXES];
        worm_boxes(&[quad([10.5, 20.0, 7.0, 7.25])], &mut out);
        assert_eq!(out[0][0], 1);
        assert_eq!(out[1], [9, 19, 19, 29]);
        worm_boxes(&[], &mut out);
        assert_eq!(out[0][0], 0, "no worm, no box");
        let many: Vec<Instance> =
            (0..WORM_BOXES + 3).map(|i| quad([i as f32 * 10.0, 0.0, 5.0, 5.0])).collect();
        worm_boxes(&many, &mut out);
        assert_eq!(out[0][0], WORM_BOXES as i32);
        let last = (WORM_BOXES + 2) as i32 * 10;
        assert_eq!(out[WORM_BOXES], [(WORM_BOXES as i32 - 1) * 10 - 1, -1, last + 6, 6], "the rest joined");
    }
}
