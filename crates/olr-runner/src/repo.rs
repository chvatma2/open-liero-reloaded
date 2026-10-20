//! The runner's own clone of the repository: reset to `origin/main` before
//! every run, checked and pushed after it. The user's checkout is never
//! touched.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Lines of a command's output kept for a report.
const TAIL_LINES: usize = 200;

pub struct Repo {
    pub dir: PathBuf,
}

fn git_in(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(dir).output().context("cannot run git")?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

fn lines(text: &str) -> Vec<String> {
    text.lines().filter(|l| !l.is_empty()).map(str::to_string).collect()
}

impl Repo {
    /// Clones `remote_url` into `dir` if needed, then resets it to a clean,
    /// detached `origin/main`, keeping `target/`.
    pub fn prepare(dir: &Path, remote_url: &str) -> Result<Repo> {
        if !dir.join(".git").exists() {
            std::fs::create_dir_all(dir)?;
            git_in(dir, &["clone", "-q", remote_url, "."])?;
        }
        let repo = Repo { dir: dir.to_path_buf() };
        repo.git(&["rebase", "--abort"]).ok();
        repo.fetch()?;
        repo.git(&["checkout", "-q", "--force", "--detach", "origin/main"])?;
        repo.git(&["reset", "-q", "--hard", "origin/main"])?;
        repo.git(&["clean", "-q", "-ffdx", "-e", "target"])?;
        Ok(repo)
    }

    fn git(&self, args: &[&str]) -> Result<String> {
        git_in(&self.dir, args)
    }

    pub fn head(&self) -> Result<String> {
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn origin_main(&self) -> Result<String> {
        self.git(&["rev-parse", "origin/main"])
    }

    /// Commit subjects in `base..head`, oldest first.
    pub fn subjects(&self, base: &str, head: &str) -> Result<Vec<String>> {
        Ok(lines(&self.git(&["log", "--reverse", "--format=%s", &format!("{base}..{head}")])?))
    }

    /// Commit ids in `base..head`, oldest first.
    pub fn commit_ids(&self, base: &str, head: &str) -> Result<Vec<String>> {
        Ok(lines(&self.git(&["rev-list", "--reverse", &format!("{base}..{head}")])?))
    }

    pub fn changed_files(&self, base: &str, head: &str) -> Result<Vec<String>> {
        // Without renames, a file moved out of a protected path lists both paths.
        Ok(lines(&self.git(&["diff", "--name-only", "--no-renames", base, head])?))
    }

    /// Whether `base` is an ancestor of `HEAD` (the session kept the history
    /// it started on).
    pub fn is_ancestor(&self, base: &str) -> Result<bool> {
        let status = Command::new("git")
            .args(["merge-base", "--is-ancestor", base, "HEAD"])
            .current_dir(&self.dir)
            .status()
            .context("cannot run git")?;
        Ok(status.success())
    }

    /// No changes and no untracked files outside `target/`.
    pub fn is_clean(&self) -> Result<bool> {
        Ok(self.git(&["status", "--porcelain", "--", ".", ":(exclude)target"])?.is_empty())
    }

    /// `path` at `rev`, empty if it does not exist there.
    pub fn show(&self, rev: &str, path: &str) -> Result<String> {
        Ok(self.git(&["show", &format!("{rev}:{path}")]).unwrap_or_default())
    }

    pub fn fetch(&self) -> Result<()> {
        self.git(&["fetch", "-q", "origin"]).map(drop)
    }

    /// Rebases onto `origin/main`; on a conflict, aborts and returns `false`.
    pub fn rebase_onto_origin_main(&self) -> Result<bool> {
        if self.git(&["rebase", "-q", "origin/main"]).is_ok() {
            return Ok(true);
        }
        self.git(&["rebase", "--abort"])?;
        Ok(false)
    }

    pub fn push_main(&self) -> Result<()> {
        self.git(&["push", "-q", "origin", "HEAD:main"]).map(drop)
    }

    /// Runs `cmd` in the clone: whether it succeeded, and the last lines of
    /// its output (stdout and stderr interleaved).
    pub fn run(&self, cmd: &[String]) -> Result<(bool, String)> {
        let out = Command::new("sh")
            .arg("-c")
            .arg("\"$@\" 2>&1")
            .arg("sh")
            .args(cmd)
            .current_dir(&self.dir)
            .output()
            .with_context(|| format!("cannot run {cmd:?}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let all: Vec<&str> = text.lines().collect();
        let tail = all[all.len().saturating_sub(TAIL_LINES)..].join("\n");
        Ok((out.status.success(), tail))
    }
}
