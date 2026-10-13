//! The download cache (`rl_content::DownloadCache`) and the content overlays
//! (`Content::with_packs`, `Content::has`), on synthetic skin packs.

use std::path::{Path, PathBuf};

use rl_content::{CACHE_BYTES, Content, DownloadCache, Eviction, InstallError, MAX_SKIN_ARCHIVE};
use rl_data::image::{Rgba, RgbaImage};
use rl_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use rl_data::skin::{FrameLayout, MaskKind, Skin, SkinDef};
use rl_pack::archive::{self, ArchiveError};
use rl_vfs::{DirFs, MemFs};
use sha2::{Digest, Sha256};

fn manifest(id: &str, kind: PackKind) -> PackManifest {
    PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: id.into(),
        name: id.into(),
        version: "1".into(),
        kind,
        authors: vec![],
        origin: None,
        license: None,
    }
}

/// A valid skin pack `id`; `shade` makes its content differ.
fn skin_pack(id: &str, shade: u8) -> MemFs {
    let mut img = RgbaImage::new(32, 18);
    for y in 0..18 {
        for x in 0..32 {
            img.set(x, y, Rgba::rgb(shade, 100, 10));
        }
    }
    let skin = Skin {
        def: SkinDef { name: id.into(), layout: FrameLayout::OLX },
        image: img,
        mask: vec![MaskKind::Keep; 32 * 18],
    };
    let mut fs = MemFs::new();
    rl_pack::write_skin(&mut fs, &rl_pack::SkinPack { manifest: manifest(id, PackKind::Skin), skin })
        .unwrap();
    fs
}

/// A pack that only claims to be a level.
fn level_pack(id: &str) -> MemFs {
    let mut fs = MemFs::new();
    rl_pack::write_manifest(&mut fs, &manifest(id, PackKind::Level)).unwrap();
    fs
}

struct Archive {
    bytes: Vec<u8>,
    hash: String,
    sha: String,
}

fn archive_of(fs: &MemFs) -> Archive {
    let bytes = archive::encode(fs).unwrap();
    Archive { hash: rl_pack::content_hash(fs).unwrap(), sha: hex::encode(Sha256::digest(&bytes)), bytes }
}

fn put(cache: &mut DownloadCache, a: &Archive) {
    let mut off = 0;
    for chunk in a.bytes.chunks(a.bytes.len() / 3 + 1) {
        off = cache.append(&a.hash, &a.sha, a.bytes.len() as u64, off, chunk).unwrap();
    }
    assert_eq!(off, a.bytes.len() as u64);
}

fn install(cache: &mut DownloadCache, a: &Archive, id: &str) -> PathBuf {
    put(cache, a);
    cache.install(&a.hash, id, PackKind::Skin, MAX_SKIN_ARCHIVE).unwrap()
}

fn assert_clean(dir: &Path, a: &Archive) {
    let tmp = dir.join("tmp");
    assert!(!tmp.exists() || std::fs::read_dir(&tmp).unwrap().next().is_none(), "tmp is not empty");
    assert!(!dir.join(format!("{}.part", a.hash)).exists(), "the partial stays");
    assert!(!dir.join("packs").join(&a.hash).exists(), "the pack was installed");
}

#[test]
fn a_complete_download_installs_and_is_found() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("downloads");
    let mut cache = DownloadCache::open(&dir).unwrap();
    let a = archive_of(&skin_pack("x", 1));
    assert!(!cache.contains(&a.hash));
    let path = install(&mut cache, &a, "x");
    assert_eq!(path, dir.join("packs").join(&a.hash));
    assert!(cache.contains(&a.hash));
    assert_eq!(cache.get(&a.hash), Some(path.clone()));
    assert_eq!(rl_pack::content_hash(&DirFs::new(&path)).unwrap(), a.hash);
    assert!(cache.partial(&a.hash).is_none());
    assert!(cache.total_bytes() > 0);
    assert_eq!(cache.get("not a hash"), None);
}

