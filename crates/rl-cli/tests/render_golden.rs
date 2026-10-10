//! GPU render goldens (plan 5 task 5): `rl-cli render <scenario> --frame N`
//! of one scenario on each level must look like its committed PNG in
//! `fixtures/render/` (a perceptual diff, so two software rasterisers that
//! round a blend differently still agree).
//!
//! The goldens are drawn from the imported OLX content at test time (spec
//! §9.3: converted assets are never committed, the goldens are accepted
//! fixtures); they were blessed on lavapipe (Vulkan, CPU), the adapter
//! `rl_render::Offscreen` prefers. They run in `cargo xtask full`, which
//! sets `RL_GPU_TESTS`; without it, the content or an adapter they skip
//! with a notice. `cargo xtask bless-render --reason "<why>"` re-draws
//! them (`RL_BLESS_RENDER`).

use std::path::{Path, PathBuf};
use std::process::Command;

use rl_data::image::RgbaImage;

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
    std::env::var_os("RL_BLESS_RENDER").is_some()
}

/// The content directory, if GPU tests are on and the content is imported.
fn content_dir() -> Option<String> {
    if std::env::var_os("RL_GPU_TESTS").is_none() && !blessing() {
        eprintln!("RL_GPU_TESTS not set (`cargo xtask full` sets it); skipping");
        return None;
    }
    let dir = std::env::var("RL_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir());
    if dir.is_none() {
        assert!(!blessing(), "blessing needs the imported content (`cargo xtask fetch-olx-content`)");
        eprintln!("RL_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
    }
    dir
}

/// Renders `scenario` at `frame` with `extra` options into
/// `target/render-actual/<name>` and compares it with
/// `fixtures/render/<name>`, or writes the golden when blessing.
fn golden(name: &str, scenario: &str, frame: i64, extra: &[&str]) {
    let Some(content) = content_dir() else { return };
    let actual = root().join("target/render-actual").join(name);
    std::fs::create_dir_all(actual.parent().unwrap()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_rl-cli"))
        .args(["render", scenario, "--frame", &frame.to_string(), "--content", &content, "--fixtures"])
        .arg(root().join("fixtures/oracle"))
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
    assert!(out.status.success(), "rl-cli render {scenario} failed:\n{stderr}");
    eprint!("{}", String::from_utf8_lossy(&out.stdout));
    let decode = |p: &Path| std::fs::read(p).ok().and_then(|b| rl_pack::image_io::decode_rgba(&b).ok());
    let img: RgbaImage = decode(&actual).expect("rl-cli wrote a PNG");
    let path = root().join("fixtures/render").join(name);
    let committed = decode(&path);
    if blessing() {
        // Keep the committed file when it still passes, so a re-bless on
        // another rasteriser does not churn the goldens.
        let same = committed.as_ref().and_then(|c| rl_render::perceptual_diff(c, &img, TOLERANCE));
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
    let Some(d) = rl_render::perceptual_diff(&committed, &img, TOLERANCE) else {
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
