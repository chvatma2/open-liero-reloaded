//! Generated levels for the performance envelope (plan 8): seeded noise
//! caves of any size and art scale, made in memory (no pack is written),
//! so the map-size and art-scale sweeps need no content but a theme
//! (Castle Strike's). Bench only: a game never loads one.
//!
//! A level's id names it: `gen-<width>x<height>-a<art scale>-s<seed>`,
//! such as `gen-2048x2048-a1-s1`. It is what a `MatchSpec`'s `level_id`
//! and a replay's level pack id hold, so `olr-cli stress`, `match` and
//! `gpu-bench` make the same level from the id alone. The arithmetic is
//! integer and `f32` without library calls, so the Windows build makes
//! the same level (its replays' checksums check it).

use olr_content::{LoadedLevel, MAX_ART_SCALE, MAX_COLOR_MAP_BYTES};
use olr_data::image::RgbaImage;
use olr_data::level::{Material, MaterialGrid};

/// The level whose theme generated levels carve with.
pub const THEME_LEVEL: &str = "olx-level-castlestrike";
/// The longest side: OLX's 12-bit worm positions (`olr-import-lxl`'s `MAX_SIDE`).
pub const MAX_SIDE: u32 = 4096;
/// The shortest side.
pub const MIN_SIDE: u32 = 64;
/// The longest side of the art (side × art scale): the texture limit
/// (plan 8 ruling PF7). Its largest level, 2048² at art scale 8, holds
/// two 1 GiB images; a replay naming a larger one is refused.
pub const MAX_ART_SIDE: u32 = 16_384;
/// What a generated level's id starts with.
pub const PREFIX: &str = "gen-";

/// A generated level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenLevel {
    pub width: u32,
    pub height: u32,
    pub art_scale: u32,
    pub seed: u32,
}

/// A deterministic hash of a lattice point, 0 to 1 (SplitMix64's finaliser).
fn hash(x: i64, y: i64, seed: u32) -> f32 {
    let mut z = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ u64::from(seed).wrapping_mul(0x1656_67B1_9E37_79F9);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// Value noise at `(x, y)` on a lattice of `cell` pixels, 0 to 1.
fn value(x: u32, y: u32, cell: u32, seed: u32) -> f32 {
    let (gx, gy) = (i64::from(x / cell), i64::from(y / cell));
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth((x % cell) as f32 / cell as f32), smooth((y % cell) as f32 / cell as f32));
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let top = lerp(hash(gx, gy, seed), hash(gx + 1, gy, seed), sx);
    let bottom = lerp(hash(gx, gy + 1, seed), hash(gx + 1, gy + 1, seed), sx);
    lerp(top, bottom, sy)
}

/// Three octaves: caves of about 64, 24 and 8 pixels.
fn density(x: u32, y: u32, seed: u32) -> f32 {
    0.55 * value(x, y, 64, seed) + 0.30 * value(x, y, 24, seed ^ 1) + 0.15 * value(x, y, 8, seed ^ 2)
}

impl GenLevel {
    /// `gen-<w>x<h>-a<scale>-s<seed>`.
    pub fn id(&self) -> String {
        format!("{PREFIX}{}x{}-a{}-s{}", self.width, self.height, self.art_scale, self.seed)
    }

    /// The level `id` names, if it is a generated one: sides from
    /// [`MIN_SIDE`] to [`MAX_SIDE`], art scale 1 to `MAX_ART_SCALE`, each
    /// side × art scale at most [`MAX_ART_SIDE`], spelt as [`GenLevel::id`]
    /// spells it (no sign, no leading zero). Ids are untrusted (a replay
    /// holds one): nothing is allocated before they pass.
    pub fn parse(id: &str) -> Option<GenLevel> {
        let mut parts = id.strip_prefix(PREFIX)?.split('-');
        let (w, h) = parts.next()?.split_once('x')?;
        let a = parts.next()?.strip_prefix('a')?;
        let s = parts.next()?.strip_prefix('s')?;
        if parts.next().is_some() {
            return None;
        }
        let g = GenLevel {
            width: w.parse().ok()?,
            height: h.parse().ok()?,
            art_scale: a.parse().ok()?,
            seed: s.parse().ok()?,
        };
        let side = MIN_SIDE..=MAX_SIDE;
        let art = |side: u32| side * g.art_scale <= MAX_ART_SIDE;
        (side.contains(&g.width)
            && side.contains(&g.height)
            && (1..=MAX_ART_SCALE).contains(&g.art_scale)
            && art(g.width)
            && art(g.height)
            && g.id() == id)
            .then_some(g)
    }

    /// Why `olr_content::Content::load_level` would refuse a pack of this
    /// level (its colour image above `MAX_COLOR_MAP_BYTES`), if it would.
    /// Generated levels bypass the loader (plan 8 ruling P3); the
    /// envelope reports the cap as the point's breaking cause.
    pub fn content_cap(&self) -> Option<String> {
        let s = u64::from(self.art_scale);
        let bytes = u64::from(self.width) * s * u64::from(self.height) * s * 4;
        (bytes > MAX_COLOR_MAP_BYTES).then(|| {
            format!(
                "the content loader refuses a {}x{} level at art scale {}: a {} MiB colour image, above \
                 MAX_COLOR_MAP_BYTES ({} MiB)",
                self.width,
                self.height,
                self.art_scale,
                bytes >> 20,
                MAX_COLOR_MAP_BYTES >> 20
            )
        })
    }

