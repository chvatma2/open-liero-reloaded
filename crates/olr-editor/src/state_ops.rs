//! The asset tab's states (issue #79; the asset-based levels design's "The
//! asset tab", calls 3 and 11): the state shown, the state operations
//! (`olr_edit::asset`'s, issue #72) as steps, a layer made a state's own
//! when it is painted there, the own pictures carried along when their
//! layers change, and the canvas's pose of the shown state.
//!
//! While a state other than the start is shown, the items hold its own
//! pictures and its entry holds the start's in their place
//! (`asset_tab::AssetParts`); `swap_shown` exchanges them, so that every
//! other operation, the bake and the history work on the items as ever.
//! Each asset tab step is bracketed by `Edit::Show` (`EditorState`'s
//! `carry_states`), so undo and redo show the state it was made in.

use std::collections::BTreeMap;
use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_edit::asset::{Asset, AssetError, OwnPicture, Piece, State, posed_in_place};
use olr_edit::compose::{Decoded, Rect};
use olr_format::assets::{MAX_STATES, PropertySource};
use olr_format::levels::{Element, ElementKind};
use olr_format::look::{GlowGroupSource, LookSource};

use crate::asset_tab::StateEntry;
use crate::document::{DocError, Document, Item};
use crate::history::{Edit, StatesSnap};
use crate::look_ops::GlowStrokeStart;

/// Why a stroke on an image is refused in a state other than the start.
pub const IMAGE_IN_STATE: &str =
    "a picture is the same in every state: merge it into a layer to paint it in a state";
/// Why the Glow brush is refused on an image in a state.
pub const IMAGE_GLOW_IN_STATE: &str =
    "an image's glow is the same in every state: paint it in the starting state ★";
/// Why the Surface brush is refused in a state.
pub const SURFACE_IN_STATE: &str =
    "surface materials are the same in every state: paint them in the starting state ★";
/// Why the start cannot be removed (the Remove button's hover text).
pub const START_STAYS: &str = "the starting state stays: make another state the start first";
/// Why health's state cannot be removed.
pub const HEALTH_ENDS_HERE: &str = "health ends in this state";

/// The edits that carry the asset's states along a step, and what they
/// dropped ("Alarm's picture of screens").
pub(crate) struct Carried {
    pub(crate) edit: Option<Edit>,
    pub(crate) forgotten: Vec<String>,
}

/// The shown state's look on the canvas (`Document::canvas_pose`): each
/// item's element as posed (hidden when the state drops it), its opacity,
/// and the glow groups as the state sets them.
pub(crate) struct CanvasPose {
    pub(crate) elements: Vec<Element>,
    pub(crate) opacity: Vec<u8>,
    pub(crate) groups: Vec<GlowGroupSource>,
}

/// The states panel's pictures (issue #79): each state posed light
/// (`olr_edit::asset::preview_state`), made again only for the states
/// whose version moved (`Document::state_version`), never all of them on
/// every release; `made` counts the pictures made.
#[derive(Default)]
pub struct StatePictures {
    /// The document they are of (`EditorState::shown_generation`).
    key: Option<u64>,
    pictures: Vec<Option<(u64, RgbaImage)>>,
    made: u64,
}

impl StatePictures {
    /// Makes the pictures of `doc`'s states (of document generation `key`)
    /// whose version moved; returns their indices.
    pub fn refresh(&mut self, doc: &Document, key: u64) -> Vec<usize> {
        if self.key != Some(key) {
            self.key = Some(key);
            self.pictures.clear();
        }
        let n = doc.states().len();
        self.pictures.resize(n, None);
        let stale: Vec<usize> =
            (0..n).filter(|&i| self.pictures[i].as_ref().map(|p| p.0) != doc.state_version(i)).collect();
        if stale.is_empty() {
            return stale;
        }
        let asset = doc.to_asset();
        for &i in &stale {
            let picture = olr_edit::asset::preview_state(&asset, i, false);
            self.pictures[i] = Some((doc.state_version(i).unwrap_or(0), picture));
            self.made += 1;
        }
        stale
    }

    /// State `i`'s picture, when made.
    pub fn picture(&self, i: usize) -> Option<&RgbaImage> {
        self.pictures.get(i)?.as_ref().map(|p| &p.1)
    }

