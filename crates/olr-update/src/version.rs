//! A build's version and which of two is newer (issue #4): semver
//! precedence, parsed tolerantly. `cargo xtask dist` names a work in
//! progress `<x.y.z>-dev.<commits>.<commit>[.dirty]` and a final release
//! `<x.y.z>`; the commit may be all digits with a leading zero (invalid
//! strict semver), so a `dev` build is ordered by its commit count alone,
//! the commit and `.dirty` ignored. So `0.2.0-dev.318.014560b` <
//! `0.2.0-dev.330.…` < `0.2.0` < `0.2.1-dev.…`.

use std::cmp::Ordering;

/// A version: `[v]<major>.<minor>.<patch>[-<pre-release>][+<build>]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The pre-release identifiers after `-` (none: a final release).
    pub pre: Vec<String>,
}

impl Version {
    /// `text` as a version (a leading `v` allowed, build metadata after `+`
    /// dropped), or `None`: a local build's `0.2.0-dev (local build)` is none.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let text = text.split_once('+').map_or(text, |(v, _)| v);
        let (core, pre) = match text.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (text, None),
        };
        let num = |n: &str| if numeric(n) { n.parse::<u64>().ok() } else { None };
        let parts: Vec<&str> = core.split('.').collect();
        let [major, minor, patch] = parts.as_slice() else { return None };
        let (major, minor, patch) = (num(major)?, num(minor)?, num(patch)?);
        let pre = match pre {
            None => Vec::new(),
            Some(p) => {
                let ids: Vec<String> = p.split('.').map(str::to_string).collect();
                let good = |id: &String| {
                    !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                };
                if !ids.iter().all(good) {
                    return None;
                }
                ids
            }
        };
        Some(Version { major, minor, patch, pre })
    }

    /// A pre-release (`-dev.…` or any other `-…`), not a final release.
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }

    /// A build `dist` made from uncommitted changes (`….dirty`): never
    /// published.
    pub fn is_dirty(&self) -> bool {
        self.pre.first().is_some_and(|p| p == "dev") && self.pre.last().is_some_and(|p| p == "dirty")
    }

    /// The final release this one leads to: `0.2.0` for `0.2.0-dev.5.abc`.
    pub fn base(&self) -> Version {
        Version { major: self.major, minor: self.minor, patch: self.patch, pre: Vec::new() }
    }

    /// The pre-release identifiers that order it: a `dev` build's `dev`
    /// and commit count, else all.
    fn ordered_pre(&self) -> &[String] {
        match self.pre.as_slice() {
            [dev, n, ..] if dev == "dev" && numeric(n) => &self.pre[..2],
            all => all,
        }
    }

    /// Semver precedence (the commit of a `dev` build and `.dirty`
    /// ignored): `Equal` for two builds of the same count.
    pub fn precedence(&self, other: &Version) -> Ordering {
        let core = (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch));
        if core != Ordering::Equal {
            return core;
        }
        let (a, b) = (self.ordered_pre(), other.ordered_pre());
        match (a.is_empty(), b.is_empty()) {
            (true, true) => return Ordering::Equal,
            // A final release is above its pre-releases.
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (false, false) => {}
        }
        for (x, y) in a.iter().zip(b) {
            let o = match (numeric(x), numeric(y)) {
                (true, true) => {
                    let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                    x.len().cmp(&y.len()).then_with(|| x.cmp(y))
                }
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => x.as_str().cmp(y.as_str()),
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        a.len().cmp(&b.len())
    }

    /// Strictly newer than `other` by [`Version::precedence`].
    pub fn is_newer_than(&self, other: &Version) -> bool {
        self.precedence(other) == Ordering::Greater
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

fn numeric(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("{s} parses"))
    }

    #[test]
    fn versions_parse_tolerantly() {
        assert_eq!(v("v0.1.0"), Version { major: 0, minor: 1, patch: 0, pre: vec![] });
        let dev = v("0.2.0-dev.318.0145601");
        assert_eq!(dev.pre, ["dev", "318", "0145601"]);
        assert!(dev.is_prerelease() && !dev.is_dirty());
        assert!(v("0.2.0-dev.3.abc1234.dirty").is_dirty());
        assert_eq!(v("1.2.3+build.5").to_string(), "1.2.3");
        assert_eq!(dev.to_string(), "0.2.0-dev.318.0145601");
        assert_eq!(dev.base(), v("0.2.0"));
        for bad in [
            "0.2.0-dev (local build)",
            "0.2",
            "0.2.0.1",
            "x.1.2",
            "0.2.0-",
            "0.2.0-dev..1",
            "",
            "v",
            "99999999999999999999.0.0",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn newer_is_semver_precedence_with_the_commit_ignored() {
        let order = [
            "0.1.0",
            "0.2.0-dev.297.ffffff0",
            "0.2.0-dev.318.014560b",
            "0.2.0-dev.330.0000001",
            "0.2.0-dev.1000.aaaaaaa",
            "0.2.0",
            "0.2.1-dev.1.abc",
            "0.2.1",
            "0.10.0",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
        ];
        for (i, a) in order.iter().enumerate() {
            for (j, b) in order.iter().enumerate() {
                assert_eq!(v(a).precedence(&v(b)), i.cmp(&j), "{a} vs {b}");
            }
        }
        // The commit and `.dirty` do not order.
        assert_eq!(v("0.2.0-dev.318.014560b").precedence(&v("0.2.0-dev.318.9999999.dirty")), Ordering::Equal);
        assert!(!v("0.2.0-dev.318.b").is_newer_than(&v("0.2.0-dev.318.a")));
    }
}
