//! `cargo xtask flatpak`: the Linux release build repackaged as a Flatpak
//! bundle, `target/release-zips/open-liero-reloaded-<version>-linux-x64.flatpak`.
//! The programs are `dist`'s (the `dist` profile, built on this machine),
//! staged as in the zip under `target/flatpak/src/app/` beside copies of
//! `tools/flatpak/`'s manifest, desktop entry, metainfo (`@VERSION@`
//! replaced) and icon; `flatpak-builder` exports them into a local
//! repository and `flatpak build-bundle` writes the bundle. Nothing is run.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::dist::{self, Platform, VERSION, ZIP_DIR};
use crate::root;

/// The application ID: the files' names in `tools/flatpak/` and the
/// window's Wayland app ID and X11 class (`olr-game`'s `window::APP_ID`).
pub const APP_ID: &str = "io.github.openlieroreloaded.OpenLieroReloaded";
/// The packaging files, in the repository.
const TOOL_DIR: &str = "tools/flatpak";
/// The packaging files' extensions after `APP_ID.`.
const FILES: [&str; 4] = ["yml", "desktop", "metainfo.xml", "svg"];
/// The bundle's runtimes come from here (`build-bundle --runtime-repo`).
const FLATHUB: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

/// The bundle's file name.
pub fn bundle_name(version: &str) -> String {
    format!("{}-{}.flatpak", dist::folder(version), Platform::Linux.suffix())
}

/// A packaging file's text `text` as copied for `version`.
fn packaging_text(text: &str, version: &str) -> String {
    text.replace("@VERSION@", version)
}

/// Why the Flatpak tools cannot be used, if they cannot.
fn tools_missing() -> Option<String> {
    let missing: Vec<&str> = ["flatpak", "flatpak-builder"]
        .into_iter()
        .filter(|t| Command::new(t).arg("--version").output().is_err())
        .collect();
    (!missing.is_empty()).then(|| {
        format!(
            "{} not found: `sudo apt install flatpak flatpak-builder` and `flatpak remote-add --user \
             --if-not-exists flathub {FLATHUB}`",
            missing.join(" and ")
        )
    })
}

fn run(what: &str, c: &mut Command) -> Result<(), String> {
    let status = c.status().map_err(|e| format!("cannot run {what}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what} failed ({status})")) }
}

/// Stages the programs and the packaging files in `src` (emptied first).
fn stage(build: &Path, src: &Path) -> Result<(), String> {
    let folder = dist::stage(Platform::Linux, build, src)?;
    let app = src.join("app");
    std::fs::rename(&folder, &app).map_err(|e| format!("{} to {}: {e}", folder.display(), app.display()))?;
    let tools = root().join(TOOL_DIR);
    for ext in FILES {
        let name = format!("{APP_ID}.{ext}");
        let from = tools.join(&name);
        let text = std::fs::read_to_string(&from).map_err(|e| format!("{}: {e}", from.display()))?;
        let to = src.join(&name);
        std::fs::write(&to, packaging_text(&text, VERSION)).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    Ok(())
}

/// `cargo xtask flatpak`.
pub fn command() -> anyhow::Result<bool> {
    if let Some(why) = tools_missing() {
        println!("FAIL flatpak: {why}");
        return Ok(false);
    }
    let target = root().join("target");
    let work = target.join("flatpak");
    let (src, repo) = (work.join("src"), work.join("repo"));
    println!("building olr-game and olr-server (the {} profile: LTO, minutes from cold)...", dist::PROFILE);
    let build = dist::build(Platform::Linux).map_err(anyhow::Error::msg)?;
    stage(&build, &src).map_err(anyhow::Error::msg)?;
    println!("building the Flatpak (the first run downloads the 26.08 runtime from Flathub)...");
    run(
        "flatpak-builder",
        Command::new("flatpak-builder")
            .args(["--user", "--install-deps-from=flathub", "--force-clean", "--disable-rofiles-fuse"])
            .arg("--state-dir")
            .arg(work.join("state"))
            .arg("--repo")
            .arg(&repo)
            .arg(work.join("build"))
            .arg(src.join(format!("{APP_ID}.yml"))),
    )
    .map_err(anyhow::Error::msg)?;
    let bundle: PathBuf = target.join(ZIP_DIR).join(bundle_name(VERSION));
    run(
        "flatpak build-bundle",
        Command::new("flatpak")
            .args(["build-bundle", &format!("--runtime-repo={FLATHUB}")])
            .arg(&repo)
            .arg(&bundle)
            .arg(APP_ID),
    )
    .map_err(anyhow::Error::msg)?;
    let bytes = std::fs::metadata(&bundle).map_or(0, |m| m.len());
    println!("{}  {:.1} MiB", bundle.display(), bytes as f64 / (1024.0 * 1024.0));
    println!("install: flatpak install --user {}", bundle.display());
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundle_is_named_like_the_zips() {
        assert_eq!(bundle_name("1.2.3"), "open-liero-reloaded-1.2.3-linux-x64.flatpak");
    }

    #[test]
    fn the_packaging_files_carry_the_app_id() {
        let tools = root().join(TOOL_DIR);
        for ext in FILES {
            let path = tools.join(format!("{APP_ID}.{ext}"));
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            if ext != "svg" {
                assert!(text.contains(APP_ID), "{} does not name {APP_ID}", path.display());
            }
        }
        let meta = std::fs::read_to_string(tools.join(format!("{APP_ID}.metainfo.xml"))).unwrap();
        assert!(packaging_text(&meta, "1.2.3").contains(r#"<release version="1.2.3"/>"#));
        // The window names itself so, for the desktop entry to match it.
        let window = std::fs::read_to_string(root().join("crates/olr-game/src/window.rs")).unwrap();
        assert!(window.contains(&format!("pub const APP_ID: &str = \"{APP_ID}\";")));
    }

    #[test]
    fn the_manifest_installs_every_packaging_file_and_the_programs() {
        let manifest = std::fs::read_to_string(root().join(TOOL_DIR).join(format!("{APP_ID}.yml"))).unwrap();
        assert!(manifest.contains(&format!("id: {APP_ID}")));
        for ext in FILES.iter().filter(|e| **e != "yml") {
            assert!(manifest.contains(&format!("install -Dm644 {APP_ID}.{ext} ")), "{ext}");
        }
        for exe in dist::EXES {
            assert!(manifest.contains(&format!("ln -s ../lib/olr/{exe} /app/bin/{exe}")), "{exe}");
        }
    }
}
