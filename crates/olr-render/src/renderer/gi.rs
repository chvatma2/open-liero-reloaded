//! Glow light (plan 15a Task 12, spec §Glow light): 2D global illumination
//! with radiance cascades over the whole level, fixed to the world, so the
//! light never depends on the camera. The shaders are `look/gi*.wgsl`;
//! the cascades' layout is in `gi.wgsl`'s header.
//!
//! Chosen parameters: cascade 0 has a probe per cell and 4 directions;
//! each cascade doubles the probe spacing, the directions and the interval
//! length (2 cells at cascade 0). Rays are sphere-traced at full resolution
//! with a jump-flood distance to the nearest cell that stops them (solid or
//! glowing), at most 96 steps a ray: a coarser grid closed thin shafts and
//! leaked through thin walls (plan 15a Task 12). Cascades are added until
//! their intervals reach the grid's diagonal.

use super::{entry, pipeline};

/// The most cells the glow light's grid holds (plan 15a R13; plan 15c:
/// 2^20, so Castle Strike's 1350 × 550 has a cell a world pixel and no thin
/// wall shares a cell with the air beside it).
pub const GI_CELL_BUDGET: u32 = 1 << 20;

/// The glow light's cell for a level of `world` pixels on a device whose
/// textures are at most `max_dim` texels a side: the smallest power of two
/// `s` (1, 2, 4, ...) with `ceil(w / s) × ceil(h / s)` at most
/// [`GI_CELL_BUDGET`] cells and every cascade's texture (at most twice the
/// grid, plus a probe block) within `max_dim`.
pub fn gi_cell(world: (u32, u32), max_dim: u32) -> u32 {
    let fits = |s: u32| {
        let (w, h) = (u64::from(world.0.div_ceil(s)), u64::from(world.1.div_ceil(s)));
        w * h <= u64::from(GI_CELL_BUDGET) && 2 * w.max(h) + 8 <= u64::from(max_dim)
    };
    let mut s = 1u32;
    while !fits(s) && s < 1 << 16 {
        s *= 2;
    }
    s
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
// Split in f16 halves (`split` in the shaders): GL draws into no R32Float
// or Rg32Float target.
const REACH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const SEED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Each cell's albedo (plan 15c: bounce): its rock and dirt's art colour,
/// averaged, linear.
const ALBEDO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The dynamic layer's reach: unsplit f16 (max blending needs one value),
/// 60 000 or more unlimited (plan 15c).
const DYNAMIC_REACH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Floats a disc: world x, y, radius, reach; radiance r, g, b, 0 (plan 15c).
pub(super) const EMITTER_FLOATS: usize = 8;

const EMITTER_STRIDE: u64 = (EMITTER_FLOATS * 4) as u64;

/// The cascades' first interval, in cells.
const L0: f32 = 2.0;

/// The most cascades (intervals past 2^15 cells reach any level).
const MAX_CASCADES: u32 = 14;

const FLOAT: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Float { filterable: false },
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};

const UINT: wgpu::BindingType = wgpu::BindingType::Texture {
    sample_type: wgpu::TextureSampleType::Uint,
    view_dimension: wgpu::TextureViewDimension::D2,
    multisampled: false,
};

/// The glow light's pipelines and layouts (one set per renderer).
pub(super) struct GiPipelines {
    /// Play's glowing discs into the dynamic layer (plan 15c).
    dynamic: wgpu::RenderPipeline,
    emission: wgpu::RenderPipeline,
    seed: wgpu::RenderPipeline,
    jfa: wgpu::RenderPipeline,
    cascade: wgpu::RenderPipeline,
    fluence: wgpu::RenderPipeline,
    dynamic_layout: wgpu::BindGroupLayout,
    emission_layout: wgpu::BindGroupLayout,
    seed_layout: wgpu::BindGroupLayout,
    jfa_layout: wgpu::BindGroupLayout,
    cascade_layout: wgpu::BindGroupLayout,
    fluence_layout: wgpu::BindGroupLayout,
}

