//! The level's loose materials in the editor (the loose dirt design,
//! `docs/superpowers/specs/2026-11-05-loose-dirt-design.md`): added with
//! the default rates, changed, removed when no dirt layer names them; a
//! dirt layer names one through `Property::Loose`. Each checks first,
//! then goes through `Document::apply` (`Edit::Loose`), as every
//! operation does.

use olr_edit::compose::Rect;
use olr_format::levels::{LooseSource, MAX_LOOSE};

use crate::document::{DocError, Document};
use crate::history::Edit;

/// A new loose material's id stem and name.
pub const NEW_LOOSE_ID: &str = "loose";
pub const NEW_LOOSE_NAME: &str = "Loose dirt";

impl Document {
    /// The level's loose materials, in order: `loose.png`'s value of
    /// `loose()[i]` is `i + 1`.
    pub fn loose(&self) -> &[LooseSource] {
        &self.level.loose
    }

    /// The elements naming loose material `id`, by name, bottom to top.
    pub fn loose_users(&self, id: &str) -> Vec<String> {
        self.items
            .iter()
            .map(|i| &i.element)
            .filter(|e| e.loose.as_deref() == Some(id))
            .map(|e| if e.name.is_empty() { e.id.clone() } else { e.name.clone() })
            .collect()
    }

    /// Adds a loose material of the default rates, `NEW_LOOSE_NAME`, its
    /// id `NEW_LOOSE_ID` made unique (`-2`, `-3`, …). One step.
    pub fn add_loose(&mut self) -> Result<(Vec<Edit>, Rect), DocError> {
        if self.level.loose.len() >= MAX_LOOSE {
            return Err(DocError::Bad(format!("a level holds at most {MAX_LOOSE} loose materials")));
        }
        let taken = |id: &str| self.level.loose.iter().any(|m| m.id == id);
        let id = (1..)
            .map(|n| if n == 1 { NEW_LOOSE_ID.to_string() } else { format!("{NEW_LOOSE_ID}-{n}") })
            .find(|id| !taken(id))
            .unwrap_or_default();
        let mut after = self.level.loose.clone();
        after.push(LooseSource::new(&id, NEW_LOOSE_NAME));
        self.set_loose_list(after)
    }

    /// Replaces loose material `id`'s name, notes and rates (its id kept).
    pub fn set_loose(&mut self, id: &str, material: LooseSource) -> Result<(Vec<Edit>, Rect), DocError> {
        let mut after = self.level.loose.clone();
        let Some(m) = after.iter_mut().find(|m| m.id == id) else {
            return Err(DocError::Bad(format!("no loose material {id:?}")));
        };
        *m = LooseSource { id: id.into(), ..material };
        self.set_loose_list(after)
    }

    /// Removes loose material `id`, refused while an element names it
    /// (named). The materials after it move down one (the bake follows).
    pub fn remove_loose(&mut self, id: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        let Some(m) = self.level.loose.iter().find(|m| m.id == id) else {
            return Err(DocError::Bad(format!("no loose material {id:?}")));
        };
        let users = self.loose_users(id);
        if !users.is_empty() {
            return Err(DocError::Bad(format!(
                "{} is used by {}: make them plain dirt first",
                m.name,
                users.join(", ")
            )));
        }
        let mut after = self.level.loose.clone();
        after.retain(|m| m.id != id);
        self.set_loose_list(after)
    }

    /// The list made `after`, checked as `level.ron`'s (ids, names, the
    /// rates' ranges, the count). One step.
    fn set_loose_list(&mut self, after: Vec<LooseSource>) -> Result<(Vec<Edit>, Rect), DocError> {
        olr_format::loose::check_loose(&after, "level.ron", "loose")
            .map_err(|e| DocError::Bad(e.to_string()))?;
        let before = self.level.loose.clone();
        Ok(self.apply_all(vec![Edit::Loose { before, after }]))
    }
}
