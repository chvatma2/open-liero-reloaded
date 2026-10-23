//! Weapons and projectiles as authoring files (spec §2). Field docs give
//! each field's meaning, unit and OLX source
//! (`docs/porting/olx-notes/mod-fields.md`). Angles are degrees, 0 = +x
//! (right), 90 = +y (down); velocities px/s, accelerations px/s².

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::spawn::{EventSpawn, ShotSpawn, TrailSpawn};
use crate::Colour;
use crate::defaults as d;

/// `weapons/<id>.ron` (OLX `weapon_t`, `include/WeaponDesc.h:90-121`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weapon {
    /// Shown in menus; restrictions and bot loadouts match on it (OLX `name`).
    pub name: String,
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Full magazines per second at LoadingTime 100 % (OLX stores the
    /// file's value ÷ 10, `CGameScript.cpp:777`; this is that value).
    pub recharge: f32,
    /// Percent of a full magazine used per shot (> 100 empties it) (OLX
    /// `drain`, `CServer_Game.cpp:395-410`).
    pub drain: f32,
    /// Seconds between shots (OLX's `ROF` ÷ 1000, `CGameScript.cpp:776`).
    pub between_shots_s: f32,
    /// Spawns a laser-sight entity every sub-step while the weapon is not
    /// reloading; the entity takes an entity-pool slot, so it counts in the
    /// simulation (OLX `CClient_Game.cpp:751-808`).
    #[serde(default, skip_serializing_if = "d::is_false")]
    pub laser_sight: bool,
    /// What a shot does (OLX `Wpn_Type` and its payload, `mod-fields.md` §5).
    pub fire: Fire,
    /// The weapon's look (look system): its muzzle light. Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<crate::look::WeaponLookSource>,
}

/// How a weapon fires (OLX `Wpn_Type`; `docs/porting/olx-notes/mod-fields.md` §5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Fire {
    /// `WPN_PROJECTILE`: the shot spawns projectiles.
    Projectile {
        /// Unused by OLX and the bots; kept for the record (`CGameScript.cpp:729`).
        #[serde(default, skip_serializing_if = "d::is_default")]
        class: WeaponClass,
        /// px/s velocity impulse against the aim, once per shot
        /// (OLX `CClient_Game.cpp:909-914`); bots read a negative value as a jetpack.
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        recoil: i32,
        /// Pack path of the firing sound, played at the shot position (OLX `sound`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound: Option<String>,
        /// What the shot spawns (OLX `Proj_SpawnInfo` of the weapon; shot context).
        shot: ShotSpawn,
    },
    /// `WPN_BEAM` (`include/WeaponDesc.h:71-85`): an instant trace.
    Beam {
        /// px/s; unused by the simulation (bots read its sign) (OLX `recoil`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        recoil: i32,
        /// `#rrggbb`. The beam is not drawn when it is `#ff00ff` (OLX `colour`).
        colour: Colour,
        /// Hole size carved at the first dirt or rock pixel (0..4); -1 = none
        /// (OLX `damage`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        hole_size: i32,
        /// Health points to the first other worm within 5 px of the trace
        /// (OLX `player_damage`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        worm_damage: i32,
        /// Maximum trace length in px (one step = 1 px along the aim) (OLX `length`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        length: i32,
        /// Parts side by side at the start, px. Default 1 (OLX `init_width`).
        #[serde(default = "d::one_i32", skip_serializing_if = "d::is_one_i32")]
        width: i32,
        /// px of width added per px of length (OLX `width_increase`).
        #[serde(default, skip_serializing_if = "d::is_zero")]
        width_growth: f32,
        /// Divides both damages over the beam's width (at least 1 per part) (OLX `distribute_damage_over_width`).
        #[serde(default, skip_serializing_if = "d::is_false")]
        split_damage_over_width: bool,
    },
    /// `WPN_SPECIAL`: a special device such as a jetpack.
    Special {
        /// Which special (OLX `Wpn_Special`).
        kind: SpecialKind,
        /// Jetpack: value × 50 px/s² upward per 10 ms frame
        /// (OLX `CClient_Game.cpp:549-585`).
        thrust: u32,
    },
}

