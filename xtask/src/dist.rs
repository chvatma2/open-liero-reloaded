//! `cargo xtask dist`: the portable release zips. `rl-game` and
//! `rl-server` are built in the `dist` profile (release without symbols or
//! debug info) for Linux and for Windows (mingw-w64, as `winbuild`), each
//! staged under one folder `rust-liero-<version>/` with the content, the
//! docs and an example server config (the mingw DLLs only when an exe
//! imports them), and zipped into `target/dist/rust-liero-<version>-
//! <platform>.zip`: deflate, entries in byte order, fixed times and modes
//! (0755 for the executables), so the same inputs give the same zip.
//!
//! Then, unless `--no-verify`, each zip is unpacked into a fresh directory
//! and its `rl-server` runs from another working directory without
//! `--content` (invite only, no relays, no STUN: nothing leaves the
//! machine) until its log shows the rotation's map loaded from the content
//! beside it; on Linux a bot client (`rl-cli net-client --bot`, the dev
//! build) joins and the server must start a match. `rl-game --help` must
//! name the content beside it as the default. The Windows zip runs in
//! `%TEMP%\rust-liero-envelope\dist` through WSL interop (skipped with a
//! notice without it) and is stopped by `winbuild`'s stage-scoped stop.

use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::{e2e, root, winbuild};

/// The workspace version (xtask's is the workspace's).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The Cargo profile of the zips' builds.
pub const PROFILE: &str = "dist";
/// The programs a zip holds (`.exe` on Windows).
pub const EXES: [&str; 2] = ["rl-game", "rl-server"];
/// Documents copied from the repository, at the same path in the folder.
pub const DOCS: [&str; 4] = ["README.md", "LICENSE", "CHANGELOG.md", "docs/online.md"];
/// The documented server config, and its name in the folder.
pub const SERVER_EXAMPLE: (&str, &str) = ("fixtures/server/example.ron", "server.example.ron");
/// The content, at the same path in the folder (where the programs look).
pub const CONTENT: &str = "content/packs";
/// Files at the top of the content the programs never read.
pub const CONTENT_LEFT_OUT: [&str; 1] = ["import-report.ron"];
/// How long a verification waits for the server to load its rotation.
const READY_WITHIN: Duration = Duration::from_secs(60);
/// How long the Linux verification waits for the bot client's match.
const MATCH_WITHIN: Duration = Duration::from_secs(90);
/// The server config the verification runs (no network beyond the machine).
const CHECK_CONFIG: &str = "// `cargo xtask dist`'s check: by address only, nothing announced, no STUN.
(
    name: \"dist check\",
    visibility: Invite,
    relays: [],
    stun: [],
    port: 0,
    max_players: 2,
    max_spectators: 0,
    fill_bots: 2,
    min_players: 1,
    rotation: [
        (mod: \"olx-classic\", level: \"olx-level-dirt-level\", time_limit: 0.2, kill_limit: 0, lives: -2, bonuses: true),
    ],
    start_when_ready: true,
)
";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    Linux,
    Windows,
}

impl Platform {
    pub const ALL: [Platform; 2] = [Platform::Linux, Platform::Windows];

    /// The zip name's platform part.
    pub fn suffix(self) -> &'static str {
        match self {
            Platform::Linux => "linux-x64",
            Platform::Windows => "windows-x64",
        }
    }

    /// The program `name`'s file name.
    pub fn exe(self, name: &str) -> String {
        match self {
            Platform::Linux => name.to_string(),
            Platform::Windows => format!("{name}.exe"),
        }
    }

    /// The cross target (none: the host's).
    fn target(self) -> Option<&'static str> {
        match self {
            Platform::Linux => None,
            Platform::Windows => Some(winbuild::TARGET),
        }
    }
}

/// The zip's one top folder.
pub fn folder(version: &str) -> String {
    format!("rust-liero-{version}")
}

