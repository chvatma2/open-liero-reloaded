//! The document's operations. Each checks before changing anything, then
//! builds its `Edit` and carries it out through `Document::apply`, the one
//! path undo and redo take too; a refused operation leaves the document
//! as it was. Painting (`paint`, `erase`) is recorded per stroke instead
//! (`stroke_edit`).

use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_edit::compose::{Decoded, Rect, resize_decoded};
use olr_edit::drawn;
use olr_edit::element::{Property, copy_name, new_element_id, new_source_name};
use olr_edit::level::{self, Paint, Shape, rescale_art};
use olr_edit::mask::{Mask, mask_colour, mask_shape, original_pixel};
use olr_edit::pixelate::Pixelation;
use olr_format::image_io::{LEVEL_ART, check_size};
use olr_format::levels::{
    Composition, Dither, Element, ElementKind, ElementMaterial, LevelSource, MAX_ART_SCALE,
    MAX_COMPOSED_PIXELS, MAX_ELEMENTS, check_elements, drawn_box,
};

use crate::catalog::Catalog;
use crate::document::{DocError, Document, Item};
use crate::history::{Edit, LevelMeta, Patch, Snapshot, Whole};

/// Why painting on a picture is refused.
const PICTURE: &str = "erase on a picture; merge it into a layer to paint it";

/// What a stroke lays (plan 14d): paint of a material, erasing, or (on a
/// picture) restoring what its mask erased.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stroke {
    Paint(Material),
    Erase,
    Restore,
}

/// A copy of what a stroke changes, taken as it begins: a drawn layer's
/// data or an image's mask.
#[derive(Clone, Debug)]
pub enum StrokeBefore {
    Layer(Decoded),
    Mask(Arc<Mask>),
}

