//! Reversible edits and the bounded history (plan 14b Task 5).

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_edit::compose::Rect;
use olr_edit::element::Property;
use olr_edit::level::{Paint, Shape};
use olr_editor::history::{Edit, History, LevelMeta, Patch};
use olr_editor::{DocError, Document};
use olr_format::levels::{ElementKind, ElementMaterial};
use proptest::prelude::*;

use crate::common::{catalog, content, files, open_fixture};
use olr_edit::compose::Decoded;
use std::sync::Arc;

/// Reverts one recorded step (last edit first).
fn undo(doc: &mut Document, h: &mut History) {
    for e in h.undo().expect("a step to undo").to_vec().iter().rev() {
        doc.apply(e, false);
    }
}

fn redo(doc: &mut Document, h: &mut History) {
    for e in h.redo().expect("a step to redo").to_vec().iter() {
        doc.apply(e, true);
    }
}

fn element<'a>(doc: &'a Document, id: &str) -> &'a olr_format::levels::Element {
    &doc.items()[doc.index_of(id).unwrap()].element
}

#[test]
fn undo_and_redo_restore_everything_exactly() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let mut h = History::new();
    let a = files(&doc);
    let c = doc.remove("tiny").unwrap();
    h.push(vec![c.edit]);
    let b = files(&doc);
    assert!(a != b && !b.contains_key("editor/images/tiny.jpg"));
    undo(&mut doc, &mut h);
    assert!(files(&doc) == a, "undo restores the element, its original and the bake");
    assert!(h.can_redo());
    redo(&mut doc, &mut h);
    assert!(files(&doc) == b);
    let c = doc.add_layer(ElementMaterial::Dirt, "Layer").unwrap();
    h.push(vec![c.edit]);
    assert!(!h.can_redo(), "a new edit clears the redo list");
}

/// The fixture's catalog with a second theme, for theme changes.
fn two_themes() -> olr_editor::Catalog {
    let mut cat = catalog(&content().unwrap());
    let mut tile = RgbaImage::new(3, 3);
    tile.set(1, 1, Rgba::rgb(200, 10, 10));
    cat.themes.push(olr_editor::ThemeEntry {
        id: "test-theme".into(),
        name: "Test".into(),
        front_tile: tile.clone(),
        back_tile: tile,
    });
    cat
}

