//! The network pump (ruling PF12c-2, amended by PU-1): a thread
//! `rl-game net` that polls an online peer whenever the UI thread has not
//! polled it for [`PUMP_AFTER`].
//!
//! The game's network, netcode and bots run inside `OnlinePeer::poll`,
//! which the UI thread calls once a frame. A frame that does not come (a
//! window dragged: Windows' modal move loop blocks winit's event loop; a
//! long match load) would silence the peer, and after 2 s its peers give up
//! on it. The pump polls in its place, so only a whole-machine stall
//! silences a peer. A slow frame is no stall (PU-1): [`PUMP_AFTER`] is
//! 100 ms, so frames at 10 fps and up play exactly as before, every tick
//! with the player's keys and drawn.
//!
//! **The lock** ([`PeerHandle`]): the peer sits behind one `Mutex`, held
//! for one poll (the UI's or the pump's) or one [`PeerHandle::with`] call,
//! never across rendering, and neither thread waits on anything else while
//! holding it: the UI waits at most for one pump poll
//! (`the_ui_waits_at_most_one_pump_poll`). The UI's last poll is an atomic,
//! so the pump checks it without the lock.
//!
//! **While the pump polls**, the local player presses the keys the UI last
//! latched for [`PUMP_HOLD`] of the stall (a short stall plays as the
//! frames would have), then nothing (a long drag does not keep firing); the
//! input latch is the UI's, untouched; nothing is drawn (no `on_tick`), and
//! the host's bots think inside the peer as always. Its events wait for
//! the UI's next poll, except the match's presentation (`Ticked`,
//! `Restored`, `StateLoaded`): those are dropped, and the UI's next poll
//! says so ([`PolledEvents::pumped`]), so the presentation starts again
//! from the world as after a loaded state. At most [`PUMP_QUEUE`] events
//! wait: beyond it the oldest `Confirmed` ticks are thinned to what the
//! HUD's log, tally and results read (kills, spawns, messages, the end),
//! then the oldest chat-like events go; control events (the match's start
//! and end, the lobby, the connection's) never go (ruling PU-2).
//!
//! The ticks the pump runs are the netcode's like any other: the peer
//! cannot tell which thread polled it, and its world is the same function
//! of the match's final inputs (the keys the pump presses are this
//! player's inputs for those ticks, sent to everyone).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rl_netcode::TickInfo;
use rl_netcode::world_sim::ConfirmedTick;
use rl_online::{OnlineEvent, OnlinePeer};
use rl_sim::{EventKind, Keys, TickEvents, World};

/// The UI thread has not polled a peer for this long: the pump polls it
/// (ruling PU-1: far inside N70's 1.5 s and the peers' 2 s, and far above
/// any frame a played game draws).
pub const PUMP_AFTER: Duration = Duration::from_millis(100);
/// The pump wakes this often, and so polls this often during a stall.
pub const PUMP_EVERY: Duration = Duration::from_millis(10);
/// The pump presses the keys the UI last latched for this long of a stall,
/// then nothing (ruling PU-1).
pub const PUMP_HOLD: Duration = Duration::from_millis(250);
/// At most this many of the pump's events wait for the UI: beyond it the
/// oldest `Confirmed` are thinned into one, then the oldest chat-like
/// events go (each counted, the first logged); control events never go
/// (ruling PU-2), so only they can pass the bound. The match's
/// presentation (`Ticked`, `Restored`, `StateLoaded`) never waits:
/// [`PolledEvents::pumped`].
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
    /// The pump's poll panicked: it stopped (said at once, minor 4).
    dead: AtomicBool,
}

struct Inner<P> {
    peer: P,
    /// The pump's events waiting for the UI.
    queue: Queue,
    /// The newest tick the pump simulated since the UI's last poll.
    pumped: Option<u32>,
    /// The UI's last `now` and the real time it was given at: the pump's
    /// time goes on from there (the UI's clock may be a test's).
    ui_clock: Option<(Instant, Instant)>,
    /// The last time given to the peer: never earlier again.
    last_now: Option<Instant>,
    /// The keys the UI's local player last latched (a tick of its polls).
    latched: Keys,
    /// The time of the pump's first poll since the UI's last: the stall's
    /// start, for [`PUMP_HOLD`].
    stall: Option<Instant>,
    /// Why the pump stopped, for the UI's next poll to say.
    died: Option<String>,
    /// The pump's polls so far.
    pump_polls: u64,
}

