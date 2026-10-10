//! Trace files: one JSON header line, then one line per frame, as written by
//! the OLX hooks (`tools/olx-oracle/oracle-hooks.patch`, `OracleHooks.cpp`).
//! Frame -1 is the state when recording started; frame `n` is the state after
//! simulating frame `n` with the keys the scenario holds during frame `n`.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{Read, Write};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::OracleError;

/// An `f32` as OLX recorded it: a JSON number, or `"NaN"`, `"inf"`, `"-inf"`.
/// Equality is bitwise (so `-0.0 != 0.0`), except that all NaNs are equal.
#[derive(Clone, Copy, Debug, Default)]
pub struct Num(pub f32);

impl PartialEq for Num {
    fn eq(&self, other: &Self) -> bool {
        (self.0.is_nan() && other.0.is_nan()) || self.0.to_bits() == other.0.to_bits()
    }
}

impl fmt::Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for Num {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            v if v.is_nan() => s.serialize_str("NaN"),
            v if v == f32::INFINITY => s.serialize_str("inf"),
            v if v == f32::NEG_INFINITY => s.serialize_str("-inf"),
            v => s.serialize_f32(v),
        }
    }
}

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Num, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            N(f64),
            S(String),
        }
        // OLX prints 9 significant digits, so the nearest f32 is the original value.
        match Raw::deserialize(d)? {
            Raw::N(v) => Ok(Num(v as f32)),
            Raw::S(s) => match s.as_str() {
                "NaN" => Ok(Num(f32::NAN)),
                "inf" => Ok(Num(f32::INFINITY)),
                "-inf" => Ok(Num(f32::NEG_INFINITY)),
                other => Err(serde::de::Error::custom(format!("not a number: {other:?}"))),
            },
        }
    }
}

