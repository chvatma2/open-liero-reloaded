//! The rollback session (networking spec §3.1-§3.3, §3.6): one struct for the
//! host, a client and a spectator, the role deciding the branches. The host
//! orders inputs (what it forwards is final), every player peer predicts the
//! inputs it lacks and rolls back when a final input differs, spectators
//! simulate confirmed ticks only.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use core::mem;

use olr_proto::{
    ClientInputs, Control, Datagram, Frame, HostInputs, HostResume, MAX_DATAGRAM, MAX_FRAMES, MAX_KEYS,
    OVERHEAD, Stamp, decode_control, decode_datagram, decode_frames, encode_control, encode_datagram,
    encode_frames,
};
use olr_sim::Keys;

use crate::clock::{JUMP_TICKS, MARGIN_SAMPLES, MAX_ADJUST_PPM, Margins, Pace, TARGET_MARGIN};
use crate::inputs::{InputLog, SetResult};
use crate::migration::HOST_SILENT_US;
use crate::sync::{
    Checks, Incoming, LOAD_TIMEOUT_US, MAX_REPAIRS, PEER_STREAM_KEEP, REPAIR_MARGIN, REPORT_WINDOW, Remote,
    Repair, state_controls,
};
use crate::{Channel, PeerId, Simulation};

mod migrate;

/// One tick lasts 10 ms (100 ticks per second, OLX's `FRAME_MS`).
pub const TICK_US: u64 = 10_000;
/// The host's fill deadline in ticks (spec §3.2's rollback window).
pub const FILL_WINDOW: u32 = 12;
/// A player peer predicts at most this many ticks past its frontier
/// (decision N1, amended by ruling T7-A). A client's frontier also waits for
/// the slowest client's inputs, up to the fill window, so 20 stalled under
/// `bad` (10 seeds × 4 clients, stall events: 20 → 310, 22 → 68, 24 → 14,
/// 26 → 3, 28 → 0).
pub const MAX_PREDICTION: u32 = 28;
/// Peers compare checksums every this many confirmed ticks (spec §3.5).
pub const CHECKSUM_EVERY: u32 = 10;
/// At most this many new ticks per [`Session::advance`].
pub const MAX_CATCHUP: u32 = 10;
/// Received inputs this many ticks or more past the current tick are
/// ignored: a bound on what a hostile peer can make the log hold. An honest
/// peer is never more than a few hundred ticks ahead.
const MAX_AHEAD: u32 = 6_000;
/// A client whose inputs miss the fill window this long gets a warning
/// ([`Event::PeerLate`], spec §3.2's "connection problem").
pub const LATE_WARNING_US: u64 = 3_000_000;
/// A client whose filled inputs add up to this many ticks within
/// [`LAGGING_WINDOW_US`] is lagging ([`Event::Lagging`]): 3 s worth.
pub const LAGGING_TICKS: u32 = 300;
/// The window [`LAGGING_TICKS`] are counted over.
pub const LAGGING_WINDOW_US: u64 = 4_000_000;
/// A lagging client is no longer lagging after this long without `Late`.
pub const LAGGING_CLEAR_US: u64 = 2_000_000;
/// Nothing heard from a peer this long: [`Event::PeerTimedOut`].
pub const SILENCE_TIMEOUT_US: u64 = 10_000_000;
/// The pace's speed change per tick of margin error (0.2 %).
const ADJUST_PPM_PER_TICK: i64 = 2_000;
/// The integral term's growth per tick of margin error and host tick: a
/// clock drift leaves no lasting margin error (critically damped with the
/// proportional term, time constant about 10 s).
const INTEGRAL_PPM_PER_TICK: i64 = 1;
/// A jump holds a client back at most this many ticks: an honest client is
/// never further ahead than its prediction limit. A hostile margin of
/// `i16::MAX` would otherwise stop it for minutes.
const MAX_JUMP_BACK: i32 = MAX_PREDICTION as i32;
/// A jump moves a client ahead at most this many ticks (the silence
/// timeout's worth: further behind, the host has given up on it).
const MAX_JUMP_AHEAD: i32 = (SILENCE_TIMEOUT_US / TICK_US) as i32;
/// The host's tick in its datagrams is taken for the integral term only
/// when it is at most this far past our own current tick: one hostile
/// value must not pin the term's clock.
const HOST_TICK_SLACK: u32 = 100;
/// The host repeats a change to its input log in this many datagrams to
/// each peer (redundancy by repetition, not by resending every
/// unacknowledged frame in every datagram: spec §3.1's budget).
pub const COPIES: usize = 4;
/// A peer whose round trip is at most this long gets each change of the
/// input log in [`COPIES`] datagrams; a longer one (or one not measured yet)
/// every frame it has not acknowledged in every datagram, as a lost burst
/// longer than `COPIES` costs one more round trip (the resend): twice the
/// round trip plus the burst must stay under [`MAX_PREDICTION`] (2 × 10 + 4
/// ticks, with room for jitter), or a client stalls.
pub const COPIES_MAX_RTT_MS: u32 = 100;
/// The host remembers this many of its datagrams to a peer for the peer's
/// echo (2.56 s at 100 Hz).
const SENDS_KEPT: usize = 256;

/// Client or spectator: the host's frame datagrams kept while they begin
/// past what we know, each waiting for the one before it (ruling SK-4).
/// A backlog of 6 s is 10 `MAX_FRAMES` datagrams.
const AHEAD_KEPT: usize = 16;

/// The input delay for a round trip (spec §3.2): `min(3, rtt_ms / 40)` ticks.
pub fn input_delay(rtt_ms: u32) -> u8 {
    (rtt_ms / 40).min(3) as u8
}

/// What a peer is in the match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seat {
    /// Plays the worm of this slot.
    Player(u8),
    /// Watches.
    Spectator,
}

#[derive(Clone, Debug)]
pub struct Config {
    /// Worms in the match; slot `i` is worm `i`.
    pub slots: u8,
    /// A client's input delay in ticks (decision N2: fixed per match); the
    /// host's local slots use none.
    pub input_delay: u8,
    pub fill_window: u32,
    pub max_prediction: u32,
    pub checksum_every: u32,
}

impl Config {
    /// The spec's numbers for `slots` worms (1 to 32) and an input delay.
    pub fn new(slots: u8, input_delay: u8) -> Config {
        assert!((1..=32).contains(&slots), "a match has 1 to 32 slots, not {slots}");
        Config {
            slots,
            input_delay,
            fill_window: FILL_WINDOW,
            max_prediction: MAX_PREDICTION,
            checksum_every: CHECKSUM_EVERY,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Role {
    /// Orders the inputs. Its local slots (its human and the bots) are those
    /// no peer holds.
    Host { peers: Vec<(PeerId, Seat)> },
    /// Plays `slot`.
    Client { host: PeerId, slot: u8 },
    /// Watches from tick 0, or from the state that let it join.
    Spectator { host: PeerId },
    /// A spectator arriving mid-match (spec §3.6): it simulates nothing
    /// until the host's state loads, then it is a `Spectator`.
    Joining { host: PeerId },
}

/// One simulated tick, as [`Driver::simulated`] sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickInfo {
    pub tick: u32,
    /// Re-simulated by a rollback.
    pub resim: bool,
    /// Every slot's final input was known when it was simulated.
    pub all_final: bool,
}

/// The game around a session: it presses the local slots' keys and sees
/// every tick simulated.
pub trait Driver<S> {
    /// The keys of the local `slots` (`out[i]` for `slots[i]`) for `tick`,
    /// from the state before the tick being simulated. Called once per new
    /// tick; twice at a host that took over a migration (decision N2): its
    /// own slot alone for `tick + input_delay`, then the bots for `tick`.
    fn local_inputs(&mut self, sim: &S, tick: u32, slots: &[u8], out: &mut [Keys]);
    fn simulated(&mut self, _sim: &S, _info: TickInfo) {}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Ticks `from..to` were re-simulated.
    Rollback { from: u32, to: u32 },
    /// The prediction limit stopped the session before `tick`: once per
    /// episode (a run of stalled advances).
    Stalled { tick: u32 },
    /// An undecodable message from `peer`.
    Malformed { peer: PeerId },
    /// The host sent a final input that differs from an earlier one.
    Inconsistent { peer: PeerId, tick: u32, slot: u8 },
    /// Client: the host filled our inputs within ticks `from..to` (they
    /// arrived too late): one per datagram of ours it answered. The span
    /// can include on-time ticks between two fill episodes.
    Late { from: u32, to: u32 },
    /// Client: the host has filled our inputs for [`LAGGING_TICKS`] ticks'
    /// worth within [`LAGGING_WINDOW_US`] (`true`), or not for
    /// [`LAGGING_CLEAR_US`] after that (`false`): spec §3.2's warning on
    /// the lagging player's own screen.
    Lagging(bool),
    /// `peer` said it leaves ([`Session::leave`]): the host abandoned its
    /// slot at once ([`Session::peer_lost`]); a client's host is gone.
    PeerLeft { peer: PeerId },
    /// Host: this peer's inputs missed the fill window for
    /// [`LATE_WARNING_US`]. Pushed again only after it recovered.
    PeerLate { peer: PeerId },
    /// Nothing arrived from this peer for [`SILENCE_TIMEOUT_US`]. The driver
    /// decides; the host's [`Session::peer_lost`] abandons the peer's slot.
    PeerTimedOut { peer: PeerId },
    /// The sync checksums of `tick` differ (spec §3.5). Host: `peer`'s
    /// report differs from its own (`local`); its state follows. Client:
    /// the host (`peer`) said so (`local` is ours, `0` if unknown).
    Desync { tick: u32, local: u64, remote: u64, peer: PeerId },
    /// Host: the state after `tick` went to `peer` (`bytes` long).
    StateSent { peer: PeerId, tick: u32, bytes: usize },
    /// Client or spectator: the host's state after `tick` arrived whole,
    /// before it is loaded (the driver may keep it).
    StateArrived { tick: u32, bytes: Vec<u8> },
    /// The state after `tick` was loaded: the timeline restarts from it.
    StateLoaded { tick: u32 },
    /// A state transfer was refused (its chunks, or the simulation refused
    /// the bytes); the old state stays.
    StateRejected { tick: u32 },
    /// Host: `peer` still desyncs after `MAX_REPAIRS` (3) states, or our
    /// state is too large to send it: no more repairs. The driver decides
    /// (the harness drops the peer, as after `PeerTimedOut`).
    RepairFailed { peer: PeerId },
    /// Host: `add_spectator` for a peer that holds a player's slot: refused,
    /// the peer stays a player.
    JoinRefused { peer: PeerId },
    /// Client or spectator: nothing from its host for
    /// [`HOST_SILENT_US`](crate::migration::HOST_SILENT_US) while this peer
    /// kept advancing (once per silence; spec §3.7).
    HostSilent,
    /// Client or spectator: a datagram from its host after `HostSilent`
    /// (before any [`Session::rehost`]); a reliable message, such as its
    /// `Bye` (`PeerLeft`), is none.
    HostHeard,
    /// The timeline resumed after a migration at `tick` (the new host's
    /// [`Session::resume`]; a client's `Control::Resume`).
    Resumed { tick: u32 },
    /// Client or spectator: a loaded state holds `slot`'s worm, which our
    /// log withdrew at `tick`, below the state (ruling PF12d-8a: the old
    /// host's mark, which the new host refused). The state is the match's:
    /// the mark is dropped, and so are our checksums confirmed from `tick`
    /// (final review A-I1). Pushed before `StateLoaded`.
    WithdrawalUndone { tick: u32, slot: u8 },
}

#[derive(Clone, Debug)]
pub struct Stats {
    pub rollbacks: u64,
    /// Rollbacks by depth in ticks (63 and more in the last).
    pub depth_hist: [u64; 64],
    pub resim_ticks: u64,
    /// Stall episodes: runs of advances the prediction limit stopped.
    pub stalls: u64,
    /// A client's own inputs the host's final values replaced.
    pub own_corrections: u64,
    pub malformed: u64,
    /// The round trip to the host (a client or spectator), or the largest
    /// over the peers (the host).
    pub rtt_ms: Option<u32>,
    /// Datagrams and bytes, each datagram counting [`OVERHEAD`] more.
    pub datagrams_in: u64,
    pub bytes_in: u64,
    pub datagrams_out: u64,
    pub bytes_out: u64,
    /// Client: the last arrival margin the host reported, in ticks.
    pub margin: Option<i16>,
    /// Host: inputs filled by the lagging-player rule.
    pub fills: u64,
    /// Client: the pace's speed adjustment.
    pub adjust_ppm: i64,
    /// Client: the pace's jumps.
    pub jumps: u64,
    /// Host: peers' checksums that differed; client: desyncs the host reported.
    pub desyncs: u64,
    /// Host: states sent (repairs and joining spectators).
    pub states_sent: u64,
    /// Client or spectator: states loaded.
    pub states_loaded: u64,
    /// Resumes after a take-over or a rehost (spec §3.7).
    pub migrations: u64,
    /// New host: reported final inputs that differed from ours (ignored, N67).
    pub report_conflicts: u64,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            rollbacks: 0,
            depth_hist: [0; 64],
            resim_ticks: 0,
            stalls: 0,
            own_corrections: 0,
            malformed: 0,
            rtt_ms: None,
            datagrams_in: 0,
            bytes_in: 0,
            datagrams_out: 0,
            bytes_out: 0,
            margin: None,
            fills: 0,
            adjust_ppm: 0,
            jumps: 0,
            desyncs: 0,
            states_sent: 0,
            states_loaded: 0,
            migrations: 0,
            report_conflicts: 0,
        }
    }
}

