//! `olr-cli skin new|double|check|preview` (issue #13) through the binary,
//! and Firefly, the double-resolution sample skin in `content/reloaded/`:
//! it passes `skin check` and the profiles' picker lists it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use olr_content::Content;
use olr_vfs::DirFs;

const FIREFLY: &str = "olr-skin-firefly";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs the binary from the repository root (`--content`'s default,
/// `content/packs`, is the committed content).
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).current_dir(repo()).args(args).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn read(dir: &Path) -> olr_format::skins::SkinSource {
    olr_format::skins::read_skin_files(&DirFs::new(dir)).unwrap()
}

#[test]
fn skin_commands_make_check_and_preview_skins() {
    let tmp = tempfile::tempdir().unwrap();
    let p = |name: &str| tmp.path().join(name).to_str().unwrap().to_string();
    // A copy of an installed skin by its id, by its OLX name, and a blank double one.
    let o = cli(&["skin", "new", &p("copy"), "--from", "olx-skin-default", "--name", "My Copy"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("made skin pack my-copy (\"My Copy\""), "{}", text(&o));
    let mut copy = read(&tmp.path().join("copy"));
    copy.name = "default".into();
    assert_eq!(copy, read(&repo().join("content/packs/olx-skin-default")));
    let o = cli(&["skin", "new", &p("blank"), "--double", "--name", "Blank", "--id", "blank"]);
    assert!(o.status.success(), "{}", text(&o));
    // `double` by OLX name: Scale2x's 40 × 36 frames.
    let o = cli(&["skin", "double", "default", "--out", &p("double")]);
    assert!(o.status.success() && text(&o).contains("\"default Double\""), "{}", text(&o));
    let d = read(&tmp.path().join("double"));
    assert_eq!((d.art_scale, d.frame_size), (4, (40, 36)));
    let o = cli(&["skin", "check", &p("copy"), &p("blank"), &p("double")]);
    let out = text(&o);
    assert!(o.status.success(), "{out}");
    assert!(
        out.contains("\"My Copy\", classic resolution") && out.contains("\"Blank\", double resolution"),
        "{out}"
    );
    // The preview: 14 × 6 cells of 16 × 14 world pixels at --scale.
    for (skin, scale, size) in [(p("double"), "8", (1792, 672)), ("default".to_string(), "2", (448, 168))] {
        let png = p("preview.png");
        let o = cli(&["skin", "preview", &skin, "--colour", "#40c040", "--out", &png, "--scale", scale]);
        assert!(o.status.success(), "{}", text(&o));
        let img = olr_format::image_io::decode_rgba_within(
            &std::fs::read(&png).unwrap(),
            olr_format::image_io::IMAGES,
        )
        .unwrap();
        assert_eq!((img.width(), img.height()), size, "{skin}");
    }
}

#[test]
fn skin_commands_refuse_bad_input() {
    let tmp = tempfile::tempdir().unwrap();
    let p = |name: &str| tmp.path().join(name).to_str().unwrap().to_string();
    // A level pack is not a skin; a missing directory has no pack.
    let o = cli(&["skin", "check", "content/packs/olx-skin-default", "content/packs/olx-level-dirt-level"]);
    assert!(!o.status.success() && text(&o).contains("not a skin"), "{}", text(&o));
    let o = cli(&["skin", "check", &p("nothing")]);
    assert!(!o.status.success(), "{}", text(&o));
    // An unknown skin, a double skin doubled, a non-empty --out, bad flags.
    let o = cli(&["skin", "double", "no-such-skin", "--out", &p("x")]);
    assert!(!o.status.success() && text(&o).contains("no-such-skin"), "{}", text(&o));
    let o = cli(&["skin", "double", FIREFLY, "--out", &p("x")]);
    assert!(!o.status.success() && text(&o).contains("only OLX's layout"), "{}", text(&o));
    assert!(!tmp.path().join("x").exists());
    let o = cli(&["skin", "new", "content/packs", "--name", "X"]);
    assert!(!o.status.success() && text(&o).contains("not an empty directory"), "{}", text(&o));
    for colour in ["red", "12345", "#gg0000"] {
        let o = cli(&["skin", "preview", "default", "--colour", colour, "--out", &p("p.png")]);
        assert!(!o.status.success() && text(&o).contains("--colour"), "{colour}: {}", text(&o));
    }
    let o = cli(&["skin", "preview", "default", "--colour", "ff0000", "--out", &p("p.png"), "--scale", "17"]);
    assert!(!o.status.success() && text(&o).contains("--scale"), "{}", text(&o));
}

/// The sample skin passes `skin check` (so `cargo xtask check` checks it),
/// double resolution with a glowing lantern and a modest light; the
/// bundled content lists it by name, as the profiles' picker shows it.
#[test]
fn firefly_checks_and_is_listed() {
    let dir = repo().join("content/reloaded").join(FIREFLY);
    let o = cli(&["skin", "check", dir.to_str().unwrap()]);
    let out = text(&o);
    assert!(o.status.success(), "{out}");
    assert!(out.contains("\"Firefly\", double resolution, glowing, light 80 % over 24 px"), "{out}");
    let content = Content::open_bundled(&repo().join("content/packs")).unwrap();
    let listed = content.skins();
    assert!(listed.iter().any(|s| s.id == FIREFLY && s.name == "Firefly"), "{listed:?}");
    assert!(content.catalog().skin(FIREFLY).is_some());
    let skin = content.load_skin(FIREFLY).unwrap();
    assert_eq!(skin.art_scale(), 4);
    assert!(skin.frame_lights(olr_data::image::Rgba::rgb(255, 80, 80)).is_some(), "the lantern lights");
    // Made from OLX's default skin: its origin, and its outline unchanged.
    let m = olr_pack::read_manifest(&DirFs::new(&dir)).unwrap();
    let olx = olr_pack::read_manifest(&DirFs::new(repo().join("content/packs/olx-skin-default"))).unwrap();
    assert_eq!(m.origin, olx.origin);
    let (ours, doubled) =
        (read(&dir), olr_edit::skin::double(&read(&repo().join("content/packs/olx-skin-default"))).unwrap());
    let w = ours.image.width();
    for y in 0..ours.image.height() {
        for x in 0..w {
            assert_eq!(ours.image.get(x, y).a == 0, doubled.image.get(x, y).a == 0, "({x}, {y})");
        }
    }
    assert_ne!(ours.image, doubled.image, "refined: not only Scale2x");
}

/// The glowing discs `render-match` reports for chaos bots on `level`
/// wearing `skin`.
fn discs(level: &str, skin: &str, out: &Path) -> usize {
    let o = cli(&[
        "render-match",
        "--mod",
        "olr-mod-liero-v1-reloaded",
        "--level",
        level,
        "--bots",
        "3",
        "--seed",
        "1",
        "--ticks",
        "200",
        "--skin",
        skin,
        "--out",
        out.to_str().unwrap(),
    ]);
    let out = text(&o);
    assert!(o.status.success(), "{level} {skin}: {out}");
    let n =
        out.split(" glowing discs").next().and_then(|s| s.rsplit(' ').next()).and_then(|n| n.parse().ok());
    n.unwrap_or_else(|| panic!("{out}"))
}

/// On the GPU tier: Firefly draws in a lit match on both Reloaded levels,
/// each worm's lantern adding a light (a disc) to the default skin's match.
#[test]
fn firefly_lights_its_surroundings_in_play() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    for level in ["olr-level-castlestrike-reloaded", "olr-level-liero-factory-after-dark"] {
        let plain = discs(level, "olx-skin-default", &tmp.path().join("plain.png"));
        let lit = discs(level, FIREFLY, &tmp.path().join("firefly.png"));
        assert!(lit > plain, "{level}: {lit} discs with Firefly, {plain} without");
    }
}
