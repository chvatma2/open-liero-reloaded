use olr_runner::limit::{self, Reset, seconds_until};
use olr_runner::result::{self, Status};
use olr_runner::stream::{Line, scan};

#[test]
fn result_done() {
    let r = result::parse("\n done \nAdded the cap.\n\nNew crates: none\n").unwrap();
    assert_eq!(r.status, Status::Done);
    assert_eq!(r.text, "Added the cap.\n\nNew crates: none");
}

#[test]
fn result_blocked() {
    let r = result::parse("BLOCKED\nThe issue contradicts the oracle.").unwrap();
    assert_eq!(r.status, Status::Blocked);
    assert_eq!(r.text, "The issue contradicts the oracle.");
}

#[test]
fn result_garbage_is_error() {
    assert!(result::parse("I think it's done").is_err());
    assert!(result::parse("").is_err());
}

#[test]
fn scan_result_line() {
    let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"All done.","session_id":"x"}"#;
    assert_eq!(scan(line), Line::Result { is_error: false, text: "All done.".into() });
    let err = r#"{"type":"result","is_error":true,"result":"You've hit your session limit · resets 3:45pm"}"#;
    assert_eq!(scan(err), Line::Result { is_error: true, text: "You've hit your session limit · resets 3:45pm".into() });
}

#[test]
fn scan_other_lines_are_activity() {
    assert_eq!(scan(r#"{"type":"assistant","message":{}}"#), Line::Activity);
    assert_eq!(scan(r#"{"type":"system","subtype":"init"}"#), Line::Activity);
}

#[test]
fn scan_non_json_is_activity() {
    assert_eq!(scan("warning: something"), Line::Activity);
    assert_eq!(scan(""), Line::Activity);
}

#[test]
fn limit_pm() {
    assert_eq!(limit::parse("You've hit your session limit · resets 3:45pm"), Some(Reset::At { hour: 15, minute: 45 }));
    assert_eq!(limit::parse("You've hit your Opus limit · resets 3pm (Europe/Prague)"), Some(Reset::At {
        hour: 15,
        minute: 0
    }));
    assert_eq!(limit::parse("You've hit your session limit · resets 15:45"), Some(Reset::At { hour: 15, minute: 45 }));
}

#[test]
fn limit_12am() {
    assert_eq!(limit::parse("You've hit your session limit · resets 12am"), Some(Reset::At { hour: 0, minute: 0 }));
    assert_eq!(limit::parse("You've hit your session limit · resets 12:30pm"), Some(Reset::At {
        hour: 12,
        minute: 30
    }));
}

#[test]
fn limit_weekly_unknown() {
    assert_eq!(limit::parse("You've hit your weekly limit · resets Mon 12:00am"), Some(Reset::Unknown));
    assert_eq!(limit::parse("You've hit your usage limit"), Some(Reset::Unknown));
}

#[test]
fn no_limit_text() {
    assert_eq!(limit::parse("Implemented the rope cap; all tests pass."), None);
    assert_eq!(limit::parse("the speed limit of the rope resets each frame"), None);
}

#[test]
fn seconds_until_later_today() {
    assert_eq!(seconds_until((14, 0, 0), (15, 45)), 105 * 60 + 60);
}

#[test]
fn seconds_until_wraps() {
    assert_eq!(seconds_until((23, 30, 0), (0, 15)), 45 * 60 + 60);
    assert_eq!(seconds_until((15, 45, 0), (15, 45)), 24 * 3600 + 60);
}
