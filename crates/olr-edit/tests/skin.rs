//! Skin operations (issue #13): a copy round-trips its source exactly, a
//! classic skin doubles by Scale2x (40 × 36 frames, each 2 × 2 block the
//! source pixel where Scale2x keeps it), blank skins, the check refusing
//! bad skins, and the preview's size and glow.

use std::path::{Path, PathBuf};

use olr_data::image::Rgba;
use olr_data::manifest::PackKind;
use olr_data::skin::SkinLight;
use olr_edit::new::{NewError, Template, new_pack};
use olr_edit::skin::{
    MAX_PREVIEW_SCALE, PREVIEW_LIGHT, SkinError, blank, check_dir, double, new_skin, preview, preview_size,
};
use olr_format::skins::{DOUBLE_SKIN, OLX_SKIN, SkinSource, compile_skin, read_skin_files};
use olr_vfs::DirFs;

use crate::common::{copy_dir, tree};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// OLX's default skin, as committed (offline too).
fn default_skin() -> PathBuf {
    repo().join("content/packs/olx-skin-default")
}

/// Firefly, OLR's double-resolution sample skin (glow and light).
fn firefly() -> PathBuf {
    repo().join("content/reloaded/olr-skin-firefly")
}

fn read(dir: &Path) -> SkinSource {
    read_skin_files(&DirFs::new(dir)).unwrap()
}

/// `skin new --from` copies a skin's files exactly: the same source but
/// for the name, `skin.png`, `tint.png` and `glow.png` byte for byte; the
/// manifest takes the new id and name and drops the origin.
#[test]
fn a_copy_round_trips_its_source_exactly() {
    let tmp = tempfile::tempdir().unwrap();
    for from in [default_skin(), firefly()] {
        let out = tmp.path().join(from.file_name().unwrap());
        assert_eq!(new_skin(&out, "my-copy", "My Copy", Some(&from), false).unwrap(), Vec::<String>::new());
        let (mut copy, src) = (read(&out), read(&from));
        assert_eq!(copy.name, "My Copy");
        copy.name = src.name.clone();
        assert_eq!(copy, src, "{}", from.display());
        let (a, b) = (tree(&out), tree(&from));
        for file in ["skin.png", "tint.png", "glow.png"] {
            assert_eq!(a.get(file), b.get(file), "{file} of {}", from.display());
        }
        let m = olr_pack::read_manifest(&DirFs::new(&out)).unwrap();
        assert_eq!(
            (m.id.as_str(), m.name.as_str(), m.kind, m.origin),
            ("my-copy", "My Copy", PackKind::Skin, None)
        );
    }
}

/// The Scale2x neighbours of `(x, y)` within its frame: above, below,
/// left, right; a frame's edge counts the pixel itself.
fn neighbours(x: u32, y: u32, (fw, fh): (u32, u32)) -> [(u32, u32); 4] {
    let (fx, fy) = (x % fw, y % fh);
    [
        if fy > 0 { (x, y - 1) } else { (x, y) },
        if fy + 1 < fh { (x, y + 1) } else { (x, y) },
        if fx > 0 { (x - 1, y) } else { (x, y) },
        if fx + 1 < fw { (x + 1, y) } else { (x, y) },
    ]
}

/// `double` gives OLX's frames at 40 × 36 (art scale 4); where Scale2x
/// keeps a source pixel (its neighbours above and below, or left and
/// right, alike) its 2 × 2 block is that pixel, colour and tint; every
/// other block is made of it and its neighbours only. It compiles to the
/// double sheet.
#[test]
fn a_classic_skin_doubles_by_scale2x() {
    let src = read(&default_skin());
    let d = double(&src).unwrap();
    assert_eq!((d.art_scale, d.frame_size, d.aim_frames, d.walk_frames), DOUBLE_SKIN);
    assert_eq!((d.image.width(), d.image.height()), (280, 108));
    assert_eq!((d.name.as_str(), d.glow.as_ref(), d.light), ("default", None, None));
    let w = src.image.width();
    let key = |s: &SkinSource, x: u32, y: u32| {
        let p = s.image.get(x, y);
        (if p.a == 0 { Rgba::default() } else { p }, s.tint[(y * s.image.width() + x) as usize])
    };
    let (mut kept, mut changed) = (0, 0);
    for y in 0..src.image.height() {
        for x in 0..w {
            let e = key(&src, x, y);
            let [b, h, l, r] = neighbours(x, y, src.frame_size).map(|(x, y)| key(&src, x, y));
            let block = [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| key(&d, 2 * x + dx, 2 * y + dy));
            if b == h || l == r {
                assert_eq!(block, [e; 4], "({x}, {y}) is kept");
                kept += 1;
            } else {
                assert!(block.iter().all(|k| [e, b, h, l, r].contains(k)), "({x}, {y})");
                changed += usize::from(block != [e; 4]);
            }
        }
    }
    assert!(kept > 4000 && changed > 1000, "{kept} kept, {changed} smoothed");
    let skin = compile_skin(&d).unwrap();
    assert_eq!((skin.image.width(), skin.image.height()), (1344, 36));
}

