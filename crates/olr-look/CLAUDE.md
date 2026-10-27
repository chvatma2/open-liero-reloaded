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
  `GlowColour::Own | Tint | Filtered` (the background glass design: the colour × each pixel's art scaled to a full brightest channel; `FrameGroup::filtered` with `own_colour`, `a_filtered_group_casts_its_tint_by_the_art`), intensity, reach, animation, phase),
  `LevelLook`,
  `Occlusion` (issue #10: `strength` percent, `reach` world pixels),
  `ProjectileLook` (`glow`, `light`, `end_flash`, `explosion`, issue #42's
  `explosion_heat` and `heat`), `WeaponLook`
  (`muzzle_light`; issue #69, `beam`: `BeamLook { light, tip_light,
  heat: Option<BeamHeat { amount, radius }> }`, `BeamHeat::part(parts)`
  one of a wide beam's parts' deposit, clamped to the projectile heat's
  caps, `None` under a degree), `ModLook` (indexed like `SimMod::projectiles` and
  `SimMod::weapons`, plus the mod-wide `explosion` and, issue #42,
  `explosion_heat`; `is_empty`;
  `explosion(proj)`: projectile `proj`'s own, else the mod's, `None` for
  -1 or neither, issue #15; `explosion_heat(proj)` the same for heat and
  `heat(proj)` a projectile's own, issue #42; `beam(weapon)` a weapon's
  own beam look and `beam_heat(weapon)` its heat, else `engine::BEAM_HEAT`,
  issue #69), `ExplosionHeatLook` (issue
  #42: `amount` percent of the engine's, `radius` world pixels or `None`
  for the engine's; `heat(damage)`: `engine::explosion_heat(damage)`
  scaled, amount clamped to `MAX_EXPLOSION_HEAT_AMOUNT` and the result to
  `MAX_TEMPERATURE`, radius to 1-`MAX_EXPLOSION_HEAT_RADIUS`; `None` under
  a degree or without the engine's), `ProjectileHeat` (issue #42: `amount`
  a tick, `radius`; `tick()` the deposit, clamped to 1-`MAX_PROJECTILE_HEAT`
  and 1-`MAX_PROJECTILE_HEAT_RADIUS`), `ExplosionLight` (issue #15: a `Light`, the
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
  each region on its own) and clears the emitters and `step` (`None`;
  `step` is the world state the frame shows, `Some` from
  `olr-present`'s presentation: frames of one step share the renderer's
  glow light, the glow light once a tick); it keeps `mode`
  (`ViewMode`: `Lit`, `FullBright`, `LightOnly`, the editor's toggle) and
  the vectors' capacity. It clamps `max_light` to 1.0-2.0, `softness` to 4,
  the `haze` (the directional-glow design: percent, 0 without a look) to 100
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
- **Identities:** a group's is `identity_of(id)` (FNV-1a 64); a live
  copy's group's (issue #87) `live_identity(copy, group)`, `identity_of`
  of `"<copy id>/<group id>"` made without a string.
- **Live copies' groups** (issue #87, the design's call 13):
  `LookFrame::push_live_group(&GlowGroup, copy, t_ms)` appends a copy's
  group after those so far, made as `begin` makes the level's
  (`push_group`) with its live identity, so two copies of one asset
  flicker and pulse apart; past the 255th (no group map value names it)
  it is left out. `olr-present` pushes every copy's groups after
  `begin`, copies in level order, each in its group order: copy `c`'s
  group `g` is value `n + offset(c) + g`. A level without live copies
  builds exactly the frame it did. Tests (`frame.rs`):
  `each_live_copy_has_its_own_groups`,
  `two_copies_of_one_asset_flicker_apart`,
  `a_level_without_live_copies_builds_todays_frame`.
  Issue #93: `push_live_group_with(g, copy, t_ms, GroupValues)` lays the
  copy's current values over the group, as `olr_edit`'s `posed` does for a
  static copy: `colour` (`0xRRGGBB`) casts as `Tint` (a `Filtered` group
  stays `Filtered` in it; negative: the group's own), `intensity` (clamped
  0-`MAX_GROUP_INTENSITY` 1000) and `reach` (1-`MAX_GROUP_REACH` 4096, only
  over a `Pixels` reach) replace the group's, `animation` replaces its
  animation at the copy's clock and phase (the live identity's, so a switch
  never brings two copies into step); `None` keeps the group's. Tests:
  `a_copys_values_override_its_groups`, `a_colour_value_tints_its_group`,
  `an_animation_switch_keeps_the_copys_clock`,
  `two_copies_of_one_asset_change_colour_independently`.
  `LookFrame::live_beams` (issue #93) are the live copies' beams this
  frame, placed (`push_live_beam`, at most `MAX_GLOW_BEAMS`; `begin`
  clears them): the renderer packs them after the level's.
- **Engine** (`engine.rs`, R13): code, not data, acting only in a match
  with a look. `FLOOR` (50 %, the least self-light of projectiles and
  bonuses in a lit match), `WORM_FLOOR` (35 %) and `worm_floor_of(ambient)`
  (plan 15b: the worms' least light, linear: the ambient hue at full
  brightness × 35 %, black → white, at least half of 35 % white's
  luminance), `explosion_light(size)` (warm; size clamped
  to 0-500, intensity `60 + 8 × size` up to 250, reach `24 + 8 × size`;
  keys over `EXPLOSION_MS`, 200 ms, which the presentation fits to the
  explosion's life), `muzzle_light()` (80 %, reach 40, 50 ms). Both are
  once-only `Keys` from 100 to 0 with phase 0. Issue #69 (picked from
  rendered frames): `beam_light(colour)` (each disc along a beam: its
  colour at 21 %, reach 40, steady), `beam_tip_light(colour)` (linear ×
  2.8 + 0.2 a channel, normalised into colour and intensity, reach 90),
  `BEAM_HEAT` (60 over 5 px a sub-step), `BEAM_DISC_SPACING` 8,
  `MAX_BEAM_DISCS` 64, `BEAM_DISC_RADIUS` 1.5, `BEAM_TIP_RADIUS` 3,
  `MAX_BEAM_REACH` 256 (`tests/beam.rs`). Plan 15c:
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
  animation. Issue #40: `explosion_heat(damage)` (`ExplosionHeat { radius,
  amount }`, the renderer's deposit: `amount` `1.2 × damage⁴` degrees up to
  `MAX_TEMPERATURE`, `radius` twice OLX's hole radius for the damage
  (`HOLE_RADII`, the dirt theme's 2.5-7.5 px for hole sizes 0-4) plus half
  the damage, up to `MAX_EXPLOSION_HEAT_RADIUS` 48; `None` for no damage
  or NaN): a minigun hit (3) under the lava ramp's first key at its rim, a
  Bazooka's (4) rim glowing a few pixels deep, a Big Nuke's fragments
  (15) far more. Issue #42, a mod's heat's caps: `MAX_EXPLOSION_HEAT_AMOUNT`
  (1000 %), `MAX_EXPLOSION_HEAT_RADIUS` (48, the engine's own widest),
  `MAX_PROJECTILE_HEAT` (100 degrees a tick) and
  `MAX_PROJECTILE_HEAT_RADIUS` (16). Issue #41 (spec's "Distant layers: the ball"):
  `HEAT_DEPTH_PX` (`K`, 1.1) and `heat_slice(heat, depth)`: the ball's
  circle at `z = layer_z(depth)`, radius `sqrt(R² − z²)`, amount `× (1 − z² / R²)`;
  `None` when `z ≥ R` or a value is not a positive number. Issue #65,
  heat on one depth line: `BACK_DEPTH_PX` (14: the level's back wall lies
  that far behind the plane, so a minigun's hit (12.5) leaves it cold and
  a Bazooka's (17) warms a circle of radius 9.6, a projectile's heat at
  its cap (16) one of 7.7) and `layer_z(depth)` (`14 + depth × K`: a Big
  Nuke's fragments reach depth 7, a Bazooka's depth 2, a mod's widest
  (48) depth 30, the renderer's largest radius (64) depth 45).
  Issue #54, the worm halo (the renderer's composite darkens the
  background round a living worm): `WORM_HALO_RADIUS` (2 world pixels
  past the silhouette on the level's back), `WORM_HALO_RADIUS_MAX` (5, at
  the fade depth), `WORM_HALO_STRENGTH` (0.5, the light lost right next
  to the worm on the back), `WORM_HALO_FADE` (0.6: no halo at that depth
  or deeper); `worm_halo_shape(depth)` (radius and strength, linear in
  the depth; `None` from the fade depth less half a byte's step) and
  `worm_halo(d, depth)` (`s × (1 − (d − 1)/r)²`, 0 past `r`).
  The background glass design: `filtered(c)` (a `Filtered` group's own
  colour: linear art scaled to a full brightest channel, black or no
  number none; the renderer's emission and the thumbnail use it,
  `filtered_scales_the_art_to_a_full_brightest_channel`).
- **Surface materials** (`material.rs`, issue #37; spec
  `docs/superpowers/specs/2026-10-22-materials-and-heat-design.md`):
  `SurfaceMaterial { id, heat: Option<Heat>, light_soak: Option<u8> }`
  (`heats()`: a sensitivity above 0; issue #57: `light_soak()` its own
  soak clamped to `MAX_SOAK`, `None` the level's), `Heat { sensitivity, capacity, conductivity, cooling, ramp,
  light }` (percent of stone's 100; each accessor clamps to the limits:
  `MAX_SENSITIVITY` 100, `MIN_CAPACITY`-`MAX_CAPACITY` 10-1000,
  `MAX_CONDUCTIVITY` 1000, `MIN_COOLING`-`MAX_COOLING` 1-1000; `ramp()` is
  its first `MAX_RAMP_KEYS` 16 keys, `LAVA_RAMP` when empty), `RampKey {
  temperature, colour, glow }` (temperatures 0-`MAX_TEMPERATURE` 1000,
  glow 0-`MAX_GLOW` 100), `ramp_at(ramp, t)` (integer linear, `None`
  below the first key, the last above it; unsorted keys never panic) and
  `SurfaceMaps { world, front, back }` (a value per world pixel, `v` the
  look's `materials[v - 1]`, 0 none; `fits`, `is_empty`, `highest`).
  `LevelLook::materials` (at most `MAX_MATERIALS` 255) and
  `LevelLook::material(v)` (`None` for 0 or past the list). The renderer
  heats them (issue #39); explosions deposit the engine's heat (issue
  #40, `olr-present`), or a mod's (issue #42).
- **Tests** (`tests/`, one binary): `anim.rs` (pulse shapes at their
  quarter points, keys interpolating, carrying colour and ending, flicker
  deterministic and bounded, phases, `hostile_animations_never_panic`) and
  `frame.rs` (the fade profile, `begin` with and without a level look, an
  animated group, `light_at`, `engine_numbers_match_r13`,
  `skin_light_is_steady_and_capped`,
  `explosion_lights_are_capped_and_fall_back_to_the_mods` (issue #15),
  `explosion_heat_grows_with_the_damage_and_is_bounded` (issue #40),
  `mod_heat_scales_the_engines_capped_and_falls_back_to_the_mods` (issue #42),
  `distant_layers_take_the_balls_slice` (issue #41),
  `the_worm_halo_fades_out_and_back` (issue #54),
  `worm_floor_hues`, `solids_settings_are_bounded_and_off_without_a_look`
  (the occlusion too, issue #10);
  plan 15c: `disc_calibration_matches_todays_light`,
  `emitters_past_the_cap_are_dropped`, `bounce_is_the_engines_in_a_lit_level`,
  `a_flicker_group_flickers_per_region`).
  Issue #37: `material.rs` (the lava ramp's rise, interpolation, hostile
  values clamped, maps).

Test: `cargo nextest run -p olr-look`.
- **Glow beams** (`glow_beams.rs`, directional-glow plan Task 2):
  `derive(front, back)` turns the baked beam maps (passed as slices in a
  `BeamLayer`; this crate does not depend on `olr-terrain`) into
  `GlowBeamSource`s: 8-connected patches of one group, direction and spread,
  front layer first, each in row-major order of its first pixel. A patch's
  `at` is the middle of its face (the corners' furthest projection on `dir`),
  `half` half its width, `glow` and `colour` (linear, brightest channel 1)
  the means over its art blocks. A misfit layer makes none. The cap
  `MAX_GLOW_BEAMS` (64) is applied by the callers: `olr-format`'s `check`
  counts derived patches and the lenient read drops a level's beams.
  Issue #93, live beams: `live_patches(layer, back)` derives a live part's
  picture as `derive` does, each `LivePatch` keeping its own direction
  (whole degrees from the map's 2° steps), spread, first pixel and rows
  (`(y, x0, x1)`, the row ends whose corners make every face);
  `LivePatch::place(picture width, &PatchPlace)` places it at the part's
  top-left, mirrored within the picture (`flip`, the direction too:
  `mirrored_direction`, `(180 − d) mod 360`), its direction and spread the
  copy's values (whole degrees, 0-359 and 1-90) or its own, its face and
  width worked out for that direction as `derive` does, its glow × the
  part's strength, its group the run-time value (none for 0), `first` its
  first pixel's world index (0 outside the world)
  (`live_patches_place_turn_and_mirror_as_derived`).