    /// The material: background where the noise is low (caves), rock where
    /// it is high, dirt between.
    pub fn material(&self) -> MaterialGrid {
        let cells = (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .map(|(x, y)| match density(x, y, self.seed) {
                d if d < 0.45 => Material::Background,
                d if d > 0.68 => Material::Rock,
                _ => Material::Dirt,
            })
            .collect();
        MaterialGrid::from_cells(self.width, self.height, cells).expect("width × height cells")
    }

    /// The level with art at its scale (front: the material's colour, back:
    /// a dark sky, each texel varied) and `theme` (a theme pack id).
    pub fn level(&self, theme: &str) -> LoadedLevel {
        let material = self.material();
        let s = self.art_scale;
        let (w, h) = (self.width * s, self.height * s);
        let mut front = Vec::with_capacity(w as usize * h as usize * 4);
        let mut back = Vec::with_capacity(w as usize * h as usize * 4);
        for ay in 0..h {
            for ax in 0..w {
                let n = (hash(i64::from(ax), i64::from(ay), self.seed ^ 7) * 40.0) as u8;
                let sky = [10 + n / 4, 12 + n / 4, 40 + n / 2, 255];
                let px = match material.get(ax / s, ay / s) {
                    Material::Background => sky,
                    Material::Dirt => [120 + n, 80 + n / 2, 40, 255],
                    Material::Rock => [90 + n, 90 + n, 100 + n, 255],
                };
                front.extend_from_slice(&px);
                back.extend_from_slice(&sky);
            }
        }
        LoadedLevel {
            name: self.id(),
            material,
            front: RgbaImage::from_raw(w, h, front).expect("w × h texels"),
            back: RgbaImage::from_raw(w, h, back).expect("w × h texels"),
            art_scale: s,
            theme: theme.to_string(),
            spawns: Vec::new(),
            look: None,
            glow: None,
            warnings: Vec::new(),
            clear: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olr_data::level::Material;

    #[test]
    fn ids_round_trip_and_bad_ones_are_refused() {
        let g = GenLevel { width: 2048, height: 1024, art_scale: 2, seed: 7 };
        assert_eq!(g.id(), "gen-2048x1024-a2-s7");
        assert_eq!(GenLevel::parse(&g.id()), Some(g));
        for bad in [
            "olx-level-castlestrike",
            "gen-2048x1024-a2",
            "gen-2048x1024-a2-s7-x",
            "gen-8192x1024-a1-s1",
            "gen-32x1024-a1-s1",
            "gen-2048x1024-a9-s1",
            "gen-2048x1024-a0-s1",
            "gen-axb-a1-s1",
        ] {
            assert_eq!(GenLevel::parse(bad), None, "{bad}");
        }
    }

    /// Replays are untrusted (plan 8 ruling PF7): a side × art scale above
    /// the texture limit, or numbers that do not fit, are refused before
    /// anything is allocated; only the canonical spelling of an id parses.
    #[test]
    fn oversized_and_odd_ids_are_refused() {
        for good in ["gen-2048x2048-a8-s1", "gen-4096x4096-a4-s1", "gen-4096x64-a4-s4294967295"] {
            assert!(GenLevel::parse(good).is_some(), "{good}");
        }
        for bad in [
            "gen-4096x64-a8-s1",
            "gen-64x4096-a5-s1",
            "gen-2049x64-a8-s1",
            "gen-4097x64-a1-s1",
            "gen-99999999999x64-a1-s1",
            "gen-64x64-a1-s4294967296",
            "gen-064x64-a1-s1",
            "gen-+64x64-a1-s1",
            "gen-64x64-a1-s",
            "gen-64x64-a1-s-1",
            "gen-",
            "",
        ] {
            assert_eq!(GenLevel::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn caves_are_seeded_and_open() {
        let g = GenLevel { width: 256, height: 128, art_scale: 1, seed: 1 };
        let m = g.material();
        assert_eq!((m.width(), m.height()), (256, 128));
        assert_eq!(m, g.material(), "the same seed, the same level");
        assert_ne!(m, GenLevel { seed: 2, ..g }.material());
        for seed in [1, 2] {
            let m = GenLevel { seed, ..g }.material();
            let open = m.count(Material::Background) as f64 / m.cells().len() as f64;
            assert!((0.2..0.8).contains(&open), "seed {seed}: background share {open}");
            assert!(m.count(Material::Rock) > 0 && m.count(Material::Dirt) > 0);
        }
    }

    #[test]
    fn art_is_at_its_scale() {
        let g = GenLevel { width: 64, height: 64, art_scale: 3, seed: 1 };
        let l = g.level("olx-theme-x");
        assert_eq!((l.front.width(), l.front.height()), (192, 192));
        assert_eq!((l.back.width(), l.back.height()), (192, 192));
        assert_eq!((l.art_scale, l.theme.as_str(), l.name.as_str()), (3, "olx-theme-x", "gen-64x64-a3-s1"));
        assert!(l.spawns.is_empty(), "worms spawn by FindSpot");
    }

    #[test]
    fn the_content_cap_is_reported_not_enforced() {
        let at = |art_scale| GenLevel { width: 2048, height: 2048, art_scale, seed: 1 };
        assert_eq!(at(4).content_cap(), None, "8192² × 4 bytes is exactly the 256 MiB cap");
        assert!(at(8).content_cap().unwrap().contains("MAX_COLOR_MAP_BYTES"));
    }
}
