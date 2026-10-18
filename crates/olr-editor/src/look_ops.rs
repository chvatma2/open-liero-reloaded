//! The look's document operations (plan 15a Task 17; look system §The level
//! editor): an element's glow (a new one gets its own group), the glow
//! groups, the look, light objects and the glow brush. Each checks first,
//! then goes through `Document::apply`, as every operation does.

use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_edit::compose::Rect;
use olr_edit::element::Property;
use olr_edit::level::{Shape, shape_bounds};
use olr_format::Colour;
use olr_format::levels::{ElementGlow, LevelObject, LevelSource, compile_look, element_box};
use olr_format::look::{
    AnimationSource, GlowColourSource, GlowGroupSource, LightSource, LookSource, ReachSource, valid_group_id,
};

use crate::document::{DocError, Document};
use crate::history::Edit;
use crate::ops::{Change, NOTHING, with_property};

/// A light the Light tool places: warm white, 100 %, 96 world pixels, radius 2.
pub fn new_light() -> LightSource {
    LightSource {
        colour: Colour(Rgba::rgb(255, 224, 176)),
        intensity: 100,
        reach: ReachSource::Pixels(96),
        animation: AnimationSource::Steady,
        phase_ms: None,
    }
}

/// A new glow's group (spec): each pixel's own colour, 100 %, 64 world
/// pixels, steady.
fn new_group(id: String) -> GlowGroupSource {
    GlowGroupSource {
        id,
        colour: GlowColourSource::Own,
        intensity: 100,
        reach: ReachSource::Pixels(64),
        animation: AnimationSource::Steady,
        phase_ms: None,
    }
}

impl Document {
    /// `compile_look`'s checks of a level with this look and these objects
    /// (no art: the look and the lights alone).
    fn check_look(&self, look: Option<&LookSource>, objects: &[LevelObject]) -> Result<(), DocError> {
        let tmp = LevelSource {
            name: String::new(),
            notes: String::new(),
            size: self.level.size,
            art_scale: self.level.art_scale,
            theme: String::new(),
            objects: objects.to_vec(),
            material: Vec::new(),
            front: RgbaImage::new(0, 0),
            back: RgbaImage::new(0, 0),
            thumb: None,
            look: look.cloned(),
            glow: None,
            clear: None,
        };
        compile_look(&tmp).map(|_| ()).map_err(|e| DocError::Bad(e.to_string()))
    }

    fn group_ids(&self) -> Vec<String> {
        self.level.look.iter().flat_map(|l| l.glow_groups.iter().map(|g| g.id.clone())).collect()
    }

    /// `base` as a new group id: itself, else with `-2`, `-3`, ... (a slug).
    fn new_group_id(&self, base: &str) -> String {
        let base = if valid_group_id(base) { base.to_string() } else { "glow".to_string() };
        let taken = self.group_ids();
        (1..)
            .map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") })
            .find(|id| !taken.contains(id))
            .unwrap()
    }

    /// The edits that make the look `after`, unglowing every element that
    /// names a group `after` lacks (and dropping its mask): checked.
    fn look_edits(&self, after: Option<LookSource>) -> Result<Vec<Edit>, DocError> {
        self.check_look(after.as_ref(), &self.level.objects)?;
        let kept: Vec<&str> =
            after.iter().flat_map(|l| l.glow_groups.iter().map(|g| g.id.as_str())).collect();
        let mut edits = vec![Edit::Look {
            before: self.level.look.clone().map(Box::new),
            after: after.clone().map(Box::new),
        }];
        for (index, item) in self.items.iter().enumerate() {
            let Some(g) = &item.element.glow else { continue };
            if kept.contains(&g.group.as_str()) {
                continue;
            }
            if item.element.locked {
                return Err(DocError::Locked(item.element.id.clone()));
            }
            let before = item.element.clone();
            let after = olr_format::levels::Element { glow: None, ..before.clone() };
            edits.extend(self.mask_dropped(&before));
            edits.push(Edit::Element { index, before, after });
        }
        Ok(edits)
    }

