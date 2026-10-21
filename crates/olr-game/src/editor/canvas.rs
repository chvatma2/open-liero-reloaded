//! The editor's canvas on the GPU (plan 14c R1, R2, spec §4): the open
//! level's bake as an `olr_terrain::ColorMap`, drawn by `olr-render` (what
//! you see is what plays) under the editor's egui panels and overlays.
//! Each edit replaces only its dirty rectangle (`ColorMap::replace_rect`,
//! then `Renderer::update_terrain`). The material view is a second map of
//! `MATERIAL_PALETTE` colours at art scale 1, built when it is turned on.
//! A level with a look draws lit (plan 15a Task 19): the map carries the
//! bake's glow and clear solids, and `look_frame` the document's look at
//! the editor's clock and view mode.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{DistantLayer, Material, MaterialGrid};
use olr_edit::compose::Rect;
use olr_editor::Document;
use olr_editor::view::View as EditorView;
use olr_format::levels::{LevelSource, MATERIAL_PALETTE};
use olr_look::{LevelLook, LookFrame, ViewMode};
use olr_scene::{DrawList, View};
use olr_terrain::{ColorMap, Terrain, ThemeArt};

/// The canvas's colour map and what it was built from.
pub struct Canvas {
    map: Option<ColorMap>,
    /// The map shows `MATERIAL_PALETTE` (art scale 1), not the bake.
    showing_material: bool,
    /// The level's size (world pixels) and art scale: the editor's view
    /// maps the level's art pixels whichever map shows.
    level: (u32, u32),
    art_scale: u32,
    /// The material view's images, kept so an upload repaints only its
    /// rectangle.
    palette: Option<(RgbaImage, RgbaImage)>,
    /// The document's look compiled, and the look it was compiled from:
    /// compiled again only when it changes.
    look: Option<(Option<LevelLook>, Option<olr_format::look::LookSource>)>,
    /// The frame's look, reused.
    frame: LookFrame,
    /// The level's distant layers (issue #11; `HostOut::Distant`), and
    /// whether the renderer has not had them yet.
    distant: Vec<DistantLayer>,
    distant_due: bool,
}

/// No stamps and no tiles: the canvas replays no simulation edit.
fn no_theme() -> ThemeArt {
    ThemeArt { holes: Vec::new(), green_mask: None, front_tile: RgbaImage::new(0, 0) }
}

/// `MATERIAL_PALETTE`'s colour of `m`.
fn palette(m: Material) -> Rgba {
    let i = m as usize * 3;
    Rgba::rgb(MATERIAL_PALETTE[i], MATERIAL_PALETTE[i + 1], MATERIAL_PALETTE[i + 2])
}

/// The level's material as a terrain (a level of another cell count, which
/// `olr-format` never makes, is all rock).
fn terrain(l: &LevelSource) -> Terrain {
    let grid = MaterialGrid::from_cells(l.size.0, l.size.1, l.material.clone())
        .unwrap_or_else(|| MaterialGrid::new(l.size.0, l.size.1, Material::Rock));
    Terrain::from_grid(&grid)
}

impl Default for Canvas {
    fn default() -> Canvas {
        Canvas::new()
    }
}

impl Canvas {
    pub fn new() -> Canvas {
        Canvas {
            map: None,
            showing_material: false,
            level: (0, 0),
            art_scale: 1,
            palette: None,
            look: None,
            frame: LookFrame::default(),
            distant: Vec::new(),
            distant_due: false,
        }
    }

    /// Rebuilds the map from `doc` (a new document, a resize or an art
    /// scale changes its size; the material view turned on or off). The
    /// caller uploads it whole (`Renderer::upload_terrain`).
    pub fn rebuild(&mut self, doc: &Document, material_view: bool) {
        let l = doc.level();
        self.level = l.size;
        self.art_scale = l.art_scale.max(1);
        self.showing_material = material_view;
        let t = terrain(l);
        self.map = Some(if material_view {
            let mut img = RgbaImage::new(l.size.0, l.size.1);
            for (i, m) in l.material.iter().enumerate().take((l.size.0 * l.size.1) as usize) {
                img.set(i as u32 % l.size.0, i as u32 / l.size.0, palette(*m));
            }
            let map = ColorMap::new(&t, &img, &img, 1, no_theme());
            self.palette = Some((img.clone(), img));
            map
        } else {
            self.palette = None;
            // A level with distant layers keeps its see-through back
            // pixels (issue #11).
            let mut map = if doc.see_through() {
                ColorMap::new_see_through(&t, &l.front, &l.back, l.art_scale, no_theme())
            } else {
                ColorMap::new(&t, &l.front, &l.back, l.art_scale, no_theme())
            };
            // The bake's glow and clear solids (they always fit its size).
            map.set_glow(l.glow.clone());
            map.set_clear(l.clear.clone());
            map
        });
    }

