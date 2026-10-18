//! The built-in textures (plan 14e Task 1, ruling R1).

use olr_data::image::{Rgba, RgbaImage};
use olr_edit::textures::{BUILTIN_SIDE, builtin};

#[test]
fn builtin_textures_are_six_opaque_tiles() {
    let set = builtin();
    let names: Vec<_> = set.iter().map(|t| t.name).collect();
    assert_eq!(names, ["Dirt", "Rock", "Grass", "Brick", "Metal", "Wood"]);
    for t in &set {
        assert_eq!((t.image.width(), t.image.height()), (BUILTIN_SIDE, BUILTIN_SIDE), "{}", t.name);
        assert!(t.image.as_raw().chunks(4).all(|p| p[3] == 255), "{} is not opaque", t.name);
    }
}

#[test]
fn builtin_textures_are_deterministic_and_distinct() {
    let (a, b) = (builtin(), builtin());
    for (x, y) in a.iter().zip(&b) {
        assert!(x.image == y.image, "{} differs between calls", x.name);
    }
    for (i, x) in a.iter().enumerate() {
        for y in &a[i + 1..] {
            assert!(x.image != y.image, "{} equals {}", x.name, y.name);
        }
        let mut colours: Vec<_> = x.image.as_raw().chunks(4).map(|p| [p[0], p[1], p[2]]).collect();
        colours.sort_unstable();
        colours.dedup();
        assert!(colours.len() >= 8, "{} has {} colours", x.name, colours.len());
    }
}

/// The largest difference of red, green and blue between two pixels.
fn step(a: Rgba, b: Rgba) -> u8 {
    [a.r.abs_diff(b.r), a.g.abs_diff(b.g), a.b.abs_diff(b.b)].into_iter().max().unwrap_or(0)
}

#[test]
fn builtin_textures_tile() {
    let get = |img: &RgbaImage, x: u32, y: u32, transpose: bool| {
        if transpose { img.get(y, x) } else { img.get(x, y) }
    };
    for t in builtin() {
        let n = BUILTIN_SIDE;
        for transpose in [false, true] {
            for row in 0..n {
                let inside = (0..n - 1)
                    .map(|i| step(get(&t.image, i, row, transpose), get(&t.image, i + 1, row, transpose)))
                    .max()
                    .unwrap_or(0);
                let seam = step(get(&t.image, n - 1, row, transpose), get(&t.image, 0, row, transpose));
                assert!(
                    seam <= inside,
                    "{}: the seam of line {row} (transposed {transpose}) is {seam} > {inside}",
                    t.name
                );
            }
        }
    }
}

/// Minors: every built-in texture's generator repeats every
/// `BUILTIN_SIDE` pixels both ways, exactly.
#[test]
fn builtin_generators_are_periodic() {
    use olr_edit::textures::builtin_texel;
    for (i, t) in builtin().iter().enumerate() {
        for (y, x) in [(0, 64), (5, 130), (70, 3), (200, 199), (63, 127)] {
            assert_eq!(builtin_texel(i, x, y), Some(t.image.get(x % 64, y % 64)), "{} at ({x}, {y})", t.name);
        }
    }
    assert_eq!(builtin_texel(6, 0, 0), None);
}

/// Review of the minors: the generators take any coordinate.
#[test]
fn builtin_texels_take_any_coordinate() {
    use olr_edit::textures::builtin_texel;
    for i in 0..6 {
        for (x, y) in [(u32::MAX, 0), (0, u32::MAX), (u32::MAX, u32::MAX), (u32::MAX - 7, 9)] {
            assert!(builtin_texel(i, x, y).is_some());
        }
    }
}
