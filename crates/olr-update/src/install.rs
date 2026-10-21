//! The Windows self-update of the portable folder (issue #4): the release
//! zip, downloaded and verified first, is unpacked into the game folder's
//! work directory [`WORK_DIR`] (`new/`), then swapped in, then the old
//! files are removed. The swap is safe against interruption:
//!
//! - Nothing in the folder changes until the new files are all staged; an
//!   interrupted download or unpacking leaves only the work directory,
//!   which the next start removes ([`recover`]).
//! - The swap's plan, the journal (`swap.txt`), is written atomically
//!   before the first file moves. Each step is a rename: the current file
//!   moves aside into `old/` (the running `olr.exe` too: Windows lets a
//!   running program be renamed, not replaced), the new one moves in; a
//!   rename that fails is retried, and a file moved aside whose new one
//!   cannot move in is moved back, so the folder keeps a program. Each
//!   swap's `old/` is a fresh one (`old-<n>/` when an earlier one cannot
//!   be removed: its program may still be running), named in the journal. The
//!   steps are idempotent, so a start that finds the journal finishes the
//!   swap (rolls forward) whatever step it stopped at; then the journal
//!   goes. The program is swapped first, so a half-swapped folder starts
//!   the new build, which finishes it.
//! - Files the new release no longer has are those the old release's
//!   [`MANIFEST_FILE`] lists and the new one does not: they move into
//!   `old/` too. Files the player added are in no manifest and stay (a
//!   folder without a manifest, an older build's, loses nothing).
//! - `old/` (the replaced files and the old program) is removed at the
//!   next start, when the old program no longer runs.
//! - One game at a time: an install holds [`claim`]'s lock, and a start's
//!   [`recover`] leaves the work directory alone while another copy of
//!   the game holds it.
//!
//! Plain `std::fs`, so the tests run it on any platform in a temporary
//! directory, interrupting it at every step.

use std::io::{self, Read as _};
use std::path::{Component, Path, PathBuf};

/// The game folder's list of the files its release installed, one
/// `/`-separated path a line (`cargo xtask dist` writes it in the zip).
pub const MANIFEST_FILE: &str = "install-manifest.txt";
/// The work directory in the game folder: the download, `new/`, `old/`
/// and the journal.
pub const WORK_DIR: &str = ".olr-update";
/// The journal: present while a swap is to be finished.
const JOURNAL: &str = "swap.txt";
/// The lock an install or a recovery holds.
const LOCK: &str = "lock";
/// The journal's first line.
const JOURNAL_MAGIC: &str = "olr-swap 1";
/// The most bytes and files a release zip may unpack to.
pub const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_FILES: usize = 100_000;

/// The work directory of the game folder `game`.
pub fn work_dir(game: &Path) -> PathBuf {
    game.join(WORK_DIR)
}

/// Whether the game folder can be updated in place: its work directory
/// made and a file written there. An error is the reason (a folder in
/// Program Files, a read-only medium).
pub fn check_writable(game: &Path) -> io::Result<()> {
    let work = work_dir(game);
    std::fs::create_dir_all(&work)?;
    let probe = work.join("probe.tmp");
    std::fs::write(&probe, b"olr")?;
    std::fs::remove_file(&probe)?;
    // The folder itself: a new file beside the program.
    let probe = game.join(".olr-update-probe.tmp");
    std::fs::write(&probe, b"olr")?;
    std::fs::remove_file(&probe)
}

/// A path from a zip or a manifest as a safe relative path: `/`-separated
/// normal components only, never the work directory or empty.
pub fn safe_relative(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() || path.starts_with('/') || parts[0].eq_ignore_ascii_case(WORK_DIR) {
        return None;
    }
    for p in &parts {
        let mut c = Path::new(p).components();
        if !matches!((c.next(), c.next()), (Some(Component::Normal(_)), None)) || p.contains(':') {
            return None;
        }
    }
    Some(parts.join("/"))
}

