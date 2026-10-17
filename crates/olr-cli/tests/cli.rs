//! End-to-end tests of the `olr-cli` binary on a synthetic OLX game directory.

use std::io::{BufRead as _, BufReader};
use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use olr_cli::transport::{SharedLog, TransportSpec};
use olr_data::image::{Rgba, RgbaImage};
use olr_discovery::{GameList, Invite, Pool, PoolConfig, PoolEvent, TestRelay};
use olr_server::config::{Config, Visibility};

use crate::common::{content_dir, copy_dir};

fn png(w: u32, h: u32, c: Rgba) -> Vec<u8> {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    olr_pack::image_io::encode_rgba(&img)
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

fn olr_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_olr-cli"))
}

#[test]
fn import_then_check() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let status = olr_cli()
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

    let status = olr_cli().args(["pack", "check"]).arg(&out).status().unwrap();
    assert!(status.success());
}

#[test]
fn pack_check_fails_on_broken_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("bad");
    std::fs::create_dir_all(&pack).unwrap();
    std::fs::write(pack.join("pack.ron"), "(oops").unwrap();
    let out = olr_cli().args(["pack", "check"]).arg(&pack).output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("pack.ron:1:"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn pack_check_reports_and_exits() {
    let Some(content) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("level");
    copy_dir(&Path::new(&content).join("olx-level-dirt-level"), &pack);
    let mut text = std::fs::read(pack.join("level.ron")).unwrap();
    text.push(b'x');
    std::fs::write(pack.join("level.ron"), text).unwrap();
    let out = olr_cli().args(["pack", "check"]).arg(&pack).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{stdout}");
    let line = stdout.lines().find(|l| l.contains("level.ron:")).unwrap_or_else(|| panic!("{stdout}"));
    let after = line.split("level.ron:").nth(1).unwrap();
    let mut pos = after.split(':');
    assert!(
        pos.next().unwrap().parse::<usize>().is_ok() && pos.next().unwrap().parse::<usize>().is_ok(),
        "{line}"
    );
    assert!(stdout.contains("checked 1 packs: 1 errors, 0 warnings"), "{stdout}");

    let out = olr_cli().arg("validate").arg(&pack).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "a clap usage error");
    assert!(String::from_utf8_lossy(&out.stderr).contains("unrecognized subcommand"));
}

#[test]
fn pack_fmt_check_exits_1_on_a_dirty_pack() {
    let Some(content) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("level");
    copy_dir(&Path::new(&content).join("olx-level-dirt-level"), &pack);
    let clean = olr_cli().args(["pack", "fmt", "--check"]).arg(&pack).output().unwrap();
    assert!(clean.status.success() && clean.stdout.is_empty(), "{}", String::from_utf8_lossy(&clean.stdout));

    let original = std::fs::read(pack.join("level.ron")).unwrap();
    std::fs::write(pack.join("level.ron"), "(name:\"Dirt Level\",size:(504,350),theme:\"olx-theme-dirt\")")
        .unwrap();
    let out = olr_cli().args(["pack", "fmt", "--check"]).arg(&pack).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{}/level.ron: not canonical\n", pack.display())
    );

    let out = olr_cli().args(["pack", "fmt"]).arg(&pack).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("formatted {}: 1 files\n", pack.display()));
    assert_eq!(std::fs::read(pack.join("level.ron")).unwrap(), original);
}

#[test]
fn pack_new_level_then_check() {
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("t");
    let out = olr_cli().args(["pack", "new", "level", "t", "--out"]).arg(&pack).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let out = olr_cli().args(["pack", "check"]).arg(&pack).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.ends_with("checked 1 packs: 0 errors, 0 warnings\n"), "{stdout}");

    let skin = tmp.path().join("s");
    let out = olr_cli().args(["pack", "new", "skin", "s", "--out"]).arg(&skin).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("skin packs start from an existing pack: pass --from <pack dir>")
    );
    let out = olr_cli()
        .args(["pack", "new", "mod", "m", "--size", "8x8", "--out"])
        .arg(tmp.path().join("m"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let out = olr_cli().args(["pack", "new", "level", "l", "--size", "8by8"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "a clap error naming the flag");
    assert!(String::from_utf8_lossy(&out.stderr).contains("--size"));
    assert!(!skin.exists() && !tmp.path().join("m").exists());
}

#[test]
fn reimport_replaces_stale_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let import = || {
        olr_cli()
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
        olr_cli()
            .args(["import", "--olx"])
            .arg(&src)
            .arg("--out")
            .arg(&out)
            .args(["--theme", "data/themes/Dirt", "--prune"])
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

#[test]
fn imports_into_one_out_keep_both_packs() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let import = |flag: &[&str]| {
        olr_cli().args(["import", "--olx"]).arg(&src).arg("--out").arg(&out).args(flag).status().unwrap()
    };
    assert!(import(&["--theme", "data/themes/Dirt"]).success());
    assert!(import(&["--core", "data/gfx/crosshair.png"]).success());
    assert!(out.join("olx-theme-dirt/pack.ron").is_file(), "the first import's pack stays");
    assert!(out.join("olx-core/pack.ron").is_file());
}

#[test]
fn a_hand_made_pack_survives_an_import_without_prune() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let mine = out.join("my-pack");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("pack.ron"), b"(hand made)").unwrap();
    let status = olr_cli()
        .args(["import", "--olx"])
        .arg(&src)
        .arg("--out")
        .arg(&out)
        .args(["--theme", "data/themes/Dirt"])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(std::fs::read(mine.join("pack.ron")).unwrap(), b"(hand made)");
    // With no item flags nothing is imported and nothing is removed.
    assert!(olr_cli().args(["import", "--olx"]).arg(&src).arg("--out").arg(&out).status().unwrap().success());
    assert!(mine.join("pack.ron").is_file() && out.join("olx-theme-dirt/pack.ron").is_file());
}

