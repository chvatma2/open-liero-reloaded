//! The document (plan 14b Task 4): a level pack as the editor holds it.

use olr_edit::element::Property;
use olr_editor::{DocError, Document, Severity};
use olr_format::levels::LevelObject;

use crate::common::{catalog, content, copy_pack, files, fixture, open_fixture, tree};

#[test]
fn opening_and_saving_changes_nothing() {
    let Some(root) = content() else { return };
    let (_tmp, doc) = open_fixture(&root);
    assert!(files(&doc) == tree(doc.dir()), "the composed fixture comes back byte for byte");

    let (_tmp, dir) = copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    let on_disk = tree(&dir);
    let doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &catalog(&root)).unwrap();
    let saved = files(&doc);
    let gained: Vec<&String> = saved.keys().filter(|k| !on_disk.contains_key(*k)).collect();
    assert_eq!(
        gained,
        [
            "editor/composition.ron",
            "editor/layers/background.png",
            "editor/layers/dirt.png",
            "editor/layers/rock.png"
        ],
        "a plain level opens as its three layers"
    );
    for (path, bytes) in &on_disk {
        assert!(saved.get(path) == Some(bytes), "{path} unchanged");
    }
}

#[test]
fn a_missing_theme_is_an_error() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    let mut cat = catalog(&root);
    cat.themes.clear();
    let e = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &cat).unwrap_err();
    assert!(matches!(&e, DocError::NoTheme(t) if t == "olx-theme-dirt"), "{e}");
}

#[test]
fn a_mod_pack_is_not_a_level() {
    let Some(root) = content() else { return };
    let dir = root.join("olx-classic");
    let e = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &catalog(&root)).unwrap_err();
    assert!(matches!(e, DocError::NotALevel), "{e}");
}

#[test]
fn diagnostics_list_spawns_outside_and_none() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    let open = |objects: Vec<LevelObject>| {
        let mut pack = olr_edit::pack::load(&dir).unwrap();
        let olr_edit::pack::Source::Level(l) = &mut pack.source else { panic!() };
        l.objects = objects;
        Document::open(pack, dir.clone(), &catalog(&root)).unwrap().diagnostics()
    };
    let none = open(Vec::new());
    assert!(none.iter().any(|p| p.severity == Severity::Warning && p.text.contains("no spawn")), "{none:?}");
    let outside = open(vec![LevelObject::Spawn { at: (-5, 0), notes: String::new() }]);
    assert!(
        outside.iter().any(|p| p.severity == Severity::Warning && p.text.contains("outside the level")),
        "{outside:?}"
    );
    assert!(!outside.iter().any(|p| p.text.contains("no spawn")), "{outside:?}");
}

/// The CRC-32 of PNG chunks (ISO 3309).
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    !c
}

/// `png` with a `tEXt` chunk before its `IEND`: the same pixels, other bytes.
fn with_text(png: &[u8]) -> Vec<u8> {
    let (body, iend) = png.split_at(png.len() - 12);
    let data = b"Comment\0from another editor";
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    let mut typed = b"tEXt".to_vec();
    typed.extend_from_slice(data);
    chunk.extend_from_slice(&typed);
    chunk.extend_from_slice(&crc32(&typed).to_be_bytes());
    [body, &chunk, iend].concat()
}

/// A layer another program wrote (other bytes, same pixels) is saved as it
/// was until it changes.
#[test]
fn a_foreign_layer_file_is_kept_until_changed() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    let path = dir.join("editor/layers/tunnels.png");
    let foreign = with_text(&std::fs::read(&path).unwrap());
    std::fs::write(&path, &foreign).unwrap();
    let mut doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir.clone(), &catalog(&root)).unwrap();
    assert!(files(&doc)["editor/layers/tunnels.png"] == foreign);
    doc.paint(
        "tunnels",
        &olr_edit::level::Shape::Rect { x: 0, y: 0, w: 2, h: 2 },
        &olr_edit::level::Paint::Colour(olr_data::image::Rgba::rgb(1, 2, 3)),
        None,
    )
    .unwrap();
    assert!(files(&doc)["editor/layers/tunnels.png"] != foreign);
}

