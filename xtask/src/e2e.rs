//! `cargo xtask e2e` and `full`'s `e2e` step (spec §7, §21 G7): plays
//! `fixtures/autoplay/menus.ron` in the real game on a virtual X server
//! (Xvfb), with `--fake-clock` (deterministic: the same frames, replay and
//! screenshots every run) and a fresh data directory holding
//! `fixtures/autoplay/settings.ron`. It passes when the game exits 0, its
//! structured log (`autoplay.jsonl`) ends in `pass`, every screenshot the
//! script names was written, and stderr has no warning, error or panic.
//!
//! Then the online run (plan 10 task 15, plan 12b task 10): a Nostr relay
//! of its own on loopback (`olr-cli nostr-relay --listen 127.0.0.1:0`,
//! built with `nat-tools`; its URL read from its first line), `olr-server`
//! with `fixtures/server/e2e.ron` (Public on that relay at 127.0.0.1,
//! `allow_local`; port 0: the port is read from its log's "listening on
//! port N", so runs never collide), a bot client (`olr-cli net-client
//! --bot`) joined, and the game (its settings naming the relay) plays
//! `fixtures/autoplay/online.ron` (its `{port}` replaced) on the real
//! clock: joining by the typed address, leaving, joining again as listed
//! on the Internet tab, chat, ready, the server's short match, its results,
//! back to the lobby, leaving and quitting. It also needs the bot client
//! to finish its match and leave by itself, the server to stop cleanly on
//! SIGTERM, the server's log to show the three joins, the chat line and a
//! match's start and end, the relay to run until then and stop on SIGTERM,
//! and the helpers' output to be clean. The helpers
//! are killed whatever happens.
//!
//! Rendering is forced onto Mesa's software rasteriser, so the run is the
//! same on every machine and the WSL relaunch (`olr-game`'s `wsl.rs`, which
//! would pick the real GPU through d3d12) never happens: `WGPU_BACKEND=gl`,
//! `LIBGL_ALWAYS_SOFTWARE=1`, `GALLIUM_DRIVER=llvmpipe`, no Wayland, and
//! `--window-system x11`. Sound is off (`--no-sound`). Without Xvfb the
//! step is skipped with a notice.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::root;

/// The script and the settings it starts from.
pub const SCRIPT: &str = "fixtures/autoplay/menus.ron";
pub const SETTINGS: &str = "fixtures/autoplay/settings.ron";

/// The online run's script (`{port}` stands for the server's port) and
/// the server's config.
pub const ONLINE_SCRIPT: &str = "fixtures/autoplay/online.ron";
pub const SERVER_CONFIG: &str = "fixtures/server/e2e.ron";

/// The bot client's name (unlike the game's profile's).
const BOT: &str = "e2e bot";

/// Where the runs write (the local run's data directory and screenshots,
/// the online run's in `online/`); made afresh.
pub const OUT: &str = "target/e2e";

/// The longest a run may take.
const TIMEOUT: Duration = Duration::from_secs(240);

/// The software rendering environment (see the module's doc).
pub const SOFTWARE_GL: [(&str, &str); 3] =
    [("WGPU_BACKEND", "gl"), ("LIBGL_ALWAYS_SOFTWARE", "1"), ("GALLIUM_DRIVER", "llvmpipe")];

/// A virtual X server, stopped when dropped.
struct Xvfb {
    child: Child,
    display: String,
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        stop(&mut self.child);
    }
}

/// Stops Xvfb with SIGTERM, so it removes its lock file (SIGKILL would
/// leave `/tmp/.X<n>-lock` behind), then waits for it.
fn stop(child: &mut Child) {
    let term = Command::new("kill").arg(child.id().to_string()).status();
    if !term.is_ok_and(|s| s.success()) {
        let _ = child.kill();
    }
    let _ = child.wait();
}

