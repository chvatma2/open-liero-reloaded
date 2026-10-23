//! Levels: a material grid at physics resolution plus color art layers.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::image::RgbaImage;

/// Terrain material of one world pixel. The numeric values are OLX's material
/// indices (`src/gusanos/level.cpp:80-145`), which the material PNG stores directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Material {
    Rock = 0,
    Background = 1,
    Dirt = 2,
}

impl Material {
    pub fn from_index(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Rock),
            1 => Some(Self::Background),
            2 => Some(Self::Dirt),
            _ => None,
        }
    }

    /// OLX `Material::indexFromLxFlag` (`src/gusanos/material.h:44-49`):
    /// rock wins over dirt, and anything else is background.
    pub fn from_lx_flags(flags: u8) -> Self {
        const PX_DIRT: u8 = 0x02;
        const PX_ROCK: u8 = 0x04;
        if flags & PX_ROCK != 0 {
            Self::Rock
        } else if flags & PX_DIRT != 0 {
            Self::Dirt
        } else {
            Self::Background
        }
    }
}

/// One material per world pixel, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialGrid {
    width: u32,
    height: u32,
    cells: Vec<Material>,
}

impl MaterialGrid {
    /// A grid filled with `fill`.
    pub fn new(width: u32, height: u32, fill: Material) -> Self {
        Self { width, height, cells: vec![fill; width as usize * height as usize] }
    }

    /// Returns `None` if `cells.len() != width * height`.
    pub fn from_cells(width: u32, height: u32, cells: Vec<Material>) -> Option<Self> {
        (cells.len() == width as usize * height as usize).then_some(Self { width, height, cells })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn cells(&self) -> &[Material] {
        &self.cells
    }

    /// Panics if `(x, y)` is out of bounds.
    pub fn get(&self, x: u32, y: u32) -> Material {
        assert!(x < self.width && y < self.height, "cell ({x},{y}) out of bounds");
        self.cells[y as usize * self.width as usize + x as usize]
    }

    /// Number of cells with the given material.
    pub fn count(&self, m: Material) -> usize {
        self.cells.iter().filter(|c| **c == m).count()
    }
}

/// A spawn point in world pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnPoint {
    pub x: i32,
    pub y: i32,
}

/// The `level.ron` part of a level pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelDef {
    pub name: String,
    /// Size of the material grid in world pixels.
    pub width: u32,
    pub height: u32,
    /// Art pixels per world pixel for `front.png` and `back.png`.
    pub art_scale: u32,
    /// Pack id of the theme used for carving holes.
    pub theme: String,
    /// Empty means "random spawn", as in OLX.
    pub spawns: Vec<SpawnPoint>,
}

/// The nearest a distant layer may be: 1 % of the way to the far scene.
pub const MIN_DEPTH: u8 = 1;
/// The farthest a distant layer may be: 99 % (100 would never move).
pub const MAX_DEPTH: u8 = 99;

/// A distant (parallax) layer of a level (issue #11): decoration seen
/// through the level's see-through back pixels, scrolling by `1 - depth`
/// of the camera's movement. No material, never in the simulation.
#[derive(Clone, Debug, PartialEq)]
pub struct DistantLayer {
    /// Art pixels (the level's art scale), alpha 0 where it is uncovered,
    /// 255 elsewhere; each side a multiple of the art scale.
    pub image: RgbaImage,
    /// Its top-left in world pixels with the camera (the view's centre) at
    /// the level's centre.
    pub at: (i32, i32),
    /// Percent, [`MIN_DEPTH`] (near: moves almost with the level, lit
    /// almost like it) to [`MAX_DEPTH`] (far: almost still, ambient light
    /// only).
    pub depth: u8,
    /// Its glow (issue #62), when it glows: its light is baked into the
    /// level's glow maps at this placement (`at`), its picture lit by it
    /// where it is drawn.
    pub glow: Option<DistantGlow>,
}

/// A distant layer's glow (issue #62): the drawn layer shows its art at
/// `strength` × its mask × its group's self-light scale; its light is the
/// level's (baked at its reference placement), never its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DistantGlow {
    /// Its glow group as the group maps hold it: 1 + its index in the
    /// level look's glow groups (never 0).
    pub group: u8,
    /// Percent, 1 to 100.
    pub strength: u8,
    /// A painted mask, one value per art pixel of the layer's image
    /// (row-major, 0 no glow to 255 full); `None` glows uniform.
    pub mask: Option<Vec<u8>>,
}

impl DistantLayer {
    /// Its top-left in world pixels with the camera's centre at `camera` in
    /// a level of `size` world pixels: `at + depth × (camera − size / 2)`,
    /// exact (the renderer rounds it to its texels).
    pub fn position(&self, camera: (f32, f32), size: (u32, u32)) -> (f32, f32) {
        let d = f32::from(self.depth.clamp(MIN_DEPTH, MAX_DEPTH)) / 100.0;
        let off = |c: f32, side: u32| if c.is_finite() { d * (c - side as f32 / 2.0) } else { 0.0 };
        (self.at.0 as f32 + off(camera.0, size.0), self.at.1 as f32 + off(camera.1, size.1))
    }
}

/// A loaded level.
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    pub def: LevelDef,
    pub material: MaterialGrid,
    /// Foreground art; where the material is background it equals `back`.
    pub front: RgbaImage,
    pub back: RgbaImage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lx_flags_follow_olx_priority() {
        assert_eq!(Material::from_lx_flags(0x01), Material::Background);
        assert_eq!(Material::from_lx_flags(0x02), Material::Dirt);
        assert_eq!(Material::from_lx_flags(0x04), Material::Rock);
        assert_eq!(Material::from_lx_flags(0x06), Material::Rock);
        assert_eq!(Material::from_lx_flags(0x00), Material::Background);
        assert_eq!(Material::from_lx_flags(0x18), Material::Background);
    }

    #[test]
    fn index_roundtrip() {
        for m in [Material::Rock, Material::Background, Material::Dirt] {
            assert_eq!(Material::from_index(m as u8), Some(m));
        }
        assert_eq!(Material::from_index(3), None);
    }

    #[test]
    fn grid_rejects_wrong_size_and_counts() {
        assert!(MaterialGrid::from_cells(2, 2, vec![Material::Dirt; 3]).is_none());
        let g = MaterialGrid::from_cells(2, 1, vec![Material::Dirt, Material::Rock]).unwrap();
        assert_eq!(g.get(1, 0), Material::Rock);
        assert_eq!(g.count(Material::Dirt), 1);
    }
}
