//! The network pump (ruling PF12c-2): a thread `rl-game net` that polls an
//! online peer whenever the UI thread has not polled it for [`PUMP_AFTER`].
//!
//! The game's network, netcode and bots run inside `OnlinePeer::poll`,
//! which the UI thread calls once a frame. A frame that does not come (a
//! window dragged: Windows' modal move loop blocks winit's event loop; a
//! long match load; a slow frame) would silence the peer, and after 2 s its
//! peers give up on it. The pump polls in its place, so only a whole-machine
//! stall silences a peer.
//!
//! **The lock** ([`PeerHandle`]): the peer sits behind one `Mutex`, held
//! for one poll (the UI's or the pump's) or one [`PeerHandle::with`] call,
//! never across rendering, and neither thread waits on anything else while
//! holding it: the UI waits at most for one pump poll
//! (`the_ui_waits_at_most_one_pump_poll`). The UI's last poll is an atomic,
//! so the pump checks it without the lock.
//!
//! **While the pump polls**, the local player's keys are `Keys(0)` (the
//! input latch is the UI's, untouched), nothing is drawn (no `on_tick`),
//! and the host's bots think inside the peer as always. Its events wait for
//! the UI's next poll, at most [`PUMP_QUEUE`] of them, except the match's
//! presentation (`Ticked`, `Restored`, `StateLoaded`): those are dropped,
//! and the UI's next poll says so ([`PolledEvents::pumped`]), so the
//! presentation starts again from the world as after a loaded state.
//! `Confirmed` ticks are kept: the HUD's log, scores and the end are never
//! lost while room remains.
//!
//! The ticks the pump runs are the netcode's like any other: the peer
//! cannot tell which thread polled it, and its world is the same function
//! of the match's final inputs (the pump's `Keys(0)` are this player's
//! inputs for those ticks, sent to everyone).

use std::cell::Cell;
use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rl_netcode::TickInfo;
use rl_online::{OnlineEvent, OnlinePeer};
use rl_sim::{Keys, TickEvents, World};

/// The UI thread has not polled a peer for this long: the pump polls it.
pub const PUMP_AFTER: Duration = Duration::from_millis(20);
/// At most this many of the pump's events wait for the UI (beyond: the
/// oldest `Confirmed` goes first, then the oldest event; each loss
/// counted, the first logged). The match's presentation (`Ticked`,
/// `Restored`, `StateLoaded`) never waits: [`PolledEvents::pumped`].
pub const PUMP_QUEUE: usize = 4_096;
/// The pump's thread name.
pub const PUMP_THREAD: &str = "rl-game net";

/// What the pump polls: an `OnlinePeer` (unit tests poll a fake).
pub trait Poll: Send + 'static {
    /// As `OnlinePeer::poll`.
    fn poll(
        &mut self,
        now: Instant,
        local_keys: &mut dyn FnMut() -> Keys,
        on_tick: &mut dyn FnMut(&World, &TickEvents, TickInfo),
    ) -> Vec<OnlineEvent>;
}

impl Poll for OnlinePeer {
    fn poll(
        &mut self,
        now: Instant,
        local_keys: &mut dyn FnMut() -> Keys,
        on_tick: &mut dyn FnMut(&World, &TickEvents, TickInfo),
    ) -> Vec<OnlineEvent> {
        OnlinePeer::poll(self, now, local_keys, on_tick)
    }
}

/// What a UI poll returns.
#[derive(Debug, Default)]
pub struct PolledEvents {
    /// The pump's queued events, then this poll's, in order.
    pub events: Vec<OnlineEvent>,
    /// The pump ran ticks since the UI's last poll: the newest tick it
    /// simulated (or a state it loaded follows). Their presentation was
    /// dropped, so the presentation starts again from the world as after a
    /// loaded state (`Presentation::state_loaded`, the colour map rebuilt
    /// from the terrain, `NetPresenter::reset` with this tick); this poll's
    /// own ticks follow on from there.
    pub pumped: Option<u32>,
}

