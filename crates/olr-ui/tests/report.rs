//! Report a bug (issue #3): the main menu's entry only with a relay, the
//! screen's texts, replay picker and preview, Send carrying exactly what
//! the preview shows; the report's assembly fixed for given facts.

use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use olr_content::{Catalog, LevelInfo, ModInfo};
use olr_sim::Keys;
use olr_ui::report::{
    self, DisplayFacts, GpuFacts, LogAttachment, LogEnd, OsFacts, Report, ReportContext, SendStatus,
    SystemFacts, VideoOverrides,
};
use olr_ui::screens::{self, MenuData};
use olr_ui::{AppCommand, FrameRate, MatchPrefs, Profiles, Screen, Settings, UiAction, UiState};

struct App {
    ui: UiState,
    data: MenuData,
    commands: Vec<AppCommand>,
}

fn catalog() -> Catalog {
    Catalog {
        mods: vec![ModInfo {
            id: "olx-classic".into(),
            name: "Liero v1.0".into(),
            presets: vec![],
            ..ModInfo::default()
        }],
        levels: vec![LevelInfo {
            id: "olx-level-dirt-level".into(),
            name: "Dirt Level".into(),
            width: 504,
            height: 350,
            thumb: None,
        }],
        skins: vec![],
        sources: vec![],
        warnings: vec![],
    }
}

fn harness(replays: Option<PathBuf>, available: bool) -> Harness<'static, App> {
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let mut data = MenuData::new(catalog(), replays);
    data.report.available = available;
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

/// Fixed facts: what the app would gather on one machine.
fn facts() -> SystemFacts {
    let mut settings = Settings::default();
    settings.video.frame_rate = FrameRate::Uncapped;
    SystemFacts {
        version: "0.2.0-dev.200.abc1234.dirty".into(),
        os: OsFacts {
            os: "linux".into(),
            arch: "x86_64".into(),
            distro: Some("Ubuntu 24.04.1 LTS".into()),
            kernel: Some("6.6.87.2-microsoft-standard-WSL2".into()),
            window_system: Some("Wayland".into()),
            session: Some("wayland".into()),
            wsl: true,
            flatpak: false,
        },
        gpu: Some(GpuFacts {
            name: "D3D12 (Intel(R) UHD Graphics 620)".into(),
            backend: "Gl".into(),
            device_type: "IntegratedGpu".into(),
            driver: "Mesa".into(),
            driver_info: "24.2.8".into(),
            present_mode: "Fifo".into(),
            format: "Rgba8Unorm".into(),
        }),
        display: Some(DisplayFacts {
            scale_factor: 1.25,
            window: (1280, 720),
            monitor: Some((2560, 1440)),
            refresh_mhz: Some(59_951),
        }),
        settings,
        overrides: VideoOverrides { frame_rate: true, ..VideoOverrides::default() },
    }
}

fn context() -> ReportContext {
    let log = "2026-10-21 14:03:07.123 info  olr: olr 0.2.0\n2026-10-21 14:03:09.000 warn  gpu: slow frame\n";
    let previous = "2026-10-21 13:00:00.000 info  olr: playing\n";
    ReportContext {
        sections: report::sections(&facts()),
        logs: vec![
            LogAttachment::new("This run", "now.log", LogEnd::Running, log.as_bytes(), None),
            LogAttachment::new("The previous run", "before.log", LogEnd::Unclean, previous.as_bytes(), None),
        ],
        warnings: vec![],
    }
}

/// The automatic section for fixed facts, and the preview naming the
/// overridden setting: the assembly is deterministic.
#[test]
fn the_automatic_section_is_fixed_for_given_facts() {
    let r = Report::new("It flickers\nafter a minute", "No flicker", &context(), None);
    let preview = r.preview();
    let expected = "\
What happened
  It flickers
  after a minute

What you expected
  No flicker

Game
  Version: 0.2.0-dev.200.abc1234.dirty

System
  OS: linux x86_64
  Distribution: Ubuntu 24.04.1 LTS
  Kernel: 6.6.87.2-microsoft-standard-WSL2
  Window system: Wayland
  Desktop session: wayland
  WSL: yes
  Flatpak: no

GPU
  Adapter: D3D12 (Intel(R) UHD Graphics 620)
  Backend: Gl
  Device type: IntegratedGpu
  Driver: Mesa
  Driver info: 24.2.8
  Present mode: Fifo
  Surface format: Rgba8Unorm

Display
  Scale factor: 1.25
  Window: 1280 × 720
  Monitor: 2560 × 1440
  Refresh rate: 59.951 Hz

Video settings
  Window size: 1280 × 720
  Fullscreen: no
  Frame rate: Uncapped (set on the command line)
  Upscale filter: Sharp
  Interpolation: on
  UI scale: 1
  HUD layout: Auto

Audio settings
  Master: 70 %
  Effects: 100 %
  Music: 70 %

Gameplay settings
";
    assert!(preview.starts_with(expected), "{preview}");
    assert!(preview.contains("  Local play's match: (mod_id:\"olx-classic\""), "{preview}");
    assert!(preview.ends_with(
        "Replay\n  none attached\n\nLogs\n  This run: now.log, still running, 1 KiB\n  The previous run: before.log, \
         ended unexpectedly (a crash or a kill), 1 KiB\n"
    ));
    // The payload: the same values, keyed.
    let json: serde_json::Value = serde_json::from_str(&r.json()).unwrap();
    assert_eq!(json["format"], 1);
    assert_eq!(json["happened"], "It flickers\nafter a minute");
    assert_eq!(
        json["sections"][3]["facts"][3],
        serde_json::json!({"name": "Refresh rate", "value": "59.951 Hz"})
    );
    assert_eq!(json["logs"][1]["ended"], "unclean");
    assert_eq!(json["replay"], serde_json::Value::Null);
    // What is never in a report: bindings, the online settings, profiles.
    for key in ["input", "online", "profiles", "identity", "relays", "turn"] {
        assert!(!r.json().contains(&format!("\"{key}\"")), "{key}");
    }
    assert_eq!(
        r,
        Report::new("It flickers\nafter a minute", "No flicker", &context(), None),
        "deterministic"
    );
}

