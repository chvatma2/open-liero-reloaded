//! Colours as hex strings: `"#rrggbb"` (opaque) or `"#rrggbbaa"`.

use olr_data::image::Rgba;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A colour in an authoring file: `"#rrggbb"` (opaque) or `"#rrggbbaa"`
/// (alpha 00 transparent to ff opaque), hex digits read in any case.
/// Written lowercase, without the alpha when opaque.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour(pub Rgba);

const SHAPE: &str = "a colour is \"#rrggbb\" or \"#rrggbbaa\" (hex digits)";

impl Colour {
    pub fn to_hex(&self) -> String {
        let Rgba { r, g, b, a } = self.0;
        if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
    }

    pub fn parse(s: &str) -> Result<Colour, String> {
        let bad = || format!("{s:?}: {SHAPE}");
        let hex = s.strip_prefix('#').ok_or_else(bad)?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(bad());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
        let a = if hex.len() == 8 { byte(6)? } else { 255 };
        Ok(Colour(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a }))
    }
}

impl Serialize for Colour {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Colour {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Colour::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_colours_are_six_digits_and_others_eight() {
        assert_eq!(Colour(Rgba::rgb(0x94, 0x88, 0x00)).to_hex(), "#948800");
        assert_eq!(Colour(Rgba { r: 1, g: 2, b: 3, a: 4 }).to_hex(), "#01020304");
    }

    #[test]
    fn hex_reads_in_any_case_and_round_trips() {
        assert_eq!(Colour::parse("#FFAA00").unwrap(), Colour(Rgba::rgb(255, 170, 0)));
        for c in [Colour(Rgba::rgb(1, 2, 3)), Colour(Rgba { r: 9, g: 8, b: 7, a: 0 })] {
            assert_eq!(Colour::parse(&c.to_hex()).unwrap(), c);
        }
    }

    #[test]
    fn malformed_colours_say_what_a_colour_looks_like() {
        for bad in ["#fa0", "ffaa00", "#ffaa0g", "#ffaa00f", ""] {
            let e = Colour::parse(bad).unwrap_err();
            assert!(e.contains("#rrggbb"), "{bad}: {e}");
        }
    }
}
