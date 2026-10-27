//! A centred modal per `Dialog`.

use egui::Context;

use super::labels;
use crate::notice::LibraryAction;
use crate::state::{Choice, Dialog, EditorAction, EditorState, Pending};

pub(super) fn show(
    ctx: &Context,
    state: &EditorState,
    data: &mut super::EditorData,
    out: &mut Vec<EditorAction>,
) {
    let Some(dialog) = state.dialog() else {
        data.asset_name = None;
        data.new_asset = None;
        data.expose_colour = None;
        return;
    };
    let modal = egui::Modal::new(egui::Id::new("olr-editor-dialog")).show(ctx, |ui| {
        let choose = |out: &mut Vec<EditorAction>, c| out.push(EditorAction::Choose(c));
        match dialog {
            Dialog::Unsaved { then } => {
                // Issue #77: naming the level or the asset.
                let name = |d: Option<&crate::document::Document>| d.map_or(String::new(), |d| d.level().name.clone());
                let (of, before) = match then {
                    Pending::Open(_) => (format!("the level {:?}", name(state.document())), "opening another level"),
                    Pending::New(_) => (format!("the level {:?}", name(state.document())), "making a new level"),
                    Pending::Close => (format!("the level {:?}", name(state.document())), "closing"),
                    Pending::EditAsset { .. } => {
                        (format!("the asset {:?}", name(state.asset_document())), "opening another asset")
                    }
                    Pending::NewAsset(_) => {
                        (format!("the asset {:?}", name(state.asset_document())), "making a new asset")
                    }
                    Pending::CloseAsset => (format!("the asset {:?}", name(state.asset_document())), "closing"),
                    Pending::EditGround => {
                        (format!("the asset {:?}", name(state.asset_document())), "editing the level's ground")
                    }
                    Pending::EditOwn(_) => {
                        (format!("the asset {:?}", name(state.asset_document())), "editing the level's asset")
                    }
                    Pending::Export => (format!("the level {:?}", name(state.document())), "exporting it"),
                };
                ui.label(format!("Save the changes to {of} before {before}?"));
                ui.horizontal(|ui| {
                    if ui.button(labels::SAVE).clicked() {
                        choose(out, Choice::Save);
                    }
                    // Issue #84: an export is of the saved level.
                    if !matches!(then, Pending::Export) && ui.button(labels::DONT_SAVE).clicked() {
                        choose(out, Choice::Discard);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::ArtScale { to } => {
                ui.label(format!(
                    "Art scale {to} rescales the drawn and raster layers (nearest neighbour) and reprocesses the images."
                ));
                ui.horizontal(|ui| {
                    if ui.button("Change").clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::RemoveState { name, .. } => {
                ui.label(format!("Remove {name} and its own pictures?"));
                ui.horizontal(|ui| {
                    if ui.button(labels::REMOVE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::RemoveGroup { id } => {
                ui.label(format!("Remove glow group {id:?}? Its elements stop glowing."));
                ui.horizontal(|ui| {
                    if ui.button(labels::REMOVE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::AssetName { suggested } => {
                let first = data.asset_name.is_none();
                let name = data.asset_name.get_or_insert_with(|| suggested.clone());
                ui.label("Save as asset: its name");
                let l = ui.label(labels::ASSET_NAME);
                let r = ui.add(egui::TextEdit::singleline(name).desired_width(280.0)).labelled_by(l.id);
                if first {
                    r.request_focus();
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    if ui.button(labels::SAVE).clicked() || enter {
                        out.push(EditorAction::NameAsset(name.clone()));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::AssetExists { name, .. } => {
                ui.label(format!("You have an asset named {name:?} already."));
                ui.horizontal(|ui| {
                    if ui.button(labels::REPLACE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::KEEP_BOTH).clicked() {
                        choose(out, Choice::KeepBoth);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::RemoveAsset { name, .. } => {
                ui.label(format!("Remove the asset {name:?} from your assets?"));
                ui.horizontal(|ui| {
                    if ui.button(labels::REMOVE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::NewAsset(start) => {
                let n = data.new_asset.get_or_insert_with(|| start.clone());
                ui.label("New asset: its name, size and art scale");
                let l = ui.label(labels::ASSET_NAME);
                ui.add(egui::TextEdit::singleline(&mut n.name).desired_width(280.0)).labelled_by(l.id);
                let side = 1..=olr_format::levels::MAX_SIDE;
                for (label, v) in [(labels::WIDTH, &mut n.size.0), (labels::HEIGHT, &mut n.size.1)] {
                    ui.horizontal(|ui| {
                        let l = ui.label(label);
                        ui.add(egui::DragValue::new(v).range(side.clone())).labelled_by(l.id);
                    });
                }
                ui.horizontal(|ui| {
                    let l = ui.label(labels::ART_SCALE);
                    ui.add(egui::DragValue::new(&mut n.art_scale).range(1..=olr_format::levels::MAX_ART_SCALE))
                        .labelled_by(l.id);
                });
                ui.horizontal(|ui| {
                    if ui.button(labels::CREATE).clicked() {
                        out.push(EditorAction::MakeAsset(n.clone()));
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::Unexpose { name } => {
                ui.label(format!("Unexpose {name}? Copies' values and scripts using {name} will be errors."));
                ui.horizontal(|ui| {
                    if ui.button(labels::UNEXPOSE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::ExposeColour { property, label } => {
                let first = data.expose_colour.is_none();
                let text = data.expose_colour.get_or_insert_with(|| "#ffffff".into());
                ui.label(format!(
                    "{label}'s glow group takes each pixel's own colour: expose it with a default colour"
                ));
                let l = ui.label(labels::DEFAULT_COLOUR);
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(120.0)).labelled_by(l.id);
                if first {
                    r.request_focus();
                }
                let colour = super::look::parse_hex(text);
                ui.horizontal(|ui| {
                    let expose = ui.add_enabled(colour.is_some(), egui::Button::new(labels::EXPOSE));
                    if let Some(c) = colour
                        && expose.on_disabled_hover_text("a colour is #rrggbb").clicked()
                    {
                        out.push(EditorAction::Expose {
                            property: property.clone(),
                            default: Some(olr_format::assets::ValueSource::Colour(c)),
                        });
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::RemoveScript => {
                ui.label("Remove the asset's script?");
                ui.horizontal(|ui| {
                    if ui.button(labels::REMOVE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::RemoveTargetScript(target) => {
                ui.label(format!("Remove {}?", target.said()));
                ui.horizontal(|ui| {
                    if ui.button(labels::REMOVE).clicked() {
                        choose(out, Choice::Confirm);
                    }
                    if ui.button(labels::CANCEL).clicked() {
                        choose(out, Choice::Cancel);
                    }
                });
            }
            Dialog::LibraryChanged => library_changed(ui, state, data.follow > 0, out),
            Dialog::Error(e) => {
                ui.label(e);
                if ui.button("OK").clicked() {
                    choose(out, Choice::Cancel);
                }
            }
        }
    });
    // Escape or a click on the backdrop cancels, unless a button answered.
    if modal.should_close()
        && !out.iter().any(|a| {
            matches!(
                a,
                EditorAction::Choose(_)
                    | EditorAction::NameAsset(_)
                    | EditorAction::MakeAsset(_)
                    | EditorAction::Expose { .. }
                    | EditorAction::Library(_)
            )
        })
    {
        out.push(EditorAction::Choose(Choice::Cancel));
    }
}

/// The library-change notice (issue #84): the level's assets your
/// library changed, with breaks or without, and those it lacks; Later
/// closes it (File ▾ Library changes… brings it back).
fn library_changed(ui: &mut egui::Ui, state: &EditorState, follow: bool, out: &mut Vec<EditorAction>) {
    use olr_edit::library::Status;
    let act = |out: &mut Vec<EditorAction>, a| out.push(EditorAction::Library(a));
    let name = |key: &str| state.document().and_then(|d| d.asset_name(key)).unwrap_or_else(|| key.to_owned());
    let links = state.library_links();
    // Within the window, whatever its size (the modal's frame aside).
    let room = ui.ctx().content_rect().size() - egui::vec2(48.0, 48.0);
    ui.set_max_width(room.x.clamp(120.0, 560.0));
    ui.add(egui::Label::new(egui::RichText::new(labels::LIBRARY_CHANGED).heading()).wrap());
    let max = (room.y - 120.0).max(60.0);
    egui::ScrollArea::vertical().max_height(max).auto_shrink([false, true]).show(ui, |ui| {
        let plain: Vec<_> = links
            .iter()
            .filter(|l| matches!(&l.status, Status::Changed { breaks } if breaks.is_empty()))
            .collect();
        if !plain.is_empty() {
            ui.strong("Changed");
            for l in &plain {
                let n = name(&l.key);
                ui.add(egui::Label::new(&n).wrap());
                ui.horizontal_wrapped(|ui| {
                    if named(ui, "Update", &labels::update(&n)) {
                        act(out, LibraryAction::Update(l.key.clone()));
                    }
                });
            }
        }
        let breaking: Vec<_> = links
            .iter()
            .filter_map(|l| match &l.status {
                Status::Changed { breaks } if !breaks.is_empty() => Some((l, breaks)),
                _ => None,
            })
            .collect();
        if !breaking.is_empty() {
            ui.strong("Changed, breaking copies");
            for (l, breaks) in breaking {
                let n = name(&l.key);
                ui.add(egui::Label::new(&n).wrap());
                ui.horizontal_wrapped(|ui| {
                    if named(ui, labels::UPDATE_ANYWAY, &labels::update_anyway(&n)) {
                        act(out, LibraryAction::UpdateAnyway(l.key.clone()));
                    }
                });
                for b in breaks {
                    ui.add(egui::Label::new(egui::RichText::new(b.to_string()).weak()).wrap());
                }
            }
        }
        let missing: Vec<_> = links.iter().filter(|l| l.status == Status::Missing).collect();
        if !missing.is_empty() {
            ui.strong("Not in your library");
            for l in missing {
                let n = name(&l.key);
                ui.add(egui::Label::new(&n).wrap());
                ui.horizontal_wrapped(|ui| {
                    if named(ui, labels::RESTORE_ASSET, &labels::restore_asset(&n)) {
                        act(out, LibraryAction::Restore(l.key.clone()));
                    }
                    if named(ui, labels::ADOPT, &labels::adopt(&n)) {
                        act(out, LibraryAction::Adopt(l.key.clone()));
                    }
                });
            }
        }
        super::asset::follow_focus(ui, follow);
    });
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        let plain =
            links.iter().any(|l| matches!(&l.status, Status::Changed { breaks } if breaks.is_empty()));
        if ui.add_enabled(plain, egui::Button::new(labels::UPDATE_ALL)).clicked() {
            act(out, LibraryAction::UpdateAll);
        }
        if ui.button(labels::LATER).clicked() {
            out.push(EditorAction::Choose(Choice::Cancel));
        }
    });
}

/// A button reading `text`, named `name` (one per entry).
fn named(ui: &mut egui::Ui, text: &str, name: &str) -> bool {
    let r = ui.button(text);
    super::relabel(&r, egui::WidgetType::Button, None, name);
    r.clicked()
}
