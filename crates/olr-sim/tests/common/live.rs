//! A level's live copies made here (issue #86), like
//! `fixtures/live/levels/live-test`'s: a console with Normal and Alarm, a
//! press whose head moves, a crate that can be wrecked.
#![allow(dead_code)]

use std::sync::Arc;

use olr_sim::{
    LiveCopy, LiveCopySetup, LiveHealth, LiveLevel, LivePart, LivePartKind, LiveProp, LiveSolid, LiveState,
    LiveTarget,
};

fn prop(name: Option<&str>, target: LiveTarget, min: i64, max: i64, default: i64) -> LiveProp {
    LiveProp { name: name.map(String::from), target, min, max, default }
}

fn part(kind: LivePartKind, layer: Option<u16>, at: (i32, i32), picture: u16) -> LivePart {
    LivePart { kind, layer, at, picture, glow_strength: 100 }
}

fn state(name: &str, values: Vec<(u16, i64)>, parts: Vec<LivePart>) -> LiveState {
    LiveState { name: name.into(), values, parts, enter_sound: None, loop_sound: None }
}

fn solid(size: (u32, u32)) -> LiveSolid {
    LiveSolid::new(size, true, |_, _| true)
}

/// The console: `screen_colour` (green in Normal, red in Alarm) and an
/// unnamed glow strength both states set (50, 100).
fn console() -> LiveCopy {
    let parts = || vec![part(LivePartKind::Rock, None, (0, 0), 0)];
    LiveCopy::new(LiveCopySetup {
        id: "console".into(),
        at: (60, 200),
        size: (8, 6),
        props: vec![
            prop(Some("screen_colour"), LiveTarget::GlowColour(1), -1, 0xff_ffff, 0x00_ff00),
            prop(None, LiveTarget::GlowStrength(1), 0, 100, 50),
        ],
        states: vec![
            state("Normal", vec![(0, 0x00_ff00), (1, 50)], parts()),
            state("Alarm", vec![(0, 0xff_0000), (1, 100)], parts()),
        ],
        pictures: vec![solid((8, 6))],
        start_state: 0,
        start_values: Vec::new(),
        health: None,
        scripts: Vec::new(),
        sounds: Vec::new(),
    })
}

/// The press: `head_y` moves its head (layer 1; -10 Up, 10 Down), `x` the
/// whole copy (its own start value 5), `head_shown` shows the head.
fn press() -> LiveCopy {
    let parts = || {
        vec![
            part(LivePartKind::Background, None, (0, 0), 0),
            part(LivePartKind::Rock, Some(0), (0, 0), 1),
            part(LivePartKind::Rock, Some(1), (2, 4), 2),
        ]
    };
    LiveCopy::new(LiveCopySetup {
        id: "press".into(),
        at: (150, 100),
        size: (10, 30),
        props: vec![
            prop(Some("head_y"), LiveTarget::OffsetY(1), -20, 20, 0),
            prop(Some("x"), LiveTarget::X, -100, 100, 0),
            prop(Some("head_shown"), LiveTarget::LayerVisible(1), 0, 1, 1),
        ],
        states: vec![state("Up", vec![(0, -10)], parts()), state("Down", vec![(0, 10)], parts())],
        pictures: vec![LiveSolid::new((10, 30), false, |_, _| false), solid((10, 4)), solid((6, 6))],
        start_state: 1,
        start_values: vec![(1, 5)],
        health: None,
        scripts: Vec::new(),
        sounds: Vec::new(),
    })
}

/// The crate: 40 points of `health`, Wrecked at 0.
fn crate_copy() -> LiveCopy {
    LiveCopy::new(LiveCopySetup {
        id: "crate".into(),
        at: (250, 180),
        size: (12, 12),
        props: vec![prop(Some("health"), LiveTarget::Health, 0, 100, 100)],
        states: vec![
            state("Whole", Vec::new(), vec![part(LivePartKind::Rock, None, (0, 0), 0)]),
            state("Wrecked", Vec::new(), vec![part(LivePartKind::Background, None, (0, 6), 1)]),
        ],
        pictures: vec![solid((12, 12)), LiveSolid::new((12, 6), false, |_, _| false)],
        start_state: 0,
        start_values: Vec::new(),
        health: Some(LiveHealth { points: 40, at_zero: 1 }),
        scripts: Vec::new(),
        sounds: Vec::new(),
    })
}

/// The three copies: console (0), press (1), crate (2).
pub fn live_level() -> Arc<LiveLevel> {
    Arc::new(LiveLevel::new(vec![console(), press(), crate_copy()], Vec::new(), None, 0))
}
