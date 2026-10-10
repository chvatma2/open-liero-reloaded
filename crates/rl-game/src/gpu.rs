//! The window's wgpu device, surface and renderers: `rl-render` for the
//! world and `egui-wgpu` for the overlay, drawn on the main thread only
//! (under WSL's d3d12 driver a thread that used the device crashes when it
//! exits). Screenshots draw the same frame into a texture and read it back.

use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use rl_data::image::RgbaImage;
use rl_render::{RenderSettings, Renderer};
use rl_scene::DrawList;
use winit::window::Window;

use crate::surface::{Acquire, Surface, SurfaceState};

/// The overlay's paint jobs for one frame.
pub struct OverlayPaint<'a> {
    pub jobs: &'a [egui::ClippedPrimitive],
    pub textures: &'a egui::TexturesDelta,
    pub pixels_per_point: f32,
}

pub struct Gpu {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    config: wgpu::SurfaceConfiguration,
    state: SurfaceState,
    pub renderer: Renderer,
    egui: egui_wgpu::Renderer,
    /// The adapter, backend and present mode, for logs and the debug panel.
    pub info: String,
}

/// The surface format: a gamma-space one when the surface has it (egui
/// wants one; `rl-render` writes the same bytes either way).
fn pick_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    formats.iter().copied().find(|f| !f.is_srgb()).or_else(|| formats.first().copied())
}

/// Mailbox when the surface has it (no tearing, no queueing), else FIFO
/// (VSync; spec §8.6). `uncapped` prefers Immediate (no VSync, to measure
/// the frame rate), then Mailbox.
fn pick_present_mode(modes: &[wgpu::PresentMode], uncapped: bool) -> wgpu::PresentMode {
    use wgpu::PresentMode as P;
    let order: &[P] = if uncapped { &[P::Immediate, P::Mailbox] } else { &[P::Mailbox] };
    order.iter().copied().find(|m| modes.contains(m)).unwrap_or(P::Fifo)
}

impl Gpu {
    /// Opens a device that can present to `window` with `backends`.
    pub fn new(
        window: Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
        backends: wgpu::Backends,
        settings: RenderSettings,
        uncapped: bool,
    ) -> anyhow::Result<Gpu> {
        let mut desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display));
        desc.backends = backends;
        let instance = wgpu::Instance::new(desc);
        let surface = instance.create_surface(window.clone()).context("creating the window surface")?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .with_context(|| format!("no GPU adapter for {backends:?} can draw to the window"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("rl-game"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .context("opening the GPU device")?;
        let caps = surface.get_capabilities(&adapter);
        let format = pick_format(&caps.formats).context("the surface has no format")?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("the surface does not work with the adapter")?;
        config.format = format;
        config.present_mode = pick_present_mode(&caps.present_modes, uncapped);
        config.desired_maximum_frame_latency = 2;
        let a = adapter.get_info();
        let info = format!(
            "{} ({:?}, {:?}), {:?}, {:?}",
            a.name, a.backend, a.device_type, config.present_mode, format
        );
        let egui = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        let renderer = Renderer::new(device, queue, format, settings);
        let state = SurfaceState::new((size.width, size.height));
        Ok(Gpu { window, instance, surface, adapter, config, state, renderer, egui, info })
    }

    /// The window is now `size` pixels (0 when minimised).
    pub fn resize(&mut self, size: (u32, u32)) {
        self.state.resize(size);
    }

    /// The size frames are drawn at.
    pub fn size(&self) -> (u32, u32) {
        self.state.size()
    }

    /// Draws `list` and the overlay into the window and presents it.
    /// Returns how long it waited for the surface (acquiring and presenting,
    /// where VSync blocks), or `None` when no frame was presented (the
    /// window is minimised or the surface keeps failing).
    pub fn present(&mut self, list: &DrawList, overlay: &OverlayPaint) -> Option<Duration> {
        let t0 = Instant::now();
        let mut target = WindowSurface {
            window: &self.window,
            instance: &self.instance,
            adapter: &self.adapter,
            surface: &mut self.surface,
            device: self.renderer.device(),
            config: &mut self.config,
        };
        let frame = self.state.frame(&mut target)?;
        let acquired = t0.elapsed();
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let size = (frame.texture.width(), frame.texture.height());
        self.draw(list, overlay, &view, size);
        self.window.pre_present_notify();
        let t1 = Instant::now();
        self.renderer.queue().present(frame);
        Some(acquired + t1.elapsed())
    }

    /// The world, then the overlay over it, into `view`.
    fn draw(&mut self, list: &DrawList, overlay: &OverlayPaint, view: &wgpu::TextureView, size: (u32, u32)) {
        self.renderer.render(list, view, size);
        let (device, queue) = (self.renderer.device().clone(), self.renderer.queue().clone());
        for (id, deltas) in &overlay.textures.set {
            for delta in deltas {
                self.egui.update_texture(&device, &queue, *id, delta);
            }
        }
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.0, size.1],
            pixels_per_point: overlay.pixels_per_point,
        };
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rl-game overlay") });
        let extra = self.egui.update_buffers(&device, &queue, &mut encoder, overlay.jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("rl-game overlay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.egui.render(&mut pass, overlay.jobs, &screen);
        }
        queue.submit(extra.into_iter().chain([encoder.finish()]));
        for id in &overlay.textures.free {
            self.egui.free_texture(id);
        }
    }

    /// Draws the frame the window shows into a texture of the window's
    /// size and writes it to `path` as a PNG.
    pub fn screenshot(&mut self, list: &DrawList, overlay: &OverlayPaint, path: &Path) -> anyhow::Result<()> {
        let (w, h) = self.state.size();
        let size = (w.max(1), h.max(1));
        let texture = self.renderer.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("rl-game screenshot"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        // The overlay's textures were uploaded with the window's frame.
        let paint = OverlayPaint { textures: &egui::TexturesDelta::default(), ..*overlay };
        self.draw(list, &paint, &view, size);
        let img = self.read(&texture)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, rl_pack::image_io::encode_rgba(&img))
            .with_context(|| format!("{}", path.display()))
    }

    /// Reads a texture of the surface format back as RGBA.
    fn read(&self, texture: &wgpu::Texture) -> anyhow::Result<RgbaImage> {
        let (device, queue) = (self.renderer.device(), self.renderer.queue());
        let (w, h) = (texture.width(), texture.height());
        let row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rl-game screenshot"),
            size: u64::from(row) * u64::from(h),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).context("waiting for the GPU")?;
        rx.recv().context("the readback never finished")?.context("mapping the readback")?;
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut pixels = Vec::with_capacity((w * h * 4) as usize);
        {
            let mapped = buffer.get_mapped_range(..).context("mapped above")?;
            for line in mapped.chunks_exact(row as usize) {
                for px in line[..(w * 4) as usize].as_chunks::<4>().0 {
                    let (r, b) = if bgra { (px[2], px[0]) } else { (px[0], px[2]) };
                    // The window is opaque: what a screenshot shows.
                    pixels.extend_from_slice(&[r, px[1], b, 255]);
                }
            }
        }
        buffer.unmap();
        RgbaImage::from_raw(w, h, pixels).context("w x h pixels")
    }
}