/// An online peer shared by the UI thread and its pump, which this handle
/// owns: [`PeerHandle::pumped`] starts it, [`PeerHandle::into_peer`] stops
/// it (joined within one poll) and gives the peer back. Not `Clone`: one
/// owner (the shell's `Net`, or the match's `OnlineDriver`), so the peer
/// always comes back.
pub struct PeerHandle<P: Poll = OnlinePeer> {
    // Dropped first: the pump stops before the peer can go.
    pump: Option<Pump>,
    shared: Arc<Shared<P>>,
}

struct Shared<P> {
    inner: Mutex<Inner<P>>,
    /// What `ui_polled` counts from.
    epoch: Instant,
    /// When the UI thread last polled (nanoseconds since `epoch`, the real
    /// clock): the pump reads it without the lock.
    ui_polled: AtomicU64,
}

struct Inner<P> {
    peer: P,
    /// The pump's events waiting for the UI.
    queue: VecDeque<OnlineEvent>,
    /// The newest tick the pump simulated since the UI's last poll.
    pumped: Option<u32>,
    /// Events the full queue let go.
    lost: u64,
    /// The UI's last `now` and the real time it was given at: the pump's
    /// time goes on from there (the UI's clock may be a test's).
    ui_clock: Option<(Instant, Instant)>,
    /// The last time given to the peer: never earlier again.
    last_now: Option<Instant>,
    /// The pump's polls so far.
    pump_polls: u64,
}

impl<P: Poll> Inner<P> {
    /// `now`, never before the last time the peer was given.
    fn clamp(&mut self, now: Instant) -> Instant {
        let now = self.last_now.map_or(now, |l| now.max(l));
        self.last_now = Some(now);
        now
    }

    /// One poll by the pump at the real time `real`: no keys, nothing
    /// drawn, its events queued.
    fn pump_poll(&mut self, real: Instant) {
        let now = self.ui_clock.map_or(real, |(ui, at)| ui + real.saturating_duration_since(at));
        let now = self.clamp(now);
        let events = self.peer.poll(now, &mut || Keys(0), &mut |_, _, _| {});
        self.pump_polls += 1;
        for e in events {
            self.queue_event(e);
        }
    }

    /// One of the pump's events kept for the UI: the presentation's
    /// dropped (`pumped` notes it), the rest queued within [`PUMP_QUEUE`].
    fn queue_event(&mut self, e: OnlineEvent) {
        let tick = match &e {
            OnlineEvent::Ticked(t) => Some(t.iter().map(|t| t.tick).max()),
            OnlineEvent::StateLoaded { tick } => Some(Some(*tick)),
            OnlineEvent::Restored(_) => Some(None),
            _ => None,
        };
        if let Some(tick) = tick {
            let newest = self.pumped.max(tick);
            self.pumped = Some(newest.unwrap_or(0));
            return;
        }
        if self.queue.len() >= PUMP_QUEUE {
            let at = self.queue.iter().position(|e| matches!(e, OnlineEvent::Confirmed(_))).unwrap_or(0);
            self.queue.remove(at);
            self.lost += 1;
            if self.lost == 1 {
                eprintln!(
                    "warning: the network pump's queue is full ({PUMP_QUEUE} events): the oldest go \
                     (counted; logged once)"
                );
            }
        }
        self.queue.push_back(e);
    }
}

thread_local! {
    /// The peer lock this thread holds (its address), to catch a second
    /// lock of the same peer on one thread, which would deadlock.
    static HELD: Cell<usize> = const { Cell::new(0) };
}

/// The lock held, and the thread's note of it.
struct Held<'a, P> {
    guard: MutexGuard<'a, Inner<P>>,
    /// The thread's note before this lock.
    before: usize,
}

impl<P> Drop for Held<'_, P> {
    fn drop(&mut self) {
        HELD.with(|h| h.set(self.before));
    }
}

impl<P> std::ops::Deref for Held<'_, P> {
    type Target = Inner<P>;
    fn deref(&self) -> &Inner<P> {
        &self.guard
    }
}

impl<P> std::ops::DerefMut for Held<'_, P> {
    fn deref_mut(&mut self) -> &mut Inner<P> {
        &mut self.guard
    }
}

