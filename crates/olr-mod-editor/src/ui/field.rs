//! The generic fields, from the field table: a number with a slider over
//! its soft range (any value may be typed: the format validates), a
//! checkbox, a text, a dropdown of the mod's files or of an enum's
//! variants; the hover help is the format's doc comment, and the base
//! mod's value, when it differs, is shown with a Reset.

use egui::Ui;

use super::{Cx, caption, draw_thumb, labels, relabel, thumb};
use crate::fields::{self, FieldInfo, Place, Widget};
use crate::state::{ModAction, get_as, ron};

/// The fields of `owner` (`Worm`, `Fire::Beam`) at `base` (`mod.worm`) in
/// place `place`, as a grid; `prefix` begins their accessible names
/// (`Hit worm: `). Whether any was drawn.
pub(super) fn section(ui: &mut Ui, cx: &mut Cx, owner: &str, base: &str, prefix: &str, place: Place) -> bool {
    let rows: Vec<&FieldInfo> =
        fields::fields_of(owner).filter(|f| f.place == place && fields::widget(&f.ty).is_some()).collect();
    if rows.is_empty() {
        return false;
    }
    egui::Grid::new(("olr-mod-editor-fields", base, owner, place == Place::More))
        .num_columns(3)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            for f in rows {
                let path = format!("{base}.{}", f.name());
                row(ui, cx, f, &path, &format!("{prefix}{}", f.label()));
                ui.end_row();
            }
        });
    true
}

/// Whether `owner` has fields under More ▸.
pub(super) fn has_more(owner: &str) -> bool {
    fields::fields_of(owner).any(|f| f.place == Place::More && fields::widget(&f.ty).is_some())
}

/// One field's row: its label, its widget, its original value.
pub(super) fn row(ui: &mut Ui, cx: &mut Cx, f: &FieldInfo, path: &str, name: &str) {
    let Some(value) = cx.state.value(path) else { return };
    caption(ui, &f.label()).on_hover_text(&f.help);
    let r = ui.horizontal(|ui| widget(ui, cx, f, path, name, &value)).inner;
    if let Some(r) = r {
        let r = r.on_hover_text(&f.help);
        if cx.state.focus() == Some(path) {
            ui.painter().rect_stroke(
                r.rect.expand(3.0),
                3.0,
                egui::Stroke::new(2.0, ui.visuals().error_fg_color),
                egui::StrokeKind::Outside,
            );
            if cx.data.scrolled != cx.state.focus_gen() {
                r.scroll_to_me(Some(egui::Align::Center));
            }
        }
    }
    original(ui, cx, path, name);
}

/// The base mod's value when it differs, and Reset.
pub(super) fn original(ui: &mut Ui, cx: &mut Cx, path: &str, name: &str) {
    match cx.state.original(path) {
        Some(v) => {
            ui.horizontal(|ui| {
                let shown: String = v.chars().take(40).collect();
                let more = if shown.len() < v.len() { "…" } else { "" };
                ui.weak(format!("Original: {shown}{more}")).on_hover_text(v);
                let r = ui.small_button("Reset");
                relabel(&r, egui::WidgetType::Button, &labels::reset(name));
                if r.clicked() {
                    cx.out.push(ModAction::Reset(path.to_string()));
                }
            });
        }
        None => {
            ui.label("");
        }
    }
}

fn set(cx: &mut Cx, path: &str, value: String) {
    cx.commits.push(ModAction::Set { path: path.to_string(), value });
}

/// The field's widget; its response (for the focus mark and the help).
fn widget(
    ui: &mut Ui,
    cx: &mut Cx,
    f: &FieldInfo,
    path: &str,
    name: &str,
    value: &str,
) -> Option<egui::Response> {
    let kind = fields::widget(&f.ty)?;
    let r = match kind {
        Widget::Float | Widget::Int => {
            let r = text(ui, cx, path, name, value, 90.0, |t| (!t.is_empty()).then(|| t.to_string()));
            if let Some((lo, hi)) = f.range {
                slider(ui, cx, path, name, value, (lo, hi), kind == Widget::Int);
            }
            r
        }
        Widget::OptInt | Widget::OptSize => {
            let shown = if value == "None" { "" } else { value };
            let r = text(ui, cx, path, name, shown, 90.0, |t| {
                Some(if t.is_empty() { "None".to_string() } else { t.to_string() })
            });
            ui.weak("empty: the default");
            r
        }
        Widget::Bool => {
            let mut b = value == "true";
            let r = ui.checkbox(&mut b, "");
            relabel(&r, egui::WidgetType::Checkbox, name);
            if r.changed() {
                set(cx, path, b.to_string());
            }
            r
        }
        Widget::Text | Widget::OptText if f.asset.is_some() => {
            let folder = f.asset.clone().unwrap_or_default();
            asset(ui, cx, path, name, value, &folder, kind == Widget::OptText)
        }
        Widget::Text | Widget::OptText => {
            let shown: String = if value == "None" {
                String::new()
            } else {
                get_as(cx.state.src(), path).unwrap_or_default()
            };
            let optional = kind == Widget::OptText;
            let width = if f.name() == "notes" { 360.0 } else { 220.0 };
            text(ui, cx, path, name, &shown, width, |t| {
                Some(if optional && t.is_empty() { "None".to_string() } else { ron(&t.to_string()) })
            })
        }
        Widget::Colour => {
            let shown: String = get_as(cx.state.src(), path).unwrap_or_default();
            if let Ok(c) = olr_format::Colour::parse(&shown) {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, egui::Color32::from_rgb(c.0.r, c.0.g, c.0.b));
            }
            text(ui, cx, path, name, &shown, 90.0, |t| Some(ron(&t.to_string())))
        }
        Widget::Choice => {
            let current = cx.state.variant(path).unwrap_or_default();
            let mut chosen = None;
            let r = egui::ComboBox::from_id_salt(("olr-mod-editor-choice", path))
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    for v in cx.state.choices(path).into_iter().filter(|v| *v != "Unknown") {
                        let r = ui.selectable_label(v == current, v);
                        if let Some(help) = fields::variant_help(f.ty.as_str(), v) {
                            r.clone().on_hover_text(help);
                        }
                        if r.clicked() {
                            chosen = Some(v.to_string());
                        }
                    }
                })
                .response;
            relabel(&r, egui::WidgetType::ComboBox, name);
            if let Some(v) = chosen.filter(|v| *v != current) {
                set(cx, path, v);
            }
            r
        }
    };
    Some(r)
}

