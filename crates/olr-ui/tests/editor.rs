//! The Editor screen (plan 14c Task 3): the level packs to edit or copy,
//! New level, the recoveries; the Skin tab's skins to edit or copy and New
//! blank skin (issue #14); each asks the app through an `AppCommand`.

use std::collections::BTreeSet;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_content::{Catalog, LevelInfo, PackSource, SkinInfo};
use olr_data::manifest::PackKind;
use olr_ui::screens::{self, MenuData};
use olr_ui::{AppCommand, Profiles, Screen, Settings, UiAction, UiState};

fn level(id: &str, name: &str) -> LevelInfo {
    LevelInfo { id: id.into(), name: name.into(), width: 504, height: 350, thumb: None }
}

fn source(id: &str, kind: PackKind, name: &str) -> PackSource {
    PackSource { id: id.into(), kind, name: name.into(), origin: None }
}

fn catalog() -> Catalog {
    Catalog {
        levels: vec![
            level("my-level", "My Level"),
            level("olx-level-dirt-level", "Dirt Level"),
            level("olx-level-liero-factory", "Liero Factory"),
        ],
        sources: vec![
            source("olx-theme-alpha", PackKind::Theme, "Alpha"),
            source("olx-theme-dirt", PackKind::Theme, "Dirt"),
            source("olx-level-dirt-level", PackKind::Level, "Dirt Level"),
        ],
        skins: vec![
            SkinInfo { id: "my-skin".into(), name: "My Skin".into() },
            SkinInfo { id: "olr-skin-firefly".into(), name: "Firefly".into() },
            SkinInfo { id: "olx-skin-default".into(), name: "default".into() },
        ],
        ..Catalog::default()
    }
}

struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

fn harness(recoveries: Vec<(String, String, bool)>) -> Harness<'static, App> {
    let mut data = MenuData::new(catalog(), None);
    data.set_editor_packs(BTreeSet::from(["my-level".to_string()]), recoveries);
    data.set_editable_skins(BTreeSet::from(["my-skin".to_string()]));
    let app =
        App { ui: UiState::new(Settings::default(), Profiles::default(), 1), data, commands: Vec::new() };
    Harness::builder().with_size(egui::vec2(1280.0, 720.0)).build_ui_state(
        |ui, app: &mut App| {
            for a in screens::show(ui, &app.ui, &mut app.data) {
                let c = app.ui.update(a);
                app.commands.extend(c);
            }
        },
        app,
    )
}

fn open_editor(h: &mut Harness<'static, App>) {
    h.get_by_label("Editor").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Editor);
}

#[test]
fn main_menu_opens_the_editor_screen() {
    let mut h = harness(vec![]);
    open_editor(&mut h);
    h.get_by_label("Back").click();
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Main);
    open_editor(&mut h);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().ui.screen(), Screen::Main);
    assert!(h.state().commands.is_empty(), "{:?}", h.state().commands);
}

#[test]
fn bundled_levels_offer_a_copy_editable_ones_edit() {
    let mut h = harness(vec![]);
    open_editor(&mut h);
    // One Edit (my-level), two copies (the bundled ones, in catalog order).
    assert_eq!(h.query_all_by_label("Edit").count(), 1);
    assert_eq!(h.query_all_by_label("Edit a copy").count(), 2);
    h.get_by_label("Edit").click();
    h.run();
    assert_eq!(h.state().commands, vec![AppCommand::EditLevel("my-level".into())]);
    h.state_mut().commands.clear();
    h.get_all_by_label("Edit a copy").next().unwrap().click();
    h.run();
    // The copy's form, named after the level.
    assert_eq!(h.get_by_label("Name").value().as_deref(), Some("Dirt Level copy"));
    h.get_by_label("Copy").click();
    h.run();
    assert_eq!(
        h.state().commands,
        vec![AppCommand::CopyLevel { from: "olx-level-dirt-level".into(), name: "Dirt Level copy".into() }]
    );
    assert!(h.query_by_label("Copy").is_none(), "the form closed");
}

