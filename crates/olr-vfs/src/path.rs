use std::fmt;

use crate::VfsError;

/// A validated relative path: components separated by `/`, none of them empty,
/// `.` or `..`, no leading `/`, no backslashes and no NUL. The empty path is the root.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VPath(String);

impl VPath {
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Validates `s`. Use [`VPath::from_olx`] for OLX names that may contain `\`.
    pub fn new(s: &str) -> Result<Self, VfsError> {
        if s.is_empty() {
            return Ok(Self::root());
        }
        let bad = |reason| Err(VfsError::InvalidPath { path: s.to_string(), reason });
        if s.contains('\\') {
            return bad("contains a backslash");
        }
        if s.contains('\0') {
            return bad("contains NUL");
        }
        if s.starts_with('/') {
            return bad("is absolute");
        }
        for c in s.split('/') {
            match c {
                "" => return bad("has an empty component"),
                "." | ".." => return bad("has a . or .. component"),
                _ => {}
            }
        }
        Ok(Self(s.to_string()))
    }

    /// Converts a file name found inside OLX data: `\` becomes `/`, then the
    /// result is validated like [`VPath::new`].
    pub fn from_olx(s: &str) -> Result<Self, VfsError> {
        Self::new(&s.replace('\\', "/"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// `self/rel`, where `rel` is validated.
    pub fn join(&self, rel: &str) -> Result<Self, VfsError> {
        let rel = VPath::new(rel)?;
        if self.is_root() {
            Ok(rel)
        } else if rel.is_root() {
            Ok(self.clone())
        } else {
            Ok(Self(format!("{}/{}", self.0, rel.0)))
        }
    }

    /// The last component, or `""` for the root.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }

    /// Everything before the last component; the root for single-component paths.
    pub fn parent(&self) -> VPath {
        match self.0.rfind('/') {
            Some(i) => Self(self.0[..i].to_string()),
            None => Self::root(),
        }
    }

    /// The components of the path.
    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|c| !c.is_empty())
    }

    /// `self` with `prefix` removed, if `self` is inside `prefix`.
    pub fn strip_prefix(&self, prefix: &VPath) -> Option<VPath> {
        if prefix.is_root() {
            return Some(self.clone());
        }
        let rest = self.0.strip_prefix(&prefix.0)?;
        if rest.is_empty() { Some(Self::root()) } else { rest.strip_prefix('/').map(|r| Self(r.to_string())) }
    }
}

/// Why `name` (one path component) is not a portable file name, if it is
/// not: one that every supported file system stores as itself, below its
/// directory. Windows reads `:` as a drive or stream separator (`C:x`
/// leaves the directory), refuses `<>"|?*` and control characters, drops
/// a trailing dot or space, and opens a device for `CON`, `PRN`, `AUX`,
/// `NUL`, `COM1`-`COM9` and `LPT1`-`LPT9` (any case, any extension).
/// Files written from untrusted content (packs, downloads) must have
/// portable names.
pub fn non_portable(name: &str) -> Option<&'static str> {
    if name.contains(':') {
        return Some("contains a colon");
    }
    if name.contains(['<', '>', '"', '|', '?', '*']) {
        return Some("contains one of <>\"|?*");
    }
    if name.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}') {
        return Some("contains a control character");
    }
    if name.ends_with(['.', ' ']) {
        return Some("ends with a dot or a space");
    }
    let base = name.split('.').next().unwrap_or("").trim_end_matches(' ');
    let device = |prefix: &str| {
        base.len() == prefix.len() + 1
            && base.is_char_boundary(prefix.len())
            && base[..prefix.len()].eq_ignore_ascii_case(prefix)
            && matches!(&base[prefix.len()..], "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
    };
    let named =
        ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].iter().any(|d| base.eq_ignore_ascii_case(d));
    let superscript = ["COM", "LPT"].iter().any(|p| {
        base.len() > p.len()
            && base.is_char_boundary(p.len())
            && base[..p.len()].eq_ignore_ascii_case(p)
            && matches!(&base[p.len()..], "\u{b9}" | "\u{b2}" | "\u{b3}")
    });
    if named || device("COM") || device("LPT") || superscript {
        return Some("is a Windows device name");
    }
    None
}

impl fmt::Display for VPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_paths() {
        assert_eq!(VPath::new("gfx/missile.png").unwrap().as_str(), "gfx/missile.png");
        assert!(VPath::new("").unwrap().is_root());
        assert_eq!(VPath::new("MW 1.0/script.lgs").unwrap().file_name(), "script.lgs");
    }

    #[test]
    fn rejects_escapes() {
        for bad in ["../x", "a/../b", "/etc/passwd", "a//b", "a/", "./a", "a\\b", "a\0b"] {
            assert!(VPath::new(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn from_olx_converts_backslashes_but_still_rejects_escapes() {
        assert_eq!(VPath::from_olx("gfx\\a.png").unwrap().as_str(), "gfx/a.png");
        assert!(VPath::from_olx("..\\..\\boot.ini").is_err());
    }

    #[test]
    fn join_parent_strip() {
        let p = VPath::new("a/b").unwrap().join("c.txt").unwrap();
        assert_eq!(p.as_str(), "a/b/c.txt");
        assert_eq!(p.parent().as_str(), "a/b");
        assert_eq!(VPath::new("x").unwrap().parent(), VPath::root());
        assert_eq!(p.strip_prefix(&VPath::new("a").unwrap()).unwrap().as_str(), "b/c.txt");
        assert!(p.strip_prefix(&VPath::new("ab").unwrap()).is_none());
        assert!(VPath::root().join("../x").is_err());
    }

    #[test]
    fn portable_names() {
        for good in
            ["a", "skin.png", "MW 1.0", "x-y_z", "console.png", "nul-2", "com10", "lpt0.ron", "auxiliary"]
        {
            assert_eq!(non_portable(good), None, "{good:?}");
        }
        for bad in [
            "C:",
            "a:b",
            "a<b",
            "a>b",
            "a\"b",
            "a|b",
            "a?b",
            "a*b",
            "a\u{1}b",
            "a\u{7f}",
            "x.",
            "x ",
            "CON",
            "con",
            "Nul.png",
            "aux.txt",
            "prn.a.b",
            "COM1",
            "lpt9.ron",
            "com\u{b9}",
            "conin$",
            "NUL .txt",
        ] {
            assert!(non_portable(bad).is_some(), "{bad:?}");
        }
    }

    proptest::proptest! {
        /// No string that `VPath` accepts can start with `/` or contain a `..` component.
        #[test]
        fn accepted_paths_never_escape(s in ".{0,40}") {
            if let Ok(p) = VPath::new(&s) {
                proptest::prop_assert!(!p.as_str().starts_with('/'));
                proptest::prop_assert!(p.components().all(|c| c != ".." && c != "."));
            }
        }
    }
}
