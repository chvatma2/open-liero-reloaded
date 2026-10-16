//! Runtime `ModDef` -> authoring mod: how imports become authoring files.
//! The inverse of `compile_mod` on everything today's importers produce.

use std::collections::{BTreeMap, BTreeSet};

use olr_data::mod_def as rt;

use super::compile::MAX_INLINE_DEPTH;
use super::spawn::{Spawn, SpawnContext};
use super::types::*;
use super::{ModSource, Rope, Worm};
use crate::{Colour, LiftError};

/// The name of a lifted projectile's event spawn.
pub const LIFTED_SPAWN: &str = "spawn";

struct Lifter<'a> {
    def: &'a rt::ModDef,
    shared: BTreeSet<&'a str>,
    emitted: BTreeSet<&'a str>,
    stack: Vec<&'a str>,
    /// Compile's depth of the projectile at the bottom of `stack`: 1 under a
    /// weapon's shot, 0 for a shared projectile.
    base: usize,
}

fn err(item: impl Into<String>, message: impl Into<String>) -> LiftError {
    LiftError { item: item.into(), message: message.into() }
}

/// Lifts a runtime mod to its authoring form, so that `compile_mod` of the
/// result gives `def` back. `Err` when the mod cannot be written that way: a
/// projectile key that is missing, in a cycle, unreachable from any weapon or
/// nested deeper than `MAX_INLINE_DEPTH` (counted as `compile_mod` does); an
/// action with a field its kind does not use set to a non-default value; an
/// inconsistent timer or trail.
pub fn lift_mod(def: &rt::ModDef) -> Result<ModSource, LiftError> {
    let mut refs: BTreeMap<&str, usize> = def.projectiles.keys().map(|k| (k.as_str(), 0)).collect();
    for w in &def.weapons {
        if let Some(p) = w.projectile() {
            *refs.entry(p).or_default() += 1;
        }
    }
    for p in def.projectiles.values() {
        for c in p.children() {
            *refs.entry(c).or_default() += 1;
        }
    }
    let shared =
        refs.iter().filter(|(k, n)| **n != 1 && def.projectiles.contains_key(**k)).map(|(k, _)| *k).collect();
    let mut l = Lifter { def, shared, emitted: BTreeSet::new(), stack: Vec::new(), base: 1 };
    let mut weapons = Vec::with_capacity(def.weapons.len());
    for w in &def.weapons {
        weapons.push((w.id.clone(), l.weapon(w)?));
    }
    l.base = 0;
    let mut shared_out = BTreeMap::new();
    let ids: Vec<&str> = l.shared.iter().copied().collect();
    for id in ids {
        shared_out.insert(id.to_string(), l.projectile(id)?);
    }
    if l.emitted.len() != def.projectiles.len() {
        let lost: Vec<&str> =
            def.projectiles.keys().map(String::as_str).filter(|k| !l.emitted.contains(k)).collect();
        return Err(err("mod", format!("projectiles no weapon reaches, in a cycle: {}", lost.join(", "))));
    }
    Ok(ModSource {
        name: def.name.clone(),
        notes: String::new(),
        worm: Worm {
            ground_acceleration: def.worm.ground_speed,
            air_acceleration: def.worm.air_speed,
            gravity: def.worm.gravity,
            jump_speed: def.worm.jump_force,
            air_drag: def.worm.air_friction,
            unused_angle_speed: def.worm.angle_speed,
            unused_ground_friction: def.worm.ground_friction,
        },
        rope: Rope {
            max_length: def.rope.max_length,
            rest_length: def.rope.rest_length,
            strength: def.rope.strength,
        },
        weapons,
        shared: shared_out,
        restrictions: def.restrictions.clone(),
    })
}

