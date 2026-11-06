//! The production line design: `crushed_dirt(n)`, the dirt a copy's
//! moving parts crushed in a step.

use olr_data::level::Material;
use olr_sim::{LIVE_SUBPIXELS, OracleMode, TickEvents, World};
use olr_terrain::Terrain;

use crate::common::{keys, level, rock_copy, setup};

#[test]
fn crushed_dirt_reaches_its_copy_only() {
    let script = "fn start() { this.set(\"v\", 0); }\n\
                  fn crushed_dirt(n) { this.set(\"v\", this.get(\"v\") + n); }";
    let level = level(
        vec![rock_copy("press", 20, &["On"], vec![0]), rock_copy("far", 150, &["On"], vec![0])],
        &[script],
        None,
    );
    let mut s = setup(level, 7, true);
    // Dirt in columns 24..34 of the copies' rows 40..44, in "press"'s way.
    let (w, h) = (s.terrain.width(), s.terrain.height());
    let cells = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let dirt = (24..34).contains(&x) && (40..44).contains(&y);
            if dirt { Material::Dirt } else { s.terrain.material(x.into(), y.into()).unwrap() }
        })
        .collect();
    let grid = olr_data::level::MaterialGrid::from_cells(w, h, cells).unwrap();
    s.terrain = Terrain::from_grid(&grid);
    let mut world = World::new(s, Box::new(OracleMode)).unwrap();
    world.move_live_value_for_test(0, 1, 10 * LIVE_SUBPIXELS, LIVE_SUBPIXELS).unwrap();
    let mut ev = TickEvents::new();
    for t in 0..20 {
        world.tick(&keys(t), &mut ev);
        assert!(!ev.iter().any(|e| matches!(e.kind, olr_sim::EventKind::ScriptError { .. })));
    }
    let v = |id: &str| world.live().unwrap().copy_by_id(id).unwrap().value("v").unwrap();
    assert_eq!(v("press"), 40, "10 steps of a 4-pixel column");
    assert_eq!(v("far"), 0);
}
