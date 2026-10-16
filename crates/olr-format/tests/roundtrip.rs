//! Random authoring values: written, read back equal; written again, the
//! same bytes (spec §6, Review Focus 1).

use std::collections::BTreeMap;

use olr_data::image::Rgba;
use olr_data::restrictions::{Preset, WeaponState};
use olr_format::Colour;
use olr_format::mods::*;
use olr_vfs::MemFs;
use proptest::collection::{btree_map, vec};
use proptest::option;
use proptest::prelude::*;

fn f32_any() -> impl Strategy<Value = f32> {
    prop_oneof![
        Just(0.0f32),
        Just(-0.0f32),
        Just(1e-40f32),
        Just(0.1f32),
        Just(0.5f32),
        any::<f32>().prop_filter("finite", |v| v.is_finite()),
    ]
}

fn name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{0,6}"
}

fn text() -> impl Strategy<Value = String> {
    "[ -~äöüé\"\\\\]{0,12}"
}

fn colour() -> impl Strategy<Value = Colour> {
    any::<[u8; 4]>().prop_map(|[r, g, b, a]| Colour(Rgba { r, g, b, a }))
}

fn effect() -> BoxedStrategy<Effect> {
    effect_inner().boxed()
}

fn effect_inner() -> impl Strategy<Value = Effect> {
    prop_oneof![
        (any::<i32>(), any::<i32>(), option::of(text())).prop_map(|(damage, shake, sound)| Effect::Explode {
            damage,
            shake,
            sound
        }),
        (f32_any(), any::<i32>()).prop_map(|(factor, explode)| Effect::Bounce { factor, explode }),
        any::<i32>().prop_map(|damage| Effect::Injure { damage }),
        any::<i32>().prop_map(|size| Effect::Carve { size }),
        any::<i32>().prop_map(|shake| Effect::GreenDirt { shake }),
        option::of(text()).prop_map(|file| Effect::Sound { file }),
        Just(Effect::Disappear),
        Just(Effect::Remove),
        Just(Effect::HomeOnOwner),
        any::<i32>().prop_map(Effect::Unknown),
        name().prop_map(Effect::Spawn),
    ]
}

fn gfx() -> BoxedStrategy<Gfx> {
    gfx_inner().boxed()
}

fn gfx_inner() -> impl Strategy<Value = Gfx> {
    prop_oneof![
        vec(colour(), 0..3).prop_map(Gfx::Pixels),
        (
            text(),
            any::<bool>(),
            any::<i32>(),
            any::<i32>(),
            any::<bool>(),
            any::<i32>(),
            option::of((f32_any(), 0..4i32))
        )
            .prop_map(|(file, rotating, step, speed, heading, frames, anim)| Gfx::Image {
                file,
                rotating,
                rotation_step_deg: step,
                rotation_speed: speed,
                frames_follow_heading: heading,
                frames_follow_heading_mirrored: !heading,
                heading_frames: frames,
                animation: anim.map(|(fps, m)| Animation {
                    fps,
                    mode: [AnimMode::Once, AnimMode::Loop, AnimMode::PingPong, AnimMode::Unknown(m)]
                        [m as usize],
                }),
            }),
    ]
}

fn spawn_fields<C: SpawnContext>(p: Option<Projectile>) -> impl Strategy<Value = Spawn<C>> {
    (
        any::<i32>(),
        any::<i32>(),
        f32_any(),
        f32_any(),
        any::<[bool; 6]>(),
        any::<i32>(),
        f32_any(),
        f32_any(),
        name(),
    )
        .prop_map(move |(amount, speed, speed_var, spread, b, angle, scale, factor, shared_name)| {
            // Exactly one of `shared` and `projectile`, both shapes.
            let (shared, projectile) =
                if b[5] { (Some(shared_name), None) } else { (None, p.clone().map(Box::new)) };
            Spawn {
                shared,
                projectile,
                amount,
                speed,
                speed_var,
                spread,
                relative_to_parent: b[0],
                angle,
                parent_velocity_as_direction: b[1],
                parent_velocity_direction_scale: scale,
                speed_var_diagonal: b[2],
                random_rotation: b[3],
                inherit_velocity: b[4],
                inherit_velocity_factor: factor,
                context: Default::default(),
            }
        })
}

fn leaf() -> BoxedStrategy<Projectile> {
    leaf_inner().boxed()
}

fn leaf_inner() -> impl Strategy<Value = Projectile> {
    (
        option::of(name()),
        text(),
        gfx(),
        option::of((any::<i32>(), any::<i32>())),
        option::of(any::<i32>()),
        f32_any(),
        option::of((f32_any(), f32_any(), vec(effect(), 0..3))),
        vec(effect(), 0..3),
        vec(effect(), 0..3),
        0..6u8,
        any::<i32>(),
    )
        .prop_map(
            |(
                id,
                notes,
                gfx,
                size,
                gravity,
                velocity_factor,
                timer,
                hit_terrain,
                hit_worm,
                trail,
                trail_unknown,
            )| {
                Projectile {
                    id,
                    notes,
                    gfx,
                    size,
                    gravity,
                    velocity_factor,
                    timer: timer.map(|(after_s, variation_s, effects)| Timer {
                        after_s,
                        variation_s,
                        effects,
                    }),
                    trail: match trail {
                        0 => Trail::Off,
                        1 => Trail::Smoke,
                        2 => Trail::ChemSmoke,
                        3 => Trail::Doomsday,
                        4 => Trail::Explosive,
                        _ => Trail::Unknown(trail_unknown),
                    },
                    hit_terrain,
                    hit_worm,
                    spawns: BTreeMap::new(),
                }
            },
        )
}

fn projectile() -> BoxedStrategy<Projectile> {
    projectile_inner().boxed()
}

