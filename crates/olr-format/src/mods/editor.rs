//! `editor/mod.ron`: the mod editor's own file in a mod pack (issue #17),
//! beside the definitions as a level keeps its composition in `editor/`.
//! Authoring only: never content (not hashed, never downloaded), never read
//! by the game.

use serde::{Deserialize, Serialize};

/// The mod editor's file's path in a mod pack.
pub const MOD_EDITOR_FILE: &str = "editor/mod.ron";

/// What the mod editor remembers of a mod: the mod it was copied from,
/// whose values it shows beside the fields that differ (Original, Reset).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModEditorFile {
    /// The pack id of the mod this one was copied from (`olx-classic`);
    /// none for a mod made otherwise. The manifest's `origin` says where a
    /// pack was imported from, so a copy clears it: this keeps the editor's
    /// own reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}
