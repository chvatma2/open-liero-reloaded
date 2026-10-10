//! Spec §5.3: `World::tick` does not allocate once the world is set up.

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_math::AbsTime;
use rl_sim::{Keys, SimSettings, World};
use rl_terrain::{Stamp, Terrain};

#[test]
fn tick_does_not_allocate() {
    let holes = [5u32, 7, 9, 11, 15]
        .into_iter()
        .map(|s| {
            let mut img = RgbaImage::new(s, s);
            for y in 0..s {
                for x in 0..s {
                    img.set(x, y, Rgba::MAGENTA);
                }
            }
            Stamp::from_image(&img)
        })
        .collect();
    let terrain = Terrain::from_grid(&MaterialGrid::new(300, 200, Material::Dirt));
    let mut w = World::new(SimSettings::lx56(), terrain, holes, AbsTime(0));
    w.spawn_worm(100, 100);
    w.spawn_worm(200, 100);
    let a = [Keys(Keys::RIGHT | Keys::ROPE | Keys::UP), Keys(Keys::LEFT | Keys::JUMP)];
    let b = [Keys(Keys::LEFT), Keys(Keys::RIGHT | Keys::DOWN)];
    let info = allocation_counter::measure(|| {
        for i in 0..200 {
            w.tick(if i % 3 == 0 { &a } else { &b });
        }
    });
    assert_eq!(info.count_total, 0, "tick allocated");
}
