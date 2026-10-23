# Mod pack fields: names and units (research for the formats spec §2, plan 13a)

Source of truth: `crates/olr-data/src/mod_def.rs` (runtime types), `crates/olr-sim/src`
(what the simulation does), `crates/olr-import-lgs/src/parse.rs` (how v7 files fill them),
OLX at `d1ad953f` (`target/olx-src/d1ad953f/`), `docs/porting/olx-notes/projectiles.md` and
`weapons-firing.md`. Bundled-content counts come from parsing every file in
`content/packs/olx-classic` (38 weapons, 58 projectile files) and `content/packs/olx-mw-1-0`
(50 weapons, 302 projectile files). Counts are per definition file, not per expanded
occurrence (Classic expands to 69 occurrences, MW to 398).

## 0. Conventions every unit below relies on

| Quantity | What it is in the sim | Evidence |
|---|---|---|
| Physics step | `LX56PhysicsDT` = `1000 / LX56PhysicsFPS` whole ms = **11 ms** at 84 FPS; `TimeDiff::seconds()` = `ms as f32 * 0.001` | `settings.rs:171-176`, `olr-math/src/time.rs:20-22` |
| Projectile step | each projectile runs its own 11 ms steps (catch-up step = `11 / GameSpeed` ms, physics still uses 11 ms) | `proj_sim.rs:90-113`, `:122` |
| Worm/weapon sub-step | `dt` = 0.011 s; beams and jetpack use the fixed 10 ms frame time | `weapons.rs:23-46`, `:130`, `beam.rs:278` |
| Velocity | **px/s**: `pos += vel * dt_s` | `proj_sim.rs:279` |
| Acceleration | **px/s²**: `vel += a * dt_s` | `proj_sim.rs:275`, `physics.rs:272` |
| Angles | **degrees**, 0 = +x (right), 90 = +y (down, screen coordinates), via `GetVecFromAngle` | `spawn.rs:14-21` (OLX `MathLib.cpp:135-155`) |
| Fixed random `r` | one entry of OLX's 256-entry table, **in [-1, 1]**, indexed by the parent's `iRandom` | `olr-math/src/rand.rs:164`, `spawn.rs:63-69` |
| Hole size | index of the theme's hole stamp, `clamp((int)size, 0, 4)` (OLX `HoleN.png`, N = size+1) | `effects.rs:41-50` (OLX `CMap.cpp:1005-1105`) |
| Health | worm health points (100 at spawn), times the shooter's damage factor | `damage.rs:147-160` |

"Default" columns: **lgs** = what the v7 importer stores when the file does not carry the field
(it is conditional on other fields); **ctor** = OLX's C++ constructor default; **ini** = OLX's
text-mod (`CompileProjectile`/`CompileWeapon`) default, a useful hint for authoring defaults.

## 1. Cross-cutting findings (read before fixing names)

1. **`ModDef.worm` and `ModDef.rope` are never read by `olr-sim`.** The sim reads
   `SimSettings::worm_*`/`rope_*` (`settings.rs:17-33`), and every play path builds them from
   `SimSettings::lx56()` (Classic's values, `olr-game/src/setup.rs:43,104`,
   `olr-content/src/matches.rs:67`). OLX copies 5 worm and 3 rope values into the
   `lx56modSettings` layer (`CGameScript.cpp:798-817`, `Game.cpp:477`), the bottom settings
   layer, and the "LieroX 0.56" preset does not override them. So **MW 1.0 currently plays with
   Classic's worm gravity 100 / jump -65 / rope rest 27 / strength 4.5 instead of its own 50 / -75 /
   30 / 3.5** (oracle replays are unaffected: they take features from the trace header). Out of
   scope for the format, but worth a STATUS entry or a fix.
2. **Several "gfx" fields are simulation state, not look.** Spec §1 says `look` slots are
   render-only and never reach `olr-sim`, and the §2 example writes the projectile graphic as
   `look: Sprite(...)`. But the sim reads `gfx`: Pixel colour count (one `rand()` per spawn and
   `Projectile::colour`, `spawn.rs:246-254`), `rotating` + `rot_increment` (one `rand()` per
   shot child, `spawn.rs:93-122`), `rot_speed` (`Projectile::rotation`, checksummed,
   `proj_sim.rs:160-163`), `animation` + the image's frame count (ANI_ONCE ends the projectile,
   `proj_sim.rs:164-187`, `actions.rs:331-337`). Only `use_angle`, `use_spec_angle`,
   `angle_images` are render-only. Recommend not calling this field `look` (e.g. `gfx:`), and
   keeping `look` for the future render-only slots.
3. **`SimMod` equality depends on `ModDef::projectiles` keys.** `SimProjectile.def` is a clone of
   the definition (`mod_rt.rs:301-303`), and the clone embeds the child keys
   (`event_spawn.projectile`, `trail.spawn.projectile`). The sim never reads those strings at run
   time (it uses `SimProjectile::event_spawn.proj` indices), but the §6 "`SimMod` equivalence" and
   "import equivalence" tests compare them. Either the compiler reproduces today's keys exactly
   (today: the first-occurrence tree path of each *structurally distinct* definition,
   `convert.rs:100-131`, which also dedups within one weapon tree), or the tests compare modulo
   these strings.
4. **`gravity: Some(100)` is not `None`.** Same behaviour (`proj_sim.rs:274`), different
   `ProjectileDef`; 5 bundled files use `Some(100)` (Classic crackler, mine, napalm, spikeballs;
   MW floating-bomber.spawn.spawn.spawn.spawn). The format must keep "custom gravity" as an
   optional field (absent = world gravity), never fold 100 into the default.
5. **Spawn flags are fixed per context in LX56.** The importer sets them from the context, never
   from the file (`parse.rs:135-142`, `:281-287`, `:299-301`); every bundled file agrees:
   - shot (weapon): `use_angle`, `use_random_rot`, `add_parent_vel` = true; `angle` 0;
   - event spawn: `use_angle` and `angle` from the file, the rest constructor defaults;
   - trail spawn: `use_special11_vec_for_speed_var` = true, `use_parent_velocity_for_spread` from
     the file, `angle` 0, `use_angle` false.
   Context-dependent defaults (shot/event/trail) make all of these vanish from imported files.
6. **`amount` default.** OLX's `Proj_SpawnInfo()` default is 0, but no bundled spawn has 0 (all
   416 have ≥ 1, 333 have exactly 1), and OLX itself uses 1 elsewhere (`Proj_Action()` sets
   `Proj.Amount = 1`, `ProjAction.h:211-215`; the ini trail default is 1, `CGameScript.cpp:1883`).
   Default 1 is "OLX's own" and saves 333 lines.
7. **Cycles:** `SimMod::compile` allows a projectile to spawn an ancestor (`mod_rt.rs:284-286`),
   but `olr-pack::validate` reports a cycle as an Error (`validate.rs:158-164`, walking
   `children()`, which includes `trail.spawn` even when the trail kind is not `Projectile`), and
   `olr-content/src/cache.rs:714` refuses packs with validate errors. The new compiler should pick
   one rule.

## 2. `ModDef`

| Field | Sim use | Unit | Default | Proposed name | Bundled |
|---|---|---|---|---|---|
| `name` | Not in `SimMod`. Bot loadouts match on it (`olr-bots/src/loadout.rs:86-90`: "Classic", "Liero v1.0"), and so does `WeaponRules` (`olr-content/src/mods.rs:42-45`). OLX: lgs header `ModName` (`CGameScript.cpp`, header bytes 24..88) | text | always present | `name` | "Liero v1.0", "Modern Warfare1.0" |
| `worm` | **Not read by olr-sim** (§1.1); OLX: `lx56modSettings` (`CGameScript.cpp:807-817`) | see §3 | lgs always present | `worm` | see §3 |
| `rope` | **Not read by olr-sim** (§1.1); OLX: `CGameScript.cpp:787-800` | see §4 | lgs always present | `rope` | see §4 |
| `weapons` | Order = weapon id (`SimMod::weapon(id)`, `mod_rt.rs:234-236`); compile walks them in order (`mod_rt.rs:186-215`) | ordered list of ids | — | `weapons` (in `mod.ron`) | 38 / 50 |
| `projectiles` | Lookup by key during expansion, plus cycle detection by key (`mod_rt.rs:281-286`); see answer A | map key → def | — | `projectiles/<id>.ron` (shared) + inline | 58 / 302 files |
| `restrictions` | Not in the sim; `WeaponRules` (`olr-data/src/restrictions.rs`) | — | — | `restrictions.ron` (as today) | — |

