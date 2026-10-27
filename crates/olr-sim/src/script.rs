//! Scripts' state in the world (issue #88; the asset-based levels design,
//! calls 10, 12 and 20). No OLX counterpart.
//!
//! The simulation runs no script itself: a [`Director`] (`olr-script`'s
//! Rhai runtime) does, once a tick, through a [`LiveCtx`]. The director is
//! stateless; everything a script keeps lives here, in the world, so it is
//! hashed, cloned for rollback and transferred like the rest:
//! - one **instance** per script run: the level script, then each live
//!   copy's scripts in run order ([`LiveInstance`]);
//! - each instance's **memory** (`this.mem`: integers, booleans and
//!   strings by key, packed into one byte buffer for all instances, keys
//!   in sorted order: [`MemValue`], [`memory_ok`]), its **timers**
//!   ([`LiveTimer`]), its **error count** and whether it is **switched
//!   off** or has **started**;
//! - the scripts' own **generator** ([`ScriptRng`], seeded from the match
//!   seed: never the world's `rand()` or `FindSpot` streams);
//! - the copies' state changes of the last script phase
//!   ([`ScriptEvent::Entered`]), which handlers see in the next tick.
//!
//! What a tick's runs cost is counted ([`World::script_peak`]), not state.

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;

use serde::{Deserialize, Serialize};

use crate::World;
use crate::buf::Buf;
use crate::checksum::Fnv;
use crate::events::{EventKind, TickEvents};
use crate::live::{LiveLevel, LiveMotion, LiveParts, LiveView, LiveWorld};
use crate::worm::Worm;

/// The most timers an instance has at once.
pub const MAX_TIMERS: usize = 16;
/// The most keys an instance's memory holds.
pub const MAX_MEMORY_KEYS: usize = 32;
/// The longest key, and the longest string value, of an instance's
/// memory, bytes.
pub const MAX_MEMORY_BYTES: usize = 64;
/// How many failed runs switch an instance off for the match.
pub const MAX_SCRIPT_ERRORS: u8 = 16;
/// The most commands the scripts give in a tick, all runs together.
pub const MAX_LIVE_COMMANDS: usize = 128;

/// The scripts of a level, run once a tick (issue #88): `olr-script`'s
/// Rhai runtime, or a test's. It must be deterministic and **stateless**:
/// [`Director::run`] takes `&self`, and nothing it holds may change while
/// it runs (two worlds ticking on two threads share it), so every bit of
/// script state is in the world, read and written through [`LiveCtx`].
/// `Clone` is [`Director::box_clone`], so the world stays `Clone`.
pub trait Director: fmt::Debug + Send + Sync {
    /// A copy of the director (an `Arc` clone), for `World::clone`.
    fn box_clone(&self) -> Box<dyn Director>;

    /// How many functions of script `script` (an index into
    /// [`LiveLevel::scripts`]) a timer can name ([`LiveTimer::func`]
    /// below it): what [`World::load_state`] checks a received timer by.
    fn timer_functions(&self, script: u16) -> u16;

    /// The tick's script phase: every run, in order.
    fn run(&self, ctx: &mut LiveCtx<'_>);
}

impl Clone for Box<dyn Director> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

/// One running script: the level script (`copy: None`) or one of a live
/// copy's scripts, `script` an index into [`LiveLevel::scripts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveInstance {
    pub copy: Option<u16>,
    pub script: u16,
}

/// The instances of `level` in run order: the level script, then each
/// copy in level order, each of its scripts in run order (its asset's
/// default, then its own).
pub fn live_instances(level: &LiveLevel) -> Vec<LiveInstance> {
    let mut out = Vec::new();
    if let Some(script) = level.level_script {
        out.push(LiveInstance { copy: None, script });
    }
    for (i, c) in level.copies.iter().enumerate() {
        let Ok(copy) = u16::try_from(i) else { break };
        out.extend(c.scripts.iter().map(|&script| LiveInstance { copy: Some(copy), script }));
    }
    out
}

/// A timer of an instance: function `func` of its script (an index the
/// director defines, below [`Director::timer_functions`]) runs in the
/// script phase of server frame `due`, then again every `period` frames
/// (0: once).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveTimer {
    pub func: u16,
    pub due: u64,
    pub period: u64,
}

/// The scripts' own generator (SplitMix64), seeded from the match seed,
/// drawn in run order: scripts never touch the world's `rand()` or
/// `FindSpot` streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptRng(u64);

