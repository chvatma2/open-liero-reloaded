//! GPU tests, run by `cargo xtask full` (which sets `OLR_GPU_TESTS`). They
//! skip with a notice without it, or when no adapter opens.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_render::{Offscreen, OffscreenOptions, Probe, RenderSettings, Renderer, UpscaleFilter};
use olr_scene::{AtlasId, AtlasImage, DrawList, Item, Layer, Line, Rect, Sprite, View};
use olr_terrain::{ColorEdit, ColorMap, Stamp, Terrain, ThemeArt};

fn offscreen() -> Option<Offscreen> {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        eprintln!("OLR_GPU_TESTS not set (`cargo xtask full` sets it); skipping");
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
        shade: Default::default(),
    };
    l.sprite(Layer::Worms, sprite(0, 5.0));
    l.sprite(Layer::Worms, sprite(7, 20.0));
    l.push(
        Layer::Beams,
        Item::Line(Line { a: (2, 25), b: (8, 25), colour: Rgba::rgb(0, 0, 255), shade: Default::default() }),
    );
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
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let Ok(gpu) =
        olr_render::Offscreen::open(&OffscreenOptions { timestamps: true, ..OffscreenOptions::default() })
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

/// [`map`] with a glow: strength `x % 7 * 30` per art pixel, group 1 on the
/// left half (look system).
fn glowing_map() -> ColorMap {
    let mut m = map();
    let mut g = olr_terrain::GlowLayers::empty((240, 180), (120, 90));
    for (i, v) in g.front.iter_mut().enumerate() {
        *v = (i % 240 % 7 * 30) as u8;
    }
    g.back.iter_mut().for_each(|v| *v = 5);
    for (i, v) in g.groups.iter_mut().enumerate() {
        *v = u8::from(i % 120 < 60);
    }
    assert!(m.set_glow(Some(g)));
    m
}

/// The lit path with no look draws today's bytes, lighting on or off
/// (Global Constraints: old content is byte-identical).
#[test]
fn look_none_draws_todays_bytes() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_map();
    let l = list(View { x: 20.5, y: 30.0, w: 80.0, h: 60.0 });
    let mut plain = gpu.renderer(RenderSettings::default());
    plain.upload_terrain(&map);
    let want = gpu.render_to_image(&mut plain, &l, (160, 120));
    let mut lit = gpu.renderer(RenderSettings::default());
    lit.set_lighting(true);
    lit.upload_terrain(&map);
    assert_eq!(gpu.render_lit_to_image(&mut lit, &l, None, (160, 120)), want);
    assert_eq!(gpu.render_to_image(&mut lit, &l, (160, 120)), want);
    // The lit world pass runs (two targets, the glow textures bound).
    let mut look = olr_look::LookFrame::default();
    look.begin(None, 0.0);
    let _ = gpu.render_lit_to_image(&mut lit, &l, Some(&look), (160, 120));
}

/// With lighting on, the glow, group and solid textures follow the colour
/// map's dirty rectangles (plan 15a Task 9).
#[test]
fn look_textures_follow_dirty_rects() {
    let Some(gpu) = offscreen() else { return };
    let mut lit_map = glowing_map();
    let map = &mut lit_map;
    let mut r = gpu.renderer(RenderSettings::default());
    assert!(r.glow_texture().is_none(), "no look textures without lighting");
    r.set_lighting(true);
    r.upload_terrain(map);
    map.take_dirty().for_each(drop);
    assert_eq!(gpu.read_bytes(r.glow_texture().unwrap()), map.glow().unwrap());
    for (i, x) in (10..110).step_by(9).enumerate() {
        map.apply(ColorEdit::Carve { hole: (i % 5) as u8, x, y: 50 + (i as i32 % 3) * 7 });
    }
    map.apply(ColorEdit::Dirt { hole: 4, x: 30, y: 20 });
    r.update_terrain(map);
    assert_eq!(gpu.read_bytes(r.glow_texture().unwrap()), map.glow().unwrap());
    assert_eq!(gpu.read_bytes(r.groups_texture().unwrap()), map.glow_groups().unwrap());
    assert_eq!(gpu.read_bytes(r.solid_texture().unwrap()), map.material().cells());
    // A map without glow uploads zeros.
    let mut plain = self::map();
    r.upload_terrain(&plain);
    r.update_terrain(&mut plain);
    assert!(gpu.read_bytes(r.glow_texture().unwrap()).iter().all(|v| *v == 0));
}

/// A look frame of ambient `a` (grey), one glow group at self-light scale
/// 0.5 (the left half of [`glowing_map`]), nothing else.
fn dim(a: f32) -> olr_look::LookFrame {
    let mut look = olr_look::LookFrame::default();
    look.begin(None, 0.0);
    look.ambient = [a; 3];
    look.groups.push(olr_look::FrameGroup { cast: [0.0; 3], own_colour: true, self_scale: 0.5, reach: 0.0 });
    look
}

/// 80 × 60 world pixels at art scale 2 onto 160 × 120 output pixels with
/// integer nearest: one output pixel per world-target texel.
fn one_to_one() -> (DrawList, (u32, u32)) {
    (list(View { x: 20.0, y: 30.0, w: 80.0, h: 60.0 }), (160, 120))
}

#[test]
fn look_full_ambient_is_identity() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_map();
    let (l, size) = one_to_one();
    let mut plain = gpu.renderer(RenderSettings::default());
    plain.upload_terrain(&map);
    let want = gpu.render_to_image(&mut plain, &l, size);
    let mut lit = gpu.renderer(RenderSettings::default());
    lit.set_lighting(true);
    lit.upload_terrain(&map);
    let mut look = dim(1.0);
    look.groups.clear();
    assert_eq!(gpu.render_lit_to_image(&mut lit, &l, Some(&look), size), want);
    assert_eq!(gpu.render_lit_to_image(&mut lit, &l, Some(&look), size), want, "same state, same bytes");
}

#[test]
fn look_ambient_darkens() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_map();
    let (l, size) = one_to_one();
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.upload_terrain(&map);
    let plain = gpu.render_to_image(&mut r, &l, size);
    r.set_lighting(true);
    r.upload_terrain(&map);
    let look = dim(0.3);
    let img = gpu.render_lit_to_image(&mut r, &l, Some(&look), size);
    let (grid, light) = gpu.read_light(&r).expect("a light grid");
    assert_eq!(grid.origin, (3, 13), "the frame's origin less the light grid's pad (16)");
    assert!(light.iter().all(|c| (c[0] - 0.3).abs() < 1e-3), "ambient everywhere");
    let glow = map.glow().unwrap();
    let mut worst = 0i32;
    for y in 0..size.1 {
        for x in 0..size.0 {
            // Output pixel (x, y) is target texel (x + 2, y + 2): art pixel (40 + x, 60 + y).
            let (ax, ay) = (40 + x, 60 + y);
            let g = f32::from(glow[(ay * 240 + ax) as usize]) / 255.0;
            let left = ax / 2 < 60;
            let self_light = if left { g * 0.5 } else { g };
            let p = plain.get(x, y);
            let want = olr_render::shade_texel(
                [p.r, p.g, p.b, 255],
                [0.3; 3],
                self_light,
                false,
                &Default::default(),
                1.0,
            );
            let got = img.get(x, y);
            for (a, b) in [got.r, got.g, got.b].iter().zip(&want[..3]) {
                worst = worst.max((i32::from(*a) - i32::from(*b)).abs());
            }
        }
    }
    assert!(worst <= 1, "every texel within 1 of the model: {worst}");
}

#[test]
fn look_steps_band_the_light() {
    let Some(gpu) = offscreen() else { return };
    let (l, size) = one_to_one();
    let mut r = gpu.renderer(RenderSettings::default());
    r.set_lighting(true);
    r.upload_terrain(&glowing_map());
    let mut look = dim(0.3);
    look.light_steps = 4;
    gpu.render_lit_to_image(&mut r, &l, Some(&look), size);
    let (_, light) = gpu.read_light(&r).unwrap();
    assert!(light.iter().all(|c| (c[0] - 0.25).abs() < 1e-3), "0.3 bands to 0.25: {:?}", light[0]);
    look.max_light = 2.0;
    look.ambient = [3.0; 3];
    gpu.render_lit_to_image(&mut r, &l, Some(&look), size);
    let (_, light) = gpu.read_light(&r).unwrap();
    assert!(light.iter().all(|c| (c[0] - 2.0).abs() < 1e-3), "capped at max light: {:?}", light[0]);
}