impl GiPipelines {
    /// `occluders` is the occluder map's layout (plan 15b), the emission's
    /// group 1.
    pub(super) fn new(device: &wgpu::Device, occluders: &wgpu::BindGroupLayout) -> GiPipelines {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries })
        };
        let emission_layout = layout(
            "olr-render gi emission",
            &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, super::UNIFORM),
                entry(2, frag, FLOAT),
                entry(3, frag, UINT),
                entry(4, frag, UINT),
                entry(5, frag, FLOAT),
                entry(6, frag, FLOAT),
                entry(7, frag, FLOAT),
                entry(8, frag, UINT),
            ],
        );
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let dynamic_layout = layout(
            "olr-render gi dynamic",
            &[entry(0, both, super::UNIFORM), entry(1, frag, UINT), entry(2, frag, super::UNIFORM)],
        );
        let seed_layout = layout("olr-render gi seed", &[entry(0, frag, FLOAT)]);
        let jfa_layout =
            layout("olr-render gi jfa", &[entry(0, frag, super::UNIFORM), entry(1, frag, FLOAT)]);
        let cascade_layout = layout(
            "olr-render gi cascade",
            &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, FLOAT),
                entry(2, frag, FLOAT),
                entry(3, frag, FLOAT),
                entry(4, frag, FLOAT),
                entry(5, frag, FLOAT),
                entry(6, frag, FLOAT),
                entry(7, frag, super::UNIFORM),
            ],
        );
        let fluence_layout = layout("olr-render gi fluence", &[entry(0, frag, FLOAT)]);
        let head = format!("{}\n{}", super::look::common(), include_str!("../look/gi.wgsl"));
        let make_with = |label, body: &str, fs, layouts: &[Option<&wgpu::BindGroupLayout>], targets: &[_]| {
            pipeline(
                device,
                label,
                &format!("{head}\n{body}"),
                ("vs_full", fs),
                layouts,
                &[],
                targets,
                wgpu::PrimitiveTopology::TriangleList,
            )
        };
        let make = |label, body: &str, fs, layout: &wgpu::BindGroupLayout, targets: &[_]| {
            make_with(label, body, fs, &[Some(layout)], targets)
        };
        let three = [(FORMAT, None), (REACH_FORMAT, None), (ALBEDO_FORMAT, None)];
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let max = wgpu::BlendComponent { operation: wgpu::BlendOperation::Max, ..add };
        let discs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        GiPipelines {
            dynamic: pipeline(
                device,
                "olr-render gi dynamic",
                &format!("{head}\n{}", include_str!("../look/gi_dynamic.wgsl")),
                ("vs_disc", "fs_disc"),
                &[Some(&dynamic_layout)],
                &[Some(wgpu::VertexBufferLayout {
                    array_stride: EMITTER_STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &discs,
                })],
                &[
                    (FORMAT, Some(wgpu::BlendState { color: add, alpha: add })),
                    (DYNAMIC_REACH_FORMAT, Some(wgpu::BlendState { color: max, alpha: max })),
                ],
                wgpu::PrimitiveTopology::TriangleStrip,
            ),
            emission: make_with(
                "olr-render gi emission",
                include_str!("../look/gi_emission.wgsl"),
                "fs_emission",
                &[Some(&emission_layout), Some(occluders)],
                &three,
            ),
            seed: make(
                "olr-render gi seed",
                include_str!("../look/gi_seed.wgsl"),
                "fs_seed",
                &seed_layout,
                &[(SEED_FORMAT, None)],
            ),
            jfa: make(
                "olr-render gi jfa",
                include_str!("../look/gi_jfa.wgsl"),
                "fs_jfa",
                &jfa_layout,
                &[(SEED_FORMAT, None)],
            ),
            cascade: make(
                "olr-render gi cascade",
                include_str!("../look/gi_cascade.wgsl"),
                "fs_cascade",
                &cascade_layout,
                &[(FORMAT, None)],
            ),
            fluence: make(
                "olr-render gi fluence",
                include_str!("../look/gi_fluence.wgsl"),
                "fs_fluence",
                &fluence_layout,
                &[(FORMAT, None)],
            ),
            dynamic_layout,
            emission_layout,
            seed_layout,
            jfa_layout,
            cascade_layout,
            fluence_layout,
        }
    }
}

