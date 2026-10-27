//! The level tab's placing (issue #82; the asset-based levels design's
//! "The level tab", calls 5 and 6): Select picks a whole copy by its
//! covered pixels (`Document::copy_at`: a transparent pixel falls through
//! to what is behind), Ctrl toggles it into the selection, a drag moves
//! the selection live (one step on release), the arrows nudge it, the
//! eight handles of one selected copy resize it (its aspect kept unless
//! free, through `olr_edit::placed::keep_aspect`; Shift snaps to whole
//! multiples of the asset's size), previewed and applied on release.
//! Delete removes, Ctrl+C and Ctrl+V paste copies (8, 8) further each
//! paste, Ctrl+D duplicates; the Copies list reorders; a double click
//! opens the copy's asset. The document's side is `copies.rs`.

use olr_edit::compose::Rect;
use olr_edit::placed::{CopyProperty, Size, keep_aspect};
use olr_format::levels::{AssetOrigin, MAX_SIDE, PlacedFile};

use crate::catalog::AssetKey;
use crate::clipboard::Clipboard;
use crate::document::DocError;
use crate::ops::NOTHING;
use crate::pointer::Drag;
use crate::state::{DocTab, EditorCommand, EditorState, PanelTab, Pending, Selection};
use crate::tools::Overlay;

/// A copy's box: its top-left and size, world pixels.
pub type CopyBox = ((i32, i32), (u32, u32));

/// How far each paste (and Ctrl+D) moves copies from the last, in world
/// pixels.
pub const COPY_PASTE_STEP: (i32, i32) = (8, 8);

/// The level tab's refusal of a paste of elements (they are the asset
/// tab's).
pub const ELEMENTS_PASTE_IN_ASSET_TAB: &str =
    "elements paste in the asset tab: select the ground, then Edit in level";

/// The asset tab's refusal of a paste of copies (they are the level's).
pub const COPIES_PASTE_IN_LEVEL_TAB: &str = "copies paste in the level tab";

/// The level tab's refusal of Cut.
pub const NO_CUT: &str = "copies are not cut: copy them (Ctrl+C), then delete them";

/// The asset tab's refusal of a placing.
pub const PLACED_IN_THE_LEVEL: &str = "assets are placed in the level tab";

/// A change of the selected copy from its panel (issue #82).
#[derive(Clone, Debug, PartialEq)]
pub enum CopyChange {
    /// Its top-left.
    At((i32, i32)),
    /// Its width (the height following unless the aspect is free).
    Width(u32),
    /// Its height (the width following unless the aspect is free).
    Height(u32),
    /// Keep aspect: the inverse of `free_aspect` (on snaps the height to
    /// the width's aspect).
    KeepAspect(bool),
    Locked(bool),
}

/// The 8 handles of a box: (column, row) in 0..=2 each, the centre left out.
pub(crate) fn handles((at, size): ((i32, i32), (u32, u32))) -> impl Iterator<Item = ((u8, u8), (i32, i32))> {
    (0..3u8).flat_map(move |row| {
        (0..3u8).filter(move |&col| (col, row) != (1, 1)).map(move |col| {
            let along = |o: i32, side: u32, k: u8| {
                (i64::from(o) + i64::from(side) * i64::from(k) / 2).clamp(i32::MIN.into(), i32::MAX.into())
                    as i32
            };
            ((col, row), (along(at.0, size.0, col), along(at.1, size.1, row)))
        })
    })
}

/// `v` snapped to the nearest whole multiple of `n` (at least 1×, at most
/// `MAX_SIDE`); below 1× nothing snaps.
fn snapped(v: u32, n: u32) -> u32 {
    let n = n.max(1);
    if v < n {
        return v;
    }
    let k = ((u64::from(v) + u64::from(n) / 2) / u64::from(n)).clamp(1, u64::from(MAX_SIDE / n).max(1));
    (k * u64::from(n)) as u32
}

