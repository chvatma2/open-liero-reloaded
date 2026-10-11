//! The Import screen (plan 7 task 7): the import job off the UI thread on
//! the committed tiny OLX game directory `fixtures/olx-mini`, and the
//! screen's flow through its widgets.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use rl_ui::import::{ImportJob, ImportProgress, ImportResult};
use rl_ui::screens::{self, MenuData};
use rl_ui::{AppCommand, Profiles, Screen, Settings, UiAction, UiState};

fn olx_mini() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/olx-mini")
}

/// Polls `job` until it ends (at most 60 s), as the app does each frame.
fn finish(job: &mut ImportJob, view: &mut ImportProgress) {
    let until = Instant::now() + Duration::from_secs(60);
    while !job.poll(view) {
        assert!(Instant::now() < until, "the import never ended: {view:?}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The names in `dir` starting with `.` (packs left half-written).
fn hidden(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect()
}

#[test]
fn import_runs_off_the_ui_thread_and_reports() {
    let data = tempfile::tempdir().unwrap();
    let packs = data.path().join("packs");
    let mut view = ImportProgress::started();
    let mut job = ImportJob::spawn(olx_mini(), packs.clone());
    finish(&mut job, &mut view);
    let worker = job.worker().expect("the thread reported");
    assert_ne!(worker, std::thread::current().id(), "the import ran on its own thread");
    assert!(!view.running && view.current.is_none());
    assert_eq!(view.gamedir.as_deref(), Some(olx_mini().as_path()));
    assert!(view.notes.iter().any(|n| n.contains(".lxl")), "{:?}", view.notes);
    let Some(ImportResult::Done { imported, failed, warnings, tried, total, cancelled }) =
        view.result.clone()
    else {
        panic!("{view:?}")
    };
    assert_eq!((imported, failed, tried, total, cancelled), (4, 2, 6, 6, false));
    assert_eq!(warnings, 2, "{:?}", view.entries);
    assert_eq!(view.entries.len(), 6);
    let bad = view.entries.iter().find(|e| e.source == "levels/Bad.lxl").unwrap();
    assert!(bad.pack.is_none() && bad.error.is_some(), "{bad:?}");
    assert!(view.entries.iter().any(|e| e.warnings.iter().any(|w| w.contains("boom.wav"))));
    // The packs load.
    let c = rl_content::Content::open(&packs).unwrap();
    assert!(c.catalog().level("olx-level-tiny-level").is_some_and(|l| l.thumb.is_some()));
    assert_eq!(c.pack_origin("olx-tiny-mod").unwrap().revision, rl_ui::import::LOCAL_REVISION);
    assert!(hidden(&packs).is_empty());
    assert!(!job.poll(&mut view), "it ends once");

    // Not an OLX directory, or no directory: the reason, no panic.
    for path in [data.path().to_path_buf(), data.path().join("nowhere")] {
        let mut view = ImportProgress::started();
        let mut job = ImportJob::spawn(path, data.path().join("other"));
        finish(&mut job, &mut view);
        assert!(matches!(&view.result, Some(ImportResult::Failed(_))), "{view:?}");
    }

    // Cancelled at once: it stops between items; every pack written is
    // whole and nothing is left hidden.
    let packs = data.path().join("cancelled");
    let mut view = ImportProgress::started();
    let mut job = ImportJob::spawn(olx_mini(), packs.clone());
    job.cancel();
    finish(&mut job, &mut view);
    let Some(ImportResult::Done { tried, total, cancelled, .. }) = view.result.clone() else {
        panic!("{view:?}")
    };
    assert!(cancelled || tried == total, "{view:?}");
    assert!(hidden(&packs).is_empty());
    if packs.is_dir() {
        let c = rl_content::Content::open(&packs).unwrap();
        for e in view.entries.iter().filter_map(|e| e.pack.as_ref()) {
            assert!(c.pack_dir(e).is_some(), "{e}");
        }
    }

    // A job dropped while it runs (the player quits) stops by itself.
    let job = ImportJob::spawn(olx_mini(), data.path().join("dropped"));
    drop(job);
}

/// The menus and what they asked the app to do.
struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

fn harness(ui: UiState, packs: Option<PathBuf>) -> Harness<'static, App> {
    let mut data = MenuData::new(Default::default(), None);
    data.packs_dir = packs;
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

fn imports(app: &App) -> Vec<&PathBuf> {
    app.commands
        .iter()
        .filter_map(|c| match c {
            AppCommand::StartImport(p) => Some(p),
            _ => None,
        })
        .collect()
}

/// The screen through its widgets: the path typed, Import pressed twice
/// starts one import and remembers the path; no game starts until the
/// import ends; progress, the report and a failure show; the next visit
/// offers the last path.
#[test]
fn import_screen_flow() {
    let mut h =
        harness(UiState::new(Settings::default(), Profiles::default(), 1), Some("/data/packs".into()));
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert_eq!(h.state().ui.screen(), Screen::Import);
    h.get_by_label_contains("/data/packs");
    assert!(h.get_by_label("Import").accesskit_node().is_disabled(), "no path yet");
    h.get_by_role_and_label(Role::TextInput, "Directory").focus();
    h.run_steps(3);
    h.get_by_role_and_label(Role::TextInput, "Directory").type_text("/games/OpenLieroX");
    h.run_steps(3);
    assert_eq!(h.state().data.import_path, "/games/OpenLieroX");
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert!(h.state().ui.is_importing());
    assert!(h.get_by_label("Import").accesskit_node().is_disabled(), "one import at a time");
    h.state_mut().ui.update(UiAction::StartImport("/games/OpenLieroX".into()));
    assert_eq!(imports(h.state()), [&PathBuf::from("/games/OpenLieroX")]);
    assert_eq!(h.state().ui.settings().import.last_path, "/games/OpenLieroX");
    assert!(
        h.state()
            .commands
            .iter()
            .any(|c| matches!(c, AppCommand::SaveSettings(s) if s.import.last_path == "/games/OpenLieroX"))
    );

    // The app's progress shows; Cancel asks once.
    h.state_mut().data.import = ImportProgress {
        running: true,
        total: 6,
        index: 2,
        current: Some("levels/Tiny Level.lxl".into()),
        ..ImportProgress::default()
    };
    h.run_steps(3);
    h.get_by_label_contains("of 6: levels/Tiny Level.lxl");
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(h.state().commands.last(), Some(&AppCommand::CancelImport));

    // Leaving the screen keeps the import running; no game starts
    // meanwhile (local play's Start disabled; the sandbox refused).
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert_eq!(h.state().ui.screen(), Screen::Main);
    h.get_by_label("Sandbox").click();
    h.run_steps(3);
    assert!(!h.state().ui.is_starting());
    h.get_by_label_contains("An import is running");
    h.get_by_label("Local Play").click();
    h.run_steps(3);
    h.get_by_label("Importing content…");
    assert!(h.get_by_label("Start").accesskit_node().is_disabled());
    h.state_mut().ui.update(UiAction::StartMatch);
    assert!(!h.state().ui.is_starting());

    // It ends: the report shows, games start again.
    let entry = |source: &str, pack: Option<&str>, error: Option<&str>, warnings: &[&str]| {
        rl_content::import::ImportEntry {
            source: source.into(),
            pack: pack.map(String::from),
            error: error.map(String::from),
            warnings: warnings.iter().map(|w| w.to_string()).collect(),
        }
    };
    h.state_mut().data.import = ImportProgress {
        entries: vec![
            entry("Tiny Mod", Some("olx-tiny-mod"), None, &["sound boom.wav not found"]),
            entry("levels/Bad.lxl", None, Some("file ends early at byte 11"), &[]),
        ],
        result: Some(ImportResult::Done {
            imported: 1,
            failed: 1,
            warnings: 1,
            tried: 2,
            total: 6,
            cancelled: true,
        }),
        ..ImportProgress::default()
    };
    h.state_mut().ui.update(UiAction::ImportFinished);
    h.run_steps(3);
    assert!(!h.state().ui.is_importing());
    assert!(!h.get_by_label("Start").accesskit_node().is_disabled());
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    h.get_by_label("Import").click();
    h.run_steps(3);
    h.get_by_label_contains("Imported 1 packs: 1 failed, 1 warnings. Cancelled after 2 of 6.");
    h.get_by_label("file ends early at byte 11");
    h.get_by_label("sound boom.wav not found");
    h.get_by_label("olx-tiny-mod");

    // A failure is shown, not panicked on.
    h.state_mut().data.import = ImportProgress::failed("/x is not an OpenLieroX game directory".into());
    h.run_steps(3);
    h.get_by_label_contains("Not imported: /x is not an OpenLieroX game directory");

    // The next run offers the path imported last; without a data
    // directory nothing can be imported.
    let mut settings = Settings::default();
    settings.import.last_path = "/games/OpenLieroX".into();
    let mut h = harness(UiState::new(settings, Profiles::default(), 1), None);
    h.get_by_label("Import").click();
    h.run_steps(3);
    assert_eq!(h.state().data.import_path, "/games/OpenLieroX");
    h.get_by_label_contains("no data directory");
    assert!(h.get_by_label("Import").accesskit_node().is_disabled());
}
