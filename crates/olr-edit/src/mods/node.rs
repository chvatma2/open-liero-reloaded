//! The navigator under [`super::get`] and [`super::set`]: every type of a
//! mod's files as a [`Node`], a value with named children. A value reads
//! and writes as canonical RON (`olr_format::ron_io`), so a set value is
//! checked by the type's own deserializer; an enum offers its variants
//! (never `Unknown`) and makes a chosen one with that variant's defaults.

use std::collections::BTreeMap;

use olr_data::image::Rgba;
use olr_data::loadouts::ShippedLoadout;
use olr_data::restrictions::WeaponState;
use olr_format::Colour;
use olr_format::look::{
    AnimationSource, ExplosionHeatSource, ExplosionLightSource, KeySource, LightSource, ModWideLookSource,
    ProjectileHeatSource, ProjectileLookSource, PulseShape, ReachSource, WeaponLookSource,
};
use olr_format::mods::{
    AnimMode, Animation, Effect, Fire, Gfx, LIFTED_SPAWN, ModSource, Projectile, Rope, Spawn, SpawnContext,
    SpecialKind, Timer, Trail, TrailSpawn, Weapon, WeaponClass, Worm,
};
use olr_format::ron_io::{from_canonical, to_canonical};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::path::Seg;

/// Where a step leads.
pub(crate) enum Step<'a> {
    Node(&'a dyn Node),
    /// A spawn's `projectile` that is the shared projectile of this id.
    Shared(&'a str),
    /// A read-only value, as canonical RON (`mod.weapons`: the order is
    /// the weapon operations').
    Text(String),
}

/// A value of a mod's files.
pub(crate) trait Node {
    /// The value as canonical RON, without the final line break.
    fn ron(&self) -> Result<String, String>;
    /// Replaces the value with `text` read as its type.
    fn parse(&mut self, text: &str) -> Result<(), String>;
    /// The children's steps in the file's order: every field (defaults
    /// too), item or key. Empty for a leaf.
    fn children(&self) -> Vec<Seg>;
    fn child(&self, seg: &Seg) -> Option<Step<'_>>;
    fn child_mut(&mut self, seg: &Seg) -> Option<&mut dyn Node>;
    /// An enum's variants offered as choices (never `Unknown`).
    fn choices(&self) -> &'static [&'static str] {
        &[]
    }
    /// An enum: becomes variant `name` with its defaults; false when there
    /// is no such choice.
    fn set_variant(&mut self, _name: &str) -> bool {
        false
    }
    /// A map: adds `key` with the value `text`; `None` when this is no map.
    fn insert(&mut self, _key: &str, _text: &str) -> Option<Result<(), String>> {
        None
    }
    /// A spawn: its `shared` and `projectile`.
    fn spawn_mut(&mut self) -> Option<(&mut Option<String>, &mut Option<Box<Projectile>>)> {
        None
    }
}

pub(crate) fn ron_of<T: Serialize>(v: &T) -> Result<String, String> {
    to_canonical("value", v).map(|s| s.trim_end().to_string()).map_err(|e| e.to_string())
}

pub(crate) fn parse_as<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    from_canonical("value", text).map_err(|e| e.to_string())
}

/// The leading identifier of canonical RON: an enum value's variant.
pub(crate) fn variant_of(ron: &str) -> Option<&str> {
    let end = ron.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(ron.len());
    (end > 0 && ron.as_bytes()[0].is_ascii_uppercase()).then(|| &ron[..end])
}

macro_rules! text {
    () => {
        fn ron(&self) -> Result<String, String> {
            ron_of(self)
        }
        fn parse(&mut self, text: &str) -> Result<(), String> {
            *self = parse_as(text)?;
            Ok(())
        }
    };
}

macro_rules! leaf {
    ($($t:ty),* $(,)?) => {$(
        impl Node for $t {
            text!();
            fn children(&self) -> Vec<Seg> {
                Vec::new()
            }
            fn child(&self, _: &Seg) -> Option<Step<'_>> {
                None
            }
            fn child_mut(&mut self, _: &Seg) -> Option<&mut dyn Node> {
                None
            }
        }
    )*};
}

leaf!(i32, u32, u16, u8, i16, f32, bool, String, Colour, (i32, i32), KeySource);