/// The zip's file name.
pub fn zip_name(version: &str, platform: Platform) -> String {
    format!("{}-{}.zip", folder(version), platform.suffix())
}

/// The files of the folder besides the content, as (source, path in the
/// folder): the programs from `build` (cargo's output directory), the
/// mingw DLLs `dlls` (name, file), the documents and the example config
/// from the repository at `repo`.
pub fn files(
    platform: Platform,
    build: &Path,
    dlls: &[(String, PathBuf)],
    repo: &Path,
) -> Vec<(PathBuf, String)> {
    let mut out: Vec<(PathBuf, String)> = EXES
        .iter()
        .map(|e| (build.join(platform.exe(e)), platform.exe(e)))
        .chain(dlls.iter().map(|(name, file)| (file.clone(), name.clone())))
        .collect();
    out.extend(DOCS.iter().map(|d| (repo.join(d), (*d).to_string())));
    out.push((repo.join(SERVER_EXAMPLE.0), SERVER_EXAMPLE.1.to_string()));
    out
}

/// The folder's executables (mode 0755 in the zip), as paths in the folder.
pub fn executables(platform: Platform) -> Vec<String> {
    EXES.iter().map(|e| platform.exe(e)).collect()
}

/// Copies the content directory `from` to `to`, leaving out `CONTENT_LEFT_OUT`.
pub fn copy_content(from: &Path, to: &Path) -> Result<(), String> {
    copy_dir(from, to, &CONTENT_LEFT_OUT)
}

/// Copies the directory `from` to `to`, leaving out the files named `skip` at its top.
fn copy_dir(from: &Path, to: &Path, skip: &[&str]) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| format!("{}: {e}", from.display()))?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let kind = e.file_type().map_err(|e| format!("{}: {e}", src.display()))?;
        if kind.is_dir() {
            copy_dir(&src, &dst, &[])?;
        } else if !skip.iter().any(|s| e.file_name() == **s) {
            copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::copy(from, to)
        .map(drop)
        .map_err(|e| format!("copying {} to {}: {e}", from.display(), to.display()))
}

/// Every file below `dir`, as `/`-separated paths relative to it, in byte order.
fn list_files(dir: &Path) -> Result<Vec<String>, String> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<(), String> {
        for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let e = e.map_err(|e| format!("{}: {e}", dir.display()))?;
            let name = e.file_name().into_string().map_err(|n| format!("{n:?}: not UTF-8"))?;
            let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
            if e.file_type().map_err(|err| format!("{}: {err}", e.path().display()))?.is_dir() {
                walk(&e.path(), &rel, out)?;
            } else {
                out.push(rel);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, "", &mut out)?;
    out.sort();
    Ok(out)
}

/// Writes the zip `out` of the directory `folder` (its name the zip's top
/// folder): every file below it in byte order of its path, deflated, dated
/// 1980-01-01 00:00, mode 0755 for `executables` (paths in the folder) and
/// 0644 for the rest.
pub fn write_zip(folder: &Path, executables: &[String], out: &Path) -> Result<(), String> {
    use zip::write::SimpleFileOptions;
    let top = folder
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: no folder name", folder.display()))?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let part = out.with_extension("zip.part");
    let file = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let fail = |e: &dyn std::fmt::Display| format!("writing {}: {e}", part.display());
    for rel in list_files(folder)? {
        let mode = if executables.contains(&rel) { 0o755 } else { 0o644 };
        let options = SimpleFileOptions::DEFAULT
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::DEFAULT)
            .unix_permissions(mode);
        zip.start_file(format!("{top}/{rel}"), options).map_err(|e| fail(&e))?;
        let src = folder.join(&rel);
        let mut f = std::fs::File::open(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        std::io::copy(&mut f, &mut zip).map_err(|e| fail(&e))?;
    }
    let mut w = zip.finish().map_err(|e| fail(&e))?;
    w.flush().map_err(|e| fail(&e))?;
    drop(w);
    std::fs::rename(&part, out).map_err(|e| format!("{} to {}: {e}", part.display(), out.display()))
}

