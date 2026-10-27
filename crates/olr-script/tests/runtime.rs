//! The runtime (issue #88): run order, handlers, timers, memory, limits,
//! the scripts' own generator, and no state outside the world.

use olr_script::{MAX_RUN_OPERATIONS, MAX_SCRIPT_ERRORS, MAX_TICK_OPERATIONS, Scripts};
use olr_sim::{OracleMode, World};

use crate::common::{copy, level, play, read, setup, world};

const ABC: &[&str] = &["A", "B", "C"];

#[test]
fn the_console_cycles_its_states_every_300_ticks() {
    // `every` on one console, `after` re-armed on the other.
    let every = "fn start() { this.every(300, \"next\"); }\n\
                 fn next() {\n\
                   let s = this.state();\n\
                   if s == \"A\" { this.set_state(\"B\"); }\n\
                   else if s == \"B\" { this.set_state(\"C\"); }\n\
                   else { this.set_state(\"A\"); }\n\
                 }";
    let after = "fn start() { this.after(300, \"next\"); }\n\
                 fn next() {\n\
                   let s = this.state();\n\
                   this.set_state(if s == \"A\" { \"B\" } else if s == \"B\" { \"C\" } else { \"A\" });\n\
                   this.after(300, \"next\");\n\
                 }";
    let mut w =
        world(level(vec![copy("a", 20, ABC, vec![0]), copy("b", 40, ABC, vec![1])], &[every, after], None));
    let mut seen = Vec::new();
    for t in 1..=905u32 {
        play(&mut w, 1);
        let (a, _) = read(&w, "a");
        let (b, _) = read(&w, "b");
        assert_eq!(a, b, "tick {t}");
        if seen.last().is_none_or(|(_, s)| *s != a) {
            seen.push((t, a));
        }
    }
    // `start` runs in the first tick: the first change is 300 ticks on.
    let want: Vec<(u32, String)> =
        [(1, "A"), (301, "B"), (601, "C"), (901, "A")].iter().map(|&(t, s)| (t, s.to_string())).collect();
    assert_eq!(seen, want);
}

#[test]
fn a_copy_script_runs_after_its_default_and_can_switch_it_off() {
    let default = "fn tick() { this.set(\"v\", 1); }";
    // Its own reads what the default set this tick, then after 5 ticks
    // switches the default off.
    let own = "fn tick() {\n\
                 this.set(\"v\", this.get(\"v\") + 5);\n\
                 if this.time() >= 40 { this.switch_off_default(); }\n\
               }";
    let mut w = world(level(vec![copy("c", 20, ABC, vec![0, 1])], &[default, own], None));
    let mut v = Vec::new();
    for _ in 0..8 {
        assert!(play(&mut w, 1)[0].is_empty());
        v.push(read(&w, "c").1);
    }
    assert_eq!(v, [6, 6, 6, 6, 6, 11, 16, 21]);
    // The level script has no copy to switch.
    let bad = "fn tick() { this.switch_off_default(); }";
    let mut w = world(level(vec![copy("c", 20, ABC, vec![])], &[bad], Some(0)));
    assert_eq!(play(&mut w, 1), [vec![0]]);
}

#[test]
fn the_level_script_drives_two_copies_by_id() {
    let script = "fn tick() {\n\
                    if this.me() != -1 { throw \"me\"; }\n\
                    let a = this.copy(\"console-a\");\n\
                    let b = this.copy(\"console-b\");\n\
                    if this.state(a) == \"A\" && this.time() >= 20 {\n\
                      this.set_state(a, \"C\");\n\
                      this.set_state(b, \"B\");\n\
                      this.set(b, \"x\", this.get(a, \"x\") + 7);\n\
                    }\n\
                    if this.has(a, \"nothing\") || !this.has(b, \"v\") { throw \"has\"; }\n\
                  }";
    let copies = vec![copy("console-a", 20, ABC, vec![]), copy("console-b", 40, ABC, vec![])];
    let mut w = world(level(copies, &[script], Some(0)));
    assert!(play(&mut w, 2).iter().all(Vec::is_empty));
    assert_eq!(read(&w, "console-a").0, "A");
    assert!(play(&mut w, 1)[0].is_empty());
    assert_eq!(read(&w, "console-a").0, "C");
    assert_eq!(read(&w, "console-b").0, "B");
    let b = w.live().unwrap().copy_by_id("console-b").unwrap();
    assert_eq!(b.rect().0, 47, "x moved the copy 7 pixels");
    // An unknown id, and this copy's own calls in the level script, are
    // script errors.
    for bad in
        ["fn tick() { this.copy(\"nobody\"); }", "fn tick() { this.state(); }", "fn tick() { this.id(); }"]
    {
        let mut w = world(level(vec![copy("c", 20, ABC, vec![])], &[bad], Some(0)));
        assert_eq!(play(&mut w, 1), [vec![0]], "{bad}");
    }
}

