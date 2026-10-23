use std::path::{Path, PathBuf};
use std::process::Command;

use olr_runner::repo::Repo;

/// Runs git in `dir` with a fixed identity, panicking on failure.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A bare origin with one commit on `main` (`README` = "one"), and a
/// scratch clone of it for pushing more commits. Returns (tempdir, origin
/// URL, scratch clone).
pub fn origin() -> (tempfile::TempDir, String, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("origin.git");
    std::fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    let url = format!("file://{}", bare.display());
    let scratch = tmp.path().join("scratch");
    git(tmp.path(), &["clone", "-q", &url, "scratch"]);
    git(&scratch, &["checkout", "-q", "-b", "main"]);
    commit(&scratch, "README", "one", "init");
    git(&scratch, &["push", "-q", "origin", "main"]);
    (tmp, url, scratch)
}

/// Writes `file` and commits it with `subject`.
pub fn commit(dir: &Path, file: &str, text: &str, subject: &str) {
    let path = dir.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", subject]);
}

#[test]
fn prepare_clones_then_resets() {
    let (tmp, url, _) = origin();
    let work = tmp.path().join("work");
    let repo = Repo::prepare(&work, &url).unwrap();
    assert_eq!(std::fs::read_to_string(work.join("README")).unwrap(), "one");
    commit(&work, "local", "x", "local commit");
    std::fs::write(work.join("stray"), "x").unwrap();
    std::fs::create_dir_all(work.join("target")).unwrap();
    std::fs::write(work.join("target/keep"), "x").unwrap();
    let again = Repo::prepare(&work, &url).unwrap();
    assert!(!work.join("local").exists() && !work.join("stray").exists());
    assert!(work.join("target/keep").exists(), "the Cargo target is kept");
    assert_eq!(again.head().unwrap(), repo.origin_main().unwrap());
    assert!(again.is_clean().unwrap());
}

#[test]
fn subjects_and_files_between() {
    let (tmp, url, _) = origin();
    let repo = Repo::prepare(&tmp.path().join("work"), &url).unwrap();
    let base = repo.head().unwrap();
    commit(&repo.dir, "a/x.rs", "1", "first (#7)");
    commit(&repo.dir, "b.md", "2", "second (#7)");
    let head = repo.head().unwrap();
    assert_eq!(repo.subjects(&base, &head).unwrap(), ["first (#7)", "second (#7)"]);
    assert_eq!(repo.changed_files(&base, &head).unwrap(), ["a/x.rs", "b.md"]);
    assert_eq!(repo.show(&head, "b.md").unwrap(), "2");
    assert_eq!(repo.show(&base, "b.md").unwrap(), "");
    std::fs::write(repo.dir.join("b.md"), "dirty").unwrap();
    std::fs::create_dir(repo.dir.join("c")).unwrap();
    std::fs::write(repo.dir.join("c/new.txt"), "").unwrap();
    assert!(!repo.is_clean().unwrap());
    assert_eq!(repo.uncommitted().unwrap(), [" M b.md", "?? c/"]);
}

#[test]
fn rebase_clean_when_origin_moved() {
    let (tmp, url, scratch) = origin();
    let repo = Repo::prepare(&tmp.path().join("work"), &url).unwrap();
    commit(&repo.dir, "mine", "m", "mine (#7)");
    commit(&scratch, "theirs", "t", "theirs");
    git(&scratch, &["push", "-q", "origin", "main"]);
    repo.fetch().unwrap();
    assert!(repo.rebase_onto_origin_main().unwrap());
    assert!(repo.dir.join("theirs").exists() && repo.dir.join("mine").exists());
}

#[test]
fn rebase_conflict_aborts() {
    let (tmp, url, scratch) = origin();
    let repo = Repo::prepare(&tmp.path().join("work"), &url).unwrap();
    commit(&repo.dir, "README", "mine", "mine (#7)");
    let mine = repo.head().unwrap();
    commit(&scratch, "README", "theirs", "theirs");
    git(&scratch, &["push", "-q", "origin", "main"]);
    repo.fetch().unwrap();
    assert!(!repo.rebase_onto_origin_main().unwrap());
    assert_eq!(repo.head().unwrap(), mine, "the rebase was aborted");
    assert!(repo.is_clean().unwrap());
}

#[test]
fn push_updates_origin_main() {
    let (tmp, url, scratch) = origin();
    let repo = Repo::prepare(&tmp.path().join("work"), &url).unwrap();
    commit(&repo.dir, "new", "n", "new (#7)");
    repo.push_main().unwrap();
    git(&scratch, &["pull", "-q", "origin", "main"]);
    assert_eq!(std::fs::read_to_string(scratch.join("new")).unwrap(), "n");
    let (ok, out) = repo.run(&["sh".into(), "-c".into(), "echo hi; exit 3".into()]).unwrap();
    assert!(!ok && out.contains("hi"));
}

#[test]
fn renames_list_both_paths() {
    let (tmp, url, _) = origin();
    let repo = Repo::prepare(&tmp.path().join("work"), &url).unwrap();
    let base = repo.head().unwrap();
    git(&repo.dir, &["mv", "README", "MOVED"]);
    git(&repo.dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "mv (#7)"]);
    assert_eq!(repo.changed_files(&base, &repo.head().unwrap()).unwrap(), ["MOVED", "README"]);
}

#[test]
fn is_ancestor_of_head() {
    let (tmp, url, _) = origin();
    let repo = Repo::prepare(&tmp.path().join("work"), &url).unwrap();
    let base = repo.head().unwrap();
    commit(&repo.dir, "a", "1", "a (#7)");
    assert!(repo.is_ancestor(&base).unwrap());
    git(&repo.dir, &["reset", "-q", "--hard", &base]);
    git(
        &repo.dir,
        &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--amend", "-m", "rewritten"],
    );
    assert!(!repo.is_ancestor(&base).unwrap(), "base itself was rewritten");
}