impl ScriptRng {
    /// The generator of match seed `seed`.
    pub fn new(seed: u32) -> Self {
        ScriptRng(u64::from(seed) ^ 0x6f6c_725f_6c69_7665)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number from 0 to `n - 1` (0 for `n` 0), one draw.
    pub fn below(&mut self, n: u64) -> u64 {
        ((u128::from(self.next_u64()) * u128::from(n)) >> 64) as u64
    }
}

/// One value of an instance's memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemValue<'a> {
    Int(i64),
    Bool(bool),
    Str(&'a str),
}

const TAG_INT: u8 = 0;
const TAG_FALSE: u8 = 1;
const TAG_TRUE: u8 = 2;
const TAG_STR: u8 = 3;

/// Appends entry `(key, value)` to an instance's memory bytes `out`: the
/// key's length (a byte) and bytes, a tag, then 8 bytes (little-endian)
/// for an integer, or the string's length and bytes. The caller keeps the
/// keys in sorted order and at most [`MAX_MEMORY_KEYS`] ([`memory_ok`]
/// checks them); a key or string longer than [`MAX_MEMORY_BYTES`] is
/// refused (false, nothing written).
pub fn push_memory(out: &mut Vec<u8>, key: &str, value: MemValue<'_>) -> bool {
    let long = |s: &str| s.len() > MAX_MEMORY_BYTES;
    if long(key) || matches!(value, MemValue::Str(s) if long(s)) {
        return false;
    }
    out.push(key.len() as u8);
    out.extend_from_slice(key.as_bytes());
    match value {
        MemValue::Int(v) => {
            out.push(TAG_INT);
            out.extend_from_slice(&v.to_le_bytes());
        }
        MemValue::Bool(b) => out.push(if b { TAG_TRUE } else { TAG_FALSE }),
        MemValue::Str(s) => {
            out.push(TAG_STR);
            out.push(s.len() as u8);
            out.extend_from_slice(s.as_bytes());
        }
    }
    true
}

/// The entries of an instance's memory bytes, in order; stops at the
/// first malformed one (bytes [`memory_ok`] accepts have none).
pub fn memory_entries(bytes: &[u8]) -> MemoryEntries<'_> {
    MemoryEntries { rest: bytes }
}

/// [`memory_entries`]' iterator.
#[derive(Clone, Debug)]
pub struct MemoryEntries<'a> {
    rest: &'a [u8],
}

impl<'a> MemoryEntries<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.rest.len() < n {
            return None;
        }
        let (a, b) = self.rest.split_at(n);
        self.rest = b;
        Some(a)
    }

    fn entry(&mut self) -> Option<(&'a str, MemValue<'a>)> {
        let n = usize::from(*self.take(1)?.first()?);
        let key = core::str::from_utf8(self.take(n)?).ok()?;
        let value = match *self.take(1)?.first()? {
            TAG_INT => MemValue::Int(i64::from_le_bytes(self.take(8)?.try_into().ok()?)),
            TAG_FALSE => MemValue::Bool(false),
            TAG_TRUE => MemValue::Bool(true),
            TAG_STR => {
                let n = usize::from(*self.take(1)?.first()?);
                MemValue::Str(core::str::from_utf8(self.take(n)?).ok()?)
            }
            _ => return None,
        };
        Some((key, value))
    }
}

impl<'a> Iterator for MemoryEntries<'a> {
    type Item = (&'a str, MemValue<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let e = self.entry();
        if e.is_none() {
            self.rest = &[];
        }
        e
    }
}

/// Whether `bytes` are an instance's memory as [`push_memory`] writes it:
/// every entry well formed, at most [`MAX_MEMORY_KEYS`], keys strictly
/// in order, keys and strings at most [`MAX_MEMORY_BYTES`] of UTF-8.
pub fn memory_ok(bytes: &[u8]) -> bool {
    let mut it = memory_entries(bytes);
    let mut n = 0;
    let mut last: Option<&str> = None;
    while let Some((key, value)) = it.entry() {
        n += 1;
        let str_ok = match value {
            MemValue::Str(s) => s.len() <= MAX_MEMORY_BYTES,
            _ => true,
        };
        if n > MAX_MEMORY_KEYS || key.len() > MAX_MEMORY_BYTES || !str_ok || last.is_some_and(|l| l >= key) {
            return false;
        }
        last = Some(key);
        if it.rest.is_empty() {
            return true;
        }
    }
    bytes.is_empty()
}

