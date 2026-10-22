//! Updates (issue #4) against fakes: a mock of GitHub and its downloads
//! on 127.0.0.1 (the real HTTP client, `https://` addresses mapped to
//! it), a test signing key, the portable folder in a temporary
//! directory, and a fake Flatpak portal. Nothing leaves the machine.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use olr_audio::Audio;
use olr_game::http::{self, GetTimeouts};
use olr_game::shell::{Outcome, Shell, ShellConfig};
use olr_game::update::{self, Event, Fetch, Install, Portal, Updater};
use olr_scene::{Interpolation, Viewport};
use olr_ui::update::{UpdateData, UpdateMethod, UpdateStatus};
use olr_ui::{Profiles, Screen, Settings, Store, UiAction, UiState};
use olr_update::flatpak::{FlatpakInfo, PortalProgress};
use olr_update::manifest::{Manifest, SigningKey};
use olr_update::release::{self, Release};
use olr_update::{Channel, Version};

/// What the mock answers for a path: a status, its headers, a body, and
/// whether to send it chunked.
#[derive(Clone)]
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    chunked: bool,
}

fn ok(body: impl Into<Vec<u8>>) -> Answer {
    Answer { status: 200, headers: vec![], body: body.into(), chunked: false }
}

fn redirect(to: &str) -> Answer {
    Answer { status: 302, headers: vec![("Location".into(), to.into())], body: vec![], chunked: false }
}

type Routes = Arc<Mutex<BTreeMap<String, Answer>>>;

/// A server on 127.0.0.1 answering every request by its path (404
/// otherwise); gives its base address and the routes to fill.
fn mock() -> (String, Routes) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes: Routes = Arc::default();
    let r = routes.clone();
    std::thread::spawn(move || {
        for s in listener.incoming() {
            let Ok(mut s) = s else { return };
            let r = r.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                loop {
                    let mut l = String::new();
                    if reader.read_line(&mut l).unwrap() == 0 || l == "\r\n" {
                        break;
                    }
                }
                let path = first.split(' ').nth(1).unwrap_or("/").to_string();
                let a = r.lock().unwrap().get(&path).cloned();
                let a = a.unwrap_or(Answer {
                    status: 404,
                    headers: vec![],
                    body: b"no".to_vec(),
                    chunked: false,
                });
                let mut head = format!("HTTP/1.1 {} X\r\n", a.status);
                for (k, v) in &a.headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                if a.chunked {
                    head.push_str("Transfer-Encoding: chunked\r\n\r\n");
                    let _ = s.write_all(head.as_bytes());
                    for c in a.body.chunks(7) {
                        let _ = s.write_all(format!("{:x}\r\n", c.len()).as_bytes());
                        let _ = s.write_all(c);
                        let _ = s.write_all(b"\r\n");
                    }
                    let _ = s.write_all(b"0\r\n\r\n");
                } else {
                    head.push_str(&format!("Content-Length: {}\r\n\r\n", a.body.len()));
                    let _ = s.write_all(head.as_bytes());
                    let _ = s.write_all(&a.body);
                }
            });
        }
    });
    (base, routes)
}

const T: GetTimeouts = GetTimeouts { connect: Duration::from_secs(5), idle: Duration::from_secs(5) };

fn get(url: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut seen = 0;
    http::get(url, "*/*", "olr-test", max, T, &mut out, &mut |done, _| seen = done)
        .map_err(|e| e.to_string())?;
    assert_eq!(seen, out.len() as u64, "the progress reaches the end");
    Ok(out)
}

#[test]
fn a_get_follows_redirects_and_streams_every_framing() {
    let (base, routes) = mock();
    let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    {
        let mut r = routes.lock().unwrap();
        r.insert("/plain".into(), ok(big.clone()));
        r.insert("/chunked".into(), Answer { chunked: true, ..ok(b"hello, chunked world".to_vec()) });
        r.insert("/hop".into(), redirect("/plain"));
        r.insert("/absolute".into(), redirect(&format!("{base}/hop")));
        r.insert("/loop".into(), redirect("/loop"));
    }
    assert_eq!(get(&format!("{base}/plain"), 1 << 20).unwrap(), big);
    assert_eq!(get(&format!("{base}/chunked"), 100).unwrap(), b"hello, chunked world");
    assert_eq!(get(&format!("{base}/absolute"), 1 << 20).unwrap(), big);
    assert!(get(&format!("{base}/loop"), 100).unwrap_err().contains("too many redirects"));
    assert!(get(&format!("{base}/missing"), 100).unwrap_err().contains("answered 404"));
    assert!(get(&format!("{base}/plain"), 1000).unwrap_err().contains("too long"));
    assert!(get(&format!("{base}/chunked"), 5).unwrap_err().contains("too long"));
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    assert!(get(&format!("http://{closed}/x"), 100).is_err());
    // A server that never answers: bounded by the idle wait.
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = silent.local_addr().unwrap();
    std::thread::spawn(move || {
        let held = silent.accept();
        std::thread::sleep(Duration::from_secs(3));
        drop(held);
    });
    let short = GetTimeouts { connect: Duration::from_secs(2), idle: Duration::from_millis(300) };
    let start = Instant::now();
    let mut sink = Vec::new();
    assert!(http::get(&format!("http://{addr}/"), "*/*", "t", 10, short, &mut sink, &mut |_, _| {}).is_err());
    assert!(start.elapsed() < Duration::from_secs(2));
}