/// The window's wgpu surface as the state machine sees it.
struct WindowSurface<'a> {
    window: &'a Arc<Window>,
    instance: &'a wgpu::Instance,
    adapter: &'a wgpu::Adapter,
    surface: &'a mut wgpu::Surface<'static>,
    device: &'a wgpu::Device,
    config: &'a mut wgpu::SurfaceConfiguration,
}

impl Surface for WindowSurface<'_> {
    type Frame = wgpu::SurfaceTexture;

    fn configure(&mut self, size: (u32, u32)) {
        self.config.width = size.0;
        self.config.height = size.1;
        self.surface.configure(self.device, self.config);
    }

    fn acquire(&mut self) -> Acquire<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => Acquire::Frame(t),
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => Acquire::Suboptimal(t),
            wgpu::CurrentSurfaceTexture::Outdated => Acquire::Outdated,
            wgpu::CurrentSurfaceTexture::Lost => Acquire::Lost,
            _ => Acquire::Skip,
        }
    }

    fn recreate(&mut self) {
        match self.instance.create_surface(self.window.clone()) {
            Ok(s) if self.adapter.is_surface_supported(&s) => *self.surface = s,
            Ok(_) => eprintln!("warning: the new window surface is not supported by the adapter"),
            Err(e) => eprintln!("warning: recreating the window surface: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_format_and_present_mode_choices() {
        use wgpu::TextureFormat as F;
        assert_eq!(pick_format(&[F::Bgra8UnormSrgb, F::Bgra8Unorm]), Some(F::Bgra8Unorm));
        assert_eq!(pick_format(&[F::Rgba8UnormSrgb]), Some(F::Rgba8UnormSrgb));
        assert_eq!(pick_format(&[]), None);
        use wgpu::PresentMode as P;
        assert_eq!(pick_present_mode(&[P::Fifo, P::Mailbox, P::Immediate], false), P::Mailbox);
        assert_eq!(pick_present_mode(&[P::Fifo, P::Immediate], false), P::Fifo);
        assert_eq!(pick_present_mode(&[P::Fifo, P::Mailbox, P::Immediate], true), P::Immediate);
        assert_eq!(pick_present_mode(&[P::Fifo, P::Mailbox], true), P::Mailbox);
        assert_eq!(pick_present_mode(&[P::Fifo], true), P::Fifo);
    }
}