/// Stray files in `editor/` are kept, and new elements' files go beside them.
#[test]
fn stray_editor_files_are_never_overwritten() {
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, &fixture());
    std::fs::write(dir.join("editor/images/photo.jpg"), b"not mine to replace").unwrap();
    std::fs::write(dir.join("editor/layers/layer.png"), b"nor this").unwrap();
    let mut doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir.clone(), &catalog(&root)).unwrap();
    let jpeg = std::fs::read(fixture().join("editor/images/tiny.jpg")).unwrap();
    doc.add_image("photo.jpg", jpeg, (10, 10)).unwrap();
    doc.add_layer(olr_format::levels::ElementMaterial::Dirt, "Layer").unwrap();
    let f = files(&doc);
    assert_eq!(f["editor/images/photo.jpg"], b"not mine to replace");
    assert_eq!(f["editor/layers/layer.png"], b"nor this");
    assert!(f.contains_key("editor/images/photo-2.jpg"));
    assert!(f.contains_key("editor/layers/layer-2.png"));
}

/// A snapshot makes the same pack as its document (plan 14c: the host's
/// recovery copy is encoded from one on a thread of its own).
#[test]
fn a_snapshot_packs_as_its_document() {
    let Some(root) = crate::common::content() else { return };
    let (_tmp, mut doc) = crate::common::open_fixture(&root);
    let shape = olr_edit::level::Shape::Rect { x: 10, y: 10, w: 20, h: 8 };
    doc.paint("tunnels", &shape, &olr_edit::level::Paint::Colour(olr_data::image::Rgba::rgb(9, 9, 9)), None)
        .unwrap();
    let snap = doc.snapshot();
    assert_eq!(snap.to_pack(), doc.to_pack());
    let moved = std::thread::spawn(move || snap.to_pack()).join().unwrap();
    assert_eq!(moved, doc.to_pack());
}

/// The playtest's request: a loaded map is three drawn layers, background,
/// dirt and rock, and erasing on dirt or rock leaves background there.
#[test]
fn a_plain_level_opens_as_three_layers_to_erase() {
    use olr_data::level::Material;
    use olr_edit::level::Shape;
    let Some(root) = content() else { return };
    let (_tmp, dir) = copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    let mut doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &catalog(&root)).unwrap();
    let ids: Vec<&str> = doc.items().iter().map(|i| i.element.id.as_str()).collect();
    assert_eq!(ids, ["background", "dirt", "rock"]);
    let l = doc.level();
    let w = l.size.0 as usize;
    let at = |m: Material| {
        l.material.iter().position(|c| *c == m).map(|i| ((i % w) as i32, (i / w) as i32)).unwrap()
    };
    let (dirt, rock) = (at(Material::Dirt), at(Material::Rock));
    for (layer, (x, y)) in [("dirt", dirt), ("rock", rock)] {
        doc.erase(layer, &Shape::Rect { x, y, w: 1, h: 1 }, false).unwrap();
        let l = doc.level();
        assert_eq!(
            l.material[y as usize * w + x as usize],
            Material::Background,
            "{layer} erased at ({x}, {y})"
        );
        let s = l.art_scale;
        let (ax, ay) = (x as u32 * s, y as u32 * s);
        assert_eq!(l.front.get(ax, ay), l.back.get(ax, ay), "the backdrop shows");
    }
}

