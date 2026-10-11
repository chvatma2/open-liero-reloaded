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

use crate::latency::PresentTiming;
use crate::surface::{Acquire, Surface, SurfaceState};

/// A surface texture to draw the next frame into.
pub struct Acquired {
    frame: wgpu::SurfaceTexture,
    /// How long acquiring it waited.
    wait: Duration,
}

/// Where egui's texture changes go (`egui_wgpu::Renderer` on the device;
/// a fake in tests).
pub trait TextureSink {
    fn set(&mut self, id: egui::TextureId, delta: &egui::epaint::ImageDelta);
    fn free(&mut self, id: egui::TextureId);
}

/// Applies a frame's egui texture changes around `draw`, whether or not it
/// draws anything: new textures and patches first, frees after, leaving
/// `delta` empty (egui checks that every delta was handled). egui hands
/// each change out once, so a frame that is not drawn (a minimised window,
/// a skipped surface) must still apply them, or later patches refer to
/// textures never allocated (egui-wgpu panics).
pub fn with_textures<S: TextureSink, R>(
    sink: &mut S,
    delta: &mut egui::TexturesDelta,
    draw: impl FnOnce(&mut S) -> R,
) -> R {
    // Each id's deltas apply in order; ids are independent of each other.
    for (id, deltas) in delta.set.drain() {
        for d in &deltas {
            sink.set(id, d);
        }
    }
    let drawn = draw(sink);
    for id in delta.free.drain() {
        sink.free(id);
    }
    drawn
}

/// `size` within the device's largest texture side (`max`) and at least 1,
/// so a huge or spanned window never asks for a surface the device refuses.
pub fn clamp_size(size: (u32, u32), max: u32) -> (u32, u32) {
    let max = max.max(1);
    (size.0.clamp(1, max), size.1.clamp(1, max))
}

/// The overlay's paint jobs for one frame.
pub struct OverlayPaint<'a> {
    pub jobs: &'a [egui::ClippedPrimitive],
    pub pixels_per_point: f32,
}

pub struct Gpu {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    config: wgpu::SurfaceConfiguration,
    /// The present modes the surface offers.
    present_modes: Vec<wgpu::PresentMode>,
    /// No VSync wanted (`--uncapped`, the options' frame rate).
    uncapped: bool,
    state: SurfaceState,
    pub renderer: Renderer,
    /// The pixels at the bottom the HUD's bar takes (`app::bar_pixels`):
    /// the world is drawn above them, never under the opaque bar.
    pub bar: u32,
    egui: egui_wgpu::Renderer,
    /// The adapter, backend and present mode, for logs and the debug panel.
    pub info: String,
}

/// The surface format: a gamma-space one when the surface has it (egui
/// wants one; `rl-render` writes the same bytes either way).
fn pick_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    use wgpu::TextureFormat as F;
    // 8-bit RGBA first (the screenshot readback reads 4 bytes a pixel),
    // gamma-space before sRGB.
    let preferred = [F::Bgra8Unorm, F::Rgba8Unorm, F::Bgra8UnormSrgb, F::Rgba8UnormSrgb];
    preferred.into_iter().find(|f| formats.contains(f)).or_else(|| formats.first().copied())
}

/// Mailbox when the surface has it (no tearing, no queueing), else FIFO
/// (VSync; spec §8.6). `uncapped` prefers Immediate (no VSync, to measure
/// the frame rate), then Mailbox.
fn pick_present_mode(modes: &[wgpu::PresentMode], uncapped: bool) -> wgpu::PresentMode {
    use wgpu::PresentMode as P;
    let order: &[P] = if uncapped { &[P::Immediate, P::Mailbox] } else { &[P::Mailbox] };
    order.iter().copied().find(|m| modes.contains(m)).unwrap_or(P::Fifo)
}

impl TextureSink for Gpu {
    fn set(&mut self, id: egui::TextureId, delta: &egui::epaint::ImageDelta) {
        self.egui.update_texture(self.renderer.device(), self.renderer.queue(), id, delta);
    }

