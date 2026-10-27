//! The copy panel's rows of issue #84, under #82's and #83's: Id, Live
//! (hidden, disabled with its reason, or a switch: call 2), Starting
//! state, Values (one row per exposed property, Default clearing the
//! copy's own), Copy script, and Distant with its Depth. Each change is a
//! `SetCopy` (one step); the fields commit once.

use egui::Ui;
use olr_data::level::{MAX_DEPTH, MIN_DEPTH};
use olr_format::levels::PlacedCopy;

use super::{EditorData, labels, relabel, slider, text};
use crate::copy_ops::{CopyFacts, LiveOffer, ScriptTarget};
use crate::level_tab::CopyChange;
use crate::state::{EditorAction, EditorState};

/// The rows for copy `c` (not the ground).
pub(super) fn rows(
    ui: &mut Ui,
    state: &EditorState,
    data: &mut EditorData,
    c: &PlacedCopy,
    out: &mut Vec<EditorAction>,
) {
    let Some(f) = state.document().and_then(|d| d.copy_facts(&c.id)) else { return };
    let id = c.id.as_str();
    let set = |out: &mut Vec<EditorAction>, change: CopyChange| {
        out.push(EditorAction::SetCopy { id: id.to_string(), change })
    };
    ui.separator();
    if let Some(new) = text(ui, data, &format!("copy/{id}/id"), labels::COPY_ID, id, false)
        && new != id
    {
        set(out, CopyChange::Id(new));
    }
    ui.weak(labels::ID_HINT);
    live(ui, c, &f, out);
    starting_state(ui, c, &f, out);
    values(ui, data, c, &f, out);
    script(ui, state, c, &f, out);
    distant(ui, data, c, &f, out);
}

/// Live, as call 2 has it.
fn live(ui: &mut Ui, c: &PlacedCopy, f: &CopyFacts, out: &mut Vec<EditorAction>) {
    let mut on = c.live;
    match &f.live {
        LiveOffer::Hidden => {}
        LiveOffer::Disabled(why) => {
            ui.add_enabled(false, egui::Checkbox::new(&mut on, labels::LIVE)).on_disabled_hover_text(why);
            ui.weak(why);
        }
        LiveOffer::Offered => {
            let r = ui
                .checkbox(&mut on, labels::LIVE)
                .on_hover_text("the copy moves in play: its states, values and scripts drive it");
            if r.changed() {
                out.push(EditorAction::SetCopy { id: c.id.clone(), change: CopyChange::Live(on) });
            }
            if c.live {
                ui.weak(labels::LIVE_NOT_YET);
            }
        }
    }
}

/// The starting state picker: the asset's (★ its start), then each
/// state; hidden for a one-state asset.
fn starting_state(ui: &mut Ui, c: &PlacedCopy, f: &CopyFacts, out: &mut Vec<EditorAction>) {
    if f.states.len() < 2 {
        return;
    }
    let asset = labels::asset_start(&f.start);
    let shown = c.state.clone().unwrap_or_else(|| asset.clone());
    let mut chosen = None;
    ui.horizontal(|ui| {
        let l = ui.label(labels::STARTING_STATE);
        let r = egui::ComboBox::from_id_salt(("copy-state", &c.id))
            .width(super::parts::picker_width(ui))
            .truncate()
            .selected_text(&shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(c.state.is_none(), &asset).clicked() {
                    chosen = Some(None);
                }
                for s in &f.states {
                    if ui.selectable_label(c.state.as_ref() == Some(s), s).clicked() {
                        chosen = Some(Some(s.clone()));
                    }
                }
            })
            .response
            .labelled_by(l.id);
        relabel(&r, egui::WidgetType::ComboBox, None, labels::STARTING_STATE);
    });
    if let Some(state) = chosen
        && state != c.state
    {
        out.push(EditorAction::SetCopy { id: c.id.clone(), change: CopyChange::State(state) });
    }
}

