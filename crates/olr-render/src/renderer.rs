//! The wgpu renderer: terrain and atlas textures, the world pass at art
//! scale and the upscale pass (spec §8.2).

use olr_scene::{AtlasImage, DrawList, SceneAssets};
use olr_terrain::ColorMap;
use wgpu::util::DeviceExt as _;

use crate::atlas::{self, AtlasLayout};
use crate::frame::Instance;
use crate::upscale::UpscaleFilter;

mod gi;
mod look;
mod pass;
mod probe;
mod regions;
mod timing;

pub use gi::{GI_CELL_BUDGET, gi_cell};
pub use look::WORM_SHADOW;
pub use pass::ViewDraw;
pub use probe::{MAX_PROBE_PASSES, MAX_PROBE_TAPS, Probe};
pub use timing::{GpuTimes, PASS_GROUPS, PassGroup, PassSpan, gpu_times};

/// The world pass's and atlas's texel format (gamma-space RGBA, like OLX's
/// surfaces: blending happens on the stored bytes).
pub const WORLD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The art scale the world pass draws at, at least: OLX's sprites are 2×.
pub const MIN_SCALE: u32 = 2;

/// The highest atlas image scale the world pass follows (a skin's largest
/// art scale, 8): an image claiming more draws at it, downscaled.
pub const MAX_SPRITE_SCALE: u32 = 8;

/// An instance's bytes in the vertex buffer.
const INSTANCE_STRIDE: u64 = (crate::frame::INSTANCE_FLOATS * 4) as u64;

/// How the renderer draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderSettings {
    pub filter: UpscaleFilter,
}

/// A colour-terrain texture and what it covers.
struct Terrain {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    /// The colour map's size in art pixels (the texture may be cropped).
    art: (u32, u32),
    art_scale: u32,
    /// The map keeps see-through pixels (issue #11): drawn over an opaque
    /// copy of itself, so they show the back tile where no distant layer
    /// covers them.
    see_through: bool,
}

/// A distant layer's texture (issue #11) and where it lies.
struct Distant {
    bind: wgpu::BindGroup,
    /// Its size in art pixels.
    art: (u32, u32),
    layer: olr_data::level::DistantLayer,
}

/// One view's world-pass target and the upscale pass's view of it (issue
/// #26: one a view, each with uniforms of its own, so every view of a
/// frame is encoded before the one submit).
struct Target {
    frame_size: (u32, u32),
    /// The texture's size: `frame_size`, unless the device's limit cut it.
    size: (u32, u32),
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    /// The world pass's globals (the target's size) and their bind.
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    /// The upscale pass's uniform, which `bind` and the lit target's read.
    upscale_uniform: wgpu::Buffer,
    /// With lighting: the second target, each texel's shade (self-light,
    /// marker), drawn with the albedo.
    shade: Option<wgpu::TextureView>,
    /// With lighting: the light grid and the lit target.
    lit: Option<look::LitTarget>,
    /// The world pixel of the target's texel (0, 0), as last drawn.
    origin: (i32, i32),
}

/// Draws [`DrawList`]s with one device: the terrain (a full upload, then
/// dirty rectangles), every sprite from one atlas, line pixels, then an
/// upscale to the output with the selected [`UpscaleFilter`].
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    srgb: bool,
    pub settings: RenderSettings,
    world_pipeline: wgpu::RenderPipeline,
    /// The terrain opaque under a level's distant layers, and the layers
    /// (issue #11).
    backing_pipeline: wgpu::RenderPipeline,
    distant_pipeline: wgpu::RenderPipeline,
    upscale_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    globals_layout: wgpu::BindGroupLayout,
    upscale_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    atlas: Option<(AtlasLayout, wgpu::BindGroup)>,
    /// The highest `px_per_world` of the atlas's images (issue #12: a
    /// double-resolution skin's 4), at most [`MAX_SPRITE_SCALE`].
    sprite_scale: u32,
    terrain: Option<Terrain>,
    /// The level's distant layers, far to near ([`Renderer::set_distant`]).
    distant: Vec<Distant>,
    /// This frame's distant quads: the layer and its instance.
    distant_draws: Vec<(usize, u32)>,
    /// Each view's world target (issue #26), the first view's first.
    targets: Vec<Target>,
    /// This frame's views: their frames and instance ranges.
    plans: Vec<pass::ViewPlan>,
    instances: Vec<Instance>,
    raw: Vec<[f32; crate::frame::INSTANCE_FLOATS]>,
    instance_buffer: wgpu::Buffer,
    dirty: Vec<olr_terrain::Rect>,
    warnings: Vec<String>,
    /// GPU timestamps of every frame (bench only: [`Renderer::enable_gpu_timing`]).
    timer: Option<timing::Timer>,
    /// The shader-headroom probe (bench only: [`Renderer::set_probe`]).
    probe: Option<probe::ProbeState>,
    /// The look's state, with lighting on ([`Renderer::set_lighting`]).
    look: Option<look::LookState>,
    /// The glow light's history steps so far (tests: one a lit frame,
    /// however many views it draws).
    glow_light_steps: u64,
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    size: (u32, u32),
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: WORLD_FORMAT,
        usage,
        view_formats: &[],
    })
}

