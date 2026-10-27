//! Element operations on a composed level (plan 14a Task 7).

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_edit::element::*;
use olr_edit::level::{Paint, Shape};
use olr_edit::pack::{Pack, Source};
use olr_format::image_io;
use olr_format::levels::{ElementKind, ElementMaterial, LevelSource, check_composition, layer_file};

const RED: Rgba = Rgba::rgb(255, 0, 0);

/// A plain `w` × `h` level pack at art scale `s` (all dirt), in memory.
fn pack(w: u32, h: u32, s: u32) -> Pack {
    let manifest = PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: "t".into(),
        name: "T".into(),
        version: "1".into(),
        kind: PackKind::Level,
        authors: vec![],
        origin: None,
        license: None,
    };
    let level = LevelSource {
        name: "T".into(),
        notes: String::new(),
        size: (w, h),
        art_scale: s,
        theme: "olx-theme-dirt".into(),
        objects: Vec::new(),
        material: vec![Material::Dirt; (w * h) as usize],
        front: RgbaImage::new(w * s, h * s),
        back: RgbaImage::new(w * s, h * s),
        thumb: None,
        look: None,
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
    };
    Pack::new(manifest, Source::Level(level))
}

fn ids(p: &Pack) -> Vec<String> {
    p.composition.as_ref().unwrap().composition.elements.iter().map(|e| e.id.clone()).collect()
}

fn layer(p: &Pack, id: &str) -> RgbaImage {
    image_io::decode_rgba(&p.composition.as_ref().unwrap().files[&layer_file(id)]).unwrap()
}

fn png(w: u32, h: u32) -> Vec<u8> {
    image_io::encode_rgba(&RgbaImage::new(w, h))
}

#[test]
fn a_plain_level_gains_a_raster_base_first() {
    let mut p = pack(8, 6, 1);
    assert!(p.composition.is_none());
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground");
    assert_eq!(ids(&p), ["base", "ground"]);
    let src = p.composition.as_ref().unwrap();
    check_composition(src, (8, 6), 1).unwrap();
}

#[test]
fn ids_are_unique_slugs() {
    let mut p = pack(8, 6, 1);
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground");
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground-2");
    let a = add_image(&mut p, "My Photo.PNG", png(4, 4), (0, 0), (4, 4), ElementMaterial::Rock, "").unwrap();
    let b = add_image(&mut p, "My Photo.PNG", png(4, 4), (0, 0), (4, 4), ElementMaterial::Rock, "").unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("my-photo", "my-photo-2"));
    let sources: Vec<String> = p
        .composition
        .as_ref()
        .unwrap()
        .composition
        .elements
        .iter()
        .filter_map(|e| match &e.kind {
            ElementKind::Image { source, .. } => Some(source.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(sources, ["My Photo.PNG", "My Photo-2.PNG"]);
}

/// A name no file system holds everywhere (a Windows device) is skipped
/// like a taken one (batch A review).
#[test]
fn ids_skip_device_names() {
    let mut p = pack(8, 6, 1);
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Aux").unwrap(), "aux-2");
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "").unwrap(), "layer");
}

#[test]
fn set_validates_and_respects_locks() {
    let mut p = pack(8, 6, 1);
    let id = add_image(&mut p, "a.png", png(4, 4), (0, 0), (4, 4), ElementMaterial::Dirt, "A").unwrap();
    assert!(
        matches!(set(&mut p, &id, Property::PixelSize(0)), Err(ElementError::BadValue(m)) if m.contains("pixel_size"))
    );
    assert!(matches!(set(&mut p, &id, Property::Colours(Some(1))), Err(ElementError::BadValue(_))));
    set(&mut p, &id, Property::Locked(true)).unwrap();
    assert_eq!(set(&mut p, &id, Property::Name("x".into())), Err(ElementError::Locked(id.clone())));
    set(&mut p, &id, Property::Locked(false)).unwrap();
    set(&mut p, &id, Property::Name("x".into())).unwrap();
    assert_eq!(set(&mut p, "nope", Property::Visible(false)), Err(ElementError::NoElement("nope".into())));
}

