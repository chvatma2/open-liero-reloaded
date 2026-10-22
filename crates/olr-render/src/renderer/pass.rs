//! One drawn frame: each view's world pass, the glow light once, each
//! view's look passes, then each view's upscale into its area (issue #26).

use std::ops::Range;

use olr_look::LookFrame;
use olr_scene::DrawList;

use super::look::{LIGHT_PAD, occluder_rect};
use super::timing::PassGroup;
use super::{Renderer, Target, WORLD_FORMAT, texture, upscale_uniform};
use crate::frame::{WorldFrame, distant_instance, occluder_instances, push_instances, terrain_instance};

/// One view of a frame ([`Renderer::render_views`], issue #26): a draw
/// list and the part of the target it is drawn into.
#[derive(Clone, Copy)]
pub struct ViewDraw<'a> {
    pub list: &'a DrawList,
    /// `[x, y, w, h]` pixels of the target (clipped to it).
    pub area: [u32; 4],
}

/// One view's share of a frame: where its world target lies and its
/// instances in the frame's instance buffer.
pub(super) struct ViewPlan {
    frame: WorldFrame,
    /// The terrain's quad (drawn only with a terrain).
    terrain: u32,
    /// Its distant layers' quads (issue #11), in `Renderer::distant_draws`.
    distant: Range<usize>,
    sprites: Range<u32>,
    /// Its occluder map's quads (plan 15b), lit frames only.
    occluders: Range<u32>,
}

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
        self.draw(&[ViewDraw { list, area }], None, target, true);
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
        self.render_views(&[ViewDraw { list, area }], look, target);
    }

    /// Several views in one frame (issue #26: split screen): `target` is
    /// cleared to black once, then each view is drawn as
    /// [`Renderer::render_lit`] draws one, into its own area. Each keeps
    /// its own world target, light grid, bloom and composite at its view's
    /// size; the glow light runs once for them all (one history step a
    /// frame). One view is exactly `render_lit`.
    pub fn render_views(&mut self, views: &[ViewDraw], look: Option<&LookFrame>, target: &wgpu::TextureView) {
        let look = self.lit_look(look);
        self.draw(views, look, target, true);
    }

    /// [`Renderer::render_views`] without the clear: each view's area is
    /// drawn over (its black bars too), the rest of `target` is left as it
    /// was.
    pub fn render_views_over(
        &mut self,
        views: &[ViewDraw],
        look: Option<&LookFrame>,
        target: &wgpu::TextureView,
    ) {
        let look = self.lit_look(look);
        self.draw(views, look, target, false);
    }

    /// `look` when lighting is on; without one, the light's history is
    /// forgotten.
    fn lit_look<'l>(&mut self, look: Option<&'l LookFrame>) -> Option<&'l LookFrame> {
        let look = look.filter(|_| self.look.is_some());
        if look.is_none() {
            self.reset_light_history();
        }
        look
    }

    /// Lays view `list`'s quads out in the instance buffer (the terrain,
    /// its distant layers, its sprites, then, lit, its occluders) and
    /// remembers where.
    fn plan_view(&mut self, list: &DrawList, frame: WorldFrame, lit: bool) {
        let terrain = self.instances.len() as u32;
        let first_distant = self.distant_draws.len();
        if let Some(t) = &self.terrain {
            // A colour map the device made us crop covers its share of the map.
            let (tw, th) = (t.texture.width(), t.texture.height());
            let share =
                |m: u32, part: u32, all: u32| (u64::from(m) * u64::from(part) / u64::from(all.max(1))) as u32;
            let map = (share(list.map.0, tw, t.art.0), share(list.map.1, th, t.art.1));
            self.instances.push(terrain_instance(&frame, map, (tw, th)));
            // The distant layers' quads (issue #11), far to near.
            let camera = list.camera();
            for (k, d) in self.distant.iter().enumerate() {
                let at = d.layer.position(camera, list.map);
                if let Some(i) = distant_instance(&frame, at, d.art, t.art_scale, list.map, d.layer.depth) {
                    self.distant_draws.push((k, self.instances.len() as u32));
                    self.instances.push(i);
                }
            }
        }
        let first_sprite = self.instances.len() as u32;
        if let Some((layout, _)) = &self.atlas {
            push_instances(list, layout, &frame, &mut self.instances);
        }
        // The occluder map's quads (plan 15b), lit frames only.
        let first_occluder = self.instances.len() as u32;
        if let (true, Some(_), Some((layout, _))) = (lit, &self.look, &self.atlas) {
            let pad = LIGHT_PAD as i32;
            let origin = (frame.origin.0 - pad, frame.origin.1 - pad);
            occluder_instances(list, layout, origin, &mut self.instances);
        }
        let end_occluder = self.instances.len() as u32;
        self.plans.push(ViewPlan {
            frame,
            terrain,
            distant: first_distant..self.distant_draws.len(),
            sprites: first_sprite..first_occluder,
            occluders: first_occluder..end_occluder,
        });
    }

    fn draw(
        &mut self,
        views: &[ViewDraw],
        look: Option<&LookFrame>,
        target: &wgpu::TextureView,
        clear: bool,
    ) {
        let out = (target.texture().width(), target.texture().height());
        let lit = look.is_some() && self.look.is_some();
        self.instances.clear();
        self.distant_draws.clear();
        self.plans.clear();
        for (i, v) in views.iter().enumerate() {
            let frame = WorldFrame::new(v.list.view, self.scale());
            self.ensure_target(i, &frame);
            self.targets[i].origin = frame.origin;
            self.plan_view(v.list, frame, lit);
        }
        self.raw.clear();
        self.raw.extend(self.instances.iter().map(|i| i.raw()));
        if self.raw.len() as u64 * super::INSTANCE_STRIDE > self.instance_buffer.size() {
            self.instance_buffer = Self::instance_buffer(&self.device, self.raw.len().next_power_of_two());
        }
        if !self.raw.is_empty() {
            self.queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&self.raw));
        }
        // Each view's uniforms are its own: the one submit draws them all.
        for (v, (plan, t)) in views.iter().zip(self.plans.iter().zip(&self.targets)) {
            let (tw, th) = t.size;
            self.queue.write_buffer(&t.globals, 0, bytemuck::cast_slice(&[tw as f32, th as f32, 0.0, 0.0]));
            let src = plan.frame.source_rect(v.list.view);
            let dst = self.dest_rect_in((src[2], src[3]), v.area);
            let uniform = upscale_uniform(src, dst, (tw, th), self.settings.filter, self.srgb);
            self.queue.write_buffer(&t.upscale_uniform, 0, bytemuck::cast_slice(&uniform));
        }
        if let (Some(look), Some(state)) = (look, self.look.as_mut()) {
            state.sync_regions(&self.queue, look);
            state.pack_emitters(look);
            if !state.packed_emitters.is_empty() {
                self.queue.write_buffer(&state.emitters, 0, bytemuck::cast_slice(&state.packed_emitters));
            }
            for (plan, t) in self.plans.iter().zip(&self.targets) {
                state.pack(look, &plan.frame, !plan.distant.is_empty(), occluder_rect(&plan.frame));
                if let Some(lt) = &t.lit {
                    self.queue.write_buffer(&lt.globals, 0, bytemuck::cast_slice(&state.packed));
                }
            }
            // The frame's globals (the glow light's, the lit terrain's):
            // the first view's, but with the level's occluder map.
            if let Some(first) = self.plans.first() {
                let map = state
                    .textures
                    .as_ref()
                    .map_or((1, 1), |t| (t.gi_occluders.width(), t.gi_occluders.height()));
                let level = [0.0, 0.0, map.0 as f32, map.1 as f32];
                state.pack(look, &first.frame, !first.distant.is_empty(), level);
                self.queue.write_buffer(&state.globals, 0, bytemuck::cast_slice(&state.packed));
            }
        }

        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("olr-render") });
        // Timed (bench only): the first view's world pass starts at slot
        // 0, the last view's upscale ends at slot 1, and every pass writes
        // a pair of its own, counted in its group (issue #34).
        if let Some(t) = &self.timer {
            t.start_frame();
        }
        let timer = self.timer.as_ref();
        let stamp =
            |group, begin: Option<u32>, end: Option<u32>| timer.and_then(|t| t.pass(group, begin, end));
        let group = |g| stamp(g, None, None);
        let views_drawn = self.plans.iter().zip(&self.targets);
        for (i, (plan, world)) in views_drawn.clone().enumerate() {
            let lit = look.and(self.look.as_ref()).zip(world.shade.as_ref());
            let (terrain, first_sprite) = (plan.terrain, plan.sprites.start);
            let mut pass = match lit {
                Some((_, shade)) => begin_two(
                    &mut encoder,
                    "olr-render world lit",
                    &world.view,
                    shade,
                    stamp(PassGroup::World, (i == 0).then_some(0), None),
                ),
                None => begin(
                    &mut encoder,
                    "olr-render world",
                    &world.view,
                    stamp(PassGroup::World, (i == 0).then_some(0), None),
                ),
            };
            pass.set_bind_group(0, &world.globals_bind, &[]);
            pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
            // A see-through terrain (a level with distant layers, issue
            // #11): the terrain opaque, the layers far to near, then the
            // terrain over them; any other, the terrain alone, as always.
            let distant = |pass: &mut wgpu::RenderPass, pipeline| {
                pass.set_pipeline(pipeline);
                for &(k, i) in &self.distant_draws[plan.distant.clone()] {
                    pass.set_bind_group(1, &self.distant[k].bind, &[]);
                    pass.draw(0..4, i..i + 1);
                }
            };
            let quad = terrain..terrain + 1;
            match (lit, &self.terrain) {
                (Some((state, _)), Some(t)) => {
                    if let Some(tex) = &state.textures {
                        if t.see_through {
                            pass.set_pipeline(&state.backing_pipeline);
                            pass.set_bind_group(1, &t.bind, &[]);
                            pass.set_bind_group(2, &tex.terrain_bind, &[]);
                            pass.draw(0..4, quad.clone());
                            distant(&mut pass, &state.distant_pipeline);
                        }
                        pass.set_pipeline(&state.terrain_pipeline);
                        pass.set_bind_group(1, &t.bind, &[]);
                        pass.set_bind_group(2, &tex.terrain_bind, &[]);
                        pass.draw(0..4, quad);
                    }
                }
                (None, Some(t)) => {
                    if t.see_through {
                        pass.set_pipeline(&self.backing_pipeline);
                        pass.set_bind_group(1, &t.bind, &[]);
                        pass.draw(0..4, quad.clone());
                        distant(&mut pass, &self.distant_pipeline);
                    }
                    pass.set_pipeline(&self.world_pipeline);
                    pass.set_bind_group(1, &t.bind, &[]);
                    pass.draw(0..4, quad);
                }
                (_, None) => {}
            }
            let end = plan.sprites.end;
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
            drop(pass);
            if let (Some((state, _)), Some(lt)) = (lit, world.lit.as_ref()) {
                // Cleared every lit frame: only this frame's worms occlude.
                let mut pass =
                    begin(&mut encoder, "olr-render occluders", &lt.occluder_view, group(PassGroup::World));
                if let Some((_, bind)) = &self.atlas
                    && !plan.occluders.is_empty()
                {
                    pass.set_pipeline(&state.occluder_pipeline);
                    pass.set_bind_group(0, &lt.occluder_globals_bind, &[]);
                    pass.set_bind_group(1, bind, &[]);
                    pass.set_vertex_buffer(0, self.instance_buffer.slice(..));
                    pass.draw(0..4, plan.occluders.clone());
                }
            }
        }
        // The glow light, once a frame whatever the views (issue #26): its
        // occluder map is the level's, each view's copied in.
        if let (Some(look), Some(state)) = (look, self.look.as_ref())
            && let Some(tex) = &state.textures
            && !views.is_empty()
        {
            let clear = group(PassGroup::Emission);
            drop(begin(&mut encoder, "olr-render glow light occluders", &tex.gi_occluder_view, clear));
            for (plan, world) in views_drawn.clone() {
                if let Some(lt) = &world.lit {
                    copy_occluders(&mut encoder, &lt.occluders, &plan.frame, &tex.gi_occluders);
                }
            }
            let casting = !look.groups.is_empty() || state.emitter_count > 0;
            let discs = (&state.emitters, state.emitter_count);
            if tex.gi.encode(&mut encoder, &state.gi, casting, &tex.gi_occluder_bind, discs, timer) {
                self.glow_light_steps += 1;
            }
        }
        for (_, world) in views_drawn.clone() {
            let lit_target = look.and(self.look.as_ref()).and(world.shade.as_ref()).and(world.lit.as_ref());
            let (Some(lt), Some(state)) = (lit_target, self.look.as_ref()) else { continue };
            let full = |encoder: &mut wgpu::CommandEncoder, g, view, pipeline, bind| {
                let mut pass = begin(encoder, "olr-render look", view, group(g));
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bind, &[]);
                pass.draw(0..3, 0..1);
            };
            // The ambient occlusion's rows (issue #10), only when the level has it.
            if look.is_some_and(|l| l.occlusion.is_some_and(|o| o.strength > 0)) {
                full(
                    &mut encoder,
                    PassGroup::Occlusion,
                    &lt.occlusion_view,
                    &state.occlusion_pipeline,
                    &lt.occlusion_bind,
                );
            }
            full(&mut encoder, PassGroup::Sum, &lt.views[0], &state.sum_pipeline, &lt.binds[0]);
            {
                // The across soak, and the rows for the deeper rock.
                let (views, g) = ([&lt.views[1], &lt.bloom_views[1]], group(PassGroup::Soak));
                let mut pass = begin_two(&mut encoder, "olr-render look", views[0], views[1], g);
                pass.set_pipeline(&state.soak_h_pipeline);
                pass.set_bind_group(0, &lt.soak_binds[0], &[]);
                pass.draw(0..3, 0..1);
            }
            full(&mut encoder, PassGroup::Soak, &lt.views[2], &state.soak_v_pipeline, &lt.soak_binds[1]);
            full(&mut encoder, PassGroup::Blur, &lt.views[0], &state.blur_h_pipeline, &lt.binds[1]);
            {
                // The toned light and the light above 1 (plan 15c's bloom).
                let (views, g) = ([&lt.views[1], &lt.bloom_views[0]], group(PassGroup::Blur));
                let mut pass = begin_two(&mut encoder, "olr-render look", views[0], views[1], g);
                pass.set_pipeline(&state.blur_v_pipeline);
                pass.set_bind_group(0, &lt.binds[2], &[]);
                pass.draw(0..3, 0..1);
            }
            full(
                &mut encoder,
                PassGroup::Bloom,
                &lt.bloom_views[1],
                &state.bloom_h_pipeline,
                &lt.bloom_binds[0],
            );
            full(
                &mut encoder,
                PassGroup::Bloom,
                &lt.bloom_views[0],
                &state.bloom_v_pipeline,
                &lt.bloom_binds[1],
            );
            full(
                &mut encoder,
                PassGroup::Composite,
                &lt.lit_view,
                &state.composite_pipeline,
                &lt.composite_bind,
            );
        }
        // Each view's upscale into its area only; the first clears the
        // whole target when asked.
        let last = views.len().saturating_sub(1);
        for (i, (v, (_, world))) in views.iter().zip(views_drawn.clone()).enumerate() {
            let load =
                if clear && i == 0 { wgpu::LoadOp::Clear(wgpu::Color::BLACK) } else { wgpu::LoadOp::Load };
            let stamp = stamp(PassGroup::Upscale, None, (i == last).then_some(1));
            let mut pass = begin_with(&mut encoder, "olr-render upscale", target, load, stamp);
            let lit = look.and(self.look.as_ref()).and(world.shade.as_ref()).and(world.lit.as_ref());
            if let Some([x, y, w, h]) = clip(v.area, out) {
                pass.set_scissor_rect(x, y, w, h);
                pass.set_pipeline(&self.upscale_pipeline);
                pass.set_bind_group(0, lit.map_or(&world.bind, |lt| &lt.upscale_bind), &[]);
                pass.draw(0..3, 0..1);
            }
        }
        if views.is_empty() && clear {
            drop(begin(&mut encoder, "olr-render clear", target, None));
        }
        if let (Some(p), Some((v, world))) = (self.probe.as_mut(), views.first().zip(self.targets.first())) {
            p.encode(
                &self.device,
                &self.queue,
                &mut encoder,
                (v.area[2], v.area[3]),
                &world.view,
                self.timer.as_ref(),
            );
        }
        let probing = self.probe.as_ref().is_some_and(|p| p.probe.passes > 0);
        if let Some(t) = self.timer.as_mut()
            && !views.is_empty()
        {
            t.resolve(&mut encoder, probing);
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Makes view `i`'s world target fit `frame` (a new one only when the
    /// size changes, so steady frames allocate nothing). Views are made in
    /// order: `i` is at most the targets made.
    fn ensure_target(&mut self, i: usize, frame: &WorldFrame) {
        let size = frame.target_size();
        let lit = self.look.is_some();
        let pad = 2 * LIGHT_PAD;
        let grid = (frame.size.0 + pad, frame.size.1 + pad);
        let grid_fits = |t: &Target| t.lit.as_ref().is_none_or(|l| l.grid == grid);
        if self
            .targets
            .get(i)
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
        let globals = self.uniform("olr-render globals", 16);
        let globals_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render globals"),
            layout: &self.globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let upscale_uniform = self.uniform("olr-render upscale", 48);
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render upscale"),
            layout: &self.upscale_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: upscale_uniform.as_entire_binding() },
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
                (&self.upscale_layout, &upscale_uniform, &self.sampler),
            )),
            _ => None,
        };
        let target = Target {
            frame_size: size,
            size: clamped,
            view,
            bind,
            globals,
            globals_bind,
            upscale_uniform,
            shade,
            lit,
            origin: frame.origin,
        };
        match self.targets.get_mut(i) {
            Some(t) => *t = target,
            None => self.targets.push(target),
        }
    }
}