/// Whether `e` may be let go when the queue is full (ruling PU-2): chat
/// and reports a later one supersedes. Everything else is a control event
/// (a match's start and end, the lobby, lagging, a lost connection, a
/// replay or a desync bundle, a join's steps) and stays; `Confirmed` is
/// thinned, never dropped whole.
fn evictable(e: &OnlineEvent) -> bool {
    matches!(
        e,
        OnlineEvent::Chat { .. }
            | OnlineEvent::Notice(_)
            | OnlineEvent::Left(_)
            | OnlineEvent::Downloads(_)
            | OnlineEvent::Reachability(_)
            | OnlineEvent::RelayJoin { .. }
            | OnlineEvent::ListenRelay { .. }
    )
}

/// What the HUD's bookkeeping (`Session::after_tick`: log, tally, results)
/// needs of a confirmed tick the queue thins: its kills, spawns, messages
/// and end, with the end's scores; no damage popup, no positions, no
/// scores else. `None` for a tick with none of these.
fn thin(mut t: ConfirmedTick) -> Option<ConfirmedTick> {
    t.events.retain(|e| {
        matches!(
            e.kind,
            EventKind::Kill { .. }
                | EventKind::Spawned { .. }
                | EventKind::Message(_)
                | EventKind::GameOver(_)
        )
    });
    if t.events.is_empty() {
        return None;
    }
    if !t.events.iter().any(|e| matches!(e.kind, EventKind::GameOver(_))) {
        t.scores = Vec::new();
    }
    t.positions = Vec::new();
    Some(t)
}

/// The pump's events waiting for the UI, bounded by [`PUMP_QUEUE`].
#[derive(Default)]
struct Queue {
    events: VecDeque<OnlineEvent>,
    /// The oldest `Confirmed` holds the thinned ticks (it stays the oldest
    /// until the UI takes the queue).
    thinned: bool,
    /// Events let go or thinned into another.
    lost: u64,
}

impl Queue {
    /// `e` queued; when the queue is full, room made first: the second
    /// oldest `Confirmed` thinned into the oldest (which holds the thinned
    /// ticks, in order), else the oldest evictable event gone, else (only
    /// control events and one `Confirmed`) the bound passed.
    fn push(&mut self, e: OnlineEvent) {
        if self.events.len() >= PUMP_QUEUE && self.make_room() {
            self.lost += 1;
            if self.lost == 1 {
                eprintln!(
                    "warning: the network pump's queue is full ({PUMP_QUEUE} events): the oldest \
                     confirmed ticks are thinned, then chat goes (counted; logged once)"
                );
            }
        }
        self.events.push_back(e);
    }

    /// Every event, for the UI.
    fn take(&mut self) -> Vec<OnlineEvent> {
        self.thinned = false;
        self.events.drain(..).collect()
    }

    fn make_room(&mut self) -> bool {
        let mut confirmed = self
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, OnlineEvent::Confirmed(_)))
            .map(|(i, _)| i);
        if let (Some(first), Some(second)) = (confirmed.next(), confirmed.next()) {
            let Some(OnlineEvent::Confirmed(next)) = self.events.remove(second) else { return false };
            if let Some(OnlineEvent::Confirmed(acc)) = self.events.get_mut(first) {
                if !self.thinned {
                    *acc = std::mem::take(acc).into_iter().filter_map(thin).collect();
                    self.thinned = true;
                }
                acc.extend(next.into_iter().filter_map(thin));
            }
            return true;
        }
        match self.events.iter().position(evictable) {
            Some(at) => {
                self.events.remove(at);
                true
            }
            None => false,
        }
    }
}

