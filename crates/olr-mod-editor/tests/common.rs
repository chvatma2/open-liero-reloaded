//! Helpers shared by the test modules: the editor on a copy of Classic.

use std::path::PathBuf;

use olr_edit::mods;
use olr_edit::pack::{self, Pack};
use olr_mod_editor::ModEditorState;

pub const CLASSIC: &str = "olx-classic";
pub const MW: &str = "olx-mw-1-0";

/// The bundled content root, when `OLR_CONTENT_DIR` names a directory.
pub fn content() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
    dir.is_dir().then_some(dir)
}

/// Installed mod `id`.
pub fn installed(id: &str) -> Option<Pack> {
    pack::load(&content()?.join(id)).ok()
}

/// A copy of Classic named "My Classic" (as the host makes one), its base
/// Classic.
pub fn classic_copy() -> Option<Pack> {
    let mut p = installed(CLASSIC)?;
    p.manifest.id = "my-classic".into();
    mods::rename_mod(&mut p, "My Classic").unwrap();
    ModEditorState::with_base(&mut p, Some(CLASSIC));
    Some(p)
}

/// The editor on a copy of Classic, Classic its base.
pub fn editor() -> Option<ModEditorState> {
    let pack = classic_copy()?;
    let base = mods::source(&installed(CLASSIC)?).unwrap().clone();
    let mods = vec![(CLASSIC.to_string(), "Liero v1.0".to_string()), (MW.to_string(), "MW 1.0".to_string())];
    Some(ModEditorState::new(pack, Some(base), mods, false).unwrap())
}
