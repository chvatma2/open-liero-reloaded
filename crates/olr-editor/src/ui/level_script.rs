//! The level tab's script editor (issue #84): a copy's script
//! (`editor/scripts/<id>.rhai`) or the level's (`editor/level.rhai`) in
//! the middle instead of the canvas, with Back to level, the file's name,
//! Remove script and the syntax errors; the asset tab's editor and its
//! draft (`parts::ScriptDraft`), committed as `SetTargetScript`.

use egui::Ui;

use super::{EditorData, labels, parts, relabel};
use crate::copy_ops::ScriptTarget;
use crate::state::{EditorAction, EditorState};

/// Follows the open script into the draft: a draft switching scripts
/// first commits what it holds to the script it was of (a renamed copy's
/// keeps its draft), then follows the new one.
pub(super) fn sync(ui: &Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let open = state.script_open().cloned();
    let script_of = |t: &ScriptTarget| state.document().and_then(|d| d.script_of(t)).map(str::to_owned);
    if data.level_script_of != open {
        let d = &mut data.level_script;
        let old = data.level_script_of.take();
        let old_gone = old.as_ref().is_none_or(|t| script_of(t).is_none());
        let renamed = old.is_some() && old_gone && open.as_ref().and_then(script_of) == d.seen_text();
        if !renamed {
            if let Some(old) = old
                && !old_gone
                && d.pending()
            {
                out.push(EditorAction::SetTargetScript { target: old, text: d.text().to_owned() });
            }
            *d = parts::ScriptDraft::default();
        }
        data.level_script_of = open.clone();
    }
    let Some(target) = open else { return };
    let script = script_of(&target);
    parts::sync_draft(
        ui,
        &mut data.level_script,
        script,
        true,
        |text| EditorAction::SetTargetScript { target, text },
        out,
    );
}

/// The middle while a script is open.
pub(super) fn show(ui: &mut Ui, state: &EditorState, data: &mut EditorData, out: &mut Vec<EditorAction>) {
    let Some(target) = state.script_open().cloned() else { return };
    egui::Frame::central_panel(ui.style()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.button(labels::BACK_TO_LEVEL).on_hover_text("the canvas again").clicked() {
                out.push(EditorAction::CloseScript);
            }
            ui.strong(target.file());
            if ui.button(labels::REMOVE_SCRIPT).clicked() {
                out.push(EditorAction::RemoveTargetScript(target.clone()));
            }
        });
        let d = &mut data.level_script;
        let issues = d.issues().to_vec();
        if issues.is_empty() {
            ui.weak(labels::NO_SCRIPT_ERRORS);
        }
        for i in &issues {
            let text = labels::script_error(i);
            let r = ui
                .add(egui::Button::new(egui::RichText::new(&text).color(ui.visuals().error_fg_color)).wrap());
            relabel(&r, egui::WidgetType::Button, None, &text);
            if r.on_hover_text("show it in the script").clicked() {
                d.go_to(i.line, i.column);
            }
        }
        ui.separator();
        let t = target.clone();
        parts::script_text(ui, d, out, move |text| EditorAction::SetTargetScript { target: t.clone(), text });
    });
}
