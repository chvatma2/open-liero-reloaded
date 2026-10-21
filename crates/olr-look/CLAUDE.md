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
  `LevelLook`,
  `Occlusion` (issue #10: `strength` percent, `reach` world pixels),
  `ProjectileLook` (`glow`, `light`, `end_flash`, `explosion`), `WeaponLook`
  (`muzzle_light`), `ModLook` (indexed like `SimMod::projectiles` and
  `SimMod::weapons`, plus the mod-wide `explosion`; `is_empty`;
  `explosion(proj)`: projectile `proj`'s own, else the mod's, `None` for
  -1 or neither, issue #15), `ExplosionLight` (issue #15: a `Light`, the
  disc's `radius`, `fitted`: the engine's flash keys fitted to the
  explosion's life; `radius()` and `reach()` clamp to the caps), `Grade` (exposure in 1/100 stop, contrast
  and saturation percent, tint; `Default` is neutral), `Ambient`.
  `olr-format` compiles the authoring files into them and checks every
  bound; this crate clamps again where a value is read, so a hand-built
  value never misbehaves.
- **`LevelLook`** (plan 15a Tasks 14a-14b, the user's Stop 1 verdict):
  ambient, `occlusion` (issue #10: `Option<Occlusion>`, the ambient
  light's darkening in air near solids; `None` off), `max_light` (100-200 percent), `light_steps` (0 or 2-32 bands),
  `softness` (0-4 world pixels of blur), `light_soak` (world pixels light
  soaks into solids, 0-`MAX_SOAK`), `solid_floor` (an `Ambient`: the least
  light an opaque solid pixel shows), `grade` and `groups`: its glowing
  pixels are its only light (plan 15c: placed lights and `Shadows` went).
  A group map value `v` (1-255) is `groups[v - 1]`; 0 is no group (plan
  15a batch A review).
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
  settings, `bounce` (`engine::BOUNCE` with a look, 0 without; plan 15c)
  and groups (`FrameGroup`: cast colour, own colour, self-light scale,
  reach; plan 15c: a `Flicker` group's `flicker` (speed, depth), `t_ms`
  (its clock with its phase, wrapped at 2^24 ms) and `seed` (its identity
  folded), its cast and self-light then unscaled: the renderer flickers
  each region on its own) and clears the emitters; it keeps `mode`
  (`ViewMode`: `Lit`, `FullBright`, `LightOnly`, the editor's toggle) and
  the vectors' capacity. It clamps `max_light` to 1.0-2.0, `softness` to 4
  and `light_soak` to `MAX_SOAK` (12), and keeps the `occlusion` with a
  strength (at most 100), its reach clamped to `MIN_OCCLUSION_REACH` (2)
  to `MAX_OCCLUSION_REACH` (32; the renderer reads the solid map that far
  past its light grid), `None` without a look; `solid_floor` is linear,
  `worm_floor` is `worm_floor_of` the ambient colour as set (35 % white
  without a level look). Without a level look the frame is full ambient
  with no soak, floor, steps, blur, grade or bounce. `emitters` (plan 15c)
  are play's lights as glowing discs (`FrameLight`: position, the disc's
  radius, colour, reach); `push_emitter` adds them in plan 15a R9's order
  (the callers'), the first `MAX_EMITTERS` (1024), never culled to a view
  (bounced light may reach it from past a disc's reach). Colours are linear
  (`srgb_to_linear`). `light_at(light, t_ms, identity, pos, radius)` is one
  light at its own clock as a `FrameLight`: `None` once it has ended, when
  it gives no light, or when a value is not finite; intensity is capped at
  1000. `fade(d, reach)` is `(1 - (d/r)²)²`, nothing at or past the reach,
  1 for `Unlimited`, 0 for `Pixels(0)` and a NaN distance.
- **Identities:** a group's is `identity_of(id)` (FNV-1a 64).
- **Engine** (`engine.rs`, R13): code, not data, acting only in a match
  with a look. `FLOOR` (50 %, the least self-light of projectiles and
  bonuses in a lit match), `WORM_FLOOR` (35 %) and `worm_floor_of(ambient)`
  (plan 15b: the worms' least light, linear: the ambient hue at full
  brightness × 35 %, black → white, at least half of 35 % white's
  luminance), `explosion_light(size)` (warm; size clamped
  to 0-500, intensity `60 + 8 × size` up to 250, reach `24 + 8 × size`;
  keys over `EXPLOSION_MS`, 200 ms, which the presentation fits to the
  explosion's life), `muzzle_light()` (80 %, reach 40, 50 ms). Both are
  once-only `Keys` from 100 to 0 with phase 0. Plan 15c:
  `disc_radius(reach)` (an eighth of the reach, 1-6; 1 not finite),
  `explosion_radius(size)` (`2 + size / 2`, 2-12), `disc_radiance(colour,
  radius, reach)` (the calibration: a disc's light at `d0 = max(reach / 3,
  2r)`, at most halfway from its edge to the reach, equals the old point
  light's there; finite, at most 64 × the colour) and `BOUNCE` (0.6).
  Issue #12: `SKIN_LIGHT` (the one switch for skins' lights, engine-wide,
  `true`; off, no skin casts light, its glowing pixels still show bright),
  `MAX_SKIN_LIGHT_INTENSITY` (200 %) and `MAX_SKIN_LIGHT_REACH` (48 world
  pixels): the caps `olr-format` refuses past, and `skin_light(colour,
  intensity, reach)` (steady, phase 0) clamps to. Issue #15:
  `MAX_EXPLOSION_RADIUS` (16, the renderer's largest disc) and
  `MAX_EXPLOSION_REACH` (4096), a mod explosion light's caps, and
  `explosion_flash(colour, intensity, reach)` (the engine's flash keys
  over `EXPLOSION_MS`, capped): a mod's explosion light without an
  animation.
- **Tests** (`tests/`, one binary): `anim.rs` (pulse shapes at their
  quarter points, keys interpolating, carrying colour and ending, flicker
  deterministic and bounded, phases, `hostile_animations_never_panic`) and
  `frame.rs` (the fade profile, `begin` with and without a level look, an
  animated group, `light_at`, `engine_numbers_match_r13`,
  `skin_light_is_steady_and_capped`,
  `explosion_lights_are_capped_and_fall_back_to_the_mods` (issue #15),
  `worm_floor_hues`, `solids_settings_are_bounded_and_off_without_a_look`
  (the occlusion too, issue #10);
  plan 15c: `disc_calibration_matches_todays_light`,
  `emitters_past_the_cap_are_dropped`, `bounce_is_the_engines_in_a_lit_level`,
  `a_flicker_group_flickers_per_region`).

Test: `cargo nextest run -p olr-look`.
