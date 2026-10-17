//! `cargo xtask determinism [--windows] [--arm64] [--all]` (networking spec
//! §9.5): rollback rests on identical simulation everywhere, so golden bot
//! matches are recorded natively and `olr-detcheck` (replays: keys only, the
//! bots' own maths is not under test) prints the world's checksum stream on
//! native Linux, on WASI (`wasm32-wasip1` under Node.js,
//! `tools/wasi-run.mjs`), on Windows (plan 8's interop runner) and on arm64
//! Linux (QEMU user mode); every leg's stream must equal the native one.
//! With no leg flag the WASM leg runs; a leg flag runs that leg only. A
//! leg whose tools are missing is skipped with a notice naming the fix.

use std::path::PathBuf;
use std::process::Command;

use anyhow::Context as _;
use serde::Deserialize;

use crate::root;
use crate::winbuild;

const MATCHES: &str = "fixtures/matches";
/// A composed level (plan 14a): every leg also prints its bake's digest
/// (`olr-detcheck --bake`), so the image pipeline and the bake are
/// compared across platforms with the simulation.
const BAKE: &str = "fixtures/edit/composed-level";
const OUT: &str = "target/determinism";
/// The golden `check` replays; `--all` (and `full`'s legs) replay every golden.
const CHECK_GOLDEN: &str = "mw-castlestrike-8chaos";
const WASM_TARGET: &str = "wasm32-wasip1";
const ARM64_TARGET: &str = "aarch64-unknown-linux-gnu";
const ARM64_GCC: &str = "aarch64-linux-gnu-gcc";
const ARM64_SYSROOT: &str = "/usr/aarch64-linux-gnu";

/// Which legs run and on which replays.
#[derive(Clone, Copy, Debug, Default)]
pub struct Opts {
    pub wasm: bool,
    pub windows: bool,
    pub arm64: bool,
    /// Every golden match, not only [`CHECK_GOLDEN`].
    pub all: bool,
}

/// The flags of `olr-cli match` a golden's spec names (other fields are ignored).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code, reason = "named so that deny_unknown_fields accepts the golden's own fields")]
struct Golden {
    #[serde(default)]
    end: String,
    #[serde(default)]
    ticks: u32,
    #[serde(default)]
    checksums: Vec<(u32, u64)>,
    spec: Spec,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    mod_id: String,
    level_id: String,
    #[serde(default)]
    mode: Mode,
    bots: u32,
    difficulty: Difficulty,
    personality: Personality,
    seed: u64,
    ticks: u32,
    bonus_frequency: Option<f32>,
    kill_limit: i32,
    time_limit: f32,
    #[serde(default)]
    loading_time: Option<i32>,
    #[serde(default)]
    pool_capacity: Option<usize>,
}

/// `olr-cli`'s `Mode`: only Deathmatch exists; another mode fails parsing.
#[derive(Deserialize, Default)]
enum Mode {
    #[default]
    Deathmatch,
}

#[derive(Deserialize)]
enum Difficulty {
    Easy,
    Medium,
    Hard,
    Xtreme,
    Mixed,
}

#[derive(Deserialize)]
enum Personality {
    Deathmatch,
    Chaos,
}

impl Spec {
    /// The `olr-cli match` arguments that play this spec.
    fn flags(&self) -> Vec<String> {
        let difficulty = match self.difficulty {
            Difficulty::Easy => "easy",
            Difficulty::Medium => "medium",
            Difficulty::Hard => "hard",
            Difficulty::Xtreme => "xtreme",
            Difficulty::Mixed => "mixed",
        };
        let personality = match self.personality {
            Personality::Deathmatch => "dm",
            Personality::Chaos => "chaos",
        };
        let Mode::Deathmatch = self.mode;
        let mut f: Vec<String> = [
            ("--mod", self.mod_id.clone()),
            ("--level", self.level_id.clone()),
            ("--bots", self.bots.to_string()),
            ("--difficulty", difficulty.into()),
            ("--personality", personality.into()),
            ("--seed", self.seed.to_string()),
            ("--ticks", self.ticks.to_string()),
            ("--kill-limit", self.kill_limit.to_string()),
            ("--time-limit", self.time_limit.to_string()),
        ]
        .into_iter()
        .flat_map(|(k, v)| [k.to_string(), v])
        .collect();
        if let Some(b) = self.bonus_frequency {
            f.extend(["--bonus-frequency".into(), b.to_string()]);
        }
        if let Some(l) = self.loading_time {
            f.extend(["--loading-time".into(), l.to_string()]);
        }
        if let Some(p) = self.pool_capacity {
            f.extend(["--pool-capacity".into(), p.to_string()]);
        }
        f
    }
}