/// A struct's or enum variant's fields by name.
trait Fields {
    fn fields(&self) -> Vec<(&'static str, &dyn Node)>;
    fn fields_mut(&mut self) -> Vec<(&'static str, &mut dyn Node)>;
}

/// A tuple variant's field is bound as `_0` and named `0`.
fn label(s: &'static str) -> &'static str {
    s.strip_prefix('_').unwrap_or(s)
}

/// `Fields` from one pattern per shape, binding the fields by name.
macro_rules! fields {
    ($t:ty { $($pat:pat => [$($f:ident),* $(,)?]),* $(,)? }) => {
        impl Fields for $t {
            #[allow(unused_variables, clippy::just_underscores_and_digits)]
            fn fields(&self) -> Vec<(&'static str, &dyn Node)> {
                match self { $($pat => vec![$((label(stringify!($f)), $f as &dyn Node)),*],)* }
            }
            #[allow(unused_variables, clippy::just_underscores_and_digits)]
            fn fields_mut(&mut self) -> Vec<(&'static str, &mut dyn Node)> {
                match self { $($pat => vec![$((label(stringify!($f)), $f as &mut dyn Node)),*],)* }
            }
        }
    };
}

fn children_of(fields: Vec<(&'static str, &dyn Node)>) -> Vec<Seg> {
    fields.into_iter().map(|(n, _)| Seg::Name(n.into())).collect()
}

fn named<T>(fields: Vec<(&'static str, T)>, seg: &Seg) -> Option<T> {
    let Seg::Name(name) = seg else { return None };
    fields.into_iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// An enum's choices and the defaults of each.
trait Variants: Sized + DeserializeOwned {
    const CHOICES: &'static [&'static str];
    /// The variant's value with its defaults: a hand-made one, else the
    /// variant read as `Name` or `Name()` (its fields' serde defaults).
    fn made(_name: &str) -> Option<Self> {
        None
    }
    fn default_of(name: &str) -> Option<Self> {
        if !Self::CHOICES.contains(&name) {
            return None;
        }
        Self::made(name).or_else(|| parse_as(name).ok()).or_else(|| parse_as(&format!("{name}()")).ok())
    }
}

/// `Node` from `Fields` (and `Variants` for an enum).
macro_rules! composite {
    ($t:ty) => {
        impl Node for $t {
            text!();
            composite!(@fields);
        }
    };
    (enum $t:ty) => {
        impl Node for $t {
            text!();
            composite!(@fields);
            fn choices(&self) -> &'static [&'static str] {
                <$t as Variants>::CHOICES
            }
            fn set_variant(&mut self, name: &str) -> bool {
                match <$t as Variants>::default_of(name) {
                    Some(v) => {
                        *self = v;
                        true
                    }
                    None => false,
                }
            }
        }
    };
    (@fields) => {
        fn children(&self) -> Vec<Seg> {
            children_of(self.fields())
        }
        fn child(&self, seg: &Seg) -> Option<Step<'_>> {
            named(self.fields(), seg).map(Step::Node)
        }
        fn child_mut(&mut self, seg: &Seg) -> Option<&mut dyn Node> {
            named(self.fields_mut(), seg)
        }
    };
}

impl<T: Node + Serialize + DeserializeOwned> Node for Option<T> {
    text!();
    fn children(&self) -> Vec<Seg> {
        self.as_ref().map(Node::children).unwrap_or_default()
    }
    fn child(&self, seg: &Seg) -> Option<Step<'_>> {
        self.as_ref()?.child(seg)
    }
    fn child_mut(&mut self, seg: &Seg) -> Option<&mut dyn Node> {
        self.as_mut()?.child_mut(seg)
    }
    fn choices(&self) -> &'static [&'static str] {
        self.as_ref().map(Node::choices).unwrap_or_default()
    }
    fn set_variant(&mut self, name: &str) -> bool {
        self.as_mut().is_some_and(|v| v.set_variant(name))
    }
    fn insert(&mut self, key: &str, text: &str) -> Option<Result<(), String>> {
        self.as_mut()?.insert(key, text)
    }
    fn spawn_mut(&mut self) -> Option<(&mut Option<String>, &mut Option<Box<Projectile>>)> {
        self.as_mut()?.spawn_mut()
    }
}

impl<T: Node + Serialize + DeserializeOwned> Node for Box<T> {
    text!();
    fn children(&self) -> Vec<Seg> {
        (**self).children()
    }
    fn child(&self, seg: &Seg) -> Option<Step<'_>> {
        (**self).child(seg)
    }
    fn child_mut(&mut self, seg: &Seg) -> Option<&mut dyn Node> {
        (**self).child_mut(seg)
    }
    fn choices(&self) -> &'static [&'static str] {
        (**self).choices()
    }
    fn set_variant(&mut self, name: &str) -> bool {
        (**self).set_variant(name)
    }
}

