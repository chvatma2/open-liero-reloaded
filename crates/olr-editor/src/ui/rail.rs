//! The tool rail (issue #8): one column of buttons, each an icon over a
//! short label. Issue #81: the level tab's holds Select, the Haze brush and
//! Spawn; the layer tools moved to the asset tab, which lays them out in
//! rows across its tools panel (issue #78, `grid`): Select, Draw ▸,
//! Pixels ▸, Glow, Surface and Import. A group's button (its corner mark)
//! shows the icon of its tool chosen last, and one click chooses that tool
//! again; a second click, a click on the corner mark or a right click
//! opens its flyout, which lists the group's tools with their shortcuts. A
//! pick, a click elsewhere or Escape closes it.

use egui::{Align2, Rect, Response, Sense, Stroke, StrokeKind, Ui, WidgetType, pos2, vec2};

use super::icons::{self, Icon};
use super::{labels, relabel};
use crate::browser::Purpose;
use crate::state::{DocTab, EditorAction, EditorState};
use crate::tools::{Tool, ToolGroup};

/// The rail's width (points).
pub(super) const RAIL_WIDTH: f32 = 64.0;
/// An icon's side on the rail.
const ICON: f32 = 22.0;
/// An icon's side in a flyout.
const ROW_ICON: f32 = 16.0;
/// A flyout row's width.
const ROW_WIDTH: f32 = 190.0;
/// The corner mark's side: a click there opens the flyout.
const CORNER: f32 = 14.0;
/// A button's width in the asset tab's rows (points).
const GRID_BUTTON: f32 = 54.0;

/// The level tab's rail (issue #81): Select, the Haze brush and Spawn;
/// the layer tools are the asset tab's.
pub(super) fn show(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    tools(ui, state, None, out);
}

/// The asset tab's tools (issue #78): the rail's buttons in rows across
/// the panel, Select, Draw ▸, Pixels ▸, Glow brush and Surface brush,
/// then Import image….
pub(super) fn grid(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    ui.horizontal_wrapped(|ui| {
        tools(ui, state, Some(GRID_BUTTON), out);
        import(ui, Some(GRID_BUTTON), out);
    });
}

/// Import image…, `width` points wide (else the rail's).
fn import(ui: &mut Ui, width: Option<f32>, out: &mut Vec<EditorAction>) {
    let import = button(ui, Icon::Import, "Import", labels::IMPORT_IMAGE, (false, false), width)
        .on_hover_text("import a picture (.png or .jpg) as a new element");
    if import.clicked() {
        out.push(EditorAction::Browse(Purpose::ImportImage));
    }
}

/// The tools' buttons, `width` points wide (else the rail's).
fn tools(ui: &mut Ui, state: &EditorState, width: Option<f32>, out: &mut Vec<EditorAction>) {
    single(ui, state, Tool::Select, "Select", width, out);
    // The level's own (issue #77: not the asset tab's); issue #81: its
    // only others.
    if state.doc_tab() == DocTab::Level {
        single(ui, state, Tool::Haze, "Haze", width, out);
        single(ui, state, Tool::Spawn, "Spawn", width, out);
        return;
    }
    for group in ToolGroup::ALL {
        grouped(ui, state, group, width, out);
    }
    // Its label is the brush's, never the element's Glow checkbox's.
    single(ui, state, Tool::Glow, "Glow", width, out);
    single(ui, state, Tool::Surface, "Surface", width, out);
}

/// A button of one tool.
fn single(
    ui: &mut Ui,
    state: &EditorState,
    tool: Tool,
    text: &str,
    width: Option<f32>,
    out: &mut Vec<EditorAction>,
) {
    let r = button(ui, Icon::of(tool), text, tool.name(), (state.tool() == tool, false), width)
        .on_hover_text(format!("{} ({})", tool.name(), tool.key()));
    if r.clicked() {
        out.push(EditorAction::Tool(tool));
    }
}