#[test]
fn a_partial_survives_reopening() {
    let root = tempfile::tempdir().unwrap();
    let a = archive_of(&skin_pack("x", 2));
    let half = a.bytes.len() / 2;
    let mut cache = DownloadCache::open(root.path()).unwrap();
    assert_eq!(
        cache.append(&a.hash, &a.sha, a.bytes.len() as u64, 0, &a.bytes[..half]).unwrap(),
        half as u64
    );
    // A write at another offset is refused; nothing changes.
    assert!(cache.append(&a.hash, &a.sha, a.bytes.len() as u64, 1, &a.bytes[1..2]).is_err());
    drop(cache);
    let mut cache = DownloadCache::open(root.path()).unwrap();
    let p = cache.partial(&a.hash).unwrap();
    assert_eq!((p.archive.as_str(), p.size, p.have), (a.sha.as_str(), a.bytes.len() as u64, half as u64));
    // An incomplete partial does not install, and stays.
    assert!(cache.install(&a.hash, "x", PackKind::Skin, MAX_SKIN_ARCHIVE).is_err());
    assert_eq!(cache.partial(&a.hash).unwrap().have, half as u64);
    cache.append(&a.hash, &a.sha, a.bytes.len() as u64, half as u64, &a.bytes[half..]).unwrap();
    cache.install(&a.hash, "x", PackKind::Skin, MAX_SKIN_ARCHIVE).unwrap();
    assert!(cache.contains(&a.hash));
}

#[test]
fn install_refuses_what_does_not_verify() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("cache");
    let mut cache = DownloadCache::open(&dir).unwrap();
    let outside = |root: &Path| std::fs::read_dir(root).unwrap().count();
    assert_eq!(outside(root.path()), 1);

    // A flipped byte.
    let mut a = archive_of(&skin_pack("x", 3));
    a.bytes[40] ^= 1;
    put(&mut cache, &a);
    let e = cache.install(&a.hash, "x", PackKind::Skin, MAX_SKIN_ARCHIVE).unwrap_err();
    assert!(matches!(e, InstallError::Hash(_)), "{e}");
    assert_clean(&dir, &a);

    // Another id.
    let a = archive_of(&skin_pack("y", 3));
    put(&mut cache, &a);
    let e = cache.install(&a.hash, "x", PackKind::Skin, MAX_SKIN_ARCHIVE).unwrap_err();
    assert!(matches!(&e, InstallError::Id { wanted, found } if wanted == "x" && found == "y"), "{e}");
    assert_clean(&dir, &a);

    // A level archived as a skin.
    let a = archive_of(&level_pack("x"));
    put(&mut cache, &a);
    let e = cache.install(&a.hash, "x", PackKind::Skin, MAX_SKIN_ARCHIVE).unwrap_err();
    assert!(matches!(e, InstallError::Kind { wanted: PackKind::Skin, found: PackKind::Level }), "{e}");
    assert_clean(&dir, &a);

    // An invalid pack.
    let a = archive_of(&skin_pack("x", 4).with("skin.ron", "garbage"));
    put(&mut cache, &a);
    let e = cache.install(&a.hash, "x", PackKind::Skin, MAX_SKIN_ARCHIVE).unwrap_err();
    assert!(matches!(e, InstallError::Invalid(_)), "{e}");
    assert_clean(&dir, &a);

    // An archive over the limit.
    let a = archive_of(&skin_pack("x", 5).with("pad.bin", vec![7u8; MAX_SKIN_ARCHIVE as usize]));
    put(&mut cache, &a);
    let e = cache.install(&a.hash, "x", PackKind::Skin, MAX_SKIN_ARCHIVE).unwrap_err();
    assert!(matches!(e, InstallError::Archive(ArchiveError::TooLarge(_))), "{e}");
    assert_clean(&dir, &a);

    // The archive's SHA-256 matches the bytes, but they are another pack:
    // the content hash is the real defence.
    let wanted = archive_of(&skin_pack("x", 6));
    let other = archive_of(&skin_pack("x", 7));
    let a = Archive { bytes: other.bytes, hash: wanted.hash, sha: other.sha };
    put(&mut cache, &a);
    let e = cache.install(&a.hash, "x", PackKind::Skin, MAX_SKIN_ARCHIVE).unwrap_err();
    assert!(matches!(&e, InstallError::Hash(m) if m.contains("content")), "{e}");
    assert_clean(&dir, &a);

    assert_eq!(cache.total_bytes(), 0);
    assert_eq!(outside(root.path()), 1);
}

