//! The open level's links to the asset library (issue #83; the asset-based
//! levels design's "The library, export and import", calls 7, 9, 17 and
//! 18): the library owns its assets, and a level holds a copy of each it
//! uses. The state never reads the library: it asks the host
//! (`EditorCommand::CompareLibrary`, the level's linked origins) and the
//! host answers with the library's sources (`EditorAction::
//! LibraryCompared`, `olr_edit::library::read_origins`), on which
//! `olr_edit::library`'s pure `compare`, `update`, `relink` and `adopt`
//! run as level steps (`Edit::Placement`, named by `Edit::Name`):
//!
//! - **A library save reaching the level**: after `AssetSaved` and
//!   `AssetsChanged`, a changed asset without breaks is updated, one step
//!   ("Update <asset>"); one that would break copies is not, the status
//!   line saying so; a missing one changes nothing.
//! - **Placing again**: a copy of the library's version when it differs
//!   from the level's (`olr_edit::placed::add_copy` gives it a new key),
//!   then the level's older copies are offered it as above, the read
//!   source standing for the library.
//! - **Unlink** (`adopt`), **Save to library** (an own asset written to
//!   the user's library, then linked to it: `relink` to the files written)
//!   and **Use your copy in this level** (the level's copies of a built-in
//!   asset relinked to the user's edited copy of it).
//!
//! After an update or a relink the level's assets of one origin that are
//! now the same are made one (`merge_same`): a level holds two versions of
//! a library asset only while an update is held back.

use std::sync::Arc;

use olr_edit::library::{self, Break, Libraries, LibraryError, Link, Status};
use olr_edit::pack::Pack;
use olr_edit::placed::same_asset;
use olr_format::assets::AssetSource;
use olr_format::levels::{AssetOrigin, ElementKind, GROUND_KEY};

use crate::catalog::AssetKey;
use crate::document::{DocError, Document};
use crate::ops::Change;
use crate::state::{DocTab, EditorCommand, EditorState};

/// The offer of Use your copy in this level (issue #83): the asset tab
/// saved built-in asset `builtin` (folder) as the user's `yours` (folder,
/// its files `source`), and the open level has copies of the built-in one.
#[derive(Clone, Debug, PartialEq)]
pub struct UseYours {
    pub builtin: String,
    pub yours: String,
    pub name: String,
    pub source: Arc<AssetSource>,
}

/// "1 copy" or "3 copies".
pub fn copies_said(n: usize) -> String {
    if n == 1 { "1 copy".into() } else { format!("{n} copies") }
}

/// Why asset `src` (the level's own; `ground` for the level's ground) may
/// not go in the library, said plainly: a distant or raster layer first,
/// then a glow group no layer glows in, then `check_asset`'s refusal.
pub fn library_refusal(src: &AssetSource, ground: bool) -> Option<String> {
    let f = &src.file;
    let what = if ground { "a level's ground".to_string() } else { format!("{:?}", f.name) };
    if f.elements.iter().any(|e| e.distant.is_some()) {
        return Some(format!("{what} with distant layers stays the level's"));
    }
    if f.elements.iter().any(|e| matches!(e.kind, ElementKind::Raster)) {
        return Some(format!("{what} with a raster layer stays the level's"));
    }
    let glows_in = |id: &str| f.elements.iter().any(|e| e.glow.as_ref().is_some_and(|g| g.group == id));
    if let Some(g) = f.glow_groups.iter().find(|g| !glows_in(&g.id)) {
        return Some(format!("remove the unused glow group {} first", g.id));
    }
    olr_format::assets::check_asset(src).err().map(|e| format!("{what} does not go in the library: {e}"))
}

/// Makes the level's assets of one library origin that are the same one
/// asset: the copies of each later one point at the first, and the later
/// ones go.
fn merge_same(pack: &mut Pack) {
    let Some(p) = pack.placed.as_mut() else { return };
    let listed = p.file.assets.clone();
    let mut gone: Vec<String> = Vec::new();
    for (i, a) in listed.iter().enumerate() {
        if a.origin == AssetOrigin::Own || gone.contains(&a.key) {
            continue;
        }
        for b in &listed[i + 1..] {
            if b.origin != a.origin || gone.contains(&b.key) {
                continue;
            }
            let same = match (p.assets.get(&a.key), p.assets.get(&b.key)) {
                (Some(x), Some(y)) => same_asset(x, y),
                _ => false,
            };
            if same {
                for c in p.file.copies.iter_mut().filter(|c| c.asset == b.key) {
                    c.asset = a.key.clone();
                }
                gone.push(b.key.clone());
            }
        }
    }
    p.file.assets.retain(|a| !gone.contains(&a.key));
    p.assets.retain(|k, _| !gone.contains(k));
}

