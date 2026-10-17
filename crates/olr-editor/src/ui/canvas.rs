//! The canvas: a frame without fill (the host draws the bake under it,
//! plan 14c) with the overlays on top. Pointer events become world-pixel
//! `Pointer` actions through the view (ruling R7); Space and a drag pans,
//! the wheel zooms.

use egui::{Color32, Key, Pos2, Sense, Stroke, StrokeKind, Ui};

use super::{EditorData, labels};
use crate::state::{EditorAction, EditorState};
use crate::tools::{BrushShape, Overlay, PointerKind};

/// Grid lines are drawn from this many points per art pixel.
const GRID_FROM: f32 = 8.0;

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.document() else { return };
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
    let (pos, pressed, down, released, shift, delta, scroll, space) = ui.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
            i.modifiers.shift,
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
        )
    });
    let over = pos.is_some_and(|p| rect.contains(p)) && ui.rect_contains_pointer(rect);
    // A drag ends when the button is up, whatever else happens this frame
    // (a Space pan, the pointer gone): it never follows a mere hover.
    if data.canvas_drag && !down {
        data.canvas_drag = false;
        if let Some(w) = pos.map(world).or(data.last_world) {
            out.push(EditorAction::Pointer { kind: PointerKind::Up, world: w, shift });
        }
    }
    if space && !ui.ctx().egui_wants_keyboard_input() {
        if response.dragged() && delta != egui::Vec2::ZERO {
            out.push(EditorAction::Pan([delta.x, delta.y]));
        }
    } else if let Some(p) = pos {
        let w = world(p);
        let mut send = |kind| out.push(EditorAction::Pointer { kind, world: w, shift });
        if pressed && over && state.dialog().is_none() && state.browser().is_none() {
            data.canvas_drag = true;
            send(PointerKind::Down);
        } else if released {
            // Ended above.
        } else if (down && data.canvas_drag || over) && data.last_world != Some(w) {
            send(PointerKind::Move);
        }
        data.last_world = Some(w);
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
    if state.grid() && unit >= GRID_FROM {
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
                    BrushShape::Round => {
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
}

/// A line per art pixel across the visible part of the level.
fn grid(painter: &egui::Painter, clip: egui::Rect, level: egui::Rect, unit: f32) {
    let area = clip.intersect(level);
    if !area.is_positive() {
        return;
    }
    let stroke = Stroke::new(1.0, Color32::from_black_alpha(60));
    let first = |lo: f32, origin: f32| origin + ((lo - origin) / unit).ceil() * unit;
    let mut x = first(area.min.x, level.min.x);
    while x <= area.max.x {
        painter.vline(x, area.y_range(), stroke);
        x += unit;
    }
    let mut y = first(area.min.y, level.min.y);
    while y <= area.max.y {
        painter.hline(area.x_range(), y, stroke);
        y += unit;
    }
}