/// First line of a trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// OLX version string.
    pub olx: String,
    /// Every OLX gameplay feature (`src/common/FeatureList.cpp`) as OLX printed it.
    pub features: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub f: i64,
    pub w: Vec<WormRec>,
    pub p: Vec<ProjRec>,
    pub b: Vec<BonusRec>,
    /// FNV-1a 64 of the material grid, row by row, as 16 hex digits.
    pub mh: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WormRec {
    pub id: i32,
    pub alive: bool,
    pub x: Num,
    pub y: Num,
    pub vx: Num,
    pub vy: Num,
    pub hp: Num,
    pub ang: Num,
    pub angv: Num,
    pub face: i32,
    #[serde(rename = "move")]
    pub move_dir: i32,
    pub ground: bool,
    /// `worm_state_t::asInt()`: the movement/shoot/jump/carve state OLX derived from the keys.
    pub keys: u32,
    pub wpn: i32,
    pub slots: Vec<SlotRec>,
    pub rope: RopeRec,
    pub lives: i32,
    pub kills: i32,
    pub deaths: i32,
    pub dirt: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlotRec {
    pub id: i32,
    pub ch: Num,
    pub rl: bool,
    pub lf: Num,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RopeRec {
    pub rel: bool,
    pub sh: bool,
    pub at: bool,
    pub pw: i32,
    pub x: Num,
    pub y: Num,
    pub vx: Num,
    pub vy: Num,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjRec {
    /// Slot in OLX's projectile pool.
    pub i: i32,
    /// Projectile type: its load-order index in the mod (`-1` if unknown).
    pub t: i32,
    pub x: Num,
    pub y: Num,
    pub vx: Num,
    pub vy: Num,
    pub life: Num,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BonusRec {
    pub i: i32,
    pub t: i32,
    pub x: Num,
    pub y: Num,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trace {
    pub header: Header,
    pub frames: Vec<Frame>,
}

fn line_err(line: usize, e: impl fmt::Display) -> OracleError {
    OracleError::Trace { line, message: e.to_string() }
}

impl Trace {
    /// Parses the JSON-lines text OLX wrote.
    pub fn from_jsonl(text: &str) -> Result<Trace, OracleError> {
        let mut lines = text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty());
        let (_, first) = lines.next().ok_or_else(|| line_err(1, "empty trace"))?;
        let header: Header = serde_json::from_str(first).map_err(|e| line_err(1, e))?;
        let frames = lines
            .map(|(i, l)| serde_json::from_str::<Frame>(l).map_err(|e| line_err(i + 1, e)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Trace { header, frames })
    }

    /// Parses a gzip-compressed trace (`.jsonl.gz`).
    pub fn from_gz(bytes: &[u8]) -> Result<Trace, OracleError> {
        Trace::from_jsonl(&gunzip(bytes)?)
    }

    pub fn to_jsonl(&self) -> String {
        let mut out = serde_json::to_string(&self.header).expect("serializable") + "\n";
        for f in &self.frames {
            out += &(serde_json::to_string(f).expect("serializable") + "\n");
        }
        out
    }

    /// The frame recorded with number `f`.
    pub fn frame(&self, f: i64) -> Option<&Frame> {
        self.frames.iter().find(|fr| fr.f == f)
    }
}

/// Gzip with a fixed header (no file name, mtime 0), so equal input gives equal bytes.
pub fn gzip(text: &str) -> Vec<u8> {
    let mut enc = flate2::GzBuilder::new().mtime(0).write(Vec::new(), flate2::Compression::best());
    enc.write_all(text.as_bytes()).expect("in-memory write");
    enc.finish().expect("in-memory gzip")
}

pub fn gunzip(bytes: &[u8]) -> Result<String, OracleError> {
    let mut s = String::new();
    flate2::read::GzDecoder::new(bytes).read_to_string(&mut s)?;
    Ok(s)
}

#[cfg(test)]
// Float literals below are copied from OLX's 9-digit output on purpose.
#[allow(clippy::excessive_precision)]
pub(crate) mod tests {
    use super::*;

    // Abbreviated from a real OLX recording (fall on Dirt Level).
    pub(crate) const OLX: &str = concat!(
        r#"{"olx":"OpenLieroX/20260912.1+git.d1ad953.dirty","features":{"ForceLX56Aim":"true","LX56PhysicsFPS":"84"}}"#,
        "\n",
        r#"{"f":-1,"w":[{"id":0,"alive":true,"x":456,"y":212,"vx":0,"vy":0,"hp":100,"ang":0,"angv":0,"face":1,"move":1,"ground":false,"keys":0,"wpn":0,"slots":[{"id":13,"ch":1,"rl":false,"lf":0}],"rope":{"rel":false,"sh":false,"at":false,"pw":-1,"x":0,"y":0,"vx":0,"vy":0},"lives":-2,"kills":0,"deaths":0,"dirt":0}],"p":[],"b":[],"mh":"86afa75d225d80a7"}"#,
        "\n",
        r#"{"f":0,"w":[{"id":0,"alive":true,"x":456,"y":218.001709,"vx":"-inf","vy":34.1000061,"hp":"NaN","ang":-0,"angv":0,"face":1,"move":1,"ground":false,"keys":4,"wpn":0,"slots":[{"id":13,"ch":1,"rl":false,"lf":0}],"rope":{"rel":false,"sh":false,"at":false,"pw":-1,"x":0,"y":0,"vx":0,"vy":0},"lives":-2,"kills":0,"deaths":0,"dirt":0}],"p":[{"i":3,"t":7,"x":1.5,"y":2,"vx":0,"vy":0,"life":0.25}],"b":[],"mh":"86afa75d225d80a7"}"#,
        "\n",
    );

    #[test]
    fn parses_olx_output() {
        let t = Trace::from_jsonl(OLX).unwrap();
        assert_eq!(t.header.features["LX56PhysicsFPS"], "84");
        assert_eq!(t.frames.len(), 2);
        let w = &t.frame(0).unwrap().w[0];
        assert_eq!(w.y.0, 218.001709_f32);
        assert_eq!(w.vx.0, f32::NEG_INFINITY);
        assert!(w.hp.0.is_nan());
        assert_eq!(w.ang.0.to_bits(), (-0.0_f32).to_bits());
        assert_eq!(t.frame(0).unwrap().p[0].t, 7);
    }

    #[test]
    fn nine_digit_floats_round_trip_exactly() {
        for v in [218.001709_f32, 34.1000061, 1.0e-30, -123456.789, f32::MAX, f32::MIN_POSITIVE] {
            let text = format!("{v:.9e}");
            let back: Num = serde_json::from_str(&text).unwrap();
            assert_eq!(back, Num(v), "{text}");
        }
    }

    #[test]
    fn jsonl_and_gzip_round_trip() {
        let t = Trace::from_jsonl(OLX).unwrap();
        assert_eq!(Trace::from_jsonl(&t.to_jsonl()).unwrap(), t);
        let gz = gzip(OLX);
        assert_eq!(gz, gzip(OLX), "gzip output must be deterministic");
        assert_eq!(Trace::from_gz(&gz).unwrap(), t);
    }

    #[test]
    fn errors_name_the_line() {
        let bad = OLX.replace(r#""f":0"#, r#""f":"zero""#);
        let err = Trace::from_jsonl(&bad).unwrap_err().to_string();
        assert!(err.starts_with("trace line 3:"), "{err}");
        assert!(Trace::from_jsonl("").unwrap_err().to_string().contains("empty trace"));
    }

    #[test]
    fn num_equality_is_bitwise_except_nan() {
        assert_eq!(Num(f32::NAN), Num(-f32::NAN));
        assert_ne!(Num(0.0), Num(-0.0));
        assert_ne!(Num(1.0), Num(1.0000001));
    }
}