impl Document {
    /// The origins of the level's library assets, each once, sorted.
    pub fn linked_origins(&self) -> Vec<AssetOrigin> {
        let Some(p) = self.placement() else { return Vec::new() };
        let mut out: Vec<AssetOrigin> =
            p.file.assets.iter().map(|a| a.origin.clone()).filter(|o| *o != AssetOrigin::Own).collect();
        out.sort();
        out.dedup();
        out
    }

    /// The level's linked assets against the library's sources `libs`
    /// (`olr_edit::library::compare`).
    pub fn compare_library(&self, libs: &Libraries) -> Vec<Link> {
        self.placement().map(|p| library::compare(p, libs)).unwrap_or_default()
    }

    /// The keys of the level's assets of library origin `origin`.
    pub fn keys_of(&self, origin: &AssetOrigin) -> Vec<String> {
        let Some(p) = self.placement() else { return Vec::new() };
        p.file.assets.iter().filter(|a| a.origin == *origin).map(|a| a.key.clone()).collect()
    }

    /// The name of the level's asset `key`.
    pub fn asset_name(&self, key: &str) -> Option<String> {
        self.placement()?.assets.get(key).map(|a| a.file.name.clone())
    }

    /// How many copies use the assets `keys`.
    pub fn copies_of(&self, keys: &[String]) -> usize {
        self.placement().map_or(0, |p| p.file.copies.iter().filter(|c| keys.contains(&c.asset)).count())
    }

    /// The level's assets `keys` take the library's version in `libs`
    /// (`olr_edit::library::update`, refused when it breaks a copy), the
    /// same ones then made one: one step.
    pub(crate) fn update_assets(&mut self, keys: &[String], libs: &Libraries) -> Result<Change, DocError> {
        let ((), edit) = self.placement_op_with(|pack| {
            library::update(pack, keys, false, libs)?;
            merge_same(pack);
            Ok::<(), LibraryError>(())
        })?;
        Ok(self.change(edit))
    }

    /// The level's assets `keys` point at library asset `origin`, taking
    /// its files `source` (`olr_edit::library::relink`, refused when it
    /// breaks a copy), the same ones then made one: one step.
    pub(crate) fn relink_assets(
        &mut self,
        keys: &[String],
        origin: &AssetOrigin,
        source: &AssetSource,
    ) -> Result<Change, DocError> {
        let ((), edit) = self.placement_op_with(|pack| {
            for k in keys {
                library::relink(pack, k, origin.clone(), source, false)?;
            }
            merge_same(pack);
            Ok::<(), LibraryError>(())
        })?;
        Ok(self.change(edit))
    }

    /// The level's asset `key` made its own (`olr_edit::library::adopt`):
    /// one step.
    pub(crate) fn unlink_asset(&mut self, key: &str) -> Result<Change, DocError> {
        let ((), edit) = self.placement_op_with(|pack| library::adopt(pack, key))?;
        Ok(self.change(edit))
    }

    /// The level's own asset `key` as it would go in the library, or why
    /// it does not (`library_refusal`): the ground as the level writes it
    /// (`olr_edit::pack::placement`, its glow groups the look's).
    pub fn library_copy(&self, key: &str) -> Result<AssetSource, String> {
        let ground = key == GROUND_KEY;
        let src = if ground {
            olr_edit::pack::placement(&self.to_pack()).and_then(|mut p| p.assets.remove(GROUND_KEY))
        } else {
            self.own_asset(key).cloned()
        };
        let src = src.ok_or_else(|| format!("the level has no asset {key:?} of its own"))?;
        match library_refusal(&src, ground) {
            Some(why) => Err(why),
            None => Ok(src),
        }
    }
}

impl EditorState {
    /// `CompareLibrary` of the level's linked assets; none without any.
    pub(crate) fn compare_library(&self) -> Vec<EditorCommand> {
        let origins = self.document().map(Document::linked_origins).unwrap_or_default();
        if origins.is_empty() { Vec::new() } else { vec![EditorCommand::CompareLibrary { origins }] }
    }

    /// The level's linked assets as last compared with the library (issue
    /// #83; #84's notice lists them).
    pub fn library_links(&self) -> &[Link] {
        &self.links
    }

    /// The offer of Use your copy in this level, while it stands.
    pub fn use_yours(&self) -> Option<&UseYours> {
        self.use_yours.as_ref()
    }

    /// Says `message` on the level's status line, and on the shown tab's.
    pub(crate) fn say(&mut self, message: String) {
        self.on_tab(DocTab::Level, |s| s.status.message = Some(message.clone()));
        self.status.message = Some(message);
    }