    /// How many pictures were made.
    pub fn made(&self) -> u64 {
        self.made
    }
}

/// Whether `p` is a position (or `Facing`): the canvas ignores it.
fn positional(p: &PropertySource) -> bool {
    matches!(p, PropertySource::X | PropertySource::Y | PropertySource::Facing)
        || matches!(p, PropertySource::OffsetX(_) | PropertySource::OffsetY(_))
}

fn bad(e: AssetError) -> DocError {
    DocError::Bad(e.to_string())
}

impl Document {
    /// The asset's states (an asset tab's document; empty otherwise), in
    /// the shown state's form (`AssetParts`).
    pub fn states(&self) -> &[StateEntry] {
        self.asset.as_deref().map_or(&[], |p| &p.states[..])
    }

    /// The starting state's index (an asset tab's document).
    pub fn start_state(&self) -> Option<usize> {
        self.asset.as_deref().map(|p| p.start)
    }

    /// The shown state's index (an asset tab's document).
    pub fn shown_state(&self) -> Option<usize> {
        self.asset.as_deref().map(|p| p.shown)
    }

    /// Whether a state other than the start is shown (an asset's).
    pub fn in_state(&self) -> bool {
        self.asset.as_deref().is_some_and(|p| p.shown != p.start)
    }

    /// The shown state's name (empty for a level's document).
    pub fn shown_state_name(&self) -> String {
        self.asset.as_deref().and_then(|p| p.states.get(p.shown)).map(|s| s.name.clone()).unwrap_or_default()
    }

    /// State `i`'s version: it moves when an edit changes how the state
    /// looks (the states panel makes its picture again then).
    pub fn state_version(&self, i: usize) -> Option<u64> {
        self.asset.as_deref().and_then(|p| p.versions.get(i).copied())
    }

    /// Whether the shown state, not the start, paints layer `id` itself
    /// (the layer list's dot).
    pub fn shown_owns(&self, id: &str) -> bool {
        self.asset.as_deref().is_some_and(|p| {
            p.shown != p.start && p.states.get(p.shown).is_some_and(|s| s.own.contains_key(id))
        })
    }

    /// The states painting layer `id` themselves, by name, in order.
    pub fn owners(&self, id: &str) -> Vec<String> {
        self.states().iter().filter(|s| s.own.contains_key(id)).map(|s| s.name.clone()).collect()
    }

    /// Refuses merging or splitting a layer a state paints itself (issue
    /// #79: the simplest rule that never loses a picture).
    pub(crate) fn refuse_owned(&self, i: usize) -> Result<(), DocError> {
        let e = &self.items[i].element;
        match self.owners(&e.id).first() {
            Some(state) => Err(DocError::Bad(format!(
                "{state} paints {} itself: make it follow the start first",
                e.name
            ))),
            None => Ok(()),
        }
    }

    /// Exchanges the shown state's own pictures with the items' (and their
    /// glow masks, where the state has its own): the involution between
    /// the asset as stored and the shown state's form. The start's shown:
    /// nothing.
    pub(crate) fn swap_shown(&mut self) {
        let Some(p) = self.asset.as_deref_mut() else { return };
        if p.shown == p.start {
            return;
        }
        let Some(st) = p.states.get_mut(p.shown) else { return };
        for (id, pic) in st.own.iter_mut() {
            let Some(item) = self.items.iter_mut().find(|i| i.element.id == *id) else { continue };
            if !matches!(item.decoded, Decoded::Drawn { .. }) {
                continue;
            }
            let empty = OwnPicture {
                decoded: Decoded::Drawn { art: RgbaImage::new(0, 0), cells: None, back: None },
                glow_mask: None,
            };
            let mut new = Arc::unwrap_or_clone(std::mem::replace(pic, Arc::new(empty)));
            std::mem::swap(&mut item.decoded, &mut new.decoded);
            if let (Some(m), Some(base)) = (new.glow_mask.as_mut(), self.glow_masks.get_mut(id)) {
                std::mem::swap(m, base);
            }
            *pic = Arc::new(new);
        }
    }

