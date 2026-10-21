//! GPU render goldens (plan 5 task 5): `olr-cli render <scenario> --frame N`
//! of one scenario on each level must look like its committed PNG in
//! `fixtures/render/` (a perceptual diff, so two software rasterisers that
//! round a blend differently still agree).
//!
//! The goldens are drawn from the imported OLX content at test time (spec
//! §9.3: converted assets are never committed, the goldens are accepted
//! fixtures); they were blessed on lavapipe (Vulkan, CPU), the adapter
//! `olr_render::Offscreen` prefers. They run in `cargo xtask full`, which
//! sets `OLR_GPU_TESTS`; without it, the content or an adapter they skip
//! with a notice. `cargo xtask bless-render --reason "<why>"` re-draws
//! them (`OLR_BLESS_RENDER`).

use std::path::{Path, PathBuf};
use std::process::Command;

use olr_data::image::RgbaImage;

/// A pixel differs when its YIQ distance is above this (pixelmatch's 0.1).
const TOLERANCE: f32 = 0.1;
/// The share of differing pixels a golden tolerates: about 6 pixels at
/// 640x480. Lavapipe and llvmpipe (GL) both reproduce all four goldens with
/// no differing pixel; the allowance is for another LLVM's rounding.
const MAX_DIFFERING: f64 = 0.00002;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("the workspace root").to_path_buf()
}

fn blessing() -> bool {
    std::env::var_os("OLR_BLESS_RENDER").is_some()
}

/// The content directory, if GPU tests are on and the content is imported.
fn content_dir() -> Option<String> {
    if std::env::var_os("OLR_GPU_TESTS").is_none() && !blessing() {
        eprintln!("OLR_GPU_TESTS not set (`cargo xtask full` sets it); skipping");
        return None;
    }
    let dir = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir());
    if dir.is_none() {
        assert!(!blessing(), "blessing needs the imported content (`cargo xtask fetch-olx-content`)");
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
    }
    dir
}

/// Renders `scenario` at `frame` with `extra` options into
/// `target/render-actual/<name>` and compares it with
/// `fixtures/render/<name>`, or writes the golden when blessing.
fn golden(name: &str, scenario: &str, frame: i64, extra: &[&str]) {
    let Some(content) = content_dir() else { return };
    let fixtures = root().join("fixtures/oracle");
    let frame = frame.to_string();
    let args = [
        "render",
        scenario,
        "--frame",
        &frame,
        "--content",
        &content,
        "--fixtures",
        fixtures.to_str().unwrap(),
    ];
    compare(name, &args, extra);
}

/// `olr-cli look-preview` of the look test level (`fixtures/look/levels/
/// look-test`, plan 15a Task 14) centred on its building, with `extra`
/// options, against `fixtures/render/<name>`.
fn look_golden(name: &str, extra: &[&str]) {
    look_golden_of("look-test", name, extra);
}

/// [`look_golden`] of the level pack `pack` in `fixtures/look/levels/`.
fn look_golden_of(pack: &str, name: &str, extra: &[&str]) {
    let Some(content) = content_dir() else { return };
    let pack = root().join("fixtures/look/levels").join(pack);
    let args = ["look-preview", pack.to_str().unwrap(), "--at", "256,128", "--content", &content];
    compare(name, &args, extra);
}

/// Runs `olr-cli <args> --out target/render-actual/<name> <extra>` and
/// compares the PNG with `fixtures/render/<name>`, or blesses it.
fn compare(name: &str, args: &[&str], extra: &[&str]) {
    let actual = root().join("target/render-actual").join(name);
    std::fs::create_dir_all(actual.parent().unwrap()).unwrap();
    let scenario = args[1];
    let out = Command::new(env!("CARGO_BIN_EXE_olr-cli"))
        .args(args)
        .arg("--out")
        .arg(&actual)
        .args(extra)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() && stderr.contains("no graphics adapter") && !blessing() {
        eprintln!("{stderr}skipping");
        return;
    }
    assert!(out.status.success(), "olr-cli {} {scenario} failed:\n{stderr}", args[0]);
    eprint!("{}", String::from_utf8_lossy(&out.stdout));
    let decode = |p: &Path| std::fs::read(p).ok().and_then(|b| olr_pack::image_io::decode_rgba(&b).ok());
    let img: RgbaImage = decode(&actual).expect("olr-cli wrote a PNG");
    let path = root().join("fixtures/render").join(name);
    let committed = decode(&path);
    if blessing() {
        // Keep the committed file when it still passes, so a re-bless on
        // another rasteriser does not churn the goldens.
        let same = committed.as_ref().and_then(|c| olr_render::perceptual_diff(c, &img, TOLERANCE));
        if !same.is_some_and(|d| d.differing == 0) {
            std::fs::copy(&actual, &path).unwrap();
            eprintln!("blessed {}", path.display());
        }
        return;
    }
    let bless = "if the change is intended, run `cargo xtask bless-render --reason \"<why>\"`";
    let Some(committed) = committed else {
        panic!("{}: missing or unreadable golden; {bless}", path.display());
    };
    let Some(d) = olr_render::perceptual_diff(&committed, &img, TOLERANCE) else {
        panic!(
            "{name}: the size is {}x{}, not {}x{}; the actual image is {}; {bless}",
            img.width(),
            img.height(),
            committed.width(),
            committed.height(),
            actual.display()
        );
    };
    assert!(
        d.fraction() <= MAX_DIFFERING,
        "{name}: {} of {} pixels differ (max distance {:.3}) from {}; the actual image is {}; {bless}",
        d.differing,
        d.total,
        d.max_distance,
        path.display(),
        actual.display()
    );
}