#[test]
fn look_modes() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_map();
    let (l, size) = one_to_one();
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.upload_terrain(&map);
    let plain = gpu.render_to_image(&mut r, &l, size);
    r.set_lighting(true);
    r.upload_terrain(&map);
    let mut look = dim(0.3);
    look.mode = olr_look::ViewMode::FullBright;
    assert_eq!(gpu.render_lit_to_image(&mut r, &l, Some(&look), size), plain);
    look.mode = olr_look::ViewMode::LightOnly;
    let img = gpu.render_lit_to_image(&mut r, &l, Some(&look), size);
    // Art pixel (42, 60) does not glow (42 % 7 = 0): white lit at 0.3.
    let grey = olr_render::linear_to_srgb(0.3);
    assert_eq!(img.get(2, 0), Rgba::rgb(grey, grey, grey));
}

/// A `w` × `h` open room at art scale 1 (black art), rock where `rock(x, y)`.
fn room(w: u32, h: u32, rock: impl Fn(u32, u32) -> bool) -> ColorMap {
    let cells =
        (0..w * h).map(|i| if rock(i % w, i / w) { Material::Rock } else { Material::Background }).collect();
    let t = Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).unwrap());
    let theme = ThemeArt { holes: Vec::new(), green_mask: None, front_tile: solid(1, 1, Rgba::BLACK) };
    ColorMap::new(
        &t,
        &solid(w, h, Rgba::rgb(128, 128, 128)),
        &solid(w, h, Rgba::rgb(128, 128, 128)),
        1,
        theme,
    )
}

/// A white light at `at` (world coordinates), radius 2, reach `reach`.
fn lamp(at: [f32; 2], reach: f32) -> olr_look::FrameLight {
    olr_look::FrameLight { pos: at, radius: 2.0, colour: [1.0; 3], reach }
}

/// The light at world pixel `(x, y)` of the last lit frame.
fn light_at(gpu: &Offscreen, r: &Renderer, x: i32, y: i32) -> [f32; 3] {
    let (grid, light) = gpu.read_light(r).unwrap();
    let (i, j) = (x - grid.origin.0, y - grid.origin.1);
    assert!(
        i >= 0 && j >= 0 && (i as u32) < grid.size.0 && (j as u32) < grid.size.1,
        "({x}, {y}) off the grid"
    );
    light[(j as u32 * grid.size.0 + i as u32) as usize]
}

/// A lit frame of `map` over `view` with `lights` at ambient 0.1.
fn lit_frame(
    gpu: &Offscreen,
    map: &ColorMap,
    view: View,
    lights: &[olr_look::FrameLight],
    softness: u8,
) -> Renderer {
    lit_frame_with(gpu, map, view, lights, |l| l.softness = softness)
}

/// [`lit_frame`] with the look changed by `set`.
fn lit_frame_with(
    gpu: &Offscreen,
    map: &ColorMap,
    view: View,
    lights: &[olr_look::FrameLight],
    set: impl FnOnce(&mut olr_look::LookFrame),
) -> Renderer {
    let mut r = gpu.renderer(RenderSettings::default());
    r.set_lighting(true);
    r.upload_terrain(map);
    let mut look = dim(0.1);
    set(&mut look);
    look.emitters.extend_from_slice(lights);
    let mut l = list(view);
    l.map = (map.material().width(), map.material().height());
    gpu.render_lit_to_image(&mut r, &l, Some(&look), (64, 64));
    r
}

#[test]
fn a_wall_blocks_a_light() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 40, |x, _| x == 50);
    let r =
        lit_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &[lamp([30.5, 20.5], 100.0)], 0);
    assert!(light_at(&gpu, &r, 40, 20)[0] > 0.2, "in front of the wall: lit");
    assert!(light_at(&gpu, &r, 50, 20)[0] > 0.1, "the wall's face is lit");
    assert!((light_at(&gpu, &r, 60, 20)[0] - 0.1).abs() < 1e-3, "behind it: the ambient");
}

#[test]
fn reach_bounds_a_light() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 40, |_, _| false);
    let r = lit_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &[lamp([20.5, 20.5], 30.0)], 0);
    assert!(light_at(&gpu, &r, 20, 20)[0] > 1.0 - 1e-3, "full at the source (capped at max light)");
    assert!(light_at(&gpu, &r, 40, 20)[0] > 0.1 + 1e-3, "within reach of the radius' edge");
    assert!((light_at(&gpu, &r, 60, 20)[0] - 0.1).abs() < 1e-3, "past reach: the ambient");
}

#[test]
fn an_off_view_light_reaching_the_view_lights_it() {
    let Some(gpu) = offscreen() else { return };
    let map = room(200, 40, |_, _| false);
    // The view is x 100..140; the lamp sits 60 px left of it, outside the grid.
    let r =
        lit_frame(&gpu, &map, View { x: 100.0, y: 0.0, w: 40.0, h: 40.0 }, &[lamp([40.5, 20.5], 100.0)], 0);
    assert!(light_at(&gpu, &r, 101, 20)[0] > 0.1 + 1e-3);
}

#[test]
fn a_light_against_a_wall_lights_the_room() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 40, |x, _| x >= 51);
    let r = lit_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &[lamp([50.5, 20.5], 40.0)], 0);
    assert!(light_at(&gpu, &r, 40, 20)[0] > 0.2, "the room is lit");
    // A lamp inside rock lights nothing.
    let r = lit_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &[lamp([60.5, 20.5], 40.0)], 0);
    assert!((light_at(&gpu, &r, 40, 20)[0] - 0.1).abs() < 1e-3);
    assert!((light_at(&gpu, &r, 60, 20)[0] - 0.1).abs() < 1e-3);
}

#[test]
fn placed_lights_at_the_edges() {
    let Some(gpu) = offscreen() else { return };
    let big = room(4096, 4096, |x, y| x % 512 == 0 && y % 3 == 0);
    let mut lights =
        vec![lamp([2000.5, 2000.5], f32::INFINITY), lamp([-50.0, -50.0], 300.0), lamp([0.5, 0.5], 64.0)];
    lights.extend((0..300).map(|i| lamp([1990.0 + (i % 20) as f32, 1990.0 + (i / 20) as f32], 40.0)));
    let view = View { x: 1950.0, y: 1950.0, w: 100.0, h: 100.0 };
    let a = lit_frame(&gpu, &big, view, &lights, 0);
    let b = lit_frame(&gpu, &big, view, &lights, 0);
    assert_eq!(gpu.read_light(&a).unwrap().1, gpu.read_light(&b).unwrap().1, "deterministic");
    assert!(light_at(&gpu, &a, 2000, 2000)[0] > 0.9);
}

#[test]
fn light_is_camera_independent() {
    let Some(gpu) = offscreen() else { return };
    let map = room(400, 60, |x, y| x % 37 == 0 && y > 20);
    let lights = [lamp([120.5, 30.5], 90.0), lamp([200.5, 10.5], f32::INFINITY)];
    for (softness, soak) in [(0, 0), (2, 0), (0, olr_look::MAX_SOAK), (4, olr_look::MAX_SOAK)] {
        let set = |l: &mut olr_look::LookFrame| {
            l.softness = softness;
            l.light_soak = soak;
        };
        let base = lit_frame_with(&gpu, &map, View { x: 100.0, y: 0.0, w: 120.0, h: 60.0 }, &lights, set);
        for dx in [1, 7, 31, 200] {
            let r = lit_frame_with(
                &gpu,
                &map,
                View { x: 100.0 + dx as f32, y: 0.0, w: 120.0, h: 60.0 },
                &lights,
                set,
            );
            for x in (100 + dx)..220 {
                for y in [5, 30, 55] {
                    if x >= 220 {
                        continue;
                    }
                    assert_eq!(
                        light_at(&gpu, &base, x, y),
                        light_at(&gpu, &r, x, y),
                        "({x}, {y}) moved by {dx}, softness {softness}, soak {soak}"
                    );
                }
            }
        }
    }
}

/// Rock from y 20 down, a lamp in the air above it.
fn ground() -> (ColorMap, View, [olr_look::FrameLight; 1]) {
    let lamp = olr_look::FrameLight { colour: [0.2; 3], ..lamp([50.5, 10.5], 100.0) };
    (room(100, 40, |_, y| y >= 20), View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, [lamp])
}

