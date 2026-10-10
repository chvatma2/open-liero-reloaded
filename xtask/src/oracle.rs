//! `cargo xtask oracle-record`: records OLX reference traces for the scenarios
//! in `fixtures/oracle/*.ron` with the patched OpenLieroX in `tools/olx-oracle/`
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
    Ok(tag)
}

fn id(flag: &str) -> anyhow::Result<String> {
    let out = Command::new("id").arg(flag).output().context("running id")?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
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
    if !log.contains("ORACLE_DONE") {
        let errors: Vec<&str> =
            log.lines().filter(|l| l.contains("ORACLE_ERROR") || l.starts_with("E:")).collect();
        bail!("OLX did not finish the scenario:\n{}\n...\n{}", errors.join("\n"), tail(&log, 20));
    }
    std::fs::read_to_string(work.join("trace.jsonl")).context("OLX wrote no trace")
}

/// Scenario files in `fixtures/oracle/`, sorted, optionally only `names`.
fn scenarios(root: &Path, names: &[String]) -> anyhow::Result<Vec<(String, PathBuf)>> {
    let mut all = Vec::new();
    for e in std::fs::read_dir(root.join(FIXTURES)).context("reading fixtures/oracle")? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "ron") {
            let stem = p.file_stem().expect("file name").to_string_lossy().to_string();
            all.push((stem, p));
        }
    }
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
    fn every_scenario_is_listed_and_unknown_names_fail() {
        let all = scenarios(&root(), &[]).unwrap();
        assert!(!all.is_empty());
        assert!(scenarios(&root(), &["no-such-scenario".into()]).is_err());
    }
}
