//! Updates (issue #4): the main menu's notice only for an offered release
//! in a build that checks, the Update screen's notes (every skipped
//! release's, issue #22), Update, progress,
//! errors, Restart and the release page, and the options' "Include
//! pre-releases" (saved; under Flatpak the branch's, with the command
//! switching it).

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use olr_content::Catalog;
use olr_ui::screens::{self, MenuData};
use olr_ui::update::{FlatpakChannel, ReleaseNotes, UpdateData, UpdateMethod, UpdateOffer, UpdateStatus};
use olr_ui::{AppCommand, Profiles, Screen, Settings, UiAction, UiState};

struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

pub fn offer(method: UpdateMethod) -> UpdateOffer {
    UpdateOffer {
        version: "0.2.1".into(),
        title: "Open Liero Reloaded 0.2.1".into(),
        changes: vec![ReleaseNotes {
            heading: "Open Liero Reloaded 0.2.1 (v0.2.1)".into(),
            notes: "What's new\n\n• Update from the main menu".into(),
        }],
        left_out: 0,
        page: "https://github.com/o/r/releases/tag/v0.2.1".into(),
        method,
    }
}

fn harness(update: UpdateData) -> Harness<'static, App> {
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let mut data = MenuData::new(Catalog::default(), None);
    data.version = "0.2.0".into();
    data.update = update;
    let app = App { ui, data, commands: Vec::new() };
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

fn settle(h: &mut Harness<'static, App>) {
    h.run();
    h.step();
    h.step();
}

fn enabled(offer: Option<UpdateOffer>) -> UpdateData {
    UpdateData { enabled: true, offer, ..Default::default() }
}

#[test]
fn the_notice_shows_only_an_offer_in_a_checking_build() {
    let mut h = harness(enabled(None));
    settle(&mut h);
    assert!(h.query_by_label_contains("Update available").is_none());
    // A build that does not check shows nothing, even with an offer.
    let mut h = harness(UpdateData {
        enabled: false,
        offer: Some(offer(UpdateMethod::Portable)),
        ..Default::default()
    });
    settle(&mut h);
    assert!(h.query_by_label_contains("Update available").is_none());
    let mut h = harness(enabled(Some(offer(UpdateMethod::Portable))));
    settle(&mut h);
    h.get_by_label("Update available: 0.2.1").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Update);
    assert!(h.state().commands.is_empty(), "opening installs nothing");
    // The notes, the title, the versions.
    h.get_by_label("Open Liero Reloaded 0.2.1");
    h.get_by_label("Version 0.2.1 (you have 0.2.0)");
    h.get_by_label_contains("Update from the main menu");
    h.get_by_label("Release notes");
    // One release's notes need no heading of their own.
    assert!(h.query_by_label("Open Liero Reloaded 0.2.1 (v0.2.1)").is_none());
    assert!(h.query_by_label("Changes since your version").is_none());
    // Not forced: Back returns to the menu.
    h.get_by_label("Back").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Main);
    assert!(h.state().commands.is_empty());
}

#[test]
fn update_installs_then_offers_a_restart() {
    let mut h = harness(enabled(Some(offer(UpdateMethod::Portable))));
    h.state_mut().ui.update(UiAction::Open(Screen::Update));
    settle(&mut h);
    h.get_by_label_contains("Nothing changes until you press Update");
    h.get_by_role_and_label(Role::Button, "Update").click();
    settle(&mut h);
    assert_eq!(h.state().commands, [AppCommand::InstallUpdate]);
    h.state_mut().commands.clear();
    // While it works: the step and its progress; Update is disabled.
    h.state_mut().data.update.status =
        UpdateStatus::Working { step: "Downloading 4.0 of 23.1 MB".into(), progress: Some(0.17) };
    settle(&mut h);
    h.get_by_label("Downloading 4.0 of 23.1 MB");
    h.get_by_role_and_label(Role::Button, "Update").click();
    settle(&mut h);
    assert!(h.state().commands.is_empty());
    // A failure says why; Update tries again.
    h.state_mut().data.update.status = UpdateStatus::Failed("The download's signature is bad.".into());
    settle(&mut h);
    h.get_by_label("The download's signature is bad.");
    h.get_by_role_and_label(Role::Button, "Update").click();
    settle(&mut h);
    assert_eq!(h.state().commands, [AppCommand::InstallUpdate]);
    h.state_mut().commands.clear();
    // Installed: Restart.
    h.state_mut().data.update.status = UpdateStatus::Installed;
    settle(&mut h);
    h.get_by_label_contains("The update is installed");
    h.get_by_label("Restart now").click();
    settle(&mut h);
    assert_eq!(h.state().commands, [AppCommand::RestartGame]);
    // The main menu says a restart finishes it, no longer "available".
    h.get_by_label("Back").click();
    settle(&mut h);
    assert!(h.query_by_label_contains("Update available").is_none());
    h.get_by_label("Restart to finish the update to 0.2.1").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Update);
}

