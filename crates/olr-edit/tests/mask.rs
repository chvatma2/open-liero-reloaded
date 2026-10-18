//! Image masks (plan 14d Task 2): erasing an imported picture without
//! losing its pipeline.

use olr_data::image::{Rgba, RgbaImage};
use olr_edit::compose::Rect;
use olr_edit::level::{Shape, similar};
use olr_edit::mask::{Mask, mask_colour, mask_shape, original_pixel};

const RED: Rgba = Rgba::rgb(255, 0, 0);
const BLUE: Rgba = Rgba::rgb(0, 0, 255);

fn solid(w: u32, h: u32, c: Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    img
}

#[test]
fn a_mask_round_trips_and_applies() {
    let mut m = Mask::full((4, 3));
    m.keep[1] = 0;
    m.keep[4 * 2 + 3] = 0;
    let back = Mask::decode("editor/masks/m.png", &m.encode(), (4, 3)).unwrap();
    assert_eq!(back, m);
    let out = m.apply(&solid(4, 3, RED)).unwrap();
    let clear: Vec<(u32, u32)> =
        (0..3).flat_map(|y| (0..4).map(move |x| (x, y))).filter(|&(x, y)| out.get(x, y).a == 0).collect();
    assert_eq!(clear, [(1, 0), (3, 2)]);
    assert!(m.apply(&solid(5, 3, RED)).is_none());
    assert_ne!(m.fingerprint(), 0);
    assert_ne!(m.fingerprint(), Mask::full((4, 3)).fingerprint());
}

#[test]
fn mask_shape_maps_original_pixels_to_the_box() {
    let mut m = Mask::full((8, 8));
    let shape = Shape::Rect { x: 10, y: 10, w: 1, h: 1 };
    let r = mask_shape(&mut m, (10, 10), (4, 4), &shape, false);
    assert!(r.is_some());
    let erased: Vec<(u32, u32)> = (0..8)
        .flat_map(|y| (0..8).map(move |x| (x, y)))
        .filter(|&(x, y)| m.keep[(y * 8 + x) as usize] == 0)
        .collect();
    assert_eq!(erased, [(0, 0), (1, 0), (0, 1), (1, 1)]);
    mask_shape(&mut m, (10, 10), (4, 4), &shape, true);
    assert_eq!(m, Mask::full((8, 8)));
    assert_eq!(original_pixel((10, 10), (4, 4), (8, 8), (11, 10)), Some((3, 1)), "the pixel at its centre");
    assert_eq!(original_pixel((10, 10), (4, 4), (8, 8), (14, 10)), None);
}

#[test]
fn a_far_or_huge_shape_masks_within_the_box_only() {
    let mut m = Mask::full((8, 8));
    let far = Shape::Rect { x: i32::MIN, y: i32::MIN, w: i32::MAX, h: i32::MAX };
    assert_eq!(mask_shape(&mut m, (10, 10), (4, 4), &far, false), None);
    assert_eq!(m, Mask::full((8, 8)));
    let huge = Shape::Circle { cx: 0, cy: 0, r: i32::MAX };
    mask_shape(&mut m, (i32::MAX - 2, i32::MIN), (4096, 4096), &huge, false);
    assert_eq!(original_pixel((i32::MAX, i32::MAX), (4096, 4096), (8, 8), (i32::MIN, 0)), None);
}

#[test]
fn mask_colour_floods_similar_connected_pixels() {
    let row = [RED, RED, Rgba::rgb(250, 0, 0), BLUE, RED, RED];
    let mut img = RgbaImage::new(6, 1);
    for (x, c) in row.iter().enumerate() {
        img.set(x as u32, 0, *c);
    }
    let erased = |m: &Mask| m.keep.iter().map(|&k| k == 0).collect::<Vec<_>>();
    let mut m = Mask::full((6, 1));
    mask_colour(&mut m, &img, (0, 0), 0);
    assert_eq!(erased(&m), [true, true, false, false, false, false]);
    let mut m = Mask::full((6, 1));
    mask_colour(&mut m, &img, (0, 0), 5);
    assert_eq!(erased(&m), [true, true, true, false, false, false]);
    assert_eq!(mask_colour(&mut Mask::full((6, 1)), &img, (9, 0), 5), None);
}

#[test]
fn diff_crop_and_patch_restore_a_mask() {
    let before = Mask::full((5, 4));
    let mut after = before.clone();
    after.keep[5 + 1] = 0;
    after.keep[2 * 5 + 3] = 0;
    let r = after.diff(&before).unwrap();
    assert_eq!(r, Rect { x: 1, y: 1, w: 3, h: 2 });
    assert_eq!(before.diff(&before), None);
    assert_eq!(before.diff(&Mask::full((4, 4))), None);
    let mut m = after.clone();
    assert!(m.patch(r, &before.crop(r)));
    assert_eq!(m, before);
    assert!(!m.patch(Rect { x: 4, y: 0, w: 3, h: 1 }, &[0; 3]));
    assert_eq!(m, before);
}

#[test]
fn similar_follows_the_percentage_rule() {
    let black = Rgba::rgb(0, 0, 0);
    assert!(similar(black, Rgba::rgb(12, 0, 0), 5));
    assert!(!similar(black, Rgba::rgb(13, 0, 0), 5));
    assert!(similar(black, Rgba::rgb(255, 255, 255), 200));
    assert!(!similar(black, Rgba { a: 0, ..black }, 99));
}
