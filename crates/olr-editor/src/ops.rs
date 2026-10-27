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
use olr_edit::element::{DISTANT_ORDER, Property, copy_name, new_element_id, new_source_name};
use olr_edit::level::{self, Paint, Shape, rescale_art};
use olr_edit::mask::{Mask, mask_colour, mask_shape, original_pixel};
use olr_edit::pixelate::Pixelation;
use olr_format::image_io::{LEVEL_ART, check_size};
use olr_format::levels::{
    Dither, Element, ElementKind, ElementMaterial, LevelSource, MAX_ART_SCALE, MAX_COMPOSED_PIXELS,
    MAX_ELEMENTS, check_elements, drawn_box, surface_back_file, surface_file,
};

use crate::catalog::Catalog;
use crate::document::{DocError, Document, Item, tidied};
use crate::history::{Edit, LevelMeta, Patch, Snapshot, StatesSnap, Whole};

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

/// Sets one property of `e`, as `olr_edit::element::set` does: a locked
/// element takes only `Locked`. `locked` is its effective lock (issue #60:
/// it, or a folder holding it, `Document::locked_at`).
pub(crate) fn with_property(
    e: &mut Element,
    property: Property,
    level: (u32, u32),
    locked: bool,
) -> Result<(), DocError> {
    if locked && !matches!(property, Property::Locked(_)) {
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
        // Issue #43: a back surface needs back art (`Document::set` checks
        // the id is one of the level's materials).
        Property::Surface(m) => e.surface = m,
        Property::BackSurface(_) if !e.has_back_art() => return Err(wrong("an element with back art")),
        Property::BackSurface(m) => e.back_surface = m,
        // Issue #11: an image or drawn element (`Document::set_distant`
        // keeps the order).
        Property::Distant(_) if matches!(e.kind, ElementKind::Raster) => {
            return Err(wrong("an image or drawn element"));
        }
        Property::Distant(d) => e.distant = d,
        // The background glass design: an image or drawn element
        // (`check_elements` refuses one not of background).
        Property::Glass(_) if matches!(e.kind, ElementKind::Raster) => {
            return Err(wrong("an image or drawn element"));
        }
        Property::Glass(g) => e.glass = g,
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

/// A distant layer's depth at first, percent (issue #11): the Distant
/// switch's.
pub const DEFAULT_DEPTH: u8 = 50;

/// A new glass's opacity, percent (the background glass design): the Glass
/// switch's.
pub const DEFAULT_GLASS_OPACITY: u8 = 35;

impl Document {
    /// The element `id`, by index.
    pub(crate) fn find(&self, id: &str) -> Result<usize, DocError> {
        self.index_of(id).ok_or_else(|| DocError::NoElement(id.into()))
    }

    /// `id` (found) when it is not locked, nor any folder holding it.
    pub(crate) fn unlocked(&self, id: &str) -> Result<usize, DocError> {
        let i = self.find(id)?;
        if self.locked_at(i) {
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
        // The folders the elements use (issue #60), as saved.
        let total = check_elements(&tidied(elements.to_vec(), &self.folders), size, art_scale)
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
            id: new_element_id(&elements, &self.folders, &stray, stem, "image"),
            name: stem.into(),
            visible: true,
            locked: false,
            folder: None,
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
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        self.check_with(|es| es.push(element.clone()))?;
        let bytes = Arc::new(bytes);
        let decoded = self.image_grid(&element, &bytes, None)?;
        let item = Item { element, decoded };
        let index = self.items.len();
        Ok(self.change(Edit::Insert {
            index,
            item: Box::new(Snapshot { item, original: Some((source, bytes)), glow_mask: None }),
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

    /// Copies of `ids` in stack order (plan 14e R6), flat: in no folder
    /// (`copy_tree` copies folders too).
    pub fn copy(&self, ids: &[String]) -> Vec<Snapshot> {
        let mut items = self.copy_items(ids);
        for s in &mut items {
            s.item.element.folder = None;
        }
        items
    }

    /// Copies of elements `ids` in stack order (plan 14e R6): a drawn layer
    /// cropped to its covered pixels, an image with its original; each
    /// naming the level's folder it is in.
    pub(crate) fn copy_items(&self, ids: &[String]) -> Vec<Snapshot> {
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
                // Its mask, re-laid on the cropped box.
                let from = &self.items[i].element;
                let glow_mask = self.glow_mask(&from.id).and_then(|m| {
                    let old = olr_format::levels::element_box(&from.kind, size)?;
                    let new = olr_format::levels::element_box(&item.element.kind, size)?;
                    Some(Arc::new(crate::look_ops::relay_mask(m, old, new, s)))
                });
                Snapshot { item, original, glow_mask }
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
                    edits.extend(edit);
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
    ) -> Result<(Vec<Edit>, String), DocError> {
        let piece = self.landed(snap.piece(), from, offset, None)?;
        let mut edits = Vec::new();
        // A glow naming a group this level's look lacks gets it (plan 15a,
        // batch D review 1): the paste glows as the copy did.
        if let Some(g) = &piece.element.glow
            && !self.level.look.iter().any(|l| l.glow_groups.iter().any(|x| x.id == g.group))
        {
            let look = crate::look_ops::with_group(self.level.look.clone(), &g.group);
            for edit in self.look_change(Some(look))? {
                self.apply(&edit, true);
                edits.push(edit);
            }
        }
        let id = piece.element.id.clone();
        edits.push(self.insert_landed(piece)?);
        Ok((edits, id))
    }

    /// `piece` (made at art scale `from`) landed in this level, moved by
    /// `offset` (`olr_edit::asset::land`: a new id and name, art resampled,
    /// its mask fitted) and checked, an image's grid processed; not yet in.
    pub(crate) fn landed(
        &mut self,
        piece: olr_edit::asset::PieceRef,
        from: u32,
        offset: (i32, i32),
        name: Option<&str>,
    ) -> Result<olr_edit::asset::Piece, DocError> {
        let (size, to) = (self.level.size, self.level.art_scale);
        let stray = self.stray_files();
        let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        let landing = olr_edit::asset::Landing {
            elements: &elements,
            folders: &self.folders,
            stray: &stray,
            originals: &self.originals,
            size,
            art_scale: to,
        };
        let mut piece = olr_edit::asset::land(&landing, piece, from, offset, name)
            .map_err(|e| DocError::Bad(e.to_string()))?;
        let extra = match &piece.decoded {
            Decoded::Image { mask, .. } => mask.as_deref().map_or(0, |m| m.keep.len() as u64),
            _ => 0,
        };
        let mut all = elements;
        // Where `insert_landed` puts it: a distant one on top of the distant.
        let at = if piece.element.is_distant() { self.distant_count() } else { all.len() };
        all.insert(at, piece.element.clone());
        self.check_extra(&all, size, to, extra)?;
        if let (Decoded::Image { mask, .. }, Some((_, bytes))) = (&piece.decoded, &piece.original) {
            piece.decoded = self.image_grid(&piece.element, &bytes.clone(), mask.clone())?;
        }
        Ok(piece)
    }

    /// Inserts a landed piece on top: its edit (applied).
    pub(crate) fn insert_landed(&mut self, piece: olr_edit::asset::Piece) -> Result<Edit, DocError> {
        let olr_edit::asset::Piece { element, decoded, original, glow_mask, .. } = piece;
        // A distant layer lands on top of the distant ones (issue #11).
        let index = if element.is_distant() { self.distant_count() } else { self.items.len() };
        let snapshot = Snapshot { item: Item { element, decoded }, original, glow_mask };
        Ok(self.change(Edit::Insert { index, item: Box::new(snapshot) }).edit)
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
            id: new_element_id(&elements, &self.folders, &self.stray_files(), &name, "pasted"),
            name,
            visible: true,
            locked: false,
            folder: None,
            kind,
            glow: None,
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        self.check_with(|es| es.push(element.clone()))?;
        let id = element.id.clone();
        let index = self.items.len();
        let item = Item { element, decoded };
        Ok((
            self.change(Edit::Insert {
                index,
                item: Box::new(Snapshot { item, original: None, glow_mask: None }),
            })
            .edit,
            id,
        ))
    }

    /// An empty drawn layer on top.
    pub fn add_layer(&mut self, material: ElementMaterial, name: &str) -> Result<Change, DocError> {
        let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        let element = Element {
            id: new_element_id(&elements, &self.folders, &self.stray_files(), name, "layer"),
            name: name.into(),
            visible: true,
            locked: false,
            folder: None,
            kind: ElementKind::drawn(material),
            glow: None,
            clear: false,
            glass: None,
            distant: None,
            surface: None,
            back_surface: None,
            surface_map: false,
            back_surface_map: false,
        };
        self.check_with(|es| es.push(element.clone()))?;
        let (w, h) = self.level.size;
        let s = self.level.art_scale;
        let item = Item {
            element,
            decoded: Decoded::Drawn { art: RgbaImage::new(w * s, h * s), cells: None, back: None },
        };
        let index = self.items.len();
        Ok(self.change(Edit::Insert {
            index,
            item: Box::new(Snapshot { item, original: None, glow_mask: None }),
        }))
    }

    /// Splits element `id` (unlocked, visible) into Background, Dirt and
    /// Rock layers in its place (plan 14d R11, `drawn::split_element`): a
    /// material without pixels makes none. The edits (the element out, the
    /// layers in) are applied: one history step.
    pub fn split(&mut self, id: &str) -> Result<Vec<Edit>, DocError> {
        let index = self.unlocked(id)?;
        if self.items[index].element.is_distant() {
            return Err(DocError::Bad("a distant layer does not split".into()));
        }
        self.refuse_owned(index)?;
        let (size, s) = (self.level.size, self.level.art_scale);
        let item = self.items[index].clone();
        let mut others: Vec<Element> =
            self.items.iter().filter(|i| i.element.id != id).map(|i| i.element.clone()).collect();
        let stray = self.stray_files();
        // As the bake sees it: a hidden folder hides it (issue #60).
        let seen = Element { visible: self.shown_at(index), ..item.element.clone() };
        let parts = drawn::split_element(&seen, &item.decoded, size, s, |name, fallback| {
            let new = new_element_id(&others, &self.folders, &stray, name, fallback);
            // A raster's files cover a mixed layer's: none of them taken.
            others.push(Element {
                id: new.clone(),
                name: name.into(),
                visible: true,
                locked: false,
                folder: None,
                kind: ElementKind::Raster,
                glow: None,
                clear: false,
                glass: None,
                distant: None,
                surface: None,
                back_surface: None,
                surface_map: false,
                back_surface_map: false,
            });
            new
        })
        .map_err(|e| DocError::Bad(e.to_string()))?;
        if parts.is_empty() {
            return Err(DocError::Bad(format!("{id:?} has no pixels to split")));
        }
        // The parts take its folder (issue #60).
        let parts: Vec<(Element, Decoded)> = parts
            .into_iter()
            .map(|(e, d)| (Element { folder: item.element.folder.clone(), ..e }, d))
            .collect();
        self.check_with(|es| {
            es.splice(index..=index, parts.iter().map(|(e, _)| e.clone()));
        })?;
        let original = self.original_of(&item.element).and_then(|b| match &item.element.kind {
            ElementKind::Image { source, .. } => Some((source.clone(), b)),
            _ => None,
        });
        let glow_mask = self.glow_masks.get(&item.element.id).cloned();
        let mut edits = vec![Edit::Remove { index, item: Box::new(Snapshot { item, original, glow_mask }) }];
        for (k, (element, decoded)) in parts.into_iter().enumerate() {
            let item = Item { element, decoded };
            edits.push(Edit::Insert {
                index: index + k,
                item: Box::new(Snapshot { item, original: None, glow_mask: None }),
            });
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
        if le.element.is_distant() || ue.element.is_distant() {
            return Err(DocError::Bad("a distant layer does not merge".into()));
        }
        self.refuse_owned(upper)?;
        self.refuse_owned(lower)?;
        // The layers as the bake sees them: a hidden folder hides them
        // (issue #60).
        let seen = |i: usize, e: &Element| Element { visible: self.shown_at(i), ..e.clone() };
        let (mut element, decoded) = drawn::merge_down(
            (&seen(lower, &le.element), &le.decoded),
            (&seen(upper, &ue.element), &ue.decoded),
            self.level.size,
            self.level.art_scale,
        )
        .map_err(|e| DocError::Bad(e.to_string()))?;
        // It keeps the lower layer's folder.
        element.folder = le.element.folder.clone();
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
            Edit::Remove {
                index: upper,
                item: Box::new(Snapshot {
                    glow_mask: self.glow_masks.get(&up.element.id).cloned(),
                    item: up,
                    original,
                }),
            },
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
        if let Property::Surface(Some(m)) | Property::BackSurface(Some(m)) = &property
            && self.material_value(m).is_none()
        {
            return Err(DocError::Bad(format!("no material {m:?} in the level's materials (add it first)")));
        }
        let before = self.items[index].element.clone();
        let mut after = before.clone();
        with_property(&mut after, property, self.level.size, self.locked_at(index))?;
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
        let glow_mask = self.glow_masks.get(id).cloned();
        Ok(self.change(Edit::Remove { index, item: Box::new(Snapshot { item, original, glow_mask }) }))
    }

    /// The first of `ids` that is locked, as the refusal naming it.
    fn none_locked(&self, ids: &[String]) -> Result<(), DocError> {
        for id in ids {
            if self.locked_at(self.find(id)?) {
                return Err(DocError::Locked(id.clone()));
            }
        }
        Ok(())
    }

    /// Undoes `edits` (applied, in order), last first.
    pub(crate) fn roll_back(&mut self, edits: &[Edit]) {
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
    /// when locked (ruling R4). Distant layers stay below the rest (issue
    /// #11): the index is clamped to the element's own group, and a move
    /// that clamping undoes is refused with `DISTANT_ORDER`. Issue #60: it
    /// leaves or joins the folder its new place lies in (`landing_folder`),
    /// in the same edit.
    pub fn move_to(&mut self, id: &str, index: usize) -> Result<Change, DocError> {
        let from = self.find(id)?;
        let to = index.min(self.items.len() - 1);
        let distant = self.distant_count();
        let group = if self.items[from].element.is_distant() {
            0..=distant.saturating_sub(1)
        } else {
            distant..=self.items.len() - 1
        };
        let clamped = to.clamp(*group.start(), *group.end());
        if clamped == from && to != from {
            return Err(DocError::Bad(DISTANT_ORDER.into()));
        }
        let before = self.items[from].element.folder.clone();
        let after = self.landing_folder(from, clamped);
        if before != after {
            let mut elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
            let mut e = elements.remove(from);
            e.folder = after.clone();
            elements.insert(clamped, e);
            self.check_with(|es| *es = elements)?;
        }
        Ok(self.change(Edit::Move { from, to: clamped, before, after }))
    }

    /// How many distant layers the document holds (all at the bottom).
    pub(crate) fn distant_count(&self) -> usize {
        self.items.iter().take_while(|i| i.element.is_distant()).count()
    }

    /// Makes element `id` distant at `depth` percent (issue #11), or not
    /// (`None`): one step. Becoming distant or not moves it to the top of
    /// the distant layers (just above them when no longer distant), so they
    /// stay below the rest.
    pub fn set_distant(&mut self, id: &str, depth: Option<u8>) -> Result<(Vec<Edit>, Rect), DocError> {
        let index = self.find(id)?;
        let before = self.items[index].element.clone();
        let mut after = before.clone();
        with_property(&mut after, Property::Distant(depth), self.level.size, self.locked_at(index))?;
        if before.is_distant() == after.is_distant() {
            self.check_with(|es| es[index] = after.clone())?;
            let c = self.change(Edit::Element { index, before, after });
            return Ok((vec![c.edit], c.dirty));
        }
        // It leaves its folders: distant layers share none with the others
        // (issue #60, as `olr_edit::element::set`).
        after.folder = None;
        // Its place: the top of the distant layers without it.
        let to =
            self.items.iter().enumerate().filter(|(i, it)| *i != index && it.element.is_distant()).count();
        self.check_with(|es| {
            es.remove(index);
            es.insert(to, after.clone());
        })?;
        let folder = before.folder.clone();
        let edits = vec![
            Edit::Move { from: index, to, before: folder.clone(), after: folder },
            Edit::Element { index: to, before, after },
        ];
        Ok(self.apply_all(edits))
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
    /// erased or restored without a mask gets a full one. The conversions
    /// are applied and returned for the stroke's step, in order; none when
    /// none is needed. Painting a picture is refused.
    ///
    /// Issue #79: in an asset's state other than the start, a stroke on a
    /// picture is refused, and a drawn layer the state does not paint
    /// itself is made its own first.
    pub fn begin_stroke(&mut self, id: &str, stroke: Stroke) -> Result<Vec<Edit>, DocError> {
        let i = self.unlocked(id)?;
        let owned = self.own_for_stroke(i)?;
        match self.convert_for_stroke(id, stroke) {
            Ok(e) => Ok(owned.into_iter().chain(e).collect()),
            Err(e) => {
                if let Some(o) = &owned {
                    self.apply(o, false);
                }
                Err(e)
            }
        }
    }

    /// `begin_stroke`'s conversion (plan 14d).
    fn convert_for_stroke(&mut self, id: &str, stroke: Stroke) -> Result<Option<Edit>, DocError> {
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
        let mut edits: Vec<Edit> = self.begin_stroke(id, Stroke::Paint(m))?;
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
        let mut edits: Vec<Edit> = self.begin_stroke(id, Stroke::Erase)?;
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
            glass: None,
            glow_beams: None,
            haze: None,
            distant: Vec::new(),
            surface: None,
            distant_surfaces: Vec::new(),
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
        Whole {
            level: self.level.clone(),
            items: self.items.clone(),
            masks: self.glow_masks.clone(),
            surfaces: self.surface_maps.clone(),
            states: self.states_snap(),
        }
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
        // Glow masks follow their layers: a level-sized layer's content
        // moves by `offset` into its new box, a box keeps its own.
        let mut masks = self.glow_masks.clone();
        for (was, now) in self.items.iter().zip(&items) {
            let Some(m) = self.glow_masks.get(&was.element.id) else { continue };
            let (Some((at, b)), Some(new)) = (
                olr_format::levels::element_box(&was.element.kind, old),
                olr_format::levels::element_box(&now.element.kind, size),
            ) else {
                continue;
            };
            let moved = ((at.0.saturating_add(offset.0), at.1.saturating_add(offset.1)), b);
            masks.insert(now.element.id.clone(), Arc::new(crate::look_ops::relay_mask(m, moved, new, s)));
        }
        // Painted surface maps follow them at world resolution (issue #37),
        // a raster's as the level's pixels; the new area takes the defaults.
        let mut surfaces = self.surface_maps.clone();
        for (was, now) in self.items.iter().zip(&items) {
            let from = olr_format::levels::element_box(&was.element.kind, old).unwrap_or(((0, 0), old));
            let to = olr_format::levels::element_box(&now.element.kind, size).unwrap_or(((0, 0), size));
            let moved = ((from.0.0.saturating_add(offset.0), from.0.1.saturating_add(offset.1)), from.1);
            for file in [surface_file(&was.element.id), surface_back_file(&was.element.id)] {
                if let Some(m) = self.surface_maps.get(&file) {
                    surfaces.insert(file, Arc::new(olr_edit::element::relay_surface(m, moved, to)));
                }
            }
        }
        // Issue #79: the states' own pictures (and their glow masks) are
        // re-laid as their layers are.
        let states = self.states_snap().map(|mut snap| {
            for st in &mut snap.states {
                for (id, pic) in st.own.iter_mut() {
                    let (Some(was), Some(now)) = (
                        self.items.iter().find(|i| i.element.id == *id),
                        items.iter().find(|i| i.element.id == *id),
                    ) else {
                        continue;
                    };
                    let mut e = was.element.clone();
                    let mut d = pic.decoded.clone();
                    if resize_decoded(&mut e, &mut d, old, s, size, offset, material, &paint).is_err() {
                        continue;
                    }
                    let glow_mask = pic.glow_mask.as_ref().map(|m| {
                        match (
                            olr_format::levels::element_box(&was.element.kind, old),
                            olr_format::levels::element_box(&now.element.kind, size),
                        ) {
                            (Some((at, b)), Some(new)) => {
                                let moved =
                                    ((at.0.saturating_add(offset.0), at.1.saturating_add(offset.1)), b);
                                Arc::new(crate::look_ops::relay_mask(m, moved, new, s))
                            }
                            _ => m.clone(),
                        }
                    });
                    *pic = Arc::new(olr_edit::asset::OwnPicture { decoded: d, glow_mask });
                }
            }
            snap
        });
        let before = Box::new(self.whole());
        let after = Box::new(Whole { level, items, masks, surfaces, states });
        Ok(self.change(Edit::Whole { before, after }))
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
        // Glow masks and the level's own glow (a plain level's) at the new scale.
        let mut masks = self.glow_masks.clone();
        for i in &items {
            if let (Some(m), Some((_, b))) =
                (self.glow_masks.get(&i.element.id), olr_format::levels::element_box(&i.element.kind, (w, h)))
            {
                let m = crate::look_ops::resample_mask(m, (b.0 * from, b.1 * from), (b.0 * to, b.1 * to));
                masks.insert(i.element.id.clone(), Arc::new(m));
            }
        }
        if let Some(g) = level.glow.as_mut() {
            let (from_art, to_art) = ((w * from, h * from), (w * to, h * to));
            g.front = crate::look_ops::resample_mask(&g.front, from_art, to_art);
            g.back = crate::look_ops::resample_mask(&g.back, from_art, to_art);
            g.art = to_art;
        }
        let before = Box::new(self.whole());
        let surfaces = self.surface_maps.clone();
        let states = self.states_snap();
        Ok(self
            .change(Edit::Whole { before, after: Box::new(Whole { level, items, masks, surfaces, states }) }))
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
        self.bake_generation += 1;
        self.touch(edit);
        let dirty = match edit {
            Edit::Show { before, after } => {
                if self.show_state(if forward { *after } else { *before }) {
                    self.all()
                } else {
                    NOTHING
                }
            }
            Edit::States { before, after } => {
                let (from, to) = if forward { (before, after) } else { (after, before) };
                self.set_states(to);
                // Issue #80: the canvas poses the shown state's values.
                fn values(s: &StatesSnap) -> Option<&Vec<olr_format::assets::StateValue>> {
                    s.states.get(s.shown).map(|st| &st.values)
                }
                if values(from) != values(to) { self.all() } else { NOTHING }
            }
            Edit::Parts { before, after } => {
                let (from, to) = if forward { (before, after) } else { (after, before) };
                self.set_parts(to);
                // The canvas poses the exposed defaults.
                if from.exposed != to.exposed || from.health != to.health { self.all() } else { NOTHING }
            }
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
                if *index >= self.items.len() {
                    return NOTHING;
                }
                let shown = self.shown_at(*index);
                self.items[*index].element = to.clone();
                // A rename or a lock changes no pixel, nor a folder that
                // leaves it shown as it was (issue #60).
                let same = Element {
                    name: to.name.clone(),
                    locked: to.locked,
                    folder: to.folder.clone(),
                    ..from.clone()
                };
                if same == *to && shown == self.shown_at(*index) {
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
                let id = &item.item.element.id;
                if insert {
                    if let Some((name, bytes)) = &item.original {
                        self.originals.insert(name.clone(), bytes.clone());
                    }
                    if let Some(m) = &item.glow_mask {
                        self.glow_masks.insert(id.clone(), m.clone());
                    }
                    let at = (*index).min(self.items.len());
                    self.items.insert(at, item.item.clone());
                } else if *index < self.items.len() {
                    self.items.remove(*index);
                    self.glow_masks.remove(id);
                }
                self.element_rect(&item.item.element)
            }
            Edit::Move { from, to, before, after } => {
                let (a, b) = if forward { (*from, *to) } else { (*to, *from) };
                if a >= self.items.len() || b >= self.items.len() {
                    return NOTHING;
                }
                let mut item = self.items.remove(a);
                item.element.folder = if forward { after } else { before }.clone();
                let rect = self.element_rect(&item.element);
                self.items.insert(b, item);
                rect
            }
            Edit::Folders { before, after } => {
                let shown: Vec<bool> = (0..self.items.len()).map(|i| self.shown_at(i)).collect();
                self.folders = if forward { after } else { before }.clone();
                // The members a folder's visibility hid or showed rebake.
                let mut dirty = None;
                for (i, was) in shown.into_iter().enumerate() {
                    if was != self.shown_at(i) {
                        dirty = Some(Rect::union(dirty, self.element_rect(&self.items[i].element)));
                    }
                }
                dirty.unwrap_or(NOTHING)
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
                self.glow_masks = w.masks.clone();
                self.surface_maps = w.surfaces.clone();
                if let Some(states) = &w.states {
                    self.set_states(states);
                }
                self.keep_from_level();
                self.all()
            }
            Edit::Look { before, after } => {
                let (from, to) = if forward { (before, after) } else { (after, before) };
                self.level.look = to.as_deref().cloned();
                // The bake numbers groups and materials (issue #43) by their
                // order: others, another bake.
                let ids = |l: &Option<Box<olr_format::look::LookSource>>| {
                    let groups = l.iter().flat_map(|l| l.glow_groups.iter().map(|g| g.id.clone()));
                    let materials = l.iter().flat_map(|l| l.materials.iter().map(|m| format!("/{}", m.id)));
                    groups.chain(materials).collect::<Vec<_>>()
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
            Edit::Haze { rect, before, after, created } => {
                let rows = if forward { after } else { before };
                let (w, h) = self.level.size;
                if forward && *created && self.level.haze.is_none() {
                    self.level.haze = Some(vec![255; w as usize * h as usize]);
                }
                if let Some(map) = self.level.haze.as_mut() {
                    let width = rect.w as usize;
                    for (row, values) in rows.chunks(width.max(1)).enumerate() {
                        let start = (rect.y as usize + row) * w as usize + rect.x as usize;
                        if let Some(dst) = map.get_mut(start..start + width) {
                            dst.copy_from_slice(values);
                        }
                    }
                }
                if !forward && *created {
                    self.level.haze = None;
                }
                *rect
            }
            Edit::SurfaceMap { id, file, before, after } => {
                match if forward { after } else { before } {
                    Some(m) => self.surface_maps.insert(file.clone(), m.clone()),
                    None => self.surface_maps.remove(file),
                };
                match self.index_of(id) {
                    Some(i) => self.element_rect(&self.items[i].element),
                    None => NOTHING,
                }
            }
        };
        if dirty.w > 0 && dirty.h > 0 {
            self.rebake(dirty);
        } else if self.asset.is_some() {
            // A look edit of the same groups bakes nothing; the canvas's
            // look follows it (issue #79).
            self.refresh_canvas_look(None);
        }
        dirty
    }
}
