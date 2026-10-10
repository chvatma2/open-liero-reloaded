//! Scenario files: `fixtures/oracle/<name>.ron`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Keys, OracleError};

/// A deterministic OLX run to record and to replay in Rust.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    pub description: String,
    /// Level file name inside OLX's `share/gamedir/levels/`, such as `"Dirt Level.lxl"`.
    pub level: String,
    /// Mod directory inside OLX's `share/gamedir/`, such as `"Classic"`.
    #[serde(rename = "mod")]
    pub mod_dir: String,
    /// Seed for OLX's `rand()` (`simSetSeed`).
    pub seed: u32,
    /// Number of frames to simulate (100 per second).
    pub frames: u32,
    /// Extra OLX `setvar` values, such as `"GameOptions.GameInfo.GameSpeed": "1"`.
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
    pub worms: Vec<WormSpawn>,
    /// Key changes; a worm holds its keys until its next change.
    #[serde(default)]
    pub inputs: Vec<InputChange>,
    /// Fields allowed to differ from OLX, each with a written reason (spec §6).
    #[serde(default)]
    pub tolerances: Vec<Tolerance>,
}

/// Where a worm is spawned, in world pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WormSpawn {
    pub x: i32,
    pub y: i32,
}

/// From frame `frame` on, worm `worm` (index into `worms`) holds `keys`,
/// written as `"right+jump"`; `""` releases everything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputChange {
    pub frame: u32,
    pub worm: usize,
    pub keys: String,
}

/// Allows numeric field `field` (a trace path such as `"w[0].vy"`, where `[*]`
/// matches any index) to differ by at most `max_abs`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tolerance {
    pub field: String,
    pub max_abs: f32,
    pub reason: String,
}

impl Scenario {
    /// Parses and validates a scenario file.
    pub fn from_ron(text: &str) -> Result<Scenario, OracleError> {
        let sc: Scenario = ron::from_str(text).map_err(|e| {
            OracleError::Scenario(format!("{}:{}: {}", e.span.start.line, e.span.start.col, e.code))
        })?;
        sc.validate()?;
        Ok(sc)
    }

    fn validate(&self) -> Result<(), OracleError> {
        let bad = |m: String| Err(OracleError::Scenario(m));
        if self.frames == 0 {
            return bad("frames must be at least 1".into());
        }
        if self.worms.is_empty() {
            return bad("at least one worm is required".into());
        }
        for c in &self.inputs {
            if c.frame >= self.frames {
                return bad(format!("input at frame {} is past the last frame {}", c.frame, self.frames - 1));
            }
            if c.worm >= self.worms.len() {
                return bad(format!("input for worm {} but only {} worms", c.worm, self.worms.len()));
            }
            Keys::parse(&c.keys).map_err(OracleError::Scenario)?;
        }
        for t in &self.tolerances {
            if t.reason.trim().is_empty() || t.max_abs.is_nan() || t.max_abs < 0.0 {
                return bad(format!("tolerance for {} needs a reason and max_abs >= 0", t.field));
            }
        }
        Ok(())
    }

    /// The key schedule as `(frame, worm, keys)`, sorted by frame. Every worm
    /// gets an entry at frame 0, so OLX drives all of them by script and none
    /// runs its bot AI.
    pub fn schedule(&self) -> Vec<(u32, usize, Keys)> {
        let mut out: Vec<(u32, usize, Keys)> = (0..self.worms.len())
            .filter(|w| !self.inputs.iter().any(|c| c.frame == 0 && c.worm == *w))
            .map(|w| (0, w, Keys::NONE))
            .collect();
        out.extend(self.inputs.iter().map(|c| (c.frame, c.worm, Keys::parse(&c.keys).expect("validated"))));
        out.sort_by_key(|(f, _, _)| *f);
        out
    }

    /// The keys each worm holds during each frame: `result[frame][worm]`.
    pub fn keys_per_frame(&self) -> Vec<Vec<Keys>> {
        let mut held = vec![Keys::NONE; self.worms.len()];
        let mut changes = self.schedule().into_iter().peekable();
        (0..self.frames)
            .map(|f| {
                while let Some((_, w, k)) = changes.next_if(|(cf, _, _)| *cf == f) {
                    held[w] = k;
                }
                held.clone()
            })
            .collect()
    }

