//! Host migration (networking spec §3.7): its timing, the successor a
//! peer tries ([`Migration`]) and the report a peer sends its new host. The
//! session's side (silence, suspension, take-over, the merge, resume) is in
//! `session/migrate.rs`; the drivers (the harness, rl-online) join the
//! successors.

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
/// N65: each successor that can host has a window this long, in order, from
/// the moment the migration began.
pub const WINDOW_US: u64 = 5_000_000;
/// N65: no successor reachable this long after the migration began: the
/// host is lost (spec §3.7's "within 10 s").
pub const GIVE_UP_US: u64 = 10_000_000;
/// N65: a successor that refused (`Refusal::NotHost`: not its turn yet) is
/// tried again this much later, within its window.
pub const RETRY_US: u64 = 250_000;
/// N74: at most this many migrations per match (the driver's count); past
/// it, the host is lost.
pub const MAX_MIGRATIONS: u32 = 4;
/// A new host holds its first tick after the resume at most this long (its
/// rejoined players' longest round trip, ruling RS-1: their first inputs
/// reach it in time). `awful`'s 300 ms ping and its jitter fit.
pub const MAX_RESUME_HOLD_US: u64 = 500_000;
/// A new host repeats its resume in every datagram to a rejoined player
/// (`Datagram::Resume`) until that player's inputs come, at most this long
/// (then its frames, as to any player; the reliable `Control::Resume` went
/// too).
pub const RESUME_REPEAT_US: u64 = 2_000_000;

/// What a peer in a migration does now ([`Migration::step`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Keep an attempt to join successor `i` (an index into the order) going.
    Try(usize),
    /// This peer's turn: it is the successor, during its window, and for
    /// good once it [`committed`](Migration::committed) (pre-flight D8: one
    /// not committed by its window's end closes its gate and follows the
    /// later windows).
    Host,
    /// No successor's turn now (the windows ran out): wait for `GiveUp`.
    Wait,
    /// No successor was reachable within [`GIVE_UP_US`]: the host is lost.
    GiveUp,
}

/// One peer's view of a migration (spec §3.7, N65): which successor it
/// tries, from the moment it found its host gone. Time is an argument.
/// A peer whose attempt was welcomed (it committed to a new host) drops its
/// `Migration` (pre-flight A9): from then only the new host's resume, or
/// its silence (a further migration), ends its wait.
#[derive(Clone, Debug)]
pub struct Migration {
    /// Entry `i` of the succession may host.
    can_host: Vec<bool>,
    /// This peer's entry, if any.
    me: Option<usize>,
    began_us: u64,
    /// The successor that answered as a host (N65's contact).
    contact: Option<usize>,
    /// This peer, the successor, committed: the host for good.
    hosting: bool,
}

impl Migration {
    /// `can_host[i]`: entry `i` of the succession may host (the driver
    /// passes `false` for followers); `me`: this peer's entry, if any.
    pub fn new(can_host: Vec<bool>, me: Option<usize>, now_us: u64) -> Migration {
        Migration { can_host, me, began_us: now_us, contact: None, hosting: false }
    }

    /// The `k`-th entry that may host has the window `[began + k·WINDOW_US,
    /// began + (k+1)·WINDOW_US)`. A contacted successor is kept until
    /// [`GIVE_UP_US`] (the successor itself breaks the contact if it
    /// closes); `Host` lasts the window, or for good once `committed`.
    pub fn step(&mut self, now_us: u64) -> Step {
        if self.hosting {
            return Step::Host;
        }
        let since = now_us.saturating_sub(self.began_us);
        if since >= GIVE_UP_US {
            return Step::GiveUp;
        }
        if let Some(i) = self.contact {
            return Step::Try(i);
        }
        let k = (since / WINDOW_US) as usize;
        match self.can_host.iter().enumerate().filter(|(_, c)| **c).nth(k) {
            Some((i, _)) if Some(i) == self.me => Step::Host,
            Some((i, _)) => Step::Try(i),
            None => Step::Wait,
        }
    }

    /// This peer, the successor, committed (its first rejoin, or nobody
    /// expected): `Host` for good.
    pub fn committed(&mut self) {
        self.hosting = true;
    }

    /// Successor `i` answered as a host (N65's contact). Nothing for this
    /// peer's own entry or one out of range.
    pub fn contact(&mut self, i: usize) {
        if i < self.can_host.len() && Some(i) != self.me {
            self.contact = Some(i);
        }
    }

    /// The attempt to `i` failed after contact: back to the windows.
    pub fn lost_contact(&mut self, i: usize) {
        if self.contact == Some(i) {
            self.contact = None;
        }
    }

