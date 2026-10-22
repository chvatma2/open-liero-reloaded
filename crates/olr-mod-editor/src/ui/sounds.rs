//! The Sounds section (issue #19): every file in `sfx/`, its size and the
//! fields naming it, with ▶ (the host plays it), Rename and Remove
//! (refused while used); Import sound… above. Sounds are not edited here.

use egui::Ui;

use super::{Cx, labels, relabel};
use crate::browser::Import;
use crate::state::ModAction;

pub(super) fn show(ui: &mut Ui, cx: &mut Cx) {
    let state = cx.state;
    ui.heading(labels::SOUNDS);
    ui.horizontal(|ui| {
        let r = ui.button(format!("{}…", labels::IMPORT_SOUND));
        relabel(&r, egui::WidgetType::Button, labels::IMPORT_SOUND);
        if r.on_hover_text("A WAV or Ogg file, at most 60 s and 2 MiB, as the game plays").clicked() {
            cx.out.push(ModAction::Import(Import::Sound));
        }
        ui.weak("A weapon's or an event's Sound field chooses among them.");
    });
    ui.separator();
    let sounds = state.assets("sfx");
    if sounds.is_empty() {
        ui.weak("No sounds yet.");
    }
    egui::Grid::new("olr-mod-editor-sounds").num_columns(3).striped(true).spacing([10.0, 6.0]).show(
        ui,
        |ui| {
            for path in &sounds {
                ui.horizontal(|ui| {
                    let r = ui.small_button("▶");
                    relabel(&r, egui::WidgetType::Button, &labels::play_sound(path));
                    if r.on_hover_text("Play it").clicked() {
                        cx.out.push(ModAction::PlaySound(path.clone()));
                    }
                    let kb = state.asset(path).map_or(0, |b| b.len().div_ceil(1024));
                    ui.label(path.as_str());
                    ui.weak(format!("{kb} KiB"));
                });
                let users = state.asset_users(path);
                if users.is_empty() {
                    ui.weak("not used");
                } else {
                    let list: Vec<&str> = users.iter().map(|u| u.label.as_str()).collect();
                    let shown = list.join(", ");
                    let short: String = shown.chars().take(80).collect();
                    let more = if short.len() < shown.len() { "…" } else { "" };
                    ui.label(format!("{}: {short}{more}", labels::USED_BY)).on_hover_text(shown);
                }
                ui.horizontal(|ui| {
                    let r = ui.small_button(format!("{}…", labels::RENAME_ASSET));
                    relabel(&r, egui::WidgetType::Button, &labels::rename_sound(path));
                    if r.clicked() {
                        cx.out.push(ModAction::AskRename(path.clone()));
                    }
                    let r = ui.small_button(labels::REMOVE_ASSET);
                    relabel(&r, egui::WidgetType::Button, &labels::remove_sound(path));
                    if r.on_hover_text("Refused while a field names it").clicked() {
                        cx.out.push(ModAction::RemoveAsset(path.clone()));
                    }
                });
                ui.end_row();
            }
        },
    );
}
