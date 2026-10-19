//! Rendering without a window: for goldens, tests and `olr-cli render`.

use std::path::Path;
use std::sync::mpsc;

use olr_data::image::RgbaImage;
use olr_scene::DrawList;

use crate::renderer::{RenderSettings, Renderer};

/// The output format of offscreen renders (PNG bytes as they are).
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// No adapter could be opened (a headless machine without a software
/// rasteriser): GPU tests skip with this as their notice.
#[derive(Debug)]
pub struct NoAdapter(pub String);

impl std::fmt::Display for NoAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no graphics adapter: {}", self.0)
    }
}

impl std::error::Error for NoAdapter {}

/// A device and queue without a surface.
pub struct Offscreen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
}

/// Which adapter an [`Offscreen`] prefers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdapterPreference {
    /// lavapipe, then llvmpipe, then the rest: goldens draw the same everywhere.
    #[default]
    Software,
    /// A discrete GPU, then an integrated one, then the rest, software last
    /// (the performance envelope, plan 8).
    Hardware,
    /// An integrated GPU, then a discrete one, then the rest, software last:
    /// a laptop's GPU measured on a desktop (plan 15a Task 20).
    Integrated,
}

/// How an [`Offscreen`] opens; the default is [`Offscreen::new`]'s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OffscreenOptions {
    pub prefer: AdapterPreference,
    /// Only these backends (else what `WGPU_BACKEND` allows).
    pub backends: Option<wgpu::Backends>,
    /// Ask for `Features::TIMESTAMP_QUERY` when the adapter has it (bench only).
    pub timestamps: bool,
    /// A texture side below the adapter's (tests: a level the device
    /// crops), never below wgpu's minimum of 2048.
    pub max_texture_side: Option<u32>,
}

/// An adapter's place in `prefer`'s order (lowest first). Software: a
/// software rasteriser (lavapipe through Vulkan, then llvmpipe through GL)
/// draws the same pixels on every machine, so goldens compare tightly; any
/// other adapter comes last.
fn rank(device_type: wgpu::DeviceType, backend: wgpu::Backend, prefer: AdapterPreference) -> u8 {
    use wgpu::{Backend, DeviceType};
    match prefer {
        AdapterPreference::Software => match (device_type, backend) {
            (DeviceType::Cpu, Backend::Vulkan) => 0,
            (DeviceType::Cpu, _) => 1,
            _ => 2,
        },
        AdapterPreference::Hardware => match device_type {
            DeviceType::DiscreteGpu => 0,
            DeviceType::IntegratedGpu => 1,
            DeviceType::VirtualGpu | DeviceType::Other => 2,
            DeviceType::Cpu => 3,
        },
        AdapterPreference::Integrated => match device_type {
            DeviceType::IntegratedGpu => 0,
            DeviceType::DiscreteGpu => 1,
            DeviceType::VirtualGpu | DeviceType::Other => 2,
            DeviceType::Cpu => 3,
        },
    }
}

/// A surfaceless instance: `backends`, else what the `WGPU_*` variables allow.
fn instance(backends: Option<wgpu::Backends>) -> wgpu::Instance {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    if let Some(b) = backends {
        desc.backends = b;
    }
    wgpu::Instance::new(desc)
}

/// Every adapter wgpu finds (for `olr-cli gpu-bench --list-adapters`).
pub fn list_adapters(backends: Option<wgpu::Backends>) -> Vec<wgpu::AdapterInfo> {
    let i = instance(backends);
    pollster::block_on(i.enumerate_adapters(backends.unwrap_or(wgpu::Backends::all())))
        .iter()
        .map(|a| a.get_info())
        .collect()
}

impl Offscreen {
    /// Opens the preferred software adapter ([`OffscreenOptions::default`]).
    pub fn new() -> Result<Offscreen, NoAdapter> {
        Offscreen::open(&OffscreenOptions::default())
    }

