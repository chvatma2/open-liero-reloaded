//! The per-run API object (issue #88): `this` in every handler and timer
//! call, a fresh [`Live`] each run. It holds the run's reads of the world
//! (copies' states and values, worms, the clocks), its memory, timers and
//! draws, its command queue and its operation count; nothing else
//! outlives a run. Copy handles are integer indices into the level's
//! copies.
//!
//! A copy of `this` (`let me = this;`) is dead: every method on it is a
//! script error, so no run copies the world's reads.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use olr_sim::{
    LIVE_SUBPIXELS, LiveCommand, LiveCopy, LiveLevel, LiveReads, LiveTimer, MAX_LIVE_COMMANDS,
    MAX_MEMORY_BYTES, MAX_MEMORY_KEYS, MAX_TIMERS, ScriptRng,
};
use rhai::{Dynamic, Engine, EvalAltResult, INT, ImmutableString, Map, NativeCallContext};

type Res<T> = Result<T, Box<EvalAltResult>>;

/// The run's API object.
pub(crate) struct Live {
    /// A copy of `this`: every method fails.
    pub dead: bool,
    pub level: Arc<LiveLevel>,
    /// The copy whose script runs (none: the level script).
    pub copy: Option<u16>,
    /// The running instance, and the first of its copy's.
    pub instance: u16,
    pub first_of_copy: u16,
    pub reads: LiveReads,
    pub mem: Map,
    pub mem_dirty: bool,
    pub timers: Vec<LiveTimer>,
    pub timers_dirty: bool,
    /// The script's functions a timer may name, sorted.
    pub timer_fns: Arc<[ImmutableString]>,
    pub rng: ScriptRng,
    pub commands: Vec<LiveCommand>,
    /// Commands earlier runs of this tick applied.
    pub commands_before: usize,
    /// `__done` ran: `ops` is the run's count.
    pub done: bool,
    pub ops: u64,
}

impl Clone for Live {
    /// A dead copy (see the module's doc).
    fn clone(&self) -> Self {
        Live {
            dead: true,
            level: self.level.clone(),
            copy: None,
            instance: 0,
            first_of_copy: 0,
            reads: LiveReads::default(),
            mem: Map::new(),
            mem_dirty: false,
            timers: Vec::new(),
            timers_dirty: false,
            timer_fns: self.timer_fns.clone(),
            rng: ScriptRng::new(0),
            commands: Vec::new(),
            commands_before: 0,
            done: false,
            ops: 0,
        }
    }
}

fn err<T>(msg: impl Into<String>) -> Res<T> {
    Err(msg.into().into())
}

impl Live {
    fn live(&mut self) -> Res<&mut Live> {
        if self.dead { err("a copy of `this` cannot be used") } else { Ok(self) }
    }

    /// This copy's handle (a script error in the level script).
    fn me(&mut self) -> Res<u16> {
        self.live()?.copy.map_or_else(|| err("the level script has no copy of its own"), Ok)
    }

    /// Copy handle `c`.
    fn handle(&mut self, c: INT) -> Res<u16> {
        let n = self.live()?.level.copies.len();
        match usize::try_from(c) {
            Ok(i) if i < n => Ok(i as u16),
            _ => err(format!("no copy {c} ({n} copies)")),
        }
    }

    fn setup(&self, c: u16) -> &LiveCopy {
        &self.level.copies[usize::from(c)]
    }

    /// Copy `c`'s exposed value `name`: its index.
    fn prop(&self, c: u16, name: &str) -> Res<u16> {
        let copy = self.setup(c);
        copy.prop(name).map_or_else(|| err(format!("copy {} has no value {name:?}", copy.id)), Ok)
    }

    fn queue(&mut self, cmd: LiveCommand) -> Res<()> {
        if self.commands_before + self.commands.len() >= MAX_LIVE_COMMANDS {
            return err(format!("more than {MAX_LIVE_COMMANDS} commands in a tick"));
        }
        self.commands.push(cmd);
        Ok(())
    }

    fn id_of(&mut self, c: u16) -> ImmutableString {
        self.setup(c).id.as_str().into()
    }

    fn state_of(&mut self, c: u16) -> ImmutableString {
        let s = self.reads.state(c);
        self.setup(c).states.get(usize::from(s)).map_or("", |s| s.name.as_str()).into()
    }