/// Scale2x's rule on a small case: a corner of a diagonal edge takes its
/// neighbours' colour and tint (and glow), so a tinted edge stays tinted;
/// frames never take from each other; glow and light are kept.
#[test]
fn scale2x_rounds_corners_within_each_frame() {
    let mut src = blank("t", false);
    let w = src.image.width();
    let body = Rgba::rgb(100, 150, 250);
    // A tinted, glowing body pixel above and left of an empty one at (5, 5).
    for (x, y) in [(5, 4), (4, 5), (4, 4)] {
        src.image.set(x, y, body);
        src.tint[(y * w + x) as usize] = 255;
    }
    let mut glow = vec![0u8; src.tint.len()];
    glow[(4 * w + 5) as usize] = 200;
    glow[(5 * w + 4) as usize] = 200;
    src.glow = Some(glow);
    src.light = Some(SkinLight { intensity: 50, reach: 10 });
    // Frame 1 is solid; frame 0's last column stays empty beside it.
    for y in 0..18 {
        for x in 20..40 {
            src.image.set(x, y, Rgba::rgb(1, 2, 3));
        }
    }
    let d = double(&src).unwrap();
    let w2 = d.image.width();
    // (5, 5)'s top-left art pixel takes the body (above and left alike).
    assert_eq!(d.image.get(10, 10), body);
    assert_eq!(d.tint[(10 * w2 + 10) as usize], 255);
    assert_eq!(d.glow.as_ref().unwrap()[(10 * w2 + 10) as usize], 200);
    for (x, y) in [(11, 10), (10, 11), (11, 11)] {
        assert_eq!(d.image.get(x, y), Rgba::default(), "({x}, {y})");
    }
    for y in 0..36 {
        assert_eq!(d.image.get(39, y), Rgba::default(), "frame 0's edge, row {y}");
        assert_eq!(d.image.get(40, y), Rgba::rgb(1, 2, 3), "frame 1's edge, row {y}");
    }
    assert_eq!(d.light, Some(SkinLight { intensity: 50, reach: 10 }));
}

#[test]
fn only_a_classic_skin_doubles() {
    let d = double(&read(&default_skin())).unwrap();
    let e = double(&d).unwrap_err();
    assert!(matches!(e, SkinError::NotClassic { art_scale: 4, frame_size: (40, 36) }), "{e}");
    let mut bad = read(&default_skin());
    bad.tint.pop();
    assert!(matches!(double(&bad), Err(SkinError::Size("tint.png"))));
    // Through new_skin: nothing is written.
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("x");
    assert!(matches!(new_skin(&out, "x", "X", Some(&firefly()), true), Err(NewError::Edit(_))));
    assert!(!out.exists());
}

/// `skin new --double --from` is the source doubled, renamed; without
/// `--from` a blank skin of either layout.
#[test]
fn new_skins_from_a_copy_doubled_or_blank() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("d");
    new_skin(&out, "d", "Doubled", Some(&default_skin()), true).unwrap();
    let mut want = double(&read(&default_skin())).unwrap();
    want.name = "Doubled".into();
    assert_eq!(read(&out), want);
    for (dbl, layout) in [(false, OLX_SKIN), (true, DOUBLE_SKIN)] {
        let out = tmp.path().join(format!("blank-{dbl}"));
        new_skin(&out, "b", "Blank", None, dbl).unwrap();
        let checked = check_dir(&out);
        assert!(checked.errors.is_empty() && checked.warnings.is_empty(), "{checked:?}");
        let s = read(&out);
        assert_eq!((s.art_scale, s.frame_size, s.aim_frames, s.walk_frames), layout);
        assert!(s.image.as_raw().iter().all(|&b| b == 0) && s.glow.is_none() && s.light.is_none());
        assert_eq!(s, blank("Blank", dbl));
    }
    // A skin template makes skins only.
    let e = new_pack(&tmp.path().join("m"), PackKind::Mod, "m", "M", Template::Skin { double: false });
    assert!(matches!(e, Err(NewError::WrongKind { .. })));
}

