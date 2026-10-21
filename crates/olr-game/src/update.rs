//! Updates from the main menu (issue #4). A build `cargo xtask dist` made
//! (it bakes in `OLR_RELEASE_REPO`, [`BUILD_REPO`]) checks GitHub's
//! release list once at launch on the thread `olr-game update`; a local
//! build never checks. The newest release newer than the build in the
//! player's channel (`olr_update::channel`) is offered in
//! `MenuData::update`; nothing installs but on the Update screen's
//! Update, then:
//!
//! - **Windows** (the portable folder, [`Install::Portable`]): the
//!   release's signed manifest (`<zip>.sig`) is downloaded and verified
//!   against the baked-in key, the zip downloaded into the folder's
//!   `.olr-update/`, its size and SHA-256 checked against the manifest,
//!   unpacked, then swapped in (`olr_update::install`); a folder that
//!   cannot be written says so and offers the release page. Restart starts
//!   the new `olr.exe`; the next start finishes a swap an interruption cut
//!   short ([`finish_interrupted_update`]).
//! - **Flatpak** ([`Install::Flatpak`]): the Flatpak portal's
//!   `CreateUpdateMonitor` and `Update` (`portal.rs`, Linux only), its
//!   progress shown; an update needing new permissions is left to the
//!   software centre or `flatpak update` (the command shown). Restart is
//!   the portal's `Spawn` of the latest version. A bundle from before the
//!   repository (branch `master`) is shown the command installing from it.
//! - Anything else: the release page.
//!
//! No network, timeout or GitHub error ever shows: it is logged once.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use olr_ui::update::{FlatpakChannel, UpdateData, UpdateMethod, UpdateOffer, UpdateStatus};
use olr_update::flatpak::{FlatpakInfo, PortalProgress};
use olr_update::install::{self, Plan, Recovery};
use olr_update::manifest::{self, Sha256};
use olr_update::release::{self, Release};
use olr_update::{Channel, Version};

use crate::http;

#[cfg(target_os = "linux")]
pub mod portal;

/// The variable naming the GitHub repository whose releases a build
/// checks (`owner/name`): `cargo xtask dist` sets it at compile time.
pub const REPO_VAR: &str = "OLR_RELEASE_REPO";
/// The build's repository: only `dist`'s builds have one, and check.
pub const BUILD_REPO: Option<&str> = option_env!("OLR_RELEASE_REPO");
/// A release list's address to use instead of GitHub's (a local mock), at
/// run time; only a build that checks reads it.
pub const API_VAR: &str = "OLR_UPDATE_API";
/// The program in the Windows folder.
pub const EXE: &str = "olr.exe";
/// The check's waits: each connect, any one read.
pub const CHECK_TIMEOUTS: http::GetTimeouts =
    http::GetTimeouts { connect: Duration::from_secs(10), idle: Duration::from_secs(15) };
/// A download's waits: each connect, any one read (it may take minutes).
pub const DOWNLOAD_TIMEOUTS: http::GetTimeouts =
    http::GetTimeouts { connect: Duration::from_secs(15), idle: Duration::from_secs(60) };
/// The most bytes of a release zip downloaded.
pub const MAX_ZIP_BYTES: u64 = 512 * 1024 * 1024;

/// GitHub's release list of `repo`.
pub fn github_releases_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases?per_page=30")
}

/// Where this build reads the release list: `None` (no check, no update
/// UI) unless it was built with a repository (`build_repo`) and its
/// version parses; `runtime_api` (`OLR_UPDATE_API`) replaces GitHub's.
pub fn releases_url(build_repo: Option<&str>, runtime_api: Option<String>, version: &str) -> Option<String> {
    let repo = build_repo.map(str::trim).filter(|r| !r.is_empty())?;
    Version::parse(version)?;
    let api = runtime_api.map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
    Some(api.unwrap_or_else(|| github_releases_url(repo)))
}