fn entry(binding: u32, visibility: wgpu::ShaderStages, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility, ty, count: None }
}

const TEXTURE: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Float { filterable: true },
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};

const UNIFORM: wgpu::BindingType = wgpu::BindingType::Buffer {
    ty: wgpu::BufferBindingType::Uniform,
    has_dynamic_offset: false,
    min_binding_size: None,
};

#[allow(clippy::too_many_arguments)]
fn pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    (vs, fs): (&str, &str),
    layouts: &[Option<&wgpu::BindGroupLayout>],
    buffers: &[Option<wgpu::VertexBufferLayout>],
    targets: &[(wgpu::TextureFormat, Option<wgpu::BlendState>)],
    topology: wgpu::PrimitiveTopology,
) -> wgpu::RenderPipeline {
    let targets: Vec<Option<wgpu::ColorTargetState>> = targets
        .iter()
        .map(|&(format, blend)| {
            Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })
        })
        .collect();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(vs),
            compilation_options: Default::default(),
            buffers,
        },
        primitive: wgpu::PrimitiveState { topology, ..Default::default() },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The upscale pass's uniform: source and destination rectangles, the
/// world target's size, the filter and whether to convert to linear.
fn upscale_uniform(
    src: [f32; 4],
    dst: [f32; 4],
    tex: (u32, u32),
    filter: UpscaleFilter,
    srgb: bool,
) -> [u32; 12] {
    let f = |v: f32| v.to_bits();
    [
        f(src[0]),
        f(src[1]),
        f(src[2].max(1e-3)),
        f(src[3].max(1e-3)),
        f(dst[0]),
        f(dst[1]),
        f(dst[2].max(1e-3)),
        f(dst[3].max(1e-3)),
        f(tex.0 as f32),
        f(tex.1 as f32),
        filter.code(),
        u32::from(srgb),
    ]
}