/// `check_dir` loads and validates: the shipped skins pass; another kind
/// of pack, a missing manifest, an image of the wrong size, a light above
/// the caps and partial tint are errors, never a panic.
#[test]
fn the_check_refuses_bad_skins() {
    for dir in [default_skin(), firefly()] {
        let c = check_dir(&dir);
        assert!(c.errors.is_empty() && c.warnings.is_empty(), "{}: {c:?}", dir.display());
        assert!(c.skin.is_some());
    }
    let c = check_dir(&repo().join("content/packs/olx-level-dirt-level"));
    assert!(c.errors.iter().any(|e| e.contains("not a skin")), "{c:?}");
    let tmp = tempfile::tempdir().unwrap();
    let c = check_dir(tmp.path());
    assert!(!c.errors.is_empty() && c.skin.is_none());
    let bad = |name: &str, edit: &dyn Fn(&Path)| {
        let dir = tmp.path().join(name);
        copy_dir(&firefly(), &dir);
        edit(&dir);
        let c = check_dir(&dir);
        assert!(c.skin.is_none(), "{name}: {c:?}");
        c.errors.join("; ")
    };
    let e = bad("glow", &|d| {
        std::fs::copy(default_skin().join("tint.png"), d.join("glow.png")).map(drop).unwrap()
    });
    assert!(e.contains("glow.png"), "{e}");
    let e = bad("light", &|d| {
        let ron = std::fs::read_to_string(d.join("skin.ron")).unwrap().replace("reach: 24", "reach: 49");
        std::fs::write(d.join("skin.ron"), ron).unwrap();
    });
    assert!(e.contains("light.reach"), "{e}");
    let e = bad("tint", &|d| {
        let s = read(d);
        let mut tint = s.tint.clone();
        tint[0] = 128;
        let png = olr_format::image_io::encode_grey(s.image.width(), s.image.height(), &tint);
        std::fs::write(d.join("tint.png"), png).unwrap();
    });
    assert!(e.contains("partial tint"), "{e}");
    let e = bad("cut", &|d| std::fs::write(d.join("skin.png"), b"\x89PNG\r\n").unwrap());
    assert!(e.contains("skin.png"), "{e}");
}

/// The preview is 14 × 6 cells of 16 × 14 world pixels at its scale,
/// whichever the art scale; its light half's corners are the background;
/// glow shows only in the dark half.
#[test]
fn the_preview_has_its_size_and_shows_the_glow() {
    let classic = olr_pack::read_skin(&DirFs::new(default_skin())).unwrap().skin;
    let ff = olr_pack::read_skin(&DirFs::new(firefly())).unwrap().skin;
    let colour = Rgba::rgb(200, 60, 60);
    for scale in [1, 4, 8] {
        for skin in [&classic, &ff] {
            let img = preview(skin, colour, scale);
            assert_eq!((img.width(), img.height()), preview_size(scale));
            assert_eq!(preview_size(scale), (224 * scale, 84 * scale));
            assert_eq!(img.get(0, 0), PREVIEW_LIGHT);
        }
    }
    assert_eq!(preview_size(0), preview_size(1));
    assert_eq!(preview_size(99), preview_size(MAX_PREVIEW_SCALE));
    // The same skin without its glow and light: the light half alike, the
    // dark half darker.
    let mut plain = ff.clone();
    plain.glow.clear();
    plain.def.light = None;
    let (a, b) = (preview(&ff, colour, 4), preview(&plain, colour, 4));
    let half = a.height() / 2;
    let sum = |img: &olr_data::image::RgbaImage, rows: std::ops::Range<u32>| -> u64 {
        rows.flat_map(|y| (0..img.width()).map(move |x| (x, y)))
            .map(|(x, y)| {
                let p = img.get(x, y);
                u64::from(p.r) + u64::from(p.g) + u64::from(p.b)
            })
            .sum()
    };
    assert_eq!(sum(&a, 0..half), sum(&b, 0..half));
    assert!(sum(&a, half..a.height()) > sum(&b, half..a.height()) * 3 / 2, "the glow and light show");
}
