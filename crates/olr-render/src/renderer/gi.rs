//! Glow light (plan 15a Task 12, spec §Glow light): 2D global illumination
//! with radiance cascades fixed to the world, drawn only where the views
//! need them ([`LightRects`]): a view's light is the whole level's, but for
//! what walls outside them would bounce. The shaders are `look/gi*.wgsl`;
//! the cascades' layout is in `gi.wgsl`'s header.
//!
//! Chosen parameters: cascade 0 has a probe per cell and 4 directions;
//! each cascade doubles the probe spacing, the directions and the interval
//! length (2 cells at cascade 0). Rays are sphere-traced at full resolution
//! with a jump-flood distance to the nearest cell that stops them (solid or
//! glowing), at most 96 steps a ray: a coarser grid closed thin shafts and
//! leaked through thin walls (plan 15a Task 12). Cascades are added until
//! their intervals reach the grid's diagonal.

use super::timing::{PassGroup, Timer};
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
// or Rg32Float target. Two channels (issue #35: half the bytes).
const REACH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
/// The jump flood's seeds: each cell's nearest stopper's cell, x and y
/// whole (issue #35: exact in `Rg16Uint`, half the bytes of the f16
/// halves; GL draws into it as into the occluders' `R8Uint`), `NO_SEED`
/// none.
const SEED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Uint;
/// Each cell's albedo (plan 15c: bounce): its rock and dirt's art colour,
/// averaged, linear.
const ALBEDO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The dynamic layer's reach: unsplit f16 (max blending needs one value),
/// 60 000 or more unlimited (plan 15c); one channel (issue #35).
const DYNAMIC_REACH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;

/// Floats a disc: world x, y, radius, reach; radiance r, g, b, 0 (plan 15c).
pub(super) const EMITTER_FLOATS: usize = 8;

const EMITTER_STRIDE: u64 = (EMITTER_FLOATS * 4) as u64;

/// The cascades' first interval, in cells.
const L0: f32 = 2.0;

/// The most cascades (intervals past 2^15 cells reach any level).
const MAX_CASCADES: u32 = 14;

/// The jump flood's first step, cells: it finds the nearest stopper only
/// within about this far, and a cell with none that near lets a ray jump
/// this far less a cell and a half (`JFA_FREE` in the shaders). Half the
/// passes of a flood over the whole grid; far from walls a ray only
/// takes more jumps (`a_light_crosses_a_wide_open_level`).
const JFA_REACH: u32 = 32;

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
    /// The last frame's discs' quads back to zero (issue #35).
    dynamic_clear: wgpu::RenderPipeline,
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
    /// The cascades' group 1 (the directional-glow design): the level's
    /// beam map (`renderer/glow_beams.rs`).
    pub(super) beam_layout: wgpu::BindGroupLayout,
}

