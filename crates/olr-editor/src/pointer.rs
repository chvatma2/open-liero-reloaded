//! The canvas's pointer, per tool (plan 14b Task 7). Pointer positions are
//! untrusted world pixels: every shape is clipped by the document, every
//! sum saturates.

use std::sync::Arc;

use olr_data::image::Rgba;
use olr_edit::compose::{Decoded, Rect};
use olr_edit::element::Property;
use olr_edit::level::{MAX_STAMP_SCALE, Paint, Shape, StampMask};
use olr_format::levels::{Element, ElementKind, LevelObject, MAX_SIDE, drawn_box};

use crate::document::Document;
use crate::history::Edit;
use crate::ops::{NOTHING, Stroke, StrokeBefore};
use crate::state::{EditorCommand, EditorState, Selection};
use crate::tools::{BrushShape, Overlay, PaintChoice, PointerKind, Tool};

/// A spawn is grabbed within this many world pixels.
const SPAWN_REACH: i64 = 3;

/// A handle is grabbed within this many screen points.
const HANDLE_REACH: f32 = 5.0;

/// The pointer drag in progress.
#[derive(Clone, Debug)]
pub(crate) enum Drag {
    /// An image's box follows the pointer, live.
    Move { before: Element, from: (i32, i32) },
    /// A group's members follow the pointer together, live (plan 14e R8).
    MoveMany { befores: Vec<Element>, from: (i32, i32) },
    /// A handle drags an outline; the box changes on release (ruling R8).
    Resize { id: String, handle: (u8, u8), at: (i32, i32), size: (u32, u32), now: ((i32, i32), (u32, u32)) },
    /// A spawn follows the pointer, live.
    Spawn { index: usize, before: Vec<LevelObject>, grab: (i64, i64) },
    /// Brush or eraser dabs; `before` is the layer (or mask) as the stroke
    /// began, after `begun`, the conversion it needed (plan 14d).
    Stroke {
        id: String,
        /// Shared: the drag is cloned on every move (review of the minors).
        before: Arc<StrokeBefore>,
        last: (i32, i32),
        rect: Option<Rect>,
        stroke: Stroke,
        begun: Option<Box<Edit>>,
        /// The paint, resolved once as the stroke began (plan 14e minors).
        paint: Arc<Paint>,
    },
    /// A rectangle, ellipse or line from `start`, painted on release.
    Shape { id: String, tool: Tool, start: (i32, i32), now: (i32, i32) },
    /// The pixel rectangle being dragged on drawn layer `id` (plan 14e R7).
    Pixels { id: String, start: (i32, i32), now: (i32, i32) },
    /// Glow brush dabs on element `id`'s mask (plan 15a); `before` is the
    /// mask and its flag as the stroke began.
    Glow { id: String, before: crate::look_ops::GlowStrokeStart, last: (i32, i32) },
    /// Surface brush dabs of map value `value` on element `id`'s map
    /// (issue #43); `before` is the map and the element as it began.
    Surface { id: String, before: Box<crate::surface_ops::SurfaceStrokeStart>, last: (i32, i32), value: u8 },
}

fn add(a: i32, b: i64) -> i32 {
    (i64::from(a) + b).clamp(i32::MIN.into(), i32::MAX.into()) as i32
}

fn sub(a: (i32, i32), b: (i32, i32)) -> (i64, i64) {
    (i64::from(a.0) - i64::from(b.0), i64::from(a.1) - i64::from(b.1))
}

/// An image element's box, when `e` is one.
fn image_box(e: &Element) -> Option<((i32, i32), (u32, u32))> {
    match &e.kind {
        ElementKind::Image { at, size, .. } => Some((*at, *size)),
        _ => None,
    }
}

/// An image's or a drawn layer's box (a level of `level`): what moves.
fn element_box(e: &Element, level: (u32, u32)) -> Option<((i32, i32), (u32, u32))> {
    image_box(e).or_else(|| drawn_box(&e.kind, level))
}

/// Whether drawn item `item` covers world pixel `p` (its art at the
/// pixel's block centre is opaque; plan 14d R9).
fn covers(item: &crate::document::Item, level: (u32, u32), s: u32, p: (i32, i32)) -> bool {
    let (Some((at, size)), Decoded::Drawn { art, .. }) =
        (drawn_box(&item.element.kind, level), &item.decoded)
    else {
        return false;
    };
    let (lx, ly) = sub(p, at);
    let inside = (0..i64::from(size.0)).contains(&lx) && (0..i64::from(size.1)).contains(&ly);
    let (ax, ay) = ((lx as u32).saturating_mul(s) + s / 2, (ly as u32).saturating_mul(s) + s / 2);
    inside && ax < art.width() && ay < art.height() && art.get(ax, ay).a != 0
}

/// World rectangle `r` moved by `by`, clipped to `level`.
fn shift_rect(r: Rect, by: (i32, i32), level: (u32, u32)) -> Rect {
    if by == (0, 0) {
        return r;
    }
    let (x, y) = (i64::from(r.x) + i64::from(by.0), i64::from(r.y) + i64::from(by.1));
    clipped(x, y, x + i64::from(r.w), y + i64::from(r.h), level)
}

/// `p` moved by `by`, saturating.
fn shifted(p: (i32, i32), by: (i32, i32)) -> (i32, i32) {
    (p.0.saturating_add(by.0), p.1.saturating_add(by.1))
}

/// `p` moved back by `by`, saturating.
fn unshift(p: (i32, i32), by: (i32, i32)) -> (i32, i32) {
    (p.0.saturating_sub(by.0), p.1.saturating_sub(by.1))
}

fn contains((at, size): ((i32, i32), (u32, u32)), p: (i32, i32)) -> bool {
    let (dx, dy) = sub(p, at);
    (0..i64::from(size.0)).contains(&dx) && (0..i64::from(size.1)).contains(&dy)
}

/// The 8 handles of a box: (column, row) in 0..=2 each, the centre left out.
fn handles((at, size): ((i32, i32), (u32, u32))) -> impl Iterator<Item = ((u8, u8), (i32, i32))> {
    (0..3u8).flat_map(move |row| {
        (0..3u8).filter(move |&col| (col, row) != (1, 1)).map(move |col| {
            let along = |o: i32, side: u32, k: u8| add(o, i64::from(side) * i64::from(k) / 2);
            ((col, row), (along(at.0, size.0, col), along(at.1, size.1, row)))
        })
    })
}