#[test]
fn soak_lights_rock_near_lit_air() {
    let Some(gpu) = offscreen() else { return };
    let (map, view, lights) = ground();
    let hard = lit_frame_with(&gpu, &map, view, &lights, |_| {});
    assert!((light_at(&gpu, &hard, 50, 23)[0] - 0.1).abs() < 1e-3, "no soak: the ambient past the face");
    let soaked = lit_frame_with(&gpu, &map, view, &lights, |l| l.light_soak = 8);
    let (near, deeper) = (light_at(&gpu, &soaked, 50, 22)[0], light_at(&gpu, &soaked, 50, 25)[0]);
    assert!(near > 0.2 && deeper > 0.1 + 1e-3 && near > deeper, "light fades into the rock: {near} {deeper}");
    assert!((light_at(&gpu, &soaked, 50, 35)[0] - 0.1).abs() < 1e-3, "past the soak: the ambient");
}

#[test]
fn soak_and_floor_never_change_air() {
    let Some(gpu) = offscreen() else { return };
    let (map, view, lights) = ground();
    let hard = lit_frame_with(&gpu, &map, view, &lights, |_| {});
    let soaked = lit_frame_with(&gpu, &map, view, &lights, |l| {
        l.light_soak = olr_look::MAX_SOAK;
        l.solid_floor = [0.5; 3];
    });
    for y in 0..20 {
        for x in 0..100 {
            assert_eq!(light_at(&gpu, &hard, x, y), light_at(&gpu, &soaked, x, y), "air ({x}, {y})");
        }
    }
}

#[test]
fn solid_floor_lights_deep_rock() {
    let Some(gpu) = offscreen() else { return };
    let (map, view, _) = ground();
    let r = lit_frame_with(&gpu, &map, view, &[], |l| l.solid_floor = [0.4, 0.3, 0.2]);
    let deep = light_at(&gpu, &r, 50, 35);
    assert!((deep[0] - 0.4).abs() < 2e-3 && (deep[2] - 0.2).abs() < 2e-3, "{deep:?}");
    assert!((light_at(&gpu, &r, 50, 5)[0] - 0.1).abs() < 1e-3, "air keeps the ambient");
}

/// The user's playtest (2026-10-19): blood and gibs in dark air show at
/// least the solid floor, as rock does; an item without the flag does not.
#[test]
fn a_floored_item_never_falls_below_the_solid_floor() {
    let Some(gpu) = offscreen() else { return };
    let map = room(40, 30, |_, _| false);
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    // The atlas's white texel draws line pixels.
    r.upload_images(&[]);
    r.set_lighting(true);
    r.upload_terrain(&map);
    let mut look = dim(0.1);
    look.solid_floor = [0.4, 0.3, 0.2];
    let mut l = list(View { x: 0.0, y: 0.0, w: 40.0, h: 30.0 });
    l.map = (40, 30);
    let white = Rgba::rgb(255, 255, 255);
    for (x, shade) in [(10, olr_scene::Shade::LIT), (20, olr_scene::Shade::FLOORED)] {
        l.push(Layer::Entities, Item::Line(Line { a: (x, 10), b: (x, 10), colour: white, shade }));
    }
    let img = gpu.render_lit_to_image(&mut r, &l, Some(&look), (80, 60));
    let model =
        |light| olr_render::shade_texel([255; 4], light, 0.0, false, &Default::default(), look.max_light);
    let near = |got: Rgba, want: [u8; 4]| {
        [got.r, got.g, got.b].iter().zip(&want).all(|(a, b)| (i32::from(*a) - i32::from(*b)).abs() <= 1)
    };
    let (lit, fl) = (img.get(20, 20), img.get(40, 20));
    assert!(near(lit, model([0.1; 3])), "lit: the ambient, {lit:?}");
    assert!(near(fl, model([0.4, 0.3, 0.2])), "floored: the solid floor, {fl:?}");
}

/// Plan 15b: a worm's item shows at least the worm floor, not the solid floor.
#[test]
fn a_worm_item_never_falls_below_the_worm_floor() {
    let Some(gpu) = offscreen() else { return };
    let map = room(40, 30, |_, _| false);
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.upload_images(&[]);
    r.set_lighting(true);
    r.upload_terrain(&map);
    let mut look = dim(0.1);
    look.solid_floor = [0.4, 0.3, 0.2];
    look.worm_floor = [0.2, 0.3, 0.4];
    let mut l = list(View { x: 0.0, y: 0.0, w: 40.0, h: 30.0 });
    l.map = (40, 30);
    let white = Rgba::rgb(255, 255, 255);
    let shades = [(10, olr_scene::Shade::LIT), (20, olr_scene::Shade::FLOORED), (30, olr_scene::Shade::WORM)];
    for (x, shade) in shades {
        l.push(Layer::Entities, Item::Line(Line { a: (x, 10), b: (x, 10), colour: white, shade }));
    }
    let img = gpu.render_lit_to_image(&mut r, &l, Some(&look), (80, 60));
    let model =
        |light| olr_render::shade_texel([255; 4], light, 0.0, false, &Default::default(), look.max_light);
    let near = |got: Rgba, want: [u8; 4]| {
        [got.r, got.g, got.b].iter().zip(&want).all(|(a, b)| (i32::from(*a) - i32::from(*b)).abs() <= 1)
    };
    let (lit, fl, worm) = (img.get(20, 20), img.get(40, 20), img.get(60, 20));
    assert!(near(lit, model([0.1; 3])), "lit: the ambient, {lit:?}");
    assert!(near(fl, model([0.4, 0.3, 0.2])), "floored: the solid floor, {fl:?}");
    assert!(near(worm, model([0.2, 0.3, 0.4])), "worm: the worm floor, {worm:?}");
}

#[test]
fn look_softness_blurs() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 40, |x, _| x == 50);
    let lights = [lamp([45.5, 20.5], 30.0)];
    let sharp = lit_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &lights, 0);
    let soft = lit_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &lights, 3);
    // Softness blurs the light in the open (the lamp's falloff changes),
    // but never past the wall (the user's request, 2026-10-19).
    let changed = (0..50).any(|x| light_at(&gpu, &sharp, x, 20) != light_at(&gpu, &soft, x, 20));
    assert!(changed, "softness blurs the light");
    let (s, b) = (light_at(&gpu, &sharp, 52, 20)[0], light_at(&gpu, &soft, 52, 20)[0]);
    assert!((s - 0.1).abs() < 1e-3 && (b - 0.1).abs() < 1e-3, "no light past the wall: {s} {b}");
}

/// [`room`] whose pixels where `glows(x, y)` glow at full strength in
/// group 1 (both front and back).
fn glowing_room(
    w: u32,
    h: u32,
    rock: impl Fn(u32, u32) -> bool,
    glows: impl Fn(u32, u32) -> bool,
) -> ColorMap {
    let mut m = room(w, h, rock);
    let mut g = olr_terrain::GlowLayers::empty((w, h), (w, h));
    for y in 0..h {
        for x in 0..w {
            if glows(x, y) {
                let i = (y * w + x) as usize;
                (g.front[i], g.back[i], g.groups[i], g.groups_back[i]) = (255, 255, 1, 1);
            }
        }
    }
    assert!(m.set_glow(Some(g)));
    m
}

/// A look at ambient 0.1 whose group 1 casts white at `k` with `reach`.
fn glow_look(k: f32, reach: f32) -> olr_look::LookFrame {
    let mut look = dim(0.1);
    look.groups[0] = olr_look::FrameGroup { cast: [k; 3], own_colour: false, self_scale: k, reach };
    look
}

fn glow_frame(gpu: &Offscreen, map: &ColorMap, view: View, look: &olr_look::LookFrame) -> Renderer {
    let mut r = gpu.renderer(RenderSettings::default());
    r.set_lighting(true);
    r.upload_terrain(map);
    let mut l = list(view);
    l.map = (map.material().width(), map.material().height());
    gpu.render_lit_to_image(&mut r, &l, Some(look), (64, 64));
    r
}