/// Every string of the payload shows in the preview or in a log's text:
/// nothing is sent that the player could not read first.
#[test]
fn every_value_sent_is_previewed() {
    let dir = tempfile::tempdir().unwrap();
    let Some(bytes) = recorded_replay(&["Anna", "Bert"]) else { return };
    write_at(&dir.path().join("match-000001.olrp"), &bytes, 100);
    let choices = report::replay_choices(dir.path(), &catalog());
    let r = Report::new("It broke", "", &context(), Some(&choices[0]));
    let mut shown = r.preview();
    for l in &r.logs {
        shown.push_str(&l.text);
    }
    fn strings(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Array(a) => a.iter().for_each(|x| strings(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| strings(x, out)),
            _ => {}
        }
    }
    let mut all = Vec::new();
    strings(&serde_json::from_str(&r.json()).unwrap(), &mut all);
    for s in all.iter().filter(|s| !matches!(s.as_str(), "running" | "unclean")) {
        for line in s.lines() {
            assert!(shown.contains(line), "{line:?} is sent but not previewed");
        }
    }
    assert!(shown.contains("Worms' names in it: Anna, Bert"), "{shown}");
}

/// A recorded Deathmatch of two worms named `names` (Classic on Dirt
/// Level from the committed content), or `None` without the content.
fn recorded_replay(names: &[&str]) -> Option<Vec<u8>> {
    let dir = std::env::var_os("OLR_CONTENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/packs"));
    if !dir.join("olx-classic").is_dir() {
        return None;
    }
    let content = olr_content::Content::open(&dir).unwrap();
    let prefs = MatchPrefs { level_id: "olx-level-dirt-level".into(), bots: 1, ..MatchPrefs::default() };
    let built = content.build_match(&prefs.options(3)).unwrap();
    let (mut world, mut rec) = (built.world, built.recorder);
    rec.set_looks(names.iter().map(|n| olr_replay::WormLook::plain(n, [200, 100, 50])).collect());
    let (keys, mut out) = (vec![Keys(0); world.worms().len()], olr_sim::TickEvents::new());
    for _ in 0..30 {
        world.tick(&keys, &mut out);
        rec.push(&keys, &world).unwrap();
    }
    Some(rec.finish(&world).unwrap().encode())
}

/// Writes `bytes` to `path`, dated `secs` after 2026-10-21 12:00 UTC.
fn write_at(path: &Path, bytes: &[u8], secs: u64) {
    std::fs::write(path, bytes).unwrap();
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(1_792_584_000 + secs)).unwrap();
}

#[test]
fn report_a_bug_shows_only_with_a_relay() {
    let mut h = harness(None, false);
    settle(&mut h);
    h.get_by_label("About");
    assert!(h.query_by_label("Report a bug").is_none(), "no relay: no entry");
    let mut h = harness(None, true);
    settle(&mut h);
    h.get_by_label("Report a bug").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Report);
    assert_eq!(h.state().commands, vec![AppCommand::PrepareReport]);
    // Until the app gathered the details, Send waits.
    h.get_by_label_contains("Nothing is sent until you press Send");
    assert!(h.get_by_label("Send").accesskit_node().is_disabled());
    h.get_by_label("Back").click();
    settle(&mut h);
    assert_eq!(h.state().ui.screen(), Screen::Main);
}

