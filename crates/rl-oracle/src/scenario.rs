//! Scenario files: `fixtures/oracle/<name>.ron`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Keys, OracleError};

/// A deterministic OLX run to record and to replay in Rust.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub description: String,
    /// Level file name inside OLX's `share/gamedir/levels/`, such as `"Dirt Level.lxl"`.
    pub level: String,
    /// Mod directory inside OLX's `share/gamedir/`, such as `"Classic"`.
    #[serde(rename = "mod")]
    pub mod_dir: String,
    /// Seed for OLX's `rand()` and the `FindSpot` generators (`simSetSeed`).
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
    /// Weapon slots set right after the spawns (`simSetWeapons`).
    #[serde(default)]
    pub weapons: Vec<WormWeapons>,
    /// `(worm, hp)`: health set right after the spawns (`simSetHealth`).
    #[serde(default)]
    pub health: Vec<(usize, f32)>,
    /// Bonuses placed through OLX's server path at given frames (`simPlaceBonus`).
    #[serde(default)]
    pub bonuses: Vec<BonusPlacement>,
    /// Fields allowed to differ from OLX, each with a written reason (spec §6).
    #[serde(default)]
    pub tolerances: Vec<Tolerance>,
}

/// Where a worm is spawned, in world pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WormSpawn {
    pub x: i32,
    pub y: i32,
}

/// From frame `frame` on, worm `worm` (index into `worms`) holds `keys`,
/// written as `"right+jump"`; `""` releases everything.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputChange {
    pub frame: u32,
    pub worm: usize,
    pub keys: String,
}

/// Weapon slots per worm: OLX's `WeaponSlotsNum` (default 5, the value in every
/// trace header so far). `simSetWeapons` needs one name per slot.
pub const WEAPON_SLOTS: usize = 5;

/// Worm `worm`'s weapon slots, by OLX weapon name, exactly one per slot
/// ([`WEAPON_SLOTS`]), with slot `current` selected. Every slot starts full:
/// charge 1, not reloading, no last fire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WormWeapons {
    pub worm: usize,
    pub names: Vec<String>,
    pub current: usize,
}

/// A bonus that appears at the start of frame `frame` at `(x, y)`, in the
/// first free bonus slot of both the server and the client. `weapon` names
/// the weapon of a weapon bonus.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BonusPlacement {
    pub frame: u32,
    pub x: i32,
    pub y: i32,
    pub kind: BonusKind,
    #[serde(default)]
    pub weapon: Option<String>,
}

/// A bonus kind, written `"health"` or `"weapon"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum BonusKind {
    Health,
    Weapon,
}

impl BonusKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BonusKind::Health => "health",
            BonusKind::Weapon => "weapon",
        }
    }
}

impl TryFrom<String> for BonusKind {
    type Error = String;
    fn try_from(s: String) -> Result<BonusKind, String> {
        match s.as_str() {
            "health" => Ok(BonusKind::Health),
            "weapon" => Ok(BonusKind::Weapon),
            other => Err(format!("unknown bonus kind {other:?} (health or weapon)")),
        }
    }
}

impl From<BonusKind> for String {
    fn from(k: BonusKind) -> String {
        k.as_str().to_string()
    }
}

