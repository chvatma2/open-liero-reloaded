//! Unit tests of the one terrain query (issue #90).
extern crate std;

use super::*;
use alloc::vec::Vec;
use olr_data::level::MaterialGrid;
use proptest::prelude::*;

fn material(i: u8) -> Material {
    match i % 3 {
        0 => Material::Background,
        1 => Material::Dirt,
        _ => Material::Rock,
    }
}

proptest! {
    /// Without live rock parts the query reads exactly the ground, inside
    /// the map and out.
    #[test]
    fn the_query_reads_the_ground_without_live_copies(
        w in 1u32..40,
        h in 1u32..40,
        cells in proptest::collection::vec(0u8..3, 1600),
        points in proptest::collection::vec((-10i64..50, -10i64..50), 1..64),
    ) {
        let cells: Vec<Material> = cells[..(w * h) as usize].iter().map(|&c| material(c)).collect();
        let t = Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).unwrap());
        let q = Solid::new(&t, None);
        for (x, y) in points {
            prop_assert_eq!(q.flags(x, y), t.flags(x, y));
            prop_assert_eq!(q.material(x, y), t.material(x, y));
            prop_assert!(!q.live_rock(x, y));
            prop_assert_eq!(Solidity::flags(&q, x, y), t.flags(x, y));
        }
        prop_assert_eq!((q.width(), q.height()), (t.width(), t.height()));
    }
}

/// The index finds a part through its tiles, only where its mask is,
/// mirrored when flipped, and follows a move.
#[test]
fn the_index_reads_a_part_through_its_tiles() {
    use crate::live::{LiveCopy, LiveCopySetup, LiveSolid};
    // An L: the left column and the bottom row of a 4 × 3 picture.
    let solid = LiveSolid::new((4, 3), true, |x, y| x == 0 || y == 2);
    let copy = LiveCopy::new(LiveCopySetup {
        id: "l".into(),
        at: (0, 0),
        size: (4, 3),
        props: Vec::new(),
        states: Vec::new(),
        pictures: alloc::vec![solid],
        start_state: 0,
        start_values: Vec::new(),
        health: None,
        scripts: Vec::new(),
        sounds: Vec::new(),
    });
    let level = LiveLevel::new(alloc::vec![copy], Vec::new(), None, 0);
    let mut index = SolidIndex::new((100, 100), true);
    let part = SolidPart {
        copy: 0,
        picture: 0,
        layer: None,
        part_at: (0, 0),
        at: (30, 62),
        size: (4, 3),
        flip: false,
    };
    assert!(index.push(part));
    assert!(index.rock(&level, 30, 62) && index.rock(&level, 33, 64));
    assert!(!index.rock(&level, 31, 62) && !index.rock(&level, 34, 64) && !index.rock(&level, 29, 62));
    index.move_part(0, (31, 62));
    assert!(!index.rock(&level, 30, 62) && index.rock(&level, 31, 62) && index.rock(&level, 34, 64));
    index.clear();
    assert!(!index.rock(&level, 31, 62));
    assert!(index.push(SolidPart { flip: true, ..part }));
    assert!(index.rock(&level, 33, 62) && !index.rock(&level, 30, 62) && index.rock(&level, 30, 64));
    // A part past the map's edge is read where it is inside.
    index.clear();
    assert!(index.push(SolidPart { at: (-2, 98), ..part }));
    assert!(index.rock(&level, 0, 98 + 2) && !index.rock(&level, -2, 98));
}
