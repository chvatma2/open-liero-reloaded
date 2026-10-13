//! The gameplay settings the simulation reads (OLX `FeatureList`).

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;

use rl_math::TimeDiff;

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
    /// `WeaponCombos`: the weapon can be changed while firing.
    pub weapon_combos: bool,
    /// `InstantAirJump`: worms can jump in mid-air (`CWorm::Prepare`, `src/common/CWorm.cpp:323`).
    pub instant_air_jump: bool,
    /// `LoadingTime`, in percent: weapons reload in `100 / LoadingTime` times their time.
    pub loading_time: i32,
    /// `ProjRelativeVel`: shots carry the worm's velocity.
    pub proj_relative_vel: bool,
    /// `ShootSpawnDistance`: projectiles spawn this far from the worm along the aim.
    pub shoot_spawn_distance: f32,
    /// `LX56WallShooting`: new projectiles ignore collisions for 11-12 ms.
    pub lx56_wall_shooting: bool,
    /// `DisableWpnsWhenEmpty`: an empty weapon is removed instead of reloading.
    pub disable_wpns_when_empty: bool,
    /// `ProjGravityFactor`: multiplies every projectile's gravity.
    pub proj_gravity_factor: f32,
    /// `SelfHit`: a worm's own projectiles can hit it (after their first 100 ms).
    pub self_hit: bool,
    /// `SelfInjure`: a worm's own damage hurts it (`CClient::InjureWorm`).
    pub self_injure: bool,
    /// `WormDamageFactor`: each worm's initial `fDamageFactor` (`CWorm::Prepare`).
    pub worm_damage_factor: f32,
    /// `WormShieldFactor`: each worm's initial `fShieldFactor`.
    pub worm_shield_factor: f32,
    /// `Lives`: below 0 unlimited (`WRM_UNLIM`).
    pub lives: i32,
    /// `KillLimit`. Read for `CGameMode::CheckGameOver`, which is not ported yet
    /// (`docs/porting/STATUS.md`).
    pub kill_limit: i32,
    /// `RespawnTime`, in seconds.
    pub respawn_time: f32,
    /// `MaxRespawnTime`, in seconds: from 0 on, the server spawns a dead worm
    /// itself once both times have passed; below 0 the worm waits for its jump key.
    pub max_respawn_time: f32,
    /// `EmptyWeaponsOnRespawn`: a respawned worm's weapons start empty and reloading.
    pub empty_weapons_on_respawn: bool,
    /// `AllowNegativeScore`: deaths and suicides count even at a score of 0 or less.
    pub allow_negative_score: bool,
    /// `DeathDecreasesScore`: each death takes this much off the score.
    pub death_decreases_score: f32,
    /// `SuicideDecreasesScore`: each suicide takes one off the score.
    pub suicide_decreases_score: bool,
    /// `Bonuses`: bonuses fall, are picked up and explode, and the server spawns them.
    pub bonuses: bool,
    /// `BonusFrequency`, in seconds: the server spawns a bonus when this much has passed (strictly).
    pub bonus_frequency: f32,
    /// `BonusLife`, in seconds: how long the server keeps a bonus's slot.
    pub bonus_life: f32,
    /// `BonusHealthToWeaponChance`: the chance that a spawned bonus is a weapon.
    pub bonus_health_to_weapon_chance: f32,
    /// OLX `d1ad953f`'s hit and death bugs, which play fixes (spec §18 D7
    /// and D9, amended 2026-10-13; `docs/porting/STATUS.md`). On, as in OLX:
    /// - projectile hit tests and explosions read the worm's position
    ///   recorder through `VecTimeRecorder::get`'s off-by-one, so
    ///   frames-back 0 is the *oldest* entry (about 1.1 s old, or an
    ///   earlier life's), and a respawn keeps the old entries;
    /// - a worm left at exactly 0 health dies at the start of the next
    ///   frame credited to nobody, with no death sound, gibs or blood.
    ///
    /// Off (play): hits test the newest recorded position, a respawn
    /// forgets the old life's, and the exact-zero death is credited to the
    /// worm whose damage left it at 0 and plays the death effects.
    /// [`SimSettings::from_olx_features`] (an oracle trace's header: the
    /// oracle, scenarios and what is drawn from them) turns it on;
    /// [`SimSettings::lx56`] and every game leave it off.
    pub olx_quirks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsError(pub String);

