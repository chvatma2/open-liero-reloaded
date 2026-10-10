//! The gameplay settings the simulation reads (OLX `FeatureList`).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;

/// Typed gameplay settings. [`SimSettings::lx56`] is OLX's "LieroX 0.56"
/// preset with the Classic mod, which is what the oracle traces use.
#[derive(Clone, Debug, PartialEq)]
pub struct SimSettings {
    /// `LX56PhysicsFPS`: the physics step is `1000 / fps` whole milliseconds.
    pub lx56_physics_fps: i32,
    pub game_speed: f32,
    pub game_speed_only_for_projs: bool,
    pub worm_ground_speed: f32,
    pub worm_air_speed: f32,
    pub worm_max_ground_move_speed: f32,
    pub worm_max_air_move_speed: f32,
    pub worm_jump_force: f32,
    pub worm_gravity: f32,
    pub worm_air_friction: f32,
    pub worm_ground_friction: f32,
    pub worm_ground_stop_speed: f32,
    pub worm_speed_factor: f32,
    pub relative_air_jump: bool,
    pub relative_air_jump_delay: f32,
    pub jump_to_aim_dir: bool,
    pub full_aim_angle: bool,
    pub rope_max_length: i32,
    pub rope_rest_length: i32,
    pub rope_strength: f32,
    pub rope_speed: f32,
    pub rope_gravity: f32,
    pub rope_falling_gravity: f32,
    pub rope_add_parent_speed: bool,
    pub rope_can_attach_worm: bool,
    pub ninjarope_precision: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsError(pub String);

impl SimSettings {
    /// OLX's "LieroX 0.56" values with the Classic mod (as in the oracle trace headers).
    pub fn lx56() -> Self {
        Self {
            lx56_physics_fps: 84,
            game_speed: 1.0,
            game_speed_only_for_projs: false,
            worm_ground_speed: 8.0,
            worm_air_speed: 4.0,
            worm_max_ground_move_speed: 30.0,
            worm_max_air_move_speed: 30.0,
            worm_jump_force: -65.0,
            worm_gravity: 100.0,
            worm_air_friction: 0.0,
            worm_ground_friction: 0.1,
            worm_ground_stop_speed: 5.0,
            worm_speed_factor: 1.0,
            relative_air_jump: false,
            relative_air_jump_delay: 0.7,
            jump_to_aim_dir: false,
            full_aim_angle: false,
            rope_max_length: 300,
            rope_rest_length: 27,
            rope_strength: 4.5,
            rope_speed: 250.0,
            rope_gravity: 100.0,
            rope_falling_gravity: 150.0,
            rope_add_parent_speed: false,
            rope_can_attach_worm: true,
            ninjarope_precision: 1.0,
        }
    }