#[test]
fn raise_lower_remove_reorder_and_drop_files() {
    let mut p = pack(8, 6, 1);
    for name in ["a", "b", "c"] {
        add_layer(&mut p, ElementMaterial::Dirt, name).unwrap();
    }
    raise(&mut p, "a").unwrap();
    assert_eq!(ids(&p), ["base", "b", "a", "c"]);
    lower(&mut p, "c").unwrap();
    assert_eq!(ids(&p), ["base", "b", "c", "a"]);
    raise(&mut p, "a").unwrap(); // top: no-op
    lower(&mut p, "base").unwrap(); // bottom: no-op
    assert_eq!(ids(&p), ["base", "b", "c", "a"]);
    remove(&mut p, "b").unwrap();
    assert_eq!(ids(&p), ["base", "c", "a"]);
    assert!(!p.composition.as_ref().unwrap().files.contains_key(&layer_file("b")));
}

#[test]
fn draw_and_erase_paint_whole_art_blocks() {
    let mut p = pack(8, 6, 2);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    draw(&mut p, &id, &Shape::Rect { x: 1, y: 1, w: 2, h: 2 }, &Paint::Colour(RED), None).unwrap();
    let art = layer(&p, &id);
    for ay in 0..12 {
        for ax in 0..16 {
            let inside = (2..6).contains(&ax) && (2..6).contains(&ay);
            assert_eq!(art.get(ax, ay).a, if inside { 255 } else { 0 }, "({ax}, {ay})");
            if inside {
                assert_eq!(art.get(ax, ay), RED);
            }
        }
    }
    erase(&mut p, &id, &Shape::Circle { cx: 2, cy: 2, r: 0 }).unwrap();
    let art = layer(&p, &id);
    for ay in 4..6 {
        for ax in 4..6 {
            assert_eq!(art.get(ax, ay).a, 0);
        }
    }
    assert_eq!(art.get(2, 2).a, 255);
    assert!(matches!(
        draw(&mut p, "base", &Shape::Rect { x: 0, y: 0, w: 1, h: 1 }, &Paint::Colour(RED), None),
        Err(ElementError::WrongKind { .. })
    ));
}

#[test]
fn add_image_refuses_a_hostile_file_and_writes_nothing() {
    let mut p = pack(8, 6, 1);
    add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    let before = p.composition.clone();
    for (name, bytes) in
        [("huge.png", image_io::declared_png(100_000, 100_000, false)), ("junk.png", b"junk".to_vec())]
    {
        let e = add_image(&mut p, name, bytes, (0, 0), (4, 4), ElementMaterial::Dirt, "").unwrap_err();
        assert!(matches!(&e, ElementError::BadValue(m) if m.contains(name)), "{e}");
        assert_eq!(p.composition, before);
    }
}

#[test]
fn resize_moves_boxes_and_relays_layers() {
    let mut p = pack(8, 6, 1);
    let img = add_image(&mut p, "a.png", png(2, 2), (3, 2), (2, 2), ElementMaterial::Rock, "").unwrap();
    let l = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    draw(&mut p, &l, &Shape::Rect { x: 0, y: 0, w: 1, h: 1 }, &Paint::Colour(RED), None).unwrap();
    resize(&mut p, (13, 9), (5, 3), Material::Rock, &Paint::Colour(RED)).unwrap();
    let src = p.composition.as_ref().unwrap();
    let at = src.composition.elements.iter().find(|e| e.id == img).map(|e| match &e.kind {
        ElementKind::Image { at, .. } => *at,
        _ => unreachable!(),
    });
    assert_eq!(at, Some((8, 5)));
    let art = layer(&p, &l);
    assert_eq!((art.width(), art.height()), (13, 9));
    assert_eq!(art.get(5, 3), RED);
    assert_eq!(art.get(0, 0).a, 0, "the new area is uncovered");
    let material = image_io::decode_indexed(&src.files["editor/layers/base.material.png"]).unwrap();
    assert_eq!((material.0, material.1), (13, 9));
    assert_eq!(material.2[0], Material::Rock as u8, "the raster's new area is the given material");
    check_composition(src, (13, 9), 1).unwrap();
}

/// Batch C review: a hand-edited layer of the wrong size is refused by
/// `resize`, never a panic, and nothing changes.
#[test]
fn resize_refuses_a_wrong_sized_layer() {
    for file in [
        layer_file("l"),
        "editor/layers/base.front.png".to_string(),
        "editor/layers/base.material.png".into(),
    ] {
        let mut p = pack(8, 6, 1);
        add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
        p.composition.as_mut().unwrap().files.insert(file.clone(), png(3, 3));
        let before = p.composition.clone();
        let e = resize(&mut p, (10, 9), (1, 1), Material::Rock, &Paint::Colour(RED)).unwrap_err();
        assert!(matches!(&e, ElementError::BadValue(m) if m.contains(&file)), "{file}: {e}");
        assert_eq!(p.composition, before);
    }
}