#[test]
fn a_pack_missing_from_the_index_is_adopted() {
    let root = tempfile::tempdir().unwrap();
    let index = root.path().join("index.ron");
    let mut cache = DownloadCache::open(root.path()).unwrap();
    let before = std::fs::read(&index).unwrap();
    let a = archive_of(&skin_pack("x", 50));
    install(&mut cache, &a, "x");
    let bytes = cache.total_bytes();
    drop(cache);
    // A crash between the rename and the index write; junk in packs/ too.
    std::fs::write(&index, before).unwrap();
    std::fs::create_dir(root.path().join("packs/junk")).unwrap();
    let cache = DownloadCache::open(root.path()).unwrap();
    assert!(cache.contains(&a.hash));
    assert_eq!(cache.total_bytes(), bytes);
    assert!(!root.path().join("packs/junk").exists());
    drop(cache);
    std::fs::write(&index, "(counter: 0, entries: {})").unwrap();
    let mut cache = DownloadCache::open(root.path()).unwrap();
    cache.clear(&[]).unwrap();
    assert!(!root.path().join("packs").join(&a.hash).exists());
    assert_eq!(cache.total_bytes(), 0);
}

#[test]
fn eviction_keeps_what_the_lobby_needs() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = DownloadCache::open(root.path()).unwrap();
    let hashes: Vec<String> = (0..3u8)
        .map(|i| {
            let a = archive_of(&skin_pack("x", 10 + i));
            install(&mut cache, &a, "x");
            a.hash
        })
        .collect();
    let one = cache.total_bytes() / 3;
    // Use order: 2, 1, 0 touched last, so 2 is the least recently used, then 1.
    cache.get(&hashes[2]);
    cache.get(&hashes[1]);
    cache.get(&hashes[0]);
    // The use counters are saved when the cache closes, not at every `get`.
    drop(cache);
    let mut cache = DownloadCache::open(root.path()).unwrap();
    assert_eq!(cache.evict(&[], CACHE_BYTES), Eviction::default());
    let evicted = cache.evict(&[hashes[2].as_str()], one * 2 + one / 2);
    assert_eq!(evicted, Eviction { evicted: vec![hashes[1].clone()], stuck: vec![] });
    assert!(cache.contains(&hashes[2]) && cache.contains(&hashes[0]) && !cache.contains(&hashes[1]));
    assert!(!root.path().join("packs").join(&hashes[1]).exists());
    // Never what is kept, even over the limit.
    assert_eq!(cache.evict(&[hashes[2].as_str()], 0).evicted, vec![hashes[0].clone()]);
    assert!(cache.contains(&hashes[2]));
    cache.clear(&[]).unwrap();
    assert_eq!(cache.total_bytes(), 0);
    assert!(!cache.contains(&hashes[2]));
}

#[test]
fn a_damaged_index_is_rebuilt() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = DownloadCache::open(root.path()).unwrap();
    let a = archive_of(&skin_pack("x", 20));
    install(&mut cache, &a, "x");
    let bytes = cache.total_bytes();
    drop(cache);
    std::fs::write(root.path().join("index.ron"), "((garbage").unwrap();
    let mut cache = DownloadCache::open(root.path()).unwrap();
    assert!(cache.contains(&a.hash));
    assert_eq!(cache.total_bytes(), bytes);
    assert!(cache.get(&a.hash).is_some());
    // Missing too.
    drop(cache);
    std::fs::remove_file(root.path().join("index.ron")).unwrap();
    assert!(DownloadCache::open(root.path()).unwrap().contains(&a.hash));
}

/// Writes `fs` into `dir/<id>`.
fn write_pack(dir: &Path, id: &str, fs: &MemFs) -> PathBuf {
    let path = dir.join(id);
    let mut sink = DirFs::new(&path);
    for (p, bytes) in fs.files() {
        rl_vfs::ContentSink::write(&mut sink, p, bytes).unwrap();
    }
    path
}

fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn an_overlay_hides_a_local_pack_of_the_same_id() {
    let root = tempfile::tempdir().unwrap();
    let local = root.path().join("local");
    let other = root.path().join("other");
    let a = skin_pack("x", 30);
    let b = skin_pack("x", 31);
    write_pack(&local, "x", &a);
    write_pack(&local, "keep", &skin_pack("keep", 32));
    let cached = write_pack(&other, "x", &b);
    let (ha, hb) = (rl_pack::content_hash(&a).unwrap(), rl_pack::content_hash(&b).unwrap());
    let before = tree(root.path());

    let content = Content::open_layers(&[local.as_path()]).unwrap();
    let over = content.with_packs(&[("x".to_string(), cached.clone())]).unwrap();
    assert_eq!(over.pack_dir("x"), Some(cached.as_path()));
    assert_eq!(over.pack_hash("x", PackKind::Skin).unwrap(), hb);
    assert_eq!(content.pack_hash("x", PackKind::Skin).unwrap(), ha);
    assert_eq!(over.ids(PackKind::Skin), vec!["keep".to_string(), "x".to_string()]);
    assert!(over.pack_dir("keep").is_some());
    assert_eq!(tree(root.path()), before);

    // A pack of another kind than this content's pack of that id is
    // refused: a skin never stands in for a level.
    let level = root.path().join("level");
    write_pack(&level, "lvl", &level_pack("lvl"));
    let fake = write_pack(&other, "lvl", &skin_pack("lvl", 33));
    let with_level = Content::open_layers(&[level.as_path()]).unwrap();
    let refused = with_level.with_packs(&[("lvl".to_string(), fake)]);
    assert!(refused.is_err_and(|e| format!("{e:#}").contains("Level")), "another kind");

    // A directory that is not the named pack is refused.
    assert!(content.with_packs(&[("y".to_string(), cached)]).is_err());
    assert!(content.with_packs(&[("x".to_string(), root.path().join("nothing"))]).is_err());
}

#[test]
fn has_compares_the_hash() {
    let root = tempfile::tempdir().unwrap();
    let a = skin_pack("x", 40);
    write_pack(root.path(), "x", &a);
    let content = Content::open_layers(&[root.path()]).unwrap();
    let ha = rl_pack::content_hash(&a).unwrap();
    assert!(content.has("x", &ha));
    assert!(content.has("x", &ha)); // memoised
    assert!(!content.has("x", &rl_pack::content_hash(&skin_pack("x", 41)).unwrap()));
    assert!(!content.has("nope", &ha));
}

/// A hash computed elsewhere (a worker thread) is remembered for the pack
/// it was computed from: `known_hash` never hashes, and `has` uses it.
#[test]
fn a_remembered_hash_is_used_without_hashing() {
    let root = tempfile::tempdir().unwrap();
    let dir = write_pack(root.path(), "x", &skin_pack("x", 50));
    write_pack(root.path(), "y", &skin_pack("y", 51));
    let content = Content::open_layers(&[root.path()]).unwrap();
    assert_eq!(content.known_hash("x"), None, "not hashed yet");
    let elsewhere = "ab".repeat(32);
    // Not the pack's directory: ignored.
    content.remember_hash("x", &root.path().join("y"), &elsewhere);
    content.remember_hash("nope", &dir, &elsewhere);
    assert_eq!(content.known_hash("x"), None);
    content.remember_hash("x", &dir, &elsewhere);
    assert_eq!(content.known_hash("x"), Some(elsewhere.clone()));
    assert!(content.has("x", &elsewhere), "the remembered hash, not a fresh one");
    // `known_hash` sees what `pack_hash` memoised too; an overlay keeps both.
    let hy = content.pack_hash("y", PackKind::Skin).unwrap();
    assert_eq!(content.known_hash("y"), Some(hy.clone()));
    let over = content.with_packs(&[]).unwrap();
    assert_eq!((over.known_hash("x"), over.known_hash("y")), (Some(elsewhere), Some(hy)));
}