impl SimSettings {
    /// OLX's "LieroX 0.56" values with the Classic mod (as in the oracle
    /// trace headers), without OLX's hit and death bugs
    /// ([`SimSettings::olx_quirks`] off, as every game plays).
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
            weapon_combos: true,
            instant_air_jump: false,
            loading_time: 100,
            proj_relative_vel: true,
            shoot_spawn_distance: 8.0,
            lx56_wall_shooting: true,
            disable_wpns_when_empty: false,
            proj_gravity_factor: 1.0,
            self_hit: true,
            self_injure: true,
            worm_damage_factor: 1.0,
            worm_shield_factor: 1.0,
            lives: -2,
            kill_limit: 15,
            respawn_time: 2.5,
            max_respawn_time: 0.0,
            empty_weapons_on_respawn: false,
            allow_negative_score: false,
            death_decreases_score: 0.0,
            suicide_decreases_score: false,
            bonuses: false,
            bonus_frequency: 30.0,
            bonus_life: 60.0,
            bonus_health_to_weapon_chance: 0.5,
            olx_quirks: false,
        }
    }

    /// `LX56PhysicsDT`: `1000 / LX56PhysicsFPS` whole milliseconds, at least
    /// 1 ms, so a frame always ends even with settings outside OLX's range
    /// ([`SimSettings::validate`]).
    pub fn lx56_physics_dt(&self) -> TimeDiff {
        TimeDiff((1000 / self.lx56_physics_fps.max(1)).max(1) as u64)
    }

    /// Checks every number against OLX's own range for it (`src/common/FeatureList.cpp`).
    /// Outside them the simulation could overflow, hang or recurse without bound.
    /// NaN and infinities are outside every range.
    pub fn validate(&self) -> Result<(), SettingsError> {
        let ranges = [
            ("LX56PhysicsFPS", self.lx56_physics_fps as f32, 1.0, 200.0),
            ("GameSpeed", self.game_speed, 0.1, 10.0),
            ("WormGroundSpeed", self.worm_ground_speed, 0.0, 100.0),
            ("WormAirSpeed", self.worm_air_speed, 0.0, 30.0),
            ("WormMaxGroundMoveSpeed", self.worm_max_ground_move_speed, 0.1, 200.0),
            ("WormMaxAirMoveSpeed", self.worm_max_air_move_speed, 0.1, 200.0),
            ("WormJumpForce", self.worm_jump_force, -500.0, 100.0),
            ("WormGravity", self.worm_gravity, -200.0, 1000.0),
            ("WormAirFriction", self.worm_air_friction, 0.0, 10.0),
            ("WormGroundFriction", self.worm_ground_friction, 0.0, 1.0),
            ("WormGroundStopSpeed", self.worm_ground_stop_speed, 0.0, 10.0),
            ("WormSpeedFactor", self.worm_speed_factor, -2.0, 10.0),
            ("RelativeAirJumpDelay", self.relative_air_jump_delay, 0.0, 5.0),
            ("RopeMaxLength", self.rope_max_length as f32, 0.0, 2000.0),
            ("RopeRestLength", self.rope_rest_length as f32, 0.0, 200.0),
            ("RopeStrength", self.rope_strength, 0.0, 10.0),
            ("RopeSpeed", self.rope_speed, 0.0, 1000.0),
            ("RopeGravity", self.rope_gravity, 0.0, 200.0),
            ("RopeFallingGravity", self.rope_falling_gravity, 0.0, 200.0),
            ("NinjaropePrecision", self.ninjarope_precision, 0.0, 10.0),
            ("LoadingTime", self.loading_time as f32, 0.0, 500.0),
            ("ShootSpawnDistance", self.shoot_spawn_distance, 0.0, 20.0),
            ("ProjGravityFactor", self.proj_gravity_factor, -10.0, 10.0),
            ("WormDamageFactor", self.worm_damage_factor, -2.0, 10.0),
            ("WormShieldFactor", self.worm_shield_factor, -2.0, 10.0),
            ("Lives", self.lives as f32, -2.0, 150.0),
            ("KillLimit", self.kill_limit as f32, -1.0, 150.0),
            ("RespawnTime", self.respawn_time, 0.0, 20.0),
            ("MaxRespawnTime", self.max_respawn_time, -1.0, 20.0),
            ("DeathDecreasesScore", self.death_decreases_score, 0.0, 5.0),
            ("BonusFrequency", self.bonus_frequency, 1.0, 150.0),
            ("BonusLife", self.bonus_life, 1.0, 150.0),
            ("BonusHealthToWeaponChance", self.bonus_health_to_weapon_chance, 0.0, 1.0),
        ];
        for (k, v, min, max) in ranges {
            if !(min..=max).contains(&v) {
                return Err(SettingsError(format!("feature {k}={v} is outside OLX's range {min} to {max}")));
            }
        }
        Ok(())
    }

    /// Reads the settings from OLX feature values (a trace header's
    /// `features`), with [`SimSettings::olx_quirks`] on: a trace is OLX's play.
    /// Fails if a needed feature is missing or malformed, or if OLX had a mode
    /// on that this simulation does not implement (wrap-around maps, Gusanos
    /// worm physics, worm or projectile friction, non-LX56 aim), or a value is outside
    /// OLX's range ([`SimSettings::validate`]).
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
        for k in ["WormFriction", "ProjFriction"] {
            if num(k)? != 0.0 {
                return Err(SettingsError(format!("feature {k} > 0 is not supported")));
            }
        }
        let settings = Self {
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
            weapon_combos: flag("WeaponCombos")?,
            instant_air_jump: flag("InstantAirJump")?,
            loading_time: int("LoadingTime")?,
            proj_relative_vel: flag("ProjRelativeVel")?,
            shoot_spawn_distance: num("ShootSpawnDistance")?,
            lx56_wall_shooting: flag("LX56WallShooting")?,
            disable_wpns_when_empty: flag("DisableWpnsWhenEmpty")?,
            proj_gravity_factor: num("ProjGravityFactor")?,
            self_hit: flag("SelfHit")?,
            self_injure: flag("SelfInjure")?,
            worm_damage_factor: num("WormDamageFactor")?,
            worm_shield_factor: num("WormShieldFactor")?,
            lives: int("Lives")?,
            kill_limit: int("KillLimit")?,
            respawn_time: num("RespawnTime")?,
            max_respawn_time: num("MaxRespawnTime")?,
            empty_weapons_on_respawn: flag("EmptyWeaponsOnRespawn")?,
            allow_negative_score: flag("AllowNegativeScore")?,
            death_decreases_score: num("DeathDecreasesScore")?,
            suicide_decreases_score: flag("SuicideDecreasesScore")?,
            bonuses: flag("Bonuses")?,
            bonus_frequency: num("BonusFrequency")?,
            bonus_life: num("BonusLife")?,
            bonus_health_to_weapon_chance: num("BonusHealthToWeaponChance")?,
            // A trace is OLX's play, bugs included.
            olx_quirks: true,
        };
        settings.validate()?;
        Ok(settings)
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
            ("WeaponCombos", "true"),
            ("InstantAirJump", "false"),
            ("LoadingTime", "100"),
            ("ProjRelativeVel", "true"),
            ("ShootSpawnDistance", "8"),
            ("LX56WallShooting", "true"),
            ("DisableWpnsWhenEmpty", "false"),
            ("ProjFriction", "0"),
            ("ProjGravityFactor", "1"),
            ("SelfHit", "true"),
            ("SelfInjure", "true"),
            ("WormDamageFactor", "1"),
            ("WormShieldFactor", "1"),
            ("Lives", "-2"),
            ("KillLimit", "15"),
            ("RespawnTime", "2.5"),
            ("MaxRespawnTime", "0"),
            ("EmptyWeaponsOnRespawn", "false"),
            ("AllowNegativeScore", "false"),
            ("DeathDecreasesScore", "0"),
            ("SuicideDecreasesScore", "false"),
            ("Bonuses", "false"),
            ("BonusFrequency", "30"),
            ("BonusLife", "60"),
            ("BonusHealthToWeaponChance", "0.5"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn olx_header_gives_the_lx56_defaults_with_olx_quirks() {
        let olx = SimSettings { olx_quirks: true, ..SimSettings::lx56() };
        assert_eq!(SimSettings::from_olx_features(&olx_header()), Ok(olx));
        assert!(!SimSettings::lx56().olx_quirks, "play fixes OLX's hit and death bugs");
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

    #[test]
    fn values_outside_olx_ranges_are_errors() {
        // OLX's own limits (src/common/FeatureList.cpp). Outside them the port
        // could overflow, hang (a 0 ms physics step) or recurse without bound.
        for (k, v) in [
            ("LX56PhysicsFPS", "1001"),
            ("LX56PhysicsFPS", "0"),
            ("RelativeAirJumpDelay", "1e30"),
            ("RelativeAirJumpDelay", "-1"),
            ("NinjaropePrecision", "inf"),
            ("RopeSpeed", "NaN"),
            ("GameSpeed", "0"),
            ("RespawnTime", "-1"),
            ("Lives", "151"),
            ("WormShieldFactor", "NaN"),
            ("BonusFrequency", "0.5"),
            ("BonusLife", "151"),
            ("BonusHealthToWeaponChance", "1.5"),
        ] {
            let mut f = olx_header();
            f.insert(k.into(), v.into());
            let err = SimSettings::from_olx_features(&f).unwrap_err().0;
            assert!(err.contains(k) && err.contains("outside"), "{k}={v}: {err}");
        }
    }
}