    /// Whether the canvas draws `doc` lit: a look, glow or clear solids,
    /// and not the material view. The host turns the
    /// renderer's lighting on and uploads the map whole when it changes.
    pub fn lit(&self, doc: &Document) -> bool {
        let l = doc.level();
        !self.showing_material && (l.look.is_some() || l.glow.is_some() || l.clear.is_some())
    }

    /// The frame's look of `doc` at clock `t_ms` (the editor's) in `mode`;
    /// `None` when the canvas is not lit. A look that does not check draws
    /// as no look.
    pub fn look_frame(&mut self, doc: &Document, t_ms: f64, mode: ViewMode) -> Option<&LookFrame> {
        if !self.lit(doc) {
            return None;
        }
        let l = doc.level();
        if self.look.as_ref().is_none_or(|(_, look)| *look != l.look) {
            let compiled = olr_format::levels::compile_look(l).ok().flatten();
            self.look = Some((compiled, l.look.clone()));
        }
        let level = self.look.as_ref().and_then(|(c, ..)| c.as_ref());
        self.frame.mode = mode;
        self.frame.begin(level, t_ms);
        Some(&self.frame)
    }

    /// Drops the map (the editor closed, or its Play runs: the session has
    /// its own); the next frame of the editor makes it again.
    pub fn clear(&mut self) {
        self.map = None;
        self.palette = None;
    }

    /// Whether the map shows the material view.
    pub fn showing_material(&self) -> bool {
        self.showing_material
    }

    /// Whether the map matches `doc`'s size and art scale (else it must be
    /// rebuilt: a resize or an art-scale change).
    pub fn fits(&self, doc: &Document) -> bool {
        let l = doc.level();
        // A glow that came or went changes the map's channels: built again.
        let glow = self.showing_material
            || self.map.as_ref().is_some_and(|m| m.glow().is_some() == l.glow.is_some());
        // So does a see-through bake that came or went (issue #11).
        let see_through =
            self.showing_material || self.map.as_ref().is_some_and(|m| m.see_through() == doc.see_through());
        self.map.is_some()
            && self.level == l.size
            && self.art_scale == l.art_scale.max(1)
            && glow
            && see_through
    }

    /// The document's distant layers changed (`HostOut::Distant`).
    pub fn set_distant(&mut self, layers: Vec<DistantLayer>) {
        self.distant = layers;
        self.distant_due = true;
    }

    /// The distant layers for `Renderer::set_distant`, when they changed or
    /// `full` (a whole upload); none in the material view, which leaves them
    /// out.
    pub fn take_distant(&mut self, full: bool) -> Option<&[DistantLayer]> {
        if !std::mem::take(&mut self.distant_due) && !full {
            return None;
        }
        Some(if self.showing_material { &[] } else { &self.distant })
    }

    /// `doc`'s world rectangle `r` into the map (`replace_rect`, art pixels
    /// = `r` × the map's art scale), marked dirty for `update_terrain`:
    /// the rectangle's work and copies only.
    pub fn upload(&mut self, doc: &Document, r: Rect) {
        let Some(map) = &mut self.map else { return };
        let l = doc.level();
        let cells = l.size.0 as usize * l.size.1 as usize;
        if l.size != self.level || r.w == 0 || r.h == 0 || l.material.len() != cells {
            return;
        }
        let s = map.art_scale();
        let art = olr_terrain::Rect { x: r.x * s, y: r.y * s, w: r.w * s, h: r.h * s };
        match &mut self.palette {
            Some((front, back)) => {
                let (x1, y1) = ((r.x + r.w).min(l.size.0), (r.y + r.h).min(l.size.1));
                for y in r.y..y1 {
                    for x in r.x..x1 {
                        let c = palette(l.material[(y * l.size.0 + x) as usize]);
                        front.set(x, y, c);
                        back.set(x, y, c);
                    }
                }
                map.replace_rect(art, front, back, &l.material);
            }
            None => {
                map.replace_rect(art, &l.front, &l.back, &l.material);
                if let Some(g) = &l.glow {
                    map.replace_glow_rect(art, g);
                }
                // Clear flags change with an element's Clear (its rectangle
                // uploads): the renderer reads them with the rectangle.
                if map.clear() != l.clear.as_deref() {
                    map.set_clear(l.clear.clone());
                }
            }
        }
    }

