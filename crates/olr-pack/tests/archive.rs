//! The `.olpk` pack archive (plan 11 decision N29).

use olr_pack::archive::{self, ArchiveError};
use olr_vfs::{ContentSource, DirFs, MemFs};

fn pack() -> MemFs {
    MemFs::new()
        .with(
            "pack.ron",
            "(format_version: 4, id: \"x\", name: \"X\", version: \"1\", kind: Skin, authors: [])",
        )
        .with("skin.ron", "(name: \"x\")")
        .with("gfx/a.png", vec![1u8, 2, 3])
}

#[test]
fn round_trip_is_exact_and_deterministic() {
    let a = archive::encode(&pack()).unwrap();
    assert_eq!(a, archive::encode(&pack()).unwrap());
    let mut out = MemFs::new();
    assert_eq!(archive::decode(&mut a.as_slice(), &mut out, &archive::Limits::PACK).unwrap().files, 3);
    for (path, bytes) in pack().files() {
        assert_eq!(out.read(path).unwrap(), bytes);
    }
}

fn with_entry(path: &str) -> Vec<u8> {
    let mut a = archive::MAGIC.to_vec();
    a.extend(1u32.to_le_bytes());
    a.extend((path.len() as u16).to_le_bytes());
    a.extend(path.as_bytes());
    a.extend(1u64.to_le_bytes());
    a.push(0);
    a
}

#[test]
fn hostile_archives_are_refused() {
    for bad in ["../x", "/x", "a\\b", ".hidden", "a//b", ""] {
        let r = archive::decode(&mut with_entry(bad).as_slice(), &mut MemFs::new(), &archive::Limits::PACK);
        assert!(matches!(r, Err(ArchiveError::BadPath(_))), "{bad}: {r:?}");
    }
    let long = "a".repeat(archive::MAX_ARCHIVE_PATH + 1);
    assert!(matches!(
        archive::decode(&mut with_entry(&long).as_slice(), &mut MemFs::new(), &archive::Limits::PACK),
        Err(ArchiveError::BadPath(_))
    ));
    let good = archive::encode(&pack()).unwrap();
    assert!(matches!(
        archive::decode(&mut &good[..good.len() - 1], &mut MemFs::new(), &archive::Limits::PACK),
        Err(ArchiveError::Truncated)
    ));
    let mut padded = good.clone();
    padded.push(0);
    assert!(matches!(
        archive::decode(&mut padded.as_slice(), &mut MemFs::new(), &archive::Limits::PACK),
        Err(ArchiveError::Trailing)
    ));
    assert!(matches!(
        archive::decode(
            &mut good.as_slice(),
            &mut MemFs::new(),
            &archive::Limits { bytes: 16, ..archive::Limits::PACK }
        ),
        Err(ArchiveError::TooLarge(16))
    ));
    let mut many = archive::MAGIC.to_vec();
    many.extend((archive::MAX_ARCHIVE_FILES + 1).to_le_bytes());
    assert!(matches!(
        archive::decode(&mut many.as_slice(), &mut MemFs::new(), &archive::Limits::PACK),
        Err(ArchiveError::TooManyFiles)
    ));
}

/// An archive of `files` (path, bytes) in the given order, unchecked.
fn raw(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut a = archive::MAGIC.to_vec();
    a.extend((files.len() as u32).to_le_bytes());
    for (p, data) in files {
        a.extend((p.len() as u16).to_le_bytes());
        a.extend(p.as_bytes());
        a.extend((data.len() as u64).to_le_bytes());
        a.extend(*data);
    }
    a
}

/// Review C1: paths Windows reads otherwise (a drive, a stream, a device,
/// a dropped trailing dot or space) or folds together (case) are refused
/// before they are written, and refused by `encode` too.
#[test]
fn non_portable_paths_are_refused() {
    for bad in ["C:/x", "a/b:c", "NUL.png", "con", "aux.txt", "a./b", "b /c", "a?", "x/COM1.ron"] {
        let mut out = MemFs::new();
        let r = archive::decode(&mut raw(&[(bad, b"x")]).as_slice(), &mut out, &archive::Limits::PACK);
        assert!(matches!(r, Err(ArchiveError::BadPath(_))), "{bad}: {r:?}");
        assert!(out.files().next().is_none(), "{bad}: nothing is written");
        let src = pack().with(bad, vec![1u8]);
        assert!(archive::encode(&src).is_err(), "{bad} is archived");
    }
    // Two paths alike but for case: one file on Windows and macOS.
    for pair in [["A/x", "a/x"], ["Skin.png", "skin.png"], ["G/a", "g/b"]] {
        let mut out = MemFs::new();
        let r = archive::decode(
            &mut raw(&[(pair[0], b"1"), (pair[1], b"2")]).as_slice(),
            &mut out,
            &archive::Limits::PACK,
        );
        assert!(matches!(r, Err(ArchiveError::BadPath(_))), "{pair:?}: {r:?}");
        let written: Vec<&str> = out.files().map(|(p, _)| p.as_str()).collect();
        assert_eq!(written, [pair[0]], "{pair:?}: the second is not written");
        let src = MemFs::new().with(pair[0], vec![1u8]).with(pair[1], vec![2u8]);
        assert!(archive::encode(&src).is_err(), "{pair:?} is archived");
    }
    // A file and a directory of one name: refused before the write fails.
    let r = archive::decode(
        &mut raw(&[("a", b"1"), ("a/x", b"2")]).as_slice(),
        &mut MemFs::new(),
        &archive::Limits::PACK,
    );
    assert!(matches!(r, Err(ArchiveError::BadPath(_))), "{r:?}");
}

