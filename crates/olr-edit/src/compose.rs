//! The bake (plan 14a, spec §3): a level's composition made into the
//! files the runtime reads, `material.png`, `front.png`, `back.png` and
//! `thumb.png`, whole or a rectangle at a time. Rulings R3-R6 of the plan:
//! - a world pixel's material is the topmost visible element covering the
//!   centre art pixel of its block (background when none covers it);
//! - an art pixel's front is the topmost visible element covering it, else
//!   its back;
//! - an art pixel's back is the topmost visible background image or drawn
//!   element covering it, or a raster element's back, else the theme's
//!   back tile repeated from the art origin;
//! - then `front := back` over every background block;
//! - baked art is opaque: an image or drawn element's colour keeps only
//!   its RGB.
//!
//! Integer arithmetic only: the same composition bakes to the same bytes
//! on every platform.

use std::path::Path;

use olr_data::image::{Rgba, RgbaImage};
use olr_data::level::Material;
use olr_data::manifest::PackKind;
use olr_format::image_io::{self, IMAGES, LEVEL_ART};
use olr_format::levels::{
    Composition, CompositionSource, Element, ElementKind, LevelSource, check_composition, element_files,
    encode_material, raster_files, write_level_files,
};
use olr_format::themes::read_theme_files;
use olr_vfs::{ContentSource, DirFs, MemFs, VPath};

use crate::level::refresh_thumb;
use crate::pack::{Pack, Source};
use crate::pixelate::{Pixelation, process_grid};

pub use crate::level::Rect;

#[derive(Debug, thiserror::Error)]
pub enum ComposeError {
    /// An original, a layer or a raster file that does not decode, or of the wrong size.
    #[error("{file}: {message}")]
    File { file: String, message: String },
    #[error(transparent)]
    Format(#[from] olr_format::FormatError),
}

/// One element, decoded and processed.
enum Layer {
    /// An image's element grid, placed at `origin` (art pixels) with
    /// `size` art pixels; art pixel `(x, y)` of the box is grid pixel
    /// `(x / pixel_size, y / pixel_size)`, covered where its alpha is 255.
    Image { material: Material, grid: RgbaImage, pixel_size: u32, origin: (i64, i64), size: (i64, i64) },
    /// A level-sized art layer, covered where its alpha is not 0.
    Drawn { material: Material, art: RgbaImage },
    /// A whole level: it covers everything.
    Raster { material: Vec<Material>, front: RgbaImage, back: RgbaImage },
}

/// Opaque: the colour's RGB.
fn opaque(c: Rgba) -> Rgba {
    Rgba { a: 255, ..c }
}

impl Layer {
    /// The element's colour at art pixel `(ax, ay)` when it covers it.
    fn front(&self, ax: u32, ay: u32) -> Option<Rgba> {
        match self {
            Layer::Image { grid, pixel_size, origin, size, .. } => {
                let (lx, ly) = (i64::from(ax) - origin.0, i64::from(ay) - origin.1);
                if !(0..size.0).contains(&lx) || !(0..size.1).contains(&ly) {
                    return None;
                }
                let ps = i64::from(*pixel_size);
                let p = grid.get((lx / ps) as u32, (ly / ps) as u32);
                (p.a == 255).then_some(p)
            }
            Layer::Drawn { art, .. } => {
                let p = art.get(ax, ay);
                (p.a != 0).then(|| opaque(p))
            }
            Layer::Raster { front, .. } => Some(front.get(ax, ay)),
        }
    }

    /// The element's backdrop at art pixel `(ax, ay)` (R4): a background
    /// image or drawn element's art, a raster element's back.
    fn back(&self, ax: u32, ay: u32) -> Option<Rgba> {
        match self {
            Layer::Image { material: Material::Background, .. }
            | Layer::Drawn { material: Material::Background, .. } => self.front(ax, ay),
            Layer::Image { .. } | Layer::Drawn { .. } => None,
            Layer::Raster { back, .. } => Some(back.get(ax, ay)),
        }
    }