impl<P: Poll> Shared<P> {
    /// The lock (a poll that panicked on the pump leaves the peer as it
    /// was: it is used on).
    fn lock(&self) -> Held<'_, P> {
        let addr = std::ptr::from_ref(self) as usize;
        let before = HELD.with(Cell::get);
        assert!(
            before != addr,
            "a peer locked twice on one thread (PeerHandle::with inside another): a deadlock"
        );
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        HELD.with(|h| h.set(addr));
        Held { guard, before }
    }

    /// The UI thread polls (or is about to) at `at`.
    fn mark_ui(&self, at: Instant) {
        let ns = u64::try_from(at.saturating_duration_since(self.epoch).as_nanos()).unwrap_or(u64::MAX);
        self.ui_polled.store(ns, Ordering::Release);
    }

    /// How long since the UI thread last polled, at `now`.
    fn ui_idle(&self, now: Instant) -> Duration {
        let at = self.epoch + Duration::from_nanos(self.ui_polled.load(Ordering::Acquire));
        now.saturating_duration_since(at)
    }
}

impl<P: Poll> PeerHandle<P> {
    /// `peer` behind the lock, without a pump (tests that drive the clock
    /// themselves; [`PeerHandle::start_pump`] starts one).
    pub fn new(peer: P) -> PeerHandle<P> {
        let epoch = Instant::now();
        let inner = Inner {
            peer,
            queue: VecDeque::new(),
            pumped: None,
            lost: 0,
            ui_clock: None,
            last_now: None,
            pump_polls: 0,
        };
        let shared = Shared { inner: Mutex::new(inner), epoch, ui_polled: AtomicU64::new(0) };
        PeerHandle { pump: None, shared: Arc::new(shared) }
    }

    /// `peer` with its pump started (every online peer of the game's). A
    /// pump that cannot start is a warning: the UI's frames alone poll it.
    pub fn pumped(peer: P) -> PeerHandle<P> {
        let mut h = PeerHandle::new(peer);
        if let Err(e) = h.start_pump() {
            eprintln!("warning: cannot start the network pump ({e}): the network runs in the frames alone");
        }
        h
    }

    /// Starts the pump (none runs yet; a running one stays).
    pub fn start_pump(&mut self) -> io::Result<()> {
        if self.pump.is_none() {
            self.pump = Some(Pump::start(&self.shared)?);
        }
        Ok(())
    }

    /// Stops the pump, if one runs: joined within one of its polls.
    pub fn stop_pump(&mut self) {
        if let Some(p) = self.pump.take() {
            p.stop();
        }
    }

    /// Whether a pump runs.
    pub fn is_pumped(&self) -> bool {
        self.pump.is_some()
    }

    /// The UI thread's poll at `now`: as `OnlinePeer::poll`, the pump's
    /// queued events first, then this poll's; marks the UI's poll time.
    /// `now` is never taken earlier than a time the peer was already
    /// given (the pump may have polled since the caller read its clock).
    pub fn poll(
        &self,
        now: Instant,
        local_keys: &mut dyn FnMut() -> Keys,
        on_tick: &mut dyn FnMut(&World, &TickEvents, TickInfo),
    ) -> PolledEvents {
        // Marked before the lock too: a pump about to poll stands back.
        self.shared.mark_ui(Instant::now());
        let mut inner = self.shared.lock();
        let now = inner.clamp(now);
        inner.ui_clock = Some((now, Instant::now()));
        let mut events: Vec<OnlineEvent> = inner.queue.drain(..).collect();
        let pumped = inner.pumped.take();
        events.extend(inner.peer.poll(now, local_keys, on_tick));
        self.shared.mark_ui(Instant::now());
        PolledEvents { events, pumped }
    }

    /// Anything else on the peer (lobby actions, state, stats, its
    /// world): the lock, held for the closure only. The closure must not
    /// use this handle again (that would deadlock: it panics instead).
    pub fn with<R>(&self, f: impl FnOnce(&mut P) -> R) -> R {
        f(&mut self.shared.lock().peer)
    }

    /// [`PeerHandle::with`], the closure also told the newest tick the
    /// pump simulated since the UI's last poll (`None`: the peer's world
    /// is as that poll left it), which the UI's next poll reports.
    pub fn with_pumped<R>(&self, f: impl FnOnce(&mut P, Option<u32>) -> R) -> R {
        let mut inner = self.shared.lock();
        let pumped = inner.pumped;
        f(&mut inner.peer, pumped)
    }

    /// The pump's polls so far.
    pub fn pump_polls(&self) -> u64 {
        self.shared.lock().pump_polls
    }