/// Unpacks the zip `zip` into the directory `into`.
pub fn unpack(zip: &Path, into: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    archive.extract(into).map_err(|e| format!("unpacking {} into {}: {e}", zip.display(), into.display()))
}

/// Builds `rl-game` and `rl-server` for `platform` in the `dist` profile;
/// gives cargo's output directory.
fn build(platform: Platform) -> Result<PathBuf, String> {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root()).args(["build", "--quiet", "--profile", PROFILE]);
    for exe in EXES {
        c.args(["-p", exe]);
    }
    if let Some(target) = platform.target() {
        c.args(["--target", target]).env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", winbuild::LINKER);
    }
    let status = c.status().map_err(|e| format!("cannot run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("the {} build failed ({status})", platform.suffix()));
    }
    let target = root().join("target");
    Ok(match platform.target() {
        Some(t) => target.join(t).join(PROFILE),
        None => target.join(PROFILE),
    })
}

/// Stages the folder for `platform` in `dir` (emptied first) from cargo's
/// output directory `build`; gives the folder.
fn stage(platform: Platform, build: &Path, dir: &Path) -> Result<PathBuf, String> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("{}: {e}", dir.display()));
        }
        _ => {}
    }
    let folder = dir.join(folder(VERSION));
    let mut dlls: Vec<(String, PathBuf)> = Vec::new();
    if platform == Platform::Windows {
        for exe in EXES {
            for found in winbuild::mingw_imports(&build.join(platform.exe(exe)))? {
                if !dlls.iter().any(|(name, _)| name.eq_ignore_ascii_case(&found.0)) {
                    dlls.push(found);
                }
            }
        }
        dlls.sort();
    }
    let repo = root();
    for (src, rel) in files(platform, build, &dlls, &repo) {
        copy(&src, &folder.join(&rel))?;
    }
    copy_content(&repo.join(CONTENT), &folder.join(CONTENT))?;
    Ok(folder)
}

/// Lines a child prints (stdout and stderr), read as they come.
struct Watched {
    name: String,
    child: Child,
    lines: Receiver<String>,
    seen: Vec<String>,
}

impl Watched {
    fn start(name: &str, cmd: &mut Command) -> Result<Watched, String> {
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start {name}: {e}"))?;
        let (tx, rx) = mpsc::channel();
        fn forward(pipe: Option<impl Read + Send + 'static>, tx: mpsc::Sender<String>) {
            let Some(pipe) = pipe else { return };
            std::thread::spawn(move || {
                let mut r = BufReader::new(pipe);
                let mut buf = Vec::new();
                while matches!(r.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
                    let line = String::from_utf8_lossy(&buf).trim_end_matches(['\r', '\n']).to_string();
                    if tx.send(line).is_err() {
                        return;
                    }
                    buf.clear();
                }
            });
        }
        forward(child.stdout.take(), tx.clone());
        forward(child.stderr.take(), tx);
        Ok(Watched { name: name.to_string(), child, lines: rx, seen: Vec::new() })
    }

    /// Waits up to `within` for a line (its UTC stamp removed) `found` takes.
    fn wait_for<T>(
        &mut self,
        what: &str,
        within: Duration,
        found: impl Fn(&str) -> Option<T>,
    ) -> Result<T, String> {
        let deadline = Instant::now() + within;
        loop {
            match self.lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(line) => {
                    let hit = found(e2e::unstamped(&line));
                    self.seen.push(line);
                    if let Some(v) = hit {
                        return Ok(v);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(self.report(&format!("no {what} within {} s", within.as_secs())));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let status = self.child.wait().map_or_else(|e| e.to_string(), |s| s.to_string());
                    return Err(self.report(&format!("{} ended ({status}) before {what}", self.name)));
                }
            }
        }
    }