/// The size `size` gives an asset of `natural`: `Natural` when it is the
/// natural one (a copy then stores none).
fn normal(size: Size, natural: (u32, u32)) -> Size {
    match keep_aspect(natural, size) {
        Some(s) if s == natural => Size::Natural,
        _ => size,
    }
}

/// The box and size a copy's handle drag gives (issue #82): `start` the
/// copy's box, `handle` (column, row), the pointer moved by `delta`;
/// `natural` its asset's size. A corner resizes from the opposite corner,
/// an edge from the opposite edge; with the aspect kept (not `free`) the
/// side the pointer moved more leads and the other follows
/// (`keep_aspect`), centred on its axis for an edge. With `snap` (Shift)
/// the leading side (each moved side, free) snaps to the nearest whole
/// multiple of the natural size, none below 1×. Each side 1 to `MAX_SIDE`.
pub fn copy_resized(
    (at, size): ((i32, i32), (u32, u32)),
    (col, row): (u8, u8),
    delta: (i64, i64),
    natural: (u32, u32),
    free: bool,
    snap: bool,
) -> (CopyBox, Size) {
    let max = i64::from(MAX_SIDE);
    let (x0, y0) = (i64::from(at.0), i64::from(at.1));
    let (x1, y1) = (x0 + i64::from(size.0), y0 + i64::from(size.1));
    let (mut nx0, mut ny0, mut nx1, mut ny1) = (x0, y0, x1, y1);
    match col {
        0 => nx0 = (x0 + delta.0).clamp(x1 - max, x1 - 1),
        2 => nx1 = (x1 + delta.0).clamp(x0 + 1, x0 + max),
        _ => {}
    }
    match row {
        0 => ny0 = (y0 + delta.1).clamp(y1 - max, y1 - 1),
        2 => ny1 = (y1 + delta.1).clamp(y0 + 1, y0 + max),
        _ => {}
    }
    let (mut w, mut h) = ((nx1 - nx0) as u32, (ny1 - ny0) as u32);
    let n = (natural.0.max(1), natural.1.max(1));
    let wanted = if free {
        if snap && col != 1 {
            w = snapped(w, n.0);
        }
        if snap && row != 1 {
            h = snapped(h, n.1);
        }
        Size::Free(w, h)
    } else {
        // The side the pointer moved more, relative to the box, leads.
        let width_leads = match (col, row) {
            (1, _) => false,
            (_, 1) => true,
            _ => {
                let (sx, sy) = (i64::from(size.0.max(1)), i64::from(size.1.max(1)));
                (i64::from(w) * sy - sx * sy).abs() >= (i64::from(h) * sx - sx * sy).abs()
            }
        };
        let lead = |v: u32, n: u32| if snap { snapped(v, n) } else { v };
        let s = if width_leads { Size::Width(lead(w, n.0)) } else { Size::Height(lead(h, n.1)) };
        // A follower past the bound: the leader within it.
        match (s, keep_aspect(n, s)) {
            (Size::Width(_), Some((_, fh))) if fh > MAX_SIDE => Size::Height(MAX_SIDE),
            (Size::Height(_), Some((fw, _))) if fw > MAX_SIDE => Size::Width(MAX_SIDE),
            _ => s,
        }
    };
    let (nw, nh) = keep_aspect(n, wanted).unwrap_or(n);
    let (nw, nh) = (nw.clamp(1, MAX_SIDE), nh.clamp(1, MAX_SIDE));
    let place = |lo: i64, hi: i64, side: u32, was: u32, k: u8| match k {
        0 => hi - i64::from(side),
        // An edge's other side follows centred on its axis.
        1 if !free => lo + (i64::from(was) - i64::from(side)).div_euclid(2),
        _ => lo,
    };
    let clamp = |v: i64| v.clamp(i32::MIN.into(), i32::MAX.into()) as i32;
    let at = (clamp(place(x0, x1, nw, size.0, col)), clamp(place(y0, y1, nh, size.1, row)));
    ((at, (nw, nh)), normal(wanted, natural))
}

