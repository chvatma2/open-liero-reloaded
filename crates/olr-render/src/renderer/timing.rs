//! GPU time of each frame (bench only, plan 8): timestamps at the start of
//! the world pass (slot 0), the end of the upscale pass (1) and around the
//! probe passes (2, 3), when the device has `Features::TIMESTAMP_QUERY`
//! (an `Offscreen` opened with `timestamps`). Issue #34: every other pass
//! of the frame writes a pair of its own from [`PASS_BASE`] on (the first
//! world pass's start is slot 0, the last upscale's end slot 1), each
//! counted in its [`PassGroup`]. Reading them waits for the frame, so only
//! offscreen measurements use them.

use std::cell::{Cell, RefCell};
use std::sync::mpsc;

/// The slots every timed frame writes (the world and upscale passes); the
/// probe's slots follow them.
const FRAME_QUERIES: u32 = 2;
/// The frame's and the probe's slots.
const FIXED_QUERIES: u32 = 4;
/// The first per-pass slot: a query resolve lands on a 256-byte boundary
/// (`wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT`), 32 slots of 8 bytes.
pub(crate) const PASS_BASE: u32 = 32;
/// The per-pass slots: a pair a pass, enough for four views' look passes
/// and the glow light's (about 85 passes); a pass past them goes untimed.
pub(crate) const PASS_SLOTS: u32 = 256;
/// Query slots in all.
pub(crate) const QUERIES: u32 = PASS_BASE + PASS_SLOTS;

/// The groups a frame's passes are timed in (issue #34), in the order the
/// GPU runs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PassGroup {
    /// The world pass (the albedo, and lit the shade) and the occluder map.
    World,
    /// The glow light's inputs: its occluder map, play's discs, the emission.
    Emission,
    /// The jump flood's seed and its passes.
    JumpFlood,
    /// The radiance cascades.
    Cascades,
    /// The fluence (and the history's clear on a fresh start).
    Fluence,
    /// The ambient occlusion's rows (issue #10, only with occlusion).
    Occlusion,
    /// The light sum.
    Sum,
    /// The soak, across and down.
    Soak,
    /// The softness blur, across and down (toned and banded).
    Blur,
    /// The bloom, across and down.
    Bloom,
    /// The composite.
    Composite,
    /// The upscale into the output.
    Upscale,
}

/// How many [`PassGroup`]s there are.
pub const PASS_GROUPS: usize = 12;

impl PassGroup {
    /// Every group, in the GPU's order ([`GpuTimes::passes`]' indices).
    pub const ALL: [PassGroup; PASS_GROUPS] = [
        PassGroup::World,
        PassGroup::Emission,
        PassGroup::JumpFlood,
        PassGroup::Cascades,
        PassGroup::Fluence,
        PassGroup::Occlusion,
        PassGroup::Sum,
        PassGroup::Soak,
        PassGroup::Blur,
        PassGroup::Bloom,
        PassGroup::Composite,
        PassGroup::Upscale,
    ];

    /// Its name in `gpu-bench`'s table.
    pub fn name(self) -> &'static str {
        match self {
            PassGroup::World => "world",
            PassGroup::Emission => "emission",
            PassGroup::JumpFlood => "jump flood",
            PassGroup::Cascades => "cascades",
            PassGroup::Fluence => "fluence",
            PassGroup::Occlusion => "occlusion",
            PassGroup::Sum => "light sum",
            PassGroup::Soak => "soak",
            PassGroup::Blur => "blur",
            PassGroup::Bloom => "bloom",
            PassGroup::Composite => "composite",
            PassGroup::Upscale => "upscale",
        }
    }
}

/// One timed pass: its group and its start and end slots.
pub type PassSpan = (PassGroup, u32, u32);

/// What the GPU spent on the last frame, in ms.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuTimes {
    /// The world and upscale passes.
    pub frame_ms: f64,
    /// The probe passes (0 without them).
    pub probe_ms: f64,
    /// Each [`PassGroup`]'s passes summed (indexed as [`PassGroup::ALL`]; 0
    /// for a group the frame ran none of). The frame less their sum is the
    /// copies and the gaps between passes.
    pub passes: [f64; PASS_GROUPS],
    /// The passes timed (issue #35: what a change saves in passes; the
    /// probe's not counted).
    pub pass_count: u32,
}