    /// The lines read so far and any still waiting.
    fn drain(&mut self) {
        while let Ok(line) = self.lines.try_recv() {
            self.seen.push(line);
        }
    }

    /// Its warning, error and panic lines.
    fn bad_lines(&self) -> Vec<String> {
        e2e::bad_lines(&self.seen.iter().map(|l| e2e::unstamped(l)).collect::<Vec<_>>().join("\n"))
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn report(&self, why: &str) -> String {
        let tail = &self.seen[self.seen.len().saturating_sub(30)..];
        format!("{why}; {} printed:\n{}", self.name, tail.join("\n"))
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The server's port, from its "listening on port N" line.
fn listening_port(line: &str) -> Option<u16> {
    line.strip_prefix("listening on port ")?.split(' ').next()?.parse().ok()
}

/// A fresh directory for an unpacked zip and the run's working directory,
/// the check's server config written there; gives (folder, run directory).
fn unpack_fresh(zip: &Path, base: &Path) -> Result<(PathBuf, PathBuf), String> {
    match std::fs::remove_dir_all(base) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("{}: {e}", base.display()));
        }
        _ => {}
    }
    let (unpacked, run) = (base.join("unpacked"), base.join("run"));
    unpack(zip, &unpacked)?;
    std::fs::create_dir_all(&run).map_err(|e| format!("{}: {e}", run.display()))?;
    std::fs::write(run.join("server.ron"), CHECK_CONFIG).map_err(|e| format!("{}: {e}", run.display()))?;
    Ok((unpacked.join(folder(VERSION)), run))
}

/// The Linux zip, unpacked in a temporary directory and run from another.
fn verify_linux(zip: &Path) -> Result<String, String> {
    let tmp = tempfile::Builder::new()
        .prefix("rust-liero-dist-")
        .tempdir()
        .map_err(|e| format!("a temporary directory: {e}"))?;
    let (folder, run) = unpack_fresh(zip, tmp.path())?;
    #[cfg(unix)]
    for exe in executables(Platform::Linux) {
        use std::os::unix::fs::PermissionsExt as _;
        let mode =
            std::fs::metadata(folder.join(&exe)).map_err(|e| format!("{exe}: {e}"))?.permissions().mode();
        if mode & 0o777 != 0o755 {
            return Err(format!("{exe} unpacked with mode {:o}, not 755", mode & 0o777));
        }
    }
    let folder = folder.canonicalize().map_err(|e| format!("{}: {e}", folder.display()))?;
    let help = Command::new(folder.join("rl-game"))
        .arg("--help")
        .current_dir(&run)
        .output()
        .map_err(|e| format!("cannot run rl-game --help: {e}"))?;
    let text = String::from_utf8_lossy(&help.stdout);
    let beside = format!("[default: {}]", folder.join("content").join("packs").display());
    if !help.status.success() || !text.contains(&beside) {
        return Err(format!(
            "rl-game --help ({}) does not show {beside}:\n{text}{}",
            help.status,
            String::from_utf8_lossy(&help.stderr)
        ));
    }

    // The bot client: the dev build, the repository's content.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let built = Command::new(cargo)
        .current_dir(root())
        .args(["build", "--quiet", "-p", "rl-cli"])
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !built.success() {
        return Err(format!("building rl-cli failed ({built})"));
    }
    let mut server = Watched::start(
        "rl-server",
        Command::new(folder.join("rl-server")).args(["--config", "server.ron"]).current_dir(&run),
    )?;
    let port = server.wait_for("listening line", READY_WITHIN, listening_port)?;
    let map =
        server.wait_for("rotation", READY_WITHIN, |l| l.strip_prefix("next map: ").map(String::from))?;
    let mut bot = Watched::start(
        "the bot client",
        Command::new(root().join("target/debug/rl-cli"))
            .args(["net-client", "--join", &format!("127.0.0.1:{port}"), "--bot", "--name", "dist bot"])
            .args(["--timeout", &MATCH_WITHIN.as_secs().to_string()])
            .arg("--content")
            .arg(root().join(CONTENT))
            .current_dir(root()),
    )?;
    let started = server
        .wait_for("match start", MATCH_WITHIN, |l| l.strip_prefix("match start: ").map(String::from))
        .map_err(|why| {
            bot.drain();
            format!("{why}\n{}", bot.report("the bot client"))
        })?;
    // SIGTERM: the server says goodbye and stops cleanly.
    let pid = server.child.id().to_string();
    let _ = Command::new("kill").args(["-TERM", &pid]).status();
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match server.child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => break None,
        }
    };
    drop(bot);
    // The pipes close at its exit; their last lines follow.
    let _ = server.wait_for("end of output", Duration::from_secs(2), |_| None::<()>);
    server.drain();
    match status {
        Some(s) if s.success() => {}
        Some(s) => return Err(server.report(&format!("rl-server exited with {s} on SIGTERM"))),
        None => return Err(server.report("rl-server did not stop on SIGTERM: killed")),
    }
    let bad = server.bad_lines();
    if !bad.is_empty() {
        return Err(server.report(&format!("rl-server's output is not clean: {bad:?}")));
    }
    Ok(format!(
        "rl-game --help names the content beside it; rl-server loaded {map} and started a match ({started})"
    ))
}

