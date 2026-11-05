//! Loose dirt's format (the loose dirt design): `level.ron`'s `loose`
//! materials and `loose.png`, a kind per world pixel.

use olr_data::image::RgbaImage;
use olr_data::level::{LooseMaterial, Material};
use olr_format::image_io;
use olr_format::levels::{
    LOOSE_FILE, LevelObject, LevelSource, MAX_LOOSE, compile_loose, read_level_files,
    read_level_files_lenient, write_level_files,
};
use olr_format::loose::LooseSource;
use olr_vfs::{ContentSource, MemFs, VPath};
use proptest::prelude::*;

const W: u32 = 4;
const H: u32 = 3;

/// A 4 × 3 dirt level whose top row is rock.
fn level() -> LevelSource {
    let mut material = vec![Material::Dirt; (W * H) as usize];
    material[..W as usize].fill(Material::Rock);
    LevelSource {
        name: "Loose".into(),
        notes: String::new(),
        size: (W, H),
        art_scale: 1,
        theme: "olx-theme-dirt".into(),
        objects: vec![LevelObject::spawn((1, 1))],
        material,
        front: RgbaImage::new(W, H),
        back: RgbaImage::new(W, H),
        thumb: None,
        look: None,
        glow: None,
        clear: None,
        glass: None,
        glow_beams: None,
        haze: None,
        distant: Vec::new(),
        surface: None,
        distant_surfaces: Vec::new(),
        live: None,
        loose: Vec::new(),
        loose_map: None,
    }
}

fn sand(id: &str) -> LooseSource {
    LooseSource::new(id, "Sand")
}

/// Two materials, sand (1) on the middle row and gravel (2) on the bottom.
fn loose_level() -> LevelSource {
    let mut src = level();
    src.loose =
        vec![sand("sand"), LooseSource { repose: 30, gravity: 400, ..LooseSource::new("gravel", "Gravel") }];
    let mut map = vec![0; (W * H) as usize];
    map[4..8].fill(1);
    map[8..12].fill(2);
    src.loose_map = Some(map);
    src
}

fn files(src: &LevelSource) -> MemFs {
    let mut fs = MemFs::new();
    write_level_files(&mut fs, src).unwrap();
    fs
}

fn text(fs: &MemFs, file: &str) -> String {
    String::from_utf8(fs.read(&VPath::new(file).unwrap()).unwrap()).unwrap()
}

#[test]
fn loose_round_trips() {
    let src = loose_level();
    let fs = files(&src);
    let ron = text(&fs, "level.ron");
    assert!(ron.contains("loose: ["), "{ron}");
    assert!(
        ron.contains("id: \"gravel\"") && ron.contains("repose: 30") && ron.contains("gravity: 400"),
        "{ron}"
    );
    // Defaults are left out.
    assert!(!ron.contains("repose: 45") && !ron.contains("top_speed"), "{ron}");
    let back = read_level_files(&fs).unwrap();
    assert_eq!(back, src);
    assert_eq!(files(&back), fs);
    assert_eq!(olr_format::canonical_text("level.ron", &ron).unwrap(), ron);
    assert_eq!(
        compile_loose(&back),
        vec![LooseMaterial::DEFAULT, LooseMaterial { repose: 30, gravity: 400, ..LooseMaterial::DEFAULT }]
    );
    assert!(olr_format::levels::file_paths(&src).contains(&LOOSE_FILE.to_string()));
}

#[test]
fn a_level_without_loose_writes_none() {
    let fs = files(&level());
    assert!(!text(&fs, "level.ron").contains("loose"));
    assert!(!fs.exists(&VPath::new(LOOSE_FILE).unwrap()));
    assert!(!olr_format::levels::file_paths(&level()).contains(&LOOSE_FILE.to_string()));
    assert_eq!(compile_loose(&level()), Vec::new());
}

fn refused(src: &LevelSource) -> String {
    write_level_files(&mut MemFs::new(), src).unwrap_err().to_string()
}

/// A change to a loose material.
type Edit = fn(&mut LooseSource);

#[test]
fn loose_limits() {
    let mut src = loose_level();
    src.loose = (0..=MAX_LOOSE).map(|i| sand(&format!("s{i}"))).collect();
    let e = refused(&src);
    assert!(e.contains("level.ron") && e.contains("loose") && e.contains("17"), "{e}");
    let cases: [(Edit, &str); 10] = [
        (|m| m.repose = 4, "loose[0].repose"),
        (|m| m.repose = 86, "loose[0].repose"),
        (|m| m.gravity = 0, "loose[0].gravity"),
        (|m| m.gravity = 2001, "loose[0].gravity"),
        (|m| m.top_speed = 0, "loose[0].top_speed"),
        (|m| m.top_speed = 501, "loose[0].top_speed"),
        (|m| m.slide_speed = 101, "loose[0].slide_speed"),
        (|m| m.id = "Bad Id".into(), "loose[0].id"),
        (|m| m.name = String::new(), "loose[0].name"),
        (|m| m.id = "gravel".into(), "loose[1].id"),
    ];
    for (edit, field) in cases {
        let mut src = loose_level();
        edit(&mut src.loose[0]);
        let e = refused(&src);
        assert!(e.contains("level.ron") && e.contains(field), "{field}: {e}");
    }
    // The bounds themselves are fine.
    let mut src = loose_level();
    src.loose[0] = LooseSource { repose: 5, gravity: 1, top_speed: 1, slide_speed: 0, ..sand("sand") };
    src.loose[1] =
        LooseSource { repose: 85, gravity: 2000, top_speed: 500, slide_speed: 100, ..sand("gravel") };
    write_level_files(&mut MemFs::new(), &src).unwrap();
}