/// `rect` of i64 corners clipped to `size` (empty when outside).
fn clipped(x0: i64, y0: i64, x1: i64, y1: i64, size: (u32, u32)) -> Rect {
    let c = |v: i64, side: u32| v.clamp(0, i64::from(side)) as u32;
    let (x0, y0, x1, y1) = (c(x0, size.0), c(y0, size.1), c(x1, size.0), c(y1, size.1));
    Rect { x: x0, y: y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
}

/// A box (`at`, `size`) as a world rectangle clipped to `level`.
fn box_rect((at, size): ((i32, i32), (u32, u32)), level: (u32, u32)) -> Rect {
    let (x, y) = (i64::from(at.0), i64::from(at.1));
    clipped(x, y, x + i64::from(size.0), y + i64::from(size.1), level)
}

/// The box a handle drag gives: the handle's sides follow `delta`; with
/// `keep` (Shift) the aspect ratio stays, the opposite side anchored.
fn resized(
    (at, size): ((i32, i32), (u32, u32)),
    (col, row): (u8, u8),
    delta: (i64, i64),
    keep: bool,
) -> ((i32, i32), (u32, u32)) {
    let (mut x0, mut y0) = (i64::from(at.0), i64::from(at.1));
    let (mut x1, mut y1) = (x0 + i64::from(size.0), y0 + i64::from(size.1));
    let max = i64::from(MAX_SIDE);
    match col {
        0 => x0 = (x0 + delta.0).clamp(x1 - max, x1 - 1),
        2 => x1 = (x1 + delta.0).clamp(x0 + 1, x0 + max),
        _ => {}
    }
    match row {
        0 => y0 = (y0 + delta.1).clamp(y1 - max, y1 - 1),
        2 => y1 = (y1 + delta.1).clamp(y0 + 1, y0 + max),
        _ => {}
    }
    let (mut w, mut h) = (x1 - x0, y1 - y0);
    if keep {
        let (ow, oh) = (i64::from(size.0).max(1), i64::from(size.1).max(1));
        let round = |a: i64, b: i64, c: i64| ((a * b + c / 2) / c).clamp(1, max);
        // The side the pointer moved more (relative to the box) leads.
        let width_leads = match (col, row) {
            (1, _) => false,
            (_, 1) => true,
            _ => w * oh >= h * ow,
        };
        if width_leads {
            h = round(w, oh, ow)
        } else {
            w = round(h, ow, oh)
        }
        if col == 0 {
            x0 = x1 - w;
        }
        if row == 0 {
            y0 = y1 - h;
        }
    }
    let at = (
        x0.clamp(i32::MIN.into(), i32::MAX.into()) as i32,
        y0.clamp(i32::MIN.into(), i32::MAX.into()) as i32,
    );
    (at, (w as u32, h as u32))
}

/// The pixel rectangle from `start` to `now` (both included) on layer
/// `id`: within its box and the level (empty when outside).
fn pixel_span(doc: &Document, id: &str, start: (i32, i32), now: (i32, i32)) -> Rect {
    let (x, y, w, h) = span(start, now);
    let level = doc.level().size;
    let layer = doc.index_of(id).and_then(|i| element_box(&doc.items()[i].element, level));
    let r = box_rect(((x, y), (w as u32, h as u32)), level);
    layer.map_or(NOTHING, |b| intersect(r, box_rect(b, level)))
}

/// The rectangle both `a` and `b` hold (empty when none).
fn intersect(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let x1 = a.x.saturating_add(a.w).min(b.x.saturating_add(b.w));
    let y1 = a.y.saturating_add(a.h).min(b.y.saturating_add(b.h));
    Rect { x: x0, y: y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
}

/// The world box from `a` to `b`, both included.
fn span(a: (i32, i32), b: (i32, i32)) -> (i32, i32, i32, i32) {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    let side = |lo: i32, hi: i32| (i64::from(hi) - i64::from(lo) + 1).min(i64::from(i32::MAX)) as i32;
    (x0, y0, side(x0, x1), side(y0, y1))
}

impl EditorState {
    /// The paint `self.paint` lays in `doc`; a texture no longer there
    /// paints the default colour (plan 14e R3).
    fn paint_of(&self, doc: &Document) -> Paint {
        match &self.paint {
            PaintChoice::Colour(c) => Paint::Colour(*c),
            PaintChoice::FrontTile => Paint::Tile(doc.front_tile().clone()),
            PaintChoice::BackTile => Paint::Tile(doc.back_tile().clone()),
            p @ PaintChoice::Texture(_) => match self.texture(p) {
                Some(Some(img)) => Paint::Tile(img.clone()),
                _ => match PaintChoice::default() {
                    PaintChoice::Colour(c) => Paint::Colour(c),
                    _ => Paint::Colour(Rgba::BLACK),
                },
            },
        }
    }

    /// The selected drawn layer a painting tool paints into (or, with
    /// `picture`, the selected image the eraser masks), or why not.
    fn paint_target(&mut self, picture: bool) -> Option<String> {
        let doc = self.doc.as_ref()?;
        let id = match &self.selection {
            Selection::Element(id) => id.clone(),
            _ => String::new(),
        };
        let found = doc.index_of(&id);
        let item = found.map(|i| &doc.items()[i]);
        let why = match item {
            // Locked itself or by a folder (issue #60).
            Some(_) if found.is_some_and(|i| doc.locked_at(i)) => format!("element {id:?} is locked"),
            Some(item) if matches!(item.decoded, Decoded::Drawn { .. }) => return Some(id),
            Some(item) if matches!(item.decoded, Decoded::Image { .. }) && picture => return Some(id),
            Some(item) if matches!(item.decoded, Decoded::Image { .. }) => {
                "erase on a picture; merge it into a layer to paint it".into()
            }
            _ => self.one_element("select a drawn layer to paint"),
        };
        self.status.message = Some(why);
        None
    }

    /// What a stroke into `id` lays: the eraser's erase or restore (a
    /// picture's), or paint of "Paint as" (else the layer's material).
    fn stroke_of(&self, id: &str, erase: bool) -> Stroke {
        let item = self.doc.as_ref().and_then(|d| d.index_of(id).map(|i| &d.items()[i]));
        match item.map(|i| &i.element.kind) {
            Some(ElementKind::Image { .. }) if erase && self.restore => Stroke::Restore,
            _ if erase => Stroke::Erase,
            Some(ElementKind::Drawn { material, .. }) => {
                Stroke::Paint(self.paint_material.unwrap_or(*material).into())
            }
            _ => Stroke::Paint(olr_data::level::Material::Dirt),
        }
    }

    /// Begins a stroke of `stroke` into `id` (`Document::begin_stroke`):
    /// its conversion and the copy it starts from, or `None` (the status
    /// says why).
    fn begin(&mut self, id: &str, stroke: Stroke) -> Option<(Option<Edit>, StrokeBefore)> {
        let doc = self.doc.as_mut()?;
        let begun = match doc.begin_stroke(id, stroke) {
            Ok(e) => e,
            Err(e) => {
                self.status.message = Some(e.to_string());
                return None;
            }
        };
        let Some(before) = doc.stroke_before(id) else {
            if let Some(e) = &begun {
                doc.apply(e, false);
            }
            return None;
        };
        Some((begun, before))
    }

    /// The crater brush's hole and scale (plan 14e R4): `None` when the
    /// brush is no crater or the theme has no such hole.
    fn crater(&self) -> Option<(Arc<StampMask>, u32)> {
        let BrushShape::Crater(i) = self.brush.shape else { return None };
        let mask = self.holes().get(usize::from(i))?.clone();
        let side = mask.size.0.max(mask.size.1).max(1);
        Some((mask, (self.brush.size / side).clamp(1, MAX_STAMP_SCALE)))
    }

    /// A brush dab at `p`; a crater whose hole the theme lacks dabs round.
    fn dab(&self, p: (i32, i32)) -> Shape {
        let size = self.brush.size;
        if let Some((mask, scale)) = self.crater() {
            return Shape::Stamp { x: p.0, y: p.1, scale, mask };
        }
        match self.brush.shape {
            BrushShape::Round | BrushShape::Crater(_) => {
                Shape::Circle { cx: p.0, cy: p.1, r: (size / 2) as i32 }
            }
            BrushShape::Square => {
                let half = i64::from(size / 2);
                Shape::Rect { x: add(p.0, -half), y: add(p.1, -half), w: size as i32, h: size as i32 }
            }
        }
    }

    /// The shapes joining a stroke's dab at `from` to the next at `to`: a
    /// line as wide as the brush, or for a crater brush (R4) its dabs every
    /// half crater along the segment, `to`'s included.
    fn joins(&self, from: (i32, i32), to: (i32, i32)) -> Vec<Shape> {
        let Some((mask, scale)) = self.crater() else {
            return vec![Shape::Line { from, to, width: self.brush.size }, self.dab(to)];
        };
        let half = (i64::from(mask.size.0.max(mask.size.1)) * i64::from(scale) / 2).max(1);
        let (dx, dy) = sub(to, from);
        // The pointer's moves are short; a far jump is bounded all the same.
        // Rounded up: dabs at most half a crater apart (plan 14e minors).
        let steps = ((dx.abs().max(dy.abs()) + half - 1) / half).clamp(1, 1024);
        (1..=steps)
            .map(|k| Shape::Stamp {
                x: add(from.0, dx * k / steps),
                y: add(from.1, dy * k / steps),
                scale,
                mask: mask.clone(),
            })
            .collect()
    }

    /// Paints (or erases, or restores) `shapes` into `id`; the rectangle
    /// rebaked.
    fn lay(&mut self, id: &str, shapes: &[Shape], stroke: Stroke, paint: &Paint) -> Option<Rect> {
        let doc = self.doc.as_mut()?;
        let mut rect = None;
        for shape in shapes {
            let r = match stroke {
                Stroke::Paint(m) => doc.paint(id, shape, paint, Some(m)),
                Stroke::Erase => doc.erase(id, shape, false),
                Stroke::Restore => doc.erase(id, shape, true),
            };
            match r {
                Ok(Some(r)) => rect = Some(Rect::union(rect, r)),
                Ok(None) => {}
                Err(e) => {
                    self.status.message = Some(e.to_string());
                    return rect;
                }
            }
        }
        rect
    }

    /// The canvas's camera for distant layers (issue #11): the world point
    /// at its centre; `None` before the canvas or a document.
    pub fn parallax_camera(&self) -> Option<(f32, f32)> {
        let doc = self.doc.as_ref()?;
        Some(self.view.centre(self.canvas?, doc.level().art_scale))
    }

    /// Where distant element `e` shows on the canvas, relative to its
    /// place, in whole world pixels: `depth × (camera − level centre)`, as
    /// the renderer draws it (to within its texel rounding); (0, 0) for
    /// any other element.
    pub(crate) fn parallax(&self, e: &Element) -> (i32, i32) {
        let (Some(depth), Some(camera), Some(doc)) = (e.distant, self.parallax_camera(), &self.doc) else {
            return (0, 0);
        };
        let layer =
            olr_data::level::DistantLayer { image: olr_data::image::RgbaImage::new(0, 0), at: (0, 0), depth };
        let (x, y) = layer.position(camera, doc.level().size);
        let whole = |v: f32| v.round().clamp(i32::MIN as f32, i32::MAX as f32) as i32;
        (whole(x), whole(y))
    }

    /// World pixel `world` on the selected element, where a distant one
    /// shows shifted (issue #11).
    fn to_layer(&self, world: (i32, i32)) -> (i32, i32) {
        let (Selection::Element(id), Some(doc)) = (&self.selection, &self.doc) else { return world };
        match doc.index_of(id) {
            Some(i) => unshift(world, self.parallax(&doc.items()[i].element)),
            None => world,
        }
    }

    pub(crate) fn pointer(
        &mut self,
        kind: PointerKind,
        world: (i32, i32),
        shift: bool,
        toggle: bool,
    ) -> Vec<EditorCommand> {
        self.pointer_at(kind, world, shift, toggle, false)
    }

    /// An Alt+press (issue #61): with Select, the element under the pointer
    /// itself, at any depth; another tool takes it as a press.
    pub(crate) fn alt_down(&mut self, world: (i32, i32)) -> Vec<EditorCommand> {
        self.pointer_at(PointerKind::Down, world, false, false, true)
    }

    /// A pointer event; `deep` (Alt) presses select the element itself.
    fn pointer_at(
        &mut self,
        kind: PointerKind,
        world: (i32, i32),
        shift: bool,
        toggle: bool,
        deep: bool,
    ) -> Vec<EditorCommand> {
        if self.dialog.is_some() {
            // A drag still ends on its Up.
            return if kind == PointerKind::Up { self.release() } else { Vec::new() };
        }
        let Some(doc) = &self.doc else { return Vec::new() };
        let l = doc.level();
        self.status.cursor = Some(world);
        let inside = (0..i64::from(l.size.0)).contains(&i64::from(world.0))
            && (0..i64::from(l.size.1)).contains(&i64::from(world.1));
        self.status.material = inside
            .then(|| l.material.get(world.1 as usize * l.size.0 as usize + world.0 as usize).copied())
            .flatten();
        // A tool working on the selected distant layer works where it shows
        // (issue #11); Select shifts each element its own way (`hit`).
        let world = match self.tool {
            Tool::Select | Tool::Spawn | Tool::Eyedropper => world,
            _ => self.to_layer(world),
        };
        match kind {
            PointerKind::Down => self.down(world, toggle, deep),
            PointerKind::Move => self.drag_to(world, shift),
            PointerKind::Up => {
                let cmds = self.drag_to(world, shift);
                cmds.into_iter().chain(self.release()).collect()
            }
        }
    }

    fn down(&mut self, world: (i32, i32), toggle: bool, deep: bool) -> Vec<EditorCommand> {
        // A drag whose Up never came ends here.
        let mut cmds = self.release();
        cmds.extend(self.press(world, toggle, deep));
        cmds
    }

    /// A double click (issue #61): with Select and a folder selected on the
    /// way to the element under the pointer, its child holding that element
    /// is selected and the folder becomes the focus.
    pub(crate) fn double_click(&mut self, world: (i32, i32)) -> Vec<EditorCommand> {
        if self.tool != Tool::Select || self.dialog.is_some() {
            return Vec::new();
        }
        let (Some(hit), Selection::Folder(f)) = (self.hit(world), self.selection.clone()) else {
            return Vec::new();
        };
        let Some(doc) = &self.doc else { return Vec::new() };
        let path = path_to(doc, &hit);
        if let Some(k) = path.iter().position(|p| *p == f)
            && let Some(child) = path.get(k + 1)
        {
            self.selection = if doc.is_folder(child) {
                Selection::Folder(child.clone())
            } else {
                Selection::Element(child.clone())
            };
            self.focus = Some(f);
        }
        Vec::new()
    }

    /// What a plain click on element `hit` selects (issue #61): inside the
    /// focus, the focus's child holding it; else the outermost folder
    /// holding it, or the element when none does. A click outside the
    /// focus leaves it.
    fn click_pick(&mut self, hit: &str) -> String {
        let Some(doc) = &self.doc else { return hit.to_string() };
        let path = path_to(doc, hit);
        let inside = self.focus.as_ref().and_then(|f| path.iter().position(|p| p == f));
        if inside.is_none() {
            self.focus = None;
        }
        match inside {
            Some(k) => path.get(k + 1),
            None => path.first(),
        }
        .cloned()
        .unwrap_or_else(|| hit.to_string())
    }

    fn press(&mut self, world: (i32, i32), toggle: bool, deep: bool) -> Vec<EditorCommand> {
        match self.tool {
            Tool::Select if deep => self.select_down(world, true),
            Tool::Select if toggle => {
                // Ctrl+click: what a plain click would pick in or out of the
                // selection (plan 14e R8; issue #61).
                if let Some(hit) = self.hit(world) {
                    let focus = self.focus.clone();
                    let id = self.click_pick(&hit);
                    self.focus = focus;
                    let mut ids = self.selection.ids();
                    match ids.iter().position(|e| *e == id) {
                        Some(i) => {
                            ids.remove(i);
                        }
                        None => ids.push(id),
                    }
                    self.selection = self.group(ids);
                }
                Vec::new()
            }
            Tool::Select => self.select_down(world, false),
            Tool::Brush | Tool::Eraser => {
                let erase = self.tool == Tool::Eraser;
                let Some(id) = self.paint_target(erase) else { return Vec::new() };
                let stroke = self.stroke_of(&id, erase);
                let Some((begun, before)) = self.begin(&id, stroke) else { return Vec::new() };
                let Some(paint) = self.doc.as_ref().map(|d| Arc::new(self.paint_of(d))) else {
                    return Vec::new();
                };
                let rect = self.lay(&id, &[self.dab(world)], stroke, &paint);
                let before = Arc::new(before);
                self.drag = Some(Drag::Stroke {
                    id,
                    before,
                    last: world,
                    rect,
                    stroke,
                    begun: begun.map(Box::new),
                    paint,
                });
                rect.map(|r| vec![EditorCommand::Upload(r)]).unwrap_or_default()
            }
            Tool::Wand => {
                if !matches!(self.selection, Selection::Element(_)) {
                    self.status.message = Some(self.one_element("select a picture to remove a colour from"));
                    return Vec::new();
                }
                let Some(id) = self.paint_target(true) else { return Vec::new() };
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let Some(e) = doc.index_of(&id).map(|i| doc.items()[i].element.clone()) else {
                    return Vec::new();
                };
                if !matches!(e.kind, ElementKind::Image { .. }) {
                    self.status.message = Some("select a picture to remove a colour from".into());
                    return Vec::new();
                }
                match doc.remove_colour(&id, world, self.tolerance) {
                    // Nothing removed: no step, no unsaved mark.
                    Ok(edits) if edits.is_empty() => Vec::new(),
                    Ok(edits) => {
                        let rect = doc.element_rect(&e);
                        self.history.push(edits);
                        self.committed(rect)
                    }
                    Err(e) => {
                        self.status.message = Some(e.to_string());
                        Vec::new()
                    }
                }
            }
            Tool::Fill => {
                let Some(id) = self.paint_target(false) else { return Vec::new() };
                let Some(paint) = self.doc.as_ref().map(|d| self.paint_of(d)) else { return Vec::new() };
                let material = self.paint_material.map(Into::into);
                let tolerance = self.fill_tolerance;
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                match doc.fill(&id, world, &paint, material, tolerance) {
                    // Nothing filled: no step, no unsaved mark.
                    Ok(edits) if edits.is_empty() => Vec::new(),
                    Ok(edits) => {
                        let rect =
                            doc.index_of(&id).map_or(NOTHING, |i| doc.element_rect(&doc.items()[i].element));
                        self.history.push(edits);
                        self.committed(rect)
                    }
                    Err(e) => {
                        self.status.message = Some(e.to_string());
                        Vec::new()
                    }
                }
            }
            Tool::SelectPixels if self.select_by_colour => {
                // The connected pixels alike in colour (the user's request,
                // 2026-10-19); Ctrl adds to the selection.
                let Selection::Element(id) = self.selection.clone() else {
                    self.status.message = Some(self.one_element("select an element to pick its pixels"));
                    return Vec::new();
                };
                let Some(doc) = &self.doc else { return Vec::new() };
                let Some((rect, region)) = doc.colour_pixels(&id, world, self.tolerance) else {
                    self.status.message = Some(format!("no pixels of {id:?} there"));
                    return Vec::new();
                };
                let s = doc.level().art_scale;
                let (rect, region) = match (&self.pixel_rect, &self.pixel_region) {
                    (Some((old, r)), Some(g)) if toggle && *old == id => {
                        join_regions((*r, g), (rect, &region), s)
                    }
                    _ => (rect, region),
                };
                self.pixel_rect = Some((id, rect));
                self.pixel_region = Some(Arc::new(region));
                self.pixel_world = None;
                Vec::new()
            }
            Tool::SelectPixels => {
                self.pixel_rect = None;
                self.pixel_region = None;
                if let Some(id) = self.paint_target(false) {
                    self.drag = Some(Drag::Pixels { id, start: world, now: world });
                }
                Vec::new()
            }
            Tool::Rect | Tool::Ellipse | Tool::Line => {
                if let Some(id) = self.paint_target(false) {
                    self.drag = Some(Drag::Shape { id, tool: self.tool, start: world, now: world });
                }
                Vec::new()
            }
            Tool::Glow => {
                let Selection::Element(id) = self.selection.clone() else {
                    self.status.message = Some(self.one_element("select an element to paint its glow"));
                    return Vec::new();
                };
                // By colour: one click, one step (the user's request, 2026-10-19).
                if self.glow_by_colour {
                    let (strength, tolerance) = (self.glow_strength, self.tolerance);
                    return self.edits(|d| d.glow_by_colour(&id, world, strength, tolerance));
                }
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let before = match doc.glow_stroke_before(&id) {
                    Ok(b) => b,
                    Err(e) => {
                        self.status.message = Some(e.to_string());
                        return Vec::new();
                    }
                };
                let shape = self.dab(world);
                let strength = self.glow_strength;
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let rect = doc.paint_glow(&id, &shape, strength).unwrap_or(NOTHING);
                self.drag = Some(Drag::Glow { id, before, last: world });
                vec![EditorCommand::Upload(rect)]
            }
            Tool::Surface => {
                let Selection::Element(id) = self.selection.clone() else {
                    self.status.message = Some(self.one_element("select an element to paint its surface"));
                    return Vec::new();
                };
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let back = self.surface_back
                    && doc.index_of(&id).is_some_and(|i| doc.items()[i].element.has_back_art());
                let value = match &self.surface_paint {
                    Some(m) => match doc.material_value(m) {
                        Some(v) => v,
                        None => {
                            self.status.message =
                                Some(format!("the level has no material {m:?}: pick another"));
                            return Vec::new();
                        }
                    },
                    None => 0,
                };
                let before = match doc.surface_stroke_before(&id, back) {
                    Ok(b) => b,
                    Err(e) => {
                        self.status.message = Some(e.to_string());
                        return Vec::new();
                    }
                };
                let shape = self.dab(world);
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let rect = doc.paint_surface(&id, back, &shape, value).unwrap_or(NOTHING);
                self.drag = Some(Drag::Surface { id, before: Box::new(before), last: world, value });
                vec![EditorCommand::Upload(rect)]
            }
            Tool::Spawn => {
                let cmds = self.edit(|d| d.add_spawn(world));
                if !cmds.is_empty()
                    && let Some(d) = &self.doc
                {
                    self.selection = Selection::Object(d.level().objects.len() - 1);
                }
                cmds
            }
            Tool::Eyedropper => {
                let Some(doc) = &self.doc else { return Vec::new() };
                let l = doc.level();
                let s = l.art_scale;
                let inside = (0..i64::from(l.size.0)).contains(&i64::from(world.0))
                    && (0..i64::from(l.size.1)).contains(&i64::from(world.1));
                if inside {
                    let c = l.front.get(world.0 as u32 * s, world.1 as u32 * s);
                    self.paint = PaintChoice::Colour(Rgba { a: 255, ..c });
                    self.tool = self.prev_tool;
                }
                Vec::new()
            }
        }
    }

    /// Select's press: a handle, a spawn, else what the click picks (issue
    /// #61: a selected item under the pointer stays selected and drags;
    /// else `click_pick`'s, or with `deep` (Alt) the element itself).
    fn select_down(&mut self, world: (i32, i32), deep: bool) -> Vec<EditorCommand> {
        let Some(doc) = &self.doc else { return Vec::new() };
        // A handle of the selected unlocked image.
        if let Selection::Element(id) = &self.selection
            && let Some(i) = doc.index_of(id)
            && let e = &doc.items()[i].element
            && !doc.locked_at(i)
            && doc.shown_at(i)
            && let Some(b) = image_box(e)
        {
            let reach = HANDLE_REACH / (self.view.scale() * doc.level().art_scale as f32);
            let reach = i64::from(reach.ceil().max(1.0) as i32);
            let local = unshift(world, self.parallax(e));
            let hit = handles(b).find(|(_, at)| {
                let (dx, dy) = sub(local, *at);
                dx.abs() <= reach && dy.abs() <= reach
            });
            if let Some((handle, _)) = hit {
                self.drag = Some(Drag::Resize { id: id.clone(), handle, at: b.0, size: b.1, now: b });
                return Vec::new();
            }
        }
        // A spawn (drawn over the boxes).
        let spawn = doc.level().objects.iter().position(|o| match o {
            LevelObject::Spawn { at, .. } => {
                let (dx, dy) = sub(world, *at);
                dx.abs() <= SPAWN_REACH && dy.abs() <= SPAWN_REACH
            }
        });
        if let Some(index) = spawn {
            let grab = sub(world, doc.level().objects[index].at());
            self.selection = Selection::Object(index);
            self.drag = Some(Drag::Spawn { index, before: doc.level().objects.clone(), grab });
            return Vec::new();
        }
        let Some(hit) = self.hit(world) else {
            self.selection = Selection::None;
            self.focus = None;
            return Vec::new();
        };
        // The folders holding the element under the pointer, outermost
        // first, and the element (issue #61).
        let path = path_to(doc, &hit);
        // A press on a group's member, or on a selected folder's, drags the
        // group (plan 14e R8; issue #61); a folder moves only once selected.
        let group = match &self.selection {
            Selection::Many(ids) => !deep && ids.iter().any(|id| path.contains(id)),
            Selection::Folder(f) => !deep && path.contains(f),
            _ => false,
        };
        if group {
            let found: Vec<usize> =
                self.selected_elements().iter().filter_map(|id| doc.index_of(id)).collect();
            let befores: Vec<Element> = found.iter().map(|&i| doc.items()[i].element.clone()).collect();
            match found.iter().find(|&&i| doc.locked_at(i)) {
                Some(&i) => {
                    self.status.message = Some(format!("element {:?} is locked", doc.items()[i].element.id))
                }
                None => self.drag = Some(Drag::MoveMany { befores, from: world }),
            }
            return Vec::new();
        }
        let kept = matches!(&self.selection, Selection::Element(e) if !deep && path.contains(e));
        let picked = if deep {
            self.focus = doc.parent_of(&hit);
            hit
        } else if kept {
            hit
        } else {
            self.click_pick(&hit)
        };
        let Some(doc) = &self.doc else { return Vec::new() };
        if doc.is_folder(&picked) {
            // A press on an unselected folder selects it (issue #61).
            self.selection = Selection::Folder(picked);
            return Vec::new();
        }
        match doc.index_of(&picked).map(|i| &doc.items()[i]) {
            Some(item) => {
                let e = item.element.clone();
                let locked = doc.index_of(&e.id).is_some_and(|i| doc.locked_at(i));
                // A drawn layer moves only once selected: a click on the map
                // never drags a whole layer by accident.
                let moves = image_box(&e).is_some() || self.selection == Selection::Element(e.id.clone());
                self.selection = Selection::Element(e.id.clone());
                if locked {
                    self.status.message = Some(format!("element {:?} is locked", e.id));
                } else if moves {
                    self.drag = Some(Drag::Move { before: e, from: world });
                }
            }
            None => self.selection = Selection::None,
        }
        Vec::new()
    }

    /// The topmost shown image box or covered drawn pixel under world
    /// pixel `world` (plan 14d R9): its id. A member of a hidden folder is
    /// not shown (issue #60).
    fn hit(&self, world: (i32, i32)) -> Option<String> {
        let doc = self.doc.as_ref()?;
        let (level, s) = (doc.level().size, doc.level().art_scale);
        doc.items()
            .iter()
            .enumerate()
            .rev()
            .find(|(i, item)| {
                doc.shown_at(*i)
                    && match &item.element.kind {
                        ElementKind::Image { .. } => image_box(&item.element)
                            .is_some_and(|b| contains(b, unshift(world, self.parallax(&item.element)))),
                        ElementKind::Drawn { .. } => {
                            covers(item, level, s, unshift(world, self.parallax(&item.element)))
                        }
                        ElementKind::Raster => false,
                    }
            })
            .map(|(_, item)| item.element.id.clone())
    }

    fn drag_to(&mut self, world: (i32, i32), shift: bool) -> Vec<EditorCommand> {
        let Some(drag) = self.drag.clone() else { return Vec::new() };
        match drag {
            Drag::MoveMany { befores, from } => {
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let level = doc.level().size;
                let (dx, dy) = sub(world, from);
                let mut dirty: Option<Rect> = None;
                for before in &befores {
                    let Some((at, _)) = element_box(before, level) else { continue };
                    let to = (add(at.0, dx), add(at.1, dy));
                    let current =
                        doc.index_of(&before.id).and_then(|i| element_box(&doc.items()[i].element, level));
                    if current.map(|b| b.0) == Some(to) {
                        continue;
                    }
                    match doc.set(&before.id, Property::At(to)) {
                        Ok(change) => dirty = Some(Rect::union(dirty, change.dirty)),
                        Err(e) => self.status.message = Some(e.to_string()),
                    }
                }
                dirty.map(|r| vec![EditorCommand::Upload(r)]).unwrap_or_default()
            }
            Drag::Move { before, from } => {
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let level = doc.level().size;
                let Some((at, _)) = element_box(&before, level) else { return Vec::new() };
                let (dx, dy) = sub(world, from);
                let to = (add(at.0, dx), add(at.1, dy));
                let current =
                    doc.index_of(&before.id).and_then(|i| element_box(&doc.items()[i].element, level));
                if current.map(|b| b.0) == Some(to) {
                    return Vec::new();
                }
                match doc.set(&before.id, Property::At(to)) {
                    Ok(change) => vec![EditorCommand::Upload(change.dirty)],
                    Err(e) => {
                        self.status.message = Some(e.to_string());
                        Vec::new()
                    }
                }
            }
            Drag::Resize { id, handle, at, size, .. } => {
                let start = handles((at, size)).find(|(h, _)| *h == handle).map_or(at, |(_, p)| p);
                let world = self.to_layer(world);
                let now = resized((at, size), handle, sub(world, start), shift);
                self.drag = Some(Drag::Resize { id, handle, at, size, now });
                Vec::new()
            }
            Drag::Spawn { index, grab, .. } => {
                let to = (add(world.0, -grab.0), add(world.1, -grab.1));
                if let Some(doc) = self.doc.as_mut() {
                    let _ = doc.move_object(index, to);
                }
                Vec::new()
            }
            Drag::Glow { id, before, last } => {
                if world == last {
                    return Vec::new();
                }
                let shapes = self.joins(last, world);
                let strength = self.glow_strength;
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let mut rect = None;
                for shape in &shapes {
                    if let Ok(r) = doc.paint_glow(&id, shape, strength)
                        && r.w > 0
                        && r.h > 0
                    {
                        rect = Some(Rect::union(rect, r));
                    }
                }
                self.drag = Some(Drag::Glow { id, before, last: world });
                rect.map(|r| vec![EditorCommand::Upload(r)]).unwrap_or_default()
            }
            Drag::Surface { id, before, last, value } => {
                if world == last {
                    return Vec::new();
                }
                let shapes = self.joins(last, world);
                let back = before.back;
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let mut rect = None;
                for shape in &shapes {
                    if let Ok(r) = doc.paint_surface(&id, back, shape, value)
                        && r.w > 0
                        && r.h > 0
                    {
                        rect = Some(Rect::union(rect, r));
                    }
                }
                self.drag = Some(Drag::Surface { id, before, last: world, value });
                rect.map(|r| vec![EditorCommand::Upload(r)]).unwrap_or_default()
            }
            Drag::Stroke { id, before, last, rect, stroke, begun, paint } => {
                if world == last {
                    return Vec::new();
                }
                let r = self.lay(&id, &self.joins(last, world), stroke, &paint);
                let rect = match r {
                    Some(r) => Some(Rect::union(rect, r)),
                    None => rect,
                };
                self.drag = Some(Drag::Stroke { id, before, last: world, rect, stroke, begun, paint });
                r.map(|r| vec![EditorCommand::Upload(r)]).unwrap_or_default()
            }
            Drag::Shape { id, tool, start, .. } => {
                self.drag = Some(Drag::Shape { id, tool, start, now: world });
                Vec::new()
            }
            Drag::Pixels { id, start, .. } => {
                self.drag = Some(Drag::Pixels { id, start, now: world });
                Vec::new()
            }
        }
    }

    /// Ends the drag: one history step for what it changed.
    pub(crate) fn release(&mut self) -> Vec<EditorCommand> {
        let Some(drag) = self.drag.take() else { return Vec::new() };
        // Only a shape paints on release (a stroke carries its own paint).
        let paint = match (&drag, &self.doc) {
            (Drag::Shape { .. }, Some(d)) => Some(self.paint_of(d)),
            _ => None,
        };
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        match drag {
            Drag::Pixels { id, start, now } => {
                // The rectangle within the layer's box and the level (plan
                // 14e minors): what a copy takes is what shows.
                let r = pixel_span(doc, &id, start, now);
                if r.w > 0 && r.h > 0 {
                    // Save as asset cuts every visible layer: the rectangle
                    // within the level only (issue #9).
                    let (x, y, w, h) = span(start, now);
                    self.pixel_world = Some(box_rect(((x, y), (w as u32, h as u32)), doc.level().size));
                    self.pixel_rect = Some((id, r));
                    self.pixel_region = None;
                } else {
                    self.status.message = Some(format!("no pixels of {id:?} there"));
                }
                Vec::new()
            }
            Drag::MoveMany { befores, .. } => {
                let edits: Vec<Edit> = befores
                    .into_iter()
                    .filter_map(|before| {
                        let index = doc.index_of(&before.id)?;
                        let after = doc.items()[index].element.clone();
                        (after != before).then_some(Edit::Element { index, before, after })
                    })
                    .collect();
                if edits.is_empty() {
                    return Vec::new();
                }
                self.history.push(edits);
                self.committed(NOTHING)
            }
            Drag::Move { before, .. } => {
                let Some(index) = doc.index_of(&before.id) else { return Vec::new() };
                let after = doc.items()[index].element.clone();
                if after == before {
                    return Vec::new();
                }
                self.history.push(vec![Edit::Element { index, before, after }]);
                self.committed(NOTHING)
            }
            Drag::Resize { id, at, size, now, .. } => {
                if now == (at, size) {
                    return Vec::new();
                }
                // The size (its glow mask fitted with it, plan 15a), then the place.
                let (first, first_dirty) = match doc.set_property(&id, Property::Size(now.1)) {
                    Ok(c) => c,
                    Err(e) => {
                        self.status.message = Some(e.to_string());
                        return Vec::new();
                    }
                };
                match doc.set(&id, Property::At(now.0)) {
                    Ok(second) => {
                        let dirty = Rect::union(Some(first_dirty), second.dirty);
                        self.history.push(first.into_iter().chain([second.edit]).collect());
                        self.committed(dirty)
                    }
                    Err(e) => {
                        let mut dirty = None;
                        for edit in first.iter().rev() {
                            dirty = Some(Rect::union(dirty, doc.apply(edit, false)));
                        }
                        self.status.message = Some(e.to_string());
                        vec![EditorCommand::Upload(dirty.unwrap_or(NOTHING))]
                    }
                }
            }
            Drag::Spawn { before, .. } => {
                let after = doc.level().objects.clone();
                if after == before {
                    return Vec::new();
                }
                self.history.push(vec![Edit::Objects { before, after }]);
                self.committed(NOTHING)
            }
            Drag::Glow { id, before, .. } => {
                let edits = doc.glow_stroke_edits(&id, before);
                if edits.is_empty() {
                    return Vec::new();
                }
                self.history.push(edits);
                self.committed(NOTHING)
            }
            Drag::Surface { id, before, .. } => {
                let edits = doc.surface_stroke_edits(&id, *before);
                if edits.is_empty() {
                    return Vec::new();
                }
                self.history.push(edits);
                self.committed(NOTHING)
            }
            Drag::Stroke { id, before, rect, begun, .. } => {
                // A mask's change is its own, on or off the level.
                let edit = match &*before {
                    StrokeBefore::Mask(_) => doc.stroke_edit(&id, NOTHING, &before),
                    StrokeBefore::Layer(_) => rect.and_then(|r| doc.stroke_edit(&id, r, &before)),
                };
                // A stroke of no change leaves no step, nor its conversion
                // (batch C review 1).
                let Some(edit) = edit.filter(|e| !e.is_noop()) else {
                    if let Some(b) = &begun {
                        doc.apply(b, false);
                    }
                    return Vec::new();
                };
                self.history.push(begun.map(|b| *b).into_iter().chain([edit]).collect());
                self.committed(NOTHING)
            }
            Drag::Shape { id, tool, start, now } => {
                let Some(paint) = paint else { return Vec::new() };
                let (x, y, w, h) = span(start, now);
                let shape = match tool {
                    Tool::Rect => Shape::Rect { x, y, w, h },
                    Tool::Ellipse => Shape::Ellipse { x, y, w, h },
                    _ => Shape::Line { from: start, to: now, width: self.brush.size },
                };
                let stroke = self.stroke_of(&id, false);
                let Some((begun, before)) = self.begin(&id, stroke) else { return Vec::new() };
                let Stroke::Paint(m) = stroke else { return Vec::new() };
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let painted = doc.paint(&id, &shape, &paint, Some(m));
                let edit = match &painted {
                    Ok(Some(rect)) => doc.stroke_edit(&id, *rect, &before),
                    _ => None,
                };
                if let Err(e) = &painted {
                    self.status.message = Some(e.to_string());
                }
                let Some(edit) = edit.filter(|e| !e.is_noop()) else {
                    if let (Some(b), Some(doc)) = (&begun, self.doc.as_mut()) {
                        doc.apply(b, false);
                    }
                    return Vec::new();
                };
                self.history.push(begun.into_iter().chain([edit]).collect());
                self.committed(painted.ok().flatten().unwrap_or(NOTHING))
            }
        }
    }

    /// Moves the selected image's or drawn layer's box by `(dx, dy)`, or
    /// each of a group's or a folder's (plan 14e R8, issue #61): one step.
    pub(crate) fn nudge(&mut self, dx: i32, dy: i32) -> Vec<EditorCommand> {
        if let Selection::Many(_) | Selection::Folder(_) = self.selection {
            let ids = self.selected_elements();
            return self.group_edit(|d| d.move_many(&ids, (dx, dy)));
        }
        let Selection::Element(id) = self.selection.clone() else { return Vec::new() };
        let Some(b) = self
            .doc
            .as_ref()
            .and_then(|d| d.index_of(&id).and_then(|i| element_box(&d.items()[i].element, d.level().size)))
        else {
            return Vec::new();
        };
        let to = (add(b.0.0, dx.into()), add(b.0.1, dy.into()));
        self.edit(|d| d.set(&id, Property::At(to)))
    }

    /// What the canvas draws over the bake, bottom to top.
    pub fn overlays(&self) -> Vec<Overlay> {
        let Some(doc) = &self.doc else { return Vec::new() };
        let size = doc.level().size;
        let mut out = Vec::new();
        let selected = |id: &str| self.selection.has(id);
        // The selected element's shift: a distant layer's tools draw where
        // it shows (issue #11).
        let layer = match &self.selection {
            Selection::Element(id) => {
                doc.index_of(id).map_or((0, 0), |i| self.parallax(&doc.items()[i].element))
            }
            _ => (0, 0),
        };
        for (i, item) in doc.items().iter().enumerate() {
            let e = &item.element;
            // Images' boxes, and the selected drawn layer's (plan 14d R9);
            // shown and locked as their folders make them (issue #60).
            let b = image_box(e).or_else(|| drawn_box(&e.kind, size).filter(|_| selected(&e.id)));
            if let (true, Some(b)) = (doc.shown_at(i), b) {
                let b = (shifted(b.0, self.parallax(e)), b.1);
                out.push(Overlay::Box {
                    rect: box_rect(b, size),
                    selected: selected(&e.id),
                    locked: doc.locked_at(i),
                });
            }
        }
        // A selected folder: one outline around its shown members' boxes
        // (issue #61).
        for f in self.selection.ids().iter().filter(|id| doc.is_folder(id)) {
            let mut rect = None;
            for m in doc.folder_members(f) {
                let Some(i) = doc.index_of(&m) else { continue };
                let e = &doc.items()[i].element;
                if let (true, Some(b)) = (doc.shown_at(i), element_box(e, size)) {
                    let r = box_rect((shifted(b.0, self.parallax(e)), b.1), size);
                    if r.w > 0 && r.h > 0 {
                        rect = Some(Rect::union(rect, r));
                    }
                }
            }
            if let Some(rect) = rect {
                let locked = olr_edit::folders::folder_locked(&doc.composition(), f);
                out.push(Overlay::Box { rect, selected: true, locked });
            }
        }
        if let Selection::Element(id) = &self.selection
            && let Some(i) = doc.index_of(id)
            && let e = &doc.items()[i].element
            && !doc.locked_at(i)
            && doc.shown_at(i)
            && let Some(b) = image_box(e)
        {
            let by = self.parallax(e);
            out.extend(handles(b).map(|(_, at)| Overlay::Handle { at: shifted(at, by) }));
        }
        match &self.drag {
            Some(Drag::Resize { id, now, .. }) => {
                let by = doc.index_of(id).map_or((0, 0), |i| self.parallax(&doc.items()[i].element));
                out.push(Overlay::Outline { rect: box_rect((shifted(now.0, by), now.1), size) })
            }
            Some(Drag::Shape { tool: Tool::Line, start, now, .. }) => {
                out.push(Overlay::Segment { from: shifted(*start, layer), to: shifted(*now, layer) })
            }
            Some(Drag::Shape { start, now, .. }) => {
                let (x, y, w, h) = span(*start, *now);
                let at = shifted((x, y), layer);
                out.push(Overlay::Outline { rect: box_rect((at, (w as u32, h as u32)), size) });
            }
            // The outline is what release keeps (review of the minors).
            Some(Drag::Pixels { id, start, now }) => out
                .push(Overlay::Outline { rect: shift_rect(pixel_span(doc, id, *start, *now), layer, size) }),
            _ => {}
        }
        if let Some((_, rect)) = &self.pixel_rect {
            out.push(Overlay::Outline { rect: shift_rect(*rect, layer, size) });
        }
        for (i, o) in doc.level().objects.iter().enumerate() {
            match o {
                LevelObject::Spawn { at, .. } => {
                    out.push(Overlay::Spawn { at: *at, selected: self.selection == Selection::Object(i) })
                }
            }
        }
        if matches!(self.tool, Tool::Brush | Tool::Eraser | Tool::Glow | Tool::Surface)
            && let Some(centre) = self.status.cursor
        {
            // A crater's outline spans its extent (plan 14e R4).
            let size = self.crater().map_or(self.brush.size, |(m, scale)| m.size.0.max(m.size.1) * scale);
            out.push(Overlay::Brush { centre, size, shape: self.brush.shape });
        }
        out
    }
}

/// The folders holding element `id` in `doc`, outermost first, then the
/// element (issue #61).
fn path_to(doc: &Document, id: &str) -> Vec<String> {
    let mut path = doc.folders_holding(id);
    path.push(id.to_string());
    path
}

/// Two pixel selections (world rectangles, a flag per art pixel at art
/// scale `s`) as one over their union; flags of another layout (a
/// selection made at another art scale) add nothing (plan 15a's final
/// review: they once indexed past the union).
fn join_regions(a: (Rect, &[bool]), b: (Rect, &[bool]), s: u32) -> (Rect, Vec<bool>) {
    let u = Rect::union(Some(a.0), b.0);
    let (uw, uh) = (u.w * s, u.h * s);
    let mut out = vec![false; uw as usize * uh as usize];
    for (r, flags) in [a, b] {
        if flags.len() != r.w as usize * s as usize * r.h as usize * s as usize {
            continue;
        }
        let rw = r.w * s;
        for (i, _) in flags.iter().enumerate().filter(|(_, f)| **f) {
            let (x, y) = (i as u32 % rw.max(1), i as u32 / rw.max(1));
            let (ux, uy) = ((r.x - u.x) * s + x, (r.y - u.y) * s + y);
            out[(uy * uw + ux) as usize] = true;
        }
    }
    (u, out)
}

#[cfg(test)]
mod join_tests {
    use super::*;

    /// A selection laid out at another art scale (4 flags per world pixel,
    /// joined at scale 1) adds nothing instead of indexing past the union.
    #[test]
    fn a_stale_selection_adds_nothing() {
        let a = Rect { x: 0, y: 0, w: 2, h: 2 };
        let b = Rect { x: 4, y: 0, w: 1, h: 1 };
        let (u, flags) = join_regions((a, &[true; 16]), (b, &[true]), 1);
        assert_eq!((u, flags.iter().filter(|f| **f).count()), (Rect { x: 0, y: 0, w: 5, h: 2 }, 1));
    }
}
