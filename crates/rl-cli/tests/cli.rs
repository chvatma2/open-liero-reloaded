//! End-to-end tests of the `rl-cli` binary on a synthetic OLX game directory.

use std::io::{BufRead as _, BufReader};
use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rl_cli::transport::{SharedLog, TransportSpec};
use rl_data::image::{Rgba, RgbaImage};
use rl_discovery::{GameList, Invite, Pool, PoolConfig, PoolEvent, TestRelay};
use rl_server::config::{Config, Visibility};

use crate::common::content_dir;

fn png(w: u32, h: u32, c: Rgba) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    rl_pack::image_io::encode_rgba(&img)
}

fn gamedir(root: &Path) {
    let theme = root.join("data/themes/Dirt");
    std::fs::create_dir_all(&theme).unwrap();
    std::fs::write(theme.join("theme.txt"), "[General]\nNumStones = 0\nNumMisc = 0\n").unwrap();
    for f in ["Hole1", "Hole2", "Hole3", "Hole4", "Hole5", "FrontTile", "BackTile"] {
        std::fs::write(theme.join(format!("{f}.png")), png(4, 4, Rgba::BLACK)).unwrap();
    }
    let skins = root.join("skins");
    std::fs::create_dir_all(&skins).unwrap();
    std::fs::write(skins.join("default.png"), png(672, 36, Rgba::rgb(200, 200, 200))).unwrap();
    std::fs::write(skins.join("Default.png"), png(672, 36, Rgba::rgb(100, 100, 100))).unwrap();
    std::fs::write(skins.join("broken.png"), b"not a png").unwrap();
    std::fs::write(skins.join("readme.txt"), b"ignored").unwrap();
    std::fs::create_dir_all(root.join("data/gfx")).unwrap();
    std::fs::write(root.join("data/gfx/crosshair.png"), png(12, 6, Rgba::MAGENTA)).unwrap();
    std::fs::create_dir_all(root.join("data/frontend")).unwrap();
    std::fs::write(root.join("data/frontend/colours.cfg"), "[Colours]\nRopeColour1 = #A05000\n").unwrap();
}

fn rl_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rl-cli"))
}

#[test]
fn import_then_validate() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let status = rl_cli()
        .args(["import", "--olx"])
        .arg(&src)
        .arg("--out")
        .arg(&out)
        .args(["--revision", "abc", "--theme", "data/themes/Dirt", "--skins"])
        .args(["--core", "data/gfx/crosshair.png", "--core", "data/frontend/colours.cfg"])
        .status()
        .unwrap();
    assert!(!status.success(), "the broken skin must make import fail");
    let core = std::fs::read_to_string(out.join("olx-core/core.ron")).unwrap();
    assert!(core.contains("crosshair") && core.contains("RopeColour1"), "{core}");
    assert!(out.join("olx-core/gfx/crosshair.png").is_file());
    // Both skins slug to the same id; the second gets a suffix.
    assert!(out.join("olx-skin-default/pack.ron").is_file());
    assert!(out.join("olx-skin-default-2/pack.ron").is_file());
    assert!(out.join("olx-theme-dirt/theme.ron").is_file());
    let report = std::fs::read_to_string(out.join("import-report.ron")).unwrap();
    assert!(report.contains("skins/broken.png") && report.contains("error: Some("), "{report}");

    let status = rl_cli().arg("validate").arg(&out).status().unwrap();
    assert!(status.success());
}

#[test]
fn validate_fails_on_broken_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("bad");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("pack.ron"), "(oops").unwrap();
    let out = rl_cli().arg("validate").arg(&pack).output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("pack.ron:1:"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn reimport_replaces_stale_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let import = || {
        rl_cli()
            .args(["import", "--olx"])
            .arg(&src)
            .arg("--out")
            .arg(&out)
            .args(["--theme", "data/themes/Dirt"])
            .status()
            .unwrap()
    };
    assert!(import().success());
    let stale = out.join("olx-theme-dirt/stale.png");
    std::fs::write(&stale, b"left over from an older import").unwrap();
    assert!(import().success());
    assert!(!stale.exists(), "re-import must remove files the importer no longer writes");
}

