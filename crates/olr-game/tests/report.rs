//! Bug reports (issue #3) against a local mock relay (plain HTTP on
//! 127.0.0.1, `OLR_REPORT_URL`'s use in tests): the one multipart POST,
//! every failure in words without hanging, the menus' Send off the UI
//! thread, the logs a report carries.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use olr_audio::Audio;
use olr_game::report::{self, Environment, http};
use olr_game::shell::{Outcome, Shell, ShellConfig};
use olr_scene::{Interpolation, Viewport};
use olr_ui::report::{LogEnd, ReplayBytes, ReplayInfo, Report, ReportContext, SendStatus};
use olr_ui::{Profiles, Screen, Settings, Store, UiAction, UiState};

/// A request the mock received: its head and body.
struct Request {
    head: String,
    body: Vec<u8>,
}

/// A relay on 127.0.0.1 answering each of `answers` (status, JSON) to one
/// request in turn, after `delay`; gives its URL and the requests.
fn mock(answers: Vec<(u16, &'static str)>, delay: Duration) -> (String, mpsc::Receiver<Request>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/report", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for (status, json) in answers {
            let Ok((mut s, _)) = listener.accept() else { return };
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            let (head, body) = loop {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
                let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
                let head = String::from_utf8_lossy(&got[..at]).into_owned();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("Content-Length: "))
                    .map_or(0, |v| v.trim().parse().unwrap());
                if got.len() >= at + 4 + len {
                    break (head, got[at + 4..at + 4 + len].to_vec());
                }
            };
            tx.send(Request { head, body }).unwrap();
            std::thread::sleep(delay);
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{json}",
                json.len()
            );
            let _ = s.write_all(reply.as_bytes());
        }
    });
    (url, rx)
}

fn report(with_replay: bool) -> Report {
    let context = ReportContext {
        sections: olr_ui::report::sections(&olr_ui::report::SystemFacts::default()),
        logs: vec![olr_ui::report::LogAttachment::new("This run", "a.log", LogEnd::Running, b"line\n", None)],
        warnings: vec![],
    };
    let mut r = Report::new("It crashed", "No crash", &context, None);
    if with_replay {
        r.replay = Some(ReplayInfo {
            file: "match-000001.olrp".into(),
            date: "2026-10-21 12:00 UTC".into(),
            mod_name: "Liero v1.0".into(),
            level: "Dirt Level".into(),
            worms: vec!["Anna".into()],
            bytes: 9,
        });
        r.replay_bytes = Some(ReplayBytes(std::sync::Arc::new(b"OLRP\x01\x00abc".to_vec())));
    }
    r
}

const FILED: &str = r#"{"ok":true,"url":"https://github.com/o/r/issues/7"}"#;

#[test]
fn a_report_is_one_multipart_post() {
    let (url, requests) = mock(vec![(201, FILED)], Duration::ZERO);
    let r = report(true);
    assert_eq!(report::send_now(&url, &r, report::TIMEOUTS), Ok("https://github.com/o/r/issues/7".into()));
    let got = requests.recv().unwrap();
    assert!(got.head.starts_with("POST /report HTTP/1.1\r\n"), "{}", got.head);
    let boundary = got
        .head
        .lines()
        .find_map(|l| l.strip_prefix("Content-Type: multipart/form-data; boundary="))
        .expect("multipart")
        .to_string();
    assert!(got.head.contains("\r\nConnection: close"));
    let body = String::from_utf8_lossy(&got.body).into_owned();
    let parts: Vec<&str> = body.split(&format!("--{boundary}")).collect();
    // A preamble, the report, the replay, the end.
    assert_eq!(parts.len(), 4, "{body}");
    assert!(parts[1].contains("name=\"report\""));
    assert!(parts[1].ends_with(&format!("\r\n\r\n{}\r\n", r.json())), "the report's JSON unchanged");
    assert!(parts[2].contains("name=\"replay\"; filename=\"replay.olrp\""));
    assert!(parts[2].ends_with("\r\n\r\nOLRP\u{1}\u{0}abc\r\n"), "the replay unchanged");
    assert_eq!(parts[3], "--\r\n");
    // Without a replay: the report alone.
    let (url, requests) = mock(vec![(201, FILED)], Duration::ZERO);
    report::send_now(&url, &report(false), report::TIMEOUTS).unwrap();
    let body = requests.recv().unwrap().body;
    assert!(!String::from_utf8_lossy(&body).contains("name=\"replay\""));
}

