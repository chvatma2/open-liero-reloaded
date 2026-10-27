//! The canvas: a frame without fill (the host draws the bake under it,
//! plan 14c) with the overlays on top. Pointer events become world-pixel
//! `Pointer` actions through the view (ruling R7); Space and a drag pans,
//! the wheel zooms. Issue #61: an Alt+press is `AltDown`, a double click
//! `DoubleClick`.

use egui::{Color32, Key, Pos2, Sense, Stroke, StrokeKind, Ui};

use super::{EditorData, labels};
use crate::catalog::AssetKey;
use crate::state::{EditorAction, EditorState};
use crate::tools::{BrushShape, Overlay, PointerKind};

/// Grid lines are at least this many points apart.
const GRID_FROM: f32 = 8.0;

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, labels::CANVAS));
    data.canvas_rect = Some(rect);
    let size = [rect.width(), rect.height()];
    if data.canvas_size != Some(size) {
        data.canvas_size = Some(size);
        out.push(EditorAction::Canvas(size));
    }
    let view = state.view();
    let s = doc.level().art_scale;
    let unit = view.scale() * s as f32;
    let screen = |w: (i32, i32)| {
        let p = view.to_screen(w, s);
        rect.min + egui::vec2(p[0], p[1])
    };
    let world = |p: Pos2| view.to_world([p.x - rect.min.x, p.y - rect.min.y], s);

    // The pointer.
    let (pos, pressed, down, released, (shift, toggle, alt), delta, scroll, space, double) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
            (i.modifiers.shift, i.modifiers.command, i.modifiers.alt),
            i.pointer.delta(),
            i.events
                .iter()
                .map(|e| match e {
                    // One zoom step per wheel event.
                    egui::Event::MouseWheel { delta, .. } if delta.y != 0.0 => delta.y.signum() as i32,
                    _ => 0,
                })
                .sum::<i32>(),
            i.key_down(Key::Space),
            i.pointer.button_double_clicked(egui::PointerButton::Primary),
        )
    });
    let over = pos.is_some_and(|p| rect.contains(p)) && ui.rect_contains_pointer(rect);
    // A drag ends when the button is up, whatever else happens this frame
    // (a Space pan, the pointer gone): it never follows a mere hover.
    if data.canvas_drag && !down {
        data.canvas_drag = false;
        if let Some(w) = pos.map(world).or(data.last_world) {
            out.push(EditorAction::Pointer { kind: PointerKind::Up, world: w, shift, toggle });
        }
    }
    if space && !ui.ctx().egui_wants_keyboard_input() {
        if response.dragged() && delta != egui::Vec2::ZERO {
            out.push(EditorAction::Pan([delta.x, delta.y]));
        }
    } else if let Some(p) = pos {
        let w = world(p);
        let mut send = |kind| out.push(EditorAction::Pointer { kind, world: w, shift, toggle });
        if pressed
            && over
            && state.placing()
            && state.dialog().is_none()
            && state.browser().is_none()
            && let Some(key) = state.picked_asset()
        {
            // A picked asset lands where pressed (issue #9).
            egui::Popup::close_all(ui.ctx());
            out.push(EditorAction::PlaceAsset { key: key.clone(), at: w });
        } else if pressed && over && state.dialog().is_none() && state.browser().is_none() {
            // A press on the canvas closes a flyout or menu left open (a
            // stroke is no click: it would not close it).
            egui::Popup::close_all(ui.ctx());
            data.canvas_drag = true;
            if alt {
                out.push(EditorAction::AltDown { world: w });
            } else {
                send(PointerKind::Down);
            }
        } else if released {
            // Ended above.
        } else if (down && data.canvas_drag || over) && data.last_world != Some(w) {
            send(PointerKind::Move);
        }
        data.last_world = Some(w);
        if double && over && state.dialog().is_none() && state.browser().is_none() {
            out.push(EditorAction::DoubleClick { world: w });
        }
    }
    // An asset dragged from the Assets tab lands where it is let go.
    let carried = egui::DragAndDrop::payload::<AssetKey>(ui.ctx());
    if let Some(key) = response.dnd_release_payload::<AssetKey>()
        && let Some(p) = pos
    {
        out.push(EditorAction::PlaceAsset { key: (*key).clone(), at: world(p) });
    }
    if over
        && scroll != 0
        && let Some(p) = pos
    {
        out.push(EditorAction::Zoom { steps: scroll, about: [p.x - rect.min.x, p.y - rect.min.y] });
    }

    // The level's frame (the host draws the level itself) and the overlays.
    let painter = ui.painter_at(rect);
    let (w, h) = doc.level().size;
    let level_rect = egui::Rect::from_min_max(screen((0, 0)), screen((w as i32, h as i32)));
    painter.rect_stroke(level_rect, 0.0, Stroke::new(1.0, Color32::GRAY), StrokeKind::Outside);
    if state.grid() {
        grid(&painter, rect, level_rect, unit);
    }
    let accent = ui.visuals().selection.stroke.color;
    for o in state.overlays() {
        match o {
            Overlay::Box { rect: r, selected, locked } => {
                let b = egui::Rect::from_min_max(
                    screen((r.x as i32, r.y as i32)),
                    screen(((r.x + r.w) as i32, (r.y + r.h) as i32)),
                );
                let colour = if locked {
                    Color32::from_gray(140)
                } else if selected {
                    accent
                } else {
                    Color32::from_white_alpha(90)
                };
                painter.rect_stroke(
                    b,
                    0.0,
                    Stroke::new(if selected { 2.0 } else { 1.0 }, colour),
                    StrokeKind::Outside,
                );
            }
            Overlay::Handle { at } => {
                painter.rect_filled(
                    egui::Rect::from_center_size(screen(at), egui::vec2(7.0, 7.0)),
                    1.0,
                    accent,
                );
            }
            Overlay::Outline { rect: r } => {
                let b = egui::Rect::from_min_max(
                    screen((r.x as i32, r.y as i32)),
                    screen(((r.x + r.w) as i32, (r.y + r.h) as i32)),
                );
                painter.rect_stroke(b, 0.0, Stroke::new(1.0, Color32::YELLOW), StrokeKind::Outside);
            }
            Overlay::Segment { from, to } => {
                let half = egui::vec2(unit / 2.0, unit / 2.0);
                painter.line_segment(
                    [screen(from) + half, screen(to) + half],
                    Stroke::new(1.0, Color32::YELLOW),
                );
            }
            Overlay::Brush { centre, size, shape } => {
                let c = screen(centre) + egui::vec2(unit / 2.0, unit / 2.0);
                let radius = size as f32 * unit / 2.0;
                let stroke = Stroke::new(1.0, Color32::WHITE);
                match shape {
                    // A crater's outline is the circle about its extent.
                    BrushShape::Round | BrushShape::Crater(_) => {
                        painter.circle_stroke(c, radius, stroke);
                    }
                    BrushShape::Square => {
                        let square = egui::Rect::from_center_size(c, egui::vec2(radius * 2.0, radius * 2.0));
                        painter.rect_stroke(square, 0.0, stroke, StrokeKind::Middle);
                    }
                }
            }
            Overlay::Spawn { at, selected } => {
                let c = screen(at) + egui::vec2(unit / 2.0, unit / 2.0);
                let colour = if selected { accent } else { Color32::from_rgb(80, 220, 80) };
                painter.circle_filled(c, 4.0, colour);
                painter.circle_stroke(c, 6.0, Stroke::new(1.0, Color32::BLACK));
            }
        }
    }
    // Where an asset picked or carried would land: its box at the pointer.
    let landing = carried
        .map(|k| (*k).clone())
        .or_else(|| state.placing().then(|| state.picked_asset().cloned()).flatten());
    if over
        && let (Some(key), Some(p)) = (landing, pos)
        && let Some(a) = state.asset(&key)
    {
        let at = world(p);
        let b = egui::Rect::from_min_max(
            screen(at),
            screen((at.0.saturating_add(a.size.0 as i32), at.1.saturating_add(a.size.1 as i32))),
        );
        painter.rect_filled(b, 0.0, Color32::from_white_alpha(24));
        painter.rect_stroke(b, 0.0, Stroke::new(1.0, Color32::YELLOW), StrokeKind::Outside);
        painter.text(
            b.left_top() - egui::vec2(0.0, 2.0),
            egui::Align2::LEFT_BOTTOM,
            &a.name,
            egui::FontId::proportional(12.0),
            Color32::YELLOW,
        );
    }
    if state.surface_view() {
        legend(ui, state, rect);
    }
}