    fn set_state_of(&mut self, c: u16, name: &str) -> Res<()> {
        let copy = self.setup(c);
        let Some(state) = copy.state(name) else {
            return err(format!("copy {} has no state {name:?}", copy.id));
        };
        self.queue(LiveCommand::SetState { copy: c, state })
    }

    fn get_of(&mut self, c: u16, name: &str) -> Res<INT> {
        let p = self.prop(c, name)?;
        let v = self.reads.values(c).get(usize::from(p)).copied().unwrap_or(0);
        let position = self.setup(c).props[usize::from(p)].target.is_position();
        Ok(if position { v.div_euclid(LIVE_SUBPIXELS) } else { v })
    }

    fn set_of(&mut self, c: u16, name: &str, v: INT) -> Res<()> {
        let prop = self.prop(c, name)?;
        let position = self.setup(c).props[usize::from(prop)].target.is_position();
        let value = if position { v.saturating_mul(LIVE_SUBPIXELS) } else { v };
        self.queue(LiveCommand::SetValue { copy: c, prop, value })
    }

    fn has_of(&mut self, c: u16, name: &str) -> bool {
        self.setup(c).prop(name).is_some()
    }

    /// The timer function `name`: its index.
    fn timer_fn(&self, name: &str) -> Res<u16> {
        match self.timer_fns.binary_search_by(|f| f.as_str().cmp(name)) {
            Ok(i) => Ok(i as u16),
            Err(_) => err(format!("no function {name}() in this script for a timer")),
        }
    }

    /// A timer of `name` in `ticks` (at least 1), then every `period`
    /// (0: once), in place of one of the same function.
    fn timer(&mut self, ticks: INT, name: &str, every: bool) -> Res<()> {
        self.live()?;
        let Some(ticks) = u64::try_from(ticks).ok().filter(|&t| t >= 1) else {
            return err(format!("a timer of {ticks} ticks: at least 1"));
        };
        let func = self.timer_fn(name)?;
        self.timers.retain(|t| t.func != func);
        if self.timers.len() >= MAX_TIMERS {
            return err(format!("more than {MAX_TIMERS} timers"));
        }
        let due = self.reads.frame.saturating_add(ticks);
        self.timers.push(LiveTimer { func, due, period: if every { ticks } else { 0 } });
        self.timers_dirty = true;
        Ok(())
    }

    fn worm(&mut self, i: INT) -> Res<(bool, i64, i64)> {
        let w = self.live()?.reads.worms();
        match usize::try_from(i).ok().and_then(|i| w.get(i)) {
            Some(&w) => Ok(w),
            None => err(format!("no worm {i} ({} worms)", w.len())),
        }
    }
}

/// Whether `v` is something memory keeps: an integer, a boolean or a
/// string of at most [`MAX_MEMORY_BYTES`].
fn memory_value_ok(v: &Dynamic) -> bool {
    v.is_int() || v.is_bool() || v.read_lock::<ImmutableString>().is_some_and(|s| s.len() <= MAX_MEMORY_BYTES)
}

