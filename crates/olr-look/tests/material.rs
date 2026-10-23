//! Surface materials (materials design, issue #37): the ramp, the clamps,
//! the maps.

use olr_look::{Heat, LAVA_RAMP, MAX_RAMP_KEYS, RampKey, SurfaceMaps, SurfaceMaterial, ramp_at};

fn heat(ramp: Vec<RampKey>) -> Heat {
    Heat { sensitivity: 100, capacity: 100, conductivity: 100, cooling: 100, ramp, light: 100 }
}

#[test]
fn the_lava_ramp_goes_from_dull_red_to_yellow_white() {
    let h = heat(Vec::new());
    assert_eq!(h.ramp(), &LAVA_RAMP);
    assert_eq!(h.look_at(0), None, "cold shows the art");
    assert_eq!(h.look_at(99), None);
    assert_eq!(h.look_at(100), Some((LAVA_RAMP[0].colour, 0)));
    let (dull, _) = h.look_at(250).unwrap();
    assert!(dull[0] > dull[1] && dull[1] > dull[2], "{dull:?}");
    assert_eq!(h.look_at(u16::MAX), Some(([255, 244, 220], 100)));
    // Brighter and glowing more as it heats.
    let mut last = (0u32, 0u8);
    for t in (100..=1000).step_by(10) {
        let (c, g) = h.look_at(t).unwrap();
        let sum = c.iter().map(|v| u32::from(*v)).sum::<u32>();
        assert!(sum >= last.0 && g >= last.1, "{t}: {c:?} {g}");
        last = (sum, g);
    }
}

#[test]
fn a_ramp_interpolates_between_its_keys() {
    let ramp = [
        RampKey { temperature: 200, colour: [0, 0, 0], glow: 0 },
        RampKey { temperature: 400, colour: [200, 100, 50], glow: 100 },
    ];
    assert_eq!(ramp_at(&ramp, 300), Some(([100, 50, 25], 50)));
    assert_eq!(ramp_at(&ramp, 400), Some(([200, 100, 50], 100)));
    assert_eq!(ramp_at(&[], 500), None);
}

#[test]
fn hostile_values_are_clamped_and_never_panic() {
    let wild = Heat {
        sensitivity: 255,
        capacity: 0,
        conductivity: u16::MAX,
        cooling: 0,
        ramp: (0..100).map(|i| RampKey { temperature: 1000 - i, colour: [255; 3], glow: 255 }).collect(),
        light: 255,
    };
    assert_eq!(wild.sensitivity(), 100);
    assert_eq!(wild.capacity(), olr_look::MIN_CAPACITY);
    assert_eq!(wild.conductivity(), olr_look::MAX_CONDUCTIVITY);
    assert_eq!(wild.cooling(), olr_look::MIN_COOLING);
    assert_eq!(wild.light(), 100);
    assert_eq!(wild.ramp().len(), MAX_RAMP_KEYS);
    for t in [0, 1, 500, 985, 1000, u16::MAX] {
        if let Some((_, g)) = wild.look_at(t) {
            assert!(g <= 100);
        }
    }
    let m = SurfaceMaterial { id: "x".into(), heat: Some(Heat { sensitivity: 0, ..wild }), light_soak: None };
    assert!(m.heats().is_none(), "sensitivity 0 is no heat");
}

#[test]
fn maps_fit_and_name_materials() {
    let mut m = SurfaceMaps::empty((3, 2));
    assert!(m.fits((3, 2)) && !m.fits((2, 3)) && m.is_empty());
    m.back[5] = 7;
    assert!(!m.is_empty());
    assert_eq!(m.highest(), 7);
    m.front.pop();
    assert!(!m.fits((3, 2)));
}