/// One cascade's shape.
#[derive(Clone, Copy, Debug)]
struct Cascade {
    dirs: u32,
    bw: u32,
    bh: u32,
    probes: (u32, u32),
}

impl Cascade {
    fn new(i: u32, grid: (u32, u32)) -> Cascade {
        let dirs = 4 << i;
        let bw = 1 << (i + 2).div_ceil(2);
        let spacing = 1 << i;
        Cascade { dirs, bw, bh: dirs / bw, probes: (grid.0.div_ceil(spacing), grid.1.div_ceil(spacing)) }
    }

    fn size(&self) -> (u32, u32) {
        (self.probes.0 * self.bw, self.probes.1 * self.bh)
    }
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    texture_with(device, label, size, format, wgpu::TextureUsages::empty()).create_view(&Default::default())
}

/// A render target that may also be copied (`extra`).
fn texture_with(
    device: &wgpu::Device,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    extra: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | extra,
        view_formats: &[],
    })
}

/// One level's glow light: its grid, pyramid, cascades and fluence.
pub(super) struct GiState {
    /// World pixels per cell.
    pub(super) cell: u32,
    /// The emission grid (rgb cast, solid share), its reach and its albedo.
    emission: (wgpu::TextureView, wgpu::TextureView, wgpu::TextureView),
    /// Play's discs this frame (plan 15c): radiance and reach per cell.
    dynamic: (wgpu::TextureView, wgpu::TextureView),
    dynamic_bind: wgpu::BindGroup,
    /// The jump flood's two textures: the seeds go into the first.
    seeds: [wgpu::TextureView; 2],
    cascades: Vec<wgpu::TextureView>,
    /// The glow light per cell, which the light sum samples.
    pub(super) fluence: wgpu::TextureView,
    fluence_texture: wgpu::Texture,
    /// The last frame's fluence, which bounce reads (plan 15c).
    history: wgpu::TextureView,
    history_texture: wgpu::Texture,
    /// The history holds nothing yet: cleared before the next frame reads it.
    fresh: std::cell::Cell<bool>,
    emission_bind: wgpu::BindGroup,
    seed_bind: wgpu::BindGroup,
    /// Each flood pass: the texture it writes and its bind (step, the other).
    jfa: Vec<(usize, wgpu::BindGroup)>,
    cascade_binds: Vec<wgpu::BindGroup>,
    fluence_bind: wgpu::BindGroup,
}

/// A uniform buffer holding `floats`.
fn params(device: &wgpu::Device, floats: &[f32]) -> wgpu::Buffer {
    use wgpu::util::DeviceExt as _;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("olr-render gi params"),
        contents: bytemuck::cast_slice(floats),
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

fn bind<'a>(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    resources: impl IntoIterator<Item = wgpu::BindingResource<'a>>,
) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = resources
        .into_iter()
        .enumerate()
        .map(|(i, resource)| wgpu::BindGroupEntry { binding: i as u32, resource })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("olr-render gi"),
        layout,
        entries: &entries,
    })
}

/// The look's textures the emission pass reads.
pub(super) struct GiInputs<'a> {
    pub(super) globals: &'a wgpu::Buffer,
    pub(super) glow: &'a wgpu::TextureView,
    pub(super) groups: &'a wgpu::TextureView,
    pub(super) solid: &'a wgpu::TextureView,
    pub(super) terrain: &'a wgpu::TextureView,
    /// Each world pixel's flicker region (plan 15c).
    pub(super) regions: &'a wgpu::TextureView,
}

