//! GPU time of each frame (bench only, plan 8): timestamps at the start of
//! the world pass (slot 0), the end of the upscale pass (1) and around the
//! probe passes (2, 3), when the device has `Features::TIMESTAMP_QUERY`
//! (an `Offscreen` opened with `timestamps`). Issue #34: every other pass
//! of the frame writes a pair of its own from [`PASS_BASE`] on (the first
//! world pass's start is slot 0, the last upscale's end slot 1), each
//! counted in its [`PassGroup`]. Reading them waits for the frame, so only
//! offscreen measurements use them. The render profile (its spec
//! `docs/superpowers/specs/2026-10-24-render-profile-design.md`): the
//! heat's operations in groups of their own, its copies timed between two
//! encoder timestamps where the device has them, every pass's own span
//! kept, and the passes no slot was left for counted.

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
    /// The heat's operations (issue #39; only while anything is hot),
    /// before the world pass reads it: the whole texture cleared (a reset),
    HeatClear,
    /// the frame's deposits,
    HeatDeposit,
    /// a step (conduction and cooling) over the active boxes,
    HeatStep,
    /// a dropped box zeroed,
    HeatZero,
    /// the pixels that changed layer zeroed (issue #67),
    HeatLayer,
    /// and the scratch copied back after a deposit, step or layer pass
    /// (encoder timestamps: only where the device has
    /// `TIMESTAMP_QUERY_INSIDE_ENCODERS`, else in the frame's gaps).
    HeatCopy,
    /// The world pass (the albedo, and lit the shade) and the occluder map.
    World,
    /// The glow light's inputs: play's discs, the emission.
    Emission,
    /// The jump flood's seed and its passes.
    JumpFlood,
    /// The radiance cascades.
    Cascades,
    /// The fluence (and its clear on a fresh start).
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
pub const PASS_GROUPS: usize = 18;

impl PassGroup {
    /// Every group, in the GPU's order ([`GpuTimes::passes`]' indices).
    pub const ALL: [PassGroup; PASS_GROUPS] = [
        PassGroup::HeatClear,
        PassGroup::HeatDeposit,
        PassGroup::HeatStep,
        PassGroup::HeatZero,
        PassGroup::HeatLayer,
        PassGroup::HeatCopy,
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
            PassGroup::HeatClear => "heat clear",
            PassGroup::HeatDeposit => "heat deposit",
            PassGroup::HeatStep => "heat step",
            PassGroup::HeatZero => "heat zero",
            PassGroup::HeatLayer => "heat layer",
            PassGroup::HeatCopy => "heat copy",
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
    /// The passes no slot was left for: run, but in no group.
    pub untimed: u32,
    /// [`PassGroup::HeatCopy`] was measured (the device has timestamps
    /// inside encoders); else the heat's copies are in the gaps.
    pub copies_timed: bool,
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
        ..GpuTimes::default()
    };
    for &(group, begin, end) in spans {
        t.passes[group as usize] += span(begin, end);
    }
    if t.frame_ms == 0.0 { GpuTimes::default() } else { t }
}

/// Each timed pass's own span in ms, in the GPU's order (as [`gpu_times`]
/// reads them: unwritten or backwards, 0).
pub fn pass_ms(ticks: &[u64], period_ns: f32, spans: &[PassSpan]) -> Vec<(PassGroup, f64)> {
    let mut out = Vec::with_capacity(spans.len());
    pass_ms_into(ticks, period_ns, spans, &mut out);
    out
}

fn pass_ms_into(ticks: &[u64], period_ns: f32, spans: &[PassSpan], out: &mut Vec<(PassGroup, f64)>) {
    let at = |slot: u32| ticks.get(slot as usize).copied().unwrap_or(0);
    out.clear();
    out.extend(spans.iter().map(|&(group, a, b)| {
        let (a, b) = (at(a), at(b));
        (group, if a == 0 || b < a { 0.0 } else { (b - a) as f64 * f64::from(period_ns) / 1e6 })
    }));
}

/// The frame's per-pass slots: handed out in pairs from [`PASS_BASE`],
/// counting the passes left without a pair.
pub(crate) struct Slots {
    next: Cell<u32>,
    untimed: Cell<u32>,
}

impl Slots {
    pub fn new() -> Slots {
        Slots { next: Cell::new(PASS_BASE), untimed: Cell::new(0) }
    }

    /// A new frame: every slot free, nothing untimed.
    pub fn reset(&self) {
        self.next.set(PASS_BASE);
        self.untimed.set(0);
    }

    fn take(&self) -> Option<u32> {
        let s = self.next.get();
        (s < PASS_BASE + PASS_SLOTS).then(|| {
            self.next.set(s + 1);
            s
        })
    }

    /// A pass's begin and end slots, `begin` or `end` given where it is a
    /// frame slot; `None` (counted untimed) when no slot is left for the
    /// rest.
    pub fn pair(&self, begin: Option<u32>, end: Option<u32>) -> Option<(u32, u32)> {
        let (b, e) = (begin.or_else(|| self.take()), end.or_else(|| self.take()));
        let pair = b.zip(e);
        if pair.is_none() {
            self.untimed.set(self.untimed.get() + 1);
        }
        pair
    }

    /// The first slot no pass took.
    pub fn next(&self) -> u32 {
        self.next.get()
    }

