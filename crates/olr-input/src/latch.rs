//! Presses and releases between two 100 Hz ticks, latched so the tick sees
//! each of them (spec §8.5).

/// The actions' bits ([`crate::Action::bit`]) as the devices report them
/// between ticks, and as the ticks see them.
///
/// - A key pressed since the last tick is down for this tick, even if it
///   was released again (a tap shorter than a tick).
/// - A key that was down for the last tick and released since is up for
///   this tick, even if it was pressed again; that new press is down for
///   the next tick. So the simulation sees every release and every press,
///   which its own latches (`olr_sim::KeyLatch`, OLX's `CInput`) turn into
///   OLX's `isDownOnce` taps, such as digging or the rope.
/// - Otherwise a key is what the device says now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickLatch {
    /// What the device says now.
    held: u32,
    /// What the last tick saw.
    seen: u32,
    /// Pressed since the last tick.
    pressed: u32,
    /// Released since the last tick.
    released: u32,
    /// Pressed again after a release of a key the last tick saw down: down
    /// for the tick after the next.
    again: u32,
}

impl TickLatch {
    /// The device now holds `now`.
    pub fn set(&mut self, now: u32) {
        let down = now & !self.held;
        let up = self.held & !now;
        // A press after a release of a key the last tick saw down waits a
        // tick, so the release is seen first.
        let after_release = self.released & self.seen;
        self.again |= down & after_release;
        self.pressed |= down & !after_release;
        self.released |= up;
        self.held = now;
    }

    /// What the device holds now.
    pub fn held(&self) -> u32 {
        self.held
    }

    /// The bits for the next tick; starts latching anew.
    pub fn latch(&mut self) -> u32 {
        let kept = self.seen & self.held & !self.released;
        let new = !self.seen & (self.held | self.pressed);
        let out = kept | new;
        self.seen = out;
        self.pressed = self.again;
        self.again = 0;
        self.released = 0;
        out
    }

    /// What the last tick saw: its [`TickLatch::latch`], and any
    /// [`TickLatch::tap`] on top.
    pub fn last(&self) -> u32 {
        self.seen
    }

    /// The last tick also saw `bits` down, which the device does not hold
    /// (a press made for it, such as the dig gesture): they are up for the
    /// next tick, as a release, and a press of one of them made meanwhile
    /// is down for the tick after.
    pub fn tap(&mut self, bits: u32) {
        let bits = bits & !self.held;
        self.seen |= bits;
        self.released |= bits;
    }
}
