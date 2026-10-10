//! The colour map follows the simulation (Review Focus 2 of plan 5): after a
//! long match of digging, explosions, dirt, green dirt, beams and stains,
//! every pixel's colour still agrees with the material grid. Needs the
//! imported OLX content (`RL_CONTENT_DIR`), skipped with a notice without it.

use std::path::Path;

use rl_content::{Content, SandboxSetup, SpawnSpec, color_edit, olx_level_id, olx_mod_id};
use rl_data::image::Rgba;
use rl_data::level::Material;
use rl_sim::{CosmeticKind, EventKind, Keys, SimSettings, StampId, TerrainOp, TickEvents};
use rl_terrain::{Area, ColorEdit, ColorMap, GREEN_SHADES, GreenPixel, PX_DIRT, PX_ROCK, Rect, StampPixel};

#[test]
fn color_edit_converts_every_terrain_op() {
    let edit = |op, stamp| color_edit(&EventKind::TerrainEdit { op, stamp, x: -3, y: 7 });
    assert_eq!(edit(TerrainOp::Carve, StampId::Hole(2)), Some(ColorEdit::Carve { hole: 2, x: -3, y: 7 }));
    assert_eq!(edit(TerrainOp::Beam, StampId::Hole(0)), Some(ColorEdit::Carve { hole: 0, x: -3, y: 7 }));
    assert_eq!(edit(TerrainOp::Dirt, StampId::Hole(4)), Some(ColorEdit::Dirt { hole: 4, x: -3, y: 7 }));
    assert_eq!(edit(TerrainOp::GreenDirt, StampId::GreenBall), Some(ColorEdit::GreenDirt { x: -3, y: 7 }));
    assert_eq!(edit(TerrainOp::Carve, StampId::GreenBall), None);
    assert_eq!(edit(TerrainOp::GreenDirt, StampId::Hole(1)), None);
    assert_eq!(color_edit(&EventKind::Spawned { worm: 0 }), None);
    assert_eq!(
        rl_content::lost_area(rl_sim::LostArea { x0: -1, y0: 2, x1: 3, y1: 4 }),
        Area { x0: -1, y0: 2, x1: 3, y1: 4 }
    );
}

fn content_dir() -> Option<String> {
    let Ok(dir) = std::env::var("RL_CONTENT_DIR") else {
        eprintln!("RL_CONTENT_DIR is not set (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    if Path::new(&dir).is_dir() {
        Some(dir)
    } else {
        eprintln!("{dir} not found (run `cargo xtask fetch-olx-content`); skipping");
        None
    }
}

/// xorshift64*: the test's own generator for the keys.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) % n
    }
}

/// The colours a dirt pixel may have besides its level art: the theme's
/// rims (holes and green ball) and green shades. The tile is per pixel.
fn edit_colours(map_theme: &rl_content::LoadedTheme) -> Vec<Rgba> {
    let mut v: Vec<Rgba> = GREEN_SHADES.to_vec();
    for s in map_theme.stamps() {
        for y in 0..s.height() {
            for x in 0..s.width() {
                if let StampPixel::Paint(c) = s.get(x, y) {
                    v.push(c);
                }
            }
        }
    }
    if let Some(m) = map_theme.green_mask() {
        for y in 0..m.height() {
            for x in 0..m.width() {
                if let GreenPixel::Rim(c) = m.get(x, y) {
                    v.push(c);
                }
            }
        }
    }
    v
}

