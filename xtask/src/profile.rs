//! `cargo xtask profile`: the render profile (`olr-cli render-profile`,
//! spec `docs/superpowers/specs/2026-10-24-render-profile-design.md`) on
//! the Windows GPU, staged and run from WSL as the envelope does
//! (`winbuild`), or with `--native` the Linux build here. Each run's
//! report is `target/render-profile/last.ron`, compared with the
//! backend's base (`base-<backend>.ron`, written by `--set-base` after a
//! run that worked). `--game` then plays the fight's replay in `olr.exe`
//! fullscreen and sets the game's frame log beside the profile.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::envelope::results::FrameLogOut;
use crate::{root, winbuild};

/// The run's report and the fight's replay, as the profile writes them
/// (in its working directory).
pub const LAST: &str = "last.ron";
pub const REPLAY: &str = "fight.olrp";
/// Where the reports are kept.
pub const DIR: &str = "target/render-profile";
/// The game plays the 20 s fight and a little more.
const GAME_SECONDS: u32 = 22;

/// The base a `backend`'s runs compare with: the Windows build's, or
/// the native build's (`--native`, another machine's GPU or the software
/// adapter) apart. `olr-cli` also refuses an unlike base (another adapter,
/// window or frame rate) rather than compare it.
pub fn base_name(backend: &str, native: bool) -> String {
    if native { format!("base-native-{backend}.ron") } else { format!("base-{backend}.ron") }
}

/// `olr-cli`'s arguments: on `backend`, against `base` when there is one,
/// saving the fight's replay when asked, `extra` last.
pub fn profile_args(backend: &str, base: Option<&str>, save_replay: bool, extra: &[String]) -> Vec<String> {
    let mut a: Vec<String> =
        ["render-profile", "--backend", backend, "--out", LAST].map(String::from).to_vec();
    if let Some(b) = base {
        a.extend(["--against".to_string(), b.to_string()]);
    }
    if save_replay {
        a.extend(["--save-replay".to_string(), REPLAY.to_string()]);
    }
    a.extend(extra.iter().cloned());
    a
}

/// Keeps a run's report in `dir`: `last.ron`, and the base `base` too
/// with `set_base`; nothing when the run failed.
pub fn finish(dir: &Path, base: &str, result: Result<String, String>, set_base: bool) -> Result<(), String> {
    let text = result?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let write = |name: &str| std::fs::write(dir.join(name), &text).map_err(|e| format!("{name}: {e}"));
    write(LAST)?;
    if set_base {
        write(base)?;
        println!("  the base is now this run ({DIR}/{base})");
    }
    Ok(())
}

/// What the game check reads of the profile.
#[derive(Deserialize)]
struct ProfileOut {
    sizes: Vec<SizeOut>,
}

#[derive(Deserialize)]
struct SizeOut {
    size: (u32, u32),
    phases: Vec<PhaseOut>,
}

#[derive(Deserialize)]
struct PhaseOut {
    phase: PhaseName,
    frames: u32,
    gpu_ms: Option<StatOut>,
    cpu_ms: StatOut,
}

#[derive(Debug, Deserialize)]
enum PhaseName {
    Calm,
    Blast,
    Hot,
}

#[derive(Deserialize)]
struct StatOut {
    mean: f64,
    p99: f64,
    max: f64,
}

