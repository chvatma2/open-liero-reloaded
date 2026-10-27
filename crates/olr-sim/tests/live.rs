//! Issue #85: a level's live copies as setup data, found by name.

use olr_sim::{
    LiveCopy, LiveCopySetup, LiveHealth, LiveLevel, LivePart, LivePartKind, LiveProp, LiveSolid, LiveState,
    LiveTarget,
};

fn state(name: &str, values: Vec<(u16, i64)>) -> LiveState {
    LiveState {
        name: name.into(),
        values,
        parts: vec![LivePart {
            kind: LivePartKind::Rock,
            layer: Some(0),
            at: (0, 0),
            picture: 0,
            glow_strength: 0,
        }],
        enter_sound: None,
        loop_sound: None,
    }
}

fn copy(id: &str, states: &[&str], props: &[Option<&str>]) -> LiveCopy {
    LiveCopy::new(LiveCopySetup {
        id: id.into(),
        at: (3, 4),
        size: (3, 2),
        props: props
            .iter()
            .enumerate()
            .map(|(i, n)| LiveProp {
                name: n.map(String::from),
                target: LiveTarget::OffsetX(i as u16),
                min: -2,
                max: 2,
                default: 0,
            })
            .collect(),
        states: states.iter().map(|s| state(s, vec![(0, 1)])).collect(),
        pictures: vec![LiveSolid::new((3, 2), true, |x, y| x == y)],
        start_state: 0,
        start_values: Vec::new(),
        health: Some(LiveHealth { points: 40, at_zero: 1 }),
        scripts: vec![0],
        sounds: vec![("tick.wav".into(), 0)],
    })
}

#[test]
fn live_level_finds_copies_states_and_props_by_name() {
    let level = LiveLevel::new(
        vec![
            copy("press", &["Up", "Down"], &[Some("head_y"), None]),
            copy("console-b", &["Normal", "Alarm"], &[Some("screen_colour")]),
            copy("console-a", &["Normal", "Alarm"], &[None, Some("screen_colour"), Some("blink")]),
        ],
        vec!["fn tick() {}".into()],
        None,
        1,
    );
    assert_eq!(level.copy("press"), Some(0));
    assert_eq!(level.copy("console-a"), Some(2));
    assert_eq!(level.copy("console-b"), Some(1));
    assert_eq!(level.copy("crate"), None);
    let a = level.copy_by_id("console-a").unwrap();
    assert_eq!((a.state("Normal"), a.state("Alarm"), a.state("alarm")), (Some(0), Some(1), None));
    assert_eq!((a.prop("screen_colour"), a.prop("blink"), a.prop("head_y")), (Some(1), Some(2), None));
    let press = level.copy_by_id("press").unwrap();
    assert_eq!((press.prop("head_y"), press.state("Down")), (Some(0), Some(1)));
    // An unexposed value has no name to find it by.
    assert_eq!(press.props[1].name, None);
    assert_eq!(press.sound("tick.wav"), Some(0));
    // The rock mask: a bit per world pixel.
    let solid = &press.pictures[0];
    assert!(solid.solid(0, 0) && solid.solid(1, 1) && !solid.solid(1, 0) && !solid.solid(5, 0));
    assert!(!LiveSolid::new((3, 2), false, |_, _| true).solid(0, 0));
}

// Issue #86: live copies in the world.

use crate::live_level::live_level;
use crate::world;

use olr_sim::{EventKind, Keys, LIVE_SUBPIXELS, LiveError, TickEvents, World, WorldState};

const PX: i64 = LIVE_SUBPIXELS;

fn keys(t: u32, n: u32) -> Vec<Keys> {
    (0..n)
        .map(|i| Keys(if (t + i) % 5 < 3 { Keys::FIRE | Keys::LEFT } else { Keys::UP | Keys::JUMP }))
        .collect()
}

fn live_world() -> World {
    world::world_live(4, 400, 300, live_level())
}