    /// Opens an adapter as `o` says; `WGPU_BACKEND` and the other `WGPU_*`
    /// variables narrow the choice unless `o.backends` is given.
    pub fn open(o: &OffscreenOptions) -> Result<Offscreen, NoAdapter> {
        let instance = instance(o.backends);
        let mut adapters =
            pollster::block_on(instance.enumerate_adapters(o.backends.unwrap_or(wgpu::Backends::all())));
        adapters.sort_by_key(|a| {
            let i = a.get_info();
            rank(i.device_type, i.backend, o.prefer)
        });
        let adapter = match adapters.into_iter().next() {
            Some(a) => a,
            None => pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .map_err(|e| NoAdapter(e.to_string()))?,
        };
        let info = adapter.get_info();
        let required_features = if o.timestamps {
            adapter.features() & wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let mut required_limits = adapter.limits();
        if let Some(side) = o.max_texture_side {
            required_limits.max_texture_dimension_2d =
                side.clamp(2048, required_limits.max_texture_dimension_2d);
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("olr-render offscreen"),
            required_features,
            required_limits,
            ..Default::default()
        }))
        .map_err(|e| NoAdapter(format!("{} ({:?}): {e}", info.name, info.backend)))?;
        Ok(Offscreen { device, queue, info })
    }

    /// The adapter's name, backend and type, for logs and reports.
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// A renderer drawing [`OUTPUT_FORMAT`] targets on this device.
    pub fn renderer(&self, settings: RenderSettings) -> Renderer {
        Renderer::new(self.device.clone(), self.queue.clone(), OUTPUT_FORMAT, settings)
    }

    /// Renders `list` at `size` pixels and reads the picture back.
    pub fn render_to_image(&self, renderer: &mut Renderer, list: &DrawList, size: (u32, u32)) -> RgbaImage {
        self.render_lit_to_image(renderer, list, None, size)
    }

    /// [`Offscreen::render_to_image`] with a look ([`Renderer::render_lit`]).
    pub fn render_lit_to_image(
        &self,
        renderer: &mut Renderer,
        list: &DrawList,
        look: Option<&olr_look::LookFrame>,
        size: (u32, u32),
    ) -> RgbaImage {
        let size = (size.0.max(1), size.1.max(1));
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("olr-render output"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        renderer.render_lit(list, look, &view, [0, 0, size.0, size.1]);
        self.read_texture(&texture)
    }

    /// [`Offscreen::render_to_image`], written to `path` as a PNG.
    pub fn render_to_png(
        &self,
        renderer: &mut Renderer,
        list: &DrawList,
        size: (u32, u32),
        path: &Path,
    ) -> std::io::Result<RgbaImage> {
        let img = self.render_to_image(renderer, list, size);
        std::fs::write(path, olr_pack::image_io::encode_rgba(&img))?;
        Ok(img)
    }

    /// Reads an RGBA8 texture (made with `COPY_SRC`) back to the CPU.
    pub fn read_texture(&self, texture: &wgpu::Texture) -> RgbaImage {
        let (w, h) = (texture.width(), texture.height());
        RgbaImage::from_raw(w, h, self.read_bytes(texture)).expect("w x h texels")
    }

    /// The renderer's light grid as last drawn ([`Renderer::light_grid`]):
    /// where it lies and each texel's linear light.
    pub fn read_light(&self, renderer: &Renderer) -> Option<(crate::LightGrid, Vec<[f32; 3]>)> {
        let (texture, grid) = renderer.light_grid()?;
        let bytes = self.read_bytes(texture);
        let half = |b: &[u8]| f16_to_f32(u16::from_le_bytes([b[0], b[1]]));
        let light = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|t| [half(&t[0..2]), half(&t[2..4]), half(&t[4..6])])
            .collect();
        Some((grid, light))
    }

    /// Any texture (made with `COPY_SRC`) back to the CPU, its texels' bytes
    /// row-major without padding.
    pub fn read_bytes(&self, texture: &wgpu::Texture) -> Vec<u8> {
        let (w, h) = (texture.width(), texture.height());
        let bpp = texture.format().block_copy_size(None).unwrap_or(4);
        let row = (w * bpp).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("olr-render readback"),
            size: u64::from(row) * u64::from(h),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("olr-render readback") });
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
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("the device is lost");
        rx.recv().expect("the map callback ran").expect("the readback buffer maps");
        let mapped = buffer.get_mapped_range(..).expect("mapped above");
        let mut pixels = Vec::with_capacity((w * h * bpp) as usize);
        for line in mapped.chunks_exact(row as usize) {
            pixels.extend_from_slice(&line[..(w * bpp) as usize]);
        }
        drop(mapped);
        buffer.unmap();
        pixels
    }
}

/// An IEEE half as an `f32`.
fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x3ff);
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 if frac == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        e => sign * (1.0 + frac / 1024.0) * 2f32.powi(e - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Integrated` (plan 15a Task 20, a laptop's GPU on a desktop) ranks the
    /// integrated GPU first, then the discrete one, software last; `Hardware`
    /// the discrete one first.
    #[test]
    fn preferences_rank_adapters() {
        use wgpu::DeviceType::*;
        let order = |p| {
            let mut v = [Cpu, DiscreteGpu, IntegratedGpu, Other];
            v.sort_by_key(|&t| rank(t, wgpu::Backend::Vulkan, p));
            v
        };
        assert_eq!(order(AdapterPreference::Hardware), [DiscreteGpu, IntegratedGpu, Other, Cpu]);
        assert_eq!(order(AdapterPreference::Integrated), [IntegratedGpu, DiscreteGpu, Other, Cpu]);
        assert_eq!(order(AdapterPreference::Software)[0], Cpu);
    }
}