/// Allows numeric field `field` (a trace path such as `"w[0].vy"`, where `[*]`
/// matches any index) to differ by at most `max_abs`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
        for w in &self.weapons {
            if w.worm >= self.worms.len() {
                return bad(format!("weapons for worm {} but only {} worms", w.worm, self.worms.len()));
            }
            if w.names.len() != WEAPON_SLOTS {
                return bad(format!(
                    "worm {} needs exactly {WEAPON_SLOTS} weapon names (one per slot), got {}",
                    w.worm,
                    w.names.len()
                ));
            }
            if w.current >= w.names.len() {
                return bad(format!(
                    "current weapon {} but worm {} has {} slots",
                    w.current,
                    w.worm,
                    w.names.len()
                ));
            }
            if let Some(n) = w.names.iter().find(|n| !valid_weapon_name(n)) {
                return bad(format!(
                    "weapon name {n:?} must be non-empty, without commas, quotes or line breaks"
                ));
            }
        }
        for &(worm, hp) in &self.health {
            if worm >= self.worms.len() {
                return bad(format!("health for worm {worm} but only {} worms", self.worms.len()));
            }
            // Exactly 0 is a Gusanos death path the port does not reproduce (spec §18 D9).
            if !hp.is_finite() || hp <= 0.0 {
                return bad(format!("health {hp} for worm {worm} must be finite and > 0"));
            }
        }
        for b in &self.bonuses {
            if b.frame >= self.frames {
                return bad(format!("bonus at frame {} is past the last frame {}", b.frame, self.frames - 1));
            }
            if i16::try_from(b.x).is_err() || i16::try_from(b.y).is_err() {
                return bad(format!(
                    "bonus position ({}, {}) must fit in 16 bits, as OLX sends it (S2C_SPAWNBONUS)",
                    b.x, b.y
                ));
            }
            match (b.kind, &b.weapon) {
                (BonusKind::Health, Some(_)) => return bad("a health bonus has no weapon".into()),
                (BonusKind::Weapon, None) => return bad("a weapon bonus needs a weapon".into()),
                (_, Some(n)) if !valid_weapon_name(n) => return bad(format!("bad bonus weapon name {n:?}")),
                _ => {}
            }
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
        let bonuses: Vec<serde_json::Value> = self
            .bonuses
            .iter()
            .map(|b| {
                serde_json::json!([b.frame, b.x, b.y, b.kind.as_str(), b.weapon.as_deref().unwrap_or("")])
            })
            .collect();
        serde_json::json!({
            "level": self.level,
            "mod": self.mod_dir,
            "seed": self.seed,
            "frames": self.frames,
            "settings": self.settings,
            "worms": worms,
            "inputs": inputs,
            "weapons": self.weapons,
            "health": self.health,
            "bonuses": bonuses,
            "out": out,
        })
        .to_string()
    }
}