/// The Windows zip, unpacked under the stage in Windows' temp directory
/// and run from another directory there through interop; `Ok(None)` when
/// interop is missing (a notice).
fn verify_windows(zip: &Path) -> Result<Option<String>, String> {
    if let Some(why) = winbuild::interop_missing() {
        println!("skip dist verification on Windows ({why})");
        return Ok(None);
    }
    let stage = winbuild::Stage { dir: winbuild::windows_temp()?.join(winbuild::STAGE) };
    std::fs::create_dir_all(&stage.dir).map_err(|e| format!("{}: {e}", stage.dir.display()))?;
    // A run an interrupted xtask left behind would hold the files.
    winbuild::stop_stage_processes(&stage.dir)?;
    let (folder, run) = unpack_fresh(zip, &stage.dir.join("dist"))?;
    let help = winbuild::run_in(
        &stage,
        &folder.join("rl-game.exe"),
        &run,
        &["--help".to_string()],
        &[],
        READY_WITHIN,
    )?;
    let text = String::from_utf8_lossy(&help.stdout);
    let beside = format!("{}\\content\\packs]", self::folder(VERSION));
    if !text.lines().any(|l| l.contains("[default: ") && l.trim_end().ends_with(&beside)) {
        return Err(format!("rl-game.exe --help does not show the content beside it (…{beside}):\n{text}"));
    }
    let mut server = Watched::start(
        "rl-server.exe",
        Command::new(folder.join("rl-server.exe")).args(["--config", "server.ron"]).current_dir(&run),
    )?;
    let ready = server.wait_for("listening line", READY_WITHIN, listening_port).and_then(|_| {
        server.wait_for("rotation", READY_WITHIN, |l| l.strip_prefix("next map: ").map(String::from))
    });
    // Stopped by path under the stage, then the interop child.
    let stopped = winbuild::stop_stage_processes(&stage.dir);
    let _ = server.child.kill();
    let _ = server.child.wait();
    let map = ready?;
    stopped?;
    server.drain();
    let bad = server.bad_lines();
    if !bad.is_empty() {
        return Err(server.report(&format!("rl-server.exe's output is not clean: {bad:?}")));
    }
    Ok(Some(format!("rl-game.exe --help names the content beside it; rl-server.exe loaded {map}")))
}