    /// Shows state `to` (not a step; a state the asset lacks shows the
    /// start). Whether it changed; the caller rebakes.
    pub(crate) fn show_state(&mut self, to: usize) -> bool {
        let Some(p) = self.asset.as_deref() else { return false };
        let to = if to < p.states.len() { to } else { p.start };
        if to == p.shown {
            return false;
        }
        self.swap_shown();
        if let Some(p) = self.asset.as_deref_mut() {
            p.shown = to;
        }
        self.swap_shown();
        true
    }

    /// Selects state `i` (`EditorAction::SelectState`; not a step): the
    /// canvas and the previews show it. The rectangle rebaked.
    pub fn select_state(&mut self, i: usize) -> Result<Rect, DocError> {
        if i >= self.states().len() {
            return Err(DocError::Bad(format!("the asset has no state {}", i + 1)));
        }
        if !self.show_state(i) {
            return Ok(Rect { x: 0, y: 0, w: 0, h: 0 });
        }
        let all = self.all();
        self.rebake(all);
        Ok(all)
    }

    /// The states as they are (an asset tab's document).
    pub(crate) fn states_snap(&self) -> Option<StatesSnap> {
        let p = self.asset.as_deref()?;
        Some(StatesSnap { states: p.states.clone(), start: p.start, shown: p.shown })
    }

    /// Sets the states (an `Edit::States` or `Whole` applied): a state
    /// that changed gets a new version.
    pub(crate) fn set_states(&mut self, snap: &StatesSnap) {
        let Some(p) = self.asset.as_deref_mut() else { return };
        let mut versions = Vec::with_capacity(snap.states.len());
        for (i, s) in snap.states.iter().enumerate() {
            match (p.states.get(i), p.versions.get(i)) {
                (Some(old), Some(v)) if old == s => versions.push(*v),
                _ => {
                    versions.push(p.clock);
                    p.clock += 1;
                }
            }
        }
        p.states = snap.states.clone();
        p.start = snap.start;
        p.shown = snap.shown;
        p.versions = versions;
    }

    /// Gives the states `edit` changed the look of a new version (issue
    /// #79: the states panel makes only their pictures again). A picture
    /// edit of a layer the shown state paints itself changes that state; of
    /// another layer, every state following the start for it; anything
    /// else every state. Showing a state, or the states' own edit (versions
    /// by `set_states`), changes none.
    pub(crate) fn touch(&mut self, edit: &Edit) {
        let Some(p) = self.asset.as_deref_mut() else { return };
        let picture = match edit {
            Edit::Show { .. } | Edit::States { .. } | Edit::Objects { .. } => return,
            // Issue #80: sounds and the script change no state's look.
            Edit::Parts { before, after }
                if before.exposed == after.exposed && before.health == after.health =>
            {
                return;
            }
            Edit::Meta { before, after } if before.theme == after.theme => return,
            Edit::Pixels { id, .. } | Edit::Mask { id, .. } => Some((id.clone(), false)),
            Edit::GlowMask { id, .. } => Some((id.clone(), true)),
            Edit::Layer { before, .. } => Some((before.element.id.clone(), false)),
            _ => None,
        };
        let shown = p.shown;
        let owns = |s: &StateEntry, id: &str, mask: bool| {
            s.own.get(id).is_some_and(|o| !mask || o.glow_mask.is_some())
        };
        let changed: Vec<usize> = match &picture {
            Some((id, mask))
                if shown != p.start && p.states.get(shown).is_some_and(|s| owns(s, id, *mask)) =>
            {
                vec![shown]
            }
            Some((id, mask)) => {
                (0..p.states.len()).filter(|&i| i == p.start || !owns(&p.states[i], id, *mask)).collect()
            }
            None => (0..p.states.len()).collect(),
        };
        for i in changed {
            if let Some(v) = p.versions.get_mut(i) {
                *v = p.clock;
                p.clock += 1;
            }
        }
    }

    /// A new state's name: "State N", the first number free (case aside).
    pub fn new_state_name(&self) -> String {
        let taken = |n: &str| self.states().iter().any(|s| s.name.eq_ignore_ascii_case(n));
        (1..).map(|n| format!("State {n}")).find(|n| !taken(n)).unwrap_or_default()
    }

    /// Why no state can be added (Add state, Duplicate): at `MAX_STATES`.
    pub fn add_refusal(&self) -> Option<String> {
        (self.states().len() >= MAX_STATES).then(|| format!("an asset has at most {MAX_STATES} states"))
    }