impl GiState {
    /// The glow light of a level of `level` world pixels.
    pub(super) fn new(
        device: &wgpu::Device,
        pipes: &GiPipelines,
        level: (u32, u32),
        max_dim: u32,
        inputs: &GiInputs,
    ) -> GiState {
        let cell = gi_cell(level, max_dim);
        let grid = (level.0.div_ceil(cell).max(1), level.1.div_ceil(cell).max(1));
        let diag = f64::from(grid.0).hypot(f64::from(grid.1)) as f32;
        let mut count = 1;
        while L0 * ((1u32 << count) as f32 - 1.0) < diag && count < MAX_CASCADES {
            count += 1;
        }
        let emission = (
            texture(device, "olr-render gi emission", grid, FORMAT),
            texture(device, "olr-render gi reach", grid, REACH_FORMAT),
            texture(device, "olr-render gi albedo", grid, ALBEDO_FORMAT),
        );
        let dynamic = (
            texture(device, "olr-render gi dynamic", grid, FORMAT),
            texture(device, "olr-render gi dynamic reach", grid, DYNAMIC_REACH_FORMAT),
        );
        let seeds = [0, 1].map(|_| texture(device, "olr-render gi seeds", grid, SEED_FORMAT));
        let shapes: Vec<Cascade> = (0..count).map(|i| Cascade::new(i, grid)).collect();
        let cascades: Vec<_> =
            shapes.iter().map(|c| texture(device, "olr-render gi cascade", c.size(), FORMAT)).collect();
        let fluence_texture =
            texture_with(device, "olr-render glow light", grid, FORMAT, wgpu::TextureUsages::COPY_SRC);
        let fluence = fluence_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let history_texture = texture_with(
            device,
            "olr-render glow light history",
            grid,
            FORMAT,
            wgpu::TextureUsages::COPY_DST,
        );
        let history = history_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let tv = wgpu::BindingResource::TextureView;
        let emission_params = params(
            device,
            &[
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                1.0,
                grid.0 as f32,
                grid.1 as f32,
                cell as f32,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0,
            ],
        );
        let emission_bind = bind(
            device,
            &pipes.emission_layout,
            [
                inputs.globals.as_entire_binding(),
                emission_params.as_entire_binding(),
                tv(inputs.glow),
                tv(inputs.groups),
                tv(inputs.solid),
                tv(inputs.terrain),
                tv(&dynamic.0),
                tv(&dynamic.1),
                tv(inputs.regions),
            ],
        );
        let dynamic_bind = bind(
            device,
            &pipes.dynamic_layout,
            [emission_params.as_entire_binding(), tv(inputs.solid), inputs.globals.as_entire_binding()],
        );
        let seed_bind = bind(device, &pipes.seed_layout, [tv(&emission.0)]);
        // Steps of half the grid's power of two down to 1, ping-ponging.
        let mut jfa = Vec::new();
        let mut step = grid.0.max(grid.1).next_power_of_two() / 2;
        let mut current = 0;
        while step >= 1 {
            let p = params(device, &[step as f32, 0.0, 0.0, 0.0]);
            jfa.push((
                1 - current,
                bind(device, &pipes.jfa_layout, [p.as_entire_binding(), tv(&seeds[current])]),
            ));
            current = 1 - current;
            step /= 2;
        }
        let nearest = &seeds[current];
        let cascade_binds = (0..count as usize)
            .map(|i| {
                let c = shapes[i];
                let up = shapes.get(i + 1);
                let spacing = (1u32 << i) as f32;
                let u = up.map_or([0.0; 5], |u| {
                    [u.dirs as f32, u.bw as f32, u.bh as f32, u.probes.0 as f32, u.probes.1 as f32]
                });
                let p = params(
                    device,
                    &[
                        i as f32,
                        c.dirs as f32,
                        c.bw as f32,
                        c.bh as f32,
                        c.probes.0 as f32,
                        c.probes.1 as f32,
                        L0 * (spacing - 1.0),
                        L0 * spacing,
                        spacing,
                        grid.0 as f32,
                        grid.1 as f32,
                        cell as f32,
                        u[0],
                        u[1],
                        u[2],
                        u[3],
                        u[4],
                        f32::from(u8::from(up.is_some())),
                        0.0,
                        0.0,
                    ],
                );
                // The top cascade reads no next one: bind the emission
                // (never sampled).
                let upper = cascades.get(i + 1).unwrap_or(&emission.0);
                bind(
                    device,
                    &pipes.cascade_layout,
                    [
                        p.as_entire_binding(),
                        tv(&emission.0),
                        tv(&emission.1),
                        tv(upper),
                        tv(nearest),
                        tv(&emission.2),
                        tv(&history),
                        inputs.globals.as_entire_binding(),
                    ],
                )
            })
            .collect();
        let fluence_bind = bind(device, &pipes.fluence_layout, [tv(&cascades[0])]);
        GiState {
            cell,
            emission,
            dynamic,
            dynamic_bind,
            seeds,
            cascades,
            fluence,
            fluence_texture,
            history,
            history_texture,
            fresh: std::cell::Cell::new(true),
            emission_bind,
            seed_bind,
            jfa,
            cascade_binds,
            fluence_bind,
        }
    }

