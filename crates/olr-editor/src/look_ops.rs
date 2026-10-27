//! The look's document operations (plan 15a Task 17; look system §The level
//! editor): an element's glow (a new one gets its own group), the glow
//! groups, the look, light objects and the glow brush. Each checks first,
//! then goes through `Document::apply`, as every operation does.

use std::sync::Arc;

use olr_data::image::RgbaImage;
use olr_edit::compose::Rect;
use olr_edit::element::Property;
use olr_edit::level::{Shape, shape_bounds};
use olr_format::levels::{
    ElementGlow, LevelObject, LevelSource, compile_look, element_box, surface_back_file, surface_file,
};
use olr_format::look::{
    AnimationSource, GlowColourSource, GlowGroupSource, LookSource, ReachSource, valid_group_id,
};

use crate::document::{DocError, Document};
use crate::history::Edit;
use crate::ops::{Change, NOTHING, with_property};

/// An element's box: its world origin and size.
type ElementBox = ((i32, i32), (u32, u32));

/// What a glow stroke starts from: an element's mask and glow, and the
/// look (a new glow adds a group).
#[derive(Clone, Debug)]
pub struct GlowStrokeStart {
    pub mask: Option<Arc<Vec<u8>>>,
    pub glow: Option<ElementGlow>,
    pub look: Option<LookSource>,
}

pub use olr_edit::glow::{relay_mask, resample_mask};

