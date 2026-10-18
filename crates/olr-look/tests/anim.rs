//! Animation sampling (spec §Animation).

use olr_look::{Animation, Key, PulseShape, default_phase};

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn pulse(shape: PulseShape) -> Animation {
    Animation::Pulse { period_ms: 1000, low: 60, high: 100, shape }
}

#[test]
fn pulse_sine_at_quarter_points() {
    let a = pulse(PulseShape::Sine);
    for (t, want) in [(0.0, 0.80), (250.0, 1.00), (500.0, 0.80), (750.0, 0.60)] {
        let got = a.sample(t, 0, 7).scale;
        assert!(near(got, want), "t {t}: {got} != {want}");
    }
}

#[test]
fn square_and_triangle_shapes() {
    let sq = pulse(PulseShape::Square);
    assert!(near(sq.sample(100.0, 0, 0).scale, 1.0));
    assert!(near(sq.sample(600.0, 0, 0).scale, 0.6));
    let tri = pulse(PulseShape::Triangle);
    assert!(near(tri.sample(0.0, 0, 0).scale, 0.8));
    assert!(near(tri.sample(125.0, 0, 0).scale, 0.9));
    assert!(near(tri.sample(250.0, 0, 0).scale, 1.0));
    assert!(near(tri.sample(375.0, 0, 0).scale, 0.9));
    assert!(near(tri.sample(750.0, 0, 0).scale, 0.6));
}

fn keys(looped: bool) -> Animation {
    let k = |at_ms, level| Key { at_ms, level, colour: None };
    Animation::Keys { keys: vec![k(0, 100), k(150, 40), k(400, 0)], looped }
}

#[test]
fn keys_interpolate_and_end() {
    let once = keys(false);
    assert!(near(once.sample(75.0, 0, 0).scale, 0.70));
    assert!(near(once.sample(400.0, 0, 0).scale, 0.0));
    assert!(near(once.sample(1000.0, 0, 0).scale, 0.0));
    assert!(once.ended(401.0));
    assert!(!once.ended(399.0));
    let looped = keys(true);
    assert!(near(looped.sample(450.0, 0, 0).scale, looped.sample(50.0, 0, 0).scale));
    assert!(near(looped.sample(450.0, 0, 0).scale, 0.80));
    assert!(!looped.ended(1e9));
    assert!(!Animation::Steady.ended(1e9));
}

#[test]
fn keys_carry_colour() {
    let a = Animation::Keys {
        keys: vec![
            Key { at_ms: 0, level: 100, colour: Some([255, 0, 0]) },
            Key { at_ms: 100, level: 100, colour: Some([0, 0, 255]) },
            Key { at_ms: 200, level: 100, colour: None },
        ],
        looped: false,
    };
    assert_eq!(a.sample(0.0, 0, 0).colour, Some([255, 0, 0]));
    assert_eq!(a.sample(50.0, 0, 0).colour, Some([128, 0, 128]));
    // A segment with a key lacking a colour keeps the earlier key's colour.
    assert_eq!(a.sample(150.0, 0, 0).colour, Some([0, 0, 255]));
    assert_eq!(Animation::Steady.sample(5.0, 0, 0).colour, None);
}

#[test]
fn flicker_is_deterministic_and_bounded() {
    let a = Animation::Flicker { speed: 8, depth: 25 };
    let run = |id| (0..10_000).map(|i| a.sample(i as f64 * 1.7, 0, id).scale).collect::<Vec<_>>();
    let first = run(1);
    assert!(first.iter().all(|s| (0.75 - 1e-6..=1.0 + 1e-6).contains(s)), "out of bounds");
    assert_eq!(first, run(1));
    assert_ne!(first, run(2));
    // It moves: not one value throughout.
    assert!(first.iter().any(|s| (s - first[0]).abs() > 0.01));
}

#[test]
fn phase_shifts_time() {
    let a = pulse(PulseShape::Sine);
    for t in [0.0, 130.0, 777.0] {
        assert!(near(a.sample(t, 250, 0).scale, a.sample(t + 250.0, 0, 0).scale));
    }
}

#[test]
fn default_phase_spreads() {
    let mut phases: Vec<u32> = (0..10u64).map(default_phase).collect();
    assert!(phases.iter().all(|p| *p < 65_536));
    phases.sort_unstable();
    phases.dedup();
    assert_eq!(phases.len(), 10);
}

#[test]
fn hostile_animations_never_panic() {
    let zero = Animation::Pulse { period_ms: 0, low: 60, high: 100, shape: PulseShape::Sine };
    assert!(zero.sample(12.0, 0, 0).scale.is_finite());
    let empty = Animation::Keys { keys: vec![], looped: true };
    assert!(near(empty.sample(12.0, 0, 0).scale, 1.0));
    assert!(!empty.ended(12.0));
    let far = Animation::Keys {
        keys: vec![Key { at_ms: u32::MAX, level: 1000, colour: None }, Key { at_ms: 0, level: 0, colour: None }],
        looped: true,
    };
    assert!(far.sample(5.0, u32::MAX, u64::MAX).scale.is_finite());
    let flick = Animation::Flicker { speed: 0, depth: 65_535 };
    let s = flick.sample(f64::MAX, u32::MAX, 0).scale;
    assert!(s.is_finite() && s >= 0.0);
    for a in [pulse(PulseShape::Sine), keys(false), flick] {
        assert!(near(a.sample(f64::NAN, 0, 0).scale, 1.0));
        assert!(near(a.sample(f64::INFINITY, 0, 0).scale, 1.0));
    }
}
