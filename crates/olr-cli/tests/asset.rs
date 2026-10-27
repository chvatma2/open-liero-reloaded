//! `olr-cli asset save|check` (issue #9) and the built-in assets in
//! `content/assets/`: each reads whole and is what its command in the
//! README makes; the torch carries Castle Strike Reloaded's `flames`, not
//! its `sky`. Placing one is a copy now (issue #75, `copy.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn assets_dir() -> PathBuf {
    repo().join("content/assets")
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_olr-cli")).current_dir(repo()).args(args).output().unwrap()
}

fn text(o: &std::process::Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// The asset folders of `content/assets`, sorted.
fn built_in() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(assets_dir()).unwrap().map(|e| e.unwrap().path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

#[test]
fn built_in_assets_check() {
    let dirs = built_in();
    assert!(dirs.len() >= 3, "{dirs:?}");
    let mut args = vec!["asset", "check"];
    args.extend(dirs.iter().map(|d| d.to_str().unwrap()));
    let o = cli(&args);
    assert!(o.status.success(), "{}", text(&o));
    // And as the editor's Assets tab lists them: named, with a thumbnail.
    let (list, warnings) = olr_edit::asset::list_library(&assets_dir(), usize::MAX);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(list.len(), dirs.len());
    assert!(list.iter().all(|(_, s)| s.thumb.is_some() && s.glows), "every starter asset glows");
}

/// Every file under `dir`, by `/`-separated relative path, with its bytes.
pub(crate) fn tree(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fn go(base: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                go(base, &p, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_str().unwrap().replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    go(dir, dir, &mut out);
    out
}

/// Splits a README command line into its arguments (double quotes group).
fn words(line: &str) -> Vec<String> {
    let (mut out, mut word, mut quoted) = (Vec::new(), String::new(), false);
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            c => word.push(c),
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

#[test]
fn built_in_assets_are_what_their_commands_make() {
    let readme = std::fs::read_to_string(assets_dir().join("README.md")).unwrap();
    let commands: Vec<Vec<String>> =
        readme.lines().map(str::trim).filter(|l| l.starts_with("olr-cli asset save ")).map(words).collect();
    assert_eq!(commands.len(), built_in().len(), "one command per asset");
    let t = tempfile::tempdir().unwrap();
    for c in commands {
        let out = c.iter().position(|w| w == "--out").map(|i| c[i + 1].clone()).unwrap();
        let fresh = t.path().join(Path::new(&out).file_name().unwrap());
        let mut args: Vec<&str> = c[1..].iter().map(String::as_str).collect();
        let i = args.iter().position(|w| *w == "--out").unwrap();
        args[i + 1] = fresh.to_str().unwrap();
        let o = cli(&args);
        assert!(o.status.success(), "{c:?}: {}", text(&o));
        assert_eq!(tree(&fresh), tree(&repo().join(&out)), "{out}");
    }
}

#[test]
fn the_torch_carries_flames_not_sky() {
    let torch = olr_edit::asset::load_dir(&assets_dir().join("torch")).unwrap();
    let groups: Vec<&str> = torch.groups.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(groups, ["flames"]);
    assert!(matches!(torch.groups[0].animation, olr_format::look::AnimationSource::Flicker { .. }));
    assert!(torch.pieces.iter().all(|p| p.element.id != "background-2"), "no duplicate background");
    assert!(
        torch.pieces.iter().any(|p| p.element.glow.as_ref().is_some_and(|g| g.group == "flames" && g.mask))
    );
    // Without --layers, a rectangle reaching past the torch's hole cuts
    // `background-2` too (what the README's --layers avoids); its `sky`
    // mask is dark in the cavern, so its glow and group stay behind.
    let t = tempfile::tempdir().unwrap();
    let out = t.path().join("wide");
    let o = cli(&[
        "asset",
        "save",
        "content/reloaded/olr-level-castlestrike-reloaded",
        "--rect",
        "200,420,20,22",
        "--name",
        "Wide torch",
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let wide = olr_edit::asset::load_dir(&out).unwrap();
    let ids: Vec<&str> = wide.pieces.iter().map(|p| p.element.id.as_str()).collect();
    assert_eq!(ids, ["background", "background-2"]);
    assert_eq!(wide.pieces[1].element.glow, None);
    assert!(wide.groups.iter().all(|g| g.id != "sky"), "{:?}", wide.groups);
}

#[test]
fn cli_refuses_bad_assets_and_arguments() {
    let t = tempfile::tempdir().unwrap();
    let bad = t.path().join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("asset.ron"), "(name: \"X\", size: (0, 0), elements: [])").unwrap();
    let o = cli(&["asset", "check", "content/assets/torch", bad.to_str().unwrap()]);
    assert!(!o.status.success());
    assert!(text(&o).contains("bad: error:"), "{}", text(&o));
    // One of --elements and --rect.
    let o = cli(&[
        "asset",
        "save",
        "content/reloaded/olr-level-castlestrike-reloaded",
        "--name",
        "X",
        "--out",
        "x",
    ]);
    assert!(!o.status.success());
    let o = cli(&[
        "asset",
        "save",
        "content/reloaded/olr-level-castlestrike-reloaded",
        "--elements",
        "rock",
        "--rect",
        "0,0,4,4",
        "--name",
        "X",
        "--out",
        "x",
    ]);
    assert!(!o.status.success());
    assert!(!repo().join("x").exists());
    // A rectangle off the level.
    let o = cli(&[
        "asset",
        "save",
        "content/reloaded/olr-level-castlestrike-reloaded",
        "--rect",
        "5000,0,4,4",
        "--name",
        "X",
        "--out",
        t.path().join("off").to_str().unwrap(),
    ]);
    assert!(!o.status.success() && text(&o).contains("no pixels"), "{}", text(&o));
}

/// Issue #71: `asset check` says how many states an asset has after its
/// elements, and nothing more for a stateless one.
#[test]
fn asset_check_counts_states() {
    let t = tempfile::tempdir().unwrap();
    let mut torch = olr_edit::asset::load_dir(&assets_dir().join("torch")).unwrap();
    let dir = t.path().join("torch");
    let check = || {
        let o = cli(&["asset", "check", dir.to_str().unwrap()]);
        assert!(o.status.success(), "{}", text(&o));
        String::from_utf8_lossy(&o.stdout).replace(dir.to_str().unwrap(), "torch")
    };
    olr_edit::asset::save_dir(&dir, &olr_edit::asset::encode(&torch).unwrap()).unwrap();
    assert_eq!(check(), "torch: \"Torch\", 14 × 16, 1 element, materials stone, wood\n");
    torch.states.push(olr_edit::asset::State::new("Out"));
    torch.states.push(olr_edit::asset::State::new("Burnt"));
    olr_edit::asset::save_dir(&dir, &olr_edit::asset::encode(&torch).unwrap()).unwrap();
    assert_eq!(check(), "torch: \"Torch\", 14 × 16, 1 element, 3 states, materials stone, wood\n");
}

// Issue #72: the commands that edit an asset folder in place.

/// `olr-cli <args>` succeeding: its stdout.
fn ok(args: &[&str]) -> String {
    let o = cli(args);
    assert!(o.status.success(), "{args:?}: {}", text(&o));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A WAV of `data` silent bytes at 8000 bytes a second.
fn wav(data: u32) -> Vec<u8> {
    let mut b = b"RIFF".to_vec();
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 1, 0]);
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&8000u32.to_le_bytes());
    b.extend_from_slice(&[1, 0, 8, 0]);
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data as usize, 128);
    b
}

/// A `w` × `h` PNG of one colour.
fn png(path: &Path, w: u32, h: u32, c: olr_data::image::Rgba) {
    let mut img = olr_data::image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    std::fs::write(path, olr_format::image_io::encode_rgba(&img)).unwrap();
}

/// A strip lamp made in `t`: a level the test writes (a rock strip, a
/// glowing background lamp under it in group `lamps`), saved as an asset
/// by its elements, then given states On and Off (Off painting the lamp
/// dark, unlit and ticking), an exposed glow and a script. Returns the
/// asset folder.
pub(crate) fn build_lamp(t: &Path) -> PathBuf {
    let level = t.join("level");
    let l = level.to_str().unwrap();
    ok(&["pack", "new", "level", "lamp-level", "--out", l, "--size", "40x20"]);
    ok(&["level", "group", "add", l, "lamps", "--reach", "40", "--colour", "ffd080"]);
    ok(&["level", "element", "add-layer", l, "--material", "rock", "--name", "Strip"]);
    ok(&["level", "element", "draw", l, "strip", "--rect", "0,0,20,4", "--colour", "808080"]);
    ok(&["level", "element", "add-layer", l, "--material", "background", "--name", "Lamp"]);
    ok(&["level", "element", "draw", l, "lamp", "--rect", "2,4,16,3", "--colour", "ffe080"]);
    ok(&["level", "element", "set", l, "lamp", "--glow", "100", "--glow-group", "lamps"]);
    let dir = t.join("strip-lamp");
    let d = dir.to_str().unwrap();
    ok(&["asset", "save", l, "--elements", "strip,lamp", "--name", "Strip lamp", "--out", d]);
    assert_eq!(ok(&["asset", "state", "add", d, "On"]), "added state On\n");
    assert_eq!(ok(&["asset", "state", "add", d, "Off"]), "added state Off\n");
    let dark = t.join("dark.png");
    png(&dark, 16, 3, olr_data::image::Rgba::rgb(20, 20, 20));
    ok(&["asset", "state", "picture", d, "Off", "lamp", "--art", dark.to_str().unwrap()]);
    ok(&["asset", "state", "value", d, "Off", "lamp.glow", "0"]);
    let tick = t.join("Tick.WAV");
    std::fs::write(&tick, wav(800)).unwrap();
    assert_eq!(ok(&["asset", "sound", "add", d, tick.to_str().unwrap()]), "added sound tick.wav: 0.1 s\n");
    ok(&["asset", "state", "set", d, "Off", "--enter-sound", "tick.wav"]);
    ok(&["asset", "expose", d, "lamps_glow", "lamp.glow"]);
    let script = t.join("lamp.rhai");
    std::fs::write(&script, "fn on_enter(state) {\n    print(state);\n}\n").unwrap();
    ok(&["asset", "script", d, "--set", script.to_str().unwrap()]);
    dir
}

#[test]
fn cli_builds_a_lamp_with_states_sounds_and_a_script() {
    let t = tempfile::tempdir().unwrap();
    let dir = build_lamp(t.path());
    let d = dir.to_str().unwrap();
    assert!(ok(&["asset", "check", d]).ends_with(": \"Strip lamp\", 20 × 7, 2 elements, 3 states\n"));
    assert_eq!(
        ok(&["asset", "info", d]),
        "\"Strip lamp\", 20 × 7, art scale 1, 2 elements\n\
         layer strip: rock\n\
         layer lamp: background, glows in lamps\n\
         state Normal ★\n\
         state On\n\
         state Off: owns lamp; lamp.glow = 0; enters tick.wav\n\
         exposed lamps_glow: lamp.glow, default 100, 0 to 100\n\
         sound tick.wav: 0.1 s\n\
         script: 3 lines\n"
    );
    assert!(dir.join("sounds/tick.wav").is_file() && dir.join("script.rhai").is_file());
    // Off's picture is stacked below the start's.
    let sheet = std::fs::read(dir.join("layers/lamp.png")).unwrap();
    assert_eq!(olr_format::image_io::png_size(&sheet), Some((16, 6)));
    // Off as the start: the thumbnail darkens, and every state looks as it did.
    let before = olr_edit::asset::load_dir(&dir).unwrap();
    let thumb = std::fs::read(dir.join("thumb.png")).unwrap();
    assert_eq!(
        ok(&["asset", "state", "set", d, "Off", "--start", "--name", "Dark"]),
        "state Off is the start\nrenamed state Off to Dark\n"
    );
    let after = olr_edit::asset::load_dir(&dir).unwrap();
    assert_eq!(after.states[after.start].name, "Dark");
    for s in 0..3 {
        let none = std::collections::BTreeMap::new();
        assert_eq!(
            olr_edit::asset::posed(&after, s, &none).unwrap(),
            olr_edit::asset::posed(&before, s, &none).unwrap()
        );
    }
    assert_ne!(std::fs::read(dir.join("thumb.png")).unwrap(), thumb);
    assert!(ok(&["asset", "info", d]).contains(
        "state Normal: owns lamp\nstate On: owns lamp\nstate Dark ★: lamp.glow = 0; enters tick.wav\n"
    ));
    // The same edits give the same bytes.
    let u = tempfile::tempdir().unwrap();
    let again = build_lamp(u.path());
    let a = olr_edit::asset::load_dir(&again).unwrap();
    assert_eq!(tree(&again).len(), 6);
    assert_eq!(a.states.len(), 3);
    ok(&["asset", "state", "set", again.to_str().unwrap(), "Off", "--name", "Dark", "--start"]);
    assert_eq!(tree(&again), tree(&dir));
}

#[test]
fn cli_refuses_and_leaves_the_folder() {
    let t = tempfile::tempdir().unwrap();
    let dir = build_lamp(t.path());
    let d = dir.to_str().unwrap();
    let before = tree(&dir);
    let small = t.path().join("small.png");
    png(&small, 4, 3, olr_data::image::Rgba::rgb(1, 2, 3));
    let huge = t.path().join("huge.wav");
    std::fs::write(&huge, vec![0u8; 3 << 20]).unwrap();
    let cases: Vec<(Vec<&str>, Vec<&str>)> = vec![
        (vec!["asset", "state", "value", d, "Of", "lamp.glow", "1"], vec!["no state \"Of\""]),
        (
            vec!["asset", "state", "value", d, "Off", "lamp.glow", "true"],
            vec!["\"Off\"", "lamp.glow", "true is not a whole number"],
        ),
        (
            vec!["asset", "state", "value", d, "Off", "roof.glow", "1"],
            vec!["\"Off\"", "roof.glow", "no layer \"roof\""],
        ),
        (vec!["asset", "state", "value", d, "Off", "strip.glow", "1"], vec!["strip.glow", "does not glow"]),
        (
            vec!["asset", "state", "remove", d, "Normal"],
            vec!["\"Normal\"", "make another state the start first"],
        ),
        (
            vec!["asset", "state", "picture", d, "Off", "lamp", "--art", small.to_str().unwrap()],
            vec!["\"lamp\"", "4 × 3", "16 × 3"],
        ),
        (
            vec!["asset", "state", "picture", d, "Normal", "lamp", "--art", small.to_str().unwrap()],
            vec!["\"Normal\"", "starting state"],
        ),
        (vec!["asset", "state", "picture", d, "Off", "lamp", "--art", "missing.png"], vec!["missing.png"]),
        (vec!["asset", "sound", "remove", d, "tick.wav"], vec!["\"tick.wav\"", "\"Off\""]),
        (vec!["asset", "sound", "add", d, huge.to_str().unwrap()], vec!["huge.wav", "more than"]),
        (vec!["asset", "state", "set", d, "On", "--loop-sound", "bell.wav"], vec!["\"On\"", "\"bell.wav\""]),
        (vec!["asset", "state", "add", d, "off"], vec!["\"off\"", "\"Off\""]),
        (vec!["asset", "expose", d, "lamps_glow", "visible"], vec!["\"lamps_glow\"", "already"]),
        (vec!["asset", "expose", d, "strip_x", "strip.x"], vec!["strip.x", "min and a max"]),
        (
            vec!["asset", "expose", d, "glow_2", "lamp.glow", "--default", "#ff0000"],
            vec!["lamp.glow", "not a whole number"],
        ),
        (vec!["asset", "unexpose", d, "nothing"], vec!["\"nothing\""]),
        (vec!["asset", "health", d, "--points", "10", "--at-zero", "Broken"], vec!["no state \"Broken\""]),
    ];
    for (args, words) in cases {
        let o = cli(&args);
        let e = String::from_utf8_lossy(&o.stderr);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", text(&o));
        assert!(e.starts_with(&format!("{d}: ")), "{args:?}: {e}");
        for w in words {
            assert!(e.contains(w), "{args:?}: {w:?} not in {e}");
        }
        assert_eq!(tree(&dir), before, "{args:?} left the folder as it was");
    }
    // A bad argument is clap's refusal, the folder untouched too.
    let o = cli(&["asset", "state", "value", d, "Off", "lamp.colour", "1"]);
    assert!(!o.status.success() && text(&o).contains("<layer>"), "{}", text(&o));
    assert_eq!(tree(&dir), before);
}
