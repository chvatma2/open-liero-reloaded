//! Mod definitions: worm and rope physics, weapons, projectiles.
//!
//! Mirrors OLX's classic gamescript model (`include/WeaponDesc.h`,
//! `include/ProjectileDesc.h`, `include/ProjAction.h`) with the LX56 (v7)
//! semantics made explicit. Values are stored exactly as OLX holds them in
//! memory after loading, e.g. `rof` is already divided by 1000
//! (`src/common/CGameScript.cpp:776-777`).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::image::Rgba;

/// A complete mod. `weapons` keeps the original order (it defines weapon ids);
/// projectiles are shared definitions referenced by name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModDef {
    pub name: String,
    pub worm: WormParams,
    pub rope: RopeParams,
    pub weapons: Vec<WeaponDef>,
    pub projectiles: BTreeMap<String, ProjectileDef>,
}

/// OLX `gs_worm_t` (`src/common/CGameScript.cpp:45-55`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WormParams {
    /// Read from the file but ignored by OLX.
    pub angle_speed: f32,
    pub ground_speed: f32,
    pub air_speed: f32,
    pub gravity: f32,
    pub jump_force: f32,
    pub air_friction: f32,
    /// Read from the file but ignored by OLX.
    pub ground_friction: f32,
}

/// Ninja rope settings stored in the mod (`CGameScript.cpp:788-800`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RopeParams {
    pub max_length: i32,
    pub rest_length: i32,
    pub strength: f32,
}

/// OLX `weapon_t`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponDef {
    /// Stable identifier; it is the file name in a pack, so it is not serialized.
    #[serde(skip)]
    pub id: String,
    pub name: String,
    /// Already divided by 10, as OLX does after loading.
    pub recharge: f32,
    pub drain: f32,
    /// Already divided by 1000 (seconds between shots).
    pub rof: f32,
    pub laser_sight: bool,
    pub kind: WeaponKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WeaponKind {
    /// `WPN_PROJECTILE`.
    Projectile { class: WeaponClass, recoil: i32, sound: Option<String>, spawn: SpawnDef },
    /// `WPN_BEAM`.
    Beam { recoil: i32, beam: BeamDef },
    /// `WPN_SPECIAL`.
    Special { special: SpecialKind, thrust: u32 },
}

/// OLX `Wpn_Class`; only used by bot AI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponClass {
    Automatic,
    PowerGun,
    Grenade,
    Missile,
    CloseRange,
    Other(i32),
}

impl WeaponClass {
    pub fn from_lx(v: i32) -> Self {
        match v {
            0 => Self::Automatic,
            1 => Self::PowerGun,
            3 => Self::Grenade,
            4 => Self::Missile,
            5 => Self::CloseRange,
            v => Self::Other(v),
        }
    }
}

/// OLX `Wpn_Special`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecialKind {
    None,
    Jetpack,
    Other(i32),
}

impl SpecialKind {
    pub fn from_lx(v: i32) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Jetpack,
            v => Self::Other(v),
        }
    }
}

/// OLX `Wpn_Beam`. LX56 files carry only colour, damage, player damage and
/// length; the other fields keep OLX's defaults.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BeamDef {
    pub colour: Rgba,
    pub damage: i32,
    pub player_damage: i32,
    pub length: i32,
    pub init_width: i32,
    pub width_increase: f32,
    pub distribute_damage_over_width: bool,
}

/// OLX `Proj_SpawnInfo`: how child projectiles are spawned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpawnDef {
    /// Name of the spawned projectile in [`ModDef::projectiles`].
    pub projectile: String,
    pub amount: i32,
    pub speed: i32,
    pub speed_var: f32,
    pub spread: f32,
    pub use_angle: bool,
    pub angle: i32,
    pub use_parent_velocity_for_spread: bool,
    pub parent_vel_spread_factor: f32,
    pub use_special11_vec_for_speed_var: bool,
    pub use_random_rot: bool,
    pub add_parent_vel: bool,
    pub parent_vel_factor: f32,
}

impl SpawnDef {
    /// OLX's `Proj_SpawnInfo()` constructor defaults, spawning `projectile`.
    pub fn olx_default(projectile: String) -> Self {
        Self {
            projectile,
            amount: 0,
            speed: 0,
            speed_var: 0.0,
            spread: 0.0,
            use_angle: false,
            angle: 0,
            use_parent_velocity_for_spread: false,
            parent_vel_spread_factor: 0.3,
            use_special11_vec_for_speed_var: false,
            use_random_rot: false,
            add_parent_vel: false,
            parent_vel_factor: 1.0,
        }
    }
}