fn projectile_inner() -> impl Strategy<Value = Projectile> {
    leaf().prop_recursive(3, 12, 2, |inner| {
        // `shape`: bit 0 adds an event spawn, bit 1 a projectile trail.
        (leaf(), inner.clone(), inner, name(), f32_any(), 0..4u8)
            .prop_flat_map(|(base, event_p, trail_p, spawn_name, every_s, shape)| {
                (
                    Just(base),
                    Just(spawn_name),
                    Just(every_s),
                    Just(shape),
                    spawn_fields::<EventContext>(Some(event_p)),
                    spawn_fields::<TrailContext>(Some(trail_p)),
                )
            })
            .prop_map(|(mut base, spawn_name, every_s, shape, ev, tr)| {
                if shape & 1 != 0 {
                    base.spawns.insert(spawn_name, ev);
                }
                if shape & 2 != 0 {
                    base.trail = Trail::Projectile { every_s, spawn: tr };
                }
                base
            })
    })
}

fn weapon_class() -> impl Strategy<Value = WeaponClass> {
    prop_oneof![
        Just(WeaponClass::Automatic),
        Just(WeaponClass::PowerGun),
        Just(WeaponClass::Grenade),
        Just(WeaponClass::Missile),
        Just(WeaponClass::CloseRange),
        any::<i32>().prop_map(WeaponClass::Unknown),
    ]
}

fn special_kind() -> impl Strategy<Value = SpecialKind> {
    prop_oneof![
        Just(SpecialKind::Off),
        Just(SpecialKind::Jetpack),
        any::<i32>().prop_map(SpecialKind::Unknown)
    ]
}

fn fire() -> BoxedStrategy<Fire> {
    fire_inner().boxed()
}

fn fire_inner() -> impl Strategy<Value = Fire> {
    let projectile_fire = (
        weapon_class(),
        any::<i32>(),
        option::of(text()),
        projectile().prop_flat_map(|p| spawn_fields::<ShotContext>(Some(p))),
    )
        .prop_map(|(class, recoil, sound, shot)| Fire::Projectile { class, recoil, sound, shot });
    let beam = (
        (any::<i32>(), colour(), any::<i32>(), any::<i32>()),
        (any::<i32>(), any::<i32>(), f32_any(), any::<bool>()),
    )
        .prop_map(
            |(
                (recoil, colour, hole_size, worm_damage),
                (length, width, width_growth, split_damage_over_width),
            )| {
                Fire::Beam {
                    recoil,
                    colour,
                    hole_size,
                    worm_damage,
                    length,
                    width,
                    width_growth,
                    split_damage_over_width,
                }
            },
        );
    let special = (special_kind(), any::<u32>()).prop_map(|(kind, thrust)| Fire::Special { kind, thrust });
    prop_oneof![projectile_fire, beam, special]
}

fn weapon() -> BoxedStrategy<Weapon> {
    weapon_inner().boxed()
}

fn weapon_inner() -> impl Strategy<Value = Weapon> {
    (text(), text(), f32_any(), f32_any(), f32_any(), any::<bool>(), fire()).prop_map(
        |(name, notes, recharge, drain, between_shots_s, laser_sight, fire)| Weapon {
            name,
            notes,
            recharge,
            drain,
            between_shots_s,
            laser_sight,
            fire,
        },
    )
}

fn worm() -> BoxedStrategy<Worm> {
    worm_inner().boxed()
}

fn worm_inner() -> impl Strategy<Value = Worm> {
    (f32_any(), f32_any(), f32_any(), f32_any(), f32_any(), f32_any(), f32_any()).prop_map(
        |(
            ground_acceleration,
            air_acceleration,
            gravity,
            jump_speed,
            air_drag,
            unused_angle_speed,
            unused_ground_friction,
        )| {
            Worm {
                ground_acceleration,
                air_acceleration,
                gravity,
                jump_speed,
                air_drag,
                unused_angle_speed,
                unused_ground_friction,
            }
        },
    )
}

fn rope() -> BoxedStrategy<Rope> {
    rope_inner().boxed()
}

fn rope_inner() -> impl Strategy<Value = Rope> {
    (any::<i32>(), any::<i32>(), f32_any()).prop_map(|(max_length, rest_length, strength)| Rope {
        max_length,
        rest_length,
        strength,
    })
}

fn restrictions() -> BoxedStrategy<BTreeMap<String, Preset>> {
    restrictions_inner().boxed()
}

fn restrictions_inner() -> impl Strategy<Value = BTreeMap<String, Preset>> {
    let state = prop_oneof![Just(WeaponState::Enabled), Just(WeaponState::Bonus), Just(WeaponState::Banned)];
    btree_map(name(), btree_map(text(), state, 0..4), 0..3)
}

fn mod_source() -> impl Strategy<Value = ModSource> {
    (
        (text(), text(), worm(), rope()),
        btree_map(name(), weapon(), 1..3),
        btree_map(name(), leaf(), 0..2),
        restrictions(),
    )
        .prop_map(|((name, notes, worm, rope), weapons, shared, restrictions)| ModSource {
            name,
            notes,
            worm,
            rope,
            weapons: weapons.into_iter().collect(),
            shared: shared
                .into_iter()
                .map(|(k, mut p)| {
                    p.id = None;
                    (k, p)
                })
                .collect(),
            restrictions,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn authoring_values_round_trip_canonically(src in mod_source()) {
        let mut a = MemFs::new();
        write_mod_files(&mut a, &src).unwrap();
        let back = read_mod_files(&a).unwrap();
        prop_assert_eq!(&back, &src);
        let mut b = MemFs::new();
        write_mod_files(&mut b, &back).unwrap();
        prop_assert_eq!(a, b);
    }
}
