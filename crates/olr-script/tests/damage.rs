//! Issue #94: `hit(amount)` and `wrecked()`.

use olr_math::Vec2;
use olr_sim::{LiveCopy, LiveHealth, LiveLevel, LiveProp, LiveTarget, TickEvents, World};
use std::sync::Arc;

use crate::common::{copy, keys, world};

/// [`copy`] in Whole or Wrecked with 30 points of health (its `health`
/// value), Wrecked at 0, running script 0.
fn destructible(id: &str, x: i32) -> LiveCopy {
    let mut c = copy(id, x, &["Whole", "Wrecked"], vec![0]);
    c.props.push(LiveProp {
        name: Some("health".into()),
        target: LiveTarget::Health,
        min: 0,
        max: 100,
        default: 100,
    });
    c.health = Some(LiveHealth { points: 30, at_zero: 1 });
    LiveCopy::new(olr_sim::LiveCopySetup {
        id: c.id,
        at: c.at,
        size: c.size,
        props: c.props,
        states: c.states,
        pictures: c.pictures,
        start_state: c.start_state,
        start_values: c.start_values,
        health: c.health,
        scripts: c.scripts,
        sounds: c.sounds,
    })
}

fn tick(w: &mut World) {
    let mut ev = TickEvents::new();
    w.tick(&keys(w.server_frame() as u32), &mut ev);
    assert!(!ev.iter().any(|e| matches!(e.kind, olr_sim::EventKind::ScriptError { .. })));
}

fn v(w: &World, id: &str) -> (String, i64, i64) {
    let c = w.live().unwrap().copy_by_id(id).unwrap();
    (c.state_name().to_string(), c.value("v").unwrap(), c.value("health").unwrap())
}

#[test]
fn hit_and_wrecked_handlers_run() {
    // Each handler appends two digits to `v`: the amount, 99 wrecked, 77
    // entering Wrecked.
    let script = "fn start() { this.set(\"v\", 0); }\n\
                  fn hit(amount) { this.set(\"v\", this.get(\"v\") * 100 + amount); }\n\
                  fn wrecked() { this.set(\"v\", this.get(\"v\") * 100 + 99); }\n\
                  fn entered(s) { if s == \"Wrecked\" { this.set(\"v\", this.get(\"v\") * 100 + 77); } }";
    let level = Arc::new(LiveLevel::new(
        vec![destructible("box", 20), destructible("far", 150)],
        vec![script.into()],
        None,
        0,
    ));
    let mut w = world(level);
    tick(&mut w);
    assert_eq!(v(&w, "box"), ("Whole".into(), 0, 30));
    // The box covers (20, 40) to (24, 44): an explosion of 12.9 beside it
    // takes 12 (truncated), and only the box's handlers hear it.
    w.explode_for_test(Vec2::new(26.0, 42.0), 12.9);
    tick(&mut w);
    assert_eq!(v(&w, "box"), ("Whole".into(), 12, 18));
    assert_eq!(v(&w, "far"), ("Whole".into(), 0, 30));
    // The next leaves it at 0: hit, wrecked, then its entry, in this
    // tick's script phase.
    w.explode_for_test(Vec2::new(22.0, 42.0), 20.0);
    tick(&mut w);
    assert_eq!(v(&w, "box"), ("Wrecked".into(), 12_20_99_77, 0));
    // At 0 nothing more is heard.
    w.explode_for_test(Vec2::new(22.0, 42.0), 20.0);
    tick(&mut w);
    assert_eq!(v(&w, "box"), ("Wrecked".into(), 12_20_99_77, 0));
    assert_eq!(v(&w, "far"), ("Whole".into(), 0, 30));
}