/// The game's frame log beside the profile's frames at the game's size.
fn game_check(log: &FrameLogOut, profile: &str) -> String {
    let mut t = format!(
        "game check: {} at {}x{}: {:.0} fps, frame ms p50 {:.2} p99 {:.2} max {:.2}, busy ms p50 {:.2} p99 {:.2}\n",
        log.gpu,
        log.size.0,
        log.size.1,
        log.fps,
        log.frame_ms.p50,
        log.frame_ms.p99,
        log.frame_ms.max,
        log.busy_ms.p50,
        log.busy_ms.p99
    );
    let Ok(p) = ron::from_str::<ProfileOut>(profile) else {
        t.push_str("  (the profile's report did not read)\n");
        return t;
    };
    let near = |s: &&SizeOut| {
        (u64::from(s.size.0) * u64::from(s.size.1)).abs_diff(u64::from(log.size.0) * u64::from(log.size.1))
    };
    if let Some(s) = p.sizes.iter().min_by_key(near) {
        t.push_str(&format!("  the profile at {}x{}:\n", s.size.0, s.size.1));
        for ph in &s.phases {
            let gpu = ph
                .gpu_ms
                .as_ref()
                .map_or("-".to_string(), |g| format!("{:.2}/{:.2}/{:.2}", g.mean, g.p99, g.max));
            t.push_str(&format!(
                "    {:?}: {} frames, gpu {gpu} ms (mean/p99/max), cpu {:.2}/{:.2}/{:.2}\n",
                ph.phase, ph.frames, ph.cpu_ms.mean, ph.cpu_ms.p99, ph.cpu_ms.max
            ));
        }
    }
    t
}

/// `content/reloaded` beside the stage's content (`Content::open_bundled`
/// lays it over `content`), copied again only when it changed.
fn stage_reloaded(stage: &winbuild::Stage) -> Result<(), String> {
    let src = root().join("content/reloaded");
    let (dst, stamp_file) = (stage.dir.join("reloaded"), stage.dir.join("reloaded.stamp"));
    let stamp = winbuild::content_stamp(&src)?;
    if std::fs::read_to_string(&stamp_file).ok().as_deref() == Some(stamp.as_str()) {
        return Ok(());
    }
    match std::fs::remove_dir_all(&dst) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("{}: {e}", dst.display())),
        _ => {}
    }
    winbuild::copy_dir(&src, &dst)?;
    std::fs::write(&stamp_file, stamp).map_err(|e| format!("{}: {e}", stamp_file.display()))
}