## 3. `WormParams` (OLX `gs_worm_t`, `CGameScript.cpp:45-55`)

None of these reach `olr-sim` today (§1.1). The "Sim use" column describes the
`SimSettings` field OLX feeds from it.

| Field | Sim use (via the `SimSettings` field OLX feeds) | Unit | Default | Proposed name | Bundled (Classic / MW) |
|---|---|---|---|---|---|
| `angle_speed` | **Ignored by OLX** ("was always ignored", `CGameScript.cpp:45`); the sim's aim speed is the constant `AIM_ACCEL`/`AIM_MAX_SPEED` (`physics.rs:75-86`) | none (would be deg/s) | lgs always present; no OLX default | keep for losslessness: `unused_angle_speed` | 150.0 / 150.0 (constant) |
| `ground_speed` | `FT_WormGroundSpeed`: `vel.x += speed * dt * 90` while walking on ground, until `WormMaxGroundMoveSpeed` (`physics.rs:223-236`, OLX `PhysicsLX56.cpp:88-469`) | acceleration: **value × 90 px/s²** | FeatureList 8.0 (`FeatureList.cpp:67`) | `ground_acceleration` | 8.0 / 8.0 (constant, = default) |
| `air_speed` | `FT_WormAirSpeed`: the same in the air, capped by `WormMaxAirMoveSpeed` | value × 90 px/s² | FeatureList 4.0 (`:68`) | `air_acceleration` | 4.0 / 4.0 (constant, = default) |
| `gravity` | `FT_WormGravity`: `vel.y += gravity * dt` (`physics.rs:272`) | px/s² | FeatureList 50.0 (`:70`) | `gravity` | 100.0 / 50.0 (1 non-default) |
| `jump_force` | `FT_WormJumpForce`: a ground jump *sets* `vel.y = jump_force` (`physics.rs:255-259`) | px/s, negative = up | FeatureList -75.0 (`:71`) | `jump_speed` | -65.0 / -75.0 (1 non-default) |
| `air_friction` | `FT_WormAirFriction`: `v -= v² · sign(v) · k · dt` per axis in the air (`physics.rs:266-270`) | quadratic drag, 1/px | FeatureList 0.0 (`:69`) | `air_drag` | 0.0 / 0.0 (constant, = default) |
| `ground_friction` | **Ignored by OLX** ("was always ignored", `CGameScript.cpp:53`); the sim's `worm_ground_friction` (0.1) comes from the FeatureList setting, not the mod | none | lgs always present | `unused_ground_friction` | 0.4 / 0.5 |

## 4. `RopeParams` (`CGameScript.cpp:787-800`)

These are not read by `olr-sim` either; the sim uses `SimSettings::rope_*`.

| Field | Sim use (the `SimSettings` field) | Unit | Default | Proposed name | Bundled |
|---|---|---|---|---|---|
| `max_length` | `FT_RopeMaxLength`: a flying hook farther than this from the worm stops and falls (`rope.rs:124-131`; OLX `CNinjaRope.cpp`, `PhysicsLX56.cpp:505-604`) | px | FeatureList 300 (`:89`) | `max_length` | 300 / 300 (constant, = default) |
| `rest_length` | `FT_RopeRestLength`: no pull while the worm is closer than this (`rope.rs:85-93`, `:133`) | px | FeatureList 30 (`:90`) | `rest_length` | 27 / 30 (1 non-default) |
| `strength` | `FT_RopeStrength`: pull = `strength * 100` toward the hook (`rope.rs:92`, then `w.vel += force * dt`, `physics.rs:219-220`) | value × 100 px/s² | FeatureList 3.5 (`:91`) | `pull` (or `strength`) | 4.5 / 3.5 (1 non-default) |

## 5. `WeaponDef` (OLX `weapon_t`, `include/WeaponDesc.h:90-121`)

| Field | Sim use | Unit | Default | Proposed name | Bundled (non-default Classic / MW) |
|---|---|---|---|---|---|
| `id` | Not serialized (the file name). `SimWeapon.id`, the root projectile path (`mod_rt.rs:191`) | slug | — | file name `weapons/<id>.ron` | — |
| `name` | `SimWeapon.name`: weapon restrictions and bot loadouts match on names | text | lgs always present | `name` | 84 distinct of 88 |
| `recharge` | `charge += dt * recharge * (1 / (LoadingTime * 0.01))` while reloading, current slot only (`weapons.rs:35-45`; OLX `PhysicsLX56.cpp:471-498`) | **full magazines per second at LoadingTime 100 %**; stored = file ÷ 10 (`CGameScript.cpp:777`) | lgs always present; ini 0 | `recharge` (`recharge_per_s` if a unit is wanted) | all 88 non-zero; e.g. 0.171 |
| `drain` | `charge -= drain / 100` per shot (`weapons.rs:59-69`; OLX `CServer_Game.cpp:395-410`); special weapons per thrust call (`beam.rs:286`) | **percent of a full magazine per shot** (> 100 empties it in one shot) | lgs always present; ini 0 | `drain` (`drain_percent`) | all non-zero; e.g. 33.334, 101.0 (25 ×) |
| `rof` | `LastFire = rof` on each shot; the slot counts it down by `dt` and fires again at ≤ 0 (`weapons.rs:32-34`, `:84`, `:137`) | **seconds between shots**; stored = file ms ÷ 1000 (`CGameScript.cpp:776`) | lgs always present; ini 0 | `between_shots_s` | all > 0; e.g. 1.071 |
| `laser_sight` | Spawns the `ENT_LASERSIGHT` entity every sub-step while not reloading (an entity pool slot, so it counts in the sim) (`weapons.rs:151-192`; OLX `CClient_Game.cpp:751-808`) | bool | false (ini false) | `laser_sight` | 2 / 7; true on Classic's handgun |
| `kind` | `WeaponType` and payload (`mod_rt.rs:189-199`) | enum | — | `fire: Projectile(…) \| Beam(…) \| Special(…)` | 86 Projectile, 2 Beam, 0 Special |

### `WeaponKind` variants

| Variant.field | Sim use | Unit | Default | Proposed name | Bundled |
|---|---|---|---|---|---|
| `Projectile.class` | **Unused** by OLX (only read and written, `CGameScript.cpp:729`), dropped by `SimMod::compile`, unused by `olr-bots` | enum | lgs always present; ini `WCL_AUTOMATIC` (`CGameScript.cpp:1607`) | `class` | 21 / 30 not Automatic: Grenade 42, Automatic 35, CloseRange 6, PowerGun 3, Missile 0 |
| `Projectile.recoil` | At delivery: `worm.vel -= GetVecFromAngle(aim) * recoil`, once per shot (`shots.rs:189-192`; OLX `CClient_Game.cpp:909-914`). Bots treat `recoil < 0` as a jetpack (`olr-bots/src/moving.rs:210`; OLX `CWormBot.cpp:3737`) | **px/s velocity impulse**, opposite to the aim | ini 0 | `recoil` | 24 / 44 non-zero; MW jet-pack -8 |
| `Projectile.sound` | `SimWeapon.sound`: a `Sound` event at the shot position on delivery (`shots.rs:186-188`). Dropped at import when OLX would fail to load it (`convert.rs:140-148`) | pack path | None | `sound` | 33 / 45 set; e.g. "sfx/bazooka.ogg" |
| `Projectile.spawn` | The shot's `SpawnInfo`, applied with a shot parent at delivery (`shots.rs:202`, `mod_rt.rs:190-193`) | `SpawnDef` (§9, shot context) | — | `shot` | 86 |
| `Beam.recoil` | **Unused by the sim**: `ProcessShot` returns for beams before the recoil (`shots.rs:178-183`; OLX `CClient_Game.cpp:888-891`). Bots read its sign | px/s (unused) | 0 | `recoil` | 0 / 5 |
| `Beam.beam` | `SimWeapon.beam` (`mod_rt.rs:194`) | `BeamDef` (§6) | — | `beam` (or flatten into `Beam(...)`) | 2 |
| `Special.special` | `SpecialKind` (§5.2) | enum | — | positional `Special(Jetpack, …)` | none |
| `Special.thrust` | Jetpack: `vel.y += -50 * thrust * 0.010` per call, plus a spray entity whose velocity is `thrust` (`beam.rs:279-284`; OLX `CClient_Game.cpp:549-585`). Stored `u32`, cast `as i32` at compile (`mod_rt.rs:196`). In local play it only fires with ROF ≤ 0 (`weapons-firing.md` §0.7) | value × 50 px/s² upward (at the fixed 10 ms frame) | lgs always present | `thrust` | none |

