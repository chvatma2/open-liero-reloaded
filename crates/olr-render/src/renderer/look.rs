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

/// The globals' floats: six vec4s, then two arrays of [`GROUPS`] vec4s.
pub(super) const GLOBAL_FLOATS: usize = 4 * (6 + 2 * GROUPS);

/// The format of the world pass's second target: self-light, marker, 0, alpha.
pub(super) const SHADE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The look's textures for one colour map.
pub(super) struct LookTextures {
    pub(super) glow: wgpu::Texture,
    pub(super) groups: wgpu::Texture,
    pub(super) solid: wgpu::Texture,
    pub(super) art_scale: u32,
    /// The level's size in world pixels (uncropped).
    pub(super) level: (u32, u32),
    /// The lit terrain's group 2: globals, glow, groups.
    pub(super) terrain_bind: wgpu::BindGroup,
}

pub(super) struct LookState {
    pub(super) terrain_pipeline: wgpu::RenderPipeline,
    pub(super) sprite_pipeline: wgpu::RenderPipeline,
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) globals: wgpu::Buffer,
    pub(super) textures: Option<LookTextures>,
    /// The packed globals, reused every frame.
    pub(super) packed: Vec<f32>,
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
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render look globals"),
            size: (GLOBAL_FLOATS * 4) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        LookState {
            terrain_pipeline,
            sprite_pipeline,
            layout,
            globals,
            textures: None,
            packed: Vec::with_capacity(GLOBAL_FLOATS),
        }
    }

    /// New textures for `map`, uploaded whole (zeros where it has no glow).
    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        map: &ColorMap,
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
        let t = LookTextures { glow, groups, solid, art_scale: map.art_scale(), level, terrain_bind };
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
        let (level, art_scale) = self.textures.as_ref().map_or(((0, 0), 1), |t| (t.level, t.art_scale));
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
        p.extend([level.0 as f32, level.1 as f32, art_scale as f32, 1.0]);
        let mode = match look.mode {
            ViewMode::Lit => 0.0,
            ViewMode::FullBright => 1.0,
            ViewMode::LightOnly => 2.0,
        };
        p.extend([mode, look.groups.len().min(GROUPS) as f32, 0.0, 0.0]);
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
            _ => {}
        }
        // The world target gains or loses its shade target.
        self.target = None;
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