/// A pack whose drawn layer `l` was hand-edited to the wrong size.
fn broken() -> Pack {
    let mut p = pack(8, 6, 1);
    add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    add_layer(&mut p, ElementMaterial::Dirt, "M").unwrap();
    p.composition.as_mut().unwrap().files.insert(layer_file("l"), png(3, 3));
    p
}

/// Deferred minors: an error already in the composition is not labelled
/// with the property being set; an edit that repairs it works.
#[test]
fn an_existing_error_is_not_labelled_as_the_edit() {
    let mut p = broken();
    let e = set(&mut p, "m", Property::Name("x".into())).unwrap_err().to_string();
    assert!(e.starts_with("editor/layers/l.png"), "{e}");
    remove(&mut p, "l").unwrap();
}

/// Deferred minors: an original named like a Windows device is renamed.
#[test]
fn a_device_named_original_is_renamed() {
    let mut p = pack(8, 6, 1);
    add_image(&mut p, "CON.png", png(2, 2), (0, 0), (2, 2), ElementMaterial::Rock, "").unwrap();
    let src = p.composition.as_ref().unwrap();
    assert!(src.files.contains_key(&olr_format::levels::image_file("CON-2.png")), "{:?}", src.files.keys());
}

/// Deferred minors: ids are at most 64 bytes.
#[test]
fn long_names_make_short_ids() {
    let mut p = pack(8, 6, 1);
    let id = add_layer(&mut p, ElementMaterial::Dirt, &"a".repeat(300)).unwrap();
    assert_eq!(id.len(), 64);
    let id2 = add_layer(&mut p, ElementMaterial::Dirt, &"a".repeat(300)).unwrap();
    assert!(id2.len() <= 64 && id2 != id, "{id2}");
}

/// Deferred minors: an image's settings apply with it, checked together:
/// a pixel size that brings the composition within its pixel budget
/// counts from the start.
#[test]
fn settings_apply_together() {
    let mut p = pack(8, 6, 1);
    // The base and 15 boxes of 4096² (16 Mi grid pixels each at art scale
    // 1) nearly fill the 256 Mi budget.
    for i in 0..15 {
        add_image(&mut p, &format!("{i}.png"), png(2, 2), (0, 0), (4096, 4096), ElementMaterial::Dirt, "")
            .unwrap();
    }
    assert!(add_image(&mut p, "x.png", png(2, 2), (0, 0), (4096, 4096), ElementMaterial::Dirt, "").is_err());
    add_image_with(
        &mut p,
        "x.png",
        png(2, 2),
        (0, 0),
        (4096, 4096),
        ElementMaterial::Dirt,
        "",
        &[Property::PixelSize(2)],
    )
    .unwrap();
    // A 2048² box fits; growing it to 4096² fits only with pixel size 2,
    // set in the same step.
    let id = add_image(&mut p, "z.png", png(2, 2), (0, 0), (2048, 2048), ElementMaterial::Dirt, "").unwrap();
    assert!(set(&mut p, &id, Property::Size((4096, 4096))).is_err());
    set_all(&mut p, &id, &[Property::Size((4096, 4096)), Property::PixelSize(2)]).unwrap();
}

/// Deferred minors: locking a locked element is no change, not an error;
/// a locked element refuses `remove`, `draw` and `erase`.
#[test]
fn locks() {
    let mut p = pack(8, 6, 1);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    set(&mut p, &id, Property::Locked(true)).unwrap();
    set(&mut p, &id, Property::Locked(true)).unwrap();
    let locked = Err(ElementError::Locked(id.clone()));
    let rect = Shape::Rect { x: 0, y: 0, w: 1, h: 1 };
    assert_eq!(draw(&mut p, &id, &rect, &Paint::Colour(RED), None), locked);
    assert_eq!(erase(&mut p, &id, &rect), locked);
    assert_eq!(remove(&mut p, &id), locked);
}