    /// The pump's events the full queue let go so far.
    pub fn lost(&self) -> u64 {
        self.shared.lock().lost
    }

    /// The peer back, its pump stopped first (`release`, `shutdown`); the
    /// pump's events still queued go with the handle.
    pub fn into_peer(mut self) -> P {
        self.stop_pump();
        let PeerHandle { pump: _, shared } = self;
        let shared =
            Arc::into_inner(shared).expect("a handle is its peer's only owner once its pump stopped");
        shared.inner.into_inner().unwrap_or_else(PoisonError::into_inner).peer
    }
}

/// The thread `rl-game net`: wakes every `PUMP_AFTER / 2`, polls the peer
/// when the UI has not for [`PUMP_AFTER`]. Stopping (or dropping) it joins
/// it within one poll.
struct Pump {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Pump {
    fn start<P: Poll>(shared: &Arc<Shared<P>>) -> io::Result<Pump> {
        let stop = Arc::new(AtomicBool::new(false));
        let (s, halt) = (Arc::clone(shared), Arc::clone(&stop));
        let thread = std::thread::Builder::new().name(PUMP_THREAD.into()).spawn(move || run(&s, &halt))?;
        Ok(Pump { stop, thread: Some(thread) })
    }

    fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.thread().unpark();
            if t.join().is_err() {
                eprintln!("warning: the network pump panicked");
            }
        }
    }
}

