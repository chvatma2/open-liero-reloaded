# Porting status

What has been ported from OpenLieroX (commit `d1ad953f`) and how it is verified.

| Area | OLX source | Rust | Status | Verified by |
|---|---|---|---|---|
| Gamescript v7 loader | `src/common/CGameScript.cpp:582-1152` | `rl-import-lgs` | ported | unit and property tests; Classic and MW 1.0 import exactly |
| `.lxl` image levels | `src/common/MapLoader_LieroX.cpp` | `rl-import-lxl` | ported | unit tests; 4 levels in scope, dirt counts match |
| Skin colorization | `src/client/CGameSkin.cpp:720-790` | `rl-data::skin`, `rl-import-gfx` | ported | unit tests; all 409 skins import |
| Theme loading | `src/common/CMap.cpp:308-370` | `rl-import-gfx` | ported | Dirt theme imports |
| Gamescript v8 | `CGameScript.cpp` (v8 branches) | — | not started | no content in scope |
| Pixmap `.lxl` | `MapLoader_LieroX.cpp:440+` | — | not started | no content in scope |
| Fidelity oracle (test hooks) | `src/game/Game.cpp`, `src/client/CInput.cpp`, `src/common/CWormHuman.cpp` | `tools/olx-oracle`, `rl-oracle` | done (plans 2–3) | 7 movement scenarios recorded twice and identical; `cargo xtask oracle-record --check` |
| Oracle v2: shots, weapons, bonuses, clocks (spec §18) | `src/common/CShootList.cpp`, `src/server/CServer_Send.cpp:556-569`, `src/server/CServer_Game.cpp:266-316`, `src/common/CGameScript.cpp:937,1159` | `tools/olx-oracle`, `rl-oracle` (`Fields`, `compare_fields`), `rl-scenario::CHECKED` | done (plan 4 task 1) | `fire-minigun` recorded twice and identical; the 7 M3 traces changed only in added fields |
| Oracle v2: message delivery (spec §18 D10); weapon, damage and bonus scenarios | `src/common/CChannel.cpp:69-85`, `src/client/CClient_Parse.cpp:468`, `src/server/CServer_Parse.cpp:90` | `tools/olx-oracle`, `xtask oracle-gen-weapons`, `fixtures/oracle/weapons/` | done (plan 4 task 3) | 88 weapon scenarios and 10 special ones recorded twice and identical; `rl-oracle/tests/fixtures.rs` checks what each shows |
| Frame and physics clocks | `src/game/Game.cpp:897-959`, `src/client/CClient_Game.cpp:60-222` | `rl-sim::World::tick` | ported | all 7 oracle scenarios, bit for bit; `docs/porting/timing.md` |
| Worm movement and collision | `src/common/PhysicsLX56.cpp:88-469` | `rl-sim::physics` | ported | `fall`, `walk`, `jump`, `dig` |
| Keyboard input (aim, move, jump, dig, rope) | `src/common/CWormHuman.cpp:81-560`, `src/client/CInput.cpp` | `rl-sim::physics::get_input`, `rl-sim::input` | ported (keyboard, LX56 aim) | `aim`, `tap`, `walk`, `jump`, `dig`, `rope` |
| Ninja rope | `src/client/CNinjaRope.cpp`, `PhysicsLX56.cpp:505-604` | `rl-sim::rope` | ported | `rope` |
| Hole carving (material) | `src/common/CMap.cpp:1005-1105`, `2172-2199` | `rl-terrain::Terrain::carve_hole` | ported | `dig` (244 dirt cells), spawn holes (`mh` at frame −1) |
| Float `sinf`/`cosf` (aim directions) | glibc 2.35 `sysdeps/ieee754/flt-32/s_sinf.c`, `s_cosf.c` (x86-64 FMA variant) | `rl-math::trig` | ported | all 2³² inputs bit-identical to the oracle container's glibc (a throwaway FFI comparator run in `rl-olx-src`) |
| Mod compiled to OLX's projectile tree (`SimMod`) | `src/common/CGameScript.cpp:833-1157` (`LoadProjectile` order) | `rl-sim::mod_rt` | ported (plan 4 task 4) | every scenario: indices and path names equal the trace header's (`rl-scenario` checks) |
| Weapon slots, firing, laser sight entity | `src/common/PhysicsLX56.cpp:471-498`, `src/server/CServer_Game.cpp:366-466`, `src/client/CClient_Game.cpp:496-544`, `751-808` | `rl-sim::weapons` | ported | `s` (slots) of all 106 scenarios for the whole trace (103 match fully); `beam-burst`; the laser sight event's length by a unit test |
| Shot list, encode/decode round trip, Model R delivery, `ProcessShot` | `src/common/CShootList.cpp`, `CClient_Game.cpp:813-946` | `rl-sim::shots`, `rl-sim::shot_codec` | ported | `s` of every scenario, `multi-shot` in particular (corrupted `spd`), `die-before-delivery` (dropped shots) |
| Beams (`ProcessShot_Beam`, `DrawBeam`) and specials (`ShootSpecial`, the jetpack) | `CClient_Game.cpp:549-711`, `926-1055` | `rl-sim::beam` | ported (plan 4 task 8; the bonus check task 9) | `weapons/classic/laser`, `weapons/mw-1-0/laser` and `beam-burst` match fully; specials by unit tests only (`jetpack_quirks`: no special weapon in Classic or MW 1.0) |
| Projectile pool, `Proj_SpawnInfo::apply` (shots and projectiles), `CProjectile::Spawn` | `include/FastVector.h`, `src/common/PhysicsLX56_Projectiles.cpp:495-640`, `src/common/CProjectile.cpp:43-166` | `rl-sim::projectile`, `rl-sim::spawn` | ported | every projectile field (`p`, hidden fields included) of every scenario for the whole trace; the pool limit by `pool_full_drops_new_projectiles_like_olx` |
| Projectile motion, collision, timers, trails, animation, worm hit test, position recorder | `PhysicsLX56_Projectiles.cpp:52-491`, `1234-1411`, `src/util/VecTimeRecorder.h`, `PhysicsLX56.cpp:468` | `rl-sim::proj_sim`, `rl-sim::worm::PosRecorder` | ported (plan 4 task 5) | as above; trails and animation by every weapon scenario (`mine`, `float-mine`, `nuclear-mines`, `trap-mines`, `booby-trap`, `electro-gun`, `jet-pack`, `big-nuke` for the timers and recorder) |
| Projectile actions (`applyTo`, `execute`): bounce, disappear, nothing, go-through, speed and heading actions, children, the junk rule | `PhysicsLX56_Projectiles.cpp:670-945`, `1115-1230`, `CProjectile.cpp:376-429` | `rl-sim::actions` | ported (plan 4 tasks 5-6) | all 88 weapon scenarios of both mods (`p`, `mh`, `dirt`, `hp`, `rc`); 103 of 106 scenarios match fully |
| Float `atan2f` (projectile headings) | glibc 2.35 `sysdeps/ieee754/flt-32/e_atan2f.c`, `s_atanf.c` | `rl-math::atan2f` | ported | 6 million input pairs bit-identical to the oracle container's glibc; 20 pinned in a test |
| Explosions, carve, dirt, green dirt and their dirt counts; injury actions | `CClient_Game.cpp:227-330`, `CMap.cpp:1114-1340`, `PhysicsLX56_Projectiles.cpp:781-824`, `1115-1154`, `CWorm.cpp:1381-1389` | `rl-sim::effects`, `rl-terrain::Terrain::{place_dirt, place_green_dirt}` | ported (plan 4 task 6; the bonus chain task 9) | `mh`, `dirt` and `hp` of every scenario; 103 of 106 scenarios match fully (the 3 D9 ones up to their respawn); `place_dirt_at_the_edges` and `green_dirt_at_the_edges` for the map edges |
| Worm damage (`InjureWorm`): factors, `SelfInjure`, damage score, blood draws | `CClient_Game.cpp:335-460`, `CGameObject.cpp:35-44`, `CWorm.cpp:1600-1627` | `rl-sim::damage::injure_worm` | ported (plan 4 task 7) | `hp`, `rc`, `ents` of every weapon scenario (`classic/*`, `mw-1-0/*`); factors and `SelfInjure` by unit tests only |
| Death, kills, deaths, suicides, lives; death draws (sound, gibs by the entity pool, blood); the exact-zero death (`CWorm::think`) | `CServer_Game.cpp:108-162`, `CGameMode.cpp:69-252`, `CWorm.cpp:1399-1449`, `src/gusanos/base_worm.cpp:365-381`, `623-659`, `src/client/Entity.cpp:161-205` | `rl-sim::damage::{kill_worm, die_at_zero_health}` | ported (plan 4 task 7); game over is not | `explode-kill-respawn`, `die-before-delivery`, the weapon scenarios' kills; exact-zero deaths in `assassin-s-shot`, `flak`, `plasma-launcher` |
| Bleeding | `PhysicsLX56.cpp:317-330` | `rl-sim::physics::bleed` | ported | `bleed`, `die-before-delivery` |
| Respawn loop, `SpawnWorm`, `FindSpot`, `CWorm::Spawn` | `CServer_Game.cpp:38-104`, `200-225`, `CMap.cpp:2381-2396`, `CWorm.cpp:591-651`, `CWormHuman.cpp:115-119` | `rl-sim::respawn` | ported (DM; no predefined spawns, no team spawns) | `explode-kill-respawn` and every weapon scenario whose dummy dies (`tod`, `respawn`, positions, `mt`) |
| Bonuses: server spawn, lifetime and grab handling; client spawn (hole), physics, pickup, health and weapon effects; the messages between them (D10); explosions and beams destroying bonuses | `src/server/CServer_Game.cpp:227-316`, `src/server/CServer_Parse.cpp:834-903`, `src/client/CClient_Parse.cpp:1464-1516`, `1977-1990`, `PhysicsLX56.cpp:619-715`, `CClient_Game.cpp:128-151`, `255-270`, `736-745`, `986-1002`, `CWorm.cpp:1454-1506` | `rl-sim::bonus`, `rl-sim::effects::explosion`, `rl-sim::beam` | ported (plan 4 task 9) | `bonus-health`, `bonus-weapon` (`b`, `sb` and every other field); natural spawning, lifetime, explosions and beams by unit tests only |
| Acceptance: all weapons of both mods | `fixtures/oracle/weapons/` | `rl-scenario/tests/oracle.rs` (`CHECKED == Fields::ALL`) | done (plan 4 task 11) | 103 of 106 scenarios match OLX bit for bit in every field; the other 3 are pinned to their first divergence (D9, below) |
| Tick events (sounds, map edits, explosions, cosmetics, shots, damage, kills, pickups, spawns) and the state checksum | every `StartSound`, `PlaySoundSample`, `SpawnEntity` and `SetWormBeamEntity` above; `src/game/Sounds.cpp:25-30` | `rl-sim::events`, `rl-sim::checksum` | done (plan 4 task 10) | unit tests; the oracle traces are unchanged (no draw added); `rl-scenario/tests/smoke.rs` (8 matches, clone checksum streams) |
| Event data for rendering (spec §19 E6): explosion `shake`, the gib's worm, the dirt particle's probe pixel, each event's sub-step (entity passes before it) and `World::substeps_last_tick`, `Damage.dealt`, the `RotIncrement` 0→1 bump (`World::rot_bumped`), `World::physics_time`, `Projectile::serial`, `NinjaRope::throws` (a throw count; not OLX state) | `CClient_Game.cpp:206-208`, `229-330`, `380`, `PhysicsLX56_Projectiles.cpp:590-593`, `777`, `1115-1130`, `CMap.cpp:2172-2199`, `PhysicsLX56.cpp:595-607`, `CProjectile.cpp:291-292` | `rl-sim::events`, `rl-sim::world` | done (plan 5 task 1) | `events_carry_e6_data`, `projectile_serials_differ_across_slot_reuse`, `every_rope_throw_counts_even_while_the_rope_is_out`; the oracle traces are unchanged (no draw added); smoke matches check every event's sub-step |
| Core pack: shared game sprites, game sounds (`.ogg`), `colours.cfg` (spec §19 E9) | `src/client/DeprecatedGUI/Graphics.cpp:56-97`, `134-321`, `src/common/CWorm.cpp:661`, `src/game/Sounds.cpp:25-30`, `src/common/StringUtils.cpp:409-447` | `rl-import-gfx::import_core`, `rl-pack::CorePack`, `rl-content::CorePack` | done (plan 5 task 1) | `rl-content/tests/content.rs` (`loads_every_manifest_item`), the corpus snapshot |
| Colour terrain (spec §19 E7): carve (back pixels, brown rim), `PlaceDirt` (`FrontTile`), `PlaceGreenDirt` (OLX's shades), beam holes, blood and gib stains, dirty rectangles | `src/common/CMap.cpp:1005-1340`, `2355-2373`, `src/client/Entity.cpp:355-390`, `src/client/GfxPrimitives.cpp` (`DrawImageStretch2`) | `rl-terrain::ColorMap`, `rl-content::{color_edit, apply_edits}` | done (plan 5 task 2) | `rl-terrain` colour tests; `rl-content/tests/color.rs` (`color_map_agrees_with_material_after_a_match`, 3000 ticks); golden `fixtures/render/dig-color.png` (`rl-scenario/tests/render_golden.rs`) |
| Presentation (spec §19 E1-E4): camera (hard lock, clamp, shake, dead worm, respawn), skin frames (the facing-left mirror one column off), crosshair, muzzle flash, old-style health bar, rope, laser sights, beams, projectile frames (animation, special angle, angle, rotation with the `RotIncrement` bump), bonuses (blinking), layer order; interpolation (E2) | `src/client/CViewport.cpp:118-472`, `src/common/CWorm.cpp:1016-1300`, `src/client/CGameSkin.cpp:584-600`, `src/client/CNinjaRope.cpp:132-180`, `src/common/CProjectile.cpp:227-300`, `src/common/CBonus.cpp:49-76`, `src/client/Entity.cpp:30-157`, `305-314`, `src/client/GfxPrimitives.cpp:1795-2520`, `src/gusanos/viewport.cpp:105-253` | `rl-scene` | done (plan 5 task 3) | `camera_locks_clamps_and_shakes_like_olx`, `skin_frame_matches_olx_table`, `projectile_frames_match_olx`, `interpolation_never_blends_reused_slots_or_respawns`, `draw_list_order_matches_olx_layers`, `beams_and_laser_sights_live_two_entity_passes`; scene snapshots (`rl-scene/tests/snapshots`) |
| Renderer (spec §8.2, §19 E3): art-scale world pass (terrain, instanced sprites from one atlas, world-pixel lines), upscale (integer nearest, sharp bilinear), offscreen PNGs; OLX's corner-rule colour key for sprites | `src/client/GfxPrimitives.cpp:116-150` (`SetColorKey`); OLX blits with SDL onto a 2× screen | `rl-render`, `rl-data::image`, `rl-cli render` | done (plan 5 task 5) | `dirty_rect_upload_equals_full_upload`, `upscale_filters_are_selectable`, `missing_assets_use_placeholders`, `srgb_targets_show_the_same_colours`; goldens `fixtures/render/frame-*.png` (`rl-cli/tests/render_golden.rs`, `cargo xtask full`) |
| Cosmetic entities (spec §19 E1, E8): the 1024-slot pool, per-sub-step physics and animation, stains, blood drips, gib spins and colours, drawing and clipping | `src/client/Entity.cpp:161-468`, `include/FastVector.h`, `src/common/CWorm.cpp:671-757`, `src/common/CMap.cpp:338-352` | `rl-fx` | done (plan 5 task 4) | `blood_falls_and_stains_like_olx`, `explosion_frames_and_invisible_start`, `gibs_use_the_victims_colour`, `entity_cap_drops_new_ones`, `animations_live_as_long_as_olx`, `fx_never_touches_sim_randomness`, `colour_edits_follow_the_sim_like_apply_edits` |
| Sound (spec §8.4, §19 E1, E12): positional mix (gain 0.7, inverse distance clamped, reference 50, rolloff 2, listener 20 px out at the view centre), THROW and PICKUP at the listener with gain 1.0, voice limits, live voices re-mixed as the listener moves, silence without a device | `src/sound/sound_sample_openal.cpp:206-270`, `src/sound/sfx.cpp:120-134`, `src/sound/sfxdriver_openal.cpp:43-92`, `src/game/Sounds.cpp:25-30`, `src/common/CGameScript.cpp:1185-1205` | `rl-audio` | done (plan 5 task 6) | `positional_gain_matches_olx_openal_defaults`, `voice_limits`, `silent_when_no_device`, `plays_events_on_a_mock_device`, `decodes_every_mod_sound`; `device_check` example (`cargo xtask full`) |
| Input devices (spec §8.5, §19 E5): OLX's player 1 keyboard and gamepad defaults, trigger and stick thresholds, presses and releases latched between ticks | `src/client/Options.cpp:45-73`, `src/client/CInput.cpp:200-226` | `rl-input` | done (plan 5 task 6) | `default_bindings_match_olx`, `gamepad_maps_to_keys`, `tap_between_ticks_is_latched`, `tick_latch_sees_every_press_and_release` |
| Game loop and sandbox (spec §8.6, §19 E10-E12): fixed 100 Hz tick, interpolated frames, catch-up cap, window surface states, WSL GPU defaults, F5 restart from assets loaded once, overlay | `src/game/Game.cpp:897-959` (the frame clock it drives) | `rl-game` | done (plan 5 task 7) | `loop_never_runs_more_than_n_catchup_ticks`, `surface_survives_resize_and_outdated`, `sandbox_setup_spawns_targets`, `session_plays_and_restarts_from_loaded_assets`; scripted runs in a WSLg window on all 4 levels × 2 mods |
| Long sessions (plan 5 review focus 5): 60 000 ticks (10 minutes) of the sandbox with chaos keys, `rl-fx`, mock audio and an offscreen render every 10th tick | — | `rl-game/tests/soak.rs` | done (plan 5 task 8) | `soak_sandbox_10_minutes_headless` (`cargo xtask full`, about 15 s): cosmetic entities, projectiles, entity count, audio voices, popups, instance buffer, atlas and terrain texture stay bounded, no panic |

## Deliberate deviations

- `rl-sim` supports LX56 aim only (`ForceLX56Aim`). It rejects wrap-around maps
  (`InfiniteMap`), Gusanos worm physics, `WormFriction > 0` and
  `ProjFriction > 0` (`SimSettings::from_olx_features` errors).
- Projectile actions assume a deathmatch (no team game): `TeamHit` is not
  read, an "enemy" is any other worm and there are no team mates.
- The green-dirt mask, OLX's shared `data/gfx/greenball.png`, is imported
  into the theme pack (optional `green_ball.png`). Without it green dirt
  places nothing, as in OLX.
- `rl-sim` does not keep the weapon name an injury passes on (OLX uses it only
  for the kill message).
- `rl-scenario` refuses a mod pack with an image projectile whose image is
  missing; OLX would run it without an image (no `ANI_ONCE` end, the junk rule).
- `rl-sim` rejects settings outside the ranges OLX's feature list gives
  (`src/common/FeatureList.cpp`), and NaN or infinite values
  (`SimSettings::validate`). Outside them the port could overflow, hang or
  recurse without bound. For hand-built settings, the physics step is at least 1 ms.
- **Game over is not ported** (`CGameMode::CheckGameOver`, `GameServer::GameOver`):
  no kill limit, time limit or last-worm-standing end, so `killWorm` and
  `SpawnWorm` never return early for it. `KillLimit` is read but unused. No
  scenario reaches it (`KillLimit` 15, `TimeLimit` 6 min).
- **The damage score (`fDamage`) is counted once**, on the client
  (`addDamage(.., false)` in `InjureWorm`). OLX also counts it a second time on
  the server, a frame or more later, from the batched `C2S_REPORTDAMAGE`
  (`ParseReportDamage`), so its `fDamage` is about twice the real damage. It
  only breaks score ties and is not in the trace.
- **Exact-zero deaths' Gusanos particles (spec §18 D9).** Three scenarios stay
  pinned in `rl-scenario/tests/oracle.rs`, the only ones that do not match
  bit for bit (each row there carries this reason: bit-exactness is impossible
  without porting the Gusanos particle system, whose `rnd()` draws shift the
  next random respawn spot). A worm left at exactly
  0 health dies at the start of the next frame, uncredited, as in OLX; the
  Gusanos `death.obj` particles OLX then spawns are not simulated. They draw
  from the `FindSpot` stream for as long as they live (597, 544 and 669 draws
  before the next respawn in `weapons/mw-1-0/{assassin-s-shot,flak,plasma-launcher}`),
  so later spawn and bonus positions differ. Those three scenarios diverge
  from their respawn frame on (`mt`, then the worm's position).
- **Blood amount and particles:** `rl-sim` uses OLX's defaults, `Game.Blood`
  100 and `Game.Particles` on (blood entities exist), which dedicated servers
  keep; `rl-scenario` refuses a trace header with other values.
- Only deathmatch: no team protection in `InjureWorm`, no teamkills, no
  `FindSpotCloseToTeam`, and LX levels have no predefined spawn points.
- **Bonus weapons ignore weapon restrictions.** OLX spawns weapon bonuses only
  with weapons that `WeaponRestrictionsFile` (default `Standard 100lt.wps`,
  which bans Bazooka, Grenade, Larpa, Laser, Minigun and more in Classic)
  does not ban. The restriction files are not imported yet, so every weapon
  is allowed unless `World::set_bonus_weapons` gives the list. Only natural
  spawns use it; the `bonus-natural` scenario records OLX with the `Bonus only`
  preset, which allows every Classic weapon, so it checks the spawns but not
  the restrictions.
- **The bonus spawn timer starts at the world's start** (`T0 + K`), as the
  oracle resets it when a recording starts; OLX sets it in `PrepareGame`.
- `World::place_bonus` places nothing when no server slot is free or the
  weapon id is outside the mod (the oracle reports an error; OLX's
  `CBonus::Spawn` would read the weapon name out of bounds).
- A grabbed weapon bonus does not call `WormShootEnd` for the replaced weapon
  if the worm holds fire: it acts only on a `FinalProj`, which LX56 mods do not have.
- **Message and GameOver events are not emitted yet** (spec §5.7): there is no
  game over (above), so no `GameOver`, and OLX's kill and other chat messages
  have no event. They come with Deathmatch in plan 6.
- **Sound ids** (`SimMod::sounds`): OLX's game sounds come first, each with the
  `olx:` prefix (a path in OLX's data directory); mod sounds are paths in the
  mod's pack and never carry it (a mod path that does is ignored).
- **Tick events report what a player's client shows** (spec §5.7): every
  cosmetic entity, even when the dedicated server's pool (D4) is full, and the
  local client's `ENT_SPAWN` sparkle when a worm (re)spawns, which takes no
  pool slot on a dedicated server. Two cosmetic details are left to the
  renderer, which uses its own generator (D5): the shades of green dirt
  (OLX picks them with `rand()`) (the laser sight's length is traced and carried in its event, as OLX
  passes it to the entity). `WeaponFired` and the weapon's
  sound come when the client processes the shot (Model R delivery), as in OLX.
  A tick reports at most 7168 sounds and cosmetics, 32768 terrain edits and
  8192 other gameplay events, each class in its own quota; the rest are
  counted (`TickEvents::cosmetic_dropped`, `gameplay_dropped`), and the area
  of dropped terrain edits is reported (`TickEvents::lost_area`) for the
  renderer to rebuild from the material grid.
- **A zero `RotIncrement` is set to 1 per world, not in the mod.** For a
  rotating image projectile spawned with `UseRandomRot`, OLX writes
  `RotIncrement = 1` into the loaded `proj_t` before drawing the rotation
  (`PhysicsLX56_Projectiles.cpp:590-593`), and the change lasts as long as the
  mod stays loaded, across games. The simulation draws the same rotation,
  leaves `SimMod` as loaded and records the bump in the world
  (`World::rot_bumped`, spec §19 E6), so a renderer spins such a projectile
  as `CProjectile::Draw` does (`CProjectile.cpp:291-292`) from the first such
  spawn of the match, where OLX would remember it from an earlier game.
  Cosmetic only, and no Classic or MW 1.0 projectile is rotating with a zero
  increment.

- **The oracle is not a plain dedicated server** (spec §18, `tools/olx-oracle/CLAUDE.md`):
  - shots are delivered by a hook between frames (Model R), not over UDP;
  - while recording, `tLX->currentTime` is `simulationTime + 10 ms`;
  - `simulationTime` restarts at 100 000 ms when the inputs load (`T0` in the
    trace header). OLX starts it at the wall clock, about 0.4 s after launch,
    which made bleeding (`fLastBlood + 2 s` with `fLastBlood = 0`) start at a
    different frame in every run;
  - projectile images are loaded in full in dedicated mode, so animations end
    as on a player's client;
  - the entity pool is cleared and the server's bonus timer reset when a recording starts.
  - the local connection's reliable messages (bonus spawns and destruction, bonus
    grabs, damage reports, kill messages) are handed over after the frame they
    were queued in (D10), not over UDP;
  - scenarios place bonuses with `simPlaceBonus` and stay shorter than
    `BonusFrequency` (30 s), so the server never spawns one at a random spot,
    except `bonus-natural`, which sets `BonusFrequency` to 1 s.

- **`SimMod` limits** (`rl-sim::mod_rt`): a pack's projectile tree may have at
  most 4096 occurrences after expansion and 128 levels, and a spawn amount at
  most 1000 (Classic and MW 1.0 use at most 150); a missing reference is an error. OLX loads anything and
  crashes on a missing projectile. A projectile that spawns an ancestor (a
  cycle, which only hand-made packs can express) refers back to it.
- **Spawn time past frame 819 205** (about 2.3 h): when the decoded shot time is
  later than the server time, OLX falls back to `currentTime - ping / 2`
  (`shoot_t::spawnTime`). The port takes the local client's ping as 0.
- `rl-sim` delivers the shot list at the start of `World::tick`, before
  `serverFrame` advances; that is the oracle's `betweenFrames` moment, and a
  frame's state is recorded before it, as in OLX's trace.

- Importers reject malformed input that OLX would read as garbage: truncated
  files, unknown weapon or projectile types, and nesting deeper than 128 levels.
- A missing projectile image is an import error. OLX would load the mod and draw nothing.
- Imported level art is stored at 1×, because OLX's 2× surfaces are plain pixel doubling.
- **Colour terrain** (`rl-terrain::ColorMap`): the map keeps its own copy of
  the material grid and replays each edit on it after painting, so the colour
  rules read the material before the edit, as OLX's single loop does (the plan
  had `apply(edit, material_after)`, which cannot tell a carved pixel from one
  that was background already, such as a gib stain on background). Green-dirt
  shades come from the map's own generator (spec §18 D5). The composite is
  opaque (OLX's draw surface has no alpha). A gib stain also skips fully
  transparent source pixels besides OLX's magenta colour key. Terrain edits a
  tick drops (`lost_area`) are redrawn from the material grid
  (`ColorMap::rebuild_area`): back image, `FrontTile`, level art; stains and
  the level's own dirt art there are lost.
- **Scene** (`rl-scene`, plan 5 task 3):
  - *Interpolation* (E2, the default) draws positions between the last two
    ticks at sub-pixel precision, without OLX's `(int)` truncation; the
    camera follows the blended position. `Interpolation::Latest` is OLX's
    whole-pixel drawing of the latest state.
  - *Per tick, not per drawn frame:* OLX shows the muzzle flash and the
    green crosshair for one drawn frame (about one tick at its 95 FPS cap);
    the scene shows them for every frame drawn of that tick. The shake's
    0.2 s runs on the world's clock.
  - *Lines at world resolution* (E3): the rope is the non-antialiased one,
    alternating `RopeColour2`, `RopeColour1` per world pixel from the hook;
    beams are solid 1-pixel lines (a wide beam, which LX56 mods cannot
    define, is drawn as its centre line); laser dots run OLX's recurrence
    per screen pixel and land on world pixels.
  - *Laser sights* are drawn after the other entities (OLX draws them in
    entity slot order); the green crosshair is recomputed from the
    trace's end (a pixel that lets particles through before 9999 means a
    worm stopped it), and the event is matched to the nearest alive worm.
  - *Health bar:* `worm_health.png` is not imported, so the bar is OLX's
    old-style fallback (five segments in a grid).
  - With interpolation, a hook attached to a worm is drawn on that worm
    as drawn, and laser sights and beams start at their worm as drawn (OLX
    draws them from the worm's position in the sub-step, which matches its
    unblended worm); bonuses are blended too.
- **Entities** (`rl-fx`, plan 5 task 4):
  - *Dirt colour of a bonus chain:* OLX reads an explosion's dirt colour
    before the bonuses it destroys explode, but the sim reports the chained
    explosions (and their carves) before that explosion's particles. A
    particle whose probe is no longer dirt when `rl-fx` reaches it takes the
    theme's `iDefaultColour` instead of the colour before the carve.
  - Entities are drawn at their latest state, not interpolated (E2 applies
    to worms, projectiles and bonuses).
  - A falling entity whose position is not finite is removed (OLX's
    `(uint)` cast of NaN is undefined), so it cannot hold a slot forever.
  - Only the air jump's sparkles exist in the sim (`Color()`, fade 10), so
    the coloured-sparkle drawing (`EntityEffect`) is not ported.
- **Sound** (`rl-audio`, plan 5 task 6):
  - *Pan:* OLX gets stereo from OpenAL Soft's panning of the 3D source
    direction; the remake pans by the direction's left-right component
    `dx / d` with kira's constant-power law. Gains are OpenAL's exactly.
  - *Global voice limit:* OLX's only global limit is OpenAL's source count;
    the remake skips a start beyond 256 voices (OpenAL Soft's default).
  - A voice that ended counts against its sound's limit until the next
    tick's play call, as OLX's copies count until the next `Sfx::think`.
  - The announcer, team-score and menu sounds are not played (the sim does
    not emit them yet).
- **Input devices** (`rl-input`, plan 5 task 6):
  - OLX's twin-stick controls (left stick walks, right stick aims
    analogue) are not ported: the sim has 8 digital keys. The left stick's
    X axis presses Left/Right and the right stick's Y axis Up/Down, from
    OLX's 60 % deadzone.
  - Mouse aiming (spec §8.5) is not implemented yet.
  - Between ticks, a key released and pressed again is up for one tick and
    down for the next, so the sim sees both edges (OLX's `CInput` reads the
    devices every sub-step and needs no tick latch).
- **Game loop** (`rl-game`, plan 5 task 7): the 100 Hz ticks are drawn
  at the display's rate with interpolation (spec §8.6); at most 10 ticks
  run per drawn frame, and the rest of a longer stall is dropped. The
  sandbox HUD, names and damage popups are egui text at the display's
  resolution (E10), not OLX's bitmap fonts.
- **Renderer** (`rl-render`, plan 5 task 5): OLX blits each sprite with SDL
  onto a 2× screen; the renderer draws the same texels at art scale (at
  least 2×) and then upscales (E3). With interpolation (E2), a sprite's
  blended position snaps to the nearest art pixel. A 1× level's art is
  doubled with nearest sampling. Blending is on the stored 8-bit values,
  like SDL's.
- **One terrain texture** (spec §8.2, E7): the renderer uploads the
  `ColorMap` composite as one opaque RGBA texture, not separate back, front
  and material textures with a shader blend as §8.2 sketches; the composite is
  made on the CPU (E7), and edits upload as dirty rectangles. Recorded as an
  amendment under spec §19.
- **Render goldens** are compared at YIQ distance 0.1 with at most 0.002 % of
  pixels differing (about 6 at 640x480). On this machine lavapipe (Vulkan) and
  llvmpipe (GL) reproduce all four goldens with no differing pixel.
- **Bless without a GPU:** `cargo xtask bless-render` re-draws the CPU golden
  and, when no adapter opens, skips the GPU goldens with a notice; it records
  what changed in `fixtures/blessings.log` even when a later test fails.

## Spec §18 decisions that deviate from a plain OLX run

- **D3, client animation semantics.** `ANI_ONCE` deletes projectiles when
  their animation ends and action-less animated projectiles live until then,
  as on a player's client (the oracle loads images in dedicated mode).
  `SimMod` carries each image projectile's frame count.
- **D4, cosmetic `rand()` draws are emulated** (dig, rope hook, explosion dirt,
  blood, bleeding, death, gibs, colour), including the dedicated server's
  1024-entry entity pool that is never freed. A real client frees entities
  and simulates them with `rand()`, which no port can reproduce, so a client
  game's draw count differs from the traces.
- **D7, the `VecTimeRecorder::get(0)` quirk is kept** (see Kept OLX quirks).
- **D9, exact-zero health** (see above): the death is ported, the Gusanos
  particles' `rnd()` use is not.

## Kept OLX quirks

- **The physics step is 11 ms** (`1000 / 84`, integer division), not 1/84 s.
- **The lossy per-frame Gusanos speed round trip** (`GusSpeedScope`) on worm and
  rope velocities, from `gusLogicFrame()` (`docs/porting/timing.md`).
- **The collision scan continues instead of breaking** at the left and right
  map edges, and breaks at the top edge (`PhysicsLX56.cpp:132-206`). OLX's
  comments note that LX56 differed here.
- **`CarveHole` clipping:** near the map edge the stamp is read from its top-left
  corner, not offset by the clipped amount. `PlaceDirt` and `PlaceGreenDirt`
  do offset it.
- **`PlaceDirt` counts twice:** a background cell under an edge-coloured
  (non-magenta, non-black) hole pixel adds 2 to the placed count, so to the
  owner's dirt count.
- **Explosions have no falloff:** every alive worm whose recorded position is
  within 5 px (`d² <= 25`) takes the full damage. The hole is
  `clamp((int)damage, 0, 4)`; on x86 `(int)` of NaN or a huge value is
  `INT_MIN`, so those carve the smallest hole.
- **A timer explosion takes `PlyHit`'s damage** when `PlyHit` is an explosion
  (`projectile_doTimerExplode`).
- **Projectile dirt** is three `PlaceDirt(5, ...)` (the largest hole) at
  `pos - (6,6)`, `pos + (6,-6)`, `pos + (0,6)`, added in float and truncated
  toward zero.
- **`(Uint64)` of a float** (`TimeDiff(float)`, `AbsTime + float`) is GCC's
  x86-64 sequence: negative values and NaN wrap through a 64-bit `cvttss2si`,
  infinity and values from 2^64 on give 0 (`rl-sim::projectile::c_u64`).
- **`VecTimeRecorder::get(0)` is the oldest entry** (spec §18 D7): with the
  pinned clock, a projectile step at exactly the frame time hit-tests worms
  where they were 100 physics steps ago, or at a stale or zero position in
  their first 100 steps after a spawn.
- **`Attr` assignments skip equal values** (`src/game/Attr.h:194-204`):
  `vPos = v` and `vVelocity = v` of a projectile or worm do nothing when `v`
  compares equal, so a zero keeps its old sign. A reused projectile slot can
  start with the previous projectile's `-0` velocity (`nuclear-mines`, frame 206).
  `rl-sim` follows this for projectiles (`attr_set`).
- **LX56's `Bounce`:** each collision side sets both factors, and left/right
  override top/bottom (`CProjectile.cpp:376-429`).
- **Wall shooting:** for the first 10-12 ms a projectile ignores the terrain and
  does not save its position, so a later collision resets it to its spawn position.
- **Beams carve twice over.** The delivered shot (`ProcessShot_Beam`) traces
  from the shot's 12-bit position along its whole-degree angle, injures the
  first other worm within 5 px (`d² < 25`) or carves at the first dirt or
  rock (outside the map is rock). `DrawBeam` runs in every sub-step its
  slot's `LastFire` is above 0 (0.3 s after each delivery), from the worm's
  current position at its unrounded aim: it carves too, but a worm only
  stops it. The 0.3 throttles the server's fire gate, so beams fire in bursts.
- **A negative beam width becomes the map's smaller side:** `MIN(int, uint)`
  resolves to `MIN(unsigned long, unsigned int)` (`include/MathLib.h:68`).
- **The jetpack never thrusts with ROF > 0** in a local or hosted game:
  `WormShoot` sets `LastFire` first, so `PlayerShoot` returns. With ROF ≤ 0
  it drains twice per sub-step (`WormShoot`'s `<= 0` rule, then
  `ShootSpecial`'s `< 0` rule, which ignores `DisableWpnsWhenEmpty`).
- **Bonuses exist twice.** The server's slot stays used for the whole
  `BonusLife`, even after a pickup or an explosion (which the server is never
  told about), so a new bonus takes the next slot. The client's copy has the
  position truncated to `int` (sent as 16 bits) and weapon 0 for a health
  bonus; the server's has the float `FindSpot` position and, for a natural
  health bonus, a random weapon (-1 when placed). Only the client carves a
  spawn hole.
- **A grabbed weapon is set twice:** at the pickup in the current slot, and
  one frame later by the server in the slot that was current at the pickup,
  full and not reloading again (`ParseGrabBonus`).
- **Bonus physics:** each sub-step after the projectiles, gravity 80, no
  sideways motion. The 5 × 5 box is scanned from the top row; at the first
  dirt or rock pixel (or the bottom or right edge; the left edge is skipped)
  the bonus stops 2 px above that row, so a bonus buried in its top row climbs
  2 px a sub-step.
- **Explosions destroy client bonuses within 15 px** on both axes before their
  own particles and damage; each destroyed bonus explodes again at the same
  position (damage 15). A beam stops within 3 px of a bonus in an explosion of
  damage 0, even with `Bonuses` off (then the bonus survives).
- **A health bonus heals `GetRandomInt(40) + 10`**, at most up to 100, through
  `InjureWorm` (so the worm's damage and shield factors apply); a worm at 100
  or more leaves it on the map.
- **The "junk projectile" rule:** a projectile with no hit, worm-hit or timer
  action is deleted after its first step, unless it ends by an `ANI_ONCE`
  animation of a loaded image.

- `ROF /= 1000` and `Recharge /= 10` are applied after loading (`CGameScript.cpp:776-777`).
- Beam and pixel colors stored as ints are truncated to bytes, as with OLX's `Color(Uint8, …)`.
- Front pixels where the `PX_EMPTY` flag is set become the back pixel, even when other flags are set.
- **Colour key with a corner rule.** OLX uses (254,0,254) as the colour key when
  any corner pixel of the image has that colour, otherwise (255,0,255)
  (`SetColorKey`, `src/client/GfxPrimitives.cpp:116-150`).
  - Skins: applied at import.
  - Core sprites, mod projectile images and gibs: stored raw and keyed when
    `rl-scene`/`rl-fx` load them (`RgbaImage::key_transparent`, the same rule).
    One affected file in scope is `skins/Wario.png`; no core sprite, mod image
    or theme image at `d1ad953f` has a (254,0,254) corner.
- **Hi-res level art.** With an "OLX hi-res data" chunk, the front 2×2 block
  becomes the back block wherever `PX_EMPTY` is set, and PNG alpha is ignored
  (`LoadLevelImageHiRes`, `MapLoader_LieroX.cpp:285-360`).
- **Entities** (`rl-fx`): a large explosion (size > 15) starts before the
  image and stays invisible (10 sub-steps for size 20); frame sums are
  floats and drift low (a size-8 explosion is at frame 10 after 10 passes);
  chemsmoke's and spawn's last frames are past their images and draw
  nothing; a blood dropper that hits the map still drips in that pass; a
  jetpack spray that hits the map in the pass its frame ends is freed twice,
  which can hide the entity below it from the pool's iteration until a
  higher slot is used (`FastVector::refreshObj`).

## Facts the oracle established (needed by the simulation port)

- **Frames:** a frame is 10 ms. LX56 physics runs in 11 ms sub-steps (`1000 / 84`) from its
  own clock (`CClient::fLastSimulationTime`), so some 100 Hz frames run no
  physics sub-step at all. After the oracle aligns that clock when a scenario
  starts, frames 0 and 1 run none, and the first sub-step comes in frame 2.
  The step is 11 ms, not 1/84 s (see `docs/porting/timing.md`).
- **Input:** OLX's keyboard handler (`CWormHumanInputHandler::getInput`,
  `src/common/CWormHuman.cpp:81-560`) runs once per physics sub-step and
  latches keyboard events between sub-steps, then clears the latches with
  `CInput::reset()`.
  - **Held:** a key counts as held if it is down now *or* was pressed since the
    previous sub-step (`CInput::Binding::isDown`: `wasDown() || bDown`,
    `src/client/CInput.cpp:861-862`). A tap shorter than the gap between two
    sub-steps is still seen once (oracle scenario `tap`).
  - **Pressed once:** `isDownOnce` (`nDownOnce`) is true at the first sub-step after a press.
  - The simulation's `PlayerInput` must keep both latches.
- **Digging** with the keyboard is a tap of the opposite direction while
  holding a direction (`leftOnce` / `rightOnce`), not a dedicated key.
- **Shot runs:** consecutive shots share a packet only while worm, weapon,
  time (≤ 255 ms), angle, `nSpeed` and the integer worm velocity each change
  by at most 255 between neighbours. `nSpeed` is the worm's speed × 100, so a
  speed change of 2.55 px/s already splits a pair (`multi-shot`, frames 59–63).
  The decoded `spd` in a trace is not `nSpeed`: the header keeps it modulo 256
  above 510, and a small shot gets a wrapped extra speed byte through the
  misplaced `SHF_XWRMVEL` flag.
- **Shots:** a Minigun shot fired in frame 2's sub-step (the first one) is
  delivered after frame 3 and its projectile appears in frame 4's trace line
  (`fire-minigun`). Shots of different worms form separate runs, so separate
  packets (`two-worms-fire`). In a multi-shot packet every later shot reuses the
  first shot's position and random seed (`multi-shot`).
  - A shot delivered after its worm died spawns nothing (`die-before-delivery`).
  - When a beam shot arrives, the slot's `LastFire` becomes 0.3, so a held beam
    fires in bursts of two shots every 17–18 frames (`beam-burst`).
  - Only the current slot reloads; a reloading slot that is not selected keeps
    its charge (`reload-switch`).
- **Death and respawn:** a worm killed in frame F is respawned by the server in
  frame F + 250 at a `FindSpot` position (one try, 2 `rnd()` draws): RespawnTime
  2.5 s, compared strictly with the pinned clock (`explode-kill-respawn`). The
  respawn is synchronous: the local client shares the server's worm, and
  `SendSpawnWorm` sends it nothing.
- **Deaths at a score of 0 are not counted** without `AllowNegativeScore`
  (`die-before-delivery`, most weapon scenarios: the dummy gets no death);
  a killer that is already dead still gets the kill.
- **Gibs draw 4 `rand()` while the entity pool has room, 2 after** (the
  dedicated server's pool never frees; `disease-bomb`, `nuclear-bomb` and
  `sunray-beamer` kill with a full pool). The gib image exists in dedicated
  mode (`explode-kill-respawn`'s 580 draws).
- **The position recorder survives a respawn** (`clear(vPos)`): `get(0)` is
  a position from the previous life (D7).
- **Bleeding:** below 15 hp a worm bleeds in the first sub-step and then every
  2 s, 80 `rand()` calls and 40 entities each time (`bleed`).
- **Bonuses:** a grab reaches the server one frame later, and its
  S2C_DESTROYBONUS the client one more frame later; the server keeps its copy
  until BonusLife ends (`bonus-health`, `bonus-weapon`).
- **Aim:** the traces use LX56 aim (`ForceLX56Aim`: max speed 100,
  acceleration 500, no friction, `REDUCE_CONST(200)` when no key is held). The
  angle is clamped to [−90°, 60°].
