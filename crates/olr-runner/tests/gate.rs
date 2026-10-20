use std::path::Path;

use olr_runner::gate::{
    bad_subjects, claude_md_edits, glob_match, new_packages, protected_hits, touches_render,
};
use olr_runner::report::{Done, HandBack, done_comment, handback_comment};
use olr_runner::trust::MARKER;

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn subjects_need_issue_suffix() {
    let subjects =
        strings(&["olr-sim: cap the rope (#7)", "docs: rope (#77)", "fixup", "olr-ui: label (#7) "]);
    assert_eq!(bad_subjects(&subjects, 7), strings(&["docs: rope (#77)", "fixup"]));
}

#[test]
fn glob_double_star() {
    assert!(glob_match("crates/olr-runner/**", "crates/olr-runner/src/a.rs"));
    assert!(glob_match("crates/olr-runner/**", "crates/olr-runner/Cargo.toml"));
    assert!(!glob_match("crates/olr-runner/**", "crates/olr-runner2/x"));
    assert!(glob_match(".github/**", ".github/ISSUE_TEMPLATE/bug.yml"));
}

#[test]
fn glob_single_star() {
    assert!(glob_match("fixtures/oracle/*.jsonl.gz", "fixtures/oracle/a.jsonl.gz"));
    assert!(!glob_match("fixtures/oracle/*.jsonl.gz", "fixtures/oracle/a/b.jsonl.gz"));
    assert!(!glob_match("fixtures/oracle/*.jsonl.gz", "fixtures/oracle/a.ron"));
}

#[test]
fn protected_hits_listed() {
    let patterns = strings(&["crates/olr-runner/**", ".github/**", "fixtures/oracle/*.jsonl.gz"]);
    let files =
        strings(&["crates/olr-sim/src/rope.rs", ".github/workflows/x.yml", "fixtures/oracle/r.jsonl.gz"]);
    assert_eq!(
        protected_hits(&files, &patterns),
        strings(&[".github/workflows/x.yml", "fixtures/oracle/r.jsonl.gz"])
    );
}

#[test]
fn render_paths_detected() {
    assert!(touches_render(&strings(&["crates/olr-sim/src/a.rs", "crates/olr-render/src/gi.rs"])));
    assert!(touches_render(&strings(&["fixtures/ui/hud.png"])));
    assert!(touches_render(&strings(&["crates/olr-sim/src/a.rs"])), "the goldens replay the simulation");
    assert!(!touches_render(&strings(&["crates/olr-bots/src/a.rs", "docs/x.md"])));
    assert_eq!(
        claude_md_edits(&strings(&["CLAUDE.md", "crates/olr-sim/CLAUDE.md", "README.md"])),
        strings(&["CLAUDE.md", "crates/olr-sim/CLAUDE.md"])
    );
}

#[test]
fn new_packages_from_locks() {
    let old = "[[package]]\nname = \"anyhow\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.1\"\n";
    let new = format!("{old}\n[[package]]\nname = \"rand\"\nversion = \"0.9.0\"\n");
    let bumped = old.replace("1.0.1", "1.0.2");
    assert_eq!(new_packages(old, &new), strings(&["rand 0.9.0"]));
    assert_eq!(new_packages(old, &bumped), strings(&["serde 1.0.2"]));
    assert!(new_packages(old, old).is_empty());
}

#[test]
fn done_comment_has_revert_and_marker() {
    let d = Done {
        summary: "Capped the rope.".into(),
        first: "aaaaaaa".into(),
        last: "bbbbbbb".into(),
        commits: 2,
        claude_md: strings(&["crates/olr-sim/CLAUDE.md"]),
        new_packages: strings(&["rand 0.9.0"]),
        deferred: true,
        render_unverified: false,
    };
    let c = done_comment(&d);
    assert!(c.starts_with(MARKER));
    for needle in [
        "Capped the rope.",
        "aaaaaaa..bbbbbbb",
        "git revert --no-edit aaaaaaa^..bbbbbbb",
        "crates/olr-sim/CLAUDE.md",
        "rand 0.9.0",
        "docs/deferred-verification.md",
    ] {
        assert!(c.contains(needle), "done comment lacks {needle:?}:\n{c}");
    }
    let plain = done_comment(&Done { claude_md: vec![], new_packages: vec![], deferred: false, ..d });
    assert!(!plain.contains("CLAUDE.md") && !plain.contains("deferred-verification"));
}

#[test]
fn handback_comment_has_reason_log_and_retry() {
    let c =
        handback_comment(&HandBack::Protected(strings(&[".github/x.yml"])), Path::new("/s/runs/7-1.jsonl"));
    assert!(c.starts_with(MARKER));
    for needle in [
        "Handed back to the maintainer",
        ".github/x.yml",
        "/s/runs/7-1.jsonl",
        "needs-maintainer",
        "approved",
    ] {
        assert!(c.contains(needle), "hand-back comment lacks {needle:?}:\n{c}");
    }
}

#[test]
fn gate_sandbox_masks_credentials() {
    let home = tempfile::tempdir().unwrap();
    for d in [".config/gh", ".ssh", ".config/olr-runner", ".cargo"] {
        std::fs::create_dir_all(home.path().join(d)).unwrap();
    }
    std::fs::write(home.path().join(".git-credentials"), "x").unwrap();
    let ws = home.path().join("work");
    let cmd = olr_runner::gate::sandboxed(&strings(&["cargo", "xtask", "check"]), &ws, home.path());
    let joined = cmd.join(" ");
    assert_eq!(cmd[0], "bwrap");
    assert!(joined.contains("--ro-bind / /"));
    let h = home.path().display();
    for masked in [
        format!("--tmpfs {h}/.config/gh"),
        format!("--tmpfs {h}/.ssh"),
        format!("--tmpfs {h}/.config/olr-runner"),
    ] {
        assert!(joined.contains(&masked), "{masked} in {joined}");
    }
    assert!(joined.contains(&format!("--ro-bind /dev/null {h}/.git-credentials")));
    assert!(joined.contains(&format!("--bind {} {}", ws.display(), ws.display())));
    assert!(joined.ends_with("-- nice -n 19 ionice -c 3 cargo xtask check"), "{joined}");
}