/// A raster element (a level composed before) splits into the same three
/// layers in its place, as one step that changes nothing visible.
#[test]
fn a_raster_splits_into_three_layers() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let before = (doc.level().material.clone(), doc.level().front.clone(), doc.level().back.clone());
    let edits = doc.split("base").unwrap();
    assert_eq!(edits.len(), 4, "the raster out, three layers in");
    let ids: Vec<&str> = doc.items().iter().map(|i| i.element.id.as_str()).collect();
    assert_eq!(ids[..3], ["background", "dirt", "rock"], "{ids:?}");
    assert_eq!((doc.level().material.clone(), doc.level().front.clone(), doc.level().back.clone()), before);
    // Plan 14d R11: any element splits; a drawn background layer into one.
    let edits = doc.split("tunnels").unwrap();
    assert_eq!(edits.len(), 2, "the layer out, its Background in");
}

/// Reverts `edits` (one step), last first, as `Undo` does.
fn undo(doc: &mut Document, edits: &[olr_editor::Edit]) {
    for e in edits.iter().rev() {
        doc.apply(e, false);
    }
}

fn opened_dirt_level() -> Option<(tempfile::TempDir, Document)> {
    let root = content()?;
    let (tmp, dir) = copy_pack(&root, std::path::Path::new("olx-level-dirt-level"));
    Some((tmp, Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &catalog(&root)).unwrap()))
}

/// A world pixel of `m` in `doc`'s bake.
fn pixel_of(doc: &Document, m: olr_data::level::Material) -> (i32, i32) {
    let l = doc.level();
    let i = l.material.iter().position(|c| *c == m).unwrap();
    ((i % l.size.0 as usize) as i32, (i / l.size.0 as usize) as i32)
}

/// Review Focus 1: a loaded map's Dirt merged into its Background, then
/// erased, digs to the level's own background.
#[test]
fn erasing_a_merged_map_digs_to_its_background() {
    use olr_data::level::Material;
    use olr_editor::ops::Stroke;
    let Some((_tmp, mut doc)) = opened_dirt_level() else { return };
    let back = doc.level().back.clone();
    doc.merge_down("dirt").unwrap();
    let (x, y) = pixel_of(&doc, Material::Dirt);
    doc.begin_stroke("background", Stroke::Erase).unwrap();
    doc.erase("background", &olr_edit::level::Shape::Rect { x, y, w: 1, h: 1 }, false).unwrap();
    let l = doc.level();
    let (w, s) = (l.size.0 as usize, l.art_scale);
    assert_eq!(l.material[y as usize * w + x as usize], Material::Background);
    let (ax, ay) = (x as u32 * s, y as u32 * s);
    assert_eq!(l.front.get(ax, ay), back.get(ax, ay), "the level's own background shows");
}

/// Review Focus 2: a stroke that converted a plain layer to mixed, undone,
/// leaves a plain layer that saves no material file.
#[test]
fn an_undone_conversion_saves_a_plain_layer() {
    use olr_data::level::Material;
    use olr_editor::ops::Stroke;
    let Some((_tmp, mut doc)) = opened_dirt_level() else { return };
    let (x, y) = pixel_of(&doc, Material::Dirt);
    let mut step: Vec<olr_editor::Edit> =
        doc.begin_stroke("dirt", Stroke::Paint(Material::Rock)).unwrap().into_iter().collect();
    assert_eq!(step.len(), 1, "the conversion");
    let before = doc.stroke_before("dirt").unwrap();
    let shape = olr_edit::level::Shape::Rect { x, y, w: 2, h: 2 };
    let paint = olr_edit::level::Paint::Colour(olr_data::image::Rgba::rgb(9, 9, 9));
    let rect = doc.paint("dirt", &shape, &paint, Some(Material::Rock)).unwrap().unwrap();
    assert_eq!(doc.level().material[y as usize * doc.level().size.0 as usize + x as usize], Material::Rock);
    step.push(doc.stroke_edit("dirt", rect, &before).unwrap());
    assert!(files(&doc).contains_key("editor/layers/dirt.material.png"));
    undo(&mut doc, &step);
    let f = files(&doc);
    assert!(!f.contains_key("editor/layers/dirt.material.png"));
    let text = String::from_utf8(f["editor/composition.ron"].clone()).unwrap();
    assert!(!text.contains("mixed"), "{text}");
}

