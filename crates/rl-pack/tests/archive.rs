//! The `.rlpk` pack archive (plan 11 decision N29).

use rl_pack::archive::{self, ArchiveError};
use rl_vfs::{ContentSource, DirFs, MemFs};

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
    assert_eq!(archive::decode(&mut a.as_slice(), &mut out, archive::MAX_ARCHIVE).unwrap(), 3);
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
        let r = archive::decode(&mut with_entry(bad).as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE);
        assert!(matches!(r, Err(ArchiveError::BadPath(_))), "{bad}: {r:?}");
    }
    let long = "a".repeat(archive::MAX_ARCHIVE_PATH + 1);
    assert!(matches!(
        archive::decode(&mut with_entry(&long).as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE),
        Err(ArchiveError::BadPath(_))
    ));
    let good = archive::encode(&pack()).unwrap();
    assert!(matches!(
        archive::decode(&mut &good[..good.len() - 1], &mut MemFs::new(), archive::MAX_ARCHIVE),
        Err(ArchiveError::Truncated)
    ));
    let mut padded = good.clone();
    padded.push(0);
    assert!(matches!(
        archive::decode(&mut padded.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE),
        Err(ArchiveError::Trailing)
    ));
    assert!(matches!(
        archive::decode(&mut good.as_slice(), &mut MemFs::new(), 16),
        Err(ArchiveError::TooLarge(16))
    ));
    let mut many = archive::MAGIC.to_vec();
    many.extend((archive::MAX_ARCHIVE_FILES + 1).to_le_bytes());
    assert!(matches!(
        archive::decode(&mut many.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE),
        Err(ArchiveError::TooManyFiles)
    ));
}

#[test]
fn bad_magic_and_empty_input_are_refused() {
    assert!(matches!(
        archive::decode(&mut [].as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE),
        Err(ArchiveError::Magic)
    ));
    assert!(matches!(
        archive::decode(&mut b"NOPE!!rest".as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE),
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
        let r = archive::decode(&mut entries(names).as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE);
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
        archive::decode(&mut a.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE),
        Err(ArchiveError::TooLarge(_))
    ));
}

fn packs_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::var_os("RL_CONTENT_DIR")
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
        archive::decode(&mut a.as_slice(), &mut out, archive::MAX_ARCHIVE).unwrap();
        assert_eq!(rl_pack::content_hash(&src).unwrap(), rl_pack::content_hash(&out).unwrap(), "{name}");
        if name == "olx-skin-default" {
            assert!(a.len() <= 64 * 1024, "{name} archive is {} bytes: N31's skin limit is wrong", a.len());
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 256, failure_persistence: None, ..Default::default() })]
    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..4096)) {
        let _ = archive::decode(&mut bytes.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE);
    }
    #[test]
    fn mutated_archives_never_panic(edits in proptest::collection::vec((0usize..200, proptest::prelude::any::<u8>()), 1..8), cut in 0usize..200) {
        let mut a = archive::encode(&pack()).unwrap();
        for (i, b) in edits { if i < a.len() { a[i] = b; } }
        a.truncate(a.len().saturating_sub(cut));
        let _ = archive::decode(&mut a.as_slice(), &mut MemFs::new(), archive::MAX_ARCHIVE);
    }
}
