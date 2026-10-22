//! The material editor's live heat preview (issue #45, Materials 9/10):
//! a scene of its own drawn by a second `olr-render` `Renderer` into an
//! offscreen texture the app hands egui (`App::frame`), so the heat seen is
//! the real heat pass's: the deposits, the step, the look and the glow
//! light as play draws them, never an approximation.
//!
//! The scene (`olr_material_editor::preview`'s layout): two identical
//! patches of open air over dirt with rock in it, under a dark look, the
//! material open on the left (value 1) and the one compared on the right
//! (value 2), split by a wall of rock without a material. A blast carves
//! the theme's hole for its damage in both patches (`ColorEdit::Carve`, the
//! simulation's clamp of the size to a hole) and deposits the engine's
//! heat for it (`olr_look::engine::explosion_heat`, Materials 4/10's curve)
//! where it carved, as `olr-present` does in play. The heat steps on the
//! preview's clock (a frame's time, at most [`MAX_FRAME_MS`], times the
//! time scale); a material edited changes the table at once and keeps the
//! heat (`Renderer::set_surface_materials`).
//!
//! It costs nothing unless the material editor shows it: the app makes it
//! when the editor opens and drops it when it closes, and draws it only on
//! the frames the editor is drawn. Once nothing is hot it draws
//! [`SETTLE_FRAMES`] more frames (the glow light converging) and then
//! none until something changes.

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::{Material, MaterialGrid};
use olr_format::materials::{MaterialSource, compile_material, material_file};
use olr_look::{LookFrame, SurfaceMaps, SurfaceMaterial};
use olr_material_editor::preview::{PATCH, SCENE, patch_x};
use olr_material_editor::{BlastSize, PreviewCommand};
use olr_render::{HeatDeposit, OUTPUT_FORMAT, RenderSettings, Renderer, UpscaleFilter};
use olr_scene::{DrawList, View};
use olr_terrain::{ColorEdit, ColorMap, Stamp, Terrain, ThemeArt};

/// The longest a frame advances the preview's clock, ms (before the time
/// scale): a stall or a hidden stretch is not caught up.
pub const MAX_FRAME_MS: u32 = 100;
/// Frames drawn after the heat went idle or anything changed, for the glow
/// light to settle; then none until the next change.
pub const SETTLE_FRAMES: u32 = 30;
/// The picture's texels per world pixel: the renderer's world target (2
/// texels a world pixel) drawn at twice its size.
pub const PICTURE_SCALE: u32 = 4;
/// The fastest the clock may run (a host's time scale is clamped to it).
pub const MAX_TIME_SCALE: u32 = 16;
/// The scene's ambient light, linear: dark, so the glow shows.
const AMBIENT: f32 = 0.1;

/// The picture's size, pixels.
pub const PICTURE: (u32, u32) = (SCENE.0 * PICTURE_SCALE, SCENE.1 * PICTURE_SCALE);

/// The holes OLX's dirt theme carves with (`hole1` to `hole5`, 5 to 15
/// pixels across) drawn as discs, for a preview without the theme: the
/// carved disc's rim one pixel of dark dirt, as the theme's.
pub fn round_holes() -> Vec<Stamp> {
    [5u32, 7, 9, 11, 15]
        .into_iter()
        .map(|d| {
            let mut img = RgbaImage::new(d, d);
            let c = (d as f32 - 1.0) / 2.0;
            let r = d as f32 / 2.0;
            for y in 0..d {
                for x in 0..d {
                    let dist = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
                    let px = if dist < r - 1.0 {
                        Rgba::MAGENTA
                    } else if dist < r {
                        Rgba::rgb(64, 40, 20)
                    } else {
                        Rgba::rgb(0, 0, 0)
                    };
                    img.set(x, y, px);
                }
            }
            Stamp::from_image(&img)
        })
        .collect()
}

/// A small, fixed noise for the art: -8 to 8.
fn noise(x: u32, y: u32) -> i32 {
    let h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77);
    let h = (h ^ (h >> 15)).wrapping_mul(0x2C1B_3C6D);
    ((h >> 27) as i32 - 16) / 2
}