    /// Why state `i` cannot be removed: it is the start, or health ends in
    /// it.
    pub fn remove_refusal(&self, i: usize) -> Option<String> {
        let p = self.asset.as_deref()?;
        let st = p.states.get(i)?;
        if i == p.start {
            return Some(START_STAYS.into());
        }
        if p.health.as_ref().is_some_and(|h| h.at_zero == st.name) {
            return Some(HEALTH_ENDS_HERE.into());
        }
        None
    }

    /// Add state (`from` none: a copy of the start, "State N") or Duplicate
    /// (`from` a state: its copy, `copy_name`d): one step, the new state
    /// shown.
    pub fn add_state(&mut self, from: Option<usize>) -> Result<(Vec<Edit>, Rect), DocError> {
        if let Some(why) = self.add_refusal() {
            return Err(DocError::Bad(why));
        }
        let names: Vec<String> = self.states().iter().map(|s| s.name.clone()).collect();
        let (name, from) = match from {
            None => (self.new_state_name(), None),
            Some(i) => {
                let f =
                    names.get(i).ok_or_else(|| DocError::Bad(format!("the asset has no state {}", i + 1)))?;
                (olr_edit::element::copy_name(f, names.iter().map(String::as_str)), Some(f.clone()))
            }
        };
        self.restate(|a| olr_edit::asset::add_state(a, &name, from.as_deref()))
    }

    /// Removes state `i` and its own pictures (refused for the start and
    /// health's state): one step, the start shown when it was shown.
    pub fn remove_state(&mut self, i: usize) -> Result<(Vec<Edit>, Rect), DocError> {
        if let Some(why) = self.remove_refusal(i) {
            return Err(DocError::Bad(why));
        }
        let name = self
            .states()
            .get(i)
            .map(|s| s.name.clone())
            .ok_or_else(|| DocError::Bad("no such state".into()))?;
        let shown = self.shown_state().unwrap_or(0);
        self.restate(|a| {
            olr_edit::asset::remove_state(a, &name)?;
            Ok(if shown == i {
                a.start
            } else if shown > i {
                shown - 1
            } else {
                shown
            })
        })
    }

    /// Renames state `i` (a duplicate or a bad name refused, with why).
    pub fn rename_state(&mut self, i: usize, new: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        let name = self
            .states()
            .get(i)
            .map(|s| s.name.clone())
            .ok_or_else(|| DocError::Bad("no such state".into()))?;
        let shown = self.shown_state().unwrap_or(0);
        self.restate(|a| {
            olr_edit::asset::rename_state(a, &name, new)?;
            Ok(shown)
        })
    }

    /// Makes state `i` the start (★), every state's look kept
    /// (`olr_edit::asset::set_start`): one step, `i` shown.
    pub fn make_start(&mut self, i: usize) -> Result<(Vec<Edit>, Rect), DocError> {
        let name = self
            .states()
            .get(i)
            .map(|s| s.name.clone())
            .ok_or_else(|| DocError::Bad("no such state".into()))?;
        if self.start_state() == Some(i) {
            return Ok((Vec::new(), Rect { x: 0, y: 0, w: 0, h: 0 }));
        }
        self.restate(|a| {
            olr_edit::asset::set_start(a, &name)?;
            Ok(a.start)
        })
    }

    /// Follow start: the shown state drops its own picture of layer `id`.
    pub fn follow_start(&mut self, id: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        let Some(shown) = self.shown_state() else { return Err(DocError::Bad("not an asset".into())) };
        let name = self.states().get(shown).map(|s| s.name.clone()).unwrap_or_default();
        self.restate(|a| {
            olr_edit::asset::follow_start(a, &name, id)?;
            Ok(shown)
        })
    }

