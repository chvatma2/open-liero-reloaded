//! Field paths through a mod's files (issue #16): `mod.worm.gravity`,
//! `weapons/bazooka.fire.shot.speed`,
//! `projectiles/bazooka.spawn.hit_worm[0].damage`,
//! `restrictions["Standard 100lt"]["Big Nuke"]`.
//!
//! A path is a file, `mod`, `restrictions`, `weapons/<id>` or
//! `projectiles/<id>` (an id may hold dots: the longest id the mod has
//! wins), then steps: `.name` (a field, a map key or a tuple variant's
//! `.0`), `[i]` (a list item) or `["key"]` (any map key). A step is
//! printed `.name` when the name is plain (`[A-Za-z0-9_-]`), else
//! `["name"]`.

use std::fmt;

use olr_format::mods::ModSource;

/// A file of a mod.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum File {
    /// `mod.ron`.
    Mod,
    /// `restrictions.ron`.
    Restrictions,
    /// `weapons/<id>.ron`.
    Weapon(String),
    /// `projectiles/<id>.ron`, a shared projectile.
    Shared(String),
}

impl File {
    /// The file's path in the pack, such as `weapons/bazooka.ron`.
    pub fn pack_path(&self) -> String {
        format!("{self}.ron")
    }

    /// The file a pack path names (`mod.ron`, `weapons/<id>.ron`, …), if any.
    pub fn of_pack_path(path: &str) -> Option<File> {
        let stem = path.strip_suffix(".ron")?;
        Some(match stem {
            "mod" => File::Mod,
            "restrictions" => File::Restrictions,
            _ => match stem.split_once('/')? {
                ("weapons", id) if !id.is_empty() => File::Weapon(id.into()),
                ("projectiles", id) if !id.is_empty() => File::Shared(id.into()),
                _ => return None,
            },
        })
    }
}

impl fmt::Display for File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            File::Mod => f.write_str("mod"),
            File::Restrictions => f.write_str("restrictions"),
            File::Weapon(id) => write!(f, "weapons/{id}"),
            File::Shared(id) => write!(f, "projectiles/{id}"),
        }
    }
}

/// One step of a path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Seg {
    /// A field, a map key or a tuple variant's field (`0`).
    Name(String),
    /// A list item.
    Index(usize),
}

fn plain(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl fmt::Display for Seg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Seg::Name(n) if plain(n) => write!(f, ".{n}"),
            Seg::Name(n) => write!(f, "[{n:?}]"),
            Seg::Index(i) => write!(f, "[{i}]"),
        }
    }
}

/// A file and the steps inside it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FieldPath {
    pub file: File,
    pub segs: Vec<Seg>,
}

impl FieldPath {
    pub fn new(file: File, segs: Vec<Seg>) -> Self {
        FieldPath { file, segs }
    }

    /// This path, one step further.
    pub fn join(&self, seg: Seg) -> FieldPath {
        let mut segs = self.segs.clone();
        segs.push(seg);
        FieldPath { file: self.file.clone(), segs }
    }
}

impl fmt::Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.file)?;
        self.segs.iter().try_for_each(|s| write!(f, "{s}"))
    }
}