/// One random operation on the fixture document, recorded as one step.
fn operate(doc: &mut Document, h: &mut History, cat: &olr_editor::Catalog, op: (u8, i32, i32)) {
    let (kind, a, b) = op;
    let ids: Vec<String> = doc.items().iter().map(|i| i.element.id.clone()).collect();
    let id = ids[a.unsigned_abs() as usize % ids.len()].clone();
    let objects = doc.level().objects.len().max(1);
    // A stroke into `target` of `material` (or erasing), as the pointer
    // makes one: its conversion first, then its pixels.
    let stroke = |doc: &mut Document, target: &str, erase: bool, material: Option<Material>| {
        use olr_editor::ops::Stroke;
        // An earlier operation may have removed the layer.
        if doc.index_of(target).is_none() {
            return Ok(Vec::new());
        }
        let kind =
            if erase { Stroke::Erase } else { Stroke::Paint(material.unwrap_or(Material::Background)) };
        let mut step: Vec<Edit> = doc.begin_stroke(target, kind)?.into_iter().collect();
        let Some(before) = doc.stroke_before(target) else { return Ok(step) };
        let shape = Shape::Circle { cx: a, cy: b, r: 6 };
        let r = if erase {
            doc.erase(target, &shape, false)
        } else {
            doc.paint(target, &shape, &Paint::Colour(Rgba::rgb(1, 2, 3)), material)
        };
        step.extend(r?.and_then(|rect| doc.stroke_edit(target, rect, &before)));
        Ok::<_, DocError>(step)
    };
    let result: Result<Vec<Edit>, DocError> = match kind {
        0 => doc.set(&id, Property::Name(format!("n{b}"))).map(|c| vec![c.edit]),
        1 => doc.set(&id, Property::Visible(b % 2 == 0)).map(|c| vec![c.edit]),
        2 => doc.set(&id, Property::At((a, b))).map(|c| vec![c.edit]),
        3 => doc.set(&id, Property::PixelSize(b.unsigned_abs() % 5 + 1)).map(|c| vec![c.edit]),
        4 => doc.set(&id, Property::Colours(Some(b.unsigned_abs() as u16 % 30 + 2))).map(|c| vec![c.edit]),
        5 => doc.move_to(&id, b.unsigned_abs() as usize % ids.len()).map(|c| vec![c.edit]),
        6 => doc.add_layer(ElementMaterial::Rock, "L").map(|c| vec![c.edit]),
        7 => stroke(doc, "tunnels", false, None),
        8 => stroke(doc, "tunnels", true, None),
        9 => doc.add_spawn((a, b)).map(|c| vec![c.edit]),
        10 => doc.move_object(b.unsigned_abs() as usize % objects, (b, a)).map(|c| vec![c.edit]),
        11 => doc.remove_object(a.unsigned_abs() as usize % objects).map(|c| vec![c.edit]),
        12 => {
            let theme = if b % 2 == 0 { "test-theme" } else { "olx-theme-dirt" };
            let meta =
                LevelMeta { name: format!("N{a}"), notes: doc.level().notes.clone(), theme: theme.into() };
            doc.set_meta(meta, cat).map(|c| vec![c.edit])
        }
        13 => doc.remove(&id).map(|c| vec![c.edit]),
        14 => doc.set_art_scale(b.unsigned_abs() % 2 + 1).map(|c| vec![c.edit]),
        // Plan 14d: another material (a conversion), masks, merge, split.
        16 => stroke(doc, "tunnels", false, Some([Material::Rock, Material::Dirt][(b & 1) as usize])),
        17 => stroke(doc, "badge", true, None),
        18 => doc.merge_down(&id),
        19 => doc.split(&id),
        20 => stroke(doc, "wall", true, None),
        _ => doc
            .resize(
                (504 + a.unsigned_abs() % 20, 350 + b.unsigned_abs() % 20),
                (a % 7, b % 7),
                Material::Rock,
                Rgba::rgb(9, 9, 9),
            )
            .map(|c| vec![c.edit]),
    };
    if let Ok(step) = result
        && !step.is_empty()
    {
        h.push(step);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn every_operation_undoes_to_the_same_files(
        ops in prop::collection::vec((0u8..21, -20i32..520, -20i32..360), 1..=6),
    ) {
        let Some(root) = content() else { return Ok(()) };
        let (_tmp, mut doc) = open_fixture(&root);
        let cat = two_themes();
        let mut h = History::new();
        let start = files(&doc);
        for op in ops {
            operate(&mut doc, &mut h, &cat, op);
        }
        let end = files(&doc);
        let mut steps = 0;
        while h.can_undo() {
            undo(&mut doc, &mut h);
            steps += 1;
        }
        prop_assert!(files(&doc) == start);
        for _ in 0..steps {
            redo(&mut doc, &mut h);
        }
        prop_assert!(files(&doc) == end, "redo gives back the end");
    }
}

/// A step of one `Pixels` edit whose before and after are `w` × `h` (and
/// differ: a step of no change is not recorded).
fn pixels(w: u32, h: u32) -> Vec<Edit> {
    let mut after = RgbaImage::new(w, h);
    after.set(0, 0, Rgba::rgb(1, 2, 3));
    let patch = |art| Patch { art, cells: None, back: None };
    vec![Edit::Pixels {
        id: "x".into(),
        rect: Rect { x: 0, y: 0, w, h },
        before: patch(RgbaImage::new(w, h)),
        after: patch(after),
    }]
}

#[test]
fn history_drops_the_oldest_steps_over_the_cap() {
    let step = pixels(1024, 1024);
    let size: usize = step.iter().map(Edit::bytes).sum();
    assert!(size >= 8 << 20);
    let mut h = History::new();
    let n = History::MAX_BYTES / size + 3;
    for i in 0..n {
        let mut s = pixels(1024, 1024);
        if let Edit::Pixels { rect, .. } = &mut s[0] {
            rect.x = i as u32;
        }
        h.push(s);
    }
    assert!(h.bytes() <= History::MAX_BYTES);
    let mut kept = Vec::new();
    while let Some(s) = h.undo() {
        let Edit::Pixels { rect, .. } = &s[0] else { panic!() };
        kept.push(rect.x as usize);
    }
    let want: Vec<usize> = (n - kept.len()..n).rev().collect();
    assert!(kept.len() < n, "the oldest are gone");
    assert_eq!(kept, want, "the newest are kept, newest first");
    drop(h);

    let mut h = History::new();
    h.push(pixels(1024, 1024));
    h.push(pixels(8192, 4200));
    assert!(h.bytes() > History::MAX_BYTES);
    assert!(h.undo().is_some() && !h.can_undo(), "a step larger than the cap is kept alone");
}

#[test]
fn a_locked_element_refuses_changes_but_moves_in_the_stack() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    doc.set("tunnels", Property::Locked(true)).unwrap();
    let locked = |r: Result<_, DocError>| matches!(r, Err(DocError::Locked(_)));
    assert!(locked(doc.set("tunnels", Property::Name("x".into())).map(|_| ())));
    assert!(locked(doc.remove("tunnels").map(|_| ())));
    let shape = Shape::Rect { x: 0, y: 0, w: 4, h: 4 };
    assert!(locked(doc.paint("tunnels", &shape, &Paint::Colour(Rgba::rgb(1, 1, 1)), None).map(|_| ())));
    assert!(locked(doc.erase("tunnels", &shape, false).map(|_| ())));
    doc.move_to("tunnels", 0).unwrap();
    assert_eq!(doc.index_of("tunnels"), Some(0));
    doc.set("tunnels", Property::Locked(false)).unwrap();
}