thread_local! {
    /// The peer locks this thread holds (their addresses), to catch a
    /// second lock of one of them on this thread (directly or through
    /// another peer's), which would deadlock.
    static HELD: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// The lock held, and the thread's note of it.
struct Held<'a, P> {
    guard: MutexGuard<'a, Inner<P>>,
    addr: usize,
}

impl<P> Drop for Held<'_, P> {
    fn drop(&mut self) {
        HELD.with(|h| {
            let mut h = h.borrow_mut();
            if let Some(i) = h.iter().rposition(|&a| a == self.addr) {
                h.remove(i);
            }
        });
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

impl<P: Poll> Inner<P> {
    /// `now`, never before the last time the peer was given.
    fn clamp(&mut self, now: Instant) -> Instant {
        let now = self.last_now.map_or(now, |l| now.max(l));
        self.last_now = Some(now);
        now
    }

    /// One poll by the pump at the real time `real`: the keys the UI last
    /// latched for [`PUMP_HOLD`] of the stall, then none; nothing drawn;
    /// its events queued.
    fn pump_poll(&mut self, real: Instant) {
        let now = self.ui_clock.map_or(real, |(ui, at)| ui + real.saturating_duration_since(at));
        let now = self.clamp(now);
        let stall = *self.stall.get_or_insert(now);
        let keys = if now.saturating_duration_since(stall) < PUMP_HOLD { self.latched } else { Keys(0) };
        let events = self.peer.poll(now, &mut || keys, &mut |_, _, _| {});
        self.pump_polls += 1;
        for e in events {
            self.queue_event(e);
        }
    }

    /// One of the pump's events kept for the UI: the presentation's
    /// dropped (`pumped` notes it), the rest queued.
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
        self.queue.push(e);
    }
}

impl<P: Poll> Shared<P> {
    /// The lock (a poll that panicked leaves the peer as it was: it is
    /// used on). A second lock of a peer this thread holds panics instead
    /// of deadlocking.
    fn lock(&self) -> Held<'_, P> {
        let addr = std::ptr::from_ref(self) as usize;
        let again = HELD.with(|h| h.borrow().contains(&addr));
        assert!(!again, "a peer locked twice on one thread (PeerHandle::with inside another): a deadlock");
        let guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        HELD.with(|h| h.borrow_mut().push(addr));
        Held { guard, addr }
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
            queue: Queue::default(),
            pumped: None,
            ui_clock: None,
            last_now: None,
            latched: Keys(0),
            stall: None,
            died: None,
            pump_polls: 0,
        };
        let shared = Shared {
            inner: Mutex::new(inner),
            epoch,
            ui_polled: AtomicU64::new(0),
            dead: AtomicBool::new(false),
        };
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
            self.shared.dead.store(false, Ordering::Release);
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

    /// Whether a pump runs (one whose poll panicked stopped).
    pub fn is_pumped(&self) -> bool {
        self.pump.is_some() && !self.shared.dead.load(Ordering::Acquire)
    }

    /// The UI thread's poll at `now`: as `OnlinePeer::poll`, the pump's
    /// queued events first (and, once, a notice when the pump stopped on a
    /// panic), then this poll's; marks the UI's poll time and keeps the
    /// keys `local_keys` last gave (the pump's to press in a stall). `now`
    /// is never taken earlier than a time the peer was already given (the
    /// pump may have polled since the caller read its clock).
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
        inner.stall = None;
        let mut events = inner.queue.take();
        if let Some(why) = inner.died.take() {
            events.push(OnlineEvent::Notice(format!(
                "The network pump stopped ({why}): a window held still may now drop this game."
            )));
        }
        let pumped = inner.pumped.take();
        let mut latched = None;
        let mut keys = || {
            let k = local_keys();
            latched = Some(k);
            k
        };
        events.extend(inner.peer.poll(now, &mut keys, on_tick));
        if let Some(k) = latched {
            inner.latched = k;
        }
        self.shared.mark_ui(Instant::now());
        PolledEvents { events, pumped }
    }

    /// Anything else on the peer (lobby actions, state, stats, its
    /// world): the lock, held for the closure only. The closure must not
    /// use this handle again (that would deadlock: it panics instead).
    pub fn with<R>(&self, f: impl FnOnce(&mut P) -> R) -> R {
        f(&mut self.shared.lock().peer)
    }

    /// [`PeerHandle::with`], the closure also handed the newest tick the
    /// pump simulated since the UI's last poll (`None`: the peer's world
    /// is as that poll left it), taken: the caller starts its
    /// presentation again from the world now, so the next poll does not
    /// report those ticks again (minor 6).
    pub fn with_pumped<R>(&self, f: impl FnOnce(&mut P, Option<u32>) -> R) -> R {
        let mut inner = self.shared.lock();
        let pumped = inner.pumped.take();
        f(&mut inner.peer, pumped)
    }

    /// The pump's polls so far.
    pub fn pump_polls(&self) -> u64 {
        self.shared.lock().pump_polls
    }

    /// The pump's events the full queue let go or thinned so far.
    pub fn lost(&self) -> u64 {
        self.shared.lock().queue.lost
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

/// The thread `rl-game net`: wakes every [`PUMP_EVERY`], polls the peer
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
            // A panicking poll is caught and said in `run`; this is anything else.
            if t.join().is_err() {
                eprintln!("warning: the network pump's thread panicked");
            }
        }
    }
}

