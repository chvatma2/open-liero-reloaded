//! The scripts of a level compiled, and their tick (issue #88): the
//! [`olr_sim::Director`] the world runs once a tick.
//!
//! Each run is one call of a script's function through a wrapper
//! (`__olr_run0` to `__olr_run3`, merged into every script) that calls it
//! with `this` bound and ends in `this.__done()`, a native function that
//! stores the run's operation count (Rhai's runtime state, restarted at
//! each call) in the run's API object. A run that never reaches it failed.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt;

use olr_sim::{
    Director, LiveCtx, LiveLevel, LiveTimer, MAX_TIMERS, MemValue, ScriptEvent, memory_entries, push_memory,
};
use rhai::packages::{
    ArithmeticPackage, BasicArrayPackage, BasicIteratorPackage, BasicMapPackage, BasicStringPackage,
    LogicPackage, Package,
};
use rhai::{AST, CallFnOptions, Dynamic, Engine, FnPtr, ImmutableString, Map, Scope};

use crate::api::{Live, register};
use crate::{MAX_RUN_OPERATIONS, MAX_TICK_OPERATIONS, ScriptIssue, compile_text, engine};

/// The wrappers each run goes through (see the module's doc), by the
/// called function's arity.
const WRAPPERS: &str = "\
fn __olr_run0(f) { this.call(f); this.__done(); }
fn __olr_run1(f, a) { this.call(f, a); this.__done(); }
fn __olr_run2(f, a, b) { this.call(f, a, b); this.__done(); }
fn __olr_run3(f, a, b, c) { this.call(f, a, b, c); this.__done(); }
";
const WRAPPER_NAMES: [&str; 4] = ["__olr_run0", "__olr_run1", "__olr_run2", "__olr_run3"];

/// The handlers a script may define, by name and arity.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Handler {
    Start,
    Tick,
    Entered,
    Spawned,
    Killed,
    Exploded,
    Damaged,
    Arrived,
}

const HANDLERS: [(Handler, &str, usize); 8] = [
    (Handler::Start, "start", 0),
    (Handler::Tick, "tick", 0),
    (Handler::Entered, "entered", 1),
    (Handler::Spawned, "spawned", 1),
    (Handler::Killed, "killed", 2),
    (Handler::Exploded, "exploded", 3),
    (Handler::Damaged, "damaged", 3),
    (Handler::Arrived, "arrived", 1),
];

/// One compiled script.
struct Compiled {
    /// The script with the wrappers merged in.
    ast: AST,
    /// Its functions of no parameter, sorted: what a timer names (by
    /// index).
    timer_fns: Arc<[ImmutableString]>,
    /// Which [`Handler`]s it defines, a bit each.
    handlers: u8,
}

impl Compiled {
    fn has(&self, h: Handler) -> bool {
        self.handlers & (1 << h as u8) != 0
    }
}

struct Inner {
    engine: Engine,
    scripts: Vec<Compiled>,
}

/// A level's scripts, compiled ([`Scripts::compile`]): the engine and the
/// ASTs behind an `Arc`, shared by every world of the match. Stateless:
/// the engine's native functions write only the run's own API object.
#[derive(Clone)]
pub struct Scripts {
    inner: Arc<Inner>,
}

impl fmt::Debug for Scripts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Scripts({})", self.inner.scripts.len())
    }
}

/// Why a level's scripts were refused: script `script` (an index into
/// [`LiveLevel::scripts`]) does not compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    pub script: u16,
    pub issue: ScriptIssue,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "script {}: {}", self.script, self.issue)
    }
}

/// The runtime's engine: [`engine`]'s with integer arithmetic, logic,
/// strings, ranges, arrays and maps (no `exit`, no time, no floats) and
/// the API ([`crate::api`]).
fn runtime_engine() -> Engine {
    let mut e = engine();
    for m in [
        ArithmeticPackage::new().as_shared_module(),
        LogicPackage::new().as_shared_module(),
        BasicStringPackage::new().as_shared_module(),
        BasicIteratorPackage::new().as_shared_module(),
        BasicArrayPackage::new().as_shared_module(),
        BasicMapPackage::new().as_shared_module(),
    ] {
        e.register_global_module(m);
    }
    register(&mut e);
    e
}

impl Scripts {
    /// Every script of `level` compiled; the first that does not compile
    /// refuses them all (every peer refuses the same bytes alike).
    pub fn compile(level: &LiveLevel) -> Result<Scripts, CompileError> {
        let engine = runtime_engine();
        let wrappers = engine.compile(WRAPPERS).expect("the wrappers compile");
        let mut scripts = Vec::with_capacity(level.scripts.len());
        for (k, text) in level.scripts.iter().enumerate() {
            let script = u16::try_from(k).unwrap_or(u16::MAX);
            let ast = compile_text(&engine, text).map_err(|issue| CompileError { script, issue })?;
            // Rhai lists functions in hash order: sorted here.
            let mut fns: Vec<(String, usize)> = ast
                .iter_functions()
                .filter(|f| !WRAPPER_NAMES.contains(&f.name))
                .map(|f| (String::from(f.name), f.params.len()))
                .collect();
            fns.sort();
            let mut handlers = 0u8;
            for (h, name, arity) in HANDLERS {
                if fns.iter().any(|(n, a)| n == name && *a == arity) {
                    handlers |= 1 << h as u8;
                }
            }
            let timer_fns: Arc<[ImmutableString]> =
                fns.iter().filter(|(_, a)| *a == 0).map(|(n, _)| ImmutableString::from(n.as_str())).collect();
            scripts.push(Compiled { ast: ast.merge(&wrappers), timer_fns, handlers });
        }
        Ok(Scripts { inner: Arc::new(Inner { engine, scripts }) })
    }