    /// The terrain-only draw list and the physical-pixel area for `view`
    /// over `canvas` (egui points) at `pixels_per_point`: the world under
    /// the whole canvas (black where there is no level; a size that panning
    /// never changes, so the renderer's world target stays), cut at
    /// `max_view` world pixels per side (at most `MAX_VIEW`; less when the
    /// device's textures are smaller: `max_view_for`; then over the visible
    /// level), and the screen rectangle it fills. `None` when no level is open.
    pub fn draw(
        &self,
        view: &EditorView,
        canvas: egui::Rect,
        pixels_per_point: f32,
        max_view: f32,
    ) -> Option<(DrawList, [u32; 4])> {
        self.map.as_ref()?;
        let unit = view.scale() * self.art_scale as f32; // points per world pixel
        if unit.is_nan() || unit <= 0.0 || !canvas.is_positive() {
            return None;
        }
        let max = max_view.clamp(1.0, olr_render::MAX_VIEW);
        // One axis: the view's start and length in world pixels, and where it
        // starts on screen (points). The whole canvas when it fits; else a
        // `max` long window over the visible level (its size still fixed).
        let axis = |offset: f32, len: f32, lo: f32| {
            let (start, all) = (-offset / unit, len / unit);
            if all <= max {
                return (start, all, lo);
            }
            let x = start.max(0.0).clamp(start, start + all - max);
            (x, max, lo + (x - start) * unit)
        };
        let (x, w, sx) = axis(view.offset[0], canvas.width(), canvas.min.x);
        let (y, h, sy) = axis(view.offset[1], canvas.height(), canvas.min.y);
        let px = |v: f32| (v * pixels_per_point).round().max(0.0) as u32;
        let (sx0, sy0) = (px(sx), px(sy));
        let (sx1, sy1) = (px(sx + w * unit), px(sy + h * unit));
        if sx1 <= sx0 || sy1 <= sy0 {
            return None;
        }
        let mut list = DrawList::default();
        list.view = View { x, y, w, h };
        list.map = self.level;
        // Distant layers scroll by the canvas's centre (issue #11), not
        // the cut view's: as the editor hit-tests them.
        list.parallax = Some(view.centre([canvas.width(), canvas.height()], self.art_scale));
        Some((list, [sx0, sy0, sx1 - sx0, sy1 - sy0]))
    }

    /// The widest view in world pixels whose world pass fits a device
    /// texture of `max_texels` at the renderer's `scale` (texels per world
    /// pixel), its margins included.
    pub fn max_view_for(max_texels: u32, scale: u32) -> f32 {
        ((max_texels / scale.max(1)) as f32 - 2.0 - 2.0 * olr_render::MARGIN as f32).max(1.0)
    }