/// The real client with `https://github.example/` mapped to the mock.
struct Mapped(String);

impl Fetch for Mapped {
    fn get(
        &self,
        url: &str,
        accept: &str,
        max: u64,
        sink: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), String> {
        let url = url.replace("https://github.example", &self.0);
        http::get(&url, accept, "olr-test", max, T, sink, progress).map_err(|e| e.to_string())
    }
}

const SEED: [u8; 32] = [42; 32];
const OLD: &str = "0.2.0-dev.318.014560b";
const NEW: &str = "0.2.0-dev.330.0145601";

fn key() -> SigningKey {
    SigningKey::from_seed(&SEED).unwrap()
}

/// The new release's zip: `olr.exe` and a pack.
fn release_zip(version: &str) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let o = zip::write::SimpleFileOptions::default();
    let top = release::folder_name(version);
    for (f, text) in
        [("olr.exe", format!("exe {version}")), ("content/packs/a.ron", format!("pack {version}"))]
    {
        z.start_file(format!("{top}/{f}"), o).unwrap();
        z.write_all(text.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// The list GitHub would give: a final release, a pre-release between
/// `OLD` and `NEW`, and the newer pre-release `NEW` with its signed zip
/// (the signature `sig` as given).
fn publish(routes: &Routes, zip: &[u8], sig: &str) {
    let zip_name = release::windows_zip_name(NEW);
    let list = format!(
        r###"[{{"tag_name": "v{NEW}", "name": "Dev build 330", "body": "## New\n- **Updates** from the menu",
             "prerelease": true, "html_url": "https://github.example/o/r/releases/tag/v{NEW}",
             "assets": [
               {{"name": "{zip_name}", "browser_download_url": "https://github.example/dl/zip", "size": {}}},
               {{"name": "{zip_name}.sig", "browser_download_url": "https://github.example/dl/sig", "size": 1}}]}},
            {{"tag_name": "v0.2.0-dev.320.0aaaaaa", "name": "Dev build 320", "body": "- Fixes",
              "prerelease": true, "html_url": "https://github.example/o/r/releases/tag/v0.2.0-dev.320.0aaaaaa"}},
            {{"tag_name": "v0.1.0", "name": "Open Liero Reloaded 0.1.0", "prerelease": false,
              "html_url": "https://github.example/o/r/releases/tag/v0.1.0"}}]"###,
        zip.len()
    );
    let mut r = routes.lock().unwrap();
    r.insert("/releases".into(), Answer { chunked: true, ..ok(list) });
    // GitHub's downloads redirect to its storage.
    r.insert("/dl/zip".into(), redirect("/storage/zip"));
    r.insert("/storage/zip".into(), ok(zip.to_vec()));
    r.insert("/dl/sig".into(), redirect("/storage/sig"));
    r.insert("/storage/sig".into(), ok(sig.as_bytes().to_vec()));
}

fn signed(zip: &[u8], version: &str) -> String {
    key().sign(&Manifest::of(version, &release::windows_zip_name(version), zip))
}

/// The installed old build: its program, its manifest, the player's file.
fn old_folder(game: &Path) {
    std::fs::create_dir_all(game.join("content/packs")).unwrap();
    std::fs::write(game.join("olr.exe"), "exe old").unwrap();
    std::fs::write(game.join("content/packs/a.ron"), "pack old").unwrap();
    std::fs::write(game.join("gone.txt"), "old only").unwrap();
    std::fs::write(
        game.join("install-manifest.txt"),
        "content/packs/a.ron\ngone.txt\ninstall-manifest.txt\nolr.exe\n",
    )
    .unwrap();
    std::fs::write(game.join("mine.txt"), "the player's").unwrap();
}

fn releases(base: &str) -> Vec<Release> {
    update::fetch_releases(&Mapped(base.into()), "https://github.example/releases").unwrap()
}

fn install(base: &str, game: &Path, pubkey: &[u8; 32]) -> (Event, Vec<Event>) {
    let list = releases(base);
    let current = Version::parse(OLD).unwrap();
    let r = release::newest(&list, &current, Channel::PreRelease).unwrap();
    let mut steps = Vec::new();
    let end =
        update::install_portable(&Mapped(base.into()), r, &current, game, pubkey, &mut |e| steps.push(e));
    (end, steps)
}