impl GiPipelines {
    /// `heat` is the heat look's layout (issue #39), the emission's group 1.
    pub(super) fn new(device: &wgpu::Device, heat: &wgpu::BindGroupLayout) -> GiPipelines {
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
        let jfa_layout = layout("olr-render gi jfa", &[entry(0, frag, super::UNIFORM), entry(1, frag, UINT)]);
        let cascade_layout = layout(
            "olr-render gi cascade",
            &[
                entry(0, frag, super::UNIFORM),
                entry(1, frag, FLOAT),
                entry(2, frag, FLOAT),
                entry(3, frag, FLOAT),
                entry(4, frag, UINT),
                entry(5, frag, FLOAT),
                entry(6, frag, FLOAT),
                entry(7, frag, super::UNIFORM),
                entry(8, frag, FLOAT),
            ],
        );
        let fluence_layout = layout("olr-render gi fluence", &[entry(0, frag, FLOAT)]);
        let beam_layout = layout("olr-render gi glow beams", &[entry(0, frag, UINT)]);
        let head = format!(
            "{}\n{}\nconst JFA_FREE: f32 = {:?};\n",
            super::look::common(),
            include_str!("../look/gi.wgsl"),
            JFA_REACH as f32 - 1.5
        );
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
        let emitted = [(FORMAT, None), (REACH_FORMAT, None), (ALBEDO_FORMAT, None), (FORMAT, None)];
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let max = wgpu::BlendComponent { operation: wgpu::BlendOperation::Max, ..add };
        let discs = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
        let dynamic = |fs, targets: &[_]| {
            pipeline(
                device,
                "olr-render gi dynamic",
                &format!("{head}\n{}", include_str!("../look/gi_dynamic.wgsl")),
                ("vs_disc", fs),
                &[Some(&dynamic_layout)],
                &[Some(wgpu::VertexBufferLayout {
                    array_stride: EMITTER_STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &discs,
                })],
                targets,
                wgpu::PrimitiveTopology::TriangleStrip,
            )
        };
        GiPipelines {
            dynamic: dynamic(
                "fs_disc",
                &[
                    (FORMAT, Some(wgpu::BlendState { color: add, alpha: add })),
                    (DYNAMIC_REACH_FORMAT, Some(wgpu::BlendState { color: max, alpha: max })),
                ],
            ),
            dynamic_clear: dynamic("fs_disc_clear", &[(FORMAT, None), (DYNAMIC_REACH_FORMAT, None)]),
            emission: make_with(
                "olr-render gi emission",
                &format!("{}\n{}", super::heat::heat_reader(1), include_str!("../look/gi_emission.wgsl")),
                "fs_emission",
                &[Some(&emission_layout), Some(heat)],
                &emitted,
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
            cascade: make_with(
                "olr-render gi cascade",
                include_str!("../look/gi_cascade.wgsl"),
                "fs_cascade",
                &[Some(&cascade_layout), Some(&beam_layout)],
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
            beam_layout,
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

/// A render target the passes also read.
fn texture(
    device: &wgpu::Device,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// The rectangles the glow light's passes draw for the frame's views
/// (the user's ruling, 2026-11-04: the cascades only where a view needs
/// them; a 427 x 192 view on Castle Strike cost 54 ms of cascades a
/// frame on the AMD integrated GPU over the whole level, 8 within its
/// rectangles). `levels[0]` is the fluence's cells, `levels[1 + i]`
/// cascade `i`'s texels, each `[x, y, w, h]`, apart. Cascade 0's probes
/// are the cells the views' light grids sample (bilinear: a cell more
/// each way); each cascade's are those the probes below merge
/// (`fs_cascade`: `q0` and `q0 + 1`, clamped), so the light in a view is
/// the whole level's, but for the bounce of walls outside the
/// rectangles: their history is not made (zero), at most 1 of 255 of
/// Liero Factory Reloaded's picture (the spike's 60 frames). Reused
/// frame after frame: no allocation once warm.
#[derive(Default)]
pub(super) struct LightRects {
    levels: Vec<Vec<[u32; 4]>>,
    /// Probe rectangles `[x0, y0, x1, y1]` (ends included) being merged.
    scratch: Vec<[i32; 4]>,
    next: Vec<[i32; 4]>,
}

impl LightRects {
    /// The rectangles of `count` cascades over `grid` cells of `cell`
    /// world pixels, for light grids over the world-pixel rectangles
    /// `views` (`[x0, y0, x1, y1]`, ends excluded).
    pub(super) fn fill(
        &mut self,
        grid: (u32, u32),
        cell: u32,
        count: u32,
        views: impl IntoIterator<Item = [i32; 4]>,
    ) {
        let levels = 1 + count as usize;
        self.levels.resize_with(levels, Vec::new);
        self.levels.truncate(levels);
        self.levels.iter_mut().for_each(Vec::clear);
        let c = cell.max(1) as i32;
        let (gw, gh) = (grid.0 as i32, grid.1 as i32);
        self.scratch.clear();
        for [x0, y0, x1, y1] in views {
            let r = [
                x0.div_euclid(c) - 1,
                y0.div_euclid(c) - 1,
                (x1 - 1).div_euclid(c) + 1,
                (y1 - 1).div_euclid(c) + 1,
            ];
            if let Some(r) = clamped(r, (gw, gh)) {
                self.scratch.push(r);
            }
        }
        merge(&mut self.scratch);
        let out = |r: &[i32; 4], (bw, bh): (u32, u32)| {
            [r[0] as u32 * bw, r[1] as u32 * bh, (r[2] - r[0] + 1) as u32 * bw, (r[3] - r[1] + 1) as u32 * bh]
        };
        self.levels[0].extend(self.scratch.iter().map(|r| out(r, (1, 1))));
        for i in 0..count {
            let shape = Cascade::new(i, grid);
            self.levels[1 + i as usize].extend(self.scratch.iter().map(|r| out(r, (shape.bw, shape.bh))));
            if i + 1 == count {
                break;
            }
            // The probes above that these merge.
            let upper = Cascade::new(i + 1, grid).probes;
            let q0 = |p: i32| (2 * p - 1).div_euclid(4);
            self.next.clear();
            for r in &self.scratch {
                let up = [q0(r[0]), q0(r[1]), q0(r[2]) + 1, q0(r[3]) + 1];
                if let Some(up) = clamped(up, (upper.0 as i32, upper.1 as i32)) {
                    self.next.push(up);
                }
            }
            std::mem::swap(&mut self.scratch, &mut self.next);
            merge(&mut self.scratch);
        }
    }

    /// Level `i`'s rectangles (0 the fluence, `1 + i` cascade `i`).
    pub(super) fn level(&self, i: usize) -> &[[u32; 4]] {
        self.levels.get(i).map_or(&[], Vec::as_slice)
    }
}

/// `r` (ends included) cut to `0..size`, or `None` outside it.
fn clamped(r: [i32; 4], size: (i32, i32)) -> Option<[i32; 4]> {
    let c = [r[0].max(0), r[1].max(0), r[2].min(size.0 - 1), r[3].min(size.1 - 1)];
    (c[0] <= c[2] && c[1] <= c[3]).then_some(c)
}

/// Overlapping rectangles (ends included) become their bounding box,
/// until none overlap.
fn merge(rects: &mut Vec<[i32; 4]>) {
    'again: loop {
        for i in 0..rects.len() {
            for j in i + 1..rects.len() {
                let (a, b) = (rects[i], rects[j]);
                if a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3] {
                    rects[i] = [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])];
                    rects.swap_remove(j);
                    continue 'again;
                }
            }
        }
        return;
    }
}

/// Whether every rectangle of `need` lies inside one of `have`.
fn covered(need: &[[u32; 4]], have: &[[u32; 4]]) -> bool {
    need.iter().all(|n| {
        have.iter()
            .any(|h| n[0] >= h[0] && n[1] >= h[1] && n[0] + n[2] <= h[0] + h[2] && n[1] + n[3] <= h[1] + h[3])
    })
}

/// One level's glow light: its grid, pyramid, cascades and fluence.
pub(super) struct GiState {
    /// World pixels per cell.
    pub(super) cell: u32,
    /// The grid's cells across and down.
    pub(super) grid: (u32, u32),
    /// The emission grid (rgb cast, solid share), its reach, its albedo
    /// and its haze (hot open background: light along a ray, no stop).
    emission: (wgpu::TextureView, wgpu::TextureView, wgpu::TextureView, wgpu::TextureView),
    /// Play's discs this frame (plan 15c): radiance and reach per cell.
    /// Zero but under the quads of the discs last drawn, which the next
    /// frame zeroes again before drawing its own (issue #35).
    dynamic: (wgpu::TextureView, wgpu::TextureView),
    dynamic_bind: wgpu::BindGroup,
    /// Two disc buffers ([`EMITTER_FLOATS`] floats a disc, the first
    /// [`olr_look::MAX_EMITTERS`]): the frame's discs go into the one not
    /// holding the discs last drawn.
    discs: [wgpu::Buffer; 2],
    /// The buffer holding the discs last drawn into the dynamic layer, and
    /// how many (0: the layer is all zero).
    drawn: std::cell::Cell<(usize, u32)>,
    /// The jump flood's two textures: the seeds go into the first.
    seeds: [wgpu::TextureView; 2],
    pub(super) cascades: Vec<wgpu::TextureView>,
    /// The glow light per cell, which the light sum samples. It is also
    /// the history bounce reads (plan 15c): the cascades run before
    /// `fs_fluence` writes it, so they read the last frame's (issue #35:
    /// no copy).
    pub(super) fluence: wgpu::TextureView,
    /// The fluence holds no last light: cleared before the next frame's
    /// cascades read it.
    fresh: std::cell::Cell<bool>,
    /// What the fluence's light is of (the glow light once a tick): the
    /// frame's step ([`olr_look::LookFrame::step`]) and the heat's passes
    /// run by then; `None` when its inputs changed since, or it has none.
    kept: std::cell::Cell<Option<(u64, u64)>>,
    /// The frame's rectangles ([`GiState::plan`]).
    rects: std::cell::RefCell<LightRects>,
    /// The fluence's cells the kept light was made over: a view moved
    /// past them within its step needs it made again.
    lit: std::cell::RefCell<Vec<[u32; 4]>>,
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
            texture(device, "olr-render gi haze", grid, FORMAT),
        );
        let dynamic = (
            texture(device, "olr-render gi dynamic", grid, FORMAT),
            texture(device, "olr-render gi dynamic reach", grid, DYNAMIC_REACH_FORMAT),
        );
        let discs = [0, 1].map(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("olr-render emitters"),
                size: (olr_look::MAX_EMITTERS * EMITTER_FLOATS * 4) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let seeds = [0, 1].map(|_| texture(device, "olr-render gi seeds", grid, SEED_FORMAT));
        let shapes: Vec<Cascade> = (0..count).map(|i| Cascade::new(i, grid)).collect();
        let cascades: Vec<_> =
            shapes.iter().map(|c| texture(device, "olr-render gi cascade", c.size(), FORMAT)).collect();
        let fluence = texture(device, "olr-render glow light", grid, FORMAT);
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
        // Steps of half the grid's power of two, at most `JFA_REACH`, down
        // to 1, ping-ponging.
        let mut jfa = Vec::new();
        let mut step = (grid.0.max(grid.1).next_power_of_two() / 2).min(JFA_REACH);
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
                        super::heat::HEAT_REACH,
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
                        tv(&fluence),
                        inputs.globals.as_entire_binding(),
                        tv(&emission.3),
                    ],
                )
            })
            .collect();
        let fluence_bind = bind(device, &pipes.fluence_layout, [tv(&cascades[0])]);
        GiState {
            cell,
            grid,
            emission,
            dynamic,
            dynamic_bind,
            discs,
            drawn: std::cell::Cell::new((0, 0)),
            seeds,
            cascades,
            fluence,
            fresh: std::cell::Cell::new(true),
            kept: std::cell::Cell::new(None),
            rects: std::cell::RefCell::new(LightRects::default()),
            lit: std::cell::RefCell::new(Vec::new()),
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
    /// `heat` is the heat look's bind group (issue #39): hot pixels cast; no
    /// worm takes part (#53). `beams` is the cascades' group 1, the level's
    /// beam map (the directional-glow design). `count` discs were last given to
    /// [`GiState::write_discs`] (plan 15c). Returns whether the history
    /// took a step (the renderer encodes it once a frame, however many
    /// views: issue #26).
    pub(super) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipes: &GiPipelines,
        casting: bool,
        (heat, beams): (&wgpu::BindGroup, &wgpu::BindGroup),
        count: u32,
        timer: Option<&Timer>,
    ) -> bool {
        // Each pass timed in its group (bench only, issue #34).
        let stamp = |group| timer.and_then(|t| t.pass(group, None, None));
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    group: PassGroup,
                    targets: &[&wgpu::TextureView],
                    work: Option<(&wgpu::RenderPipeline, &wgpu::BindGroup)>,
                    more: &[&wgpu::BindGroup],
                    within: Option<&[[u32; 4]]>| {
            // At most four targets, kept on the stack (issue #35: no
            // allocation a pass).
            let attachments: [Option<wgpu::RenderPassColorAttachment>; 4] = std::array::from_fn(|i| {
                targets.get(i).map(|view| wgpu::RenderPassColorAttachment {
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
                label: Some("olr-render gi"),
                color_attachments: &attachments[..targets.len().min(4)],
                depth_stencil_attachment: None,
                timestamp_writes: stamp(group),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some((pipeline, bind)) = work {
                p.set_pipeline(pipeline);
                p.set_bind_group(0, bind, &[]);
                for (i, g) in more.iter().enumerate() {
                    p.set_bind_group(1 + i as u32, *g, &[]);
                }
                match within {
                    None => p.draw(0..3, 0..1),
                    // Only these rectangles (the rest stays cleared).
                    Some(rects) => {
                        for r in rects {
                            p.set_scissor_rect(r[0], r[1], r[2], r[3]);
                            p.draw(0..3, 0..1);
                        }
                    }
                }
            }
        };
        if !casting {
            pass(encoder, PassGroup::Fluence, &[&self.fluence], None, &[], None);
            self.reset();
            return false;
        }
        if self.fresh.replace(false) {
            pass(encoder, PassGroup::Fluence, &[&self.fluence], None, &[], None);
        }
        // The discs (issue #35): the quads last drawn back to zero, then
        // this frame's added; no pass when neither has any.
        let (last, drawn) = self.drawn.get();
        if drawn > 0 || count > 0 {
            let both = [&self.dynamic.0, &self.dynamic.1];
            let attachments = both.map(|view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })
            });
            let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("olr-render gi dynamic"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: stamp(PassGroup::Emission),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            p.set_bind_group(0, &self.dynamic_bind, &[]);
            if drawn > 0 {
                p.set_pipeline(&pipes.dynamic_clear);
                p.set_vertex_buffer(0, self.discs[last].slice(..));
                p.draw(0..4, 0..drawn);
            }
            if count > 0 {
                p.set_pipeline(&pipes.dynamic);
                p.set_vertex_buffer(0, self.discs[1 - last].slice(..));
                p.draw(0..4, 0..count);
            }
            self.drawn.set((1 - last, count));
        }
        let emission = Some((&pipes.emission, &self.emission_bind));
        let emitted = [&self.emission.0, &self.emission.1, &self.emission.2, &self.emission.3];
        pass(encoder, PassGroup::Emission, &emitted, emission, &[heat], None);
        let seed = Some((&pipes.seed, &self.seed_bind));
        pass(encoder, PassGroup::JumpFlood, &[&self.seeds[0]], seed, &[], None);
        for (target, b) in &self.jfa {
            pass(encoder, PassGroup::JumpFlood, &[&self.seeds[*target]], Some((&pipes.jfa, b)), &[], None);
        }
        // Each cascade, then the fluence, only where the views need them.
        let rects = self.rects.borrow();
        for i in (0..self.cascades.len()).rev() {
            let cascade = Some((&pipes.cascade, &self.cascade_binds[i]));
            pass(
                encoder,
                PassGroup::Cascades,
                &[&self.cascades[i]],
                cascade,
                &[beams],
                Some(rects.level(1 + i)),
            );
        }
        // The next frame's bounce reads this frame's light, kept here.
        let fluence = Some((&pipes.fluence, &self.fluence_bind));
        pass(encoder, PassGroup::Fluence, &[&self.fluence], fluence, &[], Some(rects.level(0)));
        true
    }

    /// Writes the frame's discs (`packed`, [`EMITTER_FLOATS`] floats each,
    /// at most [`olr_look::MAX_EMITTERS`]) into the buffer not holding the
    /// discs last drawn, which [`GiState::encode`] draws.
    pub(super) fn write_discs(&self, queue: &super::uploads::Queue, packed: &[f32]) {
        if !packed.is_empty() {
            queue.write_buffer(&self.discs[1 - self.drawn.get().0], 0, bytemuck::cast_slice(packed));
        }
    }

    /// Forgets the light's history: the next frame bounces nothing old.
    pub(super) fn reset(&self) {
        self.fresh.set(true);
        self.stale();
    }

    /// The frame's views: their light grids over the world-pixel
    /// rectangles `views` (`[x0, y0, x1, y1]`, ends excluded), which the
    /// next [`GiState::encode`] lights ([`LightRects`]).
    pub(super) fn plan(&self, views: impl IntoIterator<Item = [i32; 4]>) {
        let count = self.cascades.len() as u32;
        self.rects.borrow_mut().fill(self.grid, self.cell, count, views);
    }

    /// Whether the fluence holds the light of `key` (a step and the heat
    /// passes run) over the planned views, so a frame of it may keep it:
    /// never for `None`, nor when a view moved past the cells it was made
    /// over.
    pub(super) fn holds(&self, key: Option<(u64, u64)>) -> bool {
        key.is_some() && self.kept.get() == key && covered(self.rects.borrow().level(0), &self.lit.borrow())
    }

    /// The light just encoded is of `key`, over the planned views.
    pub(super) fn made(&self, key: Option<(u64, u64)>) {
        self.kept.set(key);
        let mut lit = self.lit.borrow_mut();
        lit.clear();
        lit.extend_from_slice(self.rects.borrow().level(0));
    }

    /// What the light reads changed (the terrain, the heat's materials):
    /// the next frame makes it again, whatever its step.
    pub(super) fn stale(&self) {
        self.kept.set(None);
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

    /// The probes of cascade `i + 1` a probe `p` of cascade `i` merges
    /// (`fs_cascade`: bilinear between `q0` and `q0 + 1`, clamped).
    fn upper_probes(p: (u32, u32), upper: (u32, u32)) -> [(u32, u32); 4] {
        let q0 = |p: u32| (((p as f32 + 0.5) / 2.0 - 0.5).floor()) as i32;
        let at = |q: i32, n: u32| q.clamp(0, n as i32 - 1) as u32;
        let (x, y) = (q0(p.0), q0(p.1));
        [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| (at(x + dx, upper.0), at(y + dy, upper.1)))
    }

    fn inside(rects: &[[u32; 4]], (x, y): (u32, u32)) -> bool {
        rects.iter().any(|r| x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3])
    }

    fn light_rects(grid: (u32, u32), cell: u32, count: u32, views: &[[i32; 4]]) -> Vec<Vec<[u32; 4]>> {
        let mut r = LightRects::default();
        r.fill(grid, cell, count, views.iter().copied());
        (0..=count as usize).map(|i| r.level(i).to_vec()).collect()
    }

    /// Castle Strike's grid and cascades (cell 1).
    const GRID: (u32, u32) = (1350, 550);
    const COUNT: u32 = 10;

    #[test]
    fn a_view_over_the_whole_level_lights_all_of_it() {
        let r = light_rects(GRID, 1, COUNT, &[[-40, -40, 1400, 600]]);
        assert_eq!(r.len(), 1 + COUNT as usize);
        assert_eq!(r[0], [[0, 0, 1350, 550]]);
        for i in 0..COUNT {
            let (w, h) = Cascade::new(i, GRID).size();
            assert_eq!(r[1 + i as usize], [[0, 0, w, h]], "cascade {i}");
        }
    }

    #[test]
    fn each_cascade_holds_every_probe_the_one_below_merges() {
        // One view, two apart, two overlapping, one at a corner; cells of 2.
        let cases: [(u32, &[[i32; 4]]); 4] = [
            (1, &[[500, 200, 927, 392]]),
            (1, &[[10, 10, 437, 202], [900, 330, 1327, 522]]),
            (1, &[[100, 100, 527, 292], [300, 200, 727, 392]]),
            (2, &[[-30, 300, 397, 492]]),
        ];
        for (cell, views) in cases {
            let grid = (GRID.0.div_ceil(cell), GRID.1.div_ceil(cell));
            let r = light_rects(grid, cell, COUNT, views);
            // The fluence holds every cell the views' light grids sample
            // (bilinear: the cells around each world pixel).
            for v in views {
                for (x, y) in [(v[0], v[1]), (v[2] - 1, v[3] - 1), (v[0], v[3] - 1)] {
                    let c = |w: i32, n: u32| {
                        ((w as f32 / cell as f32 - 0.5).floor() as i32).clamp(0, n as i32 - 1) as u32
                    };
                    for (dx, dy) in [(0, 0), (1, 1)] {
                        let p = ((c(x, grid.0) + dx).min(grid.0 - 1), (c(y, grid.1) + dy).min(grid.1 - 1));
                        assert!(inside(&r[0], p), "cell {cell} {views:?}: fluence {p:?} of {:?}", r[0]);
                    }
                }
            }
            // Cascade 0's probes are the fluence's cells; each cascade's
            // probes merge only probes above them that are drawn.
            let probes = |i: usize| -> Vec<[u32; 4]> {
                let s = Cascade::new(i as u32, grid);
                r[1 + i].iter().map(|t| [t[0] / s.bw, t[1] / s.bh, t[2] / s.bw, t[3] / s.bh]).collect()
            };
            assert_eq!(probes(0), r[0]);
            for i in 0..COUNT as usize - 1 {
                let upper = Cascade::new(i as u32 + 1, grid).probes;
                for rect in probes(i) {
                    for y in rect[1]..rect[1] + rect[3] {
                        for x in rect[0]..rect[0] + rect[2] {
                            for q in upper_probes((x, y), upper) {
                                assert!(
                                    inside(&probes(i + 1), q),
                                    "{views:?}: cascade {i} ({x}, {y}) reads {q:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_game_view_lights_a_small_part_of_a_large_level() {
        // A 427 x 192 view and its light grid's pad, mid-level.
        let r = light_rects(GRID, 1, COUNT, &[[440, 150, 440 + 427 + 58, 150 + 192 + 58]]);
        let area = |rects: &[[u32; 4]]| rects.iter().map(|t| u64::from(t[2]) * u64::from(t[3])).sum::<u64>();
        let all = |i: u32| {
            let (w, h) = Cascade::new(i, GRID).size();
            u64::from(w) * u64::from(h)
        };
        assert!(area(&r[1]) * 5 < all(0), "cascade 0: {:?}", r[1]);
        assert!(area(&r[4]) * 3 < all(3), "cascade 3: {:?}", r[4]);
        // Overlapping views' rectangles merge: never a texel drawn twice.
        let r = light_rects(GRID, 1, COUNT, &[[100, 100, 527, 292], [300, 200, 727, 392]]);
        for level in &r {
            for (i, a) in level.iter().enumerate() {
                for b in &level[i + 1..] {
                    let apart = a[0] + a[2] <= b[0]
                        || b[0] + b[2] <= a[0]
                        || a[1] + a[3] <= b[1]
                        || b[1] + b[3] <= a[1];
                    assert!(apart, "{a:?} and {b:?} overlap");
                }
            }
        }
    }

    #[test]
    fn a_view_off_the_level_lights_nothing() {
        let r = light_rects(GRID, 1, COUNT, &[[5000, 5000, 5400, 5200]]);
        assert!(r.iter().all(|level| level.is_empty()), "{r:?}");
        assert!(light_rects(GRID, 1, COUNT, &[]).iter().all(|level| level.is_empty()));
    }
}
