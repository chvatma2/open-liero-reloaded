//! Random edit sequences: undoing to any mark restores that moment exactly,
//! and the tracked cell hash always equals the hash from scratch.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_terrain::{GreenMask, Stamp, Terrain};
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Op {
    Carve(u32, i32, i32),
    Dirt(u32, i32, i32),
    Green(i32, i32),
    Mark,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (1u32..12, -10i32..70, -10i32..50).prop_map(|(n, x, y)| Op::Carve(n, x, y)),
        (1u32..12, -10i32..70, -10i32..50).prop_map(|(n, x, y)| Op::Dirt(n, x, y)),
        (-10i32..70, -10i32..50).prop_map(|(x, y)| Op::Green(x, y)),
        Just(Op::Mark),
    ]
}

fn square(n: u32) -> Stamp {
    let mut img = RgbaImage::new(n, n);
    for y in 0..n {
        for x in 0..n {
            img.set(x, y, if (x + y) % 3 == 0 { Rgba::BLACK } else { Rgba::MAGENTA });
        }
    }
    Stamp::from_image(&img)
}

fn green() -> GreenMask {
    let mut img = RgbaImage::new(5, 5);
    for y in 0..5 {
        for x in 0..5 {
            img.set(x, y, Rgba::rgb(0, 255, 0));
        }
    }
    GreenMask::from_image(&img)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn undo_to_any_mark_restores_it(ops in prop::collection::vec(op(), 1..40), pick in any::<prop::sample::Index>()) {
        let mut cells = vec![Material::Dirt; 60 * 40];
        for x in 0..60 { cells[20 * 60 + x] = Material::Rock; }
        let grid = MaterialGrid::from_cells(60, 40, cells).unwrap();
        let mut t = Terrain::from_grid(&grid);
        t.start_journal();
        let mut marks = vec![(t.journal_mark().unwrap(), t.clone())];
        let mut seed = 7i32;
        for o in &ops {
            match *o {
                Op::Carve(n, x, y) => { t.carve_hole(&square(n), x, y); }
                Op::Dirt(n, x, y) => { t.place_dirt(&square(n), x, y); }
                Op::Green(x, y) => { t.place_green_dirt(&green(), x, y, &mut || { seed = seed.wrapping_mul(31) + 1; seed & 3 }); }
                Op::Mark => marks.push((t.journal_mark().unwrap(), t.clone())),
            }
            let mut fresh = t.clone();
            fresh.stop_journal_for_tests();
            prop_assert_eq!(fresh.cell_hash(), t.cell_hash());
        }
        let (mark, at) = &marks[pick.index(marks.len())];
        t.undo_to(*mark).unwrap();
        prop_assert_eq!(&t, at);
        let mut fresh = t.clone();
        fresh.stop_journal_for_tests();
        prop_assert_eq!(fresh.cell_hash(), t.cell_hash());
    }
}