#[test]
fn reimport_removes_packs_no_longer_imported() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let import = |extra: &[&str]| {
        rl_cli()
            .args(["import", "--olx"])
            .arg(&src)
            .arg("--out")
            .arg(&out)
            .args(["--theme", "data/themes/Dirt"])
            .args(extra)
            .status()
            .unwrap()
    };
    import(&["--skins"]);
    assert!(out.join("olx-skin-default/pack.ron").is_file());
    let unrelated = out.join("notes.txt");
    std::fs::write(&unrelated, b"not a pack").unwrap();
    assert!(import(&[]).success());
    assert!(!out.join("olx-skin-default").exists(), "packs dropped from the import must be removed");
    assert!(out.join("olx-theme-dirt/pack.ron").is_file());
    assert!(unrelated.exists(), "only pack directories are removed");
}

/// `rl-server` in a thread of this process, until dropped.
struct InProcessServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<anyhow::Result<()>>>,
    log: SharedLog,
    /// Its invite once it carries an address (N47: the configured one).
    invite: Arc<Mutex<Option<Invite>>>,
}

impl InProcessServer {
    /// A server of `config` on content `dir` (its data in `data`).
    fn start(config: Config, dir: &str, data: &Path) -> InProcessServer {
        let stop = Arc::new(AtomicBool::new(false));
        let log = SharedLog::default();
        let invite: Arc<Mutex<Option<Invite>>> = Arc::default();
        let content = rl_content::Content::open(Path::new(dir)).unwrap();
        let (s, mut l, i, data) = (Arc::clone(&stop), log.clone(), Arc::clone(&invite), data.to_path_buf());
        let thread = std::thread::spawn(move || {
            rl_server::run_with(config, content, &data, &s, &mut l, &mut |server| {
                let code = server.invite();
                if code.addr.is_some() {
                    *i.lock().unwrap() = Some(code);
                }
            })
        });
        InProcessServer { stop, thread: Some(thread), log, invite }
    }

