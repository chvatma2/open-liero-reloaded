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
