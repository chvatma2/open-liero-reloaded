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

/// First line of a trace. The fields after `features` were added in M4
/// (oracle v2); older traces leave them empty.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// OLX version string.
    pub olx: String,
    /// Every OLX gameplay feature (`src/common/FeatureList.cpp`) as OLX printed it.
    pub features: BTreeMap<String, String>,
    /// Weapon names, indexed by OLX weapon id (`slots[].id`, `s[].wpn`).
    #[serde(default)]
    pub weapons: Vec<String>,
    /// Projectile names, indexed by OLX projectile index (`p[].t`). A name is
    /// the path `rl-import-lgs` derives: the weapon's id, then `.spawn` or
    /// `.trail` per level. Where the pack shares an identical definition, it
    /// keeps the first such path as the name.
    #[serde(default)]
    pub projectiles: Vec<String>,
    /// OLX options that are not features: `Game.Blood`, `Game.Particles`,
    /// `DoProjectileSimulationInDedicated`, the clock offset `K` (ms), and
    /// `T0`, frame -1's absolute `simulationTime` (ms).
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    /// The scenario seed (`simSetSeed`).
    #[serde(default)]
    pub seed: Option<u32>,
}

/// One trace line. Times (`st`, `ct`, and the time fields of worms,
/// projectiles, shots and bonuses) are integer milliseconds since frame -1's
/// `simulationTime`, so traces do not depend on when OLX started.
/// The `Option` fields and `s`, `sb` were added in M4 (oracle v2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub f: i64,
    /// OLX's `serverFrame`.
    #[serde(default)]
    pub sf: Option<i64>,
    /// `simulationTime` (ms since frame -1).
    #[serde(default)]
    pub st: Option<i64>,
    /// `tLX->currentTime` as pinned by the oracle: `st + K` (ms since frame -1).
    #[serde(default)]
    pub ct: Option<i64>,
    /// `rand()` calls since the scenario's seed was set.
    #[serde(default)]
    pub rc: Option<u64>,
    /// `rnd()` (the `FindSpot` MT19937) draws since the scenario's seed was set.
    #[serde(default)]
    pub mt: Option<u64>,
    /// Used slots in the entity pool (1024 slots, never freed in a dedicated server).
    #[serde(default)]
    pub ents: Option<i64>,
    /// Shots delivered and processed right before this frame (spec §18 D1).
    #[serde(default)]
    pub s: Vec<ShotRec>,
    pub w: Vec<WormRec>,
    pub p: Vec<ProjRec>,
    /// The client's bonuses.
    pub b: Vec<BonusRec>,
    /// The server's bonuses.
    #[serde(default)]
    pub sb: Vec<ServerBonusRec>,
    /// From the frame `GameServer::GameOver` ran in on (absent before):
    /// that frame and the winner (plan 6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub go: Option<GameOverRec>,
    /// FNV-1a 64 of the material grid, row by row, as 16 hex digits.
    pub mh: String,
}

/// The match's end: `game.gameOverFrame` (a `serverFrame`) and the winner
/// `GameServer::GameOver` computed (`CGameMode::Winner`, -1 for none).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameOverRec {
    pub f: i64,
    pub w: i32,
}

/// A shot as the client decoded it (`shoot_t` after `readSingle`/`readMulti`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShotRec {
    pub w: i32,
    pub wpn: i32,
    pub x: Num,
    pub y: Num,
    pub vx: Num,
    pub vy: Num,
    pub ang: i32,
    pub rnd: i32,
    /// `nSpeed`: the worm's speed times 100 (`ProjRelativeVel`).
    pub spd: i32,
    pub rel: bool,
    /// `fTime`: the server time of the shot, absolute ms (`serverFrame * 10`).
    pub t: i64,
    /// `spawnTime()` (ms since frame -1).
    pub spawn: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub suicides: Option<i32>,
    /// `fTimeofDeath` (ms since frame -1); null while the worm is alive.
    #[serde(default)]
    pub tod: Option<i64>,
    /// Bits: 1 `bRespawnRequested`, 2 `bCanRespawnNow`, 4 `bSpawnedOnce`.
    #[serde(default)]
    pub respawn: Option<u32>,
    /// `fLastBlood` (ms since frame -1); null if the worm never bled.
    #[serde(default)]
    pub blood: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SlotRec {
    pub id: i32,
    pub ch: Num,
    pub rl: bool,
    pub lf: Num,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
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
    /// `iOwner`.
    #[serde(default)]
    pub own: Option<i32>,
    /// `iRandom`, the index into the fixed random table.
    #[serde(default)]
    pub rnd: Option<i32>,
    /// `fLastSimulationTime` (ms since frame -1).
    #[serde(default)]
    pub ls: Option<i64>,
    /// `fSpawnTime` (ms since frame -1).
    #[serde(default)]
    pub sp: Option<i64>,
    /// `fIgnoreWormCollBeforeTime` (ms since frame -1).
    #[serde(default)]
    pub ig: Option<i64>,
    /// `fWallshootTime`, a duration in seconds.
    #[serde(default)]
    pub ws: Option<Num>,
    /// `fLastTrailProj` (ms since frame -1); null if never set.
    #[serde(default)]
    pub tr: Option<i64>,
    /// `vOldPos`, the last terrain-safe position.
    #[serde(default)]
    pub ox: Option<Num>,
    #[serde(default)]
    pub oy: Option<Num>,
    /// `fFrame`, the animation frame (decides `ANI_ONCE` deletion).
    #[serde(default)]
    pub fr: Option<Num>,
    /// `iColour` as `0xRRGGBB`.
    #[serde(default)]
    pub col: Option<u32>,
}