/// How this copy of the game is installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// Windows' portable folder (the one holding `olr.exe`).
    Portable(PathBuf),
    /// A Flatpak, as `/.flatpak-info` says.
    Flatpak(FlatpakInfo),
    /// Neither: the release page only.
    Other,
}

impl Install {
    /// This process's.
    pub fn detect() -> Install {
        if cfg!(windows) {
            if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
                return Install::Portable(dir);
            }
            return Install::Other;
        }
        match std::fs::read_to_string("/.flatpak-info")
            .ok()
            .as_deref()
            .and_then(olr_update::flatpak::parse_info)
        {
            Some(info) => Install::Flatpak(info),
            None => Install::Other,
        }
    }

    fn flatpak(&self) -> Option<&FlatpakInfo> {
        match self {
            Install::Flatpak(i) => Some(i),
            _ => None,
        }
    }

    /// How a release installs here, for a player on `channel`.
    pub fn method(&self, channel: Channel) -> UpdateMethod {
        match self {
            Install::Portable(_) => UpdateMethod::Portable,
            Install::Flatpak(info) if info.channel().is_some() => UpdateMethod::Portal,
            Install::Flatpak(info) => UpdateMethod::Command(info.switch_command(channel)),
            Install::Other => UpdateMethod::Page,
        }
    }
}

/// Fetching over HTTP: [`Http`], or the tests' own.
pub trait Fetch: Send + Sync {
    /// GETs `url` into `sink`, at most `max` bytes, with its progress.
    fn get(
        &self,
        url: &str,
        accept: &str,
        max: u64,
        sink: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), String>;
}

/// The real thing: [`http::get`].
pub struct Http {
    pub timeouts: http::GetTimeouts,
}

impl Fetch for Http {
    fn get(
        &self,
        url: &str,
        accept: &str,
        max: u64,
        sink: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<(), String> {
        let agent = format!("olr/{}", crate::VERSION);
        http::get(url, accept, &agent, max, self.timeouts, sink, progress).map_err(|e| e.to_string())
    }
}

/// The release list at `url`.
pub fn fetch_releases(f: &dyn Fetch, url: &str) -> Result<Vec<Release>, String> {
    let mut body = Vec::new();
    f.get(url, "application/vnd.github+json", release::MAX_LIST_BYTES as u64, &mut body, &mut |_, _| {})?;
    release::parse_releases(&body)
}

/// What an install reports, in order: steps, then one ending.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Step(String, Option<f32>),
    Installed,
    Failed(String),
    NotWritable(String),
    Outside { why: String, command: String },
}

