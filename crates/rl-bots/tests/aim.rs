//! The ballistic solver against the simulation (spec §20 F2): a lobbed
//! shell aimed by the solver hits a worm standing still; OLX's direct aim
//! drops it short.

mod common;

use common::*;
use rl_bots::{Intent, Launch, Motor, ballistic_from, olx_direct};
use rl_math::Vec2;
use rl_sim::{Dir, EventKind, Keys, TickEvents, World};

/// Worm 0 turns to `aim` through the motor, fires the lob once, and waits
/// up to 3 s: whether worm 1 got hurt by worm 0.
fn shoot(w: &mut World, aim: (Dir, f32)) -> bool {
    let mut out = TickEvents::new();
    let mut m = Motor::new();
    let intent =
        Intent { aim: Some(aim.1), face: Some(aim.0), weapon: Some(LOB as usize), ..Intent::default() };
    for _ in 0..300 {
        let me = &w.worms()[0];
        if me.face == aim.0
            && (me.angle - aim.1).abs() < 0.5
            && me.angle_speed == 0.0
            && me.current_weapon == LOB
        {
            break;
        }
        let k = m.keys(me, w.settings(), &intent);
        press(w, &mut out, k);
    }
    let me = &w.worms()[0];
    assert!((me.angle - aim.1).abs() < 0.5, "aimed at {} for {}", me.angle, aim.1);
    press(w, &mut out, Keys(Keys::FIRE));
    for _ in 0..300 {
        idle(w, &mut out, 1);
        if out.iter().any(|e| matches!(e.kind, EventKind::Damage { worm: 1, by: 0, .. })) {
            return true;
        }
    }
    false
}

#[test]
fn ballistic_hits_static_target() {
    // A level target 150 px away, and one on a ledge 40 px up, 100 px away.
    let ledge = rocks(400, 200, |x, y| y >= 150 || (x >= 180 && y >= 110));
    for (terrain, at) in [(arena(), (250, 140)), (ledge, (200, 100))] {
        let fresh = || {
            let mut w = world(terrain.clone(), &[(100, 140), at], 3);
            idle(&mut w, &mut TickEvents::new(), 60);
            w
        };
        let mut w = fresh();
        let (me, target) = (w.worms()[0].pos, w.worms()[1].pos);
        let s = w.settings().clone();
        let g = 100.0 * s.proj_gravity_factor;
        let launch = Launch {
            speed: LOB_SPEED as f32,
            gravity: g,
            spawn_distance: s.shoot_spawn_distance,
            carried: Vec2::ZERO,
        };
        let aim = ballistic_from(me, target, &launch, &s).expect("in reach");
        assert!(aim.0 == Dir::Right && aim.1 < -10.0, "a lob: {aim:?}");
        assert!(shoot(&mut w, aim), "the solver's shot hits at {at:?}");
        // OLX aims straight at the target and lobs short.
        let mut w = fresh();
        assert!(!shoot(&mut w, olx_direct(me, target)), "the direct shot falls short at {at:?}");
    }
}
