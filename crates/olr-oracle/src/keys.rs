//! Key masks shared with the OLX hooks (`OracleHooks.h`: KEY_UP = 1 ...
//! KEY_CHANGE = 128, KEY_STRAFE = 256, KEY_PREV = 512, KEY_NEXT = 1024).

use std::fmt;

/// The keyboard actions a scenario can hold down during a frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Keys(pub u16);

const NAMES: [(&str, u16); 11] = [
    ("up", 1),
    ("down", 2),
    ("left", 4),
    ("right", 8),
    ("jump", 16),
    ("fire", 32),
    ("rope", 64),
    ("change", 128),
    ("strafe", 256),
    ("prev", 512),
    ("next", 1024),
];

impl Keys {
    pub const NONE: Keys = Keys(0);

    /// Parses `"right+jump"`; the empty string means no keys.
    pub fn parse(s: &str) -> Result<Keys, String> {
        let mut mask = 0;
        for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            let bit = NAMES.iter().find(|(n, _)| *n == part).map(|(_, b)| *b).ok_or_else(|| {
                format!(
                    "unknown key {part:?} (expected one of up, down, left, right, jump, fire, rope, change, strafe, prev, next)"
                )
            })?;
            mask |= bit;
        }
        Ok(Keys(mask))
    }

    pub fn contains(self, name: &str) -> bool {
        NAMES.iter().any(|(n, b)| *n == name && self.0 & b != 0)
    }
}

impl fmt::Display for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = NAMES.iter().filter(|(_, b)| self.0 & b != 0).map(|(n, _)| *n).collect();
        f.write_str(&names.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        assert_eq!(Keys::parse("").unwrap(), Keys::NONE);
        assert_eq!(Keys::parse("right+jump").unwrap(), Keys(8 | 16));
        assert_eq!(Keys::parse(" up + change ").unwrap(), Keys(1 | 128));
        assert_eq!(Keys(8 | 16).to_string(), "right+jump");
        assert!(Keys(64).contains("rope"));
        assert_eq!(Keys::parse("left+strafe+prev+next").unwrap(), Keys(4 | 256 | 512 | 1024));
        assert_eq!(Keys(256 | 1024).to_string(), "strafe+next");
    }

    #[test]
    fn unknown_key_is_an_error() {
        let err = Keys::parse("right+jmup").unwrap_err();
        assert!(err.contains("\"jmup\""), "{err}");
    }
}