/// Deferred minors: a JPEG cut in its scan, or declaring a huge frame, is
/// refused by `add_image` naming the file.
#[test]
fn add_image_refuses_a_cut_or_huge_jpeg() {
    const GRADIENT: &[u8] = include_bytes!("../../olr-format/tests/data/gradient.jpg");
    let mut huge = include_bytes!("../../olr-format/tests/data/tiny.jpg").to_vec();
    let sof = huge.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    huge[sof + 5..sof + 9].copy_from_slice(&[0xEA, 0x60, 0xEA, 0x60]);
    let mut p = pack(8, 6, 1);
    for (name, bytes) in [("cut.jpg", GRADIENT[..GRADIENT.len() / 2].to_vec()), ("huge.jpg", huge)] {
        let e = add_image(&mut p, name, bytes, (0, 0), (4, 4), ElementMaterial::Dirt, "").unwrap_err();
        assert!(matches!(&e, ElementError::BadValue(m) if m.contains(name)), "{e}");
        assert_eq!(p.composition, None);
    }
}

#[test]
fn new_element_id_and_source_name_follow_r7_r8() {
    let mut p = pack(8, 6, 1);
    add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap();
    let ground: Vec<_> = p
        .composition
        .as_ref()
        .unwrap()
        .composition
        .elements
        .iter()
        .filter(|e| e.id == "ground")
        .cloned()
        .collect();
    let none = std::collections::BTreeSet::new();
    assert_eq!(new_element_id(&ground, &[], &none, "Ground", "layer"), "ground-2");
    // A stray file a new element would overwrite is skipped too.
    let stray: std::collections::BTreeSet<String> = ["editor/layers/ground-2.png".to_string()].into();
    assert_eq!(new_element_id(&ground, &[], &stray, "Ground", "layer"), "ground-3");
    assert_eq!(new_source_name(["a.png"].into_iter(), "A.PNG"), "A-2.PNG");
    assert_eq!(new_source_name([].into_iter(), "CON.png"), "CON-2.png");
}

/// Plan 14b Task 3: re-laying each decoded element gives the files
/// `element::resize` writes.
#[test]
fn resize_decoded_matches_element_resize() {
    use olr_edit::compose::{ImageCache, decode_element, encode_decoded, resize_decoded};
    let mut p = pack(8, 6, 1);
    add_image(&mut p, "a.png", png(2, 2), (3, 2), (2, 2), ElementMaterial::Rock, "").unwrap();
    let l = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    draw(&mut p, &l, &Shape::Rect { x: 0, y: 0, w: 1, h: 1 }, &Paint::Colour(RED), None).unwrap();
    let before = p.composition.clone().unwrap();
    let paint = Paint::Colour(RED);
    resize(&mut p, (13, 9), (5, 3), Material::Rock, &paint).unwrap();
    let want = p.composition.unwrap();
    let mut got = before.clone();
    let mut cache = ImageCache::new();
    for e in &mut got.composition.elements {
        let mut d = decode_element(&before, e, (8, 6), 1, &mut cache).unwrap();
        resize_decoded(e, &mut d, (8, 6), 1, (13, 9), (5, 3), Material::Rock, &paint).unwrap();
        for (path, bytes) in encode_decoded(e, &d, (13, 9)) {
            got.files.insert(path, bytes);
        }
    }
    assert_eq!(got.composition, want.composition);
    assert!(got.files == want.files);
}

/// A stray file in `editor/` (in `other`) is never overwritten by a new
/// element's file.
#[test]
fn new_elements_skip_stray_editor_files() {
    let mut p = pack(8, 6, 1);
    let stray = |p: &str| olr_vfs::VPath::new(p).unwrap();
    p.other.insert(stray("editor/layers/ground.png"), b"mine".to_vec());
    p.other.insert(stray("editor/images/a.png"), b"also mine".to_vec());
    assert_eq!(add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap(), "ground-2");
    add_image(&mut p, "a.png", png(2, 2), (0, 0), (2, 2), ElementMaterial::Rock, "").unwrap();
    let src = p.composition.as_ref().unwrap();
    assert!(src.files.contains_key("editor/images/a-2.png"), "{:?}", src.files.keys());
}

