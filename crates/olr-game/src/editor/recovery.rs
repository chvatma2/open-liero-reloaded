//! The editor's recovery copies (plan 14c R7, spec §4): `recovery/<id>/` in
//! the data directory holds the open pack's files (`olr_edit::pack::files`)
//! and `recovery.ron`, written atomically (a hidden sibling, then a rename:
//! `olr_pack::install::replace_dir`) every `RECOVERY_INTERVAL` while there
//! are unsaved changes; removed on a successful save and on Close. The
//! Editor screen offers each, and a damaged one is listed so it can be
//! discarded: never a panic, never a refusal to start.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use olr_edit::pack::Pack;
use olr_vfs::VPath;
use serde::{Deserialize, Serialize};

/// A recovery's own file, beside the pack's.
pub const INFO_FILE: &str = "recovery.ron";

/// The most `recovery.ron` read.
const MAX_INFO_BYTES: u64 = 64 << 10;

/// `recovery/<id>/recovery.ron`: where the pack was edited, and its level's
/// name for the Editor screen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryInfo {
    pub origin: PathBuf,
    pub name: String,
}

/// A recovery id is one plain directory name: never a path out of `recovery/`.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with('.')
        && !id.contains(['/', '\\', ':'])
        && olr_vfs::non_portable(id).is_none()
}

fn checked(dir: &Path, id: &str) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(valid_id(id), "{id:?} is not a recovery");
    Ok(dir.join(id))
}

/// Writes `pack` (edited at `origin`, its level named `name`) as recovery
/// `id` in `dir`, replacing the last copy whole; first removes what an
/// interrupted write of this copy left beside it (`replace_dir`'s hidden
/// `.import-<id>` and `.replaced-<id>[~k]`). Returns `replace_dir`'s
/// warnings (an old copy that could not be removed).
pub fn write(dir: &Path, id: &str, pack: &Pack, info: &RecoveryInfo) -> anyhow::Result<Vec<String>> {
    let target = checked(dir, id)?;
    let files = olr_edit::pack::files(pack)?;
    let text = ron::ser::to_string_pretty(info, ron::ser::PrettyConfig::default())?;
    std::fs::create_dir_all(dir)?;
    let mut warnings = Vec::new();
    for entry in std::fs::read_dir(dir)?.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let replaced = name
            .strip_prefix(".replaced-")
            .and_then(|r| r.strip_prefix(id))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('~'));
        if (replaced || name == format!(".import-{id}"))
            && let Err(e) = std::fs::remove_dir_all(entry.path())
        {
            warnings.push(format!("{}: {e}", entry.path().display()));
        }
    }
    warnings.extend(olr_pack::install::replace_dir(&target, |tmp| {
        for (path, bytes) in &files {
            let file = tmp.join(path.as_str());
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(file, bytes)?;
        }
        std::fs::write(tmp.join(INFO_FILE), &text)
    })?);
    Ok(warnings)
}

/// Recovery `id`'s `recovery.ron`, bounded.
fn info(dir: &Path) -> anyhow::Result<RecoveryInfo> {
    let mut text = String::new();
    std::fs::File::open(dir.join(INFO_FILE))?.take(MAX_INFO_BYTES).read_to_string(&mut text)?;
    Ok(ron::from_str(&text)?)
}

/// The recoveries in `dir`: (id, level name, damaged), sorted by id. A
/// damaged one (its `recovery.ron` unreadable, or no `pack.ron`) is named
/// by its id. Hidden entries (a write in progress) are left out.
pub fn list_recoveries(dir: &Path) -> Vec<(String, String, bool)> {
    promote_leftovers(dir);
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, String, bool)> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let id = e.file_name().to_str()?.to_string();
            valid_id(&id).then_some(id)
        })
        .map(|id| {
            let path = dir.join(&id);
            match info(&path) {
                Ok(i) if path.join(olr_pack::MANIFEST).is_file() => (id, i.name, false),
                _ => (id.clone(), id, true),
            }
        })
        .collect();
    out.sort();
    out
}

/// A copy whose write a kill interrupted between `replace_dir`'s renames
/// is `<id>` again: its hidden new copy (`.import-<id>`) when it holds a
/// whole one, else its hidden old copy (`.replaced-<id>`), so a crash's
/// copy is offered and never cleaned away by the next write.
fn promote_leftovers(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let names: Vec<String> =
        rd.filter_map(Result::ok).filter_map(|e| e.file_name().to_str().map(str::to_string)).collect();
    for prefix in [".import-", ".replaced-"] {
        for name in &names {
            let Some(id) = name.strip_prefix(prefix).filter(|id| valid_id(id)) else { continue };
            let (from, to) = (dir.join(name), dir.join(id));
            if !to.exists() && from.join(INFO_FILE).is_file() && from.join(olr_pack::MANIFEST).is_file() {
                let _ = std::fs::rename(&from, &to);
            }
        }
    }
}

/// Removes recovery `id` from `dir` (absent: nothing to do).
pub fn discard_recovery(dir: &Path, id: &str) -> anyhow::Result<()> {
    let target = checked(dir, id)?;
    match std::fs::remove_dir_all(&target) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Recovery `id`: its pack (without `recovery.ron`) and its info.
pub fn load(dir: &Path, id: &str) -> anyhow::Result<(Pack, RecoveryInfo)> {
    let path = checked(dir, id)?;
    let info = info(&path).map_err(|e| anyhow::anyhow!("recovery {id} is damaged: {e}"))?;
    let mut pack =
        olr_edit::pack::load(&path).map_err(|e| anyhow::anyhow!("recovery {id} is damaged: {e}"))?;
    if let Ok(p) = VPath::new(INFO_FILE) {
        pack.other.remove(&p);
    }
    Ok((pack, info))
}