/// The Materials view's legend (issue #43), at the canvas's bottom left:
/// each of the level's materials' tint and name, then "None".
fn legend(ui: &Ui, state: &EditorState, canvas: egui::Rect) {
    let Some(doc) = state.active_document() else { return };
    egui::Area::new(egui::Id::new("olr-editor-legend"))
        .fixed_pos(canvas.left_bottom() + egui::vec2(8.0, -8.0))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(labels::LEGEND).strong());
                let side = ui.text_style_height(&egui::TextStyle::Body);
                let rows = doc
                    .materials()
                    .iter()
                    .enumerate()
                    .map(|(i, m)| (u8::try_from(i + 1).unwrap_or(0), &m.name));
                let none = labels::NO_SURFACE.to_string();
                for (v, name) in rows.chain([(0, &none)]) {
                    ui.horizontal(|ui| {
                        super::look::tint_swatch(ui, v, side, &labels::legend_tint(name));
                        ui.label(name);
                    });
                }
            });
        });
}

/// The world pixels between two grid lines at `unit` points per world
/// pixel: 1 when that is at least `GRID_FROM` points apart, else the
/// smallest power of two that is (the playtest: zoomed out, the grid was
/// never drawn and the checkbox seemed to do nothing).
fn grid_step(unit: f32) -> u32 {
    let mut step = 1u32;
    while (step as f32) * unit < GRID_FROM && step < 1 << 16 {
        step *= 2;
    }
    step
}

/// A line every `grid_step` world pixels across the visible part of the
/// level, from its origin.
fn grid(painter: &egui::Painter, clip: egui::Rect, level: egui::Rect, unit: f32) {
    let area = clip.intersect(level);
    if !area.is_positive() || unit.is_nan() || unit <= 0.0 {
        return;
    }
    let gap = grid_step(unit) as f32 * unit;
    // Light lines: black ones vanished on dark dirt.
    let stroke = Stroke::new(1.0, Color32::from_white_alpha(48));
    let first = |lo: f32, origin: f32| origin + ((lo - origin) / gap).ceil() * gap;
    let mut x = first(area.min.x, level.min.x);
    while x <= area.max.x {
        painter.vline(x, area.y_range(), stroke);
        x += gap;
    }
    let mut y = first(area.min.y, level.min.y);
    while y <= area.max.y {
        painter.hline(area.x_range(), y, stroke);
        y += gap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_shows_at_every_zoom() {
        assert_eq!(grid_step(16.0), 1, "zoomed in: every pixel");
        assert_eq!(grid_step(8.0), 1);
        assert_eq!(grid_step(4.0), 2);
        assert_eq!(grid_step(1.0), 8);
        assert_eq!(grid_step(0.25), 32, "zoomed out: every 32 pixels, 8 points apart");
        assert!(grid_step(0.0) >= 1 << 16, "no endless loop");
    }
}
