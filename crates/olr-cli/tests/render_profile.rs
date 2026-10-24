//! The render profile's staged nuke fight (its spec
//! `docs/superpowers/specs/2026-10-24-render-profile-design.md`): it sets
//! off nukes early, the same every time, and refuses a fight without any.

use std::path::PathBuf;

use olr_cli::render_profile::{FIGHT_TICKS, NUKES, stage_fight, stage_with_weapons};
use olr_content::Content;

/// The bundled content: the Reloaded mod and level over the committed packs.
fn content() -> Content {
    let packs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../content/packs");
    Content::open_bundled(&packs).unwrap()
}

#[test]
fn the_nuke_fight_sets_off_nukes_and_is_the_same_every_time() {
    let content = content();
    let a = stage_fight(&content).unwrap();
    assert!(a.explosions >= 20, "{}", a.explosions);
    // Only nukes are fired: the worms keep the nuke loadout all fight.
    assert!(a.shots.iter().all(|(w, _)| NUKES.contains(&w.as_str())), "{:?}", a.shots);
    assert!(a.shots.iter().map(|(_, n)| n).sum::<u32>() >= 10, "{:?}", a.shots);
    assert_eq!(a.blasts.len() as u32, a.explosions);
    assert!(a.blasts.windows(2).any(|w| w[1] - w[0] > 100), "a hot stretch between blasts: {:?}", a.blasts);
    let first = a.first_explosion.unwrap();
    assert!(first < FIGHT_TICKS / 4, "nukes early enough for a calm phase and a hot one: {first}");
    let b = stage_fight(&content).unwrap();
    assert_eq!(a.replay.checksums, b.replay.checksums);
}

#[test]
fn a_fight_without_nukes_is_refused() {
    let err = stage_with_weapons(&content(), &["Laser"]).unwrap_err().to_string();
    assert!(err.contains("set off no nukes"), "{err}");
}