#[test]
fn gi_cell_sizes() {
    let big = 16384;
    assert_eq!(olr_render::gi_cell((504, 350), big), 1);
    // Plan 15c: Castle Strike's light at one cell a world pixel, so no
    // thin wall shares a cell with the air beside it.
    assert_eq!(olr_render::gi_cell((1350, 550), big), 1);
    assert_eq!(olr_render::gi_cell((2048, 1024), big), 2);
    assert_eq!(olr_render::gi_cell((4096, 4096), big), 4);
    assert_eq!(olr_render::gi_cell((0, 0), big), 1);
    assert_eq!(olr_render::gi_cell((4096, 64), big), 1);
    // A cascade is up to twice the grid: a small device takes larger cells.
    assert_eq!(olr_render::gi_cell((4096, 64), 8192), 2);
    assert_eq!(olr_render::gi_cell((2048, 40), 2048), 4);
}

#[test]
fn glow_lights_its_surroundings() {
    let Some(gpu) = offscreen() else { return };
    // A lava strip (dirt) along the floor of an open room.
    let map = glowing_room(100, 40, |_, y| y >= 36, |x, y| y >= 36 && (20..80).contains(&x));
    let r = glow_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &glow_look(1.0, 64.0));
    let near = light_at(&gpu, &r, 50, 33)[0];
    let far = light_at(&gpu, &r, 50, 5)[0];
    assert!(near > 0.15, "the strip lights the room above it: {near}");
    assert!(far > 0.1 + 1e-3 && far < near, "less farther up: {far} < {near}");
    assert!(light_at(&gpu, &r, 2, 33)[0] < near, "less to the side");
}

#[test]
fn a_wall_blocks_glow_light() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_room(100, 40, |x, y| x == 50 || y >= 36, |x, y| y >= 36 && x < 40);
    let r = glow_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &glow_look(1.0, 200.0));
    assert!(light_at(&gpu, &r, 30, 30)[0] > 0.15);
    assert!((light_at(&gpu, &r, 75, 20)[0] - 0.1).abs() < 0.01, "behind the wall: about the ambient");
}

#[test]
fn glow_reach_fades() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_room(100, 60, |_, y| y >= 56, |_, y| y >= 56);
    let r = glow_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 60.0 }, &glow_look(1.0, 20.0));
    assert!(light_at(&gpu, &r, 50, 50)[0] > 0.12, "within reach");
    assert!((light_at(&gpu, &r, 50, 20)[0] - 0.1).abs() < 0.01, "past reach: the ambient");
}

#[test]
fn a_deeper_sky_lights_the_same() {
    let Some(gpu) = offscreen() else { return };
    let look = glow_look(1.0, f32::INFINITY);
    // The sky's lower edge stays at row 64; only its depth changes.
    let lit = |depth: u32| {
        let map = glowing_room(100, 128, |_, _| false, |_, y| (64 - depth..64).contains(&y));
        let r = glow_frame(&gpu, &map, View { x: 0.0, y: 64.0, w: 100.0, h: 64.0 }, &look);
        light_at(&gpu, &r, 50, 100)[0]
    };
    let (a, b) = (lit(16), lit(48));
    assert!(a > 0.15, "the sky lights the room: {a}");
    assert!((a - b).abs() < 0.02 * a, "a sky's depth does not change its light: {a} {b}");
}

#[test]
fn no_popping() {
    let Some(gpu) = offscreen() else { return };
    // A sky along the top, open space with pillars down to a floor 600 px
    // below: the sky lights the floor from far outside the view. (A narrow
    // shaft passes almost none of a far sky's light, physically and in the
    // cascades, which resolve far light only at their probe spacing.)
    let rock = |x: u32, y: u32| y >= 690 || (x % 50 == 25 && (100..600).contains(&y));
    let map = glowing_room(200, 700, rock, |_, y| y < 16);
    let look = glow_look(1.0, f32::INFINITY);
    let a = glow_frame(&gpu, &map, View { x: 50.0, y: 600.0, w: 100.0, h: 60.0 }, &look);
    let b = glow_frame(&gpu, &map, View { x: 70.0, y: 630.0, w: 100.0, h: 60.0 }, &look);
    let (la, lb) = (light_at(&gpu, &a, 100, 655), light_at(&gpu, &b, 100, 655));
    assert!(la[0] > 0.1 + 1e-3, "a sky 600 px above lights the floor: {la:?}");
    assert_eq!(la, lb, "wherever the camera is");
}

#[test]
fn group_animation_scales_glow() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_room(100, 40, |_, y| y >= 36, |_, y| y >= 36);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    let full = light_at(&gpu, &glow_frame(&gpu, &map, view, &glow_look(1.0, 64.0)), 50, 30)[0] - 0.1;
    let half = light_at(&gpu, &glow_frame(&gpu, &map, view, &glow_look(0.5, 64.0)), 50, 30)[0] - 0.1;
    assert!(full > 0.02 && (half / full - 0.5).abs() < 0.03, "{half} is half {full}");
}

#[test]
fn glow_light_is_deterministic() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_room(300, 200, |x, y| x % 41 == 0 && y > 50, |x, y| y < 8 || (x + y) % 97 == 0);
    let view = View { x: 50.0, y: 40.0, w: 120.0, h: 80.0 };
    let look = glow_look(0.8, 150.0);
    let a = glow_frame(&gpu, &map, view, &look);
    let b = glow_frame(&gpu, &map, view, &look);
    assert_eq!(gpu.read_light(&a).unwrap().1, gpu.read_light(&b).unwrap().1);
}

/// Sets `map`'s clear flags where `clear(x, y)`.
fn with_clear(mut map: ColorMap, clear: impl Fn(u32, u32) -> bool) -> ColorMap {
    let (w, h) = (map.material().width(), map.material().height());
    let flags = (0..w * h).map(|i| u8::from(clear(i % w, i / w))).collect();
    assert!(map.set_clear(Some(flags)));
    map
}

#[test]
fn a_lamp_shines_through_a_clear_wall() {
    let Some(gpu) = offscreen() else { return };
    let map = with_clear(room(100, 40, |x, _| x == 50), |x, _| x == 50);
    let r =
        lit_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &[lamp([30.5, 20.5], 100.0)], 0);
    assert!(light_at(&gpu, &r, 60, 20)[0] > 0.2, "past the glass: lit");
    // A flag on air changes nothing; the wall's other rows stay opaque.
    let map = with_clear(room(100, 40, |x, _| x == 50), |x, y| x == 20 || (x == 50 && y < 10));
    let r =
        lit_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &[lamp([30.5, 20.5], 100.0)], 0);
    assert!((light_at(&gpu, &r, 60, 20)[0] - 0.1).abs() < 1e-3, "behind the opaque part: the ambient");
}

#[test]
fn glow_light_passes_a_clear_wall() {
    let Some(gpu) = offscreen() else { return };
    let map =
        with_clear(glowing_room(100, 40, |x, y| x == 50 || y >= 36, |x, y| y >= 36 && x < 40), |x, y| {
            x == 50 && y < 36
        });
    let r = glow_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &glow_look(1.0, 200.0));
    assert!(light_at(&gpu, &r, 75, 20)[0] > 0.12, "past the glass: glow light");
}

#[test]
fn a_clear_solid_takes_no_soak_or_floor() {
    let Some(gpu) = offscreen() else { return };
    let (map, view, _) = ground();
    let map = with_clear(map, |_, y| y >= 30);
    let r = lit_frame_with(&gpu, &map, view, &[], |l| {
        l.light_soak = 8;
        l.solid_floor = [0.4; 3];
    });
    assert!((light_at(&gpu, &r, 50, 25)[0] - 0.4).abs() < 2e-3, "opaque rock: the floor");
    assert!((light_at(&gpu, &r, 50, 35)[0] - 0.1).abs() < 1e-3, "clear rock: lit as air");
}

#[test]
fn glow_light_past_the_device_crop_is_open() {
    if offscreen().is_none() {
        return;
    }
    let small = OffscreenOptions { max_texture_side: Some(2048), ..OffscreenOptions::default() };
    let Ok(gpu) = olr_render::Offscreen::open(&small) else { return };
    assert_eq!(gpu.device().limits().max_texture_dimension_2d, 2048);
    // Glowing ground from x 2040 to the level's end at 3000: the device keeps
    // x < 2048, so only 8 columns of it glow; the rest is cut off.
    let glows = |x: u32, y: u32| y >= 36 && x >= 2040;
    let cropped = glowing_room(3000, 40, |_, y| y >= 36, glows);
    let view = View { x: 1950.0, y: 0.0, w: 98.0, h: 40.0 };
    let r = glow_frame(&gpu, &cropped, view, &glow_look(1.0, 400.0));
    // The same 8 columns on a level that ends at the crop.
    let whole = glowing_room(2048, 40, |_, y| y >= 36, glows);
    let w = glow_frame(&gpu, &whole, view, &glow_look(1.0, 400.0));
    let (a, b) = (light_at(&gpu, &r, 2000, 20)[0], light_at(&gpu, &w, 2000, 20)[0]);
    assert!(a < b * 1.5 + 0.01, "past the crop nothing glows: {a} against {b}");
}

