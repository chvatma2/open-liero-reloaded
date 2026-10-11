//! The replays: the matches saved automatically and any file a player put
//! in the data directory's `replays/`, or a file by its path; played with
//! the game's replay session.

use std::path::PathBuf;

use egui::{RichText, ScrollArea, Ui};

use super::{MenuData, header, message};
use crate::state::{UiAction, UiState};

pub(super) fn show(ui: &mut Ui, state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    header(ui, "Replays", out);
    ui.horizontal(|ui| {
        ui.label("File");
        ui.add(
            egui::TextEdit::singleline(&mut data.replay_path).desired_width(420.0).hint_text("a .rlrp path"),
        );
        let path = data.replay_path.trim();
        if ui.add_enabled(!path.is_empty(), egui::Button::new("Play file")).clicked() {
            out.push(UiAction::PlayReplay(PathBuf::from(path)));
        }
        if ui.button("Refresh").clicked() {
            data.refresh_replays();
        }
    });
    if state.is_starting() {
        ui.label("Loading…");
    }
    message(ui, state);
    ui.separator();
    match &data.replays_dir {
        None => {
            ui.label(RichText::new("No data directory: no replays are saved.").weak());
        }
        Some(dir) if data.replays.is_empty() => {
            ui.label(RichText::new(format!("No replays in {} yet.", dir.display())).weak());
        }
        Some(_) => {
            ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("replays").num_columns(2).striped(true).spacing([24.0, 4.0]).show(ui, |ui| {
                    for r in &data.replays {
                        if ui.button(&r.name).clicked() {
                            out.push(UiAction::PlayReplay(r.path.clone()));
                        }
                        ui.label(RichText::new(format!("{} KiB", r.bytes.div_ceil(1024))).weak());
                        ui.end_row();
                    }
                });
            });
        }
    }
}