/// Review Focus 4: a layer moved off the level and back is as it was.
#[test]
fn a_layer_moved_off_and_back_is_unchanged() {
    let Some((_tmp, mut doc)) = opened_dirt_level() else { return };
    let start = files(&doc);
    doc.set("rock", Property::At((-500, 0))).unwrap();
    assert!(files(&doc) != start);
    doc.set("rock", Property::At((0, 0))).unwrap();
    assert!(files(&doc) == start);
}

#[test]
fn masking_an_image_erases_it_from_the_bake() {
    use olr_editor::ops::Stroke;
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let start = files(&doc);
    // `tiny` has no mask yet: the stroke's first edit gives it one.
    let conversion = doc.begin_stroke("tiny", Stroke::Erase).unwrap();
    assert!(conversion.is_some());
    let before = doc.stroke_before("tiny").unwrap();
    let shape = olr_edit::level::Shape::Circle { cx: 70, cy: 52, r: 10 };
    let rect = doc.erase("tiny", &shape, false).unwrap().unwrap();
    let edit = doc.stroke_edit("tiny", rect, &before).unwrap();
    assert!(matches!(edit, olr_editor::Edit::Mask { .. }), "{edit:?}");
    assert!(files(&doc) != start);
    // Restoring brings it back.
    let again = doc.stroke_before("tiny").unwrap();
    let r = doc.erase("tiny", &shape, true).unwrap().unwrap();
    let restored = doc.stroke_edit("tiny", r, &again).unwrap();
    undo(&mut doc, &[restored]);
    undo(&mut doc, &[conversion.unwrap(), edit]);
    assert!(files(&doc) == start, "undone, the image is as it was");
}

#[test]
fn remove_colour_masks_a_region() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let start = doc.level().clone();
    let edits = doc.remove_colour("gradient", (340, 230), 30).unwrap();
    assert!(!edits.is_empty());
    assert!(doc.level().front != start.front || doc.level().material != start.material);
    assert!(doc.remove_colour("gradient", (0, 0), 30).is_err(), "outside the image");
    undo(&mut doc, &edits);
    assert!(doc.level().front == start.front && doc.level().material == start.material);
}

/// C4: choosing a material on a mixed layer makes it plain, one step.
#[test]
fn a_mixed_layers_material_makes_it_plain() {
    use olr_format::levels::{ElementKind, ElementMaterial};
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let change = doc.set("wall", Property::Material(ElementMaterial::Rock)).unwrap();
    assert!(matches!(change.edit, olr_editor::Edit::Layer { .. }));
    let e = &doc.items()[doc.index_of("wall").unwrap()];
    assert!(matches!(
        e.element.kind,
        ElementKind::Drawn { material: ElementMaterial::Rock, mixed: false, .. }
    ));
}

#[test]
fn merge_down_is_refused_on_the_bottom_or_onto_an_image() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let start = files(&doc);
    assert!(matches!(doc.merge_down("base"), Err(DocError::Bad(_))));
    assert!(matches!(doc.merge_down("gradient"), Err(DocError::Bad(_))), "tiny below is an image");
    assert!(files(&doc) == start);
    let edits = doc.merge_down("wall").unwrap();
    assert_eq!(edits.len(), 2);
    undo(&mut doc, &edits);
    assert!(files(&doc) == start);
}

#[test]
fn a_locked_image_is_not_masked() {
    use olr_editor::ops::Stroke;
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    doc.set("tiny", Property::Locked(true)).unwrap();
    let shape = olr_edit::level::Shape::Circle { cx: 70, cy: 52, r: 10 };
    assert!(matches!(doc.begin_stroke("tiny", Stroke::Erase), Err(DocError::Locked(_))));
    assert!(matches!(doc.erase("tiny", &shape, false), Err(DocError::Locked(_))));
    assert!(matches!(doc.remove_colour("tiny", (70, 52), 10), Err(DocError::Locked(_))));
}

