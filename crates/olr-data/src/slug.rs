//! Canonical identifiers for pack ids and file names.

use alloc::string::String;

/// Lowercases ASCII letters and digits, maps every other run of characters to
/// a single `-`, and trims `-` from both ends. Returns `"unnamed"` if nothing remains.
///
/// `slug("MW 1.0")` is `"mw-1-0"`; `slug("Liero Factory")` is `"liero-factory"`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() { String::from("unnamed") } else { out }
}

#[cfg(test)]
mod tests {
    use super::slug;

    #[test]
    fn examples() {
        assert_eq!(slug("MW 1.0"), "mw-1-0");
        assert_eq!(slug("Liero Factory"), "liero-factory");
        assert_eq!(slug("  --Bazooka!!"), "bazooka");
        assert_eq!(slug("Amy'sLion"), "amy-slion");
        assert_eq!(slug("ŽŽŽ"), "unnamed");
        assert_eq!(slug(""), "unnamed");
    }
}
