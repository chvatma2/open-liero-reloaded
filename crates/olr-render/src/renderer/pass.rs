//! One drawn frame: the world pass, then the upscale pass.

use olr_look::LookFrame;
use olr_scene::DrawList;

use super::{Renderer, Target, WORLD_FORMAT, texture, upscale_uniform};
use crate::frame::{WorldFrame, occluder_instances, push_instances, terrain_instance};
use crate::upscale::dest_rect_in;

impl Renderer {
    /// Draws `list` into `target` (`size` pixels, of the renderer's
    /// surface format): the terrain and the layers at art scale into the
    /// world target (the view plus a margin), then the view's part of it
    /// upscaled with [`super::RenderSettings::filter`]. Black where there
    /// is no map, and around the picture when the aspects differ.
    pub fn render(&mut self, list: &DrawList, target: &wgpu::TextureView, size: (u32, u32)) {
        self.render_in(list, target, [0, 0, size.0, size.1]);
    }

    /// [`Renderer::render`] with the picture placed in the part `area`
    /// (`[x, y, w, h]` pixels) of `target` (the game's playfield above its
    /// HUD bar: [`crate::dest_rect_in`]); the rest of `target` is black.
    pub fn render_in(&mut self, list: &DrawList, target: &wgpu::TextureView, area: [u32; 4]) {
        self.draw(list, None, target, area);
    }

    /// [`Renderer::render_in`] with the look of `look` (plan 15a): with
    /// lighting on ([`Renderer::set_lighting`]) and `Some`, the world pass
    /// also draws each texel's shade and the look passes light it; with
    /// `None`, or lighting off, the frame is exactly `render_in`'s.
    pub fn render_lit(
        &mut self,
        list: &DrawList,
        look: Option<&LookFrame>,
        target: &wgpu::TextureView,
        area: [u32; 4],
    ) {
        let look = look.filter(|_| self.look.is_some());
        self.draw(list, look, target, area);
    }