/// What a script's run asks of the world, applied in order after the run
/// ([`LiveCtx::apply`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveCommand {
    /// Copy `copy` enters state `state` (indices).
    SetState { copy: u16, state: u16 },
    /// Copy `copy`'s value `prop` becomes `value` (the world's units),
    /// clamped into its limits.
    SetValue { copy: u16, prop: u16, value: i64 },
    /// Instance `instance` is switched off for the match.
    SwitchOff { instance: u16 },
    /// Copy `copy`'s position-like value `prop` moves towards `target`
    /// (the world's units, clamped into its limits) by `speed` a tick
    /// (1/[`crate::LIVE_SUBPIXELS`] pixel, clamped into 1 to
    /// [`crate::MAX_MOTION_SPEED`]), in place of its motion (issue #89).
    Move { copy: u16, prop: u16, target: i64, speed: i64 },
    /// Copy `copy`'s value `prop` stops moving.
    Stop { copy: u16, prop: u16 },
}

/// An event a script's handlers see ([`LiveCtx::events`]): positions in
/// whole world pixels and amounts truncated toward zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptEvent {
    /// Copy `copy` entered `state`.
    Entered {
        copy: u16,
        state: u16,
    },
    /// Copy `copy`'s value `prop` reached its motion's target (issue #89).
    Arrived {
        copy: u16,
        prop: u16,
    },
    /// Copy `copy`'s value `prop` stopped short of its motion's target:
    /// rock was in the way of a part it moves (issue #90).
    Blocked {
        copy: u16,
        prop: u16,
    },
    Spawned {
        worm: i32,
    },
    Killed {
        victim: i32,
        killer: i32,
    },
    Exploded {
        x: i64,
        y: i64,
        damage: i64,
    },
    Damaged {
        worm: i32,
        amount: i64,
        by: i32,
    },
}

impl ScriptEvent {
    /// The script event of `e`, if handlers see it.
    fn of(e: &EventKind) -> Option<ScriptEvent> {
        let px = |v: f32| v as i64;
        Some(match *e {
            EventKind::LiveState { copy, state } => ScriptEvent::Entered { copy, state },
            EventKind::LiveArrived { copy, prop } => ScriptEvent::Arrived { copy, prop },
            EventKind::LiveBlocked { copy, prop } => ScriptEvent::Blocked { copy, prop },
            EventKind::Spawned { worm } => ScriptEvent::Spawned { worm },
            EventKind::Kill { victim, killer } => ScriptEvent::Killed { victim, killer },
            EventKind::Explosion { pos, damage, .. } => {
                ScriptEvent::Exploded { x: px(pos.x), y: px(pos.y), damage: px(damage) }
            }
            EventKind::Damage { worm, amount, by, .. } => {
                ScriptEvent::Damaged { worm, amount: px(amount), by }
            }
            _ => return None,
        })
    }
}

/// The most operations any one run, and any one tick's runs together,
/// used since the world was made ([`World::script_peak`]). Not state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptPeak {
    pub run: u64,
    pub tick: u64,
}

/// What a run reads of the world, moved into the run's API object and
/// back (no copy a run): every copy's state, values and their motions,
/// the worms, the clocks. [`LiveCtx::take_reads`] gives it up to date.
#[derive(Clone, Debug, Default)]
pub struct LiveReads {
    states: Vec<u16>,
    values: Vec<i64>,
    motions: Vec<LiveMotion>,
    offsets: Vec<u32>,
    worms: Vec<(bool, i64, i64)>,
    /// Milliseconds since the match started (the frame clock).
    pub time_ms: i64,
    /// The server frame: what timers count in.
    pub frame: u64,
    /// The match is over.
    pub game_over: bool,
}

impl LiveReads {
    /// Copy `copy`'s state (0 for none).
    pub fn state(&self, copy: u16) -> u16 {
        self.states.get(usize::from(copy)).copied().unwrap_or(0)
    }

    fn span(&self, copy: u16) -> core::ops::Range<usize> {
        let i = usize::from(copy);
        let at = |k: usize| self.offsets.get(k).map_or(0, |&o| o as usize);
        let (s, e) = (at(i), at(i + 1));
        s..e.max(s)
    }