/// `area` (`[x, y, w, h]`) clipped to a target of `size`; `None` when
/// nothing is left.
fn clip([x, y, w, h]: [u32; 4], size: (u32, u32)) -> Option<[u32; 4]> {
    let (x0, y0) = (x.min(size.0), y.min(size.1));
    let (x1, y1) = (x.saturating_add(w).min(size.0), y.saturating_add(h).min(size.1));
    (x1 > x0 && y1 > y0).then(|| [x0, y0, x1 - x0, y1 - y0])
}

/// Copies the part of a view's occluder map `from` (over its light grid,
/// `frame`'s plus [`LIGHT_PAD`]) inside the level into the glow light's
/// level-wide occluder map `to` (issue #26).
fn copy_occluders(
    encoder: &mut wgpu::CommandEncoder,
    from: &wgpu::Texture,
    frame: &WorldFrame,
    to: &wgpu::Texture,
) {
    let pad = LIGHT_PAD as i64;
    let origin = (i64::from(frame.origin.0) - pad, i64::from(frame.origin.1) - pad);
    let span = |o: i64, len: u32, level: u32| {
        let (a, b) = (o.max(0), (o + i64::from(len)).min(i64::from(level)));
        (b > a).then(|| ((a - o) as u32, a as u32, (b - a) as u32))
    };
    let (Some((sx, dx, w)), Some((sy, dy, h))) =
        (span(origin.0, from.width(), to.width()), span(origin.1, from.height(), to.height()))
    else {
        return;
    };
    let at = |texture, x, y| wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d { x, y, z: 0 },
        aspect: wgpu::TextureAspect::All,
    };
    encoder.copy_texture_to_texture(
        at(from, sx, sy),
        at(to, dx, dy),
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
}

/// A render pass that clears `view` to black, writing `timestamps` (GPU
/// timing, bench only) when given.
pub(super) fn begin<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'e> {
    begin_with(encoder, label, view, wgpu::LoadOp::Clear(wgpu::Color::BLACK), timestamps)
}

/// [`begin`] that starts from `load` (a clear, or what `view` holds).
fn begin_with<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    label: &str,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
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