/// Every world pixel's art block agrees with its material: background shows
/// the back art, rock the level art, dirt the level's dirt art (where the
/// level had dirt) or one colour placed by an edit (the tile there, a rim, a
/// green shade). Stained pixels may also
/// show their stain.
fn check_classes(
    map: &ColorMap,
    level: &rl_content::LoadedLevel,
    theme: &rl_content::LoadedTheme,
    stained: &[bool],
    what: &str,
) {
    let s = map.art_scale();
    let (w, h) = (level.material.width(), level.material.height());
    let allowed = edit_colours(theme);
    let art = |x: u32, y: u32| {
        let i = ((y * map.width() + x) * 4) as usize;
        let p = &map.pixels()[i..i + 4];
        Rgba::rgb(p[0], p[1], p[2])
    };
    let same = |img: &rl_data::image::RgbaImage, x: u32, y: u32| {
        (0..s)
            .all(|dy| (0..s).all(|dx| art(x * s + dx, y * s + dy).same_rgb(img.get(x * s + dx, y * s + dy))))
    };
    let uniform = |x: u32, y: u32| {
        let c = art(x * s, y * s);
        (0..s).all(|dy| (0..s).all(|dx| art(x * s + dx, y * s + dy) == c)).then_some(c)
    };
    let mut bad = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let m = map.material().material(x as i64, y as i64).unwrap();
            let ok = match m {
                Material::Background => same(&level.back, x, y),
                Material::Rock => same(&level.front, x, y),
                Material::Dirt => {
                    let tile = &theme.front_tile;
                    let t = tile.get(x % tile.width(), y % tile.height());
                    // The level's dirt art, only where the level had dirt:
                    // elsewhere the front art is the back art.
                    (level.material.get(x, y) == Material::Dirt && same(&level.front, x, y))
                        || uniform(x, y)
                            .is_some_and(|c| c.same_rgb(t) || allowed.iter().any(|a| a.same_rgb(c)))
                }
            };
            if !ok && !stained[(y * w + x) as usize] {
                bad.push((x, y, m));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{what}: {} pixels disagree with the material, first {:?}",
        bad.len(),
        &bad[..bad.len().min(5)]
    );
}

/// Where a falling entity first touches rock or dirt (`SimulateEntities`'
/// motion: `vel.y += 100 * 0.011; pos += vel * 0.011`), if it does in 2000 steps.
fn land(t: &rl_terrain::Terrain, mut pos: (f32, f32), mut vel: (f32, f32)) -> Option<(i32, i32)> {
    for _ in 0..2000 {
        vel.1 += 100.0 * 0.011;
        pos = (pos.0 + vel.0 * 0.011, pos.1 + vel.1 * 0.011);
        let (x, y) = (pos.0 as i64, pos.1 as i64);
        if pos.0 < 0.0 || pos.1 < 0.0 || x >= t.width() as i64 || y >= t.height() as i64 {
            return None;
        }
        if t.flags(x, y) & (PX_ROCK | PX_DIRT) != 0 {
            return Some((x as i32, y as i32));
        }
    }
    None
}

/// The first spot, row by row, with background 4 px around a row of 4 worms 8 px apart.
fn open_space(m: &rl_data::level::MaterialGrid) -> (i32, i32) {
    let free = |x: u32, y: u32| {
        (y - 4..=y + 4).all(|yy| (x - 4..=x + 28).all(|xx| m.get(xx, yy) == Material::Background))
    };
    (5..m.height() - 5)
        .flat_map(|y| (5..m.width().saturating_sub(29)).map(move |x| (x, y)))
        .find(|&(x, y)| free(x, y))
        .map(|(x, y)| (x as i32, y as i32))
        .expect("an open space")
}

