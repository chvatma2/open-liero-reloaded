//! Import OLX content (plan 7 task 7): an OpenLieroX game directory typed
//! in, imported into the user data's `packs/` by the app off the UI thread
//! (`crate::import`), with its progress, a Cancel, and the report: each
//! item's pack or error and its warnings, as `import-report.ron` lists
//! them. OLX has no such screen (it reads its game directory directly).

use egui::{Button, Color32, ProgressBar, RichText, ScrollArea, TextEdit, Ui};

use super::{MenuData, footer, form_line, message, page, primary, section};
use crate::import::ImportResult;
use crate::state::{UiAction, UiState};

const ERROR: Color32 = Color32::from_rgb(255, 110, 90);
const WARNING: Color32 = Color32::from_rgb(240, 200, 90);

/// The page's column.
const WIDTH: f32 = 900.0;

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let mut ui = page(ui, "Import OLX content", WIDTH, out);
    let ui = &mut ui;
    let importing = state.is_importing();
    let path = crate::import::clean_typed_path(&data.import_path).to_string();
    // Import, Cancel while it runs, and why it was refused.
    let mut ui = footer(ui, "import", |ui| {
        let can = !importing && !path.is_empty() && data.packs_dir.is_some();
        if primary(ui, "Import", can).clicked() {
            out.push(UiAction::StartImport(path.clone()));
        }
        if importing {
            let p = &data.import;
            let label = if p.cancelling { "Cancelling…" } else { "Cancel" };
            if ui.add_enabled(!p.cancelling, Button::new(label)).clicked() {
                out.push(UiAction::CancelImport);
            }
        }
        message(ui, state);
    });
    let ui = &mut ui;
    section(ui, "OpenLieroX game directory", |ui, _| {
        ui.label(
            "Imports the mods, levels, themes and skins of an OpenLieroX game directory \
             (its share/gamedir, or the folder OpenLieroX is installed in).",
        );
        match &data.packs_dir {
            Some(dir) => ui.label(
                RichText::new(format!(
                    "They go to {} and take the place of bundled packs of the same name.",
                    dir.display()
                ))
                .weak(),
            ),
            None => {
                ui.label(RichText::new("There is no data directory: nothing can be imported.").color(ERROR))
            }
        };
        ui.add_space(6.0);
        form_line(ui, |ui| {
            let label = ui.label("Directory");
            let field = TextEdit::singleline(&mut data.import_path)
                .desired_width(ui.available_width())
                .hint_text("/path/to/OpenLieroX");
            ui.add_enabled(!importing, field).labelled_by(label.id);
        });
        // A relative path is the game's working directory's: say which.
        if !path.is_empty() && !path.starts_with('~') && std::path::Path::new(&path).is_relative() {
            let cwd = std::env::current_dir().map_or_else(|_| "?".into(), |d| d.display().to_string());
            ui.label(RichText::new(format!("Relative to {cwd}")).weak());
        }
    });
    ui.add_space(10.0);
    let p = &data.import;
    if importing {
        // The import reports from its thread: show each report.
        ui.ctx().request_repaint();
        let text = match (&p.current, p.total) {
            (Some(source), total) if total > 0 => format!("{} of {total}: {source}", p.entries.len()),
            _ => "Looking for content…".to_string(),
        };
        ui.add(ProgressBar::new(p.fraction()).text(text));
    }
    match &p.result {
        Some(ImportResult::Done { imported, failed, warnings, tried, total, cancelled }) => {
            let mut text = format!("Imported {imported} packs: {failed} failed, {warnings} warnings.");
            if *cancelled {
                text.push_str(&format!(" Cancelled after {tried} of {total}."));
            }
            ui.label(RichText::new(text).strong());
        }
        Some(ImportResult::Failed(why)) => {
            ui.label(RichText::new(format!("Not imported: {why}")).color(ERROR));
        }
        None => {}
    }
    if let Some(dir) = &p.gamedir {
        ui.label(RichText::new(format!("From {}", dir.display())).weak());
    }
    for n in &p.notes {
        ui.label(RichText::new(n).weak());
    }
    if p.entries.is_empty() {
        return;
    }
    ui.separator();
    // One line a row (an item, then each of its warnings), laid out only
    // in view: a full OLX import reports over 400 items.
    let lines: Vec<(&str, &str, Color32)> = p
        .entries
        .iter()
        .flat_map(|e| {
            let (text, colour) = match (&e.pack, &e.error) {
                (_, Some(err)) => (err.as_str(), ERROR),
                (Some(pack), None) => (pack.as_str(), ui.visuals().weak_text_color()),
                (None, None) => ("", ui.visuals().text_color()),
            };
            std::iter::once((e.source.as_str(), text, colour))
                .chain(e.warnings.iter().map(|w| ("", w.as_str(), WARNING)))
        })
        .collect();
    let row = ui.text_style_height(&egui::TextStyle::Body);
    ScrollArea::vertical().id_salt("import report").auto_shrink([false, true]).show_rows(
        ui,
        row,
        lines.len(),
        |ui, range| {
            for &(source, text, colour) in &lines[range] {
                ui.horizontal(|ui| {
                    let left = egui::Layout::left_to_right(egui::Align::Center);
                    ui.allocate_ui_with_layout(egui::vec2(SOURCE_WIDTH, row), left, |ui| {
                        ui.set_min_width(SOURCE_WIDTH);
                        ui.add(egui::Label::new(source).truncate());
                    });
                    ui.label(RichText::new(text).color(colour));
                });
            }
        },
    );
}

/// The report's source column.
const SOURCE_WIDTH: f32 = 280.0;
