//! The file browser's window (plan 14c R9), modal as the dialogs are: the
//! path (typed paths too: a Windows drive, a WSL path), Up, the entries,
//! Choose and Cancel. A folder is entered with a click; a file (or, to
//! open a pack, a pack folder) is selected with a click and chosen with
//! Choose or a double click.

use egui::{Color32, Context, RichText};

use super::{EditorData, labels};
use crate::browser::Purpose;
use crate::state::{EditorAction, EditorState};

pub(super) fn show(ctx: &Context, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(b) = state.browser() else {
        data.browser = None;
        return;
    };
    // The typed path follows the directory shown; the selection is its.
    let dir = b.dir().to_path_buf();
    let draft = data.browser.get_or_insert_with(Default::default);
    if draft.dir.as_ref() != Some(&dir) {
        *draft =
            super::BrowserDraft { dir: Some(dir.clone()), path: dir.display().to_string(), selected: None };
    }
    let title = match b.purpose() {
        Purpose::ImportImage => "Import an image (PNG or JPEG)",
        Purpose::OpenPack => "Open a level pack",
    };
    let modal = egui::Modal::new(egui::Id::new("olr-editor-browser")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading(title);
        ui.horizontal(|ui| {
            let l = ui.label(labels::BROWSER_PATH);
            let r =
                ui.add(egui::TextEdit::singleline(&mut draft.path).desired_width(400.0)).labelled_by(l.id);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                out.push(EditorAction::BrowseTo(draft.path.trim().into()));
            }
            if ui.add_enabled(dir.parent().is_some(), egui::Button::new("Up")).clicked()
                && let Some(up) = dir.parent()
            {
                out.push(EditorAction::BrowseTo(up.to_path_buf()));
            }
        });
        if let Some(m) = b.message() {
            ui.label(RichText::new(m).color(Color32::from_rgb(255, 110, 90)));
        }
        ui.separator();
        egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, false]).show(ui, |ui| {
            let shown = b.shown();
            if shown.is_empty() {
                ui.weak("Nothing to choose here.");
            }
            for e in shown {
                let choosable = match b.purpose() {
                    Purpose::ImportImage => !e.is_dir,
                    Purpose::OpenPack => e.is_pack,
                };
                let text = if e.is_pack {
                    format!("📦 {}", e.name)
                } else if e.is_dir {
                    format!("📁 {}", e.name)
                } else {
                    e.name.clone()
                };
                let selected = draft.selected.as_deref() == Some(e.name.as_str());
                let r = ui.selectable_label(selected, text);
                // The accessible name is the entry's own.
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &e.name)
                });
                if r.double_clicked() && choosable {
                    out.push(EditorAction::Pick(e.name.clone()));
                } else if r.clicked() {
                    if choosable {
                        draft.selected = Some(e.name.clone());
                    } else {
                        out.push(EditorAction::BrowseTo(dir.join(&e.name)));
                    }
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            let chosen = draft.selected.clone();
            if ui.add_enabled(chosen.is_some(), egui::Button::new(labels::CHOOSE)).clicked()
                && let Some(name) = chosen
            {
                out.push(EditorAction::Pick(name));
            }
            if ui.button(labels::CANCEL).clicked() {
                out.push(EditorAction::CloseBrowser);
            }
        });
    });
    // Escape or the backdrop closes it, unless a button answered.
    if modal.should_close()
        && !out.iter().any(|a| matches!(a, EditorAction::Pick(_) | EditorAction::CloseBrowser))
    {
        out.push(EditorAction::CloseBrowser);
    }
}
