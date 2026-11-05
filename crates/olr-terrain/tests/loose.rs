//! Loose dirt's kind layer (the loose dirt design): kept with the cells,
//! journaled with them, cleared by every edit that changes a cell.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_terrain::{Area, Stamp, Terrain};

fn dirt(w: u32, h: u32) -> Terrain {
    Terrain::from_grid(&MaterialGrid::new(w, h, Material::Dirt))
}

fn loose(w: u32, h: u32, kind: u8) -> Terrain {
    dirt(w, h).with_loose(vec![kind; (w * h) as usize]).expect("one kind a cell")
}

fn dot() -> Stamp {
    let mut img = RgbaImage::new(1, 1);
    img.set(0, 0, Rgba::MAGENTA);
    Stamp::from_image(&img)
}

#[test]
fn a_carve_clears_the_kinds_it_digs() {
    let mut t = loose(4, 4, 1);
    t.carve_hole(&dot(), 1, 2);
    assert_eq!(t.loose(1, 2), 0);
    assert_eq!(t.material(1, 2), Some(Material::Background));
    assert_eq!((t.loose(0, 0), t.loose(2, 2), t.loose(3, 3)), (1, 1, 1));
    assert_eq!((t.loose(-1, 0), t.loose(4, 0), t.loose(0, 4)), (0, 0, 0), "outside: none");
}

#[test]
fn placed_dirt_is_not_loose() {
    let mut t = loose(4, 4, 2);
    t.carve_hole(&dot(), 1, 1);
    t.place_dirt(&dot(), 1, 1);
    assert_eq!(t.material(1, 1), Some(Material::Dirt));
    assert_eq!(t.loose(1, 1), 0);
}

#[test]
fn undo_restores_kinds_and_cells() {
    let mut t = loose(4, 4, 1);
    t.carve_hole(&dot(), 0, 1);
    t.start_journal();
    let (cells, hash) = (t.cells().to_vec(), t.cell_hash());
    let kinds = t.loose_cells().expect("a layer").to_vec();
    let mark = t.journal_mark().expect("on");
    t.move_grain(0, 4);
    assert_eq!((t.loose(0, 1), t.loose(0, 0)), (1, 0), "the grain moved with its kind");
    assert_ne!(t.cell_hash(), hash);
    t.undo_to(mark).expect("undo");
    assert_eq!(t.cells(), &cells[..]);
    assert_eq!(t.loose_cells(), Some(&kinds[..]));
    assert_eq!(t.cell_hash(), hash);
    let mut fresh = t.clone();
    fresh.stop_journal_for_tests();
    assert_eq!(fresh.cell_hash(), hash, "the tracked hash is the scratch hash");
}

#[test]
fn kinds_of_zero_hash_as_before() {
    let plain = dirt(5, 3);
    let zero = dirt(5, 3).with_loose(vec![0; 15]).expect("sized");
    assert!(zero.has_loose() && !plain.has_loose());
    assert_eq!(plain.cell_hash(), zero.cell_hash());
    let one = loose(5, 3, 1);
    assert_ne!(plain.cell_hash(), one.cell_hash(), "a kind is hashed");
    assert!(dirt(2, 2).with_loose(vec![1; 3]).is_none(), "a wrong length");
}

#[test]
fn kinds_on_rock_are_cleared() {
    let grid =
        MaterialGrid::from_cells(3, 1, vec![Material::Rock, Material::Dirt, Material::Background]).unwrap();
    let t = Terrain::from_grid(&grid).with_loose(vec![1, 1, 1]).unwrap();
    assert_eq!((t.loose(0, 0), t.loose(1, 0), t.loose(2, 0)), (0, 1, 0));
}

#[test]
fn take_touched_reports_edits() {
    let mut t = loose(6, 6, 1);
    t.carve_hole(&dot(), 2, 3);
    assert_eq!(t.take_touched().collect::<Vec<_>>(), [Area { x0: 2, y0: 3, x1: 3, y1: 4 }]);
    assert_eq!(t.take_touched().count(), 0);
    let mut plain = dirt(6, 6);
    plain.carve_hole(&dot(), 2, 3);
    assert_eq!(plain.take_touched().count(), 0, "kept only with a layer");
}

#[test]
fn state_transfer_carries_the_kinds() {
    let mut a = loose(4, 4, 3);
    a.carve_hole(&dot(), 0, 0);
    let (cells, kinds) = (a.cells().to_vec(), a.loose_cells().unwrap().to_vec());
    let mut b = loose(4, 4, 1);
    assert!(b.load_cells(&cells));
    assert!(b.load_loose(Some(&kinds)));
    assert_eq!(b, a);
    assert!(!b.load_loose(Some(&kinds[..3])), "a wrong length is refused");
    let mut bad = kinds.clone();
    bad[0] = 2; // (0, 0) is background now
    assert!(!b.load_loose(Some(&bad)), "a kind off dirt is refused");
    assert_eq!(b, a, "a refused load changes nothing");
}
