//! The formats guide's examples (`docs/formats/README.md`, plan 13d Task 9)
//! are real: each parses as the file it names and is canonical, and the
//! mod and level examples make packs that check clean.

use std::collections::BTreeSet;

use olr_data::manifest::{PACK_FORMAT_VERSION, PackKind, PackManifest};
use olr_vfs::{ContentSink, MemFs, VPath};

/// A fenced block the guide marks `<!-- file: <kind>/<path> -->`.
struct Block {
    kind: String,
    path: String,
    text: String,
    /// The marker's line in the guide (1-based).
    line: usize,
}

fn guide() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/formats/README.md");
    std::fs::read_to_string(path).unwrap()
}

fn blocks(guide: &str) -> Vec<Block> {
    let lines: Vec<&str> = guide.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let Some(file) = l.trim().strip_prefix("<!-- file: ").and_then(|r| r.strip_suffix(" -->")) else {
            continue;
        };
        let (kind, path) =
            file.split_once('/').unwrap_or_else(|| panic!("README.md:{}: no kind in {file:?}", i + 1));
        assert_eq!(
            lines.get(i + 1).copied(),
            Some("```ron"),
            "README.md:{}: a ```ron block must follow",
            i + 1
        );
        let end = lines[i + 2..].iter().position(|l| *l == "```").expect("an unclosed block") + i + 2;
        let text = lines[i + 2..end].iter().map(|l| format!("{l}\n")).collect();
        out.push(Block { kind: kind.into(), path: path.into(), text, line: i + 1 });
    }
    out
}

fn manifest(kind: PackKind) -> PackManifest {
    PackManifest {
        format_version: PACK_FORMAT_VERSION,
        id: "guide".into(),
        name: "Guide".into(),
        version: "1".into(),
        kind,
        authors: vec![],
        origin: None,
        license: None,
    }
}

/// `text` as the canonical writer of its file gives it back.
fn canonical(b: &Block) -> Result<String, String> {
    if b.kind == "pack" {
        let fs = MemFs::new().with("pack.ron", b.text.as_str());
        let m = olr_pack::read_manifest(&fs).map_err(|e| e.to_string())?;
        let mut out = MemFs::new();
        olr_pack::write_manifest(&mut out, &m).map_err(|e| e.to_string())?;
        return Ok(String::from_utf8(out.read_file("pack.ron")).unwrap());
    }
    olr_format::canonical_text(&b.path, &b.text).map_err(|e| e.to_string())
}

trait ReadFile {
    fn read_file(&self, path: &str) -> Vec<u8>;
}

impl ReadFile for MemFs {
    fn read_file(&self, path: &str) -> Vec<u8> {
        olr_vfs::ContentSource::read(self, &VPath::new(path).unwrap()).unwrap()
    }
}

#[test]
fn every_guide_example_parses_and_is_canonical() {
    let blocks = blocks(&guide());
    let kinds: BTreeSet<&str> = blocks.iter().map(|b| b.kind.as_str()).collect();
    assert_eq!(
        kinds,
        BTreeSet::from(["core", "level", "mod", "pack", "skin", "theme"]),
        "one example per kind"
    );
    let mut failures = Vec::new();
    for b in &blocks {
        match canonical(b) {
            Ok(c) if c == b.text => {}
            Ok(c) => failures
                .push(format!("README.md:{}: {}/{} is not canonical; it is:\n{c}", b.line, b.kind, b.path)),
            Err(e) => failures.push(format!("README.md:{}: {}/{}: {e}", b.line, b.kind, b.path)),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The pack files of `kind`'s blocks, with a generated `pack.ron`.
fn pack(blocks: &[Block], kind: &str, pack_kind: PackKind) -> MemFs {
    let mut fs = MemFs::new();
    olr_pack::write_manifest(&mut fs, &manifest(pack_kind)).unwrap();
    for b in blocks.iter().filter(|b| b.kind == kind) {
        fs.write(&VPath::new(&b.path).unwrap(), b.text.as_bytes()).unwrap();
    }
    fs
}

#[test]
fn the_guide_examples_make_packs_that_check_clean() {
    let blocks = blocks(&guide());

    // The mod, with a stand-in for each asset its files name.
    let mut fs = pack(&blocks, "mod", PackKind::Mod);
    let assets: BTreeSet<String> = blocks
        .iter()
        .filter(|b| b.kind == "mod")
        .flat_map(|b| b.text.split('"').skip(1).step_by(2).map(str::to_string).collect::<Vec<_>>())
        .filter(|s| s.starts_with("gfx/") || s.starts_with("sfx/"))
        .collect();
    for a in &assets {
        let bytes = if a.ends_with(".png") {
            olr_format::image_io::encode_rgba(&olr_data::image::RgbaImage::new(4, 4))
        } else {
            b"OggS".to_vec()
        };
        fs.write(&VPath::new(a).unwrap(), &bytes).unwrap();
    }
    let errors: Vec<_> =
        olr_pack::validate(&fs).into_iter().filter(|d| d.severity == olr_pack::Severity::Error).collect();
    assert!(errors.is_empty(), "the mod example: {errors:?}");

    // The level, with generated material and art of its size.
    let mut fs = pack(&blocks, "level", PackKind::Level);
    let header = olr_format::levels::read_level_header(&fs).unwrap();
    let (w, h) = header.size;
    let s = header.art_scale;
    let dirt = vec![olr_data::level::Material::Dirt as u8; (w * h) as usize];
    let material = olr_format::image_io::encode_indexed(w, h, &dirt, &olr_format::levels::MATERIAL_PALETTE);
    let art = olr_format::image_io::encode_rgba(&olr_data::image::RgbaImage::new(w * s, h * s));
    for (path, bytes) in [("material.png", &material), ("front.png", &art), ("back.png", &art)] {
        fs.write(&VPath::new(path).unwrap(), bytes).unwrap();
    }
    let d = olr_pack::validate(&fs);
    assert!(d.iter().all(|d| d.severity != olr_pack::Severity::Error), "the level example: {d:?}");
}