#[test]
fn a_cache_directory_belongs_to_one_open_cache() {
    let root = tempfile::tempdir().unwrap();
    let cache = DownloadCache::open(root.path()).unwrap();
    // A second open, as another process would: the lock is per open file
    // (flock on Linux, LockFileEx on Windows), so the same process sees it.
    assert!(matches!(DownloadCache::open(root.path()), Err(InstallError::Locked)));
    drop(cache);
    let mut cache = DownloadCache::open(root.path()).unwrap();
    // Clearing the cache keeps the lock file.
    cache.clear(&[]).unwrap();
    assert!(matches!(DownloadCache::open(root.path()), Err(InstallError::Locked)));
}

#[cfg(unix)]
#[test]
fn eviction_reports_what_it_cannot_remove() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let mut cache = DownloadCache::open(root.path()).unwrap();
    let a = archive_of(&skin_pack("x", 60));
    let b = archive_of(&skin_pack("x", 61));
    let pa = install(&mut cache, &a, "x");
    install(&mut cache, &b, "x");
    // `a` (the least recently used) holds a directory whose files cannot be removed.
    let locked = pa.join("sub");
    std::fs::create_dir(&locked).unwrap();
    std::fs::write(locked.join("f"), b"x").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let protected = std::fs::remove_file(locked.join("f")).is_err();
    let e = cache.evict(&[], 0);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    if !protected {
        eprintln!("this user may remove read-only directories' files; skipping");
        return;
    }
    assert_eq!(e, Eviction { evicted: vec![b.hash.clone()], stuck: vec![a.hash.clone()] });
    // A pack that is partly gone is no longer offered.
    assert!(!cache.contains(&a.hash));
    assert_eq!(cache.get(&a.hash), None);
    assert_eq!(cache.total_bytes(), 0);
}

#[test]
fn partials_count_toward_the_budget() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = DownloadCache::open(root.path()).unwrap();
    let parts: Vec<Archive> = (0..3u8).map(|i| archive_of(&skin_pack("x", 70 + i))).collect();
    for p in &parts {
        cache.append(&p.hash, &p.sha, p.bytes.len() as u64, 0, &p.bytes[..100]).unwrap();
    }
    let installed = archive_of(&skin_pack("x", 80));
    install(&mut cache, &installed, "x");
    let packs = cache.total_bytes();
    assert_eq!(cache.partial_bytes(), 300);
    assert!(cache.trim_partials(&[], packs + 300).is_empty());
    // Oldest first, never a kept one: 0 is kept, so 1 goes, then 2.
    let keep = [parts[0].hash.as_str()];
    assert_eq!(cache.trim_partials(&keep, packs + 250), vec![parts[1].hash.clone()]);
    assert_eq!(cache.partial_bytes(), 200);
    assert_eq!(cache.trim_partials(&keep, 0), vec![parts[2].hash.clone()]);
    assert!(cache.partial(&parts[0].hash).is_some() && cache.partial(&parts[2].hash).is_none());
    // The order survives reopening; restarting a partial makes it the newest.
    let b = &parts[1];
    cache.append(&b.hash, &b.sha, b.bytes.len() as u64, 0, &b.bytes[..10]).unwrap();
    let a = &parts[0];
    cache.append(&a.hash, &a.sha, a.bytes.len() as u64, 0, &a.bytes[..10]).unwrap_err();
    drop(cache);
    let mut cache = DownloadCache::open(root.path()).unwrap();
    assert_eq!(cache.trim_partials(&[], 0), vec![parts[0].hash.clone(), parts[1].hash.clone()]);
    assert_eq!(cache.partial_bytes(), 0);
}

#[test]
fn an_install_verifies_outside_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = DownloadCache::open(root.path()).unwrap();
    let a = archive_of(&skin_pack("x", 90));
    put(&mut cache, &a);
    let pending =
        cache.begin_install(&a.hash, "x", rl_data::manifest::PackKind::Skin, MAX_SKIN_ARCHIVE).unwrap();
    // The verification needs no `&DownloadCache`: a worker runs it unlocked.
    let verified = pending.verify();
    let path = cache.finish_install(pending, verified).unwrap();
    assert_eq!(path, root.path().join("packs").join(&a.hash));
    assert!(cache.contains(&a.hash) && cache.partial(&a.hash).is_none());
}