/// A used client bonus slot.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BonusRec {
    pub i: i32,
    /// Kind: 0 weapon, 1 health (`BNS_*`).
    pub t: i32,
    pub x: Num,
    pub y: Num,
    #[serde(default)]
    pub vx: Option<Num>,
    #[serde(default)]
    pub vy: Option<Num>,
    #[serde(default)]
    pub life: Option<Num>,
    /// Weapon id (0 for a client health bonus).
    #[serde(default)]
    pub wpn: Option<i32>,
}

/// A used server bonus slot (`cServer->cBonuses`). The server keeps a slot
/// for the bonus's whole life, even after a worm picked it up.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerBonusRec {
    pub i: i32,
    /// Kind: 0 weapon, 1 health.
    pub t: i32,
    pub x: Num,
    pub y: Num,
    pub wpn: i32,
    /// `fSpawnTime` (ms since frame -1).
    pub spawn: i64,
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
        r#"{"f":0,"w":[{"id":0,"alive":true,"x":456,"y":218.001709,"vx":"-inf","vy":34.1000061,"hp":"NaN","ang":-0,"angv":0,"face":1,"move":1,"ground":false,"keys":4,"wpn":0,"slots":[{"id":13,"ch":1,"rl":false,"lf":0}],"rope":{"rel":false,"sh":false,"at":false,"pw":-1,"x":0,"y":0,"vx":0,"vy":0},"lives":-2,"kills":0,"deaths":0,"dirt":0}],"p":[{"i":3,"t":7,"x":1.5,"y":2,"vx":0,"vy":0,"life":0.25}],"b":[{"i":0,"t":1,"x":10,"y":20}],"mh":"86afa75d225d80a7"}"#,
        "\n",
    );

    // The same shape as an oracle v2 recording (fire on Liero Factory): every
    // field added in M4, with made-up values.
    pub(crate) const OLX_V2: &str = concat!(
        r#"{"olx":"OpenLieroX/20260912.1+git.d1ad953.dirty","features":{"ForceLX56Aim":"true","LX56PhysicsFPS":"84"},"weapons":["Bazooka","Minigun"],"projectiles":["bazooka","bazooka.spawn","minigun"],"options":{"DoProjectileSimulationInDedicated":"true","Game.Blood":"100","Game.Particles":"true","K":"10"},"seed":12345}"#,
        "\n",
        r#"{"f":-1,"sf":100,"st":0,"ct":10,"rc":0,"mt":0,"ents":0,"s":[],"w":[{"id":0,"alive":true,"x":190,"y":315,"vx":0,"vy":0,"hp":100,"ang":0,"angv":0,"face":1,"move":1,"ground":false,"keys":0,"wpn":1,"slots":[{"id":1,"ch":1,"rl":false,"lf":0}],"rope":{"rel":false,"sh":false,"at":false,"pw":-1,"x":0,"y":0,"vx":0,"vy":0},"lives":-2,"kills":0,"deaths":0,"dirt":0,"suicides":0,"tod":null,"respawn":4,"blood":null}],"p":[],"b":[],"sb":[],"mh":"86afa75d225d80a7"}"#,
        "\n",
        r#"{"f":0,"sf":101,"st":10,"ct":20,"rc":17,"mt":2,"ents":3,"s":[{"w":0,"wpn":1,"x":190,"y":315,"vx":0,"vy":-1,"ang":0,"rnd":77,"spd":0,"rel":false,"t":1000,"spawn":-10}],"w":[{"id":0,"alive":false,"x":190,"y":315.5,"vx":0,"vy":0.5,"hp":0,"ang":0,"angv":0,"face":1,"move":1,"ground":true,"keys":2,"wpn":1,"slots":[{"id":1,"ch":0.98,"rl":false,"lf":0.05}],"rope":{"rel":false,"sh":false,"at":false,"pw":-1,"x":0,"y":0,"vx":0,"vy":0},"lives":-2,"kills":0,"deaths":1,"dirt":0,"suicides":1,"tod":10,"respawn":5,"blood":-20}],"p":[{"i":0,"t":2,"x":198,"y":310,"vx":250,"vy":0,"life":0,"own":0,"rnd":78,"ls":11,"sp":0,"ig":30,"ws":0.0115,"tr":null,"ox":190,"oy":310,"fr":0.25,"col":16711680}],"b":[{"i":0,"t":1,"x":200,"y":300,"vx":0,"vy":0.880000055,"life":0.0110000009,"wpn":0}],"sb":[{"i":0,"t":1,"x":200.5,"y":300,"wpn":-1,"spawn":0}],"mh":"86afa75d225d80a8"}"#,
        "\n",
    );

    #[test]
    fn parses_v2_fields() {
        let t = Trace::from_jsonl(OLX_V2).unwrap();
        let h = &t.header;
        assert_eq!(h.weapons, ["Bazooka", "Minigun"]);
        assert_eq!(h.projectiles, ["bazooka", "bazooka.spawn", "minigun"]);
        assert_eq!((h.options["K"].as_str(), h.options["Game.Blood"].as_str()), ("10", "100"));
        assert_eq!(h.seed, Some(12345));
        let f = t.frame(0).unwrap();
        assert_eq!(
            (f.sf, f.st, f.ct, f.rc, f.mt, f.ents),
            (Some(101), Some(10), Some(20), Some(17), Some(2), Some(3))
        );
        let s = &f.s[0];
        assert_eq!(
            (s.w, s.wpn, s.ang, s.rnd, s.spd, s.rel, s.t, s.spawn),
            (0, 1, 0, 77, 0, false, 1000, -10)
        );
        assert_eq!((s.x, s.y, s.vx, s.vy), (Num(190.0), Num(315.0), Num(0.0), Num(-1.0)));
        let w = &f.w[0];
        assert_eq!((w.suicides, w.tod, w.respawn, w.blood), (Some(1), Some(10), Some(5), Some(-20)));
        assert_eq!(t.frames[0].w[0].tod, None, "null while alive");
        let p = &f.p[0];
        assert_eq!(
            (p.own, p.rnd, p.ls, p.sp, p.ig, p.tr, p.col),
            (Some(0), Some(78), Some(11), Some(0), Some(30), None, Some(0xff0000))
        );
        assert_eq!(
            (p.ws, p.ox, p.oy, p.fr),
            (Some(Num(0.0115)), Some(Num(190.0)), Some(Num(310.0)), Some(Num(0.25)))
        );
        let b = &f.b[0];
        assert_eq!(
            (b.vx, b.vy, b.life, b.wpn),
            (Some(Num(0.0)), Some(Num(0.880000055)), Some(Num(0.0110000009)), Some(0))
        );
        let sb = &f.sb[0];
        assert_eq!((sb.i, sb.t, sb.x, sb.y, sb.wpn, sb.spawn), (0, 1, Num(200.5), Num(300.0), -1, 0));
        assert_eq!(Trace::from_jsonl(&t.to_jsonl()).unwrap(), t);
    }

    #[test]
    fn parses_v1_traces() {
        let t = Trace::from_jsonl(OLX).unwrap();
        let h = &t.header;
        assert!(h.weapons.is_empty() && h.projectiles.is_empty() && h.options.is_empty() && h.seed.is_none());
        let f = t.frame(0).unwrap();
        assert_eq!((f.sf, f.st, f.ct, f.rc, f.mt, f.ents), (None, None, None, None, None, None));
        assert!(f.s.is_empty() && f.sb.is_empty());
        let w = &f.w[0];
        assert_eq!((w.suicides, w.tod, w.respawn, w.blood), (None, None, None, None));
        let p = &f.p[0];
        assert_eq!((p.own, p.rnd, p.ls, p.sp, p.ig, p.tr, p.col), (None, None, None, None, None, None, None));
        assert_eq!((p.ws, p.ox, p.oy, p.fr), (None, None, None, None));
        let b = &f.b[0];
        assert_eq!((b.vx, b.vy, b.life, b.wpn), (None, None, None, None));
    }

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
