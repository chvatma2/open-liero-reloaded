//! The wgpu renderer: terrain and atlas textures, the world pass at art
//! scale and the upscale pass (spec §8.2).

use rl_scene::{AtlasImage, DrawList, SceneAssets};
use rl_terrain::ColorMap;
use wgpu::util::DeviceExt as _;

use crate::atlas::{self, AtlasLayout};
use crate::frame::Instance;
use crate::upscale::UpscaleFilter;

mod pass;

/// The world pass's and atlas's texel format (gamma-space RGBA, like OLX's
/// surfaces: blending happens on the stored bytes).
pub const WORLD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The art scale the world pass draws at, at least: OLX's sprites are 2×.
pub const MIN_SCALE: u32 = 2;

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
}

/// The world-pass target and the upscale pass's view of it.
struct Target {
    frame_size: (u32, u32),
    /// The texture's size: `frame_size`, unless the device's limit cut it.
    size: (u32, u32),
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
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
    upscale_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    upscale_layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    upscale_uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    atlas: Option<(AtlasLayout, wgpu::BindGroup)>,
    terrain: Option<Terrain>,
    target: Option<Target>,
    instances: Vec<Instance>,
    raw: Vec<[f32; 12]>,
    instance_buffer: wgpu::Buffer,
    dirty: Vec<rl_terrain::Rect>,
    warnings: Vec<String>,
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
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
    topology: wgpu::PrimitiveTopology,
) -> wgpu::RenderPipeline {
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
            targets: &[Some(wgpu::ColorTargetState { format, blend, write_mask: wgpu::ColorWrites::ALL })],
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
            label: Some("rl-render texture"),
            entries: &[entry(0, frag, TEXTURE)],
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rl-render globals"),
            entries: &[entry(0, wgpu::ShaderStages::VERTEX, UNIFORM)],
        });
        let upscale_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rl-render upscale"),
            entries: &[
                entry(0, frag, UNIFORM),
                entry(1, frag, TEXTURE),
                entry(2, frag, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];
        let quads = wgpu::VertexBufferLayout {
            array_stride: 48,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &attributes,
        };
        let world_pipeline = pipeline(
            &device,
            "rl-render world",
            include_str!("world.wgsl"),
            ("vs_world", "fs_world"),
            &[Some(&globals_layout), Some(&texture_layout)],
            &[Some(quads)],
            WORLD_FORMAT,
            Some(wgpu::BlendState::ALPHA_BLENDING),
            wgpu::PrimitiveTopology::TriangleStrip,
        );
        let upscale_pipeline = pipeline(
            &device,
            "rl-render upscale",
            include_str!("upscale.wgsl"),
            ("vs_full", "fs_upscale"),
            &[Some(&upscale_layout)],
            &[],
            surface_format,
            None,
            wgpu::PrimitiveTopology::TriangleList,
        );
        let uniform = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let globals = uniform("rl-render globals", 16);
        let upscale_uniform = uniform("rl-render upscale", 48);
        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rl-render globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("rl-render linear"),
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
            upscale_pipeline,
            texture_layout,
            upscale_layout,
            globals,
            globals_bind,
            upscale_uniform,
            sampler,
            atlas: None,
            terrain: None,
            target: None,
            instances: Vec::with_capacity(4 * DrawList::RESERVED),
            raw: Vec::with_capacity(4 * DrawList::RESERVED),
            instance_buffer,
            dirty: Vec::with_capacity(ColorMap::MAX_DIRTY + 1),
            warnings: Vec::new(),
        }
    }

    fn instance_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rl-render instances"),
            size: (count * 48) as u64,
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

    /// The world pass's art scale: [`MIN_SCALE`], or the terrain's when larger.
    pub fn scale(&self) -> u32 {
        self.terrain.as_ref().map_or(MIN_SCALE, |t| t.art_scale.max(MIN_SCALE))
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
    /// whatever `rl-fx` registered. Register everything first: an image
    /// added later draws as a magenta square until this runs again.
    pub fn upload_atlas(&mut self, assets: &SceneAssets) {
        self.upload_images(assets.images());
    }

    /// [`Renderer::upload_atlas`] of a plain image list, indexed by
    /// [`rl_scene::AtlasId`] like [`SceneAssets::images`].
    pub fn upload_images(&mut self, images: &[AtlasImage]) {
        let packed = atlas::pack(images, self.max_dim());
        self.warnings.extend(packed.warnings);
        let img = &packed.pixels;
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("rl-render atlas"),
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
        let bind = self.bind_texture("rl-render atlas", &texture);
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
        let texture = texture(&self.device, "rl-render terrain", size, usage);
        let bind = self.bind_texture("rl-render terrain", &texture);
        self.terrain =
            Some(Terrain { texture, bind, art: (map.width(), map.height()), art_scale: map.art_scale() });
        self.write_terrain(map, rl_terrain::Rect { x: 0, y: 0, w: map.width(), h: map.height() });
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
        }
    }

    fn write_terrain(&self, map: &ColorMap, r: rl_terrain::Rect) {
        Self::write_rect(&self.queue, self.terrain.as_ref(), (map.pixels(), map.width()), r);
    }

    /// Writes rectangle `r` of a `width`-wide RGBA image into the terrain
    /// texture, clipped to it.
    fn write_rect(
        queue: &wgpu::Queue,
        terrain: Option<&Terrain>,
        (pixels, width): (&[u8], u32),
        r: rl_terrain::Rect,
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
