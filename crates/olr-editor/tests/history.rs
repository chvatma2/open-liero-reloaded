//! Reversible edits and the bounded history (plan 14b Task 5).

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_edit::compose::Rect;
use olr_edit::element::Property;
use olr_edit::level::{Paint, Shape};
use olr_editor::history::{Edit, History, LevelMeta};
use olr_editor::{DocError, Document};
use olr_format::levels::{ElementKind, ElementMaterial};
use proptest::prelude::*;

use crate::common::{catalog, content, files, open_fixture};

/// Reverts one recorded step (last edit first).
fn undo(doc: &mut Document, h: &mut History) {
    for e in h.undo().expect("a step to undo").iter().rev() {
        doc.apply(e, false);
    }
}

fn redo(doc: &mut Document, h: &mut History) {
    for e in &h.redo().expect("a step to redo") {
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

/// One random operation on the fixture document, recorded as one step.
fn operate(doc: &mut Document, h: &mut History, op: (u8, i32, i32)) {
    let (kind, a, b) = op;
    let cat = catalog(&content().unwrap());
    let ids: Vec<String> = doc.items().iter().map(|i| i.element.id.clone()).collect();
    let id = ids[a.unsigned_abs() as usize % ids.len()].clone();
    let result: Result<Vec<Edit>, DocError> = match kind {
        0 => doc.set(&id, Property::Name(format!("n{b}"))).map(|c| vec![c.edit]),
        1 => doc.set(&id, Property::Visible(b % 2 == 0)).map(|c| vec![c.edit]),
        2 => doc.set(&id, Property::At((a, b))).map(|c| vec![c.edit]),
        3 => doc.set(&id, Property::PixelSize(b.unsigned_abs() % 5 + 1)).map(|c| vec![c.edit]),
        4 => doc.set(&id, Property::Colours(Some(b.unsigned_abs() as u16 % 30 + 2))).map(|c| vec![c.edit]),
        5 => doc.move_to(&id, b.unsigned_abs() as usize % ids.len()).map(|c| vec![c.edit]),
        6 => doc.add_layer(ElementMaterial::Rock, "L").map(|c| vec![c.edit]),
        7 => {
            let before = doc.layer_art("tunnels").unwrap().clone();
            let shape = Shape::Circle { cx: a, cy: b, r: 6 };
            match doc.paint("tunnels", &shape, &Paint::Colour(Rgba::rgb(1, 2, 3))) {
                Ok(Some(rect)) => Ok(vec![doc.stroke_edit("tunnels", rect, &before)]),
                Ok(None) => Ok(vec![]),
                Err(e) => Err(e),
            }
        }
        8 => doc.add_spawn((a, b)).map(|c| vec![c.edit]),
        9 => {
            let meta = LevelMeta {
                name: format!("N{a}"),
                notes: doc.level().notes.clone(),
                theme: doc.level().theme.clone(),
            };
            doc.set_meta(meta, &cat).map(|c| vec![c.edit])
        }
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
        ops in prop::collection::vec((0u8..11, -20i32..520, -20i32..360), 1..=6),
    ) {
        let Some(root) = content() else { return Ok(()) };
        let (_tmp, mut doc) = open_fixture(&root);
        let mut h = History::new();
        let start = files(&doc);
        for op in ops {
            operate(&mut doc, &mut h, op);
        }
        while h.can_undo() {
            undo(&mut doc, &mut h);
        }
        prop_assert!(files(&doc) == start);
    }
}

/// A step of one `Pixels` edit whose before and after are `w` × `h`.
fn pixels(w: u32, h: u32) -> Vec<Edit> {
    vec![Edit::Pixels {
        id: "x".into(),
        rect: Rect { x: 0, y: 0, w, h },
        before: RgbaImage::new(w, h),
        after: RgbaImage::new(w, h),
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
    assert!(locked(doc.paint("tunnels", &shape, &Paint::Colour(Rgba::rgb(1, 1, 1))).map(|_| ())));
    assert!(locked(doc.erase("tunnels", &shape).map(|_| ())));
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
    doc.set("tiny", Property::PixelSize(5)).unwrap();
    assert_eq!(doc.cache_processed(), n + 1);
}