/// OLX `Wpn_Class` (`WeaponDesc.h:25-34`); unused everywhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponClass {
    /// LX value 0 (the default).
    #[default]
    Automatic,
    /// LX value 1.
    PowerGun,
    /// LX value 3.
    Grenade,
    /// LX value 4.
    Missile,
    /// LX value 5.
    CloseRange,
    /// Any other LX value (2 and unnamed ones).
    Unknown(i32),
}

/// OLX `Wpn_Special`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecialKind {
    /// OLX `SPC_NONE` (0): drains the charge, no thrust.
    Off,
    /// OLX `SPC_JETPACK` (1): thrust, see `Fire::Special::thrust`.
    Jetpack,
    /// Any other value; behaves like `Off`.
    Unknown(i32),
}

/// A projectile (OLX `proj_t`, `include/ProjectileDesc.h:80-121`), inline in
/// a spawn or in `projectiles/<id>.ron` (shared); at most `MAX_SHARED` (4096) shared ones.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projectile {
    /// The runtime id, written only when it differs from the one derived
    /// from its place: `<weapon id>`, then the chain of spawns and trails
    /// as `s` and `t`, a run its letter and count (`gun.s`, `gun.s2t`).
    /// Never set in `projectiles/` files (their id is the file name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Free text for authors; ignored by the game.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Simulation data, not only looks: the colour count, rotation and
    /// animation affect play (`mod-fields.md` §1.2, §8).
    pub gfx: Gfx,
    /// Collision box in px (OLX `width`, `height`; radius = half). Default
    /// (2, 2) for `Pixels`, (4, 4) for `Image` (`CGameScript.cpp:873-878`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<(i32, i32)>,
    /// px/s² added to the vertical velocity (OLX `ProjGravity`); absent =
    /// the world's gravity (100). `0` and `100` are written: they are not
    /// "absent" (`mod-fields.md` §1.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gravity: Option<i32>,
    /// Velocity × this once per 11 ms physics step (> 1 accelerates).
    /// OLX's `Dampening`.
    #[serde(default = "d::one", skip_serializing_if = "d::is_one")]
    pub velocity_factor: f32,
    /// The timed event (OLX `Timer`); absent = none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer: Option<Timer>,
    /// What the projectile leaves behind while flying (OLX `Trail`, `mod-fields.md` §11).
    #[serde(default, skip_serializing_if = "Trail::is_off")]
    pub trail: Trail,
    /// When the projectile hits rock or dirt (OLX `Hit`). `[]` = OLX's
    /// `Nothing`. The effect kind also selects the collision physics:
    /// `Bounce`, `Nothing` (half-move), `GoThrough` (no stop), `Explode` in
    /// dirt only (keeps position); everything else stops at the last safe
    /// position (`mod-fields.md` §7). At most `MAX_EFFECTS` (16).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hit_terrain: Vec<Effect>,
    /// When it hits a worm (OLX `PlayerHit`). `[]` = `Nothing`, which also
    /// turns worm collision off; `GoThrough` hits but does not stop. At most `MAX_EFFECTS` (16).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hit_worm: Vec<Effect>,
    /// Named spawns that `Spawn(<name>)` effects use. This runtime takes at
    /// most one, shared by all events (OLX `GeneralSpawnInfo`, expanded as
    /// `<path>.spawn`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub spawns: BTreeMap<String, EventSpawn>,
    /// The projectile's look (look system): glow, a light, an end flash,
    /// its explosions' light and heat, its heat a tick; presentation only.
    /// Default none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<crate::look::ProjectileLookSource>,
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
            look: None,
        }
    }
}