impl Drop for Pump {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The pump's loop. A poll that panics (a bug) stops it at once, said on
/// the console now and to the UI's next poll (`Inner::died`, a notice);
/// the UI's frames poll the peer on.
fn run<P: Poll>(shared: &Shared<P>, stop: &AtomicBool) {
    loop {
        std::thread::park_timeout(PUMP_EVERY);
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
        if shared.ui_idle(now) < PUMP_AFTER {
            continue;
        }
        let polled = std::panic::catch_unwind(AssertUnwindSafe(|| inner.pump_poll(now)));
        if let Err(p) = polled {
            let why = p
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic".into());
            eprintln!(
                "warning: the network pump stopped: its poll panicked ({why}); the frames alone poll the peer"
            );
            inner.died = Some(why);
            shared.dead.store(true, Ordering::Release);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rl_math::Vec2;
    use rl_netcode::world_sim::{ConfirmedTick, TickedTick};
    use rl_sim::{Event, EventKind};

    /// One poll of the fake: who (the pump's thread or not), when it
    /// started and ended, and the keys it pressed.
    #[derive(Clone, Copy, Debug)]
    struct Polled {
        pump: bool,
        start: Instant,
        end: Instant,
        keys: Keys,
    }

    /// A peer that records its polls (pressing the local keys once each,
    /// as a poll running one tick), takes `delay` for each, and hands the
    /// pump's polls `batches`, one each; with `panics`, the pump's first
    /// poll panics.
    struct Fake {
        log: Arc<Mutex<Vec<Polled>>>,
        delay: Duration,
        batches: VecDeque<Vec<OnlineEvent>>,
        /// A pump's poll is running (the tests aim the UI's poll into it).
        busy: Arc<AtomicBool>,
        panics: bool,
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
                assert!(!self.panics, "a fake peer's poll panics");
                self.busy.store(true, Ordering::Release);
            }
            let keys = local_keys();
            let start = Instant::now();
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            let end = Instant::now();
            self.log.lock().unwrap().push(Polled { pump, start, end, keys });
            self.busy.store(false, Ordering::Release);
            if pump { self.batches.pop_front().unwrap_or_default() } else { Vec::new() }
        }
    }

    type Log = Arc<Mutex<Vec<Polled>>>;

    fn fake(delay: Duration, batch: Vec<OnlineEvent>) -> (Fake, Log, Arc<AtomicBool>) {
        let log = Log::default();
        let busy = Arc::new(AtomicBool::new(false));
        let f = Fake {
            log: Arc::clone(&log),
            delay,
            batches: VecDeque::from([batch]),
            busy: Arc::clone(&busy),
            panics: false,
        };
        (f, log, busy)
    }

    fn ui_poll<P: Poll>(h: &PeerHandle<P>) -> PolledEvents {
        ui_poll_with(h, Keys(Keys::FIRE))
    }

    fn ui_poll_with<P: Poll>(h: &PeerHandle<P>, keys: Keys) -> PolledEvents {
        h.poll(Instant::now(), &mut || keys, &mut |_, _, _| {})
    }

    /// Waits (at most 2 s) until the pump polled `n` times.
    fn until_pumped<P: Poll>(h: &PeerHandle<P>, n: u64) {
        let start = Instant::now();
        while h.pump_polls() < n {
            assert!(start.elapsed() < Duration::from_secs(2), "the pump polls an idle UI's peer");
            std::thread::sleep(Duration::from_millis(2));
        }
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

    /// The UI polling every `every` for `d`: the widest gap between its
    /// polls.
    fn ui_every<P: Poll>(h: &PeerHandle<P>, every: Duration, d: Duration) -> Duration {
        let start = Instant::now();
        let mut widest = Duration::ZERO;
        let mut last = Instant::now();
        ui_poll(h);
        while start.elapsed() < d {
            std::thread::sleep(every.saturating_sub(last.elapsed()));
            widest = widest.max(last.elapsed());
            last = Instant::now();
            ui_poll(h);
        }
        widest
    }

    /// Ruling PU-1: a UI polling every 5 ms, then every 30 ms and every
    /// 45 ms (slow frames: 33 and 22 fps), for a second each: the pump
    /// stands back; then no UI poll for 400 ms: the pump polls about every
    /// `PUMP_EVERY` from `PUMP_AFTER` on, each at least `PUMP_AFTER` after
    /// the UI's last.
    #[test]
    fn the_pump_polls_only_when_the_ui_does_not() {
        let (f, log, _) = fake(Duration::ZERO, Vec::new());
        let h = PeerHandle::pumped(f);
        assert!(h.is_pumped());
        for every in [5, 30, 45].map(Duration::from_millis) {
            let widest = ui_every(&h, every, Duration::from_secs(1));
            // Slow frames are no stall: only a machine that lost the
            // pace (a gap far beyond the frames') may see the pump.
            if widest < every + Duration::from_millis(15) {
                assert_eq!(
                    h.pump_polls(),
                    0,
                    "the UI polled every {every:?} ({widest:?} at most): the pump never did"
                );
            } else {
                eprintln!("a loaded machine: the UI's polls were up to {widest:?} apart");
            }
        }
        let busy_ui = h.pump_polls();
        let quiet = Instant::now();
        std::thread::sleep(Duration::from_millis(400));
        let pumped = h.pump_polls() - busy_ui;
        let took = quiet.elapsed();
        let most = (took.saturating_sub(PUMP_AFTER).as_millis() / PUMP_EVERY.as_millis()) as u64 + 2;
        assert!((10..=most).contains(&pumped), "{pumped} pump polls in {took:?} without the UI");
        drop(h);
        pump_polls_wait_for_the_ui(&log.lock().unwrap());
    }

    /// Ruling PU-1: the pump presses the keys the UI last latched for
    /// `PUMP_HOLD` of pumping (a short stall plays as the frames would
    /// have), then nothing (a long drag does not keep firing); a UI that
    /// last latched nothing gets nothing.
    #[test]
    fn the_pump_presses_the_last_latched_keys_for_a_while() {
        let (f, log, _) = fake(Duration::ZERO, Vec::new());
        let h = PeerHandle::pumped(f);
        let held = Keys(Keys::FIRE | Keys::RIGHT);
        for latched in [held, Keys(0)] {
            ui_poll_with(&h, latched);
            let from = log.lock().unwrap().len();
            std::thread::sleep(PUMP_AFTER + PUMP_HOLD + Duration::from_millis(250));
            let polls: Vec<Polled> = log.lock().unwrap()[from..].to_vec();
            assert!(polls.iter().all(|p| p.pump));
            let first = polls.first().expect("the pump polled").start;
            let (mut kept, mut zeroed) = (0, 0);
            for p in &polls {
                let into = p.start.saturating_duration_since(first);
                if into + Duration::from_millis(5) < PUMP_HOLD {
                    assert_eq!(p.keys, latched, "{into:?} into the stall: the last latched keys");
                    kept += 1;
                } else if into > PUMP_HOLD + Duration::from_millis(5) {
                    assert_eq!(p.keys, Keys(0), "{into:?} into the stall: nothing");
                    zeroed += 1;
                }
            }
            assert!(kept >= 5 && zeroed >= 5, "{kept} polls held, {zeroed} let go");
        }
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

    /// A confirmed tick; every 20th carries a kill, and a damage.
    fn confirmed(tick: u32) -> OnlineEvent {
        let kind = |kind| Event { substep: 0, death: false, kind };
        let events = if tick % 20 == 3 {
            vec![
                kind(EventKind::Damage { worm: 1, by: 0, amount: 5.0, dealt: 5.0 }),
                kind(EventKind::Kill { victim: 1, killer: 0 }),
            ]
        } else {
            Vec::new()
        };
        OnlineEvent::Confirmed(vec![ConfirmedTick {
            tick,
            events,
            scores: Vec::new(),
            time_left_ms: Some(u64::from(tick)),
            result: None,
            positions: vec![(1, Vec2::new(1.0, 2.0))],
        }])
    }

    fn is_control(e: &OnlineEvent) -> bool {
        matches!(
            e,
            OnlineEvent::Lobby(_)
                | OnlineEvent::MatchStarting { .. }
                | OnlineEvent::MatchEnded(_)
                | OnlineEvent::Disconnected(_)
                | OnlineEvent::Kicked { .. }
                | OnlineEvent::Refused(_)
                | OnlineEvent::Lagging(_)
                | OnlineEvent::Connected { .. }
                | OnlineEvent::DownloadPrompt { .. }
        )
    }

    /// The ticks the confirmed events of `events` hold, in order.
    fn confirmed_ticks(events: &[OnlineEvent]) -> Vec<&ConfirmedTick> {
        events
            .iter()
            .flat_map(|e| match e {
                OnlineEvent::Confirmed(c) => c.as_slice(),
                _ => &[],
            })
            .collect()
    }

    /// 10 000 events from one pump poll: the presentation's (`Ticked`)
    /// never wait (the UI hears the newest pumped tick instead), the
    /// others wait in order, at most `PUMP_QUEUE`: the oldest `Confirmed`
    /// are thinned first (minor 3: every tick with a kill, a spawn, a log
    /// message or the end is kept, without its damage or positions),
    /// every chat line and control event stays, the loss counted.
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
        until_pumped(&h, 1);
        assert!(h.lost() > 0, "each loss counted");
        let p = ui_poll(&h);
        assert_eq!(p.pumped, Some(9_998), "the newest tick the pump ran");
        assert!(p.events.len() <= PUMP_QUEUE, "at most PUMP_QUEUE waited: {}", p.events.len());
        assert!(p.events.iter().all(|e| !matches!(e, OnlineEvent::Ticked(_))), "no presentation waits");
        let kept_chats = p.events.iter().filter(|e| matches!(e, OnlineEvent::Chat { .. })).count();
        assert_eq!(kept_chats, chats, "every chat line kept: the confirmed ticks are thinned first");
        let ticks = confirmed_ticks(&p.events);
        assert!(ticks.windows(2).all(|w| w[0].tick < w[1].tick), "in order");
        assert_eq!(ticks.last().map(|t| t.tick), Some(9_999), "the newest kept");
        // Every kill is still there; the thinned ticks lost their damage
        // and positions, the newest are whole.
        let kills =
            ticks.iter().filter(|t| t.events.iter().any(|e| matches!(e.kind, EventKind::Kill { .. })));
        assert_eq!(kills.count(), 500, "every kill kept");
        assert!(ticks.len() < confirms, "the ticks without a kill went first");
        let whole = ticks.iter().rev().take(100).all(|t| t.positions.len() == 1);
        assert!(whole, "the newest ticks are whole");
        let again = ui_poll(&h);
        assert!(again.events.is_empty() && again.pumped.is_none(), "handed over once");
    }

    /// Ruling PU-2: control events (a match's start and end, the lobby, a
    /// lost connection) are never let go; beyond `PUMP_QUEUE` the oldest
    /// other events (chat) go.
    #[test]
    fn the_pumps_queue_never_lets_control_events_go() {
        let control = [
            OnlineEvent::Connected { path: rl_proto::Path::Direct, remote: ([127, 0, 0, 1], 9).into() },
            OnlineEvent::DownloadPrompt { bytes: 7 },
            OnlineEvent::Lagging(true),
            OnlineEvent::Kicked { banned: false },
            OnlineEvent::Refused("no".into()),
            OnlineEvent::Disconnected("gone".into()),
        ];
        let mut batch: Vec<OnlineEvent> = Vec::new();
        for i in 0..6_000u32 {
            if i % 1_000 == 0 {
                batch.extend(control.iter().cloned());
            }
            batch.push(OnlineEvent::Chat { from: "a".into(), member: 1, text: i.to_string() });
        }
        let (f, _, _) = fake(Duration::ZERO, batch);
        let h = PeerHandle::pumped(f);
        until_pumped(&h, 1);
        let p = ui_poll(&h);
        assert_eq!(p.events.len(), PUMP_QUEUE, "the bound holds while other events can go");
        let kept: Vec<&OnlineEvent> = p.events.iter().filter(|e| is_control(e)).collect();
        let want: Vec<&OnlineEvent> = (0..6).flat_map(|_| control.iter()).collect();
        assert_eq!(kept, want, "every control event, in order");
        let first_chat = p.events.iter().find_map(|e| match e {
            OnlineEvent::Chat { text, .. } => text.parse::<u32>().ok(),
            _ => None,
        });
        assert!(first_chat.is_some_and(|t| t > 1_000), "the oldest chat went: {first_chat:?}");
        assert_eq!(h.lost(), (6_000 + 36 - PUMP_QUEUE) as u64);
    }

    /// Minor 6: `with_pumped` takes the pump's mark, so a frame that
    /// resynced under it is not resynced again by the next poll.
    #[test]
    fn with_pumped_takes_the_mark() {
        let (f, _, _) = fake(Duration::ZERO, vec![ticked(5)]);
        let h = PeerHandle::pumped(f);
        until_pumped(&h, 1);
        assert_eq!(h.with_pumped(|_, p| p), Some(5));
        assert_eq!(h.with_pumped(|_, p| p), None, "taken");
        assert_eq!(ui_poll(&h).pumped, None, "the next poll reports nothing again");
    }

    /// Minor 4: a pump poll that panics is reported at once (the pump
    /// stops, `is_pumped` says so, logged) and the UI's next poll carries
    /// a notice; the UI goes on polling the peer.
    #[test]
    fn a_pump_panic_is_reported_at_once() {
        let (mut f, log, _) = fake(Duration::ZERO, Vec::new());
        f.panics = true;
        let h = PeerHandle::pumped(f);
        let start = Instant::now();
        while h.is_pumped() {
            assert!(start.elapsed() < Duration::from_secs(2), "the pump's end is known at once");
            std::thread::sleep(Duration::from_millis(5));
        }
        let p = ui_poll(&h);
        let said = p.events.iter().any(|e| matches!(e, OnlineEvent::Notice(t) if t.contains("network pump")));
        assert!(said, "{:?}", p.events);
        assert!(log.lock().unwrap().last().is_some_and(|p| !p.pump), "the UI polls on");
        assert!(!ui_poll(&h).events.iter().any(|e| matches!(e, OnlineEvent::Notice(_))), "said once");
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

    /// Minor 5: peer A locked again inside B inside A on one thread panics
    /// too (a deadlock otherwise).
    #[test]
    fn a_nested_lock_through_another_peer_panics() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (a, b) = (
                PeerHandle::new(fake(Duration::ZERO, Vec::new()).0),
                PeerHandle::new(fake(Duration::ZERO, Vec::new()).0),
            );
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                a.with(|_| b.with(|_| a.with(|_| ())))
            }));
            let _ = tx.send(r.is_err());
        });
        let panicked = rx.recv_timeout(Duration::from_secs(2)).expect("no deadlock: A inside B inside A");
        assert!(panicked, "A inside B inside A panics");
    }
}
