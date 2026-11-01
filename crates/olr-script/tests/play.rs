//! Issue #92: `this.play(name)` and `this.play(c, name)`.

use olr_sim::{EventKind, LiveCopy, LiveLevel, TickEvents, World};
use std::sync::Arc;

use crate::common::{copy, keys, world};

const AB: &[&str] = &["A", "B"];

/// [`copy`] with its asset's sounds `sounds` (name, level sound index).
fn sounding(id: &str, at: i32, sounds: &[(&str, u16)], scripts: Vec<u16>) -> LiveCopy {
    let mut c = copy(id, at, AB, scripts);
    c.sounds = sounds.iter().map(|&(n, k)| (n.to_string(), k)).collect();
    c
}

/// Ticks `w` once: its `LiveSound` events and script errors.
fn tick(w: &mut World) -> (Vec<(u16, u16)>, usize) {
    let mut ev = TickEvents::new();
    w.tick(&keys(w.server_frame() as u32), &mut ev);
    let sounds = ev
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::LiveSound { copy, sound } => Some((copy, sound)),
            _ => None,
        })
        .collect();
    (sounds, ev.iter().filter(|e| matches!(e.kind, EventKind::ScriptError { .. })).count())
}

#[test]
fn play_queues_a_live_sound() {
    let a = "fn start() { this.play(\"hum.wav\"); this.play(this.copy(\"b\"), \"tick.ogg\"); }\n\
             fn tick() { if this.time() == 20 { this.play(\"nope.wav\"); } }";
    let level_script = "fn start() { this.play(this.copy(\"a\"), \"tick.ogg\"); }";
    let copies = vec![
        sounding("a", 20, &[("hum.wav", 0), ("tick.ogg", 1)], vec![0]),
        sounding("b", 60, &[("tick.ogg", 1)], vec![]),
    ];
    let level = Arc::new(LiveLevel::new(copies, vec![a.into(), level_script.into()], Some(1), 2));
    let mut w = world(level);
    let before = w.live().unwrap().copies().map(|c| (c.state(), c.values().to_vec())).collect::<Vec<_>>();
    // The level script runs first, then copy a's; each queued sound is
    // one event, in order.
    assert_eq!(tick(&mut w), (vec![(0, 1), (0, 0), (1, 1)], 0));
    let after = w.live().unwrap().copies().map(|c| (c.state(), c.values().to_vec())).collect::<Vec<_>>();
    assert_eq!(before, after, "a sound changes no copy");
    assert_eq!(tick(&mut w), (vec![], 0));
    // A name the copy's asset lacks is a script error: nothing plays.
    assert_eq!(tick(&mut w), (vec![], 1));
    // The level script has no copy of its own.
    let own = "fn start() { this.play(\"hum.wav\"); }";
    let level = Arc::new(LiveLevel::new(vec![sounding("a", 20, &[("hum.wav", 0)], vec![])], vec![own.into()], Some(0), 1));
    assert_eq!(tick(&mut world(level)), (vec![], 1));
}