    /// The edit dropping element `e`'s painted mask, when it has one.
    fn mask_dropped(&self, e: &olr_format::levels::Element) -> Option<Edit> {
        let mask = self.glow_masks.get(&e.id)?;
        Some(Edit::GlowMask { id: e.id.clone(), before: Some(mask.clone()), after: None })
    }

    /// Carries out `edits` in order: one step's, and the rectangle rebaked.
    pub(crate) fn apply_all(&mut self, edits: Vec<Edit>) -> (Vec<Edit>, Rect) {
        let mut dirty = None;
        for e in &edits {
            let r = self.apply(e, true);
            if r.w > 0 && r.h > 0 {
                dirty = Some(Rect::union(dirty, r));
            }
        }
        (edits, dirty.unwrap_or(NOTHING))
    }

    /// Sets element `id`'s glow; `None` removes it and its mask. A glow
    /// naming no group of the look gets a new one, named after the element
    /// (made unique). One step.
    pub fn set_glow(&mut self, id: &str, glow: Option<ElementGlow>) -> Result<(Vec<Edit>, Rect), DocError> {
        let index = self.unlocked(id)?;
        let mut edits = Vec::new();
        let mut glow = glow;
        if let Some(g) = glow.as_mut()
            && !self.group_ids().contains(&g.group)
        {
            g.group = self.new_group_id(if g.group.is_empty() { id } else { &g.group });
            let mut look = self.level.look.clone().unwrap_or_default();
            look.glow_groups.push(new_group(g.group.clone()));
            edits.extend(self.look_edits(Some(look))?);
        }
        let before = self.items[index].element.clone();
        let mut after = before.clone();
        with_property(&mut after, Property::Glow(glow.clone()), self.level.size)?;
        self.check_with(|es| es[index] = after.clone())?;
        if glow.is_none() {
            edits.extend(self.mask_dropped(&before));
        }
        edits.push(Edit::Element { index, before, after });
        Ok(self.apply_all(edits))
    }

    /// Replaces the look (settings and groups; `None` removes it). Elements
    /// naming a group it no longer has lose their glow, in the same step.
    pub fn set_look(&mut self, look: Option<LookSource>) -> Result<(Vec<Edit>, Rect), DocError> {
        let edits = self.look_edits(look)?;
        Ok(self.apply_all(edits))
    }

    /// Adds glow group `group` (its id new).
    pub fn add_group(&mut self, group: GlowGroupSource) -> Result<(Vec<Edit>, Rect), DocError> {
        if self.group_ids().contains(&group.id) {
            return Err(DocError::Bad(format!("a glow group {:?} exists already", group.id)));
        }
        let mut look = self.level.look.clone().unwrap_or_default();
        look.glow_groups.push(group);
        self.set_look(Some(look))
    }

    /// Replaces glow group `id`'s fields (its id kept).
    pub fn set_group(&mut self, id: &str, group: GlowGroupSource) -> Result<(Vec<Edit>, Rect), DocError> {
        let mut look = self.level.look.clone().unwrap_or_default();
        let Some(g) = look.glow_groups.iter_mut().find(|g| g.id == id) else {
            return Err(DocError::Bad(format!("no glow group {id:?}")));
        };
        *g = GlowGroupSource { id: id.into(), ..group };
        self.set_look(Some(look))
    }

    /// Removes glow group `id`; its elements lose their glow (one step).
    pub fn remove_group(&mut self, id: &str) -> Result<(Vec<Edit>, Rect), DocError> {
        let mut look = self.level.look.clone().unwrap_or_default();
        let n = look.glow_groups.len();
        look.glow_groups.retain(|g| g.id != id);
        if look.glow_groups.len() == n {
            return Err(DocError::Bad(format!("no glow group {id:?}")));
        }
        self.set_look(Some(look))
    }

    /// A light object at `at` ([`new_light`]).
    pub fn add_light(&mut self, at: (i32, i32)) -> Result<Change, DocError> {
        let before = self.level.objects.clone();
        if before.len() >= olr_format::levels::MAX_OBJECTS {
            return Err(DocError::Bad(format!(
                "a level holds at most {} objects",
                olr_format::levels::MAX_OBJECTS
            )));
        }
        let mut after = before.clone();
        after.push(LevelObject::light(at, 2, new_light()));
        self.check_look(self.level.look.as_ref(), &after)?;
        Ok(self.change(Edit::Objects { before, after }))
    }