impl<'a> Lifter<'a> {
    fn weapon(&mut self, w: &'a rt::WeaponDef) -> Result<Weapon, LiftError> {
        let fire = match &w.kind {
            rt::WeaponKind::Projectile { class, recoil, sound, spawn } => Fire::Projectile {
                class: match class {
                    rt::WeaponClass::Automatic => WeaponClass::Automatic,
                    rt::WeaponClass::PowerGun => WeaponClass::PowerGun,
                    rt::WeaponClass::Grenade => WeaponClass::Grenade,
                    rt::WeaponClass::Missile => WeaponClass::Missile,
                    rt::WeaponClass::CloseRange => WeaponClass::CloseRange,
                    rt::WeaponClass::Other(i) => WeaponClass::Unknown(*i),
                },
                recoil: *recoil,
                sound: sound.clone(),
                shot: self.spawn(spawn, &w.id)?,
            },
            rt::WeaponKind::Beam { recoil, beam } => Fire::Beam {
                recoil: *recoil,
                colour: Colour(beam.colour),
                hole_size: beam.damage,
                worm_damage: beam.player_damage,
                length: beam.length,
                width: beam.init_width,
                width_growth: beam.width_increase,
                split_damage_over_width: beam.distribute_damage_over_width,
            },
            rt::WeaponKind::Special { special, thrust } => Fire::Special {
                kind: match special {
                    rt::SpecialKind::None => SpecialKind::Off,
                    rt::SpecialKind::Jetpack => SpecialKind::Jetpack,
                    rt::SpecialKind::Other(i) => SpecialKind::Unknown(*i),
                },
                thrust: *thrust,
            },
        };
        Ok(Weapon {
            name: w.name.clone(),
            notes: String::new(),
            recharge: w.recharge,
            drain: w.drain,
            between_shots_s: w.rof,
            laser_sight: w.laser_sight,
            fire,
        })
    }

    fn spawn<C: SpawnContext>(
        &mut self,
        s: &'a rt::SpawnDef,
        default_key: &str,
    ) -> Result<Spawn<C>, LiftError> {
        let (shared, projectile) = if self.shared.contains(s.projectile.as_str()) {
            (Some(s.projectile.clone()), None)
        } else {
            let mut p = self.projectile(&s.projectile)?;
            if s.projectile != default_key {
                p.id = Some(s.projectile.clone());
            }
            (None, Some(Box::new(p)))
        };
        Ok(Spawn {
            shared,
            projectile,
            amount: s.amount,
            speed: s.speed,
            speed_var: s.speed_var,
            spread: s.spread,
            relative_to_parent: s.use_angle,
            angle: s.angle,
            parent_velocity_as_direction: s.use_parent_velocity_for_spread,
            parent_velocity_direction_scale: s.parent_vel_spread_factor,
            speed_var_diagonal: s.use_special11_vec_for_speed_var,
            random_rotation: s.use_random_rot,
            inherit_velocity: s.add_parent_vel,
            inherit_velocity_factor: s.parent_vel_factor,
            context: Default::default(),
        })
    }

    fn projectile(&mut self, key: &'a str) -> Result<Projectile, LiftError> {
        let item = || format!("projectile {key:?}");
        let p = self.def.projectiles.get(key).ok_or_else(|| err(item(), "no such projectile"))?;
        if self.stack.contains(&key) {
            return Err(err(item(), "spawns one of its own ancestors"));
        }
        if self.base + self.stack.len() > MAX_INLINE_DEPTH {
            return Err(err(item(), format!("nested deeper than {MAX_INLINE_DEPTH}")));
        }
        self.emitted.insert(key);
        self.stack.push(key);
        let result = self.projectile_body(key, p);
        self.stack.pop();
        result
    }

