//! The asset tab's states panel (issue #79; the asset-based levels
//! design's "The asset tab"): every state small, in order, its picture
//! posed light (about [`PICTURE_HEIGHT`] points tall, shrunk to a narrow
//! panel), its name under it and ★ on the start; a click shows it. Under
//! them Add state, Duplicate, Make starting state and Remove state (each
//! disabled with the reason as its hover text), then the shown state's
//! Name. The pictures are made on release, only for the states whose
//! version moved (`state_ops::StatePictures`), kept as textures.

use egui::Ui;

use super::{EditorData, labels, relabel, text};
use crate::state::{EditorAction, EditorState};

/// A state's picture's height (points), at most.
const PICTURE_HEIGHT: f32 = 48.0;
/// A state's cell's narrowest (points): room for its name.
const CELL_MIN: f32 = 56.0;

pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let (Some(start), Some(shown)) = (doc.start_state(), doc.shown_state()) else { return };
    ui.strong(labels::STATES);
    if !state.dragging() {
        let made = data.state_pictures.refresh(doc, state.shown_generation());
        data.state_textures.resize(doc.states().len(), None);
        data.state_textures.truncate(doc.states().len());
        for i in made {
            let Some(img) = data.state_pictures.picture(i) else { continue };
            let size = [img.width() as usize, img.height() as usize];
            data.state_textures[i] = Some(ui.ctx().load_texture(
                format!("olr-editor-state-{i}"),
                egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                egui::TextureOptions::NEAREST,
            ));
        }
    }
    // The picture's size: PICTURE_HEIGHT tall, its aspect kept, shrunk to
    // the panel's width.
    let (w, h) = doc.level().size;
    let aspect = w.max(1) as f32 / h.max(1) as f32;
    let pad = 2.0 * (ui.spacing().button_padding.x + ui.visuals().widgets.inactive.bg_stroke.width);
    let room = ui.available_width() - pad;
    let height = PICTURE_HEIGHT.min(room / aspect).max(1.0);
    let picture = egui::vec2(height * aspect, height);
    let cell = (picture.x + pad).max(CELL_MIN).min(ui.available_width());
    ui.horizontal_wrapped(|ui| {
        for (i, st) in doc.states().iter().enumerate() {
            ui.allocate_ui_with_layout(
                egui::vec2(cell, picture.y + 40.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(cell);
                    let button = match data.state_textures.get(i).and_then(Option::as_ref) {
                        Some(t) => egui::Button::image(egui::Image::new((t.id(), picture))),
                        None => egui::Button::new("").min_size(picture),
                    };
                    let name = labels::state(&st.name, i == start);
                    let r = ui.add(button.selected(i == shown));
                    relabel(&r, egui::WidgetType::Button, Some(i == shown), &name);
                    if r.on_hover_text(&name).clicked() && i != shown {
                        out.push(EditorAction::SelectState(i));
                    }
                    ui.add(egui::Label::new(name).truncate());
                },
            );
        }
    });
    buttons(ui, state, out);
    let Some(st) = doc.states().get(shown) else { return };
    let key = format!("state/{shown}/name");
    // A state just added: its name field takes the keyboard (not one
    // added before the panel was first drawn).
    if data.state_named.is_some_and(|n| n != state.state_named()) {
        data.focus_text = Some(key.clone());
    }
    data.state_named = Some(state.state_named());
    if let Some(name) = text(ui, data, &key, labels::STATE_NAME, &st.name, false) {
        out.push(EditorAction::RenameState(name));
    }
}

/// Add state, Duplicate, Make starting state, Remove state: each disabled
/// with the reason as its hover text.
fn buttons(ui: &mut Ui, state: &EditorState, out: &mut Vec<EditorAction>) {
    let Some(doc) = state.active_document() else { return };
    let (Some(start), Some(shown)) = (doc.start_state(), doc.shown_state()) else { return };
    let full = doc.add_refusal();
    let remove = doc.remove_refusal(shown);
    let name = doc.shown_state_name();
    ui.horizontal_wrapped(|ui| {
        let button = |ui: &mut Ui, label: &str, why: Option<&str>, hint: &str| {
            let r = ui.add_enabled(why.is_none(), egui::Button::new(label));
            match why {
                Some(w) => r.on_disabled_hover_text(w),
                None => r.on_hover_text(hint),
            }
            .clicked()
        };
        if button(ui, labels::ADD_STATE, full.as_deref(), "a new state, looking like the starting state") {
            out.push(EditorAction::AddState);
        }
        if button(ui, labels::DUPLICATE_STATE, full.as_deref(), &format!("a copy of {name}")) {
            out.push(EditorAction::DuplicateState);
        }
        let started = (shown == start).then_some("it is the starting state");
        if button(ui, labels::MAKE_START, started, &format!("{name} starts: ★")) {
            out.push(EditorAction::MakeStart);
        }
        if button(ui, labels::REMOVE_STATE, remove.as_deref(), &format!("remove {name} and its own pictures"))
        {
            out.push(EditorAction::RemoveState);
        }
    });
}