#[test]
fn bad_magic_and_empty_input_are_refused() {
    assert!(matches!(
        archive::decode(&mut [].as_slice(), &mut MemFs::new(), &archive::Limits::PACK),
        Err(ArchiveError::Magic)
    ));
    assert!(matches!(
        archive::decode(&mut b"NOPE!!rest".as_slice(), &mut MemFs::new(), &archive::Limits::PACK),
        Err(ArchiveError::Magic)
    ));
}

#[test]
fn duplicates_and_disorder_are_refused() {
    // Two entries "b" then "a": out of order.
    let entries = |names: [&str; 2]| {
        let mut a = archive::MAGIC.to_vec();
        a.extend(2u32.to_le_bytes());
        for p in names {
            a.extend(1u16.to_le_bytes());
            a.extend(p.as_bytes());
            a.extend(0u64.to_le_bytes());
        }
        a
    };
    for names in [["b", "a"], ["a", "a"]] {
        let r = archive::decode(&mut entries(names).as_slice(), &mut MemFs::new(), &archive::Limits::PACK);
        assert!(matches!(r, Err(ArchiveError::Order(_))), "{names:?}: {r:?}");
    }
}

#[test]
fn a_file_length_beyond_the_limit_is_refused_before_reading() {
    let mut a = archive::MAGIC.to_vec();
    a.extend(1u32.to_le_bytes());
    a.extend(1u16.to_le_bytes());
    a.extend(b"a");
    a.extend(u64::MAX.to_le_bytes());
    assert!(matches!(
        archive::decode(&mut a.as_slice(), &mut MemFs::new(), &archive::Limits::PACK),
        Err(ArchiveError::TooLarge(_))
    ));
}

fn packs_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("OLR_CONTENT_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/packs"));
    dir.join("olx-classic").is_dir().then_some(dir)
}

#[test]
fn every_committed_pack_round_trips() {
    let Some(dir) = packs_dir() else {
        eprintln!("skipping: content/packs is absent");
        return;
    };
    for name in ["olx-classic", "olx-level-dirt-level", "olx-skin-default"] {
        let src = DirFs::new(dir.join(name));
        let a = archive::encode(&src).unwrap();
        let mut out = MemFs::new();
        archive::decode(&mut a.as_slice(), &mut out, &archive::Limits::PACK).unwrap();
        assert_eq!(olr_pack::content_hash(&src).unwrap(), olr_pack::content_hash(&out).unwrap(), "{name}");
        if name == "olx-skin-default" {
            assert!(
                a.len() <= 64 * 1024,
                "{name} archive is {} bytes: the skin archive limit is wrong",
                a.len()
            );
        }
    }
}

/// Every committed pack satisfies the archive's path rules (portable names,
/// no case clashes), so every one can be sent to any platform.
#[test]
fn every_committed_pack_is_archived() {
    let Some(dir) = packs_dir() else {
        eprintln!("skipping: content/packs is absent");
        return;
    };
    let mut packs: Vec<_> =
        std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    packs.sort();
    assert!(packs.len() > 100, "{} packs", packs.len());
    for pack in packs {
        if let Err(e) = archive::encode(&DirFs::new(&pack)) {
            panic!("{}: {e}", pack.display());
        }
    }
}

#[test]
fn hidden_files_are_not_part_of_a_pack() {
    let plain = pack();
    let messy = pack().with(".DS_Store", vec![9u8]).with(".git/x", vec![8u8]).with("gfx/.hidden", vec![7u8]);
    assert_eq!(olr_pack::content_hash(&plain).unwrap(), olr_pack::content_hash(&messy).unwrap());
    assert_eq!(archive::encode(&plain).unwrap(), archive::encode(&messy).unwrap());
}

