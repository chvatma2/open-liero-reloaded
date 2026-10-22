//! Issue #37: the surface material libraries, the built-in set in
//! `content/materials/` and the user's.

use std::path::{Path, PathBuf};

use olr_edit::material::{MAX_USER_MATERIALS, list_library, list_materials};
use olr_format::materials::{MaterialSource, material_text};

fn built_in_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/materials")
}

/// The design's basic set: every one reads cleanly, is canonical, heats,
/// and the folder holds nothing else but its README.
#[test]
fn the_built_in_set_loads_cleanly() {
    let dir = built_in_dir();
    let (list, warnings) = list_library(&dir, usize::MAX);
    assert_eq!(warnings, Vec::<String>::new());
    let mut ids: Vec<&str> = list.iter().map(|m| m.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["brick", "concrete", "dirt", "glass", "metal", "sand", "stone", "wood"]);
    for m in &list {
        let text = std::fs::read_to_string(dir.join(format!("{}.ron", m.id))).unwrap();
        assert_eq!(material_text(m).unwrap(), text, "{} is not canonical", m.id);
        assert!(m.heat.as_ref().is_some_and(|h| h.sensitivity > 0), "{} heats", m.id);
        assert!(!m.notes.is_empty(), "{} says what it is", m.id);
    }
    let stone = list.iter().find(|m| m.id == "stone").unwrap().heat.as_ref().unwrap();
    assert_eq!((stone.capacity, stone.conductivity, stone.cooling), (100, 100, 100), "stone is the reference");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|f| !f.starts_with('.'))
        .collect();
    files.sort();
    let mut want: Vec<String> = ids.iter().map(|id| format!("{id}.ron")).collect();
    want.push("README.md".into());
    want.sort();
    assert_eq!(files, want);
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

fn mine(id: &str, name: &str) -> String {
    let m = MaterialSource { id: id.into(), name: name.into(), notes: String::new(), heat: None };
    material_text(&m).unwrap()
}

/// The user's materials come after the built-in ones, in name order, at
/// most `MAX_USER_MATERIALS`; a file that does not read is a warning.
#[test]
fn the_users_library_is_listed_after_and_capped() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("materials");
    assert_eq!(list_library(&user, 10), (Vec::new(), Vec::new()), "a missing library is empty");
    std::fs::create_dir(&user).unwrap();
    write(&user, "lava-rock.ron", &mine("lava-rock", "Lava rock"));
    write(&user, "basalt.ron", &mine("basalt", "Basalt"));
    write(&user, "broken.ron", "(id: \"broken\"");
    write(&user, "other.ron", &mine("elsewhere", "Elsewhere"));
    write(&user, "Bad Name.ron", &mine("bad", "Bad"));
    write(&user, "notes.txt", "not a material");
    let (list, warnings) = list_library(&user, 10);
    let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Basalt", "Lava rock"]);
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.starts_with("material broken:")), "{warnings:?}");
    assert!(warnings.iter().any(|w| w.starts_with("material other:") && w.contains("elsewhere")), "{warnings:?}");
    let (list, warnings) = list_library(&user, 1);
    assert_eq!(list.len(), 1);
    assert!(warnings.iter().any(|w| w.contains("more than 1 materials")), "{warnings:?}");

    let (all, warnings) = list_materials(Some(&built_in_dir()), Some(&user));
    assert_eq!(warnings.len(), 2);
    assert_eq!(all.len(), 10);
    assert!(all[..8].iter().all(|e| e.builtin) && all[8..].iter().all(|e| !e.builtin));
    assert_eq!(all[8].material.id, "basalt");
    // The cap holds.
    let many = tmp.path().join("many");
    std::fs::create_dir(&many).unwrap();
    for i in 0..=MAX_USER_MATERIALS {
        write(&many, &format!("m{i:03}.ron"), &mine(&format!("m{i:03}"), &format!("M {i:03}")));
    }
    let (all, warnings) = list_materials(None, Some(&many));
    assert_eq!(all.len(), MAX_USER_MATERIALS);
    assert_eq!(warnings.len(), 1);
}
