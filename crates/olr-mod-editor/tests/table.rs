//! The field table (`fields.ron`): it reads, and every field the generic
//! fields show has a widget, so a new format field cannot be dropped
//! unseen (`cargo xtask format-docs --check` keeps the table equal to the
//! format).

use olr_mod_editor::fields::{self, Place, Widget};

#[test]
fn the_table_reads() {
    let rows: Vec<fields::FieldInfo> = ron::from_str(fields::TABLE).unwrap();
    assert_eq!(fields::rows(), rows.as_slice());
    assert!(rows.len() > 100, "{}", rows.len());
    let w = fields::get("Worm.gravity").unwrap();
    assert_eq!((w.unit.as_str(), w.place, w.label()), ("px/s²", Place::Common, "Gravity (px/s²)".into()));
    assert!(w.help.contains("vertical velocity"));
    let names: Vec<&str> = fields::fields_of("Fire::Beam").map(|r| r.name()).collect();
    assert_eq!(names[..2], ["recoil", "colour"]);
    assert!(fields::variant_help("Effect", "Explode").is_some_and(|h| h.contains("blast")));
}

#[test]
fn every_shown_field_has_a_widget() {
    for r in fields::rows().iter().filter(|r| r.ty != "variant" && r.place != Place::Panel) {
        let w = fields::widget(&r.ty);
        assert!(w.is_some(), "{} ({}) has no widget: make it a Panel or give it one", r.key, r.ty);
        if r.asset.is_some() {
            assert!(matches!(w, Some(Widget::Text | Widget::OptText)), "{}: an asset is a path", r.key);
        }
        if let Some((lo, hi)) = r.range {
            assert!(lo < hi && matches!(w, Some(Widget::Float | Widget::Int | Widget::OptInt | Widget::OptSize)));
        }
    }
}