    /// The material of world pixel `cell` (row-major) whose centre art
    /// pixel is `(ax, ay)`, when the element covers that art pixel.
    fn material(&self, cell: usize, ax: u32, ay: u32) -> Option<Material> {
        match self {
            Layer::Image { material, .. } | Layer::Drawn { material, .. } => {
                self.front(ax, ay).map(|_| *material)
            }
            Layer::Raster { material, .. } => material.get(cell).copied(),
        }
    }
}

/// Decoded layers and processed images of one composition, ready to bake
/// any region. `bake_region` never fails: everything was checked here.
pub struct Baker {
    size: (u32, u32),
    art_scale: u32,
    back_tile: RgbaImage,
    /// The visible elements, bottom to top.
    layers: Vec<Layer>,
}

impl Baker {
    /// Checks `src` against a level of `size` at `art_scale`
    /// (`check_composition`), then decodes its layers and originals and
    /// processes its images: originals within `IMAGES`, layers within
    /// `LEVEL_ART`.
    pub fn new(
        src: &CompositionSource,
        size: (u32, u32),
        art_scale: u32,
        back_tile: &RgbaImage,
    ) -> Result<Baker, ComposeError> {
        let s = art_scale;
        let art = match (size.0.checked_mul(s), size.1.checked_mul(s)) {
            (Some(w), Some(h)) if image_io::check_size(w, h, LEVEL_ART).is_ok() => (w, h),
            _ => {
                return Err(ComposeError::File {
                    file: "level.ron".into(),
                    message: format!("{}x{} at art scale {s}: art beyond the level bound", size.0, size.1),
                });
            }
        };
        check_composition(src, size, art_scale)?;
        let mut layers = Vec::new();
        for e in &src.composition.elements {
            match (&e.kind, e.visible) {
                (_, true) => layers.push(decode(src, e, size, art, s)?),
                // A hidden image's original is decoded for its errors, not
                // processed: it is left out of the bake.
                (ElementKind::Image { .. }, false) => {
                    let file = &element_files(e)[0];
                    let bytes = src.files.get(file).map(Vec::as_slice).unwrap_or_default();
                    image_io::decode_image_within(bytes, IMAGES)
                        .map_err(|message| ComposeError::File { file: file.clone(), message })?;
                }
                (_, false) => {
                    decode(src, e, size, art, s)?;
                }
            }
        }
        Ok(Baker { size, art_scale, back_tile: back_tile.clone(), layers })
    }

    /// The back of art pixel `(ax, ay)` (R4).
    fn back(&self, ax: u32, ay: u32) -> Rgba {
        if let Some(c) = self.layers.iter().rev().find_map(|l| l.back(ax, ay)) {
            return c;
        }
        let t = &self.back_tile;
        if t.width() == 0 || t.height() == 0 {
            return Rgba::BLACK;
        }
        opaque(t.get(ax % t.width(), ay % t.height()))
    }