    /// The host's `LibraryCompared`: the level takes the library's
    /// version of each changed asset that breaks no copy.
    pub(crate) fn library_compared(&mut self, libs: Libraries) -> Vec<EditorCommand> {
        let (cmds, said) = self.on_tab(DocTab::Level, |s| {
            let links = s.doc.as_ref().map(|d| d.compare_library(&libs)).unwrap_or_default();
            s.links = links;
            s.take_library(&libs)
        });
        if !said.is_empty() {
            self.say(said.join("; "));
        }
        cmds
    }

    /// The level tab's (in place) changed assets of `libs` taken, one step
    /// each origin; those that would break copies held back. What it said.
    pub(crate) fn take_library(&mut self, libs: &Libraries) -> (Vec<EditorCommand>, Vec<String>) {
        let Some(doc) = &self.doc else { return (Vec::new(), Vec::new()) };
        let links = doc.compare_library(libs);
        let mut origins: Vec<AssetOrigin> = links.iter().map(|l| l.origin.clone()).collect();
        origins.dedup();
        let (mut cmds, mut said) = (Vec::new(), Vec::new());
        for origin in origins {
            let of = || links.iter().filter(|l| l.origin == origin);
            let take: Vec<String> = of()
                .filter(|l| matches!(&l.status, Status::Changed { breaks } if breaks.is_empty()))
                .map(|l| l.key.clone())
                .collect();
            let breaks: Vec<&Break> = of()
                .flat_map(|l| match &l.status {
                    Status::Changed { breaks } => breaks.iter().collect(),
                    _ => Vec::new(),
                })
                .collect();
            let Some(doc) = &self.doc else { break };
            // The library's name for it (a save may have renamed it).
            let key = of().next().map(|l| l.key.clone()).unwrap_or_default();
            let library = libs.get(&origin).and_then(Option::as_ref).map(|a| a.file.name.clone());
            let name = library.or_else(|| doc.asset_name(&key)).unwrap_or(key);
            if !take.is_empty() {
                let n = doc.copies_of(&take);
                let step = format!("Update {name}");
                if let Some((c, ())) =
                    self.copy_edit_named(Some(step), |d| d.update_assets(&take, libs).map(|c| (c, ())))
                {
                    cmds.extend(c);
                    said.push(format!("{} of {name} updated", copies_said(n)));
                }
            }
            if !breaks.is_empty() {
                let mut copies: Vec<&str> = breaks.iter().map(|b| b.copy()).collect();
                copies.sort_unstable();
                copies.dedup();
                said.push(format!("{name} changed: {} would break", copies_said(copies.len())));
            }
        }
        (cmds, said)
    }

    /// Placing (in the level tab, in place) gave library asset `origin` a
    /// new key, its version `source` the library's: the level's older
    /// copies of it are offered it, as a library save's are.
    pub(crate) fn offer_newer(&mut self, origin: AssetOrigin, source: &AssetSource) -> Vec<EditorCommand> {
        let older = self.doc.as_ref().is_some_and(|d| d.keys_of(&origin).len() > 1);
        if !older {
            return Vec::new();
        }
        let libs = Libraries::from([(origin, Some(source.clone()))]);
        let (cmds, said) = self.take_library(&libs);
        if !said.is_empty() {
            self.status.message = Some(said.join("; "));
        }
        cmds
    }

    /// Unlink (issue #83): copy `id`'s asset becomes the level's own, one
    /// step; it follows the library no more.
    pub(crate) fn unlink(&mut self, id: &str) -> Vec<EditorCommand> {
        if self.shown != DocTab::Level {
            return Vec::new();
        }
        let Some(doc) = &self.doc else { return Vec::new() };
        let Some(c) = doc.placed_copy(id) else { return Vec::new() };
        let name = doc.asset_name(&c.asset).unwrap_or_else(|| c.asset.clone());
        let n = doc.copies_of(std::slice::from_ref(&c.asset));
        let step = format!("Unlink {name}");
        match self.copy_edit_named(Some(step), |d| d.unlink_asset(&c.asset).map(|c| (c, ()))) {
            Some((cmds, ())) => {
                self.status.message = Some(format!(
                    "{} of {name} now the level's own: it no longer follows the library",
                    copies_said(n)
                ));
                cmds
            }
            None => Vec::new(),
        }
    }

