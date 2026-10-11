//! `cargo xtask oracle-record`: records OLX reference traces for the scenarios
//! in `fixtures/oracle/**/*.ron` with the patched OpenLieroX in `tools/olx-oracle/`
//! (needs Docker). Each scenario is recorded twice and must come out identical,
//! which proves the recording is deterministic. `--check` compares with the
//! committed traces instead of writing them.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use rl_oracle::{Scenario, Trace};
use sha2::{Digest, Sha256};

use crate::root;

const TOOL_DIR: &str = "tools/olx-oracle";
const FIXTURES: &str = "fixtures/oracle";
const TOOL_FILES: [&str; 3] = ["Dockerfile", "oracle-hooks.patch", "oracle_control.py"];
/// Kills a hung OLX after this many seconds.
const RUN_TIMEOUT_SECS: u32 = 300;

/// Docker image tag derived from the oracle's input files, so editing any of
/// them builds a new image instead of silently reusing a stale one.
pub fn image_tag(files: &[(&str, &[u8])]) -> String {
    let mut h = Sha256::new();
    for (name, data) in files {
        h.update(name.as_bytes());
        h.update([0u8]);
        h.update((data.len() as u64).to_le_bytes());
        h.update(data);
    }
    format!("rl-olx-oracle:{}", &hex::encode(h.finalize())[..12])
}

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn ensure_image(root: &Path) -> anyhow::Result<String> {
    let mut files = Vec::new();
    for f in TOOL_FILES {
        let path = root.join(TOOL_DIR).join(f);
        files.push((f, std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?));
    }
    let tag = image_tag(&files.iter().map(|(n, d)| (*n, d.as_slice())).collect::<Vec<_>>());
    let exists = Command::new("docker")
        .args(["image", "inspect", &tag])
        .output()
        .context("running docker (is Docker installed and running?)")?
        .status
        .success();
    if !exists {
        println!("building {tag} (OLX from source; takes a few minutes the first time)");
        let out = Command::new("docker").args(["build", "-t", &tag]).arg(root.join(TOOL_DIR)).output()?;
        if !out.status.success() {
            bail!("docker build failed:\n{}", tail(&String::from_utf8_lossy(&out.stderr), 40));
        }
    }
    prune_old(&tag, false);
    Ok(tag)
}

/// Which of `images` (`repo:tag` lines) to remove after building `current`:
/// other `rl-olx-oracle` images, except the tag `dev` (kept for manual use).
pub fn images_to_prune<'a>(images: &[&'a str], current: &str) -> Vec<&'a str> {
    images
        .iter()
        .copied()
        .filter(|i| {
            i.strip_prefix("rl-olx-oracle:").is_some_and(|t| !t.is_empty() && t != "dev") && *i != current
        })
        .collect()
}

/// The `docker` commands that prune after building `current`, given the
/// `images` (`repo:tag` lines): remove each superseded oracle image
/// ([`images_to_prune`]; none when the current tag is unknown), then, with
/// `build_cache` only, bound Docker's build cache. That cache is the whole
/// machine's (Docker cannot select one project's layers), so it is opt-in:
/// `cargo xtask tidy --docker-cache`.
pub fn prune_commands(images: &[&str], current: Option<&str>, build_cache: bool) -> Vec<Vec<String>> {
    let old = current.map(|c| images_to_prune(images, c)).unwrap_or_default();
    let mut out: Vec<Vec<String>> =
        old.into_iter().map(|i| vec!["image".into(), "rm".into(), i.into()]).collect();
    if build_cache {
        out.push(["builder", "prune", "-f", "--keep-storage", "2GB"].map(String::from).to_vec());
    }
    out
}

/// `tidy`: like [`prune_old`] for the current image tag, whatever it is.
/// When the tag cannot be computed no image is removed (which one is
/// current is unknown), but an asked-for build-cache prune still runs.
pub fn prune_all(build_cache: bool) {
    let tag = TOOL_FILES
        .iter()
        .map(|f| std::fs::read(root().join(TOOL_DIR).join(f)).map(|d| (*f, d)))
        .collect::<Result<Vec<_>, _>>()
        .map(|files| image_tag(&files.iter().map(|(n, d)| (*n, d.as_slice())).collect::<Vec<_>>()));
    match tag {
        Ok(tag) => prune_old(&tag, build_cache),
        Err(e) => {
            eprintln!("warning: cannot compute oracle image tag: {e}; no oracle image is removed");
            run_docker(prune_commands(&[], None, build_cache));
        }
    }
}

/// Runs [`prune_commands`] for `current`. Failures are warnings: pruning
/// is housekeeping and must not fail a recording.
fn prune_old(current: &str, build_cache: bool) {
    let list = Command::new("docker")
        .args(["image", "ls", "rl-olx-oracle", "--format", "{{.Repository}}:{{.Tag}}"])
        .output();
    let text = match list {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("warning: could not list oracle images to prune");
            String::new()
        }
    };
    let images: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    run_docker(prune_commands(&images, Some(current), build_cache));
}