#[test]
fn events_from_the_script_phase_reach_handlers_next_tick() {
    // The starting state's entry comes with the first tick's events; the
    // script's own change reaches `entered` a tick later, and entering B
    // again within `entered` does not loop.
    let script = "fn start() { this.set_state(\"B\"); }\n\
                  fn entered(s) {\n\
                    if s == \"A\" { this.set(\"v\", 100 + this.time()); }\n\
                    if s == \"B\" { this.set(\"v\", 200 + this.time()); this.set_state(\"B\"); }\n\
                  }";
    let mut w = world(level(vec![copy("c", 20, ABC, vec![0])], &[script], None));
    let mut v = Vec::new();
    for _ in 0..4 {
        assert!(play(&mut w, 1)[0].is_empty());
        v.push(read(&w, "c"));
    }
    let want: Vec<(String, i64)> =
        [("B", 100), ("B", 210), ("B", 220), ("B", 230)].iter().map(|&(s, v)| (s.to_string(), v)).collect();
    assert_eq!(v, want);
    // A worm event: the setup's spawns reach `spawned` in the first tick.
    let script = "fn spawned(worm) { this.set(\"v\", this.get(\"v\") * 10 + worm + 1); }";
    let mut w = world(level(vec![copy("c", 20, ABC, vec![0])], &[script], None));
    play(&mut w, 1);
    assert_eq!(read(&w, "c").1, -88, "worms 0 then 1: -1, -9, -88");
}

#[test]
fn timers_fire_in_order_and_every_repeats() {
    let script = "fn start() { this.set(\"v\", 0); this.after(2, \"a\"); this.after(2, \"b\"); this.every(3, \"c\"); \
                    this.after(50, \"d\"); this.cancel(\"d\"); }\n\
                  fn add(n) { this.set(\"v\", this.get(\"v\") * 10 + n); }\n\
                  fn a() { this.add(1); }\n\
                  fn b() { this.add(2); }\n\
                  fn c() { this.add(3); }\n\
                  fn d() { this.add(9); }";
    let mut w = world(level(vec![copy("c", 20, ABC, vec![0])], &[script], None));
    let mut v = Vec::new();
    for _ in 0..60 {
        assert!(play(&mut w, 1)[0].is_empty());
        v.push(read(&w, "c").1);
    }
    assert_eq!(&v[..10], [0, 0, 12, 123, 123, 123, 1233, 1233, 1233, 12333]);
    assert!(!v[59].to_string().contains('9'), "d was cancelled: {}", v[59]);
    // A timer naming no function, or of 0 ticks, is a script error.
    for bad in ["fn tick() { this.after(1, \"nothing\"); }", "fn tick() { this.every(0, \"tick\"); }"] {
        let mut w = world(level(vec![copy("c", 20, ABC, vec![0])], &[bad], None));
        assert_eq!(play(&mut w, 1), [vec![0]], "{bad}");
    }
}

