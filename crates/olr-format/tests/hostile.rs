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