    /// The director, boxed for [`olr_sim::MatchSetup::director`].
    pub fn boxed(self) -> Box<dyn Director> {
        Box::new(self)
    }
}

/// The director of `level`'s scripts: `None` when no copy and no level
/// script runs one (every script is compiled all the same, so a broken
/// one refuses the level).
pub fn director(level: &LiveLevel) -> Result<Option<Box<dyn Director>>, CompileError> {
    let scripts = Scripts::compile(level)?;
    let runs = level.level_script.is_some() || level.copies.iter().any(|c| !c.scripts.is_empty());
    Ok(runs.then(|| scripts.boxed()))
}

impl Director for Scripts {
    fn box_clone(&self) -> Box<dyn Director> {
        Box::new(self.clone())
    }

    fn timer_functions(&self, script: u16) -> u16 {
        self.inner
            .scripts
            .get(usize::from(script))
            .map_or(0, |c| u16::try_from(c.timer_fns.len()).unwrap_or(u16::MAX))
    }

    fn run(&self, ctx: &mut LiveCtx<'_>) {
        tick(&self.inner, ctx);
    }
}

/// Whether another run may start: the tick has a run's limit left.
fn room(ctx: &LiveCtx<'_>) -> bool {
    ctx.tick_ops().saturating_add(MAX_RUN_OPERATIONS) <= MAX_TICK_OPERATIONS
}

/// The script phase: the level script's instance, then each copy's in
/// level and run order; in each, `start` (its first turn), its due timers
/// in the order set, its handlers for the tick's events in order, then
/// `tick`. Once the budget leaves less than a run's limit, every
/// remaining run is skipped (timers stay due: they fire next tick).
fn tick(inner: &Inner, ctx: &mut LiveCtx<'_>) {
    let n = ctx.instances().len();
    for i in 0..n {
        let Ok(i) = u16::try_from(i) else { return };
        let inst = ctx.instances()[usize::from(i)];
        let Some(c) = inner.scripts.get(usize::from(inst.script)) else { continue };
        if ctx.is_off(i) {
            continue;
        }
        if !room(ctx) {
            return;
        }
        if !ctx.started(i) {
            ctx.set_started(i);
            if c.has(Handler::Start) {
                call(inner, ctx, i, c, "start", Vec::new());
            }
        }
        // Due timers, in the order set; each advances before it runs.
        let frame = ctx.frame();
        let mut due = [(0u16, 0u64); MAX_TIMERS];
        let mut k = 0;
        for t in ctx.timers(i).iter().filter(|t| t.due <= frame) {
            if k < due.len() {
                due[k] = (t.func, t.due);
                k += 1;
            }
        }
        for &(func, at) in &due[..k] {
            if ctx.is_off(i) {
                break;
            }
            let Some(pos) = ctx.timers(i).iter().position(|t| t.func == func && t.due == at) else {
                continue;
            };
            if !room(ctx) {
                return;
            }
            let mut timers: Vec<LiveTimer> = ctx.timers(i).to_vec();
            if timers[pos].period == 0 {
                timers.remove(pos);
            } else {
                timers[pos].due = frame.saturating_add(timers[pos].period);
            }
            ctx.set_timers(i, &timers);
            let Some(name) = c.timer_fns.get(usize::from(func)) else { continue };
            call(inner, ctx, i, c, name, Vec::new());
        }
        // Handlers of the tick's events.
        for e in 0..ctx.events().len() {
            if ctx.is_off(i) {
                break;
            }
            let ev = ctx.events()[e];
            let int = |v: i64| Dynamic::from_int(v);
            let (name, args) = match ev {
                ScriptEvent::Entered { copy, state }
                    if inst.copy == Some(copy) && c.has(Handler::Entered) =>
                {
                    let name = ctx
                        .level()
                        .copies
                        .get(usize::from(copy))
                        .and_then(|c| c.states.get(usize::from(state)))
                        .map_or("", |s| s.name.as_str());
                    ("entered", alloc::vec![Dynamic::from(ImmutableString::from(name))])
                }
                ScriptEvent::Arrived { copy, prop } if inst.copy == Some(copy) && c.has(Handler::Arrived) => {
                    let name = ctx
                        .level()
                        .copies
                        .get(usize::from(copy))
                        .and_then(|c| c.props.get(usize::from(prop)))
                        .and_then(|p| p.name.as_deref())
                        .unwrap_or("");
                    ("arrived", alloc::vec![Dynamic::from(ImmutableString::from(name))])
                }
                ScriptEvent::Spawned { worm } if c.has(Handler::Spawned) => {
                    ("spawned", alloc::vec![int(worm.into())])
                }
                ScriptEvent::Killed { victim, killer } if c.has(Handler::Killed) => {
                    ("killed", alloc::vec![int(victim.into()), int(killer.into())])
                }
                ScriptEvent::Exploded { x, y, damage } if c.has(Handler::Exploded) => {
                    ("exploded", alloc::vec![int(x), int(y), int(damage)])
                }
                ScriptEvent::Damaged { worm, amount, by } if c.has(Handler::Damaged) => {
                    ("damaged", alloc::vec![int(worm.into()), int(amount), int(by.into())])
                }
                _ => continue,
            };
            if !room(ctx) {
                return;
            }
            call(inner, ctx, i, c, name, args);
        }
        if c.has(Handler::Tick) && !ctx.is_off(i) {
            if !room(ctx) {
                return;
            }
            call(inner, ctx, i, c, "tick", Vec::new());
        }
    }
}

