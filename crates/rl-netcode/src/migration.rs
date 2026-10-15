//! Host migration (networking spec §3.7): its timing and the report a peer
//! sends its new host. The session's side (silence, suspension, take-over,
//! the merge, resume) is in `session/migrate.rs`.

use alloc::vec::Vec;

use rl_proto::{
    Control, InputsReport, MAX_CONTROL, MAX_REPORT_FRAMES, MAX_REPORT_OWN, encode_control, encode_frames,
};

/// Spec §3.7: a client or spectator whose host sent nothing for this long,
/// while it kept advancing, finds it gone.
pub const HOST_SILENT_US: u64 = 2_000_000;
/// N70: a host whose own polls stopped this long steps down (below
/// [`HOST_SILENT_US`], so jitter cannot split the match).
pub const STEP_DOWN_US: u64 = 1_500_000;
/// A suspended session's frame-less keepalive datagram period (pre-flight
/// A6: a collecting new host never looks silent).
pub const KEEPALIVE_US: u64 = 250_000;

/// The report of what this peer knows (N67): frames for ticks `first..upto`
/// (`value`, `base` as `encode_frames` takes them), the span halved from its
/// end until the encoded `Control::Report` fits `MAX_CONTROL` and
/// `MAX_REPORT_FRAMES` (the earliest ticks matter); `own` cut to
/// `MAX_REPORT_OWN`.
#[allow(clippy::too_many_arguments)]
pub fn fit_report(
    ack: u32,
    first: u32,
    upto: u32,
    slots: usize,
    value: &dyn Fn(u32, usize) -> Option<u8>,
    base: &dyn Fn(usize) -> Option<u8>,
    own_first: u32,
    mut own: Vec<u8>,
) -> InputsReport {
    // The decoder refuses a span past `u32::MAX`.
    own.truncate(MAX_REPORT_OWN.min((u32::MAX - own_first) as usize));
    let mut n = upto.saturating_sub(first).min(u32::MAX - first);
    let mut report = InputsReport { ack, first, frames: Vec::new(), own_first, own };
    loop {
        if n as usize <= MAX_REPORT_FRAMES {
            report.frames = encode_frames(first, n, slots, value, base);
            if n == 0 || encode_control(&Control::Report(report.clone())).len() <= MAX_CONTROL {
                return report;
            }
        }
        n /= 2;
    }
}