    /// Copy `copy`'s values in the world's units, by prop index.
    pub fn values(&self, copy: u16) -> &[i64] {
        self.values.get(self.span(copy)).unwrap_or(&[])
    }

    /// Copy `copy`'s values' motions, by prop index (issue #89).
    pub fn motions(&self, copy: u16) -> &[LiveMotion] {
        self.motions.get(self.span(copy)).unwrap_or(&[])
    }

    /// The worms: alive, and their position in whole pixels (truncated).
    pub fn worms(&self) -> &[(bool, i64, i64)] {
        &self.worms
    }

    /// Copies copy `copy`'s state, values and motions from the world's.
    fn sync_copy(&mut self, w: &LiveParts<'_>, copy: u16) {
        let i = usize::from(copy);
        if let (Some(d), Some(&s)) = (self.states.get_mut(i), w.states.get(i)) {
            *d = s;
        }
        let at = |k: usize| w.offsets.get(k).map_or(0, |&o| o as usize);
        let r = at(i)..at(i + 1).max(at(i));
        if let (Some(d), Some(s)) = (self.values.get_mut(r.clone()), w.values.get(r.clone())) {
            d.copy_from_slice(s);
        }
        if let (Some(d), Some(s)) = (self.motions.get_mut(r.clone()), w.motions.get(r)) {
            d.copy_from_slice(s);
        }
    }
}

/// Items of every instance in one buffer: instance `i`'s are
/// `items[at[i]..at[i + 1]]`. A rollback snapshot clones two buffers.
#[derive(Clone, Debug, PartialEq)]
struct Arena<T> {
    items: Buf<T>,
    at: Buf<u32>,
}

impl<T: Copy> Arena<T> {
    fn new(n: usize) -> Self {
        let mut at = Buf::with_capacity(n + 1);
        at.resize(n + 1, 0);
        Arena { items: Buf::with_capacity(0), at }
    }

    fn range(&self, i: usize) -> core::ops::Range<usize> {
        let s = self.at.get(i).map_or(0, |&o| o as usize);
        let e = self.at.get(i + 1).map_or(s, |&o| o as usize);
        s.min(self.items.len())..e.max(s).min(self.items.len())
    }

    fn get(&self, i: usize) -> &[T] {
        &self.items[self.range(i)]
    }

    fn set(&mut self, i: usize, src: &[T]) {
        if i + 1 >= self.at.len() {
            return;
        }
        let r = self.range(i);
        if r.len() == src.len() {
            self.items[r].copy_from_slice(src);
            return;
        }
        let old = r.len();
        self.items.splice(r, src.iter().copied());
        for o in self.at[i + 1..].iter_mut() {
            *o = (*o as usize + src.len() - old) as u32;
        }
    }

    /// Whether `items` and `at` are an arena of `n` instances.
    fn shape_ok(items: &[T], at: &[u32], n: usize) -> bool {
        at.len() == n + 1
            && at.first() == Some(&0)
            && at.last().is_some_and(|&e| e as usize == items.len())
            && at.windows(2).all(|w| w[0] <= w[1])
    }
}

/// Not state: always equal, so two worlds that differ only here are equal.
#[derive(Clone, Debug, Default)]
struct NotState<T>(T);

impl<T> PartialEq for NotState<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// The director of a world: setup, never hashed or transferred.
#[derive(Clone, Debug)]
struct DirectorSlot(Option<Box<dyn Director>>);

impl PartialEq for DirectorSlot {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// Per-instance flags.
const OFF: u8 = 1;
const STARTED: u8 = 2;

/// What a script phase uses and does not keep.
#[derive(Clone, Debug, Default)]
struct Phase {
    events: Vec<ScriptEvent>,
    entered: Vec<(u16, u16)>,
    reads: LiveReads,
    peak: ScriptPeak,
}

/// The scripts' part of [`LiveWorld`] (present when the setup has a
/// director).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ScriptWorld {
    director: DirectorSlot,
    /// Setup: the instances in run order.
    instances: Arc<[LiveInstance]>,
    memory: Arena<u8>,
    timers: Arena<LiveTimer>,
    errors: Buf<u8>,
    flags: Buf<u8>,
    rng: ScriptRng,
    /// The copies' state changes of the last script phase, which handlers
    /// see in the next tick.
    entered: Buf<(u16, u16)>,
    phase: NotState<Phase>,
}