impl<T: Node + Serialize + DeserializeOwned> Node for Vec<T> {
    text!();
    fn children(&self) -> Vec<Seg> {
        (0..self.len()).map(Seg::Index).collect()
    }
    fn child(&self, seg: &Seg) -> Option<Step<'_>> {
        let Seg::Index(i) = seg else { return None };
        self.get(*i).map(|v| Step::Node(v as &dyn Node))
    }
    fn child_mut(&mut self, seg: &Seg) -> Option<&mut dyn Node> {
        let Seg::Index(i) = seg else { return None };
        self.get_mut(*i).map(|v| v as &mut dyn Node)
    }
}

impl<V: Node + Serialize + DeserializeOwned> Node for BTreeMap<String, V> {
    text!();
    fn children(&self) -> Vec<Seg> {
        self.keys().map(|k| Seg::Name(k.clone())).collect()
    }
    fn child(&self, seg: &Seg) -> Option<Step<'_>> {
        let Seg::Name(k) = seg else { return None };
        self.get(k).map(|v| Step::Node(v as &dyn Node))
    }
    fn child_mut(&mut self, seg: &Seg) -> Option<&mut dyn Node> {
        let Seg::Name(k) = seg else { return None };
        self.get_mut(k).map(|v| v as &mut dyn Node)
    }
    fn insert(&mut self, key: &str, text: &str) -> Option<Result<(), String>> {
        Some(parse_as::<V>(text).map(|v| {
            BTreeMap::insert(self, key.to_string(), v);
        }))
    }
}

// mod.ron's parts, and restrictions.ron's.

fields!(Worm {
    Worm {
        ground_acceleration,
        air_acceleration,
        gravity,
        jump_speed,
        air_drag,
        unused_angle_speed,
        unused_ground_friction,
    } => [
        ground_acceleration,
        air_acceleration,
        gravity,
        jump_speed,
        air_drag,
        unused_angle_speed,
        unused_ground_friction,
    ],
});
composite!(Worm);

fields!(Rope { Rope { max_length, rest_length, strength } => [max_length, rest_length, strength] });
composite!(Rope);

fields!(ShippedLoadout { ShippedLoadout { loading_time, unique } => [loading_time, unique] });
composite!(ShippedLoadout);

fields!(WeaponState { _ => [] });
impl Variants for WeaponState {
    const CHOICES: &'static [&'static str] = &["Enabled", "Bonus", "Banned"];
}
composite!(enum WeaponState);

// Weapons.

fields!(Weapon {
    Weapon { name, notes, recharge, drain, between_shots_s, laser_sight, fire, look } =>
        [name, notes, recharge, drain, between_shots_s, laser_sight, fire, look],
});
composite!(Weapon);

fields!(Fire {
    Fire::Projectile { class, recoil, sound, shot } => [class, recoil, sound, shot],
    Fire::Beam { recoil, colour, hole_size, worm_damage, length, width, width_growth, split_damage_over_width } =>
        [recoil, colour, hole_size, worm_damage, length, width, width_growth, split_damage_over_width],
    Fire::Special { kind, thrust } => [kind, thrust],
});
impl Variants for Fire {
    const CHOICES: &'static [&'static str] = &["Projectile", "Beam", "Special"];
    fn made(name: &str) -> Option<Self> {
        Some(match name {
            "Projectile" => super::weapons::blank_weapon("").fire,
            "Beam" => Fire::Beam {
                recoil: 0,
                colour: Colour(Rgba::rgb(255, 0, 0)),
                hole_size: 0,
                worm_damage: 0,
                length: 0,
                width: 1,
                width_growth: 0.0,
                split_damage_over_width: false,
            },
            "Special" => Fire::Special { kind: SpecialKind::Off, thrust: 0 },
            _ => return None,
        })
    }
}
composite!(enum Fire);

fields!(WeaponClass { WeaponClass::Unknown(_0) => [_0], _ => [] });
impl Variants for WeaponClass {
    const CHOICES: &'static [&'static str] = &["Automatic", "PowerGun", "Grenade", "Missile", "CloseRange"];
}
composite!(enum WeaponClass);

fields!(SpecialKind { SpecialKind::Unknown(_0) => [_0], _ => [] });
impl Variants for SpecialKind {
    const CHOICES: &'static [&'static str] = &["Off", "Jetpack"];
}
composite!(enum SpecialKind);

