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
        }],
        mods: vec![],
        home: None,
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
        .paint("tunnels", &Shape::Rect { x: 60, y: 40, w: 50, h: 30 }, &Paint::Colour(Rgba::rgb(255, 0, 255)))
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