    /// Reads the settings from OLX feature values (a trace header's `features`).
    /// Fails if a needed feature is missing or malformed, or if OLX had a mode
    /// on that this simulation does not implement (wrap-around maps, Gusanos
    /// worm physics, worm friction, non-LX56 aim).
    pub fn from_olx_features(f: &BTreeMap<String, String>) -> Result<Self, SettingsError> {
        let get = |k: &str| f.get(k).ok_or_else(|| SettingsError(format!("feature {k} missing")));
        let num = |k: &str| -> Result<f32, SettingsError> {
            get(k)?.parse::<f32>().map_err(|_| SettingsError(format!("feature {k} is not a number")))
        };
        let int = |k: &str| -> Result<i32, SettingsError> {
            get(k)?.parse::<i32>().map_err(|_| SettingsError(format!("feature {k} is not an integer")))
        };
        let flag = |k: &str| -> Result<bool, SettingsError> {
            match get(k)?.as_str() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(SettingsError(format!("feature {k} is not a boolean"))),
            }
        };
        for (k, want) in [("InfiniteMap", false), ("GusanosWormPhysics", false), ("ForceLX56Aim", true)] {
            if flag(k)? != want {
                return Err(SettingsError(format!("feature {k}={} is not supported", !want)));
            }
        }
        if num("WormFriction")? != 0.0 {
            return Err(SettingsError("feature WormFriction > 0 is not supported".into()));
        }
        Ok(Self {
            lx56_physics_fps: int("LX56PhysicsFPS")?,
            game_speed: num("GameSpeed")?,
            game_speed_only_for_projs: flag("GameSpeedOnlyForProjs")?,
            worm_ground_speed: num("WormGroundSpeed")?,
            worm_air_speed: num("WormAirSpeed")?,
            worm_max_ground_move_speed: num("WormMaxGroundMoveSpeed")?,
            worm_max_air_move_speed: num("WormMaxAirMoveSpeed")?,
            worm_jump_force: num("WormJumpForce")?,
            worm_gravity: num("WormGravity")?,
            worm_air_friction: num("WormAirFriction")?,
            worm_ground_friction: num("WormGroundFriction")?,
            worm_ground_stop_speed: num("WormGroundStopSpeed")?,
            worm_speed_factor: num("WormSpeedFactor")?,
            relative_air_jump: flag("RelativeAirJump")?,
            relative_air_jump_delay: num("RelativeAirJumpDelay")?,
            jump_to_aim_dir: flag("JumpToAimDir")?,
            full_aim_angle: flag("FullAimAngle")?,
            rope_max_length: int("RopeMaxLength")?,
            rope_rest_length: int("RopeRestLength")?,
            rope_strength: num("RopeStrength")?,
            rope_speed: num("RopeSpeed")?,
            rope_gravity: num("RopeGravity")?,
            rope_falling_gravity: num("RopeFallingGravity")?,
            rope_add_parent_speed: flag("RopeAddParentSpeed")?,
            rope_can_attach_worm: flag("RopeCanAttachWorm")?,
            ninjarope_precision: num("NinjaropePrecision")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    /// The relevant features of the oracle trace headers (`fixtures/oracle/*.jsonl.gz`).
    fn olx_header() -> BTreeMap<String, String> {
        [
            ("InfiniteMap", "false"),
            ("GusanosWormPhysics", "false"),
            ("ForceLX56Aim", "true"),
            ("WormFriction", "0"),
            ("LX56PhysicsFPS", "84"),
            ("GameSpeed", "1"),
            ("GameSpeedOnlyForProjs", "false"),
            ("WormGroundSpeed", "8"),
            ("WormAirSpeed", "4"),
            ("WormMaxGroundMoveSpeed", "30"),
            ("WormMaxAirMoveSpeed", "30"),
            ("WormJumpForce", "-65"),
            ("WormGravity", "100"),
            ("WormAirFriction", "0"),
            ("WormGroundFriction", "0.1"),
            ("WormGroundStopSpeed", "5"),
            ("WormSpeedFactor", "1"),
            ("RelativeAirJump", "false"),
            ("RelativeAirJumpDelay", "0.7"),
            ("JumpToAimDir", "false"),
            ("FullAimAngle", "false"),
            ("RopeMaxLength", "300"),
            ("RopeRestLength", "27"),
            ("RopeStrength", "4.5"),
            ("RopeSpeed", "250"),
            ("RopeGravity", "100"),
            ("RopeFallingGravity", "150"),
            ("RopeAddParentSpeed", "false"),
            ("RopeCanAttachWorm", "true"),
            ("NinjaropePrecision", "1"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn olx_header_gives_the_lx56_defaults() {
        assert_eq!(SimSettings::from_olx_features(&olx_header()), Ok(SimSettings::lx56()));
    }

    #[test]
    fn unsupported_or_broken_features_are_errors() {
        let mut f = olx_header();
        f.insert("InfiniteMap".into(), "true".into());
        assert!(SimSettings::from_olx_features(&f).unwrap_err().0.contains("InfiniteMap"));
        let mut f = olx_header();
        f.remove("RopeSpeed");
        assert_eq!(SimSettings::from_olx_features(&f).unwrap_err().0, "feature RopeSpeed missing");
        let mut f = olx_header();
        f.insert("WormGravity".into(), "heavy".into());
        assert!(SimSettings::from_olx_features(&f).unwrap_err().0.contains("not a number"));
    }
}