impl Event {
    fn status(self) -> UpdateStatus {
        match self {
            Event::Step(step, progress) => UpdateStatus::Working { step, progress },
            Event::Installed => UpdateStatus::Installed,
            Event::Failed(why) => UpdateStatus::Failed(why),
            Event::NotWritable(why) => UpdateStatus::NotWritable(why),
            Event::Outside { why, command } => UpdateStatus::Outside { why, command },
        }
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

/// Installs `release` into the portable folder `game` for a build
/// `current`, trusting `key`: the signature first, the download checked
/// against it, then unpacked and swapped in. Every failure before the swap
/// leaves the folder as it was.
pub fn install_portable(
    f: &dyn Fetch,
    release: &Release,
    current: &Version,
    game: &Path,
    key: &[u8; 32],
    report: &mut dyn FnMut(Event),
) -> Event {
    let version = release.version_text().to_string();
    let work = install::work_dir(game);
    if work.join("swap.txt").exists() {
        return Event::Failed("An earlier update is not finished yet: restart the game first.".into());
    }
    report(Event::Step("Checking the game's folder".into(), None));
    if let Err(e) = install::check_writable(game) {
        return Event::NotWritable(format!(
            "The game's folder {} cannot be written ({e}), so the game cannot update itself there. \
             Download the new version from its release page, or move the game to a folder you can write.",
            game.display()
        ));
    }
    let zip_name = release::windows_zip_name(&version);
    let sig_name = release::signature_name(&zip_name);
    let (Some(zip), Some(sig)) = (release.asset(&zip_name), release.asset(&sig_name)) else {
        return Event::Failed(format!(
            "The release has no signed Windows download ({zip_name} and {sig_name})."
        ));
    };
    report(Event::Step("Checking the release's signature".into(), None));
    let mut sig_text = Vec::new();
    let max_sig = manifest::MAX_SIGNATURE_BYTES as u64;
    if let Err(e) = f.get(&sig.url, "application/octet-stream", max_sig, &mut sig_text, &mut |_, _| {}) {
        return Event::Failed(format!("Could not download the release's signature ({e}). Try again later."));
    }
    let signed = match manifest::verify(&sig_text, key) {
        Ok(m) => m,
        Err(e) => {
            return Event::Failed(format!(
                "The release's signature is not valid ({e}): nothing was installed."
            ));
        }
    };
    if signed.version != version || signed.file != zip_name || signed.size > MAX_ZIP_BYTES {
        return Event::Failed(format!(
            "The release's signature is for {} {}, not this release: nothing was installed.",
            signed.file, signed.version
        ));
    }
    let part = work.join(format!("{zip_name}.part"));
    let file = match std::fs::File::create(&part) {
        Ok(f) => f,
        Err(e) => return Event::Failed(format!("Could not write {} ({e}).", part.display())),
    };
    let mut sink = Hashing { file: std::io::BufWriter::new(file), sha: Sha256::new() };
    let total = signed.size;
    let mut last = u64::MAX;
    let got = f.get(&zip.url, "application/octet-stream", total, &mut sink, &mut |done, _| {
        // A step every MiB.
        if done >> 20 != last >> 20 || done == total {
            last = done;
            let p = if total > 0 { done as f32 / total as f32 } else { 0.0 };
            report(Event::Step(format!("Downloading {} of {} MB", mb(done), mb(total)), Some(p.min(1.0))));
        }
    });
    let flushed = sink.file.flush().and_then(|()| sink.file.get_ref().sync_all());
    let size = std::fs::metadata(&part).map_or(0, |m| m.len());
    let digest = sink.sha.finish();
    if let Err(e) = got.and(flushed.map_err(|e| e.to_string())) {
        let _ = std::fs::remove_file(&part);
        return Event::Failed(format!("The download failed ({e}). Try again later."));
    }
    if let Err(e) = signed.check(&version, current, &zip_name, size, &digest) {
        let _ = std::fs::remove_file(&part);
        return Event::Failed(format!(
            "The download does not match its signature ({e}): nothing was installed."
        ));
    }
    let zip_path = work.join(&zip_name);
    if let Err(e) = std::fs::rename(&part, &zip_path) {
        return Event::Failed(format!("Could not keep the download ({e})."));
    }
    report(Event::Step("Unpacking".into(), None));
    let files = match install::stage_zip(&zip_path, game, &release::folder_name(&version), EXE) {
        Ok(files) => files,
        Err(e) => {
            install::cleanup(&work);
            return Event::Failed(format!(
                "The download could not be unpacked ({e}): nothing was installed."
            ));
        }
    };
    let _ = std::fs::remove_file(&zip_path);
    report(Event::Step("Installing".into(), None));
    let plan = Plan::new(game, &version, &files, EXE);
    match install::swap(game, &plan, &mut |_| Ok(())) {
        Ok(()) => {
            log::info!("update: installed {version} into {}", game.display());
            Event::Installed
        }
        Err(e) => Event::Failed(format!(
            "The update stopped half way ({e}). The game finishes it when it starts next: restart it."
        )),
    }
}

/// A download written to its file and hashed as it comes.
struct Hashing {
    file: std::io::BufWriter<std::fs::File>,
    sha: Sha256,
}

impl Write for Hashing {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(buf)?;
        self.sha.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// The Flatpak portal's update (`portal.rs`; the tests' fake).
pub trait Portal {
    /// Asks the portal to update this Flatpak, reporting each `Progress`
    /// until the last (done, nothing to do, failed). An error is the
    /// portal unreachable or refusing the call.
    fn update(&mut self, progress: &mut dyn FnMut(&PortalProgress)) -> Result<(), String>;
}

/// Updates the Flatpak `info` through `portal`.
pub fn install_flatpak(portal: &mut dyn Portal, info: &FlatpakInfo, report: &mut dyn FnMut(Event)) -> Event {
    report(Event::Step("Asking Flatpak to update the game".into(), None));
    let mut last = None;
    let called = portal.update(&mut |p| {
        if let PortalProgress::Running(f) = p {
            report(Event::Step("Updating through Flatpak".into(), Some(*f)));
        }
        last = Some(p.clone());
    });
    let outside = |why: &str| Event::Outside { why: why.to_string(), command: info.update_command() };
    if let Err(e) = called {
        log::info!("update: the Flatpak portal: {e}");
        return outside(&format!(
            "The game could not reach Flatpak to update ({e}). Update from your software centre, or run:"
        ));
    }
    match last {
        Some(PortalProgress::Done) => {
            log::info!("update: Flatpak updated {}//{}", info.app_id, info.branch);
            Event::Installed
        }
        Some(PortalProgress::UpToDate) => Event::Failed(
            "Flatpak's repository has no newer build yet (it may still be on its way): try again in a few \
             minutes."
                .into(),
        ),
        Some(p @ PortalProgress::Failed { .. }) if p.needs_permissions() => outside(
            "This update needs permissions the installed build does not have, so only your software centre \
             or Flatpak itself can install it. Update from your software centre, or run:",
        ),
        Some(PortalProgress::Failed { error, message }) => {
            log::info!("update: the Flatpak portal failed: {error}: {message}");
            outside(&format!(
                "Flatpak could not update the game ({message}). Update from your software centre, or run:"
            ))
        }
        Some(PortalProgress::Running(_)) | None => outside(
            "Flatpak stopped without saying how the update went. Update from your software centre, or run:",
        ),
    }
}

/// Opens `url` in the browser (`https://` only).
pub fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("{url} is no https address"));
    }
    let mut c = if cfg!(windows) {
        let mut c = std::process::Command::new("explorer");
        c.arg(url);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    c.spawn().map(drop).map_err(|e| format!("could not open {url}: {e}"))
}

/// The update check and install, for the shell.
pub struct Updater {
    current: Version,
    install: Install,
    /// Downloads' fetcher (the real one waits longer than the check's).
    download: Arc<dyn Fetch>,
    key: [u8; 32],
    checking: Option<Receiver<Result<Vec<Release>, String>>>,
    releases: Vec<Release>,
    /// The setting the offer was made for (`None`: not made yet).
    offered_for: Option<Option<bool>>,
    offered: Option<Release>,
    installing: Option<Receiver<Event>>,
    /// Installed: the offer stays as it is.
    done: bool,
}

impl Updater {
    /// This build's: `None` for a build that does not check (a local one,
    /// or a version that does not parse).
    pub fn of_build() -> Option<Updater> {
        let url = releases_url(BUILD_REPO, std::env::var(API_VAR).ok(), crate::VERSION)?;
        let current = Version::parse(crate::VERSION)?;
        let fetch = Arc::new(Http { timeouts: CHECK_TIMEOUTS });
        let mut u = Updater::start(&url, current, Install::detect(), fetch, manifest::release_key());
        u.download = Arc::new(Http { timeouts: DOWNLOAD_TIMEOUTS });
        Some(u)
    }