/// GL (WSL's GL over Mesa's d3d12, the e2e's software GL) draws into no
/// R32Float target (plan 15a Task 19's e2e): a lit frame with lights and
/// glow draws there too, as on Vulkan.
#[test]
fn a_lit_frame_draws_on_gl() {
    if offscreen().is_none() {
        return;
    }
    let gl = OffscreenOptions { backends: Some(wgpu::Backends::GL), ..OffscreenOptions::default() };
    let Ok(gpu) = olr_render::Offscreen::open(&gl) else {
        eprintln!("no GL adapter; skipping");
        return;
    };
    let map = glowing_room(100, 40, |x, y| x == 50 || y >= 36, |x, y| y >= 36 && x < 40);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    // A worm's occluder (plan 15b) draws there too.
    let body = [([20.0, 10.0, 4.0, 6.0], 1)];
    let r = worm_frame(&gpu, &map, view, &[lamp([30.5, 20.5], 100.0)], &body, |l| {
        l.groups[0] =
            olr_look::FrameGroup { cast: [1.0; 3], own_colour: true, self_scale: 1.0, reach: 200.0 };
        l.light_soak = 4;
    });
    assert!(light_at(&gpu, &r, 40, 20)[0] > 0.2, "the lamp lights");
    assert!((light_at(&gpu, &r, 75, 20)[0] - 0.1).abs() < 0.05, "the wall shadows it");
}

/// A worm's silhouette for the occluder tests (plan 15b): `[x, y, w, h]`
/// world pixels of worm `worm` (its index + 1), all opaque.
type Body = ([f32; 4], u8);

/// [`lit_frame_with`] with each body drawn into the occluder map (atlas
/// image 0, a white 16 × 16 at one texel per world pixel).
fn worm_frame(
    gpu: &Offscreen,
    map: &ColorMap,
    view: View,
    lights: &[olr_look::FrameLight],
    bodies: &[Body],
    set: impl FnOnce(&mut olr_look::LookFrame),
) -> Renderer {
    let mut r = gpu.renderer(RenderSettings::default());
    r.upload_images(&[white_image()]);
    r.set_lighting(true);
    r.upload_terrain(map);
    let mut look = dim(0.1);
    set(&mut look);
    look.emitters.extend_from_slice(lights);
    let mut l = list(view);
    l.map = (map.material().width(), map.material().height());
    push_bodies(&mut l, bodies);
    gpu.render_lit_to_image(&mut r, &l, Some(&look), (64, 64));
    r
}

fn white_image() -> AtlasImage {
    AtlasImage { name: "body".into(), image: solid(16, 16, Rgba::rgb(255, 255, 255)), px_per_world: 1 }
}

fn push_bodies(l: &mut DrawList, bodies: &[Body]) {
    for &([x, y, w, h], worm) in bodies {
        l.occluders.push(olr_scene::Occluder {
            atlas: AtlasId(0),
            src: Rect { x: 0, y: 0, w: w as u32, h: h as u32 },
            pos: [x, y],
            size: [w, h],
            flip: false,
            worm,
        });
    }
}

#[test]
fn a_worm_shadows_the_wall_behind_it() {
    let Some(gpu) = offscreen() else { return };
    let (map, view, lights) = ground();
    let bare = worm_frame(&gpu, &map, view, &lights, &[], |_| {});
    let worm = worm_frame(&gpu, &map, view, &lights, &[([50.0, 13.0, 4.0, 6.0], 1)], |_| {});
    // The rock face (y 20) right under the worm.
    let (lit, shadowed) = (light_at(&gpu, &bare, 51, 20)[0], light_at(&gpu, &worm, 51, 20)[0]);
    assert!(lit > 0.2 && (shadowed - 0.1).abs() < 1e-3, "the worm shadows the face: {lit} {shadowed}");
    assert_eq!(light_at(&gpu, &bare, 30, 20), light_at(&gpu, &worm, 30, 20), "beside it: unchanged");
}

#[test]
fn a_light_inside_a_worm_ignores_it() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 40, |_, _| false);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    let flash = olr_look::FrameLight { pos: [52.0, 20.0], radius: 5.0, colour: [1.0; 3], reach: 40.0 };
    let r = worm_frame(&gpu, &map, view, &[flash], &[([50.0, 17.0, 4.0, 6.0], 1)], |_| {});
    for (x, y) in [(46, 20), (57, 20), (52, 13), (52, 26)] {
        let v = light_at(&gpu, &r, x, y)[0];
        assert!(v > 0.2, "({x}, {y}) lit all round: {v}");
    }
}

#[test]
fn overlapping_worms_block_each_others_lights() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 40, |_, _| false);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    let flash = olr_look::FrameLight { pos: [42.5, 21.5], radius: 2.0, colour: [1.0; 3], reach: 40.0 };
    let pair = [([40.0, 18.0, 4.0, 7.0], 1), ([44.0, 18.0, 4.0, 7.0], 2)];
    let r = worm_frame(&gpu, &map, view, &[flash], &pair, |_| {});
    assert!(light_at(&gpu, &r, 37, 21)[0] > 0.2, "worm 1's near side is lit");
    assert!((light_at(&gpu, &r, 50, 21)[0] - 0.1).abs() < 1e-3, "worm 2's far side: the ambient");
    // 32 worms, some overlapping, ids 1..=32: no panic, the same bytes twice.
    let mut seed = 7u32;
    let mut next = |m: u32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        (seed >> 16) % m
    };
    let many: Vec<Body> = (1..=32).map(|i| ([next(90) as f32, next(30) as f32, 4.0, 6.0], i as u8)).collect();
    let lights = [lamp([20.5, 10.5], 60.0), lamp([70.5, 30.5], 60.0)];
    let a = worm_frame(&gpu, &map, view, &lights, &many, |_| {});
    let b = worm_frame(&gpu, &map, view, &lights, &many, |_| {});
    assert_eq!(gpu.read_light(&a).unwrap().1, gpu.read_light(&b).unwrap().1);
}

#[test]
fn an_off_view_worm_still_shadows_the_view() {
    let Some(gpu) = offscreen() else { return };
    let map = room(200, 40, |x, _| x == 101);
    let view = View { x: 100.0, y: 0.0, w: 100.0, h: 40.0 };
    let lights = [lamp([90.5, 20.5], 60.0)];
    let bare = worm_frame(&gpu, &map, view, &lights, &[], |_| {});
    assert!(light_at(&gpu, &bare, 101, 20)[0] > 0.2, "the face is lit without the worm");
    let r = worm_frame(&gpu, &map, view, &lights, &[([94.0, 17.0, 4.0, 7.0], 1)], |_| {});
    assert!((light_at(&gpu, &r, 101, 20)[0] - 0.1).abs() < 1e-3, "the worm off the view shadows it");
}

#[test]
fn a_worm_blocks_glow_light() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_room(100, 40, |x, y| x == 10 || y >= 30, |x, y| x == 10 && y < 30);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    let glow = |l: &mut olr_look::LookFrame| {
        l.groups[0] = olr_look::FrameGroup { cast: [1.0; 3], own_colour: false, self_scale: 1.0, reach: 64.0 }
    };
    let bare = worm_frame(&gpu, &map, view, &[], &[], glow);
    let worm = worm_frame(&gpu, &map, view, &[], &[([20.0, 22.0, 4.0, 7.0], 1)], glow);
    let (b, w) = (light_at(&gpu, &bare, 26, 27)[0], light_at(&gpu, &worm, 26, 27)[0]);
    assert!(b - w >= 0.05, "behind the worm: darker, {b} {w}");
    let (b, w) = (light_at(&gpu, &bare, 18, 27)[0], light_at(&gpu, &worm, 18, 27)[0]);
    assert!((b - w).abs() < 0.02, "in front of it: about the same, {b} {w}");
}