/// Three development builds skipped, newest first.
fn three_builds() -> UpdateOffer {
    let notes = |n: u32, text: &str| ReleaseNotes {
        heading: format!("0.2.0 development build {n} (v0.2.0-dev.{n})"),
        notes: text.into(),
    };
    UpdateOffer {
        version: "0.2.0-dev.320".into(),
        title: "0.2.0 development build 320".into(),
        changes: vec![notes(320, "• Faster menus"), notes(319, ""), notes(318, "• The mod editor")],
        page: "https://github.com/o/r/releases/tag/v0.2.0-dev.320".into(),
        ..offer(UpdateMethod::Portable)
    }
}

#[test]
fn every_skipped_release_shows_under_its_heading_newest_first() {
    let mut h = harness(enabled(Some(three_builds())));
    h.state_mut().ui.update(UiAction::Open(Screen::Update));
    settle(&mut h);
    h.get_by_label("Changes since your version");
    assert!(h.query_by_label("Release notes").is_none());
    let top = |h: &Harness<'static, App>, label: &str| h.get_by_label(label).rect().top();
    let (a, b, c) = (
        top(&h, "0.2.0 development build 320 (v0.2.0-dev.320)"),
        top(&h, "0.2.0 development build 319 (v0.2.0-dev.319)"),
        top(&h, "0.2.0 development build 318 (v0.2.0-dev.318)"),
    );
    assert!(a < b && b < c, "{a} {b} {c}");
    // Each its notes below its heading; none said so.
    let faster = top(&h, "• Faster menus");
    let none = top(&h, "No notes.");
    let editor = top(&h, "• The mod editor");
    assert!(a < faster && faster < b && b < none && none < c && c < editor);
    assert!(h.query_by_label_contains("not shown").is_none());
}

#[test]
fn releases_over_the_caps_are_counted_with_the_release_page() {
    let mut offer = three_builds();
    offer.left_out = 7;
    let mut h = harness(enabled(Some(offer)));
    h.state_mut().ui.update(UiAction::Open(Screen::Update));
    settle(&mut h);
    let line = h.get_by_label("7 older releases are not shown").rect();
    // The title's link, and the one beside the line.
    let link = h.get_all_by_label("Open the release page").last().unwrap();
    assert!((link.rect().center().y - line.center().y).abs() < 4.0, "beside the line");
    link.click();
    settle(&mut h);
    assert_eq!(
        h.state().commands,
        [AppCommand::OpenUrl("https://github.com/o/r/releases/tag/v0.2.0-dev.320".into())]
    );
    assert_eq!(olr_ui::screens::update::not_shown(1), "1 older release is not shown");
    // One release shown and others left out: still the changes.
    let mut offer = three_builds();
    offer.changes.truncate(1);
    offer.left_out = 2;
    let mut h = harness(enabled(Some(offer)));
    h.state_mut().ui.update(UiAction::Open(Screen::Update));
    settle(&mut h);
    h.get_by_label("Changes since your version");
    h.get_by_label("0.2.0 development build 320 (v0.2.0-dev.320)");
    h.get_by_label("2 older releases are not shown");
}

#[test]
fn an_unwritable_folder_offers_the_release_page() {
    let mut h = harness(enabled(Some(offer(UpdateMethod::Portable))));
    h.state_mut().ui.update(UiAction::Open(Screen::Update));
    h.state_mut().data.update.status =
        UpdateStatus::NotWritable("The game's folder cannot be written (access denied).".into());
    settle(&mut h);
    h.get_by_label_contains("cannot be written");
    h.get_by_role_and_label(Role::Button, "Open the release page").click();
    settle(&mut h);
    assert_eq!(
        h.state().commands,
        [AppCommand::OpenUrl("https://github.com/o/r/releases/tag/v0.2.1".into())]
    );
}

#[test]
fn flatpak_updates_say_what_to_run_when_the_game_cannot() {
    let cmd = "flatpak update --user io.github.openlieroreloaded.OpenLieroReloaded//stable";
    let mut h = harness(enabled(Some(offer(UpdateMethod::Portal))));
    h.state_mut().ui.update(UiAction::Open(Screen::Update));
    h.state_mut().data.update.status = UpdateStatus::Outside {
        why: "This update needs new permissions: update from your software centre or run".into(),
        command: cmd.into(),
    };
    settle(&mut h);
    h.get_by_label(cmd);
    h.get_by_role_and_label(Role::Button, "Copy");
    assert!(h.query_by_role_and_label(Role::Button, "Update").is_none());
    // An older bundle: the command installing from the repository.
    let mut h = harness(enabled(Some(offer(UpdateMethod::Command("flatpak install x".into())))));
    h.state_mut().ui.update(UiAction::Open(Screen::Update));
    settle(&mut h);
    h.get_by_label("flatpak install x");
    assert!(h.query_by_role_and_label(Role::Button, "Update").is_none());
}