/// OLX `proj_t`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectileDef {
    pub gfx: ProjectileGfx,
    /// `Some` when `UseCustomGravity` is set.
    pub gravity: Option<i32>,
    pub dampening: f32,
    pub width: i32,
    pub height: i32,
    pub timer: TimerDef,
    pub trail: TrailDef,
    /// Terrain hit.
    pub hit: ActionDef,
    /// Worm hit.
    pub player_hit: ActionDef,
    /// `GeneralSpawnInfo`: what `spawns_projectiles` actions spawn.
    pub event_spawn: Option<SpawnDef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProjectileGfx {
    /// `PRJ_PIXEL`: one of `colours` is picked per projectile.
    Pixel { colours: Vec<Rgba> },
    /// `PRJ_IMAGE`. `file` is a path inside the mod pack, such as `gfx/missile.png`.
    Image {
        file: String,
        rotating: bool,
        rot_increment: i32,
        rot_speed: i32,
        use_angle: bool,
        use_spec_angle: bool,
        angle_images: i32,
        animation: Option<Animation>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub rate: f32,
    pub kind: AnimKind,
}

/// OLX `Proj_AnimType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimKind {
    Once,
    Loop,
    PingPong,
    Other(i32),
}

impl AnimKind {
    pub fn from_lx(v: i32) -> Self {
        match v {
            0 => Self::Once,
            1 => Self::Loop,
            2 => Self::PingPong,
            v => Self::Other(v),
        }
    }
}

/// OLX `Proj_LX56Timer`. `action` is `Some` exactly when `time > 0`,
/// because only then does the file contain the timer action.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimerDef {
    pub time: f32,
    pub time_var: f32,
    pub action: Option<ActionDef>,
}

/// OLX `Proj_Trail`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrailDef {
    pub kind: TrailKind,
    /// Seconds (OLX divides the stored milliseconds by 1000).
    pub delay: f32,
    /// `Some` exactly when `kind` is [`TrailKind::Projectile`].
    pub spawn: Option<SpawnDef>,
}

/// OLX `Proj_TrailType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrailKind {
    None,
    Smoke,
    ChemSmoke,
    Projectile,
    Doomsday,
    Explosive,
    Other(i32),
}

impl TrailKind {
    pub fn from_lx(v: i32) -> Self {
        match v {
            0 => Self::None,
            1 => Self::Smoke,
            2 => Self::ChemSmoke,
            3 => Self::Projectile,
            4 => Self::Doomsday,
            5 => Self::Explosive,
            v => Self::Other(v),
        }
    }
}

/// The LX56 part of OLX `Proj_Action`. Fields a file does not contain keep the
/// constructor defaults from `include/ProjAction.h:211-215`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionDef {
    pub kind: ActionKind,
    pub damage: i32,
    pub spawns_projectiles: bool,
    /// Path inside the mod pack, such as `sfx/bazooka.ogg`.
    pub sound: Option<String>,
    pub shake: i32,
    pub bounce_coeff: f32,
    pub bounce_explode: i32,
}

impl ActionDef {
    /// OLX's `Proj_Action()` defaults with the given kind.
    pub fn olx_default(kind: ActionKind) -> Self {
        Self {
            kind,
            damage: 0,
            spawns_projectiles: false,
            sound: None,
            shake: 0,
            bounce_coeff: 0.5,
            bounce_explode: 0,
        }
    }
}

/// OLX `Proj_ActionType` (`include/ProjAction.h:147-175`). Value 3 is unused by OLX.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    Bounce,
    Explode,
    Injure,
    Carve,
    Dirt,
    GreenDirt,
    Disappear,
    Nothing,
    Disappear2,
    GoThrough,
    InjureProj,
    PlaySound,
    InjureWorm,
    ChangeRadius,
    OverwriteOwnSpeed,
    MultiplyOwnSpeed,
    DiffOwnSpeed,
    OverwriteTargetSpeed,
    MultiplyTargetSpeed,
    DiffTargetSpeed,
    HeadingToNextWorm,
    HeadingToOwner,
    HeadingToNextOtherWorm,
    HeadingToNextEnemyWorm,
    HeadingToNextTeamMate,
    HeadTargetToUs,
    Other(i32),
}

impl ActionKind {
    pub fn from_lx(v: i32) -> Self {
        use ActionKind::*;
        match v {
            0 => Bounce,
            1 => Explode,
            2 => Injure,
            4 => Carve,
            5 => Dirt,
            6 => GreenDirt,
            7 => Disappear,
            8 => Nothing,
            9 => Disappear2,
            10 => GoThrough,
            11 => InjureProj,
            12 => PlaySound,
            13 => InjureWorm,
            14 => ChangeRadius,
            15 => OverwriteOwnSpeed,
            16 => MultiplyOwnSpeed,
            17 => DiffOwnSpeed,
            18 => OverwriteTargetSpeed,
            19 => MultiplyTargetSpeed,
            20 => DiffTargetSpeed,
            21 => HeadingToNextWorm,
            22 => HeadingToOwner,
            23 => HeadingToNextOtherWorm,
            24 => HeadingToNextEnemyWorm,
            25 => HeadingToNextTeamMate,
            26 => HeadTargetToUs,
            v => Other(v),
        }
    }
}