/// One row per exposed property: its widget by type within its limits,
/// a set row marked, Default clearing it.
fn values(ui: &mut Ui, data: &mut EditorData, c: &PlacedCopy, f: &CopyFacts, out: &mut Vec<EditorAction>) {
    if f.values.is_empty() {
        return;
    }
    ui.label(egui::RichText::new(labels::VALUES).strong());
    for row in &f.values {
        let shown = if row.set { format!("● {}", row.name) } else { row.name.clone() };
        let key = format!("copy/{}/value/{}", c.id, row.name);
        let name = labels::copy_value(&row.name);
        ui.push_id(&key, |ui| {
            let v = super::parts::value_field(
                ui,
                data,
                &key,
                (&shown, &name),
                (row.kind, row.range),
                row.value.as_ref(),
            );
            if let Some(v) = v {
                out.push(EditorAction::SetCopy {
                    id: c.id.clone(),
                    change: CopyChange::Value(row.name.clone(), Some(v)),
                });
            }
            if row.set {
                let r = ui.small_button(labels::DEFAULT_VALUE).on_hover_text("the asset's value again");
                relabel(&r, egui::WidgetType::Button, None, &labels::default_value(&row.name));
                if r.clicked() {
                    out.push(EditorAction::SetCopy {
                        id: c.id.clone(),
                        change: CopyChange::Value(row.name.clone(), None),
                    });
                }
            }
        });
    }
}

/// The copy's script: New or Edit and Remove, and Also run the asset's
/// script (offered while Live).
fn script(ui: &mut Ui, state: &EditorState, c: &PlacedCopy, f: &CopyFacts, out: &mut Vec<EditorAction>) {
    let target = ScriptTarget::Copy(c.id.clone());
    ui.label(egui::RichText::new(labels::COPY_SCRIPT).strong());
    let has = state.document().is_some_and(|d| d.script_of(&target).is_some());
    ui.horizontal_wrapped(|ui| {
        let open = if has { labels::EDIT_SCRIPT } else { labels::NEW_SCRIPT };
        if ui.button(open).on_hover_text(target.file()).clicked() {
            out.push(EditorAction::OpenScript(target.clone()));
        }
        if has && ui.button(labels::REMOVE_SCRIPT).clicked() {
            out.push(EditorAction::RemoveTargetScript(target.clone()));
        }
    });
    if f.default_script {
        let mut on = c.default_script;
        let r = ui
            .add_enabled(c.live, egui::Checkbox::new(&mut on, format!("Also run {}'s script", f.asset_name)))
            .on_disabled_hover_text("only a Live copy runs scripts");
        relabel(&r, egui::WidgetType::Checkbox, Some(on), labels::RUN_DEFAULT_SCRIPT);
        if r.changed() {
            out.push(EditorAction::SetCopy { id: c.id.clone(), change: CopyChange::DefaultScript(on) });
        }
    }
}

/// Distant and its Depth, for an asset of Background images and plain
/// drawn layers; disabled while Live.
fn distant(ui: &mut Ui, data: &mut EditorData, c: &PlacedCopy, f: &CopyFacts, out: &mut Vec<EditorAction>) {
    let Some(refused) = &f.distant else { return };
    let set = |out: &mut Vec<EditorAction>, depth| {
        out.push(EditorAction::SetCopy { id: c.id.clone(), change: CopyChange::Distant(depth) })
    };
    let mut on = c.distant.is_some();
    let r = ui
        .add_enabled(refused.is_none(), egui::Checkbox::new(&mut on, labels::DISTANT))
        .on_hover_text("far behind the level: it scrolls slower and takes less light");
    if let Some(why) = refused {
        r.clone().on_disabled_hover_text(why);
        ui.weak(why);
    }
    if r.changed() {
        set(out, on.then_some(crate::ops::DEFAULT_DEPTH));
    }
    let Some(depth) = c.distant else { return };
    if let Some(v) = slider(
        ui,
        data,
        &format!("copy/{}/depth", c.id),
        labels::DEPTH,
        ("near", "far"),
        depth.into(),
        i64::from(MIN_DEPTH)..=i64::from(MAX_DEPTH),
    ) {
        set(out, Some(v as u8));
    }
}