#[test]
fn a_signed_release_is_downloaded_and_swapped_in() {
    let (base, routes) = mock();
    let zip = release_zip(NEW);
    publish(&routes, &zip, &signed(&zip, NEW));
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path().join("open-liero-reloaded-0.2.0-dev.318.014560b");
    old_folder(&game);
    let (end, steps) = install(&base, &game, &key().public());
    assert_eq!(end, Event::Installed, "{steps:?}");
    assert!(steps.iter().any(|e| matches!(e, Event::Step(s, Some(_)) if s.starts_with("Downloading"))));
    assert_eq!(std::fs::read_to_string(game.join("olr.exe")).unwrap(), format!("exe {NEW}"));
    assert_eq!(std::fs::read_to_string(game.join("content/packs/a.ron")).unwrap(), format!("pack {NEW}"));
    assert_eq!(std::fs::read_to_string(game.join("mine.txt")).unwrap(), "the player's");
    assert!(!game.join("gone.txt").exists(), "a file the new release lacks is removed");
    // The old program is removed at the next start.
    assert_eq!(olr_update::install::recover(&game, "olr.exe"), olr_update::install::Recovery::Cleaned);
    assert!(!game.join(".olr-update").exists());
}

#[test]
fn unverified_downloads_are_refused_and_change_nothing() {
    let zip = release_zip(NEW);
    let mut tampered = zip.clone();
    let last = tampered.len() - 30;
    tampered[last] ^= 1;
    let other = SigningKey::from_seed(&[1; 32]).unwrap();
    let older = release_zip("0.2.0-dev.300.0000000");
    let cases: [(&str, Vec<u8>, String); 5] = [
        (
            "another key's signature",
            zip.clone(),
            other.sign(&Manifest::of(NEW, &release::windows_zip_name(NEW), &zip)),
        ),
        ("a changed zip", tampered, signed(&zip, NEW)),
        // An older, validly signed zip passed off as the new release.
        ("an older release's signature", older.clone(), signed(&older, "0.2.0-dev.300.0000000")),
        ("no signature", zip.clone(), "olr-update-manifest 1\n".into()),
        ("a cut download", zip[..zip.len() / 2].to_vec(), signed(&zip, NEW)),
    ];
    for (what, served, sig) in cases {
        let (base, routes) = mock();
        publish(&routes, &served, &sig);
        let dir = tempfile::tempdir().unwrap();
        let game = dir.path().join("game");
        old_folder(&game);
        let (end, _) = install(&base, &game, &key().public());
        assert!(
            matches!(&end, Event::Failed(why) if why.contains("nothing was installed")),
            "{what}: {end:?}"
        );
        assert_eq!(std::fs::read_to_string(game.join("olr.exe")).unwrap(), "exe old", "{what}");
        assert!(game.join("gone.txt").exists(), "{what}");
    }
}