    /// Starts checking `url`'s release list through `fetch` for a build
    /// `current` installed as `install`; downloads go through `fetch` too,
    /// trusting `key`.
    pub fn start(
        url: &str,
        current: Version,
        install: Install,
        fetch: Arc<dyn Fetch>,
        key: [u8; 32],
    ) -> Updater {
        let (tx, rx) = mpsc::channel();
        let (f, u) = (fetch.clone(), url.to_string());
        let spawned = std::thread::Builder::new().name("olr-game update".into()).spawn(move || {
            let _ = tx.send(fetch_releases(f.as_ref(), &u));
        });
        if let Err(e) = &spawned {
            log::info!("update check: not started: {e}");
        }
        log::info!(
            "update check: {}",
            if url.starts_with("https://api.github.com/") { "GitHub" } else { url }
        );
        Updater {
            current,
            install,
            download: fetch,
            key,
            checking: spawned.is_ok().then_some(rx),
            releases: Vec::new(),
            offered_for: None,
            offered: None,
            installing: None,
            done: false,
        }
    }

    /// The views that never change: fills `data` once.
    pub fn fill(&self, data: &mut UpdateData) {
        data.enabled = true;
        data.build_prerelease = self.current.is_prerelease();
        data.flatpak = self.install.flatpak().and_then(|info| {
            let beta = info.channel()? == Channel::PreRelease;
            let other = if beta { Channel::Stable } else { Channel::PreRelease };
            Some(FlatpakChannel { beta, switch_command: info.switch_command(other) })
        });
    }