    /// Runs one of `olr_edit::asset`'s state operations on the asset as
    /// stored (its start shown), `op` returning the state to show after:
    /// the step's edits applied (the start shown, the start's pictures and
    /// masks it changed, the states, the state shown after), and the
    /// rectangle rebaked. A refusal changes nothing.
    fn restate(
        &mut self,
        op: impl FnOnce(&mut Asset) -> Result<usize, AssetError>,
    ) -> Result<(Vec<Edit>, Rect), DocError> {
        let Some(p) = self.asset.as_deref() else { return Err(DocError::Bad("not an asset".into())) };
        let (k0, start0) = (p.shown, p.start);
        let show = Edit::Show { before: k0, after: start0 };
        let mut dirty = self.apply(&show, true);
        let mut asset = self.to_asset();
        let k1 = match op(&mut asset) {
            Ok(k) => k,
            Err(e) => {
                self.apply(&show, false);
                return Err(bad(e));
            }
        };
        let mut edits = vec![show];
        for piece in &asset.pieces {
            let id = &piece.element.id;
            let Some(index) = self.index_of(id) else { continue };
            if self.items[index].decoded != piece.decoded {
                let before = Box::new(self.items[index].clone());
                let after =
                    Box::new(Item { element: before.element.clone(), decoded: piece.decoded.clone() });
                edits.push(Edit::Layer { index, before, after });
            }
            let now = self.glow_masks.get(id).cloned();
            if piece.glow_mask.is_some() && now != piece.glow_mask {
                edits.push(Edit::GlowMask { id: id.clone(), before: now, after: piece.glow_mask.clone() });
            }
        }
        let before =
            self.states_snap().unwrap_or_else(|| StatesSnap { states: Vec::new(), start: 0, shown: 0 });
        // Pictures that did not change keep their address (counted once).
        let states = asset
            .states
            .iter()
            .map(|s| {
                let mut e = StateEntry::of(s);
                if let Some(old) = before.states.iter().find(|o| o.name == s.name) {
                    for (id, pic) in e.own.iter_mut() {
                        if let Some(o) = old.own.get(id).filter(|o| ***o == **pic) {
                            *pic = o.clone();
                        }
                    }
                }
                e
            })
            .collect();
        let after = StatesSnap { states, start: asset.start, shown: asset.start };
        edits.push(Edit::States { before: Box::new(before), after: Box::new(after) });
        edits.push(Edit::Show { before: asset.start, after: k1 });
        for e in &edits[1..] {
            let r = self.apply(e, true);
            if r.w > 0 && r.h > 0 {
                dirty = Rect::union(Some(dirty), r);
            }
        }
        Ok((edits, dirty))
    }

    /// Before a stroke into item `i` in a state other than the start: an
    /// image refused ([`IMAGE_IN_STATE`]); a drawn layer the state does not
    /// paint itself made its own (a copy of the start's art, cells, back
    /// art and glow mask), the edit applied and returned for the stroke's
    /// step.
    pub(crate) fn own_for_stroke(&mut self, i: usize) -> Result<Option<Edit>, DocError> {
        let Some(p) = self.asset.as_deref() else { return Ok(None) };
        if p.shown == p.start {
            return Ok(None);
        }
        let item = &self.items[i];
        let id = item.element.id.clone();
        match (&item.element.kind, &item.decoded) {
            (ElementKind::Image { .. }, _) => return Err(DocError::Bad(IMAGE_IN_STATE.into())),
            (ElementKind::Drawn { .. }, Decoded::Drawn { .. }) => {}
            _ => return Ok(None),
        }
        if p.states.get(p.shown).is_none_or(|s| s.own.contains_key(&id)) {
            return Ok(None);
        }
        let masked = item.element.glow.as_ref().is_some_and(|g| g.mask);
        let glow_mask = self.glow_masks.get(&id).filter(|_| masked).cloned();
        let picture = OwnPicture { decoded: item.decoded.clone(), glow_mask };
        Ok(Some(self.own_with(&id, picture)))
    }

    /// The shown state paints layer `id` itself from now, the start's
    /// picture `base` kept in its entry: the edit, applied.
    fn own_with(&mut self, id: &str, base: OwnPicture) -> Edit {
        let before =
            self.states_snap().unwrap_or_else(|| StatesSnap { states: Vec::new(), start: 0, shown: 0 });
        let mut after = before.clone();
        if let Some(st) = after.states.get_mut(after.shown) {
            st.own.insert(id.into(), Arc::new(base));
        }
        let edit = Edit::States { before: Box::new(before), after: Box::new(after) };
        self.apply(&edit, true);
        edit
    }