impl ScriptWorld {
    pub(crate) fn new(director: Box<dyn Director>, level: &LiveLevel, seed: u32) -> ScriptWorld {
        let instances: Arc<[LiveInstance]> = live_instances(level).into();
        let n = instances.len();
        let mut errors = Buf::with_capacity(n);
        errors.resize(n, 0);
        let mut flags = Buf::with_capacity(n);
        flags.resize(n, 0);
        ScriptWorld {
            director: DirectorSlot(Some(director)),
            instances,
            memory: Arena::new(n),
            timers: Arena::new(n),
            errors,
            flags,
            rng: ScriptRng::new(seed),
            entered: Buf::with_capacity(MAX_LIVE_COMMANDS),
            phase: NotState::default(),
        }
    }

    pub(crate) fn peak(&self) -> ScriptPeak {
        self.phase.0.peak
    }

    pub(crate) fn hash(&self, h: &mut Fnv) {
        let ScriptWorld { director: _, instances, memory, timers, errors, flags, rng, entered, phase: _ } =
            self;
        h.usize(instances.len());
        h.usize(memory.items.len());
        h.bytes(&memory.items);
        for &o in memory.at.iter().chain(timers.at.iter()) {
            h.u32(o);
        }
        for t in timers.items.iter() {
            h.bytes(&t.func.to_le_bytes());
            h.u64(t.due);
            h.u64(t.period);
        }
        h.bytes(errors);
        h.bytes(flags);
        h.u64(rng.0);
        h.usize(entered.len());
        for &(c, s) in entered.iter() {
            h.bytes(&c.to_le_bytes());
            h.bytes(&s.to_le_bytes());
        }
    }

    pub(crate) fn wire(&self) -> ScriptWire {
        let ScriptWorld { director: _, instances: _, memory, timers, errors, flags, rng, entered, phase: _ } =
            self;
        ScriptWire {
            memory: memory.items.to_vec(),
            memory_at: memory.at.to_vec(),
            timers: timers.items.to_vec(),
            timer_at: timers.at.to_vec(),
            errors: errors.to_vec(),
            flags: flags.to_vec(),
            rng: rng.0,
            entered: entered.to_vec(),
        }
    }

    /// Whether `w` fits: as many instances, each one's memory well formed
    /// and within its bounds, at most [`MAX_TIMERS`] timers each naming a
    /// function of its script, error counts within
    /// [`MAX_SCRIPT_ERRORS`], known flags, and state changes of known
    /// copies' states, at most [`MAX_LIVE_COMMANDS`].
    pub(crate) fn can_load(&self, w: &ScriptWire, level: &LiveLevel) -> bool {
        let ScriptWire { memory, memory_at, timers, timer_at, errors, flags, rng: _, entered } = w;
        let n = self.instances.len();
        let Some(director) = &self.director.0 else { return false };
        if !(Arena::shape_ok(memory, memory_at, n)
            && Arena::shape_ok(timers, timer_at, n)
            && errors.len() == n
            && flags.len() == n
            && errors.iter().all(|&e| e <= MAX_SCRIPT_ERRORS)
            && flags.iter().all(|&f| f & !(OFF | STARTED) == 0)
            && entered.len() <= MAX_LIVE_COMMANDS)
        {
            return false;
        }
        let entered_ok = entered
            .iter()
            .all(|&(c, s)| level.copies.get(usize::from(c)).is_some_and(|c| usize::from(s) < c.states.len()));
        entered_ok
            && self.instances.iter().enumerate().all(|(i, inst)| {
                let (m, t) = (&memory[span(memory_at, i)], &timers[span(timer_at, i)]);
                let funcs = director.timer_functions(inst.script);
                memory_ok(m) && t.len() <= MAX_TIMERS && t.iter().all(|t| t.func < funcs)
            })
    }

    /// Takes `w`, checked by [`ScriptWorld::can_load`].
    pub(crate) fn load(&mut self, w: ScriptWire) {
        let ScriptWire { memory, memory_at, timers, timer_at, errors, flags, rng, entered } = w;
        self.memory.items.clear();
        self.memory.items.extend(memory);
        self.memory.at.copy_from_slice(&memory_at);
        self.timers.items.clear();
        self.timers.items.extend(timers);
        self.timers.at.copy_from_slice(&timer_at);
        self.errors.copy_from_slice(&errors);
        self.flags.copy_from_slice(&flags);
        self.rng = ScriptRng(rng);
        self.entered.clear();
        self.entered.extend(entered);
    }
}