    /// Rebakes `rect` (clipped to the level) of `level`'s material, front
    /// and back. A level of another size or art scale than the baker's is
    /// left as it is.
    pub fn bake_region(&self, level: &mut LevelSource, rect: Rect) {
        let (w, h) = self.size;
        let s = self.art_scale;
        let fits = level.size == self.size
            && level.art_scale == s
            && level.material.len() == w as usize * h as usize
            && (level.front.width(), level.front.height()) == (w * s, h * s)
            && (level.back.width(), level.back.height()) == (w * s, h * s);
        if !fits {
            return;
        }
        let x1 = rect.x.saturating_add(rect.w).min(w);
        let y1 = rect.y.saturating_add(rect.h).min(h);
        for y in rect.y.min(h)..y1 {
            for x in rect.x.min(w)..x1 {
                let cell = y as usize * w as usize + x as usize;
                let (cx, cy) = (x * s + s / 2, y * s + s / 2);
                let material = self
                    .layers
                    .iter()
                    .rev()
                    .find_map(|l| l.material(cell, cx, cy))
                    .unwrap_or(Material::Background);
                level.material[cell] = material;
                for ay in y * s..(y + 1) * s {
                    for ax in x * s..(x + 1) * s {
                        let back = self.back(ax, ay);
                        let front = if material == Material::Background {
                            back
                        } else {
                            self.layers.iter().rev().find_map(|l| l.front(ax, ay)).unwrap_or(back)
                        };
                        level.back.set(ax, ay, back);
                        level.front.set(ax, ay, front);
                    }
                }
            }
        }
    }
}

/// One element's files decoded (and an image processed), each checked
/// against the level's size.
fn decode(
    src: &CompositionSource,
    e: &Element,
    size: (u32, u32),
    art: (u32, u32),
    s: u32,
) -> Result<Layer, ComposeError> {
    let bytes = |file: &str| {
        src.files.get(file).ok_or_else(|| ComposeError::File { file: file.into(), message: "missing".into() })
    };
    let fail = |file: &str, message: String| ComposeError::File { file: file.into(), message };
    let rgba = |file: &str, expected: (u32, u32)| -> Result<RgbaImage, ComposeError> {
        let img = image_io::decode_rgba_within(bytes(file)?, LEVEL_ART).map_err(|m| fail(file, m))?;
        if (img.width(), img.height()) != expected {
            return Err(fail(
                file,
                format!("{}x{}, the level needs {}x{}", img.width(), img.height(), expected.0, expected.1),
            ));
        }
        Ok(img)
    };
    Ok(match &e.kind {
        ElementKind::Image {
            material,
            size: (bw, bh),
            at,
            pixel_size,
            colours,
            dither,
            alpha_threshold,
            ..
        } => {
            let file = &element_files(e)[0];
            let original = image_io::decode_image_within(bytes(file)?, IMAGES).map_err(|m| fail(file, m))?;
            let box_art = (bw * s, bh * s);
            let p = Pixelation {
                pixel_size: *pixel_size,
                colours: *colours,
                dither: *dither,
                alpha_threshold: *alpha_threshold,
            };
            Layer::Image {
                material: (*material).into(),
                grid: process_grid(&original, box_art, &p),
                pixel_size: (*pixel_size).max(1),
                origin: (i64::from(at.0) * i64::from(s), i64::from(at.1) * i64::from(s)),
                size: (i64::from(box_art.0), i64::from(box_art.1)),
            }
        }
        ElementKind::Drawn { material } => {
            Layer::Drawn { material: (*material).into(), art: rgba(&element_files(e)[0], art)? }
        }
        ElementKind::Raster => {
            let [m, f, b] = raster_files(&e.id);
            let (mw, mh, indices) =
                image_io::decode_indexed_within(bytes(&m)?, LEVEL_ART).map_err(|why| fail(&m, why))?;
            if (mw, mh) != size {
                return Err(fail(&m, format!("{mw}x{mh}, the level is {}x{}", size.0, size.1)));
            }
            let material = indices
                .iter()
                .map(|&i| {
                    Material::from_index(i)
                        .ok_or_else(|| fail(&m, format!("palette index {i}: not a material")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Layer::Raster { material, front: rgba(&f, art)?, back: rgba(&b, art)? }
        }
    })
}

/// The whole bake: material, front, back and the thumbnail (`refresh_thumb`).
pub fn bake(
    level: &mut LevelSource,
    src: &CompositionSource,
    back_tile: &RgbaImage,
) -> Result<(), ComposeError> {
    let baker = Baker::new(src, level.size, level.art_scale, back_tile)?;
    baker.bake_region(level, Rect { x: 0, y: 0, w: level.size.0, h: level.size.1 });
    refresh_thumb(level);
    Ok(())
}

/// `level` as a composition of one raster element `base` (ruling R6): its
/// material, front and back encoded as `write_level_files` encodes them.
pub fn compose_from(level: &LevelSource) -> CompositionSource {
    let element = Element {
        id: "base".into(),
        name: "Base".into(),
        visible: true,
        locked: false,
        kind: ElementKind::Raster,
    };
    let [m, f, b] = raster_files(&element.id);
    let files = [
        (m, encode_material(level.size, &level.material)),
        (f, image_io::encode_rgba(&level.front)),
        (b, image_io::encode_rgba(&level.back)),
    ]
    .into_iter()
    .collect();
    CompositionSource { composition: Composition { elements: vec![element] }, files }
}

/// The `back_tile` of the theme pack whose manifest id is `theme` among
/// `content`'s immediate subdirectories (ruling R9), taken in name order;
/// hidden directories and directories that are not packs are skipped.
pub fn theme_back_tile(content: &Path, theme: &str) -> Result<RgbaImage, ComposeError> {
    let fail = |message: String| ComposeError::File { file: content.display().to_string(), message };
    let mut dirs: Vec<_> = std::fs::read_dir(content)
        .map_err(|e| fail(e.to_string()))?
        .filter_map(Result::ok)
        .filter(|e| {
            // A linked pack counts: `metadata` follows the link.
            std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir())
                && !e.file_name().to_string_lossy().starts_with('.')
        })
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let src = DirFs::new(&dir);
        let Ok(manifest) = olr_pack::read_manifest(&src) else { continue };
        if manifest.kind == PackKind::Theme && manifest.id == theme {
            return Ok(read_theme_files(&src)?.back_tile);
        }
    }
    Err(ComposeError::File {
        file: theme.into(),
        message: format!("no theme pack with this id in {}", content.display()),
    })
}

/// The files a bake writes that it can leave stale.
const BAKED: [&str; 4] = ["material.png", "front.png", "back.png", "thumb.png"];

/// The baked files of `pack` (a composed level) that differ from what its
/// composition bakes to: `material.png`, `front.png`, `back.png`,
/// `thumb.png`, in that order. Empty for any other pack.
pub fn stale(pack: &Pack, back_tile: &RgbaImage) -> Result<Vec<String>, ComposeError> {
    let (Source::Level(level), Some(src)) = (&pack.source, &pack.composition) else { return Ok(Vec::new()) };
    let mut baked = level.clone();
    bake(&mut baked, src, back_tile)?;
    let files = |l: &LevelSource| -> Result<MemFs, ComposeError> {
        let mut fs = MemFs::new();
        write_level_files(&mut fs, l)?;
        Ok(fs)
    };
    let (now, then) = (files(level)?, files(&baked)?);
    Ok(BAKED
        .iter()
        .filter(|f| {
            let path = VPath::new(f).expect("a valid path");
            now.read(&path).ok() != then.read(&path).ok()
        })
        .map(|f| f.to_string())
        .collect())
}
