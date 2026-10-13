//! How a tick's events become entities and colour edits, in OLX's order
//! (spec §19: an event's sub-step counts the entity passes before it).

use crate::common;

use common::{DEFAULT_DIRT, DIRT, DT, SKY, Setup, art, cosmetic};
use rl_data::image::Rgba;
use rl_fx::{FxTick, Kind};
use rl_sim::{CosmeticKind, Event, EventKind, LostArea, StampId, TerrainOp};

fn carve(substep: u8, hole: u8, x: i32, y: i32) -> Event {
    Event {
        substep,
        death: false,
        kind: EventKind::TerrainEdit { op: TerrainOp::Carve, stamp: StampId::Hole(hole), x, y },
    }
}

#[test]
fn dirt_takes_its_colour_between_edits() {
    let mut s = Setup::new();
    // A blood stain on the dirt first: the particle copies whatever the image shows.
    s.tick(&[cosmetic(0, CosmeticKind::Blood { red: 200 }, (95.5, 30.0), (0.0, 0.0))], 1);
    let dirt = |probe| cosmetic(0, CosmeticKind::Dirt { probe }, (84.0, 30.0), (0.0, -10.0));
    let ev = [
        dirt((84, 30)),
        carve(0, 4, 84, 30),
        // The bonus chain's case: the probe was carved before its particles.
        dirt((84, 30)),
        dirt((95, 30)),
    ];
    s.tick(&ev, 0);
    let colours: Vec<Rgba> = s.fx.entities().map(|(_, e)| e.colour).collect();
    assert_eq!(colours, [DIRT, DEFAULT_DIRT, Rgba::rgb(200, 0, 0)]);
    assert_eq!(art(&s.map, 84, 30), [SKY; 4], "Fx applied the carve");
}

#[test]
fn entities_spawn_before_their_entity_pass() {
    let mut s = Setup::new();
    let smoke = |k| cosmetic(k, CosmeticKind::Smoke, (20.0, 20.0), (0.0, 0.0));
    // A sub-step beyond the tick's (never sent) spawns after the last pass.
    s.tick(&[smoke(0), smoke(1), smoke(2), smoke(9)], 2);
    // `realdt.seconds() * 15` a pass, with OLX's fixed 10 ms frame.
    let f = (10.0f32 * 0.001) * 15.0;
    let frames: Vec<f32> = s.fx.entities().map(|(_, e)| e.frame).collect();
    assert_eq!(frames, [f + f, f, 0.0, 0.0]);

    // A tick without a sub-step still spawns its events.
    s.tick(&[smoke(0)], 0);
    assert_eq!(s.fx.len(), 5);
}

#[test]
fn a_lost_area_is_rebuilt_from_the_material() {
    let mut s = Setup::new();
    let mut material = s.terrain.clone();
    let hole = rl_terrain::Stamp::from_image(&common::solid(5, 5, Rgba::MAGENTA));
    material.carve_hole(&hole, 85, 30);
    let lost = Some(LostArea { x0: 83, y0: 28, x1: 88, y1: 33 });
    let tick = FxTick { events: &[], substeps: 0, dt: DT, lost, material: &material };
    s.fx.step_tick(&tick, &mut s.map, &s.assets);
    assert_eq!(art(&s.map, 85, 30), [SKY; 4]);
    assert_eq!(art(&s.map, 82, 30), [DIRT; 4]);
}

#[test]
fn entities_are_clipped_to_the_view_like_olx() {
    let mut s = Setup::new();
    let ev = [
        cosmetic(0, CosmeticKind::Spawn, (10.0, 10.0), (0.0, 0.0)),
        cosmetic(0, CosmeticKind::Spawn, (60.0, 10.0), (0.0, 0.0)),
    ];
    s.tick(&ev, 0);
    let mut out = rl_scene::DrawList::new();
    out.view = rl_scene::View { x: 20.0, y: 0.0, w: 40.0, h: 30.0 };
    s.fx.draw(&s.assets, &mut out);
    // `x > Left + VirtW` is out, the right edge itself is in.
    assert_eq!(out.layer(rl_scene::Layer::Entities).len(), 1);
}

#[test]
fn odd_input_never_panics() {
    let mut s = Setup::new();
    s.assets.add_worm(Rgba::rgb(1, 2, 3), &mut s.atlas);
    let ev = [
        cosmetic(0, CosmeticKind::Blood { red: 1 }, (f32::NAN, 10.0), (0.0, 0.0)),
        cosmetic(0, CosmeticKind::Smoke, (f32::INFINITY, -1e30), (0.0, 0.0)),
        cosmetic(0, CosmeticKind::Explosion { size: i32::MIN }, (20.0, 20.0), (0.0, 0.0)),
        cosmetic(0, CosmeticKind::Explosion { size: i32::MAX }, (20.0, 20.0), (0.0, 0.0)),
        cosmetic(0, CosmeticKind::Gib { worm: i32::MIN }, (1e9, 20.0), (1e30, 1e30)),
        cosmetic(0, CosmeticKind::Dirt { probe: (i32::MIN, i32::MAX) }, (20.0, 20.0), (0.0, 0.0)),
        cosmetic(0, CosmeticKind::Doomsday, (20.0, 20.0), (f32::NAN, 0.0)),
    ];
    s.tick(&ev, 3);
    s.draw();
    assert!(
        s.fx.entities().all(|(_, e)| e.kind != Kind::Blood),
        "a falling entity without a position is removed"
    );
    let e = s.fx.entities().find(|(_, e)| e.kind == Kind::Particle).unwrap().1;
    assert_eq!(e.colour, DEFAULT_DIRT, "a probe outside the map");

    // Images of the wrong size are clipped, never a panic.
    let mut atlas = common::Images::default();
    let mut images = common::images();
    images.explosion = common::solid(3, 1, Rgba::rgb(1, 1, 1));
    images.giblets = common::solid(1, 1, Rgba::rgb(1, 1, 1));
    s.assets = rl_fx::FxAssets::new(images, DEFAULT_DIRT, &mut atlas);
    s.tick(&[cosmetic(0, CosmeticKind::Gib { worm: 0 }, (20.0, 59.5), (0.0, 1.0))], 1);
    s.passes(3);
    s.draw();
}

#[test]
fn entities_stay_on_the_world_grid_under_a_sub_pixel_camera() {
    let mut s = Setup::new();
    s.tick(&[cosmetic(0, CosmeticKind::Blood { red: 200 }, (30.3, 20.7), (0.0, 0.0))], 0);
    let at = |x: f32| {
        let mut out = rl_scene::DrawList::new();
        out.view = rl_scene::View { x, y: x, w: 40.0, h: 30.0 };
        s.fx.draw(&s.assets, &mut out);
        match out.layer(rl_scene::Layer::Entities) {
            [rl_scene::Item::Sprite(sp)] => sp.pos,
            other => panic!("{other:?}"),
        }
    };
    // OLX's view corner is a whole pixel: the half-pixel grid is the world's.
    assert_eq!(at(10.0), [29.5, 20.0]);
    assert_eq!(at(10.3), [29.5, 20.0]);
    assert_eq!(at(10.6), [29.5, 20.0]);
}