/// Plan 14d Task 6: a loaded map merged and split again bakes as it was.
#[test]
fn merge_down_and_split_round_trip_a_plain_level() {
    let Some(root) = crate::common::content() else { return };
    let mut p = olr_edit::pack::load(&root.join("olx-level-dirt-level")).unwrap();
    let Source::Level(original) = &p.source else { panic!() };
    let original = original.clone();
    p.composition = Some(olr_edit::compose::split_from(&original));
    merge_down(&mut p, "dirt").unwrap();
    assert_eq!(ids(&p), ["background", "rock"]);
    let parts = split(&mut p, "background").unwrap();
    assert_eq!(parts.len(), 2, "{parts:?}");
    let tile = olr_edit::compose::theme_back_tile(&root, &original.theme).unwrap();
    let mut baked = original.clone();
    olr_edit::compose::bake(&mut baked, p.composition.as_ref().unwrap(), &tile).unwrap();
    assert!(
        baked.material == original.material && baked.front == original.front && baked.back == original.back
    );
}

#[test]
fn drawing_rock_into_a_dirt_layer_makes_it_mixed() {
    let mut p = pack(8, 6, 1);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap();
    draw(
        &mut p,
        &id,
        &Shape::Rect { x: 1, y: 1, w: 2, h: 2 },
        &Paint::Colour(RED),
        Some(ElementMaterial::Rock),
    )
    .unwrap();
    let src = p.composition.as_ref().unwrap();
    let e = src.composition.elements.iter().find(|e| e.id == id).unwrap();
    assert!(
        matches!(e.kind, ElementKind::Drawn { mixed: true, material: ElementMaterial::Dirt, .. }),
        "{:?}",
        e.kind
    );
    assert!(src.files.contains_key("editor/layers/ground.material.png"));
    check_composition(src, (8, 6), 1).unwrap();
}

#[test]
fn mask_by_shape_and_colour() {
    let mut p = pack(8, 6, 1);
    let id = add_image(&mut p, "a.png", png(4, 4), (0, 0), (4, 4), ElementMaterial::Dirt, "A").unwrap();
    mask(&mut p, &id, &MaskOp::Shape(Shape::Rect { x: 0, y: 0, w: 1, h: 1 }), false).unwrap();
    let src = p.composition.as_ref().unwrap();
    let e = src.composition.elements.iter().find(|e| e.id == id).unwrap();
    assert!(matches!(e.kind, ElementKind::Image { mask: true, .. }));
    let file = olr_format::levels::mask_file(&id);
    let (_, _, keep) = image_io::decode_grey_within(&src.files[&file], image_io::IMAGES).unwrap();
    assert_eq!(keep.iter().filter(|k| **k == 0).count(), 1);
    // The whole transparent image is one colour: the flood takes it all.
    mask(&mut p, &id, &MaskOp::ColourAt { at: (2, 2), tolerance: 0 }, false).unwrap();
    let src = p.composition.as_ref().unwrap();
    let (_, _, keep) = image_io::decode_grey_within(&src.files[&file], image_io::IMAGES).unwrap();
    assert!(keep.iter().all(|k| *k == 0));
    let e = mask(&mut p, &id, &MaskOp::ColourAt { at: (2, 2), tolerance: 0 }, true).unwrap_err();
    assert!(e.to_string().contains("--restore takes a shape"), "{e}");
}

#[test]
fn set_at_moves_a_drawn_layer() {
    let mut p = pack(8, 6, 1);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    set(&mut p, &id, Property::At((2, 3))).unwrap();
    let e = &p.composition.as_ref().unwrap().composition.elements[1];
    assert!(
        matches!(e.kind, ElementKind::Drawn { at: (2, 3), size: Some((8, 6)), .. }),
        "moving pins its box: {:?}",
        e.kind
    );
}

/// Batch B review 2: a layer moved back to the origin is level-sized again.
#[test]
fn a_layer_moved_back_to_the_origin_follows_the_level_again() {
    let mut p = pack(8, 6, 1);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    set(&mut p, &id, Property::At((2, 3))).unwrap();
    set(&mut p, &id, Property::At((0, 0))).unwrap();
    let e = &p.composition.as_ref().unwrap().composition.elements[1];
    assert_eq!(e.kind, ElementKind::drawn(ElementMaterial::Dirt));
}