/// The files `MANIFEST_FILE` in `game` lists (none without one, or an
/// unreadable one: then nothing is removed).
pub fn installed_files(game: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(game.join(MANIFEST_FILE)) else { return Vec::new() };
    text.lines().filter_map(safe_relative).collect()
}

/// A manifest's text for `files`.
pub fn manifest_text(files: &[String]) -> String {
    let mut sorted: Vec<&String> = files.iter().collect();
    sorted.sort();
    sorted.dedup();
    sorted.iter().map(|f| format!("{f}\n")).collect()
}

/// Unpacks the release zip `zip` into `game`'s work directory (`new/`,
/// emptied first): its one top folder `top` stripped, every entry a safe
/// path, no links, within [`MAX_FILES`] and [`MAX_UNPACKED_BYTES`], and
/// the program `exe` among them. Writes the new release's
/// [`MANIFEST_FILE`] there and gives the files (the manifest included).
pub fn stage_zip(zip: &Path, game: &Path, top: &str, exe: &str) -> Result<Vec<String>, String> {
    let new = work_dir(game).join("new");
    remove_dir_if_any(&new).map_err(|e| format!("{}: {e}", new.display()))?;
    std::fs::create_dir_all(&new).map_err(|e| format!("{}: {e}", new.display()))?;
    let file = std::fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("the download is no zip: {e}"))?;
    if archive.len() > MAX_FILES {
        return Err(format!("the zip holds over {MAX_FILES} entries"));
    }
    let mut files = Vec::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("the zip's entry {i}: {e}"))?;
        let name = entry.name().to_string();
        let rel = name
            .strip_prefix(top)
            .and_then(|r| r.strip_prefix('/'))
            .ok_or_else(|| format!("the zip's {name:?} is outside its folder {top}"))?;
        if entry.is_dir() || rel.is_empty() {
            continue;
        }
        if entry.is_symlink() {
            return Err(format!("the zip's {name:?} is a link"));
        }
        let rel = safe_relative(rel).ok_or_else(|| format!("the zip's {name:?} is no safe path"))?;
        if rel == MANIFEST_FILE {
            // Written below from what is unpacked.
            continue;
        }
        let size = entry.size();
        total = total.saturating_add(size);
        if total > MAX_UNPACKED_BYTES {
            return Err("the zip unpacks to too many bytes".into());
        }
        let to = new.join(&rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut out = std::fs::File::create(&to).map_err(|e| format!("{}: {e}", to.display()))?;
        // At most what the entry says it holds.
        let copied = io::copy(&mut (&mut entry).take(size), &mut out)
            .map_err(|e| format!("unpacking {name:?}: {e}"))?;
        if copied != size {
            return Err(format!("the zip's {name:?} is cut short"));
        }
        out.sync_all().map_err(|e| format!("{}: {e}", to.display()))?;
        files.push(rel);
    }
    if !files.iter().any(|f| f == exe) {
        return Err(format!("the zip holds no {exe}"));
    }
    files.push(MANIFEST_FILE.to_string());
    let manifest = new.join(MANIFEST_FILE);
    std::fs::write(&manifest, manifest_text(&files)).map_err(|e| format!("{}: {e}", manifest.display()))?;
    files.sort();
    files.dedup();
    Ok(files)
}

/// A swap to do: the files to move in (the program first) and those to
/// remove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The release swapped in (for the log).
    pub version: String,
    /// Each staged file, the program `exe` first.
    pub install: Vec<String>,
    /// The old release's files the new one does not have.
    pub remove: Vec<String>,
    /// The work directory's folder the replaced files go to (`swap`
    /// picks a fresh one).
    aside: String,
}

