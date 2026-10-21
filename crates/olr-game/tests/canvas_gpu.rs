//! The editor's canvas on a GPU (plan 14c Task 6): the fixture's bake drawn
//! by `olr-render`; an edit's rectangle uploaded draws the same picture as
//! the whole map uploaded afresh. Runs with `OLR_GPU_TESTS` (`cargo xtask
//! full`) and the bundled content; skips without either, or an adapter.

use std::path::{Path, PathBuf};

use olr_data::image::Rgba;
use olr_edit::level::{Paint, Shape};
use olr_editor::view::View;
use olr_editor::{Catalog, Document, ThemeEntry};
use olr_game::editor::canvas::Canvas;
use olr_render::{Offscreen, RenderSettings, UpscaleFilter};

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.path().is_dir() {
            copy(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

#[test]
fn the_canvas_draws_the_bake() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        eprintln!("OLR_GPU_TESTS not set (`cargo xtask full` sets it); skipping");
        return;
    }
    let Some(root) = std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir()) else {
        eprintln!("OLR_CONTENT_DIR not set; skipping");
        return;
    };
    let gpu = match Offscreen::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{e}; skipping");
            return;
        }
    };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("level");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edit/composed-level"), &dir);
    let theme =
        olr_format::themes::read_theme_files(&olr_vfs::DirFs::new(root.join("olx-theme-dirt"))).unwrap();
    let catalog = Catalog {
        themes: vec![ThemeEntry {
            id: "olx-theme-dirt".into(),
            name: "Dirt".into(),
            front_tile: theme.front_tile,
            back_tile: theme.back_tile,
            holes: Vec::new(),
        }],
        mods: vec![],
        home: None,
        textures: Vec::new(),
        assets: Vec::new(),
    };
    let mut doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &catalog).unwrap();
    let settings = RenderSettings { filter: UpscaleFilter::SharpBilinear };
    let size = (640u32, 400u32);
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(size.0 as f32, size.1 as f32));
    let l = doc.level();
    let view = View::fit([size.0 as f32, size.1 as f32], (l.size.0 * l.art_scale, l.size.1 * l.art_scale));
    let picture = |canvas: &mut Canvas, r: &mut olr_render::Renderer| {
        let (list, _) = canvas.draw(&view, rect, 1.0, olr_render::MAX_VIEW).expect("the level shows");
        gpu.render_to_image(r, &list, size)
    };

    let mut canvas = Canvas::new();
    canvas.rebuild(&doc, false);
    let mut r = gpu.renderer(settings);
    r.upload_terrain(canvas.map_mut().unwrap());
    let before = picture(&mut canvas, &mut r);
    let painted = doc
        .paint(
            "tunnels",
            &Shape::Rect { x: 60, y: 40, w: 50, h: 30 },
            &Paint::Colour(Rgba::rgb(255, 0, 255)),
            None,
        )
        .unwrap()
        .expect("the paint changed the bake");
    canvas.upload(&doc, painted);
    r.update_terrain(canvas.map_mut().unwrap());
    let updated = picture(&mut canvas, &mut r);
    assert_ne!(updated, before, "the paint shows");

    let mut fresh = Canvas::new();
    fresh.rebuild(&doc, false);
    let mut r2 = gpu.renderer(settings);
    r2.upload_terrain(fresh.map_mut().unwrap());
    assert_eq!(updated, picture(&mut fresh, &mut r2), "a rectangle upload equals a full one");
}

/// Plan 15a Task 19: a level with a look draws lit on the canvas: darker
/// than its art with a dark ambient, its art again in Full bright.
#[test]
fn the_canvas_draws_lit() {
    if std::env::var_os("OLR_GPU_TESTS").is_none() {
        return;
    }
    let Some(root) = std::env::var_os("OLR_CONTENT_DIR").map(PathBuf::from).filter(|d| d.is_dir()) else {
        return;
    };
    let Ok(gpu) = Offscreen::new() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("level");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edit/composed-level"), &dir);
    let theme =
        olr_format::themes::read_theme_files(&olr_vfs::DirFs::new(root.join("olx-theme-dirt"))).unwrap();
    let catalog = Catalog {
        themes: vec![ThemeEntry {
            id: "olx-theme-dirt".into(),
            name: "Dirt".into(),
            front_tile: theme.front_tile,
            back_tile: theme.back_tile,
            holes: Vec::new(),
        }],
        mods: vec![],
        home: None,
        textures: Vec::new(),
        assets: Vec::new(),
    };
    let mut doc = Document::open(olr_edit::pack::load(&dir).unwrap(), dir, &catalog).unwrap();
    let mut look = olr_format::look::LookSource::default();
    look.ambient.intensity = 20;
    doc.set_look(Some(look)).unwrap();
    let size = (640u32, 400u32);
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(size.0 as f32, size.1 as f32));
    let l = doc.level();
    let view = View::fit([size.0 as f32, size.1 as f32], (l.size.0 * l.art_scale, l.size.1 * l.art_scale));
    let mut canvas = Canvas::new();
    canvas.rebuild(&doc, false);
    assert!(canvas.lit(&doc));
    let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::SharpBilinear });
    r.set_lighting(true);
    r.upload_terrain(canvas.map_mut().unwrap());
    let (list, _) = canvas.draw(&view, rect, 1.0, olr_render::MAX_VIEW).unwrap();
    let mean = |img: &olr_data::image::RgbaImage| {
        img.as_raw().chunks(4).map(|p| u64::from(p[0]) + u64::from(p[1]) + u64::from(p[2])).sum::<u64>()
    };
    let frame = canvas.look_frame(&doc, 0.0, olr_look::ViewMode::Lit).cloned();
    let lit = gpu.render_lit_to_image(&mut r, &list, frame.as_ref(), size);
    let full = canvas.look_frame(&doc, 0.0, olr_look::ViewMode::FullBright).cloned();
    let bright = gpu.render_lit_to_image(&mut r, &list, full.as_ref(), size);
    let plain = {
        let mut r = gpu.renderer(RenderSettings { filter: UpscaleFilter::SharpBilinear });
        r.upload_terrain(canvas.map_mut().unwrap());
        gpu.render_to_image(&mut r, &list, size)
    };
    assert!(mean(&lit) < mean(&plain) * 3 / 4, "a dark ambient darkens the canvas");
    assert_eq!(bright, plain, "full bright is the art as it is");
}
