//! The document's operations. Each checks before changing anything, then
//! builds its `Edit` and carries it out through `Document::apply`, the one
//! path undo and redo take too; a refused operation leaves the document
//! as it was. Painting (`paint`, `erase`) is recorded per stroke instead
//! (`stroke_edit`).

use std::sync::Arc;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_edit::compose::{Decoded, Rect, resize_decoded};
use olr_edit::element::{Property, new_element_id, new_source_name};
use olr_edit::level::{self, Paint, Shape, draw_art, erase_art, rescale_art};
use olr_edit::pixelate::Pixelation;
use olr_format::image_io::{LEVEL_ART, check_size};
use olr_format::levels::{
    Composition, Dither, Element, ElementKind, ElementMaterial, LevelSource, MAX_ART_SCALE, MAX_ELEMENTS,
    check_elements,
};

use crate::catalog::Catalog;
use crate::document::{DocError, Document, Item};
use crate::history::{Edit, LevelMeta, Snapshot, Whole};

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
fn with_property(e: &mut Element, property: Property) -> Result<(), DocError> {
    if e.locked && !matches!(property, Property::Locked(_)) {
        return Err(DocError::Locked(e.id.clone()));
    }
    let wrong = |want| DocError::WrongKind { id: e.id.clone(), want };
    match property {
        Property::Name(n) => e.name = n,
        Property::Visible(v) => e.visible = v,
        Property::Locked(v) => e.locked = v,
        Property::Material(m) => match &mut e.kind {
            ElementKind::Image { material, .. } | ElementKind::Drawn { material } => *material = m,
            ElementKind::Raster => return Err(wrong("an image or drawn element")),
        },
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
    fn find(&self, id: &str) -> Result<usize, DocError> {
        self.index_of(id).ok_or_else(|| DocError::NoElement(id.into()))
    }

    /// `id` (found) when it is not locked.
    fn unlocked(&self, id: &str) -> Result<usize, DocError> {
        let i = self.find(id)?;
        if self.items[i].element.locked {
            return Err(DocError::Locked(id.into()));
        }
        Ok(i)
    }

    /// `check_elements` over the composition with `edit` applied to a copy.
    fn check_with(&self, edit: impl FnOnce(&mut Vec<Element>)) -> Result<(), DocError> {
        let mut elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        edit(&mut elements);
        self.check(&elements, self.level.size, self.level.art_scale)
    }

    fn check(&self, elements: &[Element], size: (u32, u32), art_scale: u32) -> Result<(), DocError> {
        if elements.len() > MAX_ELEMENTS {
            return Err(DocError::Bad(format!("a level holds at most {MAX_ELEMENTS} elements")));
        }
        check_elements(&Composition { elements: elements.to_vec() }, size, art_scale)
            .map_err(|e| DocError::Bad(e.to_string()))
    }

    /// Carries out `edit` as a change.
    fn change(&mut self, edit: Edit) -> Change {
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
        let source = new_source_name(self.originals.keys().map(String::as_str), file_name);
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
            id: new_element_id(&elements, stem, "image"),
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
            },
        };
        self.check_with(|es| es.push(element.clone()))?;
        let bytes = Arc::new(bytes);
        let decoded = self.image_grid(&element, &bytes)?;
        let item = Item { element, decoded };
        let index = self.items.len();
        Ok(self.change(Edit::Insert {
            index,
            item: Box::new(Snapshot { item, original: Some((source, bytes)) }),
        }))
    }

    /// An image element's decoded data through the cache (`None` grid when hidden).
    fn image_grid(&mut self, e: &Element, original: &[u8]) -> Result<Decoded, DocError> {
        let ElementKind::Image { source, size, pixel_size, colours, dither, alpha_threshold, .. } = &e.kind
        else {
            return Err(DocError::WrongKind { id: e.id.clone(), want: "an image element" });
        };
        if !e.visible {
            self.cache.original(source, original)?;
            return Ok(Decoded::Image { grid: None });
        }
        let s = self.level.art_scale;
        let p = Pixelation {
            pixel_size: *pixel_size,
            colours: *colours,
            dither: *dither,
            alpha_threshold: *alpha_threshold,
        };
        let art = (size.0.saturating_mul(s), size.1.saturating_mul(s));
        Ok(Decoded::Image { grid: Some(self.cache.grid(source, original, art, &p)?) })
    }

    /// An empty drawn layer on top.
    pub fn add_layer(&mut self, material: ElementMaterial, name: &str) -> Result<Change, DocError> {
        let elements: Vec<Element> = self.items.iter().map(|i| i.element.clone()).collect();
        let element = Element {
            id: new_element_id(&elements, name, "layer"),
            name: name.into(),
            visible: true,
            locked: false,
            kind: ElementKind::Drawn { material },
        };
        self.check_with(|es| es.push(element.clone()))?;
        let (w, h) = self.level.size;
        let s = self.level.art_scale;
        let item = Item { element, decoded: Decoded::Drawn { art: RgbaImage::new(w * s, h * s) } };
        let index = self.items.len();
        Ok(self.change(Edit::Insert { index, item: Box::new(Snapshot { item, original: None }) }))
    }

    /// A property (ruling R4: refused on a locked element unless it is
    /// `Locked`), checked with `check_elements`; an image reprocesses
    /// through the cache only when its pixelation or box size changes.
    pub fn set(&mut self, id: &str, property: Property) -> Result<Change, DocError> {
        let index = self.find(id)?;
        let before = self.items[index].element.clone();
        let mut after = before.clone();
        with_property(&mut after, property)?;
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

    /// Moves the element to `index` (bottom 0, clamped to the top); allowed
    /// when locked (ruling R4).
    pub fn move_to(&mut self, id: &str, index: usize) -> Result<Change, DocError> {
        let from = self.find(id)?;
        let to = index.min(self.items.len() - 1);
        Ok(self.change(Edit::Move { from, to }))
    }

    /// Drawn layer `id` (unlocked), by index.
    fn paintable(&self, id: &str) -> Result<usize, DocError> {
        let i = self.unlocked(id)?;
        if !matches!(self.items[i].decoded, Decoded::Drawn { .. }) {
            return Err(DocError::WrongKind { id: id.into(), want: "a drawn element" });
        }
        Ok(i)
    }

    /// Paints into drawn layer `id` (unlocked) without recording: strokes
    /// do (`stroke_edit`). Returns the world rectangle rebaked.
    pub fn paint(&mut self, id: &str, shape: &Shape, paint: &Paint) -> Result<Option<Rect>, DocError> {
        let i = self.paintable(id)?;
        let (size, s) = (self.level.size, self.level.art_scale);
        let Decoded::Drawn { art } = &mut self.items[i].decoded else { return Ok(None) };
        let rect = draw_art(art, s, size, shape, paint);
        if let Some(r) = rect {
            self.rebake(r);
        }
        Ok(rect)
    }

    pub fn erase(&mut self, id: &str, shape: &Shape) -> Result<Option<Rect>, DocError> {
        let i = self.paintable(id)?;
        let (size, s) = (self.level.size, self.level.art_scale);
        let Decoded::Drawn { art } = &mut self.items[i].decoded else { return Ok(None) };
        let rect = erase_art(art, s, size, shape);
        if let Some(r) = rect {
            self.rebake(r);
        }
        Ok(rect)
    }

    /// Drawn layer `id`'s art (for a stroke's "before" copy and the eyedropper).
    pub fn layer_art(&self, id: &str) -> Option<&RgbaImage> {
        match &self.items.get(self.index_of(id)?)?.decoded {
            Decoded::Drawn { art } => Some(art),
            _ => None,
        }
    }

    /// The `Pixels` edit of a finished stroke: `before` is the layer as the
    /// stroke began, `rect` (clipped to the level) what it changed.
    pub fn stroke_edit(&self, id: &str, rect: Rect, before: &RgbaImage) -> Edit {
        let rect = self.clip(rect);
        let s = self.level.art_scale;
        let crop = |img: &RgbaImage| {
            let fits = (img.width(), img.height()) == (self.level.size.0 * s, self.level.size.1 * s);
            if fits { img.crop(rect.x * s, rect.y * s, rect.w * s, rect.h * s) } else { RgbaImage::new(0, 0) }
        };
        let after = self.layer_art(id).map(crop).unwrap_or_else(|| RgbaImage::new(0, 0));
        Edit::Pixels { id: id.into(), rect, before: crop(before), after }
    }

    /// `rect` clipped to the level.
    pub(crate) fn clip(&self, r: Rect) -> Rect {
        let (w, h) = self.level.size;
        let (x, y) = (r.x.min(w), r.y.min(h));
        Rect { x, y, w: r.x.saturating_add(r.w).min(w) - x, h: r.y.saturating_add(r.h).min(h) - y }
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
            if let ElementKind::Image { at, .. } = &mut e.kind {
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
                Decoded::Drawn { art } => Decoded::Drawn { art: rescale_art(art, from, to) },
                Decoded::Raster { material, front, back } => Decoded::Raster {
                    material: material.clone(),
                    front: rescale_art(front, from, to),
                    back: rescale_art(back, from, to),
                },
                Decoded::Image { .. } => match self.original_of(&i.element) {
                    Some(bytes) => match self.image_grid(&i.element, &bytes) {
                        Ok(d) => d,
                        Err(e) => {
                            result = Err(e);
                            break;
                        }
                    },
                    None => Decoded::Image { grid: None },
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
        let decoded = match self.original_of(&e) {
            Some(bytes) => self.image_grid(&e, &bytes).unwrap_or(Decoded::Image { grid: None }),
            None => Decoded::Image { grid: None },
        };
        self.items[index].decoded = decoded;
    }

    /// The world rectangle element `e` can cover: an image's box (clipped),
    /// the whole level otherwise.
    pub(crate) fn element_rect(&self, e: &Element) -> Rect {
        match &e.kind {
            ElementKind::Image { at, size, .. } => {
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
                let img = if forward { after } else { before };
                let s = self.level.art_scale;
                let Some(i) = self.index_of(id) else { return NOTHING };
                let Decoded::Drawn { art } = &mut self.items[i].decoded else { return NOTHING };
                let (ox, oy) = (rect.x * s, rect.y * s);
                if ox + img.width() > art.width() || oy + img.height() > art.height() {
                    return NOTHING;
                }
                for y in 0..img.height() {
                    for x in 0..img.width() {
                        art.set(ox + x, oy + y, img.get(x, y));
                    }
                }
                *rect
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
        };
        if dirty.w > 0 && dirty.h > 0 {
            self.rebake(dirty);
        }
        dirty
    }
}
