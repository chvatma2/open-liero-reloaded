//! The one path between values and pack text. Canonical text is what
//! [`to_canonical`] writes: reading it and writing it again gives the same
//! bytes.

use olr_vfs::{ContentSink, ContentSource, VPath};
use ron::Options;
use ron::extensions::Extensions;
use ron::ser::PrettyConfig;
use serde::{Serialize, de::DeserializeOwned};

use crate::FormatError;

/// The longest authoring file read, in bytes (the largest import, MW 1.0's
/// `restrictions.ron`, is about 21 KB).
pub const MAX_RON_BYTES: u64 = 4 << 20;

/// RON nesting allowed while reading or writing. One projectile level is
/// about four RON levels, so this leaves room above
/// `crate::mods::MAX_INLINE_DEPTH` (32), and deeper text is an error
/// before it can exhaust the stack.
pub const RECURSION_LIMIT: usize = 256;

fn options() -> Options {
    Options::default().with_default_extension(Extensions::IMPLICIT_SOME).with_recursion_limit(RECURSION_LIMIT)
}

fn pretty() -> PrettyConfig {
    PrettyConfig::default()
        .indentor("    ".to_string())
        .struct_names(false)
        .extensions(Extensions::IMPLICIT_SOME)
}

/// `value` as canonical text, with a trailing newline. It fails only when
/// `value` nests deeper than [`RECURSION_LIMIT`].
pub fn to_canonical<T: Serialize>(file: &str, value: &T) -> Result<String, FormatError> {
    let mut s = options()
        .to_string_pretty(value, pretty())
        .map_err(|e| FormatError::Io { file: file.into(), message: e.to_string() })?;
    s.push('\n');
    Ok(s)
}

/// Parses `text` (canonical or not; a leading byte-order mark is skipped).
pub fn from_canonical<T: DeserializeOwned>(file: &str, text: &str) -> Result<T, FormatError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    options().from_str(text).map_err(|e| FormatError::Parse {
        file: file.into(),
        line: e.span.start.line,
        col: e.span.start.col,
        message: e.code.to_string(),
    })
}

#[allow(dead_code)] // used by mods::files (plan 13a Task 5)
pub(crate) fn read_text(src: &dyn ContentSource, file: &str) -> Result<String, FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    let path = VPath::new(file).map_err(|e| io(e.to_string()))?;
    let bytes = src.read_max(&path, MAX_RON_BYTES).map_err(|e| io(e.to_string()))?;
    String::from_utf8(bytes).map_err(|e| io(format!("not UTF-8: {e}")))
}

#[allow(dead_code)] // used by mods::files (plan 13a Task 5)
pub(crate) fn write_text(sink: &mut dyn ContentSink, file: &str, text: &str) -> Result<(), FormatError> {
    let io = |message: String| FormatError::Io { file: file.into(), message };
    let path = VPath::new(file).map_err(|e| io(e.to_string()))?;
    sink.write(&path, text.as_bytes()).map_err(|e| io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound: Option<String>,
        gravity: Option<i32>,
        size: (i32, i32),
    }

    #[test]
    fn options_are_implicit_and_no_header_is_written() {
        let s = Sample { sound: Some("sfx/a.ogg".into()), gravity: Some(0), size: (4, 4) };
        let text = to_canonical("t.ron", &s).unwrap();
        assert!(!text.contains("Some("), "{text}");
        assert!(!text.contains("#!["), "{text}");
        assert!(text.ends_with(")\n"), "{text}");
        assert_eq!(from_canonical::<Sample>("t.ron", &text).unwrap(), s);
    }

    #[test]
    fn parse_errors_name_the_file_line_and_column() {
        let err = from_canonical::<Sample>("weapons/x.ron", "(\n    gravity: \"no\",\n)").unwrap_err();
        match err {
            FormatError::Parse { file, line, .. } => {
                assert_eq!(file, "weapons/x.ron");
                assert_eq!(line, 2);
            }
            e => panic!("unexpected {e}"),
        }
    }

    #[test]
    fn a_byte_order_mark_and_crlf_read_like_plain_text() {
        let text = "\u{feff}(\r\n    gravity: 3,\r\n    size: (1, 2),\r\n)\r\n";
        let s: Sample = from_canonical("t.ron", text).unwrap();
        assert_eq!(s, Sample { sound: None, gravity: Some(3), size: (1, 2) });
    }

    #[test]
    fn unknown_fields_are_errors_when_the_type_denies_them() {
        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Strict {
            #[allow(dead_code)]
            a: i32,
        }
        assert!(from_canonical::<Strict>("t.ron", "(a: 1, b: 2)").is_err());
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let text = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
        assert!(from_canonical::<ron::Value>("t.ron", &text).is_err());
    }
}