/// `v` as a multiple of `n`: "2×", or "1.5×".
fn multiple(v: u32, n: u32) -> String {
    let n = n.max(1);
    if v.is_multiple_of(n) {
        return format!("{}×", v / n);
    }
    let text = format!("{:.2}", f64::from(v) / f64::from(n));
    format!("{}×", text.trim_end_matches('0').trim_end_matches('.'))
}

/// The status line while a copy resizes: "58 × 4, 2×" (each side's
/// multiple when they differ: "58 × 6, 2× × 3×").
pub fn size_note((w, h): (u32, u32), natural: (u32, u32)) -> String {
    let (n0, n1) = (natural.0.max(1), natural.1.max(1));
    let m = if u64::from(w) * u64::from(n1) == u64::from(h) * u64::from(n0) {
        multiple(w, n0)
    } else {
        format!("{} × {}", multiple(w, n0), multiple(h, n1))
    };
    format!("{w} × {h}, {m}")
}

fn add(a: i32, b: i64) -> i32 {
    (i64::from(a) + b).clamp(i32::MIN.into(), i32::MAX.into()) as i32
}

fn sub(a: (i32, i32), b: (i32, i32)) -> (i64, i64) {
    (i64::from(a.0) - i64::from(b.0), i64::from(a.1) - i64::from(b.1))
}

impl Selection {
    /// The selected copies (issue #82), bottom to top as selected.
    pub fn copies(&self) -> Vec<String> {
        match self {
            Selection::Copy(id) => vec![id.clone()],
            Selection::Copies(ids) => ids.clone(),
            _ => Vec::new(),
        }
    }
}

impl EditorState {
    /// The selection of copies `ids` (those the level has), in stack
    /// order: one is `Copy`, more `Copies`, none `None`.
    pub(crate) fn copy_selection(&self, ids: Vec<String>) -> Selection {
        let Some(doc) = &self.doc else { return Selection::None };
        let stack: Vec<String> =
            doc.copies().into_iter().map(|c| c.id).filter(|id| ids.contains(id)).collect();
        match stack.len() {
            0 => Selection::None,
            1 => Selection::Copy(stack[0].clone()),
            _ => Selection::Copies(stack),
        }
    }

    /// Whether copy `id` is locked (the ground always is: it is the
    /// level's own).
    fn copy_locked(&self, id: &str) -> bool {
        self.doc.as_ref().is_some_and(|d| d.is_ground_copy(id) || d.placed_copy(id).is_some_and(|c| c.locked))
    }

    /// The level tab's Select press (issue #82): a spawn, a handle of the
    /// one selected copy, else the copy `copy_at` finds (Ctrl: toggled into
    /// the selection); a press on a selected copy drags the selection, on a
    /// locked one nothing (a drag says so).
    pub(crate) fn level_press(&mut self, world: (i32, i32), toggle: bool) -> Vec<EditorCommand> {
        if !toggle && self.press_spawn(world) {
            return Vec::new();
        }
        self.focus = None;
        let Some(doc) = &self.doc else { return Vec::new() };
        if let Selection::Copy(id) = &self.selection
            && !self.copy_locked(id)
            && let (Some(b), Some(natural), Some(c)) =
                (doc.copy_box(id), doc.natural_size(id), doc.placed_copy(id))
        {
            let reach = crate::pointer::HANDLE_REACH / (self.view.scale() * doc.level().art_scale as f32);
            // A small copy's middle stays its own: a press there moves it.
            let reach = i64::from(reach.ceil().max(1.0) as i32);
            let (rx, ry) = (reach.min(i64::from(b.1.0) / 4), reach.min(i64::from(b.1.1) / 4));
            let hit = handles(b).find(|(_, at)| {
                let (dx, dy) = sub(world, *at);
                dx.abs() <= rx && dy.abs() <= ry
            });
            if let Some((handle, _)) = hit {
                let size = normal(Size::Free(b.1.0, b.1.1), natural);
                self.drag = Some(Drag::ResizeCopy {
                    id: id.clone(),
                    handle,
                    from: world,
                    start: b,
                    natural,
                    free: c.free_aspect,
                    now: (b, size),
                });
                return Vec::new();
            }
        }
        let hit = doc.copy_at(world);
        let selected = self.selection.copies();
        if toggle {
            if let Some(hit) = hit {
                let mut ids = selected;
                match ids.iter().position(|c| *c == hit) {
                    Some(i) => {
                        ids.remove(i);
                    }
                    None => ids.push(hit),
                }
                self.selection = self.copy_selection(ids);
            }
            return Vec::new();
        }
        let Some(hit) = hit else {
            self.selection = Selection::None;
            return Vec::new();
        };
        let ids = if selected.contains(&hit) { selected } else { vec![hit.clone()] };
        self.selection = self.copy_selection(ids.clone());
        let Some(doc) = &self.doc else { return Vec::new() };
        if let Some(locked) = ids.iter().find(|id| self.copy_locked(id)) {
            self.drag = Some(Drag::LockedCopy { id: locked.clone() });
            return Vec::new();
        }
        let starts = ids.iter().filter_map(|id| Some((id.clone(), doc.placed_copy(id)?.at))).collect();
        self.drag = Some(Drag::MoveCopies { before: Box::new(doc.placement_file()), starts, from: world });
        Vec::new()
    }