### `WeaponClass` (OLX `Wpn_Class`, `WeaponDesc.h:25-34`): unused everywhere

| Variant | LX value | Proposed | Bundled |
|---|---|---|---|
| `Automatic` | 0 | `Automatic` | 35 |
| `PowerGun` | 1 | `PowerGun` | 3 |
| `Grenade` | 3 | `Grenade` | 42 |
| `Missile` | 4 | `Missile` | 0 |
| `CloseRange` | 5 | `CloseRange` | 6 |
| `Other(i)` | 2, others | `Unknown(i)` | 0 |

### `SpecialKind` (OLX `Wpn_Special`)

| Variant | Sim use | Proposed | Bundled |
|---|---|---|---|
| `None` (0) | No thrust; the charge still drains (`beam.rs:279-290`) | `None` | 0 |
| `Jetpack` (1) | Thrust, as above | `Jetpack` | 0 (MW's "Jet Pack" is a projectile weapon) |
| `Other(i)` | Like `None` | `Unknown(i)` | 0 |

## 6. `BeamDef` (OLX `Wpn_Beam`, `WeaponDesc.h:71-85`; v7 reads colour, damage, player damage and length, `CGameScript.cpp:2058-2083`)

| Field | Sim use | Unit | Default | Proposed name | Bundled (Classic / MW laser) |
|---|---|---|---|---|---|
| `colour` | Render: the beam cosmetic, which is **not drawn when the colour is magenta `#ff00ff`** (`beam.rs:262-265`, OLX `:699-710`). v7 ints truncated `as u8` (`parse.rs:114-118`) | `#rrggbb` (alpha always 255) | lgs always present | `colour` | `#ff0000` / `#0000ff` |
| `damage` | At the first dirt/rock pixel: carve a hole of size `damage` (split over the width if enabled) and credit the dirt count (`beam.rs:196-204`, draw `:242-251`). **-1 = no carving** and skips the bonus check (`:178`) | hole size (0..4 after clamping); -1 = off | ctor 0 | `hole_size` (or `carve`) | 0 / 1 |
| `player_damage` | Injures the first other living worm within 5 px of the trace on the delivered shot (`beam.rs:206-211`, OLX `CClient_Game.cpp:926-1055`); the drawn beam never injures | health points | ctor 0 | `worm_damage` | 1 / 3 |
| `length` | Max trace steps; one step = 1 px along the aim (`beam.rs:70`, `:105`) | px | ctor 0 | `length` | 999999 / 999999 (constant) |
| `init_width` | Parts side by side at step 0 (`beam.rs:71`), clamped to the map's smaller side | px (parts) | ctor 1 (v8 only) | `width` | 1 / 1 (constant) |
| `width_increase` | `width = (int)(init_width + width_increase * step)` (`beam.rs:71`) | px of width per px of length | ctor 0.0 (v8 only) | `width_growth` | 0.0 (constant) |
| `distribute_damage_over_width` | `damage / width`, at least `sign(damage)`, for both damages (`beam.rs:117-123`) | bool | ctor false (v8 only) | `split_damage_over_width` | false (constant) |

## 7. `ProjectileDef` (OLX `proj_t`, `include/ProjectileDesc.h:80-121`; v7 loader `CGameScript.cpp:833-1157`)

| Field | Sim use | Unit | Default | Proposed name | Bundled (non-default Classic / MW) |
|---|---|---|---|---|---|
| `gfx` | See §8 and §1.2. Also: `width`/`height` follow it at import | enum | — | `gfx` (avoid `look`, §1.2) | 41 Pixel, 319 Image |
| `gravity` | `vel.y += ProjGravityFactor * g * dt` in every move (`proj_sim.rs:274-275`; OLX `:367-430`). `None` = `UseCustomGravity` off = 100 | **px/s²**, integer | None (= 100) | `gravity` (optional; absent = world gravity) | 34 / 248 set (Some(0) 196×, Some(100) 5×, Some(-10) 10×) |
| `dampening` | `vel *= dampening` **once per 11 ms step**, before the move (`proj_sim.rs:230-233`; OLX `:433-491`). Also `changes_speed = (int)dampening == 1 && …` re-computes check steps (`spawn.rs:256`, `proj_sim.rs:222-228`) | **unitless factor per physics step** (> 1 accelerates) | lgs always present (raw 4 bytes as f32); ini 1.0 (`CGameScript.cpp:1711`); ctor 0.0 | `velocity_factor` (or keep `dampening`), schema: "per 11 ms step" | 5 / 9; e.g. 1.001 (bazooka), 0.97 |
| `width` | Radius `(width / 2, height / 2)` (`spawn.rs:231`): map-border test (`proj_sim.rs:312-329`), terrain box of `2r+1` px (`:333-359`); worm test uses r only when > 2 (`:479-485`) | px | **derived**: 2 for Pixel, 4 for Image (`parse.rs:177`, OLX `CGameScript.cpp:873-878`) | `size: (w, h)`, default by gfx kind | always the derived value (0 non-default) |
| `height` | As `width` | px | derived | (in `size`) | always derived |
| `timer` | §10 | — | — | `timer` | — |
| `trail` | §11 | — | — | `trail` | — |
| `hit` | Terrain-hit action, applied when the step's collision has `PJC_TERRAIN` (`proj_sim.rs:134-136`). Its **kind also sets the collision physics**: Bounce, Nothing (half-move), GoThrough (no stop), Explode in dirt only (keeps position), everything else stops at the last safe position (`proj_sim.rs:363-436`, OLX `CProjectile.cpp:285-357`); and Bounce changes the check steps (`spawn.rs:241-242`) | `ActionDef` (§12) | — | `hit_terrain: [effects]` | kinds: Explode 182, Bounce 82, Nothing 81, Carve 14, GreenDirt 1 |
| `player_hit` | Worm-hit action, applied when a worm was hit and no Explode happened this step (`proj_sim.rs:137-139`). Kind `Nothing` **turns the worm test off** completely; `GoThrough` hits but does not stop at the worm (`proj_sim.rs:448-466`, OLX `:130-171`) | `ActionDef` | — | `hit_worm: [effects]` | kinds: Nothing 184, Injure 116, Explode 58, Dirt 1, GreenDirt 1 |
| `event_spawn` | `GeneralSpawnInfo`: what any event action with `spawns_projectiles` spawns (answer B). Expanded as `<path>.spawn` (`mod_rt.rs:312-315`) | `SpawnDef` (event context) | None | `spawns: { <name>: … }` | 28 / 187 set |

## 8. `ProjectileGfx` and `Animation` (answer E has the details)

| Variant.field | Sim / render use | Unit | Default | Proposed name | Bundled |
|---|---|---|---|---|---|
| `Pixel.colours` | One is drawn at spawn with `rand()` (one draw when the list is non-empty, none when empty), stored in `Projectile::colour` (`spawn.rs:246-254`; OLX `CProjectile.cpp:43-112`). Render: a 1-world-pixel square, alpha from the list entry (`olr-scene/src/projectile.rs:100-112`). v7 ints truncated `as u8` (`parse.rs:186-189`) | list of `#rrggbb` (alpha 255 in v7) | lgs always present; ini: black if empty | `Pixels([...])` | 28 single, 13 two-colour; alpha always 255 |
| `Image.file` | Pack path. Frame count `width / height` of the PNG feeds `SimProjectile.frames` (`mod_rt.rs:294-297`, `olr-content/src/mods.rs:69-91`); render source image | pack path | — | `Image("gfx/x.png", …)` positional | 115 distinct; "gfx/none.png" 104× |
| `Image.rotating` | Gates `rot_speed` (`proj_sim.rs:160-163`), the random start rotation of shot children (`mod_rt.rs:164-169`, `spawn.rs:93-122`: one `rand()` each) and rotation frames (render) | bool | false | `rotating` | 10 / 14 true |
| `Image.rot_increment` | Start rotation = `rand(abs(360 / inc)) * inc` (inc 0 counts as 1, and OLX makes that sticky, `rot_bumped`, `spawn.rs:95-121`); render frame = `rotation / inc` (`olr-scene/src/projectile.rs:68-71`) | degrees per image frame | 0 | `rotation_step_deg` | 10 / 14 non-zero (90, 120, 45) |
| `Image.rot_speed` | `rotation += rot_speed * dt`, `fmod 360`, while `rotating` (`proj_sim.rs:160-163`; checksummed) | **degrees per second** | 0 | `rotation_speed` (deg/s) | 10 / 14 non-zero (400, 700) |
| `Image.use_angle` | Render only: frame = `fmod(heading, 360) / (360 / angle_images)`, heading = `-atan2(vx, vy)` in degrees (0 = moving down) (`olr-scene/src/projectile.rs:62-67`; OLX `CProjectile.cpp:238-286`) | bool | false | `frames_follow_heading` | 5 / 37 true |
| `Image.use_spec_angle` | Render only: frames split into a left-moving half and a right-moving half, `(angle_images - 1) / 2` each, over OLX's 151° (`olr-scene/src/projectile.rs:46-61`) | bool | false | `frames_follow_heading_mirrored` | 0 (constant false) |
| `Image.angle_images` | Render only: frames used by the two schemes above; read only when either flag is set, else 0 (`parse.rs:199`) | frame count | 0 | `heading_frames` | 5 / 37 non-zero (18, 10, 6, 2) |
| `Image.animation` | `Some` = OLX `Animating` (`proj_sim.rs:164-187`) | — | None | `animation` | 4 / 72 set |
| `Animation.rate` | `frame += rate * dt` (or `-=` while ping-ponging back) (`proj_sim.rs:165-169`) | **frames per second** | ctor 0.0 | `fps` | 4 / 72 non-zero; e.g. 10.0, 15.0 |
| `Animation.kind` | At `frame >= frames`: see §8.1 (`proj_sim.rs:170-186`) | enum | ctor/ini `ANI_ONCE` | `mode` (or `end`) | Once 40, Loop 36 |

### 8.1 `AnimKind` (OLX `Proj_AnimType`)

| Variant | Sim use | Proposed | Bundled |
|---|---|---|---|
| `Once` (0) | **Deletes the projectile** at the last frame; and a projectile with no action and an ANI_ONCE animation is not deleted by the junk rule (`proj_sim.rs:174`, `actions.rs:330-337`) | `Once` | 40 |
| `Loop` (1) | `frame = 0` | `Loop` | 36 |
| `PingPong` (2) | Reverses at either end | `PingPong` | 0 |
| `Other(i)` | Nothing at the end: the frame grows unbounded (the renderer wraps it modulo the strip) | `Unknown(i)` | 0 |

## 9. `SpawnDef` (OLX `Proj_SpawnInfo`, `ProjAction.h:72-100`; `apply`, `PhysicsLX56_Projectiles.cpp:562-640` → `spawn.rs:86-154`)

Per child `i` in `0..amount`:
`dir = use_parent_velocity_for_spread ? parent.vel * parent_vel_spread_factor : GetVecFromAngle(angle + (use_angle ? heading : 0) + r1 * spread)`;
`v = dir * speed + (use_special11 ? (1,1) : dir) * speed_var * r2 + (add_parent_vel ? parent_vel_factor * parent.vel : 0)`.
The heading is the shot's integer aim angle, or for a projectile parent `fmod(-atan2(vx, vy)·180/π + 90, 360)` (`spawn.rs:45-61`).

| Field | Sim use | Unit | Default (ctor / per context in LX56) | Proposed name | Bundled (non-default vs the context default; shot / event / trail) |
|---|---|---|---|---|---|
| `projectile` | Key into `ModDef::projectiles`, becomes `SpawnInfo.proj` (`mod_rt.rs:191`, `:313`, `:320`) | key | — | `projectile` (inline def or shared id) | — |
| `amount` | Loop count (`spawn.rs:103`); > 1000 is a compile error (`mod_rt.rs:99-101`) | children per spawn | ctor 0; see §1.6 | `amount` (default 1) | all ≥ 1; 333 of 416 are 1 |
| `speed` | `dir * speed` (`spawn.rs:135`) | **px/s** (integer); with `use_parent_velocity_for_spread` it multiplies the scaled parent velocity instead (unitless) | ctor 0 | `speed` | e.g. 140 (bazooka shot), 1000 |
| `speed_var` | `+ var_dir * speed_var * r2`, r2 ∈ [-1, 1] (`spawn.rs:126-135`) | **px/s**, ± range | ctor 0.0 | `speed_var` | 20 / 57 / 37; e.g. 14.0 |
| `spread` | `angle + heading + r1 * spread`, r1 ∈ [-1, 1] (`spawn.rs:112-113`) | **degrees, ± half-width** (360 = any direction, twice over) | ctor 0.0 | `spread` (deg) | 46 / 103 / 74; e.g. 360.0 |
| `use_angle` | Adds the parent's heading (`spawn.rs:92`) | bool | ctor false; **shot: true (fixed)**; event: from file; trail: false (fixed) | `relative_to_parent` (or `add_parent_heading`) | shot 86/86 true; event 74/215 true; trail 0 |
| `angle` | Base direction (`spawn.rs:113`) | **degrees** (integer) | ctor 0; shot and trail: 0 (not in v7) | `angle` | event only: 105/215 non-zero (-90 62×, 180 23×) |
| `use_parent_velocity_for_spread` | `dir = parent.vel * parent_vel_spread_factor` (no random draw, `spawn.rs:109-110`; skip path `:187`) | bool | ctor false; trail: from file; shot/event: false (OLX ini forces false, `CGameScript.cpp:1619-1622`, `:1784-1787`) | `parent_velocity_as_direction` | trail 13/115 true; never elsewhere |
| `parent_vel_spread_factor` | The scale above | unitless | ctor **0.3** (v8 only) | `parent_velocity_direction_scale` | 0.3 everywhere (constant) |
| `use_special11_vec_for_speed_var` | Speed variation along `(1, 1)` instead of `dir` (`spawn.rs:126`) | bool | ctor false; **trail: true (fixed, `Proj_Trail()`)** | `speed_var_diagonal` | trail 115/115 true, shot/event never (constant per context) |
| `use_random_rot` | Random start rotation if the child `rotating` (`spawn.rs:93-122`) | bool | ctor false; **shot: true (fixed)** | `random_rotation` | shot 86/86 true, never elsewhere |
| `add_parent_vel` | Adds `parent_vel_factor * parent.vel` (shot parent: the worm velocity, zero unless `ProjRelativeVel`) (`spawn.rs:128-134`) | bool | ctor false; **shot: true (fixed)** | `inherit_velocity` | shot 86/86 true, never elsewhere |
| `parent_vel_factor` | Diagonal of `MatrixD2<float>(f)` (`spawn.rs:130-131`) | unitless | ctor **1.0** (v8 only) | `inherit_velocity_factor` | 1.0 everywhere (constant) |

## 10. `TimerDef` (OLX `Proj_LX56Timer`, `ProjAction.h:452-468`)

| Field | Sim use | Unit | Default | Proposed name | Bundled |
|---|---|---|---|---|---|
| `time` | Fires **every step** once `time + time_var * f < life` (life before this step's +11 ms; `f` = the projectile's fixed random in [-1, 1], `spawn.rs:237`), and only if `time > 0` and the action has an effect (`proj_sim.rs:125-130`, `actions.rs:85-87`; OLX `:949-956`) | **seconds** of projectile life | 0.0 (no timer) | `timer: (after_s: …)` | 29 / 216 set; e.g. 2.43 |
| `time_var` | As above | **seconds**, ± range | 0.0 | `variation_s` | 21 / 52; e.g. 0.29 |
| `action` | `Some` **exactly when `time > 0`** (`parse.rs:241-251`); kinds read from v7: any, but damage/spawn/shake only for Explode | `ActionDef` | None | the timer's effect list | kinds: Explode 241, Dirt 2, GreenDirt 2 |

Timer quirk to keep: a timer Explode uses `player_hit.damage` when the worm-hit kind is Explode
(`actions.rs:296-303`; OLX `projectile_doTimerExplode`).

## 11. `TrailDef` (OLX `Proj_Trail`, `ProjAction.h:131-138`) and `TrailKind`

| Field | Sim use | Unit | Default | Proposed name | Bundled |
|---|---|---|---|---|---|
| `kind` | `low_level` (`proj_sim.rs:189-214`), §11.1 | enum | `TRL_NONE` | `trail: Smoke \| … \| Projectile(…)` | 12 / 128 set |
| `delay` | Projectile trails only: the next spawn waits `delay / GameSpeed`, truncated to whole ms (`c_u64(delay * 1000)`, `proj_sim.rs:207-212`); the first fires on the first step. **Ignored for other kinds** (always 0.1 there) | **seconds**; stored = file ms ÷ 1000 (`parse.rs:302`) | ctor 0.1 | `every_s` (inside `Projectile(...)`) | 4 / 110 non-default; e.g. 0.04, 10.0, 100.0 |
| `spawn` | `Some` exactly when `kind == Projectile` (`parse.rs:297-309`); expanded as `<path>.trail` only then (`mod_rt.rs:316-322`; a missing one is an error) | `SpawnDef` (trail context) | None | `spawn: <name>` | 4 / 111 |

### 11.1 `TrailKind`

| Variant | Sim use | Proposed | Bundled |
|---|---|---|---|
| `None` (0) | — | (absent) | 220 |
| `Smoke` (1) | Smoke entity every **75 ms** (fixed, not `delay`); entities take pool slots (spec §18 D4) | `Smoke` | 7 |
| `ChemSmoke` (2) | Chem-smoke entity every 75 ms | `ChemSmoke` | 11 |
| `Projectile` (3) | Spawns `spawn` every `delay` | `Projectile(every_s, spawn)` | 115 |
| `Doomsday` (4) | Doomsday entity (with the projectile's velocity) every **50 ms** | `Doomsday` | 6 |
| `Explosive` (5) | Cosmetic explosion entity of size 10 every 50 ms (no damage, no hole) | `Explosive` (or `Sparks`) | 1 |
| `Other(i)` | No trail | `Unknown(i)` | 0 |

## 12. `ActionDef` (the LX56 part of `Proj_Action`, `ProjAction.h:203-253`)

`apply_to` is `actions.rs:95-231` (OLX `PhysicsLX56_Projectiles.cpp:740-945`), and `execute` is
`actions.rs:289-341` (OLX `:1165-1230`). In the new model each field becomes a parameter of the
effect that reads it.

| Field | Sim use | Unit | Default (ctor; v7 only reads it for…) | Proposed (effect parameter) | Bundled (hit / worm hit / timer) |
|---|---|---|---|---|---|
| `kind` | §12.1 | enum | ctor `PJ_EXPLODE` (PlayerHit ini default `PJ_INJURE`); v7 always present | the effect itself; `Nothing` = empty list | see §12.1 |
| `damage` | Explode: blast "damage" (`actions.rs:107-119`, `:304-305`). **One number is both** the hp to every living worm within 5 px (no falloff) **and** the hole size `clamp((int)d, 0, 4)` and the entity size (`effects.rs:167-218`); **-1 = no blast** (the projectile still dies and still spawns). Carve: hole size (`effects.rs:70-75`). Injure/InjureWorm: hp (`actions.rs:154-163`). Unused otherwise | hp and hole size (Explode), hole size (Carve), hp (Injure) | 0; v7 reads it for hit Explode/Carve, timer Explode, worm-hit Injure/Explode | `Explode(damage)`, `Carve(size)`, `Injure(damage)` | 189 / 169 / 234 non-zero; -1: 99 / 18 / 151 |
| `spawns_projectiles` | Sets the step's `spawn_projectiles`, unless cancelled (timer + Bounce/Injure/Disappear/GoThrough/Nothing, `actions.rs:126-127`, `:170-172`); spawns `event_spawn` (`:322-326`) | bool | false; v7 reads it for hit Explode, timer Explode, worm-hit Injure/Explode | `Spawn(<name>)` | 89 / 58 / 174 true |
| `sound` | Explode and PlaySound set the step's sound (the last one wins), played at the position (`actions.rs:116-124`, `:327-329`); `mod_rt.rs:298-300` interns it | pack path | None; **v7 only for hit Explode** (`parse.rs:226-230`) | `Explode(sound: …)`, `Sound(<path>)` | 49 / 0 / 0 set |
| `shake` | Explode: `max` into the step's shake (`actions.rs:113-115`); Dirt/GreenDirt: only on a timer (`:149-151`). Cosmetic: the camera offset ±`shake` world px for 200 ms (`olr-scene/src/camera.rs:13-15`, `:194-195`); it travels in `EventKind::Explosion` | world pixels of camera shake | 0; v7 only for hit/timer Explode | `Explode(shake: …)` | 59 / 0 / 27 non-zero; e.g. 2, 25 |
| `bounce_coeff` | Bounce: `vel *= (±c, ∓c)` on the colliding side, with LX56's "wrong" axis rule (`actions.rs:236-252`; OLX `CProjectile.cpp:376-429`); also `changes_speed` when it is the hit kind (`spawn.rs:256`) | unitless velocity factor | **0.5**; v7: hit Bounce, worm-hit Bounce | `Bounce(factor)` | 59 / 0 / 0 non-default; e.g. 0.65, 1.0 |
| `bounce_explode` | Bounce (not on timer): `Explosion(pos, bounce_explode, shake 0)` at once, every colliding step, if > 0 (`actions.rs:131-134`) | blast damage (hp and hole size, as Explode) | 0; v7: hit Bounce only | `Bounce(explode: …)` | 0 everywhere (constant) |

Every step also pushes a hit worm by `normalize(vel) * 100 * 0.011` px/s, except for Bounce and the
Nothing family (`actions.rs:220-227`). That is not a field.

### 12.1 `ActionKind` (`Proj_ActionType`, `ProjAction.h:147-175`) with proposed effect names

"Bundled" = hit / worm hit / timer occurrences (per definition file).

| # | Variant | What it does in LX56 events (`actions.rs`) | Proposed effect | Bundled |
|---|---|---|---|---|
| 0 | `Bounce` | Terrain or worm: bounce; optional blast. On a timer: nothing, cancels the spawn | `Bounce(factor: 0.5, explode: 0)` | 82 / 0 / 0 |
| 1 | `Explode` | Blast after the move; the projectile dies; shake; sound | `Explode(damage, shake, sound)` | 182 / 58 / 241 |
| 2 | `Injure` | Worm: injure for `damage` and die. Otherwise like `Disappear` (on a timer: nothing; on terrain: stops, dies only at the map border) | `Injure(damage)` | 0 / 116 / 0 |
| 3 | (unused; `Other(3)`) | — | `Unknown(3)` | 0 |
| 4 | `Carve` | Timer or terrain: carve a hole of size `damage` **at once** (before the move on a timer) and die; worm: nothing (push) | `Carve(size)` | 14 / 0 / 0 |
| 5 | `Dirt` | Places three dirt balls (size 5 → hole 4) and dies (`effects.rs:80-92`) | `Dirt` | 0 / 1 / 2 |
| 6 | `GreenDirt` | Places the green-dirt mask and dies (`effects.rs:97-106`) | `GreenDirt` | 1 / 1 / 2 |
| 7 | `Disappear` | "Mostly ignored (LX56)": on a worm it pushes only; otherwise like `Nothing`. **It does not remove the projectile** | `Disappear` (OLX's name; the schema must say it does not remove), or `Halt` | 0 |
| 8 | `Nothing` | Terrain: dies only at the map border; as `hit`, a half-step move through terrain; as `player_hit`, no worm collision. On a timer it cancels the spawn | empty list `[]` | 81 / 184 / 0 |
| 9 | `Disappear2` | Dies (`actions.rs:153`) | `Remove` (OLX Beta9) | 0 |
| 10 | `GoThrough` | Like `Nothing`, but as `hit` it moves on through terrain, and as `player_hit` it goes through worms | `GoThrough` | 0 |
| 11 | `InjureProj` | No effect: LX56 events have no targets | `InjureProjectile` | 0 |
| 12 | `PlaySound` | Plays `sound` (never set by v7 files, so it does nothing there) | `Sound(<path>)` | 0 |
| 13 | `InjureWorm` | Worm: injure for `damage` (v7 never reads it, so 0) and keep flying | `InjureWorm(damage)` (or `Hurt`) | 0 |
| 14 | `ChangeRadius` | `radius += (0, 0)`, clamped ≥ 0: a no-op (v8 `ChangeRadius` absent) | `ChangeRadius` | 0 |
| 15 | `OverwriteOwnSpeed` | `vel = (0, 0)` (v8 `Speed` absent) | `SetSpeed` | 0 |
| 16 | `MultiplyOwnSpeed` | `vel = 1 · vel`: a no-op apart from `Attr` sign rules | `MultiplySpeed` | 0 |
| 17 | `DiffOwnSpeed` | `vel += (0, 0)`: a no-op | `AddSpeed` | 0 |
| 18 | `OverwriteTargetSpeed` | No targets: nothing | `SetTargetSpeed` | 0 |
| 19 | `MultiplyTargetSpeed` | Nothing | `MultiplyTargetSpeed` | 0 |
| 20 | `DiffTargetSpeed` | Nothing | `AddTargetSpeed` | 0 |
| 21 | `HeadingToNextWorm` | Steers toward the nearest living worm (`actions.rs:187-191`, `:254-284`) | `HomeOnNearestWorm` | 0 |
| 22 | `HeadingToOwner` | Steers toward the owner | `HomeOnOwner` | 0 |
| 23 | `HeadingToNextOtherWorm` | Steers toward the nearest worm that is not the owner | `HomeOnNearestOtherWorm` | 0 |
| 24 | `HeadingToNextEnemyWorm` | Same as 23 in deathmatch (no teams) | `HomeOnNearestEnemy` | 0 |
| 25 | `HeadingToNextTeamMate` | `vel += 0` (no team game): a no-op | `HomeOnNearestTeamMate` | 0 |
| 26 | `HeadTargetToUs` | No targets: nothing | `PullTargetsToUs` | 0 |
| — | `Other(i)` | Nothing, but it counts as an action and the worm push still applies | `Unknown(i)` | 0 |

---

## A. Projectile identity in `SimMod::compile`

`crates/olr-sim/src/mod_rt.rs`:

- **Order.** `compile` walks `def.weapons` in order (`:186-215`). For each `Projectile` weapon it
  calls `expand(spawn.projectile, by = w.id, path = w.id)` (`:191`). `expand` (`:280-325`) pushes
  the occurrence **before** its children (pre-order, `:293-311`), then expands `event_spawn` as
  `"{path}.spawn"` (`:312-315`), then the trail spawn as `"{path}.trail"`, the latter **only when
  `trail.kind == TrailKind::Projectile`** (`:316-322`). This is OLX's v7 load order
  (`CGameScript::LoadProjectile`, `CGameScript.cpp:833-1157`: the projectile, then its
  `GeneralSpawnInfo` child, then its trail child). The index in `SimMod::projectiles()` is the
  identity: `SpawnInfo.proj`, `Projectile::proj` and the oracle's projectile types.
- **Name.** `SimProjectile.name` is the path (`:302`), the oracle header's name, used by
  `projectile_index` (`:244-246`). It never contains the map key.
- **What the keys do.**
  1. Lookup: `get_key_value(name)` (`:281`), with a missing key an error (`:282`).
  2. Cycle detection by key: the stack holds `(key, index)` of the current path (`:254`, `:311`,
     `:323`). A reference to a key already on the stack returns the ancestor's index, adding no
     occurrence and no path (`:284-286`).
  3. Through `SimProjectile.def` (a clone, `:303`), the key strings of the children stay inside
     `SimMod` and count in its `PartialEq` (§1.3), though the sim never reads them.

  Outside the sim, keys are file names (`projectiles/<key>.ron`) and appear in `olr-pack`'s
  validator (unknown or unused references, cycles, `validate.rs:133-164`) and in the content
  hash. Sounds are interned in first-use order during expansion, so keys do not affect them
  (`:259-276`, `:298-300`).
- **Twice referenced = twice expanded.** Every reference that is not an ancestor makes a new
  occurrence with its own index, path, frame count and `SpawnInfo`s. Examples: Classic's
  `shotgun` def is fired by Shotgun and Super Shotgun, so it gets 2 occurrences; Classic's
  `bazooka.spawn` is referenced by 9 places (including bouncy-larpa's event *and* trail spawn).
  That is why 58 Classic files become 69 occurrences and 302 MW files become 398. The cap is
  `MAX_PROJECTILES = 4096` occurrences (`:34`).

## B. How `spawns_projectiles`, `event_spawn` and `TrailDef.spawn` interact

- **Event spawn (one per projectile).** `ProjectileDef.event_spawn` (OLX `GeneralSpawnInfo`) is
  the single `SpawnInfo` shared by all three event actions: `hit` (terrain), `player_hit`
  (worm) and `timer.action`. In a step (`proj_sim.rs:117-144`):
  1. the timer action is applied first;
  2. then the terrain action, if the collision has `PJC_TERRAIN`, **or** the worm action, if a
     worm was hit and no Explode is pending;
  3. each applied action with `spawns_projectiles` sets the step's `spawn_projectiles` flag,
     unless its kind cancels it on a timer: Bounce, Injure, Disappear, GoThrough, Nothing
     (`actions.rs:105`, `:126-127`, `:170-172`, `:228-230`).

  The flag is boolean, so a step spawns at most once, however many actions set it.
  `execute` then spawns `event_spawn`, with the projectile as parent, after the explosion, dirt
  and the trail spawn (`actions.rs:317-326`). A flag with `event_spawn == None` does nothing.
- **Importer.** The importer writes `event_spawn` only when any of hit/timer/player-hit, **or the
  obsolete Exp/Touch actions**, has the flag (`parse.rs:264-292`). An LX56 file can therefore
  carry an `event_spawn` that no live action uses (none in the bundled mods). The new format
  needs to allow a named spawn that no effect references, or such files won't round-trip.
  `SimMod::compile` expands and validates `event_spawn` whether or not any action spawns it
  (`mod_rt.rs:312-315`).
- **Trail spawn.** `TrailDef.spawn` is independent of `spawns_projectiles`. It is used only for
  `TrailKind::Projectile`, timed by `delay` (`proj_sim.rs:207-212` sets `trail_spawn`), and
  spawned in `execute` before the event spawn (`actions.rs:317-321`). Actions never spawn the
  trail's projectile, and the trail never spawns `event_spawn`. For other trail kinds,
  `trail.spawn` is ignored by compile, though `olr-pack` still validates it via `children()`.
- **Shot spawn.** `WeaponKind::Projectile.spawn` is the third, separate `SpawnInfo`. It is applied
  with a shot parent at delivery (`shots.rs:202`).
- **Mapping to the new model.**
  - `Spawn(x)` in any of `hit_terrain`/`hit_worm`/`timer` maps to `spawns_projectiles = true`
    plus `event_spawn = spawns[x]`. All events of one projectile must therefore name the same
    spawn in 0.2 (a second spawn name is "not supported by this runtime yet").
  - `trail: Projectile(every_s, spawn: y)` maps to `TrailDef.spawn = spawns[y]`, with the trail
    context defaults. It may be the same name as the event spawn, which compiles to two copies.
  - A trap to document: `timer: [Spawn(x)]` alone compiles to kind Nothing + spawn, and **a
    Nothing timer cancels its spawn**, so it does nothing. MW gets "spawn on a timer" from
    `Explode(damage: -1) + Spawn` (151 timer actions with damage -1).

## C. Which `ActionKind`s LX56 files can contain

The v7 importer reads each kind as a raw `i32` (`ActionKind::from_lx`, `parse.rs:221`, `:242`,
`:254`). OLX's v7 loader does not validate it either (`CGameScript.cpp:953-1000`). So any value is
*possible* in a `.lgs` file, but the extra fields are read only for these kinds:

| Event | Fields read |
|---|---|
| hit | Explode: damage, spawns, sound, shake. Bounce: coeff, explode. Carve: damage |
| timer (only if `Time > 0`) | Explode: damage, spawns, shake |
| player hit | Injure/Explode: damage, spawns. Bounce: coeff |

- **LX 0.56's own kinds** are 0, 1, 2, 4, 5, 6, 7, 8: Bounce, Explode, Injure, Carve, Dirt,
  GreenDirt, Disappear and Nothing (`ProjAction.h:148-155`; 3 is unused).
- **Kinds 9–26 are OLX Beta9+ additions** (Disappear2, GoThrough, InjureProj, PlaySound,
  InjureWorm, ChangeRadius, the speed and heading kinds), meant for v8/ini mods with their extra
  fields (`Speed`, `SpeedMult`, `ChangeRadius`, `GoThroughSpeed`, targets). In a v7 file they get
  the constructor defaults, so most of them degenerate as §12.1 describes:
  - PlaySound has no sound;
  - InjureWorm has 0 damage;
  - Overwrite zeroes the velocity;
  - Multiply and Diff do nothing;
  - the target kinds do nothing;
  - the heading kinds do steer.
- **Gusanos** mods do not use `ActionKind` at all (the separate `gusEngineUsed` engine).
- **Actually used by the bundled mods:**

  | Event | Kinds |
  |---|---|
  | hit | Explode, Bounce, Nothing, Carve, GreenDirt |
  | player hit | Nothing, Injure, Explode, Dirt, GreenDirt |
  | timer | Explode, Dirt, GreenDirt |

  Never used: Injure outside the worm hit, Disappear, all kinds ≥ 9, and `Other`.

## D. Float fields whose stored value is derived by arithmetic

These must be written as the stored (derived) value. RON's shortest round-trip form keeps every
finite `f32` bit-exact (`mod_def.rs:494-505`).

| Field | Stored value | Where |
|---|---|---|
| `WeaponDef.recharge` | `file_f32 / 10.0` (f32 division) | `parse.rs:154`, OLX `CGameScript.cpp:777` |
| `WeaponDef.rof` | `file_f32 / 1000.0` | `parse.rs:156`, OLX `CGameScript.cpp:776` |
| `TrailDef.delay` (projectile trails) | `file_f32 / 1000.0` | `parse.rs:302` (OLX `Proj_Trail` read; ini path `CGameScript.cpp:1880`) |

Constants not from the file:
- `TrailDef.delay` = 0.1 for every non-projectile trail;
- `SpawnDef.parent_vel_spread_factor` = 0.3 and `parent_vel_factor` = 1.0;
- `ActionDef.bounce_coeff` = 0.5 when unread;
- `BeamDef.width_increase` = 0.0.

All other floats are the file's raw 4 bytes, including `dampening`, which OLX reads with
`sizeof(int)` into a float (`parse.rs:175-176`): speed_var, spread, time, time_var, anim rate,
bounce_coeff, the worm floats and rope strength.

Non-float derivations:
- colour components are `i32 as u8` (wrapping: 300 → 44; `parse.rs:114-118`, `:186-189`);
- `width`/`height` come from the gfx type;
- `thrust` is `u32` and becomes `i32` at compile.

Caveat: NaN payloads are not guaranteed to round-trip; only "is NaN" is tested
(`mod_def.rs:483-492`). A crafted file with a NaN float would change bits on re-save.

## E. `ProjectileGfx::Image` fields in the sim and the renderer

- **Frame counts.** `olr-content` decodes every PNG in the pack and records
  `frames = width / height` (integer division; none when the height is 0;
  `olr-content/src/mods.rs:59-70`, `:86-91`). It passes the map `pack path → frames` to
  `SimMod::compile`, which stores `SimProjectile.frames` by `file` (`mod_rt.rs:294-297`).
  `check_frames` then refuses a mod with any image projectile without a count
  (`olr-content/src/mods.rs:50-57`).

  The sim uses `frames` for the animation end (`proj_sim.rs:170-186`) and for the junk-projectile
  exception (`actions.rs:331-334`). Images are loaded even where OLX's dedicated server would not
  (spec §18 D3, `projectiles.md` §0.7). The renderer assumes square frames of side = image
  height: `src.x = ((int)framestep * height) mod width` (`olr-scene/src/projectile.rs:75-83`;
  OLX `CProjectile.cpp:288-291`).
- **Frame selection.** `framestep`, in this priority order (`olr-scene/src/projectile.rs:27-73`;
  OLX `CProjectile.cpp:238-286`), with frame 0 when the projectile is outside the view:
  1. `animation.is_some()`: the animation frame `Projectile::frame`;
  2. `use_spec_angle`: the mirrored two-half scheme over 151°, with `(angle_images - 1) / 2` frames
     per half;
  3. `use_angle`: `fmod(heading, 360) / (360 / angle_images)`;
  4. `rotating && rot_increment != 0 && moving`: `rotation / rot_increment`. A 0 increment counts
     as 1 once `rot_bumped`, OLX's sticky mutation of the shared `proj_t`
     (`spawn.rs:95-102`, `:117-120`).
- **Simulation effects.**
  - `rotating` + `rot_increment`: for shot children (`use_random_rot`), one `rand()` per child
    picks `rotation = rand(|360 / inc|) * inc` (`spawn.rs:93-122`; OLX `:590-593`). This changes
    the global RNG stream.
  - `rot_speed` advances `Projectile::rotation`, in deg/s mod 360, while `rotating`
    (`proj_sim.rs:160-163`). It is checksummed (`checksum.rs:332`), but nothing else in the sim
    reads it.
  - `animation.rate` advances `Projectile::frame` in frames/s. Its `kind` decides the end:
    `Once` deletes the projectile (`proj_sim.rs:164-187`).
  - `use_angle`, `use_spec_angle` and `angle_images` are render-only. The sim never reads them;
    `grep` finds no use in `crates/olr-sim/src`.
- **Pixel gfx.** One `rand()` per spawned projectile picks the colour (`spawn.rs:246-254`). The
  renderer takes alpha from the matching list entry (`olr-scene/src/projectile.rs:100-112`).

## 13. Final names (plan 13a)

The names the authoring types use (`crates/olr-format/src/mods/`: `mod.rs`,
`types.rs`, `spawn.rs`). Every type has `deny_unknown_fields`. "Required"
means the file must carry the field; any other field is left out when it
equals its default. Floats compare by bits. Where the research (§2-§12)
proposed another name, the last column says so.

### `ModDef` (§2) and `WormParams`, `RopeParams` (§3, §4)

| Runtime type.field | Authoring field | Default | Note |
|---|---|---|---|
| `ModDef.name` | `ModSource.name` (`mod.ron`) | required | |
| (none) | `ModSource.notes` | empty | free text; the game ignores it |
| `ModDef.worm` | `worm` in `mod.ron` | all fields default | |
| `ModDef.rope` | `rope` in `mod.ron` | all fields default | |
| `ModDef.weapons` | `weapons` in `mod.ron`: weapon ids in order; one `weapons/<id>.ron` each | required | the id is the file name |
| `ModDef.projectiles` | inline `projectile`, or `projectiles/<id>.ron` for a shared one (`shared`) | | see the key rule below |
| `ModDef.restrictions` | `restrictions.ron` | none | as before |
| `WormParams.angle_speed` | `Worm.unused_angle_speed` | 150.0 | research: not named; OLX ignores it |
| `WormParams.ground_speed` | `Worm.ground_acceleration` | 8.0 | as proposed |
| `WormParams.air_speed` | `Worm.air_acceleration` | 4.0 | as proposed |
| `WormParams.gravity` | `Worm.gravity` | 50.0 | |
| `WormParams.jump_force` | `Worm.jump_speed` | -75.0 | as proposed |
| `WormParams.air_friction` | `Worm.air_drag` | 0.0 | as proposed |
| `WormParams.ground_friction` | `Worm.unused_ground_friction` | 0.4 | OLX ignores it |
| `RopeParams.max_length` | `Rope.max_length` | 300 | |
| `RopeParams.rest_length` | `Rope.rest_length` | 30 | |
| `RopeParams.strength` | `Rope.strength` | 3.5 | research offered `pull` or `strength`: `strength` |

### `WeaponDef` and `WeaponKind` (§5, §6)

| Runtime type.field | Authoring field | Default | Note |
|---|---|---|---|
| `WeaponDef.id` | the file name `weapons/<id>.ron` | | |
| `WeaponDef.name` | `Weapon.name` | required | |
| (none) | `Weapon.notes` | empty | |
| `WeaponDef.recharge` | `Weapon.recharge` | required | |
| `WeaponDef.drain` | `Weapon.drain` | required | |
| `WeaponDef.rof` | `Weapon.between_shots_s` | required | as proposed |
| `WeaponDef.laser_sight` | `Weapon.laser_sight` | false | |
| `WeaponDef.kind` | `Weapon.fire: Fire` | required | |
| `Projectile.class` | `Fire::Projectile.class` | `Automatic` | |
| `Projectile.recoil` | `Fire::Projectile.recoil` | 0 | |
| `Projectile.sound` | `Fire::Projectile.sound` | none | |
| `Projectile.spawn` | `Fire::Projectile.shot` (a `ShotSpawn`) | required | as proposed |
| `Beam.recoil` | `Fire::Beam.recoil` | 0 | |
| `Beam.beam`, flattened | the fields of `Fire::Beam` | | research offered flattening: flattened |
| `Special.special` | `Fire::Special.kind` | required | research: positional; `kind` is named |
| `Special.thrust` | `Fire::Special.thrust` | required | |
| `WeaponClass::{Automatic, PowerGun, Grenade, Missile, CloseRange, Other(i)}` | `WeaponClass::{..., Unknown(i)}` | | |
| `SpecialKind::{None, Jetpack, Other(i)}` | `SpecialKind::{Off, Jetpack, Unknown(i)}` | | `Off`, not `None` |
| `BeamDef.colour` | `Fire::Beam.colour` | required | |
| `BeamDef.damage` | `Fire::Beam.hole_size` | 0 | research: `hole_size` (or `carve`) |
| `BeamDef.player_damage` | `Fire::Beam.worm_damage` | 0 | as proposed |
| `BeamDef.length` | `Fire::Beam.length` | 0 | |
| `BeamDef.init_width` | `Fire::Beam.width` | 1 | |
| `BeamDef.width_increase` | `Fire::Beam.width_growth` | 0.0 | as proposed |
| `BeamDef.distribute_damage_over_width` | `Fire::Beam.split_damage_over_width` | false | as proposed |

### `ProjectileDef`, `ProjectileGfx`, `Animation` (§7, §8)

| Runtime type.field | Authoring field | Default | Note |
|---|---|---|---|
| (the map key) | `Projectile.id` | none: the key is derived from its place | never set in `projectiles/` files |
| (none) | `Projectile.notes` | empty | |
| `ProjectileDef.gfx` | `Projectile.gfx` | required | not `look` (§1.2) |
| `ProjectileDef.width`, `.height` | `Projectile.size: (w, h)` | (2, 2) for `Pixels`, (4, 4) for `Image` | |
| `ProjectileDef.gravity` | `Projectile.gravity` | none: world gravity | `Some(100)` is kept (§1.4) |
| `ProjectileDef.dampening` | `Projectile.velocity_factor` | 1.0 | research offered `dampening`: `velocity_factor` |
| `ProjectileDef.timer` | `Projectile.timer` | none | |
| `ProjectileDef.trail` | `Projectile.trail` | `Off` | |
| `ProjectileDef.hit` | `Projectile.hit_terrain` | empty list | as proposed |
| `ProjectileDef.player_hit` | `Projectile.hit_worm` | empty list | as proposed |
| `ProjectileDef.event_spawn` | `Projectile.spawns`: a map from name to `EventSpawn` | empty | at most one entry compiles |
| `Pixel.colours` | `Gfx::Pixels([colour, ...])` | | at most 256 |
| `Image.file` | `Gfx::Image.file` | required | research: positional; named |
| `Image.rotating` | `Gfx::Image.rotating` | false | |
| `Image.rot_increment` | `Gfx::Image.rotation_step_deg` | 0 | as proposed |
| `Image.rot_speed` | `Gfx::Image.rotation_speed` | 0 | as proposed |
| `Image.use_angle` | `Gfx::Image.frames_follow_heading` | false | |
| `Image.use_spec_angle` | `Gfx::Image.frames_follow_heading_mirrored` | false | |
| `Image.angle_images` | `Gfx::Image.heading_frames` | 0 | |
| `Image.animation` | `Gfx::Image.animation` | none | |
| `Animation.rate` | `Animation.fps` | 0.0 | |
| `Animation.kind` | `Animation.mode` | `Once` | research offered `end`: `mode` |
| `AnimKind::{Once, Loop, PingPong, Other(i)}` | `AnimMode::{..., Unknown(i)}` | | |

### `SpawnDef` (§9)

One type, `Spawn<C>`, with three contexts: `ShotSpawn` (a weapon's shot),
`EventSpawn` (`Projectile.spawns`) and `TrailSpawn` (`Trail::Projectile.spawn`).
Four defaults depend on the context, because LX56 fixes them there (§1.5):

| Field | Shot | Event | Trail |
|---|---|---|---|
| `relative_to_parent` | true | false | false |
| `speed_var_diagonal` | false | false | true |
| `random_rotation` | true | false | false |
| `inherit_velocity` | true | false | false |

| Runtime type.field | Authoring field | Default | Note |
|---|---|---|---|
| `SpawnDef.projectile` | `shared` (a shared id) or `projectile` (inline); exactly one | required | research: `projectile` for both |
| `SpawnDef.amount` | `amount` | 1 | OLX's constructor has 0 (§1.6) |
| `SpawnDef.speed` | `speed` | 0 | |
| `SpawnDef.speed_var` | `speed_var` | 0.0 | |
| `SpawnDef.spread` | `spread` | 0.0 | |
| `SpawnDef.use_angle` | `relative_to_parent` | by context | research offered `add_parent_heading` too |
| `SpawnDef.angle` | `angle` | 0 | |
| `SpawnDef.use_parent_velocity_for_spread` | `parent_velocity_as_direction` | false | as proposed |
| `SpawnDef.parent_vel_spread_factor` | `parent_velocity_direction_scale` | 0.3 | as proposed |
| `SpawnDef.use_special11_vec_for_speed_var` | `speed_var_diagonal` | by context | true in trails, false elsewhere |
| `SpawnDef.use_random_rot` | `random_rotation` | by context | |
| `SpawnDef.add_parent_vel` | `inherit_velocity` | by context | |
| `SpawnDef.parent_vel_factor` | `inherit_velocity_factor` | 1.0 | |

The canonical writer puts a spawn's parameters before `shared` or
`projectile`.

### `TimerDef`, `TrailDef` (§10, §11)

| Runtime type.field | Authoring field | Default | Note |
|---|---|---|---|
| `TimerDef.time` | `Timer.after_s` | required | |
| `TimerDef.time_var` | `Timer.variation_s` | 0.0 | |
| `TimerDef.action` | `Timer.effects` | empty list | needs `after_s` above 0 |
| `TrailDef.kind` | `Projectile.trail: Trail` | `Off` | |
| `TrailDef.delay` | `Trail::Projectile.every_s` | 0.1 | |
| `TrailDef.spawn` | `Trail::Projectile.spawn` (a `TrailSpawn`, inline or shared) | required | research: `spawn: <name>`; it is a spawn here |
| `TrailKind::{None, Smoke, ChemSmoke, Projectile, Doomsday, Explosive, Other(i)}` | `Trail::{Off, Smoke, ChemSmoke, Projectile{..}, Doomsday, Explosive, Unknown(i)}` | | `Off`, not absent; `Explosive` as proposed |

### `ActionDef` (§12)

An event (`hit_terrain`, `hit_worm`, `Timer.effects`) is a list of effects.
It compiles to LX56's single action: at most one effect besides `Spawn`,
and at most one `Spawn`.

| Runtime kind and fields | `Effect` variant | Default |
|---|---|---|
| `Bounce`: `bounce_coeff`, `bounce_explode` | `Bounce(factor, explode)` | 0.5, 0 |
| `Explode`: `damage`, `shake`, `sound` | `Explode(damage, shake, sound)` | 0, 0, none |
| `Injure`: `damage` | `Injure(damage)` | 0 |
| `Carve`: `damage` | `Carve(size)` | 0 |
| `Dirt`: `shake` | `Dirt(shake)` | 0 |
| `GreenDirt`: `shake` | `GreenDirt(shake)` | 0 |
| `Disappear` | `Halt` (renamed 2026-10-16: it does not remove the projectile) | |
| `Disappear2` | `Remove` | |
| `GoThrough` | `GoThrough` | |
| `InjureProj` | `InjureProjectile` | |
| `PlaySound`: `sound` | `Sound(file)` | none |
| `InjureWorm`: `damage` | `InjureWorm(damage)` | 0 |
| `ChangeRadius` | `ChangeRadius` | |
| `OverwriteOwnSpeed`, `MultiplyOwnSpeed`, `DiffOwnSpeed` | `SetSpeed`, `MultiplySpeed`, `AddSpeed` | |
| `OverwriteTargetSpeed`, `MultiplyTargetSpeed`, `DiffTargetSpeed` | `SetTargetSpeed`, `MultiplyTargetSpeed`, `AddTargetSpeed` | |
| `HeadingToNextWorm`, `HeadingToOwner` | `HomeOnNearestWorm`, `HomeOnOwner` | |
| `HeadingToNextOtherWorm`, `HeadingToNextEnemyWorm`, `HeadingToNextTeamMate` | `HomeOnNearestOtherWorm`, `HomeOnNearestEnemy`, `HomeOnNearestTeamMate` | |
| `HeadTargetToUs` | `PullTargetsToUs` | |
| `Other(i)` | `Unknown(i)` | |
| `Nothing` | an empty list | |
| `spawns_projectiles` | `Spawn(name)`, naming a key of `Projectile.spawns` | |

Research offered `Hurt` for `InjureWorm` and `Sparks` for `Explosive`; the names above were used.

### Projectile keys

A shared projectile's key is its file id. An inline projectile's key is its
`id` when it has one, otherwise one derived from its place: the weapon id for
a shot, then `<parent key>.spawn` or `<parent key>.trail` (issue #64: now
compact, `olr_data::mod_def::child_key`: `gun.s`, `gun.st`, `gun.s3t`). An event spawn
is always named `spawn` when lifted (`LIFTED_SPAWN`).