    /// Refuses a Glow brush stroke on an image in a state other than the
    /// start ([`IMAGE_GLOW_IN_STATE`]: its mask is every state's).
    pub(crate) fn refuse_glow_in_state(&self, i: usize) -> Result<(), DocError> {
        let Some(p) = self.asset.as_deref() else { return Ok(()) };
        if p.shown != p.start && matches!(self.items[i].element.kind, ElementKind::Image { .. }) {
            return Err(DocError::Bad(IMAGE_GLOW_IN_STATE.into()));
        }
        Ok(())
    }

    /// Refuses the Surface brush in a state other than the start
    /// ([`SURFACE_IN_STATE`]: a state has no surface map of its own).
    pub(crate) fn refuse_surface_in_state(&self) -> Result<(), DocError> {
        match self.asset.as_deref() {
            Some(p) if p.shown != p.start => Err(DocError::Bad(SURFACE_IN_STATE.into())),
            _ => Ok(()),
        }
    }

    /// After a glow stroke into drawn layer `id` that changed its mask
    /// (from `before`), in a state other than the start: the state made to
    /// paint the layer itself with its own glow mask, the start keeping
    /// the mask it had (all dark when the stroke gave the layer its glow,
    /// whole when it had no mask). The edit, applied, goes first in the
    /// stroke's step.
    pub(crate) fn own_for_glow(&mut self, id: &str, before: &GlowStrokeStart) -> Option<Edit> {
        let p = self.asset.as_deref()?;
        if p.shown == p.start {
            return None;
        }
        let i = self.index_of(id)?;
        let item = &self.items[i];
        if !matches!(item.decoded, Decoded::Drawn { .. }) {
            return None;
        }
        let n = self.glow_masks.get(id).map_or(0, |m| m.len());
        let base = match &before.glow {
            None => Arc::new(vec![0; n]),
            Some(_) => before.mask.clone().filter(|m| m.len() == n).unwrap_or_else(|| Arc::new(vec![255; n])),
        };
        let st = p.states.get(p.shown)?;
        let picture = match st.own.get(id) {
            Some(o) if o.glow_mask.is_some() => return None,
            Some(o) => OwnPicture { decoded: o.decoded.clone(), glow_mask: Some(base) },
            None => OwnPicture { decoded: item.decoded.clone(), glow_mask: Some(base) },
        };
        Some(self.own_with(id, picture))
    }

