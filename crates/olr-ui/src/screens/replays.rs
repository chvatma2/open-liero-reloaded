//! The replays: the matches saved automatically and any file a player put
//! in the data directory's `replays/`, or a file by its path; played with
//! the game's replay session.

use std::path::PathBuf;

use egui::{Color32, RichText, ScrollArea, Ui};

use super::{MenuData, form_line, message, page, section};
use crate::state::{UiAction, UiState};

/// The page's column.
const WIDTH: f32 = 860.0;

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let mut ui = page(ui, "Replays", WIDTH, out);
    let ui = &mut ui;
    section(ui, "Play a file", |ui, _| {
        let mut play = false;
        form_line(ui, |ui| {
            let label = ui.label("File");
            let w = (ui.available_width() - 100.0).max(80.0);
            let field =
                egui::TextEdit::singleline(&mut data.replay_path).desired_width(w).hint_text("a .rlrp path");
            ui.add(field).labelled_by(label.id);
            let path = data.replay_path.trim();
            play = ui.add_enabled(!path.is_empty(), egui::Button::new("Play file")).clicked();
        });
        if play {
            out.push(UiAction::PlayReplay(PathBuf::from(data.replay_path.trim())));
        }
        if state.is_starting() {
            ui.label("Loading…");
        }
        message(ui, state);
    });
    ui.add_space(10.0);
    section(ui, "Saved", |ui, _| {
        if ui.button("Refresh").clicked() {
            data.refresh_replays();
        }
        ui.add_space(4.0);
        match &data.replays_dir {
            None => {
                ui.label(RichText::new("No data directory: no replays are saved.").weak());
            }
            Some(dir) if data.replays.is_empty() => {
                ui.label(RichText::new(format!("No replays in {} yet.", dir.display())).weak());
            }
            Some(_) => {
                ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    egui::Grid::new("replays").num_columns(3).striped(true).spacing([24.0, 4.0]).show(
                        ui,
                        |ui| {
                            for r in &data.replays {
                                // A file whose header is wrong says why, up front.
                                let button = egui::Button::new(&r.name);
                                if ui.add_enabled(r.problem.is_none(), button).clicked() {
                                    out.push(UiAction::PlayReplay(r.path.clone()));
                                }
                                ui.label(RichText::new(format!("{} KiB", r.bytes.div_ceil(1024))).weak());
                                match &r.problem {
                                    Some(p) => {
                                        ui.label(RichText::new(p).color(Color32::from_rgb(255, 110, 90)))
                                    }
                                    None => ui.label(""),
                                };
                                ui.end_row();
                            }
                        },
                    );
                });
            }
        }
    });
}
