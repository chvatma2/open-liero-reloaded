//! A small level, made-up images and helpers for the entity tests.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use olr_content::Content;
use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_fx::{Atlas, Entity, Fx, FxAssets, FxImages, FxTick, Kind};
use olr_math::{TimeDiff, Vec2};
use olr_scene::{AtlasId, DrawList, Item, Layer, Sprite, View};
use olr_sim::{CosmeticKind, Event, EventKind};
use olr_terrain::{ColorMap, Stamp, Terrain, ThemeArt};

/// `LX56PhysicsDT` at OLX's default 84 fps.
pub const DT: TimeDiff = TimeDiff(11);
/// The level's art scale.
pub const SCALE: u32 = 2;
pub const SKY: Rgba = Rgba::rgb(0, 0, 40);
pub const ROCK: Rgba = Rgba::rgb(90, 90, 90);
pub const DIRT: Rgba = Rgba::rgb(120, 80, 40);
/// The theme's default dirt colour, as the tests pass it.
pub const DEFAULT_DIRT: Rgba = Rgba::rgb(108, 56, 0);

/// 100 × 100: rock from row 60 down, a dirt wall from column 80 above it,
/// open air elsewhere.
pub fn terrain() -> Terrain {
    let cells = (0..100 * 100)
        .map(|i| match (i % 100, i / 100) {
            (_, y) if y >= 60 => Material::Rock,
            (x, _) if x >= 80 => Material::Dirt,
            _ => Material::Background,
        })
        .collect();
    Terrain::from_grid(&MaterialGrid::from_cells(100, 100, cells).unwrap())
}

pub fn solid(w: u32, h: u32, c: Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    img
}

/// The colour map of [`terrain`]: rock and dirt art in their colours, sky
/// behind, magenta squares as holes.
pub fn color_map(t: &Terrain) -> ColorMap {
    let (w, h) = (t.width() * SCALE, t.height() * SCALE);
    let mut front = solid(w, h, SKY);
    for y in 0..h {
        for x in 0..w {
            match t.material((x / SCALE) as i64, (y / SCALE) as i64) {
                Some(Material::Rock) => front.set(x, y, ROCK),
                Some(Material::Dirt) => front.set(x, y, DIRT),
                _ => {}
            }
        }
    }
    let holes = [5, 7, 9, 11, 15].map(|s| Stamp::from_image(&solid(s, s, Rgba::MAGENTA))).to_vec();
    let theme = ThemeArt { holes, green_mask: None, front_tile: solid(4, 4, DIRT) };
    ColorMap::new(t, &front, &solid(w, h, SKY), SCALE, theme)
}

/// The art pixels of world pixel `(x, y)`.
pub fn art(map: &ColorMap, x: u32, y: u32) -> [Rgba; 4] {
    let at = |ax: u32, ay: u32| {
        let i = ((ay * map.width() + ax) * 4) as usize;
        let p = &map.pixels()[i..i + 4];
        Rgba { r: p[0], g: p[1], b: p[2], a: p[3] }
    };
    let (ax, ay) = (x * SCALE, y * SCALE);
    [at(ax, ay), at(ax + 1, ay), at(ax, ay + 1), at(ax + 1, ay + 1)]
}

/// Images registered by name, in order.
#[derive(Default)]
pub struct Images(pub Vec<(String, RgbaImage, u32)>);

impl Atlas for Images {
    fn add_image(&mut self, name: String, image: RgbaImage, px_per_world: u32) -> AtlasId {
        self.0.push((name, image, px_per_world));
        AtlasId(self.0.len() as u32 - 1)
    }
}

impl Images {
    pub fn name(&self, id: AtlasId) -> &str {
        &self.0[id.0 as usize].0
    }

    pub fn image(&self, id: AtlasId) -> &RgbaImage {
        &self.0[id.0 as usize].1
    }
}

