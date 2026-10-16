//! Weapons and projectiles as authoring files (spec §2). Field docs give
//! each field's meaning, unit and OLX source
//! (`docs/porting/olx-notes/mod-fields.md`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::spawn::{EventSpawn, ShotSpawn, TrailSpawn};
use crate::Colour;
use crate::defaults as d;

/// `weapons/<id>.ron` (OLX `weapon_t`, `include/WeaponDesc.h:90-121`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weapon {
    /// Shown in menus; restrictions and bot loadouts match on it.
    pub name: String,
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Full magazines per second at LoadingTime 100 % (OLX stores the
    /// file's value ÷ 10, `CGameScript.cpp:777`; this is that value).
    pub recharge: f32,
    /// Percent of a full magazine used per shot (> 100 empties it).
    pub drain: f32,
    /// Seconds between shots (OLX's `ROF` ÷ 1000, `CGameScript.cpp:776`).
    pub between_shots_s: f32,
    /// Draws the laser sight while this weapon is selected (OLX `LaserSight`).
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub laser_sight: bool,
    /// What a shot does.
    pub fire: Fire,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// How a weapon fires (OLX `Wpn_Type`).
pub enum Fire {
    /// `WPN_PROJECTILE`.
    Projectile {
        /// Unused by OLX and the bots; kept for the record.
        #[serde(default, skip_serializing_if = "d::is_default")]
        class: WeaponClass,
        /// px/s velocity impulse against the aim, once per shot.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        recoil: i32,
        /// Pack path of the firing sound (OLX `Sound`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound: Option<String>,
        /// What the shot spawns (OLX `Proj_SpawnInfo` of the weapon).
        shot: ShotSpawn,
    },
    /// `WPN_BEAM` (`include/WeaponDesc.h:71-85`).
    Beam {
        /// Unused by the simulation (bots read its sign).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        recoil: i32,
        /// Not drawn when `#ff00ff`.
        colour: Colour,
        /// Hole carved at the first dirt or rock pixel (0..4); -1 = none.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        hole_size: i32,
        /// Health points to the first worm within 5 px of the trace.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        worm_damage: i32,
        /// px.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        length: i32,
        /// px. Default 1.
        #[serde(default = "d::one_i32", skip_serializing_if = "d::is_one_i32")]
        width: i32,
        /// px of width per px of length.
        #[serde(default, skip_serializing_if = "d::is_zero")]
        width_growth: f32,
        /// Divides `worm_damage` over the beam's width.
        #[serde(default, skip_serializing_if = "d::is_false")]
        split_damage_over_width: bool,
    },
    /// `WPN_SPECIAL`.
    Special {
        /// Which special.
        kind: SpecialKind,
        /// Jetpack: × 50 px/s² upward per 10 ms frame.
        thrust: u32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
/// OLX `Wpn_Class` (`WeaponDesc.h:25-34`); unused everywhere.
pub enum WeaponClass {
    #[default]
    Automatic,
    PowerGun,
    Grenade,
    Missile,
    CloseRange,
    Unknown(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// OLX `Wpn_Special`.
pub enum SpecialKind {
    /// OLX `SPC_NONE`: drains the charge, no thrust.
    Off,
    Jetpack,
    Unknown(i32),
}

/// A projectile (OLX `proj_t`, `include/ProjectileDesc.h:80-121`), inline in
/// a spawn or in `projectiles/<id>.ron` (shared).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projectile {
    /// The runtime id, written only when it differs from the one derived
    /// from its place (`<weapon id>`, then `.spawn` / `.trail` per level).
    /// Never set in `projectiles/` files (their id is the file name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Simulation data, not only looks: the colour count, rotation and
    /// animation affect play (`mod-fields.md` §1.2).
    pub gfx: Gfx,
    /// Collision box in px. Default (2, 2) for `Pixels`, (4, 4) for `Image`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<(i32, i32)>,
    /// px/s²; absent = the world's gravity (100). `0` and `100` are
    /// written: they are not "absent" (`mod-fields.md` §1.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gravity: Option<i32>,
    /// Velocity × this once per 11 ms physics step (> 1 accelerates).
    /// OLX's `Dampening`.
    #[serde(default = "d::one", skip_serializing_if = "d::is_one")]
    pub velocity_factor: f32,
    /// The timed event (OLX `Timer`); absent = none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer: Option<Timer>,
    /// What the projectile leaves behind while flying.
    #[serde(default, skip_serializing_if = "Trail::is_off")]
    pub trail: Trail,
    /// When the projectile hits rock or dirt. `[]` = OLX's `Nothing`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hit_terrain: Vec<Effect>,
    /// When it hits a worm. `[]` = `Nothing`, which also turns worm
    /// collision off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hit_worm: Vec<Effect>,
    /// Named spawns that `Spawn(<name>)` effects use. This runtime takes at
    /// most one, shared by all events (OLX `GeneralSpawnInfo`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub spawns: BTreeMap<String, EventSpawn>,
}

impl Projectile {
    /// A projectile with `gfx` and every other field at its default.
    pub fn new(gfx: Gfx) -> Self {
        Self {
            id: None,
            notes: String::new(),
            gfx,
            size: None,
            gravity: None,
            velocity_factor: 1.0,
            timer: None,
            trail: Trail::Off,
            hit_terrain: Vec::new(),
            hit_worm: Vec::new(),
            spawns: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// How a projectile looks (and what it costs the simulation).
pub enum Gfx {
    /// One colour is drawn at spawn (one `rand()`), as a 1 px square.
    Pixels(Vec<Colour>),
    Image {
        /// Pack path. Frames are square: frame count = width ÷ height.
        file: String,
        /// Spins at `rotation_speed` while flying (OLX `Rotating`).
        #[serde(default, skip_serializing_if = "d::is_false")]
        rotating: bool,
        /// Degrees per image frame; the random start rotation's step.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        rotation_step_deg: i32,
        /// Degrees per second while `rotating`.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        rotation_speed: i32,
        /// Render only: the frame follows the heading.
        #[serde(default, skip_serializing_if = "d::is_false")]
        frames_follow_heading: bool,
        /// Render only: left- and right-moving halves over 151°.
        #[serde(default, skip_serializing_if = "d::is_false")]
        frames_follow_heading_mirrored: bool,
        /// Render only: frames used by the two heading schemes.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        heading_frames: i32,
        /// Frame animation (OLX `Animating`, `AnimDelay`, `AnimType`); absent = static.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        animation: Option<Animation>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// OLX `Proj_AnimType` timing.
pub struct Animation {
    /// Frames per second.
    #[serde(default, skip_serializing_if = "d::is_zero")]
    pub fps: f32,
    /// At the last frame. `Once` removes the projectile.
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub mode: AnimMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
/// What an animation does at its last frame.
pub enum AnimMode {
    #[default]
    Once,
    Loop,
    PingPong,
    /// An OLX animation type this format has no name for.
    Unknown(i32),
}

/// OLX `Proj_LX56Timer`. It fires every step once its time has passed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timer {
    /// Seconds of projectile life. The effects need `after_s > 0`.
    pub after_s: f32,
    /// Seconds, ± range (× the projectile's fixed random in [-1, 1]).
    #[serde(default, skip_serializing_if = "d::is_zero")]
    pub variation_s: f32,
    /// `[]` = `Nothing`, which also cancels a spawn on a timer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
}

/// OLX `Proj_Trail`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Trail {
    #[default]
    Off,
    /// A smoke entity every 75 ms.
    Smoke,
    /// A chemical-smoke entity every 75 ms.
    ChemSmoke,
    /// A doomsday entity every 50 ms.
    Doomsday,
    /// A cosmetic explosion entity (size 10) every 50 ms.
    Explosive,
    /// A spawn every `every_s` seconds (whole ms), the first on the first step.
    Projectile {
        /// Seconds between spawns (OLX `Delay`).
        #[serde(default = "d::tenth", skip_serializing_if = "d::is_tenth")]
        every_s: f32,
        /// What each trail step spawns.
        spawn: TrailSpawn,
    },
    /// An OLX trail type this format has no name for.
    Unknown(i32),
}

impl Trail {
    /// No trail.
    pub fn is_off(&self) -> bool {
        *self == Trail::Off
    }
}

/// What an event does (OLX `Proj_ActionType`, `include/ProjAction.h:147-175`).
/// An event's list holds at most one effect besides `Spawn` in this runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    /// A blast: `damage` health points to every worm within 5 px and a hole
    /// of size `clamp(damage, 0, 4)`; -1 = no blast (still dies and spawns).
    /// `shake`: world px of camera shake.
    Explode {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        damage: i32,
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        shake: i32,
        /// Pack path of the explosion sound.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound: Option<String>,
    },
    /// Velocity × `factor` on the colliding side; `explode` > 0 blasts too.
    Bounce {
        #[serde(default = "d::half", skip_serializing_if = "d::is_half")]
        factor: f32,
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        /// Blast damage on bounce; 0 = none.
        explode: i32,
    },
    /// On a worm: `damage` health points, then the projectile dies.
    Injure {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        damage: i32,
    },
    /// A hole of `size` (0..4) at once, then dies.
    Carve {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        size: i32,
    },
    /// Three dirt balls, then dies. `shake` applies on a timer.
    Dirt {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        shake: i32,
    },
    /// Green dirt, then dies. `shake` applies on a timer.
    GreenDirt {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        shake: i32,
    },
    /// OLX's `Disappear` (7): mostly ignored in LX56, it does **not** remove
    /// the projectile. `Remove` does.
    Disappear,
    /// OLX's `Disappear2` (9): the projectile dies.
    Remove,
    /// Passes through terrain (`hit_terrain`) or worms (`hit_worm`).
    GoThrough,
    /// No effect in LX56 events (no targets).
    InjureProjectile,
    /// Plays `file` (never set by LX56 files).
    Sound {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
    /// On a worm: `damage` health points; keeps flying.
    InjureWorm {
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        damage: i32,
    },
    /// OLX `ChangeRadius`: not used by LX56 files.
    ChangeRadius,
    /// OLX `SetSpeed`: not used by LX56 files.
    SetSpeed,
    /// OLX `MultiplySpeed`: not used by LX56 files.
    MultiplySpeed,
    /// OLX `AddSpeed`: not used by LX56 files.
    AddSpeed,
    /// OLX `SetTargetSpeed`: not used by LX56 files.
    SetTargetSpeed,
    /// OLX `MultiplyTargetSpeed`: not used by LX56 files.
    MultiplyTargetSpeed,
    /// OLX `AddTargetSpeed`: not used by LX56 files.
    AddTargetSpeed,
    /// OLX `HomeOnNearestWorm`: not used by LX56 files.
    HomeOnNearestWorm,
    /// OLX `HomeOnOwner`: not used by LX56 files.
    HomeOnOwner,
    /// OLX `HomeOnNearestOtherWorm`: not used by LX56 files.
    HomeOnNearestOtherWorm,
    /// OLX `HomeOnNearestEnemy`: not used by LX56 files.
    HomeOnNearestEnemy,
    /// OLX `HomeOnNearestTeamMate`: not used by LX56 files.
    HomeOnNearestTeamMate,
    /// OLX `PullTargetsToUs`: not used by LX56 files.
    PullTargetsToUs,
    /// An OLX action number this format has no name for.
    Unknown(i32),
    /// Spawns the named entry of the projectile's `spawns`.
    Spawn(String),
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::{Rope, Worm};
    use crate::ron_io::{from_canonical, to_canonical};
    use olr_data::image::Rgba;

    fn bazooka() -> Weapon {
        let shrapnel = Projectile {
            gfx: Gfx::Pixels(vec![Colour(Rgba::rgb(0x94, 0x88, 0x00))]),
            ..Projectile::new(Gfx::Pixels(vec![]))
        };
        let mut rocket = Projectile::new(Gfx::Image {
            file: "gfx/missile.png".into(),
            rotating: false,
            rotation_step_deg: 0,
            rotation_speed: 0,
            frames_follow_heading: true,
            frames_follow_heading_mirrored: false,
            heading_frames: 18,
            animation: None,
        });
        rocket.gravity = Some(0);
        rocket.velocity_factor = 1.001;
        rocket.trail = Trail::Smoke;
        rocket.hit_terrain = vec![
            Effect::Explode { damage: 4, shake: 2, sound: Some("sfx/exp2.ogg".into()) },
            Effect::Spawn("spawn".into()),
        ];
        rocket.hit_worm =
            vec![Effect::Explode { damage: 27, shake: 0, sound: None }, Effect::Spawn("spawn".into())];
        rocket.spawns.insert(
            "spawn".into(),
            EventSpawn {
                amount: 12,
                speed: 105,
                speed_var: 14.0,
                spread: 360.0,
                projectile: Some(Box::new(shrapnel)),
                ..EventSpawn::default()
            },
        );
        Weapon {
            name: "Bazooka".into(),
            notes: String::new(),
            recharge: 0.171,
            drain: 33.334,
            between_shots_s: 1.071,
            laser_sight: false,
            fire: Fire::Projectile {
                class: WeaponClass::Automatic,
                recoil: 15,
                sound: Some("sfx/bazooka.ogg".into()),
                shot: ShotSpawn { speed: 140, projectile: Some(Box::new(rocket)), ..ShotSpawn::default() },
            },
        }
    }

    #[test]
    fn a_weapon_round_trips_through_canonical_text() {
        let w = bazooka();
        let text = to_canonical("weapons/bazooka.ron", &w).unwrap();
        assert_eq!(from_canonical::<Weapon>("weapons/bazooka.ron", &text).unwrap(), w);
        assert_eq!(
            to_canonical("weapons/bazooka.ron", &from_canonical::<Weapon>("x", &text).unwrap()).unwrap(),
            text
        );
    }

    #[test]
    fn defaults_are_left_out() {
        let text = to_canonical("weapons/bazooka.ron", &bazooka()).unwrap();
        for absent in [
            "Some(",
            "laser_sight",
            "class",
            "notes",
            "amount: 1,",
            "relative_to_parent",
            "random_rotation",
            "inherit_velocity",
            "inherit_velocity_factor",
            "parent_velocity",
            "speed_var_diagonal",
            "size",
            "timer",
            "rotating",
            "animation",
            "angle:",
            "bounce",
            "struct",
        ] {
            assert!(!text.contains(absent), "{absent:?} should be left out:\n{text}");
        }
        for present in ["\"#948800\"", "Smoke", "heading_frames: 18", "gravity: 0", "Spawn(\"spawn\")"] {
            assert!(text.contains(present), "{present:?} missing:\n{text}");
        }
    }

    #[test]
    fn spawn_defaults_depend_on_the_context() {
        let shot = to_canonical("t", &ShotSpawn::default()).unwrap();
        let event = to_canonical("t", &EventSpawn::default()).unwrap();
        let trail = to_canonical("t", &TrailSpawn::default()).unwrap();
        assert_eq!(shot, "()\n");
        assert_eq!(event, "()\n");
        assert_eq!(trail, "()\n");
        let shot_off = ShotSpawn { relative_to_parent: false, ..ShotSpawn::default() };
        assert!(to_canonical("t", &shot_off).unwrap().contains("relative_to_parent: false"));
        let event_on = EventSpawn { relative_to_parent: true, ..EventSpawn::default() };
        assert!(to_canonical("t", &event_on).unwrap().contains("relative_to_parent: true"));
        assert!(TrailSpawn::default().speed_var_diagonal);
        assert!(!EventSpawn::default().speed_var_diagonal);
        assert_eq!(ShotSpawn::default().amount, 1);
    }

    #[test]
    fn negative_zero_is_kept() {
        let mut p = Projectile::new(Gfx::Pixels(vec![]));
        p.velocity_factor = -0.0;
        let text = to_canonical("t", &p).unwrap();
        let back: Projectile = from_canonical("t", &text).unwrap();
        assert_eq!(back.velocity_factor.to_bits(), (-0.0f32).to_bits(), "{text}");
    }

    #[test]
    fn misspelt_fields_are_errors() {
        let e = from_canonical::<Projectile>("t", "(gfx: Pixels([]), hit_wrom: [])").unwrap_err();
        assert!(e.to_string().contains("hit_wrom"), "{e}");
    }

    #[test]
    fn worm_and_rope_defaults_are_olx_feature_list_values() {
        let w = Worm::default();
        assert_eq!(
            (w.ground_acceleration, w.air_acceleration, w.gravity, w.jump_speed),
            (8.0, 4.0, 50.0, -75.0)
        );
        assert!(w.is_default());
        assert!(!Worm { gravity: 100.0, ..w.clone() }.is_default());
        assert_eq!(
            (Rope::default().max_length, Rope::default().rest_length, Rope::default().strength),
            (300, 30, 3.5)
        );
    }
}