/// A 16-bit grey PNG of `values` (each the high byte of a sample).
fn grey16(w: u32, h: u32, values: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Sixteen);
        let data: Vec<u8> = values.iter().flat_map(|&v| [0, v]).collect();
        enc.write_header().unwrap().write_image_data(&data).unwrap();
    }
    out
}

#[test]
fn a_hostile_loose_map_is_refused() {
    // The design: a value past the list, or a map of another size, fails
    // `pack check` and the level does not load (strict even when played:
    // loose dirt is gameplay, like live copies).
    let fs = files(&loose_level());
    let mut past = vec![0; 12];
    past[6] = 3;
    for (bytes, why) in [
        (image_io::encode_grey(W, H, &past), "3"),
        (image_io::encode_grey(W + 1, H, &[0; 15]), "5x3"),
        (grey16(W, H, &[1; 12]), "8-bit"),
        (b"not a png".to_vec(), LOOSE_FILE),
    ] {
        let fs = fs.clone().with(LOOSE_FILE, bytes);
        let e = read_level_files(&fs).unwrap_err().to_string();
        assert!(e.contains(LOOSE_FILE) && e.contains(why), "{why}: {e}");
        let e = read_level_files_lenient(&fs).unwrap_err().to_string();
        assert!(e.contains(LOOSE_FILE), "{why}: {e}");
    }
    // A map with no list names a material that is not there.
    let mut src = loose_level();
    src.loose.clear();
    assert!(refused(&src).contains(LOOSE_FILE));
    // A list with no map is no loose dirt: fine.
    let mut src = loose_level();
    src.loose_map = None;
    let back = read_level_files(&files(&src)).unwrap();
    assert_eq!(back.loose_map, None);
}

#[test]
fn loose_on_rock_is_not_refused() {
    // The design: the map is read only where `material.png` is dirt
    // (`olr_terrain::Terrain::with_loose` drops a kind elsewhere), so a
    // value on rock is no error.
    let mut src = loose_level();
    src.loose_map.as_mut().unwrap()[1] = 1; // the rock row
    assert_eq!(read_level_files(&files(&src)).unwrap(), src);
}

#[test]
fn the_loose_file_is_hashed_and_known() {
    let manifest =
        b"(format_version: 5, id: \"loose\", name: \"Loose\", version: \"1\", kind: Level, authors: [], origin: None, license: None)\n".to_vec();
    let fs = files(&loose_level()).with("pack.ron", manifest);
    let diags = olr_pack::validate(&fs);
    assert!(diags.iter().all(|d| !d.to_string().contains("not read")), "{diags:?}");
    let a = olr_pack::content_hash(&fs).unwrap();
    let mut map = vec![0; 12];
    map[5] = 2;
    let b = olr_pack::content_hash(&fs.clone().with(LOOSE_FILE, image_io::encode_grey(W, H, &map))).unwrap();
    assert_ne!(a, b);
    let pack = olr_pack::read_level(&fs).unwrap();
    assert_eq!(pack.loose, compile_loose(&loose_level()));
    assert_eq!(pack.loose_map, loose_level().loose_map);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Mutated `loose.png` bytes and `level.ron` text give errors or a
    /// level, never a panic; a level that reads keeps its map within
    /// its list.
    #[test]
    fn mutated_loose_files_never_panic(
        map_edits in proptest::collection::vec((0usize..200, any::<u8>()), 0..6),
        ron_edits in proptest::collection::vec((0usize..400, any::<u8>()), 0..4),
    ) {
        let fs = files(&loose_level());
        let mut map = fs.read(&VPath::new(LOOSE_FILE).unwrap()).unwrap();
        for (i, b) in map_edits {
            if let Some(x) = map.get_mut(i) {
                *x = b;
            }
        }
        let mut ron = fs.read(&VPath::new("level.ron").unwrap()).unwrap();
        for (i, b) in ron_edits {
            if let Some(x) = ron.get_mut(i) {
                *x = b;
            }
        }
        let fs = fs.with(LOOSE_FILE, map).with("level.ron", ron);
        if let Ok((src, _)) = read_level_files_lenient(&fs) {
            let n = src.loose.len();
            prop_assert!(src.loose_map.iter().flatten().all(|&v| usize::from(v) <= n));
        }
        let _ = read_level_files(&fs);
    }
}
