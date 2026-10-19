# olr-look

The look's runtime data (spec `docs/superpowers/specs/2026-10-18-look-system-design.md`,
plan 15a): what a level's and a mod's look compile to, animation sampling,
the per-frame `LookFrame` the renderer's look passes read, and the engine's
own look. The simulation never sees it.

- **Layer 1, `#![no_std]` + `alloc`, wasm.** Depends only on `libm` (plan
  15a Task 1 ruling: `olr-math` exports no `powf`/`floor`/`sqrtf`).
- **Types** (`lib.rs`): `Rgb` (sRGB bytes, as files and art hold them),
  `Light` (colour, intensity percent 0-1000, `Reach`, `Animation`,
  `phase_ms`), `Reach` (`Pixels(u16)` or `Unlimited`; `pixels()` is
  `f32::INFINITY` for unlimited), `PulseShape`, `Key`, `GlowGroup` (id,
  `GlowColour::Own | Tint`, intensity, reach, animation, phase),
  `PlacedLight` (world pixel `at`, `radius` 1-16, a `Light`), `LevelLook`,
  `ProjectileLook` (`glow`, `light`, `end_flash`), `WeaponLook`
  (`muzzle_light`), `ModLook` (indexed like `SimMod::projectiles` and
  `SimMod::weapons`; `is_empty`), `Grade` (exposure in 1/100 stop, contrast
  and saturation percent, tint; `Default` is neutral), `Ambient`.
  `olr-format` compiles the authoring files into them and checks every
  bound; this crate clamps again where a value is read, so a hand-built
  value never misbehaves.
- **`LevelLook`** (plan 15a Tasks 14a-14b, the user's Stop 1 verdict):
  ambient, `max_light` (100-200 percent), `light_steps` (0 or 2-32 bands),
  `softness` (0-4 world pixels of blur), `light_soak` (world pixels light
  soaks into solids, 0-`MAX_SOAK`), `solid_floor` (an `Ambient`: the least
  light an opaque solid pixel shows), `shadows`, `grade`, `groups` and
  `lights`. A group map value `v` (1-255) is `groups[v - 1]`; 0 is no
  group (plan 15a batch A review).
- **`Shadows`** (the user's playtest, 2026-10-19): how placed lights'
  shadows meet the light: `Hard` (whole world pixels), `Smooth`
  (anti-aliased, four tests a world pixel) or `Soft` (a penumbra from the
  light's radius, wider farther behind the occluder), the default.
- **Animation** (`anim.rs`): `Animation::sample(t_ms, phase_ms, identity)`
  gives a `Sample` (a scale, 1.0 = 100 %, and maybe a colour). `Pulse`
  starts at its midpoint rising; `Flicker` is smooth value noise over buckets of
  `1000 / speed` ms seeded by the identity (`splitmix64`); `Keys` are linear, and a
  segment's colour is interpolated when both keys have one, else the
  earlier key's. A clock that is not finite samples as `Steady`; a zero
  period or speed reads as 1. A curve that does not loop `ended` past its
  last key; `loops()` is false only for such a curve. The default phase
  (`default_phase`, 0-65 535 ms from a hash of the identity) applies only
  to looping animations: a flash starts at its start (Task 1 ruling).
- **`LookFrame`** (`frame.rs`): `begin(level, t_ms)` fills ambient, grade,
  settings, groups (`FrameGroup`: cast colour, own colour, self-light scale,
  reach) and the level's lights; it keeps `mode` (`ViewMode`: `Lit`,
  `FullBright`, `LightOnly`, the editor's toggle) and the vectors'
  capacity. It clamps `max_light` to 1.0-2.0, `softness` to 4 and
  `light_soak` to `MAX_SOAK` (12); `solid_floor` is linear, and
  `shadow_filter` is `Shadows` as 0 hard, 1 smooth, 2 soft. Without a level
  look the frame is full ambient with no soak, floor, steps, blur or grade
  and hard shadows. A level light sits at its world pixel's centre (`at +
  0.5`, Task 1 ruling), its radius clamped to 1-16, its identity its index.
  `push` adds the dynamic lights in plan 15a R9's order (the callers');
  `cull(view)` keeps those whose reach plus radius touches the view, then
  the first `MAX_LIGHTS` (256), in order. Colours are linear
  (`srgb_to_linear`). `light_at(light, t_ms, identity, pos, radius)` is one
  light at its own clock as a `FrameLight`: `None` once it has ended, when
  it gives no light, or when a value is not finite; intensity is capped at
  1000. `fade(d, reach)` is `(1 - (d/r)²)²`, nothing at or past the reach,
  1 for `Unlimited`, 0 for `Pixels(0)` and a NaN distance.
- **Identities:** a group's is `identity_of(id)` (FNV-1a 64), a level
  light's its index.
- **Engine** (`engine.rs`, R13): code, not data, acting only in a match
  with a look. `FLOOR` (50 %, the least self-light of worms, projectiles
  and bonuses in a lit match), `explosion_light(size)` (warm; size clamped
  to 0-500, intensity `60 + 8 × size` up to 250, reach `24 + 8 × size`;
  keys over `EXPLOSION_MS`, 200 ms, which the presentation fits to the
  explosion's life), `muzzle_light()` (80 %, reach 40, 50 ms). Both are
  once-only `Keys` from 100 to 0 with phase 0.
- **Tests** (`tests/`, one binary): `anim.rs` (pulse shapes at their
  quarter points, keys interpolating, carrying colour and ending, flicker
  deterministic and bounded, phases, `hostile_animations_never_panic`) and
  `frame.rs` (the fade profile, `begin` with and without a level look, an
  animated group, `cull`'s order and cap and a far light whose reach
  touches the view, `light_at`, `engine_numbers_match_r13`,
  `solids_settings_are_bounded_and_off_without_a_look`).

Test: `cargo nextest run -p olr-look`.
