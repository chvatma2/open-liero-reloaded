//! `cargo xtask dist`: the portable release zips. `olr-game` and
//! `olr-server` are built in the `dist` profile (release without symbols or
//! debug info) for Linux and for Windows (mingw-w64, as `winbuild`), each
//! staged under one folder `open-liero-reloaded-<version>/` with the content
//! (`content/packs`, and OLR's own packs beside it, `content/reloaded`), the
//! docs and an example server config (the mingw DLLs only when an exe
//! imports them), and zipped into `target/release-zips/open-liero-reloaded-<version>-
//! <platform>.zip`: deflate, entries in byte order, fixed times and modes
//! (0755 for the executables), so the same inputs give the same zip.
//! `OLR_REPORT_URL` in the environment (an `https://` address) is the
//! builds' bug relay (issue #3, `tools/bug-relay/README.md`); without it
//! the game hides Report a bug. Every build carries `OLR_RELEASE_REPO`
//! (issue #4): it checks the repository's releases for updates. The
//! folder lists its files in `install-manifest.txt`, so the Windows
//! updater knows which files a release installed.
//!
//! Then, unless `--no-verify`, each zip is unpacked into a fresh directory
//! and its `olr-server` runs from another working directory without
//! `--content` (invite only, no relays, no STUN: nothing leaves the
//! machine) until its log shows the rotation's map loaded from the content
//! beside it; on Linux a bot client (`olr-cli net-client --bot`, the dev
//! build) joins and the server must start a match. `olr --help` must
//! name the content beside it as the default, and the folder must hold the
//! Reloaded levels (issue #7), which the server's rotation names too, so
//! it loads them at its start. The Windows zip runs in
//! `%TEMP%\olr-envelope\dist` through WSL interop (skipped with a
//! notice without it) and is stopped by `winbuild`'s stage-scoped stop.

use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::{e2e, root, winbuild};

/// The workspace version (xtask's is the workspace's).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The Cargo profile of the zips' builds (its output: `target/dist`).
pub const PROFILE: &str = "dist";
/// Where the zips go, under `target/`: apart from cargo's output, so
/// `tidy` sweeps that and never this (`tidy::KEPT`).
pub const ZIP_DIR: &str = "release-zips";
/// The programs a zip holds (`.exe` on Windows).
pub const EXES: [&str; 2] = ["olr", "olr-server"];
/// The packages that build [`EXES`], in the same order.
pub const PACKAGES: [&str; 2] = ["olr-game", "olr-server"];
/// Documents copied from the repository, at the same path in the folder.
pub const DOCS: [&str; 4] = ["README.md", "LICENSE", "CHANGELOG.md", "docs/online.md"];
/// The documented server config, and its name in the folder.
pub const SERVER_EXAMPLE: (&str, &str) = ("fixtures/server/example.ron", "server.example.ron");
/// The content, at the same path in the folder (where the programs look).
pub const CONTENT: &str = "content/packs";
/// Files at the top of the content the programs never read.
pub const CONTENT_LEFT_OUT: [&str; 1] = ["import-report.ron"];
/// OLR's own packs (issue #7: the Reloaded levels), beside the content
/// (`olr_content::RELOADED_DIR`) at the same path in the folder.
pub const RELOADED: &str = "content/reloaded";
/// Files at the top of OLR's own packs the programs never read.
pub const RELOADED_LEFT_OUT: [&str; 1] = ["README.md"];
/// The level editor's built-in assets (issue #9), beside the content
/// (`olr_content::ASSETS_DIR`) at the same path in the folder.
pub const ASSETS: &str = "content/assets";
/// Files at the top of the built-in assets the programs never read.
pub const ASSETS_LEFT_OUT: [&str; 1] = ["README.md"];
/// The built-in surface materials (issue #37), beside the content
/// (`olr_content::MATERIALS_DIR`) at the same path in the folder.
pub const MATERIALS: &str = "content/materials";
/// Files of the built-in materials the programs never read.
pub const MATERIALS_LEFT_OUT: [&str; 1] = ["README.md"];
/// The longest path, in UTF-16 units as Windows counts them, a file may
/// have below the zip's top folder (issue #64: `content/…`, `olr.exe`).
/// Explorer's Extract All stops at `MAX_PATH` (259) and puts the zip's
/// name before its top folder, so the version is in the path twice
/// (about 123 characters before `content\` for a short user name); 80
/// leaves room for a longer user name, OneDrive and the updater's
/// `.olr-update\new\`.
pub const MAX_SHIPPED_PATH: usize = 80;
/// The levels in [`RELOADED`] a release must hold: the verification
/// finds each in the folder, and its server's rotation names them.
pub const RELOADED_LEVELS: [&str; 2] =
    ["olr-level-liero-factory-reloaded", "olr-level-castlestrike-reloaded"];
