//! Issue #89: `move_to`, `stop`, `moving`, `min`, `max` and `arrived`.

use olr_sim::{LIVE_SUBPIXELS, LiveCopy, World};

use crate::common::{copy, level, play, read, world};

const AB: &[&str] = &["A", "B"];

/// Copy `id`'s `x` in the world's units, and whether it moves.
fn x(w: &World, id: &str) -> (i64, bool) {
    let c = w.live().unwrap().copy_by_id(id).unwrap();
    let p = usize::from(c.setup().prop("x").unwrap());
    (c.values()[p], c.motions()[p].moving())
}

/// [`copy`] with its `x` limits `-lim..=lim` (a copy stretched by
/// `lim / 50`, as the bake scales them).
fn stretched(id: &str, at: i32, lim: i64, scripts: Vec<u16>) -> LiveCopy {
    let mut c = copy(id, at, AB, scripts);
    c.props[1].min = -lim;
    c.props[1].max = lim;
    c
}

#[test]
fn move_to_reaches_its_target_at_its_speed_and_calls_arrived() {
    // 100 px a second is 1 px (256) a tick; 40 px a second 102/256 px.
    let fast = "fn start() { this.move_to(\"x\", 30, 100); }\n\
                fn arrived(name) {\n\
                  if name == \"x\" { this.set(\"v\", this.time()); this.set_state(\"B\"); }\n\
                }";
    let slow = "fn start() { this.move_to(\"x\", -1000, 40); }\n\
                fn arrived(name) { this.set(\"v\", this.get(\"x\")); }";
    let mut w =
        world(level(vec![copy("fast", 20, AB, vec![0]), copy("slow", 40, AB, vec![1])], &[fast, slow], None));
    // Tick 1 starts the motions (its script phase); each tick after moves
    // them once, before its physics.
    play(&mut w, 1);
    assert_eq!(x(&w, "fast"), (0, true));
    for k in 1..=30i64 {
        assert!(play(&mut w, 1)[0].is_empty());
        assert_eq!(x(&w, "fast"), (k * LIVE_SUBPIXELS, k < 30), "tick {}", k + 1);
        assert_eq!(x(&w, "slow").0, -102 * k);
    }
    // Arrived in tick 31 (time 300), its handler in the same tick.
    assert_eq!(read(&w, "fast"), ("B".into(), 300));
    // The slow one stops at its limit, -50 px, after 126 ticks.
    play(&mut w, 94);
    assert_eq!(x(&w, "slow"), (-102 * 124, true));
    play(&mut w, 2);
    assert_eq!(x(&w, "slow"), (-50 * LIVE_SUBPIXELS, false));
    assert_eq!(read(&w, "slow").1, -50);
    // A speed below 1, or a value that is not a position, is a script
    // error.
    for bad in ["fn start() { this.move_to(\"x\", 3, 0); }", "fn start() { this.move_to(\"v\", 3, 10); }"] {
        let mut w = world(level(vec![copy("c", 20, AB, vec![0])], &[bad], None));
        assert_eq!(play(&mut w, 1), [vec![0]], "{bad}");
    }
}

#[test]
fn min_and_max_follow_a_stretched_copy() {
    // One default script sweeps each copy's whole travel.
    let sweep = "fn start() {\n\
                   this.set(\"v\", this.min(\"x\") * 1000 + this.max(\"x\"));\n\
                   this.move_to(\"x\", this.max(\"x\"), 800);\n\
                 }\n\
                 fn arrived(name) {\n\
                   let to = if this.get(name) == this.max(name) { this.min(name) } else { this.max(name) };\n\
                   this.move_to(name, to, 800);\n\
                 }";
    let copies = vec![stretched("plain", 20, 50, vec![0]), stretched("wide", 40, 100, vec![0])];
    let mut w = world(level(copies, &[sweep], None));
    play(&mut w, 1);
    assert_eq!(read(&w, "plain").1, -50 * 1000 + 50);
    assert_eq!(read(&w, "wide").1, -100 * 1000 + 100);
    // 8 px a tick: each reaches its own max, then turns back.
    let (mut plain, mut wide) = (Vec::new(), Vec::new());
    for _ in 0..60 {
        play(&mut w, 1);
        plain.push(x(&w, "plain").0 / LIVE_SUBPIXELS);
        wide.push(x(&w, "wide").0 / LIVE_SUBPIXELS);
    }
    assert_eq!(plain.iter().max(), Some(&50));
    assert_eq!(plain.iter().min(), Some(&-50));
    assert_eq!(wide.iter().max(), Some(&100));
    assert_eq!(wide.iter().min(), Some(&-100));
    // Another copy's limits through its handle.
    let other = "fn start() { let c = this.copy(\"wide\"); this.set(\"v\", this.max(c, \"x\") * 1000 + this.min(c, \"x\")); }";
    let copies = vec![stretched("plain", 20, 50, vec![0]), stretched("wide", 40, 100, vec![])];
    let mut w = world(level(copies, &[other], None));
    play(&mut w, 1);
    assert_eq!(read(&w, "plain").1, 100 * 1000 - 100);
}

#[test]
fn a_set_stops_a_motion_and_a_state_change_does_not() {
    // Tick k runs at time (k - 1) * 10.
    let script = "fn start() { this.move_to(\"x\", 50, 100); }\n\
                  fn tick() {\n\
                    let t = this.time();\n\
                    if t == 50 { this.set_state(\"B\"); }\n\
                    if t == 100 { this.set(\"x\", 5); }\n\
                    if t == 150 { this.move_to(\"x\", -50, 100); }\n\
                    if t == 180 { this.stop(\"x\"); }\n\
                    this.set(\"v\", if this.moving(\"x\") { 1 } else { 0 });\n\
                  }";
    let mut w = world(level(vec![copy("c", 20, AB, vec![0])], &[script], None));
    let mut seen = Vec::new();
    for _ in 0..25 {
        assert!(play(&mut w, 1)[0].is_empty());
        let (v, moving) = x(&w, "c");
        seen.push((v / LIVE_SUBPIXELS, moving, read(&w, "c").1));
    }
    // The state change at tick 6 keeps the motion.
    assert_eq!(read(&w, "c").0, "B");
    assert_eq!(seen[5], (5, true, 1));
    assert_eq!(seen[9], (9, true, 1));
    // The set at tick 11: 5 px, still.
    assert_eq!(seen[10], (5, false, 1), "the tick's read was before the set");
    assert_eq!(seen[11], (5, false, 0));
    assert_eq!(seen[14], (5, false, 0));
    // A new motion at tick 16, stopped at tick 19 where it is.
    assert_eq!(seen[15], (5, true, 0));
    assert_eq!(seen[16], (4, true, 1));
    assert_eq!(seen[18], (2, false, 1));
    assert_eq!(seen[24], (2, false, 0));
}