    /// A copy's drag to `world` (issue #82): the selection moved live, a
    /// handle's box previewed (Shift snapping), a locked copy saying so.
    pub(crate) fn copy_drag(&mut self, drag: Drag, world: (i32, i32), shift: bool) -> Vec<EditorCommand> {
        match drag {
            Drag::MoveCopies { before, starts, from } => {
                let d = sub(world, from);
                let moves: Vec<(String, (i32, i32))> =
                    starts.iter().map(|(id, at)| (id.clone(), (add(at.0, d.0), add(at.1, d.1)))).collect();
                let Some(doc) = self.doc.as_mut() else { return Vec::new() };
                let now: Vec<(String, (i32, i32))> =
                    moves.iter().filter_map(|(id, _)| Some((id.clone(), doc.placed_copy(id)?.at))).collect();
                self.drag = Some(Drag::MoveCopies { before, starts, from });
                if now == moves {
                    return Vec::new();
                }
                let dirty = doc.live_move(&moves);
                if dirty.w > 0 && dirty.h > 0 { vec![EditorCommand::Upload(dirty)] } else { Vec::new() }
            }
            Drag::ResizeCopy { id, handle, from, start, natural, free, .. } => {
                let now = copy_resized(start, handle, sub(world, from), natural, free, shift);
                self.status.message = Some(size_note(now.0.1, natural));
                self.drag = Some(Drag::ResizeCopy { id, handle, from, start, natural, free, now });
                Vec::new()
            }
            Drag::LockedCopy { id } => {
                self.status.message = Some(format!("{id} is locked"));
                self.drag = Some(Drag::LockedCopy { id });
                Vec::new()
            }
            other => {
                self.drag = Some(other);
                Vec::new()
            }
        }
    }

    /// A copy's drag ended (issue #82): the moves as one step (`move_copy`
    /// per copy), the resize as one (`resize_copy`, the natural size
    /// storing none).
    pub(crate) fn copy_release(&mut self, drag: Drag) -> Vec<EditorCommand> {
        let Some(doc) = self.doc.as_mut() else { return Vec::new() };
        let done = match drag {
            Drag::MoveCopies { before, starts, .. } => {
                let moves: Vec<(String, (i32, i32))> =
                    starts.iter().filter_map(|(id, _)| Some((id.clone(), doc.placed_copy(id)?.at))).collect();
                if moves.iter().zip(&starts).all(|(m, s)| m.1 == s.1) {
                    return Vec::new();
                }
                doc.end_move(&before, &moves)
            }
            Drag::ResizeCopy { id, start, now, .. } => {
                if now.0 == start {
                    self.status.message = None;
                    return Vec::new();
                }
                doc.resize_copy(&id, now.1, now.0.0)
            }
            _ => return Vec::new(),
        };
        match done {
            Ok(change) => {
                self.history.push(vec![change.edit]);
                self.committed(change.dirty)
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                vec![EditorCommand::Upload(self.doc.as_ref().map_or(NOTHING, |d| d.all()))]
            }
        }
    }