/// How long a verification waits for the server to load its rotation.
const READY_WITHIN: Duration = Duration::from_secs(60);
/// How long the Linux verification waits for the bot client's match.
const MATCH_WITHIN: Duration = Duration::from_secs(90);
/// The server config the verification runs (no network beyond the machine).
const CHECK_CONFIG: &str = "// `cargo xtask dist`'s check: by address only, nothing announced, no
// STUN, the router never asked.
(
    name: \"dist check\",
    visibility: Invite,
    relays: [],
    stun: [],
    port_mapping: false,
    port: 0,
    max_players: 2,
    max_spectators: 0,
    fill_bots: 2,
    min_players: 1,
    rotation: [
        (mod: \"olx-classic\", level: \"olx-level-dirt-level\", time_limit: 0.2, kill_limit: 0, lives: -2),
        (mod: \"olx-classic\", level: \"olr-level-liero-factory-reloaded\"),
        (mod: \"olx-classic\", level: \"olr-level-castlestrike-reloaded\"),
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
    format!("open-liero-reloaded-{version}")
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

/// Copies OLR's own packs `from` to `to`, leaving out `RELOADED_LEFT_OUT`.
pub fn copy_reloaded(from: &Path, to: &Path) -> Result<(), String> {
    copy_dir(from, to, &RELOADED_LEFT_OUT)
}

/// Copies the built-in assets `from` to `to`, leaving out `ASSETS_LEFT_OUT`.
pub fn copy_assets(from: &Path, to: &Path) -> Result<(), String> {
    copy_dir(from, to, &ASSETS_LEFT_OUT)
}

/// Copies the built-in materials `from` to `to`, leaving out
/// `MATERIALS_LEFT_OUT`.
pub fn copy_materials(from: &Path, to: &Path) -> Result<(), String> {
    copy_dir(from, to, &MATERIALS_LEFT_OUT)
}

/// An error unless the release folder holds every level of
/// [`RELOADED_LEVELS`], every built-in asset of the repository's
/// [`ASSETS`] (its `asset.ron`) and every built-in material of its
/// [`MATERIALS`] (`<id>.ron`).
fn check_reloaded(folder: &Path) -> Result<(), String> {
    let at = |rel: &str| rel.split('/').fold(folder.to_path_buf(), |d, c| d.join(c));
    let mut materials: Vec<String> = std::fs::read_dir(root().join(MATERIALS))
        .map_err(|e| format!("{MATERIALS}: {e}"))?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|f| f.ends_with(".ron") && !f.starts_with('.'))
        .collect();
    materials.sort();
    let lacking: Vec<&str> =
        materials.iter().map(String::as_str).filter(|f| !at(MATERIALS).join(f).is_file()).collect();
    if !lacking.is_empty() {
        return Err(format!("{MATERIALS} lacks {}", lacking.join(", ")));
    }
    let mut assets: Vec<String> = std::fs::read_dir(root().join(ASSETS))
        .map_err(|e| format!("{ASSETS}: {e}"))?
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    assets.sort();
    let lacking: Vec<&str> = assets
        .iter()
        .map(String::as_str)
        .filter(|id| !at(ASSETS).join(id).join("asset.ron").is_file())
        .collect();
    if !lacking.is_empty() {
        return Err(format!("{ASSETS} lacks {}", lacking.join(", ")));
    }
    let missing: Vec<&str> = RELOADED_LEVELS
        .into_iter()
        .filter(|id| {
            !RELOADED
                .split('/')
                .fold(folder.to_path_buf(), |d, c| d.join(c))
                .join(id)
                .join("pack.ron")
                .is_file()
        })
        .collect();
    if missing.is_empty() { Ok(()) } else { Err(format!("{RELOADED} lacks {}", missing.join(", "))) }
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

/// Each path of `paths` (below the zip's top folder) longer than
/// [`MAX_SHIPPED_PATH`], as `<length> <path>`.
pub fn too_long(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|p| (p.encode_utf16().count(), p))
        .filter(|(n, _)| *n > MAX_SHIPPED_PATH)
        .map(|(n, p)| format!("{n} {p}"))
        .collect()
}

/// An error naming every path of `paths` over [`MAX_SHIPPED_PATH`].
fn check_lengths(paths: &[String]) -> Result<(), String> {
    let long = too_long(paths);
    if long.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} paths are over {MAX_SHIPPED_PATH} characters below the release's top folder \
         (Explorer cannot extract them on Windows; shorten their names):\n  {}",
        long.len(),
        long.join("\n  ")
    ))
}

/// Writes the zip `out` of the directory `folder` (its name the zip's top
/// folder): every file below it in byte order of its path, deflated, dated
/// 1980-01-01 00:00, mode 0755 for `executables` (paths in the folder) and
/// 0644 for the rest. A path over [`MAX_SHIPPED_PATH`] below the folder is
/// an error naming each, before anything is written.
pub fn write_zip(folder: &Path, executables: &[String], out: &Path) -> Result<(), String> {
    use zip::write::SimpleFileOptions;
    let top = folder
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{}: no folder name", folder.display()))?;
    let files = list_files(folder)?;
    check_lengths(&files)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let part = out.with_extension("zip.part");
    let file = std::fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let fail = |e: &dyn std::fmt::Display| format!("writing {}: {e}", part.display());
    for rel in files {
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

/// Builds `olr-game` and `olr-server` for `platform` in the `dist` profile,
/// carrying `version` (`version::BUILD_VERSION_VAR`) and the bug relay's
/// address when this environment has one (`version::REPORT_URL_VAR`,
/// `https://` only; without it the game hides Report a bug); gives
/// cargo's output directory.
pub fn build(platform: Platform, version: &str) -> Result<PathBuf, String> {
    use crate::version::REPORT_URL_VAR;
    let report_url = crate::version::report_url(std::env::var(REPORT_URL_VAR).ok().as_deref())?;
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root()).args(["build", "--quiet", "--profile", PROFILE]);
    c.env(crate::version::BUILD_VERSION_VAR, version);
    c.env(crate::version::RELEASE_REPO_VAR, crate::version::RELEASE_REPO);
    match report_url {
        Some(url) => c.env(REPORT_URL_VAR, url),
        None => c.env_remove(REPORT_URL_VAR),
    };
    for package in PACKAGES {
        c.args(["-p", package]);
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
/// output directory `build`, named for `version`; gives the folder.
pub fn stage(platform: Platform, build: &Path, dir: &Path, version: &str) -> Result<PathBuf, String> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("{}: {e}", dir.display()));
        }
        _ => {}
    }
    let folder = dir.join(folder(version));
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
    copy_reloaded(&repo.join(RELOADED), &folder.join(RELOADED))?;
    copy_assets(&repo.join(ASSETS), &folder.join(ASSETS))?;
    copy_materials(&repo.join(MATERIALS), &folder.join(MATERIALS))?;
    write_install_manifest(&folder)?;
    Ok(folder)
}