#[test]
fn memory_persists_between_runs_and_not_after_a_failed_run() {
    let script = "fn tick() {\n\
                    if this.mem.n == () { this.mem.n = 0; this.mem.name = \"lamp\"; this.mem.on = true; }\n\
                    this.mem.n += 1;\n\
                    this.set(\"v\", this.mem.n);\n\
                    if this.time() == 20 { this.mem.n = 100; throw \"boom\"; }\n\
                    if this.mem.name != \"lamp\" || !this.mem.on { throw \"lost\"; }\n\
                  }";
    let mut w = world(level(vec![copy("c", 20, ABC, vec![0])], &[script], None));
    let errors = play(&mut w, 5);
    assert_eq!(errors, [vec![], vec![], vec![0], vec![], vec![]]);
    assert_eq!(read(&w, "c").1, 4, "the failed run kept neither its memory nor its value");
    // Memory past its bounds is a script error.
    for bad in [
        "fn tick() { this.mem.x = [1]; }",
        "fn tick() { let m = #{}; for i in 0..33 { m[`k${i}`] = i; } this.mem = m; }",
        "fn tick() { this.mem.s = \"x\".pad(65, \"x\"); }",
    ] {
        let mut w = world(level(vec![copy("c", 20, ABC, vec![0])], &[bad], None));
        assert_eq!(play(&mut w, 1), [vec![0]], "{bad}");
    }
}

#[test]
fn an_endless_loop_hits_the_run_limit_and_changes_nothing() {
    let script = "fn tick() { this.set(\"v\", 5); this.mem.x = 1; this.rand(10); loop {} }";
    let quiet = "fn tick() { }";
    let mut w = world(level(vec![copy("c", 20, ABC, vec![0])], &[script], None));
    assert_eq!(play(&mut w, 3), [vec![0], vec![0], vec![0]]);
    assert_eq!(read(&w, "c").1, -1);
    assert_eq!(w.script_peak().run, MAX_RUN_OPERATIONS);
    // Apart from its error count, the world is as with a script that does
    // nothing: compare the copies and the peers' worms.
    let mut q = world(level(vec![copy("c", 20, ABC, vec![0])], &[quiet], None));
    play(&mut q, 3);
    assert_eq!(read(&w, "c"), read(&q, "c"));
    assert_eq!(w.worms()[0].pos, q.worms()[0].pos);
    assert_ne!(w.checksum(), q.checksum(), "the error count is state");
}

/// A script spending about `rounds` × 10 operations, then counting its
/// runs in `v`.
fn heavy(rounds: u32) -> String {
    format!(
        "fn tick() {{ let i = 0; while i < {rounds} {{ i += 1; }} this.set(\"v\", this.get(\"v\") + 1); }}"
    )
}

#[test]
fn the_tick_budget_skips_the_same_runs_every_time() {
    let text = heavy(400);
    let copies = (0..8).map(|i| copy(&format!("c{i}"), 10 + i * 5, ABC, vec![0])).collect();
    let lvl = level(copies, &[&text], None);
    let mut w = world(lvl.clone());
    assert!(play(&mut w, 10).iter().all(Vec::is_empty));
    let v: Vec<i64> = (0..8).map(|i| read(&w, &format!("c{i}")).1).collect();
    // Each run takes a few thousand operations: the first copies run
    // every tick, the rest never (the same ones every tick).
    let ran = v.iter().filter(|&&v| v == 9).count();
    assert!((2..8).contains(&ran), "{v:?}");
    assert!(v[..ran].iter().all(|&v| v == 9) && v[ran..].iter().all(|&v| v == -1), "{v:?}");
    let peak = w.script_peak();
    assert!(
        peak.tick <= MAX_TICK_OPERATIONS && peak.tick > MAX_TICK_OPERATIONS - MAX_RUN_OPERATIONS,
        "{peak:?}"
    );
    // Another world plays the same.
    let mut again = world(lvl);
    play(&mut again, 10);
    assert_eq!(again.checksum(), w.checksum());
    assert_eq!(again.script_peak(), peak);
}

