//! Level thumbnails for the menus (spec §21 G6), made by
//! `olr_format::levels::thumbnail` (moved there in plan 13d, so the level
//! operations make them too).

use olr_data::image::RgbaImage;
use olr_data::level::Level;

pub use olr_format::levels::letterbox;

/// The thumbnail of `level`: [`olr_format::levels::thumbnail`] of its front.
pub fn level_thumbnail(level: &Level) -> RgbaImage {
    olr_format::levels::thumbnail(&level.front)
}