/// Writes the folder's `install-manifest.txt` (`olr_update::install::
/// MANIFEST_FILE`): every file in it, itself included, one path a line in
/// byte order. The Windows updater removes what a release listed and the
/// next one lacks, and leaves the player's own files.
pub fn write_install_manifest(folder: &Path) -> Result<(), String> {
    use olr_update::install::{MANIFEST_FILE, manifest_text};
    let mut files = list_files(folder)?;
    files.push(MANIFEST_FILE.to_string());
    let path = folder.join(MANIFEST_FILE);
    std::fs::write(&path, manifest_text(&files)).map_err(|e| format!("{}: {e}", path.display()))
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
fn unpack_fresh(zip: &Path, base: &Path, version: &str) -> Result<(PathBuf, PathBuf), String> {
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
    Ok((unpacked.join(folder(version)), run))
}

/// The release folder the Linux Flatpak holds (`flatpak::staged_app`), run
/// from a temporary directory; `olr --version` must say `version`.
fn verify_linux(folder: &Path, version: &str) -> Result<String, String> {
    let tmp = tempfile::Builder::new()
        .prefix("olr-dist-")
        .tempdir()
        .map_err(|e| format!("a temporary directory: {e}"))?;
    let run = tmp.path().join("run");
    std::fs::create_dir_all(&run).map_err(|e| format!("{}: {e}", run.display()))?;
    std::fs::write(run.join("server.ron"), CHECK_CONFIG).map_err(|e| format!("{}: {e}", run.display()))?;
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
    let said = Command::new(folder.join("olr"))
        .arg("--version")
        .current_dir(&run)
        .output()
        .map_err(|e| format!("cannot run olr --version: {e}"))?;
    let said = String::from_utf8_lossy(&said.stdout).trim().to_string();
    if said != format!("olr {version}") {
        return Err(format!("olr --version says {said:?}, not \"olr {version}\""));
    }
    let help = Command::new(folder.join("olr"))
        .arg("--help")
        .current_dir(&run)
        .output()
        .map_err(|e| format!("cannot run olr --help: {e}"))?;
    let text = String::from_utf8_lossy(&help.stdout);
    let beside = format!("[default: {}]", folder.join("content").join("packs").display());
    if !help.status.success() || !text.contains(&beside) {
        return Err(format!(
            "olr --help ({}) does not show {beside}:\n{text}{}",
            help.status,
            String::from_utf8_lossy(&help.stderr)
        ));
    }
    check_reloaded(&folder)?;

    // The bot client: the dev build, the repository's content.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let built = Command::new(cargo)
        .current_dir(root())
        .args(["build", "--quiet", "-p", "olr-cli"])
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !built.success() {
        return Err(format!("building olr-cli failed ({built})"));
    }
    let mut server = Watched::start(
        "olr-server",
        Command::new(folder.join("olr-server")).args(["--config", "server.ron"]).current_dir(&run),
    )?;
    let port = server.wait_for("listening line", READY_WITHIN, listening_port)?;
    let map =
        server.wait_for("rotation", READY_WITHIN, |l| l.strip_prefix("next map: ").map(String::from))?;
    let mut bot = Watched::start(
        "the bot client",
        Command::new(root().join("target/debug/olr-cli"))
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
        Some(s) => return Err(server.report(&format!("olr-server exited with {s} on SIGTERM"))),
        None => return Err(server.report("olr-server did not stop on SIGTERM: killed")),
    }
    let bad = server.bad_lines();
    if !bad.is_empty() {
        return Err(server.report(&format!("olr-server's output is not clean: {bad:?}")));
    }
    Ok(format!(
        "olr --help names the content beside it, OLR's own levels beside that; olr-server loaded {map} and started a match ({started})"
    ))
}

/// The Windows zip, unpacked under the stage in Windows' temp directory
/// and run from another directory there through interop; `Ok(None)` when
/// interop is missing (a notice).
fn verify_windows(zip: &Path, version: &str) -> Result<Option<String>, String> {
    if let Some(why) = winbuild::interop_missing() {
        println!("skip dist verification on Windows ({why})");
        return Ok(None);
    }
    let stage = winbuild::Stage { dir: winbuild::windows_temp()?.join(winbuild::STAGE) };
    std::fs::create_dir_all(&stage.dir).map_err(|e| format!("{}: {e}", stage.dir.display()))?;
    // A run an interrupted xtask left behind would hold the files.
    winbuild::stop_stage_processes(&stage.dir)?;
    let (folder, run) = unpack_fresh(zip, &stage.dir.join("dist"), version)?;
    let help =
        winbuild::run_in(&stage, &folder.join("olr.exe"), &run, &["--help".to_string()], &[], READY_WITHIN)?;
    let text = String::from_utf8_lossy(&help.stdout);
    let beside = format!("{}\\content\\packs]", self::folder(version));
    if !text.lines().any(|l| l.contains("[default: ") && l.trim_end().ends_with(&beside)) {
        return Err(format!("olr.exe --help does not show the content beside it (…{beside}):\n{text}"));
    }
    check_reloaded(&folder)?;
    let mut server = Watched::start(
        "olr-server.exe",
        Command::new(folder.join("olr-server.exe")).args(["--config", "server.ron"]).current_dir(&run),
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
        return Err(server.report(&format!("olr-server.exe's output is not clean: {bad:?}")));
    }
    Ok(Some(format!(
        "olr.exe --help names the content beside it, OLR's own levels beside that; olr-server.exe loaded {map}"
    )))
}

/// A platform's release file: the Windows zip, the Linux Flatpak.
pub fn release_name(version: &str, platform: Platform) -> String {
    match platform {
        Platform::Windows => zip_name(version, platform),
        Platform::Linux => crate::flatpak::bundle_name(version),
    }
}

/// `cargo xtask dist`: the releases (the Windows zip, the Linux Flatpak),
/// then (unless `verify` is off) their checks; a work in progress's version
/// unless `final_` (`version::release_version`).
pub fn command(verify: bool, final_: bool) -> anyhow::Result<bool> {
    let version = match crate::version::release_version(final_) {
        Ok(v) => v,
        Err(why) => {
            println!("FAIL dist: {why}");
            return Ok(false);
        }
    };
    println!("releasing {version}{}", if final_ { " (final)" } else { " (a work in progress)" });
    match crate::version::report_url(std::env::var(crate::version::REPORT_URL_VAR).ok().as_deref()) {
        Ok(Some(url)) => println!("Report a bug sends to {url}"),
        Ok(None) => println!("{} is not set: these builds hide Report a bug", crate::version::REPORT_URL_VAR),
        Err(why) => {
            println!("FAIL dist: {why}");
            return Ok(false);
        }
    }
    if let Some(why) = winbuild::build_missing() {
        println!("FAIL dist: the Windows build cannot be made here: {why}");
        return Ok(false);
    }
    if let Some(why) = crate::flatpak::tools_missing() {
        println!("FAIL dist: the Linux Flatpak cannot be made here: {why}");
        return Ok(false);
    }
    let target = root().join("target");
    let staging = target.join("dist-stage");
    // The Linux zip is no longer a release: an old one would mislead.
    let _ = std::fs::remove_file(target.join(ZIP_DIR).join(zip_name(VERSION, Platform::Linux)));
    let mut releases = Vec::new();
    for platform in Platform::ALL {
        println!(
            "building olr-game and olr-server for {} (the {PROFILE} profile: LTO, minutes from cold)...",
            platform.suffix()
        );
        let build = build(platform, &version).map_err(anyhow::Error::msg)?;
        let file = match platform {
            Platform::Linux => crate::flatpak::bundle(&build, &version).map_err(anyhow::Error::msg)?,
            Platform::Windows => {
                let folder = stage(platform, &build, &staging.join(platform.suffix()), &version)
                    .map_err(anyhow::Error::msg)?;
                let zip = target.join(ZIP_DIR).join(release_name(&version, platform));
                write_zip(&folder, &executables(platform), &zip).map_err(anyhow::Error::msg)?;
                zip
            }
        };
        releases.push((platform, file));
    }
    let _ = std::fs::remove_dir_all(&staging);
    let mut ok = true;
    if verify {
        for (platform, file) in &releases {
            println!("checking {} ...", file.display());
            let result = match platform {
                Platform::Linux => verify_linux(&crate::flatpak::staged_app(), &version).map(Some),
                Platform::Windows => verify_windows(file, &version),
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
    for (_, file) in &releases {
        let bytes = std::fs::metadata(file).map_or(0, |m| m.len());
        println!("{}  {:.1} MiB", file.display(), bytes as f64 / (1024.0 * 1024.0));
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_carry_the_workspace_version() {
        assert_eq!(folder("1.2.3"), "open-liero-reloaded-1.2.3");
        assert_eq!(zip_name("1.2.3", Platform::Windows), "open-liero-reloaded-1.2.3-windows-x64.zip");
        assert_eq!(zip_name("1.2.3", Platform::Linux), "open-liero-reloaded-1.2.3-linux-x64.zip");
        // The releases: the Windows zip and the Linux Flatpak.
        assert_eq!(release_name("1.2.3", Platform::Windows), "open-liero-reloaded-1.2.3-windows-x64.zip");
        assert_eq!(release_name("1.2.3", Platform::Linux), "open-liero-reloaded-1.2.3-linux-x64.flatpak");
        let meta = cargo_metadata::MetadataCommand::new()
            .manifest_path(root().join("Cargo.toml"))
            .no_deps()
            .exec()
            .unwrap();
        for name in PACKAGES {
            let p = meta.packages.iter().find(|p| p.name.as_str() == name).unwrap();
            assert_eq!(p.version.to_string(), VERSION, "{name}");
        }
    }

    /// The names the updater looks for are the ones `dist` writes.
    #[test]
    fn the_updater_knows_the_release_names() {
        let v = "0.2.0-dev.5.0abc123";
        assert_eq!(olr_update::release::windows_zip_name(v), zip_name(v, Platform::Windows));
        assert_eq!(olr_update::release::flatpak_bundle_name(v), release_name(v, Platform::Linux));
        assert_eq!(olr_update::release::folder_name(v), folder(v));
        assert_eq!(olr_update::flatpak::APP_ID, crate::flatpak::APP_ID);
    }

    #[test]
    fn the_folder_lists_its_files() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join(folder("1.2.3"));
        for f in ["olr.exe", "content/packs/a/pack.ron", "README.md"] {
            copy_text(&folder.join(f));
        }
        write_install_manifest(&folder).unwrap();
        let text = std::fs::read_to_string(folder.join("install-manifest.txt")).unwrap();
        assert_eq!(text, "README.md\ncontent/packs/a/pack.ron\ninstall-manifest.txt\nolr.exe\n");
        fn copy_text(p: &Path) {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
    }

    #[test]
    fn the_folder_holds_the_programs_dlls_docs_and_example() {
        let dlls = [("libwinpthread-1.dll".to_string(), PathBuf::from("/m/libwinpthread-1.dll"))];
        let got = files(Platform::Windows, Path::new("/b"), &dlls, Path::new("/r"));
        let want: Vec<(PathBuf, String)> = [
            ("/b/olr.exe", "olr.exe"),
            ("/b/olr-server.exe", "olr-server.exe"),
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
            [(PathBuf::from("/b/olr"), "olr".into()), (PathBuf::from("/b/olr-server"), "olr-server".into())]
        );
        assert_eq!(linux.len(), 7);
        assert_eq!(executables(Platform::Windows), ["olr.exe", "olr-server.exe"]);
        assert_eq!(executables(Platform::Linux), ["olr", "olr-server"]);
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

    /// Issue #7: OLR's own packs ship beside the content, without their
    /// README, and the release must hold the Reloaded levels.
    #[test]
    fn the_reloaded_levels_ship_beside_the_content() {
        let t = tempfile::tempdir().unwrap();
        let folder = t.path().join("out");
        copy_assets(&root().join(ASSETS), &folder.join(ASSETS)).unwrap();
        copy_materials(&root().join(MATERIALS), &folder.join(MATERIALS)).unwrap();
        assert!(check_reloaded(&folder).unwrap_err().contains(RELOADED_LEVELS[0]));
        let from = root().join(RELOADED);
        copy_reloaded(&from, &folder.join(RELOADED)).unwrap();
        check_reloaded(&folder).unwrap();
        assert!(from.join("README.md").is_file() && !folder.join(RELOADED).join("README.md").exists());
        // The verification's server loads them from the folder.
        for id in RELOADED_LEVELS {
            assert!(CHECK_CONFIG.contains(&format!("level: \"{id}\"")), "{id}");
        }
    }

    /// Issue #9: the built-in assets ship beside the content, without
    /// their README, and the release must hold every one.
    #[test]
    fn the_built_in_assets_ship_beside_the_content() {
        let t = tempfile::tempdir().unwrap();
        let folder = t.path().join("out");
        copy_reloaded(&root().join(RELOADED), &folder.join(RELOADED)).unwrap();
        copy_materials(&root().join(MATERIALS), &folder.join(MATERIALS)).unwrap();
        assert!(check_reloaded(&folder).unwrap_err().contains("torch"));
        let from = root().join(ASSETS);
        copy_assets(&from, &folder.join(ASSETS)).unwrap();
        check_reloaded(&folder).unwrap();
        assert!(from.join("README.md").is_file() && !folder.join(ASSETS).join("README.md").exists());
        assert!(folder.join(ASSETS).join("torch").join("thumb.png").is_file());
    }

    /// Issue #37: the built-in materials ship beside the content, without
    /// their README, and the release must hold every one.
    #[test]
    fn the_built_in_materials_ship_beside_the_content() {
        let t = tempfile::tempdir().unwrap();
        let folder = t.path().join("out");
        copy_reloaded(&root().join(RELOADED), &folder.join(RELOADED)).unwrap();
        copy_assets(&root().join(ASSETS), &folder.join(ASSETS)).unwrap();
        assert!(check_reloaded(&folder).unwrap_err().contains("stone.ron"));
        let from = root().join(MATERIALS);
        copy_materials(&from, &folder.join(MATERIALS)).unwrap();
        check_reloaded(&folder).unwrap();
        assert!(from.join("README.md").is_file() && !folder.join(MATERIALS).join("README.md").exists());
        assert!(folder.join(MATERIALS).join("glass.ron").is_file());
    }

    /// The content directories a release ships, at the same paths in the folder.
    const SHIPPED_CONTENT: [&str; 4] = [CONTENT, RELOADED, ASSETS, MATERIALS];

    /// Every file of [`SHIPPED_CONTENT`] under `repo` as its path in the
    /// release folder (`content/…`).
    fn shipped_content(repo: &Path) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        for dir in SHIPPED_CONTENT {
            let at = dir.split('/').fold(repo.to_path_buf(), |d, c| d.join(c));
            out.extend(list_files(&at)?.into_iter().map(|f| format!("{dir}/{f}")));
        }
        Ok(out)
    }

    /// Issue #64: every file the release ships from `content/` stays within
    /// `MAX_SHIPPED_PATH` of the folder, so Explorer extracts it on Windows.
    #[test]
    fn the_shipped_content_keeps_its_paths_short() {
        let files = shipped_content(&root()).unwrap();
        for dir in SHIPPED_CONTENT {
            assert!(files.iter().any(|f| f.starts_with(&format!("{dir}/"))), "{dir}");
        }
        let long = too_long(&files);
        assert!(long.is_empty(), "over {MAX_SHIPPED_PATH} characters:\n{}", long.join("\n"));
    }

    #[test]
    fn a_long_path_is_named_with_its_length() {
        let at = |n: usize| format!("content/{}", "x".repeat(n - "content/".len()));
        assert!(too_long(&[at(80), "olr.exe".into()]).is_empty());
        assert_eq!(too_long(&[at(80), at(81)]), [format!("81 {}", at(81))]);
        // Windows counts UTF-16 units: one per character here, two beyond U+FFFF.
        let wide = format!("{}é", at(79));
        assert!(too_long(std::slice::from_ref(&wide)).is_empty());
        let emoji = format!("{}\u{1F600}", at(79));
        assert_eq!(too_long(std::slice::from_ref(&emoji)), [format!("81 {emoji}")]);
    }

    /// Issue #64: `dist` refuses a zip with a path over the limit below its
    /// top folder (a new top-level file too), naming it, and writes nothing.
    #[test]
    fn a_zip_with_a_long_path_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("open-liero-reloaded-9.9.9");
        let long = format!("docs/{}.md", "n".repeat(MAX_SHIPPED_PATH - 7));
        for path in ["olr", "content/packs/a/pack.ron", long.as_str()] {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        let out = t.path().join("zips/a.zip");
        let err = write_zip(&dir, &[], &out).unwrap_err();
        assert!(err.contains(&format!("{} {long}", MAX_SHIPPED_PATH + 1)), "{err}");
        assert!(!err.contains("pack.ron"), "{err}");
        assert!(!out.exists() && !out.with_extension("zip.part").exists());
        // At the limit it is written.
        let at_limit = format!("docs/{}.md", "n".repeat(MAX_SHIPPED_PATH - 8));
        assert_eq!(at_limit.len(), MAX_SHIPPED_PATH);
        std::fs::rename(dir.join(&long), dir.join(&at_limit)).unwrap();
        write_zip(&dir, &[], &out).unwrap();
    }

    #[test]
    fn the_zip_is_ordered_dated_and_moded_the_same_every_time() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("open-liero-reloaded-9.9.9");
        for (path, text) in [
            ("olr-server", "server"),
            ("olr", "game"),
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
            ("open-liero-reloaded-9.9.9/README.md", 0o644, "readme"),
            ("open-liero-reloaded-9.9.9/content/packs/b/x.png", 0o644, "png"),
            ("open-liero-reloaded-9.9.9/content/packs/olx-core/pack.ron", 0o644, "core"),
            ("open-liero-reloaded-9.9.9/docs/online.md", 0o644, "online"),
            ("open-liero-reloaded-9.9.9/olr", 0o755, "game"),
            ("open-liero-reloaded-9.9.9/olr-server", 0o755, "server"),
            ("open-liero-reloaded-9.9.9/server.example.ron", 0o644, "()"),
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
            assert_eq!(mode("open-liero-reloaded-9.9.9/olr-server"), 0o755);
            assert_eq!(mode("open-liero-reloaded-9.9.9/README.md"), 0o644);
        }
    }
}