    /// The map, for `Renderer::upload_terrain` and `update_terrain`.
    pub fn map_mut(&mut self) -> Option<&mut ColorMap> {
        self.map.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Option<(tempfile::TempDir, Document)> {
        let root = std::path::PathBuf::from(std::env::var_os("OLR_CONTENT_DIR")?);
        if !root.is_dir() {
            return None;
        }
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/edit/composed-level");
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("level");
        copy(&fixture, &dir);
        let theme =
            olr_format::themes::read_theme_files(&olr_vfs::DirFs::new(root.join("olx-theme-dirt"))).unwrap();
        let catalog = olr_editor::Catalog {
            themes: vec![olr_editor::ThemeEntry {
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
        let pack = olr_edit::pack::load(&dir).unwrap();
        Some((tmp, Document::open(pack, dir, &catalog).unwrap()))
    }

    fn copy(from: &std::path::Path, to: &std::path::Path) {
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

    /// A canvas over a level of `size` at art scale `s`, without a document.
    fn canvas(size: (u32, u32), s: u32) -> Canvas {
        let t = Terrain::from_grid(&MaterialGrid::new(size.0, size.1, Material::Dirt));
        let img = RgbaImage::new(size.0 * s, size.1 * s);
        Canvas {
            map: Some(ColorMap::new(&t, &img, &img, s, no_theme())),
            showing_material: false,
            level: size,
            art_scale: s,
            palette: None,
            look: None,
            frame: LookFrame::default(),
            distant: Vec::new(),
            distant_due: false,
        }
    }

    #[test]
    fn draw_maps_the_view_to_the_canvas() {
        let c = canvas((504, 350), 2);
        let view = EditorView { zoom: 3, offset: [10.0, 20.0] };
        assert_eq!(view.scale(), 2.0);
        let rect = egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(500.0, 350.0));
        let (list, area) = c.draw(&view, rect, 1.5, olr_render::MAX_VIEW).unwrap();
        // 4 points per world pixel: the whole canvas, from world (-2.5, -5)
        // (black where there is no level).
        assert_eq!((list.view.x, list.view.y), (-2.5, -5.0));
        assert_eq!((list.view.w, list.view.h), (400.0 / 4.0, 300.0 / 4.0));
        assert_eq!(list.map, (504, 350));
        assert_eq!(area, [150, 75, 600, 450], "the canvas's physical rectangle");
        // Panning, a level edge in view or not, keeps the view's size: the
        // renderer's world target is not made again (deferred minor of batch C).
        for offset in [[-300.0, -100.0], [0.0, 0.0], [-5000.0, 0.0], [123.4, 56.7]] {
            let (l, a) = c.draw(&EditorView { zoom: 3, offset }, rect, 1.5, olr_render::MAX_VIEW).unwrap();
            assert_eq!((l.view.w, l.view.h, a), (100.0, 75.0, area), "{offset:?}");
        }
        // A view wider than the device's world target allows is cut there.
        let wide = canvas((4096, 100), 1);
        let far = EditorView { zoom: 0, offset: [0.0, 0.0] };
        let huge = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(4000.0, 100.0));
        let (list, area) = wide.draw(&far, huge, 1.0, olr_render::MAX_VIEW).unwrap();
        assert_eq!(list.view.w, olr_render::MAX_VIEW);
        assert_eq!(area[2], 1024, "4096 world pixels at a quarter point each");
        // Panned right on that canvas, the cut view follows the level, not
        // the canvas's left edge (review of the minors round).
        let right = EditorView { zoom: 0, offset: [2000.0, 0.0] };
        let (list, area) = wide.draw(&right, huge, 1.0, olr_render::MAX_VIEW).unwrap();
        assert_eq!((list.view.x, list.view.w), (0.0, olr_render::MAX_VIEW), "the level's left edge on");
        assert_eq!((area[0], area[2]), (2000, 1024), "drawn where the level is");
        for offset in [[2100.0, 0.0], [-500.0, 0.0], [3990.0, 0.0]] {
            let (l, a) = wide.draw(&EditorView { zoom: 0, offset }, huge, 1.0, olr_render::MAX_VIEW).unwrap();
            assert_eq!(l.view.w, olr_render::MAX_VIEW, "{offset:?}: its size stays");
            assert!(a[0] + a[2] <= 4000, "{offset:?}: inside the canvas");
        }
        let max = Canvas::max_view_for(4096, 2);
        let (list, _) = wide.draw(&far, huge, 1.0, max).unwrap();
        assert_eq!(list.view.w, max);
        assert!((list.view.w.ceil() + 1.0 + 2.0 * olr_render::MARGIN as f32) * 2.0 <= 4096.0);
        assert!(Canvas::new().draw(&far, huge, 1.0, olr_render::MAX_VIEW).is_none(), "no level, nothing");
    }

    #[test]
    fn upload_replaces_only_its_rectangle() {
        let Some((_tmp, mut d)) = doc() else { return };
        let mut c = Canvas::new();
        c.rebuild(&d, false);
        assert!(c.fits(&d));
        let s = d.level().art_scale;
        c.map_mut().unwrap().take_dirty().for_each(drop);
        let before = c.map_mut().unwrap().pixels().to_vec();
        // Paint red into the drawn layer, as a brush stroke does.
        let shape = olr_edit::level::Shape::Rect { x: 40, y: 30, w: 6, h: 4 };
        let r = d
            .paint("tunnels", &shape, &olr_edit::level::Paint::Colour(Rgba::rgb(250, 0, 0)), None)
            .unwrap()
            .unwrap();
        c.upload(&d, r);
        let map = c.map_mut().unwrap();
        let w = map.width();
        let changed: Vec<(u32, u32)> = map
            .pixels()
            .chunks(4)
            .zip(before.chunks(4))
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| (i as u32 % w, i as u32 / w))
            .collect();
        assert!(!changed.is_empty(), "the paint shows");
        for (x, y) in changed {
            assert!(
                (r.x * s..(r.x + r.w) * s).contains(&x) && (r.y * s..(r.y + r.h) * s).contains(&y),
                "({x}, {y}) is outside {r:?} × {s}"
            );
        }
        assert!(map.take_dirty().next().is_some(), "marked for update_terrain");
        // The material view's map shows MATERIAL_PALETTE's colours.
        c.rebuild(&d, true);
        assert!(c.showing_material());
        let l = d.level();
        let map = c.map_mut().unwrap();
        assert_eq!(map.art_scale(), 1);
        for (x, y) in [(0u32, 0u32), (100, 100), (l.size.0 - 1, l.size.1 - 1)] {
            let m = l.material[(y * l.size.0 + x) as usize];
            assert_eq!(map.color_at(x.into(), y.into()), Some(palette(m)));
        }
    }

    /// Plan 15a Task 19: the canvas's look frame follows the document's
    /// look, the editor's clock and view mode.
    #[test]
    fn the_canvas_frame_follows_the_document() {
        let Some((_tmp, mut d)) = doc() else { return };
        let mut c = Canvas::new();
        c.rebuild(&d, false);
        assert!(!c.lit(&d), "no look: drawn as always");
        assert!(c.look_frame(&d, 0.0, ViewMode::Lit).is_none());
        let mut look = olr_format::look::LookSource::default();
        look.ambient.intensity = 30;
        d.set_look(Some(look)).unwrap();
        assert!(c.lit(&d));
        let f = c.look_frame(&d, 0.0, ViewMode::Lit).expect("lit");
        assert!((f.ambient[0] - 0.3).abs() < 1e-3 && f.emitters.is_empty());
        assert_eq!(c.look_frame(&d, 0.0, ViewMode::FullBright).unwrap().mode, ViewMode::FullBright);
        // The material view is never lit.
        c.rebuild(&d, true);
        assert!(!c.lit(&d));
    }

    /// Issue #11: a distant layer makes the map see-through (built again),
    /// its layers reach the renderer when they change and with every whole
    /// upload (none in the material view), and the draw list's camera is
    /// the canvas's centre.
    #[test]
    fn distant_layers_reach_the_canvas() {
        let Some((_tmp, mut d)) = doc() else { return };
        let mut c = Canvas::new();
        c.rebuild(&d, false);
        assert!(!c.map_mut().unwrap().see_through());
        d.set_distant("tunnels", Some(40)).unwrap();
        assert!(d.see_through());
        assert!(!c.fits(&d), "the map is not see-through yet: built again");
        c.rebuild(&d, false);
        assert!(c.fits(&d) && c.map_mut().unwrap().see_through());
        c.set_distant(d.level().distant.clone());
        assert_eq!(c.take_distant(false).map(<[_]>::len), Some(1));
        assert!(c.take_distant(false).is_none(), "sent once");
        assert_eq!(c.take_distant(true).map(<[_]>::len), Some(1), "again with a whole upload");
        let view = EditorView { zoom: 3, offset: [10.0, 20.0] };
        let rect = egui::Rect::from_min_max(egui::pos2(100.0, 50.0), egui::pos2(500.0, 350.0));
        let (list, _) = c.draw(&view, rect, 1.5, olr_render::MAX_VIEW).unwrap();
        assert_eq!(list.parallax, Some(view.centre([400.0, 300.0], d.level().art_scale)));
        assert_eq!(list.camera(), (list.view.x + list.view.w / 2.0, list.view.y + list.view.h / 2.0));
        c.rebuild(&d, true);
        assert!(c.fits(&d), "the material view is never see-through");
        assert_eq!(c.take_distant(true).map(<[_]>::len), Some(0), "the material view leaves them out");
    }

    #[test]
    fn a_glow_edit_uploads_its_rectangle() {
        let Some((_tmp, mut d)) = doc() else { return };
        let mut c = Canvas::new();
        c.rebuild(&d, false);
        let glow = olr_format::levels::ElementGlow { strength: 100, group: String::new(), mask: false };
        d.set_glow("tunnels", Some(glow)).unwrap();
        assert!(!c.fits(&d), "the map has no glow yet: built again");
        c.rebuild(&d, false);
        assert_eq!(c.map_mut().unwrap().glow(), d.level().glow.as_ref().map(|g| &g.front[..]));
        c.map_mut().unwrap().take_dirty().for_each(drop);
        let r =
            d.paint_glow("tunnels", &olr_edit::level::Shape::Rect { x: 40, y: 30, w: 6, h: 4 }, 0).unwrap();
        assert!(c.fits(&d));
        c.upload(&d, r);
        let map = c.map_mut().unwrap();
        assert_eq!(map.glow(), d.level().glow.as_ref().map(|g| &g.front[..]), "the glow follows");
        assert!(map.take_dirty().next().is_some());
    }
}
