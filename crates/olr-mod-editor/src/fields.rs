//! The field table (`fields.ron`, issue #17): every field of the mod
//! format by `Type.field` (`Type::Variant.field` in an enum, `.0` for a
//! tuple variant's; each variant by `Type::Variant`), with its type, the
//! format's doc comment as the hover text, and the editor's own columns:
//! the unit shown in the label, a soft range for the slider (any value may
//! be typed: the format validates), where it shows (Common, More, or
//! Panel: one of the editor's own panels draws it) and, for a pack path,
//! the asset folder whose files a dropdown offers. `cargo xtask
//! format-docs` writes the generated columns and keeps the others;
//! `check` fails when the table drifts from the format.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// The table as checked in.
pub const TABLE: &str = include_str!("../fields.ron");

/// Where a field shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Place {
    /// With the panel's first fields.
    Common,
    /// Under More ▸.
    More,
    /// Drawn by one of the editor's own panels (a fire's kind, a spawn's
    /// projectile, an event): not by the generic fields.
    Panel,
}

/// One field (or variant) of the format.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldInfo {
    /// `Worm.gravity`, `Fire::Beam.width`, `Effect::Explode`.
    pub key: String,
    /// The Rust type as written (`f32`, `Option<String>`), or `variant`.
    pub ty: String,
    /// Shown in the label (`px/s²`); empty for none.
    pub unit: String,
    /// The slider's soft range; none: a number field alone.
    pub range: Option<(f64, f64)>,
    pub place: Place,
    /// `gfx` or `sfx`: the value is a pack path offered from that folder.
    pub asset: Option<String>,
    /// The format's doc comment.
    pub help: String,
}

impl FieldInfo {
    /// The field's name (after the type's prefix).
    pub fn name(&self) -> &str {
        self.key.rsplit_once('.').map_or(self.key.as_str(), |(_, n)| n)
    }

    /// The label: the field's name in words, with its unit.
    pub fn label(&self) -> String {
        let mut words = self.name().replace('_', " ");
        if let Some(first) = words.get(..1) {
            words = first.to_uppercase() + &words[1..];
        }
        if self.unit.is_empty() { words } else { format!("{words} ({})", self.unit) }
    }
}

struct Table {
    rows: Vec<FieldInfo>,
    by_key: BTreeMap<String, usize>,
}

fn table() -> &'static Table {
    static TABLE_ROWS: OnceLock<Table> = OnceLock::new();
    TABLE_ROWS.get_or_init(|| {
        // Checked in and tested (`the_table_reads`): never content.
        let rows: Vec<FieldInfo> = ron::from_str(TABLE).unwrap_or_default();
        let by_key = rows.iter().enumerate().map(|(i, r)| (r.key.clone(), i)).collect();
        Table { rows, by_key }
    })
}

/// Every row, in the table's order (types in order of first reference
/// from `mod.ron`, a weapon and a projectile; fields as declared).
pub fn rows() -> &'static [FieldInfo] {
    &table().rows
}

/// The row of `key`.
pub fn get(key: &str) -> Option<&'static FieldInfo> {
    let t = table();
    t.by_key.get(key).map(|&i| &t.rows[i])
}

/// The fields of type (or variant) `owner` (`Worm`, `Fire::Beam`), in
/// order.
pub fn fields_of(owner: &str) -> impl Iterator<Item = &'static FieldInfo> {
    let owner = owner.to_string();
    rows()
        .iter()
        .filter(move |r| r.ty != "variant" && r.key.rsplit_once('.').is_some_and(|(o, _)| o == owner))
}

/// The hover text of variant `variant` of `ty` (`Effect`, `Explode`).
pub fn variant_help(ty: &str, variant: &str) -> Option<&'static str> {
    get(&format!("{ty}::{variant}")).map(|r| r.help.as_str())
}

/// How a field's value is edited, by its type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Widget {
    /// `f32`: a number with a slider over the soft range.
    Float,
    /// `i32`, `u32`: a whole number with a slider.
    Int,
    Bool,
    /// `String`: a text field (or, with `asset`, a dropdown of files).
    Text,
    /// `Option<String>`: as `Text`, or none.
    OptText,
    /// `Colour`: `#rrggbb`.
    Colour,
    /// `Option<i32>`: a whole number, or none (the default).
    OptInt,
    /// `Option<(i32, i32)>`: two whole numbers, or none.
    OptSize,
    /// An enum without fields: a dropdown of its variants.
    Choice,
}

/// The widget for type `ty` (the table's), if the generic fields edit it.
pub fn widget(ty: &str) -> Option<Widget> {
    Some(match ty {
        "f32" => Widget::Float,
        "i32" | "u32" => Widget::Int,
        "bool" => Widget::Bool,
        "String" => Widget::Text,
        "Option<String>" => Widget::OptText,
        "Colour" => Widget::Colour,
        "Option<i32>" => Widget::OptInt,
        "Option<(i32,i32)>" => Widget::OptSize,
        "WeaponClass" | "SpecialKind" | "AnimMode" => Widget::Choice,
        _ => return None,
    })
}
