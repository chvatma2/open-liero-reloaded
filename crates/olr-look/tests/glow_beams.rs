//! Glow beams from directional glow patches.

use olr_look::glow_beams::{BeamLayer, GlowBeamSource, derive};
use olr_look::srgb_to_linear;

/// A small layer: art scale 1, white art, glow 255 on every pixel.
struct L {
    world: (u32, u32),
    groups: Vec<u8>,
    glow: Vec<u8>,
    art: Vec<u8>,
    beams: Vec<u8>,
}

impl L {
    fn new(w: u32, h: u32) -> L {
        let n = (w * h) as usize;
        L {
            world: (w, h),
            groups: vec![0; n],
            glow: vec![255; n],
            art: vec![255; n * 4],
            beams: vec![0; n * 2],
        }
    }
    fn set(&mut self, x: u32, y: u32, group: u8, deg: u16, spread: u8) {
        let i = (y * self.world.0 + x) as usize;
        self.groups[i] = group;
        self.beams[i * 2] = (deg / 2) as u8 + 1;
        self.beams[i * 2 + 1] = spread;
    }
    fn layer(&self) -> BeamLayer<'_> {
        BeamLayer {
            world: self.world,
            art_scale: 1,
            groups: &self.groups,
            glow: &self.glow,
            art: &self.art,
            beams: &self.beams,
        }
    }
}

fn front_only(l: &L) -> Vec<GlowBeamSource> {
    let none = L::new(l.world.0, l.world.1);
    derive(&l.layer(), &none.layer())
}

fn column(l: &mut L, x: u32, deg: u16) {
    for y in 20..25 {
        l.set(x, y, 1, deg, 15);
    }
}

#[test]
fn a_patch_is_one_beam_as_wide_as_it() {
    let mut l = L::new(32, 32);
    column(&mut l, 10, 0);
    let b = front_only(&l);
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].at, [11.0, 22.5]);
    assert_eq!(b[0].dir, [1.0, 0.0]);
    assert_eq!(b[0].half, 2.5);
    assert_eq!((b[0].group, b[0].first, b[0].back), (1, 20 * 32 + 10, false));
    assert!((b[0].spread - 15f32.to_radians()).abs() < 1e-6);
}

#[test]
fn a_tilted_patch_starts_flush() {
    let mut l = L::new(32, 32);
    column(&mut l, 10, 166);
    let b = &front_only(&l)[0];
    let reach = b.at[0] * b.dir[0] + b.at[1] * b.dir[1];
    let mut best = f32::NEG_INFINITY;
    for y in 20..25 {
        for (cx, cy) in [(10.0, y as f32), (11.0, y as f32), (10.0, y as f32 + 1.0), (11.0, y as f32 + 1.0)] {
            let d = cx * b.dir[0] + cy * b.dir[1];
            assert!(d <= reach + 1e-4, "{d} > {reach}");
            best = best.max(d);
        }
    }
    assert!((best - reach).abs() <= 1e-4, "{best} {reach}");
}

#[test]
fn different_directions_are_different_patches() {
    let mut l = L::new(32, 32);
    column(&mut l, 10, 0);
    column(&mut l, 11, 180);
    assert_eq!(front_only(&l).len(), 2);
}

#[test]
fn different_groups_are_different_patches() {
    let mut l = L::new(32, 32);
    column(&mut l, 10, 0);
    l.set(10, 22, 2, 0, 15);
    assert_eq!(front_only(&l).len(), 3);
}

#[test]
fn diagonal_pixels_join() {
    let mut l = L::new(8, 8);
    l.set(1, 1, 1, 90, 20);
    l.set(2, 2, 1, 90, 20);
    l.set(3, 3, 1, 90, 20);
    assert_eq!(front_only(&l).len(), 1);
    l.set(5, 1, 1, 90, 20);
    assert_eq!(front_only(&l).len(), 2);
}

#[test]
fn the_beam_takes_the_patchs_mean_glow_and_colour() {
    let mut l = L::new(8, 8);
    l.set(1, 1, 1, 0, 10);
    l.set(2, 1, 1, 0, 10);
    l.glow[9] = 128;
    l.glow[10] = 255;
    for i in [9usize, 10] {
        l.art[i * 4..i * 4 + 3].copy_from_slice(&[200, 100, 50]);
    }
    let b = &front_only(&l)[0];
    assert!((b.glow - 0.75).abs() < 0.002, "{}", b.glow);
    let top = srgb_to_linear(200);
    let want = [1.0, srgb_to_linear(100) / top, srgb_to_linear(50) / top];
    for (g, w) in b.colour.iter().zip(want) {
        assert!((g - w).abs() < 1e-5, "{:?}", b.colour);
    }
    // Black art gives zeros.
    l.art.iter_mut().for_each(|b| *b = 0);
    assert_eq!(front_only(&l)[0].colour, [0.0; 3]);
}

#[test]
fn the_order_is_row_major_front_first() {
    let mut f = L::new(16, 16);
    f.set(9, 2, 1, 0, 10);
    f.set(1, 5, 1, 0, 10);
    f.set(4, 2, 2, 90, 10);
    let mut k = L::new(16, 16);
    k.set(0, 0, 1, 0, 10);
    let a = derive(&f.layer(), &k.layer());
    assert_eq!(a, derive(&f.layer(), &k.layer()));
    let order: Vec<(u32, bool)> = a.iter().map(|b| (b.first, b.back)).collect();
    assert_eq!(order, [(2 * 16 + 4, false), (2 * 16 + 9, false), (5 * 16 + 1, false), (0, true)]);
}

#[test]
fn undirected_pixels_make_no_beam() {
    let mut l = L::new(8, 8);
    l.groups.iter_mut().for_each(|g| *g = 1);
    assert!(front_only(&l).is_empty());
    // A direction without a group is none either.
    let mut l = L::new(8, 8);
    l.set(1, 1, 0, 0, 10);
    assert!(front_only(&l).is_empty());
}

#[test]
fn a_misfit_layer_makes_none() {
    let mut l = L::new(8, 8);
    l.set(1, 1, 1, 0, 10);
    let good = l.layer();
    for bad in [
        BeamLayer { groups: &l.groups[1..], ..good },
        BeamLayer { glow: &l.glow[1..], ..good },
        BeamLayer { art: &l.art[1..], ..good },
        BeamLayer { beams: &l.beams[1..], ..good },
        BeamLayer { art_scale: 0, ..good },
        BeamLayer { art_scale: 2, ..good },
        BeamLayer { world: (u32::MAX, u32::MAX), ..good },
    ] {
        assert!(derive(&bad, &bad).is_empty());
    }
}