/// The steps of `text`, which starts with `.`, `[` or is empty; a leading
/// bare name is taken as `.name` (`olr-format`'s error paths, such as
/// `fire.shot.projectile.spawns["spawn"]`, read too).
pub fn parse_segs(text: &str) -> Result<Vec<Seg>, String> {
    let bad = |why: &str| Err(format!("{text:?}: {why}"));
    let b = text.as_bytes();
    let mut segs = Vec::new();
    let mut i = 0;
    let name_end =
        |from: usize| b[from..].iter().position(|&c| c == b'.' || c == b'[').map_or(b.len(), |p| from + p);
    if !text.is_empty() && b[0] != b'.' && b[0] != b'[' {
        let end = name_end(0);
        segs.push(Seg::Name(text[..end].to_string()));
        i = end;
    }
    while i < b.len() {
        match b[i] {
            b'.' => {
                let end = name_end(i + 1);
                if end == i + 1 {
                    return bad("an empty step");
                }
                segs.push(Seg::Name(text[i + 1..end].to_string()));
                i = end;
            }
            b'[' if b.get(i + 1) == Some(&b'"') => {
                let mut name = String::new();
                let mut j = i + 2;
                let mut chars = text[j..].char_indices();
                loop {
                    match chars.next() {
                        None => return bad("an unclosed string"),
                        Some((k, '"')) => {
                            j += k + 1;
                            break;
                        }
                        Some((_, '\\')) => match chars.next() {
                            Some((_, c @ ('"' | '\\'))) => name.push(c),
                            _ => return bad("only \\\" and \\\\ escape in a key"),
                        },
                        Some((_, c)) => name.push(c),
                    }
                }
                if b.get(j) != Some(&b']') {
                    return bad("a key needs its closing ]");
                }
                segs.push(Seg::Name(name));
                i = j + 1;
            }
            b'[' => {
                let Some(close) = b[i..].iter().position(|&c| c == b']') else { return bad("an unclosed [") };
                let digits = &text[i + 1..i + close];
                match digits.parse::<usize>() {
                    Ok(n) if digits.bytes().all(|c| c.is_ascii_digit()) => segs.push(Seg::Index(n)),
                    _ => return bad("an index is [0], [1], …; a key is [\"key\"]"),
                }
                i += close + 1;
            }
            _ => return bad("a step starts with . or ["),
        }
    }
    Ok(segs)
}

/// The id among `ids` that `rest` starts with, followed by its end, `.`
/// or `[`; the longest wins.
fn longest_id<'a>(rest: &str, ids: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    ids.filter(|id| {
        rest.strip_prefix(*id)
            .is_some_and(|after| after.is_empty() || after.starts_with('.') || after.starts_with('['))
    })
    .max_by_key(|id| id.len())
}

/// `text` as a path of `src` (weapon and projectile ids are those it has).
pub fn parse(src: &ModSource, text: &str) -> Result<FieldPath, String> {
    let file_then = |name: &str| {
        text.strip_prefix(name).filter(|r| r.is_empty() || r.starts_with('.') || r.starts_with('['))
    };
    let (file, rest) = if let Some(rest) = file_then("mod") {
        (File::Mod, rest)
    } else if let Some(rest) = file_then("restrictions") {
        (File::Restrictions, rest)
    } else if let Some(rest) = text.strip_prefix("weapons/") {
        let id = longest_id(rest, src.weapons.iter().map(|(id, _)| id.as_str()))
            .ok_or_else(|| format!("{text:?}: no such weapon (the ids are mod.weapons)"))?;
        (File::Weapon(id.into()), &rest[id.len()..])
    } else if let Some(rest) = text.strip_prefix("projectiles/") {
        let id = longest_id(rest, src.shared.keys().map(String::as_str))
            .ok_or_else(|| format!("{text:?}: no such shared projectile"))?;
        (File::Shared(id.into()), &rest[id.len()..])
    } else {
        return Err(format!(
            "{text:?}: a path starts with mod, restrictions, weapons/<id> or projectiles/<id>"
        ));
    };
    Ok(FieldPath { file, segs: parse_segs(rest)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_parse_and_print() {
        let segs = parse_segs(r#"fire.shot.projectile.spawns["spawn"].x[3]["Big \"N\""].0"#).unwrap();
        let n = |s: &str| Seg::Name(s.into());
        assert_eq!(
            segs,
            [
                n("fire"),
                n("shot"),
                n("projectile"),
                n("spawns"),
                n("spawn"),
                n("x"),
                Seg::Index(3),
                n("Big \"N\""),
                n("0")
            ]
        );
        let printed: String = segs.iter().map(|s| s.to_string()).collect();
        assert_eq!(printed, r#".fire.shot.projectile.spawns.spawn.x[3]["Big \"N\""].0"#);
        for bad in ["..x", ".x[", ".x[a]", ".x[\"a]", ".x[\"a\"", ".x[-1]", ".x[+1]"] {
            assert!(parse_segs(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn pack_paths_name_files() {
        assert_eq!(File::of_pack_path("weapons/a.b.ron"), Some(File::Weapon("a.b".into())));
        assert_eq!(File::of_pack_path("mod.ron"), Some(File::Mod));
        assert_eq!(File::of_pack_path("projectiles/"), None);
        assert_eq!(File::Shared("x.spawn".into()).pack_path(), "projectiles/x.spawn.ron");
    }
}
