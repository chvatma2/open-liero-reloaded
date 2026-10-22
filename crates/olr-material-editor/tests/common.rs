//! Helpers shared by the test modules: a small library, no content needed.

use olr_edit::material::MaterialEntry;
use olr_format::materials::{HeatLookSource, HeatSource, MaterialSource};

/// A material of id `id`, name `name`, heating as `capacity` says.
pub fn material(id: &str, name: &str, capacity: u16) -> MaterialSource {
    MaterialSource {
        id: id.into(),
        name: name.into(),
        notes: String::new(),
        heat: Some(HeatSource {
            sensitivity: 100,
            capacity,
            conductivity: 100,
            cooling: 100,
            look: HeatLookSource::default(),
        }),
    }
}

/// Two built-in materials (Stone, Wood), then the user's Lava rock.
pub fn library() -> Vec<MaterialEntry> {
    vec![
        MaterialEntry { builtin: true, material: material("stone", "Stone", 100) },
        MaterialEntry { builtin: true, material: material("wood", "Wood", 50) },
        MaterialEntry { builtin: false, material: material("lava-rock", "Lava rock", 120) },
    ]
}
