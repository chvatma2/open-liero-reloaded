//! One authoring file's text in canonical form (plan 13d): what an editor
//! saving a single file, and the formats guide's checked examples, need.

use std::collections::BTreeMap;

use olr_data::restrictions::Preset;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::FormatError;
use crate::ron_io::{from_canonical, to_canonical};

/// The ground's `asset.ron` in a level pack (issue #74).
const GROUND_ASSET_FILE: &str = "editor/asset.ron";

/// `text`, the file at `path` in a pack (`level.ron`, `weapons/<id>.ron`,
/// ...), parsed as that file's type and written again in canonical form.
/// Only the file itself is checked (no references, no images).
pub fn canonical_text(path: &str, text: &str) -> Result<String, FormatError> {
    fn again<T: Serialize + DeserializeOwned>(path: &str, text: &str) -> Result<String, FormatError> {
        to_canonical(path, &from_canonical::<T>(path, text)?)
    }
    // As the mod reader: `<id>.ron` directly in the folder, `id` valid.
    let definition = |dir: &str| {
        path.strip_prefix(dir).and_then(|r| r.strip_suffix(".ron")).is_some_and(crate::mods::valid_id)
    };
    match path {
        "mod.ron" => again::<crate::mods::ModFile>(path, text),
        "restrictions.ron" => again::<BTreeMap<String, Preset>>(path, text),
        _ if definition("weapons/") => again::<crate::mods::Weapon>(path, text),
        _ if definition("projectiles/") => again::<crate::mods::Projectile>(path, text),
        "level.ron" => again::<crate::levels::LevelFile>(path, text),
        "skin.ron" => again::<crate::skins::SkinFile>(path, text),
        "theme.ron" => again::<crate::themes::ThemeFile>(path, text),
        "core.ron" => again::<crate::core_pack::CoreFile>(path, text),
        crate::mods::MOD_EDITOR_FILE => again::<crate::mods::ModEditorFile>(path, text),
        crate::levels::COMPOSITION_FILE => again::<crate::levels::Composition>(path, text),
        crate::levels::PLACED_FILE => again::<crate::levels::PlacedFile>(path, text),
        // The played form of the live copies (issue #85).
        crate::levels::LIVE_FILE => again::<crate::levels::LiveFile>(path, text),
        // The ground's `asset.ron` (issue #74).
        GROUND_ASSET_FILE => again::<crate::assets::AssetFile>(path, text),
        _ => Err(FormatError::invalid(path, "", "not an authoring file this format reads")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hand_edited_file_becomes_canonical() {
        let text = "(theme:\"t\",  size:(4,3), art_scale: 1, name: \"L\")";
        assert_eq!(
            canonical_text("level.ron", text).unwrap(),
            "(\n    name: \"L\",\n    size: (4, 3),\n    theme: \"t\",\n)\n"
        );
        let weapon =
            "(name:\"G\",recharge:1.0,drain:10.0,between_shots_s:0.1,fire:Special(kind:Jetpack,thrust:3))";
        let canonical = canonical_text("weapons/g.ron", weapon).unwrap();
        assert_eq!(canonical_text("weapons/g.ron", &canonical).unwrap(), canonical);
        assert!(canonical.contains("    fire: Special(\n"), "{canonical}");
    }

    #[test]
    fn a_composition_becomes_canonical() {
        assert_eq!(
            canonical_text("editor/composition.ron", "(elements:[])").unwrap(),
            "(\n    elements: [],\n)\n"
        );
    }

    #[test]
    fn errors_name_the_file() {
        let e = canonical_text("skin.ron", "(name: \"x\", oops: 1)").unwrap_err();
        assert!(e.to_string().contains("skin.ron"), "{e}");
        assert!(canonical_text("notes.txt", "hi").is_err());
        assert!(canonical_text("projectiles/a/b.ron", "()").is_err());
        // Review C-4: as the mod reader, only a valid id's file is a definition.
        for path in ["weapons/.ron", "weapons/.x.ron", "projectiles/Bad Id.ron"] {
            let e = canonical_text(path, "()").unwrap_err();
            assert!(e.to_string().contains("not an authoring file"), "{path}: {e}");
        }
    }
}
