//! Issue #58: OLX's ground contact lets a standing worm's `y` creep down
//! about 0.012 px a tick and pushes it back up a pixel once it crosses one
//! (`fixtures/oracle/fall`). Linear interpolation draws a grounded worm at
//! its whole-pixel `y`, as OLX does, so it neither bobs nor jumps.

use crate::common;

use olr_data::image::Rgba;
use olr_scene::{DrawList, Interpolation, Item, Layer, SceneOptions};

const LINEAR: SceneOptions = SceneOptions { interpolation: Interpolation::Linear, show_health_bars: false };

/// Worm 0's skin sprite's `y` and the view's corner in `list`.
fn drawn(list: &DrawList, assets: &olr_scene::SceneAssets) -> (f32, (f32, f32)) {
    let id = assets.worm(0).skin;
    let y = list
        .layer(Layer::Worms)
        .iter()
        .find_map(|it| match it {
            Item::Sprite(s) if s.atlas == id && s.tint == Rgba::rgb(255, 255, 255) => Some(s.pos[1]),
            _ => None,
        })
        .expect("worm 0 is drawn");
    (y, (list.view.x, list.view.y))
}

/// Plays `name`, drawing worm 0 at each of `alphas` after every frame in
/// `frames`: `(frame, alpha, drawn y, view corner, the world's y, on the
/// ground before and after)` in drawing order.
#[allow(clippy::type_complexity)]
fn frames(
    name: &str,
    frames: core::ops::RangeInclusive<i64>,
    alphas: &[f32],
) -> Option<Vec<(i64, f32, f32, (f32, f32), f32, bool, bool)>> {
    let content = common::content()?;
    let (mut out, mut list) = (Vec::new(), DrawList::new());
    common::play(&content, name, LINEAR, |world, frame, _, scene, assets| {
        if !frames.contains(&frame) {
            return;
        }
        let w = &world.worms()[0];
        let before = scene.prev().worms()[0].on_ground;
        for &alpha in alphas {
            scene.build(world, alpha, assets, &mut list);
            let (y, view) = drawn(&list, assets);
            out.push((frame, alpha, y, view, w.pos.y, before, w.on_ground));
        }
    });
    Some(out)
}

#[test]
fn a_standing_worm_draws_still() {
    let Some(drawn) = frames("fall", 62..=199, &[0.0, 0.5, 1.0]) else { return };
    assert_eq!(drawn.len(), 138 * 3);
    let (_, _, y0, view0, ..) = drawn[0];
    let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
    for &(frame, alpha, y, view, world_y, before, after) in &drawn {
        assert!(before && after, "frame {frame}: the worm stands on the ground");
        assert_eq!(
            (y, view),
            (y0, view0),
            "frame {frame} at alpha {alpha}: the worm and the camera stay still"
        );
        (lowest, highest) = (lowest.min(world_y), highest.max(world_y));
    }
    // The world's `y` does creep (the sawtooth the drawing hides).
    assert!(highest - lowest > 0.9, "the world's y spans only {lowest}..{highest}");
}

#[test]
fn landing_draws_no_jump() {
    let Some(drawn) = frames("fall", 55..=70, &[0.0, 0.5, 1.0]) else { return };
    assert!(drawn.iter().any(|d| !d.5 && d.6), "the worm lands within frames 55-70");
    for pair in drawn.windows(2) {
        let ((f0, a0, y0, ..), (f1, a1, y1, ..)) = (pair[0], pair[1]);
        let up = y0 - y1;
        assert!(up <= 1.0, "frame {f0} at {a0} to frame {f1} at {a1}: up {up} px");
        // In fact the landing never draws it moving up at all (exact, the
        // ground pushed it up a quarter pixel the tick after it landed).
        assert!(up <= 0.0, "frame {f0} at {a0} to frame {f1} at {a1}: up {up} px");
    }
}

#[test]
fn a_worm_walking_over_a_step_moves_at_most_a_pixel_a_tick() {
    // `dig` walks over the uneven floor of the tunnel it digs in Dirt Level.
    let Some(drawn) = frames("dig", 0..=359, &[0.0, 0.5]) else { return };
    for pair in drawn.windows(2) {
        let ((f0, a0, y0, ..), (f1, a1, y1, ..)) = (pair[0], pair[1]);
        assert!((y1 - y0).abs() <= 1.0, "frame {f0} at {a0} to frame {f1} at {a1}: {y0} to {y1}");
    }
    // Steps on the ground: a whole pixel over a tick, half of it at 0.5.
    let steps = drawn
        .windows(3)
        .filter(|w| w[0].1 == 0.0 && w[0].5 && w[0].6 && w[2].6)
        .filter(|w| (w[2].2 - w[0].2).abs() == 1.0 && w[1].2 == (w[0].2 + w[2].2) / 2.0)
        .count();
    assert!(steps > 0, "the worm never stepped a pixel on the ground");
}