/// Plan 14d minors: new ids never take a stray mask file, nor the names a
/// plain layer's material and back art would take.
#[test]
fn new_ids_reserve_masks_and_layer_files() {
    use olr_format::levels::Element;
    let files: std::collections::BTreeSet<String> = ["editor/masks/x.png".to_string()].into();
    assert_ne!(new_element_id(&[], &[], &files, "x", "image"), "x");
    let wall = Element {
        id: "wall".into(),
        name: String::new(),
        visible: true,
        locked: false,
        folder: None,
        kind: ElementKind::drawn(ElementMaterial::Dirt),
        glow: None,
        clear: false,
        glass: None,
        distant: None,
        surface: None,
        back_surface: None,
        surface_map: false,
        back_surface_map: false,
    };
    assert_ne!(new_element_id(&[wall], &[], &Default::default(), "wall.material", "layer"), "wall.material");
}

/// Plan 14d minors: a mixed layer made plain drops its material file.
#[test]
fn a_layer_made_plain_drops_its_material_file() {
    let mut p = pack(8, 6, 1);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "Ground").unwrap();
    draw(
        &mut p,
        &id,
        &Shape::Rect { x: 1, y: 1, w: 2, h: 2 },
        &Paint::Colour(RED),
        Some(ElementMaterial::Rock),
    )
    .unwrap();
    set(&mut p, &id, Property::Material(ElementMaterial::Rock)).unwrap();
    assert!(!p.composition.as_ref().unwrap().files.contains_key("editor/layers/ground.material.png"));
}

/// Plan 14d minors: a mask shape that misses the picture changes nothing.
#[test]
fn a_mask_that_misses_the_picture_is_refused() {
    let mut p = pack(8, 6, 1);
    let id = add_image(&mut p, "a.png", png(4, 4), (0, 0), (4, 4), ElementMaterial::Dirt, "A").unwrap();
    let before = p.composition.clone();
    let e = mask(&mut p, &id, &MaskOp::Shape(Shape::Rect { x: 6, y: 5, w: 1, h: 1 }), false).unwrap_err();
    assert!(e.to_string().contains("misses"), "{e}");
    assert_eq!(p.composition, before);
}

fn element<'a>(p: &'a Pack, id: &str) -> &'a olr_format::levels::Element {
    p.composition.as_ref().unwrap().composition.elements.iter().find(|e| e.id == id).unwrap()
}

#[test]
fn duplicate_copies_files_and_shares_originals() {
    let mut p = pack(8, 6, 1);
    let layer_id = add_layer(&mut p, ElementMaterial::Dirt, "Crate").unwrap();
    draw(
        &mut p,
        &layer_id,
        &Shape::Rect { x: 1, y: 1, w: 2, h: 2 },
        &Paint::Colour(RED),
        Some(ElementMaterial::Rock),
    )
    .unwrap();
    let image = add_image(&mut p, "a.png", png(4, 4), (1, 1), (4, 4), ElementMaterial::Rock, "Sign").unwrap();
    mask(&mut p, &image, &MaskOp::Shape(Shape::Rect { x: 1, y: 1, w: 1, h: 1 }), false).unwrap();
    assert_eq!(ids(&p), ["base", "crate", "sign"]);

    assert_eq!(duplicate(&mut p, "crate").unwrap(), "crate-2");
    assert_eq!(duplicate(&mut p, "sign").unwrap(), "sign-2");
    assert_eq!(duplicate(&mut p, "base").unwrap(), "base-2");
    assert_eq!(ids(&p), ["base", "base-2", "crate", "crate-2", "sign", "sign-2"]);
    let src = p.composition.as_ref().unwrap();
    check_composition(src, (8, 6), 1).unwrap();
    assert_eq!(element(&p, "crate-2").name, "Crate 2");
    assert_eq!(element(&p, "sign-2").name, "Sign 2");
    assert_eq!(element(&p, "crate-2").kind, element(&p, "crate").kind);
    for (a, b) in [("crate", "crate-2"), ("base", "base-2")] {
        let (fa, fb) = (
            olr_format::levels::element_files(element(&p, a)),
            olr_format::levels::element_files(element(&p, b)),
        );
        assert_eq!(fa.len(), fb.len());
        for (x, y) in fa.iter().zip(&fb) {
            assert_ne!(x, y);
            assert_eq!(src.files[x], src.files[y], "{y} copies {x}");
        }
    }
    let ElementKind::Image { source: s1, .. } = &element(&p, "sign").kind else { panic!() };
    let ElementKind::Image { source: s2, mask: true, .. } = &element(&p, "sign-2").kind else { panic!() };
    assert_eq!(s1, s2, "an image copy shares its original");
    let (m1, m2) = (olr_format::levels::mask_file("sign"), olr_format::levels::mask_file("sign-2"));
    assert_eq!(src.files[&m1], src.files[&m2]);
    // Locked elements duplicate too; unknown ones are refused.
    assert!(matches!(duplicate(&mut p, "nope"), Err(ElementError::NoElement(_))));
}

