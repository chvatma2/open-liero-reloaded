//! The look's GPU state (plan 15a): its textures (glow, glow groups, solid
//! map), its per-frame globals and the lit world pass's pipelines. Made by
//! [`Renderer::set_lighting`]; without it nothing here exists and every
//! frame is drawn as before.

use olr_look::{LookFrame, ViewMode};
use olr_terrain::{ColorMap, Rect};

use super::{Renderer, WORLD_FORMAT, entry, pipeline};
use crate::frame::{WorldFrame, look_texture_sizes};

/// The shared WGSL of every look pass.
pub(super) const COMMON: &str = include_str!("../look/common.wgsl");

/// Groups in the globals (a group map value is one byte, 0 none).
pub(super) const GROUPS: usize = 255;

/// The globals' floats: seven vec4s, then two arrays of [`GROUPS`] vec4s.
pub(super) const GLOBAL_FLOATS: usize = 4 * (7 + 2 * GROUPS);

/// The format of the world pass's second target: self-light, marker, 0, alpha.
pub(super) const SHADE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The look's textures for one colour map.
pub(super) struct LookTextures {
    pub(super) glow: wgpu::Texture,
    pub(super) groups: wgpu::Texture,
    pub(super) solid: wgpu::Texture,
    pub(super) solid_view: wgpu::TextureView,
    pub(super) art_scale: u32,
    /// The level's size in world pixels (uncropped).
    pub(super) level: (u32, u32),
    /// The lit terrain's group 2: globals, glow, groups.
    pub(super) terrain_bind: wgpu::BindGroup,
    /// The shadow pass's group 0: globals, lights, solid map.
    pub(super) shadow_bind: wgpu::BindGroup,
    /// The level's glow light.
    pub(super) gi: super::gi::GiState,
}

/// The light grid's format.
pub(super) const LIGHT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// World pixels the light grid reaches past the world target on every side
/// (`LIGHT_PAD` in the shaders): the soak (up to [`olr_look::MAX_SOAK`]) and
/// a blur of up to 4 never see its edge, so the light never depends on the
/// camera.
pub(super) const LIGHT_PAD: u32 = olr_look::MAX_SOAK as u32 + 4;

/// Rays per placed light's shadow map (`SHADOW_RAYS` in the shaders).
pub(super) const SHADOW_RAYS: u32 = 512;

/// The placed lights' uniform: two vec4s per light.
const LIGHT_FLOATS: usize = 8 * olr_look::MAX_LIGHTS;

/// One world target's look passes: the light grid (gathered into 0, soaked
/// into solids across into 1 and down into 2, blurred across into 0 and
/// down, capped and banded, into 1) and the lit target the upscale reads.
pub(super) struct LitTarget {
    /// World pixels: the light grid's size.
    pub(super) grid: (u32, u32),
    pub(super) light: [wgpu::Texture; 3],
    pub(super) views: [wgpu::TextureView; 3],
    /// The sum pass's, the across blur's (reads 2) and the down blur's (reads 0).
    pub(super) binds: [wgpu::BindGroup; 3],
    /// The across soak's (reads 0) and the down soak's (reads 1).
    pub(super) soak_binds: [wgpu::BindGroup; 2],
    pub(super) composite_bind: wgpu::BindGroup,
    pub(super) lit_view: wgpu::TextureView,
    /// The upscale pass's view of the lit target.
    pub(super) upscale_bind: wgpu::BindGroup,
}

pub(super) struct LookState {
    pub(super) terrain_pipeline: wgpu::RenderPipeline,
    pub(super) sprite_pipeline: wgpu::RenderPipeline,
    pub(super) sum_pipeline: wgpu::RenderPipeline,
    pub(super) blur_h_pipeline: wgpu::RenderPipeline,
    pub(super) blur_v_pipeline: wgpu::RenderPipeline,
    pub(super) composite_pipeline: wgpu::RenderPipeline,
    pub(super) soak_h_pipeline: wgpu::RenderPipeline,
    pub(super) soak_v_pipeline: wgpu::RenderPipeline,
    pub(super) soak_layout: wgpu::BindGroupLayout,
    pub(super) shadow_pipeline: wgpu::RenderPipeline,
    pub(super) shadow_layout: wgpu::BindGroupLayout,
    pub(super) light_layout: wgpu::BindGroupLayout,
    pub(super) composite_layout: wgpu::BindGroupLayout,
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) globals: wgpu::Buffer,
    pub(super) lights: wgpu::Buffer,
    pub(super) shadow_view: wgpu::TextureView,
    pub(super) gi: super::gi::GiPipelines,
    /// A 1 × 1 dark glow light, bound before a level's exists.
    no_glow: wgpu::TextureView,
    /// A 1 × 1 solid map, bound before a level's exists (the soak reads
    /// nothing outside the level, whose size is then 0).
    no_solid: wgpu::TextureView,
    pub(super) textures: Option<LookTextures>,
    /// The packed globals, reused every frame.
    pub(super) packed: Vec<f32>,
    /// The packed lights, reused every frame.
    pub(super) packed_lights: Vec<f32>,
    /// The placed lights this frame draws.
    pub(super) light_count: u32,
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
    queue: &wgpu::Queue,
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

