//! Renderer benchmarks (spec §7, §21 G8): one frame offscreen at 1080p
//! and 1440p, drawn from a level-sized colour map made here (504 × 350
//! world pixels at art scale 2) and a busy draw list (2000 sprites, 500
//! line pixels), waited for on the device. They need a graphics adapter
//! (lavapipe or llvmpipe, as the GPU tests): without one they print a
//! notice and measure nothing. `cargo xtask bench` runs them.

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_render::{OUTPUT_FORMAT, Offscreen, RenderSettings, Renderer};
use rl_scene::{AtlasId, AtlasImage, DrawList, Item, Layer, Line, Rect, Sprite, View};
use rl_terrain::{ColorMap, Stamp, Terrain, ThemeArt};

const W: u32 = 504;
const H: u32 = 350;

fn main() {
    match Offscreen::new() {
        Ok(_) => divan::main(),
        Err(e) => println!("skip rl-render benches: {e}"),
    }
}

fn solid(w: u32, h: u32, c: Rgba) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, c);
        }
    }
    img
}

/// Dirt with tunnels, at art scale 2, every pixel its own colour.
fn map() -> ColorMap {
    let cells = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            if (x / 40 + y / 30) % 3 == 0 { Material::Background } else { Material::Dirt }
        })
        .collect();
    let t = Terrain::from_grid(&MaterialGrid::from_cells(W, H, cells).expect("a grid"));
    let mut front = RgbaImage::new(W * 2, H * 2);
    for y in 0..H * 2 {
        for x in 0..W * 2 {
            front.set(x, y, Rgba::rgb((x * 7 % 256) as u8, (y * 5 % 256) as u8, ((x + y) % 256) as u8));
        }
    }
    let holes = [5, 7, 9, 11, 15].map(|s| Stamp::from_image(&solid(s, s, Rgba::MAGENTA))).to_vec();
    let theme = ThemeArt { holes, green_mask: None, front_tile: solid(4, 4, Rgba::rgb(120, 80, 40)) };
    ColorMap::new(&t, &front, &solid(W * 2, H * 2, Rgba::rgb(0, 0, 40)), 2, theme)
}

/// A renderer with the map and eight sprite images uploaded, and a draw
/// list over a 16:9 view of 320 × 180 world pixels.
fn scene(gpu: &Offscreen) -> (Renderer, DrawList) {
    let mut r = gpu.renderer(RenderSettings::default());
    r.upload_terrain(&map());
    let images: Vec<AtlasImage> = (0..8)
        .map(|i| AtlasImage {
            name: format!("sprite {i}"),
            image: solid(16, 16, Rgba::rgb(30 * i as u8, 255 - 30 * i as u8, 128)),
            px_per_world: 2,
        })
        .collect();
    r.upload_images(&images);
    let mut l = DrawList::new();
    l.view = View { x: 90.5, y: 80.25, w: 320.0, h: 180.0 };
    l.map = (W, H);
    for i in 0..2000u32 {
        let pos = [90.0 + (i * 37 % 320) as f32, 80.0 + (i * 53 % 180) as f32];
        let sprite = Sprite {
            atlas: AtlasId(i % 8),
            src: Rect { x: 0, y: 0, w: 16, h: 16 },
            pos,
            size: [8.0, 8.0],
            flip: i % 2 == 0,
            tint: Rgba::rgb(255, 255, 255),
        };
        l.sprite(if i % 3 == 0 { Layer::Worms } else { Layer::Projectiles }, sprite);
    }
    for i in 0..500i32 {
        let (x, y) = (100 + i % 300, 90 + i / 3 % 170);
        l.push(Layer::Beams, Item::Line(Line { a: (x, y), b: (x + 1, y), colour: Rgba::rgb(255, 0, 0) }));
    }
    (r, l)
}

/// One frame into a `size` target, waited for on the device.
fn frame(b: divan::Bencher, size: (u32, u32)) {
    let gpu = Offscreen::new().expect("an adapter (checked in main)");
    let (mut r, list) = scene(&gpu);
    let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("bench target"),
        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OUTPUT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    // Warm: the world target and the instance buffer are made once.
    r.render(&list, &view, size);
    b.bench_local(|| {
        r.render(&list, &view, size);
        gpu.device().poll(wgpu::PollType::wait_indefinitely()).expect("the device");
    });
}

#[divan::bench(sample_count = 30)]
fn frame_1080p(b: divan::Bencher) {
    frame(b, (1920, 1080));
}

#[divan::bench(sample_count = 30)]
fn frame_1440p(b: divan::Bencher) {
    frame(b, (2560, 1440));
}