/// `jpeg` with an EXIF APP1 segment right after SOI holding `Orientation` = `o`
/// (little-endian; as `olr-format`'s `image_io` test helper).
fn with_orientation(jpeg: &[u8], o: u8) -> Vec<u8> {
    let mut tiff = b"II".to_vec();
    tiff.extend_from_slice(&0x2Au16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&u16::from(o).to_le_bytes());
    tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    let mut data = b"Exif\0\0".to_vec();
    data.extend_from_slice(&tiff);
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&u16::try_from(data.len() + 2).unwrap().to_be_bytes());
    out.extend_from_slice(&data);
    out.extend_from_slice(&jpeg[2..]);
    out
}

pub const GRADIENT_JPG: &[u8] = include_bytes!("../../olr-format/tests/data/gradient.jpg");

#[test]
fn importing_turns_by_exif_and_centres_the_box() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let c = doc.add_image("photo.jpg", with_orientation(GRADIENT_JPG, 6), (100, 50)).unwrap();
    let Edit::Insert { item, .. } = &c.edit else { panic!("an insert") };
    let ElementKind::Image { size, at, pixel_size, colours, alpha_threshold, material, .. } =
        &item.item.element.kind
    else {
        panic!("an image")
    };
    assert_eq!((*size, *at), ((48, 64), (76, 18)));
    assert_eq!((*pixel_size, *colours, *alpha_threshold, *material), (1, None, 128, ElementMaterial::Dirt));
    assert_eq!(doc.items().last().unwrap().element.id, "photo");
}

#[test]
fn a_property_change_reprocesses_only_its_image() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let n = doc.cache_processed();
    doc.set("tiny", Property::At((1, 2))).unwrap();
    assert!(matches!(element(&doc, "tiny").kind, ElementKind::Image { at: (1, 2), .. }));
    assert_eq!(doc.cache_processed(), n, "a move never reprocesses");
    let looked = doc.cache_lookups();
    doc.set("tiny", Property::At((3, 4))).unwrap();
    doc.set("tiny", Property::Material(olr_format::levels::ElementMaterial::Dirt)).unwrap();
    assert_eq!(doc.cache_lookups(), looked, "a move or a material keeps the grid: no lookup, no hashing");
    doc.set("tiny", Property::PixelSize(5)).unwrap();
    assert_eq!(doc.cache_processed(), n + 1);
}

/// Batch B review: a resize past the composed-pixel budget is refused from
/// the elements' metadata, before any layer is re-laid.
#[test]
fn a_resize_past_the_pixel_budget_is_refused() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    for _ in 0..16 {
        doc.add_layer(ElementMaterial::Dirt, "L").unwrap();
    }
    let a = files(&doc);
    let e = doc.resize((4096, 4096), (0, 0), Material::Rock, Rgba::rgb(1, 1, 1)).unwrap_err().to_string();
    assert!(e.contains("pixels"), "{e}");
    assert!(files(&doc) == a);
}

/// An image grid shared by several steps (and the cache) counts once.
#[test]
fn a_shared_grid_counts_once() {
    let Some(root) = content() else { return };
    let (_tmp, doc) = open_fixture(&root);
    // `tiny` has no mask (a mask counts in full in every step, R12).
    let item = doc.items()[doc.index_of("tiny").unwrap()].clone();
    let Decoded::Image { grid: Some(grid), .. } = &item.decoded else { panic!("a shown image") };
    let grid_bytes = grid.as_raw().len();
    let snapshot =
        |item: &olr_editor::Item| olr_editor::history::Snapshot { item: item.clone(), original: None };
    let mut h = History::new();
    h.push(vec![Edit::Remove { index: 2, item: Box::new(snapshot(&item)) }]);
    let one = h.bytes();
    h.push(vec![Edit::Insert { index: 2, item: Box::new(snapshot(&item)) }]);
    assert!(h.bytes() < one + grid_bytes, "{} then {}", one, h.bytes());
    assert!(Arc::strong_count(grid) >= 3);
}

