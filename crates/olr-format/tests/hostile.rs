//! Malformed and hostile mod files: errors, never panics.

use olr_format::mods::{compile_mod, read_mod_files};
use olr_vfs::MemFs;
use proptest::prelude::*;

const MOD: &str = "(name: \"T\", weapons: [\"w\"])";
const WEAPON: &str = r##"(
    name: "W",
    recharge: 0.5,
    drain: 30.0,
    between_shots_s: 0.25,
    fire: Projectile(
        shot: (
            speed: 100,
            projectile: (
                gfx: Pixels(["#948800"]),
                hit_terrain: [Explode(damage: 4), Spawn("s")],
                spawns: {"s": (amount: 3, projectile: (gfx: Pixels([])))},
            ),
        ),
    ),
)
"##;

#[test]
fn the_fixture_reads_and_compiles() {
    let fs = MemFs::new().with("mod.ron", MOD).with("weapons/w.ron", WEAPON);
    compile_mod(&read_mod_files(&fs).unwrap()).unwrap();
}

#[test]
fn two_hundred_nested_projectiles_are_an_error_not_a_crash() {
    let mut p = String::from("(gfx: Pixels([]))");
    for _ in 0..200 {
        p = format!("(gfx: Pixels([]), spawns: {{\"s\": (projectile: {p})}})");
    }
    let w = format!(
        "(name: \"W\", recharge: 1.0, drain: 1.0, between_shots_s: 1.0, fire: Projectile(shot: (projectile: {p})))"
    );
    let fs = MemFs::new().with("mod.ron", MOD).with("weapons/w.ron", w);
    assert!(read_mod_files(&fs).is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn mutated_files_never_panic(edits in proptest::collection::vec((0usize..WEAPON.len(), any::<u8>()), 1..8)) {
        let mut bytes = WEAPON.as_bytes().to_vec();
        for (at, b) in edits {
            bytes[at] = b;
        }
        let fs = MemFs::new().with("mod.ron", MOD).with("weapons/w.ron", bytes);
        if let Ok(src) = read_mod_files(&fs) {
            let _ = compile_mod(&src);
        }
    }
}

/// An asset with states (issue #71): a stacked sheet, a glow mask, sounds.
const ASSET: &str = r##"(
    name: "Desk",
    size: (2, 2),
    elements: [
        (
            id: "screen",
            kind: Drawn(
                material: Background,
                size: (2, 2),
            ),
            glow: (
                strength: 80,
                group: "screens",
                mask: true,
            ),
        ),
    ],
    glow_groups: [
        (
            id: "screens",
            colour: Tint("#40ff80"),
            reach: Pixels(12),
        ),
    ],
    states: [
        (
            name: "Normal",
        ),
        (
            name: "Alarm",
            own: [
                "screen",
            ],
            values: [
                (
                    property: GlowColour("screen"),
                    value: Colour("#ff2020"),
                ),
            ],
            loop_sound: "alarm.ogg",
        ),
    ],
    exposed: [
        (
            name: "glow",
            property: GlowStrength("screen"),
            default: Int(50),
            min: 0,
            max: 100,
        ),
    ],
    sounds: [
        "alarm.ogg",
    ],
    health: (
        points: 20,
        at_zero: "Alarm",
    ),
)
"##;

fn asset_fs(ron: &[u8], sheet: &[u8]) -> MemFs {
    use olr_data::image::RgbaImage;
    use olr_format::image_io;
    MemFs::new()
        .with("asset.ron", ron.to_vec())
        .with("layers/screen.png", sheet.to_vec())
        .with("glow/screen.png", image_io::encode_grey(2, 4, &[9; 8]))
        .with("sounds/alarm.ogg", b"OggS".to_vec())
        .with("script.rhai", "fn x() {}".to_string())
        .with("unused", image_io::encode_rgba(&RgbaImage::new(1, 1)))
}

fn sheet() -> Vec<u8> {
    olr_format::image_io::encode_rgba(&olr_data::image::RgbaImage::new(2, 4))
}

#[test]
fn the_asset_fixture_reads_and_checks() {
    use olr_format::assets::{check_asset, read_asset_files};
    let src = read_asset_files(&asset_fs(ASSET.as_bytes(), &sheet())).unwrap();
    check_asset(&src).unwrap();
    assert_eq!(src.file.states.len(), 2);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn mutated_asset_files_never_panic(
        edits in proptest::collection::vec((0usize..ASSET.len(), any::<u8>()), 1..8),
        sheet_edits in proptest::collection::vec((0usize..64, any::<u8>()), 0..6),
    ) {
        use olr_format::assets::{check_asset, check_own_asset, read_asset_files};
        let mut bytes = ASSET.as_bytes().to_vec();
        for (at, b) in edits {
            bytes[at] = b;
        }
        let mut png = sheet();
        for (at, b) in sheet_edits {
            let at = at % png.len();
            png[at] = b;
        }
        if let Ok(src) = read_asset_files(&asset_fs(&bytes, &png)) {
            let _ = check_asset(&src);
            let _ = check_own_asset(&src);
        }
    }
}

/// A placement (issue #73): the asset above placed twice, one Live with
/// its own script, a level script.
const PLACED: &str = r##"(
    assets: [
        (
            key: "a",
            origin: Yours("desk"),
        ),
    ],
    copies: [
        (
            id: "desk-1",
            asset: "a",
            at: (2, 3),
        ),
        (
            id: "desk-2",
            asset: "a",
            at: (-4, 1),
            size: (4, 3),
            live: true,
            state: "Alarm",
            values: {
                "glow": Int(20),
            },
            script: true,
            default_script: false,
        ),
    ],
    script: true,
)
"##;

fn placed_fs(placed: &[u8]) -> MemFs {
    let mut fs = MemFs::new()
        .with("editor/placed.ron", placed.to_vec())
        .with("editor/scripts/desk-2.rhai", "fn x() {}".to_string())
        .with("editor/level.rhai", "// level".to_string());
    let asset = asset_fs(ASSET.as_bytes(), &sheet());
    for p in olr_vfs::walk(&asset, &olr_vfs::VPath::root()).unwrap() {
        let to = olr_vfs::VPath::new(&format!("editor/a/{}", p.as_str())).unwrap();
        olr_vfs::ContentSink::write(&mut fs, &to, &olr_vfs::ContentSource::read(&asset, &p).unwrap())
            .unwrap();
    }
    fs
}

#[test]
fn the_placed_fixture_reads_and_checks() {
    use olr_format::levels::{check_placed, read_placed_files};
    let src = read_placed_files(&placed_fs(PLACED.as_bytes())).unwrap().unwrap();
    check_placed(&src, (16, 16), 2).unwrap();
    assert_eq!(src.file.copies.len(), 2);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]
    #[test]
    fn hostile_placements_are_errors(
        edits in proptest::collection::vec((0usize..PLACED.len(), any::<u8>()), 1..8),
    ) {
        use olr_format::levels::{check_placed, read_placed_files, write_placed_files};
        let mut bytes = PLACED.as_bytes().to_vec();
        for (at, b) in edits {
            bytes[at] = b;
        }
        if let Ok(Some(src)) = read_placed_files(&placed_fs(&bytes)) {
            let _ = check_placed(&src, (16, 16), 2);
            let _ = write_placed_files(&mut MemFs::new(), &src);
        }
    }
}
