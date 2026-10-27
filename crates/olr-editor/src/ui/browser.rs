//! The file browser's window (plan 14c R9), modal as the dialogs are: the
//! path (typed paths too: a Windows drive, a WSL path), Up, the entries,
//! Choose and Cancel. A click selects an entry; Choose or a double click
//! picks it: a file (or, to open a pack, a pack folder) is chosen, any
//! other folder entered.

use egui::{Color32, Context, RichText};

use super::{EditorData, labels};
use crate::browser::Purpose;
use crate::state::{EditorAction, EditorState};

/// The preview column's width (points), beside an import's list.
const PREVIEW_WIDTH: f32 = 200.0;

/// The selected image's preview (the host's small copy) and its size.
fn preview(ui: &mut egui::Ui, b: &crate::browser::Browser, draft: &mut super::BrowserDraft) {
    ui.vertical(|ui| {
        ui.set_width(PREVIEW_WIDTH);
        let shown = b.preview().filter(|p| draft.selected.as_deref() == Some(p.name.as_str()));
        let Some(p) = shown else {
            ui.weak("Select an image to see it here.");
            return;
        };
        match &p.image {
            Ok(img) => {
                if draft.preview.as_ref().is_none_or(|(n, _)| *n != p.name) {
                    let size = [img.width() as usize, img.height() as usize];
                    let colour = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                    // A small image is enlarged to fill the box: sharp pixels.
                    let small = img.width().max(img.height()) < PREVIEW_WIDTH as u32;
                    let filter =
                        if small { egui::TextureOptions::NEAREST } else { egui::TextureOptions::LINEAR };
                    let tex = ui.ctx().load_texture("olr-editor-preview", colour, filter);
                    draft.preview = Some((p.name.clone(), tex));
                }
                if let Some((_, tex)) = &draft.preview {
                    let fit = tex.size_vec2() * (PREVIEW_WIDTH / tex.size_vec2().max_elem().max(1.0));
                    ui.image((tex.id(), fit));
                }
                if let Some((w, h)) = p.size {
                    ui.label(format!("{w} × {h}"));
                }
            }
            Err(why) => {
                ui.label(RichText::new(why).color(Color32::from_rgb(255, 110, 90)));
            }
        }
    });
}

pub(super) fn show(ctx: &Context, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(b) = state.browser() else {
        data.browser = None;
        return;
    };
    // The typed path follows the directory shown; the selection is its.
    let dir = b.dir().to_path_buf();
    let draft = data.browser.get_or_insert_with(Default::default);
    if draft.dir.as_ref() != Some(&dir) {
        *draft = super::BrowserDraft {
            dir: Some(dir.clone()),
            path: dir.display().to_string(),
            selected: None,
            preview: None,
        };
    }
    let title = match b.purpose() {
        Purpose::ImportImage => "Import an image (PNG or JPEG)",
        Purpose::ImportTexture => "Import a texture (PNG or JPEG, at most 512 × 512)",
        Purpose::OpenPack => "Open a level pack",
        Purpose::ImportAsset => "Import an asset (a folder holding asset.ron)",
        Purpose::ImportSound => "Import a sound (WAV or Ogg)",
    };
    let modal = egui::Modal::new(egui::Id::new("olr-editor-browser")).show(ctx, |ui| {
        ui.set_width(620.0);
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
        let import = b.purpose().images();
        ui.horizontal_top(|ui| {
            let list =
                if import { ui.available_width() - PREVIEW_WIDTH - 12.0 } else { ui.available_width() };
            ui.vertical(|ui| {
                ui.set_width(list);
                egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, false]).show(ui, |ui| {
                    let shown = b.shown();
                    if shown.is_empty() {
                        ui.weak("Nothing to choose here.");
                    }
                    for e in shown {
                        let text = if e.is_pack {
                            format!("📦 {}", e.name)
                        } else if e.is_asset {
                            format!("🧩 {}", e.name)
                        } else if e.is_dir {
                            format!("📁 {}", e.name)
                        } else {
                            e.name.clone()
                        };
                        let selected = draft.selected.as_deref() == Some(e.name.as_str());
                        // Each row's id is its directory's and name's: a row of a new
                        // listing never inherits a click begun on the old one.
                        let r = ui.push_id((&dir, &e.name), |ui| ui.selectable_label(selected, text)).inner;
                        // The accessible name is the entry's own.
                        r.widget_info(|| {
                            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &e.name)
                        });
                        // A click selects; a double click (or Choose) picks, which
                        // enters a folder (a single click entering one made a
                        // double click act twice: batch B review).
                        if r.double_clicked() {
                            out.push(EditorAction::Pick(e.name.clone()));
                        } else if r.clicked() {
                            draft.selected = Some(e.name.clone());
                            out.push(EditorAction::PreviewImage(e.name.clone()));
                        }
                    }
                });
            });
            if import {
                preview(ui, b, draft);
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