#[test]
fn cheap_runs_are_charged_their_own_count() {
    let text = heavy(0);
    let copies = (0..40).map(|i| copy(&format!("c{i}"), 5 + i * 4, ABC, vec![0])).collect();
    let mut w = world(level(copies, &[&text], None));
    play(&mut w, 3);
    assert!((0..40).all(|i| read(&w, &format!("c{i}")).1 == 2), "every run ran");
    let peak = w.script_peak();
    assert!(peak.run > 0 && peak.run < 100, "{peak:?}");
    assert!(peak.tick >= 40 * 5 && peak.tick < 40 * 100, "{peak:?}");
    // 40 runs charged their limit would pass the budget; a heavier script
    // costs more.
    let mut h = world(level(vec![copy("c", 20, ABC, vec![0])], &[&heavy(100)], None));
    play(&mut h, 1);
    assert!(h.script_peak().run > 10 * peak.run, "{:?} vs {peak:?}", h.script_peak());
}

#[test]
fn two_worlds_on_two_threads_count_alike() {
    let text = "fn tick() { let n = this.rand(300); let i = 0; while i < n { i += 1; } \
           this.set(\"v\", n); if this.chance(30) { this.set_state(\"B\"); } else { this.set_state(\"A\"); } }";
    let copies = (0..6).map(|i| copy(&format!("c{i}"), 10 + i * 8, ABC, vec![0])).collect();
    let s = setup(level(copies, &[text], None), 3, true);
    let run = |s: olr_sim::MatchSetup| {
        let mut w = World::new(s, Box::new(OracleMode)).unwrap();
        let mut sums = Vec::new();
        for _ in 0..300 {
            play(&mut w, 1);
            sums.push(w.checksum());
        }
        (sums, w.script_peak())
    };
    let (a, b) = std::thread::scope(|t| {
        let (s1, s2) = (s.clone(), s.clone());
        let a = t.spawn(move || run(s1));
        let b = t.spawn(move || run(s2));
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(a, b);
    assert_eq!(run(s), a, "and alone");
}

#[test]
fn a_script_switched_off_after_too_many_errors() {
    let script = "fn tick() { throw \"broken\"; }";
    let fine = "fn tick() { this.set(\"v\", this.get(\"v\") + 1); }";
    let mut w = world(level(vec![copy("c", 20, ABC, vec![0, 1])], &[script, fine], None));
    let errors = play(&mut w, 30);
    let n = usize::from(MAX_SCRIPT_ERRORS);
    assert!(errors[..n].iter().all(|e| e == &[0]), "{errors:?}");
    assert!(errors[n..].iter().all(Vec::is_empty), "{errors:?}");
    assert_eq!(read(&w, "c").1, 29, "the other instance runs on");
}

#[test]
fn floats_and_imports_do_not_compile() {
    for bad in [
        "let x = 1.5;",
        "fn tick() { let x = 2.0 * 3; }",
        "import \"a\" as b;",
        "fn tick() { eval(\"1\"); }",
        "fn tick() { let s = timestamp(); }",
    ] {
        let lvl = level(vec![copy("c", 20, ABC, vec![0])], &["fn tick() {}", bad], None);
        let e = Scripts::compile(&lvl).map(|_| ()).err();
        let compiles = e.is_none();
        if bad.contains("timestamp") {
            // No such function: it compiles and fails when run.
            assert!(compiles, "{bad}");
            let mut w = world(level(vec![copy("c", 20, ABC, vec![0])], &[bad], None));
            assert_eq!(play(&mut w, 1), [vec![0]]);
        } else {
            assert_eq!(e.map(|e| e.script), Some(1), "{bad}");
            assert_eq!(olr_script::check(bad).len(), 1, "{bad}");
        }
    }
    // A level with a script that does not compile has no director.
    assert!(olr_script::director(&level(vec![], &["fn ("], None)).is_err());
    // Overflow is a script error, not a panic.
    let mut w = world(level(
        vec![copy("c", 20, ABC, vec![0])],
        &["fn tick() { let x = 9223372036854775807; x += 1; }"],
        None,
    ));
    assert_eq!(play(&mut w, 1), [vec![0]]);
}

#[test]
fn rand_never_draws_from_the_worlds_generators() {
    let script = "fn tick() { this.set(\"v\", this.rand(1000)); this.chance(50); }";
    let lvl = level(vec![copy("c", 20, ABC, vec![0])], &[script], None);
    let mut a = World::new(setup(lvl.clone(), 11, true), Box::new(OracleMode)).unwrap();
    let mut b = World::new(setup(lvl.clone(), 11, false), Box::new(OracleMode)).unwrap();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..200 {
        play(&mut a, 1);
        play(&mut b, 1);
        assert_eq!((a.rand_calls(), a.mt_calls()), (b.rand_calls(), b.mt_calls()));
        assert_eq!(a.worms()[0].pos, b.worms()[0].pos);
        seen.insert(read(&a, "c").1);
    }
    assert!(seen.len() > 100 && seen.iter().all(|v| (0..1000).contains(v)), "{seen:?}");
    // The seed seeds it.
    let mut c = World::new(setup(lvl, 12, true), Box::new(OracleMode)).unwrap();
    play(&mut c, 1);
    let mut a1 = World::new(
        setup(level(vec![copy("c", 20, ABC, vec![0])], &[script], None), 11, true),
        Box::new(OracleMode),
    )
    .unwrap();
    play(&mut a1, 1);
    assert_ne!(read(&c, "c").1, read(&a1, "c").1);
    assert!(
        play(&mut world(level(vec![copy("c", 20, ABC, vec![0])], &["fn tick() { this.rand(0); }"], None)), 1)
            [0]
            == [0]
    );
}

#[test]
fn rhai_version_matches_cargo_lock() {
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock")).unwrap();
    let mut lines = lock.lines();
    let mut versions = Vec::new();
    while let Some(l) = lines.next() {
        if l == "name = \"rhai\"" {
            versions.push(lines.next().unwrap_or_default().to_string());
        }
    }
    assert_eq!(versions, [format!("version = \"{}\"", olr_script::RHAI_VERSION)]);
}

/// A mode whose match ends at the third tick.
#[derive(Clone, Debug)]
struct EndsSoon;

impl olr_sim::GameMode for EndsSoon {
    fn box_clone(&self) -> Box<dyn olr_sim::GameMode> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn checksum(&self, _: &mut olr_sim::Fnv) {}

    fn on_kill(&mut self, v: i32, k: i32, ctx: &mut olr_sim::ModeCtx) -> olr_sim::KillCredit {
        let _ = (v, k, ctx);
        olr_sim::KillCredit::default()
    }

    fn is_over(&self, w: &World) -> Option<olr_sim::MatchResult> {
        (w.server_frame() >= 3)
            .then_some(olr_sim::MatchResult { winner: -1, reason: olr_sim::EndReason::Other(0) })
    }
}

#[test]
fn scripts_read_the_match_and_a_frozen_world_runs_none() {
    let script = "fn tick() {\n\
                    if this.worm_count() != 2 || !this.worm_alive(1) || this.worm_x(1) != 160 { throw \"worms\"; }\n\
                    if this.worm_y(0) < 50 { throw \"y\"; }\n\
                    this.set(\"v\", if this.game_over() { 1000 + this.get(\"v\") } else { this.time() });\n\
                  }";
    let lvl = level(vec![copy("c", 20, ABC, vec![0])], &[script], None);
    let mut w = World::new(setup(lvl, 1, true), Box::new(EndsSoon)).unwrap();
    assert!(play(&mut w, 2).iter().all(Vec::is_empty));
    assert_eq!(read(&w, "c").1, 10);
    // Over from the third tick; frozen 3 s on, when nothing runs.
    assert!(play(&mut w, 600).iter().all(Vec::is_empty));
    let v = read(&w, "c").1;
    assert!(v > 1000 * 250 && v < 1000 * 350, "{v}");
    play(&mut w, 100);
    assert_eq!(read(&w, "c").1, v, "a frozen world runs no script");
    // A worm that is not there is a script error.
    let mut w = world(level(vec![copy("c", 20, ABC, vec![0])], &["fn tick() { this.worm_x(2); }"], None));
    assert_eq!(play(&mut w, 1), [vec![0]]);
}