    fn projectile_body(&mut self, key: &'a str, p: &'a rt::ProjectileDef) -> Result<Projectile, LiftError> {
        let item = || format!("projectile {key:?}");
        let spawns_any = p.hit.spawns_projectiles
            || p.player_hit.spawns_projectiles
            || p.timer.action.as_ref().is_some_and(|a| a.spawns_projectiles);
        if spawns_any && p.event_spawn.is_none() {
            return Err(err(item(), "an action spawns projectiles but there is no event spawn"));
        }
        let hit_terrain = action(&p.hit).map_err(|m| err(item(), format!("hit: {m}")))?;
        let hit_worm = action(&p.player_hit).map_err(|m| err(item(), format!("player_hit: {m}")))?;
        let zero = |v: f32| v.to_bits() == 0f32.to_bits();
        let timer = match (&p.timer.action, p.timer.time > 0.0) {
            (None, _) if zero(p.timer.time) && zero(p.timer.time_var) => None,
            (Some(a), true) => Some(Timer {
                after_s: p.timer.time,
                variation_s: p.timer.time_var,
                effects: action(a).map_err(|m| err(item(), format!("timer: {m}")))?,
            }),
            (None, false) => {
                Some(Timer { after_s: p.timer.time, variation_s: p.timer.time_var, effects: Vec::new() })
            }
            _ => return Err(err(item(), "a timer has an action exactly when its time is above 0")),
        };
        let mut spawns = BTreeMap::new();
        if let Some(s) = &p.event_spawn {
            spawns.insert(LIFTED_SPAWN.to_string(), self.spawn(s, &format!("{key}.spawn"))?);
        }
        let trail = match (p.trail.kind, &p.trail.spawn) {
            (rt::TrailKind::Projectile, Some(s)) => {
                Trail::Projectile { every_s: p.trail.delay, spawn: self.spawn(s, &format!("{key}.trail"))? }
            }
            (rt::TrailKind::Projectile, None) => {
                return Err(err(item(), "a projectile trail without a spawn"));
            }
            (kind, None) if p.trail.delay.to_bits() == 0.1f32.to_bits() => match kind {
                rt::TrailKind::None => Trail::Off,
                rt::TrailKind::Smoke => Trail::Smoke,
                rt::TrailKind::ChemSmoke => Trail::ChemSmoke,
                rt::TrailKind::Doomsday => Trail::Doomsday,
                rt::TrailKind::Explosive => Trail::Explosive,
                rt::TrailKind::Other(i) => Trail::Unknown(i),
                rt::TrailKind::Projectile => unreachable!("matched above"),
            },
            _ => return Err(err(item(), "only a projectile trail has a spawn or a delay other than 0.1")),
        };
        let gfx = match &p.gfx {
            rt::ProjectileGfx::Pixel { colours } => Gfx::Pixels(colours.iter().map(|c| Colour(*c)).collect()),
            rt::ProjectileGfx::Image {
                file,
                rotating,
                rot_increment,
                rot_speed,
                use_angle,
                use_spec_angle,
                angle_images,
                animation,
            } => Gfx::Image {
                file: file.clone(),
                rotating: *rotating,
                rotation_step_deg: *rot_increment,
                rotation_speed: *rot_speed,
                frames_follow_heading: *use_angle,
                frames_follow_heading_mirrored: *use_spec_angle,
                heading_frames: *angle_images,
                animation: animation.map(|a| Animation {
                    fps: a.rate,
                    mode: match a.kind {
                        rt::AnimKind::Once => AnimMode::Once,
                        rt::AnimKind::Loop => AnimMode::Loop,
                        rt::AnimKind::PingPong => AnimMode::PingPong,
                        rt::AnimKind::Other(i) => AnimMode::Unknown(i),
                    },
                }),
            },
        };
        let default_size = match gfx {
            Gfx::Pixels(_) => (2, 2),
            Gfx::Image { .. } => (4, 4),
        };
        Ok(Projectile {
            id: None,
            notes: String::new(),
            size: ((p.width, p.height) != default_size).then_some((p.width, p.height)),
            gfx,
            gravity: p.gravity,
            velocity_factor: p.dampening,
            timer,
            trail,
            hit_terrain,
            hit_worm,
            spawns,
        })
    }
}