/// A gib sheet whose every pixel is traceable: `(100 + x, 50 + 10 * y, 30)`,
/// with a keyed (transparent) pixel and a gun grey in the first column of each row.
pub fn giblets() -> RgbaImage {
    let mut img = RgbaImage::new(32, 12);
    for y in 0..12 {
        for x in 0..32 {
            let c = match x {
                0 => Rgba::TRANSPARENT,
                1 => Rgba::rgb(180, 180, 180),
                _ => Rgba::rgb(100 + x as u8, 50 + 10 * y as u8, 30),
            };
            img.set(x, y, c);
        }
    }
    img
}

/// OLX's sprite sizes, in solid colours.
pub fn images() -> FxImages {
    FxImages {
        explosion: solid(512, 32, Rgba::rgb(250, 200, 0)),
        smoke: solid(70, 14, Rgba::rgb(128, 128, 128)),
        chemsmoke: solid(40, 10, Rgba::rgb(0, 200, 0)),
        spawn: solid(160, 32, Rgba::rgb(200, 200, 255)),
        sparkle: solid(30, 10, Rgba::rgb(200, 180, 255)),
        giblets: giblets(),
    }
}

pub fn assets(atlas: &mut Images) -> FxAssets {
    FxAssets::new(images(), DEFAULT_DIRT, atlas)
}

/// A level, its colour map, entities and assets.
pub struct Setup {
    pub terrain: Terrain,
    pub map: ColorMap,
    pub fx: Fx,
    pub atlas: Images,
    pub assets: FxAssets,
}

impl Setup {
    pub fn new() -> Setup {
        let terrain = terrain();
        let map = color_map(&terrain);
        let mut atlas = Images::default();
        let assets = assets(&mut atlas);
        Setup { terrain, map, fx: Fx::new(7), atlas, assets }
    }

    /// One tick of `substeps` sub-steps with these events.
    pub fn tick(&mut self, events: &[Event], substeps: u8) {
        let tick = FxTick { events, substeps, dt: DT, lost: None, material: &self.terrain };
        self.fx.step_tick(&tick, &mut self.map, &self.assets);
    }

    /// `n` ticks of one sub-step each, without events.
    pub fn passes(&mut self, n: usize) {
        for _ in 0..n {
            self.tick(&[], 1);
        }
    }

    pub fn kinds(&self) -> Vec<(usize, Kind)> {
        self.fx.entities().map(|(i, e)| (i, e.kind)).collect()
    }

    pub fn only(&self) -> Entity {
        let all: Vec<Entity> = self.fx.entities().map(|(_, e)| *e).collect();
        assert_eq!(all.len(), 1, "{all:?}");
        all[0]
    }

    /// The entity layer drawn with the whole map in view.
    pub fn draw(&self) -> Vec<Sprite> {
        let mut out = DrawList::new();
        out.view = View { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        self.fx.draw(&self.assets, &mut out);
        out.layer(Layer::Entities)
            .iter()
            .map(|i| match i {
                Item::Sprite(s) => *s,
                Item::Line(l) => panic!("entities draw no lines: {l:?}"),
            })
            .collect()
    }
}

/// A cosmetic event of sub-step `substep`.
pub fn cosmetic(substep: u8, kind: CosmeticKind, pos: (f32, f32), vel: (f32, f32)) -> Event {
    let (pos, vel) = (Vec2::new(pos.0, pos.1), Vec2::new(vel.0, vel.1));
    Event { substep, death: false, kind: EventKind::SpawnCosmetic { kind, pos, vel } }
}

/// `dt.seconds()` for the sub-step: `11 * 0.001f`.
pub fn dt() -> f32 {
    11.0f32 * 0.001
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).expect("the workspace root").to_path_buf()
}

pub fn content() -> Option<Content> {
    let Some(dir) = std::env::var("OLR_CONTENT_DIR").ok().filter(|d| Path::new(d).is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set or missing (run `cargo xtask fetch-olx-content`); skipping");
        return None;
    };
    Some(Content::open(Path::new(&dir)).expect("the content directory"))
}