/// `at[i]..at[i + 1]`, checked by `Arena::shape_ok`.
fn span(at: &[u32], i: usize) -> core::ops::Range<usize> {
    at[i] as usize..at[i + 1] as usize
}

/// The scripts' part of a [`crate::LiveWire`]: every instance's memory
/// and timers (flat, with offsets), error counts and flags, the
/// generator, and the last script phase's state changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptWire {
    memory: Vec<u8>,
    memory_at: Vec<u32>,
    timers: Vec<LiveTimer>,
    timer_at: Vec<u32>,
    errors: Vec<u8>,
    flags: Vec<u8>,
    rng: u64,
    entered: Vec<(u16, u16)>,
}

/// One part of a [`ScriptWire`] to spoil, for tests of
/// [`World::load_state`]'s checks.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum ScriptWireEdit {
    /// Byte `i` of the memory becomes the value.
    MemoryByte(usize, u8),
    /// Appends bytes `n` copies of the value to the memory (the last
    /// instance's).
    MemoryGrow(usize, u8),
    /// Timer `i`'s function becomes the value.
    TimerFunc(usize, u16),
    /// Instance `i`'s error count becomes the value.
    Errors(usize, u8),
    /// Instance `i`'s flags become the value.
    Flags(usize, u8),
    /// A state change of copy and state.
    Entered(u16, u16),
}

impl ScriptWire {
    #[doc(hidden)]
    pub fn edit_for_test(&mut self, e: ScriptWireEdit) {
        match e {
            ScriptWireEdit::MemoryByte(i, v) => {
                if let Some(b) = self.memory.get_mut(i) {
                    *b = v;
                }
            }
            ScriptWireEdit::MemoryGrow(n, v) => {
                self.memory.extend(core::iter::repeat_n(v, n));
                if let Some(last) = self.memory_at.last_mut() {
                    *last = self.memory.len() as u32;
                }
            }
            ScriptWireEdit::TimerFunc(i, f) => {
                if let Some(t) = self.timers.get_mut(i) {
                    t.func = f;
                }
            }
            ScriptWireEdit::Errors(i, v) => {
                if let Some(x) = self.errors.get_mut(i) {
                    *x = v;
                }
            }
            ScriptWireEdit::Flags(i, v) => {
                if let Some(x) = self.flags.get_mut(i) {
                    *x = v;
                }
            }
            ScriptWireEdit::Entered(c, s) => self.entered.push((c, s)),
        }
    }

    /// How many timers and memory bytes it holds, for tests.
    #[doc(hidden)]
    pub fn sizes_for_test(&self) -> (usize, usize) {
        (self.timers.len(), self.memory.len())
    }
}

/// The tick's script phase as the [`Director`] sees it: the world's live
/// copies, worms and clocks to read, the instances' state to read and
/// write, the commands to apply, and the operations to charge.
pub struct LiveCtx<'a> {
    live: &'a mut LiveWorld,
    events: &'a mut TickEvents,
    worms: &'a [Worm],
    time_ms: i64,
    frame: u64,
    game_over: bool,
    commands: usize,
    tick_ops: u64,
}

impl<'a> LiveCtx<'a> {
    /// The phase of `live` (with a script part) at the end of a tick:
    /// handlers will see the tick's events so far and the last phase's
    /// state changes.
    fn new(
        live: &'a mut LiveWorld,
        events: &'a mut TickEvents,
        worms: &'a [Worm],
        (time_ms, frame, game_over): (i64, u64, bool),
    ) -> Self {
        let s = live.script.as_mut().expect("a script part");
        let p = &mut s.phase.0;
        p.events.clear();
        p.events.extend(s.entered.iter().map(|&(copy, state)| ScriptEvent::Entered { copy, state }));
        p.events.extend(events.iter().filter_map(|e| ScriptEvent::of(&e.kind)));
        p.entered.clear();
        let mut ctx = LiveCtx { live, events, worms, time_ms, frame, game_over, commands: 0, tick_ops: 0 };
        ctx.rebuild_reads();
        ctx
    }

