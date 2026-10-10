//! Case-insensitive lookup, needed because OLX content names files inconsistently
//! (`GFX/`, `Gfx/`, `SCRIPT.LGS`) and OLX itself matches case-insensitively.

use crate::{ContentSource, VPath};

/// Resolves `path` component by component, ignoring ASCII case. Exact matches win;
/// among case-insensitive matches the first in sorted order wins, as on OLX.
pub fn find_ci(src: &dyn ContentSource, path: &VPath) -> Option<VPath> {
    let mut resolved = VPath::root();
    let comps: Vec<&str> = path.components().collect();
    for (i, want) in comps.iter().enumerate() {
        let last = i + 1 == comps.len();
        let entries = src.list(&resolved).ok()?;
        let pick = |exact: bool| {
            entries.iter().find(|e| {
                let (name, is_dir) = match e.strip_suffix('/') {
                    Some(n) => (n, true),
                    None => (e.as_str(), false),
                };
                (is_dir != last) && if exact { name == *want } else { name.eq_ignore_ascii_case(want) }
            })
        };
        let entry = pick(true).or_else(|| pick(false))?;
        resolved = resolved.join(entry.trim_end_matches('/')).ok()?;
    }
    Some(resolved)
}

/// The first of `candidates` that [`find_ci`] resolves.
pub fn find_ci_any(src: &dyn ContentSource, candidates: &[VPath]) -> Option<VPath> {
    candidates.iter().find_map(|c| find_ci(src, c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemFs;

    #[test]
    fn resolves_mixed_case() {
        let fs = MemFs::new().with("Classic/GFX/Missile.png", "m").with("Classic/script.lgs", "s");
        let p = find_ci(&fs, &VPath::new("classic/gfx/missile.PNG").unwrap()).unwrap();
        assert_eq!(p.as_str(), "Classic/GFX/Missile.png");
        assert!(find_ci(&fs, &VPath::new("classic/gfx").unwrap()).is_none(), "directory is not a file");
        assert!(find_ci(&fs, &VPath::new("classic/nope.png").unwrap()).is_none());
    }

    #[test]
    fn exact_match_wins() {
        let fs = MemFs::new().with("a/X.png", "upper").with("a/x.png", "lower");
        assert_eq!(find_ci(&fs, &VPath::new("a/x.png").unwrap()).unwrap().as_str(), "a/x.png");
    }

    #[test]
    fn any_takes_first_hit() {
        let fs = MemFs::new().with("data/gfx/a.png", "d");
        let c = [VPath::new("mod/gfx/a.png").unwrap(), VPath::new("data/gfx/A.png").unwrap()];
        assert_eq!(find_ci_any(&fs, &c).unwrap().as_str(), "data/gfx/a.png");
    }
}