// Spawns: a field list of their own, and `projectile` leading to the shared
// projectile when the spawn names one.

fn spawn_fields<C: SpawnContext>(s: &Spawn<C>) -> Vec<(&'static str, &dyn Node)> {
    vec![
        ("amount", &s.amount),
        ("speed", &s.speed),
        ("speed_var", &s.speed_var),
        ("spread", &s.spread),
        ("relative_to_parent", &s.relative_to_parent),
        ("angle", &s.angle),
        ("parent_velocity_as_direction", &s.parent_velocity_as_direction),
        ("parent_velocity_direction_scale", &s.parent_velocity_direction_scale),
        ("speed_var_diagonal", &s.speed_var_diagonal),
        ("random_rotation", &s.random_rotation),
        ("inherit_velocity", &s.inherit_velocity),
        ("inherit_velocity_factor", &s.inherit_velocity_factor),
        ("shared", &s.shared),
        ("projectile", &s.projectile),
    ]
}

fn spawn_fields_mut<C: SpawnContext>(s: &mut Spawn<C>) -> Vec<(&'static str, &mut dyn Node)> {
    vec![
        ("amount", &mut s.amount),
        ("speed", &mut s.speed),
        ("speed_var", &mut s.speed_var),
        ("spread", &mut s.spread),
        ("relative_to_parent", &mut s.relative_to_parent),
        ("angle", &mut s.angle),
        ("parent_velocity_as_direction", &mut s.parent_velocity_as_direction),
        ("parent_velocity_direction_scale", &mut s.parent_velocity_direction_scale),
        ("speed_var_diagonal", &mut s.speed_var_diagonal),
        ("random_rotation", &mut s.random_rotation),
        ("inherit_velocity", &mut s.inherit_velocity),
        ("inherit_velocity_factor", &mut s.inherit_velocity_factor),
        ("shared", &mut s.shared),
        ("projectile", &mut s.projectile),
    ]
}

impl<C: SpawnContext> Node for Spawn<C> {
    text!();
    fn children(&self) -> Vec<Seg> {
        children_of(spawn_fields(self))
    }
    fn child(&self, seg: &Seg) -> Option<Step<'_>> {
        if *seg == Seg::Name("projectile".into())
            && self.projectile.is_none()
            && let Some(id) = &self.shared
        {
            return Some(Step::Shared(id));
        }
        named(spawn_fields(self), seg).map(Step::Node)
    }
    fn child_mut(&mut self, seg: &Seg) -> Option<&mut dyn Node> {
        named(spawn_fields_mut(self), seg)
    }
    fn spawn_mut(&mut self) -> Option<(&mut Option<String>, &mut Option<Box<Projectile>>)> {
        Some((&mut self.shared, &mut self.projectile))
    }
}

// Projectiles.

fields!(Projectile {
    Projectile { id, notes, gfx, size, gravity, velocity_factor, timer, trail, hit_terrain, hit_worm, spawns, look } =>
        [id, notes, gfx, size, gravity, velocity_factor, timer, trail, hit_terrain, hit_worm, spawns, look],
});
composite!(Projectile);

fields!(Gfx {
    Gfx::Pixels(_0) => [_0],
    Gfx::Image {
        file,
        rotating,
        rotation_step_deg,
        rotation_speed,
        frames_follow_heading,
        frames_follow_heading_mirrored,
        heading_frames,
        animation,
    } => [
        file,
        rotating,
        rotation_step_deg,
        rotation_speed,
        frames_follow_heading,
        frames_follow_heading_mirrored,
        heading_frames,
        animation,
    ],
});
impl Variants for Gfx {
    const CHOICES: &'static [&'static str] = &["Pixels", "Image"];
    /// `Image` names no file yet: a set is refused until `file` names an
    /// image of the pack (give the whole value, `Image(file: "gfx/x.png")`).
    fn made(name: &str) -> Option<Self> {
        Some(match name {
            "Pixels" => Gfx::Pixels(vec![Colour(Rgba::rgb(255, 255, 255))]),
            "Image" => Gfx::Image {
                file: String::new(),
                rotating: false,
                rotation_step_deg: 0,
                rotation_speed: 0,
                frames_follow_heading: false,
                frames_follow_heading_mirrored: false,
                heading_frames: 0,
                animation: None,
            },
            _ => return None,
        })
    }
}
composite!(enum Gfx);