    /// Replaces light object `index`: its place, light and radius (notes kept).
    pub fn set_light(
        &mut self,
        index: usize,
        at: (i32, i32),
        light: LightSource,
        radius: u8,
    ) -> Result<Change, DocError> {
        let before = self.level.objects.clone();
        let mut after = before.clone();
        let notes = match after.get(index) {
            Some(LevelObject::Light { notes, .. }) => notes.clone(),
            _ => return Err(DocError::Bad(format!("object {index} is not a light"))),
        };
        let mut new = LevelObject::light(at, radius, light);
        if let LevelObject::Light { notes: n, .. } = &mut new {
            *n = notes;
        }
        after[index] = new;
        self.check_look(self.level.look.as_ref(), &after)?;
        Ok(self.change(Edit::Objects { before, after }))
    }

    /// What a glow stroke into `id` starts from: its mask and whether the
    /// element's glow uses one. Refused for an element without a glow, a
    /// locked or a raster one.
    pub fn glow_stroke_before(&self, id: &str) -> Result<(Option<Arc<Vec<u8>>>, bool), DocError> {
        let index = self.unlocked(id)?;
        let e = &self.items[index].element;
        let Some(g) = &e.glow else {
            return Err(DocError::Bad(format!("element {id:?} has no glow: give it one first")));
        };
        if element_box(&e.kind, self.level.size).is_none() {
            return Err(DocError::WrongKind { id: id.into(), want: "an image or drawn element" });
        }
        Ok((self.glow_masks.get(id).cloned(), g.mask))
    }

    /// Paints element `id`'s glow mask under world `shape` at `strength`
    /// percent (0 erases), within a stroke (no step: `glow_stroke_edits`
    /// records it). A first mask keeps everything; the glow uses it at
    /// once. Returns the rectangle rebaked.
    pub fn paint_glow(&mut self, id: &str, shape: &Shape, strength: u8) -> Result<Rect, DocError> {
        self.glow_stroke_before(id)?;
        let index = self.find(id)?;
        let s = self.level.art_scale;
        let Some((at, b)) = element_box(&self.items[index].element.kind, self.level.size) else {
            return Ok(NOTHING);
        };
        let n = b.0 as usize * s as usize * b.1 as usize * s as usize;
        let mask = self.glow_masks.entry(id.to_string()).or_insert_with(|| Arc::new(vec![255; n]));
        if mask.len() != n {
            *mask = Arc::new(vec![255; n]);
        }
        let value = ((u32::from(strength.min(100)) * 255 + 50) / 100) as u8;
        olr_edit::glow::paint_mask(Arc::make_mut(mask).as_mut_slice(), at, b, s, shape, value);
        if let Some(g) = self.items[index].element.glow.as_mut() {
            g.mask = true;
        }
        let rect = shape_bounds(shape, self.level.size).unwrap_or(NOTHING);
        if rect.w > 0 && rect.h > 0 {
            self.rebake(rect);
        }
        Ok(rect)
    }

    /// The step a glow stroke into `id` made, from what it started from
    /// (`glow_stroke_before`): the glow's mask flag, then the mask. Empty
    /// when it changed nothing.
    pub fn glow_stroke_edits(&self, id: &str, before: (Option<Arc<Vec<u8>>>, bool)) -> Vec<Edit> {
        let Some(index) = self.index_of(id) else { return Vec::new() };
        let after_mask = self.glow_masks.get(id).cloned();
        if after_mask == before.0 {
            return Vec::new();
        }
        let mut edits = Vec::new();
        let after = self.items[index].element.clone();
        if after.glow.as_ref().is_some_and(|g| g.mask != before.1) {
            let mut was = after.clone();
            if let Some(g) = was.glow.as_mut() {
                g.mask = before.1;
            }
            edits.push(Edit::Element { index, before: was, after });
        }
        edits.push(Edit::GlowMask { id: id.into(), before: before.0, after: after_mask });
        edits
    }
}