/// Instance `i`'s memory as a map.
fn memory_map(bytes: &[u8]) -> Map {
    memory_entries(bytes)
        .map(|(k, v)| {
            let v = match v {
                MemValue::Int(x) => Dynamic::from_int(x),
                MemValue::Bool(b) => Dynamic::from_bool(b),
                MemValue::Str(s) => Dynamic::from(ImmutableString::from(s)),
            };
            (k.into(), v)
        })
        .collect()
}

/// `map` as memory bytes (keys sorted), or `None` past the bounds.
fn memory_bytes(map: &Map) -> Option<Vec<u8>> {
    let mut entries: Vec<(&str, &Dynamic)> = map.iter().map(|(k, v)| (k.as_str(), v)).collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut out = Vec::new();
    for (k, v) in entries {
        let pushed = if let Ok(x) = v.as_int() {
            push_memory(&mut out, k, MemValue::Int(x))
        } else if let Ok(b) = v.as_bool() {
            push_memory(&mut out, k, MemValue::Bool(b))
        } else {
            let s = v.read_lock::<ImmutableString>()?;
            push_memory(&mut out, k, MemValue::Str(s.as_str()))
        };
        if !pushed {
            return None;
        }
    }
    Some(out)
}

/// One run: function `name` of instance `i`'s script with `args`, `this`
/// the run's API object. Its commands, memory, timers and draws apply only
/// when it succeeds; a failed run is charged its limit and counted.
fn call(inner: &Inner, ctx: &mut LiveCtx<'_>, i: u16, c: &Compiled, name: &str, args: Vec<Dynamic>) {
    let inst = ctx.instances()[usize::from(i)];
    let first = ctx.instances().iter().position(|x| x.copy.is_some() && x.copy == inst.copy).unwrap_or(0);
    let live = Live {
        dead: false,
        level: ctx.level_shared(),
        copy: inst.copy,
        instance: i,
        first_of_copy: u16::try_from(first).unwrap_or(i),
        reads: ctx.take_reads(),
        mem: memory_map(ctx.memory(i)),
        mem_dirty: false,
        timers: ctx.timers(i).to_vec(),
        timers_dirty: false,
        timer_fns: c.timer_fns.clone(),
        rng: ctx.rng(),
        commands: Vec::new(),
        commands_before: ctx.commands(),
        done: false,
        ops: 0,
    };
    let mut this = Dynamic::from(live);
    let Ok(f) = FnPtr::new(name) else {
        ctx.rebuild_reads();
        return;
    };
    let mut all = Vec::with_capacity(args.len() + 1);
    all.push(Dynamic::from(f));
    let wrapper = WRAPPER_NAMES[args.len().min(3)];
    all.extend(args);
    let options = CallFnOptions::new().eval_ast(false).rewind_scope(true).bind_this_ptr(&mut this);
    let result =
        inner.engine.call_fn_with_options::<Dynamic>(options, &mut Scope::new(), &c.ast, wrapper, all);
    let Some(live) = this.try_cast::<Live>() else {
        // The script replaced `this`: what it read is lost.
        ctx.rebuild_reads();
        ctx.charge(MAX_RUN_OPERATIONS);
        ctx.fail(i);
        return;
    };
    let Live { dead, reads, mem, mem_dirty, timers, timers_dirty, rng, commands, done, ops, .. } = live;
    if dead {
        ctx.rebuild_reads();
    } else {
        ctx.give_reads(reads);
    }
    let memory = if mem_dirty { memory_bytes(&mem) } else { None };
    if result.is_err() || dead || !done || (mem_dirty && memory.is_none()) {
        ctx.charge(MAX_RUN_OPERATIONS);
        ctx.fail(i);
        return;
    }
    ctx.charge(ops.min(MAX_RUN_OPERATIONS));
    if let Some(m) = memory
        && !ctx.set_memory(i, &m)
    {
        ctx.fail(i);
        return;
    }
    if timers_dirty {
        ctx.set_timers(i, &timers);
    }
    ctx.set_rng(rng);
    for cmd in commands {
        let _: bool = ctx.apply(cmd);
    }
}