    /// Makes what a run reads anew from the world (when a run lost it).
    pub fn rebuild_reads(&mut self) {
        let (worms, time_ms, frame, game_over) = (self.worms, self.time_ms, self.frame, self.game_over);
        let w = self.live.parts_mut();
        let r = &mut w.script.as_mut().expect("a script part").phase.0.reads;
        r.states.clear();
        r.states.extend_from_slice(w.states);
        r.values.clear();
        r.values.extend_from_slice(w.values);
        r.motions.clear();
        r.motions.extend_from_slice(w.motions);
        r.offsets.clear();
        r.offsets.extend_from_slice(w.offsets);
        r.worms.clear();
        r.worms.extend(worms.iter().map(|w| (w.alive, w.pos.x as i64, w.pos.y as i64)));
        (r.time_ms, r.frame, r.game_over) = (time_ms, frame, game_over);
    }

    /// The server frame: what timers count in.
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// The level, shared (for a run's API object).
    pub fn level_shared(&self) -> Arc<LiveLevel> {
        self.live.level.clone()
    }

    fn script(&self) -> &ScriptWorld {
        self.live.script.as_ref().expect("a script part")
    }

    fn script_mut(&mut self) -> &mut ScriptWorld {
        self.live.script.as_mut().expect("a script part")
    }

    /// The level: copies, their states and props by name, the scripts.
    pub fn level(&self) -> &LiveLevel {
        &self.live.level
    }

    /// The live copies as they are now.
    pub fn view(&self) -> LiveView<'_> {
        LiveView::new(self.live)
    }

    /// The instances in run order.
    pub fn instances(&self) -> &[LiveInstance] {
        &self.script().instances
    }

    /// The events handlers see this tick, in order: the last script
    /// phase's state changes, then this tick's events before its script
    /// phase (setup calls' first).
    pub fn events(&self) -> &[ScriptEvent] {
        &self.script().phase.0.events
    }

    /// What a run reads, up to date (the commands applied so far
    /// included); give it back with [`LiveCtx::give_reads`] before the
    /// next command is applied.
    pub fn take_reads(&mut self) -> LiveReads {
        core::mem::take(&mut self.script_mut().phase.0.reads)
    }

    /// Gives back what [`LiveCtx::take_reads`] gave.
    pub fn give_reads(&mut self, reads: LiveReads) {
        self.script_mut().phase.0.reads = reads;
    }

    /// Instance `i`'s memory bytes ([`memory_entries`]).
    pub fn memory(&self, i: u16) -> &[u8] {
        self.script().memory.get(usize::from(i))
    }

    /// Sets instance `i`'s memory; refused (false) unless [`memory_ok`].
    pub fn set_memory(&mut self, i: u16, bytes: &[u8]) -> bool {
        if !memory_ok(bytes) {
            return false;
        }
        self.script_mut().memory.set(usize::from(i), bytes);
        true
    }

    /// Instance `i`'s timers, in the order set.
    pub fn timers(&self, i: u16) -> &[LiveTimer] {
        self.script().timers.get(usize::from(i))
    }

    /// Sets instance `i`'s timers (the first [`MAX_TIMERS`]).
    pub fn set_timers(&mut self, i: u16, timers: &[LiveTimer]) {
        let t = &timers[..timers.len().min(MAX_TIMERS)];
        self.script_mut().timers.set(usize::from(i), t);
    }

    /// Instance `i`'s failed runs so far.
    pub fn errors(&self, i: u16) -> u8 {
        self.script().errors.get(usize::from(i)).copied().unwrap_or(0)
    }

    fn flag(&self, i: u16, f: u8) -> bool {
        self.script().flags.get(usize::from(i)).is_some_and(|&x| x & f != 0)
    }

    fn set_flag(&mut self, i: u16, f: u8) {
        if let Some(x) = self.script_mut().flags.get_mut(usize::from(i)) {
            *x |= f;
        }
    }

    /// Whether instance `i` is switched off (it runs no more).
    pub fn is_off(&self, i: u16) -> bool {
        self.flag(i, OFF)
    }

    /// Whether instance `i` has had its first turn (`start`).
    pub fn started(&self, i: u16) -> bool {
        self.flag(i, STARTED)
    }

    /// Marks instance `i` started.
    pub fn set_started(&mut self, i: u16) {
        self.set_flag(i, STARTED);
    }

    /// The scripts' generator.
    pub fn rng(&self) -> ScriptRng {
        self.script().rng
    }

    pub fn set_rng(&mut self, rng: ScriptRng) {
        self.script_mut().rng = rng;
    }

    /// How many commands this tick's runs have applied.
    pub fn commands(&self) -> usize {
        self.commands
    }

    /// Applies `c` now; refused (false, nothing changed) past
    /// [`MAX_LIVE_COMMANDS`] a tick, or for an index the level lacks. A
    /// state change emits [`EventKind::LiveState`] and reaches handlers in
    /// the next tick.
    pub fn apply(&mut self, c: LiveCommand) -> bool {
        if self.commands >= MAX_LIVE_COMMANDS {
            return false;
        }
        let ok = match c {
            LiveCommand::SetState { copy, state } => {
                let ok = self.live.set_state(copy, state, self.events).is_ok();
                if ok {
                    self.script_mut().phase.0.entered.push((copy, state));
                }
                ok
            }
            LiveCommand::SetValue { copy, prop, value } => self.live.set_value(copy, prop, value).is_ok(),
            LiveCommand::SwitchOff { instance } => {
                let ok = usize::from(instance) < self.instances().len();
                if ok {
                    self.set_flag(instance, OFF);
                }
                ok
            }
            LiveCommand::Move { copy, prop, target, speed } => {
                self.live.move_value(copy, prop, target, speed).is_ok()
            }
            LiveCommand::Stop { copy, prop } => self.live.stop_value(copy, prop).is_ok(),
        };
        if ok {
            self.commands += 1;
            if let LiveCommand::SetState { copy, .. }
            | LiveCommand::SetValue { copy, .. }
            | LiveCommand::Move { copy, .. }
            | LiveCommand::Stop { copy, .. } = c
            {
                let w = self.live.parts_mut();
                let mut reads = core::mem::take(&mut w.script.as_mut().expect("a script part").phase.0.reads);
                reads.sync_copy(&w, copy);
                w.script.as_mut().expect("a script part").phase.0.reads = reads;
            }
        }
        ok
    }

    /// Instance `i`'s run failed: its error count goes up, an
    /// [`EventKind::ScriptError`] is emitted, and at
    /// [`MAX_SCRIPT_ERRORS`] it is switched off.
    pub fn fail(&mut self, i: u16) {
        let Some(e) = self.script_mut().errors.get_mut(usize::from(i)) else { return };
        *e = e.saturating_add(1).min(MAX_SCRIPT_ERRORS);
        let off = *e >= MAX_SCRIPT_ERRORS;
        self.events.push(EventKind::ScriptError { instance: i });
        if off {
            self.set_flag(i, OFF);
        }
    }

    /// A run used `ops` operations: counted for the tick and the peaks.
    pub fn charge(&mut self, ops: u64) {
        self.tick_ops = self.tick_ops.saturating_add(ops);
        let p = &mut self.script_mut().phase.0.peak;
        p.run = p.run.max(ops);
    }

    /// The operations this tick's runs have used.
    pub fn tick_ops(&self) -> u64 {
        self.tick_ops
    }

    /// The end of the phase: its state changes wait for the next tick.
    fn finish(self) {
        let tick = self.tick_ops;
        let s = self.live.script.as_mut().expect("a script part");
        let ScriptWorld { entered, phase, .. } = s;
        entered.clear();
        entered.extend_from_slice(&phase.0.entered);
        phase.0.peak.tick = phase.0.peak.tick.max(tick);
    }
}