/// Plan 14d minors: erasing a plain Background layer with back art needs
/// no conversion: its background is never dug.
#[test]
fn erasing_a_plain_background_layer_converts_nothing() {
    use olr_editor::ops::Stroke;
    use olr_format::levels::{ElementKind, ElementMaterial};
    let Some((_tmp, mut doc)) = opened_dirt_level() else { return };
    doc.merge_down("dirt").unwrap();
    doc.set("background", Property::Material(ElementMaterial::Background)).unwrap();
    let e = &doc.items()[doc.index_of("background").unwrap()].element;
    assert!(matches!(e.kind, ElementKind::Drawn { mixed: false, back: true, .. }), "{:?}", e.kind);
    assert!(doc.begin_stroke("background", Stroke::Erase).unwrap().is_none());
}

fn kind_of<'a>(doc: &'a Document, id: &str) -> &'a olr_format::levels::ElementKind {
    &doc.items()[doc.index_of(id).unwrap()].element.kind
}

/// Plan 14e R6: a pasted copy is its own element; an image shares its
/// original and has its own mask.
#[test]
fn a_copy_pastes_as_an_independent_element() {
    use olr_format::levels::ElementKind;
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let s = doc.level().art_scale;
    let items = doc.copy(&["badge".to_string()]);
    let (edits, ids) = doc.paste(&items, s, (4, 4)).unwrap();
    assert_eq!(ids, ["badge-2"]);
    let e = &doc.items().last().unwrap().element;
    assert_eq!((e.id.as_str(), e.name.as_str()), ("badge-2", "Badge 2"));
    let ElementKind::Image { at, source, mask: true, .. } = &e.kind else { panic!("{:?}", e.kind) };
    assert_eq!((*at, source.as_str()), ((-2, 104), "badge.png"));
    let f = files(&doc);
    assert!(f.contains_key("editor/masks/badge-2.png") && f.contains_key("editor/masks/badge.png"));
    let mask = f["editor/masks/badge.png"].clone();
    // Erasing the copy's mask leaves the original's.
    doc.erase("badge-2", &olr_edit::level::Shape::Rect { x: 0, y: 104, w: 30, h: 30 }, false).unwrap();
    let f = files(&doc);
    assert_eq!(f["editor/masks/badge.png"], mask);
    assert_ne!(f["editor/masks/badge-2.png"], mask);
    // One undo of the paste's edits removes it.
    let n = doc.items().len();
    undo(&mut doc, &edits);
    assert_eq!(doc.items().len(), n - 1);
    assert!(doc.index_of("badge-2").is_none());
}

#[test]
fn a_drawn_copy_is_cropped_to_its_pixels() {
    use olr_format::levels::ElementKind;
    let Some(root) = content() else { return };
    let (_tmp, doc) = open_fixture(&root);
    let covered = |art: &olr_data::image::RgbaImage| art.as_raw().chunks(4).filter(|p| p[3] != 0).count();
    let whole = covered(doc.layer_art("tunnels").unwrap());
    assert!(whole > 0);
    let items = doc.copy(&["tunnels".to_string()]);
    let item = &items[0].item;
    let ElementKind::Drawn { size: Some(size), .. } = item.element.kind else {
        panic!("{:?}", item.element.kind)
    };
    assert!(size.0 < doc.level().size.0 || size.1 < doc.level().size.1, "cropped: {size:?}");
    let olr_edit::compose::Decoded::Drawn { art, .. } = &item.decoded else { panic!() };
    assert_eq!(covered(art), whole, "every painted pixel kept");
}