    fn draw(
        &mut self,
        list: &DrawList,
        look: Option<&LookFrame>,
        target: &wgpu::TextureView,
        area: [u32; 4],
    ) {
        let frame = WorldFrame::new(list.view, self.scale());
        self.ensure_target(&frame);
        if let Some(t) = self.target.as_mut() {
            t.origin = frame.origin;
        }
        self.instances.clear();
        if let Some(t) = &self.terrain {
            // A colour map the device made us crop covers its share of the map.
            let (tw, th) = (t.texture.width(), t.texture.height());
            let share =
                |m: u32, part: u32, all: u32| (u64::from(m) * u64::from(part) / u64::from(all.max(1))) as u32;
            let map = (share(list.map.0, tw, t.art.0), share(list.map.1, th, t.art.1));
            self.instances.push(terrain_instance(&frame, map, (tw, th)));
        }
        let first_sprite = self.instances.len() as u32;
        if let Some((layout, _)) = &self.atlas {
            push_instances(list, layout, &frame, &mut self.instances);
        }
        // The occluder map's quads (plan 15b), lit frames only.
        let first_occluder = self.instances.len() as u32;
        if let (Some(_), Some(_), Some((layout, _))) = (look, &self.look, &self.atlas) {
            let pad = super::look::LIGHT_PAD as i32;
            let origin = (frame.origin.0 - pad, frame.origin.1 - pad);
            occluder_instances(list, layout, origin, &mut self.instances);
        }
        let end_occluder = self.instances.len() as u32;
        self.raw.clear();
        self.raw.extend(self.instances.iter().map(|i| i.raw()));
        if self.raw.len() as u64 * super::INSTANCE_STRIDE > self.instance_buffer.size() {
            self.instance_buffer = Self::instance_buffer(&self.device, self.raw.len().next_power_of_two());
        }
        if !self.raw.is_empty() {
            self.queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.raw));
        }
        let (tw, th) = self.target.as_ref().expect("ensured above").size;
        self.queue.write_buffer(&self.globals, 0, bytemuck::cast_slice(&[tw as f32, th as f32, 0.0, 0.0]));
        let src = frame.source_rect(list.view);
        let dst = dest_rect_in(self.settings.filter, (src[2], src[3]), area);
        let uniform = upscale_uniform(src, dst, (tw, th), self.settings.filter, self.srgb);
        self.queue.write_buffer(&self.upscale_uniform, 0, bytemuck::cast_slice(&uniform));
        if let (Some(look), Some(state)) = (look, self.look.as_mut()) {
            state.pack(look, &frame);
            self.queue.write_buffer(&state.globals, 0, bytemuck::cast_slice(&state.packed));
            if !state.packed_emitters.is_empty() {
                self.queue.write_buffer(&state.emitters, 0, bytemuck::cast_slice(&state.packed_emitters));
            }
        }

        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("olr-render") });
        let world = &self.target.as_ref().expect("ensured above");
        let stamp = |begin: Option<u32>, end: Option<u32>| {
            self.timer.as_ref().map(|t| wgpu::RenderPassTimestampWrites {
                query_set: &t.set,
                beginning_of_pass_write_index: begin,
                end_of_pass_write_index: end,
            })
        };
        let lit = look.and(self.look.as_ref()).zip(world.shade.as_ref());
        {
            let mut pass = match lit {
                Some((_, shade)) => {
                    begin_two(&mut encoder, "olr-render world lit", &world.view, shade, stamp(Some(0), None))
                }
                None => begin(&mut encoder, "olr-render world", &world.view, stamp(Some(0), None)),
            };
            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
            match (lit, &self.terrain) {
                (Some((state, _)), Some(t)) => {
                    if let Some(tex) = &state.textures {
                        pass.set_pipeline(&state.terrain_pipeline);
                        pass.set_bind_group(1, &t.bind, &[]);
                        pass.set_bind_group(2, &tex.terrain_bind, &[]);
                        pass.draw(0..4, 0..1);
                    }
                }
                (None, Some(t)) => {
                    pass.set_pipeline(&self.world_pipeline);
                    pass.set_bind_group(1, &t.bind, &[]);
                    pass.draw(0..4, 0..1);
                }
                (_, None) => {}
            }
            let end = first_occluder;
            if let Some((_, bind)) = &self.atlas
                && end > first_sprite
            {
                match lit {
                    Some((state, _)) => pass.set_pipeline(&state.sprite_pipeline),
                    None => pass.set_pipeline(&self.world_pipeline),
                }
                pass.set_bind_group(1, bind, &[]);
                pass.draw(0..4, first_sprite..end);
            }
        }
        let lit_target = lit.and(world.lit.as_ref()).zip(self.look.as_ref());
        if let Some((lt, state)) = lit_target {
            {
                // Cleared every lit frame: only this frame's worms occlude.
                let mut pass = begin(&mut encoder, "olr-render occluders", &lt.occluder_view, None);
                if let Some((_, bind)) = &self.atlas
                    && end_occluder > first_occluder
                {
                    pass.set_pipeline(&state.occluder_pipeline);
                    pass.set_bind_group(0, &state.occluder_globals_bind, &[]);
                    pass.set_bind_group(1, bind, &[]);
                    pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                    pass.draw(0..4, first_occluder..end_occluder);
                }
            }
            if let Some(tex) = &state.textures {
                let casting = look.is_some_and(|l| !l.groups.is_empty()) || state.emitter_count > 0;
                let discs = (&state.emitters, state.emitter_count);
                tex.gi.encode(&mut encoder, &state.gi, casting, &lt.occluder_bind, discs);
            }
            let passes = [
                (&lt.views[0], &state.sum_pipeline, &lt.binds[0]),
                (&lt.views[1], &state.soak_h_pipeline, &lt.soak_binds[0]),
                (&lt.views[2], &state.soak_v_pipeline, &lt.soak_binds[1]),
                (&lt.views[0], &state.blur_h_pipeline, &lt.binds[1]),
                (&lt.views[1], &state.blur_v_pipeline, &lt.binds[2]),
                (&lt.lit_view, &state.composite_pipeline, &lt.composite_bind),
            ];
            for (view, pipeline, bind) in passes {
                let mut pass = begin(&mut encoder, "olr-render look", view, None);
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        {
            let mut pass = begin(&mut encoder, "olr-render upscale", target, stamp(None, Some(1)));
            pass.set_pipeline(&self.upscale_pipeline);
            let bind = lit_target.map_or(&world.bind, |(lt, _)| &lt.upscale_bind);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
        if let Some(p) = self.probe.as_mut() {
            p.encode(
                &self.device,
                &self.queue,
                &mut encoder,
                (area[2], area[3]),
                &world.view,
                self.timer.as_ref(),
            );
        }
        let probing = self.probe.as_ref().is_some_and(|p| p.probe.passes > 0);
        if let Some(t) = self.timer.as_mut() {
            t.resolve(&mut encoder, probing);
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Makes the world target fit `frame` (a new one only when the size
    /// changes, so steady frames allocate nothing).
    fn ensure_target(&mut self, frame: &WorldFrame) {
        let size = frame.target_size();
        let lit = self.look.is_some();
        let pad = 2 * super::look::LIGHT_PAD;
        let grid = (frame.size.0 + pad, frame.size.1 + pad);
        let grid_fits = |t: &Target| t.lit.as_ref().is_none_or(|l| l.grid == grid);
        if self
            .target
            .as_ref()
            .is_some_and(|t| t.frame_size == size && t.shade.is_some() == lit && grid_fits(t))
        {
            return;
        }
        let max = self.device.limits().max_texture_dimension_2d;
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let clamped = (size.0.min(max), size.1.min(max));
        let tex = texture(&self.device, "olr-render world", clamped, usage);
        debug_assert_eq!(tex.format(), WORLD_FORMAT);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render upscale"),
            layout: &self.upscale_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.upscale_uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let shade = lit.then(|| {
            let t = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("olr-render shade"),
                size: wgpu::Extent3d {
                    width: clamped.0.max(1),
                    height: clamped.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: super::look::SHADE_FORMAT,
                usage,
                view_formats: &[],
            });
            t.create_view(&wgpu::TextureViewDescriptor::default())
        });
        let lit = match (&self.look, &shade) {
            (Some(state), Some(shade)) => Some(state.target(
                &self.device,
                grid,
                clamped,
                &view,
                shade,
                (&self.upscale_layout, &self.upscale_uniform, &self.sampler),
            )),
            _ => None,
        };
        self.target =
            Some(Target { frame_size: size, size: clamped, view, bind, shade, lit, origin: frame.origin });
    }
}

/// A render pass that clears `view` to black, writing `timestamps` (GPU
/// timing, bench only) when given.
pub(super) fn begin<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: timestamps,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// [`begin`] with a second target, `shade`, cleared to zero (lit, no glow).
fn begin_two<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    shade: &wgpu::TextureView,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'e> {
    let attachment = |view, clear| {
        Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
        })
    };
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[
            attachment(view, wgpu::Color::BLACK),
            attachment(shade, wgpu::Color::TRANSPARENT),
        ],
        depth_stencil_attachment: None,
        timestamp_writes: timestamps,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}
