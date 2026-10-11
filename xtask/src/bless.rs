//! `cargo xtask bless-render --reason <text>`: re-generates the committed
//! render goldens in `fixtures/render/` and the HUD snapshots in
//! `fixtures/ui/`, and records why in `fixtures/blessings.log` (spec §7.2),
//! with the date and the files that changed. Each golden test writes its
//! PNGs when `RL_BLESS_RENDER` is set; the GPU goldens (`rl-cli render`
//! frames, `rl-ui`'s HUD snapshots) need a graphics adapter.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail, ensure};

use crate::root;

/// The golden tests as `(package, test target, needs a graphics adapter)`:
/// the colour terrain (CPU), `rl-cli render`'s frames and the HUD's image
/// snapshots (GPU).
const GOLDEN_TESTS: &[(&str, &str, bool)] = &[
    ("rl-scenario", "render_golden", false),
    ("rl-cli", "render_golden", true),
    ("rl-ui", "hud_snapshots", true),
];
/// Where they write.
const GOLDEN_DIRS: &[&str] = &["fixtures/render", "fixtures/ui"];
const LOG: &str = "fixtures/blessings.log";

/// Files a golden test leaves beside the goldens and git ignores
/// (`egui_kittest`'s last image, diff and backup).
fn by_product(name: &str) -> bool {
    [".new.png", ".diff.png", ".old.png"].iter().any(|s| name.ends_with(s))
}

/// The files in `dir` by name, with their bytes (by-products left out).
pub fn snapshot(dir: &Path) -> anyhow::Result<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(out) };
    for e in entries {
        let e = e?;
        if e.file_type()?.is_file() && !by_product(&e.file_name().to_string_lossy()) {
            out.insert(e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path())?);
        }
    }
    Ok(out)
}

/// The UTC date of `secs` since the Unix epoch, as `YYYY-MM-DD`
/// (Howard Hinnant's `civil_from_days`).
pub fn date(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Appends `what: reason` with today's date and the `changed` files to
/// `fixtures/blessings.log` (shared by `bless-render` and `bless-regression`).
pub fn log_blessing(what: &str, reason: &str, changed: &[String]) -> anyhow::Result<()> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let entry = log_entry(&date(secs), what, reason, changed);
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(root().join(LOG))?;
    log.write_all(entry.as_bytes())?;
    print!("blessed, recorded in {LOG}:\n{entry}");
    Ok(())
}

/// One entry of the log: the date, the command and the reason on one
/// line, then each changed file indented.
pub fn log_entry(date: &str, what: &str, reason: &str, changed: &[String]) -> String {
    let mut entry = format!("{date} {what}: {}\n", reason.trim().replace('\n', " "));
    for f in changed {
        entry.push_str(&format!("  {f}\n"));
    }
    entry
}

/// The golden tests to run: all of them with an adapter, else only the CPU ones.
fn tests_to_run(adapter: bool) -> Vec<(&'static str, &'static str)> {
    GOLDEN_TESTS.iter().filter(|(_, _, gpu)| adapter || !gpu).map(|&(p, t, _)| (p, t)).collect()
}

/// The files that differ between two snapshots of `dir` (a repository path),
/// as repository paths.
pub fn changed_files(
    dir: &str,
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|f| before.get(*f) != after.get(*f))
        .map(|f| format!("{dir}/{f}"))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn run(reason: &str) -> anyhow::Result<bool> {
    let reason = reason.trim();
    ensure!(!reason.is_empty(), "--reason must say why the goldens change");
    let root = root();
    let content = std::env::var_os("RL_CONTENT_DIR").map_or_else(|| root.join("content/packs"), Into::into);
    if !content.is_dir() {
        bail!("{} not found: run `cargo xtask fetch-olx-content` first", content.display());
    }
    let snap_all = || -> anyhow::Result<Vec<BTreeMap<String, Vec<u8>>>> {
        GOLDEN_DIRS.iter().map(|d| snapshot(&root.join(d))).collect()
    };
    let before = snap_all()?;
    let adapter = match crate::check::probe_adapter().map_err(anyhow::Error::msg)? {
        Ok(said) => {
            println!("gpu adapter: {said}");
            true
        }
        Err(said) => {
            println!("skip the GPU goldens ({said}); only the CPU goldens are re-drawn");
            false
        }
    };
    // A failing test must not leave a changed golden unlogged: record what
    // changed first, then fail.
    let mut failure = None;
    for (package, test) in tests_to_run(adapter) {
        let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["test", "--quiet", "-p", package, "--test", test])
            .current_dir(&root)
            .env("RL_BLESS_RENDER", "1")
            .env("RL_GPU_TESTS", "1")
            .env("RL_CONTENT_DIR", &content)
            .output()
            .context("cannot run cargo")?;
        if !out.status.success() {
            eprintln!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            failure = Some(format!("{package} --test {test} failed while blessing"));
            break;
        }
    }
    let after = snap_all()?;
    let changed: Vec<String> = GOLDEN_DIRS
        .iter()
        .zip(before.iter().zip(&after))
        .flat_map(|(d, (b, a))| changed_files(d, b, a))
        .collect();
    if changed.is_empty() {
        if let Some(f) = failure {
            bail!(f);
        }
        println!("the render goldens are unchanged: nothing to bless");
        return Ok(true);
    }
    log_blessing("bless-render", reason, &changed)?;
    if !adapter {
        println!("the GPU goldens were not re-drawn (no adapter): bless them on a machine with one");
    }
    if let Some(f) = failure {
        bail!(f);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    #[test]
    fn without_an_adapter_only_the_cpu_goldens_run() {
        assert_eq!(super::tests_to_run(false), [("rl-scenario", "render_golden")]);
        assert_eq!(super::tests_to_run(true).len(), 3);
    }

    #[test]
    fn snapshot_by_products_are_no_goldens() {
        assert!(super::by_product("hud-1080p.new.png") && super::by_product("hud-1080p.diff.png"));
        assert!(!super::by_product("hud-1080p.png"));
    }

    #[test]
    fn changed_files_lists_added_removed_and_edited_goldens() {
        let snap = |v: &[(&str, &str)]| -> BTreeMap<String, Vec<u8>> {
            v.iter().map(|(k, b)| (k.to_string(), b.as_bytes().to_vec())).collect()
        };
        let before = snap(&[("a.png", "1"), ("b.png", "1"), ("c.png", "1")]);
        let after = snap(&[("a.png", "1"), ("b.png", "2"), ("d.png", "1")]);
        assert_eq!(
            super::changed_files("fixtures/render", &before, &after),
            ["fixtures/render/b.png", "fixtures/render/c.png", "fixtures/render/d.png"]
        );
    }

    #[test]
    fn log_entries_put_the_reason_on_one_line() {
        let e = super::log_entry("2026-10-11", "bless-regression", " bots aim\nbetter ", &["a.ron".into()]);
        assert_eq!(e, "2026-10-11 bless-regression: bots aim better\n  a.ron\n");
    }

    #[test]
    fn dates_are_civil_utc() {
        assert_eq!(super::date(0), "1970-01-01");
        assert_eq!(super::date(951_782_400), "2000-02-29");
        assert_eq!(super::date(1_791_590_399), "2026-10-09");
    }
}
