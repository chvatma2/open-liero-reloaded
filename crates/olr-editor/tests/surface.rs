//! Issue #37: a plain level's surface maps in the editor. Opened as three
//! layers they bake back as they were; a resize re-lays them with the
//! layers (the new area none) and its undo brings them back.

use olr_data::image::Rgba;
use olr_data::level::Material;
use olr_data::manifest::PackKind;
use olr_edit::new::{Template, new_pack};
use olr_edit::pack::Source;
use olr_format::levels::SurfaceMaps;
use olr_format::look::LookSource;
use olr_format::materials::MaterialSource;

use crate::common::{catalog, content};

fn material(id: &str) -> MaterialSource {
    MaterialSource { id: id.into(), name: id.into(), notes: String::new(), heat: None }
}

#[test]
fn a_plain_levels_surface_maps_follow_a_resize() {
    let Some(root) = content() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("surfaces");
    new_pack(
        &dir,
        PackKind::Level,
        "surfaces",
        "Surfaces",
        Template::Level { size: (8, 4), theme: "olx-theme-dirt" },
    )
    .unwrap();
    let mut pack = olr_edit::pack::load(&dir).unwrap();
    let Source::Level(level) = &mut pack.source else { panic!("a level") };
    level.look =
        Some(LookSource { materials: vec![material("stone"), material("wood")], ..LookSource::default() });
    // Every pixel is solid (dirt in a rock border): stone in front, wood
    // behind.
    let maps = SurfaceMaps { world: (8, 4), front: vec![1; 32], back: vec![2; 32] };
    level.surface = Some(maps.clone());
    olr_edit::pack::save(&dir, &pack).unwrap();
    let pack = olr_edit::pack::load(&dir).unwrap();
    let mut doc = olr_editor::Document::open(pack, dir, &catalog(&root)).unwrap();
    assert_eq!(doc.level().surface.as_ref(), Some(&maps));

    let change = doc.resize((10, 4), (2, 0), Material::Rock, Rgba::rgb(1, 1, 1)).unwrap();
    let got = doc.level().surface.clone().expect("maps kept");
    assert_eq!(got.world, (10, 4));
    for y in 0..4 {
        let row = |m: &[u8]| m[y * 10..y * 10 + 10].to_vec();
        assert_eq!(row(&got.front), [0, 0, 1, 1, 1, 1, 1, 1, 1, 1], "front, row {y}");
        assert_eq!(row(&got.back), [0, 0, 2, 2, 2, 2, 2, 2, 2, 2], "back, row {y}");
    }
    // Saved, the painted maps fit and bake the same.
    let Source::Level(saved) = doc.to_pack().source else { panic!("a level") };
    assert_eq!(saved.surface, Some(got));
    doc.apply(&change.edit, false);
    assert_eq!(doc.level().surface.as_ref(), Some(&maps));
}