#[test]
fn prune_removes_packs_the_run_did_not_produce() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let mine = out.join("my-pack");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("pack.ron"), b"(hand made)").unwrap();
    let status = olr_cli()
        .args(["import", "--prune", "--olx"])
        .arg(&src)
        .arg("--out")
        .arg(&out)
        .args(["--theme", "data/themes/Dirt"])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(!mine.exists(), "--prune removes the pack this run did not produce");
    assert!(out.join("olx-theme-dirt/pack.ron").is_file());
}

#[test]
fn all_imports_what_discovery_finds_and_excludes_item_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let (src, out) = (tmp.path().join("gamedir"), tmp.path().join("packs"));
    gamedir(&src);
    let run = |extra: &[&str]| {
        olr_cli().args(["import", "--olx"]).arg(&src).arg("--out").arg(&out).args(extra).status().unwrap()
    };
    // The broken skin makes the run exit 1, but the rest is imported.
    run(&["--all"]);
    assert!(out.join("olx-theme-dirt/pack.ron").is_file());
    assert!(out.join("olx-skin-default/pack.ron").is_file());
    assert!(!run(&["--all", "--skins"]).success(), "--all and item flags conflict");
}

/// `olr-server` in a thread of this process, until dropped.
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
        let content = olr_content::Content::open(Path::new(dir)).unwrap();
        let (s, mut l, i, data) = (Arc::clone(&stop), log.clone(), Arc::clone(&invite), data.to_path_buf());
        let thread = std::thread::spawn(move || {
            olr_server::run_with(config, content, &data, &s, &mut l, &mut |server| {
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

/// Runs `olr-cli net-client` with `args` and `--timeout` (seconds) to its
/// end (at most 60 s): its status, stdout and stderr.
fn net_client(args: &[&str], timeout: &str, content: &str) -> (bool, String, String) {
    let mut child = olr_cli()
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
    let mut relay = olr_cli()
        .args(["nostr-relay", "--listen", "127.0.0.1:0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stun = olr_cli()
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
    socket.send_to(&olr_discovery::binding_request(t), server).unwrap();
    let mut buf = [0u8; 200];
    let (len, from) = socket.recv_from(&mut buf).unwrap();
    assert_eq!(from, server);
    assert_eq!(olr_discovery::parse_binding_response(&buf[..len], &t), Some(socket.local_addr().unwrap()));

    for (name, child) in [("nostr-relay", &mut relay), ("stun-server", &mut stun)] {
        assert!(child.try_wait().unwrap().is_none(), "{name} still runs");
        child.kill().unwrap();
        child.wait().unwrap();
    }
}

/// `olr-cli net-sim` with `args` (and the test's content and a failures
/// directory of its own): its status, stdout and stderr.
fn net_sim(args: &[&str], content: &str, failures: &Path) -> (bool, String, String) {
    let out = olr_cli()
        .args(["net-sim", "--content", content])
        .arg("--failures")
        .arg(failures)
        .args(args)
        .output()
        .unwrap();
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (out.status.success(), text(&out.stdout), text(&out.stderr))
}

/// Plan 12c: `net-sim --exit 0:300:crash` migrates the match to peer 1 and
/// passes; the same spec kept as a `NetCase` and played with `--from` prints
/// the same summary; a bad `--exit`, `--host-freeze` or `--profile-change`
/// is an error naming the flag.
#[test]
fn net_sim_migration_flags_run_and_replay() {
    let Some(dir) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let (ok, stdout, stderr) = net_sim(&["--exit", "0:300:crash", "--ticks", "800"], &dir, tmp.path());
    assert!(ok, "net-sim failed:\n{stdout}\n{stderr}");
    assert!(stdout.lines().any(|l| l == "ok"), "{stdout}");
    assert!(stdout.lines().any(|l| l.starts_with("migrated: 0 -> 1 at tick ")), "{stdout}");
    let spec = olr_cli::netsim::NetSpec {
        mod_id: "olx-classic".into(),
        level_id: "olx-level-castlestrike".into(),
        seed: 1,
        slots: 8,
        clients: 3,
        spectators: 1,
        join_tick: None,
        profile: "typical".into(),
        drift_ppm: 0,
        ticks: 800,
        outages: vec![],
        net_seed: 0,
        difficulty: None,
        faults: vec![],
        exits: vec![(0, 300, "crash".into())],
        host_freeze: None,
        profile_changes: vec![],
        migration: false,
    };
    let case = olr_cli::netsim::NetCase { spec, failure: None };
    let path = tmp.path().join("case.ron");
    std::fs::write(&path, ron::ser::to_string_pretty(&case, ron::ser::PrettyConfig::default()).unwrap())
        .unwrap();
    let (ok, replayed, stderr) = net_sim(&["--from", path.to_str().unwrap()], &dir, tmp.path());
    assert!(ok, "net-sim --from failed:\n{replayed}\n{stderr}");
    assert_eq!(replayed, stdout, "the kept case plays the same run");
    for (flag, value) in [("--exit", "0:300:explode"), ("--host-freeze", "5"), ("--profile-change", "1:2")] {
        let (ok, out, err) = net_sim(&[flag, value], &dir, tmp.path());
        assert!(!ok, "{flag} {value} is refused:\n{out}");
        assert!(err.contains(flag), "{flag} named in:\n{err}");
    }
}