/// The first difference of two checksum streams (`<file> <tick> <checksum>`
/// lines), or a length difference; `other` names the compared platform.
pub fn compare(native: &str, other_stream: &str, other: &str) -> Result<(), String> {
    if native.trim().is_empty() {
        return Err("the native stream is empty".into());
    }
    let (mut a, mut b) = (native.lines().map(str::trim), other_stream.lines().map(str::trim));
    let mut n = 0usize;
    loop {
        match (a.next(), b.next()) {
            (None, None) => return Ok(()),
            (Some(x), Some(y)) if x == y => n += 1,
            (Some(x), Some(y)) => {
                let (fx, tx, cx) = fields(x);
                let (fy, ty, cy) = fields(y);
                return Err(if (fx, tx) == (fy, ty) {
                    format!("{fx} {tx}: native {cx}, {other} {cy}")
                } else {
                    format!("line {}: native `{x}`, {other} `{y}`", n + 1)
                });
            }
            (Some(x), None) => {
                return Err(format!("{other} stream ends after {n} lines; native continues with `{x}`"));
            }
            (None, Some(y)) => {
                return Err(format!("native stream ends after {n} lines; {other} continues with `{y}`"));
            }
        }
    }
}

fn fields(line: &str) -> (&str, &str, &str) {
    let mut p = line.split(' ');
    (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""))
}

fn tail(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn ok_output(mut c: Command, what: &str) -> Result<String, String> {
    let out = c.output().map_err(|e| format!("cannot run {what}: {e}"))?;
    if !out.status.success() {
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        return Err(format!("{what} failed ({}):\n{}", out.status, tail(&text, 40)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn tool_works(tool: &str) -> bool {
    Command::new(tool).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// Node.js's major version (`node --version` prints `v20.11.0`).
fn node_major() -> Option<u32> {
    let out = Command::new("node").arg("--version").output().ok().filter(|o| o.status.success())?;
    parse_node_major(&String::from_utf8_lossy(&out.stdout))
}

fn parse_node_major(version: &str) -> Option<u32> {
    version.trim().strip_prefix('v')?.split('.').next()?.parse().ok()
}

fn rustup_has(target: &str) -> bool {
    Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == target))
        .unwrap_or(false)
}

/// The golden specs to record, as `(stem, spec)`, by file name.
fn goldens(all: bool) -> Result<Vec<(String, Spec)>, String> {
    let dir = root().join(MATCHES);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ron"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        let stem = p.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        if !all && stem != CHECK_GOLDEN {
            continue;
        }
        let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let g: Golden = ron::from_str(&text)
            .with_context(|| format!("parsing {}", p.display()))
            .map_err(|e| format!("{e:#}"))?;
        out.push((stem, g.spec));
    }
    if out.is_empty() {
        return Err(format!("no golden match in {MATCHES}"));
    }
    Ok(out)
}

/// Records each golden's replay natively into `target/determinism/<stem>.olrp`.
fn record(goldens: &[(String, Spec)]) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(root().join(OUT)).map_err(|e| format!("{OUT}: {e}"))?;
    let mut build = cargo();
    build.args(["build", "--quiet", "--release", "-p", "olr-cli", "-p", "olr-detcheck"]);
    ok_output(build, "cargo build olr-cli and olr-detcheck")?;
    let mut files = Vec::new();
    for (stem, spec) in goldens {
        let file = PathBuf::from(OUT).join(format!("{stem}.olrp"));
        let mut c = Command::new(root().join("target/release/olr-cli"));
        c.current_dir(root()).arg("match").args(spec.flags()).args(["--no-verify", "--record"]).arg(&file);
        ok_output(c, &format!("olr-cli match ({stem})"))?;
        files.push(file);
    }
    Ok(files)
}

fn native_stream(replays: &[PathBuf]) -> Result<String, String> {
    let mut c = Command::new(root().join("target/release/olr-detcheck"));
    c.current_dir(root()).args(["--content", "content/packs", "--bake", BAKE]).args(replays);
    ok_output(c, "olr-detcheck (native)")
}

type Leg = fn(&str, &[PathBuf]) -> Result<(), String>;

fn skip(leg: &str, why: &str) -> Result<(), String> {
    println!("skip {leg} ({why})");
    Ok(())
}

fn wasm_missing() -> Option<String> {
    match node_major() {
        None => return Some("no node: `sudo apt install nodejs`".into()),
        Some(m) if m < 18 => {
            return Some(format!(
                "node {m} is too old for the WASI module (18 or newer): `sudo apt install nodejs`"
            ));
        }
        Some(_) => {}
    }
    (!rustup_has(WASM_TARGET))
        .then(|| format!("the Rust target is missing: `rustup target add {WASM_TARGET}`"))
}

fn wasm_leg(native: &str, replays: &[PathBuf]) -> Result<(), String> {
    if !tool_works("node") {
        return skip("determinism", "no node: `sudo apt install nodejs`");
    }
    if !rustup_has(WASM_TARGET) {
        return skip(
            "determinism",
            &format!("the Rust target is missing: `rustup target add {WASM_TARGET}`"),
        );
    }
    let mut build = cargo();
    build.args(["build", "--quiet", "--release", "-p", "olr-detcheck", "--target", WASM_TARGET]);
    ok_output(build, "cargo build olr-detcheck for WASI")?;
    let mut c = Command::new("node");
    c.current_dir(root())
        .arg("tools/wasi-run.mjs")
        .arg(format!("target/{WASM_TARGET}/release/olr-detcheck.wasm"))
        .arg(root())
        .args(["--content", "/content/packs"])
        .args(["--bake".to_string(), format!("/{BAKE}")])
        .args(replays.iter().map(|r| format!("/{}", r.display())));
    let wasm = ok_output(c, "olr-detcheck under Node.js WASI")?;
    compare(native, &wasm, "wasm").map_err(|e| format!("WASM differs from native: {e}"))
}

fn arm64_missing() -> Option<String> {
    let fix = "`sudo apt install qemu-user gcc-aarch64-linux-gnu` and `rustup target add aarch64-unknown-linux-gnu`";
    if !tool_works("qemu-aarch64") {
        return Some(format!("no qemu-aarch64: {fix}"));
    }
    if !tool_works(ARM64_GCC) {
        return Some(format!("no {ARM64_GCC}: {fix}"));
    }
    (!rustup_has(ARM64_TARGET)).then(|| format!("the Rust target is missing: {fix}"))
}

fn arm64_leg(native: &str, replays: &[PathBuf]) -> Result<(), String> {
    let mut build = cargo();
    build.args(["build", "--quiet", "--release", "-p", "olr-detcheck", "--target", ARM64_TARGET]);
    ok_output(build, "cargo build olr-detcheck for arm64")?;
    let mut c = Command::new("qemu-aarch64");
    c.current_dir(root())
        .args(["-L", ARM64_SYSROOT])
        .arg(format!("target/{ARM64_TARGET}/release/olr-detcheck"))
        .args(["--content", "content/packs", "--bake", BAKE])
        .args(replays);
    let arm = ok_output(c, "olr-detcheck under qemu-aarch64")?;
    compare(native, &arm, "arm64").map_err(|e| format!("arm64 differs from native: {e}"))
}

fn windows_leg(native: &str, replays: &[PathBuf]) -> Result<(), String> {
    let release = winbuild::build()?;
    let files: Vec<PathBuf> = replays.iter().map(|r| root().join(r)).collect();
    let stage = winbuild::stage(&release, &root().join("content/packs"), &files)?;
    let bake = stage.dir.join("bake");
    match std::fs::remove_dir_all(&bake) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("{}: {e}", bake.display())),
        _ => {}
    }
    winbuild::copy_dir(&root().join(BAKE), &bake.join("composed-level"))?;
    let mut args: Vec<String> =
        vec!["--content".into(), "content".into(), "--bake".into(), "bake/composed-level".into()];
    args.extend(
        files.iter().map(|f| format!("replays/{}", f.file_name().unwrap_or_default().to_string_lossy())),
    );
    let out = winbuild::run(&stage, "olr-detcheck.exe", &args, &[], winbuild::RUN_TIMEOUT)?;
    compare(native, &String::from_utf8_lossy(&out.stdout), "windows")
        .map_err(|e| format!("Windows differs from native: {e}"))
}

