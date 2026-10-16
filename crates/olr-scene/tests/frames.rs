//! OLX's sprite frame math against hand-computed OLX values.

use olr_data::mod_def::{AnimKind, Animation, ProjectileGfx};
use olr_math::Vec2;
use olr_scene::{Rect, frame_x, framestep, skin_frame};
use olr_sim::{Dir, Projectile};

/// `CWorm::Draw`'s aim bucket `min((int)((angle + 90) / 151 * 7), 6)`,
/// worked out by hand: bucket k starts at `151 k / 7 - 90` degrees
/// (-68.43, -46.86, -25.29, -3.71, 17.86, 39.43; 61 would be 7).
const ANGLE_BUCKETS: [(f32, u32); 16] = [
    (-90.0, 0),
    (-69.0, 0),
    (-68.43, 0),
    (-68.42, 1),
    (-47.0, 1),
    (-46.0, 2),
    (-26.0, 2),
    (-25.0, 3),
    (-4.0, 3),
    (-3.0, 4),
    (0.0, 4),
    (17.0, 4),
    (18.0, 5),
    (39.0, 5),
    (40.0, 6),
    (60.0, 6),
];

#[test]
fn skin_frame_matches_olx_table() {
    for (angle, bucket) in ANGLE_BUCKETS {
        for walk in [0.0, 1.0, 1.99, 2.0, 2.9] {
            let f = (walk as u32) * 7 + bucket;
            let sx = f * 32 + 4;
            // Facing right: the 20 × 18 sprite at `32 f + 4`.
            let right = skin_frame(angle, Dir::Right, walk, 21);
            assert_eq!(right, (Rect { x: sx, y: 0, w: 20, h: 18 }, false), "angle {angle} walk {walk}");
            // Facing left: the exact mirror of the same columns (OLX's
            // one-column offset dropped, spec 2026-10-16 formats §4).
            let left = skin_frame(angle, Dir::Left, walk, 21);
            assert_eq!(left, (Rect { x: sx, y: 0, w: 20, h: 18 }, true), "angle {angle} walk {walk}");
        }
    }
    // A sheet with fewer frames clamps to its last frame.
    assert_eq!(skin_frame(60.0, Dir::Right, 2.0, 10).0.x, 9 * 32 + 4);
    // The last frame (20) facing left reads columns 644..664 of 672.
    assert_eq!(skin_frame(60.0, Dir::Left, 2.0, 21).0, Rect { x: 644, y: 0, w: 20, h: 18 });
    // Nonsense never panics.
    let _ = skin_frame(f32::NAN, Dir::Left, f32::INFINITY, 0);
}

fn image(
    rotating: bool,
    rot_increment: i32,
    use_angle: bool,
    use_spec_angle: bool,
    angle_images: i32,
) -> ProjectileGfx {
    ProjectileGfx::Image {
        file: "gfx/x.png".into(),
        rotating,
        rot_increment,
        rot_speed: 0,
        use_angle,
        use_spec_angle,
        angle_images,
        animation: None,
    }
}

fn proj(vel: Vec2, rotation: f32, frame: f32) -> Projectile {
    let mut p = Projectile::default();
    (p.vel, p.rotation, p.frame) = (vel, rotation, frame);
    p
}

/// The frame index drawn: `MOD((int)framestep * 8, 8 * frames)` / 8.
fn frame(gfx: &ProjectileGfx, p: &Projectile, bumped: bool, frames: u32) -> u32 {
    frame_x(framestep(gfx, p, bumped, true), 8, 8 * frames) / 8
}

#[test]
fn projectile_frames_match_olx() {
    let (up, down, left, right) =
        (Vec2::new(0.0, -10.0), Vec2::new(0.0, 10.0), Vec2::new(-10.0, 0.0), Vec2::new(10.0, 0.0));

    // Animation wins over everything: `fFrame`, truncated.
    let mut anim = image(true, 90, true, true, 7);
    if let ProjectileGfx::Image { animation, .. } = &mut anim {
        *animation = Some(Animation { rate: 10.0, kind: AnimKind::Loop });
    }
    assert_eq!(frame(&anim, &proj(right, 200.0, 2.7), false, 4), 2);

    // Special angle, 7 images (num 3): up is the middle image; `-atan2(x, y)`
    // is -180 up, 90 left, -90 right, -0 down (which counts as up).
    let spec = image(false, 0, false, true, 7);
    assert_eq!(frame(&spec, &proj(up, 0.0, 0.0), false, 7), 3);
    assert_eq!(frame(&spec, &proj(left, 0.0, 0.0), false, 7), 1); // (151 - 90) / 151 * 3 = 1.21
    assert_eq!(frame(&spec, &proj(right, 0.0, 0.0), false, 7), 5); // 90 / 151 * 3 + 4 = 5.79
    assert_eq!(frame(&spec, &proj(down, 0.0, 0.0), false, 7), 3);
    assert_eq!(frame(&spec, &proj(Vec2::new(-10.0, -10.0), 0.0, 0.0), false, 7), 2); // -atan2 is 135: (151 - 45) / 151 * 3 = 2.1

    // Angle images, 8 (45° each), clockwise from down: `FMOD(-atan2(x, y), 360)`.
    let angle = image(false, 0, true, false, 8);
    assert_eq!(frame(&angle, &proj(down, 0.0, 0.0), false, 8), 0);
    assert_eq!(frame(&angle, &proj(left, 0.0, 0.0), false, 8), 2);
    assert_eq!(frame(&angle, &proj(up, 0.0, 0.0), false, 8), 4);
    assert_eq!(frame(&angle, &proj(right, 0.0, 0.0), false, 8), 6);

    // Rotation: `rotation / RotIncrement`, only while moving.
    let rot = image(true, 90, false, false, 0);
    assert_eq!(frame(&rot, &proj(right, 200.0, 0.0), false, 4), 2);
    assert_eq!(frame(&rot, &proj(Vec2::ZERO, 200.0, 0.0), false, 4), 0);
    assert_eq!(frame(&image(false, 90, false, false, 0), &proj(right, 200.0, 0.0), false, 4), 0);

    // A zero RotIncrement draws unrotated until OLX's sticky 0 -> 1
    // mutation (`rot_bumped`), then frame `(int)rotation` (degrees!) modulo
    // the frame count: 201 % 4 = 1.
    let zero = image(true, 0, false, false, 0);
    assert_eq!(frame(&zero, &proj(right, 201.0, 0.0), false, 4), 0);
    assert_eq!(frame(&zero, &proj(right, 201.0, 0.0), true, 4), 1);

    // `MOD` never goes negative: frame -1 is the last one.
    assert_eq!(frame_x(-1.5, 8, 32), 24);
    // Off screen OLX draws frame 0; an empty image never divides by zero.
    assert_eq!(framestep(&rot, &proj(right, 200.0, 0.0), false, false), 0.0);
    assert_eq!(frame_x(3.0, 8, 0), 0);
    // A pixel projectile has no frames.
    let pixel = ProjectileGfx::Pixel { colours: vec![] };
    assert_eq!(framestep(&pixel, &proj(right, 200.0, 2.0), true, true), 0.0);
}