fn values(w: &World, copy: u16) -> Vec<i64> {
    w.live().unwrap().copy(copy).unwrap().values().to_vec()
}

/// The checksums a world without live copies had before issue #86 (the
/// match goldens and the oracle traces check the same on real content).
#[test]
fn a_world_without_live_copies_hashes_as_before() {
    let mut w = world::world_carving(4, 400, 300);
    assert!(w.live().is_none());
    let mut ev = TickEvents::new();
    for t in 0..300 {
        w.tick(&keys(t, 4), &mut ev);
    }
    assert_eq!(w.checksum(), 0x83b5_c219_a44c_d0ae);
    assert_eq!(w.sync_checksum(), 0xe95d_6a22_d69b_e651);
}

#[test]
fn entering_a_state_sets_its_complete_look() {
    let mut w = live_world();
    let console = w.live().unwrap().copy_by_id("console").unwrap();
    assert_eq!((console.index(), console.state_name()), (0, "Normal"));
    assert_eq!(console.values(), [0x00_ff00, 50]);
    w.set_live_state(0, 1).unwrap();
    let console = w.live().unwrap().copy(0).unwrap();
    assert_eq!((console.state(), console.state_name()), (1, "Alarm"));
    assert_eq!(console.values(), [0xff_0000, 100]);
    assert_eq!(console.value("screen_colour"), Some(0xff_0000));
    // A value set afterwards lasts until the next state change.
    w.set_live_value(0, 0, 0x12_3456).unwrap();
    assert_eq!(values(&w, 0), [0x12_3456, 100]);
    w.set_live_state(0, 0).unwrap();
    assert_eq!(values(&w, 0), [0x00_ff00, 50], "Normal restores what Alarm set");
    // The entries come with the next tick's events, after setup's.
    let mut ev = TickEvents::new();
    w.tick(&[], &mut ev);
    let entered: Vec<_> = ev
        .iter()
        .filter_map(|e| match e.kind {
            EventKind::LiveState { copy, state } => Some((copy, state)),
            _ => None,
        })
        .collect();
    assert_eq!(entered, [(0, 0), (1, 1), (2, 0), (0, 1), (0, 0)]);
    w.tick(&[], &mut ev);
    assert!(!ev.iter().any(|e| matches!(e.kind, EventKind::LiveState { .. })), "once");
}

#[test]
fn starting_values_follow_the_starting_state() {
    let w = live_world();
    let live = w.live().unwrap();
    assert_eq!(live.len(), 3);
    let press = live.copy_by_id("press").unwrap();
    assert_eq!(press.state_name(), "Down");
    // Down's head_y (10 px), then the copy's own x (5 px), in 1/256 pixel.
    assert_eq!(press.values(), [10 * PX, 5 * PX, 1]);
    assert_eq!(press.rect(), (155, 100, 10, 30));
    let rock: Vec<_> = press.rock_parts().map(|(p, at)| (p.picture, at)).collect();
    assert_eq!(rock, [(1, (155, 100)), (2, (157, 114))]);
    assert_eq!(press.visible_parts().count(), 3);
    // Health starts within 0..=points.
    assert_eq!(live.copy_by_id("crate").unwrap().value("health"), Some(40));
}

#[test]
fn values_are_clamped_and_parts_placed_at_the_whole_pixel() {
    let mut w = live_world();
    w.set_live_value(1, 0, 1000 * PX).unwrap();
    assert_eq!(values(&w, 1)[0], 20 * PX, "head_y's limits");
    // Sub-pixel steps round down to the pixel.
    w.set_live_value(1, 0, 10 * PX + 255).unwrap();
    let at = |w: &World| w.live().unwrap().copy(1).unwrap().rock_parts().map(|(_, at)| at).nth(1);
    assert_eq!(at(&w), Some((157, 114)));
    w.set_live_value(1, 0, -1).unwrap();
    assert_eq!(at(&w), Some((157, 103)));
    w.set_live_value(1, 2, 0).unwrap();
    assert_eq!(at(&w), None, "the head is hidden");
    w.set_live_value(2, 0, 100).unwrap();
    assert_eq!(values(&w, 2), [40], "health at most its points");
    w.set_live_value(2, 0, -5).unwrap();
    assert_eq!(values(&w, 2), [0]);
}

