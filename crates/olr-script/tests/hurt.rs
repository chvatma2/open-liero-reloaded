//! 2026-11-05: `this.hurt(worm, damage)` (the factory's press kills a worm
//! it wedges, in its `crushed(worm)` handler).

use olr_sim::{EventKind, TickEvents};

use crate::common::{copy, keys, level, world};

/// The level script hurts worm 1 by `damage` in its first tick: the
/// tick's events and worm 1's health after it.
fn hurt(damage: &str) -> (Vec<EventKind>, f32) {
    let script = format!("fn start() {{ this.hurt(1, {damage}); }}");
    let mut w = world(level(vec![copy("a", 40, &["A"], vec![])], &[script.as_str()], Some(0)));
    let mut ev = TickEvents::new();
    w.tick(&keys(0), &mut ev);
    (ev.iter().map(|e| e.kind).collect(), w.worms()[1].health)
}

#[test]
fn hurt_deals_damage_by_nobody_and_kills() {
    let (ev, health) = hurt("30");
    assert_eq!(health, 70.0);
    assert!(ev.iter().any(|e| matches!(e, EventKind::Damage { worm: 1, by: -1, .. })), "{ev:?}");
    let (ev, health) = hurt("1000");
    assert!(health <= 0.0);
    assert!(ev.iter().any(|e| matches!(e, EventKind::Kill { victim: 1, killer: -1 })), "{ev:?}");
}

#[test]
fn hurt_refuses_no_damage_and_no_worm() {
    for bad in ["0", "1001"] {
        let (ev, health) = hurt(bad);
        assert_eq!(health, 100.0, "{bad}");
        assert!(ev.iter().any(|e| matches!(e, EventKind::ScriptError { .. })), "{bad}: {ev:?}");
    }
    let script = "fn start() { this.hurt(5, 10); }";
    let mut w = world(level(vec![copy("a", 40, &["A"], vec![])], &[script], Some(0)));
    let mut ev = TickEvents::new();
    w.tick(&keys(0), &mut ev);
    assert!(ev.iter().any(|e| matches!(e.kind, EventKind::ScriptError { .. })));
}