#[test]
fn a_length_within_the_limit_but_beyond_the_data_is_truncated() {
    let mut a = archive::MAGIC.to_vec();
    a.extend(1u32.to_le_bytes());
    a.extend(1u16.to_le_bytes());
    a.extend(b"a");
    a.extend(1000u64.to_le_bytes());
    a.extend([0u8; 10]);
    let r = archive::decode(&mut a.as_slice(), &mut MemFs::new(), &archive::Limits::PACK);
    assert!(matches!(r, Err(ArchiveError::Truncated)), "{r:?}");
}

#[test]
fn encode_order_agrees_with_decode_across_levels() {
    // Byte order: '-' (0x2d) sorts before '/' (0x2f), so "a-b" precedes "a/x".
    let src = MemFs::new().with("a/x", vec![1u8]).with("a-b", vec![2u8]).with("a.c", vec![3u8]);
    let a = archive::encode(&src).unwrap();
    let mut out = MemFs::new();
    assert_eq!(archive::decode(&mut a.as_slice(), &mut out, &archive::Limits::PACK).unwrap().files, 3);
    assert_eq!(out, src);
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 256, failure_persistence: None, ..Default::default() })]
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096)) {
        let _ = archive::decode(&mut bytes.as_slice(), &mut MemFs::new(), &archive::Limits::PACK);
    }
    #[test]
    fn mutated_archives_never_panic(edits in proptest::collection::vec((0usize..200, proptest::prelude::any::<u8>()), 1..8), cut in 0usize..200) {
        let mut a = archive::encode(&pack()).unwrap();
        for (i, b) in edits { if i < a.len() { a[i] = b; } }
        a.truncate(a.len().saturating_sub(cut));
        let _ = archive::decode(&mut a.as_slice(), &mut MemFs::new(), &archive::Limits::PACK);
    }
}

/// Review C2: a path deeper than `MAX_DEPTH` components is refused.
#[test]
fn a_deep_path_is_refused() {
    let deep = "a/".repeat(20) + "x";
    let r = archive::decode(&mut raw(&[(&deep, b"")]).as_slice(), &mut MemFs::new(), &archive::Limits::PACK);
    assert!(r.is_err(), "{r:?}");
}

/// Review C2: more directories than files is refused.
#[test]
fn many_directories_are_refused() {
    let names: Vec<String> = (0..300).map(|i| format!("d{i:03}/e/x")).collect();
    let files: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), &b""[..])).collect();
    let r = archive::decode(&mut raw(&files).as_slice(), &mut MemFs::new(), &archive::Limits::PACK);
    assert!(r.is_err(), "{r:?}");
}

/// The disk charge: a block a directory, each file rounded up to blocks
/// plus one; `decode` refuses a charge over the caller's limit before the
/// file is written, and the charge agrees with `file_charge`.
#[test]
fn the_disk_charge_is_bounded() {
    let files: [(&str, &[u8]); 3] = [("a/x", &[1u8; 5000]), ("a/y", b""), ("b", b"1")];
    let a = raw(&files);
    let mut out = MemFs::new();
    let got = archive::decode(&mut a.as_slice(), &mut out, &archive::Limits::PACK).unwrap();
    let block = archive::BLOCK;
    assert_eq!(got.disk, block + (2 * block + block) + block + (block + block), "{got:?}");
    assert_eq!(archive::file_charge(0), block);
    assert_eq!(archive::file_charge(1), 2 * block);
    assert_eq!(archive::file_charge(block), 2 * block);
    let tight = archive::Limits { disk: got.disk - 1, ..archive::Limits::PACK };
    let mut out = MemFs::new();
    let r = archive::decode(&mut a.as_slice(), &mut out, &tight);
    assert!(matches!(r, Err(archive::ArchiveError::TooLargeUnpacked(_))), "{r:?}");
    assert_eq!(out.files().count(), 2, "the last file is not written");
}

/// A skin's limits (few files, shallow): the review's shape (a valid skin
/// of many deep empty files) is refused.
#[test]
fn a_skin_of_many_deep_files_is_refused() {
    let skin = archive::Limits { bytes: 64 * 1024, disk: 256 * 1024, files: 16, depth: 2 };
    let names: Vec<String> = (0..300).map(|i| format!("z/{}{i:03}", "q/".repeat(6))).collect();
    let mut files: Vec<(&str, &[u8])> = vec![("pack.ron", b"x"), ("skin.ron", b"x")];
    files.extend(names.iter().map(|n| (n.as_str(), &b""[..])));
    let r = archive::decode(&mut raw(&files).as_slice(), &mut MemFs::new(), &skin);
    assert!(matches!(r, Err(ArchiveError::TooManyFiles)), "{r:?}");
    let few: Vec<(&str, &[u8])> = vec![("a/b/c", b"")];
    let r = archive::decode(&mut raw(&few).as_slice(), &mut MemFs::new(), &skin);
    assert!(matches!(r, Err(ArchiveError::TooDeep(_))), "{r:?}");
}