    fn free(&mut self, id: egui::TextureId) {
        self.egui.free_texture(&id);
    }
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
        let inner = window.inner_size();
        let max = device.limits().max_texture_dimension_2d;
        let first = clamp_size((inner.width, inner.height), max);
        let mut config = surface
            .get_default_config(&adapter, first.0, first.1)
            .context("the surface does not work with the adapter")?;
        config.format = format;
        config.present_mode = pick_present_mode(&caps.present_modes, uncapped);
        // One frame in flight: the least input lag where the backend honours
        // it (Vulkan, DX12; wgpu's GL backend ignores it). Frames are cheap,
        // so it costs no throughput (plan 5 final review, `latency.rs`).
        config.desired_maximum_frame_latency = 1;
        let a = adapter.get_info();
        let info = format!(
            "{} ({:?}, {:?}), {:?}, {:?}",
            a.name, a.backend, a.device_type, config.present_mode, format
        );
        let egui = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        let renderer = Renderer::new(device, queue, format, settings);
        let zero = inner.width == 0 || inner.height == 0;
        let state = SurfaceState::new(if zero { (inner.width, inner.height) } else { first });
        let present_modes = caps.present_modes;
        Ok(Gpu {
            window,
            instance,
            surface,
            adapter,
            config,
            present_modes,
            uncapped,
            state,
            renderer,
            bar: 0,
            egui,
            info,
        })
    }

    /// Draws to `window` from now on, a window made again (`app.rs`,
    /// `window::Plan::Recreate`): a new surface on the same instance,
    /// adapter and device, so the renderer, its textures and egui's stay.
    /// The surface keeps its format (the pipelines were made for it) and
    /// picks its present mode as [`Gpu::new`] did. On an error nothing
    /// changed: the old window's surface is still the one drawn to.
    ///
    /// The old surface is dropped here, while the caller still holds the
    /// old window, so it never outlives its window; all on the main
    /// thread (module docs).
    pub fn rebind(&mut self, window: Arc<Window>) -> anyhow::Result<()> {
        let surface = self.instance.create_surface(window.clone()).context("creating the window surface")?;
        anyhow::ensure!(
            self.adapter.is_surface_supported(&surface),
            "the adapter cannot draw to the new window"
        );
        let caps = surface.get_capabilities(&self.adapter);
        anyhow::ensure!(
            caps.formats.contains(&self.config.format),
            "the new window's surface has no {:?}",
            self.config.format
        );
        self.config.present_mode = pick_present_mode(&caps.present_modes, self.uncapped);
        self.present_modes = caps.present_modes;
        self.surface = surface;
        self.window = window;
        let inner = self.window.inner_size();
        let zero = inner.width == 0 || inner.height == 0;
        let size = (inner.width, inner.height);
        self.state = SurfaceState::new(if zero { size } else { clamp_size(size, self.max_side()) });
        Ok(())
    }

    /// The window is now `size` pixels (0 when minimised).
    pub fn resize(&mut self, size: (u32, u32)) {
        // Minimised stays 0 (no frames); otherwise within the device's limit.
        let size = if size.0 == 0 || size.1 == 0 { size } else { clamp_size(size, self.max_side()) };
        self.state.resize(size);
    }

    /// VSync (`false`) or as fast as the GPU draws: the surface is
    /// configured again before the next frame when its present mode
    /// changes (never under WSLg, which offers FIFO only).
    pub fn set_uncapped(&mut self, uncapped: bool) {
        self.uncapped = uncapped;
        let mode = pick_present_mode(&self.present_modes, uncapped);
        if mode != self.config.present_mode {
            self.config.present_mode = mode;
            self.state.reconfigure();
        }
    }

    /// The window is hidden or shown again (winit's `Occluded`).
    pub fn set_occluded(&mut self, occluded: bool) {
        self.state.set_occluded(occluded);
    }

    /// Whether frames can be drawn now.
    pub fn drawable(&self) -> bool {
        self.state.drawable()
    }

    /// The device's largest texture side.
    fn max_side(&self) -> u32 {
        self.renderer.device().limits().max_texture_dimension_2d
    }

    /// The size frames are drawn at.
    pub fn size(&self) -> (u32, u32) {
        self.state.size()
    }

    /// Waits for the next surface texture to draw into: none while the
    /// window is minimised or the surface keeps failing. Acquire before
    /// reading input, so the wait for VSync is not between reading input
    /// and drawing it (`latency.rs`).
    pub fn acquire(&mut self) -> Option<Acquired> {
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
        Some(Acquired { frame, wait: t0.elapsed() })
    }

    /// Draws `list` (none: black, as behind the menus) and the overlay into `frame` and presents it, applying
    /// the overlay's texture changes (`textures`, left empty) whether or
    /// not there is a frame. Returns how long the swap chain made the frame
    /// wait, or `None` when there was no frame.
    pub fn present(
        &mut self,
        frame: Option<Acquired>,
        list: Option<&DrawList>,
        overlay: &OverlayPaint,
        textures: &mut egui::TexturesDelta,
    ) -> Option<PresentTiming> {
        with_textures(self, textures, |gpu| frame.map(|f| gpu.present_frame(f, list, overlay)))
    }

    /// [`Gpu::present`] once the overlay's textures are up to date.
    fn present_frame(
        &mut self,
        acquired: Acquired,
        list: Option<&DrawList>,
        overlay: &OverlayPaint,
    ) -> PresentTiming {
        let frame = acquired.frame;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let size = (frame.texture.width(), frame.texture.height());
        self.draw(list, overlay, &view, size);
        self.window.pre_present_notify();
        let t1 = Instant::now();
        self.renderer.queue().present(frame);
        PresentTiming { acquire: acquired.wait, present: t1.elapsed() }
    }

    /// The world (none: black) above the HUD's bar, then the overlay over
    /// it, into `view`.
    fn draw(
        &mut self,
        list: Option<&DrawList>,
        overlay: &OverlayPaint,
        view: &wgpu::TextureView,
        size: (u32, u32),
    ) {
        if let Some(list) = list {
            // The playfield: the window above the HUD's bar.
            self.renderer.render(list, view, crate::app::playfield(size, self.bar));
        }
        let load = if list.is_some() { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) };
        let (device, queue) = (self.renderer.device().clone(), self.renderer.queue().clone());
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
                        ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.egui.render(&mut pass, overlay.jobs, &screen);
        }
        queue.submit(extra.into_iter().chain([encoder.finish()]));
    }

    /// Draws the frame the window shows into a texture of the window's
    /// size and writes it to `path` as a PNG; gives the picture.
    /// Call it after [`Gpu::present`], which applies the overlay's texture
    /// changes.
    pub fn screenshot(
        &mut self,
        list: Option<&DrawList>,
        overlay: &OverlayPaint,
        path: &Path,
    ) -> anyhow::Result<RgbaImage> {
        let size = clamp_size(self.state.size(), self.max_side());
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
        // `present` applied this frame's texture changes, drawn or not.
        self.draw(list, overlay, &view, size);
        let img = self.read(&texture)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, rl_pack::image_io::encode_rgba(&img))
            .with_context(|| format!("{}", path.display()))?;
        Ok(img)
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
        let (w, h) = clamp_size(size, self.device.limits().max_texture_dimension_2d);
        self.config.width = w;
        self.config.height = h;
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
        // 8-bit RGBA layouts first: the screenshot readback assumes 4 bytes a pixel.
        assert_eq!(pick_format(&[F::Rgba16Float, F::Rgb10a2Unorm, F::Rgba8Unorm]), Some(F::Rgba8Unorm));
        assert_eq!(pick_format(&[F::Rgba16Float, F::Bgra8UnormSrgb]), Some(F::Bgra8UnormSrgb));
        use wgpu::PresentMode as P;
        assert_eq!(pick_present_mode(&[P::Fifo, P::Mailbox, P::Immediate], false), P::Mailbox);
        assert_eq!(pick_present_mode(&[P::Fifo, P::Immediate], false), P::Fifo);
        assert_eq!(pick_present_mode(&[P::Fifo, P::Mailbox, P::Immediate], true), P::Immediate);
        assert_eq!(pick_present_mode(&[P::Fifo, P::Mailbox], true), P::Mailbox);
        assert_eq!(pick_present_mode(&[P::Fifo], true), P::Fifo);
    }

    #[derive(Default)]
    struct Sink(Vec<String>);

    impl TextureSink for Sink {
        fn set(&mut self, id: egui::TextureId, _: &egui::epaint::ImageDelta) {
            self.0.push(format!("set {id:?}"));
        }
        fn free(&mut self, id: egui::TextureId) {
            self.0.push(format!("free {id:?}"));
        }
    }

    #[test]
    fn texture_changes_apply_even_without_a_frame() {
        let image = egui::epaint::ImageDelta::full(
            egui::ColorImage::filled([2, 2], egui::Color32::WHITE),
            egui::TextureOptions::default(),
        );
        let mut delta = egui::TexturesDelta::default();
        delta.set.insert(egui::TextureId::Managed(0), [image].into_iter().collect());
        delta.free.insert(egui::TextureId::Managed(1));
        // No frame acquired (minimised, occluded, skipped): still applied.
        let mut sink = Sink::default();
        let drawn: Option<()> = with_textures(&mut sink, &mut delta, |_| None);
        assert_eq!(drawn, None);
        assert_eq!(sink.0, ["set Managed(0)", "free Managed(1)"]);
        assert!(delta.is_empty(), "every change handled");
        // A drawn frame: set before drawing, free after.
        let image = egui::epaint::ImageDelta::full(
            egui::ColorImage::filled([2, 2], egui::Color32::WHITE),
            egui::TextureOptions::default(),
        );
        delta.set.insert(egui::TextureId::Managed(0), [image].into_iter().collect());
        delta.free.insert(egui::TextureId::Managed(1));
        let mut sink = Sink::default();
        with_textures(&mut sink, &mut delta, |s| s.0.push("draw".into()));
        assert_eq!(sink.0, ["set Managed(0)", "draw", "free Managed(1)"]);
    }

    #[test]
    fn surface_size_is_clamped_to_the_device_limit() {
        assert_eq!(clamp_size((1920, 1080), 8192), (1920, 1080));
        assert_eq!(clamp_size((10000, 3000), 8192), (8192, 3000));
        assert_eq!(clamp_size((20000, 20000), 2048), (2048, 2048));
        assert_eq!(clamp_size((0, 5), 2048), (1, 5));
    }
}