    /// Its invite once it has an address (within 15 s).
    fn invite_with_address(&self) -> Invite {
        let begun = Instant::now();
        loop {
            if let Some(i) = self.invite.lock().unwrap().clone() {
                return i;
            }
            assert!(
                begun.elapsed() < Duration::from_secs(15),
                "an invite with an address:\n{}",
                self.log.text()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for InProcessServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A short server's config on the relay `relay`, reachable at 127.0.0.1
/// (N62's `allow_local`), never asking the router.
fn server_config(name: &str, visibility: Visibility, relay: &TestRelay, data: &Path) -> Config {
    let mut config = TransportSpec::new(1, 0.1, None, data).server_config();
    config.name = name.into();
    config.visibility = visibility;
    config.relays = vec![relay.url()];
    config.public_address = Some("127.0.0.1".into());
    config.allow_local = true;
    config.port_mapping = false;
    config.stun = Vec::new();
    config
}

/// Runs `rl-cli net-client` with `args` and `--timeout` (seconds) to its
/// end (at most 60 s): its status, stdout and stderr.
fn net_client(args: &[&str], timeout: &str, content: &str) -> (bool, String, String) {
    let mut child = rl_cli()
        .args(["net-client", "--bot", "--name", "Cli", "--timeout", timeout, "--content", content])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let begun = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if begun.elapsed() > Duration::from_secs(60) {
            child.kill().unwrap();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (out.status.success(), text(&out.stdout), text(&out.stderr))
}

/// `--invite` (N46): the code of an Invite server with an address (its
/// public address, with `allow_local`) and the test relay as its hint
/// joins directly first; `--lobby-only` ends once in the lobby. Only the
/// direct path is taken here: a join through the relays from the CLI
/// (a relay-only invite, `--key`, a listed game without an address) is
/// unit-tested in its resolution and first runs in the Docker NAT tests
/// (its candidates would leave loopback). Leaving says goodbye: the server
/// sees the leave at once, not after a timeout.
#[test]
fn net_client_joins_by_invite_through_a_test_relay() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let data = tempfile::tempdir().unwrap();
    let config = server_config("Invite test", Visibility::Invite, &relay, data.path());
    let server = InProcessServer::start(config, &dir, data.path());
    let invite = server.invite_with_address();
    assert_eq!(invite.relay_urls(), [relay.url()], "the relay is the code's hint");
    let code = invite.encode();
    let (ok, stdout, stderr) = net_client(&["--invite", &code, "--lobby-only"], "40", &dir);
    assert!(ok, "net-client failed:\n{stdout}\n{stderr}\nthe server's log:\n{}", server.log.text());
    assert!(stdout.lines().any(|l| l == "path: direct"), "{stdout}");
    assert!(server.log.text().contains("join: Cli ("), "{}", server.log.text());
    let left = Instant::now();
    while !server.log.text().contains("leave Cli [") {
        assert!(left.elapsed() < Duration::from_secs(2), "the leave at once:\n{}", server.log.text());
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// `--listed` (N46): a Public server's game found by its name on the
/// relay (local addresses allowed) is joined as the Internet tab joins it:
/// its address first.
#[test]
fn net_client_joins_a_listed_game() {
    let Some(dir) = content_dir() else { return };
    let relay = TestRelay::start();
    let data = tempfile::tempdir().unwrap();
    let config = server_config("Listed test", Visibility::Public, &relay, data.path());
    let server = InProcessServer::start(config, &dir, data.path());
    let url = relay.url();
    let args = ["--listed", "Listed test", "--relay", &url, "--allow-local", "--lobby-only"];
    let (ok, stdout, stderr) = net_client(&args, "40", &dir);
    assert!(ok, "net-client failed:\n{stdout}\n{stderr}\nthe server's log:\n{}", server.log.text());
    assert!(stdout.lines().any(|l| l == "path: direct"), "{stdout}");
    // A name nobody announces is not found (the wait cut to `--timeout`).
    let args = ["--listed", "No such game", "--relay", &url, "--allow-local", "--lobby-only"];
    let (ok, stdout, stderr) = net_client(&args, "1", &dir);
    assert!(!ok, "{stdout}");
    assert!(stderr.contains("no game named \"No such game\""), "{stderr}");
}

/// The first line of `child`'s stdout (within 20 s).
fn first_line(child: &mut Child) -> String {
    let out = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(out).read_line(&mut line);
        let _ = tx.send(line);
    });
    rx.recv_timeout(Duration::from_secs(20)).expect("a first line").trim().to_string()
}

/// The dev tools of the Docker NAT tests (feature `nat-tools`, on in this
/// test build through the self dev-dependency): each prints where it
/// listens (loopback here), answers until killed.
#[test]
fn nostr_relay_and_stun_server_run_until_killed() {
    let mut relay = rl_cli()
        .args(["nostr-relay", "--listen", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stun = rl_cli()
        .args(["stun-server", "--listen", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let relay_line = first_line(&mut relay);
    let url =
        relay_line.strip_prefix("nostr relay: ").unwrap_or_else(|| panic!("{relay_line:?}")).to_string();
    assert!(url.starts_with("ws://127.0.0.1:"), "{url}");
    let stun_line = first_line(&mut stun);
    let server: SocketAddr =
        stun_line.strip_prefix("stun server: ").unwrap_or_else(|| panic!("{stun_line:?}")).parse().unwrap();

    // One REQ: its EOSE.
    let mut pool = Pool::start(PoolConfig { relays: vec![url], ..PoolConfig::default() });
    pool.subscribe("games", vec![GameList::filter()]);
    let begun = Instant::now();
    let mut events = Vec::new();
    while !events.iter().any(|e| matches!(e, PoolEvent::Eose { .. })) {
        assert!(begun.elapsed() < Duration::from_secs(10), "the relay's EOSE: {events:?}");
        pool.poll(&mut events);
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(pool);

    // One binding: the answer is this socket's own address.
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let t = [9u8; 12];
    socket.send_to(&rl_discovery::binding_request(t), server).unwrap();
    let mut buf = [0u8; 200];
    let (len, from) = socket.recv_from(&mut buf).unwrap();
    assert_eq!(from, server);
    assert_eq!(rl_discovery::parse_binding_response(&buf[..len], &t), Some(socket.local_addr().unwrap()));

    for (name, child) in [("nostr-relay", &mut relay), ("stun-server", &mut stun)] {
        assert!(child.try_wait().unwrap().is_none(), "{name} still runs");
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