/// Starts Xvfb on the first free display from `:90`, 1920 × 1080 (the
/// largest window preset fits); ready once it writes its display number
/// (`-displayfd`). WSLg mounts `/tmp/.X11-unix` read-only, so Xvfb listens
/// on the abstract socket there and the lock files tell free displays.
/// `Ok(None)`: Xvfb is not installed.
fn xvfb() -> Result<Option<Xvfb>, String> {
    for n in 90..130 {
        if Path::new(&format!("/tmp/.X{n}-lock")).exists() {
            continue;
        }
        let child = Command::new("Xvfb")
            .args([&format!(":{n}"), "-screen", "0", "1920x1080x24", "-nolisten", "tcp", "-displayfd", "1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("cannot start Xvfb: {e}")),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        if let Some(out) = child.stdout.take() {
            std::thread::spawn(move || {
                let mut line = String::new();
                let _ = std::io::BufRead::read_line(&mut std::io::BufReader::new(out), &mut line);
                let _ = tx.send(line);
            });
        }
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(line) if line.trim() == n.to_string() => {
                return Ok(Some(Xvfb { child, display: format!(":{n}") }));
            }
            _ => stop(&mut child),
        }
    }
    Err("no free X display from :90 to :129 for Xvfb".into())
}

/// The screenshots `script` takes, by name.
fn shots(script: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in script.split("Screenshot(\"").skip(1) {
        if let Some(end) = part.find('"') {
            out.push(part[..end].to_string());
        }
    }
    out
}

/// What is wrong with a run's stderr: its warning, error and panic lines.
pub fn bad_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("warning:") || t.starts_with("error:") || l.contains("panicked")
        })
        .collect()
}

/// The dev builds `e2e` and `net-nat` run (`target/debug`): `olr-game` and
/// `olr-server`, then `olr-cli` with its dev tools (`nostr-relay`,
/// `stun-server`) in a cargo run of its own: in one run, feature
/// unification would give olr-game and olr-server olr-discovery's test relay
/// too. Both steps build exactly these, so in `full` the second finds
/// them built (no feature flips between them).
pub fn build_binaries(root: &Path) -> Result<(), String> {
    for args in [&["-p", "olr-game", "-p", "olr-server"][..], &["-p", "olr-cli", "--features", "nat-tools"]] {
        let build = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["build", "--quiet"])
            .args(args)
            .current_dir(root)
            .output()
            .map_err(|e| format!("cannot run cargo: {e}"))?;
        if !build.status.success() {
            return Err(String::from_utf8_lossy(&build.stderr).into_owned());
        }
    }
    Ok(())
}