/// Review Focus 1: an image pasted into a level whose picture has the same
/// file name but other bytes takes a new name; both stay intact.
#[test]
fn a_pasted_image_never_overwrites_another_original() {
    use olr_format::levels::ElementKind;
    let Some(root) = content() else { return };
    let (_tmp, fixture) = open_fixture(&root);
    let tiny = std::fs::read(crate::common::fixture().join("editor/images/tiny.jpg")).unwrap();
    let other = std::fs::read(crate::common::fixture().join("editor/images/gradient.jpg")).unwrap();
    let items = fixture.copy(&["tiny".to_string()]);
    let Some((_t2, mut doc)) = opened_dirt_level() else { return };
    doc.add_image("tiny.jpg", other.clone(), (50, 50)).unwrap();
    let (_, ids) = doc.paste(&items, fixture.level().art_scale, (4, 4)).unwrap();
    let ElementKind::Image { source, .. } = kind_of(&doc, &ids[0]) else { panic!() };
    assert_eq!(source, "tiny-2.jpg");
    let dir = doc.dir().to_path_buf();
    olr_edit::pack::save(&dir, &doc.to_pack()).unwrap();
    let reopened = olr_edit::pack::load(&dir).unwrap();
    let files = &reopened.composition.as_ref().unwrap().files;
    assert_eq!(files["editor/images/tiny.jpg"], other);
    assert_eq!(files["editor/images/tiny-2.jpg"], tiny);
    // The same bytes under the same name are shared, not copied.
    let (_, again) = doc.paste(&items, fixture.level().art_scale, (8, 8)).unwrap();
    let ElementKind::Image { source, .. } = kind_of(&doc, &again[0]) else { panic!() };
    assert_eq!(source, "tiny-2.jpg");
}

/// Review Focus 2: copies between art scales are resampled; a raster into a
/// level of another size is refused.
#[test]
fn pasting_across_art_scales_resamples() {
    use olr_format::levels::ElementKind;
    let Some(root) = content() else { return };
    let (_tmp, fixture) = open_fixture(&root);
    let from = fixture.level().art_scale;
    let items = fixture.copy(&["tunnels".to_string(), "tiny".to_string()]);
    let Some((_t2, mut doc)) = opened_dirt_level() else { return };
    doc.set_art_scale(from * 2).unwrap();
    let to = doc.level().art_scale;
    let (_, ids) = doc.paste(&items, from, (4, 4)).unwrap();
    let d = doc.items();
    for (id, src) in ids.iter().zip(&items) {
        let item = &d[doc.index_of(id).unwrap()];
        match (&item.element.kind, &item.decoded, &src.item.decoded) {
            (
                ElementKind::Drawn { size: Some(size), .. },
                olr_edit::compose::Decoded::Drawn { art, .. },
                olr_edit::compose::Decoded::Drawn { art: old, .. },
            ) => {
                assert_eq!((art.width(), art.height()), (size.0 * to, size.1 * to));
                assert_eq!((art.width(), art.height()), (old.width() * 2, old.height() * 2));
            }
            (
                ElementKind::Image { .. },
                olr_edit::compose::Decoded::Image { grid: Some(g), .. },
                olr_edit::compose::Decoded::Image { grid: Some(old), .. },
            ) => {
                // The processed grid is the box × art scale ÷ pixel size.
                assert_eq!((g.width(), g.height()), (old.width() * 2, old.height() * 2));
            }
            k => panic!("{k:?}"),
        }
    }
    // The fixture's base raster is 504 × 350; the level made another size.
    doc.resize((300, 200), (0, 0), olr_data::level::Material::Dirt, olr_data::image::Rgba::rgb(1, 1, 1))
        .unwrap();
    let base = fixture.copy(&["base".to_string()]);
    let e = doc.paste(&base, from, (4, 4)).err();
    assert!(matches!(&e, Some(DocError::Bad(m)) if m.contains("its size")), "{e:?}");
}