fn run_docker(commands: Vec<Vec<String>>) {
    for args in commands {
        match Command::new("docker").args(&args).output() {
            Ok(o) if o.status.success() && args[0] == "image" => {
                println!("removed superseded image {}", args[2])
            }
            Ok(o) if o.status.success() => {}
            _ => eprintln!("warning: `docker {}` failed", args.join(" ")),
        }
    }
}

fn id(flag: &str) -> anyhow::Result<String> {
    let out = Command::new("id").arg(flag).output().context("running id")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Messages that OLX console commands (`setvar`, `simLoadInputs`, `spawnWorm`,
/// `simSetSeed`, ...) wrote back to the control script. OLX only logs them, so
/// a failed command would otherwise go unnoticed; a clean run has none.
fn olx_command_errors(log: &str) -> Vec<&str> {
    log.lines().filter(|l| l.contains("Script Dedicated:")).collect()
}

/// Plays `sc` once in OLX and returns the trace text.
fn record_once(image: &str, user: &str, work: &Path, sc: &Scenario) -> anyhow::Result<String> {
    if work.exists() {
        std::fs::remove_dir_all(work)?;
    }
    std::fs::create_dir_all(work.join("home"))?;
    std::fs::write(work.join("scenario.json"), sc.control_json("/work/trace.jsonl"))?;
    let out = Command::new("docker")
        .args(["run", "--rm", "--init", "-u", user, "-v"])
        .arg(format!("{}:/work", work.display()))
        .args(["-e", "HOME=/work/home", "-e", "ORACLE_SCENARIO=/work/scenario.json", image])
        .args(["timeout", "-s", "KILL", &RUN_TIMEOUT_SECS.to_string(), "/olx/bin/openlierox"])
        .args(["-dedicated", "-disablestdincli", "-script", "/olx/oracle_control.py"])
        .output()
        .context("running docker")?;
    let log = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    std::fs::write(work.join("olx.log"), &log)?;
    let errors = olx_command_errors(&log);
    if !errors.is_empty() {
        bail!("an OLX command failed (see {}):\n{}", work.join("olx.log").display(), errors.join("\n"));
    }
    if !log.contains("ORACLE_DONE") {
        let errors: Vec<&str> =
            log.lines().filter(|l| l.contains("ORACLE_ERROR") || l.starts_with("E:")).collect();
        bail!("OLX did not finish the scenario:\n{}\n...\n{}", errors.join("\n"), tail(&log, 20));
    }
    std::fs::read_to_string(work.join("trace.jsonl")).context("OLX wrote no trace")
}

/// Scenario files in `fixtures/oracle/` and its subdirectories, sorted by name
/// (the path without `.ron`, such as `weapons/classic/bazooka`), optionally only `names`.
fn scenarios(root: &Path, names: &[String]) -> anyhow::Result<Vec<(String, PathBuf)>> {
    fn walk(dir: &Path, prefix: &str, all: &mut Vec<(String, PathBuf)>) -> anyhow::Result<()> {
        for e in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let p = e?.path();
            if p.is_dir() {
                let sub = p.file_name().expect("directory name").to_string_lossy().to_string();
                walk(&p, &format!("{prefix}{sub}/"), all)?;
            } else if p.extension().is_some_and(|x| x == "ron") {
                let stem = p.file_stem().expect("file name").to_string_lossy().to_string();
                all.push((format!("{prefix}{stem}"), p));
            }
        }
        Ok(())
    }
    let mut all = Vec::new();
    walk(&root.join(FIXTURES), "", &mut all)?;
    all.sort();
    for n in names {
        if !all.iter().any(|(s, _)| s == n) {
            bail!("no scenario fixtures/oracle/{n}.ron");
        }
    }
    Ok(all.into_iter().filter(|(s, _)| names.is_empty() || names.contains(s)).collect())
}

pub fn run(names: &[String], check: bool) -> anyhow::Result<bool> {
    let root = root();
    let list = scenarios(&root, names)?;
    let image = ensure_image(&root)?;
    let user = format!("{}:{}", id("-u")?, id("-g")?);
    let mut ok = true;
    for (name, path) in list {
        let sc = Scenario::from_ron(&std::fs::read_to_string(&path)?)
            .with_context(|| format!("{}", path.display()))?;
        let work = root.join("target/oracle").join(&name);
        let a = record_once(&image, &user, &work.join("a"), &sc).with_context(|| name.clone())?;
        let b = record_once(&image, &user, &work.join("b"), &sc).with_context(|| name.clone())?;
        let (ta, tb) = (Trace::from_jsonl(&a)?, Trace::from_jsonl(&b)?);
        if a != b {
            let why =
                rl_oracle::compare(&ta, &tb, &[]).err().map_or("header differs".into(), |d| d.to_string());
            println!("FAIL {name}: two recordings differ, OLX is not deterministic here: {why}");
            ok = false;
            continue;
        }
        if ta.frames.len() != sc.frames as usize + 1 {
            println!(
                "FAIL {name}: expected {} frames (-1..{}), got {}",
                sc.frames + 1,
                sc.frames - 1,
                ta.frames.len()
            );
            ok = false;
            continue;
        }
        let gz_path = root.join(FIXTURES).join(format!("{name}.jsonl.gz"));
        if check {
            let committed = match std::fs::read(&gz_path) {
                Ok(bytes) => Trace::from_gz(&bytes)?,
                Err(_) => {
                    println!("FAIL {name}: no committed trace {}", gz_path.display());
                    ok = false;
                    continue;
                }
            };
            match rl_oracle::compare(&committed, &ta, &[]) {
                Ok(n) if committed.header == ta.header => {
                    println!("ok   {name}: matches the committed trace ({n} frames)")
                }
                Ok(_) => {
                    println!(
                        "FAIL {name}: header (OLX version or settings) differs from the committed trace"
                    );
                    ok = false;
                }
                Err(d) => {
                    println!("FAIL {name}: {d}");
                    ok = false;
                }
            }
        } else {
            std::fs::write(&gz_path, rl_oracle::gzip(&a))?;
            println!("ok   {name}: recorded {} frames", ta.frames.len());
        }
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_tag_depends_on_names_and_contents() {
        let t = image_tag(&[("a", b"1"), ("b", b"2")]);
        assert!(t.starts_with("rl-olx-oracle:") && t.len() == "rl-olx-oracle:".len() + 12, "{t}");
        assert_eq!(t, image_tag(&[("a", b"1"), ("b", b"2")]));
        assert_ne!(t, image_tag(&[("a", b"1"), ("b", b"3")]));
        assert_ne!(t, image_tag(&[("a", b"12"), ("b", b"")]));
    }

    #[test]
    fn prune_keeps_current_and_dev_only() {
        let all = [
            "rl-olx-oracle:aaa",
            "rl-olx-oracle:bbb",
            "rl-olx-oracle:dev",
            "rl-olx-oracle:<none>x",
            "ubuntu:22.04",
            "rl-olx-oracle-other:old",
        ];
        assert_eq!(
            images_to_prune(&all, "rl-olx-oracle:aaa"),
            ["rl-olx-oracle:bbb", "rl-olx-oracle:<none>x"]
        );
        assert!(images_to_prune(&[], "rl-olx-oracle:aaa").is_empty());
    }

    /// Plan 6's final review: Docker's build cache is the machine's, not
    /// this project's, so only `cargo xtask tidy --docker-cache` prunes it;
    /// `oracle-record` and a plain `tidy` remove only superseded oracle images.
    #[test]
    fn build_cache_is_pruned_only_when_asked() {
        let images = ["rl-olx-oracle:aaa", "rl-olx-oracle:bbb", "ubuntu:22.04"];
        let plain = prune_commands(&images, Some("rl-olx-oracle:aaa"), false);
        assert_eq!(plain, [vec!["image", "rm", "rl-olx-oracle:bbb"]]);
        let asked = prune_commands(&images, Some("rl-olx-oracle:aaa"), true);
        assert_eq!(asked[..1], plain[..]);
        assert_eq!(asked[1], ["builder", "prune", "-f", "--keep-storage", "2GB"]);
        assert_eq!(asked.len(), 2);
        // Without a current tag (it cannot be computed), the opt-in cache
        // prune still runs and no image is removed.
        let untagged = prune_commands(&images, None, true);
        assert_eq!(untagged, [["builder", "prune", "-f", "--keep-storage", "2GB"]]);
    }

    #[test]
    fn olx_command_errors_are_found_in_the_log() {
        let clean = "n: Loading (100%): Done!\nH: DEDICATED MODE\nORACLE_DONE frames=200\n";
        assert!(olx_command_errors(clean).is_empty());
        let bad = "n: x\nH: Script Dedicated: SetVar: no var with name GameOptions.GameInfo.GameSped\n\
                   E: Script Dedicated: worm 3 is not local\nORACLE_DONE frames=200\n";
        assert_eq!(olx_command_errors(bad).len(), 2);
    }

    #[test]
    fn every_scenario_is_listed_and_unknown_names_fail() {
        let all = scenarios(&root(), &[]).unwrap();
        assert!(all.iter().any(|(n, _)| n == "walk"));
        assert!(
            all.iter()
                .any(|(n, p)| n == "weapons/classic/bazooka" && p.ends_with("weapons/classic/bazooka.ron"))
        );
        assert!(scenarios(&root(), &["no-such-scenario".into()]).is_err());
    }
}
