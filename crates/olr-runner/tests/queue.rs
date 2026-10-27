use olr_runner::github::{IssueRef, Milestone};
use olr_runner::queue::{Pick, eligible, pick};

fn ms(titles: &[&str]) -> Vec<Milestone> {
    titles.iter().enumerate().map(|(i, t)| Milestone { number: i as u64 + 1, title: t.to_string() }).collect()
}

fn issue(number: u64, labels: &[&str], at: Option<&str>) -> (IssueRef, Option<String>) {
    let labels = labels.iter().map(|l| l.to_string()).collect();
    (IssueRef { number, title: format!("#{number}"), labels }, at.map(str::to_string))
}

#[test]
fn no_milestone() {
    assert_eq!(pick(&[], &[issue(1, &["approved"], Some("t"))], &[]), Pick::NoMilestone);
}

#[test]
fn two_milestones_does_nothing() {
    let p = pick(&ms(&["0.2.0", "0.3.0"]), &[issue(1, &["approved"], Some("t"))], &[]);
    assert_eq!(p, Pick::ManyMilestones(vec!["0.2.0".into(), "0.3.0".into()]));
}

#[test]
fn oldest_approval_first() {
    let candidates = [
        issue(5, &["approved"], Some("2026-10-20T12:00:00Z")),
        issue(9, &["approved"], None),
        issue(8, &["approved"], Some("2026-10-20T09:00:00Z")),
        issue(3, &["approved"], Some("2026-10-20T12:00:00Z")),
    ];
    assert_eq!(pick(&ms(&["0.2.0"]), &candidates, &[]), Pick::Next { milestone: "0.2.0".into(), number: 8 });
    let rest = &candidates[..2];
    assert_eq!(pick(&ms(&["0.2.0"]), rest, &[]), Pick::Next { milestone: "0.2.0".into(), number: 5 });
    assert_eq!(
        pick(&ms(&["0.2.0"]), &[candidates[3].clone(), candidates[0].clone()], &[]),
        Pick::Next { milestone: "0.2.0".into(), number: 3 }
    );
}

#[test]
fn skips_in_progress_and_needs_maintainer() {
    assert!(eligible(&["approved".into(), "bug".into()]));
    assert!(!eligible(&["approved".into(), "in-progress".into()]));
    assert!(!eligible(&["approved".into(), "needs-maintainer".into()]));
    assert!(!eligible(&["bug".into()]));
    let candidates = [
        issue(1, &["approved", "in-progress"], Some("a")),
        issue(2, &["approved", "needs-maintainer"], Some("b")),
    ];
    assert_eq!(pick(&ms(&["0.2.0"]), &candidates, &[]), Pick::Empty { milestone: "0.2.0".into() });
}

#[test]
fn empty_queue() {
    assert_eq!(pick(&ms(&["0.2.0"]), &[], &[]), Pick::Empty { milestone: "0.2.0".into() });
}

#[test]
fn an_older_handed_back_issue_holds_the_queue() {
    let m = || "0.3.0".to_string();
    // #12 was approved first, so it is next unless an older issue is handed back.
    let candidates = [
        issue(12, &["approved"], Some("2026-10-20T09:00:00Z")),
        issue(9, &["approved"], Some("2026-10-20T10:00:00Z")),
    ];
    assert_eq!(pick(&ms(&["0.3.0"]), &candidates, &[13]), Pick::Next { milestone: m(), number: 12 });
    assert_eq!(pick(&ms(&["0.3.0"]), &candidates, &[13, 10]), Pick::Held { milestone: m(), number: 10 });
    assert_eq!(pick(&ms(&["0.3.0"]), &candidates, &[11, 8]), Pick::Held { milestone: m(), number: 8 });
    assert_eq!(pick(&ms(&["0.3.0"]), &candidates[1..], &[10]), Pick::Next { milestone: m(), number: 9 });
    assert_eq!(pick(&ms(&["0.3.0"]), &[], &[10]), Pick::Held { milestone: m(), number: 10 });
}