impl ProjectileDef {
    /// Names of the projectiles this one can spawn (event spawn, then trail).
    pub fn children(&self) -> impl Iterator<Item = &str> {
        self.event_spawn.iter().chain(self.trail.spawn.iter()).map(|s| s.projectile.as_str())
    }

    /// Paths of pack assets this projectile references (image, sounds).
    pub fn assets(&self) -> impl Iterator<Item = &str> {
        let image = match &self.gfx {
            ProjectileGfx::Image { file, .. } => Some(file.as_str()),
            ProjectileGfx::Pixel { .. } => None,
        };
        let timer_sound = self.timer.action.as_ref().and_then(|a| a.sound.as_deref());
        image
            .into_iter()
            .chain(self.hit.sound.as_deref())
            .chain(timer_sound)
            .chain(self.player_hit.sound.as_deref())
    }
}

impl WeaponDef {
    /// Name of the projectile fired by this weapon, if it is a projectile weapon.
    pub fn projectile(&self) -> Option<&str> {
        match &self.kind {
            WeaponKind::Projectile { spawn, .. } => Some(spawn.projectile.as_str()),
            _ => None,
        }
    }

    /// Path of the firing sound inside the pack, if any.
    pub fn sound(&self) -> Option<&str> {
        match &self.kind {
            WeaponKind::Projectile { sound, .. } => sound.as_deref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    pub(crate) fn sample_projectile() -> ProjectileDef {
        ProjectileDef {
            gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(255, 255, 0)] },
            gravity: Some(50),
            dampening: 1.0,
            width: 2,
            height: 2,
            timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
            trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
            hit: ActionDef { damage: 5, ..ActionDef::olx_default(ActionKind::Explode) },
            player_hit: ActionDef::olx_default(ActionKind::Injure),
            event_spawn: Some(SpawnDef::olx_default("bits".to_string())),
        }
    }

    #[test]
    fn lx_codes_map_to_variants() {
        assert_eq!(ActionKind::from_lx(4), ActionKind::Carve);
        assert_eq!(ActionKind::from_lx(3), ActionKind::Other(3));
        assert_eq!(TrailKind::from_lx(3), TrailKind::Projectile);
        assert_eq!(WeaponClass::from_lx(2), WeaponClass::Other(2));
        assert_eq!(AnimKind::from_lx(2), AnimKind::PingPong);
        assert_eq!(SpecialKind::from_lx(1), SpecialKind::Jetpack);
    }

    #[test]
    fn projectile_ron_roundtrip() {
        let p = sample_projectile();
        let s = ron::ser::to_string_pretty(&p, ron::ser::PrettyConfig::default()).unwrap();
        let back: ProjectileDef = ron::from_str(&s).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn weapon_id_is_not_serialized() {
        let w = WeaponDef {
            id: "bazooka".to_string(),
            name: "Bazooka".to_string(),
            recharge: 1.0,
            drain: 2.0,
            rof: 0.5,
            laser_sight: false,
            kind: WeaponKind::Special { special: SpecialKind::Jetpack, thrust: 3 },
        };
        let s = ron::to_string(&w).unwrap();
        assert!(!s.contains("bazooka"), "id leaked into RON: {s}");
        let back: WeaponDef = ron::from_str(&s).unwrap();
        assert_eq!(back.id, "");
        assert_eq!(back.name, "Bazooka");
    }

    #[test]
    fn children_and_assets() {
        let mut p = sample_projectile();
        p.hit.sound = Some("sfx/boom.ogg".to_string());
        assert_eq!(p.children().collect::<Vec<_>>(), vec!["bits"]);
        assert_eq!(p.assets().collect::<Vec<_>>(), vec!["sfx/boom.ogg"]);
    }

    #[test]
    fn non_finite_floats_survive_ron() {
        for v in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let back: f32 = ron::from_str(&ron::to_string(&v).unwrap()).unwrap();
            assert_eq!(back.is_nan(), v.is_nan());
            if !v.is_nan() {
                assert_eq!(back, v);
            }
        }
    }

    proptest::proptest! {
        /// Every f32, including subnormals, must survive a RON round trip bit-exactly,
        /// because physics parameters have to match OLX exactly.
        #[test]
        fn f32_survives_ron_bit_exactly(bits in proptest::num::u32::ANY) {
            let v = f32::from_bits(bits);
            proptest::prop_assume!(v.is_finite());
            let s = ron::to_string(&v).unwrap();
            let back: f32 = ron::from_str(&s).unwrap();
            proptest::prop_assert_eq!(back.to_bits(), v.to_bits());
        }
    }
}