/// `look` (or a new one) with a new glow group `id` (a pasted glow's).
pub(crate) fn with_group(look: Option<LookSource>, id: &str) -> LookSource {
    let mut look = look.unwrap_or_default();
    look.glow_groups.push(new_group(id.to_string()));
    look
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
            glass: None,
            glow_beams: None,
            haze: None,
            distant: Vec::new(),
            surface: None,
            distant_surfaces: Vec::new(),
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
        // Issue #43: the painted maps follow the materials by id, and a
        // surface naming a material gone is cleared.
        let materials = after.as_ref().map_or(&[][..], |l| &l.materials[..]);
        let has = |id: &Option<String>| id.as_ref().is_none_or(|id| materials.iter().any(|m| m.id == *id));
        let mut edits = vec![Edit::Look {
            before: self.level.look.clone().map(Box::new),
            after: after.clone().map(Box::new),
        }];
        edits.extend(self.surface_maps_change(self.materials(), materials));
        for (index, item) in self.items.iter().enumerate() {
            let before = &item.element;
            let unglow = before.glow.as_ref().is_some_and(|g| !kept.contains(&g.group.as_str()));
            let (surface, back_surface) = (has(&before.surface), has(&before.back_surface));
            if !unglow && surface && back_surface {
                continue;
            }
            if self.locked_at(index) {
                return Err(DocError::Locked(before.id.clone()));
            }
            let mut after = before.clone();
            if unglow {
                after.glow = None;
                edits.extend(self.mask_dropped(before));
            }
            if !surface {
                after.surface = None;
            }
            if !back_surface {
                after.back_surface = None;
            }
            edits.push(Edit::Element { index, before: before.clone(), after });
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
        with_property(&mut after, Property::Glow(glow.clone()), self.level.size, self.locked_at(index))?;
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
        if let Property::Distant(d) = property {
            return self.set_distant(id, d);
        }
        let size = self.level.size;
        let s = self.level.art_scale;
        let before = self.index_of(id).map(|i| self.items[i].element.clone());
        let change = self.set(id, property)?;
        let mut edits = vec![change.edit];
        let mut dirty = change.dirty;
        if let (Some(was), Some(i)) = (before.as_ref(), self.index_of(id))
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
        // So do its painted surface maps, at world resolution (issue #37).
        if let (Some(was), Some(i)) = (before.as_ref(), self.index_of(id))
            && let (Some((_, ob)), Some((_, nb))) =
                (element_box(&was.kind, size), element_box(&self.items[i].element.kind, size))
            && ob != nb
        {
            for file in [surface_file(id), surface_back_file(id)] {
                // One that does not fit its old box is left as it is (and
                // dropped on saving).
                let fits = |m: &&Arc<Vec<u8>>| m.len() == ob.0 as usize * ob.1 as usize;
                let Some(m) = self.surface_maps.get(&file).filter(fits).cloned() else { continue };
                let after = Arc::new(resample_mask(&m, ob, nb));
                let edit = Edit::SurfaceMap { id: id.into(), file, before: Some(m), after: Some(after) };
                let r = self.apply(&edit, true);
                dirty = Rect::union(Some(dirty), r);
                edits.push(edit);
            }
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

    /// What a glow stroke (or a glow by colour) into `id` starts from:
    /// its mask, its glow and the look. Refused for a locked or a raster
    /// element.
    pub fn glow_stroke_before(&self, id: &str) -> Result<GlowStrokeStart, DocError> {
        let index = self.unlocked(id)?;
        let e = &self.items[index].element;
        if element_box(&e.kind, self.level.size).is_none() {
            return Err(DocError::WrongKind { id: id.into(), want: "an image or drawn element" });
        }
        self.refuse_glow_in_state(index)?;
        Ok(GlowStrokeStart {
            mask: self.glow_masks.get(id).cloned(),
            glow: e.glow.clone(),
            look: self.level.look.clone(),
        })
    }

    /// Element `id`'s mask, ready to paint (no step: the stroke's records
    /// it): an element without a glow gets one, in a new group named after
    /// it, dark (0) but for what is painted (the user's request,
    /// 2026-10-19); a glow without a mask, or one that no longer fits, starts
    /// whole (255). Returns its index, box and whether all of it changed.
    fn ready_mask(&mut self, id: &str) -> Result<(usize, ElementBox, bool), DocError> {
        self.glow_stroke_before(id)?;
        let index = self.find(id)?;
        let s = self.level.art_scale;
        let Some((at, b)) = element_box(&self.items[index].element.kind, self.level.size) else {
            return Err(DocError::WrongKind { id: id.into(), want: "an image or drawn element" });
        };
        let n = b.0 as usize * s as usize * b.1 as usize * s as usize;
        let mut reset = false;
        if self.items[index].element.glow.is_none() {
            let group = self.new_group_id(id);
            self.level.look = Some(with_group(self.level.look.clone(), &group));
            self.items[index].element.glow =
                Some(ElementGlow { strength: 100, group, mask: true, beam: None });
            self.glow_masks.insert(id.to_string(), Arc::new(vec![0; n]));
            reset = true;
        }
        let mask = self.glow_masks.entry(id.to_string()).or_insert_with(|| Arc::new(vec![255; n]));
        // A mask that no longer fits starts again whole: all of it changes.
        if mask.len() != n {
            *mask = Arc::new(vec![255; n]);
            reset = true;
        }
        if let Some(g) = self.items[index].element.glow.as_mut() {
            reset |= !g.mask;
            g.mask = true;
        }
        Ok((index, (at, b), reset))
    }

    /// Paints element `id`'s glow mask under world `shape` at `strength`
    /// percent (0 erases), within a stroke (no step: `glow_stroke_edits`
    /// records it). Returns the rectangle rebaked.
    pub fn paint_glow(&mut self, id: &str, shape: &Shape, strength: u8) -> Result<Rect, DocError> {
        let (index, (at, b), reset) = self.ready_mask(id)?;
        let s = self.level.art_scale;
        let value = ((u32::from(strength.min(100)) * 255 + 50) / 100) as u8;
        if let Some(mask) = self.glow_masks.get_mut(id) {
            olr_edit::glow::paint_mask(Arc::make_mut(mask).as_mut_slice(), at, b, s, shape, value);
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

    /// The pixels of element `id`'s own art alike in colour to the one
    /// under world point `at` (4-connected, within `tolerance` percent):
    /// their world rectangle in the level, and a flag per art pixel of it
    /// (row-major). `None` when nothing of it is under `at`.
    pub fn colour_pixels(&self, id: &str, at: (i32, i32), tolerance: u8) -> Option<(Rect, Vec<bool>)> {
        let index = self.index_of(id)?;
        let s = self.level.art_scale;
        let (origin, b) = element_box(&self.items[index].element.kind, self.level.size)?;
        let art = match &self.items[index].decoded {
            olr_edit::compose::Decoded::Drawn { art, .. } => art,
            olr_edit::compose::Decoded::Image { grid: Some(g), .. } => &**g,
            _ => return None,
        };
        let local = |v: i32, o: i32, side: u32| {
            let d = i64::from(v) - i64::from(o);
            (0..i64::from(side)).contains(&d).then(|| d as u32 * s + s / 2)
        };
        let seed = (local(at.0, origin.0, b.0)?, local(at.1, origin.1, b.1)?);
        let region = olr_edit::glow::colour_region(art, seed, tolerance)?;
        // Its world bounding box (in the box's world pixels), cut to the level.
        let (aw, ah) = (art.width(), art.height());
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for (i, _) in region.iter().enumerate().filter(|(_, r)| **r) {
            let (x, y) = (i as u32 % aw.max(1), i as u32 / aw.max(1));
            (x0, y0, x1, y1) = (x0.min(x / s), y0.min(y / s), x1.max(x / s + 1), y1.max(y / s + 1));
        }
        let (lw, lh) = self.level.size;
        let clip = |v: i64, side: u32| v.clamp(0, i64::from(side)) as u32;
        let (wx0, wy0) =
            (clip(i64::from(origin.0) + i64::from(x0), lw), clip(i64::from(origin.1) + i64::from(y0), lh));
        let (wx1, wy1) =
            (clip(i64::from(origin.0) + i64::from(x1), lw), clip(i64::from(origin.1) + i64::from(y1), lh));
        if wx1 <= wx0 || wy1 <= wy0 {
            return None;
        }
        let rect = Rect { x: wx0, y: wy0, w: wx1 - wx0, h: wy1 - wy0 };
        let (rw, rh) = (rect.w * s, rect.h * s);
        let mut out = vec![false; rw as usize * rh as usize];
        for ry in 0..rh {
            for rx in 0..rw {
                let bx = i64::from(rect.x * s + rx) - i64::from(origin.0) * i64::from(s);
                let by = i64::from(rect.y * s + ry) - i64::from(origin.1) * i64::from(s);
                if (0..i64::from(aw)).contains(&bx) && (0..i64::from(ah)).contains(&by) {
                    out[(ry * rw + rx) as usize] = region[(by as u32 * aw + bx as u32) as usize];
                }
            }
        }
        Some((rect, out))
    }

    /// Sets element `id`'s glow to `strength` percent on the pixels of
    /// world rectangle `rect` (level pixels) that `region` (a flag per art
    /// pixel of `rect`, row-major; `None` all) picks and its box holds: a
    /// pixel selection's glow (the user's request, 2026-10-19). An element
    /// without a glow gets one, dark elsewhere. One step: the edits, and
    /// the rectangle rebaked.
    pub fn glow_pixels(
        &mut self,
        id: &str,
        rect: Rect,
        region: Option<&[bool]>,
        strength: u8,
    ) -> Result<(Vec<Edit>, Rect), DocError> {
        let start = self.glow_stroke_before(id)?;
        let s = self.level.art_scale;
        let (rw, rh) = (rect.w * s, rect.h * s);
        if region.is_some_and(|r| r.len() != rw as usize * rh as usize) {
            return Err(DocError::Bad("the selection does not fit its rectangle".into()));
        }
        let (index, (origin, b), _) = self.ready_mask(id)?;
        let value = ((u32::from(strength.min(100)) * 255 + 50) / 100) as u8;
        let (bw, bh) = (i64::from(b.0 * s), i64::from(b.1 * s));
        if let Some(mask) = self.glow_masks.get_mut(id) {
            let m = Arc::make_mut(mask);
            for ry in 0..rh {
                for rx in 0..rw {
                    if !region.is_none_or(|r| r[(ry * rw + rx) as usize]) {
                        continue;
                    }
                    let bx = i64::from(rect.x * s + rx) - i64::from(origin.0) * i64::from(s);
                    let by = i64::from(rect.y * s + ry) - i64::from(origin.1) * i64::from(s);
                    if (0..bw).contains(&bx) && (0..bh).contains(&by) {
                        m[(by * bw + bx) as usize] = value;
                    }
                }
            }
        }
        let dirty = self.element_rect(&self.items[index].element);
        self.rebake(dirty);
        Ok((self.glow_stroke_edits(id, start), dirty))
    }

    /// Glow by colour (the user's request, 2026-10-19): `colour_pixels`
    /// under `at` take `strength` percent ([`Document::glow_pixels`]).
    /// Nothing under `at` is no change.
    pub fn glow_by_colour(
        &mut self,
        id: &str,
        at: (i32, i32),
        strength: u8,
        tolerance: u8,
    ) -> Result<(Vec<Edit>, Rect), DocError> {
        self.glow_stroke_before(id)?;
        match self.colour_pixels(id, at, tolerance) {
            Some((rect, region)) => self.glow_pixels(id, rect, Some(&region), strength),
            None => Ok((Vec::new(), NOTHING)),
        }
    }

    /// The step a glow stroke into `id` made, from what it started from
    /// (`glow_stroke_before`): a new group in the look, the element's glow,
    /// then the mask. Empty when the mask did not change (anything the
    /// stroke made for it is taken back). Issue #79: in an asset's state
    /// other than the start, the state's own picture of the layer with its
    /// own mask comes first (`own_for_glow`).
    pub fn glow_stroke_edits(&mut self, id: &str, before: GlowStrokeStart) -> Vec<Edit> {
        let mut edits = self.glow_stroke_edits_of(id, &before);
        if !edits.is_empty()
            && let Some(own) = self.own_for_glow(id, &before)
        {
            edits.insert(0, own);
        }
        edits
    }

    fn glow_stroke_edits_of(&mut self, id: &str, before: &GlowStrokeStart) -> Vec<Edit> {
        let before = before.clone();
        let Some(index) = self.index_of(id) else { return Vec::new() };
        let after_mask = self.glow_masks.get(id).cloned();
        if after_mask == before.mask {
            // A stroke of no change: what it set up goes again.
            self.level.look = before.look;
            self.items[index].element.glow = before.glow;
            match before.mask {
                Some(m) => self.glow_masks.insert(id.into(), m),
                None => self.glow_masks.remove(id),
            };
            return Vec::new();
        }
        let mut edits = Vec::new();
        if self.level.look != before.look {
            edits.push(Edit::Look {
                before: before.look.map(Box::new),
                after: self.level.look.clone().map(Box::new),
            });
        }
        let after = self.items[index].element.clone();
        if after.glow != before.glow {
            let was = olr_format::levels::Element { glow: before.glow, ..after.clone() };
            edits.push(Edit::Element { index, before: was, after });
        }
        edits.push(Edit::GlowMask { id: id.into(), before: before.mask, after: after_mask });
        edits
    }
}

/// What a Haze brush stroke starts from: the level's haze map as it was.
#[derive(Clone, Debug)]
pub struct HazeStrokeStart {
    pub map: Option<Vec<u8>>,
}

impl Document {
    /// What a Haze brush stroke starts from.
    pub fn haze_stroke_before(&self) -> HazeStrokeStart {
        HazeStrokeStart { map: self.level.haze.clone() }
    }

    /// Paints the level's haze map under world `shape` at `strength`
    /// percent (`round(strength * 255 / 100)`; 0 clears), within a stroke
    /// (no step: `haze_stroke_edits` records it). A level without a haze
    /// map gets one of 255 first (ruling T6a). Returns the rectangle
    /// painted.
    pub fn paint_haze(&mut self, shape: &Shape, strength: u8) -> Rect {
        let size = self.level.size;
        let value = ((u32::from(strength.min(100)) * 255 + 50) / 100) as u8;
        let n = size.0 as usize * size.1 as usize;
        let map = self.level.haze.get_or_insert_with(|| vec![255; n]);
        if map.len() != n {
            *map = vec![255; n];
        }
        olr_edit::glow::paint_mask(map, (0, 0), size, 1, shape, value);
        shape_bounds(shape, size).unwrap_or(NOTHING)
    }

    /// The step a Haze brush stroke made, from what it started from and
    /// the rectangle it touched: the touched bytes before and after. Empty
    /// when nothing changed (a map the stroke made is taken back).
    pub fn haze_stroke_edits(&mut self, before: HazeStrokeStart, touched: Rect) -> Vec<Edit> {
        let (w, h) = self.level.size;
        let n = w as usize * h as usize;
        let fit = |m: &Option<Vec<u8>>| m.as_ref().filter(|m| m.len() == n).cloned();
        let was = fit(&before.map);
        let Some(now) = fit(&self.level.haze) else { return Vec::new() };
        let rect = Rect {
            x: touched.x.min(w),
            y: touched.y.min(h),
            w: touched.w.min(w.saturating_sub(touched.x)),
            h: touched.h.min(h.saturating_sub(touched.y)),
        };
        let rows = |m: Option<&Vec<u8>>| -> Vec<u8> {
            let mut out = Vec::with_capacity(rect.w as usize * rect.h as usize);
            for y in rect.y..rect.y + rect.h {
                let start = y as usize * w as usize + rect.x as usize;
                match m {
                    Some(m) => out.extend_from_slice(&m[start..start + rect.w as usize]),
                    None => out.resize(out.len() + rect.w as usize, 255),
                }
            }
            out
        };
        let (b, a) = (rows(was.as_ref()), rows(Some(&now)));
        if b == a || rect.w == 0 || rect.h == 0 {
            // Nothing changed: the map stays as it began.
            self.level.haze = before.map;
            return Vec::new();
        }
        vec![Edit::Haze { rect, before: b, after: a, created: was.is_none() }]
    }
}
