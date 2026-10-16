//! Random authoring values: written, read back equal; written again, the
//! same bytes (spec §6, Review Focus 1).

use std::collections::BTreeMap;

use olr_data::image::Rgba;
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

fn effect() -> impl Strategy<Value = Effect> {
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

fn gfx() -> impl Strategy<Value = Gfx> {
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
        option::of(name()),
    )
        .prop_map(move |(amount, speed, speed_var, spread, b, angle, scale, factor, shared)| Spawn {
            shared,
            projectile: p.clone().map(Box::new),
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
        })
}

fn leaf() -> impl Strategy<Value = Projectile> {
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
    )
        .prop_map(
            |(id, notes, gfx, size, gravity, velocity_factor, timer, hit_terrain, hit_worm, trail)| {
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
                        _ => Trail::Unknown(7),
                    },
                    hit_terrain,
                    hit_worm,
                    spawns: BTreeMap::new(),
                }
            },
        )
}

fn projectile() -> impl Strategy<Value = Projectile> {
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

fn weapon() -> impl Strategy<Value = Weapon> {
    (text(), f32_any(), f32_any(), f32_any(), any::<bool>(), projectile(), 0..3u8, colour(), any::<u32>())
        .prop_flat_map(move |(name, recharge, drain, rof, laser, p, kind, colour, thrust)| {
            (spawn_fields::<ShotContext>(Some(p)), any::<i32>()).prop_map(move |(shot, n)| Weapon {
                name: name.clone(),
                notes: String::new(),
                recharge,
                drain,
                between_shots_s: rof,
                laser_sight: laser,
                fire: match kind {
                    0 => Fire::Projectile { class: WeaponClass::Unknown(n), recoil: n, sound: None, shot },
                    1 => Fire::Beam {
                        recoil: n,
                        colour,
                        hole_size: n,
                        worm_damage: n,
                        length: n,
                        width: n,
                        width_growth: drain,
                        split_damage_over_width: laser,
                    },
                    _ => Fire::Special { kind: SpecialKind::Jetpack, thrust },
                },
            })
        })
}

fn mod_source() -> impl Strategy<Value = ModSource> {
    (text(), f32_any(), any::<i32>(), btree_map(name(), weapon(), 1..3), btree_map(name(), leaf(), 0..2))
        .prop_map(|(name, gravity, rest, weapons, shared)| ModSource {
            name,
            notes: String::new(),
            worm: Worm { gravity, ..Worm::default() },
            rope: Rope { rest_length: rest, ..Rope::default() },
            weapons: weapons.into_iter().collect(),
            shared: shared
                .into_iter()
                .map(|(k, mut p)| {
                    p.id = None;
                    (k, p)
                })
                .collect(),
            restrictions: BTreeMap::new(),
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