#[test]
fn update_commands_come_only_from_the_update_screen() {
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    assert!(ui.update(UiAction::InstallUpdate).is_empty());
    assert!(ui.update(UiAction::RestartGame).is_empty());
    assert!(ui.update(UiAction::OpenUrl("https://x.example".into())).is_empty());
    ui.update(UiAction::Open(Screen::Update));
    assert!(ui.update(UiAction::OpenUrl("file:///etc/passwd".into())).is_empty());
    assert_eq!(
        ui.update(UiAction::OpenUrl("https://x.example".into())),
        [AppCommand::OpenUrl("https://x.example".into())]
    );
    assert_eq!(ui.update(UiAction::InstallUpdate), [AppCommand::InstallUpdate]);
}

#[test]
fn include_prereleases_follows_the_build_until_set() {
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let data = UpdateData { enabled: true, build_prerelease: true, ..Default::default() };
    assert_eq!(ui.settings().updates.include_prereleases, None);
    assert!(data.includes_prereleases(None));
    assert!(!UpdateData::default().includes_prereleases(None));
    let c = ui.update(UiAction::SetIncludePrereleases(false));
    assert!(
        matches!(c.as_slice(), [AppCommand::SaveSettings(s)] if s.updates.include_prereleases == Some(false))
    );
    assert!(ui.update(UiAction::SetIncludePrereleases(false)).is_empty(), "unchanged: nothing saved");
    assert!(!data.includes_prereleases(Some(false)));
    // The setting survives the file.
    let text = ron::to_string(ui.settings()).unwrap();
    let back: Settings = ron::from_str(&text).unwrap();
    assert_eq!(back.updates.include_prereleases, Some(false));
    let old: Settings = ron::from_str("(video: (fullscreen: true))").unwrap();
    assert_eq!(old.updates.include_prereleases, None);
}

fn options_online(h: &mut Harness<'static, App>) {
    h.state_mut().ui.update(UiAction::Open(Screen::Options));
    settle(h);
    h.get_by_label("Online").click();
    settle(h);
}

/// Clicks "Include pre-releases", below the Online tab's lists: scrolled
/// into view first.
fn toggle(h: &mut Harness<'static, App>) {
    h.get_by_role_and_label(Role::CheckBox, "Include pre-releases").scroll_to_me();
    settle(h);
    h.get_by_role_and_label(Role::CheckBox, "Include pre-releases").click();
    settle(h);
}

#[test]
fn the_options_toggle_pre_releases() {
    let mut h = harness(UpdateData { enabled: true, build_prerelease: true, ..Default::default() });
    options_online(&mut h);
    toggle(&mut h);
    settle(&mut h);
    let saved: Vec<_> =
        h.state().commands.iter().filter(|c| matches!(c, AppCommand::SaveSettings(_))).collect();
    assert!(
        matches!(saved.as_slice(), [AppCommand::SaveSettings(s)] if s.updates.include_prereleases == Some(false)),
        "{saved:?}"
    );
    // A build that does not check has no such option.
    let mut h = harness(UpdateData::default());
    options_online(&mut h);
    assert!(h.query_by_label("Include pre-releases").is_none());
}

#[test]
fn under_flatpak_the_toggle_shows_the_switch_command() {
    let switch = "flatpak install --user -y --from https://openlieroreloaded.github.io/flatpak/beta.flatpakref \
                  && flatpak uninstall --user -y io.github.openlieroreloaded.OpenLieroReloaded//stable";
    let data = UpdateData {
        enabled: true,
        flatpak: Some(FlatpakChannel { beta: false, switch_command: switch.into() }),
        ..Default::default()
    };
    let mut h = harness(data);
    options_online(&mut h);
    assert!(h.query_by_label(switch).is_none());
    toggle(&mut h);
    h.get_by_label(switch);
    h.get_by_label_contains("runs from its stable branch");
    assert!(
        !h.state().commands.iter().any(|c| matches!(c, AppCommand::SaveSettings(_))),
        "the branch decides: nothing saved"
    );
    // Toggled back: the command goes.
    toggle(&mut h);
    assert!(h.query_by_label(switch).is_none());
}