    pub fn began_us(&self) -> u64 {
        self.began_us
    }

    /// When the current window ends (the deadline for peers not yet back,
    /// N68).
    pub fn window_end(&self, now_us: u64) -> u64 {
        let k = now_us.saturating_sub(self.began_us) / WINDOW_US;
        self.began_us.saturating_add(k.saturating_add(1).saturating_mul(WINDOW_US))
    }
}

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
    let mut report = InputsReport { ack, first, frames: Vec::new(), own_first, own };
    fit_frames(first, upto, &mut |n| {
        report.frames = encode_frames(first, n, slots, value, base);
        encode_control(&Control::Report(report.clone())).len()
    });
    report
}

/// The new host's `Control::Resume { tick, at_ms, first, frames }` to a
/// rejoined peer (ruling RS-1): the final inputs it lacks, ticks
/// `first..upto` (`value`, `base` as `encode_frames` takes them), cut as a
/// report is.
pub fn fit_resume(
    tick: u32,
    at_ms: u32,
    first: u32,
    upto: u32,
    slots: usize,
    value: &dyn Fn(u32, usize) -> Option<u8>,
    base: &dyn Fn(usize) -> Option<u8>,
) -> Control {
    let mut resume = Control::Resume { tick, at_ms, first, frames: Vec::new() };
    fit_frames(first, upto, &mut |n| {
        resume = Control::Resume { tick, at_ms, first, frames: encode_frames(first, n, slots, value, base) };
        encode_control(&resume).len()
    });
    resume
}

/// Ticks `first..upto`, the span halved from its end until `encode(n)`
/// (the message with `n` frames, its encoded length) fits `MAX_CONTROL`
/// and `n` fits `MAX_REPORT_FRAMES`: the earliest ticks matter. The last
/// call is the one that fits (or `n == 0`).
fn fit_frames(first: u32, upto: u32, encode: &mut dyn FnMut(u32) -> usize) {
    // The decoder refuses a span past `u32::MAX`.
    let mut n = upto.saturating_sub(first).min(u32::MAX - first);
    loop {
        if n as usize <= MAX_REPORT_FRAMES && (encode(n) <= MAX_CONTROL || n == 0) {
            return;
        }
        n /= 2;
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    const S: u64 = 1_000_000;

    #[test]
    fn each_successor_that_can_host_has_its_window() {
        // Entry 1 cannot host (a follower); this peer is entry 3.
        let mut m = Migration::new(vec![true, false, true, true], Some(3), 100);
        assert_eq!(m.step(100), Step::Try(0));
        assert_eq!(m.step(100 + 5 * S - 1), Step::Try(0));
        assert_eq!(m.step(100 + 5 * S), Step::Try(2));
        // Its own window would open at 10 s, when the migration gives up.
        assert_eq!(m.step(100 + 10 * S), Step::GiveUp);
    }

    #[test]
    fn a_successor_hosts_its_window_and_for_good_once_committed() {
        let mut m = Migration::new(vec![true, true], Some(1), 0);
        assert_eq!(m.step(4 * S), Step::Try(0));
        assert_eq!(m.step(5 * S), Step::Host);
        assert_eq!(m.window_end(5 * S), 10 * S);
        m.committed();
        assert_eq!(m.step(60 * S), Step::Host, "committed: the host for good, no give-up");
    }

    #[test]
    fn an_uncommitted_successor_closes_at_its_windows_end() {
        let mut m = Migration::new(vec![true, true, true], Some(0), 0);
        assert_eq!(m.step(0), Step::Host);
        assert_eq!(m.step(5 * S - 1), Step::Host);
        assert_eq!(m.step(5 * S), Step::Try(1), "nobody came: it follows the next window (D8)");
        assert_eq!(m.step(10 * S), Step::GiveUp);
    }

    #[test]
    fn a_contacted_successor_is_kept_past_its_window() {
        let mut m = Migration::new(vec![true, true, true], Some(2), 0);
        m.contact(0);
        assert_eq!(m.step(7 * S), Step::Try(0));
        m.lost_contact(0);
        assert_eq!(m.step(7 * S), Step::Try(1));
        assert_eq!(m.step(10 * S), Step::GiveUp);
    }

    #[test]
    fn a_follower_waits_once_the_windows_run_out() {
        let mut m = Migration::new(vec![true, false], Some(1), 0);
        assert_eq!(m.step(0), Step::Try(0));
        assert_eq!(m.step(5 * S), Step::Wait);
        assert_eq!(m.step(10 * S), Step::GiveUp);
        let mut none = Migration::new(vec![], None, 0);
        assert_eq!(none.step(0), Step::Wait);
    }
}
