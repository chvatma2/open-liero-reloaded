//! Pack authoring operations shared by `olr-cli` and the editors (plan 13d):
//! a pack's authoring source loaded and saved whole ([`pack`]), the
//! canonical formatter ([`fmt`]), pack templates ([`new`]), skin operations
//! ([`skin`], issue #13) and the image
//! pipeline of level composition ([`pixelate`], plan 14a).

pub mod asset;
pub mod compose;
pub mod drawn;
pub mod element;
pub mod fmt;
pub mod glow;
pub mod level;
pub mod look;
pub mod mask;
pub mod new;
pub mod pack;
pub mod pixelate;
pub mod skin;
pub mod textures;