/// How a projectile looks (and what it costs the simulation); OLX
/// `ProjectileGfx`, `mod-fields.md` §8.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Gfx {
    /// A 1 px square. One of the `#rrggbb` colours is drawn at spawn (one
    /// `rand()`; none when the list is empty). OLX `Pixel.colours`. At most `MAX_COLOURS` (256).
    Pixels(Vec<Colour>),
    /// An image strip (OLX `Image`).
    Image {
        /// Pack path of the image strip. Frames are square: frame count =
        /// width ÷ height (`mod-fields.md` §8).
        file: String,
        /// Spins at `rotation_speed` and gets a random start rotation when a
        /// shot spawns it (OLX `Rotating`).
        #[serde(default, skip_serializing_if = "d::is_false")]
        rotating: bool,
        /// Degrees per image frame; the random start rotation's step
        /// (OLX `rot_increment`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        rotation_step_deg: i32,
        /// Degrees per second while `rotating` (OLX `rot_speed`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        rotation_speed: i32,
        /// Render only: the frame follows the heading (OLX `use_angle`).
        #[serde(default, skip_serializing_if = "d::is_false")]
        frames_follow_heading: bool,
        /// Render only: left- and right-moving halves over 151° (OLX `use_spec_angle`).
        #[serde(default, skip_serializing_if = "d::is_false")]
        frames_follow_heading_mirrored: bool,
        /// Render only: number of frames the two heading schemes use
        /// (OLX `angle_images`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        heading_frames: i32,
        /// Frame animation (OLX `Animating`); absent = static.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        animation: Option<Animation>,
    },
}

/// A projectile's frame animation (OLX `Animating`, `AnimRate`, `AnimType`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    /// Frames per second (OLX `rate`).
    #[serde(default, skip_serializing_if = "d::is_zero")]
    pub fps: f32,
    /// What happens at the last frame. `Once` removes the projectile (OLX `AnimType`).
    #[serde(default, skip_serializing_if = "d::is_default")]
    pub mode: AnimMode,
}

/// What an animation does at its last frame (OLX `Proj_AnimType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimMode {
    /// `ANI_ONCE` (0): deletes the projectile at the last frame.
    #[default]
    Once,
    /// `ANI_LOOP` (1): back to frame 0.
    Loop,
    /// `ANI_PINGPONG` (2): reverses at either end.
    PingPong,
    /// An OLX animation type this format has no name for: nothing at the end.
    Unknown(i32),
}

/// OLX `Proj_LX56Timer` (`ProjAction.h:452-468`). It fires every step once
/// its time has passed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timer {
    /// Seconds of projectile life (OLX `time`). The effects need `after_s > 0`.
    pub after_s: f32,
    /// Seconds, ± range (× the projectile's fixed random in [-1, 1]; OLX `time_var`).
    #[serde(default, skip_serializing_if = "d::is_zero")]
    pub variation_s: f32,
    /// What fires. `[]` = `Nothing`, which also cancels a spawn on a timer (OLX `action`; at most `MAX_EFFECTS` (16)).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
}

/// OLX `Proj_Trail` (`ProjAction.h:131-138`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Trail {
    /// `TRL_NONE`: no trail.
    #[default]
    Off,
    /// A smoke entity every 75 ms (OLX trail type 1).
    Smoke,
    /// A chemical-smoke entity every 75 ms (OLX trail type 2).
    ChemSmoke,
    /// A doomsday entity every 50 ms (OLX trail type 4).
    Doomsday,
    /// A cosmetic explosion entity (size 10) every 50 ms (OLX trail type 5).
    Explosive,
    /// A spawn every `every_s` seconds (whole ms), the first on the first step (OLX trail type 3).
    Projectile {
        /// Seconds between spawns (OLX `delay`). Default 0.1.
        #[serde(default = "d::tenth", skip_serializing_if = "d::is_tenth")]
        every_s: f32,
        /// What each trail step spawns (OLX `spawn`; trail context).
        spawn: TrailSpawn,
    },
    /// An OLX trail type this format has no name for: no trail.
    Unknown(i32),
}

impl Trail {
    /// No trail.
    pub fn is_off(&self) -> bool {
        *self == Trail::Off
    }
}

