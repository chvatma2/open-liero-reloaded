//! Beam shadow maps (issue #100, spec
//! `docs/superpowers/specs/2026-11-02-beam-shadow-maps-design.md`): each
//! glow beam's shadows traced once a frame, outward from its source points
//! along a fan of rays fixed in the world, so the composite looks a texel's
//! visibility up instead of walking to the source. No OLX counterpart.
//!
//! The CPU decides which rays each (beam, source point) row marches, and
//! how far ([`rows`]); [`BeamShadow`] holds the map and draws its pass
//! (`look/beam_shadow.wgsl`), once a frame before the views' composites.

use super::timing::{PassGroup, Timer};
use super::{UNIFORM, entry, pipeline};

/// Rays over the full circle: ray `i` leaves its point at the world angle
/// `i × 2π / BEAM_RAYS` (y down), whatever the beam's direction, so a
/// turning beam's rays never move over still scenery. At the longest reach
/// (`MAX_BEAM_REACH`, 512) neighbours are 0.8 px apart.
pub(super) const BEAM_RAYS: u32 = 4000;

/// The map's width: a half-plane's rays and one each side (within the
/// 2048 texels every device allows).
pub(super) const SHADOW_WIDTH: u32 = BEAM_RAYS / 2 + 2;

/// The points across a beam's source a pixel looks for (soft shadows;
/// `BEAM_SHADOW_POINTS` in `composite.wgsl`).
pub(super) const BEAM_POINTS: u32 = 8;

/// Stored distance units a world pixel.
pub(super) const SHADOW_SCALE: f32 = 8.0;

/// A stored distance: nothing stops the ray within its limit.
pub(super) const SHADOW_CLEAR: u16 = u16::MAX;

/// The map's rows: every beam's points.
pub(super) const MAX_SHADOW_ROWS: u32 = olr_look::glow_beams::MAX_GLOW_BEAMS as u32 * BEAM_POINTS;

/// The map's format: a stored distance a texel (GL renders into it).
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Uint;

/// The near zone's radius round each source point: a lit pixel within it
/// walks to the point exactly (the walks are short there); beyond it, its
/// angle from the point lies within its row's span.
pub(super) fn near(half: f32) -> f32 {
    6.0 * half.max(0.0) + 24.0
}

/// The widest angle from a beam's direction, radians, at which a lit pixel
/// at least [`near`] from one of its source points lies as seen from it:
/// a pixel `a` along and `b` aside (`|b| < half + a (tan + 0.03) + 0.5`,
/// the composite's wedge and penumbra) is `a + 0.5` ahead of the point and
/// at most `2 half + 0.5 + a (tan + 0.03)` aside; beyond `near` its `a +
/// 0.5` is at least `(near − 2 half − 0.5) / (1 + tan + 0.03)`. Below a
/// right angle: every lit pixel is ahead of every point.
fn widest(half: f32, tan: f32) -> f32 {
    let t = tan.max(0.0) + 0.03;
    let width = 2.0 * half.max(0.0) + 0.5;
    let ahead = (near(half) - width) / (1.0 + t);
    (t + width / ahead).atan().min(core::f32::consts::FRAC_PI_2)
}

/// One row of the map: the rays `start..start + count` (mod
/// [`BEAM_RAYS`]) from one source point, marched to `limit` world pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ShadowRow {
    pub start: u32,
    pub count: u32,
    pub limit: f32,
}

/// The rows of `packed` (`glow_beams::pack`'s beams, `BEAM_FLOATS` each):
/// each beam's [`BEAM_POINTS`] rows in point order.
pub(super) fn rows(packed: &[f32], out: &mut Vec<ShadowRow>) {
    use super::glow_beams::BEAM_FLOATS;
    out.clear();
    let step = core::f32::consts::TAU / BEAM_RAYS as f32;
    for b in packed.as_chunks::<BEAM_FLOATS>().0 {
        let (dir, half, tan, reach) = ([b[2], b[3]], b[4].max(0.0), b[5].max(0.0), b[6].max(0.0));
        let facing = dir[1].atan2(dir[0]);
        let wide = widest(half, tan);
        // One ray of margin each side of the bracketing pair.
        let first = ((facing - wide) / step).floor() as i64 - 1;
        let last = ((facing + wide) / step).ceil() as i64 + 1;
        let count = ((last - first + 1) as u32).min(SHADOW_WIDTH);
        let start = first.rem_euclid(i64::from(BEAM_RAYS)) as u32;
        // The farthest lit pixel: `reach` along, at the wedge's edge, from
        // the farthest point.
        let aside = 2.0 * half + reach * (tan + 0.03) + 0.5;
        let limit = (reach + 0.5).hypot(aside) + 2.0;
        for _ in 0..BEAM_POINTS {
            out.push(ShadowRow { start, count, limit });
        }
    }
}

