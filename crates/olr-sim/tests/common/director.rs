//! Issue #88: a director made here, in place of `olr-script`'s Rhai
//! runtime: every instance keeps a counter in its memory, draws from the
//! scripts' generator, sets timers, changes its copy's state and values
//! now and then, and fails now and then, all through `LiveCtx`.
#![allow(dead_code)]

use std::sync::Arc;

use olr_sim::{
    Director, LiveCommand, LiveCtx, LiveLevel, LiveTimer, MemValue, ScriptEvent, memory_entries, push_memory,
};

#[derive(Clone, Debug)]
pub struct TestDirector;

/// Its timer functions per script.
pub const FUNCS: u16 = 2;

impl Director for TestDirector {
    fn box_clone(&self) -> Box<dyn Director> {
        Box::new(TestDirector)
    }

    fn timer_functions(&self, _script: u16) -> u16 {
        FUNCS
    }

    fn run(&self, ctx: &mut LiveCtx<'_>) {
        let frame = ctx.frame();
        for i in 0..ctx.instances().len() as u16 {
            if ctx.is_off(i) {
                continue;
            }
            ctx.set_started(i);
            let copy = ctx.instances()[usize::from(i)].copy.unwrap_or(0);
            let mut timers: Vec<LiveTimer> = ctx.timers(i).to_vec();
            let fired = timers.iter().filter(|t| t.due <= frame).count() as i64;
            timers.retain(|t| t.due > frame || t.period > 0);
            for t in timers.iter_mut().filter(|t| t.due <= frame) {
                t.due = frame + t.period;
            }
            if timers.is_empty() {
                timers.push(LiveTimer {
                    func: i % FUNCS,
                    due: frame + 3 + u64::from(i),
                    period: u64::from(i % 2) * 5,
                });
            }
            ctx.set_timers(i, &timers);
            let seen = ctx
                .events()
                .iter()
                .filter(|e| matches!(e, ScriptEvent::Entered { copy: c, .. } if *c == copy))
                .count() as i64;
            let n = memory_entries(ctx.memory(i))
                .find_map(|(k, v)| match (k, v) {
                    ("n", MemValue::Int(n)) => Some(n),
                    _ => None,
                })
                .unwrap_or(0);
            let mut mem = Vec::new();
            push_memory(&mut mem, "fired", MemValue::Int(fired));
            push_memory(&mut mem, "n", MemValue::Int(n + 1 + seen));
            push_memory(&mut mem, "odd", MemValue::Bool(n % 2 == 1));
            push_memory(&mut mem, "who", MemValue::Str(if i == 0 { "level" } else { "copy" }));
            assert!(ctx.set_memory(i, &mem));
            let mut rng = ctx.rng();
            let r = rng.below(4);
            ctx.set_rng(rng);
            let states = ctx.level().copies.get(usize::from(copy)).map_or(1, |c| c.states.len() as u64);
            if r == 0 {
                let mut rng = ctx.rng();
                let state = rng.below(states) as u16;
                ctx.set_rng(rng);
                ctx.apply(LiveCommand::SetState { copy, state });
            } else if r == 1 {
                ctx.apply(LiveCommand::SetValue { copy, prop: 0, value: n * 3 });
            }
            if frame % 37 == u64::from(i) {
                ctx.fail(i);
            }
            ctx.charge(10 + u64::from(i));
        }
    }
}

/// `live`'s level with a level script and a script on every copy (the
/// texts are never read: this director runs them).
pub fn scripted(live: &LiveLevel) -> Arc<LiveLevel> {
    let mut copies = live.copies.clone();
    for c in &mut copies {
        c.scripts = vec![1];
    }
    Arc::new(LiveLevel::new(copies, vec!["level".into(), "copy".into()], Some(0), live.sounds))
}