/// `cargo xtask profile`.
pub fn command(
    backend: &str,
    native: bool,
    set_base: bool,
    game: bool,
    extra: &[String],
) -> anyhow::Result<bool> {
    let keep = root().join(DIR);
    let name = base_name(backend, native);
    let base = keep.join(&name);
    let has_base = base.is_file();
    if native {
        anyhow::ensure!(!game, "--game plays the Windows build: leave out --native");
        println!("building olr-cli (release)...");
        let built = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .current_dir(root())
            .args(["build", "--quiet", "--release", "-p", "olr-cli"])
            .status()?;
        anyhow::ensure!(built.success(), "building olr-cli failed");
        std::fs::create_dir_all(&keep)?;
        let mut more = vec!["--content".to_string(), root().join("content/packs").display().to_string()];
        more.extend(extra.iter().cloned());
        let args = profile_args(backend, has_base.then_some(name.as_str()), false, &more);
        let _ = std::fs::remove_file(keep.join(LAST));
        let status =
            Command::new(root().join("target/release/olr-cli")).current_dir(&keep).args(&args).status()?;
        let result = if status.success() {
            std::fs::read_to_string(keep.join(LAST)).map_err(|e| format!("{LAST}: {e}"))
        } else {
            Err(format!("olr-cli render-profile failed ({status})"))
        };
        return match finish(&keep, &name, result, set_base) {
            Ok(()) => Ok(true),
            Err(why) => {
                println!("FAIL profile: {why}");
                Ok(false)
            }
        };
    }
    if let Some(why) = winbuild::missing() {
        println!("FAIL profile: {why} (or run with --native)");
        return Ok(false);
    }
    println!("building olr-cli and olr-game for {} (release)...", winbuild::TARGET);
    let release = winbuild::build().map_err(anyhow::Error::msg)?;
    let files: Vec<PathBuf> = if has_base { vec![base.clone()] } else { Vec::new() };
    let stage =
        winbuild::stage(&release, &root().join("content/packs"), &files).map_err(anyhow::Error::msg)?;
    stage_reloaded(&stage).map_err(anyhow::Error::msg)?;
    let staged_base = format!("replays/{name}");
    let mut more = vec!["--content".to_string(), "content".to_string()];
    more.extend(extra.iter().cloned());
    let args = profile_args(backend, has_base.then_some(staged_base.as_str()), game, &more);
    let _ = std::fs::remove_file(stage.dir.join(LAST));
    println!("profiling on the Windows GPU ({backend}), about a minute...");
    let run = winbuild::run(&stage, "olr-cli.exe", &args, &[], winbuild::RUN_TIMEOUT);
    let result = run.and_then(|out| {
        print!("{}", String::from_utf8_lossy(&out.stdout));
        std::fs::read_to_string(stage.dir.join(LAST)).map_err(|e| format!("{LAST}: {e}"))
    });
    let profile = result.clone().unwrap_or_default();
    if let Err(why) = finish(&keep, &name, result, set_base) {
        println!("FAIL profile: {why}");
        return Ok(false);
    }
    if !game {
        return Ok(true);
    }
    println!("game check: the game opens fullscreen on the display for about {GAME_SECONDS} s");
    let log = stage.dir.join("frame-log.ron");
    let _ = std::fs::remove_file(&log);
    let secs = GAME_SECONDS.to_string();
    let args: Vec<String> = [
        "--replay",
        REPLAY,
        "--fullscreen",
        "--uncapped",
        "--filter",
        "sharp",
        "--no-sound",
        "--autoquit-after",
        &secs,
        "--frame-log",
        "frame-log.ron",
        "--frame-log-skip",
        "1",
        "--content",
        "content",
        "--data-dir",
        "game-data",
    ]
    .map(String::from)
    .to_vec();
    let out = winbuild::run(&stage, "olr.exe", &args, &[("WGPU_BACKEND", backend)], winbuild::RUN_TIMEOUT);
    println!("game check: done");
    let read = out.and_then(|_| {
        let text = std::fs::read_to_string(&log).map_err(|e| format!("{}: {e}", log.display()))?;
        ron::from_str::<FrameLogOut>(&text).map_err(|e| format!("{}: {e}", log.display()))
    });
    match read {
        Ok(l) => {
            print!("{}", game_check(&l, &profile));
            Ok(true)
        }
        Err(why) => {
            println!("FAIL profile --game: {why}");
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_compares_with_its_backends_base() {
        assert_eq!(base_name("vulkan", false), "base-vulkan.ron");
        assert_eq!(base_name("vulkan", true), "base-native-vulkan.ron", "a native run's base is its own");
        let a = profile_args("vulkan", Some("base-vulkan.ron"), false, &[]);
        assert_eq!(a[0], "render-profile");
        assert!(a.windows(2).any(|w| w == ["--backend", "vulkan"]));
        assert!(a.windows(2).any(|w| w == ["--against", "base-vulkan.ron"]));
        assert!(a.windows(2).any(|w| w == ["--out", "last.ron"]));
        assert!(!a.contains(&"--save-replay".to_string()));
        let b = profile_args("dx12", None, true, &["--ticks".into(), "0..100".into()]);
        assert!(!b.contains(&"--against".to_string()));
        assert!(b.windows(2).any(|w| w == ["--save-replay", REPLAY]));
        assert_eq!(b[b.len() - 2..], ["--ticks", "0..100"], "the extra args last");
    }

    #[test]
    fn a_failed_run_leaves_the_base_alone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("base-vulkan.ron"), "old").unwrap();
        assert!(finish(dir.path(), "base-vulkan.ron", Err("timed out".into()), true).is_err());
        assert_eq!(std::fs::read_to_string(dir.path().join("base-vulkan.ron")).unwrap(), "old");
        assert!(!dir.path().join("last.ron").exists());
        finish(dir.path(), "base-vulkan.ron", Ok("new".into()), false).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("last.ron")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(dir.path().join("base-vulkan.ron")).unwrap(), "old");
        finish(dir.path(), "base-vulkan.ron", Ok("newer".into()), true).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("base-vulkan.ron")).unwrap(), "newer");
    }
}