/// Runs both scripts, the local one then the online one; `content` is the
/// imported content directory.
pub fn run(content: Option<&Path>) -> Result<(), String> {
    let root = root();
    let Some(content) = content else {
        println!("skip e2e (no OLX content: --offline)");
        return Ok(());
    };
    build_binaries(&root)?;
    let Some(x) = xvfb()? else {
        println!("skip e2e (Xvfb is not installed)");
        return Ok(());
    };
    let out = root.join(OUT);
    let _ = std::fs::remove_dir_all(&out);
    let started = Instant::now();
    let script = std::fs::read_to_string(root.join(SCRIPT)).map_err(|e| format!("{SCRIPT}: {e}"))?;
    let settings = std::fs::read_to_string(root.join(SETTINGS)).map_err(|e| format!("{SETTINGS}: {e}"))?;
    let local = play(&x, content, &script, &settings, &out, true)?;
    println!(
        "     e2e: {SCRIPT} passed on Xvfb {} in {:.1} s ({} frames)",
        x.display, local.secs, local.frames
    );
    let online = online(&x, content, &settings, &out.join("online"))?;
    println!(
        "     e2e: {ONLINE_SCRIPT} passed against olr-server on port {} in {:.1} s ({} frames); both runs {:.1} s",
        online.port,
        online.game.secs,
        online.game.frames,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// A game's run that passed: how long it took and its frames.
struct Played {
    secs: f32,
    frames: String,
}

/// Plays `script` in the game on `x`, from a fresh `<out>/data` holding
/// `settings` (as `settings.ron`), its screenshots and log in
/// `<out>/shots`; with `--fake-clock` or on the real clock. Fails unless
/// it passed cleanly.
fn play(
    x: &Xvfb,
    content: &Path,
    script: &str,
    settings: &str,
    out: &Path,
    fake_clock: bool,
) -> Result<Played, String> {
    let root = root();
    let data = out.join("data");
    let shot_dir = out.join("shots");
    std::fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;
    let settings_file = data.join("settings.ron");
    std::fs::write(&settings_file, settings).map_err(|e| format!("{}: {e}", settings_file.display()))?;
    let script_file = out.join("script.ron");
    std::fs::write(&script_file, script).map_err(|e| format!("{}: {e}", script_file.display()))?;

    let mut cmd = Command::new(root.join("target/debug/olr-game"));
    cmd.args(["--window-system", "x11", "--no-sound"]);
    if fake_clock {
        cmd.arg("--fake-clock");
    }
    cmd.arg("--autoplay")
        .arg(&script_file)
        .arg("--content")
        .arg(content)
        .arg("--data-dir")
        .arg(&data)
        .arg("--shot-dir")
        .arg(&shot_dir)
        .current_dir(&root)
        .env("DISPLAY", &x.display)
        .env_remove("WAYLAND_DISPLAY")
        .envs(SOFTWARE_GL)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let started = Instant::now();
    let child = cmd.spawn().map_err(|e| format!("cannot start olr-game: {e}"))?;
    let output = wait(child, TIMEOUT)?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let log = std::fs::read_to_string(shot_dir.join("autoplay.jsonl")).unwrap_or_default();
    let tail: Vec<&str> = log.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();
    let report = |why: &str| {
        format!(
            "{why}\n--- autoplay.jsonl (last lines)\n{}\n--- stdout\n{}\n--- stderr\n{}",
            tail.join("\n"),
            stdout.trim(),
            stderr.trim()
        )
    };
    if !output.status.success() {
        return Err(report(&format!("olr-game exited with {}", output.status)));
    }
    if !log.lines().last().is_some_and(|l| l.contains("\"event\":\"pass\"")) {
        return Err(report("the autoplay log does not end in a pass"));
    }
    let bad = bad_lines(&stderr);
    if !bad.is_empty() {
        return Err(report(&format!("the log is not clean: {bad:?}")));
    }
    for name in shots(script) {
        let png = shot_dir.join(format!("{name}.png"));
        if !png.is_file() {
            return Err(report(&format!("no screenshot {}", png.display())));
        }
    }
    let frames = log
        .lines()
        .last()
        .and_then(|l| l.split("\"frame\":").nth(1))
        .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next().map(str::to_string));
    Ok(Played { secs: started.elapsed().as_secs_f32(), frames: frames.unwrap_or_default() })
}

/// The online run's outcome.
struct Online {
    port: u16,
    game: Played,
}

/// A helper process of the online run (the server, the bot client): its
/// output goes to files in the run's directory; killed when dropped, so a
/// failing run leaves nothing behind.
struct Helper {
    name: &'static str,
    child: Child,
    /// Its stdout and stderr files.
    out: PathBuf,
    err: PathBuf,
}

impl Helper {
    fn start(name: &'static str, cmd: &mut Command, dir: &Path) -> Result<Helper, String> {
        let out = dir.join(format!("{name}.out"));
        let err = dir.join(format!("{name}.err"));
        let file = |p: &Path| std::fs::File::create(p).map_err(|e| format!("{}: {e}", p.display()));
        let child = cmd
            .stdin(Stdio::null())
            .stdout(file(&out)?)
            .stderr(file(&err)?)
            .spawn()
            .map_err(|e| format!("cannot start {name}: {e}"))?;
        Ok(Helper { name, child, out, err })
    }

    /// Its stdout and stderr so far.
    fn output(&self) -> (String, String) {
        let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
        (read(&self.out), read(&self.err))
    }

    /// Waits until its stdout has a line `find` accepts, giving it; fails
    /// when it exits first or after `within`.
    fn wait_for(
        &mut self,
        what: &str,
        within: Duration,
        find: impl Fn(&str) -> Option<String>,
    ) -> Result<String, String> {
        let until = Instant::now() + within;
        loop {
            if let Some(found) = self.output().0.lines().find_map(&find) {
                return Ok(found);
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                return Err(self.report(&format!("{} exited with {status} before {what}", self.name)));
            }
            if Instant::now() >= until {
                return Err(self.report(&format!(
                    "no {what} from {} within {} s",
                    self.name,
                    within.as_secs()
                )));
            }
            std::thread::sleep(POLL);
        }
    }

    /// Lets it end: SIGTERM first when `term` (else it ends by itself),
    /// then waits at most `within`; killed past it. Its exit status, if it
    /// ended in time.
    fn end(&mut self, term: bool, within: Duration) -> Option<ExitStatus> {
        if term && matches!(self.child.try_wait(), Ok(None)) {
            let _ = Command::new("kill").arg(self.child.id().to_string()).status();
        }
        let until = Instant::now() + within;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if Instant::now() < until => std::thread::sleep(POLL),
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return None;
                }
            }
        }
    }

    /// `why`, then its output.
    fn report(&self, why: &str) -> String {
        format!("{why}\n{}", self.logs())
    }

    /// Its output, for a failure's report.
    fn logs(&self) -> String {
        let (out, err) = self.output();
        format!("--- {} stdout\n{}\n--- {} stderr\n{}", self.name, out.trim(), self.name, err.trim())
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// How often the online run looks at its helpers.
const POLL: Duration = Duration::from_millis(50);

/// The longest the server may take to listen and the bot client to join.
const READY_WITHIN: Duration = Duration::from_secs(30);

/// The first line of `olr-cli nostr-relay`, before its URL.
const RELAY_LINE: &str = "nostr relay: ";

/// [`SETTINGS`]' text with the relay `url` in place of its empty relay
/// list: the online run's game lists the games on the run's relay.
pub fn online_settings(settings: &str, url: &str) -> Result<String, String> {
    const NONE: &str = "relays: Some([])";
    if !settings.contains(NONE) {
        return Err(format!("{SETTINGS} has no `{NONE}` for the online run's relay"));
    }
    Ok(settings.replace(NONE, &format!("relays: Some([{url:?}])")))
}

/// The chat line `ONLINE_SCRIPT` types, which the server must log.
const CHAT: &str = "hello from the e2e run";

/// The online run in `out`: `olr-cli nostr-relay` on loopback, `olr-server`
/// with [`SERVER_CONFIG`] (`{relay}` replaced; port 0, its data in
/// `<out>/server`), `olr-cli net-client --bot` joined (named [`BOT`]), then
/// the game plays [`ONLINE_SCRIPT`] with the server's port on the real
/// clock, from `settings` naming the relay ([`online_settings`]). Passes
/// when the game passed (as [`play`] checks), the bot client played its
/// match and left by itself (exit 0), the server stopped cleanly on
/// SIGTERM, the server's log shows the three joins (the bot's, the game's
/// by address and as listed), the chat line, a match's start and end, and
/// no helper's output has a warning, error or panic.
fn online(x: &Xvfb, content: &Path, settings: &str, out: &Path) -> Result<Online, String> {
    let root = root();
    let server_dir = out.join("server");
    std::fs::create_dir_all(&server_dir).map_err(|e| format!("{}: {e}", server_dir.display()))?;
    // The run's own relay: loopback only, nothing leaves the machine.
    let mut relay = Helper::start(
        "relay",
        Command::new(root.join("target/debug/olr-cli"))
            .args(["nostr-relay", "--listen", "127.0.0.1:0"])
            .current_dir(&root),
        out,
    )?;
    let url = relay.wait_for("its URL", READY_WITHIN, |l| l.strip_prefix(RELAY_LINE).map(str::to_string))?;
    let config =
        std::fs::read_to_string(root.join(SERVER_CONFIG)).map_err(|e| format!("{SERVER_CONFIG}: {e}"))?;
    let config_file = out.join("server.ron");
    std::fs::write(&config_file, config.replace("{relay}", &url))
        .map_err(|e| format!("{}: {e}", config_file.display()))?;
    let settings = online_settings(settings, &url)?;
    let mut server = Helper::start(
        "server",
        Command::new(root.join("target/debug/olr-server"))
            .arg("--config")
            .arg(&config_file)
            .arg("--content")
            .arg(content)
            .arg("--data-dir")
            .arg(&server_dir)
            .current_dir(&root),
        out,
    )?;
    let port = server.wait_for("a port", READY_WITHIN, |l| {
        let rest = unstamped(l).strip_prefix("listening on port ")?;
        rest.split(' ').next().map(str::to_string)
    })?;
    let port: u16 = port.parse().map_err(|_| server.report(&format!("a bad port {port:?}")))?;
    let mut bot = Helper::start(
        "bot",
        Command::new(root.join("target/debug/olr-cli"))
            .args(["net-client", "--join", &format!("127.0.0.1:{port}"), "--bot", "--name", BOT])
            .args(["--timeout", &TIMEOUT.as_secs().to_string()])
            .arg("--content")
            .arg(content)
            .current_dir(&root),
        out,
    )?;
    // The bot is in the lobby before the game joins: the game's lobby
    // shows both at once (the script counts them).
    let joined = format!("join: {BOT} (");
    server
        .wait_for("the bot's join", READY_WITHIN, |l| unstamped(l).starts_with(&joined).then(String::new))?;

    let script =
        std::fs::read_to_string(root.join(ONLINE_SCRIPT)).map_err(|e| format!("{ONLINE_SCRIPT}: {e}"))?;
    let script = script.replace("{port}", &port.to_string());
    let helpers = |why: String, server: &Helper, bot: &Helper, relay: &Helper| {
        format!("{why}\n{}\n{}\n{}", server.logs(), bot.logs(), relay.logs())
    };
    let game = match play(x, content, &script, &settings, out, false) {
        Ok(g) => g,
        Err(why) => return Err(helpers(why, &server, &bot, &relay)),
    };
    // The bot client leaves after its match; the server stops on SIGTERM,
    // then the relay (which runs until killed).
    let bot_status = bot.end(false, Duration::from_secs(15));
    let server_status = server.end(true, Duration::from_secs(15));
    // The relay serves until killed: it must still run, then stop on SIGTERM.
    let relay_ran = matches!(relay.child.try_wait(), Ok(None));
    let relay_status = relay.end(true, Duration::from_secs(5));
    let fail = |why: &str| Err(helpers(why.to_string(), &server, &bot, &relay));
    if !relay_ran {
        return fail("the relay exited before the run's end");
    }
    if relay_status.is_none() {
        return fail("the relay did not stop on SIGTERM: killed");
    }
    match bot_status {
        Some(s) if s.success() => {}
        Some(s) => return fail(&format!("the bot client exited with {s}")),
        None => return fail("the bot client did not leave after its match: killed"),
    }
    match server_status {
        Some(s) if s.success() => {}
        Some(s) => return fail(&format!("the server exited with {s} on SIGTERM")),
        None => return fail("the server did not stop on SIGTERM: killed"),
    }
    let (server_out, server_err) = server.output();
    let (bot_out, bot_err) = bot.output();
    let (relay_out, relay_err) = relay.output();
    let server_log: Vec<&str> = server_out.lines().map(unstamped).collect();
    let server_text = server_log.join("\n");
    let mut bad = bad_lines(&server_text);
    for text in [&server_err, &bot_out, &bot_err, &relay_out, &relay_err] {
        bad.extend(bad_lines(text));
    }
    if !bad.is_empty() {
        return fail(&format!("the helpers' logs are not clean: {bad:?}"));
    }
    let has = |prefix: &str| server_log.iter().any(|l| l.starts_with(prefix));
    let chat = format!(": {CHAT}");
    let wanted = [
        // The bot's, then the game's by address and as listed.
        ("the game's two joins", server_log.iter().filter(|l| l.starts_with("join: ")).count() == 3),
        ("the chat line", server_log.iter().any(|l| l.starts_with("chat ") && l.ends_with(&chat))),
        ("a match start", has("match start: ")),
        ("a match end", has("match end ")),
        ("a clean stop", server_log.last().is_some_and(|l| *l == "server stopped")),
    ];
    if let Some((what, _)) = wanted.iter().find(|(_, seen)| !seen) {
        return fail(&format!("the server's log lacks {what}"));
    }
    if !bot_out.lines().any(|l| l.starts_with("match 1: ")) {
        return fail("the bot client finished no match");
    }
    Ok(Online { port, game })
}

/// A server log line without its UTC stamp (`YYYY-MM-DD HH:MM:SS `).
pub fn unstamped(line: &str) -> &str {
    let b = line.as_bytes();
    let stamped = b.len() >= 20
        && b[..19].iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b' ',
            13 | 16 => *c == b':',
            _ => c.is_ascii_digit(),
        })
        && b[19] == b' ';
    if stamped { &line[20..] } else { line }
}