/// What the session knows of a remote peer.
struct PeerState {
    /// The host's view of a peer; clients use `Seat::Spectator` for the host.
    seat: Seat,
    /// Host: the peer's frontier. Client: the host's `your_ack`.
    ack: u32,
    /// Their newest `sent_ms` and when it arrived (µs).
    echo: Option<(u32, u64)>,
    /// Our first `sent_ms` to them: older echoes are no echo.
    first_sent_ms: Option<u32>,
    /// The round trip's moving average in eighths of a millisecond.
    rtt_x8: Option<i64>,
    /// When anything last arrived from them (µs).
    heard_us: u64,
    /// [`Event::PeerTimedOut`] was pushed for the current silence.
    timed_out: bool,
    /// Client or spectator, for its host: [`Event::HostSilent`] was pushed
    /// for the current silence.
    silent: bool,
    /// Abandoned ([`Session::peer_lost`]): nothing more is sent or taken.
    lost: bool,
    /// Host: the arrival margins of a player's inputs.
    margins: Margins,
    /// Our clock's ms at each stamped datagram's arrival minus its
    /// `sent_ms` (the peer's clock offset plus one trip): a resume's
    /// `at_ms` in the peer's clock (ruling RS-1).
    offsets: Margins,
    /// Host: the newest input tick sampled into `margins`.
    sampled: Option<u32>,
    /// Host: the player's filled ticks not yet answered by `Late`, from
    /// `first_max` on.
    filled: BTreeSet<u32>,
    /// Host: the highest `first` of their datagrams; they send nothing below it again.
    first_max: u32,
    /// Host: consecutive host ticks at which their slot was filled.
    late_run: u32,
    /// Host: [`Event::PeerLate`] was pushed for the current run.
    late_warned: bool,
    /// Host: the peer's checksums and its repair.
    checks: Remote,
    /// Host: a spectator joining mid-match, its state not sent yet.
    joining: bool,
    /// Host: the first tick the peer's input log holds (a joined
    /// spectator's): frames from it carry every value, no base.
    origin: u32,
    /// Host: our recent datagrams to the peer, oldest first: their
    /// `sent_ms` and our frontier when we sent them. Client, for its host:
    /// their `sent_ms` and our `samples_sent` then, to know when the host's
    /// margins are fresh again.
    sends: VecDeque<(u32, u32)>,
    /// Host: where our input log changed before each of our last
    /// [`COPIES`] datagrams to the peer (`u32::MAX`: nowhere).
    recent: VecDeque<u32>,
    /// Host: the peer lacks inputs we sent: our next datagram starts at its `ack`.
    resend: bool,
    /// New host, after its resume: our `Datagram::Resume` goes to this
    /// player instead of its frames until its inputs come (ruling RS-1).
    resuming: Option<Resuming>,
    /// Host: the `sent_ms` of our last such datagram.
    resent_ms: u32,
    /// Host: what the peer lacks did not fit our last datagram, which ended
    /// before this tick: the next one goes on from it (0: nothing left).
    cursor: u32,
}

impl PeerState {
    fn new(seat: Seat, now_us: u64) -> Self {
        Self {
            seat,
            ack: 0,
            echo: None,
            first_sent_ms: None,
            rtt_x8: None,
            heard_us: now_us,
            timed_out: false,
            silent: false,
            lost: false,
            margins: Margins::default(),
            offsets: Margins::default(),
            sampled: None,
            filled: BTreeSet::new(),
            first_max: 0,
            late_run: 0,
            late_warned: false,
            checks: Remote::default(),
            joining: false,
            origin: 0,
            sends: VecDeque::new(),
            recent: VecDeque::new(),
            resend: false,
            resuming: None,
            resent_ms: 0,
            cursor: 0,
        }
    }
}

/// Client or spectator: a host datagram's frames kept ahead of our
/// frontier, with their withdrawal marks (decision N83).
#[derive(Clone, Debug)]
struct Ahead {
    from: PeerId,
    first: u32,
    frames: Vec<Frame>,
    gone: Vec<(u8, u32)>,
}

/// A new host's resume, repeated to one player in every datagram until its
/// inputs come or `until_us` ([`Session::resume`]).
#[derive(Clone, Copy, Debug)]
struct Resuming {
    until_us: u64,
    tick: u32,
    at_ms: u32,
}

/// A client's time sync after a jump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimeSync {
    /// Margins are taken.
    Free,
    /// Jumped; the pace is still catching up.
    Jumped,
    /// Margins are stale until the host echoes our datagram that was this
    /// sample of ours (`samples_sent`): its median then holds a median's
    /// worth of samples made since we set it.
    Until(u32),
}

pub struct Session<S: Simulation> {
    cfg: Config,
    role: Role,
    /// `pub(crate)` for `world_sim.rs`'s presentation hooks.
    pub(crate) sim: S,
    log: InputLog,
    /// The slots this peer presses (`Driver::local_inputs`).
    local: Vec<u8>,
    local_keys: Vec<Keys>,
    /// A client's own slot's provisional inputs, by tick from 0.
    own: Vec<u16>,
    /// The keys used for each tick in `base..current`.
    used: VecDeque<[u16; 32]>,
    /// The state before each tick in `base..current`.
    snaps: VecDeque<S::Snapshot>,
    base: u32,
    /// The next tick to simulate.
    current: u32,
    /// New ticks run (the pace's count).
    ticks_run: u32,
    rollback_from: Option<u32>,
    pace: Pace,
    /// The pace's tick of the last datagram: one per tick.
    sent_at: Option<i64>,
    /// Client: whether margins are taken (they are stale after a jump).
    sync: TimeSync,
    /// The last `advance` left due ticks unrun (a stall, or catching up).
    behind: bool,
    /// Client: the most ticks due and not run after an advance (uncapped)
    /// since the last one that left none (the deficit being run off; 0:
    /// none).
    deficit: u32,
    /// Client: our datagrams so far whose newest input was newer than the
    /// last one's: each that reaches the host is one margin sample there.
    samples_sent: u32,
    /// Client: `own.len()` in our last datagram.
    last_sent_len: usize,
    /// The prediction limit stopped the last advance that ran or stalled:
    /// a stall episode is on ([`Event::Stalled`] once per episode).
    stalling: bool,
    /// Client: the `Late` answers of the last [`LAGGING_WINDOW_US`]: when
    /// each arrived and the ticks it spans.
    lates: VecDeque<(u64, u32)>,
    /// Client: [`Event::Lagging`]`(true)` was pushed, not yet ended.
    lagging: bool,
    /// Client: the speed adjustment's integral term (ppm) and the host tick
    /// it was last integrated at.
    sync_i: i64,
    sync_tick: Option<u32>,
    /// Per remote peer: the host keeps one per client, others one for the host.
    peers: BTreeMap<PeerId, PeerState>,
    out: Vec<(PeerId, Channel, Vec<u8>)>,
    events: Vec<Event>,
    stats: Stats,
    /// Decoded frame values before they are applied: `(tick, slot, value,
    /// withdrawn there)`.
    scratch: Vec<(u32, u8, u16, bool)>,
    /// Client or spectator: the host's frames that began past our
    /// frontier, by `first`, with their withdrawal marks (at most
    /// [`AHEAD_KEPT`]): taken again once it reaches them.
    ahead: Vec<Ahead>,
    /// The sync checksums of our ticks.
    checks: Checks,
    /// The state floor: ticks below it count as confirmed for rollback and
    /// checksums (a loaded state, or a change outside the timeline), though
    /// their inputs may still arrive.
    floor: u32,
    /// After a loaded state: the ticks up to this one are simulated again
    /// at the next advance.
    restart_to: Option<u32>,
    /// Client or spectator: a state arriving in chunks.
    incoming: Option<Incoming>,
    /// Host: the lowest tick of the input log that changed since our last
    /// datagrams (`u32::MAX`: none).
    dirty: u32,
    /// The slot whose keys this peer presses `input_delay` ticks ahead: a
    /// client's, and a new host's own after it took over (decision N2: the
    /// delay is fixed for the match).
    own_slot: Option<u8>,
    /// A migration stopped the timeline ([`Session::suspend`]).
    suspended: bool,
    /// Suspended: when the last keepalive datagram went.
    keepalive_us: Option<u64>,
    /// When the last [`Session::advance`] ran: a longer gap than half of
    /// [`HOST_SILENT_US`] restarts the host's silence clock (`silence_from_us`).
    last_advance_us: Option<u64>,
    silence_from_us: u64,
    /// Client or spectator: rehosted ([`Session::rehost`]), waiting for the
    /// new host's `Control::Resume`.
    rehosted: bool,
    /// New host: the migration it collects (take-over to resume and after).
    collection: Option<migrate::Collection>,
    /// Host: slots no peer plays on (a migration's absent players): `Keys(0)`
    /// from their first unknown tick, as a lost peer's (decision N6).
    abandoned: BTreeSet<u8>,
}

/// A driver that presses nothing and sees nothing: re-simulations that
/// give the same ticks again ([`Session::encode_confirmed_state`]).
struct Quiet;

impl<S> Driver<S> for Quiet {
    fn local_inputs(&mut self, _sim: &S, _tick: u32, _slots: &[u8], _out: &mut [Keys]) {}
}

impl<S: Simulation> Session<S> {
    pub fn new(cfg: Config, role: Role, sim: S, now_us: u64) -> Self {
        let slots = cfg.slots;
        let mut peers = BTreeMap::new();
        let mut own = Vec::new();
        let mut own_slot = None;
        let local: Vec<u8> = match &role {
            Role::Host { peers: list } => {
                for &(id, seat) in list {
                    if let Seat::Player(s) = seat {
                        assert!(s < slots, "peer {id} plays slot {s} of {slots}");
                    }
                    peers.insert(id, PeerState::new(seat, now_us));
                }
                (0..slots).filter(|s| !list.iter().any(|&(_, seat)| seat == Seat::Player(*s))).collect()
            }
            Role::Client { host, slot } => {
                assert!(*slot < slots, "the client plays slot {slot} of {slots}");
                own_slot = Some(*slot);
                peers.insert(*host, PeerState::new(Seat::Spectator, now_us));
                own = vec![0; cfg.input_delay as usize];
                vec![*slot]
            }
            Role::Spectator { host } | Role::Joining { host } => {
                peers.insert(*host, PeerState::new(Seat::Spectator, now_us));
                Vec::new()
            }
        };
        Session {
            log: InputLog::new(slots),
            local_keys: vec![Keys(0); local.len()],
            local,
            own,
            cfg,
            role,
            sim,
            used: VecDeque::new(),
            snaps: VecDeque::new(),
            base: 0,
            current: 0,
            ticks_run: 0,
            rollback_from: None,
            pace: Pace::new(now_us),
            sent_at: None,
            sync: TimeSync::Free,
            behind: false,
            deficit: 0,
            samples_sent: 0,
            last_sent_len: 0,
            stalling: false,
            lates: VecDeque::new(),
            lagging: false,
            sync_i: 0,
            sync_tick: None,
            peers,
            out: Vec::new(),
            events: Vec::new(),
            stats: Stats::default(),
            scratch: Vec::new(),
            ahead: Vec::new(),
            checks: Checks::default(),
            floor: 0,
            restart_to: None,
            incoming: None,
            dirty: u32::MAX,
            own_slot,
            suspended: false,
            keepalive_us: None,
            last_advance_us: None,
            silence_from_us: now_us,
            rehosted: false,
            collection: None,
            abandoned: BTreeSet::new(),
        }
    }

    pub fn sim(&self) -> &S {
        &self.sim
    }

    /// The simulation, to change outside the timeline (a test's planted
    /// fault). The change cannot be rolled back across: the session forgets
    /// its snapshots and the state floor moves to the current tick (ticks
    /// before it count as confirmed); checksums of unconfirmed ticks are
    /// dropped (they are no longer known). For tests and the harness only
    /// (hidden from the docs, like [`Session::change_confirmed`], which
    /// keeps the predicted ticks open); the game reads [`Session::sim`].
    #[doc(hidden)]
    pub fn sim_mut(&mut self) -> &mut S {
        self.snaps.clear();
        self.used.clear();
        self.rollback_from = None;
        self.base = self.current;
        self.floor = self.floor.max(self.current);
        self.checks.restart(self.current, None);
        &mut self.sim
    }

    /// Changes the simulation as it was after the last confirmed tick, then
    /// simulates the unconfirmed ticks again on top of the change (a test's
    /// planted fault): unlike [`Session::sim_mut`], the ticks predicted so
    /// far are not made final with it, so the change is the only
    /// difference to an undisturbed peer. Rollbacks never reach before it
    /// (every tick below it has its final inputs). A pending rollback is
    /// consumed by the re-simulation (it leaves no `Rollback` event or
    /// rollback statistic; `resim_ticks` counts the ticks). Without that
    /// tick's snapshot it is `sim_mut`. For tests and the harness only.
    #[doc(hidden)]
    pub fn change_confirmed(&mut self, driver: &mut dyn Driver<S>, change: impl FnOnce(&mut S)) {
        let x = self.confirmed().min(self.current).min(self.rollback_from.unwrap_or(u32::MAX));
        let i = x.checked_sub(self.base).map_or(usize::MAX, |i| i as usize);
        if x == self.current || i >= self.snaps.len() {
            change(self.sim_mut());
            return;
        }
        let to = self.current;
        self.sim.restore(&self.snaps[i]);
        change(&mut self.sim);
        self.snaps.truncate(i);
        self.used.truncate(i);
        self.checks.rewind(x);
        self.rollback_from = None;
        self.current = x;
        while self.current < to {
            self.simulate(driver, true);
        }
        self.stats.resim_ticks += (to - x) as u64;
    }

    /// The next tick to simulate.
    pub fn current_tick(&self) -> u32 {
        self.current
    }

    /// Ticks below it have every slot's final input.
    pub fn frontier(&self) -> u32 {
        self.log.frontier()
    }

    pub fn final_input(&self, tick: u32, slot: u8) -> Option<Keys> {
        self.log.get(tick, slot).map(Keys)
    }