impl World {
    /// The scripts' phase of a frame (issue #88): after the mode's
    /// `on_tick` and its commands, before the frame clock advances; none
    /// in a frozen world or one without a director.
    pub(crate) fn script_phase(&mut self, frozen: bool) {
        if frozen {
            return;
        }
        let Some(live) = &mut self.live else { return };
        let Some(script) = &mut live.script else { return };
        let Some(director) = script.director.0.take() else { return };
        let time_ms = i64::try_from(self.time.0.saturating_sub(live.start.0)).unwrap_or(i64::MAX);
        let over = self.referee.over.is_some();
        let mut ctx =
            LiveCtx::new(live, &mut self.draws.events, &self.worms, (time_ms, self.server_frame, over));
        director.run(&mut ctx);
        ctx.finish();
        if let Some(s) = &mut live.script {
            s.director.0 = Some(director);
        }
    }

    /// The most operations any one script run and any one tick's runs
    /// used since the world was made (zero without scripts), for headroom
    /// tests. Not state, like [`World::actions_triggered`]: two worlds that
    /// differ only here are equal and hash alike.
    pub fn script_peak(&self) -> ScriptPeak {
        self.live.as_ref().and_then(|l| l.script.as_ref()).map_or(ScriptPeak::default(), |s| s.peak())
    }
}