/// `local` (world pixels of a box at `at`) as a rectangle of a level of
/// `level`, clipped.
fn to_level(at: (i32, i32), local: Rect, level: (u32, u32)) -> Rect {
    let c = |v: i64, side: u32| v.clamp(0, i64::from(side)) as u32;
    let (x0, y0) = (i64::from(at.0) + i64::from(local.x), i64::from(at.1) + i64::from(local.y));
    let (x1, y1) = (x0 + i64::from(local.w), y0 + i64::from(local.h));
    let (x0, y0, x1, y1) = (c(x0, level.0), c(y0, level.1), c(x1, level.0), c(y1, level.1));
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

/// Drawn data `d`'s pixels over `r` (world pixels of its box, `cols` wide).
fn patch_of(d: &Decoded, r: Rect, s: u32, cols: u32) -> Option<Patch> {
    let Decoded::Drawn { art, cells, back } = d else { return None };
    let crop = |i: &RgbaImage| i.crop(r.x * s, r.y * s, r.w * s, r.h * s);
    let cells = cells.as_ref().map(|c| {
        (r.y..r.y + r.h)
            .flat_map(|y| {
                let row = y as usize * cols as usize;
                c[row + r.x as usize..row + (r.x + r.w) as usize].iter().copied()
            })
            .collect()
    });
    Some(Patch { art: crop(art), cells, back: back.as_ref().map(crop) })
}

/// A rectangle of a drawn layer's pixels (plan 14e R7): its size in world
/// pixels, its art (size × art scale) and one material per world pixel.
#[derive(Clone, Debug, PartialEq)]
pub struct PixelClip {
    pub size: (u32, u32),
    pub art: RgbaImage,
    pub cells: Vec<Material>,
}

/// A change made: its edit, and the world rectangle rebaked (to upload).
#[derive(Debug)]
pub struct Change {
    pub edit: Edit,
    pub dirty: Rect,
}

/// An empty rectangle: nothing rebaked.
pub(crate) const NOTHING: Rect = Rect { x: 0, y: 0, w: 0, h: 0 };

/// What an image's processed grid depends on: its original, its box's
/// size, its pixelation, and whether it is shown (a hidden one has none).
fn grid_key(e: &Element) -> Option<(&str, (u32, u32), Pixelation, bool)> {
    match &e.kind {
        ElementKind::Image { source, size, pixel_size, colours, dither, alpha_threshold, .. } => Some((
            source,
            *size,
            Pixelation {
                pixel_size: *pixel_size,
                colours: *colours,
                dither: *dither,
                alpha_threshold: *alpha_threshold,
            },
            e.visible,
        )),
        _ => None,
    }
}

/// Sets one property of `e`, as `olr_edit::element::set` does (a locked
/// element takes only `Locked`).
pub(crate) fn with_property(e: &mut Element, property: Property, level: (u32, u32)) -> Result<(), DocError> {
    if e.locked && !matches!(property, Property::Locked(_)) {
        return Err(DocError::Locked(e.id.clone()));
    }
    let wrong = |want| DocError::WrongKind { id: e.id.clone(), want };
    match property {
        Property::Name(n) => e.name = n,
        Property::Visible(v) => e.visible = v,
        Property::Locked(v) => e.locked = v,
        // Look system (plan 15a): an image or drawn element only.
        Property::Glow(_) | Property::Clear(_) if matches!(e.kind, ElementKind::Raster) => {
            return Err(wrong("an image or drawn element"));
        }
        Property::Glow(g) => e.glow = g,
        Property::Clear(c) => e.clear = c,
        Property::Material(m) => match &mut e.kind {
            ElementKind::Image { material, .. } | ElementKind::Drawn { material, .. } => *material = m,
            ElementKind::Raster => return Err(wrong("an image or drawn element")),
        },
        // A drawn layer moves; level-sized again only at the origin (plan
        // 14d R2, batch B review 2).
        Property::At(v) if matches!(e.kind, ElementKind::Drawn { .. }) => {
            if let ElementKind::Drawn { at, size, .. } = &mut e.kind {
                let b = size.unwrap_or(level);
                *at = v;
                *size = (v != (0, 0) || b != level).then_some(b);
            }
        }
        p => {
            let ElementKind::Image { at, size, pixel_size, colours, dither, alpha_threshold, .. } =
                &mut e.kind
            else {
                return Err(wrong("an image element"));
            };
            match p {
                Property::At(v) => *at = v,
                Property::Size(v) => *size = v,
                Property::PixelSize(v) => *pixel_size = v,
                Property::Colours(v) => *colours = v,
                Property::Dither(v) => *dither = v,
                Property::Threshold(v) => *alpha_threshold = v,
                _ => {}
            }
        }
    }
    Ok(())
}

impl Document {
    /// The element `id`, by index.
    pub(crate) fn find(&self, id: &str) -> Result<usize, DocError> {
        self.index_of(id).ok_or_else(|| DocError::NoElement(id.into()))
    }

    /// `id` (found) when it is not locked.
    pub(crate) fn unlocked(&self, id: &str) -> Result<usize, DocError> {
        let i = self.find(id)?;
        if self.items[i].element.locked {
            return Err(DocError::Locked(id.into()));
        }
        Ok(i)
    }

    /// `check_elements` over the composition with `edit` applied to a copy.
    pub(crate) fn check_with(&self, edit: impl FnOnce(&mut Vec<Element>)) -> Result<(), DocError> {
        let mut elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        edit(&mut elements);
        self.check(&elements, self.level.size, self.level.art_scale)
    }

    fn check(&self, elements: &[Element], size: (u32, u32), art_scale: u32) -> Result<(), DocError> {
        self.check_extra(elements, size, art_scale, 0)
    }

    /// `check`, counting the items' masks and `extra` pixels more in the
    /// budget (batch A review I2: a mask is decoded with its element).
    fn check_extra(
        &self,
        elements: &[Element],
        size: (u32, u32),
        art_scale: u32,
        extra: u64,
    ) -> Result<(), DocError> {
        if elements.len() > MAX_ELEMENTS {
            return Err(DocError::Bad(format!("a level holds at most {MAX_ELEMENTS} elements")));
        }
        let total = check_elements(&Composition { elements: elements.to_vec() }, size, art_scale)
            .map_err(|e| DocError::Bad(e.to_string()))?;
        // The masks of the elements that stay (a merge or split may take one).
        let masks: u64 = self
            .items
            .iter()
            .filter(|i| elements.iter().any(|e| e.id == i.element.id))
            .map(|i| match &i.decoded {
                Decoded::Image { mask: Some(m), .. } => m.keep.len() as u64,
                _ => 0,
            })
            .sum();
        let all = total.saturating_add(masks).saturating_add(extra);
        if all > MAX_COMPOSED_PIXELS {
            return Err(DocError::Bad(format!(
                "the elements and masks would decode to {all} pixels, more than {MAX_COMPOSED_PIXELS}"
            )));
        }
        Ok(())
    }

    /// Carries out `edit` as a change.
    pub(crate) fn change(&mut self, edit: Edit) -> Change {
        let dirty = self.apply(&edit, true);
        Change { edit, dirty }
    }

    /// An image element on top, centred on `centre` (world), its box the
    /// oriented original's size ÷ art scale (rounded up, at least 1): pixel
    /// size 1, no reduction, threshold 128, material dirt (spec §5). The id
    /// and the name from the file stem.
    pub fn add_image(
        &mut self,
        file_name: &str,
        bytes: Vec<u8>,
        centre: (i32, i32),
    ) -> Result<Change, DocError> {
        // Stray files in `editor/images/` (in `other`) keep their names too.
        let stray = self.stray_files();
        let dir = olr_format::levels::image_file("");
        let names = self
            .originals
            .keys()
            .map(String::as_str)
            .chain(stray.iter().filter_map(|f| f.strip_prefix(dir.as_str())));
        let source = new_source_name(names, file_name);
        let original = self.cache.original(&source, &bytes)?;
        let s = self.level.art_scale.max(1);
        let size = (original.width().div_ceil(s).max(1), original.height().div_ceil(s).max(1));
        let half = |v: u32| i64::from(v / 2);
        let at = (
            (i64::from(centre.0) - half(size.0)).clamp(i32::MIN.into(), i32::MAX.into()) as i32,
            (i64::from(centre.1) - half(size.1)).clamp(i32::MIN.into(), i32::MAX.into()) as i32,
        );
        let stem = file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem);
        let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        let element = Element {
            id: new_element_id(&elements, &stray, stem, "image"),
            name: stem.into(),
            visible: true,
            locked: false,
            kind: ElementKind::Image {
                material: ElementMaterial::Dirt,
                source: source.clone(),
                at,
                size,
                pixel_size: 1,
                colours: None,
                dither: Dither::None,
                alpha_threshold: 128,
                mask: false,
            },
            glow: None,
            clear: false,
        };
        self.check_with(|es| es.push(element.clone()))?;
        let bytes = Arc::new(bytes);
        let decoded = self.image_grid(&element, &bytes, None)?;
        let item = Item { element, decoded };
        let index = self.items.len();
        Ok(self.change(Edit::Insert {
            index,
            item: Box::new(Snapshot { item, original: Some((source, bytes)) }),
        }))
    }

    /// An image element's decoded data through the cache (`None` grid when
    /// hidden), with `mask`.
    fn image_grid(
        &mut self,
        e: &Element,
        original: &[u8],
        mask: Option<Arc<Mask>>,
    ) -> Result<Decoded, DocError> {
        let ElementKind::Image { source, size, pixel_size, colours, dither, alpha_threshold, .. } = &e.kind
        else {
            return Err(DocError::WrongKind { id: e.id.clone(), want: "an image element" });
        };
        if !e.visible {
            self.cache.original(source, original)?;
            return Ok(Decoded::Image { grid: None, mask });
        }
        let s = self.level.art_scale;
        let p = Pixelation {
            pixel_size: *pixel_size,
            colours: *colours,
            dither: *dither,
            alpha_threshold: *alpha_threshold,
        };
        let art = (size.0.saturating_mul(s), size.1.saturating_mul(s));
        let grid = self.cache.grid(source, original, art, &p, mask.as_deref())?;
        Ok(Decoded::Image { grid: Some(grid), mask })
    }

    /// Copies of `ids` in stack order (plan 14e R6): a drawn layer cropped
    /// to its covered pixels, an image with its original.
    pub fn copy(&self, ids: &[String]) -> Vec<Snapshot> {
        let (size, s) = (self.level.size, self.level.art_scale);
        let mut found: Vec<usize> = ids.iter().filter_map(|id| self.index_of(id)).collect();
        found.sort_unstable();
        found.dedup();
        found
            .into_iter()
            .map(|i| {
                let mut item = self.items[i].clone();
                if let ElementKind::Drawn { material, .. } = item.element.kind
                    && let Some(p) =
                        drawn::placed(&item.element, &item.decoded, size, s).and_then(|p| drawn::crop(&p, s))
                {
                    (item.element.kind, item.decoded) = drawn::into_layer(p, material, size, s);
                }
                let original = match &item.element.kind {
                    ElementKind::Image { source, .. } => {
                        self.originals.get(source).map(|b| (source.clone(), b.clone()))
                    }
                    _ => None,
                };
                Snapshot { item, original }
            })
            .collect()
    }

    /// Pastes `items` (copied at art scale `from`) on top, each moved by
    /// `offset` (plan 14e R6): new ids, `copy_name`d names; drawn and raster
    /// art resampled to this level's art scale, images processed again; an
    /// original whose name this level holds with other bytes takes a new
    /// name (one with its bytes is shared). A raster pastes only into a
    /// level of its size. The edits (applied: one step) and the new ids.
    pub fn paste(
        &mut self,
        items: &[Snapshot],
        from: u32,
        offset: (i32, i32),
    ) -> Result<(Vec<Edit>, Vec<String>), DocError> {
        let (mut edits, mut ids) = (Vec::new(), Vec::new());
        for snap in items {
            match self.paste_one(snap, from, offset) {
                Ok((edit, id)) => {
                    edits.push(edit);
                    ids.push(id);
                }
                Err(e) => {
                    self.roll_back(&edits);
                    return Err(e);
                }
            }
        }
        Ok((edits, ids))
    }

    fn paste_one(
        &mut self,
        snap: &Snapshot,
        from: u32,
        offset: (i32, i32),
    ) -> Result<(Edit, String), DocError> {
        let (size, to) = (self.level.size, self.level.art_scale);
        let from = from.clamp(1, MAX_ART_SCALE);
        let stray = self.stray_files();
        let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        let mut e = snap.item.element.clone();
        let names = elements.iter().map(|x| if x.name.is_empty() { x.id.as_str() } else { x.name.as_str() });
        e.name = copy_name(if e.name.is_empty() { &e.id } else { &e.name }, names);
        e.id = new_element_id(&elements, &stray, &e.name, "copy");
        let shift = |a: (i32, i32)| {
            let add =
                |v: i32, d: i32| (i64::from(v) + i64::from(d)).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
            (add(a.0, offset.0), add(a.1, offset.1))
        };
        let mut original = None;
        let mut extra = 0;
        let decoded = match (&mut e.kind, &snap.item.decoded) {
            (ElementKind::Image { at, source, .. }, Decoded::Image { mask, .. }) => {
                *at = shift(*at);
                let (name, bytes) = snap.original.clone().ok_or_else(|| {
                    DocError::Bad(format!("{:?}: its original is missing", snap.item.element.id))
                })?;
                // Its bytes under a name here already are shared; another
                // original of its name is never overwritten (Review Focus 1).
                let same = |(n, b): (&String, &Arc<Vec<u8>>)| (**b == *bytes).then(|| n.clone());
                let shared = self
                    .originals
                    .get_key_value(&name)
                    .and_then(same)
                    .or_else(|| self.originals.iter().find_map(same));
                *source = shared.unwrap_or_else(|| {
                    let dir = olr_format::levels::image_file("");
                    let taken = self
                        .originals
                        .keys()
                        .map(String::as_str)
                        .chain(stray.iter().filter_map(|f| f.strip_prefix(dir.as_str())));
                    new_source_name(taken, &name)
                });
                extra = mask.as_deref().map_or(0, |m| m.keep.len() as u64);
                original = Some((source.clone(), bytes));
                Decoded::Image { grid: None, mask: mask.clone() }
            }
            (ElementKind::Drawn { at, size: boxed, .. }, Decoded::Drawn { art, cells, back }) => {
                let side = (art.width() / from, art.height() / from);
                *at = shift(*at);
                *boxed = (*at != (0, 0) || side != size).then_some(side);
                Decoded::Drawn {
                    art: rescale_art(art, from, to),
                    cells: cells.clone(),
                    back: back.as_ref().map(|b| rescale_art(b, from, to)),
                }
            }
            (ElementKind::Raster, Decoded::Raster { material, front, back }) => {
                let side = (front.width() / from, front.height() / from);
                if side != size || material.len() as u64 != u64::from(size.0) * u64::from(size.1) {
                    return Err(DocError::Bad(format!(
                        "{:?} is a raster of {} × {}: it pastes only into a level of its size ({} × {} here)",
                        snap.item.element.id, side.0, side.1, size.0, size.1
                    )));
                }
                Decoded::Raster {
                    material: material.clone(),
                    front: rescale_art(front, from, to),
                    back: rescale_art(back, from, to),
                }
            }
            _ => {
                return Err(DocError::Bad(format!("{:?}: its data does not match it", snap.item.element.id)));
            }
        };
        let mut all = elements;
        all.push(e.clone());
        self.check_extra(&all, size, to, extra)?;
        let decoded = match (decoded, &original) {
            (Decoded::Image { mask, .. }, Some((_, bytes))) => self.image_grid(&e, &bytes.clone(), mask)?,
            (d, _) => d,
        };
        let id = e.id.clone();
        let item = Item { element: e, decoded };
        let index = self.items.len();
        Ok((self.change(Edit::Insert { index, item: Box::new(Snapshot { item, original }) }).edit, id))
    }

    /// `rect` (level pixels) of drawn layer `id`, within its box (plan 14e
    /// R7): its art and its cells (a plain layer's material), with the
    /// world pixel of its top-left. `None` outside the layer or for
    /// anything but a drawn layer.
    pub fn copy_pixels(&self, id: &str, rect: Rect) -> Option<(PixelClip, (i32, i32))> {
        let i = self.index_of(id)?;
        let item = &self.items[i];
        let (ElementKind::Drawn { material, .. }, Decoded::Drawn { art, cells, .. }) =
            (&item.element.kind, &item.decoded)
        else {
            return None;
        };
        let (at, size) = self.layer_box(i);
        let s = self.level.art_scale;
        if (art.width(), art.height()) != (size.0.checked_mul(s)?, size.1.checked_mul(s)?) {
            return None;
        }
        let local =
            |v: u32, a: i32, side: u32| (i64::from(v) - i64::from(a)).clamp(0, i64::from(side)) as u32;
        let (x0, y0) = (local(rect.x, at.0, size.0), local(rect.y, at.1, size.1));
        let (x1, y1) = (
            local(rect.x.saturating_add(rect.w), at.0, size.0),
            local(rect.y.saturating_add(rect.h), at.1, size.1),
        );
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let (w, h) = (x1 - x0, y1 - y0);
        let cells = (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .map(|(x, y)| match cells {
                Some(c) => {
                    c.get(y as usize * size.0 as usize + x as usize).copied().unwrap_or((*material).into())
                }
                None => (*material).into(),
            })
            .collect();
        let clip = PixelClip { size: (w, h), art: art.crop(x0 * s, y0 * s, w * s, h * s), cells };
        let world =
            |a: i32, v: u32| (i64::from(a) + i64::from(v)).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
        Some((clip, (world(at.0, x0), world(at.1, y0))))
    }

    /// A new boxed layer "Pasted" of `clip` (copied at art scale `from`,
    /// resampled to this level's) at world pixel `at`, on top (plan 14e R7):
    /// plain when its covered pixels share one material, else mixed (C3).
    /// The edit (applied) and its id.
    pub fn paste_pixels(
        &mut self,
        clip: &PixelClip,
        from: u32,
        at: (i32, i32),
    ) -> Result<(Edit, String), DocError> {
        let (size, to) = (self.level.size, self.level.art_scale);
        let from = from.clamp(1, MAX_ART_SCALE);
        let fits = (clip.art.width(), clip.art.height())
            == (clip.size.0.saturating_mul(from), clip.size.1.saturating_mul(from))
            && clip.cells.len() as u64 == u64::from(clip.size.0) * u64::from(clip.size.1);
        if !fits || clip.size.0 == 0 || clip.size.1 == 0 {
            return Err(DocError::Bad("the pixels copied do not fit their size".into()));
        }
        let art = rescale_art(&clip.art, from, to);
        // The layer's material: its covered pixels' most common (C3).
        let covered = |x: u32, y: u32| art.get(x * to + to / 2, y * to + to / 2).a != 0;
        let mut counts = [0usize; 3];
        for y in 0..clip.size.1 {
            for x in 0..clip.size.0 {
                if covered(x, y) {
                    counts[usize::from(drawn::priority(clip.cells[(y * clip.size.0 + x) as usize]))] += 1;
                }
            }
        }
        // The most common; a tie goes to the higher priority, rock first
        // (plan 14e minors); none covered: dirt.
        let most = (0..3).max_by_key(|&k| (counts[k], k)).filter(|&k| counts[k] > 0);
        let material = match most {
            Some(2) => ElementMaterial::Rock,
            Some(0) => ElementMaterial::Background,
            _ => ElementMaterial::Dirt,
        };
        let placed = drawn::Placed { at, size: clip.size, art, cells: clip.cells.clone(), back: None };
        let (mut kind, decoded) = drawn::into_layer(placed, material, size, to);
        if let ElementKind::Drawn { size: boxed, .. } = &mut kind {
            *boxed = Some(clip.size).filter(|s| at != (0, 0) || *s != size);
        }
        let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        let names = elements.iter().map(|x| if x.name.is_empty() { x.id.as_str() } else { x.name.as_str() });
        let name =
            if names.clone().any(|n| n == "Pasted") { copy_name("Pasted", names) } else { "Pasted".into() };
        let element = Element {
            id: new_element_id(&elements, &self.stray_files(), &name, "pasted"),
            name,
            visible: true,
            locked: false,
            kind,
            glow: None,
            clear: false,
        };
        self.check_with(|es| es.push(element.clone()))?;
        let id = element.id.clone();
        let index = self.items.len();
        let item = Item { element, decoded };
        Ok((self.change(Edit::Insert { index, item: Box::new(Snapshot { item, original: None }) }).edit, id))
    }

    /// An empty drawn layer on top.
    pub fn add_layer(&mut self, material: ElementMaterial, name: &str) -> Result<Change, DocError> {
        let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        let element = Element {
            id: new_element_id(&elements, &self.stray_files(), name, "layer"),
            name: name.into(),
            visible: true,
            locked: false,
            kind: ElementKind::drawn(material),
            glow: None,
            clear: false,
        };
        self.check_with(|es| es.push(element.clone()))?;
        let (w, h) = self.level.size;
        let s = self.level.art_scale;
        let item = Item {
            element,
            decoded: Decoded::Drawn { art: RgbaImage::new(w * s, h * s), cells: None, back: None },
        };
        let index = self.items.len();
        Ok(self.change(Edit::Insert { index, item: Box::new(Snapshot { item, original: None }) }))
    }

    /// Splits element `id` (unlocked, visible) into Background, Dirt and
    /// Rock layers in its place (plan 14d R11, `drawn::split_element`): a
    /// material without pixels makes none. The edits (the element out, the
    /// layers in) are applied: one history step.
    pub fn split(&mut self, id: &str) -> Result<Vec<Edit>, DocError> {
        let index = self.unlocked(id)?;
        let (size, s) = (self.level.size, self.level.art_scale);
        let item = self.items[index].clone();
        let mut others: Vec<Element> =
            self.items.iter().filter(|i| i.element.id != id).map(|i| i.element.clone()).collect();
        let stray = self.stray_files();
        let parts = drawn::split_element(&item.element, &item.decoded, size, s, |name, fallback| {
            let new = new_element_id(&others, &stray, name, fallback);
            // A raster's files cover a mixed layer's: none of them taken.
            others.push(Element {
                id: new.clone(),
                name: name.into(),
                visible: true,
                locked: false,
                kind: ElementKind::Raster,
                glow: None,
                clear: false,
            });
            new
        })
        .map_err(|e| DocError::Bad(e.to_string()))?;
        if parts.is_empty() {
            return Err(DocError::Bad(format!("{id:?} has no pixels to split")));
        }
        self.check_with(|es| {
            es.splice(index..=index, parts.iter().map(|(e, _)| e.clone()));
        })?;
        let original = self.original_of(&item.element).and_then(|b| match &item.element.kind {
            ElementKind::Image { source, .. } => Some((source.clone(), b)),
            _ => None,
        });
        let mut edits = vec![Edit::Remove { index, item: Box::new(Snapshot { item, original }) }];
        for (k, (element, decoded)) in parts.into_iter().enumerate() {
            let item = Item { element, decoded };
            edits.push(Edit::Insert { index: index + k, item: Box::new(Snapshot { item, original: None }) });
        }
        for e in &edits {
            self.apply(e, true);
        }
        Ok(edits)
    }

    /// Merges element `id` into the drawn layer directly below it (plan 14d
    /// R10, `drawn::merge_down`): both unlocked and visible. The edits (the
    /// upper element out, the lower one swapped for the merge) are applied:
    /// one history step.
    pub fn merge_down(&mut self, id: &str) -> Result<Vec<Edit>, DocError> {
        let upper = self.unlocked(id)?;
        if upper == 0 {
            return Err(DocError::Bad(format!("{id:?} is the bottom element: nothing to merge into")));
        }
        let lower_id = self.items[upper - 1].element.id.clone();
        let lower = self.unlocked(&lower_id)?;
        let (le, ue) = (&self.items[lower], &self.items[upper]);
        let (element, decoded) = drawn::merge_down(
            (&le.element, &le.decoded),
            (&ue.element, &ue.decoded),
            self.level.size,
            self.level.art_scale,
        )
        .map_err(|e| DocError::Bad(e.to_string()))?;
        self.check_with(|es| {
            es[lower] = element.clone();
            es.remove(upper);
        })?;
        let up = self.items[upper].clone();
        let original = self.original_of(&up.element).and_then(|b| match &up.element.kind {
            ElementKind::Image { source, .. } => Some((source.clone(), b)),
            _ => None,
        });
        let edits = vec![
            Edit::Remove { index: upper, item: Box::new(Snapshot { item: up, original }) },
            Edit::Layer {
                index: lower,
                before: Box::new(self.items[lower].clone()),
                after: Box::new(Item { element, decoded }),
            },
        ];
        for e in &edits {
            self.apply(e, true);
        }
        Ok(edits)
    }

    /// Whether `item`'s data fits its element: a drawn layer's art, cells
    /// and back art its box and flags, an image's mask its flag.
    fn fits(&self, item: &Item) -> bool {
        let s = self.level.art_scale;
        let dims = |i: &RgbaImage| (i.width(), i.height());
        match (&item.element.kind, &item.decoded) {
            (ElementKind::Drawn { mixed, back: backed, .. }, Decoded::Drawn { art, cells, back }) => {
                let Some((_, size)) = drawn_box(&item.element.kind, self.level.size) else { return false };
                let box_art = (size.0.saturating_mul(s), size.1.saturating_mul(s));
                dims(art) == box_art
                    && *mixed == cells.is_some()
                    && *backed == back.is_some()
                    && cells.as_ref().is_none_or(|c| c.len() as u64 == u64::from(size.0) * u64::from(size.1))
                    && back.as_ref().is_none_or(|b| dims(b) == box_art)
            }
            (ElementKind::Image { mask, .. }, Decoded::Image { mask: m, .. }) => *mask == m.is_some(),
            (ElementKind::Raster, Decoded::Raster { .. }) => true,
            _ => false,
        }
    }

    /// Swaps item `index` for `after` (an `Edit::Layer`, applied).
    fn swap(&mut self, index: usize, after: Item) -> Edit {
        let before = Box::new(self.items[index].clone());
        let edit = Edit::Layer { index, before, after: Box::new(after) };
        self.apply(&edit, true);
        edit
    }

    /// A property (ruling R4: refused on a locked element unless it is
    /// `Locked`), checked with `check_elements`; an image reprocesses
    /// through the cache only when its pixelation or box size changes.
    pub fn set(&mut self, id: &str, property: Property) -> Result<Change, DocError> {
        let index = self.find(id)?;
        // C4: a material chosen for a mixed layer makes it plain, one swap.
        if let Property::Material(m) = property
            && let ElementKind::Drawn { mixed: true, .. } = self.items[index].element.kind
        {
            self.unlocked(id)?;
            let mut after = self.items[index].clone();
            if let ElementKind::Drawn { material, mixed, .. } = &mut after.element.kind {
                (*material, *mixed) = (m, false);
            }
            if let Decoded::Drawn { cells, .. } = &mut after.decoded {
                *cells = None;
            }
            self.check_with(|es| es[index] = after.element.clone())?;
            let edit = self.swap(index, after);
            let dirty = match &edit {
                Edit::Layer { after, .. } => self.element_rect(&after.element),
                _ => NOTHING,
            };
            return Ok(Change { edit, dirty });
        }
        let before = self.items[index].element.clone();
        let mut after = before.clone();
        with_property(&mut after, property, self.level.size)?;
        self.check_with(|es| es[index] = after.clone())?;
        Ok(self.change(Edit::Element { index, before, after }))
    }

    pub fn remove(&mut self, id: &str) -> Result<Change, DocError> {
        let index = self.unlocked(id)?;
        let item = self.items[index].clone();
        let original = match &item.element.kind {
            ElementKind::Image { source, .. } => {
                self.originals.get(source).map(|b| (source.clone(), b.clone()))
            }
            _ => None,
        };
        Ok(self.change(Edit::Remove { index, item: Box::new(Snapshot { item, original }) }))
    }

    /// The first of `ids` that is locked, as the refusal naming it.
    fn none_locked(&self, ids: &[String]) -> Result<(), DocError> {
        for id in ids {
            if self.items[self.find(id)?].element.locked {
                return Err(DocError::Locked(id.clone()));
            }
        }
        Ok(())
    }

    /// Undoes `edits` (applied, in order), last first.
    fn roll_back(&mut self, edits: &[Edit]) {
        for e in edits.iter().rev() {
            self.apply(e, false);
        }
    }

    /// Runs `op` on each of `ids` in order as one step: its edits (applied)
    /// and the rectangle rebaked; the first refusal undoes the others.
    fn each(
        &mut self,
        ids: &[String],
        mut op: impl FnMut(&mut Document, &str) -> Result<Change, DocError>,
    ) -> Result<(Vec<Edit>, Rect), DocError> {
        let (mut edits, mut dirty) = (Vec::new(), None);
        for id in ids {
            match op(self, id) {
                Ok(c) => {
                    if c.dirty.w > 0 && c.dirty.h > 0 {
                        dirty = Some(Rect::union(dirty, c.dirty));
                    }
                    edits.push(c.edit);
                }
                Err(e) => {
                    self.roll_back(&edits);
                    return Err(e);
                }
            }
        }
        Ok((edits, dirty.unwrap_or(NOTHING)))
    }

    /// One property on each of `ids` (plan 14e R8), checked together: one
    /// step's edits (applied) and the rectangle rebaked. A locked member
    /// refuses all but `Locked`, naming it.
    pub fn set_many(&mut self, ids: &[String], property: Property) -> Result<(Vec<Edit>, Rect), DocError> {
        if !matches!(property, Property::Locked(_)) {
            self.none_locked(ids)?;
        }
        self.each(ids, |d, id| d.set(id, property.clone()))
    }

    /// Moves each of `ids` by `delta` (a raster, which cannot move, stays):
    /// one step; refused when any is locked, naming it.
    pub fn move_many(&mut self, ids: &[String], delta: (i32, i32)) -> Result<(Vec<Edit>, Rect), DocError> {
        self.none_locked(ids)?;
        let add =
            |a: i32, b: i32| (i64::from(a) + i64::from(b)).clamp(i32::MIN.into(), i32::MAX.into()) as i32;
        let mut moves = Vec::new();
        for id in ids {
            match &self.items[self.find(id)?].element.kind {
                ElementKind::Image { at, .. } | ElementKind::Drawn { at, .. } => {
                    moves.push((id.clone(), (add(at.0, delta.0), add(at.1, delta.1))))
                }
                ElementKind::Raster => {}
            }
        }
        let ids: Vec<String> = moves.iter().map(|m| m.0.clone()).collect();
        self.each(&ids, |d, id| {
            let to = moves.iter().find(|m| m.0 == id).map_or((0, 0), |m| m.1);
            d.set(id, Property::At(to))
        })
    }

    /// Removes `ids` (top first): one step's edits (applied) and the
    /// rectangle rebaked; refused when any is locked, naming it.
    pub fn remove_many(&mut self, ids: &[String]) -> Result<(Vec<Edit>, Rect), DocError> {
        self.none_locked(ids)?;
        let mut top_first = ids.to_vec();
        top_first.sort_by_key(|id| std::cmp::Reverse(self.index_of(id)));
        self.each(&top_first, |d, id| d.remove(id))
    }

    /// Moves the element to `index` (bottom 0, clamped to the top); allowed
    /// when locked (ruling R4).
    pub fn move_to(&mut self, id: &str, index: usize) -> Result<Change, DocError> {
        let from = self.find(id)?;
        let to = index.min(self.items.len() - 1);
        Ok(self.change(Edit::Move { from, to }))
    }

    /// Drawn layer `id` (unlocked), by index; a picture is refused with
    /// `PICTURE`.
    fn paintable(&self, id: &str) -> Result<usize, DocError> {
        let i = self.unlocked(id)?;
        match self.items[i].decoded {
            Decoded::Drawn { .. } => Ok(i),
            Decoded::Image { .. } => Err(DocError::Bad(PICTURE.into())),
            _ => Err(DocError::WrongKind { id: id.into(), want: "a drawn element" }),
        }
    }

    /// Drawn layer `i`'s box (its top-left, its size).
    fn layer_box(&self, i: usize) -> ((i32, i32), (u32, u32)) {
        drawn_box(&self.items[i].element.kind, self.level.size).unwrap_or(((0, 0), self.level.size))
    }

    /// Makes `id` ready for `stroke` (plan 14d R5-R7): a plain layer painted
    /// another material, or erased with back art, becomes mixed; a picture
    /// erased or restored without a mask gets a full one. The conversion is
    /// applied and returned for the stroke's step; `None` when none is
    /// needed. Painting a picture is refused.
    pub fn begin_stroke(&mut self, id: &str, stroke: Stroke) -> Result<Option<Edit>, DocError> {
        let i = self.unlocked(id)?;
        let s = self.level.art_scale;
        let item = self.items[i].clone();
        match (&item.element.kind, &item.decoded, stroke) {
            (ElementKind::Image { .. }, _, Stroke::Paint(_)) => Err(DocError::Bad(PICTURE.into())),
            // Nothing to restore without a mask (batch C review 1).
            (ElementKind::Image { .. }, Decoded::Image { mask: None, .. }, Stroke::Restore) => Ok(None),
            (ElementKind::Image { source, .. }, Decoded::Image { mask: None, .. }, _) => {
                let bytes = self
                    .original_of(&item.element)
                    .ok_or_else(|| DocError::Bad(format!("{id:?}: its original is missing")))?;
                let img = self.cache.original(source, &bytes)?;
                let mask = Mask::full((img.width(), img.height()));
                let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
                self.check_extra(&elements, self.level.size, s, mask.keep.len() as u64)?;
                let mut after = item.clone();
                if let ElementKind::Image { mask, .. } = &mut after.element.kind {
                    *mask = true;
                }
                if let Decoded::Image { mask: m, .. } = &mut after.decoded {
                    *m = Some(Arc::new(mask));
                }
                Ok(Some(self.swap(i, after)))
            }
            (ElementKind::Drawn { material, .. }, Decoded::Drawn { cells: None, back, .. }, stroke) => {
                let needed = match stroke {
                    Stroke::Paint(m) => m != Material::from(*material),
                    // Only dirt and rock dig: a background layer erases plain.
                    Stroke::Erase => back.is_some() && *material != ElementMaterial::Background,
                    Stroke::Restore => false,
                };
                if !needed {
                    return Ok(None);
                }
                let mut after = item.clone();
                drawn::mix(&mut after.decoded, s, (*material).into());
                if let ElementKind::Drawn { mixed, .. } = &mut after.element.kind {
                    *mixed = true;
                }
                self.check_with(|es| es[i] = after.element.clone())?;
                Ok(Some(self.swap(i, after)))
            }
            _ => Ok(None),
        }
    }

    /// Paints into drawn layer `id` (unlocked) without recording: strokes
    /// do (`stroke_edit`). `material` none is the layer's; another on a
    /// plain layer is refused (`begin_stroke` makes it mixed first).
    /// Returns the level rectangle rebaked.
    pub fn paint(
        &mut self,
        id: &str,
        shape: &Shape,
        paint: &Paint,
        material: Option<Material>,
    ) -> Result<Option<Rect>, DocError> {
        let i = self.paintable(id)?;
        let (size, s) = (self.level.size, self.level.art_scale);
        let (at, _) = self.layer_box(i);
        let item = &mut self.items[i];
        let ElementKind::Drawn { material: own, .. } = item.element.kind else { return Ok(None) };
        let m = material.unwrap_or(own.into());
        if m != Material::from(own) && matches!(item.decoded, Decoded::Drawn { cells: None, .. }) {
            return Err(DocError::Bad(format!(
                "{id:?} is plain: begin the stroke to paint another material"
            )));
        }
        let rect = drawn::draw(&mut item.decoded, at, s, size, shape, paint, m);
        if let Some(r) = rect {
            self.rebake(r);
        }
        Ok(rect)
    }

    /// Erases `shape` in drawn layer `id` (unlocked; plan 14d R5), or masks
    /// picture `id` there (`restore`: unmasks; it needs its mask,
    /// `begin_stroke`). Returns the level rectangle rebaked.
    pub fn erase(&mut self, id: &str, shape: &Shape, restore: bool) -> Result<Option<Rect>, DocError> {
        let i = self.unlocked(id)?;
        let (size, s) = (self.level.size, self.level.art_scale);
        if let ElementKind::Image { at, size: bsize, .. } = self.items[i].element.kind {
            let Decoded::Image { mask: Some(mask), .. } = &mut self.items[i].decoded else {
                // Restoring a picture with no mask restores nothing.
                if restore {
                    return Ok(None);
                }
                return Err(DocError::Bad(format!("{id:?} has no mask: begin the stroke first")));
            };
            if mask_shape(Arc::make_mut(mask), at, bsize, shape, restore).is_none() {
                return Ok(None);
            }
            self.redecode(i);
            let rect = self.element_rect(&self.items[i].element);
            if rect.w == 0 || rect.h == 0 {
                return Ok(None);
            }
            self.rebake(rect);
            return Ok(Some(rect));
        }
        let i = self.paintable(id)?;
        let (at, _) = self.layer_box(i);
        let rect = drawn::erase(&mut self.items[i].decoded, at, s, size, shape);
        if let Some(r) = rect {
            self.rebake(r);
        }
        Ok(rect)
    }

    /// Flood-fills drawn layer `id` (unlocked) from world pixel `seed`
    /// (plan 14e R5, `olr_edit::drawn::fill`) with `paint` of `material`
    /// (none: the layer's): one step, its edits applied (its conversion to
    /// mixed first, when the material needs it). No edits when nothing
    /// changed; a picture is refused.
    pub fn fill(
        &mut self,
        id: &str,
        seed: (i32, i32),
        paint: &Paint,
        material: Option<Material>,
        tolerance: u8,
    ) -> Result<Vec<Edit>, DocError> {
        let i = self.paintable(id)?;
        let ElementKind::Drawn { material: own, .. } = self.items[i].element.kind else {
            return Ok(Vec::new());
        };
        let m = material.unwrap_or(own.into());
        let mut edits: Vec<Edit> = self.begin_stroke(id, Stroke::Paint(m))?.into_iter().collect();
        let before = self.stroke_before(id);
        let (size, s) = (self.level.size, self.level.art_scale);
        let (at, _) = self.layer_box(i);
        let rect = drawn::fill(&mut self.items[i].decoded, at, s, size, seed, paint, m, tolerance);
        if let Some(r) = rect {
            self.rebake(r);
        }
        match rect.zip(before).and_then(|(r, b)| self.stroke_edit(id, r, &b)).filter(|e| !e.is_noop()) {
            Some(edit) => edits.push(edit),
            // Nothing filled: no step, nor its conversion.
            None => {
                for e in edits.drain(..).rev() {
                    self.apply(&e, false);
                }
            }
        }
        Ok(edits)
    }

    /// Masks the region of similar colour around world pixel `at` of
    /// picture `id` (unlocked; plan 14d R8, `tolerance` percent): one step,
    /// its edits applied (a full mask first when it had none).
    pub fn remove_colour(&mut self, id: &str, at: (i32, i32), tolerance: u8) -> Result<Vec<Edit>, DocError> {
        let i = self.unlocked(id)?;
        let ElementKind::Image { source, at: box_at, size: bsize, .. } = self.items[i].element.kind.clone()
        else {
            return Err(DocError::WrongKind { id: id.into(), want: "an image element" });
        };
        let bytes = self
            .original_of(&self.items[i].element)
            .ok_or_else(|| DocError::Bad(format!("{id:?}: its original is missing")))?;
        let img = self.cache.original(&source, &bytes)?;
        let seed = original_pixel(box_at, bsize, (img.width(), img.height()), at)
            .ok_or_else(|| DocError::Bad(format!("({}, {}) is outside the picture", at.0, at.1)))?;
        let mut edits: Vec<Edit> = self.begin_stroke(id, Stroke::Erase)?.into_iter().collect();
        let before = self.stroke_before(id);
        if let Decoded::Image { mask: Some(mask), .. } = &mut self.items[i].decoded {
            mask_colour(Arc::make_mut(mask), &img, seed, tolerance);
        }
        self.redecode(i);
        let rect = self.element_rect(&self.items[i].element);
        if rect.w > 0 && rect.h > 0 {
            self.rebake(rect);
        }
        match before.and_then(|b| self.stroke_edit(id, NOTHING, &b)) {
            Some(edit) => edits.push(edit),
            // Nothing masked: no step, nor the mask it began with.
            None => {
                for e in edits.drain(..).rev() {
                    self.apply(&e, false);
                }
            }
        }
        Ok(edits)
    }

    /// Drawn layer `id`'s art (for the eyedropper and tests).
    pub fn layer_art(&self, id: &str) -> Option<&RgbaImage> {
        match &self.items.get(self.index_of(id)?)?.decoded {
            Decoded::Drawn { art, .. } => Some(art),
            _ => None,
        }
    }

    /// A copy of what a stroke of `id` changes: a drawn layer's data, an
    /// image's mask (none without one).
    pub fn stroke_before(&self, id: &str) -> Option<StrokeBefore> {
        match &self.items.get(self.index_of(id)?)?.decoded {
            d @ Decoded::Drawn { .. } => Some(StrokeBefore::Layer(d.clone())),
            Decoded::Image { mask: Some(m), .. } => Some(StrokeBefore::Mask(m.clone())),
            _ => None,
        }
    }

    /// The edit of a finished stroke: for a drawn layer a `Pixels` edit of
    /// `rect` (a level rectangle; the part in the layer's box), for a
    /// picture a `Mask` edit of what changed. `None` for a stale copy (of
    /// another size or kind: its step could restore nothing), or a mask
    /// that did not change.
    pub fn stroke_edit(&self, id: &str, rect: Rect, before: &StrokeBefore) -> Option<Edit> {
        let i = self.index_of(id)?;
        let s = self.level.art_scale;
        match (before, &self.items[i].decoded) {
            (StrokeBefore::Layer(b @ Decoded::Drawn { art: ba, cells: bc, back: bb }), now) => {
                let Decoded::Drawn { art, cells, back } = now else { return None };
                let dims = |i: &RgbaImage| (i.width(), i.height());
                let alike = dims(ba) == dims(art)
                    && bc.as_ref().map(Vec::len) == cells.as_ref().map(Vec::len)
                    && bb.as_ref().map(dims) == back.as_ref().map(dims);
                let (at, size) = self.layer_box(i);
                if !alike || dims(art) != (size.0.saturating_mul(s), size.1.saturating_mul(s)) {
                    return None;
                }
                let c = |v: i64, side: u32| v.clamp(0, i64::from(side)) as u32;
                let (x0, y0) = (i64::from(rect.x) - i64::from(at.0), i64::from(rect.y) - i64::from(at.1));
                let (x1, y1) = (x0 + i64::from(rect.w), y0 + i64::from(rect.h));
                let (x0, y0, x1, y1) = (c(x0, size.0), c(y0, size.1), c(x1, size.0), c(y1, size.1));
                let local = Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
                Some(Edit::Pixels {
                    id: id.into(),
                    rect: local,
                    before: patch_of(b, local, s, size.0)?,
                    after: patch_of(now, local, s, size.0)?,
                })
            }
            (StrokeBefore::Mask(b), Decoded::Image { mask: Some(now), .. }) => {
                let r = now.diff(b)?;
                Some(Edit::Mask { id: id.into(), rect: r, before: b.crop(r), after: now.crop(r) })
            }
            _ => None,
        }
    }

    /// The objects with `edit` applied to a copy (the level's bounds on them).
    fn objects(
        &mut self,
        edit: impl FnOnce(&mut LevelSource) -> Result<(), level::EditError>,
    ) -> Result<Change, DocError> {
        let before = self.level.objects.clone();
        let mut tmp = LevelSource {
            name: String::new(),
            notes: String::new(),
            size: self.level.size,
            art_scale: self.level.art_scale,
            theme: String::new(),
            objects: before.clone(),
            material: Vec::new(),
            front: RgbaImage::new(0, 0),
            back: RgbaImage::new(0, 0),
            thumb: None,
            look: None,
            glow: None,
            clear: None,
        };
        edit(&mut tmp)?;
        Ok(self.change(Edit::Objects { before, after: tmp.objects }))
    }

    pub fn add_spawn(&mut self, at: (i32, i32)) -> Result<Change, DocError> {
        self.objects(|l| level::add_spawn(l, at, "").map(|_| ()))
    }

    pub fn move_object(&mut self, index: usize, at: (i32, i32)) -> Result<Change, DocError> {
        self.objects(|l| level::move_object(l, index, at))
    }

    pub fn remove_object(&mut self, index: usize) -> Result<Change, DocError> {
        self.objects(|l| level::remove_object(l, index).map(|_| ()))
    }

    /// Name, notes and theme (`catalog` gives a new theme's tiles; a whole rebake).
    pub fn set_meta(&mut self, meta: LevelMeta, catalog: &Catalog) -> Result<Change, DocError> {
        let theme = catalog
            .themes
            .iter()
            .find(|t| t.id == meta.theme)
            .ok_or_else(|| DocError::NoTheme(meta.theme.clone()))?;
        self.themes.insert(theme.id.clone(), (theme.front_tile.clone(), theme.back_tile.clone()));
        let before = self.meta();
        Ok(self.change(Edit::Meta { before, after: meta }))
    }

    pub fn meta(&self) -> LevelMeta {
        LevelMeta {
            name: self.level.name.clone(),
            notes: self.level.notes.clone(),
            theme: self.level.theme.clone(),
        }
    }

    fn whole(&self) -> Whole {
        Whole { level: self.level.clone(), items: self.items.clone() }
    }

    /// As `olr-cli level resize`: the elements follow (`resize_decoded`),
    /// then the level.
    pub fn resize(
        &mut self,
        size: (u32, u32),
        offset: (i32, i32),
        material: Material,
        colour: Rgba,
    ) -> Result<Change, DocError> {
        let paint = Paint::Colour(colour);
        let (old, s) = (self.level.size, self.level.art_scale);
        // The budget from the metadata first: no layer is re-laid for a
        // resize that is refused.
        let mut moved: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        for e in &mut moved {
            // As `resize_decoded`: a level-sized layer off the origin is
            // pinned to its box, and boxes move.
            if let ElementKind::Drawn { at, size: boxed @ None, .. } = &mut e.kind
                && *at != (0, 0)
            {
                *boxed = Some(old);
            }
            if let ElementKind::Image { at, .. } | ElementKind::Drawn { at, size: Some(_), .. } = &mut e.kind
            {
                let shift =
                    |v: i32, by: i32| v.checked_add(by).ok_or_else(|| level::EditError::Moved(e.id.clone()));
                *at = (shift(at.0, offset.0)?, shift(at.1, offset.1)?);
            }
        }
        self.check(&moved, size, s)?;
        let mut level = self.level.clone();
        level::resize(&mut level, size, offset, material, &paint)?;
        let mut items = self.items.clone();
        for i in &mut items {
            resize_decoded(&mut i.element, &mut i.decoded, old, s, size, offset, material, &paint)?;
        }
        let elements: Vec<Element> = items.iter().map(|i| i.element.clone()).collect();
        self.check(&elements, size, s)?;
        let before = Box::new(self.whole());
        Ok(self.change(Edit::Whole { before, after: Box::new(Whole { level, items }) }))
    }

    /// Rescales drawn and raster layers (`rescale_art`) and reprocesses images.
    pub fn set_art_scale(&mut self, to: u32) -> Result<Change, DocError> {
        let (w, h) = self.level.size;
        let from = self.level.art_scale;
        if !(1..=MAX_ART_SCALE).contains(&to) {
            return Err(DocError::Bad(format!("art scale {to}: 1 to {MAX_ART_SCALE}")));
        }
        let (aw, ah) = (w * to, h * to);
        check_size(aw, ah, LEVEL_ART).map_err(|e| DocError::Bad(format!("art of {aw}x{ah}: {e}")))?;
        let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        self.check(&elements, (w, h), to)?;
        let mut level = self.level.clone();
        level.art_scale = to;
        level.front = rescale_art(&level.front, from, to);
        level.back = rescale_art(&level.back, from, to);
        let mut items = Vec::with_capacity(self.items.len());
        let old = std::mem::replace(&mut self.level.art_scale, to);
        let mut result = Ok(());
        for i in &self.items.clone() {
            let decoded = match &i.decoded {
                Decoded::Drawn { art, cells, back } => Decoded::Drawn {
                    art: rescale_art(art, from, to),
                    cells: cells.clone(),
                    back: back.as_ref().map(|b| rescale_art(b, from, to)),
                },
                Decoded::Raster { material, front, back } => Decoded::Raster {
                    material: material.clone(),
                    front: rescale_art(front, from, to),
                    back: rescale_art(back, from, to),
                },
                Decoded::Image { mask, .. } => match self.original_of(&i.element) {
                    Some(bytes) => match self.image_grid(&i.element, &bytes, mask.clone()) {
                        Ok(d) => d,
                        Err(e) => {
                            result = Err(e);
                            break;
                        }
                    },
                    None => Decoded::Image { grid: None, mask: mask.clone() },
                },
            };
            items.push(Item { element: i.element.clone(), decoded });
        }
        self.level.art_scale = old;
        result?;
        let before = Box::new(self.whole());
        Ok(self.change(Edit::Whole { before, after: Box::new(Whole { level, items }) }))
    }

    /// An image element's original bytes.
    pub(crate) fn original_of(&self, e: &Element) -> Option<Arc<Vec<u8>>> {
        match &e.kind {
            ElementKind::Image { source, .. } => self.originals.get(source).cloned(),
            _ => None,
        }
    }

    /// The cache's `lookups()` (tests: a move keeps its grid).
    #[doc(hidden)]
    pub fn cache_lookups(&self) -> usize {
        self.cache.lookups()
    }

    /// The cache's `processed()` (tests: a move never reprocesses).
    #[doc(hidden)]
    pub fn cache_processed(&self) -> usize {
        self.cache.processed()
    }

    /// Re-decodes an image item after its element changed (a cache hit
    /// unless its pixelation or box changed).
    pub(crate) fn redecode(&mut self, index: usize) {
        let e = self.items[index].element.clone();
        if !matches!(e.kind, ElementKind::Image { .. }) {
            return;
        }
        let mask = match &self.items[index].decoded {
            Decoded::Image { mask, .. } => mask.clone(),
            _ => None,
        };
        let decoded = match self.original_of(&e) {
            Some(bytes) => self
                .image_grid(&e, &bytes, mask.clone())
                .unwrap_or(Decoded::Image { grid: None, mask: mask.clone() }),
            None => Decoded::Image { grid: None, mask },
        };
        self.items[index].decoded = decoded;
    }

    /// The world rectangle element `e` can cover: an image's or a drawn
    /// layer's box (clipped), the whole level for a raster.
    pub(crate) fn element_rect(&self, e: &Element) -> Rect {
        let boxed = match &e.kind {
            ElementKind::Image { at, size, .. } => Some((*at, *size)),
            kind => drawn_box(kind, self.level.size),
        };
        match boxed {
            Some((at, size)) => {
                let (w, h) = self.level.size;
                let clip = |v: i64, side: u32| v.clamp(0, i64::from(side)) as u32;
                let (x0, y0) = (clip(at.0.into(), w), clip(at.1.into(), h));
                let x1 = clip(i64::from(at.0) + i64::from(size.0), w);
                let y1 = clip(i64::from(at.1) + i64::from(size.1), h);
                Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
            }
            _ => self.all(),
        }
    }

    /// Applies (`forward`) or reverts `edit`; returns the rectangle it rebaked.
    pub fn apply(&mut self, edit: &Edit, forward: bool) -> Rect {
        let dirty = match edit {
            Edit::Pixels { id, rect, before, after } => {
                let p = if forward { after } else { before };
                let s = self.level.art_scale;
                let Some(i) = self.index_of(id) else { return NOTHING };
                let (at, size) = self.layer_box(i);
                let Decoded::Drawn { art, cells, back } = &mut self.items[i].decoded else { return NOTHING };
                // A rectangle off the layer, or a patch of another shape (a
                // forged edit), changes nothing.
                let inside = rect.x.checked_add(rect.w).is_some_and(|e| e <= size.0)
                    && rect.y.checked_add(rect.h).is_some_and(|e| e <= size.1);
                let dims = |i: &RgbaImage| (i.width(), i.height());
                let want = (rect.w.saturating_mul(s), rect.h.saturating_mul(s));
                let n = rect.w as usize * rect.h as usize;
                let box_art = (size.0.saturating_mul(s), size.1.saturating_mul(s));
                let fits = inside
                    && dims(&p.art) == want
                    && dims(art) == box_art
                    && cells.as_ref().is_none_or(|c| c.len() as u64 == u64::from(size.0) * u64::from(size.1))
                    && back.as_ref().is_none_or(|b| dims(b) == box_art)
                    && p.cells.as_ref().map(Vec::len) == cells.as_ref().map(|_| n)
                    && p.back.as_ref().map(dims) == back.as_ref().map(|_| want);
                if !fits {
                    return NOTHING;
                }
                let paste = |to: &mut RgbaImage, from: &RgbaImage| {
                    for y in 0..from.height() {
                        for x in 0..from.width() {
                            to.set(rect.x * s + x, rect.y * s + y, from.get(x, y));
                        }
                    }
                };
                paste(art, &p.art);
                if let (Some(to), Some(from)) = (back.as_mut(), p.back.as_ref()) {
                    paste(to, from);
                }
                if let (Some(to), Some(from)) = (cells.as_mut(), p.cells.as_ref()) {
                    for (k, row) in from.chunks(rect.w.max(1) as usize).enumerate() {
                        let at = (rect.y as usize + k) * size.0 as usize + rect.x as usize;
                        to[at..at + row.len()].copy_from_slice(row);
                    }
                }
                to_level(at, *rect, self.level.size)
            }
            Edit::Layer { index, before, after } => {
                let to = if forward { after } else { before };
                // A swap of another element, or of data that does not fit
                // its element, changes nothing (batch C review 3).
                let same = self.items.get(*index).is_some_and(|i| i.element.id == to.element.id);
                if !same || !self.fits(to) {
                    return NOTHING;
                }
                let from = std::mem::replace(&mut self.items[*index], (**to).clone());
                Rect::union(Some(self.element_rect(&from.element)), self.element_rect(&to.element))
            }
            Edit::Mask { id, rect, before, after } => {
                let bytes = if forward { after } else { before };
                let Some(i) = self.index_of(id) else { return NOTHING };
                let Decoded::Image { mask: Some(mask), .. } = &mut self.items[i].decoded else {
                    return NOTHING;
                };
                if !Arc::make_mut(mask).patch(*rect, bytes) {
                    return NOTHING;
                }
                self.redecode(i);
                self.element_rect(&self.items[i].element)
            }
            Edit::Element { index, before, after } => {
                let (from, to) = if forward { (before, after) } else { (after, before) };
                let Some(item) = self.items.get_mut(*index) else { return NOTHING };
                item.element = to.clone();
                // A rename or a lock changes no pixel.
                let same = Element { name: to.name.clone(), locked: to.locked, ..from.clone() };
                if same == *to {
                    return NOTHING;
                }
                // A move or a material change keeps the grid: no lookup.
                if grid_key(from) != grid_key(to) {
                    self.redecode(*index);
                }
                Rect::union(Some(self.element_rect(from)), self.element_rect(to))
            }
            Edit::Insert { index, item } | Edit::Remove { index, item } => {
                let insert = matches!(edit, Edit::Insert { .. }) == forward;
                if insert {
                    if let Some((name, bytes)) = &item.original {
                        self.originals.insert(name.clone(), bytes.clone());
                    }
                    let at = (*index).min(self.items.len());
                    self.items.insert(at, item.item.clone());
                } else if *index < self.items.len() {
                    self.items.remove(*index);
                }
                self.element_rect(&item.item.element)
            }
            Edit::Move { from, to } => {
                let (a, b) = if forward { (*from, *to) } else { (*to, *from) };
                if a >= self.items.len() || b >= self.items.len() {
                    return NOTHING;
                }
                let item = self.items.remove(a);
                let rect = self.element_rect(&item.element);
                self.items.insert(b, item);
                rect
            }
            Edit::Objects { before, after } => {
                self.level.objects = if forward { after } else { before }.clone();
                NOTHING
            }
            Edit::Meta { before, after } => {
                let m = if forward { after } else { before };
                self.level.name = m.name.clone();
                self.level.notes = m.notes.clone();
                if self.level.theme == m.theme {
                    return NOTHING;
                }
                self.level.theme = m.theme.clone();
                if let Some((front, back)) = self.themes.get(&m.theme) {
                    (self.front_tile, self.back_tile) = (front.clone(), back.clone());
                }
                self.all()
            }
            Edit::Whole { before, after } => {
                let w = if forward { after } else { before };
                self.level = w.level.clone();
                self.items = w.items.clone();
                self.all()
            }
            Edit::Look { before, after } => {
                let (from, to) = if forward { (before, after) } else { (after, before) };
                self.level.look = to.as_deref().cloned();
                // The bake numbers groups by their order: other groups, another bake.
                let ids = |l: &Option<Box<olr_format::look::LookSource>>| {
                    l.iter().flat_map(|l| l.glow_groups.iter().map(|g| g.id.clone())).collect::<Vec<_>>()
                };
                if ids(from) == ids(to) { NOTHING } else { self.all() }
            }
            Edit::GlowMask { id, before, after } => {
                let to = if forward { after } else { before };
                match to {
                    Some(m) => self.glow_masks.insert(id.clone(), m.clone()),
                    None => self.glow_masks.remove(id),
                };
                match self.index_of(id) {
                    Some(i) => self.element_rect(&self.items[i].element),
                    None => NOTHING,
                }
            }
        };
        if dirty.w > 0 && dirty.h > 0 {
            self.rebake(dirty);
        }
        dirty
    }
}