    pub fn input_log(&self) -> &InputLog {
        &self.log
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// The round trip to `peer` in ms (its stamps' 1/8 moving average), once
    /// measured; `None` for a peer not in the session. A host's
    /// [`Stats::rtt_ms`] is the largest of these.
    pub fn peer_rtt_ms(&self, peer: PeerId) -> Option<u32> {
        let r = self.peers.get(&peer)?.rtt_x8?;
        Some((r / 8).clamp(0, i64::from(u32::MAX)) as u32)
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Our sync checksums of confirmed ticks `t` with `(t + 1) %
    /// checksum_every == 0`, ascending (spec §3.5).
    pub fn checksums(&self) -> &[(u32, u64)] {
        self.checks.confirmed()
    }

    /// Host: `peer`'s sync checksums it compared with ours, ascending (the
    /// last [`PEER_STREAM_KEEP`]): the other stream of a desync bundle
    /// (spec §3.5). Empty for an unknown peer, and on a client.
    pub fn peer_checksums(&self, peer: PeerId) -> Vec<(u32, u64)> {
        self.peers.get(&peer).map(|p| p.checks.compared.iter().copied().collect()).unwrap_or_default()
    }

    /// The state after the last confirmed tick that was simulated, and that
    /// tick: the snapshot before the next is restored and encoded, then the
    /// ticks after it are simulated again (unreported: they come out the
    /// same; counted in `resim_ticks`). `None` before any tick is confirmed
    /// and simulated (or while joining).
    pub fn encode_confirmed_state(&mut self) -> Option<(u32, Vec<u8>)> {
        if matches!(self.role, Role::Joining { .. }) {
            return None;
        }
        let x = self.confirmed().min(self.current).min(self.rollback_from.unwrap_or(u32::MAX));
        let tick = x.checked_sub(1)?;
        if x == self.current {
            return Some((tick, self.sim.encode_state()));
        }
        let i = x.checked_sub(self.base)? as usize;
        if i >= self.snaps.len() {
            return None; // not reached: snapshots are kept from the frontier on
        }
        let to = self.current;
        self.sim.restore(&self.snaps[i]);
        let bytes = self.sim.encode_state();
        self.snaps.truncate(i);
        self.used.truncate(i);
        self.checks.rewind(x);
        self.current = x;
        while self.current < to {
            self.simulate(&mut Quiet, true);
        }
        self.stats.resim_ticks += (to - x) as u64;
        Some((tick, bytes))
    }

    /// Host: a spectator arrives mid-match (spec §3.6). It gets frames from
    /// our frontier and, at the end of the next advance, the confirmed
    /// state ([`Session::encode_confirmed_state`]); its input log begins
    /// after that state. A spectator already known is replaced; a peer
    /// holding a player's slot (lost or not) is refused
    /// ([`Event::JoinRefused`]): its slot would be orphaned.
    pub fn add_spectator(&mut self, peer: PeerId, now_us: u64) {
        if self.peers.get(&peer).is_some_and(|p| matches!(p.seat, Seat::Player(_))) {
            self.events.push(Event::JoinRefused { peer });
            return;
        }
        let Role::Host { peers } = &mut self.role else { return };
        peers.retain(|&(id, _)| id != peer);
        peers.push((peer, Seat::Spectator));
        let mut p = PeerState::new(Seat::Spectator, now_us);
        p.ack = self.log.frontier();
        p.joining = true;
        p.checks.repair = Repair::Owed;
        self.peers.insert(peer, p);
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        mem::take(&mut self.events)
    }

    /// Hands every queued datagram to `out`.
    pub fn transmit(&mut self, out: &mut dyn FnMut(PeerId, Channel, Vec<u8>)) {
        for (peer, ch, bytes) in self.out.drain(..) {
            out(peer, ch, bytes);
        }
    }

    /// Whether this peer orders the inputs (the first host, or one that took over).
    pub fn is_host(&self) -> bool {
        matches!(self.role, Role::Host { .. })
    }

    /// A spectator joining mid-match whose state has not loaded: it has no
    /// world yet (ruling SK-8: its host gone, it cannot follow a migration).
    pub fn is_joining(&self) -> bool {
        matches!(self.role, Role::Joining { .. })
    }

    fn is_spectator(&self) -> bool {
        matches!(self.role, Role::Spectator { .. })
    }

    /// Ticks below it are final (the HUD's ticks, spec §3.4): the input
    /// log's frontier, or the state floor above it (ticks before a loaded
    /// state). Rollbacks and checksums never reach below it.
    pub fn confirmed(&self) -> u32 {
        self.log.frontier().max(self.floor)
    }

    fn checksum_every(&self) -> u32 {
        self.cfg.checksum_every.max(1)
    }

    fn own_slot(&self) -> Option<u8> {
        self.own_slot
    }

    /// The delay between pressing and applying a local input.
    fn delay(&self) -> u32 {
        match self.role {
            Role::Client { .. } => self.cfg.input_delay as u32,
            _ => 0,
        }
    }

    /// How far the pace is into the tick after the newest one run, in
    /// `[0, 1)`, for drawing between the last two ticks (spec §3.4: the
    /// newest predicted tick, interpolated like local play's). `None` when
    /// it would mislead, the caller drawing the newest tick as it is: the
    /// last [`Session::advance`] left due ticks unrun (a stall at the
    /// prediction limit, or catching up), a suspended session (the last
    /// frame stays), or a spectator, whose ticks follow the host's frames,
    /// not the pace.
    pub fn tick_phase(&self, now_us: u64) -> Option<f32> {
        let paced = matches!(self.role, Role::Host { .. } | Role::Client { .. });
        // Held (a resume's hold, a jump back), the pace runs while no tick does.
        let held = self.pace.ticks(now_us) < i64::from(self.ticks_run);
        (paced && !self.behind && !self.stalling && !self.suspended && !held).then(|| self.pace.phase(now_us))
    }

    /// Runs the ticks that are due: first a pending rollback, then new ticks.
    /// Suspended ([`Session::suspend`]): no new tick and no input frames, but
    /// an owed rollback or a loaded state still re-simulates up to the
    /// current tick, checksums are confirmed and sent, and a frame-less
    /// keepalive goes every [`KEEPALIVE_US`](crate::migration::KEEPALIVE_US).
    pub fn advance(&mut self, now_us: u64, driver: &mut dyn Driver<S>) {
        if let Some(to) = self.restart_to.take() {
            // A loaded state: the ticks after it again, on what we know.
            let from = self.current;
            while self.current < to {
                self.simulate(driver, true);
            }
            self.stats.resim_ticks += to.saturating_sub(from) as u64;
        }
        if let Some(from) = self.rollback_from.take() {
            self.resimulate(from, driver);
        }
        // A peer's own sleep is not its host's silence: a long gap between
        // two advances restarts the clock.
        if self.last_advance_us.is_some_and(|last| now_us.saturating_sub(last) > HOST_SILENT_US / 2) {
            self.silence_from_us = now_us;
        }
        self.last_advance_us = Some(now_us);
        if self.suspended {
            self.confirm_checksums();
            self.send_owed_states(now_us);
            self.trim_snapshots();
            self.check_silence(now_us);
            self.check_host_silence(now_us);
            self.check_loads(now_us);
            self.check_lagging(now_us);
            self.keepalive(now_us);
            return;
        }
        let due = self.pace.due(now_us, self.ticks_run);
        let mut ran = 0;
        let mut stalled = false;
        for _ in 0..due {
            if matches!(self.role, Role::Joining { .. }) {
                break;
            }
            let frontier = self.log.frontier();
            if self.is_spectator() {
                if self.current >= frontier {
                    break;
                }
            } else if self.current.saturating_sub(frontier) >= self.cfg.max_prediction {
                if !self.stalling {
                    self.stalling = true;
                    self.events.push(Event::Stalled { tick: self.current });
                    self.stats.stalls += 1;
                }
                stalled = true;
                break;
            }
            self.new_tick(driver);
            ran += 1;
        }
        if ran > 0 && !stalled {
            self.stalling = false;
        }
        self.ticks_run += ran;
        self.behind = self.pace.due(now_us, self.ticks_run) > 0;
        // The whole deficit, not one advance's catch-up (`due` is capped).
        let left =
            self.pace.ticks(now_us).saturating_sub(i64::from(self.ticks_run)).clamp(0, i64::from(u32::MAX));
        let deficit = if self.behind { self.deficit.max(left as u32) } else { 0 };
        let ran_off = mem::replace(&mut self.deficit, deficit);
        let fresh_from = self.samples_sent.saturating_add(MARGIN_SAMPLES as u32);
        if self.sync == TimeSync::Jumped && (!self.behind || stalled) {
            // Caught up, or as far as the prediction limit lets it (else a
            // stall-limited client would wait for ever): margins are fresh
            // once the host has a median's worth of inputs made from now on.
            // Samples from before and during the catch-up would trigger
            // further jumps (an oscillation).
            self.sync = TimeSync::Until(fresh_from);
        } else if self.sync == TimeSync::Free && !self.behind && ran_off > MAX_PREDICTION {
            // Caught up after a deficit longer than a stall at the
            // prediction limit runs up on its own (an outage): the inputs
            // it ran meanwhile reached the host late by the deficit, not by
            // its pace, and the host's median still holds them. Taken now,
            // they would jump the pace ahead by the deficit just run off,
            // into a stall at the prediction limit (SK-2: 63 ticks after a
            // 4.65 s outage on lan). Fresh as after a jump. (Every deficit
            // so judged, `awful`'s steady stalls made 13 % more own
            // corrections.)
            self.sync = TimeSync::Until(fresh_from.saturating_add(MARGIN_SAMPLES as u32));
        }
        self.confirm_checksums();
        self.send_owed_states(now_us);
        self.trim_snapshots();
        self.check_silence(now_us);
        self.check_host_silence(now_us);
        self.check_loads(now_us);
        self.check_lagging(now_us);
        // At most one datagram per tick of the pace. A stalled peer sends
        // too: a peer waiting for frames the other side already has would
        // otherwise wait for ever.
        let tick = self.pace.ticks(now_us);
        // A new host's resume goes every tick of its hold too.
        let resuming = self.peers.values().any(|p| p.resuming.is_some_and(|r| now_us < r.until_us));
        if (ran > 0 || due > 0 || resuming) && self.sent_at != Some(tick) {
            self.sent_at = Some(tick);
            self.queue_datagrams(now_us);
        }
    }

    /// Pushes [`Event::PeerTimedOut`] once for every peer silent for
    /// [`SILENCE_TIMEOUT_US`].
    fn check_silence(&mut self, now_us: u64) {
        for (&id, p) in self.peers.iter_mut() {
            if !p.lost && !p.timed_out && now_us.saturating_sub(p.heard_us) >= SILENCE_TIMEOUT_US {
                p.timed_out = true;
                self.events.push(Event::PeerTimedOut { peer: id });
            }
        }
    }

    /// Client, spectator or joiner: [`Event::HostSilent`] once its host has sent
    /// nothing for [`HOST_SILENT_US`], counted only over time this peer
    /// advanced (from `silence_from_us`): a frozen peer that wakes up does
    /// not blame its host for its own sleep.
    fn check_host_silence(&mut self, now_us: u64) {
        // A joiner too (ruling SK-8): a host that crashed before its state
        // arrived says no `Bye`.
        let (Role::Client { host, .. } | Role::Spectator { host } | Role::Joining { host }) = self.role
        else {
            return;
        };
        let from = self.silence_from_us;
        if let Some(p) = self.peers.get_mut(&host)
            && !p.lost
            && !p.silent
            && now_us.saturating_sub(p.heard_us.max(from)) >= HOST_SILENT_US
        {
            p.silent = true;
            self.events.push(Event::HostSilent);
        }
    }

    /// Leaves the match: a client or spectator says `Control::Bye` to the
    /// host, the host to every peer, each then abandoned
    /// ([`Session::peer_lost`]: nothing more is sent or taken). The other
    /// side abandons us at once ([`Event::PeerLeft`]), without waiting for
    /// [`SILENCE_TIMEOUT_US`].
    pub fn leave(&mut self) {
        let ids: Vec<PeerId> = self.peers.iter().filter(|(_, p)| !p.lost).map(|(&id, _)| id).collect();
        for id in ids {
            self.peer_lost(id);
            self.push_out(id, Channel::Reliable, encode_control(&Control::Bye));
        }
    }

    /// Host: a peer that has not said it loaded our state
    /// (`Control::StateLoaded`) [`LOAD_TIMEOUT_US`] after we sent it cannot
    /// be repaired: [`Event::RepairFailed`].
    fn check_loads(&mut self, now_us: u64) {
        for (&id, p) in self.peers.iter_mut() {
            if let Repair::Sent { until: None, deadline_us, .. } = p.checks.repair
                && !p.lost
                && now_us >= deadline_us
            {
                p.checks.repair = Repair::Failed;
                self.events.push(Event::RepairFailed { peer: id });
            }
        }
    }

    /// Client: a `Late` answer for ticks `from..to` arrived. Filled ticks
    /// adding up to [`LAGGING_TICKS`] within [`LAGGING_WINDOW_US`]:
    /// [`Event::Lagging`]`(true)`.
    fn on_late(&mut self, from: u32, to: u32, now_us: u64) {
        self.events.push(Event::Late { from, to });
        self.lates.push_back((now_us, to.saturating_sub(from)));
        while self.lates.front().is_some_and(|l| l.0.saturating_add(LAGGING_WINDOW_US) <= now_us) {
            self.lates.pop_front();
        }
        let ticks: u64 = self.lates.iter().map(|l| l.1 as u64).sum();
        if !self.lagging && ticks >= LAGGING_TICKS as u64 {
            self.lagging = true;
            self.events.push(Event::Lagging(true));
        }
    }

    /// Client: lagging, and no `Late` for [`LAGGING_CLEAR_US`]:
    /// [`Event::Lagging`]`(false)`.
    fn check_lagging(&mut self, now_us: u64) {
        let last = self.lates.back().map(|l| l.0);
        if self.lagging && last.is_none_or(|at| now_us.saturating_sub(at) >= LAGGING_CLEAR_US) {
            self.lagging = false;
            self.events.push(Event::Lagging(false));
        }
    }

    /// Abandons `peer` (decision N6): the host fills its slot with
    /// `Keys(0)` at every new tick without waiting, and nothing more is sent
    /// to it or taken from it.
    pub fn peer_lost(&mut self, peer: PeerId) {
        if let Some(p) = self.peers.get_mut(&peer) {
            p.lost = true;
            p.filled.clear();
            p.checks = Remote::default();
            p.joining = false;
        }
        // Not even what is queued already.
        self.out.retain(|&(to, _, _)| to != peer);
    }

    /// The host's lagging-player rule (spec §3.2) before its new tick `t`: a
    /// player's input still unknown `fill_window` ticks later becomes its
    /// previous final value; an abandoned slot's becomes `Keys(0)` at once,
    /// its worm withdrawn at its first unknown tick (decision N83).
    /// A run of fills of [`LATE_WARNING_US`] pushes [`Event::PeerLate`].
    fn fill(&mut self, t: u32) {
        let window = self.cfg.fill_window;
        let warn_after = (LATE_WARNING_US / TICK_US) as u32;
        let floor = t.saturating_sub(MAX_AHEAD);
        let mut scratch = mem::take(&mut self.scratch);
        scratch.clear();
        for (&id, p) in self.peers.iter_mut() {
            let Seat::Player(s) = p.seat else { continue };
            if p.lost {
                while self.log.known_upto(s) <= t {
                    let k = self.log.known_upto(s);
                    // N83: its worm is withdrawn at its first unknown tick, with the
                    // `Keys(0)` N6 fills from there.
                    let gone = self.log.gone_at(s).is_none();
                    self.log.set_final(k, s, 0, gone);
                    scratch.push((k, s, 0, gone));
                }
                continue;
            }
            let mut filled = false;
            while self.log.known_upto(s).saturating_add(window) <= t {
                let k = self.log.known_upto(s);
                let v = k.checked_sub(1).and_then(|prev| self.log.get(prev, s)).unwrap_or(0);
                self.log.set(k, s, v);
                scratch.push((k, s, v, false));
                p.filled.insert(k);
                self.stats.fills += 1;
                filled = true;
            }
            while p.filled.first().is_some_and(|&f| f < floor) {
                p.filled.pop_first();
            }
            if filled {
                p.late_run = p.late_run.saturating_add(1);
                if p.late_run >= warn_after && !p.late_warned {
                    p.late_warned = true;
                    self.events.push(Event::PeerLate { peer: id });
                }
            } else {
                p.late_run = 0;
                p.late_warned = false;
            }
        }
        for &s in &self.abandoned {
            while self.log.known_upto(s) <= t {
                let k = self.log.known_upto(s);
                // N83, N88: as a lost player's.
                let gone = self.log.gone_at(s).is_none();
                self.log.set_final(k, s, 0, gone);
                scratch.push((k, s, 0, gone));
            }
        }
        for &(k, s, v, gone) in &scratch {
            self.on_final(k, s, v);
            if gone {
                self.on_gone(k);
            }
        }
        self.scratch = scratch;
    }

    /// The keys used for `slot` at `tick` (spec §3.3): the final value, a
    /// client's own provisional input, or the slot's last final value.
    fn keys_for(&self, tick: u32, slot: u8) -> u16 {
        if let Some(v) = self.log.get(tick, slot) {
            return v;
        }
        if self.own_slot() == Some(slot)
            && let Some(&v) = self.own.get(tick as usize)
        {
            return v;
        }
        self.log.predicted(tick, slot)
    }

    /// Saves the state before `current`, simulates it and reports it.
    fn simulate(&mut self, driver: &mut dyn Driver<S>, resim: bool) {
        let t = self.current;
        if self.snaps.is_empty() {
            self.base = t;
        }
        self.snaps.push_back(self.sim.save());
        let slots = self.cfg.slots;
        let mut row = [0u16; 32];
        let mut keys = [Keys(0); 32];
        for s in 0..slots {
            row[s as usize] = self.keys_for(t, s);
            keys[s as usize] = Keys(row[s as usize]);
        }
        let all_final = self.log.complete(t);
        // N83: a withdrawal is part of tick `t`'s input: after the snapshot
        // (a rollback restores the worm and withdraws it again), before the tick.
        for s in 0..slots {
            if self.log.gone_at(s) == Some(t) {
                self.sim.withdraw(s);
            }
        }
        self.sim.tick(&keys[..slots as usize]);
        if (t + 1).is_multiple_of(self.checksum_every()) {
            self.checks.record(t, self.sim.checksum());
        }
        self.used.push_back(row);
        driver.simulated(&self.sim, TickInfo { tick: t, resim, all_final });
        self.current = t + 1;
    }

    /// A new tick: the local inputs, then the tick itself.
    fn new_tick(&mut self, driver: &mut dyn Driver<S>) {
        let t = self.current;
        if self.is_host()
            && let Some(o) = self.own_slot
        {
            // A host that took over: its own slot keeps the match's input
            // delay (decision N2), its keys final when simulated.
            let at = t + self.cfg.input_delay as u32;
            let mut k = [Keys(0)];
            driver.local_inputs(&self.sim, at, &[o], &mut k);
            if self.own.len() == at as usize {
                self.own.push(k[0].0);
            }
            let v = self.own.get(t as usize).copied().unwrap_or_else(|| self.log.predicted(t, o));
            if self.log.set(t, o, v) == SetResult::New {
                self.dirty = self.dirty.min(t);
            }
        }
        let at = t + self.delay();
        self.local_keys.iter_mut().for_each(|k| *k = Keys(0));
        driver.local_inputs(&self.sim, at, &self.local, &mut self.local_keys);
        match self.role {
            Role::Host { .. } => {
                self.fill(t);
                for (&s, k) in self.local.iter().zip(&self.local_keys) {
                    self.log.set(t, s, k.0);
                }
                if !self.local.is_empty() {
                    self.dirty = self.dirty.min(t);
                }
            }
            Role::Client { .. } => {
                if self.own.len() == at as usize {
                    self.own.push(self.local_keys[0].0);
                }
            }
            Role::Spectator { .. } | Role::Joining { .. } => {}
        }
        self.simulate(driver, false);
    }

    /// Restores the state before `from` and re-simulates to the present.
    fn resimulate(&mut self, from: u32, driver: &mut dyn Driver<S>) {
        let to = self.current;
        let from = from.max(self.base);
        let i = (from - self.base) as usize;
        if from >= to || i >= self.snaps.len() {
            return;
        }
        self.sim.restore(&self.snaps[i]);
        self.snaps.truncate(i);
        self.used.truncate(i);
        self.checks.rewind(from);
        self.current = from;
        while self.current < to {
            self.simulate(driver, true);
        }
        let depth = to - from;
        self.stats.rollbacks += 1;
        self.stats.depth_hist[(depth as usize).min(63)] += 1;
        self.stats.resim_ticks += depth as u64;
        self.events.push(Event::Rollback { from, to });
    }

    /// Keeps the snapshots from the frontier (or a pending rollback) on, at
    /// least one, and lets the simulation forget older ones.
    fn trim_snapshots(&mut self) {
        // A rollback set in this advance (an abandoned slot's `Keys(0)`)
        // still needs its snapshot, though the frontier has passed it.
        let keep = self.confirmed().min(self.rollback_from.unwrap_or(u32::MAX));
        while self.base < keep && self.snaps.len() > 1 {
            self.snaps.pop_front();
            self.used.pop_front();
            self.base += 1;
        }
        if let Some(s) = self.snaps.front() {
            self.sim.forget_before(s);
        }
    }

    /// A final value arrived: roll back if the tick ran on other keys.
    fn on_final(&mut self, tick: u32, slot: u8, v: u16) {
        if self.is_host() {
            self.dirty = self.dirty.min(tick);
        }
        if self.own_slot() == Some(slot) && self.own.get(tick as usize).is_some_and(|&o| o != v) {
            self.stats.own_corrections += 1;
        }
        if tick < self.current
            && tick >= self.base
            && let Some(row) = self.used.get((tick - self.base) as usize)
            && row[slot as usize] != v
        {
            self.rollback_from = Some(self.rollback_from.map_or(tick, |r| r.min(tick)));
        }
    }

    /// A withdrawal at `tick` became known (decision N83). A tick already
    /// simulated was simulated without it: roll back to it. (Its value is
    /// `on_final`'s; equal to the prediction, it would roll back nothing.)
    /// A new mark sits at or past the frontier, so at or past the snapshots
    /// kept, unless a loaded state's floor lies above it: the state holds
    /// the withdrawal (final review A-M4: anything else would skip it
    /// silently).
    fn on_gone(&mut self, tick: u32) {
        debug_assert!(
            tick >= self.base || tick < self.floor,
            "a withdrawal below the snapshots kept: tick {tick}, base {}, floor {}",
            self.base,
            self.floor
        );
        if tick < self.current && tick >= self.base {
            self.rollback_from = Some(self.rollback_from.map_or(tick, |r| r.min(tick)));
        }
    }

    fn stamp(&mut self, peer: PeerId, now_us: u64) -> Stamp {
        // 0 on the wire means "no echo yet": a real time is at least 1.
        let sent_ms = ((now_us / 1000) as u32).max(1);
        let Some(p) = self.peers.get_mut(&peer) else { return Stamp::default() };
        p.first_sent_ms.get_or_insert(sent_ms);
        match p.echo {
            Some((echo_ms, at)) => {
                let held = (now_us.saturating_sub(at) / 1000).min(u16::MAX as u64) as u16;
                Stamp { sent_ms, echo_ms, held_ms: held }
            }
            None => Stamp { sent_ms, echo_ms: 0, held_ms: 0 },
        }
    }

    fn on_stamp(&mut self, peer: PeerId, st: Stamp, now_us: u64) {
        let Some(p) = self.peers.get_mut(&peer) else { return };
        if p.echo.is_none_or(|(e, _)| st.sent_ms >= e) {
            p.echo = Some((st.sent_ms, now_us));
        }
        if st.sent_ms != 0 {
            p.offsets.push(((now_us / 1000) as u32).wrapping_sub(st.sent_ms) as i32);
        }
        // An echo older than our first stamp to them is no echo.
        if p.first_sent_ms.is_some_and(|f| st.echo_ms >= f) {
            let sample = (now_us / 1000) as i64 - st.echo_ms as i64 - st.held_ms as i64;
            if sample >= 0 {
                // `rtt += (sample - rtt) / 8`, kept in eighths so that it
                // settles on the sample instead of up to 7 ms off.
                p.rtt_x8 = Some(match p.rtt_x8 {
                    None => sample * 8,
                    Some(r) => r + sample - r / 8,
                });
                let rtt = |p: &PeerState| p.rtt_x8.map(|r| (r / 8).clamp(0, u32::MAX as i64) as u32);
                self.stats.rtt_ms = self.peers.values().filter_map(rtt).max();
            }
        }
    }

    fn push_out(&mut self, peer: PeerId, ch: Channel, bytes: Vec<u8>) {
        self.stats.datagrams_out += 1;
        self.stats.bytes_out += (bytes.len() + OVERHEAD) as u64;
        self.out.push((peer, ch, bytes));
    }

    /// The datagrams after an advance: the host's frames to every peer, or a
    /// client's (spectator's) inputs and ack to the host.
    fn queue_datagrams(&mut self, now_us: u64) {
        let slots = self.cfg.slots as usize;
        match self.role {
            Role::Host { .. } => {
                let ids: Vec<PeerId> =
                    self.peers.iter().filter(|(_, p)| !p.lost).map(|(&id, _)| id).collect();
                let dirty = mem::replace(&mut self.dirty, u32::MAX);
                let frontier = self.log.frontier();
                let horizon = self.log.horizon();
                for id in ids {
                    let Some(p) = self.peers.get_mut(&id) else { continue };
                    if let Some(r) = p.resuming {
                        if now_us < r.until_us {
                            let first = p.ack.min(frontier);
                            let bytes = resume_datagram(&self.log, r, first, horizon, slots);
                            self.push_out(id, Channel::Unreliable, bytes);
                            continue;
                        }
                        p.resuming = None;
                    }
                    // Every change goes out in `COPIES` datagrams; what the
                    // peer still lacks after that (`resend`), from its ack.
                    p.recent.push_back(dirty);
                    while p.recent.len() > COPIES {
                        p.recent.pop_front();
                    }
                    let resend = mem::take(&mut p.resend);
                    let acked = p.ack.min(frontier);
                    let changed = p.recent.iter().copied().min().unwrap_or(u32::MAX);
                    let short = p.rtt_x8.is_some_and(|r| r <= i64::from(COPIES_MAX_RTT_MS) * 8);
                    let first = if resend || !short { acked } else { acked.max(changed).min(horizon) };
                    // A backlog longer than a datagram (after an outage) goes
                    // on from where the last datagram stopped, one datagram a
                    // tick, not one a round trip (a resend's): the peer runs
                    // again a round trip after its link is back, not one per
                    // `MAX_FRAMES` it lacks (SK-2). A lost one shows in its
                    // echo, and the resend starts from its ack again.
                    let first =
                        if !resend && p.cursor > acked && p.cursor < horizon { p.cursor } else { first };
                    let (seat, margin, origin) = (p.seat, p.margins.median().unwrap_or(0), p.origin);
                    let your_ack = match seat {
                        Seat::Player(s) => self.log.known_upto(s),
                        Seat::Spectator => frontier,
                    };
                    let stamp = self.stamp(id, now_us);
                    let log = &self.log;
                    let base = |s: usize| frames_base(log, first, origin, s);
                    let mut n = horizon.saturating_sub(first).min(MAX_FRAMES as u32);
                    let bytes = loop {
                        let frames = encode_frames(first, n, slots, &|t, s| log.get(t, s as u8), &base);
                        let d = Datagram::Host(HostInputs {
                            stamp,
                            tick: self.current,
                            your_ack,
                            margin,
                            first,
                            frames,
                            gone: log.marks_within(first, n),
                        });
                        let bytes = encode_datagram(&d);
                        if bytes.len() <= MAX_DATAGRAM || n == 0 {
                            break bytes;
                        }
                        n /= 2;
                    };
                    if let Some(p) = self.peers.get_mut(&id) {
                        let end = first.saturating_add(n);
                        p.cursor = if end < horizon { end } else { 0 };
                        if p.sends.len() == SENDS_KEPT {
                            p.sends.pop_front();
                        }
                        p.sends.push_back((stamp.sent_ms, frontier));
                        if resend {
                            p.resent_ms = stamp.sent_ms;
                        }
                    }
                    self.push_out(id, Channel::Unreliable, bytes);
                }
            }
            Role::Client { host, .. } | Role::Spectator { host } | Role::Joining { host } => {
                let Some(p) = self.peers.get(&host) else { return };
                if p.lost {
                    return;
                }
                // From the host's `your_ack`, but always the newest input:
                // a client the host filled past still shows how late it is.
                // With more unacknowledged inputs than fit, the newest ones:
                // the host samples its margin from the newest (it filled
                // the older ones long ago, or their acknowledgement was lost).
                let len = self.own.len();
                let from = (p.ack as usize).min(len.saturating_sub(1)).max(len.saturating_sub(MAX_KEYS));
                let first = from as u32;
                let keys: Vec<u16> = self.own[from..].iter().copied().take(MAX_KEYS).collect();
                let stamp = self.stamp(host, now_us);
                let d = Datagram::Client(ClientInputs {
                    stamp,
                    tick: self.current,
                    ack: self.log.frontier(),
                    first,
                    keys,
                });
                self.push_out(host, Channel::Unreliable, encode_datagram(&d));
                if let Some(p) = self.peers.get_mut(&host) {
                    if p.sends.len() == SENDS_KEPT {
                        p.sends.pop_front();
                    }
                    if len > self.last_sent_len {
                        self.last_sent_len = len;
                        self.samples_sent += 1;
                    }
                    p.sends.push_back((stamp.sent_ms, self.samples_sent));
                }
            }
        }
    }

    fn malformed(&mut self, peer: PeerId) {
        self.stats.malformed += 1;
        self.events.push(Event::Malformed { peer });
    }

    /// Takes one message from `peer`. Never panics: anything undecodable is
    /// counted ([`Event::Malformed`]) and changes nothing else.
    pub fn receive(&mut self, from: PeerId, ch: Channel, bytes: &[u8], now_us: u64) {
        self.stats.datagrams_in += 1;
        self.stats.bytes_in += (bytes.len() + OVERHEAD) as u64;
        match self.peers.get_mut(&from) {
            Some(p) if !p.lost => {
                p.heard_us = now_us;
                p.timed_out = false;
                // Only a client's or spectator's host is ever `silent`; it
                // is heard again by a datagram (its `Bye` is no return).
                if ch == Channel::Unreliable && mem::take(&mut p.silent) {
                    self.events.push(Event::HostHeard);
                }
            }
            _ => return,
        }
        match ch {
            Channel::Reliable => match decode_control(bytes) {
                Err(_) => self.malformed(from),
                Ok(c) => self.on_control(from, c, now_us),
            },
            Channel::Unreliable => match decode_datagram(bytes, self.cfg.slots as usize) {
                Ok(Datagram::Client(c)) if self.is_host() => self.on_client_inputs(from, c, now_us),
                Ok(Datagram::Host(h)) if matches!(self.role, Role::Joining { .. }) => {
                    // No frames before the state: our log begins after it.
                    self.on_stamp(from, h.stamp, now_us);
                }
                Ok(Datagram::Host(h)) if !self.is_host() => self.on_host_inputs(from, h, now_us),
                Ok(Datagram::Resume(r))
                    if matches!(self.role, Role::Client { .. } | Role::Spectator { .. }) =>
                {
                    self.on_resume(from, &r, now_us)
                }
                _ => self.malformed(from),
            },
        }
    }

    /// Host: a client's inputs become final in arrival order. A datagram
    /// whose newest input is newer than all before is an arrival-margin
    /// sample: that tick minus the host's current tick (positive: early).
    /// A value for a filled tick is ignored and answered by one `Late`.
    fn on_client_inputs(&mut self, from: PeerId, c: ClientInputs, now_us: u64) {
        self.on_stamp(from, c.stamp, now_us);
        let current = self.current;
        let limit = current.saturating_add(MAX_AHEAD);
        let frontier = self.log.frontier();
        let Some(p) = self.peers.get_mut(&from) else { return };
        p.ack = p.ack.max(c.ack);
        // Its inputs after our resume: it resumed.
        if !c.keys.is_empty() {
            p.resuming = None;
        }
        // The datagram of ours they had last when they sent this one held
        // every input below our frontier then, or repeated them recently: if
        // they still lack some, some were lost. Not while our resend is on
        // its way.
        let echo = c.stamp.echo_ms;
        if echo != 0 && echo >= p.resent_ms {
            let known = p.sends.iter().rev().find(|s| s.0 == echo).map(|s| s.1);
            let older = p.sends.front().is_some_and(|s| echo < s.0);
            match known {
                Some(f) if c.ack < f => p.resend = true,
                None if older && c.ack < frontier => p.resend = true,
                _ => {}
            }
        }
        let Seat::Player(slot) = p.seat else { return };
        // Not pruned below the frontier: the lagging slot usually is the
        // frontier, so no `Late` would ever be sent. Below the client's
        // highest `first` it never sends again.
        p.first_max = p.first_max.max(c.first);
        while p.filled.first().is_some_and(|&f| f < p.first_max) {
            p.filled.pop_first();
        }
        // The newest input, not only a `New` one: a client so far behind that
        // its inputs land on filled ticks must still show how late it is.
        let newest = c.keys.len().checked_sub(1).and_then(|n| c.first.checked_add(n as u32));
        if let Some(t) = newest
            && t < limit
            && p.sampled.is_none_or(|s| t > s)
        {
            p.sampled = Some(t);
            p.margins.push((t as i64 - current as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32);
        }
        // The filled ticks this datagram answers: one `Late` spans them.
        let mut late: Option<(u32, u32)> = None;
        for (k, &v) in c.keys.iter().enumerate() {
            let Some(t) = c.first.checked_add(k as u32) else { break };
            if t >= limit {
                break;
            }
            match self.log.set(t, slot, v) {
                SetResult::New => self.on_final(t, slot, v),
                // The first value stays final; a filled tick's owner is told.
                SetResult::Same | SetResult::Conflict => {
                    if self.peers.get_mut(&from).is_some_and(|p| p.filled.remove(&t)) {
                        // `t < limit`: `t + 1` does not overflow.
                        late = Some((late.map_or(t, |l| l.0), t + 1));
                    }
                }
                SetResult::Gap => break,
            }
        }
        if let Some((from_tick, to)) = late {
            let c = encode_control(&Control::Late { from: from_tick, to });
            self.push_out(from, Channel::Reliable, c);
        }
    }

    /// Client: time sync (decision N7) from the host's reported margin. An
    /// error beyond [`JUMP_TICKS`] shifts the pace, after which margins are
    /// stale until the host holds [`MARGIN_SAMPLES`] of our inputs made
    /// after the jump's catch-up; a smaller one sets the speed: proportional
    /// to the error, plus its integral over the host's ticks (a drifting
    /// clock). A client behind its own pace (stalled or catching up) is not
    /// sped up: its inputs are late because it is behind, not because its
    /// clock is wrong, and steering it ahead would only pile up a deficit.
    fn sync(&mut self, h: &HostInputs) {
        if !matches!(self.role, Role::Client { .. }) {
            return;
        }
        self.stats.margin = Some(h.margin);
        let error = TARGET_MARGIN - h.margin as i32;
        match self.sync {
            TimeSync::Jumped => return,
            // Not the host's `your_ack`: it fills a lagging player's ticks
            // without any input of ours, so its median can still hold the
            // stale samples (SK-2: a jump 30 ticks after an outage on bad).
            TimeSync::Until(t) if self.echoed_samples(h.stamp.echo_ms) < t => return,
            // Behind its own pace, a client's inputs are late because it is
            // behind: speeding it up would only pile up a deficit it cannot run.
            _ if self.behind && error > 0 => return,
            _ => self.sync = TimeSync::Free,
        }
        // The host's tick runs with ours: one older than the last taken, or
        // far past our own (hostile), is not taken.
        let ordered = self.sync_tick.is_none_or(|t| h.tick >= t);
        let fresh = ordered && h.tick <= self.current.saturating_add(HOST_TICK_SLACK);
        if error.abs() > JUMP_TICKS {
            // A reordered datagram's margin is older than one taken: a jump
            // on it would undo a correction already made (ruling RS-1). (A
            // host far ahead, after our own freeze, is no reason to wait.)
            if ordered {
                self.pace.shift(error.clamp(-MAX_JUMP_BACK, MAX_JUMP_AHEAD));
                self.stats.jumps += 1;
                self.sync = TimeSync::Jumped;
            }
        } else {
            let dt = match self.sync_tick {
                Some(t) if fresh => (h.tick - t).min(100) as i64,
                _ => 0,
            };
            self.sync_i = (self.sync_i + error as i64 * dt * INTEGRAL_PPM_PER_TICK)
                .clamp(-MAX_ADJUST_PPM, MAX_ADJUST_PPM);
            self.pace.set_adjust_ppm(error as i64 * ADJUST_PPM_PER_TICK + self.sync_i);
            self.stats.adjust_ppm = self.pace.adjust_ppm();
        }
        if fresh {
            self.sync_tick = Some(h.tick);
        }
    }

    /// Client: which of our samples (`samples_sent`) the datagram of ours
    /// the host echoes (`echo_ms`) was; 0 when unknown.
    fn echoed_samples(&self, echo_ms: u32) -> u32 {
        let (Role::Client { host, .. } | Role::Spectator { host } | Role::Joining { host }) = self.role
        else {
            return 0;
        };
        self.peers.get(&host).and_then(|p| p.sends.iter().rev().find(|s| s.0 == echo_ms)).map_or(0, |s| s.1)
    }

    /// Client or spectator: the host's final inputs.
    fn on_host_inputs(&mut self, from: PeerId, mut h: HostInputs, now_us: u64) {
        let first = h.first;
        // Sent before the host had our state (a joined spectator's log
        // begins after it): its base is a tick we never learn, and every
        // value in it is before the state or chained to one before it.
        // Old, not malformed; the host's later frames start at our origin.
        if first != 0 && first < self.log.origin() {
            self.on_stamp(from, h.stamp, now_us);
            return;
        }
        let frontier = self.log.frontier();
        if !self.take_frames(from, first, &h.frames, &h.gone) {
            self.malformed(from);
            return;
        }
        if first > frontier {
            self.keep_ahead(from, first, mem::take(&mut h.frames), mem::take(&mut h.gone));
        }
        self.take_ahead(from);
        self.on_stamp(from, h.stamp, now_us);
        if let Some(p) = self.peers.get_mut(&from) {
            p.ack = p.ack.max(h.your_ack);
        }
        // Suspended, the pace stands still: no margin is ours to steer by.
        if !self.suspended {
            self.sync(&h);
        }
    }

    /// Client or spectator: keeps the host's frames for ticks `first..`,
    /// which begin past our frontier, until it reaches them. The host's
    /// backlog after an outage goes one `MAX_FRAMES` datagram a tick, each
    /// beginning where the one before ends: one overtaking another (jitter)
    /// found the slots' values before `first` unknown and took nothing, nor
    /// did every later one, until the host's resend a round trip later; the
    /// peer stalled at the prediction limit meanwhile (ruling SK-4,
    /// net-soak's `typical-11s6c0v-seed3117434416`). The nearest
    /// [`AHEAD_KEPT`] are kept, one per `first`, none without frames; each
    /// with its withdrawal marks.
    fn keep_ahead(&mut self, from: PeerId, first: u32, frames: Vec<Frame>, gone: Vec<(u8, u32)>) {
        // A datagram without frames (a keepalive, or nothing new) would take
        // a place and sort before the backlog's (final review A-M2).
        if frames.is_empty() || self.ahead.iter().any(|a| a.from == from && a.first == first) {
            return;
        }
        let at = self.ahead.partition_point(|a| a.first < first);
        self.ahead.insert(at, Ahead { from, first, frames, gone });
        self.ahead.truncate(AHEAD_KEPT);
    }

    /// Client or spectator: takes the kept frames our frontier reached
    /// (from `from`, the host; any other's are dropped), in order, and
    /// drops those it passed.
    fn take_ahead(&mut self, from: PeerId) {
        loop {
            let frontier = self.log.frontier();
            let origin = self.log.origin();
            self.ahead.retain(|a| {
                let end = a.first.saturating_add(a.frames.len() as u32);
                a.from == from && end > frontier && (a.first == 0 || a.first >= origin)
            });
            let Some(i) = self.ahead.iter().position(|a| a.first <= frontier) else { return };
            let Ahead { first, frames, gone, .. } = self.ahead.remove(i);
            // Decoded once already (with the values before `first` it
            // lacked then): it decodes again.
            self.take_frames(from, first, &frames, &gone);
        }
    }

    /// Client or spectator: the host's final inputs for ticks `first..`
    /// (its datagram's, or its `Control::Resume`'s), each with its
    /// withdrawal mark from `gone` (decision N83, part of the value;
    /// [`decode_marked`]: a mark without its value is dropped): `false`,
    /// nothing taken, when they do not decode. The first value stays final;
    /// a differing one, or a mark for a tick final without it, is
    /// [`Event::Inconsistent`].
    fn take_frames(&mut self, from: PeerId, first: u32, frames: &[Frame], gone: &[(u8, u32)]) -> bool {
        let slots = self.cfg.slots as usize;
        let mut scratch = mem::take(&mut self.scratch);
        let decoded = decode_marked(&self.log, first, frames, slots, gone, &mut scratch);
        if decoded {
            let limit = self.current.saturating_add(MAX_AHEAD);
            for &(t, s, v, g) in &scratch {
                if t >= limit {
                    continue;
                }
                match self.log.set_final(t, s, v, g) {
                    SetResult::New => {
                        self.on_final(t, s, v);
                        if g && self.log.gone_at(s) == Some(t) {
                            self.on_gone(t);
                        }
                    }
                    SetResult::Conflict => {
                        self.events.push(Event::Inconsistent { peer: from, tick: t, slot: s })
                    }
                    SetResult::Same | SetResult::Gap => {}
                }
            }
        }
        self.scratch = scratch;
        decoded
    }

    /// A reliable message from `from`. Each is taken by the role it is for;
    /// any other changes nothing.
    fn on_control(&mut self, from: PeerId, c: Control, now_us: u64) {
        let host = self.is_host();
        match c {
            Control::Late { from, to } if !host => self.on_late(from, to, now_us),
            Control::Bye => {
                // The host abandons the peer's slot now; a client its host.
                self.peer_lost(from);
                self.events.push(Event::PeerLeft { peer: from });
            }
            Control::StateLoaded { tick } if host => self.on_state_loaded(from, tick),
            Control::StateRefused { tick } if host => self.on_state_refused(from, tick),
            Control::Checksum { tick, hash } if host => self.on_checksum(from, tick, hash),
            Control::Desync { tick, host_hash } if !host => {
                self.stats.desyncs += 1;
                let local = self.checks.get(tick).unwrap_or(0);
                self.events.push(Event::Desync { tick, local, remote: host_hash, peer: from });
            }
            Control::StateBegin { tick, len, chunks } if !host => match Incoming::begin(tick, len, chunks) {
                Some(inc) if inc.chunks() == 0 => {
                    self.incoming = None;
                    self.state_arrived(tick, Vec::new());
                }
                Some(inc) => self.incoming = Some(inc),
                None => {
                    self.incoming = None;
                    self.state_refused(tick);
                }
            },
            Control::StateChunk { index, bytes } if !host => {
                // Without a transfer (none begun, or one refused) a chunk changes nothing.
                let Some(inc) = self.incoming.as_mut() else { return };
                let tick = inc.tick;
                match inc.add(index, bytes) {
                    Ok(None) => {}
                    Ok(Some(state)) => {
                        self.incoming = None;
                        self.state_arrived(tick, state);
                    }
                    Err(()) => {
                        self.incoming = None;
                        self.state_refused(tick);
                    }
                }
            }
            Control::Collect { known } if !host => self.on_collect(from, &known),
            Control::Report(r) if host => self.on_report(from, r, now_us),
            Control::Resume { tick, at_ms, first, frames, gone } if !host => {
                self.on_resume(from, &HostResume { tick, at_ms, first, frames, gone }, now_us)
            }
            _ => {}
        }
    }

    /// Host: a peer's checksum of `tick`. Compared now if ours is
    /// confirmed, else kept (within [`REPORT_WINDOW`] ticks past ours);
    /// ticks that are no checksum ticks, and reports while a state is owed
    /// or for ticks it covers, are ignored.
    fn on_checksum(&mut self, from: PeerId, tick: u32, hash: u64) {
        if !(tick as u64 + 1).is_multiple_of(self.checksum_every() as u64) {
            return;
        }
        if self.checks.get(tick).is_some() {
            self.compare(from, tick, hash);
            return;
        }
        let upto = self.checks.confirmed_upto();
        let Some(p) = self.peers.get_mut(&from) else { return };
        let wanted = match p.checks.repair {
            Repair::Clean => true,
            Repair::Owed | Repair::Failed => false,
            Repair::Sent { tick: t, .. } => tick > t,
        };
        if wanted && tick >= upto && tick < upto.saturating_add(REPORT_WINDOW) {
            p.checks.reported.insert(tick, hash);
        }
    }

    /// Host: `peer` loaded our state after `tick`. Its repair's re-arm
    /// window ([`REPAIR_MARGIN`]) starts now: a multi-MB state over a slow
    /// link may take long to arrive, and its reports differ until then.
    fn on_state_loaded(&mut self, peer: PeerId, tick: u32) {
        let until = self.current.saturating_add(REPAIR_MARGIN);
        let Some(p) = self.peers.get_mut(&peer) else { return };
        if let Repair::Sent { tick: t, until: u @ None, .. } = &mut p.checks.repair
            && *t == tick
        {
            *u = Some(until);
        }
    }

    /// Host: `peer` refused our state after `tick` (its transfer or its
    /// bytes): the repair re-arms at once, without waiting for
    /// [`LOAD_TIMEOUT_US`] (another state, or past [`MAX_REPAIRS`]
    /// [`Event::RepairFailed`]).
    fn on_state_refused(&mut self, peer: PeerId, tick: u32) {
        let sent = self.peers.get(&peer).map(|p| p.checks.repair);
        if let Some(Repair::Sent { tick: t, until: None, .. }) = sent
            && t == tick
        {
            self.rearm(peer);
        }
    }

    /// Host: compares `peer`'s checksum of `tick` with ours. A difference
    /// is a desync ([`Session::desync`]), at most one repair in flight.
    /// While the state is on its way the peer's reports differ: a repair
    /// ends with the first report after the state's tick that matches, and
    /// re-arms with a differing one from [`REPAIR_MARGIN`] ticks after the
    /// peer said it loaded the state on (it diverged again). A peer that
    /// never says so fails after [`LOAD_TIMEOUT_US`] (`check_loads`).
    fn compare(&mut self, peer: PeerId, tick: u32, remote: u64) {
        let Some(local) = self.checks.get(tick) else { return };
        let Some(p) = self.peers.get_mut(&peer) else { return };
        if p.checks.compared.len() >= PEER_STREAM_KEEP {
            p.checks.compared.pop_front();
        }
        p.checks.compared.push_back((tick, remote));
        match p.checks.repair {
            Repair::Owed | Repair::Failed => {}
            Repair::Sent { tick: t, .. } if tick > t && local == remote => p.checks.repair = Repair::Clean,
            Repair::Sent { until: Some(until), .. } if local != remote && tick >= until => {
                self.desync(peer, tick, local, remote)
            }
            Repair::Sent { .. } => {}
            Repair::Clean if local == remote => {}
            Repair::Clean => self.desync(peer, tick, local, remote),
        }
    }

    /// Host: `peer`'s checksum of `tick` differs: the event, then
    /// `Control::Desync` and our confirmed state owed to it, or, after
    /// [`MAX_REPAIRS`] states, [`Event::RepairFailed`] and no more repairs.
    fn desync(&mut self, peer: PeerId, tick: u32, local: u64, remote: u64) {
        self.stats.desyncs += 1;
        self.events.push(Event::Desync { tick, local, remote, peer });
        if self.rearm(peer) {
            let c = encode_control(&Control::Desync { tick, host_hash: local });
            self.push_out(peer, Channel::Reliable, c);
        }
    }

    /// Host: another state is owed to `peer` (`true`), or after
    /// [`MAX_REPAIRS`] the repair fails ([`Event::RepairFailed`]).
    fn rearm(&mut self, peer: PeerId) -> bool {
        let Some(p) = self.peers.get_mut(&peer) else { return false };
        p.checks.reported.clear();
        if p.checks.repairs >= MAX_REPAIRS {
            p.checks.repair = Repair::Failed;
            self.events.push(Event::RepairFailed { peer });
            return false;
        }
        p.checks.repairs += 1;
        p.checks.repair = Repair::Owed;
        true
    }

    /// At the end of an advance: pending checksums below the confirmed
    /// ticks (and a pending rollback) are final. A client or spectator
    /// sends them to the host; the host compares the reports waiting for them.
    fn confirm_checksums(&mut self) {
        let below = self.confirmed().min(self.rollback_from.unwrap_or(u32::MAX));
        let start = self.checks.confirm(below);
        match self.role {
            Role::Host { .. } => {
                let upto = self.checks.confirmed_upto();
                let ids: Vec<PeerId> =
                    self.peers.iter().filter(|(_, p)| !p.lost).map(|(&id, _)| id).collect();
                for id in ids {
                    let ready = {
                        let Some(p) = self.peers.get_mut(&id) else { continue };
                        let later = p.checks.reported.split_off(&upto);
                        mem::replace(&mut p.checks.reported, later)
                    };
                    for (tick, hash) in ready {
                        self.compare(id, tick, hash);
                    }
                }
            }
            Role::Client { host, .. } | Role::Spectator { host } => {
                if self.peers.get(&host).is_none_or(|p| p.lost) {
                    return;
                }
                for i in start..self.checks.confirmed().len() {
                    let (tick, hash) = self.checks.confirmed()[i];
                    self.push_out(host, Channel::Reliable, encode_control(&Control::Checksum { tick, hash }));
                }
            }
            Role::Joining { .. } => {}
        }
    }

    /// Host: sends the owed states (repairs and joining spectators): our
    /// confirmed state, `StateBegin` and its chunks on the reliable channel.
    /// A joining spectator's frames then start right after the state.
    fn send_owed_states(&mut self, now_us: u64) {
        let owed: Vec<PeerId> = self
            .peers
            .iter()
            .filter(|(_, p)| !p.lost && p.checks.repair == Repair::Owed)
            .map(|(&id, _)| id)
            .collect();
        if owed.is_empty() {
            return;
        }
        let Some((tick, state)) = self.encode_confirmed_state() else { return };
        // A state past what a peer accepts cannot repair: give up on it (a
        // joining spectator would wait for ever).
        let controls = state_controls(tick, &state);
        let deadline_us = now_us.saturating_add(LOAD_TIMEOUT_US);
        for id in owed {
            let Some(p) = self.peers.get_mut(&id) else { continue };
            let Some(controls) = &controls else {
                p.checks.repair = Repair::Failed;
                self.events.push(Event::RepairFailed { peer: id });
                continue;
            };
            p.checks.repair = Repair::Sent { tick, until: None, deadline_us };
            if mem::take(&mut p.joining) {
                p.ack = tick + 1;
                p.origin = tick + 1;
            }
            for c in controls {
                self.push_out(id, Channel::Reliable, encode_control(c));
            }
            self.stats.states_sent += 1;
            self.events.push(Event::StateSent { peer: id, tick, bytes: state.len() });
        }
    }

    /// Client or spectator: the host's state after `tick` arrived whole. A
    /// state the simulation refuses changes nothing (`StateRejected`); a
    /// loaded one restarts the timeline after it: no snapshots, the state
    /// floor at `tick + 1`, the ticks we had run simulated again at the next
    /// advance; a joining spectator becomes a spectator there.
    fn state_arrived(&mut self, tick: u32, bytes: Vec<u8>) {
        let floor = tick.checked_add(1);
        // A client's own inputs are kept per tick: no state far ahead of it.
        let plausible = floor.filter(|&f| {
            !matches!(self.role, Role::Client { .. }) || f <= self.current.saturating_add(MAX_AHEAD)
        });
        let loaded = match plausible {
            Some(f) => self.sim.decode_state(&bytes).is_ok().then_some(f),
            None => None,
        };
        self.events.push(Event::StateArrived { tick, bytes });
        let Some(floor) = loaded else {
            self.state_refused(tick);
            return;
        };
        let old = self.current;
        self.snaps.clear();
        self.used.clear();
        self.rollback_from = None;
        self.restart_to = None;
        self.base = floor;
        self.floor = floor;
        self.current = floor;
        // The state's own checksum (when `tick` is a checksum tick) is kept
        // in our list but not reported: it is the host's own value echoed,
        // so it would end a repair without showing that we simulate on
        // correctly from the state. Our first report after the floor does.
        let hash = floor.is_multiple_of(self.checksum_every()).then(|| self.sim.checksum());
        // Final review A-I1: our marks below the state agree with it. A mark
        // the state lacks (PF12d-8a's refused one) would otherwise stay in our
        // record and replay, and, were we to host later, keep `fill` from ever
        // marking that player's real leave. Our checksums from it on were of
        // a world the match did not have.
        let held = self.sim.withdrawn();
        for slot in 0..self.log.slots() {
            if let Some(at) = self.log.gone_at(slot).filter(|&t| t < floor)
                && held & (1 << slot) == 0
            {
                self.log.unmark(slot);
                self.checks.restart(at, None);
                self.events.push(Event::WithdrawalUndone { tick: at, slot });
            }
        }
        self.checks.restart(tick, hash);
        match self.role {
            Role::Joining { host } => {
                self.log.start_at(floor);
                self.role = Role::Spectator { host };
            }
            _ if old > floor => self.restart_to = Some(old),
            Role::Client { .. } => {
                // Ahead of us: the ticks between count as run, and our own
                // inputs for them are none. (Deferred, plan 10: a hostile
                // host can grow `own` by up to `MAX_AHEAD` ticks per valid
                // state it sends.)
                self.ticks_run = self.ticks_run.saturating_add(floor - old);
                let at = floor as usize + self.delay() as usize;
                if self.own.len() < at {
                    self.own.resize(at, 0);
                }
            }
            _ => {}
        }
        self.stats.states_loaded += 1;
        self.events.push(Event::StateLoaded { tick });
        // The host times its repair from our load, not from its sending.
        self.tell_host(Control::StateLoaded { tick });
    }

    /// Client or spectator: the host's state after `tick` was refused
    /// (`StateRejected`); the host is told, so it can send another at once.
    fn state_refused(&mut self, tick: u32) {
        self.events.push(Event::StateRejected { tick });
        self.tell_host(Control::StateRefused { tick });
    }

    /// Client or spectator (a joining one too): `c` to the host, unless lost.
    fn tell_host(&mut self, c: Control) {
        if let Role::Client { host, .. } | Role::Spectator { host } | Role::Joining { host } = self.role
            && self.peers.get(&host).is_some_and(|p| !p.lost)
        {
            self.push_out(host, Channel::Reliable, encode_control(&c));
        }
    }
}

/// A new host's `Datagram::Resume` (ruling RS-1): its `Resuming` and the
/// final inputs from `first` to `horizon`, at most `MAX_FRAMES`, halved
/// until it fits `MAX_DATAGRAM`.
/// The base a peer whose log begins at `origin` decodes frames from
/// `first` with, slot `s`: the frame delta rule's value at `first - 1`; 0
/// before tick 0; none at its log's start (a joined spectator knows
/// nothing before its state: the frames list every known slot).
pub(crate) fn frames_base(log: &InputLog, first: u32, origin: u32, s: usize) -> Option<u16> {
    match first {
        0 => Some(0),
        f if f == origin => None,
        f => log.get(f - 1, s as u8),
    }
}

/// Decodes final inputs for ticks `first..` (a host's frames, or a
/// report's) into `scratch` as `(tick, slot, value, withdrawn there)`, each
/// value with its mark from `gone` (decision N83); `false` when they do not
/// decode. A slot `log` lacks before `first` (its datagrams were lost, or
/// the reporter knew more) takes none of these values (they are beyond its
/// known prefix: `Gap`), so any base decodes it. A mark whose (slot, tick)
/// has no value in the frames is dropped: an honest sender lists a mark
/// only with its value (`InputLog::marks_within` over the frames it sends),
/// and it carries no state on its own.
fn decode_marked(
    log: &InputLog,
    first: u32,
    frames: &[Frame],
    slots: usize,
    gone: &[(u8, u32)],
    scratch: &mut Vec<(u32, u8, u16, bool)>,
) -> bool {
    scratch.clear();
    let base = |s: usize| match first {
        0 => Some(0),
        f => log.get(f - 1, s as u8).or_else(|| (log.known_upto(s as u8) < f).then_some(0)),
    };
    decode_frames(first, frames, slots, &base, &mut |t, s, v| {
        let s = s as u8;
        scratch.push((t, s, v, gone.contains(&(s, t))))
    })
    .is_ok()
}

fn resume_datagram(log: &InputLog, r: Resuming, first: u32, horizon: u32, slots: usize) -> Vec<u8> {
    // A player's log begins at 0.
    let base = |s: usize| frames_base(log, first, 0, s);
    let mut n = horizon.saturating_sub(first).min(MAX_FRAMES as u32);
    loop {
        let frames = encode_frames(first, n, slots, &|t, s| log.get(t, s as u8), &base);
        let gone = log.marks_within(first, n);
        let d = Datagram::Resume(HostResume { tick: r.tick, at_ms: r.at_ms, first, frames, gone });
        let bytes = encode_datagram(&d);
        if bytes.len() <= MAX_DATAGRAM || n == 0 {
            return bytes;
        }
        n /= 2;
    }
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;

    use olr_proto::{Frame, decode_datagram};

    use super::*;
    use crate::ToySim;
    use crate::harness::Factory;
    use crate::toy::ToyFactory;

    fn client() -> Session<ToySim> {
        Session::new(Config::new(3, 1), Role::Client { host: 0, slot: 2 }, ToySim::new(3), 0)
    }

    fn driver() -> Box<dyn Driver<ToySim>> {
        ToyFactory { slots: 3, seed: 1 }.driver(1, None)
    }

    /// The host's datagram to the client: `frames` from `first`, nothing acknowledged.
    fn host_inputs(tick: u32, margin: i16, first: u32, frames: Vec<Frame>) -> Vec<u8> {
        host_inputs_marked(tick, margin, first, frames, vec![])
    }

    /// [`host_inputs`] with withdrawal marks.
    fn host_inputs_marked(
        tick: u32,
        margin: i16,
        first: u32,
        frames: Vec<Frame>,
        gone: Vec<(u8, u32)>,
    ) -> Vec<u8> {
        let h = HostInputs { stamp: Stamp::default(), tick, your_ack: 0, margin, first, frames, gone };
        encode_datagram(&Datagram::Host(h))
    }

    /// N83: a client keeps the mark that comes with the value at its tick.
    #[test]
    fn a_client_keeps_the_hosts_withdrawal_mark() {
        let mut c = client();
        let frames = olr_proto::encode_frames(0, 20, 3, &|_, s| (s < 2).then_some(0), &|_| Some(0));
        c.receive(0, Channel::Unreliable, &host_inputs_marked(20, 0, 0, frames, vec![(0, 12)]), 1_000);
        assert_eq!(c.input_log().gone_at(0), Some(12));
        assert_eq!(c.input_log().gone_at(1), None);
    }

    /// N83, Review Focus 1: a client that predicted a slot as `Keys(0)` past T
    /// learns, with that same value at T, that the slot is withdrawn there: it
    /// rolls back to T. The values agree, so only the mark can roll it back
    /// (RED without `on_gone`).
    #[test]
    fn a_withdrawal_learned_late_rolls_back_to_it() {
        let (mut c, mut twin) = (client(), client());
        // One driver each, alike: the twin presses the same keys.
        let (mut d, mut dt) = (driver(), driver());
        let zeros =
            |first, n| olr_proto::encode_frames(first, n, 3, &|_, s| (s < 2).then_some(0), &|_| Some(0));
        for s in [&mut c, &mut twin] {
            s.receive(0, Channel::Unreliable, &host_inputs(10, 0, 0, zeros(0, 10)), 1_000);
        }
        let mut now = 1_000;
        while c.current_tick() < 20 {
            now += 10_000;
            c.advance(now, d.as_mut());
            twin.advance(now, dt.as_mut());
        }
        assert_eq!(c.sim().checksum(), twin.sim().checksum());
        c.drain_events();
        c.receive(0, Channel::Unreliable, &host_inputs_marked(20, 0, 10, zeros(10, 10), vec![(0, 12)]), now);
        twin.receive(0, Channel::Unreliable, &host_inputs(20, 0, 10, zeros(10, 10)), now);
        now += 10_000;
        c.advance(now, d.as_mut());
        twin.advance(now, dt.as_mut());
        assert!(c.drain_events().iter().any(|e| matches!(*e, Event::Rollback { from: 12, .. })));
        assert_ne!(c.sim().checksum(), twin.sim().checksum(), "the toy world withdrew slot 0 at 12");
    }

    /// N83: a mark for a tick the client holds final without one is the
    /// host contradicting itself.
    #[test]
    fn a_late_withdrawal_mark_is_inconsistent() {
        let mut c = client();
        let frames = || olr_proto::encode_frames(0, 20, 3, &|_, s| (s < 2).then_some(0), &|_| Some(0));
        c.receive(0, Channel::Unreliable, &host_inputs(20, 0, 0, frames()), 1_000);
        c.receive(0, Channel::Unreliable, &host_inputs_marked(20, 0, 0, frames(), vec![(0, 12)]), 2_000);
        assert_eq!(c.input_log().gone_at(0), None, "the first value stays");
        assert_eq!(c.drain_events(), vec![Event::Inconsistent { peer: 0, tick: 12, slot: 0 }]);
    }

    /// N83: a mark whose tick has no value for its slot in the frames is
    /// dropped (no state, no event); the value, when it comes, is unmarked.
    #[test]
    fn a_mark_without_its_value_is_dropped() {
        let mut c = client();
        let value = |t, s: usize| (s < 2 && (s == 1 || t < 12)).then_some(0);
        let frames = olr_proto::encode_frames(0, 20, 3, &value, &|_| Some(0));
        c.receive(0, Channel::Unreliable, &host_inputs_marked(20, 0, 0, frames, vec![(0, 15)]), 1_000);
        assert_eq!(c.input_log().known_upto(0), 12);
        assert_eq!(c.input_log().gone_at(0), None);
        let rest = olr_proto::encode_frames(12, 8, 3, &|_, s| (s < 2).then_some(0), &|_| Some(0));
        c.receive(0, Channel::Unreliable, &host_inputs(20, 0, 12, rest), 2_000);
        assert_eq!(c.input_log().known_upto(0), 20);
        assert_eq!(c.input_log().gone_at(0), None);
        assert!(c.drain_events().is_empty(), "neither malformed nor inconsistent");
    }

    /// N83: frames kept ahead of the frontier keep their marks.
    #[test]
    fn frames_kept_ahead_keep_their_marks() {
        let mut c = client();
        let value = |_, _| Some(0);
        let later = olr_proto::encode_frames(10, 10, 3, &value, &|_| Some(0));
        c.receive(0, Channel::Unreliable, &host_inputs_marked(20, 0, 10, later, vec![(1, 15)]), 1_000);
        assert_eq!(c.input_log().gone_at(1), None, "past the frontier: kept");
        let earlier = olr_proto::encode_frames(0, 10, 3, &value, &|_| Some(0));
        c.receive(0, Channel::Unreliable, &host_inputs(20, 0, 0, earlier), 2_000);
        assert_eq!(c.input_log().known_upto(1), 20);
        assert_eq!(c.input_log().gone_at(1), Some(15));
        assert!(c.drain_events().is_empty());
    }

    /// N83: the host's datagrams list the marks within the frames they
    /// carry, and its client takes them.
    #[test]
    fn the_hosts_datagrams_carry_its_marks() {
        let role = Role::Host { peers: vec![(1, Seat::Player(0)), (2, Seat::Player(3))] };
        let mut h = Session::new(Config::new(4, 0), role, ToySim::new(4), 0);
        let mut c = Session::new(Config::new(4, 0), Role::Client { host: 0, slot: 0 }, ToySim::new(4), 0);
        let mut f = ToyFactory { slots: 4, seed: 1 };
        let (mut hd, mut cd) = (f.driver(0, None), f.driver(1, Some(Seat::Player(0))));
        // Slot 3's player is lost: its slot is `Keys(0)` at every tick (N6),
        // the values planted here, its worm withdrawn at tick 3 (Task 4's
        // host marks it so; until then the mark is planted with them).
        h.peer_lost(2);
        for t in 0..=3 {
            assert_eq!(h.log.set_final(t, 3, 0, t == 3), SetResult::New);
        }
        let mut sent = Vec::new();
        for step in 1..=300 {
            let now = step * 1000;
            h.advance(now, hd.as_mut());
            h.transmit(&mut |_, _, b| {
                if let Ok(Datagram::Host(d)) = decode_datagram(&b, 4) {
                    sent.push(d);
                }
                c.receive(0, Channel::Unreliable, &b, now);
            });
            c.advance(now, cd.as_mut());
            c.transmit(&mut |_, _, b| h.receive(1, Channel::Unreliable, &b, now));
        }
        assert!(sent.len() > 10);
        for d in &sent {
            let spans = d.first <= 3 && 3 < d.first + d.frames.len() as u32;
            assert_eq!(d.gone, if spans { vec![(3, 3)] } else { vec![] }, "{} +{}", d.first, d.frames.len());
        }
        assert!(sent.iter().any(|d| !d.gone.is_empty()));
        assert!(h.log.known_upto(3) > 20, "the fills went on from the planted ticks");
        assert!((0..20).all(|t| h.log.get(t, 3) == Some(0)), "as a lost player's slot is filled");
        assert_eq!(c.input_log().gone_at(3), Some(3));
        assert!(!c.drain_events().iter().any(|e| matches!(e, Event::Inconsistent { .. })));
    }

    /// N83: the resume datagram lists the marks within the frames that fit.
    #[test]
    fn the_resume_datagram_lists_the_marks_it_carries() {
        let mut log = InputLog::new(32);
        // Every slot changes every tick: 64 frames do not fit one datagram.
        for t in 0..64 {
            for s in 0..32 {
                let gone = (s == 5 && t == 2) || (s == 6 && t == 60);
                log.set_final(t, s, u16::from((t as u8 + s) % 2 + 1), gone);
            }
        }
        let r = Resuming { until_us: 0, tick: 64, at_ms: 1 };
        let Ok(Datagram::Resume(d)) = decode_datagram(&resume_datagram(&log, r, 0, 64, 32), 32) else {
            panic!("a resume")
        };
        assert!(d.frames.len() < 60, "halved to {}", d.frames.len());
        assert_eq!(d.gone, vec![(5, 2)], "the mark past the frames that fit stays out");
    }

    /// A host (slots 1 to 3 its own) and a client playing slot 0 over a
    /// link of `one_way_ms` each way, for `ms` in 1 ms steps; the host's
    /// datagrams to the client are numbered from 0 and `lost(k)` drops
    /// one. Returns both and the host's datagrams to the client.
    fn pair(
        one_way_ms: u64,
        ms: u64,
        lost: impl Fn(usize) -> bool,
    ) -> (Session<ToySim>, Session<ToySim>, Vec<HostInputs>) {
        let role = Role::Host { peers: vec![(1, Seat::Player(0))] };
        let mut h = Session::new(Config::new(4, 0), role, ToySim::new(4), 0);
        let delay = input_delay(2 * one_way_ms as u32);
        let mut c = Session::new(Config::new(4, delay), Role::Client { host: 0, slot: 0 }, ToySim::new(4), 0);
        let mut f = ToyFactory { slots: 4, seed: 1 };
        let (mut hd, mut cd) = (f.driver(0, None), f.driver(1, Some(Seat::Player(0))));
        // (arrival µs, to, channel, bytes), in sending order.
        let mut wire: Vec<(u64, PeerId, Channel, Vec<u8>)> = Vec::new();
        let mut sent = Vec::new();
        for step in 1..=ms {
            let now = step * 1000;
            let (due, later): (Vec<_>, Vec<_>) = wire.into_iter().partition(|m| m.0 <= now);
            wire = later;
            for (_, to, ch, bytes) in due {
                let (session, from) = if to == 0 { (&mut h, 1) } else { (&mut c, 0) };
                session.receive(from, ch, &bytes, now);
            }
            let at = now + one_way_ms * 1000;
            h.advance(now, hd.as_mut());
            h.transmit(&mut |to, ch, bytes| {
                if ch == Channel::Unreliable {
                    let Ok(Datagram::Host(d)) = decode_datagram(&bytes, 4) else { panic!("a host datagram") };
                    sent.push(d);
                    if lost(sent.len() - 1) {
                        return;
                    }
                }
                wire.push((at, to, ch, bytes));
            });
            c.advance(now, cd.as_mut());
            c.transmit(&mut |to, ch, bytes| wire.push((at, to, ch, bytes)));
        }
        (h, c, sent)
    }

    /// The tick phase follows the pace while every due tick runs, and is
    /// `None` (draw the newest tick as it is) once the prediction limit
    /// stalls the client, or while it catches up.
    #[test]
    fn the_tick_phase_follows_the_pace_until_a_stall() {
        let mut c = client();
        let mut d = driver();
        c.advance(25_000, d.as_mut());
        assert_eq!(c.current_tick(), 2);
        assert!(c.tick_phase(25_000).is_some_and(|p| (p - 0.5).abs() < 1e-6));
        assert!(c.tick_phase(29_000).is_some_and(|p| (p - 0.9).abs() < 1e-6), "it grows within the tick");
        // No host: the client predicts up to its limit, then stalls.
        let mut now = 25_000;
        while !c.stalling {
            now += 10_000;
            c.advance(now, d.as_mut());
            assert!(now < 10_000_000, "the client stalls");
        }
        assert_eq!(c.tick_phase(now), None, "stalled: the newest tick as it is");
        // Far behind the pace: catching up, at most MAX_CATCHUP a call.
        let mut h = Session::new(Config::new(2, 0), Role::Host { peers: vec![] }, ToySim::new(2), 0);
        h.advance(500_000, d.as_mut());
        assert_eq!((h.current_tick(), h.tick_phase(500_000)), (MAX_CATCHUP, None), "catching up");
        let mut t = 500_000;
        while h.current_tick() < 50 {
            t += 1_000;
            h.advance(t, d.as_mut());
        }
        assert!(h.tick_phase(t).is_some(), "caught up");
        let spectator = Session::new(Config::new(2, 0), Role::Spectator { host: 0 }, ToySim::new(2), 0);
        assert_eq!(spectator.tick_phase(5_000), None, "a spectator follows the host's frames");
    }

    /// The client confirmed the host's inputs and is at most `lag` ticks behind its frontier.
    fn confirmed_alike(h: &Session<ToySim>, c: &Session<ToySim>, lag: u32) {
        assert_eq!(c.stats.malformed, 0);
        assert!(c.frontier() + lag >= h.frontier(), "client {} host {}", c.frontier(), h.frontier());
        for t in 0..c.frontier() {
            assert_eq!(c.input_log().row(t), h.input_log().row(t), "tick {t}");
        }
    }

    #[test]
    fn a_short_round_trip_gets_each_change_in_a_few_datagrams() {
        // 80 ms: the client's ack is about ten ticks behind the host's frontier.
        let (h, c, sent) = pair(40, 3_000, |_| false);
        let late = &sent[100..];
        let most = late.iter().map(|d| d.frames.len()).max().unwrap();
        assert!(most <= COPIES + 3, "at most {most} frames");
        // Every unacknowledged frame would be more.
        assert!(h.frontier() - h.peers[&1].ack >= 8, "acknowledged {} of {}", h.peers[&1].ack, h.frontier());
        confirmed_alike(&h, &c, 15);
    }

    #[test]
    fn a_long_round_trip_gets_every_unacknowledged_frame() {
        // 120 ms: a resend would come too late for the prediction limit.
        let (h, c, sent) = pair(60, 3_000, |_| false);
        let least = sent[100..].iter().map(|d| d.frames.len()).min().unwrap();
        assert!(least >= 10, "at least {least} frames");
        confirmed_alike(&h, &c, 20);
    }

    #[test]
    fn inputs_lost_beyond_the_copies_are_resent_from_the_ack() {
        // Ten datagrams in a row are lost: more than `COPIES` repeat a change.
        let (h, c, sent) = pair(40, 3_000, |k| (150..160).contains(&k));
        let resent = sent[160..].iter().position(|d| d.frames.len() > COPIES + 3).expect("a resend");
        assert!(resent < 15, "the resend came {resent} datagrams after the loss");
        confirmed_alike(&h, &c, 15);
    }

    #[test]
    fn a_hostile_margin_jumps_only_so_far() {
        // Early by i16::MAX ticks: held back by the prediction limit, not for minutes.
        let mut c = client();
        c.receive(0, Channel::Unreliable, &host_inputs(0, i16::MAX, 0, vec![]), 0);
        assert_eq!(c.pace.ticks(0), -(MAX_PREDICTION as i64));
        // Late by i16::MAX ticks: ahead by the silence timeout's worth.
        let mut c = client();
        c.receive(0, Channel::Unreliable, &host_inputs(0, i16::MIN, 0, vec![]), 0);
        assert_eq!(c.pace.ticks(0), (SILENCE_TIMEOUT_US / TICK_US) as i64);
        assert_eq!(c.stats.jumps, 1);
    }

    #[test]
    fn a_held_pace_has_no_tick_phase() {
        // A jump back holds the client: no tick runs, so there is nothing
        // to draw between (the phase would swing back every tick).
        let mut c = client();
        let mut d = driver();
        c.advance(100_000, d.as_mut());
        assert!(c.tick_phase(105_000).is_some());
        c.receive(0, Channel::Unreliable, &host_inputs(10, i16::MAX, 0, vec![]), 105_000);
        assert_eq!(c.tick_phase(105_000), None, "held");
    }

    #[test]
    fn a_reordered_margin_never_jumps() {
        // Ruling RS-1: a datagram overtaken by a newer one carries a stale
        // margin; a jump on it would undo a correction already made.
        let mut c = client();
        c.receive(0, Channel::Unreliable, &host_inputs(30, 1, 0, vec![]), 0);
        c.receive(0, Channel::Unreliable, &host_inputs(20, -30, 0, vec![]), 1_000);
        assert_eq!(c.stats.jumps, 0, "an older datagram");
        c.receive(0, Channel::Unreliable, &host_inputs(31, -30, 0, vec![]), 2_000);
        assert_eq!(c.stats.jumps, 1, "a newer one");
    }

    #[test]
    fn a_resumes_moment_moves_the_pace_at_most_its_bound() {
        // `at_ms` five seconds ago, or a hundred to come: the pace moves by
        // at most `RESUME_AT_BOUND_MS` (a hostile host's whim).
        let resume = |at_ms| {
            encode_control(&Control::Resume { tick: 0, at_ms, first: 0, frames: vec![], gone: vec![] })
        };
        let now = 5_000_000;
        let mut c = client();
        c.rehost(7, 0);
        c.receive(7, Channel::Reliable, &resume(1), now);
        assert!(!c.suspended());
        assert_eq!(c.pace.ticks(now), 200, "caught up two seconds' worth");
        let mut c = client();
        c.rehost(7, 0);
        c.receive(7, Channel::Reliable, &resume(105_000), now);
        assert_eq!(c.pace.ticks(now + 2_000_000), 0, "held two seconds");
        assert_eq!(c.pace.ticks(now + 2_010_000), 1);
    }

    #[test]
    fn a_hostile_host_tick_does_not_pin_the_integral_term() {
        // Margin -1: an error of 2 ticks, integrated over the host's ticks.
        let mut c = client();
        for (i, tick) in [10, 20, u32::MAX, 30].into_iter().enumerate() {
            c.receive(0, Channel::Unreliable, &host_inputs(tick, -1, 0, vec![]), i as u64 * 100_000);
        }
        assert_eq!(c.sync_tick, Some(30));
        assert_eq!(c.sync_i, 2 * 10 + 2 * 10);
        // An older tick is not taken either.
        c.receive(0, Channel::Unreliable, &host_inputs(25, -1, 0, vec![]), 500_000);
        assert_eq!((c.sync_tick, c.sync_i), (Some(30), 40));
    }

    #[test]
    fn a_lost_peer_gets_nothing_already_queued() {
        let role = Role::Host { peers: vec![(1, Seat::Player(1)), (2, Seat::Spectator)] };
        let mut h = Session::new(Config::new(3, 0), role, ToySim::new(3), 0);
        h.advance(10_000, driver().as_mut());
        assert!(h.out.iter().any(|&(to, _, _)| to == 1));
        h.peer_lost(1);
        let mut sent = Vec::new();
        h.transmit(&mut |to, _, _| sent.push(to));
        assert_eq!(sent, vec![2]);
    }

    #[test]
    fn a_change_through_sim_mut_is_never_rolled_back() {
        // Ten ticks predicted, the change, five more.
        let mut c = client();
        let mut drv = driver();
        for ms in 1..=100u64 {
            c.advance(ms * 1000, drv.as_mut());
        }
        assert_eq!(c.current_tick(), 10);
        c.sim_mut().displace(0, 1);
        for ms in 101..=150u64 {
            c.advance(ms * 1000, drv.as_mut());
        }
        // Slot 0 pressed something else from tick 0 on: a rollback, but to
        // the change, not across it.
        let own = c.own.clone();
        let value = |t: u32, s: usize| Some([5, 0, own[t as usize]][s]);
        let frames = olr_proto::encode_frames(0, 15, 3, &value, &|_| Some(0));
        c.receive(0, Channel::Unreliable, &host_inputs(15, 1, 0, frames), 150_000);
        c.advance(151_000, drv.as_mut());
        let rollbacks: Vec<Event> =
            c.drain_events().into_iter().filter(|e| matches!(e, Event::Rollback { .. })).collect();
        assert_eq!(rollbacks, vec![Event::Rollback { from: 10, to: 15 }]);
        assert_eq!(c.checksums(), &[]); // tick 9's was not known when the change came
    }

    /// The state of a fresh `ToySim` with `displace(0, 1)` applied before
    /// tick `x`, after ticks `0..to` on the keys `c` knows (final or predicted).
    fn replay_with_change(c: &Session<ToySim>, x: u32, to: u32) -> Vec<u8> {
        let mut s = ToySim::new(3);
        for t in 0..to {
            if t == x {
                s.displace(0, 1);
            }
            let keys: Vec<Keys> = (0..3u8).map(|sl| Keys(c.keys_for(t, sl))).collect();
            s.tick(&keys);
        }
        s.encode_state()
    }

    /// The host's datagram with finals for ticks `0..n`: slot 0 presses
    /// `a(t)`, slot 1 `t`, slot 2 what the client pressed itself.
    fn finals(c: &Session<ToySim>, n: u32, a: fn(u32) -> u16) -> Vec<u8> {
        let own = c.own.clone();
        let value = move |t: u32, s: usize| Some([a(t), t as u16, own[t as usize]][s]);
        host_inputs(n, 1, 0, olr_proto::encode_frames(0, n, 3, &value, &|_| Some(0)))
    }

    #[test]
    fn a_change_to_the_confirmed_state_keeps_the_predicted_ticks_open() {
        for pending in [true, false] {
            let mut c = client();
            let mut drv = driver();
            for ms in 1..=150u64 {
                c.advance(ms * 1000, drv.as_mut());
            }
            let to = c.current_tick();
            assert_eq!(to, 15);
            // Finals for ticks 0..12 that differ from the predictions.
            let data = finals(&c, 12, |t| 5 + (t as u16 % 3));
            c.receive(0, Channel::Unreliable, &data, 150_000);
            if !pending {
                c.advance(151_000, drv.as_mut());
                c.drain_events();
            }
            let to = c.current_tick().max(to);
            assert_eq!(c.rollback_from.is_some(), pending);
            let x = if pending { 0 } else { 12 };
            let sums_before = c.checksums().to_vec();
            let (resim, rollbacks) = (c.stats.resim_ticks, c.stats.rollbacks);
            c.change_confirmed(drv.as_mut(), |s| s.displace(0, 1));
            assert_eq!(c.current_tick(), to);
            assert_eq!(c.sim().encode_state(), replay_with_change(&c, x, to), "pending {pending}");
            assert_eq!(c.stats.resim_ticks, resim + (to - x) as u64);
            assert_eq!(c.stats.rollbacks, rollbacks, "no rollback is counted");
            assert_eq!(c.rollback_from, None);
            let below = |v: &[(u32, u64)]| v.iter().filter(|e| e.0 < x).copied().collect::<Vec<_>>();
            assert_eq!(below(c.checksums()), below(&sums_before));
            if !pending {
                assert!(!below(c.checksums()).is_empty(), "tick 9's checksum is kept");
            }
            // Finals for ticks 12.. that differ again: a rollback to them,
            // not before the change; the change stays.
            let data = finals(&c, 15, |t| if t < 12 { 5 + (t as u16 % 3) } else { 9 });
            c.receive(0, Channel::Unreliable, &data, 152_000);
            c.advance(153_000, drv.as_mut());
            let rb: Vec<Event> =
                c.drain_events().into_iter().filter(|e| matches!(e, Event::Rollback { .. })).collect();
            assert_eq!(rb.len(), 1, "{rb:?}");
            assert!(matches!(rb[0], Event::Rollback { from, .. } if from >= x));
            let now = c.current_tick();
            assert_eq!(c.sim().encode_state(), replay_with_change(&c, x, now), "after the rollback");
        }
    }

    #[test]
    fn a_player_cannot_become_a_spectator() {
        let role = Role::Host { peers: vec![(1, Seat::Player(1))] };
        let mut h = Session::new(Config::new(3, 0), role, ToySim::new(3), 0);
        h.add_spectator(1, 0);
        assert_eq!(h.drain_events(), vec![Event::JoinRefused { peer: 1 }]);
        assert_eq!(h.peers[&1].seat, Seat::Player(1));
        h.peer_lost(1);
        h.add_spectator(1, 0); // a lost player still holds its slot
        assert_eq!(h.drain_events(), vec![Event::JoinRefused { peer: 1 }]);
        h.add_spectator(2, 0);
        assert_eq!(h.drain_events(), vec![]);
        assert_eq!(h.peers[&2].seat, Seat::Spectator);
    }

    /// A world whose state is larger than any peer accepts.
    struct Huge;

    impl Simulation for Huge {
        type Snapshot = ();
        fn tick(&mut self, _keys: &[Keys]) {}
        fn save(&mut self) {}
        fn restore(&mut self, _s: &()) {}
        fn forget_before(&mut self, _s: &()) {}
        fn checksum(&self) -> u64 {
            0
        }
        fn encode_state(&self) -> Vec<u8> {
            alloc::vec![0; olr_proto::MAX_STATE + 1]
        }
        fn decode_state(&mut self, _bytes: &[u8]) -> Result<(), crate::StateRejected> {
            Ok(())
        }
        fn withdraw(&mut self, _slot: u8) {}
        fn withdrawn(&self) -> u32 {
            0
        }
    }

    /// Final review A-M4: a new mark below the snapshots kept is a broken
    /// invariant (new values sit at or past the frontier), never skipped
    /// silently; only a loaded state's floor may lie above it (the state
    /// holds the withdrawal already).
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "a withdrawal below the snapshots kept")]
    fn a_mark_below_the_snapshots_is_a_broken_invariant() {
        let mut h = Session::new(Config::new(1, 0), Role::Host { peers: vec![] }, ToySim::new(1), 0);
        h.advance(100_000, &mut Quiet);
        assert!(h.base > 1 && h.floor == 0, "base {}", h.base);
        h.on_gone(0);
    }

    #[test]
    fn a_mark_below_a_loaded_state_is_held_by_it() {
        let mut h = Session::new(Config::new(1, 0), Role::Host { peers: vec![] }, ToySim::new(1), 0);
        h.advance(100_000, &mut Quiet);
        h.sim_mut();
        h.on_gone(0);
        assert_eq!(h.rollback_from, None);
    }

    #[test]
    fn a_state_too_large_to_send_fails_the_join() {
        let mut h = Session::new(Config::new(1, 0), Role::Host { peers: vec![] }, Huge, 0);
        h.add_spectator(5, 0);
        h.advance(10_000, &mut Quiet);
        let events = h.drain_events();
        assert_eq!(events, vec![Event::RepairFailed { peer: 5 }]);
        assert_eq!(h.peers[&5].checks.repair, Repair::Failed);
        assert_eq!(h.stats.states_sent, 0);
    }

    #[test]
    fn a_client_always_sends_its_newest_input() {
        // The host confirms every tick but never acknowledges our inputs:
        // more of them are unacknowledged than a datagram holds.
        let mut c = client();
        let mut drv = driver();
        let mut last = None;
        for ms in 1..=1_500u64 {
            let first = c.frontier();
            let frames = olr_proto::encode_frames(first, 10, 3, &|_, _| Some(0), &|_| Some(0));
            c.receive(0, Channel::Unreliable, &host_inputs(c.current_tick(), 1, first, frames), ms * 1000);
            c.advance(ms * 1000, drv.as_mut());
            c.transmit(&mut |_, _, b| last = Some(b));
        }
        assert!(c.own.len() > MAX_KEYS + 10);
        let Ok(Datagram::Client(ci)) = decode_datagram(&last.unwrap(), 3) else {
            panic!("a client datagram")
        };
        assert_eq!(ci.keys.len(), MAX_KEYS);
        assert_eq!(ci.first as usize + ci.keys.len(), c.own.len());
    }
}