#[cfg(unix)]
#[test]
fn an_unwritable_folder_says_so() {
    use std::os::unix::fs::PermissionsExt as _;
    let (base, routes) = mock();
    let zip = release_zip(NEW);
    publish(&routes, &zip, &signed(&zip, NEW));
    let dir = tempfile::tempdir().unwrap();
    let game = dir.path().join("game");
    old_folder(&game);
    std::fs::set_permissions(&game, std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::write(game.join("root-probe"), b"").is_ok() {
        return; // root writes anywhere
    }
    let (end, _) = install(&base, &game, &key().public());
    std::fs::set_permissions(&game, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(&end, Event::NotWritable(why) if why.contains("cannot be written")), "{end:?}");
}

/// A portal answering with `script`, or refusing the call.
struct FakePortal(Result<Vec<PortalProgress>, String>);

impl Portal for FakePortal {
    fn update(&mut self, progress: &mut dyn FnMut(&PortalProgress)) -> Result<(), String> {
        let script = self.0.clone()?;
        for p in &script {
            progress(p);
        }
        Ok(())
    }
}

#[test]
fn the_flatpak_portal_updates_or_says_what_to_run() {
    let info = FlatpakInfo {
        app_id: "io.github.openlieroreloaded.OpenLieroReloaded".into(),
        branch: "stable".into(),
        user: Some(true),
    };
    let command = "flatpak update --user io.github.openlieroreloaded.OpenLieroReloaded//stable";
    let run = |script| {
        let mut steps = Vec::new();
        let end = update::install_flatpak(&mut FakePortal(script), &info, &mut |e| steps.push(e));
        (end, steps)
    };
    let (end, steps) = run(Ok(vec![PortalProgress::Running(0.5), PortalProgress::Done]));
    assert_eq!(end, Event::Installed);
    assert!(steps.contains(&Event::Step("Updating through Flatpak".into(), Some(0.5))));
    let (end, _) = run(Ok(vec![PortalProgress::UpToDate]));
    assert!(matches!(end, Event::Failed(why) if why.contains("no newer build yet")));
    let refused = PortalProgress::Failed {
        error: "org.freedesktop.DBus.Error.AccessDenied".into(),
        message: "Update requires new permissions".into(),
    };
    let (end, _) = run(Ok(vec![refused]));
    assert!(
        matches!(&end, Event::Outside { why, command: c } if why.contains("permissions") && c == command),
        "{end:?}"
    );
    let (end, _) = run(Err("no session bus".into()));
    assert!(matches!(&end, Event::Outside { command: c, .. } if c == command));
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

fn poll_until(sh: &mut Shell, what: &str, done: impl Fn(&UpdateData) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(&sh.data.update) {
        assert!(Instant::now() < deadline, "no {what}: {:?}", sh.data.update);
        sh.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The menus' flow: a local build shows nothing; the check runs off the
/// UI thread, the offer follows the channel (never a downgrade), Update
/// installs, Restart quits.
#[test]
fn the_menus_offer_and_install_an_update() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sh = shell(&tmp.path().join("data"));
    // This test build is a local one: it never checks.
    assert_eq!(sh.data.update, UpdateData::default());

    let (base, routes) = mock();
    let zip = release_zip(NEW);
    publish(&routes, &zip, &signed(&zip, NEW));
    let game = tmp.path().join("game");
    old_folder(&game);
    let fetch: Arc<dyn Fetch> = Arc::new(Mapped(base.clone()));
    let current = Version::parse(OLD).unwrap();
    let start = Instant::now();
    let u = Updater::start(
        "https://github.example/releases",
        current,
        Install::Portable(game.clone()),
        fetch,
        key().public(),
    );
    assert!(start.elapsed() < Duration::from_millis(200), "the check waits for nothing");
    sh.set_updater(Some(u));
    assert!(sh.data.update.enabled && sh.data.update.build_prerelease);
    // A pre-release build is on the pre-release channel.
    poll_until(&mut sh, "offer", |d| d.offer.is_some());
    let offer = sh.data.update.offer.clone().unwrap();
    assert_eq!((offer.version.as_str(), offer.title.as_str()), (NEW, "Dev build 330"));
    // Every build skipped, newest first, the running one not.
    let changes: Vec<(&str, &str)> =
        offer.changes.iter().map(|n| (n.heading.as_str(), n.notes.as_str())).collect();
    assert_eq!(
        changes,
        [
            (format!("Dev build 330 (v{NEW})").as_str(), "New\n• Updates from the menu"),
            ("Dev build 320 (v0.2.0-dev.320.0aaaaaa)", "• Fixes")
        ]
    );
    assert_eq!(offer.left_out, 0);
    assert_eq!(offer.method, UpdateMethod::Portable);
    // Stable: v0.1.0 only, older than this build: nothing (no downgrade).
    sh.act(UiAction::SetIncludePrereleases(false));
    poll_until(&mut sh, "no offer", |d| d.offer.is_none());
    sh.act(UiAction::SetIncludePrereleases(true));
    poll_until(&mut sh, "the offer again", |d| d.offer.is_some());
    // Update installs, off the UI thread.
    assert!(sh.act(UiAction::Open(Screen::Update)).is_empty());
    sh.act(UiAction::InstallUpdate);
    assert!(matches!(sh.data.update.status, UpdateStatus::Working { .. }));
    poll_until(&mut sh, "the install", |d| {
        d.status != UpdateStatus::Idle && !matches!(d.status, UpdateStatus::Working { .. })
    });
    assert_eq!(sh.data.update.status, UpdateStatus::Installed);
    assert_eq!(std::fs::read_to_string(game.join("olr.exe")).unwrap(), format!("exe {NEW}"));
    // Installed: a second Update does nothing.
    sh.act(UiAction::InstallUpdate);
    assert_eq!(sh.data.update.status, UpdateStatus::Installed);
}

#[test]
fn an_unreachable_github_shows_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut sh = shell(&tmp.path().join("data"));
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
    let fetch: Arc<dyn Fetch> = Arc::new(Mapped(format!("http://{closed}")));
    let current = Version::parse("0.1.0").unwrap();
    sh.set_updater(Some(Updater::start(
        "https://github.example/releases",
        current,
        Install::Other,
        fetch,
        [0; 32],
    )));
    for _ in 0..200 {
        sh.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(sh.data.update.offer.is_none());
    assert_eq!(sh.data.update.status, UpdateStatus::Idle);
    assert!(!sh.act(UiAction::Open(Screen::Update)).contains(&Outcome::Quit));
}