fn agreement(mod_dir: &str, level_file: &str, seed: u32) {
    let Some(dir) = content_dir() else { return };
    let content = Content::open(Path::new(&dir)).unwrap();
    let what = format!("{mod_dir} on {level_file}");
    let (mod_id, level_id) = (olx_mod_id(mod_dir), olx_level_id(level_file));
    let level = content.load_level(&level_id).unwrap();
    let theme = content.load_theme(&level.theme).unwrap();
    let sim = content.sim_mod(&mod_id).unwrap();
    let id = |name: &str| sim.weapons().iter().position(|w| w.name == name).unwrap() as i32;
    let weapons = vec![id("Dirt Ball"), id("Greenball"), id("Laser"), id("Bazooka"), id("Minigun")];
    // Close together, so they hit each other: blood, deaths and gibs.
    let (x, y) = open_space(&level.material);
    let worms = (0..4).map(|i| SpawnSpec { pos: Some((x + 8 * i, y)), weapons: weapons.clone() }).collect();
    let setup = SandboxSetup { mod_id, level_id, seed, settings: SimSettings::lx56(), worms };
    let mut world = content.new_world(&setup).unwrap();
    // Low health: worms bleed (blood) and die of their own explosions (gibs).
    for i in 0..4 {
        world.set_health(i, 10.0);
    }
    let mut map = level.color_map(&theme);
    let gibs = content.core().sprite("giblets").clone();
    let (w, h) = (level.material.width(), level.material.height());
    let mut stained = vec![false; (w * h) as usize];
    let mut rng = Rng(seed as u64 * 7919 + 1);
    let mut held = [(0u8, 0u64); 4];
    let mut events = TickEvents::new();
    let mut counts = [0u32; 6];
    for tick in 0..3000 {
        let keys: Vec<Keys> = held
            .iter_mut()
            .map(|(k, left)| {
                if *left == 0 {
                    *left = 5 + rng.below(40);
                    *k =
                        [Keys::FIRE, Keys::LEFT, Keys::RIGHT, Keys::JUMP, Keys::UP, Keys::DOWN, Keys::CHANGE]
                            .into_iter()
                            .filter(|_| rng.below(100) < 35)
                            .fold(0, |a, b| a | b);
                }
                *left -= 1;
                Keys(*k)
            })
            .collect();
        world.tick(&keys, &mut events);
        for e in &events {
            if let Some(edit) = color_edit(&e.kind) {
                let EventKind::TerrainEdit { op, .. } = e.kind else { unreachable!() };
                counts[op as usize] += 1;
                map.apply(edit);
                continue;
            }
            // Blood and gibs fall (OLX's entity motion, without bounces,
            // through this tick's terrain) and stain where they land.
            let EventKind::SpawnCosmetic { kind, pos, vel } = e.kind else { continue };
            if !matches!(kind, CosmeticKind::Blood { .. } | CosmeticKind::Gib { .. }) {
                continue;
            }
            let Some((x, y)) = land(world.terrain(), (pos.x, pos.y), (vel.x, vel.y)) else { continue };
            let size = if let CosmeticKind::Blood { red } = kind {
                map.stain(x, y, Rgba::rgb(red, 0, 0));
                counts[4] += 1;
                1
            } else {
                map.stain_image(x - 1, y - 1, &gibs, Rect { x: 4 * (tick % 5), y: 8, w: 4, h: 4 });
                counts[5] += 1;
                4
            };
            let (x0, y0) = if size == 1 { (x, y) } else { (x - 1, y - 1) };
            for (sx, sy) in (0..size).flat_map(|dy| (0..size).map(move |dx| (x0 + dx, y0 + dy))) {
                if (0..w as i32).contains(&sx) && (0..h as i32).contains(&sy) {
                    stained[(sy as u32 * w + sx as u32) as usize] = true;
                }
            }
        }
        if let Some(area) = events.lost_area() {
            map.rebuild_area(rl_content::lost_area(area), world.terrain());
        }
        let _ = map.take_dirty().count();
        if tick % 100 == 99 {
            assert!(
                map.material() == world.terrain(),
                "{what} tick {tick}: the colour map's material drifted"
            );
            check_classes(&map, &level, &theme, &stained, &format!("{what} tick {tick}"));
        }
    }
    eprintln!("{what}: carve, dirt, green dirt, beam, blood, gib = {counts:?}");
    assert!(counts.iter().all(|&n| n > 0), "{what}: not every edit and stain happened: {counts:?}");
}

#[test]
fn color_map_agrees_with_material_after_a_match() {
    agreement("Classic", "Dirt Level.lxl", 11);
}

#[test]
fn color_map_agrees_with_material_after_a_match_on_rock_and_art() {
    agreement("Classic", "CastleStrike.lxl", 21);
}