/// The slots' `ticks` at `period_ns` a tick as [`GpuTimes`], the passes'
/// from their `spans`; a span whose start was never written (0), past the
/// ticks or that runs backwards is 0.
pub fn gpu_times(ticks: &[u64], period_ns: f32, probe: bool, spans: &[PassSpan]) -> GpuTimes {
    let at = |slot: u32| ticks.get(slot as usize).copied().unwrap_or(0);
    let span = |a: u32, b: u32| {
        let (a, b) = (at(a), at(b));
        if a == 0 || b < a { 0.0 } else { (b - a) as f64 * f64::from(period_ns) / 1e6 }
    };
    let mut t = GpuTimes {
        frame_ms: span(0, 1),
        probe_ms: if probe { span(2, 3) } else { 0.0 },
        passes: [0.0; PASS_GROUPS],
        pass_count: spans.len() as u32,
    };
    for &(group, begin, end) in spans {
        t.passes[group as usize] += span(begin, end);
    }
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
    /// The frame being encoded: its next free per-pass slot and its timed
    /// passes (cells: the passes are begun while the renderer is borrowed).
    next: Cell<u32>,
    spans: RefCell<Vec<PassSpan>>,
    /// The last resolved frame's timed passes.
    resolved: Vec<PassSpan>,
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
            next: Cell::new(PASS_BASE),
            spans: RefCell::new(Vec::new()),
            resolved: Vec::new(),
        }
    }

    /// Starts a frame: no pass timed yet.
    pub fn start_frame(&self) {
        self.next.set(PASS_BASE);
        self.spans.borrow_mut().clear();
    }

    /// A free per-pass slot, if any is left.
    fn slot(&self) -> Option<u32> {
        let s = self.next.get();
        (s < PASS_BASE + PASS_SLOTS).then(|| {
            self.next.set(s + 1);
            s
        })
    }

    /// The timestamp writes of a pass of `group`: `begin` and `end` (the
    /// frame's slots) where given, else slots of its own; it is counted in
    /// `group` when it has both. `None` only when no slot is left.
    pub fn pass(
        &self,
        group: PassGroup,
        begin: Option<u32>,
        end: Option<u32>,
    ) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let begin = begin.or_else(|| self.slot());
        let end = end.or_else(|| self.slot());
        if let (Some(b), Some(e)) = (begin, end) {
            self.spans.borrow_mut().push((group, b, e));
        }
        (begin.is_some() || end.is_some()).then_some(wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: begin,
            end_of_pass_write_index: end,
        })
    }

    /// The frame's timestamps into the read buffer (the end of its
    /// encoder): only the slots it wrote, the probe's only when `probe`
    /// passes ran (Vulkan resolves with a wait, so an unwritten slot could
    /// hang it). The rest of the buffer keeps an older frame's ticks, which
    /// [`gpu_times`] ignores without `probe` or a span reading them.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder, probe: bool) {
        let fixed = if probe { FIXED_QUERIES } else { FRAME_QUERIES };
        let size = |n: u32| u64::from(n) * u64::from(wgpu::QUERY_SIZE);
        encoder.resolve_query_set(&self.set, 0..fixed, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.read, 0, size(fixed));
        let next = self.next.get();
        if next > PASS_BASE {
            let at = size(PASS_BASE);
            encoder.resolve_query_set(&self.set, PASS_BASE..next, &self.resolve, at);
            encoder.copy_buffer_to_buffer(&self.resolve, at, &self.read, at, size(next - PASS_BASE));
        }
        self.resolved.clear();
        self.resolved.extend(self.spans.borrow().iter().copied());
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
            mapped.as_chunks::<8>().0.iter().map(|bytes| u64::from_le_bytes(*bytes)).collect::<Vec<u64>>()
        });
        // Unmapped whatever the read gave, so the next frame can map it.
        self.read.unmap();
        Some(gpu_times(&ticks?, self.period_ns, self.probe, &self.resolved))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_become_milliseconds() {
        let t = gpu_times(&[1_000, 2_001_000, 3_000_000, 7_000_000], 1.0, true, &[]);
        assert!((t.frame_ms - 2.0).abs() < 1e-9 && (t.probe_ms - 4.0).abs() < 1e-9, "{t:?}");
        let t = gpu_times(&[10, 20, 30, 40], 1000.0, false, &[]);
        assert!((t.frame_ms - 0.01).abs() < 1e-9 && t.probe_ms == 0.0, "no probe, no probe time");
        assert_eq!(gpu_times(&[0, 5, 0, 5], 1.0, true, &[]), GpuTimes::default(), "never written");
        assert_eq!(
            gpu_times(&[9, 5, 9, 5], 1.0, true, &[]),
            GpuTimes::default(),
            "a clock that ran backwards"
        );
    }

    /// Issue #34: each pass's span is added to its group; the first world
    /// pass starts at the frame's slot 0, the last upscale ends at slot 1.
    #[test]
    fn passes_add_up_in_their_groups() {
        let mut ticks = vec![0u64; QUERIES as usize];
        let b = PASS_BASE as usize;
        ticks[0] = 1_000_000;
        ticks[1] = 9_000_000;
        // The world pass ends at 2 ms; two soak passes of 1 ms and 0.5 ms;
        // the upscale starts at 8 ms.
        ticks[b..b + 6].copy_from_slice(&[2_000_000, 3_000_000, 4_000_000, 5_000_000, 5_500_000, 8_000_000]);
        let p = PASS_BASE;
        let spans = [
            (PassGroup::World, 0, p),
            (PassGroup::Soak, p + 1, p + 2),
            (PassGroup::Soak, p + 3, p + 4),
            (PassGroup::Upscale, p + 5, 1),
        ];
        let t = gpu_times(&ticks, 1.0, false, &spans);
        let ms = |g: PassGroup| t.passes[g as usize];
        assert!((t.frame_ms - 8.0).abs() < 1e-9, "{t:?}");
        assert!((ms(PassGroup::World) - 1.0).abs() < 1e-9, "{t:?}");
        assert!((ms(PassGroup::Soak) - 1.5).abs() < 1e-9, "{t:?}");
        assert!((ms(PassGroup::Upscale) - 1.0).abs() < 1e-9, "{t:?}");
        assert_eq!(ms(PassGroup::Cascades), 0.0, "no cascade pass ran");
        assert_eq!(t.pass_count, 4, "every pass counted");
        // A span past the slots read, or never written, counts 0.
        let t = gpu_times(&ticks[..4], 1.0, false, &spans);
        assert_eq!((t.passes[PassGroup::Soak as usize], t.frame_ms), (0.0, 8.0));
        assert!(PassGroup::ALL.iter().enumerate().all(|(i, &g)| g as usize == i));
    }
}