    /// The check's result and the install's progress into `data`; the
    /// offer for the setting "Include pre-releases" `setting`.
    pub fn poll(&mut self, setting: Option<bool>, data: &mut UpdateData) {
        if let Some(rx) = &self.checking {
            match rx.try_recv() {
                Ok(Ok(list)) => {
                    log::info!("update check: {} releases listed", list.len());
                    self.releases = list;
                    self.offered_for = None;
                    self.checking = None;
                }
                Ok(Err(why)) => {
                    log::info!("update check: none ({why})");
                    self.checking = None;
                }
                Err(TryRecvError::Disconnected) => self.checking = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &self.installing {
            loop {
                match rx.try_recv() {
                    Ok(e) => {
                        let end = !matches!(e, Event::Step(..));
                        if let Event::Failed(why) | Event::NotWritable(why) = &e {
                            log::info!("update: not installed: {why}");
                        }
                        self.done |= e == Event::Installed;
                        data.status = e.status();
                        if end {
                            self.installing = None;
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        data.status =
                            UpdateStatus::Failed("The update stopped unexpectedly. Please try again.".into());
                        self.installing = None;
                        break;
                    }
                }
            }
        }
        if self.installing.is_some() || self.done || self.offered_for == Some(setting) {
            return;
        }
        self.offered_for = Some(setting);
        let channel = olr_update::channel(setting, &self.current, self.install.flatpak());
        let newest = release::newest(&self.releases, &self.current, channel).cloned();
        if newest.as_ref().map(|r| &r.tag) != self.offered.as_ref().map(|r| &r.tag) {
            match &newest {
                Some(r) => log::info!("update check: {} is newer ({channel:?} channel)", r.tag),
                None if !self.releases.is_empty() => {
                    log::info!("update check: up to date ({channel:?} channel)")
                }
                None => {}
            }
            data.status = UpdateStatus::Idle;
        }
        data.offer = newest.as_ref().map(|r| UpdateOffer {
            version: r.version_text().to_string(),
            title: r.title.clone(),
            notes: olr_update::notes::plain_text(&r.notes),
            page: r.page.clone(),
            method: self.install.method(channel),
        });
        self.offered = newest;
    }

    /// The Update screen's Update: installs the offer on a thread.
    pub fn install(&mut self, data: &mut UpdateData) {
        if self.installing.is_some() || self.done {
            return;
        }
        let Some(release) = self.offered.clone() else { return };
        let (tx, rx) = mpsc::channel();
        let (install, current, key) = (self.install.clone(), self.current.clone(), self.key);
        let fetch = self.download.clone();
        log::info!("update: installing {}", release.tag);
        let spawned = std::thread::Builder::new().name("olr-game install".into()).spawn(move || {
            let mut report = |e: Event| {
                let _ = tx.send(e);
            };
            let end = match &install {
                Install::Portable(dir) => {
                    install_portable(fetch.as_ref(), &release, &current, dir, &key, &mut report)
                }
                #[cfg(target_os = "linux")]
                Install::Flatpak(info) => install_flatpak(&mut portal::Dbus, info, &mut report),
                _ => Event::Failed(
                    "This build cannot install updates: download it from the release page.".into(),
                ),
            };
            report(end);
        });
        match spawned {
            Ok(_) => {
                data.status = UpdateStatus::Working { step: "Starting".into(), progress: None };
                self.installing = Some(rx);
            }
            Err(e) => data.status = UpdateStatus::Failed(format!("The update could not start ({e}).")),
        }
    }

    /// Starts the installed build (Restart): the new `olr.exe`, or the
    /// Flatpak's latest version through the portal. The caller quits.
    pub fn restart(&self) -> Result<(), String> {
        let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        match &self.install {
            Install::Portable(dir) => std::process::Command::new(dir.join(EXE))
                .args(&args)
                .spawn()
                .map(drop)
                .map_err(|e| format!("could not start {}: {e}", dir.join(EXE).display())),
            #[cfg(target_os = "linux")]
            Install::Flatpak(_) => portal::spawn_latest(&args),
            _ => Err("this build cannot restart into an update".into()),
        }
    }
}

/// At start, before anything else (Windows `dist` builds only): finishes
/// an update an interruption cut short, else removes the last update's
/// leftovers (the old `olr.exe`). `true` when this process is the old
/// build of a swap finished only now: it started the new one and must
/// exit.
pub fn finish_interrupted_update() -> bool {
    if !cfg!(windows) || BUILD_REPO.is_none() {
        return false;
    }
    let Install::Portable(dir) = Install::detect() else { return false };
    match install::recover(&dir, EXE) {
        Recovery::Nothing => false,
        Recovery::Cleaned => {
            log::info!("update: removed the last update's leftovers");
            false
        }
        Recovery::Failed(why) => {
            log::warn!("update: an interrupted update could not be finished: {why}");
            false
        }
        Recovery::Finished { version, restart } => {
            log::info!("update: finished the interrupted update to {version}");
            if !restart {
                return false;
            }
            let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
            match std::process::Command::new(dir.join(EXE)).args(&args).spawn() {
                Ok(_) => true,
                Err(e) => {
                    log::warn!("update: could not start the new build ({e}): this one runs");
                    false
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_dist_build_checks() {
        let gh = "https://api.github.com/repos/o/r/releases?per_page=30";
        assert_eq!(releases_url(Some("o/r"), None, "0.2.0-dev.5.0abc123").as_deref(), Some(gh));
        assert_eq!(releases_url(Some("o/r"), Some(" ".into()), "0.2.0").as_deref(), Some(gh));
        assert_eq!(
            releases_url(Some("o/r"), Some("http://127.0.0.1:9/r".into()), "0.2.0").as_deref(),
            Some("http://127.0.0.1:9/r")
        );
        // A local build: no repository, or the local version.
        assert_eq!(releases_url(None, Some("http://127.0.0.1:9/r".into()), "0.2.0"), None);
        assert_eq!(releases_url(Some(""), None, "0.2.0"), None);
        assert_eq!(releases_url(Some("o/r"), None, "0.2.0-dev (local build)"), None);
        // This test build is a local one.
        if BUILD_REPO.is_none() {
            assert!(Updater::of_build().is_none());
        }
    }

    #[test]
    fn each_install_has_its_method() {
        let info =
            |branch: &str| FlatpakInfo { app_id: "a.b.C".into(), branch: branch.into(), user: Some(true) };
        assert_eq!(Install::Portable(PathBuf::from("x")).method(Channel::Stable), UpdateMethod::Portable);
        assert_eq!(Install::Flatpak(info("stable")).method(Channel::Stable), UpdateMethod::Portal);
        let UpdateMethod::Command(c) = Install::Flatpak(info("master")).method(Channel::Stable) else {
            panic!()
        };
        assert!(c.contains("stable.flatpakref") && c.ends_with("uninstall --user -y a.b.C//master"), "{c}");
        assert_eq!(Install::Other.method(Channel::PreRelease), UpdateMethod::Page);
    }
}
