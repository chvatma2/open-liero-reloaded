//! The update screen (issue #4): the offered release's title, version and
//! notes, and its Update; then the install's progress, a clear error, and
//! Restart. Nothing installs but on Update, and nothing is forced: Back
//! returns to the menu and the game plays on as it is.

use egui::{Color32, ProgressBar, RichText, ScrollArea, Ui};

use super::{MenuData, footer, page, primary, section};
use crate::state::{UiAction, UiState};
use crate::update::{UpdateMethod, UpdateStatus};

/// The page's column.
const WIDTH: f32 = 860.0;

/// The screen's title.
pub const TITLE: &str = "Update";
/// Main's notice, before the version.
pub const NOTICE: &str = "Update available:";
/// Main's notice once the update is installed, before the version.
pub const INSTALLED_NOTICE: &str = "Restart to finish the update to";
/// The release page's button.
pub const OPEN_PAGE: &str = "Open the release page";
/// Said once the update is installed.
pub const INSTALLED: &str =
    "The update is installed. It starts with the next launch: restart now, or keep playing this one.";
/// Said before Update is pressed.
pub const IDLE: &str = "Nothing changes until you press Update; you can keep playing this version.";
/// The restart's button.
pub const RESTART: &str = "Restart now";

const RED: Color32 = Color32::from_rgb(255, 110, 90);
const GREEN: Color32 = Color32::from_rgb(140, 220, 140);

pub(super) fn show(ui: &mut Ui, _state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let mut ui = page(ui, TITLE, WIDTH, out);
    let ui = &mut ui;
    let u = &data.update;
    let Some(offer) = &u.offer else {
        ui.label("This version is the newest in your channel.");
        return;
    };
    let mut body = footer(ui, "update", |ui| match (&offer.method, &u.status) {
        (_, UpdateStatus::Installed) => {
            ui.label(RichText::new(INSTALLED).color(GREEN));
            ui.add_space(4.0);
            if primary(ui, RESTART, true).clicked() {
                out.push(UiAction::RestartGame);
            }
        }
        (_, UpdateStatus::NotWritable(why)) => {
            ui.label(RichText::new(why).color(RED));
            ui.add_space(4.0);
            if primary(ui, OPEN_PAGE, !offer.page.is_empty()).clicked() {
                out.push(UiAction::OpenUrl(offer.page.clone()));
            }
        }
        (_, UpdateStatus::Outside { why, command }) => {
            ui.label(why);
            command_line(ui, command);
        }
        (UpdateMethod::Command(command), _) => {
            ui.label(
                "This game was installed from a Flatpak bundle that does not update. Install the \
                     update from the game's repository (this also removes the old build) with:",
            );
            command_line(ui, command);
        }
        (UpdateMethod::Page, _) => {
            ui.label("This build cannot install updates itself: download the new one from its page.");
            ui.add_space(4.0);
            if primary(ui, OPEN_PAGE, !offer.page.is_empty()).clicked() {
                out.push(UiAction::OpenUrl(offer.page.clone()));
            }
        }
        (UpdateMethod::Portable | UpdateMethod::Portal, status) => {
            let working = matches!(status, UpdateStatus::Working { .. });
            if *status == UpdateStatus::Idle {
                ui.label(RichText::new(IDLE).weak());
                ui.add_space(4.0);
            }
            if primary(ui, TITLE, !working).clicked() {
                out.push(UiAction::InstallUpdate);
            }
            ui.add_space(4.0);
            match status {
                UpdateStatus::Working { step, progress } => {
                    match progress {
                        Some(p) => {
                            ui.add(ProgressBar::new(*p).desired_width(320.0).show_percentage());
                        }
                        None => {
                            ui.spinner();
                        }
                    }
                    ui.label(step);
                }
                UpdateStatus::Failed(why) => {
                    ui.label(RichText::new(why).color(RED));
                }
                _ => {}
            }
        }
    });
    let ui = &mut body;
    ui.label(RichText::new(&offer.title).size(22.0).strong());
    let you = if data.version.is_empty() { String::new() } else { format!(" (you have {})", data.version) };
    ui.label(format!("Version {}{you}", offer.version));
    if !offer.page.is_empty() && !matches!(offer.method, UpdateMethod::Page) && ui.link(OPEN_PAGE).clicked() {
        out.push(UiAction::OpenUrl(offer.page.clone()));
    }
    ui.add_space(6.0);
    section(ui, "Release notes", |ui, _| {
        ScrollArea::vertical().id_salt("update notes").auto_shrink([false, true]).show(ui, |ui| {
            let notes = if offer.notes.is_empty() { "No notes." } else { offer.notes.as_str() };
            ui.add(egui::Label::new(notes).selectable(true).wrap());
        });
    });
}

/// A command to run, selectable, with Copy.
pub(super) fn command_line(ui: &mut Ui, command: &str) {
    ui.add(egui::Label::new(RichText::new(command).monospace()).selectable(true).wrap());
    if ui.button("Copy").clicked() {
        ui.ctx().copy_text(command.to_string());
    }
}