/// The player's flow: text typed, a replay attached (the newest picked,
/// its worms' names in the preview), Send's report is the preview's; a
/// failure keeps the text, a success shows the address and clears it.
#[test]
fn the_report_sent_is_the_one_previewed() {
    let dir = tempfile::tempdir().unwrap();
    let Some(older) = recorded_replay(&["Old One", "Old Two"]) else { return };
    let newer = recorded_replay(&["Anna", "Bert"]).unwrap();
    write_at(&dir.path().join("replay-001.olrp"), &older, 60);
    write_at(&dir.path().join("match-000003.olrp"), &newer, 3 * 60);
    write_at(&dir.path().join("match-000002.olrp"), b"OLRP junk", 2 * 60);
    let mut h = harness(Some(dir.path().to_path_buf()), true);
    h.state_mut().data.report.context = Some(context());
    settle(&mut h);
    h.get_by_label("Report a bug").click();
    settle(&mut h);
    assert!(h.get_by_label("Send").accesskit_node().is_disabled(), "nothing typed yet");
    h.get_by_role_and_label(Role::MultilineTextInput, "What happened").focus();
    settle(&mut h);
    h.get_by_role_and_label(Role::MultilineTextInput, "What happened").type_text("The screen flickers");
    settle(&mut h);
    // The replays, newest first, only once attached.
    assert!(h.query_by_label_contains("Dirt Level").is_none());
    h.get_by_label("Attach a replay").click();
    settle(&mut h);
    let lines: Vec<String> = h.state().data.report.replays.iter().map(|r| r.info.file.clone()).collect();
    assert_eq!(lines, ["match-000003.olrp", "match-000002.olrp", "replay-001.olrp"]);
    assert_eq!(h.state().data.report.selected, Some(0), "the newest preselected");
    h.get_by_label_contains("2026-10-21 12:03 UTC  ·  Liero v1.0  ·  Dirt Level");
    h.get_by_label_contains("match-000002.olrp: not a replay this build reads");
    h.get_by_label_contains("Worms' names in it: Anna, Bert");
    // The older one picked instead.
    h.get_by_label_contains("2026-10-21 12:01 UTC").click();
    settle(&mut h);
    h.get_by_label_contains("Worms' names in it: Old One, Old Two");
    let shown = h.state().data.report.report().unwrap();
    h.get_by_label("Send").click();
    settle(&mut h);
    let sent: Vec<&Report> = h
        .state()
        .commands
        .iter()
        .filter_map(|c| match c {
            AppCommand::SendReport(r) => Some(&**r),
            _ => None,
        })
        .collect();
    assert_eq!(sent, [&shown]);
    assert_eq!(sent[0].happened, "The screen flickers");
    assert_eq!(sent[0].replay_bytes.as_ref().map(|b| b.0.as_slice()), Some(older.as_slice()));
    // The app: a failure, said, the text kept.
    h.state_mut().data.report.status = SendStatus::Sending;
    // The spinner repaints every frame.
    h.run_steps(3);
    h.get_by_label("Sending…");
    assert!(h.get_by_label("Send").accesskit_node().is_disabled());
    h.state_mut().data.report.finished(Err("Too many reports right now, please try again later.".into()));
    h.run_steps(3);
    settle(&mut h);
    h.get_by_label("Too many reports right now, please try again later.");
    assert_eq!(h.state().data.report.happened, "The screen flickers");
    // A success: the address, Copy; the text cleared.
    let url = "https://github.com/martinchvatal/open-liero-reloaded/issues/42";
    h.state_mut().data.report.finished(Ok(url.into()));
    settle(&mut h);
    h.get_by_label_contains("Thanks, report filed");
    h.get_by_label(url);
    h.get_by_label("Copy").click();
    settle(&mut h);
    assert!(h.state().data.report.happened.is_empty());
    assert!(h.get_by_label("Send").accesskit_node().is_disabled());
}

/// The logs are in the preview, collapsed until opened.
#[test]
fn the_logs_are_previewed_collapsed() {
    let mut h = harness(None, true);
    h.state_mut().data.report.context = Some(context());
    settle(&mut h);
    h.get_by_label("Report a bug").click();
    settle(&mut h);
    h.get_by_label_contains("This run: now.log, still running");
    assert!(h.query_by_label_contains("warn  gpu: slow frame").is_none(), "collapsed");
    h.get_by_label("This run (log)").scroll_to_me();
    h.run_steps(5);
    h.get_by_label("This run (log)").click();
    // Its opening is animated.
    h.run_steps(30);
    h.get_by_label_contains("warn  gpu: slow frame");
}

/// Send is refused by the state machine off the Report screen and
/// without text.
#[test]
fn send_needs_the_screen_and_text() {
    let mut ui = UiState::new(Settings::default(), Profiles::default(), 1);
    let r = Report::new("broken", "", &context(), None);
    assert!(ui.update(UiAction::SendReport(Box::new(r.clone()))).is_empty());
    assert_eq!(ui.update(UiAction::Open(Screen::Report)), vec![AppCommand::PrepareReport]);
    let empty = Report::new("  \n ", "", &context(), None);
    assert!(ui.update(UiAction::SendReport(Box::new(empty))).is_empty());
    assert_eq!(
        ui.update(UiAction::SendReport(Box::new(r.clone()))),
        vec![AppCommand::SendReport(Box::new(r))]
    );
}
