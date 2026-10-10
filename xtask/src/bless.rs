//! `cargo xtask bless-render --reason <text>`: re-generates the committed
//! render goldens in `fixtures/render/` and records why in
//! `fixtures/blessings.log` (spec §7.2), with the date and the files that
//! changed. Each golden test writes its PNGs when `RL_BLESS_RENDER` is set.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail, ensure};

use crate::root;

/// The golden tests as `(package, test target)`.
const GOLDEN_TESTS: &[(&str, &str)] = &[("rl-scenario", "render_golden")];
const GOLDEN_DIR: &str = "fixtures/render";
const LOG: &str = "fixtures/blessings.log";

/// The files in `dir` by name, with their bytes.
fn snapshot(dir: &Path) -> anyhow::Result<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(out) };
    for e in entries {
        let e = e?;
        if e.file_type()?.is_file() {
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

pub fn run(reason: &str) -> anyhow::Result<bool> {
    let reason = reason.trim();
    ensure!(!reason.is_empty(), "--reason must say why the goldens change");
    let root = root();
    let content = std::env::var_os("RL_CONTENT_DIR").map_or_else(|| root.join("target/content"), Into::into);
    if !content.is_dir() {
        bail!("{} not found: run `cargo xtask fetch-olx-content` first", content.display());
    }
    let dir = root.join(GOLDEN_DIR);
    let before = snapshot(&dir)?;
    for (package, test) in GOLDEN_TESTS {
        let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["test", "--quiet", "-p", package, "--test", test])
            .current_dir(&root)
            .env("RL_BLESS_RENDER", "1")
            .env("RL_CONTENT_DIR", &content)
            .output()
            .context("cannot run cargo")?;
        if !out.status.success() {
            eprintln!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            bail!("{package} --test {test} failed while blessing");
        }
    }
    let after = snapshot(&dir)?;
    let changed: Vec<String> = before
        .keys()
        .chain(after.keys())
        .filter(|f| before.get(*f) != after.get(*f))
        .map(|f| format!("{GOLDEN_DIR}/{f}"))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if changed.is_empty() {
        println!("the render goldens are unchanged: nothing to bless");
        return Ok(true);
    }
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut entry = format!("{} bless-render: {}\n", date(secs), reason.replace('\n', " "));
    for f in &changed {
        entry.push_str(&format!("  {f}\n"));
    }
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(root.join(LOG))?;
    log.write_all(entry.as_bytes())?;
    print!("blessed, recorded in {LOG}:\n{entry}");
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_are_civil_utc() {
        assert_eq!(super::date(0), "1970-01-01");
        assert_eq!(super::date(951_782_400), "2000-02-29");
        assert_eq!(super::date(1_791_590_399), "2026-10-09");
    }
}
