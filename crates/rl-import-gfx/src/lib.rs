//! OpenLieroX skin, theme and core pack importers, and level thumbnails.

mod core_pack;
mod skin;
mod theme;
mod thumb;

pub use core_pack::{CORE_PACK_ID, ImportedCore, import_core};
pub use skin::{ImportedSkin, import_skin};
pub use theme::{ImportedTheme, import_theme};
pub use thumb::{letterbox, level_thumbnail};

#[derive(Debug, thiserror::Error)]
pub enum GfxError {
    #[error("{file}: {message}")]
    Image { file: String, message: String },
    #[error("{0} not found")]
    MissingFile(String),
    #[error("{file}: {message}")]
    Invalid { file: String, message: String },
    #[error(transparent)]
    Vfs(#[from] rl_vfs::VfsError),
}

pub(crate) fn decode(
    src: &dyn rl_vfs::ContentSource,
    path: &rl_vfs::VPath,
) -> Result<rl_data::image::RgbaImage, GfxError> {
    let bytes = src.read(path)?;
    rl_pack::image_io::decode_rgba(&bytes)
        .map_err(|message| GfxError::Image { file: path.to_string(), message })
}

pub(crate) fn manifest(
    id: String,
    name: String,
    kind: rl_data::manifest::PackKind,
    origin: rl_data::manifest::Origin,
) -> rl_data::manifest::PackManifest {
    rl_data::manifest::PackManifest {
        format_version: rl_data::manifest::PACK_FORMAT_VERSION,
        id,
        name,
        version: "1".to_string(),
        kind,
        authors: Vec::new(),
        origin: Some(origin),
        license: None,
    }
}