/// An `ActionDef` as an effect list; `Err` names a field its kind does not use
/// that holds a value.
fn action(a: &rt::ActionDef) -> Result<Vec<Effect>, String> {
    use rt::ActionKind as K;
    let (damage, shake, sound, bounce) = match a.kind {
        K::Explode => (true, true, true, false),
        K::Bounce => (false, false, false, true),
        K::Injure | K::InjureWorm | K::Carve => (true, false, false, false),
        K::Dirt | K::GreenDirt => (false, true, false, false),
        K::PlaySound => (false, false, true, false),
        _ => (false, false, false, false),
    };
    let d = rt::ActionDef::olx_default(a.kind);
    let unused = |field: &str| Err(format!("{:?} never uses `{field}`, which is set", a.kind));
    if !damage && a.damage != d.damage {
        return unused("damage");
    }
    if !shake && a.shake != d.shake {
        return unused("shake");
    }
    if !sound && a.sound != d.sound {
        return unused("sound");
    }
    if !bounce
        && (a.bounce_coeff.to_bits() != d.bounce_coeff.to_bits() || a.bounce_explode != d.bounce_explode)
    {
        return unused("bounce_coeff or bounce_explode");
    }
    let effect = match a.kind {
        K::Nothing => None,
        K::Explode => Some(Effect::Explode { damage: a.damage, shake: a.shake, sound: a.sound.clone() }),
        K::Bounce => Some(Effect::Bounce { factor: a.bounce_coeff, explode: a.bounce_explode }),
        K::Injure => Some(Effect::Injure { damage: a.damage }),
        K::InjureWorm => Some(Effect::InjureWorm { damage: a.damage }),
        K::Carve => Some(Effect::Carve { size: a.damage }),
        K::Dirt => Some(Effect::Dirt { shake: a.shake }),
        K::GreenDirt => Some(Effect::GreenDirt { shake: a.shake }),
        K::PlaySound => Some(Effect::Sound { file: a.sound.clone() }),
        K::Disappear => Some(Effect::Disappear),
        K::Disappear2 => Some(Effect::Remove),
        K::GoThrough => Some(Effect::GoThrough),
        K::InjureProj => Some(Effect::InjureProjectile),
        K::ChangeRadius => Some(Effect::ChangeRadius),
        K::OverwriteOwnSpeed => Some(Effect::SetSpeed),
        K::MultiplyOwnSpeed => Some(Effect::MultiplySpeed),
        K::DiffOwnSpeed => Some(Effect::AddSpeed),
        K::OverwriteTargetSpeed => Some(Effect::SetTargetSpeed),
        K::MultiplyTargetSpeed => Some(Effect::MultiplyTargetSpeed),
        K::DiffTargetSpeed => Some(Effect::AddTargetSpeed),
        K::HeadingToNextWorm => Some(Effect::HomeOnNearestWorm),
        K::HeadingToOwner => Some(Effect::HomeOnOwner),
        K::HeadingToNextOtherWorm => Some(Effect::HomeOnNearestOtherWorm),
        K::HeadingToNextEnemyWorm => Some(Effect::HomeOnNearestEnemy),
        K::HeadingToNextTeamMate => Some(Effect::HomeOnNearestTeamMate),
        K::HeadTargetToUs => Some(Effect::PullTargetsToUs),
        K::Other(i) => Some(Effect::Unknown(i)),
    };
    let mut v: Vec<Effect> = effect.into_iter().collect();
    if a.spawns_projectiles {
        v.push(Effect::Spawn(LIFTED_SPAWN.into()));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::compile_mod;
    use olr_data::image::Rgba;
    use olr_data::mod_def::*;
    use olr_data::mod_def::{AnimKind, SpecialKind, WeaponClass};

    /// Like `olr-pack`'s sample: a rocket spawning bits, and bits shared by
    /// a second weapon.
    fn sample() -> ModDef {
        let bits = ProjectileDef {
            gfx: ProjectileGfx::Pixel { colours: vec![Rgba::rgb(200, 0, 0)] },
            gravity: None,
            dampening: 1.0,
            width: 2,
            height: 2,
            timer: TimerDef { time: 0.0, time_var: 0.0, action: None },
            trail: TrailDef { kind: TrailKind::None, delay: 0.1, spawn: None },
            hit: ActionDef::olx_default(ActionKind::Bounce),
            player_hit: ActionDef { damage: 3, ..ActionDef::olx_default(ActionKind::Injure) },
            event_spawn: None,
        };
        let rocket = ProjectileDef {
            gfx: ProjectileGfx::Image {
                file: "gfx/missile.png".into(),
                rotating: false,
                rot_increment: 0,
                rot_speed: 0,
                use_angle: true,
                use_spec_angle: false,
                angle_images: 16,
                animation: None,
            },
            width: 4,
            height: 4,
            hit: ActionDef {
                damage: 10,
                spawns_projectiles: true,
                sound: Some("sfx/boom.ogg".into()),
                ..ActionDef::olx_default(ActionKind::Explode)
            },
            event_spawn: Some(SpawnDef { amount: 5, ..SpawnDef::olx_default("bits".into()) }),
            ..bits.clone()
        };
        let weapon = |id: &str, proj: &str| WeaponDef {
            id: id.into(),
            name: id.to_uppercase(),
            recharge: 0.5,
            drain: 30.0,
            rof: 0.25,
            laser_sight: false,
            kind: WeaponKind::Projectile {
                class: WeaponClass::Missile,
                recoil: 5,
                sound: None,
                spawn: SpawnDef {
                    amount: 1,
                    speed: 100,
                    use_angle: true,
                    use_random_rot: true,
                    add_parent_vel: true,
                    ..SpawnDef::olx_default(proj.into())
                },
            },
        };
        ModDef {
            name: "Test".into(),
            worm: WormParams {
                angle_speed: 150.0,
                ground_speed: 8.0,
                air_speed: 4.0,
                gravity: 100.0,
                jump_force: -65.0,
                air_friction: 0.0,
                ground_friction: 0.4,
            },
            rope: RopeParams { max_length: 300, rest_length: 27, strength: 4.5 },
            weapons: vec![weapon("rocket", "rocket"), weapon("pellet", "bits")],
            projectiles: [("bits".to_string(), bits), ("rocket".to_string(), rocket)].into_iter().collect(),
            restrictions: Default::default(),
        }
    }

    #[test]
    fn lift_then_compile_is_the_identity() {
        let def = sample();
        let src = lift_mod(&def).unwrap();
        assert_eq!(compile_mod(&src).unwrap(), def);
    }

    #[test]
    fn single_references_inline_and_multiple_ones_share() {
        let src = lift_mod(&sample()).unwrap();
        assert_eq!(src.shared.keys().collect::<Vec<_>>(), ["bits"]);
        let Fire::Projectile { shot, .. } = &src.weapons[0].1.fire else { panic!() };
        let rocket = shot.projectile.as_ref().unwrap();
        assert_eq!(rocket.id, None, "\"rocket\" is the derived key");
        assert_eq!(rocket.spawns[LIFTED_SPAWN].shared.as_deref(), Some("bits"));
        assert_eq!(
            rocket.hit_terrain,
            vec![
                Effect::Explode { damage: 10, shake: 0, sound: Some("sfx/boom.ogg".into()) },
                Effect::Spawn(LIFTED_SPAWN.into()),
            ]
        );
        assert_eq!(rocket.size, None);
    }

    #[test]
    fn an_unused_action_field_is_a_lift_error() {
        let mut def = sample();
        def.projectiles.get_mut("bits").unwrap().hit.damage = 7; // Bounce does not use damage
        let e = lift_mod(&def).unwrap_err();
        assert!(e.to_string().contains("damage"), "{e}");
    }

    #[test]
    fn a_missing_key_is_a_lift_error() {
        let mut def = sample();
        def.projectiles.remove("bits");
        let e = lift_mod(&def).unwrap_err();
        assert!(e.to_string().contains("no such projectile"), "{e}");
    }

    #[test]
    fn a_cycle_of_single_references_is_a_lift_error() {
        let mut def = sample();
        let mut a = def.projectiles["bits"].clone();
        a.event_spawn = Some(SpawnDef::olx_default("b".into()));
        let mut b = def.projectiles["bits"].clone();
        b.event_spawn = Some(SpawnDef::olx_default("a".into()));
        def.projectiles.insert("a".into(), a);
        def.projectiles.insert("b".into(), b);
        let e = lift_mod(&def).unwrap_err();
        assert!(e.to_string().contains("in a cycle"), "{e}");
    }

    fn weapon_with(def: &mut ModDef, projectile: &str) {
        let mut w = def.weapons[0].clone();
        w.id = "deep".into();
        if let WeaponKind::Projectile { spawn, .. } = &mut w.kind {
            spawn.projectile = projectile.into();
        }
        def.weapons = vec![w];
    }

    /// A chain `c0 -> c1 -> ... -> c<n-1>` of single references under one weapon.
    fn chain(n: usize) -> ModDef {
        let mut def = sample();
        let leaf = def.projectiles["bits"].clone();
        def.projectiles.clear();
        for i in 0..n {
            let mut p = leaf.clone();
            if i + 1 < n {
                p.event_spawn = Some(SpawnDef::olx_default(format!("c{}", i + 1)));
            }
            def.projectiles.insert(format!("c{i}"), p);
        }
        weapon_with(&mut def, "c0");
        def
    }

    #[test]
    fn the_depth_limit_matches_compiles() {
        // A weapon-rooted projectile is compile depth 1; the limit is MAX.
        let ok = chain(MAX_INLINE_DEPTH);
        assert_eq!(compile_mod(&lift_mod(&ok).unwrap()).unwrap(), ok);
        let deep = chain(MAX_INLINE_DEPTH + 1);
        let e = lift_mod(&deep).unwrap_err();
        assert!(e.to_string().contains("nested deeper"), "{e}");
        // The same source compile gets without the lift's check is rejected too.
        let mut src = lift_mod(&ok).unwrap();
        let mut leaf = Projectile { id: None, ..src_leaf(&src) };
        leaf.spawns.clear();
        let Fire::Projectile { shot, .. } = &mut src.weapons[0].1.fire else { panic!() };
        let mut cur = shot.projectile.as_mut().unwrap();
        while cur.spawns.contains_key(LIFTED_SPAWN) {
            cur = cur.spawns.get_mut(LIFTED_SPAWN).unwrap().projectile.as_mut().unwrap();
        }
        cur.spawns
            .insert(LIFTED_SPAWN.into(), Spawn { projectile: Some(Box::new(leaf)), ..Spawn::default() });
        assert!(compile_mod(&src).is_err());
    }

    fn src_leaf(src: &ModSource) -> Projectile {
        let Fire::Projectile { shot, .. } = &src.weapons[0].1.fire else { panic!() };
        let mut cur = shot.projectile.as_ref().unwrap();
        while let Some(s) = cur.spawns.get(LIFTED_SPAWN) {
            cur = s.projectile.as_ref().unwrap();
        }
        (**cur).clone()
    }

    fn round_trip(def: &ModDef) -> ModSource {
        let src = lift_mod(def).unwrap();
        assert_eq!(&compile_mod(&src).unwrap(), def);
        src
    }

    #[test]
    fn a_key_that_is_not_the_derived_one_is_written_as_id() {
        let mut def = sample();
        let r = def.projectiles.remove("rocket").unwrap();
        def.projectiles.insert("missile".into(), r);
        weapon_with(&mut def, "missile");
        let src = round_trip(&def);
        let Fire::Projectile { shot, .. } = &src.weapons[0].1.fire else { panic!() };
        assert_eq!(shot.projectile.as_ref().unwrap().id.as_deref(), Some("missile"));
    }

    #[test]
    fn a_projectile_trail_lifts_with_its_interval() {
        let mut def = sample();
        let mut trailer = def.projectiles["bits"].clone();
        trailer.trail = TrailDef {
            kind: TrailKind::Projectile,
            delay: 0.3,
            spawn: Some(SpawnDef::olx_default("puff".into())),
        };
        def.projectiles.insert("trailer".into(), trailer);
        let mut puff = def.projectiles["bits"].clone();
        puff.gfx = ProjectileGfx::Pixel { colours: vec![Rgba::rgb(1, 2, 3)] };
        def.projectiles.insert("puff".into(), puff);
        weapon_with(&mut def, "trailer");
        def.projectiles.remove("rocket");
        let src = round_trip(&def);
        let Fire::Projectile { shot, .. } = &src.weapons[0].1.fire else { panic!() };
        let Trail::Projectile { every_s, spawn } = &shot.projectile.as_ref().unwrap().trail else { panic!() };
        assert_eq!(*every_s, 0.3);
        assert_eq!(
            spawn.projectile.as_ref().unwrap().id.as_deref(),
            Some("puff"),
            "the derived key is trailer.trail"
        );
    }

    #[test]
    fn timers_with_and_without_an_action_round_trip() {
        let mut def = sample();
        let bits = def.projectiles.get_mut("bits").unwrap();
        bits.timer = TimerDef {
            time: 1.5,
            time_var: 0.25,
            action: Some(ActionDef { damage: 4, ..ActionDef::olx_default(ActionKind::Injure) }),
        };
        let src = round_trip(&def);
        assert!(
            src.shared["bits"]
                .timer
                .as_ref()
                .is_some_and(|t| t.effects == vec![Effect::Injure { damage: 4 }])
        );
        let bits = def.projectiles.get_mut("bits").unwrap();
        bits.timer = TimerDef { time: 0.0, time_var: 0.5, action: None };
        let src = round_trip(&def);
        let t = src.shared["bits"].timer.as_ref().unwrap();
        assert!(t.effects.is_empty() && t.variation_s == 0.5);
    }

    #[test]
    fn a_player_hit_spawn_lists_the_spawn_effect() {
        let mut def = sample();
        let rocket = def.projectiles.get_mut("rocket").unwrap();
        rocket.hit = ActionDef::olx_default(ActionKind::Bounce);
        rocket.player_hit =
            ActionDef { damage: 2, spawns_projectiles: true, ..ActionDef::olx_default(ActionKind::Injure) };
        let src = round_trip(&def);
        let Fire::Projectile { shot, .. } = &src.weapons[0].1.fire else { panic!() };
        assert_eq!(
            shot.projectile.as_ref().unwrap().hit_worm,
            vec![Effect::Injure { damage: 2 }, Effect::Spawn(LIFTED_SPAWN.into())]
        );
    }

    #[test]
    fn beam_and_special_weapons_round_trip() {
        let mut def = sample();
        let base = def.weapons[0].clone();
        def.weapons.push(WeaponDef {
            id: "beam".into(),
            kind: WeaponKind::Beam {
                recoil: 3,
                beam: BeamDef {
                    colour: Rgba::rgb(9, 8, 7),
                    damage: 5,
                    player_damage: 6,
                    length: 70,
                    init_width: 2,
                    width_increase: 1.0,
                    distribute_damage_over_width: true,
                },
            },
            ..base.clone()
        });
        def.weapons.push(WeaponDef {
            id: "jet".into(),
            kind: WeaponKind::Special { special: SpecialKind::Jetpack, thrust: 7 },
            ..base
        });
        let src = round_trip(&def);
        assert!(matches!(src.weapons[2].1.fire, Fire::Beam { .. }));
        assert!(matches!(src.weapons[3].1.fire, Fire::Special { .. }));
    }

    #[test]
    fn an_animated_image_round_trips() {
        let mut def = sample();
        let rocket = def.projectiles.get_mut("rocket").unwrap();
        if let ProjectileGfx::Image { animation, .. } = &mut rocket.gfx {
            *animation = Some(olr_data::mod_def::Animation { rate: 12.0, kind: AnimKind::PingPong });
        }
        let src = round_trip(&def);
        let Fire::Projectile { shot, .. } = &src.weapons[0].1.fire else { panic!() };
        let Gfx::Image { animation, .. } = &shot.projectile.as_ref().unwrap().gfx else { panic!() };
        assert_eq!(animation.as_ref().unwrap().mode, AnimMode::PingPong);
    }
}
