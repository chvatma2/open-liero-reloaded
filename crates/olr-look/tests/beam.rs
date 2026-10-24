//! A beam weapon's look (issue #69): the engine's lights and heat, a
//! weapon's own over them.

use olr_look::{BeamHeat, BeamLook, ModLook, Reach, WeaponLook, engine, srgb_to_linear};

/// The engine's tip light is the beam's colour × 2.8 + 0.2 (linear): the
/// red Laser's (3.0, 0.2, 0.2); the light along the beam its colour at
/// 21 %.
#[test]
fn the_engines_beam_lights_are_the_picked_frames() {
    let along = engine::beam_light([255, 0, 0]);
    assert_eq!((along.colour, along.intensity, along.reach), ([255, 0, 0], 21, Reach::Pixels(40)));
    let tip = engine::beam_tip_light([255, 0, 0]);
    assert_eq!(tip.reach, Reach::Pixels(90));
    let lit = tip.colour.map(|c| srgb_to_linear(c) * f32::from(tip.intensity) / 100.0);
    for (got, want) in lit.iter().zip([3.0, 0.2, 0.2]) {
        assert!((got - want).abs() < 0.01, "{lit:?}");
    }
    // Blue (MW 1.0's Laser) is whitened the same way.
    let blue = engine::beam_tip_light([0, 0, 255]);
    let lit = blue.colour.map(|c| srgb_to_linear(c) * f32::from(blue.intensity) / 100.0);
    assert!((lit[2] - 3.0).abs() < 0.01 && (lit[0] - 0.2).abs() < 0.01, "{lit:?}");
}

/// A weapon's own beam heat replaces the engine's; a wide beam splits its
/// sub-step's heat over its parts; 0 is none; the caps hold.
#[test]
fn beam_heat_is_the_weapons_else_the_engines() {
    let mut m = ModLook::default();
    assert_eq!(m.beam_heat(0), engine::BEAM_HEAT);
    let own = BeamHeat { amount: 30, radius: 3 };
    let beam = BeamLook { heat: Some(own), ..BeamLook::default() };
    m.weapons = vec![None, Some(WeaponLook { muzzle_light: None, beam: Some(beam) })];
    assert_eq!((m.beam_heat(0), m.beam_heat(1)), (engine::BEAM_HEAT, own));
    let h = engine::BEAM_HEAT.part(1).unwrap();
    assert_eq!((h.amount, h.radius), (60.0, 5.0));
    assert_eq!(engine::BEAM_HEAT.part(4).unwrap().amount, 15.0, "a wide beam's four parts");
    assert_eq!(BeamHeat { amount: 0, radius: 5 }.part(1), None, "0 heats nothing");
    let capped = BeamHeat { amount: 5000, radius: 200 }.part(1).unwrap();
    assert_eq!((capped.amount, capped.radius), (100.0, 16.0));
}