impl Drop for Pump {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The pump's loop.
fn run<P: Poll>(shared: &Shared<P>, stop: &AtomicBool) {
    loop {
        std::thread::park_timeout(PUMP_AFTER / 2);
        if stop.load(Ordering::Acquire) {
            return;
        }
        if shared.ui_idle(Instant::now()) < PUMP_AFTER {
            continue;
        }
        let mut inner = shared.lock();
        // The UI may have polled while this thread waited for the lock,
        // and a stop may have come.
        if stop.load(Ordering::Acquire) {
            return;
        }
        let now = Instant::now();
        if shared.ui_idle(now) >= PUMP_AFTER {
            inner.pump_poll(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_netcode::world_sim::{ConfirmedTick, TickedTick};

    /// One poll of the fake: who (the pump's thread or not), when it
    /// started and ended.
    #[derive(Clone, Copy, Debug)]
    struct Polled {
        pump: bool,
        start: Instant,
        end: Instant,
    }

    /// A peer that records its polls, takes `delay` for each, and hands
    /// the pump's first poll `batch`.
    struct Fake {
        log: Arc<Mutex<Vec<Polled>>>,
        delay: Duration,
        batch: Vec<OnlineEvent>,
        /// A pump's poll is running (the tests aim the UI's poll into it).
        busy: Arc<AtomicBool>,
    }

    impl Poll for Fake {
        fn poll(
            &mut self,
            _now: Instant,
            local_keys: &mut dyn FnMut() -> Keys,
            _on_tick: &mut dyn FnMut(&World, &TickEvents, TickInfo),
        ) -> Vec<OnlineEvent> {
            let pump = std::thread::current().name() == Some(PUMP_THREAD);
            if pump {
                assert_eq!(local_keys(), Keys(0), "the pump presses nothing");
                self.busy.store(true, Ordering::Release);
            }
            let start = Instant::now();
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            let end = Instant::now();
            self.log.lock().unwrap().push(Polled { pump, start, end });
            self.busy.store(false, Ordering::Release);
            if pump { std::mem::take(&mut self.batch) } else { Vec::new() }
        }
    }

    type Log = Arc<Mutex<Vec<Polled>>>;

    fn fake(delay: Duration, batch: Vec<OnlineEvent>) -> (Fake, Log, Arc<AtomicBool>) {
        let log = Log::default();
        let busy = Arc::new(AtomicBool::new(false));
        (Fake { log: Arc::clone(&log), delay, batch, busy: Arc::clone(&busy) }, log, busy)
    }

    fn ui_poll<P: Poll>(h: &PeerHandle<P>) -> PolledEvents {
        h.poll(Instant::now(), &mut || Keys(Keys::FIRE), &mut |_, _, _| {})
    }

    /// Every pump poll of `log` started at least `PUMP_AFTER` after the
    /// end of the UI's poll before it (a little less allowed for the
    /// clock reads around the fake's own).
    fn pump_polls_wait_for_the_ui(log: &[Polled]) {
        let mut ui_end: Option<Instant> = None;
        for p in log {
            if p.pump {
                if let Some(end) = ui_end {
                    let after = p.start.saturating_duration_since(end);
                    assert!(
                        after + Duration::from_millis(1) >= PUMP_AFTER,
                        "a pump poll {after:?} after the UI's"
                    );
                }
            } else {
                ui_end = Some(p.end);
            }
        }
    }

    /// A UI polling every 5 ms for 1 s: the pump stands back; then no UI
    /// poll for 300 ms: the pump polls about every `PUMP_AFTER / 2`, each
    /// at least `PUMP_AFTER` after the UI's last.
    #[test]
    fn the_pump_polls_only_when_the_ui_does_not() {
        let (f, log, _) = fake(Duration::ZERO, Vec::new());
        let h = PeerHandle::pumped(f);
        assert!(h.is_pumped());
        let start = Instant::now();
        let mut widest = Duration::ZERO;
        let mut last = Instant::now();
        ui_poll(&h);
        while start.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(5));
            widest = widest.max(last.elapsed());
            last = Instant::now();
            ui_poll(&h);
        }
        let busy_ui = h.pump_polls();
        if widest < PUMP_AFTER - Duration::from_millis(2) {
            assert_eq!(busy_ui, 0, "the UI polled every {widest:?} at most: the pump never did");
        } else {
            eprintln!("a loaded machine: the UI's polls were up to {widest:?} apart");
        }
        let quiet = Instant::now();
        std::thread::sleep(Duration::from_millis(300));
        let pumped = h.pump_polls() - busy_ui;
        let took = quiet.elapsed();
        // Polls start `PUMP_AFTER` in, then come every `PUMP_AFTER / 2`.
        let most = (took.saturating_sub(PUMP_AFTER).as_millis() / (PUMP_AFTER / 2).as_millis()) as u64 + 2;
        assert!((10..=most).contains(&pumped), "{pumped} pump polls in {took:?} without the UI");
        drop(h);
        pump_polls_wait_for_the_ui(&log.lock().unwrap());
    }

    fn ticked(tick: u32) -> OnlineEvent {
        OnlineEvent::Ticked(vec![TickedTick {
            tick,
            resim: false,
            substeps: 1,
            lost: None,
            events: Vec::new(),
        }])
    }

    fn confirmed(tick: u32) -> OnlineEvent {
        OnlineEvent::Confirmed(vec![ConfirmedTick {
            tick,
            events: Vec::new(),
            scores: Vec::new(),
            time_left_ms: None,
            result: None,
            positions: Vec::new(),
        }])
    }

    /// 10 000 events from one pump poll: the presentation's (`Ticked`)
    /// never wait (the UI hears the newest pumped tick instead), the
    /// others wait in order, at most `PUMP_QUEUE`: the oldest `Confirmed`
    /// go first, every chat line stays, the loss counted.
    #[test]
    fn the_pumps_queue_is_bounded() {
        let batch: Vec<OnlineEvent> = (0..10_000u32)
            .map(|i| match i % 10 {
                0 | 2 | 4 | 6 | 8 => ticked(i),
                1 => OnlineEvent::Chat { from: "a".into(), member: 1, text: i.to_string() },
                _ => confirmed(i),
            })
            .collect();
        let chats = batch.iter().filter(|e| matches!(e, OnlineEvent::Chat { .. })).count();
        let confirms = batch.iter().filter(|e| matches!(e, OnlineEvent::Confirmed(_))).count();
        assert_eq!((chats, confirms), (1_000, 4_000));
        let (f, _, _) = fake(Duration::ZERO, batch);
        let h = PeerHandle::pumped(f);
        let start = Instant::now();
        while h.pump_polls() == 0 {
            assert!(start.elapsed() < Duration::from_secs(2), "the pump polls an idle UI's peer");
            std::thread::sleep(Duration::from_millis(5));
        }
        let lost = chats + confirms - PUMP_QUEUE;
        assert_eq!(h.lost(), lost as u64, "each loss counted");
        let p = ui_poll(&h);
        assert_eq!(p.pumped, Some(9_998), "the newest tick the pump ran");
        assert_eq!(p.events.len(), PUMP_QUEUE, "at most PUMP_QUEUE waited");
        assert!(p.events.iter().all(|e| !matches!(e, OnlineEvent::Ticked(_))), "no presentation waits");
        let kept_chats = p.events.iter().filter(|e| matches!(e, OnlineEvent::Chat { .. })).count();
        assert_eq!(kept_chats, chats, "every other event kept");
        // The oldest confirmed ticks went; the rest are in order.
        let ticks: Vec<u32> = p
            .events
            .iter()
            .filter_map(|e| match e {
                OnlineEvent::Confirmed(c) => Some(c[0].tick),
                _ => None,
            })
            .collect();
        assert_eq!(ticks.len(), confirms - lost);
        assert!(ticks.windows(2).all(|w| w[0] < w[1]), "in order");
        assert_eq!(ticks.last(), Some(&9_999), "the newest kept");
        let first_kept = ticks[0];
        assert!(first_kept > 1_000, "the oldest went: the first kept is {first_kept}");
        let again = ui_poll(&h);
        assert!(again.events.is_empty() && again.pumped.is_none(), "handed over once");
    }

    /// A pump poll takes 5 ms: the UI's poll, called while one runs,
    /// waits for that poll alone (it starts within a millisecond or two
    /// of that poll's end), never for the pump's sleep or a second poll.
    #[test]
    fn the_ui_waits_at_most_one_pump_poll() {
        let delay = Duration::from_millis(5);
        let (f, log, busy) = fake(delay, Vec::new());
        let h = PeerHandle::pumped(f);
        let mut contended = 0;
        for round in 0..10 {
            // Idle until a pump poll runs, then poll into it.
            let start = Instant::now();
            while !busy.load(Ordering::Acquire) {
                assert!(start.elapsed() < Duration::from_secs(2), "round {round}: the pump polls");
                std::thread::yield_now();
            }
            let called = Instant::now();
            ui_poll(&h);
            let returned = called.elapsed();
            let polls = log.lock().unwrap().clone();
            let mine = polls.iter().rev().find(|p| !p.pump).copied().unwrap();
            let theirs = polls.iter().rev().find(|p| p.pump && p.start <= called).copied().unwrap();
            let waited = mine.start.saturating_duration_since(called);
            let own = mine.end.saturating_duration_since(mine.start);
            // Its own poll began once the pump's ended, not later.
            let after_theirs = mine.start.saturating_duration_since(theirs.end);
            assert!(
                after_theirs < Duration::from_millis(2),
                "round {round}: started {after_theirs:?} after the pump's poll"
            );
            assert!(waited <= theirs.end.saturating_duration_since(called) + Duration::from_millis(2));
            assert!(returned >= own, "{returned:?} {own:?}");
            contended += usize::from(waited >= Duration::from_millis(1));
        }
        assert!(contended >= 5, "the UI's polls met the pump's: {contended} of 10");
    }

    /// Stopping the pump joins its thread within 100 ms, and the handle
    /// gives the peer back.
    #[test]
    fn stopping_the_pump_joins_it() {
        let (f, log, _) = fake(Duration::from_millis(5), Vec::new());
        let h = PeerHandle::pumped(f);
        let start = Instant::now();
        while h.pump_polls() < 3 {
            assert!(start.elapsed() < Duration::from_secs(2), "the pump polls");
            std::thread::sleep(Duration::from_millis(2));
        }
        let stopping = Instant::now();
        let peer = h.into_peer();
        assert!(stopping.elapsed() < Duration::from_millis(100), "joined in {:?}", stopping.elapsed());
        let n = log.lock().unwrap().len();
        assert!(Arc::ptr_eq(&peer.log, &log), "the same peer back");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(log.lock().unwrap().len(), n, "nothing polls it any more");
    }

    /// A second lock of the same peer on one thread panics instead of
    /// deadlocking.
    #[test]
    #[should_panic(expected = "locked twice")]
    fn a_nested_lock_panics() {
        let (f, _, _) = fake(Duration::ZERO, Vec::new());
        let h = PeerHandle::new(f);
        h.with(|_| h.with(|_| ()));
    }
}