fn shade(c: [u8; 3], n: i32) -> Rgba {
    let f = |v: u8| (i32::from(v) + n).clamp(0, 255) as u8;
    Rgba::rgb(f(c[0]), f(c[1]), f(c[2]))
}

/// What patch pixel `(x, y)` is: air above the ground's line, dirt under
/// it, rock in a boulder and the bedrock.
fn patch_cell(x: u32, y: u32) -> Material {
    let (fx, fy) = (x as f32, y as f32);
    let ground = 30.0 + 3.0 * (fx * 0.21).sin() + 2.0 * (fx * 0.53 + 1.0).sin();
    let bedrock = 58.0 + 2.5 * (fx * 0.17 + 2.0).sin();
    let boulder = ((fx - 44.0) / 9.0).powi(2) + ((fy - 44.0) / 6.0).powi(2) < 1.0;
    if fy < ground {
        Material::Background
    } else if fy >= bedrock || boulder {
        Material::Rock
    } else {
        Material::Dirt
    }
}

/// The scene's cells, art (art scale 1) and surface maps.
fn scene() -> (MaterialGrid, RgbaImage, RgbaImage, SurfaceMaps) {
    let (w, h) = SCENE;
    let mut cells = Vec::with_capacity((w * h) as usize);
    let mut front = RgbaImage::new(w, h);
    let mut back = RgbaImage::new(w, h);
    let mut maps = SurfaceMaps::empty(SCENE);
    for y in 0..h {
        for x in 0..w {
            let n = noise(x, y);
            let patch = (0..2).find(|&i| (patch_x(i)..patch_x(i) + PATCH.0).contains(&x));
            let cell = patch.map_or(Material::Rock, |i| patch_cell(x - patch_x(i), y));
            cells.push(cell);
            let art = match (patch, cell) {
                (None, _) => [44, 44, 48],
                (_, Material::Rock) => [116, 110, 104],
                (_, Material::Dirt) => [128, 88, 52],
                _ => [0, 0, 0],
            };
            front.set(x, y, shade(art, n));
            back.set(x, y, shade(if patch.is_some() { [58, 48, 40] } else { [30, 30, 32] }, n / 2));
            if let Some(i) = patch {
                let v = i as u8 + 1;
                let at = (y * w + x) as usize;
                maps.back[at] = v;
                if cell != Material::Background {
                    maps.front[at] = v;
                }
            }
        }
    }
    let grid = MaterialGrid::from_cells(w, h, cells).expect("the scene's cells are its size");
    (grid, front, back, maps)
}

/// The preview's material table: the material open (value 1) and the one
/// compared (value 2); none is a material that never heats.
fn table(materials: &[Option<SurfaceMaterial>; 2]) -> Vec<SurfaceMaterial> {
    materials
        .iter()
        .map(|m| m.clone().unwrap_or_else(|| SurfaceMaterial { id: "none".into(), heat: None }))
        .collect()
}

/// `m` as the renderer reads it; `None` when it does not check (the
/// preview keeps the last that did).
fn compiled(m: &MaterialSource) -> Option<SurfaceMaterial> {
    compile_material(m, &material_file(&m.id), "").ok()
}

/// The material editor's live heat preview.
pub struct HeatPreview {
    renderer: Renderer,
    /// The picture ([`PICTURE`], [`OUTPUT_FORMAT`]), and its view egui
    /// samples.
    target: wgpu::Texture,
    view: wgpu::TextureView,
    holes: Vec<Stamp>,
    map: ColorMap,
    maps: SurfaceMaps,
    /// The materials as given, and as the renderer has them.
    given: [Option<MaterialSource>; 2],
    materials: [Option<SurfaceMaterial>; 2],
    look: LookFrame,
    list: DrawList,
    /// Match ms a ms.
    scale: u32,
    /// Frames still to draw though nothing is hot.
    settle: u32,
    /// Frames drawn (tests).
    drawn: u64,
}