/// `cargo xtask dist`: both zips, then (unless `verify` is off) their checks.
pub fn command(verify: bool) -> anyhow::Result<bool> {
    if let Some(why) = winbuild::build_missing() {
        println!("FAIL dist: the Windows build cannot be made here: {why}");
        return Ok(false);
    }
    let target = root().join("target");
    let staging = target.join("dist-stage");
    let mut zips = Vec::new();
    for platform in Platform::ALL {
        println!(
            "building rl-game and rl-server for {} (the {PROFILE} profile: LTO, minutes from cold)...",
            platform.suffix()
        );
        let build = build(platform).map_err(anyhow::Error::msg)?;
        let folder = stage(platform, &build, &staging.join(platform.suffix())).map_err(anyhow::Error::msg)?;
        let zip = target.join(PROFILE).join(zip_name(VERSION, platform));
        write_zip(&folder, &executables(platform), &zip).map_err(anyhow::Error::msg)?;
        zips.push((platform, zip));
    }
    let _ = std::fs::remove_dir_all(&staging);
    let mut ok = true;
    if verify {
        for (platform, zip) in &zips {
            println!("checking {} ...", zip.display());
            let result = match platform {
                Platform::Linux => verify_linux(zip).map(Some),
                Platform::Windows => verify_windows(zip),
            };
            match result {
                Ok(Some(what)) => println!("ok   dist {}: {what}", platform.suffix()),
                Ok(None) => {}
                Err(why) => {
                    println!("FAIL dist {}: {why}", platform.suffix());
                    ok = false;
                }
            }
        }
    }
    for (_, zip) in &zips {
        let bytes = std::fs::metadata(zip).map_or(0, |m| m.len());
        println!("{}  {:.1} MiB", zip.display(), bytes as f64 / (1024.0 * 1024.0));
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_carry_the_workspace_version() {
        assert_eq!(folder("1.2.3"), "rust-liero-1.2.3");
        assert_eq!(zip_name("1.2.3", Platform::Windows), "rust-liero-1.2.3-windows-x64.zip");
        assert_eq!(zip_name("1.2.3", Platform::Linux), "rust-liero-1.2.3-linux-x64.zip");
        let meta = cargo_metadata::MetadataCommand::new()
            .manifest_path(root().join("Cargo.toml"))
            .no_deps()
            .exec()
            .unwrap();
        for name in EXES {
            let p = meta.packages.iter().find(|p| p.name.as_str() == name).unwrap();
            assert_eq!(p.version.to_string(), VERSION, "{name}");
        }
    }

    #[test]
    fn the_folder_holds_the_programs_dlls_docs_and_example() {
        let dlls = [("libwinpthread-1.dll".to_string(), PathBuf::from("/m/libwinpthread-1.dll"))];
        let got = files(Platform::Windows, Path::new("/b"), &dlls, Path::new("/r"));
        let want: Vec<(PathBuf, String)> = [
            ("/b/rl-game.exe", "rl-game.exe"),
            ("/b/rl-server.exe", "rl-server.exe"),
            ("/m/libwinpthread-1.dll", "libwinpthread-1.dll"),
            ("/r/README.md", "README.md"),
            ("/r/LICENSE", "LICENSE"),
            ("/r/CHANGELOG.md", "CHANGELOG.md"),
            ("/r/docs/online.md", "docs/online.md"),
            ("/r/fixtures/server/example.ron", "server.example.ron"),
        ]
        .into_iter()
        .map(|(s, d)| (PathBuf::from(s), d.to_string()))
        .collect();
        assert_eq!(got, want);
        let linux = files(Platform::Linux, Path::new("/b"), &[], Path::new("/r"));
        assert_eq!(
            linux[..2],
            [
                (PathBuf::from("/b/rl-game"), "rl-game".into()),
                (PathBuf::from("/b/rl-server"), "rl-server".into())
            ]
        );
        assert_eq!(linux.len(), 7);
        assert_eq!(executables(Platform::Windows), ["rl-game.exe", "rl-server.exe"]);
        assert_eq!(executables(Platform::Linux), ["rl-game", "rl-server"]);
    }

    #[test]
    fn the_content_leaves_out_the_import_report() {
        let t = tempfile::tempdir().unwrap();
        let (from, to) = (t.path().join("packs"), t.path().join("out/content/packs"));
        std::fs::create_dir_all(from.join("olx-core/sprites")).unwrap();
        std::fs::write(from.join("import-report.ron"), "()").unwrap();
        std::fs::write(from.join("olx-core/pack.ron"), "(id: \"olx-core\")").unwrap();
        std::fs::write(from.join("olx-core/sprites/a.png"), "png").unwrap();
        // Only the top's report is left out.
        std::fs::write(from.join("olx-core/import-report.ron"), "kept").unwrap();
        copy_content(&from, &to).unwrap();
        assert!(!to.join("import-report.ron").exists());
        assert_eq!(std::fs::read_to_string(to.join("olx-core/pack.ron")).unwrap(), "(id: \"olx-core\")");
        assert_eq!(std::fs::read_to_string(to.join("olx-core/sprites/a.png")).unwrap(), "png");
        assert_eq!(std::fs::read_to_string(to.join("olx-core/import-report.ron")).unwrap(), "kept");
    }

    #[test]
    fn the_zip_is_ordered_dated_and_moded_the_same_every_time() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("rust-liero-9.9.9");
        for (path, text) in [
            ("rl-server", "server"),
            ("rl-game", "game"),
            ("README.md", "readme"),
            ("docs/online.md", "online"),
            ("content/packs/olx-core/pack.ron", "core"),
            ("content/packs/b/x.png", "png"),
            ("server.example.ron", "()"),
        ] {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
        }
        let exes = executables(Platform::Linux);
        let (a, b) = (t.path().join("a.zip"), t.path().join("b.zip"));
        write_zip(&dir, &exes, &a).unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&a).unwrap()).unwrap();
        let mut entries = Vec::new();
        for i in 0..archive.len() {
            let mut f = archive.by_index(i).unwrap();
            let when = f.last_modified().unwrap();
            assert_eq!(
                (when.year(), when.month(), when.day(), when.hour(), when.minute()),
                (1980, 1, 1, 0, 0)
            );
            assert_eq!(f.compression(), zip::CompressionMethod::Deflated, "{}", f.name());
            let mut text = String::new();
            f.read_to_string(&mut text).unwrap();
            entries.push((f.name().to_string(), f.unix_mode().unwrap() & 0o777, text));
        }
        let want = [
            ("rust-liero-9.9.9/README.md", 0o644, "readme"),
            ("rust-liero-9.9.9/content/packs/b/x.png", 0o644, "png"),
            ("rust-liero-9.9.9/content/packs/olx-core/pack.ron", 0o644, "core"),
            ("rust-liero-9.9.9/docs/online.md", 0o644, "online"),
            ("rust-liero-9.9.9/rl-game", 0o755, "game"),
            ("rust-liero-9.9.9/rl-server", 0o755, "server"),
            ("rust-liero-9.9.9/server.example.ron", 0o644, "()"),
        ]
        .map(|(n, m, t)| (n.to_string(), m, t.to_string()));
        assert_eq!(entries, want);
        // The same inputs, the same bytes (a file's own time changes nothing).
        let f = std::fs::File::options().write(true).open(dir.join("README.md")).unwrap();
        f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(2_000_000_000)).unwrap();
        write_zip(&dir, &exes, &b).unwrap();
        assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
        // And it unpacks with the modes.
        let out = t.path().join("unpacked");
        unpack(&a, &out).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = |p: &str| std::fs::metadata(out.join(p)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode("rust-liero-9.9.9/rl-server"), 0o755);
            assert_eq!(mode("rust-liero-9.9.9/README.md"), 0o644);
        }
    }
}
