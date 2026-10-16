//! CPU render goldens (plan 5): the colour terrain after an oracle scenario
//! equals a committed PNG in `fixtures/render/`. An intended change is
//! accepted with `cargo xtask bless-render --reason "<why>"`, which re-runs
//! this test with `OLR_BLESS_RENDER=1` (the test then writes the PNGs) and
//! records the reason in `fixtures/blessings.log`. Needs the imported OLX
//! content (`OLR_CONTENT_DIR`), skipped with a notice without it.

use std::path::{Path, PathBuf};

use olr_data::image::RgbaImage;
use olr_scenario::{color_after, load_fixture};

fn content_dir() -> Option<String> {
    let dir = std::env::var("OLR_CONTENT_DIR").ok()?;
    if Path::new(&dir).is_dir() {
        Some(dir)
    } else {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        None
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("the workspace root").to_path_buf()
}

/// Compares `img` with `fixtures/render/<name>`, or writes it there when
/// blessing (`OLR_BLESS_RENDER` set). A mismatch writes the actual image to
/// `target/render-diff/<name>`.
fn golden(name: &str, img: &RgbaImage) {
    let path = root().join("fixtures/render").join(name);
    let committed = std::fs::read(&path).ok().and_then(|b| olr_pack::image_io::decode_rgba(&b).ok());
    if std::env::var_os("OLR_BLESS_RENDER").is_some() {
        if committed.as_ref() != Some(img) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, olr_pack::image_io::encode_rgba(img)).unwrap();
            eprintln!("blessed {}", path.display());
        }
        return;
    }
    let bless = "if the change is intended, run `cargo xtask bless-render --reason \"<why>\"`";
    let Some(committed) = committed else {
        panic!("{}: missing or unreadable golden; {bless}", path.display());
    };
    if &committed == img {
        return;
    }
    let actual = root().join("target/render-diff").join(name);
    std::fs::create_dir_all(actual.parent().unwrap()).unwrap();
    std::fs::write(&actual, olr_pack::image_io::encode_rgba(img)).unwrap();
    let size = |i: &RgbaImage| (i.width(), i.height());
    let differing = if size(&committed) == size(img) {
        let a = committed.as_raw().chunks(4);
        format!("{} pixels differ", a.zip(img.as_raw().chunks(4)).filter(|(a, b)| a != b).count())
    } else {
        format!("the size is {:?}, not {:?}", size(img), size(&committed))
    };
    panic!("{name}: {differing} from {}; the actual image is {}; {bless}", path.display(), actual.display());
}

#[test]
fn dig_color_matches_golden() {
    let Some(dir) = content_dir() else { return };
    let content = olr_content::Content::open(Path::new(&dir)).unwrap();
    let (sc, trace) = load_fixture(&root().join("fixtures/oracle"), "dig").unwrap();
    let map = color_after(&content, &sc, &trace).unwrap();
    let img = RgbaImage::from_raw(map.width(), map.height(), map.pixels().to_vec()).unwrap();
    golden("dig-color.png", &img);
}
