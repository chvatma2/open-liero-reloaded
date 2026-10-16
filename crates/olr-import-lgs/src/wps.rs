//! Weapon restriction presets (`.wps`): `CWpnRest::loadList`
//! (`src/common/CWpnRest.cpp:183-213`) at commit `d1ad953f`.

use std::collections::BTreeMap;

use olr_data::restrictions::{Preset, WeaponState};
use olr_vfs::{ContentSource, VPath};

use crate::LgsError;

/// Where OLX keeps the presets (`cfg/presets/<mod dir>/`, then `cfg/presets/`).
const PRESETS_DIR: &str = "cfg/presets";

/// Parses a `.wps` file as `loadList` reads it: lines up to `\n`, split at
/// `,`; a line with at least two fields adds `field 0` (a non-empty name,
/// stored as is) with state `field 1` read as an int (`from_string<int>`)
/// taken `MOD 3`. Names compare ignoring ASCII case: the first spelling stays
/// and the last line's state wins. Bytes are Latin-1, like weapon names.
pub fn parse_wps(bytes: &[u8]) -> Preset {
    let mut out: Vec<(String, WeaponState)> = Vec::new();
    for line in bytes.split(|&b| b == b'\n') {
        let mut fields = line.split(|&b| b == b',');
        let (Some(name), Some(state)) = (fields.next(), fields.next()) else { continue };
        if name.is_empty() {
            continue;
        }
        let name: String = name.iter().map(|&b| b as char).collect();
        let state = WeaponState::from_olx(olx_int(state));
        match out.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&name)) {
            Some((_, s)) => *s = state,
            None => out.push((name, state)),
        }
    }
    out.into_iter().collect()
}

/// `from_string<int>` (`src/util/StringConv.h:44-48`): `istringstream >> int`
/// skips leading white space, reads an optional sign and decimal digits and
/// stops at anything else; no digits gives 0, overflow the nearest limit.
fn olx_int(s: &[u8]) -> i32 {
    let mut i = s.iter().position(|b| !b" \t\n\x0b\x0c\r".contains(b)).unwrap_or(s.len());
    let neg = match s.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let mut v: i64 = 0;
    let mut any = false;
    while let Some(d) = s.get(i).filter(|b| b.is_ascii_digit()) {
        v = (v * 10 + i64::from(d - b'0')).min(i64::from(i32::MAX) + 1);
        any = true;
        i += 1;
    }
    if !any {
        return 0;
    }
    let v = if neg { -v } else { v };
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// The presets OLX can load for the mod in directory `mod_dir` (its
/// `ModInfo::path`): every `cfg/presets/<mod dir>/*.wps`, then every
/// `cfg/presets/*.wps` of another name (`loadList` falls back to it), by
/// file name without `.wps`. Directories and file names match ignoring case.
pub fn import_presets(
    olx: &dyn ContentSource,
    mod_dir: &VPath,
) -> Result<BTreeMap<String, Preset>, LgsError> {
    let mut out: BTreeMap<String, Preset> = BTreeMap::new();
    let root = VPath::new(PRESETS_DIR)?;
    let dirs = [root.join(mod_dir.as_str())?, root];
    for dir in dirs {
        let Some(dir) = find_dir_ci(olx, &dir) else { continue };
        for name in olx.list(&dir)? {
            let b = name.as_bytes();
            // `list` marks directories with a trailing `/`.
            if b.len() < 4 || !b[b.len() - 4..].eq_ignore_ascii_case(b".wps") {
                continue;
            }
            let path = dir.join(&name)?;
            let stem = &name[..name.len() - 4];
            if out.keys().any(|k| k.eq_ignore_ascii_case(stem)) {
                continue;
            }
            out.insert(stem.to_string(), parse_wps(&olx.read(&path)?));
        }
    }
    Ok(out)
}

/// Directory `path`, matching each component ignoring ASCII case (exact
/// matches first), like [`olr_vfs::find_ci`] for files.
fn find_dir_ci(src: &dyn ContentSource, path: &VPath) -> Option<VPath> {
    let mut resolved = VPath::root();
    for want in path.components() {
        let entries = src.list(&resolved).ok()?;
        let dirs = || entries.iter().filter_map(|e| e.strip_suffix('/'));
        let name = dirs().find(|n| *n == want).or_else(|| dirs().find(|n| n.eq_ignore_ascii_case(want)))?;
        resolved = resolved.join(name).ok()?;
    }
    Some(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use WeaponState::{Banned, Bonus, Enabled};
    use olr_vfs::MemFs;

    #[test]
    fn parses_like_load_list() {
        let text = b"Rifle,2\r\nmini nuke,0\r\nMini Nuke,1\nNoState\n,0\n  Laser ,2\nBazooka,x\nUzi, -1\nZimm,7,9\nHuge,99999999999\n";
        let p = parse_wps(text);
        let got: Vec<(&str, WeaponState)> = p.iter().map(|(k, &v)| (k.as_str(), v)).collect();
        assert_eq!(
            got,
            [
                ("  Laser ", Banned),
                ("Bazooka", Enabled),
                ("Huge", Bonus),
                ("Rifle", Banned),
                ("Uzi", Banned),
                ("Zimm", Bonus),
                ("mini nuke", Bonus),
            ]
        );
    }

    #[test]
    fn olx_ints() {
        for (s, want) in [
            (&b"2\r"[..], 2),
            (b" +7x", 7),
            (b"-4", -4),
            (b"", 0),
            (b"-", 0),
            (b"9999999999", i32::MAX),
            (b"-9999999999", i32::MIN),
        ] {
            assert_eq!(olx_int(s), want, "{:?}", String::from_utf8_lossy(s));
        }
    }

    #[test]
    fn presets_of_the_mod_dir_shadow_shared_ones() {
        let fs = MemFs::new()
            .with("cfg/presets/classic/Standard 100lt.WPS", "Rifle,2\n")
            .with("cfg/presets/classic/Firebird.wps", "Rifle,0\n")
            .with("cfg/presets/Standard 100lt.wps", "Rifle,0\n")
            .with("cfg/presets/Shared.wps", "Zimm,1\n")
            .with("cfg/presets/notes.txt", "x")
            .with("cfg/presets/Other/Mine.wps", "Mine,2\n");
        let p = import_presets(&fs, &VPath::new("Classic").unwrap()).unwrap();
        let names: Vec<&str> = p.keys().map(String::as_str).collect();
        assert_eq!(names, ["Firebird", "Shared", "Standard 100lt"]);
        assert_eq!(p["Standard 100lt"]["Rifle"], Banned);
        // A mod without presets of its own, and a game without any.
        assert_eq!(import_presets(&fs, &VPath::new("MW 1.0").unwrap()).unwrap().len(), 2);
        assert!(import_presets(&MemFs::new(), &VPath::new("Classic").unwrap()).unwrap().is_empty());
    }

    proptest::proptest! {
        #[test]
        fn never_panics_on_arbitrary_bytes(bytes in proptest::collection::vec(proptest::num::u8::ANY, 0..512)) {
            let p = parse_wps(&bytes);
            // `addWeapon` skips empty names.
            for name in p.keys() {
                proptest::prop_assert!(!name.is_empty());
            }
        }
    }
}
