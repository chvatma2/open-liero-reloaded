//! Names players see: which characters a name may hold and how to clean
//! one. The one rule for olr-proto (lobby names are refused when invalid)
//! and olr-replay (a decoded name is cleaned), so they cannot drift.

use alloc::string::String;

/// At most `max` characters, no control characters.
pub fn valid_text(s: &str, max: usize) -> bool {
    s.chars().count() <= max && !s.chars().any(char::is_control)
}

/// Bidi overrides, isolates and marks.
pub fn is_bidi(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Unicode format (Cf) and separator code points that make names
/// ambiguous: bidi controls, zero-width characters, line and paragraph
/// separators, the BOM.
pub fn is_hidden(c: char) -> bool {
    is_bidi(c)
        || matches!(
            c,
            '\u{00AD}' | '\u{200B}'..='\u{200D}' | '\u{2028}' | '\u{2029}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}'
        )
}

/// A name: 1 to `max` characters, no control, hidden or bidi characters,
/// not blank, no leading or trailing whitespace.
pub fn valid_name(s: &str, max: usize) -> bool {
    valid_text(s, max) && !s.is_empty() && s.trim() == s && !s.chars().any(is_hidden)
}

/// `s` made a [`valid_name`] of at most `max` characters: control and hidden
/// characters dropped, trimmed, cut, trimmed again. Empty when nothing is
/// left (the caller names the worm or player then).
pub fn sanitise_name(s: &str, max: usize) -> String {
    let kept: String = s.chars().filter(|&c| !c.is_control() && !is_hidden(c)).collect();
    let cut: String = kept.trim().chars().take(max).collect();
    cut.trim().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sanitised_name_is_a_valid_name_or_empty() {
        for raw in [
            "Ann",
            "  padded  ",
            "tab\tand\nnewline",
            "bidi\u{202E}override",
            "zero\u{200B}width\u{FEFF}",
            "\u{00AD}soft\u{2028}hyphen",
            "twenty-one characters!!",
            "ééééééééééééééééééééééééé",
            "\u{1}\u{2}",
            "abcdefghijklmnopqrs tuv",
            "",
        ] {
            let clean = sanitise_name(raw, 20);
            assert!(clean.is_empty() || valid_name(&clean, 20), "{raw:?} -> {clean:?}");
        }
        assert_eq!(sanitise_name("abcdefghijklmnopqrs tuv", 20), "abcdefghijklmnopqrs");
        assert!(!valid_name("a\u{200B}b", 20) && !valid_name(" a", 20) && !valid_name("", 20));
    }
}
