//! The names an import leaves in a content directory while it replaces a
//! pack (plan 13c task 4): `olr-import` writes them and `olr-content`'s
//! `open_layers` lists a pack an import killed halfway left aside.

use std::path::PathBuf;

/// A pack being written.
pub const TMP_PREFIX: &str = ".import-";
/// A pack being replaced (`olr_content::Content::open_layers` lists one
/// whose id has no other pack: an import killed between its two renames).
pub const OLD_PREFIX: &str = ".replaced-";

/// Sorts `paths` so the packs moved aside (`.replaced-<id>`,
/// `.replaced-<id>~k`) come first, newest first: by their manifest's
/// modification time (written by the import that made the pack), then by
/// the highest `k` (a later import takes a higher slot while a lower one
/// cannot be removed), then by name. Other paths keep their order after them.
pub fn newest_aside_first(paths: &mut [PathBuf]) {
    let key = |p: &PathBuf| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let rest = name.strip_prefix(OLD_PREFIX)?;
        let k = rest.rsplit_once('~').and_then(|(_, k)| k.parse::<u32>().ok()).unwrap_or(1);
        let time = std::fs::metadata(p.join(crate::MANIFEST))
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        Some((std::cmp::Reverse(time), std::cmp::Reverse(k)))
    };
    // `None` (not moved aside) sorts after every `Some`.
    paths.sort_by_cached_key(|p| (key(p).is_none(), key(p), p.clone()));
}
