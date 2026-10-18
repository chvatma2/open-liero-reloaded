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

/// What a glow stroke starts from: an element's mask, and whether its glow
/// uses one.
pub type GlowStrokeStart = (Option<Arc<Vec<u8>>>, bool);

/// `mask`, the glow mask of a box `from` (world origin, size) at art scale
/// `s`, laid on box `to` by world position: what falls outside `from`
/// keeps everything (255). A crop or a level resize moves a mask this way.
pub fn relay_mask(
    mask: &[u8],
    from: ((i32, i32), (u32, u32)),
    to: ((i32, i32), (u32, u32)),
    s: u32,
) -> Vec<u8> {
    let s = s.max(1) as i64;
    let (fw, fh) = (i64::from(from.1.0) * s, i64::from(from.1.1) * s);
    let (tw, th) = (i64::from(to.1.0) * s, i64::from(to.1.1) * s);
    let mut out = vec![255u8; (tw * th).max(0) as usize];
    if mask.len() as i64 != fw * fh {
        return out;
    }
    // Art pixel (x, y) of `to` is art pixel (x + dx, y + dy) of `from`.
    let dx = (i64::from(to.0.0) - i64::from(from.0.0)) * s;
    let dy = (i64::from(to.0.1) - i64::from(from.0.1)) * s;
    for y in 0..th {
        let fy = y + dy;
        if !(0..fh).contains(&fy) {
            continue;
        }
        for x in 0..tw {
            let fx = x + dx;
            if (0..fw).contains(&fx) {
                out[(y * tw + x) as usize] = mask[(fy * fw + fx) as usize];
            }
        }
    }
    out
}

/// `mask` (`from` art pixels) resampled to `to` art pixels, nearest: a
/// resized image's, or another art scale's. One of another size than
/// `from` keeps everything (255).
pub fn resample_mask(mask: &[u8], from: (u32, u32), to: (u32, u32)) -> Vec<u8> {
    let n = to.0 as usize * to.1 as usize;
    if mask.len() != from.0 as usize * from.1 as usize || from.0 == 0 || from.1 == 0 {
        return vec![255; n];
    }
    let mut out = Vec::with_capacity(n);
    for y in 0..to.1 as u64 {
        let fy = (y * u64::from(from.1) / u64::from(to.1.max(1))) as usize;
        for x in 0..to.0 as u64 {
            let fx = (x * u64::from(from.0) / u64::from(to.0.max(1))) as usize;
            out.push(mask[fy * from.0 as usize + fx]);
        }
    }
    out
}

/// `look` (or a new one) with a new glow group `id` (a pasted glow's).
pub(crate) fn with_group(look: Option<LookSource>, id: &str) -> LookSource {
    let mut look = look.unwrap_or_default();
    look.glow_groups.push(new_group(id.to_string()));
    look
}

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
    pub(crate) fn look_change(&self, after: Option<LookSource>) -> Result<Vec<Edit>, DocError> {
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
            edits.extend(self.look_change(Some(look))?);
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

    /// Sets one property of element `id` (`Document::set`), and fits its
    /// glow mask to a new box size in the same step (resampled, nearest).
    pub fn set_property(&mut self, id: &str, property: Property) -> Result<(Vec<Edit>, Rect), DocError> {
        let size = self.level.size;
        let s = self.level.art_scale;
        let before = self.index_of(id).map(|i| self.items[i].element.clone());
        let change = self.set(id, property)?;
        let mut edits = vec![change.edit];
        let mut dirty = change.dirty;
        if let (Some(was), Some(i)) = (before, self.index_of(id))
            && let Some(m) = self.glow_masks.get(id).cloned()
            && let (Some((_, ob)), Some((_, nb))) =
                (element_box(&was.kind, size), element_box(&self.items[i].element.kind, size))
            && ob != nb
        {
            let after = Arc::new(resample_mask(&m, (ob.0 * s, ob.1 * s), (nb.0 * s, nb.1 * s)));
            let edit = Edit::GlowMask { id: id.into(), before: Some(m), after: Some(after) };
            let r = self.apply(&edit, true);
            dirty = Rect::union(Some(dirty), r);
            edits.push(edit);
        }
        Ok((edits, dirty))
    }

    /// Replaces the look (settings and groups; `None` removes it). Elements
    /// naming a group it no longer has lose their glow, in the same step.
    pub fn set_look(&mut self, look: Option<LookSource>) -> Result<(Vec<Edit>, Rect), DocError> {
        let edits = self.look_change(look)?;
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

    /// `objects` (copies) added after the level's, each moved by `offset`:
    /// one step; their indices. Lights are checked as the look's.
    pub fn paste_objects(
        &mut self,
        objects: &[LevelObject],
        offset: (i32, i32),
    ) -> Result<(Change, Vec<usize>), DocError> {
        let before = self.level.objects.clone();
        if before.len() + objects.len() > olr_format::levels::MAX_OBJECTS {
            return Err(DocError::Bad(format!(
                "a level holds at most {} objects",
                olr_format::levels::MAX_OBJECTS
            )));
        }
        let mut after = before.clone();
        for o in objects {
            let mut o = o.clone();
            let at = o.at();
            o.set_at((at.0.saturating_add(offset.0), at.1.saturating_add(offset.1)));
            after.push(o);
        }
        self.check_look(self.level.look.as_ref(), &after)?;
        let indices = (before.len()..after.len()).collect();
        Ok((self.change(Edit::Objects { before, after }), indices))
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
    pub fn glow_stroke_before(&self, id: &str) -> Result<GlowStrokeStart, DocError> {
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
        // A mask that no longer fits starts again whole: all of it changes.
        let reset = mask.len() != n;
        if reset {
            *mask = Arc::new(vec![255; n]);
        }
        let value = ((u32::from(strength.min(100)) * 255 + 50) / 100) as u8;
        olr_edit::glow::paint_mask(Arc::make_mut(mask).as_mut_slice(), at, b, s, shape, value);
        if let Some(g) = self.items[index].element.glow.as_mut() {
            g.mask = true;
        }
        let rect = if reset {
            self.element_rect(&self.items[index].element)
        } else {
            shape_bounds(shape, self.level.size).unwrap_or(NOTHING)
        };
        if rect.w > 0 && rect.h > 0 {
            self.rebake(rect);
        }
        Ok(rect)
    }

    /// The step a glow stroke into `id` made, from what it started from
    /// (`glow_stroke_before`): the glow's mask flag, then the mask. Empty
    /// when it changed nothing.
    pub fn glow_stroke_edits(&self, id: &str, before: GlowStrokeStart) -> Vec<Edit> {
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
