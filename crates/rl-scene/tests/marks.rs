//! Laser sights and beams are OLX entities that live two entity passes:
//! one spawned in the last sub-step of a tick is drawn, and a tick that
//! runs no sub-step keeps the last tick's.

use crate::common;

use rl_scene::{DrawList, Interpolation, Item, Layer, Line, SceneOptions};
use rl_sim::{CosmeticKind, EventKind};

#[test]
fn beams_and_laser_sights_live_two_entity_passes() {
    let Some(content) = common::content() else { return };
    let latest = SceneOptions { interpolation: Interpolation::Latest, ..SceneOptions::default() };
    let mut list = DrawList::new();
    let (mut kept, mut fresh, mut gone) = (0, 0, 0);
    let mut last: Vec<Line> = Vec::new();
    common::play(&content, "beam-burst", latest, |world, _, events, scene, assets| {
        scene.build(world, 1.0, assets, &mut list);
        let lines: Vec<Line> = list
            .layer(Layer::Beams)
            .iter()
            .filter_map(|i| if let Item::Line(l) = i { Some(*l) } else { None })
            .collect();
        let n = world.substeps_last_tick();
        let beams: Vec<_> = events
            .iter()
            .filter(|e| e.substep + 1 == n)
            .filter_map(|e| match e.kind {
                EventKind::SpawnCosmetic { kind: CosmeticKind::Beam { .. }, pos, vel } => {
                    Some((pos, pos + vel))
                }
                _ => None,
            })
            .collect();
        if n == 0 {
            assert_eq!(lines, last, "no sub-step: the last tick's beam stays");
            kept += usize::from(!lines.is_empty());
        } else {
            assert_eq!(lines.len(), beams.len());
            for (l, (a, b)) in lines.iter().zip(&beams) {
                assert_eq!((l.a, l.b), ((a.x as i32, a.y as i32), (b.x as i32, b.y as i32)));
            }
            fresh += usize::from(!lines.is_empty());
            gone += usize::from(lines.is_empty() && !last.is_empty());
        }
        last = lines;
    });
    assert!(kept > 0 && fresh > 0 && gone > 0, "kept {kept} fresh {fresh} gone {gone}");
}