/// Runs the legs `opts` names; a leg whose tools are missing prints its
/// notice and is skipped before anything is recorded.
pub fn step(opts: Opts) -> Result<(), String> {
    let all = opts.all || opts.windows || opts.arm64;
    let mut legs: Vec<(&str, Leg)> = Vec::new();
    for (wanted, name, missing, leg) in [
        (opts.wasm, "determinism", wasm_missing as fn() -> Option<String>, wasm_leg as Leg),
        (opts.windows, "determinism-windows", winbuild::missing, windows_leg),
        (opts.arm64, "determinism-arm64", arm64_missing, arm64_leg),
    ] {
        if !wanted {
            continue;
        }
        match missing() {
            Some(why) => skip(name, &why)?,
            None => legs.push((name, leg)),
        }
    }
    if legs.is_empty() {
        return Ok(());
    }
    let replays = record(&goldens(all)?)?;
    let native = native_stream(&replays)?;
    for (name, leg) in legs {
        leg(&native, &replays)?;
        println!(
            "  {name}: streams equal native ({} lines, {} replays)",
            native.lines().count(),
            replays.len()
        );
    }
    Ok(())
}

pub fn command(mut opts: Opts) -> anyhow::Result<bool> {
    if !(opts.wasm || opts.windows || opts.arm64) {
        opts.wasm = true;
    }
    match step(opts) {
        Ok(()) => Ok(true),
        Err(why) => {
            println!("FAIL determinism\n{why}");
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "m.olrp 0 00000000000000aa\nm.olrp 10 00000000000000bb\nm.olrp 20 00000000000000cc\n";

    #[test]
    fn equal_streams_pass() {
        assert_eq!(compare(A, A, "wasm"), Ok(()));
        assert_eq!(compare(A, &A.replace('\n', "\r\n"), "windows"), Ok(()));
    }

    #[test]
    fn a_differing_bake_digest_is_named() {
        let (a, b) = (format!("{A}bake 00ff\n"), format!("{A}bake 00fe\n"));
        assert_eq!(compare(&a, &b, "wasm"), Err("line 4: native `bake 00ff`, wasm `bake 00fe`".into()));
    }

    #[test]
    fn a_differing_checksum_names_file_and_tick() {
        let b = A.replace("bb", "bd");
        assert_eq!(
            compare(A, &b, "wasm"),
            Err("m.olrp 10: native 00000000000000bb, wasm 00000000000000bd".into())
        );
    }

    #[test]
    fn an_empty_native_stream_is_refused() {
        assert_eq!(compare("", "", "wasm"), Err("the native stream is empty".into()));
        assert!(compare("\n", A, "wasm").is_err());
    }

    #[test]
    fn node_versions_parse() {
        assert_eq!(parse_node_major("v20.11.0\n"), Some(20));
        assert_eq!(parse_node_major("v8.1.0"), Some(8));
        assert_eq!(parse_node_major("nonsense"), None);
    }

    #[test]
    fn every_golden_becomes_its_match_flags() {
        let want = [
            (
                "classic-dirt-level-8dm",
                "--mod olx-classic --level olx-level-dirt-level --bots 8 --difficulty mixed --personality dm \
                 --seed 1 --ticks 3000 --kill-limit 15 --time-limit 6 --bonus-frequency 5",
            ),
            (
                "classic-fossilfacility-4xtreme-kill-limit",
                "--mod olx-classic --level olx-level-fossilfacility --bots 4 --difficulty xtreme --personality dm \
                 --seed 3 --ticks 12000 --kill-limit 2 --time-limit 6",
            ),
            (
                "mw-castlestrike-8chaos",
                "--mod olx-mw-1-0 --level olx-level-castlestrike --bots 8 --difficulty mixed --personality chaos \
                 --seed 2 --ticks 3000 --kill-limit 0 --time-limit 6 --bonus-frequency 5",
            ),
        ];
        let found = goldens(true).unwrap();
        assert_eq!(found.len(), want.len());
        for ((stem, spec), (name, flags)) in found.iter().zip(want) {
            assert_eq!(stem, name);
            assert_eq!(spec.flags().join(" "), flags, "{stem}");
        }
    }

    #[test]
    fn a_new_spec_field_is_an_error() {
        let text = "(spec: (mod_id: \"m\", level_id: \"l\", bots: 4, difficulty: Mixed, personality: Chaos, \
                    seed: 3, ticks: 9, kill_limit: 2, time_limit: 6.0, flux: 1), end: \"x\")";
        assert!(ron::from_str::<Golden>(text).is_err());
    }

    #[test]
    fn loading_time_and_pool_pass_through() {
        let g: Golden = ron::from_str(
            "(spec: (mod_id: \"m\", level_id: \"l\", bots: 4, difficulty: Mixed, personality: Chaos, \
             seed: 3, ticks: 9, kill_limit: 2, time_limit: 6.0, loading_time: Some(0), pool_capacity: Some(99)))",
        )
        .unwrap();
        assert!(g.spec.flags().join(" ").ends_with("--loading-time 0 --pool-capacity 99"));
    }

    #[test]
    fn a_missing_tail_is_reported() {
        let short = "m.olrp 0 00000000000000aa\n";
        assert!(compare(A, short, "arm64").unwrap_err().starts_with("arm64 stream ends after 1 lines"));
        assert!(compare(short, A, "arm64").unwrap_err().starts_with("native stream ends after 1 lines"));
    }

    #[test]
    fn goldens_become_match_flags() {
        let g: Golden = ron::from_str(
            "(spec: (mod_id: \"m\", level_id: \"l\", mode: Deathmatch, bots: 4, difficulty: Xtreme, \
             personality: Chaos, seed: 3, ticks: 9, bonus_frequency: Some(5.0), kill_limit: 2, \
             time_limit: 6.0), end: \"x\")",
        )
        .unwrap();
        let f = g.spec.flags().join(" ");
        assert!(f.contains("--difficulty xtreme --personality chaos --seed 3 --ticks 9"), "{f}");
        assert!(f.ends_with("--bonus-frequency 5"), "{f}");
    }
}
