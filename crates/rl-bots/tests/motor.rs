//! The motor drives a real worm: OLX's bot turning speed and weapon
//! switching through the keys a player has (notes §4.4).

use crate::common;

use common::*;
use rl_bots::{Intent, Motor};
use rl_sim::{Dir, TickEvents, World};

/// Drives worm 0 with `intent` for `n` ticks (or until `done`, if `stop`);
/// gives the ticks after which `done` first held (or `None`) and the
/// largest angle speed seen.
fn drive_for(
    w: &mut World,
    m: &mut Motor,
    intent: Intent,
    n: u32,
    stop: bool,
    done: impl Fn(&World) -> bool,
) -> (Option<u32>, f32) {
    let mut out = TickEvents::new();
    let mut first = None;
    let mut fastest = 0.0f32;
    for i in 0..n {
        let k = m.keys(&w.worms()[0], w.settings(), &intent);
        press(w, &mut out, k);
        fastest = fastest.max(w.worms()[0].angle_speed.abs());
        if first.is_none() && done(w) {
            first = Some(i + 1);
            if stop {
                break;
            }
        }
    }
    (first, fastest)
}

fn drive(
    w: &mut World,
    m: &mut Motor,
    intent: Intent,
    n: u32,
    done: impl Fn(&World) -> bool,
) -> (Option<u32>, f32) {
    drive_for(w, m, intent, n, false, done)
}

#[test]
fn motor_reaches_target_angle_like_olx_turning() {
    let mut w = world(arena(), &[(100, 140)], 1);
    idle(&mut w, &mut TickEvents::new(), 50);
    assert_eq!(w.worms()[0].angle, 0.0);
    let mut m = Motor::new();
    // OLX's bot turns at `BotAngleSpeed` 100°/s (91°/s in practice: 10 ms a
    // call, a call per 11 ms step), so 60° takes it about 0.66 s. The motor
    // cruises at 100°/s, with 0.2 s to speed up and to brake.
    let near = |target: f32| move |w: &World| (w.worms()[0].angle - target).abs() < 1.0;
    let (t, fastest) =
        drive(&mut w, &mut m, Intent { aim: Some(-60.0), ..Intent::default() }, 150, near(-60.0));
    let t = t.expect("reached -60°");
    assert!((60..=90).contains(&t), "60° in {t} ticks");
    assert!((95.0..=110.0).contains(&fastest), "top speed {fastest}°/s");
    // It stays there, still.
    let (t2, _) = drive(&mut w, &mut m, Intent { aim: Some(-60.0), ..Intent::default() }, 50, |_| true);
    assert_eq!(t2, Some(1));
    let worm = &w.worms()[0];
    assert!(
        (worm.angle + 60.0).abs() < 1.0 && worm.angle_speed == 0.0,
        "{} {}",
        worm.angle,
        worm.angle_speed
    );
    // Small turns are quick and do not overshoot.
    let (t, _) = drive(&mut w, &mut m, Intent { aim: Some(-55.0), ..Intent::default() }, 60, near(-55.0));
    assert!(t.is_some_and(|t| t <= 25), "{t:?}");
    assert!((w.worms()[0].angle + 55.0).abs() < 1.0);
    // The whole range, down to the lowest aim (60°).
    let (t, _) = drive(&mut w, &mut m, Intent { aim: Some(60.0), ..Intent::default() }, 200, near(60.0));
    assert!(t.is_some_and(|t| t <= 160), "115° in {t:?} ticks");
}

#[test]
fn motor_switches_weapons_in_two_ticks_per_slot() {
    let mut w = world(arena(), &[(100, 140)], 1);
    idle(&mut w, &mut TickEvents::new(), 50);
    let x = w.worms()[0].pos.x;
    let mut m = Motor::new();
    let slot = |n: i32| move |w: &World| w.worms()[0].current_weapon == n;
    // Two slots up (RIGHT, RIGHT): a press, a release, a press. Walking
    // waits, since CHANGE is held.
    let want = |s: usize| Intent { weapon: Some(s), walk: Some(Dir::Right), ..Intent::default() };
    let (t, _) = drive_for(&mut w, &mut m, want(2), 20, true, slot(2));
    assert!(t.is_some_and(|t| (3..=5).contains(&t)), "0 -> 2 in {t:?} ticks");
    // 2 -> 4 goes up (2 presses), 4 -> 0 wraps up (1 press), 0 -> 3 goes
    // down (LEFT, 2 presses).
    for (to, most) in [(4, 5), (0, 3), (3, 5)] {
        let (t, _) = drive_for(&mut w, &mut m, want(to), 20, true, slot(to as i32));
        assert!(t.is_some_and(|t| t <= most), "to {to} in {t:?} ticks");
    }
    assert_eq!(w.worms()[0].pos.x, x, "no walking while switching");
    // Once switched, the walk goes on.
    drive(&mut w, &mut m, want(3), 20, |_| true);
    assert!(w.worms()[0].pos.x > x + 1.0);
}
