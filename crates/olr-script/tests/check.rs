//! `check` (issue #80): good scripts pass, a syntax error is one issue at
//! its line and column, and hostile text is cheap and never panics.

use std::time::{Duration, Instant};

use olr_script::{MAX_SCRIPT_BYTES, ScriptIssue, check};

#[test]
fn a_good_script_has_no_issues() {
    let text = "// A lamp that blinks.\n\
                let on = true;\n\
                fn toggle(x) { !x }\n\
                if on { on = toggle(on); } else { on = true; }\n\
                let n = 0;\n\
                while n < 10 { n += 1; }\n\
                let s = \"alarm\";\n";
    assert_eq!(check(text), Vec::<ScriptIssue>::new());
    assert_eq!(check(""), Vec::<ScriptIssue>::new());
}

/// The one issue of `text`.
fn issue(text: &str) -> ScriptIssue {
    let mut issues = check(text);
    assert_eq!(issues.len(), 1, "{text:?}: {issues:?}");
    issues.remove(0)
}

#[test]
fn syntax_errors_name_line_and_column() {
    // A missing `}`: at the end of the text.
    let i = issue("fn f() {\n  let x = 1;\n");
    assert_eq!(i.line, 3, "{i}");
    assert!(i.message.contains('}'), "{i}");
    // A bad token on line 2.
    let i = issue("let a = 1;\nlet b = 2 $ 3;\n");
    assert_eq!((i.line, i.column), (2, 11), "{i}");
    // An unterminated string on line 3, where it starts.
    let i = issue("let a = 1;\n\nlet s = \"never closed;\n");
    assert_eq!(i.line, 3, "{i}");
    assert_eq!(i.column, 9, "{i}");
    assert!(i.to_string().starts_with("3:9: "), "{i}");
}

/// Checks `text` and how long it took (loose: a busy machine may be slow).
fn cheap(text: &str) -> ScriptIssue {
    let t = Instant::now();
    let i = issue(text);
    assert!(t.elapsed() < Duration::from_secs(2), "{} bytes took {:?}", text.len(), t.elapsed());
    i
}

#[test]
fn hostile_scripts_are_cheap_and_never_panic() {
    // Deep nesting.
    let deep = format!("let x = {}1{};", "(".repeat(5000), ")".repeat(5000));
    cheap(&deep);
    let blocks = format!("{}{}", "{".repeat(10_000), "}".repeat(10_000));
    cheap(&blocks);
    // A huge string literal.
    cheap(&format!("let s = \"{}\";", "a".repeat(MAX_SCRIPT_BYTES - 16)));
    // MAX_SCRIPT_BYTES of `(`, and one byte more.
    cheap(&"(".repeat(MAX_SCRIPT_BYTES));
    let i = cheap(&"(".repeat(MAX_SCRIPT_BYTES + 1));
    assert!(i.message.contains("bytes"), "{i}");
    // Text that is no Rhai at all.
    cheap(&"\u{0}\u{7f}é€".repeat(1000));
}
