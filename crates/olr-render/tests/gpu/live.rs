//! Live copies drawn by state (issue #87): composited into the terrain
//! the renderer uploads, so their glow lights the level by their own groups
//! and their rock parts stop the glow light.

use std::sync::Arc;

use olr_look::{Animation, GlowColour, GlowGroup, LookFrame, Reach};
use olr_terrain::{LiveArt, LiveFrame, LivePicture, LivePlaced};

use super::*;

/// A lamp's picture: 4 × 4 world pixels of rock (art scale 1), glowing
/// full in its copy's group `group` (the run-time value).
fn lamp_picture(group: u8) -> LivePicture {
    LivePicture {
        size: (4, 4),
        art: [255, 240, 200, 255].repeat(16),
        rock: Some(vec![1; 16]),
        glow: Some(vec![255; 16]),
        groups: Some(vec![group; 16]),
    }
}

fn placed(at: (i32, i32)) -> LivePlaced {
    LivePlaced { picture: 0, rock: true, at, flip: false, alpha: 255, glow: 100 }
}

/// The lamp's group: white at `intensity` percent, reach 60.
fn lamp_group(intensity: u16) -> GlowGroup {
    GlowGroup {
        id: "lamp".into(),
        colour: GlowColour::Tint([255, 255, 255]),
        intensity,
        reach: Reach::Pixels(60),
        animation: Animation::Steady,
        phase_ms: None,
    }
}

/// Two copies of one lamp asset in a 200 × 40 room with a rock floor, at
/// x 30 and 166: the level has no groups of its own, so copy 0's group is
/// value 1, copy 1's value 2 (call 13).
#[test]
fn a_live_lamp_lights_the_level_by_its_own_group() {
    let Some(gpu) = offscreen() else { return };
    let mut map = room(200, 40, |_, y| y >= 36);
    map.set_live_art(Some(Arc::new(LiveArt { copies: vec![vec![lamp_picture(1)], vec![lamp_picture(2)]] })));
    assert!(map.glow().is_some(), "a level lit only by live copies gets a glow channel");
    let mut frame = LiveFrame::default();
    for at in [(30, 20), (166, 20)] {
        frame.push(placed(at));
        frame.end_copy();
    }
    map.set_live(&frame);
    let look = |second: u16| {
        let mut l = LookFrame::default();
        l.begin(None, 0.0);
        l.ambient = [0.1; 3];
        l.push_live_group(&lamp_group(100), "lamp-a", 0.0);
        l.push_live_group(&lamp_group(second), "lamp-b", 0.0);
        l
    };
    let view = View { x: 0.0, y: 0.0, w: 200.0, h: 40.0 };
    let r = glow_frame(&gpu, &map, view, &look(100));
    let (a, b) = (light_at(&gpu, &r, 32, 10)[0], light_at(&gpu, &r, 168, 10)[0]);
    assert!(a > 0.15 && b > 0.15, "both lamps light the room: {a}, {b}");
    // Copy 1's group dims: the light near it falls, near copy 0 it stays.
    let r = glow_frame(&gpu, &map, view, &look(0));
    let (a2, b2) = (light_at(&gpu, &r, 32, 10)[0], light_at(&gpu, &r, 168, 10)[0]);
    assert!((a2 - a).abs() < 2e-3, "copy 0's light unchanged: {a} then {a2}");
    assert!(b2 < b - 0.04 && (b2 - 0.1).abs() < 0.02, "copy 1's gone: {b} then {b2}");
}

/// A glowing strip on the floor's left, and a live rock wall (2 × 36) at
/// x 50: behind it only the ambient, as behind a wall of the ground.
#[test]
fn a_live_rock_part_occludes_the_glow_light() {
    let Some(gpu) = offscreen() else { return };
    let wall = LivePicture {
        size: (2, 36),
        art: [90, 90, 90, 255].repeat(72),
        rock: Some(vec![1; 72]),
        glow: None,
        groups: None,
    };
    let mut map = glowing_room(100, 40, |_, y| y >= 36, |x, y| y >= 36 && x < 40);
    map.set_live_art(Some(Arc::new(LiveArt { copies: vec![vec![wall]] })));
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    let r = glow_frame(&gpu, &map, view, &glow_look(1.0, 200.0));
    let open = light_at(&gpu, &r, 75, 20)[0];
    assert!(open > 0.11, "without the part the strip lights the far side: {open}");
    let mut frame = LiveFrame::default();
    frame.push(placed((50, 0)));
    frame.end_copy();
    map.set_live(&frame);
    let r = glow_frame(&gpu, &map, view, &glow_look(1.0, 200.0));
    assert!(light_at(&gpu, &r, 30, 30)[0] > 0.15, "before it: lit");
    assert!((light_at(&gpu, &r, 75, 20)[0] - 0.1).abs() < 0.01, "behind it: about the ambient");
}
