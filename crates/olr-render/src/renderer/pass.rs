//! One drawn frame: the world pass, then the upscale pass.

use olr_scene::DrawList;

use super::{Renderer, Target, WORLD_FORMAT, texture, upscale_uniform};
use crate::frame::{WorldFrame, push_instances, terrain_instance};
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
        let frame = WorldFrame::new(list.view, self.scale());
        self.ensure_target(&frame);
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
        self.raw.clear();
        self.raw.extend(self.instances.iter().map(|i| i.raw()));
        if self.raw.len() * 48 > self.instance_buffer.size() as usize {
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
        {
            let mut pass = begin(&mut encoder, "olr-render world", &world.view, stamp(Some(0), None));
            pass.set_pipeline(&self.world_pipeline);
            pass.set_bind_group(0, &self.globals_bind, &[]);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
            if let Some(t) = &self.terrain {
                pass.set_bind_group(1, &t.bind, &[]);
                pass.draw(0..4, 0..1);
            }
            let end = self.instances.len() as u32;
            if let Some((_, bind)) = &self.atlas
                && end > first_sprite
            {
                pass.set_bind_group(1, bind, &[]);
                pass.draw(0..4, first_sprite..end);
            }
        }
        {
            let mut pass = begin(&mut encoder, "olr-render upscale", target, stamp(None, Some(1)));
            pass.set_pipeline(&self.upscale_pipeline);
            pass.set_bind_group(0, &world.bind, &[]);
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
        if self.target.as_ref().is_some_and(|t| t.frame_size == size) {
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
        self.target = Some(Target { frame_size: size, size: clamped, view, bind });
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
