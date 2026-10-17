//! The canvas's pointer, per tool (plan 14b Task 7). Pointer positions are
//! untrusted world pixels: every shape is clipped by the document, every
//! sum saturates.

use olr_data::image::{Rgba, RgbaImage};
use olr_edit::compose::{Decoded, Rect};
use olr_edit::element::Property;
use olr_edit::level::{Paint, Shape};
use olr_format::levels::{Element, ElementKind, LevelObject, MAX_SIDE};

use crate::document::Document;
use crate::history::Edit;
use crate::ops::NOTHING;
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
    /// A handle drags an outline; the box changes on release (ruling R8).
    Resize { id: String, handle: (u8, u8), at: (i32, i32), size: (u32, u32), now: ((i32, i32), (u32, u32)) },
    /// A spawn follows the pointer, live.
    Spawn { index: usize, before: Vec<LevelObject>, grab: (i64, i64) },
    /// Brush or eraser dabs; `before` is the layer as the stroke began.
    Stroke { id: String, before: RgbaImage, last: (i32, i32), rect: Option<Rect>, erase: bool },
    /// A rectangle, ellipse or line from `start`, painted on release.
    Shape { id: String, tool: Tool, start: (i32, i32), now: (i32, i32) },
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

/// The world box from `a` to `b`, both included.
fn span(a: (i32, i32), b: (i32, i32)) -> (i32, i32, i32, i32) {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    let side = |lo: i32, hi: i32| (i64::from(hi) - i64::from(lo) + 1).min(i64::from(i32::MAX)) as i32;
    (x0, y0, side(x0, x1), side(y0, y1))
}

impl EditorState {
    /// The paint `self.paint` lays in `doc`.
    fn paint_of(&self, doc: &Document) -> Paint {
        match self.paint {
            PaintChoice::Colour(c) => Paint::Colour(c),
            PaintChoice::FrontTile => Paint::Tile(doc.front_tile().clone()),
            PaintChoice::BackTile => Paint::Tile(doc.back_tile().clone()),
        }
    }

    /// The selected drawn layer a painting tool paints into, or why not.
    fn paint_target(&mut self) -> Option<String> {
        let doc = self.doc.as_ref()?;
        let id = match &self.selection {
            Selection::Element(id) => id.clone(),
            _ => String::new(),
        };
        let item = doc.index_of(&id).map(|i| &doc.items()[i]);
        match item {
            Some(item) if matches!(item.decoded, Decoded::Drawn { .. }) => {
                if item.element.locked {
                    self.status.message = Some(format!("element {id:?} is locked"));
                    return None;
                }
                Some(id)
            }
            _ => {
                self.status.message = Some("select a drawn layer to paint".into());
                None
            }
        }
    }

    /// A brush dab at `p`.
    fn dab(&self, p: (i32, i32)) -> Shape {
        let size = self.brush.size;
        match self.brush.shape {
            BrushShape::Round => Shape::Circle { cx: p.0, cy: p.1, r: (size / 2) as i32 },
            BrushShape::Square => {
                let half = i64::from(size / 2);
                Shape::Rect { x: add(p.0, -half), y: add(p.1, -half), w: size as i32, h: size as i32 }
            }
        }
    }

