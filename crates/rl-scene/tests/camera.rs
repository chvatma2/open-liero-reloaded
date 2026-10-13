//! OLX's `CViewport` for a local player: hard lock, clamp, shake.

use rl_math::{AbsTime, GlibcRand, Vec2};
use rl_scene::{Camera, View, Viewport};

const MAP: (u32, u32) = (504, 350);
const VP: Viewport = Viewport { width: 320, height: 240 };

fn view(c: &Camera) -> (f32, f32) {
    let View { x, y, .. } = c.view();
    (x, y)
}

#[test]
fn camera_locks_clamps_and_shakes_like_olx() {
    let t = AbsTime(100_000);
    let mut c = Camera::new(VP, 0, 7);

    // Hard lock: `(int)(pos - Width/2)`, no lag, no lead.
    c.process(Some(Vec2::new(200.7, 150.2)), true, MAP, t);
    assert_eq!(view(&c), (40.0, 30.0));
    assert_eq!(c.listener(), Vec2::new(200.0, 150.0));
    // Sub-pixel when interpolating.
    c.process(Some(Vec2::new(200.5, 150.25)), false, MAP, t);
    assert_eq!(view(&c), (40.5, 30.25));
    // Clamped to the map on every side.
    c.process(Some(Vec2::new(100.0, 50.0)), true, MAP, t);
    assert_eq!(view(&c), (0.0, 0.0));
    c.process(Some(Vec2::new(500.0, 349.0)), true, MAP, t);
    assert_eq!(view(&c), (184.0, 110.0));
    // A map smaller than the view sits at the top-left.
    c.process(Some(Vec2::new(500.0, 349.0)), true, (300, 200), t);
    assert_eq!(view(&c), (0.0, 0.0));
    // No target: the view stays.
    c.process(Some(Vec2::new(200.0, 150.0)), true, MAP, t);
    c.process(None, true, MAP, t);
    assert_eq!(view(&c), (40.0, 30.0));

    // `inView` truncates and includes the edges.
    assert!(c.in_view(Vec2::new(360.9, 270.0)));
    assert!(!c.in_view(Vec2::new(361.0, 30.0)));
    assert!(!c.in_view(Vec2::new(39.9, 30.0)));

    // Shake: a uniform `(int)(GetRandomNum() * amount)` per axis and frame
    // for 0.2 s after the last shake, drawn from the camera's own generator.
    let mut c = Camera::new(VP, 0, 7);
    let mut rng = GlibcRand::new(7);
    c.shake(4, t);
    c.shake(2, AbsTime(t.0 + 50)); // restarts the window; the amount only grows
    assert_eq!(c.shake_amount(), 4);
    let mut moved = false;
    for ms in [50, 100, 150, 250] {
        c.process(Some(Vec2::new(200.0, 150.0)), true, MAP, AbsTime(t.0 + ms));
        let dx = (rng.random_num() * 4.0) as i32 as f32;
        let dy = (rng.random_num() * 4.0) as i32 as f32;
        assert_eq!(view(&c), (40.0 + dx, 30.0 + dy), "at {ms} ms");
        moved |= dx != 0.0 || dy != 0.0;
    }
    assert!(moved);
    // Over 0.2 s after the last shake (strictly): still, and the amount resets.
    c.process(Some(Vec2::new(200.0, 150.0)), true, MAP, AbsTime(t.0 + 251));
    assert_eq!(view(&c), (40.0, 30.0));
    assert_eq!(c.shake_amount(), 0);

    // At a map edge the shake only goes inward (clamp, shake, clamp).
    let mut c = Camera::new(VP, 0, 3);
    c.shake(5, t);
    let mut inward = false;
    for ms in 0..=200 {
        c.process(Some(Vec2::new(0.0, 0.0)), true, MAP, AbsTime(t.0 + ms));
        let (x, y) = view(&c);
        assert!((0.0..=5.0).contains(&x) && (0.0..=5.0).contains(&y), "{x},{y}");
        inward |= x > 0.0 && y > 0.0;
    }
    assert!(inward, "a camera clamped at the corner still shakes");

    // The viewport: OLX's `round_even(480 * aspect)` screen width, halved,
    // at least 160; aspect clamped to 16:9 (spec §19 E4).
    let vp = |a: f32| Viewport::from_aspect(a, 16.0 / 9.0);
    assert_eq!(vp(4.0 / 3.0), Viewport { width: 320, height: 240 });
    assert_eq!(vp(16.0 / 10.0), Viewport { width: 384, height: 240 });
    assert_eq!(vp(16.0 / 9.0), Viewport { width: 427, height: 240 });
    assert_eq!(vp(21.0 / 9.0), Viewport { width: 427, height: 240 });
    assert_eq!(vp(0.5), Viewport { width: 160, height: 240 });
    assert_eq!(vp(f32::NAN), Viewport { width: 320, height: 240 });
}

/// The game shows every player the same world area, whatever the window
/// (the user's ruling after the v0.1 playtest): 16:9's width at
/// `FOV_HEIGHT`, and the 80 % of that height above the HUD bar, as OLX
/// 0.58's local game showed 191 world pixels above its 98-pixel bar.
#[test]
fn the_game_view_is_fixed() {
    let wide = Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0);
    assert_eq!(Viewport::GAME, Viewport { width: wide.width, height: 192 });
    assert_eq!(Viewport::GAME.height, rl_scene::FOV_HEIGHT * 4 / 5);
}

/// A restarted world's clock starts over (`SANDBOX_START`): a shake started
/// in the old world must not run on until the new clock reaches it.
#[test]
fn shake_ends_when_the_world_restarts() {
    use rl_scene::{Scene, SceneOptions};
    // The clock went back past the shake's start: the shake is over.
    let mut c = Camera::new(VP, 0, 7);
    c.shake(5, AbsTime(200_000));
    c.process(Some(Vec2::new(200.0, 150.0)), true, MAP, AbsTime(100_000));
    assert_eq!(view(&c), (40.0, 30.0));
    assert_eq!(c.shake_amount(), 0);
    // `Scene::reset` (a new or restarted world) stops the shake.
    let mut scene = Scene::new(Camera::new(VP, 0, 7), SceneOptions::default(), 1);
    scene.camera.shake(5, AbsTime(100_000));
    scene.reset();
    assert_eq!(scene.camera.shake_amount(), 0);
}

/// `shake_for` is `on_events` over any list of events (the online
/// presentation's explosions first seen in a re-simulated tick).
#[test]
fn shake_for_shakes_for_explosions_in_view() {
    use rl_sim::{Event, EventKind};
    let t = AbsTime(100_000);
    let explosion = |x: f32, shake| Event {
        substep: 0,
        death: false,
        kind: EventKind::Explosion { pos: Vec2::new(x, 50.0), damage: 1.0, shake },
    };
    let mut c = Camera::new(VP, 0, 7);
    c.process(Some(Vec2::new(0.0, 0.0)), true, MAP, t);
    c.shake_for(&[explosion(400.0, 3), explosion(50.0, 0)], t);
    assert_eq!(c.shake_amount(), 0, "out of view, or no shake");
    c.shake_for(&[explosion(50.0, 3)], t);
    assert_eq!(c.shake_amount(), 3);
}