/// A group's button and its flyout.
fn grouped(
    ui: &mut Ui,
    state: &EditorState,
    group: ToolGroup,
    width: Option<f32>,
    out: &mut Vec<EditorAction>,
) {
    let last = state.last_tool(group);
    let chosen = state.tool().group() == Some(group);
    let r =
        button(ui, Icon::of(last), group.name(), group.name(), (chosen, true), width).on_hover_text(format!(
            "{} ({}); click again, or on the corner, for every {} tool",
            last.name(),
            last.key(),
            group.name().to_lowercase()
        ));
    let flyout = r.id.with("flyout");
    let on_corner = r.interact_pointer_pos().is_some_and(|p| corner(r.rect).contains(p));
    let opens = r.secondary_clicked() || (r.clicked() && (chosen || on_corner));
    if opens {
        egui::Popup::toggle_id(ui.ctx(), flyout);
    } else if r.clicked() {
        out.push(EditorAction::Tool(last));
    }
    egui::Popup::new(flyout, ui.ctx().clone(), r.rect, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .align(egui::RectAlign::RIGHT_START)
        .gap(4.0)
        .open_memory(None)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            for &tool in group.tools() {
                if row(ui, tool, state.tool() == tool).clicked() {
                    out.push(EditorAction::Tool(tool));
                }
            }
        });
}

/// A group button's corner mark.
fn corner(rect: Rect) -> Rect {
    Rect::from_min_max(rect.max - vec2(CORNER, CORNER), rect.max)
}

/// A rail button: `icon` over `text`, named `label` for AccessKit,
/// `width` points wide (else the rail's); `mark` draws a group's corner
/// mark.
fn button(
    ui: &mut Ui,
    icon: Icon,
    text: &str,
    label: &str,
    (selected, mark): (bool, bool),
    width: Option<f32>,
) -> Response {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, egui::Color32::PLACEHOLDER);
    let size = vec2(width.unwrap_or(ui.available_width()).max(ICON), ICON + galley.size().y + 12.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact_selectable(&r, selected);
        if selected || r.hovered() || r.has_focus() {
            ui.painter().rect(rect, v.corner_radius, v.weak_bg_fill, v.bg_stroke, StrokeKind::Inside);
        }
        let colour = v.text_color();
        let icon_rect =
            Rect::from_center_size(pos2(rect.center().x, rect.top() + 4.0 + ICON / 2.0), vec2(ICON, ICON));
        icons::paint(ui.painter(), icon_rect, icon, Stroke::new(1.5, colour));
        let at = pos2(rect.center().x - galley.size().x / 2.0, icon_rect.bottom() + 3.0);
        ui.painter().galley(at, galley, colour);
        if mark {
            let m = rect.max - vec2(3.0, 3.0);
            let tri = vec![m, m - vec2(0.0, 7.0), m - vec2(7.0, 0.0)];
            ui.painter().add(egui::Shape::convex_polygon(tri, colour, Stroke::NONE));
        }
    }
    relabel(&r, WidgetType::Button, Some(selected), label);
    r
}

/// A flyout's row: the tool's icon, name and shortcut.
fn row(ui: &mut Ui, tool: Tool, selected: bool) -> Response {
    let size = vec2(ROW_WIDTH, ui.spacing().interact_size.y.max(ROW_ICON + 8.0));
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact_selectable(&r, selected);
        if selected || r.hovered() || r.has_focus() {
            ui.painter().rect(rect, v.corner_radius, v.weak_bg_fill, v.bg_stroke, StrokeKind::Inside);
        }
        let colour = v.text_color();
        let pad = ui.spacing().button_padding.x;
        let icon_rect = Rect::from_center_size(
            pos2(rect.left() + pad + ROW_ICON / 2.0, rect.center().y),
            vec2(ROW_ICON, ROW_ICON),
        );
        icons::paint(ui.painter(), icon_rect, Icon::of(tool), Stroke::new(1.5, colour));
        let font = egui::TextStyle::Button.resolve(ui.style());
        let name_at = pos2(icon_rect.right() + pad, rect.center().y);
        ui.painter().text(name_at, Align2::LEFT_CENTER, tool.name(), font.clone(), colour);
        let key_at = pos2(rect.right() - pad, rect.center().y);
        ui.painter().text(key_at, Align2::RIGHT_CENTER, tool.key(), font, ui.visuals().weak_text_color());
    }
    relabel(&r, WidgetType::Button, Some(selected), tool.name());
    r.on_hover_text(format!("{} ({})", tool.name(), tool.key()))
}
