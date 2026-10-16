//! GPU time of each frame (bench only, plan 8): timestamps at the start of
//! the world pass (slot 0), the end of the upscale pass (1) and around the
//! probe passes (2, 3), when the device has `Features::TIMESTAMP_QUERY`
//! (an `Offscreen` opened with `timestamps`). Reading them waits for the
//! frame, so only offscreen measurements use them.

use std::sync::mpsc;

/// Query slots a frame writes.
pub(crate) const QUERIES: u32 = 4;

/// The slots every timed frame writes (the world and upscale passes); the
/// probe's slots follow them.
const FRAME_QUERIES: u32 = 2;

/// What the GPU spent on the last frame, in ms.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuTimes {
    /// The world and upscale passes.
    pub frame_ms: f64,
    /// The probe passes (0 without them).
    pub probe_ms: f64,
}

/// The four slots' `ticks` at `period_ns` a tick as [`GpuTimes`]; a span
/// whose start was never written (0) or that runs backwards is 0.
pub fn gpu_times(ticks: [u64; 4], period_ns: f32, probe: bool) -> GpuTimes {
    let span =
        |a: u64, b: u64| if a == 0 || b < a { 0.0 } else { (b - a) as f64 * f64::from(period_ns) / 1e6 };
    let t = GpuTimes {
        frame_ms: span(ticks[0], ticks[1]),
        probe_ms: if probe { span(ticks[2], ticks[3]) } else { 0.0 },
    };
    if t.frame_ms == 0.0 { GpuTimes::default() } else { t }
}

/// The query set and its buffers.
pub(crate) struct Timer {
    pub set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    period_ns: f32,
    /// A frame wrote the slots since the last read.
    written: bool,
    /// That frame ran probe passes.
    probe: bool,
}

impl Timer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Timer {
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("olr-render timing"),
            ty: wgpu::QueryType::Timestamp,
            count: QUERIES,
        });
        let size = u64::from(QUERIES) * u64::from(wgpu::QUERY_SIZE);
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        Timer {
            set,
            resolve: buffer(
                "olr-render timing resolve",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            read: buffer(
                "olr-render timing read",
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            ),
            period_ns: queue.get_timestamp_period(),
            written: false,
            probe: false,
        }
    }

    /// The frame's timestamps into the read buffer (the end of its
    /// encoder): only the slots it wrote, the probe's only when `probe`
    /// passes ran (Vulkan resolves with a wait, so an unwritten slot could
    /// hang it). The rest of the buffer keeps an older frame's ticks, which
    /// [`gpu_times`] ignores without `probe`.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder, probe: bool) {
        let n = if probe { QUERIES } else { FRAME_QUERIES };
        encoder.resolve_query_set(&self.set, 0..n, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve,
            0,
            &self.read,
            0,
            u64::from(n) * u64::from(wgpu::QUERY_SIZE),
        );
        self.written = true;
        self.probe = probe;
    }

    /// Waits for the last frame and reads its times.
    pub fn read(&mut self, device: &wgpu::Device) -> Option<GpuTimes> {
        if !std::mem::take(&mut self.written) {
            return None;
        }
        let (tx, rx) = mpsc::channel();
        self.read.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        let ticks = self.read.get_mapped_range(..).ok().map(|mapped| {
            let mut ticks = [0u64; 4];
            for (t, bytes) in ticks.iter_mut().zip(mapped.as_chunks::<8>().0) {
                *t = u64::from_le_bytes(*bytes);
            }
            ticks
        });
        // Unmapped whatever the read gave, so the next frame can map it.
        self.read.unmap();
        Some(gpu_times(ticks?, self.period_ns, self.probe))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_become_milliseconds() {
        let t = gpu_times([1_000, 2_001_000, 3_000_000, 7_000_000], 1.0, true);
        assert!((t.frame_ms - 2.0).abs() < 1e-9 && (t.probe_ms - 4.0).abs() < 1e-9, "{t:?}");
        let t = gpu_times([10, 20, 30, 40], 1000.0, false);
        assert!((t.frame_ms - 0.01).abs() < 1e-9 && t.probe_ms == 0.0, "no probe, no probe time");
        assert_eq!(gpu_times([0, 5, 0, 5], 1.0, true), GpuTimes::default(), "never written");
        assert_eq!(gpu_times([9, 5, 9, 5], 1.0, true), GpuTimes::default(), "a clock that ran backwards");
    }
}