/// A dark room's frame (ambient 0.05, worm floor 0.2) with each body drawn
/// both as an occluder and as a white `Shade::WORM` sprite (plan 15b), at
/// two output pixels per world pixel: world pixel (x, y) is output pixel
/// (2x, 2y). `more` adds items.
fn lit_worms(
    gpu: &Offscreen,
    map: &ColorMap,
    lights: &[olr_look::FrameLight],
    bodies: &[Body],
    more: impl FnOnce(&mut DrawList),
) -> (Renderer, RgbaImage, olr_look::LookFrame) {
    let (w, h) = (map.material().width(), map.material().height());
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::IntegerNearest });
    r.upload_images(&[white_image()]);
    r.set_lighting(true);
    r.upload_terrain(map);
    let mut look = dim(0.05);
    look.worm_floor = [0.2; 3];
    look.emitters.extend_from_slice(lights);
    let mut l = list(View { x: 0.0, y: 0.0, w: w as f32, h: h as f32 });
    l.map = (w, h);
    push_bodies(&mut l, bodies);
    for &([x, y, bw, bh], _) in bodies {
        l.sprite(
            Layer::Worms,
            Sprite {
                atlas: AtlasId(0),
                src: Rect { x: 0, y: 0, w: bw as u32, h: bh as u32 },
                pos: [x, y],
                size: [bw, bh],
                flip: false,
                tint: Rgba::rgb(255, 255, 255),
                shade: olr_scene::Shade::WORM,
            },
        );
    }
    more(&mut l);
    let img = gpu.render_lit_to_image(&mut r, &l, Some(&look), (2 * w, 2 * h));
    (r, img, look)
}

/// The sRGB a white texel shows under `light`.
fn white_under(light: [f32; 3]) -> u8 {
    olr_render::shade_texel([255; 4], light, 0.0, false, &Default::default(), 1.0)[0]
}

/// World column `x`'s mean red over world rows `ys` of `img`.
fn column(img: &RgbaImage, x: u32, ys: std::ops::Range<u32>) -> f32 {
    let n = ys.len() as f32;
    ys.map(|y| f32::from(img.get(2 * x, 2 * y).r)).sum::<f32>() / n
}

#[test]
fn a_worm_is_lit_on_the_lamps_side() {
    let Some(gpu) = offscreen() else { return };
    let map = room(50, 30, |_, _| false);
    let (_, img, _) =
        lit_worms(&gpu, &map, &[lamp([14.5, 15.0], 30.0)], &[([20.0, 12.0, 4.0, 6.0], 1)], |_| {});
    let (left, inner, right) = (column(&img, 20, 12..18), column(&img, 21, 12..18), column(&img, 23, 12..18));
    assert!(left >= right + 30.0, "the lamp's side is brighter: {left} {right}");
    assert!(inner >= right + 30.0, "the lamp's half, not one column: {inner} {right}");
    let floor = f32::from(white_under([0.2; 3]));
    assert!((right - floor).abs() <= 1.0, "the far side shows the worm floor: {right} {floor}");
}

#[test]
fn a_worm_in_anothers_shadow_keeps_its_floor() {
    let Some(gpu) = offscreen() else { return };
    let map = room(50, 30, |_, _| false);
    let bodies = [([20.0, 12.0, 4.0, 6.0], 1), ([28.0, 12.0, 4.0, 6.0], 2)];
    let (_, img, _) = lit_worms(&gpu, &map, &[lamp([14.5, 15.0], 30.0)], &bodies, |_| {});
    let floor = white_under([0.2; 3]);
    for x in 28..32 {
        for y in 12..18 {
            let v = img.get(2 * x, 2 * y).r;
            assert!(v.saturating_add(1) >= floor, "({x}, {y}): {v} below the floor {floor}");
        }
    }
}

#[test]
fn a_worm_against_a_wall_keeps_its_floor() {
    let Some(gpu) = offscreen() else { return };
    let map = room(50, 30, |x, _| x >= 24);
    let (_, img, _) =
        lit_worms(&gpu, &map, &[lamp([14.5, 15.0], 30.0)], &[([20.0, 12.0, 4.0, 6.0], 1)], |_| {});
    let floor = white_under([0.2; 3]);
    for x in 20..24 {
        for y in 12..18 {
            let v = img.get(2 * x, 2 * y).r;
            assert!(v.saturating_add(1) >= floor, "({x}, {y}): {v} below the floor {floor}");
        }
    }
    assert!(column(&img, 20, 12..18) >= f32::from(floor) + 30.0, "the lamp's side is lit");
}

#[test]
fn worm_shading_is_steady() {
    let Some(gpu) = offscreen() else { return };
    let map = room(50, 30, |_, _| false);
    let lights = [lamp([14.5, 15.0], 30.0)];
    let body = |x: f32| [([x, 12.0, 4.0, 6.0], 1)];
    let (_, a, _) = lit_worms(&gpu, &map, &lights, &body(20.0), |_| {});
    let (_, b, _) = lit_worms(&gpu, &map, &lights, &body(20.0), |_| {});
    assert_eq!(a.as_raw(), b.as_raw(), "the same bytes twice");
    let (_, moved, _) = lit_worms(&gpu, &map, &lights, &body(21.0), |_| {});
    let cols: Vec<f32> = (20..24).map(|x| column(&a, x, 12..18)).collect();
    let step = cols.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    for c in 0..4 {
        let m = column(&moved, 21 + c, 12..18);
        assert!(
            (m - cols[c as usize]).abs() <= step + 2.0,
            "column {c}: {} → {m}, step {step}",
            cols[c as usize]
        );
    }
}

#[test]
fn a_rope_pixel_has_no_side() {
    let Some(gpu) = offscreen() else { return };
    let map = room(50, 30, |_, _| false);
    let rope = |l: &mut DrawList| {
        let white = Rgba::rgb(255, 255, 255);
        for x in [16, 40] {
            l.push(
                Layer::Worms,
                Item::Line(Line { a: (x, 15), b: (x, 15), colour: white, shade: olr_scene::Shade::WORM }),
            );
        }
    };
    let (r, img, look) = lit_worms(&gpu, &map, &[lamp([14.5, 15.0], 30.0)], &[], rope);
    for x in [16, 40] {
        let light = light_at(&gpu, &r, x, 15).map(|v| v.max(0.2));
        let want = white_under(light);
        let got = img.get(2 * x as u32, 30).r;
        assert!(got.abs_diff(want) <= 1, "({x}, 15): {got} want {want} (max_light {})", look.max_light);
    }
}

/// Plan 15b's final review, Important 1, under plan 15c: a muzzle-sized
/// disc in its worm lights all round, whatever the worm's art at its
/// centre (many skins are transparent there in some frames): a disc's
/// cells never count as a worm.
#[test]
fn a_light_ignores_its_own_worm_through_a_hollow() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 40, |_, _| false);
    let mut hollow = solid(4, 6, Rgba::rgb(255, 255, 255));
    hollow.set(2, 3, Rgba::TRANSPARENT);
    let mut r = gpu.renderer(RenderSettings::default());
    r.upload_images(&[AtlasImage { name: "hollow".into(), image: hollow, px_per_world: 1 }]);
    r.set_lighting(true);
    r.upload_terrain(&map);
    let flash = olr_look::FrameLight { pos: [52.5, 20.5], radius: 5.0, colour: [1.0; 3], reach: 40.0 };
    let mut look = dim(0.1);
    look.emitters.push(flash);
    let mut l = list(View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 });
    l.map = (100, 40);
    push_bodies(&mut l, &[([50.0, 17.0, 4.0, 6.0], 1)]);
    gpu.render_lit_to_image(&mut r, &l, Some(&look), (64, 64));
    for (x, y) in [(46, 20), (57, 20), (52, 13), (52, 26)] {
        let v = light_at(&gpu, &r, x, y)[0];
        assert!(v > 0.2, "({x}, {y}) lit all round: {v}");
    }
}

/// The user's request (2026-10-19): light never passes opaque rock, however
/// thin. A one-pixel wall: the air right behind it stays at the ambient,
/// under the most softness.
#[test]
fn a_one_pixel_wall_stops_light_right_behind_it() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 40, |x, _| x == 50);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    for softness in [0, 4] {
        let r = lit_frame_with(&gpu, &map, view, &[lamp([40.5, 20.5], 60.0)], |l| l.softness = softness);
        for y in [15, 20, 25] {
            let v = light_at(&gpu, &r, 51, y)[0];
            assert!((v - 0.1).abs() < 1e-3, "softness {softness}: (51, {y}) {v}");
        }
        assert!(light_at(&gpu, &r, 49, 20)[0] > 0.2, "the lit side");
    }
}