/// Issue #87: presentation reads a copy's value by what it drives.
#[test]
fn a_value_is_read_by_its_target() {
    let mut w = live_world();
    let live = w.live().unwrap();
    let console = live.copy_by_id("console").unwrap();
    assert_eq!(console.target(LiveTarget::GlowStrength(1)), Some(50));
    assert_eq!(console.target(LiveTarget::Facing), None, "nothing drives it");
    w.set_live_value(1, 0, 3 * PX).unwrap();
    let press = w.live().unwrap().copy(1).unwrap();
    assert_eq!(press.target(LiveTarget::OffsetY(1)), Some(3 * PX), "the world's units");
}

#[test]
fn unknown_copies_states_and_values_are_refused() {
    let mut w = live_world();
    let before = w.clone();
    assert_eq!(w.set_live_state(3, 0), Err(LiveError::NoCopy(3)));
    assert_eq!(w.set_live_state(0, 2), Err(LiveError::NoState { copy: 0, state: 2 }));
    assert_eq!(w.set_live_value(1, 3, 0), Err(LiveError::NoProp { copy: 1, prop: 3 }));
    assert_eq!(w, before, "a refused call changes nothing");
    let mut plain = world::world_carving(4, 400, 300);
    assert_eq!(plain.set_live_state(0, 0), Err(LiveError::NoCopy(0)));
    assert_eq!(plain.set_live_value(0, 0, 0), Err(LiveError::NoCopy(0)));
}

#[test]
fn live_state_is_in_the_checksum() {
    let w = live_world();
    let (sum, sync) = (w.checksum(), w.sync_checksum());
    assert_ne!(sum, world::world_carving(4, 400, 300).checksum());
    let mut a = w.clone();
    a.set_live_state(0, 1).unwrap();
    assert_ne!((a.checksum(), a.sync_checksum()), (sum, sync), "a state changes it");
    let mut b = w.clone();
    b.set_live_value(1, 1, 5 * PX + 1).unwrap();
    assert_ne!((b.checksum(), b.sync_checksum()), (sum, sync), "a value changes it");
    assert_ne!(a.checksum(), b.checksum());
    assert_eq!(w.clone().checksum(), sum);
}

#[test]
fn a_restore_puts_the_live_state_back() {
    let mut w = live_world();
    w.enable_rollback();
    let mut ev = TickEvents::new();
    for t in 0..20 {
        w.tick(&keys(t, 4), &mut ev);
    }
    let snap = w.save();
    let before = w.clone();
    w.set_live_state(0, 1).unwrap();
    w.set_live_value(1, 0, -7 * PX).unwrap();
    for t in 20..40 {
        w.tick(&keys(t, 4), &mut ev);
    }
    w.restore(&snap).unwrap();
    assert_eq!(w, before);
    assert_eq!(values(&w, 0), [0x00_ff00, 50]);
    assert_eq!(w.sync_checksum(), before.sync_checksum());
}

#[test]
fn live_state_round_trips_through_a_state_transfer() {
    let mut src = live_world();
    src.set_live_state(0, 1).unwrap();
    src.set_live_state(2, 1).unwrap();
    src.set_live_value(1, 0, -3 * PX - 17).unwrap();
    let mut ev = TickEvents::new();
    for t in 0..60 {
        src.tick(&keys(t, 4), &mut ev);
    }
    let mut dst = live_world();
    dst.load_state(WorldState::decode(&src.state().encode()).unwrap()).unwrap();
    assert_eq!(dst, src);
    assert_eq!(dst.checksum(), src.checksum());
    assert_eq!(dst.live().unwrap().copy(2).unwrap().state_name(), "Wrecked");
    assert_eq!(values(&dst, 1), values(&src, 1));
}