    pub fn untimed(&self) -> u32 {
        self.untimed.get()
    }
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
    /// The frame being encoded: its free per-pass slots and its timed
    /// passes (cells: the passes are begun while the renderer is borrowed).
    slots: Slots,
    spans: RefCell<Vec<PassSpan>>,
    /// The device writes timestamps between commands (the heat's copies).
    inside_encoders: bool,
    /// The last resolved frame's timed passes and untimed count.
    resolved: Vec<PassSpan>,
    resolved_untimed: u32,
    /// The last read frame's passes, each its own span.
    last: Vec<(PassGroup, f64)>,
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
            slots: Slots::new(),
            spans: RefCell::new(Vec::new()),
            inside_encoders: device.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS),
            resolved: Vec::new(),
            resolved_untimed: 0,
            last: Vec::new(),
        }
    }

    /// Starts a frame: no pass timed yet.
    pub fn start_frame(&self) {
        self.slots.reset();
        self.spans.borrow_mut().clear();
    }

    /// Two slots around commands outside a pass (the heat's copies),
    /// counted in `group`: `None` without timestamps inside encoders (the
    /// commands stay in the gaps) or when no slot is left (untimed).
    pub fn commands(&self, group: PassGroup) -> Option<(u32, u32)> {
        if !self.inside_encoders {
            return None;
        }
        let pair = self.slots.pair(None, None)?;
        self.spans.borrow_mut().push((group, pair.0, pair.1));
        Some(pair)
    }

    /// The last read frame's passes, each its own span, in ms.
    pub fn last_passes(&self) -> &[(PassGroup, f64)] {
        &self.last
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
        match self.slots.pair(begin, end) {
            Some((b, e)) => {
                self.spans.borrow_mut().push((group, b, e));
                Some(wgpu::RenderPassTimestampWrites {
                    query_set: &self.set,
                    beginning_of_pass_write_index: Some(b),
                    end_of_pass_write_index: Some(e),
                })
            }
            // A frame slot given must still be written: the frame's span
            // reads it.
            None => (begin.is_some() || end.is_some()).then_some(wgpu::RenderPassTimestampWrites {
                query_set: &self.set,
                beginning_of_pass_write_index: begin,
                end_of_pass_write_index: end,
            }),
        }
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
        let next = self.slots.next();
        if next > PASS_BASE {
            let at = size(PASS_BASE);
            encoder.resolve_query_set(&self.set, PASS_BASE..next, &self.resolve, at);
            encoder.copy_buffer_to_buffer(&self.resolve, at, &self.read, at, size(next - PASS_BASE));
        }
        self.resolved.clear();
        self.resolved.extend(self.spans.borrow().iter().copied());
        self.resolved_untimed = self.slots.untimed();
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
        let ticks = ticks?;
        pass_ms_into(&ticks, self.period_ns, &self.resolved, &mut self.last);
        let mut t = gpu_times(&ticks, self.period_ns, self.probe, &self.resolved);
        if t.frame_ms > 0.0 {
            (t.untimed, t.copies_timed) = (self.resolved_untimed, self.inside_encoders);
        }
        Some(t)
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

    /// The heat's operations are timed apart, before the world pass.
    #[test]
    fn heat_ops_have_groups_of_their_own() {
        let names: Vec<&str> = PassGroup::ALL[..6].iter().map(|g| g.name()).collect();
        assert_eq!(
            names,
            ["heat clear", "heat deposit", "heat step", "heat zero", "heat layer", "heat copy"]
        );
        assert_eq!(PassGroup::ALL.len(), PASS_GROUPS);
        assert_eq!(PassGroup::ALL[6], PassGroup::World);
    }

    /// A pass that finds no slot left is counted untimed, never lost
    /// silently (a heat catch-up's 250 steps outrun the slots).
    #[test]
    fn passes_past_the_slots_are_counted_untimed() {
        let slots = Slots::new();
        let passes = PASS_SLOTS / 2 + 3;
        let timed = (0..passes).filter(|_| slots.pair(None, None).is_some()).count() as u32;
        assert_eq!((timed, slots.untimed()), (PASS_SLOTS / 2, 3));
        // A pass given the frame's own begin slot still needs an end slot.
        assert_eq!(slots.pair(Some(0), None), None);
        assert_eq!(slots.untimed(), 4);
        slots.reset();
        assert_eq!((slots.pair(Some(0), None), slots.untimed()), (Some((0, PASS_BASE)), 0));
    }

    /// Each timed pass keeps its own span, in the GPU's order.
    #[test]
    fn each_pass_keeps_its_own_span() {
        let mut ticks = vec![0u64; QUERIES as usize];
        let p = PASS_BASE;
        (ticks[0], ticks[1]) = (1_000_000, 9_000_000);
        ticks[p as usize..p as usize + 2].copy_from_slice(&[3_000_000, 4_500_000]);
        let spans = [(PassGroup::HeatStep, 0, p), (PassGroup::Cascades, p + 1, 1)];
        let each = pass_ms(&ticks, 1.0, &spans);
        assert_eq!(each.len(), 2);
        assert_eq!(each[0].0, PassGroup::HeatStep);
        assert!((each[0].1 - 2.0).abs() < 1e-9 && (each[1].1 - 4.5).abs() < 1e-9, "{each:?}");
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