    /// The JSON that `tools/olx-oracle/oracle_control.py` reads, writing the
    /// trace to `out` (a path inside the container).
    pub fn control_json(&self, out: &str) -> String {
        let inputs: Vec<(u32, usize, u8)> =
            self.schedule().into_iter().map(|(f, w, k)| (f, w, k.0)).collect();
        let worms: Vec<serde_json::Value> =
            self.worms.iter().map(|w| serde_json::json!({ "x": w.x, "y": w.y })).collect();
        serde_json::json!({
            "level": self.level,
            "mod": self.mod_dir,
            "seed": self.seed,
            "frames": self.frames,
            "settings": self.settings,
            "worms": worms,
            "inputs": inputs,
            "out": out,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"(
        description: "walk",
        level: "Liero Factory.lxl",
        mod: "Classic",
        seed: 12345,
        frames: 10,
        worms: [(x: 190, y: 320), (x: 300, y: 320)],
        inputs: [
            (frame: 2, worm: 0, keys: "right"),
            (frame: 0, worm: 1, keys: "up"),
            (frame: 5, worm: 0, keys: ""),
        ],
    )"#;

    #[test]
    fn parses_with_defaults() {
        let sc = Scenario::from_ron(SAMPLE).unwrap();
        assert_eq!(sc.mod_dir, "Classic");
        assert!(sc.settings.is_empty() && sc.tolerances.is_empty());
    }

    #[test]
    fn schedule_adds_frame_zero_for_unscripted_worms() {
        let sc = Scenario::from_ron(SAMPLE).unwrap();
        assert_eq!(
            sc.schedule(),
            vec![(0, 0, Keys::NONE), (0, 1, Keys(1)), (2, 0, Keys(8)), (5, 0, Keys::NONE)]
        );
    }

    #[test]
    fn keys_are_held_until_changed() {
        let k = Scenario::from_ron(SAMPLE).unwrap().keys_per_frame();
        assert_eq!(k.len(), 10);
        assert_eq!(k[1], vec![Keys::NONE, Keys(1)]);
        assert_eq!(k[2][0], Keys(8));
        assert_eq!(k[4][0], Keys(8));
        assert_eq!(k[5][0], Keys::NONE);
        assert_eq!(k[9][1], Keys(1));
    }

    #[test]
    fn control_json_has_what_the_script_reads() {
        let v: serde_json::Value =
            serde_json::from_str(&Scenario::from_ron(SAMPLE).unwrap().control_json("/work/t.jsonl")).unwrap();
        assert_eq!(v["mod"], "Classic");
        assert_eq!(v["worms"][1]["x"], 300);
        assert_eq!(v["inputs"][2], serde_json::json!([2, 0, 8]));
        assert_eq!(v["out"], "/work/t.jsonl");
    }

    #[test]
    fn rejects_invalid_scenarios() {
        let cases = [
            ("frames: 10", "frames: 0", "at least 1"),
            ("frame: 5, worm: 0", "frame: 10, worm: 0", "past the last frame"),
            ("frame: 5, worm: 0", "frame: 5, worm: 2", "only 2 worms"),
            ("keys: \"right\"", "keys: \"rigth\"", "unknown key"),
        ];
        for (from, to, msg) in cases {
            let err = Scenario::from_ron(&SAMPLE.replace(from, to)).unwrap_err().to_string();
            assert!(err.contains(msg), "{to}: {err}");
        }
        let err = Scenario::from_ron("(oops").unwrap_err().to_string();
        assert!(err.starts_with("1:"), "{err}");
    }

    #[test]
    fn tolerances_need_a_reason() {
        let with = SAMPLE.replace(
            "inputs: [",
            "tolerances: [(field: \"w[0].vy\", max_abs: 0.001, reason: \" \")], inputs: [",
        );
        assert!(Scenario::from_ron(&with).unwrap_err().to_string().contains("needs a reason"));
    }
}