impl HeatPreview {
    /// The preview on `device` and `queue` (the game's), carving with
    /// `holes` (the theme's `hole1` to `hole5`; [`round_holes`] when none
    /// is given), no material on either patch yet, time scale 1.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, holes: Option<Vec<Stamp>>) -> HeatPreview {
        let mut renderer = Renderer::new(
            device.clone(),
            queue.clone(),
            OUTPUT_FORMAT,
            RenderSettings { filter: UpscaleFilter::IntegerNearest },
        );
        renderer.set_lighting(true);
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("olr-game heat preview"),
            size: wgpu::Extent3d { width: PICTURE.0, height: PICTURE.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let holes = holes.filter(|h| !h.is_empty()).unwrap_or_else(round_holes);
        let (grid, front, back, maps) = scene();
        let map = new_map(&grid, &front, &back, &holes);
        let mut look = LookFrame::default();
        look.begin(None, 0.0);
        look.ambient = [AMBIENT; 3];
        let mut list = DrawList::new();
        list.view = View { x: 0.0, y: 0.0, w: SCENE.0 as f32, h: SCENE.1 as f32 };
        list.map = SCENE;
        let mut p = HeatPreview {
            renderer,
            target,
            view,
            holes,
            map,
            maps,
            given: [None, None],
            materials: [None, None],
            look,
            list,
            scale: 1,
            settle: SETTLE_FRAMES,
            drawn: 0,
        };
        p.renderer.set_surface(&table(&p.materials), Some(&p.maps));
        p.renderer.upload_terrain(&p.map);
        p.map.take_dirty().for_each(drop);
        p
    }

    /// The picture's view, for egui (`register_native_texture`).
    pub fn view(&self) -> &wgpu::TextureView {
        &self.view
    }

    /// The picture (tests read it back).
    pub fn target(&self) -> &wgpu::Texture {
        &self.target
    }

    /// The preview's renderer (tests: its heat texture).
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// Frames drawn so far (tests: none while idle and settled).
    pub fn drawn(&self) -> u64 {
        self.drawn
    }

    /// Match ms a ms.
    pub fn time_scale(&self) -> u32 {
        self.scale
    }

    /// The editor's command.
    pub fn command(&mut self, c: PreviewCommand) {
        match c {
            PreviewCommand::Blast { at, size } => self.blast(at, size),
            PreviewCommand::SetCompare(m) => self.set_material(1, m.as_ref()),
            PreviewCommand::Restart => self.restart(),
            PreviewCommand::TimeScale(s) => self.scale = s.clamp(1, MAX_TIME_SCALE),
        }
    }

    /// The material open, as edited (none open: none); each frame, so an
    /// edit shows in the next one.
    pub fn set_open(&mut self, m: Option<&MaterialSource>) {
        self.set_material(0, m);
    }

    /// Patch `i`'s material: the table changes and the heat stays.
    fn set_material(&mut self, i: usize, m: Option<&MaterialSource>) {
        if self.given[i].as_ref() == m {
            return;
        }
        self.given[i] = m.cloned();
        let compiled = match m {
            Some(m) => match compiled(m) {
                Some(c) => Some(c),
                None => return,
            },
            None => None,
        };
        if self.materials[i] == compiled {
            return;
        }
        self.materials[i] = compiled;
        let table = table(&self.materials);
        if !self.renderer.set_surface_materials(&table) {
            self.renderer.set_surface(&table, Some(&self.maps));
        }
        self.settle = SETTLE_FRAMES;
    }

    /// An engine explosion of `size` at patch pixel `at` in both patches:
    /// the crater carved, then the engine's heat deposited there (the
    /// renderer's next frame lands it after the carve, as play's does).
    fn blast(&mut self, at: [u32; 2], size: BlastSize) {
        let [x, y] = at;
        if x >= PATCH.0 || y >= PATCH.1 {
            return;
        }
        let damage = size.damage();
        // OLX's `CarveHole(size)`: the theme's hole `clamp(size, 0, 4)`.
        let hole = damage.clamp(0, 4) as u8;
        let heat = olr_look::engine::explosion_heat(damage as f32);
        for i in 0..2 {
            let px = patch_x(i) + x;
            self.map.apply(ColorEdit::Carve { hole, x: px as i32, y: y as i32 });
            if let Some(h) = heat {
                // At the pixel's centre: the explosion's position, which
                // the carve truncates.
                let pos = [px as f32 + 0.5, y as f32 + 0.5];
                self.renderer.deposit_heat(HeatDeposit { pos, radius: h.radius, amount: h.amount });
            }
        }
        self.settle = SETTLE_FRAMES;
    }