/// Art rectangle `r` as the world rectangle holding it at `s`.
fn world_rect(r: Rect, s: u32) -> Rect {
    let s = s.max(1);
    let (x0, y0) = (r.x / s, r.y / s);
    let (x1, y1) = (r.x.saturating_add(r.w).div_ceil(s), r.y.saturating_add(r.h).div_ceil(s));
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

impl LookState {
    pub(super) fn new(
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> LookState {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render look"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, FLOAT_TEXTURE),
                entry(2, frag, UINT_TEXTURE),
            ],
        });
        let source = format!("{COMMON}\n{}", include_str!("../look/world_lit.wgsl"));
        let attributes =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x2];
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
            &[Some(globals_layout), Some(texture_layout), Some(&layout)],
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
        let light_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render light"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, FLOAT_TEXTURE),
                entry(2, frag, super::UNIFORM),
                entry(3, frag, FLOAT_TEXTURE),
                entry(4, frag, FLOAT_TEXTURE),
            ],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render shadow"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, super::UNIFORM),
                entry(2, frag, UINT_TEXTURE),
            ],
        });
        let shadow_pipeline = pipeline(
            device,
            "olr-render shadow",
            &format!("{COMMON}\n{}", include_str!("../look/shadow.wgsl")),
            ("vs_full", "fs_shadow"),
            &[Some(&shadow_layout)],
            &[],
            &[(wgpu::TextureFormat::R32Float, None)],
            wgpu::PrimitiveTopology::TriangleList,
        );
        let shadow = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("olr-render shadow maps"),
            size: wgpu::Extent3d {
                width: SHADOW_RAYS,
                height: olr_look::MAX_LIGHTS as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow.create_view(&wgpu::TextureViewDescriptor::default());
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
            ],
        });
        let soak_source = format!("{COMMON}\n{}", include_str!("../look/soak.wgsl"));
        let soak = |fs: &str| {
            pipeline(
                device,
                "olr-render soak",
                &soak_source,
                ("vs_full", fs),
                &[Some(&soak_layout)],
                &[],
                &[(LIGHT_FORMAT, None)],
                wgpu::PrimitiveTopology::TriangleList,
            )
        };
        let (soak_h_pipeline, soak_v_pipeline) = (soak("fs_soak_h"), soak("fs_soak_v"));
        let gi = super::gi::GiPipelines::new(device);
        let lights = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render lights"),
            size: (LIGHT_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render composite"),
            entries: &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, FLOAT_TEXTURE),
                entry(2, frag, FLOAT_TEXTURE),
                entry(3, frag, FLOAT_TEXTURE),
            ],
        });
        let light_source = format!("{COMMON}\n{}", include_str!("../look/light.wgsl"));
        let light = |fs: &str| {
            pipeline(
                device,
                "olr-render light",
                &light_source,
                ("vs_full", fs),
                &[Some(&light_layout)],
                &[],
                &[(LIGHT_FORMAT, None)],
                wgpu::PrimitiveTopology::TriangleList,
            )
        };
        let (sum_pipeline, blur_h_pipeline, blur_v_pipeline) =
            (light("fs_sum"), light("fs_blur_h"), light("fs_blur_v"));
        let composite_pipeline = pipeline(
            device,
            "olr-render composite",
            &format!("{COMMON}\n{}", include_str!("../look/composite.wgsl")),
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
        LookState {
            terrain_pipeline,
            sprite_pipeline,
            sum_pipeline,
            blur_h_pipeline,
            blur_v_pipeline,
            composite_pipeline,
            soak_h_pipeline,
            soak_v_pipeline,
            soak_layout,
            shadow_pipeline,
            shadow_layout,
            light_layout,
            composite_layout,
            layout,
            globals,
            lights,
            shadow_view,
            gi,
            no_glow,
            no_solid,
            textures: None,
            packed: Vec::with_capacity(GLOBAL_FLOATS),
            packed_lights: Vec::with_capacity(LIGHT_FLOATS),
            light_count: 0,
        }
    }

    /// The look passes' textures for a world target of `texels` (`grid`
    /// world pixels): its light grid and lit target, bound to `albedo` and
    /// `shade` (the world pass's targets) and to the upscale.
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
        let uniform = || wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() };
        let light_bind = |src: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("olr-render light"),
                layout: &self.light_layout,
                entries: &[
                    uniform(),
                    tex(1, src),
                    wgpu::BindGroupEntry { binding: 2, resource: self.lights.as_entire_binding() },
                    tex(3, &self.shadow_view),
                    tex(4, self.textures.as_ref().map_or(&self.no_glow, |t| &t.gi.fluence)),
                ],
            })
        };
        // The sum reads nothing; it is bound to a grid it does not write.
        let binds = [light_bind(&views[1]), light_bind(&views[2]), light_bind(&views[0])];
        let solid = self.textures.as_ref().map_or(&self.no_solid, |t| &t.solid_view);
        let soak_bind = |src: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("olr-render soak"),
                layout: &self.soak_layout,
                entries: &[uniform(), tex(1, src), tex(2, solid)],
            })
        };
        let soak_binds = [soak_bind(&views[0]), soak_bind(&views[1])];
        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render composite"),
            layout: &self.composite_layout,
            entries: &[uniform(), tex(1, albedo), tex(2, shade), tex(3, &views[1])],
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
        LitTarget { grid, light, views, binds, soak_binds, composite_bind, lit_view, upscale_bind }
    }

    /// New textures for `map` (whose colour is `terrain`), uploaded whole
    /// (zeros where it has no glow), and its glow light.
    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        map: &ColorMap,
        terrain: &wgpu::Texture,
        max_dim: u32,
    ) {
        let level = (map.material().width(), map.material().height());
        let sizes = look_texture_sizes(level, map.art_scale(), max_dim);
        let glow = r8(device, "olr-render glow", sizes.art, wgpu::TextureFormat::R8Unorm);
        let groups = r8(device, "olr-render glow groups", sizes.world, wgpu::TextureFormat::R8Uint);
        let solid = r8(device, "olr-render solid", sizes.world, wgpu::TextureFormat::R8Uint);
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
            ],
        });
        let shadow_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render shadow"),
            layout: &self.shadow_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.lights.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view(&solid)),
                },
            ],
        });
        let inputs = super::gi::GiInputs {
            globals: &self.globals,
            glow: &view(&glow),
            groups: &view(&groups),
            solid: &view(&solid),
            terrain: &view(terrain),
        };
        let gi = super::gi::GiState::new(device, &self.gi, level, &inputs);
        let t = LookTextures {
            glow,
            groups,
            solid_view: view(&solid),
            solid,
            art_scale: map.art_scale(),
            level,
            terrain_bind,
            shadow_bind,
            gi,
        };
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
    /// holding it of its groups and material.
    pub(super) fn write(&self, queue: &wgpu::Queue, map: &ColorMap, r: Rect) {
        let Some(t) = &self.textures else { return };
        let wr = world_rect(r, t.art_scale);
        if let (Some(glow), Some(groups)) = (map.glow(), map.glow_groups()) {
            write_bytes(queue, &t.glow, (glow, map.width()), r);
            write_bytes(queue, &t.groups, (groups, t.level.0), wr);
        }
        write_bytes(queue, &t.solid, (map.material().cells(), t.level.0), wr);
    }

    /// Packs `look` (and where this frame draws) into the globals' floats.
    pub(super) fn pack(&mut self, look: &LookFrame, frame: &WorldFrame) {
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
        p.extend([
            frame.origin.0 as f32,
            frame.origin.1 as f32,
            frame.scale as f32,
            look.lights.len() as f32,
        ]);
        p.extend([level.0 as f32, level.1 as f32, art_scale as f32, cell as f32]);
        let count = look.lights.len().min(olr_look::MAX_LIGHTS);
        self.light_count = count as u32;
        let l = &mut self.packed_lights;
        l.clear();
        for f in &look.lights[..count] {
            l.extend([f.pos[0], f.pos[1], f.radius.max(0.0), f.reach.min(1e9)]);
            l.extend([f.colour[0], f.colour[1], f.colour[2], 0.0]);
        }
        l.resize(LIGHT_FLOATS, 0.0);
        for v in l.iter_mut() {
            if !v.is_finite() {
                *v = 0.0;
            }
        }
        let p = &mut self.packed;
        let mode = match look.mode {
            ViewMode::Lit => 0.0,
            ViewMode::FullBright => 1.0,
            ViewMode::LightOnly => 2.0,
        };
        p.extend([
            mode,
            look.groups.len().min(GROUPS) as f32,
            f32::from(look.light_soak.min(olr_look::MAX_SOAK)),
            0.0,
        ]);
        let [r, g, b] = look.solid_floor;
        p.extend([r, g, b, 0.0]);
        for i in 0..GROUPS {
            match look.groups.get(i) {
                Some(g) => p.extend([g.cast[0], g.cast[1], g.cast[2], g.self_scale]),
                None => p.extend([0.0; 4]),
            }
        }
        for i in 0..GROUPS {
            match look.groups.get(i) {
                Some(g) => p.extend([g.reach.min(1e9), f32::from(u8::from(g.own_colour)), 0.0, 0.0]),
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
                self.look = Some(LookState::new(&self.device, &self.globals_layout, &self.texture_layout));
            }
            (false, true) => self.look = None,
            _ => return,
        }
        // The world target gains or loses its shade target.
        self.target = None;
    }

    /// Whether lighting is on ([`Renderer::set_lighting`]).
    pub fn lighting(&self) -> bool {
        self.look.is_some()
    }

    /// The view's light grid as last drawn (tests): the texture
    /// (RGBA16Float) and where it lies; `None` before a lit frame.
    pub fn light_grid(&self) -> Option<(&wgpu::Texture, crate::LightGrid)> {
        let t = self.target.as_ref()?;
        let lit = t.lit.as_ref()?;
        let pad = LIGHT_PAD as i32;
        Some((
            &lit.light[1],
            crate::LightGrid { origin: (t.origin.0 - pad, t.origin.1 - pad), size: lit.grid },
        ))
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
}