    /// Paints (or erases) `shapes` into layer `id`; the rectangle rebaked.
    fn lay(&mut self, id: &str, shapes: &[Shape], erase: bool) -> Option<Rect> {
        let paint = self.paint_of(self.doc.as_ref()?);
        let doc = self.doc.as_mut()?;
        let mut rect = None;
        for shape in shapes {
            let r = if erase { doc.erase(id, shape) } else { doc.paint(id, shape, &paint) };
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

    pub(crate) fn pointer(
        &mut self,
        kind: PointerKind,
        world: (i32, i32),
        shift: bool,
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
        match kind {
            PointerKind::Down => self.down(world),
            PointerKind::Move => self.drag_to(world, shift),
            PointerKind::Up => {
                let cmds = self.drag_to(world, shift);
                cmds.into_iter().chain(self.release()).collect()
            }
        }
    }

    fn down(&mut self, world: (i32, i32)) -> Vec<EditorCommand> {
        // A drag whose Up never came ends here.
        let mut cmds = self.release();
        cmds.extend(self.press(world));
        cmds
    }

    fn press(&mut self, world: (i32, i32)) -> Vec<EditorCommand> {
        match self.tool {
            Tool::Select => self.select_down(world),
            Tool::Brush | Tool::Eraser => {
                let Some(id) = self.paint_target() else { return Vec::new() };
                let Some(before) = self.doc.as_ref().and_then(|d| d.layer_art(&id)).cloned() else {
                    return Vec::new();
                };
                let erase = self.tool == Tool::Eraser;
                let rect = self.lay(&id, &[self.dab(world)], erase);
                self.drag = Some(Drag::Stroke { id, before, last: world, rect, erase });
                rect.map(|r| vec![EditorCommand::Upload(r)]).unwrap_or_default()
            }
            Tool::Rect | Tool::Ellipse | Tool::Line => {
                if let Some(id) = self.paint_target() {
                    self.drag = Some(Drag::Shape { id, tool: self.tool, start: world, now: world });
                }
                Vec::new()
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

    fn select_down(&mut self, world: (i32, i32)) -> Vec<EditorCommand> {
        let Some(doc) = &self.doc else { return Vec::new() };
        // A handle of the selected unlocked image.
        if let Selection::Element(id) = &self.selection
            && let Some(i) = doc.index_of(id)
            && let e = &doc.items()[i].element
            && !e.locked
            && e.visible
            && let Some(b) = image_box(e)
        {
            let reach = HANDLE_REACH / (self.view.scale() * doc.level().art_scale as f32);
            let reach = i64::from(reach.ceil().max(1.0) as i32);
            let hit = handles(b).find(|(_, at)| {
                let (dx, dy) = sub(world, *at);
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
            let LevelObject::Spawn { at, .. } = &doc.level().objects[index];
            let grab = sub(world, *at);
            self.selection = Selection::Object(index);
            self.drag = Some(Drag::Spawn { index, before: doc.level().objects.clone(), grab });
            return Vec::new();
        }
        // The topmost visible image under the pointer.
        let hit = doc.items().iter().enumerate().rev().find(|(_, item)| {
            item.element.visible && image_box(&item.element).is_some_and(|b| contains(b, world))
        });
        match hit {
            Some((_, item)) => {
                let e = item.element.clone();
                self.selection = Selection::Element(e.id.clone());
                if e.locked {
                    self.status.message = Some(format!("element {:?} is locked", e.id));
                } else {
                    self.drag = Some(Drag::Move { before: e, from: world });
                }
            }
            None => self.selection = Selection::None,
        }
        Vec::new()
    }

    fn drag_to(&mut self, world: (i32, i32), shift: bool) -> Vec<EditorCommand> {
        let Some(drag) = self.drag.clone() else { return Vec::new() };
        match drag {
            Drag::Move { before, from } => {
                let Some((at, _)) = image_box(&before) else { return Vec::new() };
                let (dx, dy) = sub(world, from);
                let to = (add(at.0, dx), add(at.1, dy));
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let current = doc.index_of(&before.id).and_then(|i| image_box(&doc.items()[i].element));
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
            Drag::Stroke { id, before, last, rect, erase } => {
                if world == last {
                    return Vec::new();
                }
                let join = Shape::Line { from: last, to: world, width: self.brush.size };
                let r = self.lay(&id, &[join, self.dab(world)], erase);
                let rect = match r {
                    Some(r) => Some(Rect::union(rect, r)),
                    None => rect,
                };
                self.drag = Some(Drag::Stroke { id, before, last: world, rect, erase });
                r.map(|r| vec![EditorCommand::Upload(r)]).unwrap_or_default()
            }
            Drag::Shape { id, tool, start, .. } => {
                self.drag = Some(Drag::Shape { id, tool, start, now: world });
                Vec::new()
            }
        }
    }

    /// Ends the drag: one history step for what it changed.
    pub(crate) fn release(&mut self) -> Vec<EditorCommand> {
        let Some(drag) = self.drag.take() else { return Vec::new() };
        let Some(paint) = self.doc.as_ref().map(|d| self.paint_of(d)) else { return Vec::new() };
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        match drag {
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
                let first = match doc.set(&id, Property::Size(now.1)) {
                    Ok(c) => c,
                    Err(e) => {
                        self.status.message = Some(e.to_string());
                        return Vec::new();
                    }
                };
                match doc.set(&id, Property::At(now.0)) {
                    Ok(second) => {
                        let dirty = Rect::union(Some(first.dirty), second.dirty);
                        self.history.push(vec![first.edit, second.edit]);
                        self.committed(dirty)
                    }
                    Err(e) => {
                        let dirty = doc.apply(&first.edit, false);
                        self.status.message = Some(e.to_string());
                        vec![EditorCommand::Upload(dirty)]
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
            Drag::Stroke { id, before, rect, .. } => {
                let Some(rect) = rect else { return Vec::new() };
                let edit = doc.stroke_edit(&id, rect, &before);
                self.history.push(vec![edit]);
                self.committed(NOTHING)
            }
            Drag::Shape { id, tool, start, now } => {
                let (x, y, w, h) = span(start, now);
                let shape = match tool {
                    Tool::Rect => Shape::Rect { x, y, w, h },
                    Tool::Ellipse => Shape::Ellipse { x, y, w, h },
                    _ => Shape::Line { from: start, to: now, width: self.brush.size },
                };
                let Some(before) = doc.layer_art(&id).cloned() else { return Vec::new() };
                match doc.paint(&id, &shape, &paint) {
                    Ok(Some(rect)) => {
                        let edit = doc.stroke_edit(&id, rect, &before);
                        self.history.push(vec![edit]);
                        self.committed(rect)
                    }
                    Ok(None) => Vec::new(),
                    Err(e) => {
                        self.status.message = Some(e.to_string());
                        Vec::new()
                    }
                }
            }
        }
    }

    /// Moves the selected image's box by `(dx, dy)`: one step.
    pub(crate) fn nudge(&mut self, dx: i32, dy: i32) -> Vec<EditorCommand> {
        let Selection::Element(id) = self.selection.clone() else { return Vec::new() };
        let Some(b) =
            self.doc.as_ref().and_then(|d| d.index_of(&id).and_then(|i| image_box(&d.items()[i].element)))
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
        let selected = |id: &str| matches!(&self.selection, Selection::Element(s) if s == id);
        for item in doc.items() {
            let e = &item.element;
            if let (true, Some(b)) = (e.visible, image_box(e)) {
                out.push(Overlay::Box {
                    rect: box_rect(b, size),
                    selected: selected(&e.id),
                    locked: e.locked,
                });
            }
        }
        if let Selection::Element(id) = &self.selection
            && let Some(i) = doc.index_of(id)
            && let e = &doc.items()[i].element
            && !e.locked
            && e.visible
            && let Some(b) = image_box(e)
        {
            out.extend(handles(b).map(|(_, at)| Overlay::Handle { at }));
        }
        match &self.drag {
            Some(Drag::Resize { now, .. }) => out.push(Overlay::Outline { rect: box_rect(*now, size) }),
            Some(Drag::Shape { tool: Tool::Line, start, now, .. }) => {
                out.push(Overlay::Segment { from: *start, to: *now })
            }
            Some(Drag::Shape { start, now, .. }) => {
                let (x, y, w, h) = span(*start, *now);
                out.push(Overlay::Outline { rect: box_rect(((x, y), (w as u32, h as u32)), size) });
            }
            _ => {}
        }
        for (i, o) in doc.level().objects.iter().enumerate() {
            match o {
                LevelObject::Spawn { at, .. } => {
                    out.push(Overlay::Spawn { at: *at, selected: self.selection == Selection::Object(i) })
                }
            }
        }
        if matches!(self.tool, Tool::Brush | Tool::Eraser)
            && let Some(centre) = self.status.cursor
        {
            out.push(Overlay::Brush { centre, size: self.brush.size, shape: self.brush.shape });
        }
        out
    }
}
