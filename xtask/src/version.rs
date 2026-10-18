//! The version a release carries (`dist`, `flatpak`): a work in progress
//! by default, `<version>-dev.<commits since the last release tag>.<commit>`
//! (`.dirty` when the working tree has changes), or, with `--final`, the
//! plain version, only from a clean tree whose commit is tagged
//! `v<version>`. The builds get it as `BUILD_VERSION_VAR`: the game and the
//! server show it (`--version`, About, the main menu, the log).

use std::process::Command;

use crate::root;

/// The variable the release builds read their version from (`olr-game`'s
/// and `olr-server`'s `VERSION`).
pub const BUILD_VERSION_VAR: &str = "OLR_BUILD_VERSION";

/// A work in progress of `version`: `<version>-dev.<commits>.<hash>`, and
/// `.dirty` with uncommitted changes.
pub fn wip_version(version: &str, commits: u32, hash: &str, dirty: bool) -> String {
    format!("{version}-dev.{commits}.{hash}{}", if dirty { ".dirty" } else { "" })
}

/// The final release `version`, when the commit carries its tag
/// `v<version>` (`tags`: the tags at the commit) and nothing is uncommitted.
pub fn final_version(version: &str, tags: &[String], dirty: bool) -> Result<String, String> {
    let tag = format!("v{version}");
    if !tags.contains(&tag) {
        return Err(format!(
            "the commit is not tagged {tag}: tag it first (git tag {tag}), or build without --final"
        ));
    }
    if dirty {
        return Err("the working tree has uncommitted changes: commit or stash them first".into());
    }
    Ok(version.to_string())
}

/// A release tag: `v` and three numbers (`v0.1.0`; not `v0.1.0-local-play`).
pub fn is_release_tag(tag: &str) -> bool {
    tag.strip_prefix('v').is_some_and(|v| {
        let parts: Vec<&str> = v.split('.').collect();
        parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    })
}

fn git(args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root())
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The version the releases built now carry: `final_version` with `final_`,
/// else `wip_version` counting commits since the newest release tag the
/// commit has (all of them without one). Untracked files are not changes.
pub fn release_version(final_: bool) -> Result<String, String> {
    let version = crate::dist::VERSION;
    let dirty = !git(&["status", "--porcelain", "--untracked-files=no"])?.is_empty();
    if final_ {
        let tags: Vec<String> = git(&["tag", "--points-at", "HEAD"])?.lines().map(str::to_string).collect();
        return final_version(version, &tags, dirty);
    }
    let merged = git(&["tag", "--merged", "HEAD", "--sort=-creatordate"])?;
    let last = merged.lines().find(|t| is_release_tag(t));
    let range = last.map_or_else(|| "HEAD".to_string(), |t| format!("{t}..HEAD"));
    let commits = git(&["rev-list", "--count", &range])?.parse().map_err(|e| format!("git rev-list: {e}"))?;
    let hash = git(&["rev-parse", "--short=7", "HEAD"])?;
    Ok(wip_version(version, commits, &hash, dirty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_work_in_progress_names_its_commit() {
        assert_eq!(wip_version("0.2.0", 42, "abc1234", false), "0.2.0-dev.42.abc1234");
        assert_eq!(wip_version("0.2.0", 0, "abc1234", true), "0.2.0-dev.0.abc1234.dirty");
    }

    #[test]
    fn a_final_release_needs_its_tag_and_a_clean_tree() {
        let tags = |t: &[&str]| t.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(final_version("0.2.0", &tags(&["v0.2.0"]), false), Ok("0.2.0".to_string()));
        assert!(final_version("0.2.0", &tags(&["v0.2.0"]), true).unwrap_err().contains("uncommitted"));
        assert!(final_version("0.2.0", &tags(&[]), false).unwrap_err().contains("v0.2.0"));
        assert!(final_version("0.2.0", &tags(&["v0.1.0", "v0.2.0-rc"]), false).is_err());
    }

    /// The release tags `wip` counts from: `v<x.y.z>` only, not
    /// `v0.1.0-local-play`.
    #[test]
    fn release_tags_are_plain_versions() {
        assert!(is_release_tag("v0.1.0"));
        assert!(is_release_tag("v12.0.3"));
        assert!(!is_release_tag("v0.1.0-local-play"));
        assert!(!is_release_tag("0.1.0"));
        assert!(!is_release_tag("v0.1"));
    }

    /// This repository's version as `dist` would name it now: either form.
    #[test]
    fn the_repository_has_a_version() {
        let v = release_version(false).unwrap();
        assert!(v.starts_with(&format!("{}-dev.", crate::dist::VERSION)), "{v}");
    }
}