/// What an event does (OLX `Proj_ActionType`, `include/ProjAction.h:147-175`;
/// `mod-fields.md` §12). An event's list holds at most one effect besides
/// `Spawn` in this runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Effect {
    /// A blast, then the projectile dies.
    Explode {
        /// Health points to every living worm within 5 px (no falloff) and
        /// the hole size `clamp(damage, 0, 4)`; -1 = no blast (still dies
        /// and spawns) (OLX `damage`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        damage: i32,
        /// World px of camera shake (cosmetic, ±shake for 200 ms) (OLX `shake`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        shake: i32,
        /// Pack path of the explosion sound, played at the position (OLX `sound`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sound: Option<String>,
    },
    /// Bounces off terrain or a worm; `explode` > 0 blasts too.
    Bounce {
        /// Unitless velocity factor on the colliding side (OLX `bounce_coeff`).
        /// Default 0.5.
        #[serde(default = "d::half", skip_serializing_if = "d::is_half")]
        factor: f32,
        /// Blast damage on every colliding step (health points and hole
        /// size, as `Explode`); 0 = none. Not on a timer (OLX `bounce_explode`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        explode: i32,
    },
    /// On a worm: injures, then the projectile dies. On terrain it behaves
    /// like `Halt`: it stops and dies only at the map border.
    Injure {
        /// Health points (OLX `damage`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        damage: i32,
    },
    /// A hole at once (before the move on a timer), then the projectile dies.
    /// Against a worm it only pushes.
    Carve {
        /// Hole size, 0 to 4 (OLX `damage`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        size: i32,
    },
    /// Three dirt balls, then the projectile dies.
    Dirt {
        /// World px of camera shake; applies on a timer only (OLX `shake`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        shake: i32,
    },
    /// Green dirt, then the projectile dies.
    GreenDirt {
        /// World px of camera shake; applies on a timer only (OLX `shake`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        shake: i32,
    },
    /// OLX's `Disappear` (7), renamed for what it does: mostly ignored in
    /// LX56, the projectile stops but is **not** removed (it dies only at the
    /// map border); on a worm it only pushes. `Remove` removes it.
    Halt,
    /// OLX's `Disappear2` (9): the projectile dies.
    Remove,
    /// Passes through terrain (`hit_terrain`) or worms (`hit_worm`).
    GoThrough,
    /// No effect in LX56 events (no targets).
    InjureProjectile,
    /// Plays a sound (OLX `PlaySound`; LX56 files never set one).
    Sound {
        /// Pack path of the sound (OLX `sound`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<String>,
    },
    /// On a worm: injures; the projectile keeps flying.
    InjureWorm {
        /// Health points (OLX `damage`).
        #[serde(default, skip_serializing_if = "d::is_zero_i32")]
        damage: i32,
    },
    /// OLX `ChangeRadius`: a no-op in LX56.
    ChangeRadius,
    /// OLX `OverwriteOwnSpeed`: sets the velocity to (0, 0) in LX56.
    SetSpeed,
    /// OLX `MultiplyOwnSpeed`: a no-op in LX56.
    MultiplySpeed,
    /// OLX `DiffOwnSpeed`: a no-op in LX56.
    AddSpeed,
    /// OLX `OverwriteTargetSpeed`: no targets in LX56, nothing.
    SetTargetSpeed,
    /// OLX `MultiplyTargetSpeed`: no targets, nothing.
    MultiplyTargetSpeed,
    /// OLX `DiffTargetSpeed`: no targets, nothing.
    AddTargetSpeed,
    /// OLX `HeadingToNextWorm`: steers toward the nearest living worm.
    HomeOnNearestWorm,
    /// OLX `HeadingToOwner`: steers toward the owner.
    HomeOnOwner,
    /// OLX `HeadingToNextOtherWorm`: steers toward the nearest worm that is not the owner.
    HomeOnNearestOtherWorm,
    /// OLX `HeadingToNextEnemyWorm`: as `HomeOnNearestOtherWorm` without teams.
    HomeOnNearestEnemy,
    /// OLX `HeadingToNextTeamMate`: a no-op without teams.
    HomeOnNearestTeamMate,
    /// OLX `HeadTargetToUs`: no targets, nothing.
    PullTargetsToUs,
    /// An OLX action number this format has no name for: nothing, but the
    /// worm push still applies.
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

    #[test]
    fn a_misspelt_spawn_field_names_the_field_not_the_wire_type() {
        let e = from_canonical::<crate::mods::ShotSpawn>("t.ron", "(amout: 2)").unwrap_err().to_string();
        assert!(e.contains("amout") && !e.contains("SpawnWire"), "{e}");
    }

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
            look: None,
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
        let trail_off = TrailSpawn { speed_var_diagonal: false, ..Default::default() };
        assert!(to_canonical("t", &trail_off).unwrap().contains("speed_var_diagonal: false"));
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

    fn round_trips<T>(v: &T) -> String
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let text = to_canonical("t", v).unwrap();
        let back: T = from_canonical("t", &text).unwrap();
        assert_eq!(&back, v, "{text}");
        assert_eq!(to_canonical("t", &back).unwrap(), text);
        text
    }

    #[test]
    fn a_beam_round_trips() {
        let beam = Fire::Beam {
            recoil: -3,
            colour: Colour(Rgba::rgb(0xff, 0, 0)),
            hole_size: -1,
            worm_damage: 3,
            length: 999_999,
            width: 4,
            width_growth: 0.5,
            split_damage_over_width: true,
        };
        round_trips(&beam);
        let plain = Fire::Beam {
            recoil: 0,
            colour: Colour(Rgba::rgb(0, 0, 255)),
            hole_size: 0,
            worm_damage: 0,
            length: 10,
            width: 1,
            width_growth: 0.0,
            split_damage_over_width: false,
        };
        let text = round_trips(&plain);
        assert!(!text.contains("width"), "{text}");
    }

    #[test]
    fn a_special_round_trips() {
        round_trips(&Fire::Special { kind: SpecialKind::Jetpack, thrust: 7 });
        round_trips(&Fire::Special { kind: SpecialKind::Unknown(9), thrust: 0 });
    }

    #[test]
    fn a_projectile_trail_writes_only_a_non_default_interval() {
        let spawn = TrailSpawn { speed: 5, ..TrailSpawn::default() };
        let default_every = Trail::Projectile { every_s: 0.1, spawn: spawn.clone() };
        let text = round_trips(&default_every);
        assert!(!text.contains("every_s"), "{text}");
        let other = Trail::Projectile { every_s: 0.04, spawn };
        let text = round_trips(&other);
        assert!(text.contains("every_s"), "{text}");
    }

    #[test]
    fn unknown_variants_round_trip() {
        round_trips(&Effect::Unknown(42));
        round_trips(&WeaponClass::Unknown(2));
        round_trips(&AnimMode::Unknown(7));
        round_trips(&Trail::Unknown(8));
    }

    #[test]
    fn an_animated_image_round_trips() {
        let g = Gfx::Image {
            file: "gfx/a.png".into(),
            rotating: true,
            rotation_step_deg: 90,
            rotation_speed: 400,
            frames_follow_heading: false,
            frames_follow_heading_mirrored: false,
            heading_frames: 0,
            animation: Some(Animation { fps: 10.0, mode: AnimMode::Loop }),
        };
        round_trips(&g);
    }

    #[test]
    fn misspelt_variant_fields_are_errors() {
        let e = from_canonical::<Effect>("t", "Explode(damge: 4)").unwrap_err();
        assert!(e.to_string().contains("damge"), "{e}");
        let e = from_canonical::<Fire>("t", "Beam(colour: \"#ff0000\", wdth: 3)").unwrap_err();
        assert!(e.to_string().contains("wdth"), "{e}");
        let e = from_canonical::<Fire>("t", "Special(kind: Jetpack, thrust: 1, thrst: 2)").unwrap_err();
        assert!(e.to_string().contains("thrst"), "{e}");
        let e = from_canonical::<Fire>("t", "Projectile(shot: (), recol: 1)").unwrap_err();
        assert!(e.to_string().contains("recol"), "{e}");
        let e = from_canonical::<Gfx>("t", "Image(file: \"a.png\", rotatng: true)").unwrap_err();
        assert!(e.to_string().contains("rotatng"), "{e}");
        let e = from_canonical::<Trail>("t", "Projectile(evry_s: 0.1, spawn: ())").unwrap_err();
        assert!(e.to_string().contains("evry_s"), "{e}");
    }

    #[test]
    fn misspelt_worm_and_rope_fields_are_errors() {
        let e = from_canonical::<Worm>("t", "(gravty: 1.0)").unwrap_err();
        assert!(e.to_string().contains("gravty"), "{e}");
        let e = from_canonical::<Rope>("t", "(max_lenght: 1)").unwrap_err();
        assert!(e.to_string().contains("max_lenght"), "{e}");
    }
}