impl Renderer {
    /// A renderer drawing into targets of `surface_format` (an sRGB
    /// format gets linear values, so it shows the same colours).
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        settings: RenderSettings,
    ) -> Renderer {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render texture"),
            entries: &[entry(0, frag, TEXTURE)],
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render globals"),
            entries: &[entry(0, wgpu::ShaderStages::VERTEX, UNIFORM)],
        });
        let upscale_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render upscale"),
            entries: &[
                entry(0, frag, UNIFORM),
                entry(1, frag, TEXTURE),
                entry(2, frag, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let attributes =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x3];
        let quads = wgpu::VertexBufferLayout {
            array_stride: INSTANCE_STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &attributes,
        };
        let world = |label, fs, blend| {
            pipeline(
                &device,
                label,
                include_str!("world.wgsl"),
                ("vs_world", fs),
                &[Some(&globals_layout), Some(&texture_layout)],
                &[Some(quads.clone())],
                &[(WORLD_FORMAT, blend)],
                wgpu::PrimitiveTopology::TriangleStrip,
            )
        };
        let world_pipeline = world("olr-render world", "fs_world", Some(wgpu::BlendState::ALPHA_BLENDING));
        let backing_pipeline = world("olr-render terrain backing", "fs_terrain_backing", None);
        let distant_pipeline = world("olr-render distant", "fs_distant", None);
        let upscale_pipeline = pipeline(
            &device,
            "olr-render upscale",
            include_str!("upscale.wgsl"),
            ("vs_full", "fs_upscale"),
            &[Some(&upscale_layout)],
            &[],
            &[(surface_format, None)],
            wgpu::PrimitiveTopology::TriangleList,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("olr-render linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let instance_buffer = Self::instance_buffer(&device, 1024);
        Renderer {
            device,
            queue,
            srgb: surface_format.is_srgb(),
            settings,
            world_pipeline,
            backing_pipeline,
            distant_pipeline,
            upscale_pipeline,
            texture_layout,
            globals_layout,
            upscale_layout,
            sampler,
            atlas: None,
            sprite_scale: 1,
            terrain: None,
            distant: Vec::new(),
            distant_draws: Vec::new(),
            targets: Vec::new(),
            plans: Vec::new(),
            instances: Vec::with_capacity(4 * DrawList::RESERVED),
            raw: Vec::with_capacity(4 * DrawList::RESERVED),
            instance_buffer,
            dirty: Vec::with_capacity(ColorMap::MAX_DIRTY + 1),
            warnings: Vec::new(),
            timer: None,
            probe: None,
            look: None,
            glow_light_steps: 0,
        }
    }

    /// A uniform buffer of `size` bytes, written each frame.
    fn uniform(&self, label: &str, size: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn instance_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render instances"),
            size: count as u64 * INSTANCE_STRIDE,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// What went wrong with content so far (an image or a map too large
    /// for the device); the caller logs each once and may clear them.
    pub fn warnings(&mut self) -> &mut Vec<String> {
        &mut self.warnings
    }

    /// The world pass's art scale: [`MIN_SCALE`], or the terrain's or the
    /// atlas's highest image scale (issue #12: a double-resolution skin
    /// draws at full resolution) when larger.
    pub fn scale(&self) -> u32 {
        self.level_scale().max(self.sprite_scale)
    }

    /// The world pass's scale without the sprites': [`MIN_SCALE`], or the
    /// terrain's art scale when larger.
    fn level_scale(&self) -> u32 {
        self.terrain.as_ref().map_or(MIN_SCALE, |t| t.art_scale).max(MIN_SCALE)
    }

    /// Where a view of `src` target texels (the view's size × [`Renderer::scale`])
    /// lands in `area` of the output, as [`crate::dest_rect_in`] with the
    /// settings' filter; `IntegerNearest`'s whole multiples are of the
    /// level's scale (issue #12: a double-resolution skin drawn finer does
    /// not change the view's size on screen).
    pub fn dest_rect_in(&self, src: (f32, f32), area: [u32; 4]) -> [f32; 4] {
        let unit = self.scale() as f32 / self.level_scale() as f32;
        crate::upscale::dest_rect_in_units(self.settings.filter, src, area, unit)
    }

    /// The atlas layout of the last [`Renderer::upload_atlas`].
    pub fn atlas_layout(&self) -> Option<&AtlasLayout> {
        self.atlas.as_ref().map(|(l, _)| l)
    }

    /// The world pass's instance buffer, in bytes (it only grows, to the
    /// largest frame's next power of two; the soak test watches it).
    pub fn instance_buffer_bytes(&self) -> u64 {
        self.instance_buffer.size()
    }

    /// Bench only (plan 8): time every frame on the GPU from now on, when
    /// the device has `Features::TIMESTAMP_QUERY` (an `Offscreen` opened
    /// with `timestamps`). Returns whether it does.
    pub fn enable_gpu_timing(&mut self) -> bool {
        if !self.device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return false;
        }
        if self.timer.is_none() {
            self.timer = Some(timing::Timer::new(&self.device, &self.queue));
        }
        true
    }

    /// The last frame's GPU times (waits for it); `None` without timing,
    /// or when no frame was drawn since the last call.
    pub fn gpu_times(&mut self) -> Option<GpuTimes> {
        let device = self.device.clone();
        self.timer.as_mut()?.read(&device)
    }

    /// Bench only (the shader-headroom probe, plan 8): `Some` runs the
    /// probe's passes after every frame, into textures of its own (the
    /// picture is unchanged); `None`, the default, runs none.
    pub fn set_probe(&mut self, probe: Option<Probe>) {
        self.probe = probe.map(|p| probe::ProbeState::new(&self.device, p.clamped()));
    }

    /// Bench only: the probe's last output (for tests that read it back);
    /// `None` without a probe or before its first frame.
    pub fn probe_output(&self) -> Option<&wgpu::Texture> {
        self.probe.as_ref()?.output()
    }

    /// The terrain texture (for tests and tools that read it back).
    pub fn terrain_texture(&self) -> Option<&wgpu::Texture> {
        self.terrain.as_ref().map(|t| &t.texture)
    }

    fn max_dim(&self) -> u32 {
        self.device.limits().max_texture_dimension_2d
    }

    fn bind_texture(&self, label: &str, texture: &wgpu::Texture) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.texture_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        })
    }

    /// Packs every image of `assets` into one atlas at art scale and
    /// uploads it: core sprites, projectile images, colorized skins and
    /// whatever `olr-fx` registered. Register everything first: an image
    /// added later draws as a magenta square until this runs again.
    pub fn upload_atlas(&mut self, assets: &SceneAssets) {
        self.upload_images(assets.images());
    }

    /// [`Renderer::upload_atlas`] of a plain image list, indexed by
    /// [`olr_scene::AtlasId`] like [`SceneAssets::images`].
    pub fn upload_images(&mut self, images: &[AtlasImage]) {
        self.sprite_scale =
            images.iter().map(|i| i.px_per_world).max().unwrap_or(1).clamp(1, MAX_SPRITE_SCALE);
        let packed = atlas::pack(images, self.max_dim());
        self.warnings.extend(packed.warnings);
        let img = &packed.pixels;
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("olr-render atlas"),
                size: wgpu::Extent3d { width: img.width(), height: img.height(), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: WORLD_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            img.as_raw(),
        );
        let bind = self.bind_texture("olr-render atlas", &texture);
        self.atlas = Some((packed.layout, bind));
    }

    /// Uploads the whole colour map (a new or restarted level). A map
    /// larger than the device allows is cropped, with a warning.
    pub fn upload_terrain(&mut self, map: &ColorMap) {
        let max = self.max_dim();
        let size = (map.width().clamp(1, max), map.height().clamp(1, max));
        if size != (map.width(), map.height()) {
            self.warnings.push(format!(
                "terrain: the {}x{} colour map is larger than the device's {max}; cropped",
                map.width(),
                map.height()
            ));
        }
        let usage = wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC;
        let texture = texture(&self.device, "olr-render terrain", size, usage);
        let bind = self.bind_texture("olr-render terrain", &texture);
        self.terrain = Some(Terrain {
            texture,
            bind,
            art: (map.width(), map.height()),
            art_scale: map.art_scale(),
            see_through: map.see_through(),
        });
        self.write_terrain(map, olr_terrain::Rect { x: 0, y: 0, w: map.width(), h: map.height() });
        if let (Some(look), Some(t)) = (self.look.as_mut(), &self.terrain) {
            look.upload(&self.device, &self.queue, map, &t.texture, max);
            // The world targets' light passes bind the new level's glow light.
            self.targets.clear();
        }
    }

    /// Sets the level's distant (parallax) layers, far to near (issue #11;
    /// none: a level drawn as always). They show where the terrain is
    /// see-through (`ColorMap::new_see_through`), each scrolling by `1 -
    /// depth` of the camera's movement ([`DrawList::camera`]), offset by
    /// whole texels and clipped to the level. A layer larger than the
    /// device allows is left out, with a warning.
    pub fn set_distant(&mut self, layers: &[olr_data::level::DistantLayer]) {
        let max = self.max_dim();
        self.distant.clear();
        for (i, l) in layers.iter().enumerate() {
            let art = (l.image.width(), l.image.height());
            if art.0 == 0 || art.1 == 0 || art.0 > max || art.1 > max {
                self.warnings.push(format!(
                    "distant layer {i}: {}x{} is empty or larger than the device's {max}; left out",
                    art.0, art.1
                ));
                continue;
            }
            let texture = self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("olr-render distant"),
                    size: wgpu::Extent3d { width: art.0, height: art.1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: WORLD_FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                l.image.as_raw(),
            );
            let bind = self.bind_texture("olr-render distant", &texture);
            self.distant.push(Distant { bind, art, layer: l.clone() });
        }
    }

    /// Uploads the rectangles `map` changed since the last call
    /// ([`ColorMap::take_dirty`]); a map of another size is uploaded whole.
    pub fn update_terrain(&mut self, map: &mut ColorMap) {
        let same = self.terrain.as_ref().is_some_and(|t| t.art == (map.width(), map.height()));
        if !same {
            map.take_dirty().for_each(drop);
            self.upload_terrain(map);
            return;
        }
        // Each rectangle is written straight from the map's pixels: no copy.
        self.dirty.clear();
        self.dirty.extend(map.take_dirty());
        for r in &self.dirty {
            Self::write_rect(&self.queue, self.terrain.as_ref(), (map.pixels(), map.width()), *r);
            if let Some(look) = &mut self.look {
                look.write(&self.queue, map, *r);
            }
        }
    }

    fn write_terrain(&self, map: &ColorMap, r: olr_terrain::Rect) {
        Self::write_rect(&self.queue, self.terrain.as_ref(), (map.pixels(), map.width()), r);
    }

    /// Writes rectangle `r` of a `width`-wide RGBA image into the terrain
    /// texture, clipped to it.
    fn write_rect(
        queue: &wgpu::Queue,
        terrain: Option<&Terrain>,
        (pixels, width): (&[u8], u32),
        r: olr_terrain::Rect,
    ) {
        let Some(t) = terrain else { return };
        let (tw, th) = (t.texture.width(), t.texture.height());
        let (w, h) = (r.w.min(tw.saturating_sub(r.x)), r.h.min(th.saturating_sub(r.y)));
        if w == 0 || h == 0 {
            return;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &t.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: r.x, y: r.y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: (u64::from(r.y) * u64::from(width) + u64::from(r.x)) * 4,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
    }
}