fields!(Animation { Animation { fps, mode } => [fps, mode] });
composite!(Animation);

fields!(AnimMode { AnimMode::Unknown(_0) => [_0], _ => [] });
impl Variants for AnimMode {
    const CHOICES: &'static [&'static str] = &["Once", "Loop", "PingPong"];
}
composite!(enum AnimMode);

fields!(Timer { Timer { after_s, variation_s, effects } => [after_s, variation_s, effects] });
composite!(Timer);

fields!(Trail {
    Trail::Projectile { every_s, spawn } => [every_s, spawn],
    Trail::Unknown(_0) => [_0],
    _ => [],
});
impl Variants for Trail {
    const CHOICES: &'static [&'static str] =
        &["Off", "Smoke", "ChemSmoke", "Doomsday", "Explosive", "Projectile"];
    fn made(name: &str) -> Option<Self> {
        let dot = Projectile::new(Gfx::Pixels(vec![Colour(Rgba::rgb(255, 255, 255))]));
        (name == "Projectile").then(|| Trail::Projectile {
            every_s: 0.1,
            spawn: TrailSpawn { projectile: Some(Box::new(dot)), ..TrailSpawn::default() },
        })
    }
}
composite!(enum Trail);

fields!(Effect {
    Effect::Explode { damage, shake, sound } => [damage, shake, sound],
    Effect::Bounce { factor, explode } => [factor, explode],
    Effect::Injure { damage } => [damage],
    Effect::Carve { size } => [size],
    Effect::Dirt { shake } => [shake],
    Effect::GreenDirt { shake } => [shake],
    Effect::Sound { file } => [file],
    Effect::InjureWorm { damage } => [damage],
    Effect::Unknown(_0) => [_0],
    Effect::Spawn(_0) => [_0],
    _ => [],
});
impl Variants for Effect {
    const CHOICES: &'static [&'static str] = &[
        "Explode",
        "Bounce",
        "Injure",
        "Carve",
        "Dirt",
        "GreenDirt",
        "Halt",
        "Remove",
        "GoThrough",
        "InjureProjectile",
        "Sound",
        "InjureWorm",
        "ChangeRadius",
        "SetSpeed",
        "MultiplySpeed",
        "AddSpeed",
        "SetTargetSpeed",
        "MultiplyTargetSpeed",
        "AddTargetSpeed",
        "HomeOnNearestWorm",
        "HomeOnOwner",
        "HomeOnNearestOtherWorm",
        "HomeOnNearestEnemy",
        "HomeOnNearestTeamMate",
        "PullTargetsToUs",
        "Spawn",
    ];
    fn made(name: &str) -> Option<Self> {
        (name == "Spawn").then(|| Effect::Spawn(LIFTED_SPAWN.into()))
    }
}
composite!(enum Effect);

// Looks.

fields!(WeaponLookSource { WeaponLookSource { muzzle_light } => [muzzle_light] });
composite!(WeaponLookSource);

fields!(ModWideLookSource {
    ModWideLookSource { explosion, explosion_heat } => [explosion, explosion_heat],
});
composite!(ModWideLookSource);

fields!(ProjectileLookSource {
    ProjectileLookSource { glow, light, end_flash, explosion, explosion_heat, heat } =>
        [glow, light, end_flash, explosion, explosion_heat, heat],
});
composite!(ProjectileLookSource);

fields!(LightSource {
    LightSource { colour, intensity, reach, animation, phase_ms } => [colour, intensity, reach, animation, phase_ms],
});
composite!(LightSource);

fields!(ExplosionLightSource {
    ExplosionLightSource { colour, intensity, radius, reach, animation, phase_ms } =>
        [colour, intensity, radius, reach, animation, phase_ms],
});
composite!(ExplosionLightSource);

fields!(ExplosionHeatSource { ExplosionHeatSource { amount, radius } => [amount, radius] });
composite!(ExplosionHeatSource);

fields!(ProjectileHeatSource { ProjectileHeatSource { amount, radius } => [amount, radius] });
composite!(ProjectileHeatSource);

fields!(ReachSource { ReachSource::Pixels(_0) => [_0], ReachSource::Unlimited => [] });
impl Variants for ReachSource {
    const CHOICES: &'static [&'static str] = &["Pixels", "Unlimited"];
    fn made(name: &str) -> Option<Self> {
        (name == "Pixels").then_some(ReachSource::Pixels(64))
    }
}
composite!(enum ReachSource);