/// A text field committed when it loses the keyboard (Enter, Tab, a
/// click elsewhere); `commit` makes the value from the text, trimmed.
fn text(
    ui: &mut Ui,
    cx: &mut Cx,
    path: &str,
    name: &str,
    shown: &str,
    width: f32,
    commit: impl FnOnce(&str) -> Option<String>,
) -> egui::Response {
    let mut t = cx.data.drafts.get(path).cloned().unwrap_or_else(|| shown.to_string());
    let r = ui.add(
        egui::TextEdit::singleline(&mut t)
            .id(egui::Id::new(("olr-mod-editor-text", path)))
            .desired_width(width),
    );
    relabel(&r, egui::WidgetType::TextEdit, name);
    if r.changed() {
        cx.data.drafts.insert(path.to_string(), t.clone());
    }
    if r.lost_focus()
        && let Some(draft) = cx.data.drafts.remove(path)
        && draft.trim() != shown
        && let Some(v) = commit(draft.trim())
    {
        set(cx, path, v);
    }
    r
}

/// A slider over the soft range, committed when the drag stops.
fn slider(ui: &mut Ui, cx: &mut Cx, path: &str, name: &str, value: &str, (lo, hi): (f64, f64), int: bool) {
    let Ok(current) = value.parse::<f64>() else { return };
    let mut v = match &cx.data.slide {
        Some((p, v)) if p == path => *v,
        _ => current,
    };
    let mut s = egui::Slider::new(&mut v, lo..=hi).show_value(false).clamping(egui::SliderClamping::Never);
    if int {
        s = s.integer();
    }
    let r = ui.add(s);
    relabel(&r, egui::WidgetType::Slider, &labels::slider(name));
    if r.changed() {
        cx.data.slide = Some((path.to_string(), v));
    }
    if r.drag_stopped() || (r.changed() && !r.dragged()) {
        cx.data.slide = None;
        let text =
            if int { format!("{}", v.round() as i64) } else { format!("{}", (v * 1000.0).round() / 1000.0) };
        if text.parse::<f64>().ok() != Some(current) {
            set(cx, path, text);
        }
    }
}

/// A dropdown of the mod's files in `folder` (an image's with its
/// thumbnail); `optional` offers none.
fn asset(
    ui: &mut Ui,
    cx: &mut Cx,
    path: &str,
    name: &str,
    value: &str,
    folder: &str,
    optional: bool,
) -> egui::Response {
    let current: Option<String> = if value == "None" { None } else { get_as(cx.state.src(), path) };
    let images = folder == "gfx";
    if images
        && let Some(file) = &current
        && let Some(t) = thumb(ui, cx, file)
    {
        draw_thumb(ui, &t, 24.0);
    }
    let files = cx.state.assets(folder);
    let mut chosen = None;
    let r = egui::ComboBox::from_id_salt(("olr-mod-editor-asset", path))
        .selected_text(current.clone().unwrap_or_else(|| "(none)".into()))
        .width(220.0)
        .show_ui(ui, |ui| {
            if optional && ui.selectable_label(current.is_none(), "(none)").clicked() {
                chosen = Some("None".to_string());
            }
            for file in &files {
                ui.horizontal(|ui| {
                    if images && let Some(t) = thumb(ui, cx, file) {
                        draw_thumb(ui, &t, 20.0);
                    }
                    if ui.selectable_label(current.as_ref() == Some(file), file).clicked() {
                        chosen = Some(ron(file));
                    }
                });
            }
        })
        .response;
    relabel(&r, egui::WidgetType::ComboBox, name);
    if let Some(v) = chosen
        && Some(&v) != current.as_ref().map(ron).as_ref()
    {
        set(cx, path, v);
    }
    r
}
