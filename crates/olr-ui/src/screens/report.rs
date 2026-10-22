//! Report a bug (issue #3): what happened and what the player expected,
//! "Attach a replay" with a picker of the saved replays (newest first),
//! the preview of exactly what is sent (the logs collapsed), and Send.
//! The report is made once a frame from `MenuData::report`, and that one
//! value is both shown and sent: the preview never differs from the
//! payload.

use egui::{Color32, RichText, ScrollArea, TextEdit, Ui};

use super::{MenuData, footer, page, primary, section};
use crate::report::{MAX_TEXT_CHARS, SendStatus};
use crate::state::{UiAction, UiState};

/// The page's column.
const WIDTH: f32 = 900.0;

/// The screen's title (and Main's button).
pub const TITLE: &str = "Report a bug";
/// What the player is told before sending.
pub const NOTICE: &str = "Nothing is sent until you press Send. The report becomes a public issue in the \
     game's GitHub repository, exactly as previewed below; nobody can reply to you through the game.";
/// The success line, before the issue's address.
pub const THANKS: &str = "Thanks, report filed:";

const RED: Color32 = Color32::from_rgb(255, 110, 90);

pub(super) fn show(ui: &mut Ui, _state: &UiState, data: &mut MenuData, out: &mut Vec<UiAction>) {
    let mut ui = page(ui, TITLE, WIDTH, out);
    let ui = &mut ui;
    // One report this frame: the preview below and Send carry the same.
    let report = data.report.report();
    let r = &mut data.report;
    let sending = r.status == SendStatus::Sending;
    let ready = report.as_ref().is_some_and(|x| x.is_ready()) && !sending;
    let mut body = footer(ui, "report", |ui| {
        if primary(ui, "Send", ready).clicked()
            && let Some(rep) = report.clone()
        {
            out.push(UiAction::SendReport(Box::new(rep)));
        }
        ui.add_space(4.0);
        match &r.status {
            // Each on a line of its own: the footer centres its lines.
            SendStatus::Sending => {
                ui.spinner();
                ui.label("Sending…");
            }
            SendStatus::Sent(url) => {
                ui.label(RichText::new(THANKS).color(Color32::from_rgb(140, 220, 140)));
                ui.add(egui::Label::new(RichText::new(url).monospace()).selectable(true));
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(url.clone());
                }
            }
            SendStatus::Failed(why) => {
                ui.label(RichText::new(why).color(RED));
            }
            // Until then the preview says it is gathering, and the field
            // that Send needs says it is required.
            SendStatus::Idle => {}
        }
    });
    let ui = &mut body;
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.label(RichText::new(NOTICE).weak());
        ui.add_space(6.0);
        section(ui, "What happened", |ui, title| {
            let field = TextEdit::multiline(&mut r.happened)
                .hint_text("What happened (required)")
                .char_limit(MAX_TEXT_CHARS)
                .desired_rows(4)
                .desired_width(f32::INFINITY);
            ui.add(field).labelled_by(title);
        });
        ui.add_space(6.0);
        section(ui, "What you expected", |ui, title| {
            let field = TextEdit::multiline(&mut r.expected)
                .hint_text("What you expected instead")
                .char_limit(MAX_TEXT_CHARS)
                .desired_rows(2)
                .desired_width(f32::INFINITY);
            ui.add(field).labelled_by(title);
        });
        ui.add_space(6.0);
        section(ui, "Replay", |ui, _| {
            ui.checkbox(&mut r.attach, "Attach a replay");
            if !r.attach {
                return;
            }
            if r.replays.is_empty() {
                ui.label(RichText::new("No saved replays.").weak());
                return;
            }
            ScrollArea::vertical().id_salt("report replays").max_height(160.0).show(ui, |ui| {
                for (i, c) in r.replays.iter().enumerate() {
                    let line = c.info.line();
                    match &c.bytes {
                        Ok(_) => {
                            if ui.radio(r.selected == Some(i), line).clicked() {
                                r.selected = Some(i);
                            }
                        }
                        Err(why) => {
                            ui.add_enabled(false, egui::RadioButton::new(false, line));
                            ui.label(RichText::new(format!("{}: {why}", c.info.file)).color(RED).small());
                        }
                    }
                }
                crate::pad::follow_focus(ui);
            });
        });
        ui.add_space(6.0);
        section(ui, "Preview: what will be sent", |ui, _| {
            let Some(rep) = &report else {
                ui.label("Gathering the system's details…");
                return;
            };
            if let Some(c) = &r.context {
                for w in &c.warnings {
                    ui.label(RichText::new(w).color(RED));
                }
            }
            ui.add(egui::Label::new(RichText::new(rep.preview()).monospace().size(13.0)).selectable(true));
            for (i, log) in rep.logs.iter().enumerate() {
                egui::CollapsingHeader::new(format!("{} (log)", log.name)).id_salt(("report log", i)).show(
                    ui,
                    |ui| {
                        ScrollArea::vertical().id_salt(("report log text", i)).max_height(260.0).show(
                            ui,
                            |ui| {
                                let text = RichText::new(&log.text).monospace().size(12.0);
                                ui.add(egui::Label::new(text).selectable(true));
                            },
                        );
                    },
                );
            }
        });
        crate::pad::follow_focus(ui);
    });
}