/// The API on `e` (the module's doc and the crate's `CLAUDE.md` list it).
pub(crate) fn register(e: &mut Engine) {
    e.register_type_with_name::<Live>("Live");
    e.register_fn("__done", |ctx: NativeCallContext, l: &mut Live| {
        l.ops = ctx.global_runtime_state().num_operations();
        l.done = true;
    });
    // This copy, other copies.
    e.register_fn("me", |l: &mut Live| -> Res<INT> { Ok(l.live()?.copy.map_or(-1, INT::from)) });
    e.register_fn("id", |l: &mut Live| -> Res<ImmutableString> {
        let c = l.me()?;
        Ok(l.id_of(c))
    });
    e.register_fn("id", |l: &mut Live, c: INT| -> Res<ImmutableString> {
        let c = l.handle(c)?;
        Ok(l.id_of(c))
    });
    e.register_fn("copy", |l: &mut Live, id: &str| -> Res<INT> {
        match l.live()?.level.copy(id) {
            Some(c) => Ok(INT::from(c)),
            None => err(format!("no live copy {id:?}")),
        }
    });
    e.register_fn("state", |l: &mut Live| -> Res<ImmutableString> {
        let c = l.me()?;
        Ok(l.state_of(c))
    });
    e.register_fn("state", |l: &mut Live, c: INT| -> Res<ImmutableString> {
        let c = l.handle(c)?;
        Ok(l.state_of(c))
    });
    e.register_fn("set_state", |l: &mut Live, name: &str| -> Res<()> {
        let c = l.me()?;
        l.set_state_of(c, name)
    });
    e.register_fn("set_state", |l: &mut Live, c: INT, name: &str| -> Res<()> {
        let c = l.handle(c)?;
        l.set_state_of(c, name)
    });
    e.register_fn("get", |l: &mut Live, name: &str| -> Res<INT> {
        let c = l.me()?;
        l.get_of(c, name)
    });
    e.register_fn("get", |l: &mut Live, c: INT, name: &str| -> Res<INT> {
        let c = l.handle(c)?;
        l.get_of(c, name)
    });
    e.register_fn("set", |l: &mut Live, name: &str, v: INT| -> Res<()> {
        let c = l.me()?;
        l.set_of(c, name, v)
    });
    e.register_fn("set", |l: &mut Live, c: INT, name: &str, v: INT| -> Res<()> {
        let c = l.handle(c)?;
        l.set_of(c, name, v)
    });
    e.register_fn("has", |l: &mut Live, name: &str| -> Res<bool> {
        let c = l.me()?;
        Ok(l.has_of(c, name))
    });
    e.register_fn("has", |l: &mut Live, c: INT, name: &str| -> Res<bool> {
        let c = l.handle(c)?;
        Ok(l.has_of(c, name))
    });
    e.register_fn("switch_off_default", |l: &mut Live| -> Res<()> {
        l.me()?;
        for instance in l.first_of_copy..l.instance {
            l.queue(LiveCommand::SwitchOff { instance })?;
        }
        Ok(())
    });
    // Timers.
    e.register_fn("after", |l: &mut Live, ticks: INT, name: &str| l.timer(ticks, name, false));
    e.register_fn("every", |l: &mut Live, ticks: INT, name: &str| l.timer(ticks, name, true));
    e.register_fn("cancel", |l: &mut Live, name: &str| -> Res<()> {
        let func = l.live()?.timer_fn(name)?;
        let n = l.timers.len();
        l.timers.retain(|t| t.func != func);
        l.timers_dirty |= l.timers.len() != n;
        Ok(())
    });
    // Randomness: the scripts' own generator.
    e.register_fn("rand", |l: &mut Live, n: INT| -> Res<INT> {
        let l = l.live()?;
        match u64::try_from(n) {
            Ok(n) if n >= 1 => Ok(l.rng.below(n) as INT),
            _ => err(format!("rand({n}): at least 1")),
        }
    });
    e.register_fn("chance", |l: &mut Live, percent: INT| -> Res<bool> {
        let l = l.live()?;
        Ok((l.rng.below(100) as INT) < percent)
    });
    // Reading the match.
    e.register_fn("time", |l: &mut Live| -> Res<INT> { Ok(l.live()?.reads.time_ms) });
    e.register_fn("game_over", |l: &mut Live| -> Res<bool> { Ok(l.live()?.reads.game_over) });
    e.register_fn("worm_count", |l: &mut Live| -> Res<INT> { Ok(l.live()?.reads.worms().len() as INT) });
    e.register_fn("worm_alive", |l: &mut Live, i: INT| -> Res<bool> { Ok(l.worm(i)?.0) });
    e.register_fn("worm_x", |l: &mut Live, i: INT| -> Res<INT> { Ok(l.worm(i)?.1) });
    e.register_fn("worm_y", |l: &mut Live, i: INT| -> Res<INT> { Ok(l.worm(i)?.2) });
    // Memory.
    e.register_get_set(
        "mem",
        |l: &mut Live| -> Res<Map> { Ok(l.live()?.mem.clone()) },
        |l: &mut Live, m: Map| -> Res<()> {
            let l = l.live()?;
            if m.len() > MAX_MEMORY_KEYS {
                return err(format!("memory of {} keys: at most {MAX_MEMORY_KEYS}", m.len()));
            }
            if let Some((k, _)) = m.iter().find(|(k, v)| k.len() > MAX_MEMORY_BYTES || !memory_value_ok(v)) {
                return err(format!(
                    "memory key {k:?}: keys and strings of at most {MAX_MEMORY_BYTES} bytes, values integers, booleans or strings"
                ));
            }
            l.mem = m;
            l.mem_dirty = true;
            Ok(())
        },
    );
}