/// An `aside` folder's name: `old` or `old-<n>`.
fn is_aside(name: &str) -> bool {
    name == "old"
        || name.strip_prefix("old-").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A fresh folder for the replaced files: `old/` (or `old-<n>/`) removed
/// or not there. One that cannot be removed holds a program still running.
fn fresh_aside(work: &Path) -> String {
    (0..100)
        .map(|n| if n == 0 { "old".to_string() } else { format!("old-{n}") })
        .find(|name| remove_dir_if_any(&work.join(name)).is_ok())
        .unwrap_or_else(|| format!("old-{}", std::process::id()))
}

impl Plan {
    /// The plan for the staged `files` over `game`'s installed release.
    pub fn new(game: &Path, version: &str, files: &[String], exe: &str) -> Plan {
        let mut install: Vec<String> = files.to_vec();
        install.sort_by_key(|f| (f != exe, f.clone()));
        install.dedup();
        // Compared ignoring case: Windows' file names are (a file renamed
        // only in case is the same file).
        let new: Vec<String> = install.iter().map(|f| f.to_lowercase()).collect();
        let mut remove: Vec<String> =
            installed_files(game).into_iter().filter(|f| !new.contains(&f.to_lowercase())).collect();
        remove.sort();
        remove.dedup();
        Plan { version: version.to_string(), install, remove, aside: "old".into() }
    }

    fn text(&self) -> String {
        let mut t = format!("{JOURNAL_MAGIC}\nversion {}\naside {}\n", self.version, self.aside);
        t.extend(self.install.iter().map(|f| format!("install {f}\n")));
        t.extend(self.remove.iter().map(|f| format!("remove {f}\n")));
        t.push_str("end\n");
        t
    }

    fn parse(text: &str) -> Option<Plan> {
        let mut lines = text.lines();
        lines.next().filter(|l| *l == JOURNAL_MAGIC)?;
        let version = lines.next()?.strip_prefix("version ")?.to_string();
        let aside = lines.next()?.strip_prefix("aside ").filter(|a| is_aside(a))?.to_string();
        let (mut install, mut remove) = (Vec::new(), Vec::new());
        for l in lines.by_ref() {
            if l == "end" {
                return Some(Plan { version, install, remove, aside });
            }
            match l.split_once(' ')? {
                ("install", f) => install.push(safe_relative(f)?),
                ("remove", f) => remove.push(safe_relative(f)?),
                _ => return None,
            }
        }
        // No `end`: never written whole.
        None
    }
}

/// A step of the swap, as the tests' interruption sees it.
pub type Step<'a> = &'a mut dyn FnMut(&str) -> io::Result<()>;

/// Swaps the staged release in by `plan`: writes the journal, then
/// [`finish`]es. On an error the journal stays: the next start finishes.
pub fn swap(game: &Path, plan: &Plan, step: Step<'_>) -> Result<(), String> {
    let work = work_dir(game);
    let plan = &Plan { aside: fresh_aside(&work), ..plan.clone() };
    let journal = work.join(JOURNAL);
    let tmp = work.join("swap.tmp");
    write_synced(&tmp, plan.text().as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    step("journal").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &journal).map_err(|e| format!("{}: {e}", journal.display()))?;
    finish(game, plan, step)
}

/// Does what is left of `plan` (each step idempotent), then removes the
/// journal: the swap is done. `old/` stays (the running program may be
/// in it).
fn finish(game: &Path, plan: &Plan, step: Step<'_>) -> Result<(), String> {
    let work = work_dir(game);
    let (new, old) = (work.join("new"), work.join(&plan.aside));
    let fail = |what: &str, p: &Path, e: io::Error| format!("{what} {}: {e}", p.display());
    for f in &plan.install {
        let (staged, here, aside) = (new.join(f), game.join(f), old.join(f));
        if !exists(&staged) {
            // Moved in already.
            continue;
        }
        let moved = exists(&here);
        if moved {
            step(&format!("aside {f}")).map_err(|e| e.to_string())?;
            move_aside(&here, &aside).map_err(|e| fail("moving aside", &here, e))?;
        }
        step(&format!("in {f}")).map_err(|e| e.to_string())?;
        let moved_in = match here.parent() {
            Some(parent) => std::fs::create_dir_all(parent),
            None => Ok(()),
        }
        .and_then(|()| rename_retrying(&staged, &here));
        if let Err(e) = moved_in {
            // The old file back, so the folder keeps a program meanwhile.
            if moved && !exists(&here) {
                let _ = rename_retrying(&aside, &here);
            }
            return Err(fail("moving in", &staged, e));
        }
    }
    for f in &plan.remove {
        let here = game.join(f);
        if exists(&here) && here.is_file() {
            step(&format!("remove {f}")).map_err(|e| e.to_string())?;
            move_aside(&here, &old.join(f)).map_err(|e| fail("removing", &here, e))?;
        }
        // Its folders, once empty (never the game folder).
        let mut dir = here.parent();
        while let Some(d) = dir.filter(|d| *d != game && d.starts_with(game)) {
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    step("commit").map_err(|e| e.to_string())?;
    let journal = work.join(JOURNAL);
    match std::fs::remove_file(&journal) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(fail("removing", &journal, e)),
        _ => Ok(()),
    }
}

/// Moves `from` to `to` (its folders made); a file already at `to` (moved
/// aside before an interruption) is kept and `from` removed.
fn move_aside(from: &Path, to: &Path) -> io::Result<()> {
    if exists(to) {
        return std::fs::remove_file(from);
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    rename_retrying(from, to)
}

/// A rename, tried again a few times over about a second: a virus scanner
/// or an indexer may hold a fresh file for a moment.
fn rename_retrying(from: &Path, to: &Path) -> io::Result<()> {
    let mut wait = std::time::Duration::from_millis(50);
    for _ in 0..4 {
        match std::fs::rename(from, to) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => std::thread::sleep(wait),
            done => return done,
        }
        wait *= 2;
    }
    std::fs::rename(from, to)
}

/// The work directory held by one game at a time (an install, a start's
/// recovery); released when dropped.
#[derive(Debug)]
pub struct Claim {
    _lock: std::fs::File,
}

/// Takes `game`'s work directory (made if missing): `None` while another
/// copy of the game holds it.
pub fn claim(game: &Path) -> io::Result<Option<Claim>> {
    let work = work_dir(game);
    std::fs::create_dir_all(&work)?;
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(work.join(LOCK))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Claim { _lock: file })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

fn remove_dir_if_any(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// What a start found in the game folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// No work directory: nothing to do.
    Nothing,
    /// Another copy of the game is updating it: left alone.
    Busy,
    /// A finished update's or an abandoned download's leftovers removed
    /// (as far as they could be).
    Cleaned,
    /// An interrupted swap to `version` finished. `restart`: this program
    /// is the old build (its file was swapped only now), so it should
    /// start the new one and exit.
    Finished { version: String, restart: bool },
    /// An interrupted swap could not be finished (the journal is kept for
    /// the next start).
    Failed(String),
}

/// At start, in the game folder `game` holding the program `exe`: finishes
/// an interrupted swap, else removes the work directory's leftovers.
pub fn recover(game: &Path, exe: &str) -> Recovery {
    let work = work_dir(game);
    if !work.is_dir() {
        return Recovery::Nothing;
    }
    let claim = match claim(game) {
        Ok(Some(c)) => c,
        Ok(None) => return Recovery::Busy,
        Err(e) => return Recovery::Failed(format!("{}: {e}", work.display())),
    };
    let journal = work.join(JOURNAL);
    match std::fs::read_to_string(&journal) {
        Ok(text) => {
            let Some(plan) = Plan::parse(&text) else {
                return Recovery::Failed(format!(
                    "{} is damaged: the update is left as it is",
                    journal.display()
                ));
            };
            let restart = exists(&work.join("new").join(exe));
            match finish(game, &plan, &mut |_| Ok(())) {
                Ok(()) => {
                    drop(claim);
                    cleanup(&work);
                    Recovery::Finished { version: plan.version, restart }
                }
                Err(e) => Recovery::Failed(e),
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            drop(claim);
            cleanup(&work);
            Recovery::Cleaned
        }
        Err(e) => Recovery::Failed(format!("{}: {e}", journal.display())),
    }
}

/// Removes the work directory, as far as it can (a running old program
/// in `old/` stays until the next start).
pub fn cleanup(work: &Path) {
    let _ = std::fs::remove_dir_all(work);
}
