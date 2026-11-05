//! Loose dirt's panels (the loose dirt design): the level's (or asset's)
//! loose materials, each its name and rates committed as one step, and a
//! dirt layer's Loose picker.

use egui::Ui;
use olr_format::levels::{Element, ElementKind, ElementMaterial, LooseSource};
use olr_format::loose::{GRAVITY, MAX_SLIDE_SPEED, REPOSE, TOP_SPEED};

use super::{EditorData, labels, number, text};
use crate::state::{EditorAction, EditorState};

/// The Loose dirt section, after the Materials: each material's name,
/// angle of repose, gravity, top speed and slide speed (pixels a second),
/// Remove (only when no layer names it), then Add loose dirt.
pub(super) fn level_loose(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    out: &mut Vec<EditorAction>,
) {
    let Some(doc) = state.active_document() else { return };
    ui.separator();
    ui.label(egui::RichText::new(labels::LOOSE_DIRT).strong()).on_hover_text(
        "dirt that falls and slides like sand: dirt layers are made of it in their properties",
    );
    for m in doc.loose() {
        let key = |field: &str| format!("loose/{}/{field}", m.id);
        let set = |out: &mut Vec<EditorAction>, material: LooseSource| {
            out.push(EditorAction::SetLoose { id: m.id.clone(), material });
        };
        if let Some(name) = text(ui, data, &key("name"), labels::NAME, &m.name, false) {
            set(out, LooseSource { name, ..m.clone() });
        }
        let range = |(lo, hi): (u16, u16)| i64::from(lo)..=i64::from(hi);
        let fields: [(&str, &str, i64, std::ops::RangeInclusive<i64>); 4] = [
            ("repose", labels::REPOSE, m.repose.into(), range((REPOSE.0.into(), REPOSE.1.into()))),
            ("gravity", labels::GRAVITY, m.gravity.into(), range(GRAVITY)),
            ("top", labels::TOP_SPEED, m.top_speed.into(), range(TOP_SPEED)),
            ("slide", labels::SLIDE_SPEED, m.slide_speed.into(), 0..=i64::from(MAX_SLIDE_SPEED)),
        ];
        for (field, label, now, range) in fields {
            let Some(v) = number(ui, data, &key(field), label, now, range) else { continue };
            let mut to = m.clone();
            match field {
                "repose" => to.repose = u8::try_from(v).unwrap_or(to.repose),
                "gravity" => to.gravity = u16::try_from(v).unwrap_or(to.gravity),
                "top" => to.top_speed = u16::try_from(v).unwrap_or(to.top_speed),
                _ => to.slide_speed = u8::try_from(v).unwrap_or(to.slide_speed),
            }
            set(out, to);
        }
        let users = doc.loose_users(&m.id);
        let hover = if users.is_empty() {
            "remove it from the level".to_string()
        } else {
            format!("used by {}: make them plain dirt first", users.join(", "))
        };
        let r = ui
            .add_enabled(users.is_empty(), egui::Button::new(labels::REMOVE).small())
            .on_hover_text(&hover)
            .on_disabled_hover_text(&hover);
        super::relabel(&r, egui::WidgetType::Button, None, &labels::remove_material(&m.name));
        if r.clicked() {
            out.push(EditorAction::RemoveLoose(m.id.clone()));
        }
        if !users.is_empty() {
            ui.weak(format!("used by {}", users.join(", ")));
        }
    }
    if ui.button(labels::ADD_LOOSE).on_hover_text("a new loose material of the default rates").clicked() {
        out.push(EditorAction::AddLoose);
    }
}

/// Whether element `e` can be loose: a dirt image or drawn layer, or a
/// mixed drawn layer's dirt, not distant (`check_elements`' rule).
fn can_be_loose(e: &Element) -> bool {
    e.distant.is_none()
        && match &e.kind {
            ElementKind::Image { material, .. } => *material == ElementMaterial::Dirt,
            ElementKind::Drawn { material, mixed, .. } => *mixed || *material == ElementMaterial::Dirt,
            ElementKind::Raster => false,
        }
}

/// A dirt layer's Loose picker: none (plain dirt) or one of the level's
/// loose materials; one step.
pub(super) fn element_loose(ui: &mut Ui, state: &EditorState, e: &Element, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    if !can_be_loose(e) {
        return;
    }
    let loose = doc.loose();
    let hint = if loose.is_empty() {
        "add loose dirt to the level first (its settings, with nothing selected)"
    } else {
        "its dirt falls and slides like sand"
    };
    let name = |id: Option<&String>| match id {
        Some(id) => loose.iter().find(|m| m.id == *id).map_or_else(|| id.clone(), |m| m.name.clone()),
        None => labels::NOT_LOOSE.to_string(),
    };
    let now = e.loose.as_ref();
    let fit = state.doc_tab() == crate::state::DocTab::Asset;
    let needs = ui.spacing().combo_width + ui.spacing().icon_width + 2.0 * ui.spacing().button_padding.x;
    super::beside(ui, fit, labels::LOOSE, needs, |ui, l| {
        ui.add_enabled_ui(!e.locked, |ui| {
            egui::ComboBox::from_id_salt(format!("{}/{}", e.id, labels::LOOSE))
                .selected_text(name(now))
                .show_ui(ui, |ui| {
                    for id in std::iter::once(None).chain(loose.iter().map(|m| Some(&m.id))) {
                        if ui.selectable_label(id == now, name(id)).clicked() && id != now {
                            out.push(EditorAction::Set(
                                e.id.clone(),
                                olr_edit::element::Property::Loose(id.cloned()),
                            ));
                        }
                    }
                })
                .response
                .labelled_by(l)
                .on_hover_text(hint);
        });
    });
}