/// The shadow map and its pass (one for every level: the map is sized for
/// the cap, `SHADOW_WIDTH` × `MAX_SHADOW_ROWS`, 2 MB).
pub(super) struct BeamShadow {
    map: wgpu::Texture,
    map_view: wgpu::TextureView,
    /// Per row: first ray, rays, limit (f32 bits), beam (`Rows` in the
    /// shaders); the composite reads it too.
    rows_buffer: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    /// The frame's rows ([`BeamShadow::prepare`]).
    rows: Vec<ShadowRow>,
    words: Vec<[u32; 4]>,
}

impl BeamShadow {
    /// The map, the rows' uniform and the pass; `prelude` is
    /// `glow_beams::wgsl()`.
    pub(super) fn new(device: &wgpu::Device, prelude: &str) -> BeamShadow {
        let map = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("olr-render beam shadow map"),
            size: wgpu::Extent3d { width: SHADOW_WIDTH, height: MAX_SHADOW_ROWS, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let rows_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render beam shadow rows"),
            size: u64::from(MAX_SHADOW_ROWS) * 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uint = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Uint,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let (vf, f) = (wgpu::ShaderStages::VERTEX_FRAGMENT, wgpu::ShaderStages::FRAGMENT);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render beam shadow"),
            entries: &[entry(0, f, UNIFORM), entry(1, vf, UNIFORM), entry(2, f, uint), entry(3, f, uint)],
        });
        let source = format!("{prelude}\n{}", include_str!("../look/beam_shadow.wgsl"));
        let pipeline = pipeline(
            device,
            "olr-render beam shadow",
            &source,
            ("vs_row", "fs_march"),
            &[Some(&layout)],
            &[],
            &[(FORMAT, None)],
            wgpu::PrimitiveTopology::TriangleStrip,
        );
        BeamShadow {
            map_view: map.create_view(&wgpu::TextureViewDescriptor::default()),
            map,
            rows_buffer,
            layout,
            pipeline,
            rows: Vec::with_capacity(MAX_SHADOW_ROWS as usize),
            words: Vec::with_capacity(MAX_SHADOW_ROWS as usize),
        }
    }

    /// A level's group 0 for the pass: the beams uniform, the rows, its
    /// solid map and its block map.
    pub(super) fn bind(
        &self,
        device: &wgpu::Device,
        beams: &wgpu::Buffer,
        solid: &wgpu::TextureView,
        blocks: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render beam shadow"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: beams.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.rows_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(solid) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(blocks) },
            ],
        })
    }

    /// The frame's rows from its packed beams (none: no pass), written to
    /// the rows' uniform.
    pub(super) fn prepare(&mut self, queue: &super::uploads::Queue, packed: &[f32]) {
        rows(packed, &mut self.rows);
        self.words.clear();
        self.words.extend(
            self.rows
                .iter()
                .enumerate()
                .map(|(i, r)| [r.start, r.count, r.limit.to_bits(), i as u32 / BEAM_POINTS]),
        );
        if !self.words.is_empty() {
            queue.write_buffer(&self.rows_buffer, 0, bytemuck::cast_slice(&self.words));
        }
    }

    /// Draws the frame's rows into the map with `bind` (the level's
    /// [`BeamShadow::bind`]); whether a pass ran (none without rows).
    pub(super) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind: &wgpu::BindGroup,
        timer: Option<&Timer>,
    ) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("olr-render beam shadow"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.map_view,
                depth_slice: None,
                resolve_target: None,
                // Texels past a row's span are never read.
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timer.and_then(|t| t.pass(PassGroup::BeamShadow, None, None)),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..4, 0..self.rows.len() as u32);
        true
    }

    /// The map (tests).
    pub(super) fn map(&self) -> &wgpu::Texture {
        &self.map
    }

    /// The frame's rows.
    pub(super) fn rows(&self) -> &[ShadowRow] {
        &self.rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::glow_beams::{BEAM_FLOATS, walk_reach};
    use core::f32::consts::PI;

    /// Source point `k` of a beam: `composite.wgsl`'s and `beam_shadow.wgsl`'s, `half` each side of
    /// `at` across `dir`, half a pixel behind the face.
    fn point(at: [f32; 2], dir: [f32; 2], half: f32, k: u32) -> [f32; 2] {
        let u = (k as f32 + 0.5) / BEAM_POINTS as f32 * 2.0 - 1.0;
        let perp = [-dir[1], dir[0]];
        [at[0] + perp[0] * u * half - dir[0] * 0.5, at[1] + perp[1] * u * half - dir[1] * 0.5]
    }

    /// One beam as `pack` writes it (group 1, white, region 0).
    fn packed(at: [f32; 2], angle_deg: f32, half: f32, spread_deg: f32, reach: f32) -> Vec<f32> {
        let a = angle_deg.to_radians();
        let tan = spread_deg.to_radians().tan();
        let v = vec![at[0], at[1], a.cos(), a.sin(), half, tan, reach, 1.0, 1.0, 1.0, 1.0, 0.0];
        assert_eq!(v.len(), BEAM_FLOATS);
        v
    }

    /// Whether ray `i` (any integer, taken mod [`BEAM_RAYS`]) is in `row`.
    fn has(row: &ShadowRow, i: i64) -> bool {
        let r = i64::from(BEAM_RAYS);
        (i - i64::from(row.start)).rem_euclid(r) < i64::from(row.count)
    }

    #[test]
    fn every_lit_point_beyond_the_near_zone_has_its_rays() {
        let step = 2.0 * PI / BEAM_RAYS as f32;
        let mut out = Vec::new();
        for angle in [0.0f32, 37.0, 90.0, 181.0, 300.0] {
            for half in [0.5f32, 3.0, 8.0] {
                for spread in [1.0f32, 10.0, 40.0, 70.0, 89.5] {
                    let tan = spread.to_radians().tan();
                    let reach = walk_reach(half, tan).min(512.0);
                    let at = [300.0, 200.0];
                    rows(&packed(at, angle, half, spread, reach), &mut out);
                    assert_eq!(out.len(), BEAM_POINTS as usize);
                    let a = angle.to_radians();
                    let (dir, perp) = ([a.cos(), a.sin()], [-a.sin(), a.cos()]);
                    let mut along = 0.25;
                    while along <= reach {
                        let edge = half + along * tan + 0.5 + 0.03 * along;
                        let mut side = -edge + 0.01;
                        while side < edge {
                            let p = [
                                at[0] + dir[0] * along + perp[0] * side,
                                at[1] + dir[1] * along + perp[1] * side,
                            ];
                            for (k, row) in out.iter().enumerate() {
                                let s = point(at, dir, half, k as u32);
                                let v = [p[0] - s[0], p[1] - s[1]];
                                let d = v[0].hypot(v[1]);
                                if d < near(half) {
                                    continue;
                                }
                                let f = v[1].atan2(v[0]) / step;
                                let i = f.floor() as i64;
                                let what = format!("angle {angle} half {half} spread {spread} k {k} p {p:?}");
                                assert!(
                                    has(row, i) && has(row, i + 1),
                                    "{what}: rays {i}, {} not in {row:?}",
                                    i + 1
                                );
                                assert!(d <= row.limit, "{what}: {d} past the limit {}", row.limit);
                            }
                            side += (edge / 100.0).max(0.5);
                        }
                        along += if along < 40.0 { 1.0 } else { 7.0 };
                    }
                }
            }
        }
    }

    #[test]
    fn a_span_across_angle_zero_wraps() {
        let mut out = Vec::new();
        rows(&packed([100.0, 100.0], 0.0, 2.0, 10.0, 200.0), &mut out);
        for row in &out {
            assert!(row.start + row.count > BEAM_RAYS, "{row:?} wraps");
            assert!(has(row, 0) && has(row, -1), "{row:?} holds angle 0");
        }
    }

    #[test]
    fn a_narrow_beam_takes_few_rays() {
        let mut out = Vec::new();
        rows(&packed([100.0, 100.0], 37.0, 2.0, 10.0, 400.0), &mut out);
        assert!(out.iter().all(|r| r.count < SHADOW_WIDTH / 4), "{out:?}");
    }

    #[test]
    fn a_wide_beam_takes_the_half_plane() {
        let mut out = Vec::new();
        rows(&packed([100.0, 100.0], 37.0, 3.0, 89.5, 20.0), &mut out);
        assert!(out.iter().all(|r| r.count <= SHADOW_WIDTH), "{out:?}");
    }

    #[test]
    fn rows_follow_the_packed_beams() {
        let mut beams = packed([100.0, 100.0], 0.0, 2.0, 10.0, 200.0);
        beams.extend(packed([300.0, 50.0], 180.0, 2.0, 10.0, 200.0));
        let mut out = vec![ShadowRow { start: 1, count: 1, limit: 1.0 }];
        rows(&beams, &mut out);
        assert_eq!(out.len(), 16);
        assert!(
            out[8..].iter().all(|r| has(r, i64::from(BEAM_RAYS / 2))),
            "beam 1 faces left: {:?}",
            &out[8..]
        );
        rows(&[], &mut out);
        assert!(out.is_empty());
    }
}