    /// The scene afresh: the craters filled, the heat gone.
    fn restart(&mut self) {
        let (grid, front, back, _) = scene();
        self.map = new_map(&grid, &front, &back, &self.holes);
        self.renderer.upload_terrain(&self.map);
        self.map.take_dirty().for_each(drop);
        self.renderer.reset_light_history();
        self.settle = SETTLE_FRAMES;
    }

    /// One frame `elapsed_ms` after the last drawn (capped at
    /// [`MAX_FRAME_MS`], times the time scale): the heat advanced, the
    /// craters uploaded and the picture drawn; nothing while idle and
    /// settled. Whether it drew.
    pub fn frame(&mut self, elapsed_ms: u32) -> bool {
        let ms = elapsed_ms.min(MAX_FRAME_MS) * self.scale;
        self.renderer.advance_heat(ms);
        self.renderer.update_terrain(&mut self.map);
        if self.renderer.heat_active() {
            self.settle = SETTLE_FRAMES;
        } else if self.settle == 0 {
            return false;
        } else {
            self.settle -= 1;
        }
        let area = [0, 0, PICTURE.0, PICTURE.1];
        self.renderer.render_lit(&self.list, Some(&self.look), &self.view, area);
        self.drawn += 1;
        true
    }

    /// The renderer's warnings since the last call.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(self.renderer.warnings())
    }
}

/// The scene's colour map, carving with `holes`.
fn new_map(grid: &MaterialGrid, front: &RgbaImage, back: &RgbaImage, holes: &[Stamp]) -> ColorMap {
    let theme = ThemeArt { holes: holes.to_vec(), green_mask: None, front_tile: RgbaImage::new(1, 1) };
    ColorMap::new(&Terrain::from_grid(grid), front, back, 1, theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_material_editor::preview::DIVIDER;

    #[test]
    fn the_scene_is_two_patches_split_by_a_wall() {
        let (grid, front, back, maps) = scene();
        let t = Terrain::from_grid(&grid);
        assert_eq!((t.width(), t.height()), SCENE);
        assert_eq!((front.width(), back.width()), (SCENE.0, SCENE.0));
        assert!(maps.fits(SCENE));
        let at = |x: u32, y: u32| (y * SCENE.0 + x) as usize;
        for y in 0..SCENE.1 {
            for x in 0..PATCH.0 {
                // The patches are identical, each its own value.
                let (a, b) = (x, patch_x(1) + x);
                assert_eq!(t.material(a.into(), y.into()), t.material(b.into(), y.into()));
                assert_eq!(maps.back[at(a, y)], 1);
                assert_eq!(maps.back[at(b, y)], 2);
                let solid = t.material(a.into(), y.into()) != Some(Material::Background);
                assert_eq!(maps.front[at(a, y)], u8::from(solid));
                assert_eq!(maps.front[at(b, y)], 2 * u8::from(solid));
            }
            for x in PATCH.0..PATCH.0 + DIVIDER {
                assert_eq!(t.material(x.into(), y.into()), Some(Material::Rock), "the wall");
                assert_eq!((maps.front[at(x, y)], maps.back[at(x, y)]), (0, 0), "no material");
            }
        }
        // Air above, dirt and rock below.
        let kinds = |m| {
            (0..PATCH.0)
                .flat_map(|x| (0..PATCH.1).map(move |y| (x, y)))
                .filter(|&(x, y)| patch_cell(x, y) == m)
                .count()
        };
        assert!(kinds(Material::Background) > 1000);
        assert!(kinds(Material::Dirt) > 1000);
        assert!(kinds(Material::Rock) > 300);
    }

    #[test]
    fn round_holes_carve_discs_of_the_themes_sizes() {
        let holes = round_holes();
        assert_eq!(holes.iter().map(Stamp::width).collect::<Vec<_>>(), [5, 7, 9, 11, 15]);
        let big = &holes[4];
        assert_eq!(big.get(7, 7), olr_terrain::StampPixel::Carve);
        assert_eq!(big.get(0, 0), olr_terrain::StampPixel::Skip);
    }
}
