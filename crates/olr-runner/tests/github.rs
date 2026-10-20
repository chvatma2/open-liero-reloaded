use olr_runner::github::{parse_issue, parse_issues, parse_milestones};
use olr_runner::settings::Settings;

const ISSUE: &str = include_str!("data/issue.json");

#[test]
fn parse_issue_reads_everything() {
    let d = parse_issue(ISSUE).unwrap();
    assert_eq!(d.number, 7);
    assert_eq!(d.title, "Rope snaps on long swings");
    assert_eq!(d.body, "The rope breaks when I swing far.");
    assert_eq!(d.author, "reporter");
    assert_eq!(d.editor.as_deref(), Some("chvatma2"));
    assert_eq!(d.last_edited_at.as_deref(), Some("2026-10-20T10:00:00Z"));
    assert_eq!(d.labels, ["approved", "bug"]);
    assert_eq!(d.comments.len(), 2);
    assert_eq!(d.comments[0].author, "chvatma2");
    assert_eq!(d.comments[0].body, "Keep the rope length cap.");
    assert_eq!(d.comments[1].created_at, "2026-10-20T10:06:00Z");
    assert_eq!(d.label_events.len(), 3);
    assert!(d.label_events[0].added && d.label_events[0].label == "bug");
    assert!(!d.label_events[1].added);
    assert_eq!(d.label_events[2].label, "approved");
    assert_eq!(d.label_events[2].actor, "chvatma2");
    assert_eq!(d.label_events[2].at, "2026-10-20T10:10:00Z");
    assert_eq!(d.renames.len(), 1);
    assert_eq!(d.renames[0].actor, "chvatma2");
}

#[test]
fn parse_issue_without_edits() {
    let json = ISSUE
        .replace(r#""editor": { "login": "chvatma2" }"#, r#""editor": null"#)
        .replace(r#""lastEditedAt": "2026-10-20T10:00:00Z""#, r#""lastEditedAt": null"#);
    let d = parse_issue(&json).unwrap();
    assert_eq!(d.editor, None);
    assert_eq!(d.last_edited_at, None);
}

#[test]
fn parse_issues_lists_labels() {
    let issues = parse_issues(include_str!("data/issues.json")).unwrap();
    assert_eq!(issues.len(), 2, "pull requests are skipped");
    assert_eq!(issues[0].number, 7);
    assert_eq!(issues[1].labels, ["approved", "in-progress"]);
}

#[test]
fn parse_milestones_open_only() {
    let ms = parse_milestones(include_str!("data/milestones.json")).unwrap();
    assert_eq!(ms.len(), 1);
    assert_eq!((ms[0].number, ms[0].title.as_str()), (3, "0.2.0"));
}

#[test]
fn settings_default_values() {
    let s = Settings::default();
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
    assert_eq!(s.repo, "chvatma2/open-liero-reloaded");
    assert_eq!(s.maintainer, "chvatma2");
    assert_eq!(s.remote_url, "https://github.com/chvatma2/open-liero-reloaded.git");
    assert_eq!((s.idle_secs, s.ceiling_secs, s.limit_fallback_secs), (2700, 28800, 1800));
    assert_eq!(
        s.protected,
        ["crates/olr-runner/**", ".github/**", ".claude/**", "fixtures/oracle/*.jsonl.gz"]
    );
    assert_eq!(s.claude_bin, "claude");
    assert_eq!(s.check_cmd, ["cargo", "xtask", "check"]);
    assert_eq!(s.render_check_cmd, ["cargo", "xtask", "check", "--render"]);
    assert_eq!(s.workspace, home.join(".local/share/olr-runner/work"));
    assert_eq!(s.state_dir, home.join(".local/state/olr-runner"));
    assert_eq!(s.claude_config_dir, home.join(".config/olr-runner/claude"));
}