#[test]
fn copy_name_counts_on() {
    let names = ["Crate", "Crate 2", "Pipe 3", ""];
    assert_eq!(copy_name("Crate", names.iter().copied()), "Crate 3");
    assert_eq!(copy_name("Crate 2", names.iter().copied()), "Crate 3");
    assert_eq!(copy_name("Pipe 3", names.iter().copied()), "Pipe 2");
    assert_eq!(copy_name("Desk", names.iter().copied()), "Desk 2");
    assert_eq!(copy_name("Room 101", names.iter().copied()), "Room 2");
    assert_eq!(copy_name("", names.iter().copied()), "2");
}

#[test]
fn fill_paints_a_texture_into_a_layer() {
    let mut p = pack(8, 6, 2);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    draw(&mut p, &id, &Shape::Rect { x: 0, y: 0, w: 8, h: 1 }, &Paint::Colour(RED), None).unwrap();
    let tile = olr_edit::textures::builtin().remove(3).image;
    fill(&mut p, &id, (3, 3), &Paint::Tile(tile.clone()), 0, Some(ElementMaterial::Rock)).unwrap();
    let art = layer(&p, &id);
    for ay in 0..12 {
        for ax in 0..16 {
            let want = if ay < 2 { RED } else { tile.get(ax % 64, ay % 64) };
            assert_eq!(art.get(ax, ay), want, "({ax}, {ay})");
        }
    }
    let e = element(&p, &id);
    assert!(matches!(e.kind, ElementKind::Drawn { mixed: true, .. }), "another material mixes: {:?}", e.kind);
    // A seed off the layer is refused, and changes nothing (batch A review):
    // not even a file no element names, which a kept edit would drop.
    p.composition.as_mut().unwrap().files.insert("editor/layers/stray.png".into(), png(1, 1));
    let before = p.composition.clone();
    let err = fill(&mut p, &id, (8, 0), &Paint::Colour(RED), 0, None).unwrap_err();
    assert!(err.to_string().contains("outside"), "{err}");
    assert!(p.composition == before, "a refused fill keeps the composition");
    assert!(matches!(
        fill(&mut p, "base", (1, 1), &Paint::Colour(RED), 0, None),
        Err(ElementError::WrongKind { .. })
    ));
}

/// Minors: a draw or fill of another material that paints nothing leaves a
/// plain layer plain.
#[test]
fn painting_nothing_keeps_a_layer_plain() {
    let mut p = pack(8, 6, 1);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "L").unwrap();
    let clear = Paint::Tile(RgbaImage::new(2, 2));
    draw(&mut p, &id, &Shape::Rect { x: 0, y: 0, w: 4, h: 4 }, &clear, Some(ElementMaterial::Rock)).unwrap();
    fill(&mut p, &id, (1, 1), &clear, 0, Some(ElementMaterial::Rock)).unwrap();
    assert_eq!(element(&p, &id).kind, ElementKind::drawn(ElementMaterial::Dirt));
    assert!(!p.composition.as_ref().unwrap().files.contains_key("editor/layers/l.material.png"));
}

/// Minors: a locked element duplicates; its copy is locked too.
#[test]
fn a_locked_element_duplicates_locked() {
    let mut p = pack(8, 6, 1);
    let id = add_layer(&mut p, ElementMaterial::Dirt, "Crate").unwrap();
    set(&mut p, &id, Property::Locked(true)).unwrap();
    let copy = duplicate(&mut p, &id).unwrap();
    assert!(element(&p, &copy).locked);
}

/// Minors: renaming never runs out, even past a thousand taken names.
#[test]
fn a_source_name_is_always_free() {
    let taken: Vec<String> =
        std::iter::once("tiny.jpg".to_string()).chain((2..=1500).map(|i| format!("tiny-{i}.jpg"))).collect();
    assert_eq!(new_source_name(taken.iter().map(String::as_str), "TINY.jpg"), "TINY-1501.jpg");
}

