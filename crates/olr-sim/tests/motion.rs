//! Issue #89: live values moving at a speed, started by a director made
//! here: hashed, restored, transferred and checked, and stepped once a
//! tick before the physics.

use olr_sim::{
    Director, EventKind, Keys, LIVE_SUBPIXELS, LiveCommand, LiveCtx, LiveMotion, MAX_MOTION_SPEED,
    StateError, TickEvents, World, WorldState,
};

use crate::director::scripted;

/// The press (copy 1): its `x` (prop 1, -100 to 100 px) sweeps end to end
/// at 3 px a tick, a new motion each time the last arrived; its `head_y`
/// (prop 0) jumps 2 px at the fastest speed every 7 ticks.
#[derive(Clone, Debug)]
struct Sweeper;

const PRESS: u16 = 1;

impl Director for Sweeper {
    fn box_clone(&self) -> Box<dyn Director> {
        Box::new(Sweeper)
    }

    fn timer_functions(&self, _script: u16) -> u16 {
        0
    }

    fn run(&self, ctx: &mut LiveCtx<'_>) {
        let press = ctx.view().copy(PRESS).unwrap();
        let (x, moving, head) = (press.values()[1], press.motions()[1].moving(), press.values()[0]);
        if !moving {
            let target = if x <= 0 { 100 } else { -100 } * LIVE_SUBPIXELS;
            ctx.apply(LiveCommand::Move { copy: PRESS, prop: 1, target, speed: 3 * LIVE_SUBPIXELS });
        }
        if ctx.frame().is_multiple_of(7) {
            let target = if head <= 0 { head + 2 * LIVE_SUBPIXELS } else { head - 2 * LIVE_SUBPIXELS };
            ctx.apply(LiveCommand::Move { copy: PRESS, prop: 0, target, speed: i64::MAX });
        }
    }
}

fn keys() -> Vec<Keys> {
    vec![Keys(Keys::FIRE | Keys::RIGHT); 4]
}

fn fresh() -> World {
    let live = scripted(&crate::live_level::live_level());
    crate::world::world_directed(4, 400, 300, live, Box::new(Sweeper))
}

fn tick(w: &mut World, n: u32) -> Vec<u64> {
    let mut ev = TickEvents::new();
    (0..n)
        .map(|_| {
            w.tick(&keys(), &mut ev);
            w.checksum()
        })
        .collect()
}

fn motion(w: &World, prop: usize) -> LiveMotion {
    w.live().unwrap().copy(PRESS).unwrap().motions()[prop]
}

/// The flat index of the press's `x`, `head_y`, and the console's colour.
const PRESS_X: usize = 3;
const PRESS_HEAD: usize = 2;
const CONSOLE_COLOUR: usize = 0;

#[test]
fn motions_are_in_the_checksum_and_the_transfer() {
    let mut w = fresh();
    tick(&mut w, 30);
    let m = motion(&w, 1);
    assert!(m.moving() && m.speed == 3 * LIVE_SUBPIXELS, "{m:?}");
    // The fastest speed clamps `i64::MAX`: the head (Down, 10 px) moved
    // at frame 7 is at 8 px a tick later.
    let mut s = fresh();
    tick(&mut s, 7);
    assert_eq!(motion(&s, 0), LiveMotion { target: 8 * LIVE_SUBPIXELS, speed: MAX_MOTION_SPEED });
    tick(&mut s, 1);
    assert_eq!(s.live().unwrap().copy(PRESS).unwrap().values()[0], 8 * LIVE_SUBPIXELS);
    assert!(!motion(&s, 0).moving());

    // Rollback and the transfer keep motions.
    w.enable_rollback();
    let snap = w.save();
    let first = tick(&mut w.clone(), 300);
    w.restore(&snap).unwrap();
    assert_eq!(tick(&mut w.clone(), 300), first);
    let bytes = w.state().encode();
    let mut dst = fresh();
    dst.load_state(WorldState::decode(&bytes).unwrap()).unwrap();
    assert_eq!(dst.checksum(), w.checksum());
    assert_eq!(tick(&mut dst, 300), tick(&mut w, 300));

    // A motion is in the checksum: another target, another speed.
    let src = {
        let mut w = fresh();
        tick(&mut w, 30);
        w
    };
    for m in [LiveMotion { target: 0, speed: 1 }, LiveMotion { target: -50 * LIVE_SUBPIXELS, speed: 3 * 256 }]
    {
        let mut s = src.state();
        s.live_mut_for_test().as_mut().unwrap().edit_motion_for_test(PRESS_X, m);
        let mut other = fresh();
        other.load_state(s).unwrap();
        assert_ne!(other.checksum(), src.checksum(), "{m:?}");
    }

    // Hostile motions are refused, changing nothing.
    let s = src.state();
    for (i, m) in [
        (PRESS_X, LiveMotion { target: 5, speed: 0 }),
        (PRESS_X, LiveMotion { target: 0, speed: -1 }),
        (PRESS_X, LiveMotion { target: 0, speed: MAX_MOTION_SPEED + 1 }),
        (PRESS_X, LiveMotion { target: 101 * LIVE_SUBPIXELS, speed: 1 }),
        (PRESS_HEAD, LiveMotion { target: -21 * LIVE_SUBPIXELS, speed: 1 }),
        (CONSOLE_COLOUR, LiveMotion { target: 0, speed: 1 }),
    ] {
        let mut s = s.clone();
        s.live_mut_for_test().as_mut().unwrap().edit_motion_for_test(i, m);
        let mut w = fresh();
        let before = w.clone();
        assert_eq!(w.load_state(s), Err(StateError::Live), "{i} {m:?}");
        assert_eq!(w, before);
    }
    // A world without scripts takes no motion.
    let plain = crate::world::world_live(4, 400, 300, crate::live_level::live_level());
    let mut s = plain.state();
    s.live_mut_for_test().as_mut().unwrap().edit_motion_for_test(PRESS_X, LiveMotion { target: 0, speed: 1 });
    let mut w = plain.clone();
    assert_eq!(w.load_state(s), Err(StateError::Live));
    assert_eq!(w, plain);
}

#[test]
fn motion_runs_before_the_first_substep() {
    let mut w = fresh();
    let mut ev = TickEvents::new();
    let (mut arrivals, mut still_frames, mut physics_after) = (0, 0, 0);
    for _ in 0..400 {
        let before = w.live().unwrap().copy(PRESS).unwrap().values()[1];
        let speed = motion(&w, 1).speed;
        w.tick(&keys(), &mut ev);
        let after = w.live().unwrap().copy(PRESS).unwrap().values()[1];
        // Once a tick, whatever the sub-steps: a tick without one moves
        // the value all the same.
        if speed != 0 {
            assert!((after - before).abs() <= speed && after != before, "{before} {after} {speed}");
            if w.substeps_last_tick() == 0 {
                still_frames += 1;
            }
        }
        let list: Vec<_> = ev.iter().collect();
        for (k, e) in list.iter().enumerate() {
            if let EventKind::LiveArrived { copy, .. } = e.kind {
                assert_eq!(copy, PRESS);
                // Before the first sub-step's entity pass: nothing of a
                // later pass comes before it.
                assert_eq!(e.substep, 0);
                assert!(list[..k].iter().all(|p| p.substep == 0));
                physics_after += list[k..].iter().filter(|p| p.substep > 0).count();
                arrivals += 1;
            }
        }
    }
    assert!(arrivals >= 10, "{arrivals} arrivals");
    assert!(physics_after > 0, "sub-steps' events follow an arrival");
    assert!(still_frames > 0, "ticks without a sub-step still moved the press");
}