    /// Save to library (issue #83): copy `id`'s own asset asks its name,
    /// then goes in the user's library (`SaveAsset`); `AssetSaved` links
    /// it. Refused, with the reason, for one the library would not take.
    pub(crate) fn save_to_library(&mut self, id: &str) -> Vec<EditorCommand> {
        if self.shown != DocTab::Level {
            return Vec::new();
        }
        let Some(doc) = &self.doc else { return Vec::new() };
        let Some(c) = doc.placed_copy(id) else { return Vec::new() };
        if doc.copy_origin(id) != Some(AssetOrigin::Own) {
            self.status.message = Some(format!("{id} is a library asset's copy already"));
            return Vec::new();
        }
        let made = doc
            .library_copy(&c.asset)
            .and_then(|src| olr_edit::asset::decode(&src).map_err(|e| e.to_string()));
        match made {
            Ok(asset) => {
                let suggested: String = asset.name.chars().take(olr_format::assets::MAX_ASSET_NAME).collect();
                self.pending_asset = Some(Box::new(asset));
                self.pending_tab = None;
                self.pending_link = Some(c.asset.clone());
                self.dialog = Some(crate::state::Dialog::AssetName { suggested });
            }
            Err(why) => self.status.message = Some(why),
        }
        Vec::new()
    }

    /// `AssetSaved` of Save to library: the level's own asset `key`
    /// becomes the files written (`source`), linked to the user's `saved`,
    /// one step. The ground stays the level's own (it follows no library):
    /// a copy of it is in the library.
    pub(crate) fn link_saved(
        &mut self,
        key: &str,
        saved: &AssetKey,
        source: Option<&AssetSource>,
    ) -> Vec<EditorCommand> {
        let name = self.asset(saved).map_or_else(|| saved.id.clone(), |a| a.name.clone());
        if key == GROUND_KEY {
            self.say(format!("saved the ground as {name:?} in your assets; the level keeps its own ground"));
            return Vec::new();
        }
        let Some(source) = source else {
            self.say(format!(
                "saved {name:?} in your assets, but it did not read back: the level's copy stays its own"
            ));
            return Vec::new();
        };
        let origin = AssetOrigin::Yours(saved.id.clone());
        let keys = vec![key.to_string()];
        let step = format!("Save {name} to library");
        let made = self.on_tab(DocTab::Level, |s| {
            let n = s.doc.as_ref().map_or(0, |d| d.copies_of(&keys));
            let made =
                s.copy_edit_named(Some(step), |d| d.relink_assets(&keys, &origin, source).map(|c| (c, ())));
            made.map(|(cmds, ())| (cmds, n))
        });
        match made {
            Some((cmds, n)) => {
                self.say(format!(
                    "{name:?} is in your assets: its {} in the level follow it",
                    copies_said(n)
                ));
                cmds
            }
            None => Vec::new(),
        }
    }

    /// Notes the offer of Use your copy in this level when the asset tab
    /// saved built-in asset `was` as the user's `saved` and the level has
    /// copies of the built-in one.
    pub(crate) fn offer_use_yours(
        &mut self,
        was: Option<AssetKey>,
        saved: &AssetKey,
        source: Option<&AssetSource>,
    ) {
        let (Some(was), Some(source)) = (was.filter(|k| k.builtin), source) else { return };
        if saved.builtin {
            return;
        }
        let builtin = AssetOrigin::Builtin(was.id.clone());
        let has = self.document().is_some_and(|d| !d.keys_of(&builtin).is_empty());
        if !has {
            return;
        }
        self.use_yours = Some(UseYours {
            builtin: was.id,
            yours: saved.id.clone(),
            name: source.file.name.clone(),
            source: Arc::new(source.clone()),
        });
    }

    /// Use your copy in this level (issue #83): the level's copies of the
    /// built-in asset point at the user's copy of it, one step; refused,
    /// the breaks said, when it would break a copy.
    pub(crate) fn use_your_copy(&mut self) -> Vec<EditorCommand> {
        let Some(offer) = self.use_yours.clone() else { return Vec::new() };
        let builtin = AssetOrigin::Builtin(offer.builtin.clone());
        let yours = AssetOrigin::Yours(offer.yours.clone());
        let name = offer.name.clone();
        let step = format!("Use your {name}");
        let made = self.on_tab(DocTab::Level, |s| {
            let Some(doc) = &s.doc else { return None };
            let keys = doc.keys_of(&builtin);
            let n = doc.copies_of(&keys);
            let made = s.copy_edit_named(Some(step), |d| {
                d.relink_assets(&keys, &yours, &offer.source).map(|c| (c, ()))
            });
            made.map(|(cmds, ())| (cmds, n))
        });
        match made {
            Some((cmds, n)) => {
                self.use_yours = None;
                let verb = if n == 1 { "uses" } else { "use" };
                self.say(format!("{} of {name} now {verb} yours", copies_said(n)));
                cmds
            }
            None => {
                // The level tab's status says why (a break).
                let why = self.on_tab(DocTab::Level, |s| s.status.message.clone()).unwrap_or_default();
                self.status.message = Some(why);
                Vec::new()
            }
        }
    }
}