/// Review of the minors: a name without an extension, or whose every
/// suffix stays unportable, never comes back taken.
#[test]
fn a_taken_source_name_is_never_returned() {
    assert_eq!(new_source_name(["foo"].into_iter(), "foo"), "foo-2");
    let n = new_source_name(["con.x.png"].into_iter(), "con.x.png");
    assert_ne!(n, "con.x.png");
    assert!(olr_vfs::non_portable(&n).is_none(), "{n}");
    // A name only unportable (not taken) is kept for the check to refuse.
    assert_eq!(new_source_name([].into_iter(), "a:b.png"), "a:b.png");
}

/// Issue #11: a background element made distant moves to the top of the
/// distant layers, made near again to just above them; moves across that
/// edge, merging and splitting a distant layer are refused, and a dirt
/// layer cannot be distant.
#[test]
fn distant_layers_keep_below_the_rest() {
    let mut p = pack(8, 6, 1);
    for name in ["Sky", "Hills", "Wall"] {
        add_layer(&mut p, ElementMaterial::Background, name).unwrap();
    }
    assert_eq!(ids(&p), ["base", "sky", "hills", "wall"]);
    set(&mut p, "sky", Property::Distant(Some(90))).unwrap();
    assert_eq!(ids(&p), ["sky", "base", "hills", "wall"]);
    set(&mut p, "hills", Property::Distant(Some(40))).unwrap();
    assert_eq!(ids(&p), ["sky", "hills", "base", "wall"]);
    // A new depth keeps its place.
    set(&mut p, "sky", Property::Distant(Some(80))).unwrap();
    assert_eq!(ids(&p), ["sky", "hills", "base", "wall"]);
    let e = raise(&mut p, "hills").unwrap_err().to_string();
    assert!(e.contains(DISTANT_ORDER), "{e}");
    assert!(lower(&mut p, "base").unwrap_err().to_string().contains(DISTANT_ORDER));
    lower(&mut p, "hills").unwrap();
    assert_eq!(ids(&p), ["hills", "sky", "base", "wall"]);
    assert!(merge_down(&mut p, "sky").unwrap_err().to_string().contains("distant layer does not merge"));
    assert!(split(&mut p, "sky").unwrap_err().to_string().contains("distant layer does not split"));
    // A duplicate stays among the distant layers.
    let copy = duplicate(&mut p, "sky").unwrap();
    assert_eq!(ids(&p), ["hills", "sky", copy.as_str(), "base", "wall"]);
    // Near again: just above the distant layers.
    set(&mut p, "hills", Property::Distant(None)).unwrap();
    assert_eq!(ids(&p), ["sky", copy.as_str(), "hills", "base", "wall"]);
    // Only a background element.
    set(&mut p, "wall", Property::Material(ElementMaterial::Dirt)).unwrap();
    let e = set(&mut p, "wall", Property::Distant(Some(50))).unwrap_err().to_string();
    assert!(e.contains("only a background element"), "{e}");
    assert!(set(&mut p, "sky", Property::Distant(Some(100))).is_err());
    check_composition(p.composition.as_ref().unwrap(), (8, 6), 1).unwrap();
}

/// Issue #11: Save as asset leaves distant layers out.
#[test]
fn an_asset_carries_no_distant_layer() {
    let mut p = pack(8, 6, 1);
    add_layer(&mut p, ElementMaterial::Background, "Sky").unwrap();
    add_layer(&mut p, ElementMaterial::Background, "Wall").unwrap();
    for id in ["sky", "wall"] {
        draw(&mut p, id, &Shape::Rect { x: 0, y: 0, w: 4, h: 4 }, &Paint::Colour(RED), None).unwrap();
    }
    set(&mut p, "sky", Property::Distant(Some(60))).unwrap();
    let decoded = olr_edit::asset::DecodedLevel::of_pack(&p).unwrap();
    let level = decoded.level();
    let ids: Vec<String> = ["sky", "wall"].map(String::from).to_vec();
    let a = olr_edit::asset::from_elements(&level, &ids, "A").unwrap();
    assert!(a.pieces.iter().all(|x| x.element.distant.is_none()));
    assert_eq!(a.pieces.len(), 1);
    assert!(olr_edit::asset::from_elements(&level, &ids[..1], "A").is_err());
    let rect = olr_edit::level::Rect { x: 0, y: 0, w: 8, h: 6 };
    let cut = olr_edit::asset::cut_rect(&level, rect, None, "B").unwrap();
    assert!(cut.pieces.iter().all(|x| x.element.distant.is_none()));
}