    /// The level tab's overlays (issue #82): each selected copy's box (a
    /// locked one's marked), the one selected unlocked copy's eight handles,
    /// a resize's outline.
    pub(crate) fn copy_overlays(&self, out: &mut Vec<Overlay>) {
        let Some(doc) = &self.doc else { return };
        let level = doc.level().size;
        let rect = |b: ((i32, i32), (u32, u32))| {
            let c = |v: i64, side: u32| v.clamp(0, i64::from(side)) as u32;
            let (x0, y0) = (i64::from(b.0.0), i64::from(b.0.1));
            let (x1, y1) = (x0 + i64::from(b.1.0), y0 + i64::from(b.1.1));
            let (x0, y0, x1, y1) = (c(x0, level.0), c(y0, level.1), c(x1, level.0), c(y1, level.1));
            Rect { x: x0, y: y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
        };
        let selected = self.selection.copies();
        for id in &selected {
            if let Some(b) = doc.copy_box(id) {
                out.push(Overlay::Box { rect: rect(b), selected: true, locked: self.copy_locked(id) });
            }
        }
        if let [id] = selected.as_slice()
            && !self.copy_locked(id)
            && let Some(b) = doc.copy_box(id)
        {
            out.extend(handles(b).map(|(_, at)| Overlay::Handle { at }));
        }
        if let Some(Drag::ResizeCopy { now, .. }) = &self.drag {
            out.push(Overlay::Outline { rect: rect(now.0) });
        }
    }

    /// A change of the level's copies made by `op` as one step, or its
    /// refusal shown; `select` the copies selected after it.
    fn copy_edit<T>(
        &mut self,
        op: impl FnOnce(&mut crate::document::Document) -> Result<(crate::ops::Change, T), DocError>,
    ) -> Option<(Vec<EditorCommand>, T)> {
        self.copy_edit_named(None, op)
    }

    /// [`EditorState::copy_edit`], the step named `name` (`Edit::Name`,
    /// issue #83).
    pub(crate) fn copy_edit_named<T>(
        &mut self,
        name: Option<String>,
        op: impl FnOnce(&mut crate::document::Document) -> Result<(crate::ops::Change, T), DocError>,
    ) -> Option<(Vec<EditorCommand>, T)> {
        let doc = self.doc.as_mut()?;
        match op(doc) {
            Ok((change, _)) if change.edit.is_noop() => None,
            Ok((change, out)) => {
                self.history
                    .push(std::iter::once(change.edit).chain(name.map(crate::history::Edit::Name)).collect());
                Some((self.committed(change.dirty), out))
            }
            Err(e) => {
                self.status.message = Some(e.to_string());
                None
            }
        }
    }

    /// The arrows on the selected copies: each moved by `(dx, dy)`, one
    /// step.
    pub(crate) fn nudge_copies(&mut self, dx: i32, dy: i32) -> Vec<EditorCommand> {
        let ids = self.selection.copies();
        let Some(doc) = &self.doc else { return Vec::new() };
        let moves: Vec<(String, (i32, i32))> = ids
            .iter()
            .filter_map(|id| {
                let at = doc.placed_copy(id)?.at;
                Some((id.clone(), (add(at.0, dx.into()), add(at.1, dy.into()))))
            })
            .collect();
        self.copy_edit(|d| d.move_copies(&moves).map(|c| (c, ()))).map(|r| r.0).unwrap_or_default()
    }

    /// Delete on the selected copies: removed, one step; an asset no copy
    /// uses goes with its copy, and the status line says so.
    pub(crate) fn remove_copies(&mut self) -> Vec<EditorCommand> {
        let ids = self.selection.copies();
        let Some((cmds, removed)) = self.copy_edit(|d| d.remove_copies(&ids)) else { return Vec::new() };
        self.selection = Selection::None;
        let said: Vec<String> = removed
            .iter()
            .map(|r| match &r.asset {
                Some((_, a)) => {
                    format!("removed {} and its asset {:?}, no longer used", r.copy.id, a.file.name)
                }
                None => format!("removed {}", r.copy.id),
            })
            .collect();
        self.status.message = Some(said.join("; "));
        cmds
    }

    /// Ctrl+C on the selected copies: the level tab's clipboard.
    pub(crate) fn copy_copies(&mut self) -> bool {
        let ids = self.selection.copies();
        if ids.is_empty() {
            return false;
        }
        // The ground is the level's own: it is never copied.
        if self.doc.as_ref().is_some_and(|d| ids.iter().any(|id| d.is_ground_copy(id))) {
            self.status.message = Some(crate::ground::GROUND_LOCKED.into());
            return false;
        }
        self.status.message = Some(match ids.len() {
            1 => format!("copied {}", ids[0]),
            n => format!("copied {n} copies"),
        });
        self.clipboard = Some(Clipboard::Copies { ids, offset: COPY_PASTE_STEP });
        true
    }

    /// Duplicates copies `ids` `offset` further (`duplicate_copy`): one
    /// step, the new ones selected.
    fn duplicated(&mut self, ids: &[String], offset: (i32, i32)) -> Option<Vec<EditorCommand>> {
        let (cmds, new) = self.copy_edit(|d| d.duplicate_copies(ids, offset))?;
        self.selection = self.copy_selection(new);
        self.tab = PanelTab::Elements;
        Some(cmds)
    }

    /// Ctrl+V of copied copies: copies of them `offset` further, the
    /// offset growing by `COPY_PASTE_STEP` each paste.
    pub(crate) fn paste_copies(&mut self) -> Vec<EditorCommand> {
        let Some(Clipboard::Copies { ids, offset }) = self.clipboard.clone() else { return Vec::new() };
        let cmds = self.duplicated(&ids, offset);
        if cmds.is_some()
            && let Some(Clipboard::Copies { offset, .. }) = self.clipboard.as_mut()
        {
            *offset =
                (offset.0.saturating_add(COPY_PASTE_STEP.0), offset.1.saturating_add(COPY_PASTE_STEP.1));
        }
        cmds.unwrap_or_default()
    }

    /// Ctrl+D: the selected copies duplicated in place plus (8, 8).
    pub(crate) fn duplicate_copies(&mut self) -> Vec<EditorCommand> {
        let ids = self.selection.copies();
        if ids.is_empty() {
            self.status.message = Some("select copies to duplicate".into());
            return Vec::new();
        }
        self.duplicated(&ids, COPY_PASTE_STEP).unwrap_or_default()
    }

    /// A change of copy `id` from its panel or the Copies list: one step.
    pub(crate) fn set_copy(&mut self, id: &str, change: CopyChange) -> Vec<EditorCommand> {
        let Some(doc) = &self.doc else { return Vec::new() };
        let (Some(c), Some(natural), Some(b)) = (doc.placed_copy(id), doc.natural_size(id), doc.copy_box(id))
        else {
            return Vec::new();
        };
        let size = |s: Size| normal(s, natural);
        let r = self.copy_edit(|d| {
            let change = match change {
                CopyChange::At(at) => d.move_copies(&[(id.to_string(), at)]),
                CopyChange::Width(w) if c.free_aspect => d.resize_copy(id, size(Size::Free(w, b.1.1)), b.0),
                CopyChange::Width(w) => d.resize_copy(id, size(Size::Width(w)), b.0),
                CopyChange::Height(h) if c.free_aspect => d.resize_copy(id, size(Size::Free(b.1.0, h)), b.0),
                CopyChange::Height(h) => d.resize_copy(id, size(Size::Height(h)), b.0),
                CopyChange::KeepAspect(on) => d.set_copy(id, CopyProperty::FreeAspect(!on)),
                CopyChange::Locked(on) => d.set_copy(id, CopyProperty::Locked(on)),
            };
            change.map(|c| (c, ()))
        });
        r.map(|r| r.0).unwrap_or_default()
    }

    /// A row of the Copies list dropped at place `to` (bottom 0): one step.
    pub(crate) fn restack_copy(&mut self, id: &str, to: usize) -> Vec<EditorCommand> {
        self.copy_edit(|d| d.restack_copy(id, to).map(|c| (c, ()))).map(|r| r.0).unwrap_or_default()
    }

    /// Edit the asset of copy `id` (its panel's button, a double click):
    /// the ground in the asset tab (Edit in level), a library copy's asset
    /// from the library (Edit library asset).
    pub(crate) fn edit_copy(&mut self, id: &str) -> Vec<EditorCommand> {
        let Some(doc) = &self.doc else { return Vec::new() };
        if doc.is_ground_copy(id) {
            return self.act(crate::state::EditorAction::EditGround);
        }
        let name = doc.copy_name(id).unwrap_or_default();
        match doc.copy_origin(id) {
            Some(AssetOrigin::Builtin(folder)) | Some(AssetOrigin::Yours(folder)) => {
                let builtin = matches!(doc.copy_origin(id), Some(AssetOrigin::Builtin(_)));
                let key = AssetKey { builtin, id: folder };
                if self.asset(&key).is_none() {
                    self.status.message = Some(format!("{name:?} is not in the library"));
                    return Vec::new();
                }
                self.act(crate::state::EditorAction::EditAsset(key))
            }
            Some(AssetOrigin::Own) => match doc.placed_copy(id) {
                Some(c) => self.leave(Pending::EditOwn(c.asset)),
                None => Vec::new(),
            },
            None => Vec::new(),
        }
    }

    /// Whether the asset tab holds one of the level's own assets but the
    /// ground (issue #82).
    pub fn own_open(&self) -> bool {
        self.own.is_some() && self.asset_document().is_some()
    }

    /// Edit in level of an own asset (issue #82): the asset tab with the
    /// level's asset `key` open (`decode_own`), a fresh session, shown; its
    /// edits go to the level as the ground's do (`apply_own`).
    pub(crate) fn open_own_tab(&mut self, key: String) -> Vec<EditorCommand> {
        let Some(source) = self.document().and_then(|d| d.own_asset(&key)).cloned() else {
            return Vec::new();
        };
        let asset = match olr_edit::asset::decode_own(&source) {
            Ok(a) => a,
            Err(e) => {
                self.dialog = Some(crate::state::Dialog::Error(format!("asset {key}: {e}")));
                return Vec::new();
            }
        };
        let cmds = self.open_asset_tab(None, &asset);
        if self.shown == DocTab::Asset {
            self.dirty = false;
            self.own = Some(OwnOpen { key, applied: self.changes, source });
        }
        cmds
    }

    /// Issue #82: applies the asset tab's edits of an own asset to the
    /// level as one level step (its placement's asset made anew), marking
    /// the level unsaved; nothing when it holds none since the last time.
    pub(crate) fn apply_own(&mut self) {
        let Some(own) = self.own.clone() else { return };
        let (changes, made) = self.on_tab(DocTab::Asset, |s| {
            (s.changes, s.doc.as_ref().map(|d| olr_edit::asset::encode(&d.to_asset())))
        });
        if changes == own.applied {
            return;
        }
        let source = match made {
            Some(Ok(source)) => source,
            Some(Err(e)) => {
                self.status.message = Some(format!("the asset's edits are not in the level: {e}"));
                return;
            }
            None => return,
        };
        let applied = self.on_tab(DocTab::Level, |s| {
            let doc = s.doc.as_mut()?;
            match doc.set_own_asset(&own.key, source.clone()) {
                Ok(change) => {
                    if !change.edit.is_noop() {
                        s.history.push(vec![change.edit]);
                        s.committed(change.dirty);
                    }
                    Some(())
                }
                Err(e) => {
                    s.status.message = Some(e.to_string());
                    None
                }
            }
        });
        if applied.is_some() {
            self.own = Some(OwnOpen { key: own.key, applied: changes, source });
        }
    }

    /// Issue #82: on the way to the asset tab, its own asset follows the
    /// level: made again from it when the level changed it (an undone
    /// edit), closed when the level no longer has it.
    pub(crate) fn sync_own(&mut self) {
        let Some(own) = self.own.clone() else { return };
        let now = self.document().and_then(|d| d.own_asset(&own.key)).cloned();
        match now {
            Some(now) if now == own.source => {}
            Some(_) => {
                let cmds = self.open_own_tab(own.key);
                drop(cmds);
                self.status.message = Some("the asset as the level now has it".into());
            }
            None => {
                self.close_asset_tab();
                self.status.message = Some("the level no longer has the asset".into());
            }
        }
    }

    /// The asset `ReadAsset` read for placing (`key`, its top-left at
    /// `at`): a copy of it added on top, selected, one step.
    pub(crate) fn place_copy(
        &mut self,
        key: &AssetKey,
        asset: &olr_format::assets::AssetSource,
        at: (i32, i32),
    ) -> Vec<EditorCommand> {
        if self.shown != DocTab::Level {
            self.status.message = Some(PLACED_IN_THE_LEVEL.into());
            return Vec::new();
        }
        let origin = if key.builtin {
            AssetOrigin::Builtin(key.id.clone())
        } else {
            AssetOrigin::Yours(key.id.clone())
        };
        let Some((mut cmds, added)) = self.copy_edit(|d| d.add_copy(asset, origin.clone(), at)) else {
            return Vec::new();
        };
        self.selection = Selection::Copy(added.id.clone());
        self.tab = PanelTab::Elements;
        self.status.message = added.added_look.then(|| olr_edit::asset::ADDED_LIGHTING.to_string());
        // Issue #83: the library's version differed from the level's (a
        // new key): the level's older copies are offered it.
        if added.new_asset {
            let said = self.status.message.take();
            cmds.extend(self.offer_newer(origin, asset));
            let newer = self.status.message.take();
            let all: Vec<String> = said.into_iter().chain(newer).collect();
            self.status.message = (!all.is_empty()).then(|| all.join("; "));
        }
        cmds
    }

    /// The level's copies but the ground moved by `offset` (a level
    /// resize's), whatever their lock: the placement's edit, applied, or
    /// none.
    pub(crate) fn shifted_copies(
        doc: &mut crate::document::Document,
        offset: (i32, i32),
    ) -> Result<Option<crate::ops::Change>, DocError> {
        if offset == (0, 0) || !doc.placed_copies() {
            return Ok(None);
        }
        let mut after = doc.placement_file();
        let bound = olr_format::levels::MAX_COPY_AT;
        for c in after.copies.iter_mut().filter(|c| c.asset != olr_format::levels::GROUND_KEY) {
            c.at = (
                c.at.0.saturating_add(offset.0).clamp(-bound, bound),
                c.at.1.saturating_add(offset.1).clamp(-bound, bound),
            );
        }
        doc.file_change(after).map(Some)
    }
}

/// The level's own asset open in the asset tab (issue #82): its key in the
/// placement, the asset tab's change count when it was last applied (or
/// opened), and the asset as the level then had it.
#[derive(Clone, Debug)]
pub(crate) struct OwnOpen {
    pub key: String,
    pub applied: u64,
    pub source: olr_format::assets::AssetSource,
}

/// A placement file (a drag's start), boxed in the drag.
pub(crate) type FileBox = Box<PlacedFile>;