    /// After a step of the asset tab (`step`, as recorded): the states'
    /// own pictures carried along. A removed layer's are forgotten (named
    /// "Alarm's picture of screens"), and the states' values naming it
    /// ("Off's lamps.glow"; `olr_edit::asset::forget_layer`'s, but for the
    /// exposed entries: `asset_refusal` asks to unexpose them first); a
    /// layer made mixed or plain has its pictures made so too; a layer
    /// without a glow mask leaves none in them. The edit, applied, and
    /// what went.
    pub(crate) fn carry_states(&mut self, step: &[Edit]) -> Carried {
        let mut out = Carried { edit: None, forgotten: Vec::new() };
        let Some(before) = self.states_snap() else { return out };
        let s = self.level.art_scale;
        let name_of = |id: &str| {
            step.iter()
                .find_map(|e| match e {
                    Edit::Remove { item, .. } if item.item.element.id == id => {
                        Some(item.item.element.name.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| id.to_string())
        };
        let mut after = before.clone();
        for st in after.states.iter_mut() {
            let gone: Vec<String> = st.own.keys().filter(|id| self.index_of(id).is_none()).cloned().collect();
            for id in gone {
                st.own.remove(&id);
                out.forgotten.push(format!("{}'s picture of {}", st.name, name_of(&id)));
            }
            let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut st.values)
                .into_iter()
                .partition(|v| v.property.layer().is_some_and(|l| self.index_of(l).is_none()));
            st.values = kept;
            for v in gone {
                out.forgotten.push(format!("{}'s {}", st.name, olr_edit::asset::property_label(&v.property)));
            }
            for (id, pic) in st.own.iter_mut() {
                let Some(i) = self.index_of(id) else { continue };
                let e = &self.items[i].element;
                let ElementKind::Drawn { mixed, material, .. } = e.kind else { continue };
                let masked = e.glow.as_ref().is_some_and(|g| g.mask) && self.glow_masks.contains_key(id);
                let Decoded::Drawn { cells, .. } = &pic.decoded else { continue };
                let mut new = None::<OwnPicture>;
                if mixed && cells.is_none() {
                    let mut p = (**pic).clone();
                    olr_edit::drawn::mix(&mut p.decoded, s, material.into());
                    new = Some(p);
                } else if !mixed && cells.is_some() {
                    let mut p = (**pic).clone();
                    if let Decoded::Drawn { cells, .. } = &mut p.decoded {
                        *cells = None;
                    }
                    new = Some(p);
                }
                if !masked && pic.glow_mask.is_some() {
                    let mut p = new.take().unwrap_or_else(|| (**pic).clone());
                    p.glow_mask = None;
                    new = Some(p);
                }
                if let Some(p) = new {
                    *pic = Arc::new(p);
                }
            }
        }
        if after != before {
            let edit = Edit::States { before: Box::new(before), after: Box::new(after) };
            self.apply(&edit, true);
            out.edit = Some(edit);
        }
        out
    }

    /// The shown state's look on the canvas (issue #79): `posed_in_place`
    /// (its values, and the exposed defaults, but where things are: `X`,
    /// `Y`, the layer offsets and `Facing` ignored), its pictures being the
    /// items'. `None` when it sets nothing but positions (the items as
    /// they are).
    pub(crate) fn canvas_pose(&self) -> Option<CanvasPose> {
        let p = self.asset.as_deref()?;
        let st = p.states.get(p.shown)?;
        let sets = st.values.iter().any(|v| !positional(&v.property))
            || p.exposed.iter().any(|x| x.default.is_some() && !positional(&x.property));
        if !sets {
            return None;
        }
        let blank = || Decoded::Drawn { art: RgbaImage::new(0, 0), cells: None, back: None };
        let pieces = self
            .items
            .iter()
            .map(|i| Piece {
                element: i.element.clone(),
                decoded: blank(),
                original: None,
                glow_mask: None,
                surface_map: None,
                back_surface_map: None,
            })
            .collect();
        let look = self.level.look.as_ref();
        let groups = look.map_or_else(Vec::new, |l| l.glow_groups.clone());
        let materials = look.map_or_else(Vec::new, |l| l.materials.clone());
        let mut asset =
            Asset::new(&self.level.name, self.level.art_scale, self.level.size, pieces, groups, materials);
        asset.folders = self.folders.clone();
        asset.states = vec![State { own: BTreeMap::new(), ..st.to_state_values() }];
        asset.exposed = p.exposed.clone();
        asset.health = p.health.clone();
        let posed = posed_in_place(&asset, 0).ok()?;
        let mut elements = Vec::with_capacity(self.items.len());
        let mut opacity = Vec::with_capacity(self.items.len());
        for i in &self.items {
            match posed.pieces.iter().find(|q| q.piece.element.id == i.element.id) {
                Some(q) => {
                    elements.push(q.piece.element.clone());
                    opacity.push(q.opacity);
                }
                None => {
                    elements.push(Element { visible: false, ..i.element.clone() });
                    opacity.push(100);
                }
            }
        }
        Some(CanvasPose { elements, opacity, groups: posed.groups })
    }

    /// The look the canvas is lit by (issue #79): the shown state's glow
    /// groups in the level's look, when its values change them; else the
    /// level's look.
    pub fn canvas_look(&self) -> Option<&LookSource> {
        self.canvas_look.as_ref().or(self.level.look.as_ref())
    }

    /// The canvas's look made again from the shown state (`canvas_look`).
    pub(crate) fn refresh_canvas_look(&mut self, pose: Option<&CanvasPose>) {
        let made;
        let pose = match pose {
            Some(p) => Some(p),
            None => {
                made = self.canvas_pose();
                made.as_ref()
            }
        };
        self.canvas_look = match (pose, &self.level.look) {
            (Some(p), Some(l)) if p.groups != l.glow_groups => {
                Some(LookSource { glow_groups: p.groups.clone(), ..l.clone() })
            }
            _ => None,
        };
    }
}

impl StateEntry {
    /// The state without its own pictures (what the canvas's pose reads).
    fn to_state_values(&self) -> State {
        State {
            name: self.name.clone(),
            own: BTreeMap::new(),
            values: self.values.clone(),
            loop_sound: self.loop_sound.clone(),
            enter_sound: self.enter_sound.clone(),
        }
    }
}