    /// Encodes the frame's glow light: play's discs, emission, pyramid,
    /// cascades from the top down, fluence. With `casting` false (no group
    /// casts light and no disc glows) the fluence is only cleared.
    /// `occluders` is the occluder map's bind group (plan 15b): living worms
    /// block glow light. `discs` holds `count` discs ([`EMITTER_FLOATS`]
    /// each, plan 15c). Returns whether the history took a step (the
    /// renderer encodes it once a frame, however many views: issue #26).
    pub(super) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipes: &GiPipelines,
        casting: bool,
        occluders: &wgpu::BindGroup,
        (discs, count): (&wgpu::Buffer, u32),
    ) -> bool {
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    targets: &[&wgpu::TextureView],
                    work: Option<(&wgpu::RenderPipeline, &wgpu::BindGroup)>,
                    group1: Option<&wgpu::BindGroup>| {
            let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = targets
                .iter()
                .map(|view| {
                    Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })
                })
                .collect();
            let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("olr-render gi"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some((pipeline, bind)) = work {
                p.set_pipeline(pipeline);
                p.set_bind_group(0, bind, &[]);
                if let Some(g) = group1 {
                    p.set_bind_group(1, g, &[]);
                }
                p.draw(0..3, 0..1);
            }
        };
        if !casting {
            pass(encoder, &[&self.fluence], None, None);
            self.reset();
            return false;
        }
        if self.fresh.replace(false) {
            pass(encoder, &[&self.history], None, None);
        }
        {
            let both = [&self.dynamic.0, &self.dynamic.1];
            let attachments = both.map(|view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })
            });
            let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("olr-render gi dynamic"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if count > 0 {
                p.set_pipeline(&pipes.dynamic);
                p.set_bind_group(0, &self.dynamic_bind, &[]);
                p.set_vertex_buffer(0, discs.slice(..));
                p.draw(0..4, 0..count);
            }
        }
        let emission = Some((&pipes.emission, &self.emission_bind));
        pass(encoder, &[&self.emission.0, &self.emission.1, &self.emission.2], emission, Some(occluders));
        pass(encoder, &[&self.seeds[0]], Some((&pipes.seed, &self.seed_bind)), None);
        for (target, b) in &self.jfa {
            pass(encoder, &[&self.seeds[*target]], Some((&pipes.jfa, b)), None);
        }
        for i in (0..self.cascades.len()).rev() {
            pass(encoder, &[&self.cascades[i]], Some((&pipes.cascade, &self.cascade_binds[i])), None);
        }
        pass(encoder, &[&self.fluence], Some((&pipes.fluence, &self.fluence_bind)), None);
        // The next frame's bounce reads this frame's light.
        encoder.copy_texture_to_texture(
            self.fluence_texture.as_image_copy(),
            self.history_texture.as_image_copy(),
            self.fluence_texture.size(),
        );
        true
    }

    /// Forgets the light's history: the next frame bounces nothing old.
    pub(super) fn reset(&self) {
        self.fresh.set(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascades_tile_their_probes() {
        let c0 = Cascade::new(0, (100, 40));
        assert_eq!((c0.dirs, c0.bw, c0.bh, c0.probes, c0.size()), (4, 2, 2, (100, 40), (200, 80)));
        let c3 = Cascade::new(3, (100, 40));
        assert_eq!((c3.dirs, c3.bw * c3.bh, c3.probes), (32, 32, (13, 5)));
        // Never wider than twice the grid.
        for i in 0..MAX_CASCADES {
            let c = Cascade::new(i, (4096, 64));
            assert!(c.size().0 <= 2 * 4096 + c.bw, "{i}: {:?}", c.size());
        }
    }
}