/// A diagonal wall one pixel thin (its pixels touching at corners only)
/// stops light as a straight one does.
#[test]
fn a_diagonal_wall_one_pixel_thin_stops_light() {
    let Some(gpu) = offscreen() else { return };
    // The staircase x - y = 30 from (30, 0) to (69, 39): the lamp below-right of it.
    let map = room(100, 40, |x, y| x == y + 30);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    {
        let r = lit_frame_with(&gpu, &map, view, &[lamp([60.5, 20.5], 80.0)], |_| {});
        let mut leaks = Vec::new();
        for y in 5..35 {
            for x in 0..100 {
                // Above-left of the wall, past its corner pixels.
                if x + 2 < y + 30 && x > y + 10 {
                    let v = light_at(&gpu, &r, x, y)[0];
                    if (v - 0.1).abs() > 1e-3 {
                        leaks.push((x, y, v));
                    }
                }
            }
        }
        assert!(leaks.is_empty(), "light behind the wall at {leaks:?}");
    }
}

/// Glow light stops at a one-pixel wall too.
#[test]
fn a_one_pixel_wall_stops_glow_light() {
    let Some(gpu) = offscreen() else { return };
    let map = glowing_room(100, 40, |x, _| x == 10 || x == 30, |x, _| x == 10);
    let r = glow_frame(&gpu, &map, View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 }, &glow_look(1.0, 64.0));
    let (front, behind) = (light_at(&gpu, &r, 29, 20)[0], light_at(&gpu, &r, 31, 20)[0]);
    assert!(front > 0.15 && (behind - 0.1).abs() < 0.01, "front {front}, behind {behind}");
}

/// A glowing disc (plan 15c): white, `radius`, `reach`.
fn disc(at: [f32; 2], radius: f32, reach: f32) -> olr_look::FrameLight {
    olr_look::FrameLight { pos: at, radius, colour: [1.0; 3], reach }
}

/// [`lit_frame_with`] with `emitters` and nothing else.
fn disc_frame(gpu: &Offscreen, map: &ColorMap, view: View, emitters: &[olr_look::FrameLight]) -> Renderer {
    lit_frame_with(gpu, map, view, &[], |l| l.emitters.extend_from_slice(emitters))
}

/// Plan 15c, review focus 1: a size-500 explosion, a NaN position, an
/// unlimited reach and a black disc draw, and the light stays finite.
#[test]
fn huge_and_hostile_emitters_stay_finite() {
    let Some(gpu) = offscreen() else { return };
    let map = room(300, 120, |x, y| y >= 100 || x == 150);
    let view = View { x: 0.0, y: 0.0, w: 300.0, h: 120.0 };
    let big = olr_look::engine::explosion_light(500);
    let boom =
        olr_look::light_at(&big, 0.0, 0, [100.5, 50.5], olr_look::engine::explosion_radius(500)).unwrap();
    let emitters = [
        boom,
        disc([f32::NAN, 20.0], 4.0, 60.0),
        disc([200.5, 50.5], 6.0, f32::INFINITY),
        olr_look::FrameLight { colour: [0.0; 3], ..disc([50.5, 50.5], 4.0, 60.0) },
        olr_look::FrameLight {
            radius: f32::NAN,
            colour: [f32::INFINITY, 1.0, 1.0],
            ..disc([60.5, 50.5], 4.0, 60.0)
        },
        disc([f32::INFINITY, 1e30], 1e30, 1e30),
    ];
    let r = disc_frame(&gpu, &map, view, &emitters);
    let (_, light) = gpu.read_light(&r).unwrap();
    assert!(light.iter().flatten().all(|v| v.is_finite()), "the light is finite");
    assert!(light_at(&gpu, &r, 110, 50)[0] > 0.2, "the explosion lights");
}

/// Plan 15c ("My calls"): a flash's light at about a third of its reach
/// is today's point light there, and it never passes a one-pixel wall.
#[test]
fn a_flash_lights_its_surroundings_softly() {
    let Some(gpu) = offscreen() else { return };
    let view = View { x: 0.0, y: 0.0, w: 200.0, h: 60.0 };
    let open = room(200, 60, |_, _| false);
    let r = disc_frame(&gpu, &open, view, &[disc([60.5, 30.5], 4.0, 60.0)]);
    let today = olr_look::fade(16.0, olr_look::Reach::Pixels(60));
    let v = light_at(&gpu, &r, 80, 30)[0] - 0.1;
    assert!((v - today).abs() < 0.25 * today, "at d0: {v} against today's {today}");
    let walled = room(200, 60, |x, _| x == 70);
    let r = disc_frame(&gpu, &walled, view, &[disc([60.5, 30.5], 4.0, 60.0)]);
    assert!(light_at(&gpu, &r, 69, 30)[0] > 0.2, "the lit side");
    for y in [20, 25, 30, 35, 40] {
        for x in [71, 72, 75] {
            let v = light_at(&gpu, &r, x, y)[0];
            assert!((v - 0.1).abs() < 1e-3, "behind the wall ({x}, {y}): {v}");
        }
    }
}

/// Plan 15c: a worm's shadow from a disc is soft, never a hard column.
#[test]
fn a_worm_casts_a_soft_shadow() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 60, |x, _| x >= 60);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };
    let flash = [disc([30.5, 30.5], 4.0, 80.0)];
    let r = worm_frame(&gpu, &map, view, &[], &[([40.0, 27.0, 4.0, 6.0], 1)], |l| {
        l.emitters.extend_from_slice(&flash)
    });
    let face: Vec<f32> = (0..60).map(|y| light_at(&gpu, &r, 59, y)[0]).collect();
    let (lo, at) =
        face[10..50].iter().zip(10..).fold((f32::MAX, 0), |m, (&v, y)| if v < m.0 { (v, y) } else { m });
    let hi = face[at - 10..=at + 10].iter().cloned().fold(0.0, f32::max);
    let eps = 0.02 * (hi - lo);
    assert!(hi - lo > 0.05, "a shadow at all: {face:?}");
    for side in [-1i32, 1] {
        let between = (1..=12)
            .map(|k| face[(at as i32 + side * k) as usize])
            .filter(|&v| v > lo + eps && v < hi - eps)
            .count();
        assert!(between >= 3, "side {side}: {between} penumbra pixels in {face:?}");
    }
}

/// Plan 15c, review focus 2: emitters past [`olr_look::MAX_EMITTERS`]
/// change nothing.
#[test]
fn emitters_past_the_cap_change_nothing() {
    let Some(gpu) = offscreen() else { return };
    let map = room(100, 40, |x, _| x == 50);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    let capped: Vec<_> = (0..olr_look::MAX_EMITTERS)
        .map(|i| disc([5.5 + (i % 40) as f32, 5.5 + (i / 40) as f32], 1.0, 8.0))
        .collect();
    let mut more = capped.clone();
    more.extend((0..500).map(|i| disc([70.5 + (i % 20) as f32, 10.5 + (i / 20) as f32], 3.0, 60.0)));
    let a = disc_frame(&gpu, &map, view, &capped);
    let b = disc_frame(&gpu, &map, view, &more);
    assert_eq!(gpu.read_light(&a).unwrap().1, gpu.read_light(&b).unwrap().1);
}

/// Plan 15c, review focus 5: a flash just off the view lights the view as
/// if the camera had moved to include it.
#[test]
fn an_off_view_flash_lights_the_view_the_same() {
    let Some(gpu) = offscreen() else { return };
    let map = room(300, 60, |x, y| x % 41 == 0 && y > 40);
    let flash = [disc([85.5, 30.5], 4.0, 40.0)];
    let off = disc_frame(&gpu, &map, View { x: 100.0, y: 0.0, w: 100.0, h: 60.0 }, &flash);
    let on = disc_frame(&gpu, &map, View { x: 70.0, y: 0.0, w: 100.0, h: 60.0 }, &flash);
    assert!(light_at(&gpu, &off, 101, 30)[0] > 0.11, "it reaches the view");
    for x in 100..170 {
        for y in [5, 30, 45, 55] {
            assert_eq!(light_at(&gpu, &off, x, y), light_at(&gpu, &on, x, y), "({x}, {y})");
        }
    }
}