#[test]
fn failures_are_said_and_nothing_hangs() {
    let (url, _r) = mock(
        vec![
            (429, r#"{"ok":false,"error":"rate_limited"}"#),
            (502, r#"{"ok":false,"error":"upstream"}"#),
            (500, "oops"),
        ],
        Duration::ZERO,
    );
    let r = report(false);
    assert_eq!(report::send_now(&url, &r, report::TIMEOUTS), Err(report::RATE_LIMITED.to_string()));
    assert!(report::send_now(&url, &r, report::TIMEOUTS).unwrap_err().contains("GitHub did not take"));
    assert!(report::send_now(&url, &r, report::TIMEOUTS).unwrap_err().contains("answered 500"));
    // Nobody listening.
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let e = report::send_now(&format!("http://{closed}/"), &r, report::TIMEOUTS).unwrap_err();
    assert!(e.starts_with("Could not reach the report server"), "{e}");
    // A server that takes the request and never answers: bounded.
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = silent.local_addr().unwrap();
    std::thread::spawn(move || {
        let held = silent.accept();
        std::thread::sleep(Duration::from_secs(3));
        drop(held);
    });
    let short = http::Timeouts { connect: Duration::from_secs(2), exchange: Duration::from_millis(300) };
    let start = Instant::now();
    let e = report::send_now(&format!("http://{addr}/"), &r, short).unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(2), "{:?}", start.elapsed());
    assert!(e.contains("no whole answer in time"), "{e}");
    assert!(report::send_now("ftp://x/", &r, short).unwrap_err().contains("address is wrong"));
}

fn shell(data: &Path) -> Shell {
    let cfg = ShellConfig {
        content: Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/packs"),
        interpolation: Interpolation::Linear,
        viewport: Viewport::from_aspect(16.0 / 9.0, 16.0 / 9.0),
    };
    let ui = UiState::new(Settings::default(), Profiles::default(), 1);
    Shell::new(cfg, ui, Store::open(data.to_path_buf()), Audio::silent())
}

/// The menus' flow: Report a bug only with a relay; opening it gathers
/// the context; Send returns at once while the relay takes its time, the
/// address shows when it answers; a failure keeps the text.
#[test]
fn the_menus_send_off_the_ui_thread() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sh = shell(tmp.path());
    sh.set_report_url(None);
    assert!(!sh.data.report.available);
    let (url, requests) =
        mock(vec![(201, FILED), (429, r#"{"ok":false,"error":"rate_limited"}"#)], Duration::from_millis(400));
    sh.set_report_url(Some(url));
    assert!(sh.data.report.available);
    assert_eq!(sh.act(UiAction::Open(Screen::Report)), vec![Outcome::PrepareReport]);
    sh.prepare_report(Environment { window_system: Some("X11".into()), ..Environment::default() });
    let context = sh.data.report.context.clone().expect("gathered");
    let system = context.sections.iter().find(|s| s.title == "System").unwrap();
    assert!(system.facts.iter().any(|f| f.name == "Window system" && f.value == "X11"));
    assert!(requests.try_recv().is_err(), "nothing sent before Send");
    sh.data.report.happened = "It froze".into();
    let r = sh.data.report.report().unwrap();
    let start = Instant::now();
    sh.act(UiAction::SendReport(Box::new(r.clone())));
    assert!(start.elapsed() < Duration::from_millis(200), "Send waits for nothing");
    assert_eq!(sh.data.report.status, SendStatus::Sending);
    // A second Send while one is under way sends nothing more.
    sh.act(UiAction::SendReport(Box::new(r.clone())));
    let until = |sh: &mut Shell| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while sh.data.report.status == SendStatus::Sending && Instant::now() < deadline {
            sh.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    until(&mut sh);
    assert_eq!(sh.data.report.status, SendStatus::Sent("https://github.com/o/r/issues/7".into()));
    assert!(sh.data.report.happened.is_empty(), "a filed report's text is cleared");
    assert!(requests.recv().unwrap().body.windows(8).any(|w| w == b"It froze"));
    // The relay refuses the next: said, the text kept.
    sh.data.report.happened = "Again".into();
    let r = sh.data.report.report().unwrap();
    sh.act(UiAction::SendReport(Box::new(r)));
    until(&mut sh);
    assert_eq!(sh.data.report.status, SendStatus::Failed(report::RATE_LIMITED.into()));
    assert_eq!(sh.data.report.happened, "Again");
    assert_eq!(requests.iter().count(), 1, "one request each: the second Send was not sent");
}

/// The logs: this run's and the one before it (unclean when it lacks its
/// clean end), the home directory as `~`; none without a log file.
#[test]
fn a_report_carries_this_and_the_previous_runs_log() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let home = dir.join("home").join("ann");
    std::fs::write(dir.join("2026-10-21_10-00-00.000Z_1.log"), "old run\nthe run ended cleanly\n").unwrap();
    let crashed = format!("x\nloaded {}/.local/share/olr/settings.ron\npanicked\n", home.display());
    std::fs::write(dir.join("2026-10-21_11-00-00.000Z_2.log"), crashed).unwrap();
    let ours = dir.join("2026-10-21_12-00-00.000Z_3.log");
    std::fs::write(&ours, "this run\n").unwrap();
    let (logs, warnings) = report::logs(Some(&ours), Some(&home));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(logs.len(), 2);
    assert_eq!(
        (logs[0].name.as_str(), logs[0].ended, logs[0].text.as_str()),
        ("This run", LogEnd::Running, "this run\n")
    );
    assert_eq!(logs[1].file, "2026-10-21_11-00-00.000Z_2.log");
    assert_eq!(logs[1].ended, LogEnd::Unclean);
    assert!(logs[1].text.contains("loaded ~/.local/share/olr/settings.ron"), "{}", logs[1].text);
    assert!(!logs[1].text.contains(&home.display().to_string()));
    let (logs, warnings) = report::logs(None, Some(&home));
    assert!(logs.is_empty());
    assert_eq!(warnings.len(), 1);
}