/// Spec C3: pasted pixels keep their material.
#[test]
fn copied_rock_pixels_stay_rock() {
    use olr_data::level::Material;
    use olr_format::levels::ElementKind;
    let Some((_tmp, mut doc)) = opened_dirt_level() else { return };
    let (x, y) = pixel_of(&doc, Material::Rock);
    let rect = olr_edit::compose::Rect { x: x as u32, y: y as u32, w: 1, h: 1 };
    let (clip, at) = doc.copy_pixels("rock", rect).unwrap();
    assert_eq!((clip.size, at), ((1, 1), (x, y)));
    assert_eq!(clip.cells, [Material::Rock]);
    let s = doc.level().art_scale;
    let (_, id) = doc.paste_pixels(&clip, s, (x + 4, y + 4)).unwrap();
    assert!(
        matches!(
            kind_of(&doc, &id),
            ElementKind::Drawn { material: olr_format::levels::ElementMaterial::Rock, mixed: false, .. }
        ),
        "{:?}",
        kind_of(&doc, &id)
    );
    // A clip of rock and dirt makes a mixed layer.
    let mut mixed = clip.clone();
    mixed.size = (2, 1);
    mixed.cells = vec![Material::Rock, Material::Dirt];
    let mut art = olr_data::image::RgbaImage::new(2 * s, s);
    for ay in 0..s {
        for ax in 0..2 * s {
            art.set(ax, ay, olr_data::image::Rgba::rgb(5, 5, 5));
        }
    }
    mixed.art = art;
    let (_, id) = doc.paste_pixels(&mixed, s, (x, y)).unwrap();
    assert!(matches!(kind_of(&doc, &id), ElementKind::Drawn { mixed: true, .. }), "{:?}", kind_of(&doc, &id));
}

#[test]
fn copy_pixels_outside_the_layer_is_none() {
    let Some(root) = content() else { return };
    let (_tmp, doc) = open_fixture(&root);
    // `wall` is boxed at (4, 0): the level's first columns are outside it.
    let r = olr_edit::compose::Rect { x: 0, y: 0, w: 4, h: 4 };
    assert!(doc.copy_pixels("wall", r).is_none());
    assert!(doc.copy_pixels("tiny", r).is_none(), "a picture has no pixels to copy");
    assert!(doc.copy_pixels("nope", r).is_none());
}

/// Minors: a picture whose name differs from one here only in case takes
/// a new name.
#[test]
fn a_case_only_clash_takes_a_new_name() {
    use olr_format::levels::ElementKind;
    let Some(root) = content() else { return };
    let (_tmp, fixture) = open_fixture(&root);
    let other = std::fs::read(crate::common::fixture().join("editor/images/gradient.jpg")).unwrap();
    let items = fixture.copy(&["tiny".to_string()]);
    let Some((_t2, mut doc)) = opened_dirt_level() else { return };
    doc.add_image("TINY.JPG", other, (50, 50)).unwrap();
    let (_, ids) = doc.paste(&items, fixture.level().art_scale, (4, 4)).unwrap();
    let ElementKind::Image { source, .. } = kind_of(&doc, &ids[0]) else { panic!() };
    assert_eq!(source, "tiny-2.jpg");
}

/// Minors: a paste refused partway (the element limit) pastes nothing.
#[test]
fn a_paste_refused_partway_pastes_nothing() {
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let red = olr_edit::level::Paint::Colour(olr_data::image::Rgba::rgb(255, 0, 0));
    doc.add_layer(olr_format::levels::ElementMaterial::Dirt, "Dot").unwrap();
    doc.paint("dot", &olr_edit::level::Shape::Rect { x: 5, y: 5, w: 1, h: 1 }, &red, None).unwrap();
    let one = doc.copy(&["dot".to_string()]);
    let s = doc.level().art_scale;
    while doc.items().len() < olr_format::levels::MAX_ELEMENTS - 1 {
        doc.paste(&one, s, (0, 0)).unwrap();
    }
    let two = doc.copy(&["dot".to_string(), "tunnels".to_string()]);
    let n = doc.items().len();
    assert!(doc.paste(&two, s, (4, 4)).is_err());
    assert_eq!(doc.items().len(), n, "the first copy rolled back");
}

