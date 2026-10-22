//! `olr-cli look-preview` (plan 15a Task 13): a level with its look drawn
//! to a PNG. GPU tier (`OLR_GPU_TESTS`) and the imported content (the
//! theme); skips without them.

use std::path::Path;
use std::process::Command;

use crate::common::{content_dir, copy_dir};

#[test]
fn look_preview_writes_a_png() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let Some(content) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let pack = tmp.path().join("dirt");
    copy_dir(&Path::new(&content).join("olx-level-dirt-level"), &pack);
    let run = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_olr-cli")).args(args).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    let p = pack.to_str().unwrap();
    run(&["level", "look", p, "--ambient", "ffffff:20"]);
    // A glowing patch (plan 15c: glowing pixels are a level's only light).
    run(&["level", "group", "add", p, "lamp", "--colour", "ffcc88", "--intensity", "150", "--reach", "120"]);
    run(&["level", "element", "split", p, "base", "--content", &content]);
    let set = ["level", "element", "set", p, "background", "--glow", "100", "--glow-group", "lamp"];
    run(&[&set[..], &["--content", &content]].concat());
    let glow = ["level", "element", "glow", p, "background", "--rect", "0,0,1,1", "--strength", "0"];
    run(&[&glow[..], &["--content", &content]].concat());
    let patch = ["level", "element", "glow", p, "background", "--circle", "250,175,4", "--strength", "100"];
    run(&[&patch[..], &["--content", &content]].concat());
    let out = tmp.path().join("lit.png");
    let o = out.to_str().unwrap();
    run(&["look-preview", p, "--at", "250,175", "--content", &content, "--out", o]);
    let lit = olr_format::image_io::decode_rgba(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!((lit.width(), lit.height()), (854, 384));
    let full = tmp.path().join("full.png");
    run(&[
        "look-preview",
        p,
        "--at",
        "250,175",
        "--view",
        "full",
        "--content",
        &content,
        "--out",
        full.to_str().unwrap(),
    ]);
    let full = olr_format::image_io::decode_rgba(&std::fs::read(&full).unwrap()).unwrap();
    // The corner is far from the patch: darker lit than full bright.
    let (a, b) = (lit.get(4, 4), full.get(4, 4));
    assert!(u32::from(a.r) + u32::from(a.g) < u32::from(b.r) + u32::from(b.g), "{a:?} {b:?}");
    // The patch's own pixel is lit.
    let c = lit.get(427, 192);
    assert!(c.r > 0, "{c:?}");
}

/// `render-match` plays bots on bundled packs (no look: no lights) and
/// writes the frame.
#[test]
fn render_match_writes_a_png() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let Some(content) = content_dir() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("m.png");
    let o = Command::new(env!("CARGO_BIN_EXE_olr-cli"))
        .args(["render-match", "--mod", "olx-classic", "--level", "olx-level-dirt-level", "--bots", "2"])
        .args(["--seed", "1", "--ticks", "30", "--content", &content, "--out", out.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(stdout.contains("tick 30, 0 glowing discs"), "{stdout}");
    assert!(out.is_file());
    // Issue #29: local players' worms (each with its crosshair) render too;
    // issue #30: each in its own view, the solo view's size, a seam apart.
    for (locals, size) in [(2, (854, 772)), (3, (1712, 772)), (4, (1712, 772))] {
        let o = Command::new(env!("CARGO_BIN_EXE_olr-cli"))
            .args(["render-match", "--mod", "olx-classic", "--level", "olx-level-dirt-level", "--bots", "1"])
            .args(["--locals", &locals.to_string(), "--seed", "1", "--ticks", "30", "--content", &content])
            .args(["--out", out.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let stdout = String::from_utf8_lossy(&o.stdout);
        assert!(stdout.contains(&format!("({}x{}, {locals} views,", size.0, size.1)), "{stdout}");
        let png = std::fs::read(&out).unwrap();
        let be = |at: usize| u32::from_be_bytes([png[at], png[at + 1], png[at + 2], png[at + 3]]);
        assert_eq!((be(16), be(20)), size, "the PNG's size");
    }
}