#[test]
fn new_level_form_emits_new_level() {
    let mut h = harness(vec![]);
    open_editor(&mut h);
    h.get_by_label("New level").click();
    h.run();
    let create_enabled = |h: &Harness<'static, App>| !h.get_by_label("Create").accesskit_node().is_disabled();
    assert!(!create_enabled(&h), "an empty name keeps Create disabled");
    h.get_by_label("Name").click();
    h.run();
    h.get_by_label("Name").type_text("Office");
    h.run();
    for (field, value) in [("Width", "600"), ("Height", "400")] {
        h.get_by_label(field).click();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.get_by_label(field).type_text(value);
        h.run();
    }
    assert!(create_enabled(&h));
    h.get_by_label("Create").click();
    h.run();
    assert_eq!(
        h.state().commands,
        vec![AppCommand::NewLevel {
            name: "Office".into(),
            size: (600, 400),
            theme: "olx-theme-dirt".into()
        }]
    );
    // The form closes once it is sent: a second click makes no second pack.
    assert!(h.query_by_label("Create").is_none(), "the form closed");
    h.get_by_label("New level").click();
    h.run();
    // A side outside 1..=4096 keeps Create disabled.
    h.get_by_label("Width").click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label("Width").type_text("5000");
    h.run();
    assert!(!create_enabled(&h));
}

#[test]
fn recoveries_are_offered_first() {
    let mut h = harness(vec![
        ("my-level".into(), "My Level".into(), false),
        ("broken".into(), "broken".into(), true),
    ]);
    open_editor(&mut h);
    assert_eq!(h.query_all_by_label_contains("Recover").count(), 1, "a damaged one cannot be recovered");
    assert_eq!(h.query_all_by_label("Discard").count(), 2);
    h.get_by_label("Recover My Level").click();
    h.run();
    assert_eq!(h.state().commands, vec![AppCommand::Recover("my-level".into())]);
    h.state_mut().commands.clear();
    h.get_all_by_label("Discard").nth(1).unwrap().click();
    h.run();
    assert_eq!(h.state().commands, vec![AppCommand::DiscardRecovery("broken".into())]);
}

#[test]
fn editor_commands_only_from_the_editor_screen() {
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    assert!(ui.update(UiAction::EditLevel("my-level".into())).is_empty(), "not from Main");
    ui.update(UiAction::Open(Screen::Editor));
    assert_eq!(
        ui.update(UiAction::EditLevel("my-level".into())),
        vec![AppCommand::EditLevel("my-level".into())]
    );
}

/// Issue #14: the Skin tab offers New blank skin, Edit for the user's own
/// skin packs and Edit a copy for the others (never changed), searchable.
#[test]
fn the_skin_tab_edits_yours_and_copies_the_others() {
    let mut h = harness(vec![]);
    open_editor(&mut h);
    assert!(h.query_by_label("New blank skin").is_none(), "the Level tab first");
    h.get_by_label("Skin").click();
    h.run();
    assert!(h.query_by_label("New level").is_none());
    h.get_by_label("New blank skin").click();
    h.run();
    assert_eq!(h.state().commands, vec![AppCommand::NewSkin(None)]);
    h.state_mut().commands.clear();
    assert_eq!(h.query_all_by_label("Edit").count(), 1);
    assert_eq!(h.query_all_by_label("Edit a copy").count(), 2);
    h.get_by_label("Edit").click();
    h.run();
    assert_eq!(h.state().commands, vec![AppCommand::EditSkin("my-skin".into())]);
    h.state_mut().commands.clear();
    h.get_all_by_label("Edit a copy").next().unwrap().click();
    h.run();
    assert_eq!(h.state().commands, vec![AppCommand::NewSkin(Some("olr-skin-firefly".into()))]);
    // The search keeps the skins whose name or id holds it.
    h.get_by_label("Search skins").click();
    h.run();
    h.get_by_label("Search skins").type_text("fire");
    h.run();
    assert_eq!(h.query_all_by_label("Edit a copy").count(), 1);
    assert!(h.query_by_label("Edit").is_none());
    h.get_by_label("Level").click();
    h.run();
    h.get_by_label("New level");
}

#[test]
fn skin_commands_only_from_the_editor_screen() {
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    assert!(ui.update(UiAction::NewSkin(None)).is_empty(), "not from Main");
    ui.update(UiAction::Open(Screen::Editor));
    assert_eq!(ui.update(UiAction::NewSkin(None)), vec![AppCommand::NewSkin(None)]);
    assert_eq!(ui.update(UiAction::EditSkin("a".into())), vec![AppCommand::EditSkin("a".into())]);
}