/// [`room`] whose art is `art(x, y)` (front and back alike).
fn painted_room(w: u32, h: u32, rock: impl Fn(u32, u32) -> bool, art: impl Fn(u32, u32) -> Rgba) -> ColorMap {
    let cells =
        (0..w * h).map(|i| if rock(i % w, i / w) { Material::Rock } else { Material::Background }).collect();
    let t = Terrain::from_grid(&MaterialGrid::from_cells(w, h, cells).unwrap());
    let theme = ThemeArt { holes: Vec::new(), green_mask: None, front_tile: solid(1, 1, Rgba::BLACK) };
    let mut img = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.set(x, y, art(x, y));
        }
    }
    ColorMap::new(&t, &img, &img, 1, theme)
}

/// `n` lit frames of `map` over `view` on one renderer, each with
/// `emitters` and the look `set` changes (ambient 0.1, no light cap).
fn frames(
    gpu: &Offscreen,
    map: &ColorMap,
    view: View,
    emitters: &[olr_look::FrameLight],
    n: usize,
    set: impl Fn(&mut olr_look::LookFrame),
) -> Renderer {
    let mut r = gpu.renderer(RenderSettings::default());
    r.set_lighting(true);
    r.upload_terrain(map);
    let mut look = dim(0.1);
    look.max_light = 1e4;
    look.emitters.extend_from_slice(emitters);
    set(&mut look);
    let mut l = list(view);
    l.map = (map.material().width(), map.material().height());
    for _ in 0..n {
        gpu.render_lit_to_image(&mut r, &l, Some(&look), (64, 64));
    }
    r
}

/// An L-shaped corridor in rock: across at y 10..20 from x 10 to 80, then
/// down at x 70..80 to y 55.
fn corridor(art: impl Fn(u32, u32) -> Rgba) -> ColorMap {
    let open = |x: u32, y: u32| {
        (10..80).contains(&x) && (10..20).contains(&y) || (70..80).contains(&x) && (10..55).contains(&y)
    };
    painted_room(100, 60, |x, y| !open(x, y), art)
}

const WHITE: Rgba = Rgba::rgb(255, 255, 255);

/// Plan 15c: light bounces round a corner no ray from the source reaches.
#[test]
fn light_bounces_round_a_corner() {
    let Some(gpu) = offscreen() else { return };
    let map = corridor(|_, _| WHITE);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };
    let flash = [olr_look::FrameLight { colour: [4.0; 3], ..disc([20.5, 15.5], 3.0, 200.0) }];
    let flat = frames(&gpu, &map, view, &flash, 30, |l| l.bounce = 0.0);
    let v = light_at(&gpu, &flat, 75, 50)[0];
    assert!((v - 0.1).abs() < 1e-3, "no bounce: the ambient round the corner, {v}");
    let lit = frames(&gpu, &map, view, &flash, 30, |l| l.bounce = 0.6);
    let v = light_at(&gpu, &lit, 75, 50)[0];
    assert!(v > 0.12, "bounced light round the corner: {v}");
}

/// Plan 15c: bounced light takes the colour of the wall it bounced off.
#[test]
fn bounce_tints_with_the_wall() {
    let Some(gpu) = offscreen() else { return };
    let map = corridor(|_, _| Rgba::rgb(255, 0, 0));
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };
    let flash = [olr_look::FrameLight { colour: [4.0; 3], ..disc([20.5, 15.5], 3.0, 200.0) }];
    let r = frames(&gpu, &map, view, &flash, 30, |l| {
        l.bounce = 0.6;
        l.ambient = [0.0; 3];
    });
    let [red, green, _] = light_at(&gpu, &r, 75, 50);
    assert!(red > 0.01 && red > 2.0 * green, "red bounce: {red} {green}");
}

/// Plan 15c, review focus 3: a closed white room's feedback converges.
#[test]
fn bounce_never_runs_away() {
    let Some(gpu) = offscreen() else { return };
    let map = painted_room(64, 40, |x, y| x < 2 || y < 2 || x >= 62 || y >= 38, |_, _| WHITE);
    let view = View { x: 0.0, y: 0.0, w: 64.0, h: 40.0 };
    let flash = [disc([32.5, 20.5], 2.0, 100.0)];
    let direct = frames(&gpu, &map, view, &flash, 1, |l| l.bounce = 0.0);
    let near = light_at(&gpu, &direct, 36, 20)[0];
    for bounce in [0.6, 0.9, 5.0] {
        let r = frames(&gpu, &map, view, &flash, 200, |l| l.bounce = bounce);
        let (grid, light) = gpu.read_light(&r).unwrap();
        let mut most = 0.0f32;
        for (i, v) in light.iter().enumerate() {
            assert!(v.iter().all(|c| c.is_finite()));
            // Past the disc itself (its own cells hold its radiance).
            let (x, y) = (
                grid.origin.0 + (i as u32 % grid.size.0) as i32,
                grid.origin.1 + (i as u32 / grid.size.0) as i32,
            );
            if (x as f32 + 0.5 - 32.5).hypot(y as f32 + 0.5 - 20.5) > 3.0 {
                most = most.max(v[0]);
            }
        }
        assert!(most < 4.0 * near, "bounce {bounce}: {most} against {near} at 2 px");
    }
}

/// Plan 15c: a steady scene with bounce settles to the same bytes.
#[test]
fn bounce_settles_to_steady_bytes() {
    let Some(gpu) = offscreen() else { return };
    let map = corridor(|x, y| Rgba::rgb((x * 3) as u8, 200, (y * 4) as u8));
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 60.0 };
    let flash = [disc([20.5, 15.5], 3.0, 200.0)];
    let mut r = frames(&gpu, &map, view, &flash, 60, |l| l.bounce = 0.6);
    let a = gpu.read_light(&r).unwrap().1;
    let mut look = dim(0.1);
    look.max_light = 1e4;
    look.bounce = 0.6;
    look.emitters.extend_from_slice(&flash);
    let mut l = list(view);
    l.map = (100, 60);
    gpu.render_lit_to_image(&mut r, &l, Some(&look), (64, 64));
    assert_eq!(a, gpu.read_light(&r).unwrap().1);
}

/// Plan 15c (the user's request, 2026-10-19): bounced light never passes a
/// wall one pixel thin.
#[test]
fn no_bounce_through_a_thin_wall() {
    let Some(gpu) = offscreen() else { return };
    let map = painted_room(100, 40, |x, y| x == 50 || y < 2 || y >= 38 || x < 2 || x >= 98, |_, _| WHITE);
    let view = View { x: 0.0, y: 0.0, w: 100.0, h: 40.0 };
    let flash = [olr_look::FrameLight { colour: [4.0; 3], ..disc([30.5, 20.5], 3.0, 200.0) }];
    for bounce in [0.6, 0.9] {
        let r = frames(&gpu, &map, view, &flash, 30, |l| l.bounce = bounce);
        assert!(light_at(&gpu, &r, 49, 20)[0] > 0.2);
        for y in (3..37).step_by(3) {
            for x in [51, 52, 60, 80, 96] {
                let v = light_at(&gpu, &r, x, y)[0];
                assert!((v - 0.1).abs() < 1e-3, "bounce {bounce}: ({x}, {y}) {v}");
            }
        }
    }
}

/// Plan 15c, review focus 4: a new level's light starts afresh, whatever
/// the last level's history.
#[test]
fn a_new_level_starts_its_light_afresh() {
    let Some(gpu) = offscreen() else { return };
    let a = corridor(|_, _| WHITE);
    let b = painted_room(80, 50, |x, y| y >= 40 || x == 30, |_, _| WHITE);
    let flash = [disc([20.5, 15.5], 3.0, 200.0)];
    let mut r = frames(&gpu, &a, View { x: 0.0, y: 0.0, w: 100.0, h: 60.0 }, &flash, 30, |l| l.bounce = 0.6);
    r.upload_terrain(&b);
    let view = View { x: 0.0, y: 0.0, w: 80.0, h: 50.0 };
    let mut look = dim(0.1);
    look.max_light = 1e4;
    look.bounce = 0.6;
    look.emitters.extend_from_slice(&flash);
    let mut l = list(view);
    l.map = (80, 50);
    gpu.render_lit_to_image(&mut r, &l, Some(&look), (64, 64));
    let fresh = frames(&gpu, &b, view, &flash, 1, |l| l.bounce = 0.6);
    assert_eq!(gpu.read_light(&r).unwrap().1, gpu.read_light(&fresh).unwrap().1);
}
