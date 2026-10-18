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
