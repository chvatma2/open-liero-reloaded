//! The built-in textures (plan 14e ruling R1): generated in code from a
//! fixed integer hash, so there are no files and nothing to licence.
//!
//! Each texture is [`BUILTIN_SIDE`] square and tiles: every pattern and
//! every noise lattice repeats every [`BUILTIN_SIDE`] pixels, so the right
//! edge meets the left and the bottom meets the top. The noise is
//! quantised to a few shades of the material's palette, pixel-art style.

use olr_data::image::{Rgba, RgbaImage};

/// A built-in texture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Texture {
    pub name: &'static str,
    pub image: RgbaImage,
}

/// The side of a built-in texture, in art pixels.
pub const BUILTIN_SIDE: u32 = 64;

/// The largest side of a user texture, in art pixels (spec §Textures).
pub const MAX_TEXTURE_SIDE: u32 = 512;

/// The most textures the user's library holds (plan 14e).
pub const MAX_USER_TEXTURES: usize = 256;

const N: u32 = BUILTIN_SIDE;

/// A generator: the texel at any `(x, y)`, repeating every `N` pixels.
type Generator = (&'static str, fn(u32, u32) -> Rgba);

/// R1's generators, in `builtin`'s order.
const GENERATORS: [Generator; 6] =
    [("Dirt", dirt), ("Rock", rock), ("Grass", grass), ("Brick", brick), ("Metal", metal), ("Wood", wood)];

/// R1's set, in this order: Dirt, Rock, Grass, Brick, Metal, Wood.
pub fn builtin() -> Vec<Texture> {
    GENERATORS
        .into_iter()
        .map(|(name, f)| {
            let mut image = RgbaImage::new(N, N);
            for y in 0..N {
                for x in 0..N {
                    image.set(x, y, f(x, y));
                }
            }
            Texture { name, image }
        })
        .collect()
}

/// Built-in texture `i`'s generator at any `(x, y)` (it repeats every
/// `BUILTIN_SIDE` pixels); `None` past the set.
pub fn builtin_texel(i: usize, x: u32, y: u32) -> Option<Rgba> {
    GENERATORS.get(i).map(|(_, f)| f(x, y))
}

/// A fixed 32-bit integer hash of a lattice point and a seed.
fn hash(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = x.wrapping_mul(0x27d4_eb2d) ^ y.wrapping_mul(0x1656_67b1) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

/// Value noise, 0-255, on a lattice of `cx` × `cy` cells (each dividing
/// `N`), wrapped every `N` pixels; bilinear between lattice points.
fn noise(x: u32, y: u32, (cx, cy): (u32, u32), seed: u32) -> u32 {
    let (gw, gh) = (N / cx, N / cy);
    let (gx, gy, fx, fy) = (x / cx, y / cy, x % cx, y % cy);
    let at = |i: u32, j: u32| hash(i % gw, j % gh, seed) & 255;
    let top = at(gx, gy) * (cx - fx) + at(gx + 1, gy) * fx;
    let bottom = at(gx, gy + 1) * (cx - fx) + at(gx + 1, gy + 1) * fx;
    (top * (cy - fy) + bottom * fy) / (cx * cy)
}

/// Octaves of `noise`, weighted `(cell, weight)`, 0-255.
fn fbm(x: u32, y: u32, octaves: &[((u32, u32), u32)], seed: u32) -> u32 {
    let total: u32 = octaves.iter().map(|o| o.1).sum();
    let sum: u32 = octaves.iter().enumerate().map(|(i, &(c, w))| noise(x, y, c, seed + i as u32) * w).sum();
    sum / total.max(1)
}

/// Shade `v` of `palette`, dark to light: averaged noise gathers around
/// 128, so `v` is spread twice as far from it first (0-255 after).
fn shade(palette: &[Rgba], v: u32) -> Rgba {
    let v = (2 * i64::from(v) - 128).clamp(0, 255) as usize;
    palette[(v * palette.len() / 256).min(palette.len() - 1)]
}

const fn c(r: u8, g: u8, b: u8) -> Rgba {
    Rgba::rgb(r, g, b)
}

const DIRT: [Rgba; 8] = [
    c(62, 38, 20),
    c(78, 48, 26),
    c(94, 58, 32),
    c(108, 68, 38),
    c(122, 78, 44),
    c(136, 90, 52),
    c(150, 102, 60),
    c(166, 116, 70),
];

fn dirt(x: u32, y: u32) -> Rgba {
    let v = fbm(x, y, &[((16, 16), 4), ((8, 8), 2), ((4, 4), 1)], 11);
    let grain = hash(x % N, y % N, 12) & 63;
    shade(&DIRT, (v * 3 + grain * 2) / 4 + 8)
}

const ROCK: [Rgba; 8] = [
    c(52, 52, 56),
    c(68, 68, 72),
    c(84, 84, 88),
    c(100, 100, 104),
    c(116, 116, 120),
    c(132, 132, 134),
    c(150, 150, 150),
    c(170, 168, 164),
];

fn rock(x: u32, y: u32) -> Rgba {
    let v = fbm(x, y, &[((32, 32), 4), ((16, 16), 3), ((8, 8), 2), ((2, 2), 1)], 21);
    let grain = hash(x % N, y % N, 22) & 31;
    shade(&ROCK, v + grain / 2)
}

const GRASS: [Rgba; 8] = [
    c(24, 64, 18),
    c(32, 80, 22),
    c(40, 96, 26),
    c(50, 112, 30),
    c(62, 128, 34),
    c(76, 144, 40),
    c(92, 160, 46),
    c(112, 178, 56),
];

fn grass(x: u32, y: u32) -> Rgba {
    // Blades: narrow across, long down.
    let v = fbm(x, y, &[((2, 16), 3), ((4, 32), 2), ((16, 16), 2)], 31);
    let grain = hash(x % N, y % N, 32) & 31;
    shade(&GRASS, v + grain / 2)
}

const BRICK: [Rgba; 8] = [
    c(96, 34, 26),
    c(112, 40, 30),
    c(126, 46, 34),
    c(140, 52, 38),
    c(154, 60, 42),
    c(166, 70, 48),
    c(178, 82, 56),
    c(190, 96, 66),
];
const MORTAR: Rgba = c(176, 168, 154);

fn brick(x: u32, y: u32) -> Rgba {
    // 16 × 8 bricks, every other row offset by half a brick.
    // A brick's edge next to the mortar is one shadow shade, so every
    // joint, the tile's own included, steps alike.
    let row = y / 8 % (N / 8);
    let bx = (x % N + (row % 2) * 8) % N;
    if y % 8 == 7 || bx % 16 == 15 {
        return MORTAR;
    }
    if matches!(y % 8, 0 | 6) || matches!(bx % 16, 0 | 14) {
        return BRICK[1];
    }
    let base = hash(bx / 16, row, 41) % 96;
    let grain = fbm(x, y, &[((4, 4), 1)], 42) / 2 + (hash(x % N, y % N, 43) & 31);
    shade(&BRICK, base + grain / 2)
}

const METAL: [Rgba; 8] = [
    c(58, 64, 72),
    c(74, 80, 88),
    c(90, 96, 104),
    c(106, 112, 120),
    c(122, 128, 136),
    c(138, 144, 152),
    c(156, 162, 168),
    c(178, 184, 190),
];

fn metal(x: u32, y: u32) -> Rgba {
    // 32 × 32 plates: a light top-left bevel, a dark bottom-right one,
    // rivets in the corners and brushed streaks across.
    let (px, py) = (x % 32, y % 32);
    if px == 0 || py == 0 {
        return METAL[7];
    }
    if px == 31 || py == 31 {
        return METAL[0];
    }
    if matches!(px, 3 | 4 | 27 | 28) && matches!(py, 3 | 4 | 27 | 28) {
        return if (px + py) % 2 == 0 { METAL[6] } else { METAL[1] };
    }
    let v = fbm(x, y, &[((16, 1), 3), ((32, 2), 1)], 51);
    shade(&METAL[1..7], v)
}

const WOOD: [Rgba; 8] = [
    c(80, 46, 22),
    c(98, 58, 28),
    c(116, 70, 34),
    c(134, 84, 42),
    c(150, 98, 50),
    c(166, 112, 58),
    c(182, 128, 68),
    c(198, 146, 82),
];

fn wood(x: u32, y: u32) -> Rgba {
    // Planks 16 high, each with a joint at its own place; grain along them.
    let plank = y / 16 % (N / 16);
    let joint = hash(plank, 0, 61) % N;
    let x = x % N;
    if y % 16 == 15 || x == joint {
        return WOOD[0];
    }
    // Beside a gap or a joint, one worn shade (so the tile's own seam steps alike).
    if matches!(y % 16, 0 | 14) || (x + 1) % N == joint || x == (joint + 1) % N {
        return WOOD[2];
    }
    let v = fbm(x, y, &[((32, 2), 3), ((16, 4), 2), ((8, 1), 1)], 62 + plank);
    let rings = (v / 16 + y % 16 * 2) % 4 * 12;
    shade(&WOOD[1..], (v * 3 / 4 + rings + 16).min(255))
}
