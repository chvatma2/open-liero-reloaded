//! Level themes: stamps used when carving terrain, plus pixmap-level decorations.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::image::RgbaImage;

/// The `theme.ron` part of a theme pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThemeDef {
    pub name: String,
    pub holes: u32,
    pub stones: u32,
    pub misc: u32,
}

/// A loaded theme. Images keep OLX's raw colors: in hole stamps, magenta means
/// "make empty", black means "leave alone" and other colors paint the dirt edge
/// (`src/common/CMap.cpp:1007-1106`). Stones and misc use magenta as transparency.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub def: ThemeDef,
    pub holes: Vec<RgbaImage>,
    pub stones: Vec<RgbaImage>,
    pub misc: Vec<RgbaImage>,
    pub front_tile: RgbaImage,
    pub back_tile: RgbaImage,
}