/// A `Pixels` edit naming a rectangle far outside the layer changes
/// nothing, never a panic.
#[test]
fn a_forged_pixels_edit_never_panics() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let a = files(&doc);
    let edit = Edit::Pixels {
        id: "tunnels".into(),
        rect: Rect { x: u32::MAX, y: u32::MAX - 1, w: 2, h: 2 },
        before: Patch { art: RgbaImage::new(2, 2), cells: None, back: None },
        after: Patch { art: RgbaImage::new(2, 2), cells: Some(vec![Material::Rock; 4]), back: None },
    };
    doc.apply(&edit, true);
    doc.apply(&edit, false);
    // Plan 14d: a forged mask or layer swap changes nothing either.
    let mask = Edit::Mask {
        id: "badge".into(),
        rect: Rect { x: u32::MAX, y: 0, w: 9, h: 9 },
        before: vec![0; 3],
        after: vec![],
    };
    doc.apply(&mask, true);
    doc.apply(&mask, false);
    let item = doc.items()[0].clone();
    let swap = Edit::Layer { index: 999, before: Box::new(item.clone()), after: Box::new(item) };
    doc.apply(&swap, true);
    assert!(files(&doc) == a);
}

/// A stroke's `before` of another size than the layer gives no edit.
#[test]
fn a_stale_stroke_before_gives_no_edit() {
    let Some(root) = content() else { return };
    let (_tmp, doc) = open_fixture(&root);
    let rect = Rect { x: 1, y: 1, w: 4, h: 4 };
    use olr_editor::ops::StrokeBefore;
    let stale = StrokeBefore::Layer(Decoded::Drawn { art: RgbaImage::new(3, 3), cells: None, back: None });
    assert!(doc.stroke_edit("tunnels", rect, &stale).is_none());
    let before = doc.stroke_before("tunnels").unwrap();
    assert!(doc.stroke_edit("nope", rect, &before).is_none());
    assert!(doc.stroke_edit("tunnels", rect, &before).is_some());
    assert!(doc.stroke_edit("badge", rect, &before).is_none(), "a layer's copy is no mask's");
}

/// An edit that changes nothing says so.
#[test]
fn unchanged_edits_are_no_ops() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    assert!(doc.set("tiny", Property::Name("Tiny".into())).unwrap().edit.is_noop());
    let i = doc.index_of("tiny").unwrap();
    assert!(doc.move_to("tiny", i).unwrap().edit.is_noop());
    let s = doc.level().art_scale;
    assert!(doc.set_art_scale(s).unwrap().edit.is_noop());
    assert!(!doc.set("tiny", Property::Name("Small".into())).unwrap().edit.is_noop());
    let mut h = History::new();
    h.push(vec![doc.move_to("tiny", i).unwrap().edit]);
    assert!(!h.can_undo(), "a step of no-ops is not recorded");
}

/// Plan 14d: the new edits count what they hold: a patch's cells and back
/// art, a layer swap's items, a mask's bytes.
#[test]
fn the_new_edits_count_their_bytes() {
    let base = std::mem::size_of::<Edit>();
    let art = RgbaImage::new(4, 4);
    let patch = Patch { art: art.clone(), cells: Some(vec![Material::Dirt; 16]), back: Some(art.clone()) };
    let edit = Edit::Pixels {
        id: "x".into(),
        rect: Rect { x: 0, y: 0, w: 4, h: 4 },
        before: patch.clone(),
        after: patch,
    };
    assert_eq!(edit.bytes(), base + 2 * (64 + 16 + 64));
    let mask = Edit::Mask {
        id: "x".into(),
        rect: Rect { x: 0, y: 0, w: 2, h: 3 },
        before: vec![0; 6],
        after: vec![255; 6],
    };
    assert_eq!(mask.bytes(), base + 12);
    let item = olr_editor::Item {
        element: olr_format::levels::Element {
            id: "x".into(),
            name: String::new(),
            visible: true,
            locked: false,
            kind: ElementKind::drawn(ElementMaterial::Dirt),
        },
        decoded: Decoded::Drawn { art, cells: None, back: None },
    };
    let layer = Edit::Layer { index: 0, before: Box::new(item.clone()), after: Box::new(item) };
    assert!(layer.bytes() >= base + 2 * 64);
    assert!(layer.is_noop() && edit.is_noop() && !mask.is_noop());
}
