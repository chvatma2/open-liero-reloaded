//! The replays in the data directory's `replays/` (spec §21 G2): the
//! menu's list, and the matches saved there automatically (`match-NNNNNN`,
//! the last [`MAX_AUTO_REPLAYS`] kept). Files a player put there are
//! listed too and never removed.

use std::path::{Path, PathBuf};

/// A replay file's extension (`rl-replay`'s format).
pub const EXTENSION: &str = "rlrp";

/// Matches saved automatically, at most (the oldest go first).
pub const MAX_AUTO_REPLAYS: usize = 20;

/// The most files listed.
pub const MAX_LISTED: usize = 500;

const AUTO_PREFIX: &str = "match-";

/// A replay file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayFile {
    pub path: PathBuf,
    /// The file name.
    pub name: String,
    pub bytes: u64,
}

/// The number of an automatically saved replay's file name.
fn auto_number(name: &str) -> Option<u32> {
    let n = name.strip_prefix(AUTO_PREFIX)?.strip_suffix(EXTENSION)?.strip_suffix('.')?;
    if n.len() == 6 && n.bytes().all(|b| b.is_ascii_digit()) { n.parse().ok() } else { None }
}

/// The `.rlrp` files in `dir`, newest automatic saves first, then the
/// others by name; nothing when `dir` cannot be read.
pub fn list(dir: &Path) -> Vec<ReplayFile> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<ReplayFile> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let meta = e.metadata().ok()?;
            let ext = Path::new(&name).extension().and_then(|x| x.to_str());
            (meta.is_file() && ext == Some(EXTENSION)).then(|| ReplayFile {
                path: e.path(),
                name,
                bytes: meta.len(),
            })
        })
        .collect();
    files.sort_by(|a, b| match (auto_number(&a.name), auto_number(&b.name)) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.name.cmp(&b.name),
    });
    files.truncate(MAX_LISTED);
    files
}

/// Saves `bytes` as the next automatic replay in `dir` (created if
/// needed), then removes the oldest beyond [`MAX_AUTO_REPLAYS`]; returns
/// the new file.
pub fn auto_save(dir: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut numbers: Vec<u32> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter_map(|e| e.file_name().to_str().and_then(auto_number))
        .collect();
    numbers.sort_unstable();
    let next = numbers.last().map_or(1, |n| (n + 1) % 1_000_000);
    let path = dir.join(format!("{AUTO_PREFIX}{next:06}.{EXTENSION}"));
    let tmp = dir.join(format!("{AUTO_PREFIX}{next:06}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)?;
    numbers.push(next);
    let excess = numbers.len().saturating_sub(MAX_AUTO_REPLAYS);
    for n in numbers.iter().filter(|&&n| n != next).take(excess) {
        // An old replay that cannot be removed only costs disk space.
        let _ = std::fs::remove_file(dir.join(format!("{AUTO_PREFIX}{n:06}.{EXTENSION}")));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_saves_are_numbered_listed_newest_first_and_capped() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("replays");
        assert!(list(&dir).is_empty(), "no directory yet");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("mine.rlrp"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        for i in 0..MAX_AUTO_REPLAYS + 3 {
            let p = auto_save(&dir, &[i as u8; 3]).unwrap();
            assert_eq!(p.file_name().unwrap(), format!("match-{:06}.rlrp", i + 1).as_str());
        }
        let l = list(&dir);
        assert_eq!(l.len(), MAX_AUTO_REPLAYS + 1, "the player's file stays");
        assert_eq!(l[0].name, format!("match-{:06}.rlrp", MAX_AUTO_REPLAYS + 3));
        assert_eq!(l[0].bytes, 3);
        assert_eq!(l.last().unwrap().name, "mine.rlrp");
        assert_eq!(auto_number("match-000012.rlrp"), Some(12));
        assert_eq!(auto_number("match-12.rlrp"), None);
    }
}
