//! A level's live copies (issue #85): `live.ron` and `live/` are the
//! level's content, read with it; `editor/` stays authoring.

use olr_pack::{content_hash, read_level};
use olr_vfs::{ContentSource, DirFs, MemFs, VPath, walk};

/// `fixtures/live/levels/live-test` in memory.
fn fixture() -> MemFs {
    let dir = DirFs::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/live/levels/live-test"));
    walk(&dir, &VPath::root())
        .unwrap()
        .into_iter()
        .fold(MemFs::new(), |fs, p| fs.with(p.as_str(), dir.read(&p).unwrap()))
}

/// `fs` with `path`'s bytes changed by `f`.
fn edited(fs: &MemFs, path: &str, f: impl FnOnce(&mut Vec<u8>)) -> MemFs {
    let mut bytes = fs.read(&VPath::new(path).unwrap()).unwrap();
    f(&mut bytes);
    fs.clone().with(path, bytes)
}

#[test]
fn the_fixture_reads_with_its_live_copies() {
    let level = read_level(&fixture()).unwrap();
    let live = level.live.expect("live.ron");
    let ids: Vec<&str> = live.file.copies.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["lamp", "console-a", "console-b", "press", "crate", "gate", "spot", "lift", "door"]);
    assert_eq!(live.sounds.len(), 1);
}

#[test]
fn the_live_files_are_in_the_content_hash() {
    let fs = fixture();
    let hash = content_hash(&fs).unwrap();
    // A value in `live.ron`.
    let value = edited(&fs, "live.ron", |b| {
        let text = String::from_utf8(b.clone()).unwrap();
        *b = text.replacen("default: 80", "default: 81", 1).into_bytes();
        assert_ne!(b.as_slice(), text.as_bytes());
    });
    // A script's byte and a sound's.
    let script = edited(&fs, "live/0.rhai", |b| b.push(b'\n'));
    let sound = edited(&fs, "live/0.wav", |b| *b.last_mut().unwrap() ^= 1);
    for changed in [value, script, sound] {
        assert_ne!(content_hash(&changed).unwrap(), hash);
    }
    // A file in `editor/` is authoring.
    let authoring = fs.clone().with("editor/scripts/lamp.rhai", "fn tick() {}\n");
    assert_eq!(content_hash(&authoring).unwrap(), hash);
}

/// A level's live files fit in an archive twice over (the rest of the
/// pack keeps its room).
#[test]
fn the_live_file_cap_leaves_room_in_an_archive() {
    assert_eq!(olr_format::levels::MAX_LIVE_FILES * 2, olr_pack::archive::MAX_ARCHIVE_FILES as usize);
}
