//! Surface material libraries (issue #37; the format is
//! `olr_format::materials`): the game's built-in materials,
//! `content/materials/<id>.ron` beside the packs (read-only), and the
//! user's, `materials/` in the game's data directory. Listed as the asset
//! library is (`crate::asset::list_library`): the built-in ones first,
//! then the user's, at most [`MAX_USER_MATERIALS`] of them; a file that
//! does not read is a warning and left out.

use std::path::Path;

use olr_format::materials::{MaterialSource, read_material, valid_material_id};
use olr_format::ron_io::MAX_RON_BYTES;

/// The most materials of the user's library that are offered.
pub const MAX_USER_MATERIALS: usize = 256;

/// A material of a library.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialEntry {
    /// One of the game's (read-only), not the user's.
    pub builtin: bool,
    pub material: MaterialSource,
}

/// The material files of library `dir` (`<id>.ron` with a valid id) by
/// id, sorted; none when it cannot be read.
fn material_ids(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut ids: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".ron").map(str::to_string))
        .filter(|id| valid_material_id(id))
        .collect();
    ids.sort();
    ids
}

/// Material file `path` read (at most `MAX_RON_BYTES`) and checked: its
/// id must be its file's name.
pub fn read_file(path: &Path) -> Result<MaterialSource, String> {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > MAX_RON_BYTES {
        return Err(format!("{len} bytes, above {MAX_RON_BYTES}"));
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    read_material(name, &text).map_err(|e| e.to_string())
}

/// The materials of library `dir`, in name order (then id), at most
/// `max`; the files that do not read as warnings. A missing directory is
/// an empty library.
pub fn list_library(dir: &Path, max: usize) -> (Vec<MaterialSource>, Vec<String>) {
    let (mut list, mut warnings) = (Vec::new(), Vec::new());
    let mut ids = material_ids(dir);
    if ids.len() > max {
        warnings.push(format!(
            "the material library {} holds more than {max} materials: the first {max} are offered",
            dir.display()
        ));
        ids.truncate(max);
    }
    for id in ids {
        match read_file(&dir.join(format!("{id}.ron"))) {
            Ok(m) => list.push(m),
            Err(why) => warnings.push(format!("material {id}: {why}: not offered")),
        }
    }
    list.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    (list, warnings)
}

/// Every material offered: the built-in library `builtin` (all of it),
/// then the user's `user` (at most [`MAX_USER_MATERIALS`]), with the
/// warnings of both.
pub fn list_materials(builtin: Option<&Path>, user: Option<&Path>) -> (Vec<MaterialEntry>, Vec<String>) {
    let (mut out, mut warnings) = (Vec::new(), Vec::new());
    for (dir, builtin, max) in [(builtin, true, usize::MAX), (user, false, MAX_USER_MATERIALS)] {
        let Some(dir) = dir else { continue };
        let (list, why) = list_library(dir, max);
        out.extend(list.into_iter().map(|material| MaterialEntry { builtin, material }));
        warnings.extend(why);
    }
    (out, warnings)
}