/// Waits for `child` and its output, killing it after `timeout`.
fn wait(child: Child, timeout: Duration) -> Result<std::process::Output, String> {
    let id = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(out) => out.map_err(|e| format!("waiting for olr-game: {e}")),
        Err(_) => {
            let _ = Command::new("kill").args(["-9", &id.to_string()]).status();
            Err(format!("olr-game ran over {} s: killed", timeout.as_secs()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_names_its_screenshots_and_the_log_is_checked() {
        assert_eq!(
            shots(r#"(steps: [(ui: Screenshot("a")), (ui: Click("b")), (ui: Screenshot("c-1"))])"#),
            ["a", "c-1"]
        );
        let script = std::fs::read_to_string(root().join(SCRIPT)).unwrap();
        assert!(shots(&script).len() >= 5);
        // The online script types the server's port and the chat line the
        // server's log must show.
        let online = std::fs::read_to_string(root().join(ONLINE_SCRIPT)).unwrap();
        assert!(online.contains(r#"Type("127.0.0.1:{port}")"#));
        // Then the server as listed on the run's relay.
        assert!(online.contains(r#"ClickUntil("e2e server", "Join")"#));
        assert!(online.contains(&format!("Type({CHAT:?})")));
        assert!(shots(&online).contains(&"online".to_string()));
        let err =
            "olr-game: GPU x\nwarning: no gamepads\nthread 'main' panicked at x\nerror: y\nMesa: fine\n";
        assert_eq!(bad_lines(err), ["warning: no gamepads", "thread 'main' panicked at x", "error: y"]);
    }

    /// The online run's game lists the games on the run's relay; the
    /// server is listed there.
    #[test]
    fn the_online_run_names_its_relay() {
        let settings = std::fs::read_to_string(root().join(SETTINGS)).unwrap();
        let online = online_settings(&settings, "ws://127.0.0.1:7777").unwrap();
        assert!(online.contains(r#"relays: Some(["ws://127.0.0.1:7777"])"#), "{online}");
        assert!(online_settings("(settings: ())", "ws://x").is_err());
        let server = std::fs::read_to_string(root().join(SERVER_CONFIG)).unwrap();
        assert!(server.contains(r#"relays: ["{relay}"]"#) && server.contains("visibility: Public"));
    }

    #[test]
    fn server_lines_lose_their_stamp() {
        assert_eq!(
            unstamped("2026-10-13 01:30:28 listening on port 39214 (TCP and UDP)"),
            "listening on port 39214 (TCP and UDP)"
        );
        assert_eq!(unstamped("2026-10-13 01:30:28 warning: x"), "warning: x");
        assert_eq!(unstamped("Ctrl+C or quit stops the server"), "Ctrl+C or quit stops the server");
        assert_eq!(unstamped("2026-10-13"), "2026-10-13");
    }
}
