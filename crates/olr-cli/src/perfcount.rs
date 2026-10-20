//! User-space instruction counts of the calling thread (`quick-bench`).
//! Instructions, unlike wall time, barely move when the shared machine is
//! busy, so a small regression shows in one run.

use perf_event::Builder;
use perf_event::events::Hardware;

/// The calling thread's retired user-space instructions.
pub struct Instructions(perf_event::Counter);

impl Instructions {
    /// Fails where the kernel offers no counter (no PMU in the VM, or
    /// `perf_event_paranoid` above 2).
    pub fn open() -> std::io::Result<Self> {
        Builder::new(Hardware::INSTRUCTIONS).build().map(Instructions)
    }

    /// Runs `f` and counts its instructions.
    pub fn measure<T>(&mut self, f: impl FnOnce() -> T) -> (T, u64) {
        let counter = &mut self.0;
        let started = counter.reset().and_then(|()| counter.enable());
        let out = f();
        let count = counter.disable().and_then(|()| counter.read());
        (out, started.and(count).unwrap_or(0))
    }
}