/// Dirt Level: digging through solid dirt, interpolated half way between
/// two ticks and upscaled 3× (sharp bilinear over a sub-pixel camera).
#[test]
fn offscreen_renders_dig_frame_like_golden() {
    golden("frame-dig.png", "dig", 200, &["--scale", "3", "--alpha", "0.5"]);
}

/// Liero Factory: swinging on the ninja rope.
#[test]
fn offscreen_renders_rope_frame_like_golden() {
    golden("frame-rope.png", "rope", 160, &[]);
}

/// FossilFacility: a bazooka hit, blood and smoke.
#[test]
fn offscreen_renders_bazooka_frame_like_golden() {
    golden("frame-bazooka.png", "weapons/classic/bazooka", 56, &[]);
}

/// CastleStrike, the 1× level: the rope to the hall's ceiling, integer
/// nearest at 2×.
#[test]
fn offscreen_renders_castle_frame_like_golden() {
    golden("frame-castle.png", "castle-shoot-rope", 200, &["--filter", "integer-nearest"]);
}

/// The look test level lit: sky, windows, lava, two lamps, dark ambient,
/// a grade (plan 15a Task 14).
#[test]
fn look_test_level_lit() {
    look_golden("look-lit.png", &[]);
}

/// Its light alone.
#[test]
fn look_test_level_light() {
    look_golden("look-light.png", &["--view", "light"]);
}

/// Full bright: the art as it is.
#[test]
fn look_test_level_full() {
    look_golden("look-full.png", &["--view", "full"]);
}

/// Half a second on: the lava flickers and the blue lamp pulses.
#[test]
fn look_test_level_at_500_ms() {
    look_golden("look-t500.png", &["--time", "500"]);
}

/// Issue #10: a copy of the look test level with ambient occlusion
/// (strength 60, reach 12): its tunnels and the air by its rock darker.
#[test]
fn look_test_level_occluded() {
    look_golden_of("look-test-occlusion", "look-occlusion.png", &[]);
}

/// Its light alone: the occlusion shows plainly.
#[test]
fn look_test_level_occluded_light() {
    look_golden_of("look-test-occlusion", "look-occlusion-light.png", &["--view", "light"]);
}

/// Issue #11: `look-preview` of the distant demo level (a copy of the look
/// test level, `fixtures/look/levels/look-test-distant`: its base split,
/// two gaps erased through every layer, a night sky at depth 90 and a far
/// city at depth 55, both images, `level element add-image … --material
/// background` then `set --distant N`) centred on `at`.
fn distant_golden(name: &str, at: &str, extra: &[&str]) {
    let Some(content) = content_dir() else { return };
    let pack = root().join("fixtures/look/levels/look-test-distant");
    compare(name, &["look-preview", pack.to_str().unwrap(), "--at", at, "--content", &content], extra);
}

/// The camera at the level's top-left as far as the view goes: the city
/// and the moon shifted by their depths, lit by the dark ambient.
#[test]
fn distant_layers_top_left() {
    distant_golden("look-distant-a.png", "213,96", &[]);
}

/// The camera at its bottom-right: the layers moved less than the level.
#[test]
fn distant_layers_bottom_right() {
    distant_golden("look-distant-b.png", "299,160", &[]);
}

/// Full bright: the layers' own colours.
#[test]
fn distant_layers_full_bright() {
    distant_golden("look-distant-full.png", "213,96", &["--view", "full"]);
}

/// A chaos-bot match on the look test level with the look test mod's
/// Glow Rocket (plan 15a Task 16): rockets with their light, end flashes,
/// muzzle and explosion lights, the worms' floor.
#[test]
fn look_rocket() {
    let Some(content) = content_dir() else { return };
    let (mods, levels) = (root().join("fixtures/look/mods"), root().join("fixtures/look/levels"));
    let args = [
        "render-match",
        "--mod",
        "look-test-mod",
        "--level",
        "look-test",
        "--bots",
        "3",
        "--personality",
        "chaos",
        "--seed",
        "1",
        "--ticks",
        "300",
        "--packs",
        mods.to_str().unwrap(),
        "--packs",
        levels.to_str().unwrap(),
        "--content",
        &content,
    ];
    compare("look-rocket.png", &args, &[]);
}

/// Issue #12: idle bots on the dark skin test level (`fixtures/look/levels/
/// look-test-skin-level`) wearing the hand-made double-resolution skin
/// (`fixtures/look/skins/look-test-skin`, art scale 4): its cyan visor
/// glows, its light lights the air and the rock pillar stops it. At
/// `--scale 4`, one output pixel per skin art pixel.
#[test]
fn look_double_skin() {
    let Some(content) = content_dir() else { return };
    let (skins, levels) = (root().join("fixtures/look/skins"), root().join("fixtures/look/levels"));
    let args = [
        "render-match",
        "--mod",
        "olx-classic",
        "--level",
        "look-test-skin-level",
        "--bots",
        "2",
        "--personality",
        "idle",
        "--seed",
        "4",
        "--ticks",
        "500",
        "--skin",
        "look-test-skin",
        "--scale",
        "4",
        "--packs",
        skins.to_str().unwrap(),
        "--packs",
        levels.to_str().unwrap(),
        "--content",
        &content,
    ];
    compare("look-skin.png", &args, &[]);
}
