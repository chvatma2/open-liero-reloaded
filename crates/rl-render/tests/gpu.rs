//! GPU tests, run by `cargo xtask full` (which sets `RL_GPU_TESTS`). They
//! skip with a notice without it, or when no adapter opens.

use rl_data::image::{Rgba, RgbaImage};
use rl_data::level::{Material, MaterialGrid};
use rl_render::{Offscreen, OffscreenOptions, Probe, RenderSettings, Renderer, UpscaleFilter};
use rl_scene::{AtlasId, AtlasImage, DrawList, Item, Layer, Line, Rect, Sprite, View};
use rl_terrain::{ColorEdit, ColorMap, Stamp, Terrain, ThemeArt};

fn offscreen() -> Option<Offscreen> {
    if std::env::var_os("RL_GPU_TESTS").is_none() {
        eprintln!("RL_GPU_TESTS not set (`cargo xtask full` sets it); skipping");
        return None;
    }
    match Offscreen::new() {
        Ok(o) => {
            let i = o.adapter_info();
            eprintln!("adapter: {} ({:?}, {:?})", i.name, i.backend, i.device_type);
            Some(o)
        }
        Err(e) => {
            eprintln!("{e}; skipping");
            None
        }
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

/// World pixel `(x, y)`'s colour in [`map`]: a few distinct colours.
fn palette(x: u32, y: u32) -> Rgba {
    const P: [Rgba; 5] = [
        Rgba::rgb(200, 40, 40),
        Rgba::rgb(40, 200, 40),
        Rgba::rgb(40, 40, 200),
        Rgba::rgb(230, 230, 230),
        Rgba::rgb(20, 20, 20),
    ];
    P[((x / 3 + y / 2 * 7) % 5) as usize]
}

/// A 120 × 90 level at art scale 2: dirt but for open air in the last ten
/// rows, every dirt pixel coloured by [`palette`].
fn map() -> ColorMap {
    let cells =
        (0..120 * 90).map(|i| if i / 120 < 80 { Material::Dirt } else { Material::Background }).collect();
    let t = Terrain::from_grid(&MaterialGrid::from_cells(120, 90, cells).unwrap());
    let mut front = RgbaImage::new(240, 180);
    for y in 0..180 {
        for x in 0..240 {
            front.set(x, y, palette(x / 2, y / 2));
        }
    }
    let holes = [5, 7, 9, 11, 15].map(|s| Stamp::from_image(&solid(s, s, Rgba::MAGENTA))).to_vec();
    let theme = ThemeArt { holes, green_mask: None, front_tile: solid(4, 4, Rgba::rgb(120, 80, 40)) };
    ColorMap::new(&t, &front, &solid(240, 180, Rgba::rgb(0, 0, 40)), 2, theme)
}

fn list(view: View) -> DrawList {
    let mut l = DrawList::new();
    l.view = view;
    l.map = (120, 90);
    l
}

#[test]
fn dirty_rect_upload_equals_full_upload() {
    let Some(gpu) = offscreen() else { return };
    let mut map = map();
    let mut updated = gpu.renderer(RenderSettings::default());
    updated.upload_terrain(&map);
    map.take_dirty().for_each(drop);
    let before = gpu.read_texture(updated.terrain_texture().unwrap());
    for (i, x) in (10..110).step_by(9).enumerate() {
        map.apply(ColorEdit::Carve { hole: (i % 5) as u8, x, y: 50 + (i as i32 % 3) * 7 });
    }
    map.apply(ColorEdit::Dirt { hole: 4, x: 30, y: 20 });
    map.stain(60, 60, Rgba::rgb(170, 0, 0));
    updated.update_terrain(&mut map);
    assert_eq!(map.take_dirty().count(), 0, "update_terrain drained the dirty rectangles");
    let mut full = gpu.renderer(RenderSettings::default());
    full.upload_terrain(&map);
    let a = gpu.read_texture(updated.terrain_texture().unwrap());
    let b = gpu.read_texture(full.terrain_texture().unwrap());
    assert_ne!(a, before, "the edits changed the map");
    assert_eq!(a.as_raw(), map.pixels(), "the dirty rectangles carried every change");
    assert_eq!(a, b);
    // The same frame from both renderers is the same picture.
    let l = list(View { x: 20.5, y: 30.0, w: 80.0, h: 60.0 });
    assert_eq!(
        gpu.render_to_image(&mut updated, &l, (160, 120)),
        gpu.render_to_image(&mut full, &l, (160, 120))
    );
}

#[test]
fn upscale_filters_are_selectable() {
    let Some(gpu) = offscreen() else { return };
    let map = map();
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.upload_terrain(&map);
    // 40 x 30 world px = 80 x 60 texels into 300 x 200: 3x whole, 3.33x fitted.
    let l = list(View { x: 30.0, y: 20.0, w: 40.0, h: 30.0 });
    let nearest = gpu.render_to_image(&mut r, &l, (300, 200));
    r.settings.filter = UpscaleFilter::SharpBilinear;
    let sharp = gpu.render_to_image(&mut r, &l, (300, 200));
    assert_ne!(nearest, sharp);

    // Integer nearest: a 240 x 180 picture at (30, 10), each world pixel a
    // 6 x 6 block of its own colour, black bars around.
    for y in 0..200 {
        for x in 0..300 {
            let inside = (30..270).contains(&x) && (10..190).contains(&y);
            let want = if inside { palette(30 + (x - 30) / 6, 20 + (y - 10) / 6) } else { Rgba::BLACK };
            assert_eq!(nearest.get(x, y), want, "nearest ({x}, {y})");
        }
    }
    // Sharp bilinear fills the height; inside a world pixel it is flat,
    // only the one-pixel seams blend.
    let colours: Vec<Rgba> = (0..5).map(|i| palette(i * 3, 0)).collect();
    let (mut flat, mut seams) = (0, 0);
    for y in 0..200 {
        for x in 17..283 {
            if colours.contains(&sharp.get(x, y)) { flat += 1 } else { seams += 1 }
        }
    }
    assert!(seams > 0, "sharp bilinear blends at texel seams");
    assert!(flat > seams * 3, "but most pixels are flat: {flat} flat, {seams} blended");
    assert_eq!(sharp.get(5, 100), Rgba::BLACK, "pillarboxed");
    assert_eq!(sharp.get(150, 0).a, 255);
}

#[test]
fn missing_assets_use_placeholders() {
    let Some(gpu) = offscreen() else { return };
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    let green =
        AtlasImage { name: "green".into(), image: solid(8, 8, Rgba::rgb(0, 255, 0)), px_per_world: 2 };
    r.upload_images(&[green]);
    // No terrain: black. View 40 x 30 at 80 x 60 pixels: 1:1 with the texels.
    let mut l = list(View { x: 0.0, y: 0.0, w: 40.0, h: 30.0 });
    let white = Rgba::rgb(255, 255, 255);
    let src = Rect { x: 0, y: 0, w: 8, h: 8 };
    let sprite = |atlas, x| Sprite {
        atlas: AtlasId(atlas),
        src,
        pos: [x, 10.0],
        size: [4.0, 4.0],
        flip: false,
        tint: white,
    };
    l.sprite(Layer::Worms, sprite(0, 5.0));
    l.sprite(Layer::Worms, sprite(7, 20.0));
    l.push(Layer::Beams, Item::Line(Line { a: (2, 25), b: (8, 25), colour: Rgba::rgb(0, 0, 255) }));
    let img = gpu.render_to_image(&mut r, &l, (80, 60));
    // World (x, y) is output pixel (2x, 2y) at 1:1 (the view sits on whole pixels).
    assert_eq!(img.get(10, 20), Rgba::rgb(0, 255, 0));
    assert_eq!(img.get(17, 27), Rgba::rgb(0, 255, 0));
    assert_eq!(img.get(40, 20), Rgba::MAGENTA, "an unknown atlas id is a magenta square");
    assert_eq!(img.get(47, 27), Rgba::MAGENTA);
    assert_eq!(img.get(48, 28), Rgba::BLACK);
    assert_eq!(img.get(4, 50), Rgba::rgb(0, 0, 255), "line pixels are world-pixel blocks");
    assert_eq!(img.get(17, 51), Rgba::rgb(0, 0, 255));
    assert_eq!(img.get(18, 50), Rgba::BLACK);
    assert_eq!(img.get(0, 0), Rgba::BLACK, "no terrain is black");
    assert_eq!(gpu.render_to_image(&mut r, &l, (80, 60)), img, "a frame renders the same twice");
    // Nothing uploaded and nothing to draw: black, no panic.
    let empty = gpu.render_to_image(&mut gpu.renderer(RenderSettings::default()), &DrawList::new(), (8, 8));
    assert!(empty.as_raw().chunks(4).all(|p| p == [0, 0, 0, 255]));
}

#[test]
fn srgb_targets_show_the_same_colours() {
    let Some(gpu) = offscreen() else { return };
    let map = map();
    let l = list(View { x: 30.0, y: 20.0, w: 40.0, h: 30.0 });
    let mut plain = gpu.renderer(RenderSettings::default());
    plain.upload_terrain(&map);
    let want = gpu.render_to_image(&mut plain, &l, (160, 120));
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut srgb =
        Renderer::new(gpu.device().clone(), gpu.queue().clone(), format, RenderSettings::default());
    srgb.upload_terrain(&map);
    let texture = gpu.device().create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 160, height: 120, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    srgb.render(&l, &texture.create_view(&Default::default()), (160, 120));
    let got = gpu.read_texture(&texture);
    let off = want.as_raw().iter().zip(got.as_raw()).map(|(a, b)| a.abs_diff(*b)).max();
    assert!(off <= Some(1), "an sRGB target stores the same bytes, give or take rounding: {off:?}");
}

/// The shader-headroom probe draws into textures of its own: the picture
/// is the same with it as without.
#[test]
fn the_probe_never_changes_the_picture() {
    let Some(gpu) = offscreen() else { return };
    let l = list(View { x: 10.0, y: 10.0, w: 64.0, h: 36.0 });
    let mut plain = gpu.renderer(RenderSettings::default());
    plain.upload_terrain(&map());
    let mut probed = gpu.renderer(RenderSettings::default());
    probed.upload_terrain(&map());
    probed.set_probe(Some(Probe { passes: 4, taps: 9 }));
    let a = gpu.render_to_image(&mut plain, &l, (256, 144));
    let b = gpu.render_to_image(&mut probed, &l, (256, 144));
    assert_eq!(a.as_raw(), b.as_raw());
}

/// The probe blurs the drawn frame, not a black texture (which a GPU
/// compresses, so a blur of it would cost less than a real effect's).
#[test]
fn the_probe_blurs_the_drawn_frame() {
    let Some(gpu) = offscreen() else { return };
    let l = list(View { x: 10.0, y: 10.0, w: 64.0, h: 36.0 });
    let mut r = gpu.renderer(RenderSettings::default());
    r.upload_terrain(&map());
    for passes in [1, 2, 3] {
        r.set_probe(Some(Probe { passes, taps: 9 }));
        let _ = gpu.render_to_image(&mut r, &l, (256, 144));
        let out = gpu.read_texture(r.probe_output().expect("the probe ran"));
        let mut colours: Vec<&[u8]> = out.as_raw().chunks(4).collect();
        colours.sort_unstable();
        colours.dedup();
        assert!(colours.len() > 16, "{passes} passes: {} colours, a blur of a blank texture", colours.len());
    }
}

/// GPU timing reports a frame (and the probe) when the device has
/// timestamps, and nothing otherwise; it never fails. The first frame runs
/// no probe, so its slots are never written: only the frame's are resolved.
#[test]
fn gpu_timing_reports_when_the_device_has_timestamps() {
    if std::env::var_os("RL_GPU_TESTS").is_none() {
        return;
    }
    let Ok(gpu) =
        rl_render::Offscreen::open(&OffscreenOptions { timestamps: true, ..OffscreenOptions::default() })
    else {
        return;
    };
    let has = gpu.device().features().contains(wgpu::Features::TIMESTAMP_QUERY);
    eprintln!("adapter: {}; timestamps: {has}", gpu.adapter_info().name);
    let mut r = gpu.renderer(RenderSettings::default());
    r.upload_terrain(&map());
    assert_eq!(r.enable_gpu_timing(), has);
    let l = list(View { x: 10.0, y: 10.0, w: 64.0, h: 36.0 });
    let _ = gpu.render_to_image(&mut r, &l, (256, 144));
    match r.gpu_times() {
        Some(t) => assert!(has && t.frame_ms.is_finite() && t.frame_ms >= 0.0 && t.probe_ms == 0.0, "{t:?}"),
        None => assert!(!has, "timestamps were asked for and offered"),
    }
    // One pass holds both probe slots; with three or more, a middle pass
    // writes none (and must ask for no timestamps: wgpu refuses an empty set).
    for passes in [2, 1, 5] {
        r.set_probe(Some(Probe { passes, taps: 4 }));
        let _ = gpu.render_to_image(&mut r, &l, (256, 144));
        match r.gpu_times() {
            Some(t) => {
                eprintln!("{passes} passes: {t:?}");
                assert!(has && t.frame_ms.is_finite() && t.frame_ms >= 0.0 && t.probe_ms >= 0.0, "{t:?}");
            }
            None => assert!(!has, "timestamps were asked for and offered"),
        }
    }
    assert_eq!(r.gpu_times(), None, "each frame is read once");
}