/// Minors: a pasted layer's material breaks a tie towards the higher
/// priority (rock, then dirt).
#[test]
fn a_pasted_layers_material_tie_goes_to_rock() {
    use olr_data::level::Material;
    use olr_format::levels::{ElementKind, ElementMaterial};
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let s = doc.level().art_scale;
    let mut art = olr_data::image::RgbaImage::new(2 * s, s);
    for ay in 0..s {
        for ax in 0..2 * s {
            art.set(ax, ay, olr_data::image::Rgba::rgb(5, 5, 5));
        }
    }
    let clip = olr_editor::ops::PixelClip { size: (2, 1), art, cells: vec![Material::Dirt, Material::Rock] };
    let (_, id) = doc.paste_pixels(&clip, s, (10, 10)).unwrap();
    assert!(
        matches!(kind_of(&doc, &id), ElementKind::Drawn { material: ElementMaterial::Rock, .. }),
        "{:?}",
        kind_of(&doc, &id)
    );
}

/// The user's note (2026-10-19): a look set in the editor shows in the
/// saved thumbnail (drawn lit), though nothing was rebaked.
#[test]
fn a_look_shows_in_the_saved_thumbnail() {
    let Some(root) = crate::common::content() else { return };
    let (_tmp, mut doc) = crate::common::open_fixture(&root);
    let before = crate::common::files(&doc)["thumb.png"].clone();
    let dark = olr_format::look::LookSource {
        ambient: olr_format::look::AmbientSource { colour: None, intensity: 10 },
        ..Default::default()
    };
    doc.set_look(Some(dark)).unwrap();
    assert_ne!(crate::common::files(&doc)["thumb.png"], before);
}

/// Final review I3: past `MAX_GLOW_BEAMS` the renderer draws no beams (and
/// Save refuses the level), so the live diagnostics say so: 65 separate
/// directional dots are 65 beams, an error; 64 are none.
#[test]
fn diagnostics_count_the_glow_beams() {
    use olr_edit::level::Shape;
    let Some(root) = content() else { return };
    let (_tmp, mut doc) = open_fixture(&root);
    let beam = Some(olr_format::levels::GlowBeam { direction: 0, spread: 35 });
    let glow = olr_format::levels::ElementGlow { strength: 100, group: String::new(), mask: false, beam };
    doc.set_glow("tiny", Some(glow)).unwrap();
    // The picture at (40, 30), 60 × 45: no glow but 13 × 5 dots two pixels apart.
    doc.paint_glow("tiny", &Shape::Rect { x: 40, y: 30, w: 60, h: 45 }, 0).unwrap();
    let dots: Vec<(i32, i32)> = (0..5).flat_map(|j| (0..13).map(move |i| (40 + 2 * i, 30 + 2 * j))).collect();
    for &(x, y) in &dots[..64] {
        doc.paint_glow("tiny", &Shape::Rect { x, y, w: 1, h: 1 }, 100).unwrap();
    }
    let beams = |doc: &Document| {
        doc.diagnostics().into_iter().filter(|p| p.text.contains("glow beams")).collect::<Vec<_>>()
    };
    assert!(beams(&doc).is_empty(), "64: {:?}", beams(&doc));
    let (x, y) = dots[64];
    doc.paint_glow("tiny", &Shape::Rect { x, y, w: 1, h: 1 }, 100).unwrap();
    let p = beams(&doc);
    assert!(
        matches!(&p[..], [p] if p.severity == Severity::Error && p.text.contains("65 glow beams, above 64")),
        "65: {p:?}"
    );
}
