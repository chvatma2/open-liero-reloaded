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