fields!(AnimationSource {
    AnimationSource::Pulse { period_ms, low, high, shape } => [period_ms, low, high, shape],
    AnimationSource::Flicker { speed, depth } => [speed, depth],
    AnimationSource::Keys { keys, looped } => [keys, looped],
    AnimationSource::Steady => [],
});
impl Variants for AnimationSource {
    const CHOICES: &'static [&'static str] = &["Steady", "Pulse", "Flicker", "Keys"];
    fn made(name: &str) -> Option<Self> {
        Some(match name {
            "Pulse" => {
                AnimationSource::Pulse { period_ms: 1000, low: 50, high: 100, shape: PulseShape::Sine }
            }
            "Flicker" => AnimationSource::Flicker { speed: 10, depth: 50 },
            "Keys" => AnimationSource::Keys {
                keys: vec![KeySource { at_ms: 0, level: 100, colour: None }],
                looped: false,
            },
            _ => return None,
        })
    }
}
composite!(enum AnimationSource);

fields!(PulseShape { _ => [] });
impl Variants for PulseShape {
    const CHOICES: &'static [&'static str] = &["Sine", "Triangle", "Square"];
}
composite!(enum PulseShape);

/// `mod.ron` as a node of the whole mod: its fields, `weapons` read-only.
impl Node for ModSource {
    fn ron(&self) -> Result<String, String> {
        let mut fs = olr_vfs::MemFs::new();
        olr_format::mods::write_mod_files(&mut fs, self).map_err(|e| e.to_string())?;
        let text =
            fs.files().find(|(p, _)| p.as_str() == "mod.ron").map(|(_, b)| b.to_vec()).unwrap_or_default();
        Ok(String::from_utf8_lossy(&text).trim_end().to_string())
    }
    fn parse(&mut self, _text: &str) -> Result<(), String> {
        Err("mod.ron is set field by field".into())
    }
    fn children(&self) -> Vec<Seg> {
        ["name", "notes", "worm", "rope", "weapons", "loadouts", "look"].map(|n| Seg::Name(n.into())).into()
    }
    fn child(&self, seg: &Seg) -> Option<Step<'_>> {
        let Seg::Name(n) = seg else { return None };
        Some(match n.as_str() {
            "name" => Step::Node(&self.name),
            "notes" => Step::Node(&self.notes),
            "worm" => Step::Node(&self.worm),
            "rope" => Step::Node(&self.rope),
            "weapons" => {
                let ids: Vec<&str> = self.weapons.iter().map(|(id, _)| id.as_str()).collect();
                Step::Text(ron_of(&ids).ok()?)
            }
            "loadouts" => Step::Node(&self.loadouts),
            "look" => Step::Node(&self.look),
            _ => return None,
        })
    }
    fn child_mut(&mut self, seg: &Seg) -> Option<&mut dyn Node> {
        let Seg::Name(n) = seg else { return None };
        Some(match n.as_str() {
            "name" => &mut self.name,
            "notes" => &mut self.notes,
            "worm" => &mut self.worm,
            "rope" => &mut self.rope,
            "loadouts" => &mut self.loadouts,
            "look" => &mut self.look,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_choice_makes_its_variant<T: Variants + Node>() {
        for &c in T::CHOICES {
            let v = T::default_of(c).unwrap_or_else(|| panic!("{c} has no default"));
            assert_eq!(variant_of(&v.ron().unwrap()), Some(c));
        }
        assert!(T::default_of("Unknown").is_none());
    }

    #[test]
    fn every_choice_has_defaults_and_unknown_is_none() {
        every_choice_makes_its_variant::<WeaponState>();
        every_choice_makes_its_variant::<Fire>();
        every_choice_makes_its_variant::<WeaponClass>();
        every_choice_makes_its_variant::<SpecialKind>();
        every_choice_makes_its_variant::<Gfx>();
        every_choice_makes_its_variant::<AnimMode>();
        every_choice_makes_its_variant::<Trail>();
        every_choice_makes_its_variant::<Effect>();
        every_choice_makes_its_variant::<ReachSource>();
        every_choice_makes_its_variant::<AnimationSource>();
        every_choice_makes_its_variant::<PulseShape>();
    }

    #[test]
    fn a_tuple_variant_field_is_named_0() {
        let e = Effect::Unknown(4);
        assert_eq!(e.children(), [Seg::Name("0".into())]);
        assert_eq!(variant_of("Unknown(4)"), Some("Unknown"));
        assert_eq!(variant_of("(a: 1)"), None);
    }
}
