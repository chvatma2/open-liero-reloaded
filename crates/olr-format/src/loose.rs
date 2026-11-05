//! Loose materials (spec `2026-11-05-loose-dirt-design.md`): dirt that
//! falls and slides like sand, how fast and how steep it piles. A level's
//! `level.ron` holds the list (`loose`) and `loose.png` marks its pixels;
//! an asset's `asset.ron` holds the list its elements name. The
//! simulation's step is `olr_sim`'s; these are its rates in the format's
//! units, every one an integer.

use olr_data::level::LooseMaterial;
use serde::{Deserialize, Serialize};

use crate::FormatError;
use crate::materials::{MAX_MATERIAL_ID, MAX_MATERIAL_NAME, valid_material_id};

/// The most loose materials of a level or an asset (`olr_sim`'s
/// `MAX_LOOSE_MATERIALS`).
pub const MAX_LOOSE: usize = 16;
/// The angle of repose's range, degrees.
pub const REPOSE: (u8, u8) = (5, 85);
/// Gravity's range, pixels a second squared.
pub const GRAVITY: (u16, u16) = (1, 2000);
/// The top speed's range, pixels a second.
pub const TOP_SPEED: (u16, u16) = (1, 500);
/// The slide speed's most, pixels a second.
pub const MAX_SLIDE_SPEED: u8 = 100;

// The defaults are literals for the format docs; a test holds them to
// `LooseMaterial::DEFAULT`.
fn repose() -> u8 {
    45
}
fn is_repose(v: &u8) -> bool {
    *v == repose()
}
fn gravity() -> u16 {
    300
}
fn is_gravity(v: &u16) -> bool {
    *v == gravity()
}
fn top_speed() -> u16 {
    80
}
fn is_top_speed(v: &u16) -> bool {
    *v == top_speed()
}
fn slide_speed() -> u8 {
    25
}
fn is_slide_speed(v: &u8) -> bool {
    *v == slide_speed()
}

/// A loose material: one of `level.ron`'s or `asset.ron`'s `loose`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LooseSource {
    /// 1 to 64 bytes of `[a-z0-9_-]`, starting with a letter or digit (a
    /// surface material's rule); unique in its list. An element's `loose`
    /// names it.
    pub id: String,
    /// Shown in the editors, 1 to 64 characters.
    pub name: String,
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// The angle of repose, degrees, 5 (it spreads nearly flat) to 85 (it
    /// stands in near columns): a pile's sides settle at it. Default 45.
    #[serde(default = "repose", skip_serializing_if = "is_repose")]
    pub repose: u8,
    /// How fast a falling grain gathers speed, pixels a second squared, 1
    /// to 2000. Default 300.
    #[serde(default = "gravity", skip_serializing_if = "is_gravity")]
    pub gravity: u16,
    /// A falling grain's top speed, pixels a second, 1 to 500. Default 80.
    #[serde(default = "top_speed", skip_serializing_if = "is_top_speed")]
    pub top_speed: u16,
    /// How fast a grain slides down a slope steeper than its repose,
    /// pixels a second, 0 (it never slides) to 100. Default 25.
    #[serde(default = "slide_speed", skip_serializing_if = "is_slide_speed")]
    pub slide_speed: u8,
}

impl LooseSource {
    /// A material of the default rates.
    pub fn new(id: &str, name: &str) -> LooseSource {
        let d = LooseMaterial::DEFAULT;
        LooseSource {
            id: id.into(),
            name: name.into(),
            notes: String::new(),
            repose: d.repose,
            gravity: d.gravity,
            top_speed: d.top_speed,
            slide_speed: d.slide_speed,
        }
    }

    /// Its rates (unchecked: [`check_loose`] checks them).
    pub fn material(&self) -> LooseMaterial {
        LooseMaterial {
            repose: self.repose,
            gravity: self.gravity,
            top_speed: self.top_speed,
            slide_speed: self.slide_speed,
        }
    }
}

fn within(v: u32, (lo, hi): (u32, u32), file: &str, path: &str) -> Result<(), FormatError> {
    if (lo..=hi).contains(&v) {
        return Ok(());
    }
    Err(FormatError::invalid(file, path, format!("{v}, not within {lo} to {hi}")))
}

/// Checks `list` (`file`'s `path`, `loose` in both files): at most
/// [`MAX_LOOSE`], each id valid and unique, each name 1 to 64 characters
/// and each rate within its range.
pub fn check_loose(list: &[LooseSource], file: &str, path: &str) -> Result<(), FormatError> {
    if list.len() > MAX_LOOSE {
        return Err(FormatError::invalid(
            file,
            path,
            format!("{} loose materials, above {MAX_LOOSE}", list.len()),
        ));
    }
    for (i, m) in list.iter().enumerate() {
        let at = |field: &str| format!("{path}[{i}].{field}");
        if !valid_material_id(&m.id) {
            return Err(FormatError::invalid(
                file,
                &at("id"),
                format!(
                    "{:?} is not 1 to {MAX_MATERIAL_ID} bytes of [a-z0-9_-] starting with a letter or digit",
                    m.id
                ),
            ));
        }
        if list[..i].iter().any(|o| o.id == m.id) {
            return Err(FormatError::invalid(file, &at("id"), format!("{:?} is used twice", m.id)));
        }
        let chars = m.name.chars().count();
        if !(1..=MAX_MATERIAL_NAME).contains(&chars) {
            return Err(FormatError::invalid(
                file,
                &at("name"),
                format!("{chars} characters, not 1 to {MAX_MATERIAL_NAME}"),
            ));
        }
        let wide = |(lo, hi): (u16, u16)| (u32::from(lo), u32::from(hi));
        within(m.repose.into(), (REPOSE.0.into(), REPOSE.1.into()), file, &at("repose"))?;
        within(m.gravity.into(), wide(GRAVITY), file, &at("gravity"))?;
        within(m.top_speed.into(), wide(TOP_SPEED), file, &at("top_speed"))?;
        within(m.slide_speed.into(), (0, MAX_SLIDE_SPEED.into()), file, &at("slide_speed"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_simulations() {
        let m = LooseSource::new("sand", "Sand").material();
        assert_eq!(m, LooseMaterial::DEFAULT);
        assert_eq!((repose(), gravity(), top_speed(), slide_speed()), (45, 300, 80, 25));
        let read: LooseSource = crate::ron_io::from_canonical("t", "(id: \"sand\", name: \"Sand\")").unwrap();
        assert_eq!(read.material(), LooseMaterial::DEFAULT);
    }
}