/// A weapon name that survives `simSetWeapons`'s quoted comma list and the
/// line-based input schedule.
fn valid_weapon_name(n: &str) -> bool {
    !n.trim().is_empty() && !n.contains([',', '"', '\n', '\r'])
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
            ("inputs: [", "input: [", "Unexpected field"),
            ("(x: 190, y: 320)", "(x: 190, y: 320, z: 1)", "Unexpected field"),
            ("(frame: 2, worm: 0,", "(frame: 2, worm: 0, wrom: 0,", "Unexpected field"),
        ];
        for (from, to, msg) in cases {
            let err = Scenario::from_ron(&SAMPLE.replace(from, to)).unwrap_err().to_string();
            assert!(err.contains(msg), "{to}: {err}");
        }
        let err = Scenario::from_ron("(oops").unwrap_err().to_string();
        assert!(err.starts_with("1:"), "{err}");
    }

    const ARMED: &str = r#"(
        description: "fire",
        level: "Liero Factory.lxl",
        mod: "Classic",
        seed: 1,
        frames: 10,
        worms: [(x: 190, y: 320), (x: 300, y: 320)],
        weapons: [(worm: 1, names: ["Minigun", "Big Nuke", "Uzi", "Laser", "Zimm"], current: 1)],
        health: [(0, 14.5)],
        bonuses: [
            (frame: 3, x: 200, y: 300, kind: "health"),
            (frame: 4, x: 210, y: 300, kind: "weapon", weapon: Some("Big Nuke")),
        ],
    )"#;

    #[test]
    fn parses_weapons_health_and_bonuses() {
        let sc = Scenario::from_ron(ARMED).unwrap();
        assert_eq!(
            sc.weapons,
            [WormWeapons {
                worm: 1,
                names: ["Minigun", "Big Nuke", "Uzi", "Laser", "Zimm"].map(String::from).to_vec(),
                current: 1
            }]
        );
        assert_eq!(sc.health, [(0, 14.5)]);
        assert_eq!(
            sc.bonuses[0],
            BonusPlacement { frame: 3, x: 200, y: 300, kind: BonusKind::Health, weapon: None }
        );
        assert_eq!(sc.bonuses[1].kind, BonusKind::Weapon);
        assert_eq!(sc.bonuses[1].weapon.as_deref(), Some("Big Nuke"));
        let plain = Scenario::from_ron(SAMPLE).unwrap();
        assert!(plain.weapons.is_empty() && plain.health.is_empty() && plain.bonuses.is_empty());
    }

    #[test]
    fn control_json_has_weapons_health_and_bonuses() {
        let v: serde_json::Value =
            serde_json::from_str(&Scenario::from_ron(ARMED).unwrap().control_json("/work/t.jsonl")).unwrap();
        assert_eq!(
            v["weapons"][0],
            serde_json::json!({ "worm": 1, "names": ["Minigun", "Big Nuke", "Uzi", "Laser", "Zimm"], "current": 1 })
        );
        assert_eq!(v["health"][0], serde_json::json!([0, 14.5]));
        assert_eq!(v["bonuses"][0], serde_json::json!([3, 200, 300, "health", ""]));
        assert_eq!(v["bonuses"][1], serde_json::json!([4, 210, 300, "weapon", "Big Nuke"]));
    }

    #[test]
    fn rejects_invalid_weapons_health_and_bonuses() {
        let cases = [
            ("(worm: 1, names", "(worm: 2, names", "weapons for worm 2"),
            ("current: 1", "current: 5", "current weapon 5"),
            (
                "\"Uzi\", \"Laser\", \"Zimm\"], current",
                "\"Uzi\", \"Laser\"], current",
                "exactly 5 weapon names",
            ),
            ("\"Zimm\"], current", "\"Zimm\", \"Uzi\"], current", "exactly 5 weapon names"),
            ("\"Big Nuke\", \"Uzi\"", "\"Big,Nuke\", \"Uzi\"", "weapon name"),
            ("\"Big Nuke\", \"Uzi\"", "\"Big\\\"Nuke\", \"Uzi\"", "weapon name"),
            ("health: [(0, 14.5)]", "health: [(5, 14.5)]", "health for worm 5"),
            ("health: [(0, 14.5)]", "health: [(0, -1.0)]", "health -1"),
            ("health: [(0, 14.5)]", "health: [(0, 0.0)]", "health 0"),
            ("x: 200, y: 300", "x: 32768, y: 300", "bonus position (32768, 300)"),
            ("x: 200, y: 300", "x: 200, y: -32769", "bonus position (200, -32769)"),
            ("weapon: Some(\"Big Nuke\")", "weapon: Some(\"Big\\\"Nuke\")", "bad bonus weapon name"),
            ("frame: 3,", "frame: 10,", "bonus at frame 10"),
            ("kind: \"health\")", "kind: \"health\", weapon: Some(\"Minigun\"))", "health bonus"),
            ("kind: \"weapon\", weapon: Some(\"Big Nuke\")", "kind: \"weapon\"", "weapon bonus"),
            ("kind: \"health\"", "kind: \"ammo\"", "unknown bonus kind"),
        ];
        for (from, to, msg) in cases {
            assert!(ARMED.contains(from), "{from}");
            let err = Scenario::from_ron(&ARMED.replace(from, to)).unwrap_err().to_string();
            assert!(err.contains(msg), "{to}: {err}");
        }
    }

    #[test]
    fn accepts_the_edges_of_the_valid_ranges() {
        for (from, to) in [
            ("x: 200, y: 300", "x: 32767, y: -32768"),
            ("health: [(0, 14.5)]", "health: [(0, 0.001)]"),
            ("\"Big Nuke\", \"Uzi\"", "\"Assassin's Shot\", \"Uzi\""),
        ] {
            assert!(ARMED.contains(from), "{from}");
            Scenario::from_ron(&ARMED.replace(from, to)).unwrap_or_else(|e| panic!("{to}: {e}"));
        }
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
