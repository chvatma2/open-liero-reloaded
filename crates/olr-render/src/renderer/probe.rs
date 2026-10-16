//! The shader-headroom probe (bench only, plan 8): after a frame, an
//! untimed seed pass copies the drawn frame (the world target, stretched)
//! into the probe's first source texture, then `passes` full-screen
//! fragment passes, each a `taps`-tap blur of the previous pass's output,
//! at the frame's size, into two textures of its own (ping-pong). The
//! passes blur a real picture: a blank one, which GPUs compress, would
//! make them look cheaper than a real effect. The picture never changes;
//! the probe's GPU time per pass answers "how many full-screen shaders fit
//! in a frame".

use super::pass::begin;
use super::timing::Timer;
use super::{TEXTURE, UNIFORM, WORLD_FORMAT, entry, pipeline, texture};

/// The most passes a probe runs.
pub const MAX_PROBE_PASSES: u32 = 64;
/// The most taps a pass reads.
pub const MAX_PROBE_TAPS: u32 = 64;

/// A probe: `passes` passes of a `taps`-tap blur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub passes: u32,
    pub taps: u32,
}

impl Probe {
    /// At most [`MAX_PROBE_PASSES`] passes, 1 to [`MAX_PROBE_TAPS`] taps.
    pub fn clamped(self) -> Probe {
        Probe { passes: self.passes.min(MAX_PROBE_PASSES), taps: self.taps.clamp(1, MAX_PROBE_TAPS) }
    }
}

/// The probe's textures, made for one size.
struct Targets {
    size: (u32, u32),
    textures: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    /// `binds[j]` reads `views[j]`.
    binds: [wgpu::BindGroup; 2],
}

pub(crate) struct ProbeState {
    pub probe: Probe,
    pipeline: wgpu::RenderPipeline,
    /// The seed pass: the drawn frame into the first pass's source.
    seed: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    targets: Option<Targets>,
}

impl ProbeState {
    pub fn new(device: &wgpu::Device, probe: Probe) -> ProbeState {
        let frag = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("olr-render probe"),
            entries: &[
                entry(0, frag, UNIFORM),
                entry(1, frag, TEXTURE),
                entry(2, frag, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
            ],
        });
        let make = |fs: &str| {
            pipeline(
                device,
                "olr-render probe",
                include_str!("../probe.wgsl"),
                ("vs_full", fs),
                &[Some(&layout)],
                &[],
                WORLD_FORMAT,
                None,
                wgpu::PrimitiveTopology::TriangleList,
            )
        };
        let (pipeline, seed) = (make("fs_probe"), make("fs_seed"));
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render probe"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("olr-render probe"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        ProbeState { probe, pipeline, seed, layout, uniform, sampler, targets: None }
    }

    /// Makes the two textures fit `size` (new ones only when it changes).
    fn ensure(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        // COPY_SRC: tests read the last pass's output back (`Renderer::probe_output`).
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        let textures = [0, 1].map(|_| texture(device, "olr-render probe", size, usage));
        let views = [0, 1].map(|j| textures[j].create_view(&wgpu::TextureViewDescriptor::default()));
        let binds = [0, 1].map(|j| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("olr-render probe"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[j]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
        });
        self.targets = Some(Targets { size, textures, views, binds });
    }

    /// The texture the last frame's last pass wrote (bench and tests only).
    pub fn output(&self) -> Option<&wgpu::Texture> {
        let t = self.targets.as_ref()?;
        Some(&t.textures[(self.probe.passes.max(1) - 1) as usize % 2])
    }

    /// Encodes the seed (from `frame`, the drawn frame; even with 0
    /// passes, so a 0-pass baseline's wall time holds it too) and the
    /// passes for an output of `size`; with `timer`, slot 2 at the first
    /// pass's start and 3 at the last pass's end (the seed is untimed).
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
        frame: &wgpu::TextureView,
        timer: Option<&Timer>,
    ) {
        let n = self.probe.passes;
        let size = (size.0.max(1), size.1.max(1));
        self.ensure(device, size);
        let f = |v: f32| v.to_bits();
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::cast_slice(&[f(size.0 as f32), f(size.1 as f32), self.probe.taps, 0]),
        );
        let t = self.targets.as_ref().expect("ensured above");
        let seed = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("olr-render probe seed"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(frame) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        {
            // Into `views[1]`, which the first pass reads.
            let mut pass = begin(encoder, "olr-render probe seed", &t.views[1], None);
            pass.set_pipeline(&self.seed);
            pass.set_bind_group(0, &seed, &[]);
            pass.draw(0..3, 0..1);
        }
        for i in 0..n {
            // Only the first pass and the last write a slot; a middle pass
            // asks for none (wgpu refuses timestamp writes with neither).
            let timestamps =
                timer.filter(|_| i == 0 || i + 1 == n).map(|tm| wgpu::RenderPassTimestampWrites {
                    query_set: &tm.set,
                    beginning_of_pass_write_index: (i == 0).then_some(2),
                    end_of_pass_write_index: (i + 1 == n).then_some(3),
                });
            let mut pass = begin(encoder, "olr-render probe", &t.views[(i % 2) as usize], timestamps);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &t.binds[((i + 1) % 2) as usize], &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_are_bounded() {
        assert_eq!(Probe { passes: 1000, taps: 0 }.clamped(), Probe { passes: MAX_PROBE_PASSES, taps: 1 });
        assert_eq!(Probe { passes: 4, taps: 9 }.clamped(), Probe { passes: 4, taps: 9 });
        assert_eq!(Probe { passes: 1, taps: 500 }.clamped().taps, MAX_PROBE_TAPS);
    }
}
