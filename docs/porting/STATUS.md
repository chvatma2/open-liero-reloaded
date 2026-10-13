# Porting status

What has been ported from OpenLieroX (commit `d1ad953f`) and how it is verified.

| Area | OLX source | Rust | Status | Verified by |
|---|---|---|---|---|
| Gamescript v7 loader | `src/common/CGameScript.cpp:582-1152` | `rl-import-lgs` | ported | unit and property tests; Classic and MW 1.0 import exactly |
| `.lxl` image levels | `src/common/MapLoader_LieroX.cpp` | `rl-import-lxl` | ported | unit tests; 4 levels in scope, dirt counts match |
| Skin colorization | `src/client/CGameSkin.cpp:720-790` | `rl-data::skin`, `rl-import-gfx` | ported | unit tests; all 409 repository skins and 0.58 rc5's 309 more (`content/olx-0.58-rc5/`) import |
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
| Weapon slots, firing, laser sight entity | `src/common/PhysicsLX56.cpp:471-498`, `src/server/CServer_Game.cpp:366-466`, `src/client/CClient_Game.cpp:496-544`, `751-808` | `rl-sim::weapons` | ported | `s` (slots) of all 112 scenarios for the whole trace (109 match fully); `beam-burst`; the laser sight event's length by a unit test |
| Shot list, encode/decode round trip, Model R delivery, `ProcessShot` | `src/common/CShootList.cpp`, `CClient_Game.cpp:813-946` | `rl-sim::shots`, `rl-sim::shot_codec` | ported | `s` of every scenario, `multi-shot` in particular (corrupted `spd`), `die-before-delivery` (dropped shots) |
| Beams (`ProcessShot_Beam`, `DrawBeam`) and specials (`ShootSpecial`, the jetpack) | `CClient_Game.cpp:549-711`, `926-1055` | `rl-sim::beam` | ported (plan 4 task 8; the bonus check task 9) | `weapons/classic/laser`, `weapons/mw-1-0/laser` and `beam-burst` match fully; specials by unit tests only (`jetpack_quirks`: no special weapon in Classic or MW 1.0) |
| Projectile pool, `Proj_SpawnInfo::apply` (shots and projectiles), `CProjectile::Spawn` | `include/FastVector.h`, `src/common/PhysicsLX56_Projectiles.cpp:495-640`, `src/common/CProjectile.cpp:43-166` | `rl-sim::projectile`, `rl-sim::spawn` | ported | every projectile field (`p`, hidden fields included) of every scenario for the whole trace; the pool limit by `pool_full_drops_new_projectiles_like_olx` |
| Projectile motion, collision, timers, trails, animation, worm hit test, position recorder | `PhysicsLX56_Projectiles.cpp:52-491`, `1234-1411`, `src/util/VecTimeRecorder.h`, `PhysicsLX56.cpp:468` | `rl-sim::proj_sim`, `rl-sim::worm::PosRecorder` | ported (plan 4 task 5) | as above; trails and animation by every weapon scenario (`mine`, `float-mine`, `nuclear-mines`, `trap-mines`, `booby-trap`, `electro-gun`, `jet-pack`, `big-nuke` for the timers and recorder) |
| Projectile actions (`applyTo`, `execute`): bounce, disappear, nothing, go-through, speed and heading actions, children, the junk rule | `PhysicsLX56_Projectiles.cpp:670-945`, `1115-1230`, `CProjectile.cpp:376-429` | `rl-sim::actions` | ported (plan 4 tasks 5-6) | all 88 weapon scenarios of both mods (`p`, `mh`, `dirt`, `hp`, `rc`); 109 of 112 scenarios match fully |
| Float `atan2f` (projectile headings) | glibc 2.35 `sysdeps/ieee754/flt-32/e_atan2f.c`, `s_atanf.c` | `rl-math::atan2f` | ported | 6 million input pairs bit-identical to the oracle container's glibc; 20 pinned in a test |
| Explosions, carve, dirt, green dirt and their dirt counts; injury actions | `CClient_Game.cpp:227-330`, `CMap.cpp:1114-1340`, `PhysicsLX56_Projectiles.cpp:781-824`, `1115-1154`, `CWorm.cpp:1381-1389` | `rl-sim::effects`, `rl-terrain::Terrain::{place_dirt, place_green_dirt}` | ported (plan 4 task 6; the bonus chain task 9) | `mh`, `dirt` and `hp` of every scenario; 109 of 112 scenarios match fully (the 3 D9 ones up to their respawn); `place_dirt_at_the_edges` and `green_dirt_at_the_edges` for the map edges |
| Worm damage (`InjureWorm`): factors, `SelfInjure`, damage score, blood draws | `CClient_Game.cpp:335-460`, `CGameObject.cpp:35-44`, `CWorm.cpp:1600-1627` | `rl-sim::damage::injure_worm` | ported (plan 4 task 7) | `hp`, `rc`, `ents` of every weapon scenario (`classic/*`, `mw-1-0/*`); factors and `SelfInjure` by unit tests only |
| Death, kills, deaths, suicides, lives; death draws (sound, gibs by the entity pool, blood); the exact-zero death (`CWorm::think`) | `CServer_Game.cpp:108-162`, `CGameMode.cpp:69-252`, `CWorm.cpp:1399-1449`, `src/gusanos/base_worm.cpp:365-381`, `623-659`, `src/client/Entity.cpp:161-205` | `rl-sim::damage::{kill_worm, die_at_zero_health, olx_kill_credit}` | ported (plan 4 task 7); the crediting is the game mode's since plan 6 task 1 (`GameMode::on_kill`; `OracleMode` is OLX's rule) | `explode-kill-respawn`, `die-before-delivery`, the weapon scenarios' kills; exact-zero deaths in `assassin-s-shot`, `flak`, `plasma-launcher` |
| Bleeding | `PhysicsLX56.cpp:317-330` | `rl-sim::physics::bleed` | ported | `bleed`, `die-before-delivery` |
| Respawn loop, `SpawnWorm`, `FindSpot`, `CWorm::Spawn` | `CServer_Game.cpp:38-104`, `200-225`, `CMap.cpp:2381-2396`, `CWorm.cpp:591-651`, `CWormHuman.cpp:115-119` | `rl-sim::respawn` | ported (DM; no predefined spawns, no team spawns) | `explode-kill-respawn` and every weapon scenario whose dummy dies (`tod`, `respawn`, positions, `mt`) |
| Bonuses: server spawn, lifetime and grab handling; client spawn (hole), physics, pickup, health and weapon effects; the messages between them (D10); explosions and beams destroying bonuses | `src/server/CServer_Game.cpp:227-316`, `src/server/CServer_Parse.cpp:834-903`, `src/client/CClient_Parse.cpp:1464-1516`, `1977-1990`, `PhysicsLX56.cpp:619-715`, `CClient_Game.cpp:128-151`, `255-270`, `736-745`, `986-1002`, `CWorm.cpp:1454-1506` | `rl-sim::bonus`, `rl-sim::effects::explosion`, `rl-sim::beam` | ported (plan 4 task 9) | `bonus-health`, `bonus-weapon` (`b`, `sb` and every other field); natural spawning, lifetime, explosions and beams by unit tests only |
| Acceptance: all weapons of both mods | `fixtures/oracle/weapons/` | `rl-scenario/tests/oracle.rs` (`CHECKED == Fields::ALL`) | done (plan 4 task 11) | 109 of 112 scenarios match OLX bit for bit in every field; the other 3 are pinned to their first divergence (D9, below) |
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
| Match API (spec §20 F6, F9): `MatchSetup`, `GameMode` callbacks and commands, `Message` and `GameOver` events; game over's effects on the simulation (kills ignored, no respawns or bonus spawns, no input or shooting, the client's projectiles cleared, the physics stopped after `GAMEOVER_WAIT`) | `src/server/CGameMode.cpp:69-252`, `src/server/CServer.cpp:532-625`, `src/server/CServer_Game.cpp:38-42`, `108-113`, `167-261`, `src/client/CClient_Game.cpp:76-91`, `116-117`, `src/common/PhysicsLX56.cpp:300`, `src/client/CClient_Parse.cpp:1436-1451`, `src/game/Game.cpp:1267-1276`, `include/FastVector.h:72-77`, `src/common/Physics.cpp:36-44` | `rl-sim::{setup, mode, referee}` | done (plan 6 task 1) | `oracle_mode_matches_the_hard_coded_credit` and the oracle (unchanged), `mode_commands_apply_inside_tick_deterministically`, `game_over_stops_kills_spawns_input_and_bonuses`, `give_weapon_mid_match_does_not_allocate`; the oracle scenario `dm-kill-limit` (plan 6 task 2): the end in the kill's frame, the client's projectiles cleared, input ignored, no respawn, the physics stopped 300 frames later (`physics_stops_at_exactly_three_seconds_like_olx`) |
| Deathmatch (`rl-modes`): kill bookkeeping and texts (first blood, killing and dying sprees, out), `CheckGameOver` (kill limit, time limit, fewer than two not out), `Winner`/`HighestScoredWorm`/`CompareWormsScore`, the results order, `Lives` and the respawn gate | `src/server/CGameMode.cpp:31-434`, `src/server/CDeathMatch.cpp`, `src/server/CServer.cpp:435-625`, `src/server/CServer_Game.cpp:167-260`, `src/client/Options.cpp:243-253`, `787-822`, `src/common/MathLib.cpp:85-88`, `src/client/CClient_Draw.cpp:1840-1897` | `rl-modes::{Deathmatch, rules, text}` | done (plan 6 task 2) | `deathmatch_game_over_and_winner_like_olx`, `lives_out_removes_worm_from_respawn`, `spree_messages`, `compare_like_olx`, `check_order_like_olx`, `deathmatch_is_deterministic_and_allocation_free`; the oracle scenario `dm-kill-limit` (KillLimit 2, a double kill: the end and its winner, `go`, match OLX bit for bit) |
| Weapon restrictions (spec §20 F4): `.wps` presets imported into mod packs (`cfg/presets/<mod dir>/`, then `cfg/presets/`), `CWpnRest` (`loadList`'s parsing and path rule, `updateList`, `findWeapon`, `isEnabled`, `getWeaponState`), natural bonuses from the weapons not banned (`goodWpns`) | `src/common/CWpnRest.cpp:59-277`, `include/CWpnRest.h:28-32`, `src/game/Game_loadWeaponRestrictions.cpp:16-28`, `src/server/CServer_Game.cpp:292-297`, `src/game/Game.cpp:1405-1420`, `src/common/CGameScript.cpp:1211-1231` | `rl-import-lgs::{parse_wps, import_presets}`, `rl-data::restrictions`, `rl-pack` (`restrictions.ron`, format 3), `rl-content` (`LoadedMod::weapon_rules`, `SandboxSetup::restrictions`), `rl-scenario` (the header's `WeaponRestrictionsFile`) | done (plan 6 task 3) | `wps_presets_enable_what_olx_enables` (every bundled Classic and MW 1.0 preset), `parses_like_load_list`, `lookups_follow_cwpnrest`, `natural_bonuses_hold_what_the_restrictions_allow`; the oracle scenarios `bonus-natural` (`Bonus only`) and `bonus-natural-default` (`Standard 100lt`, `natural_bonuses_use_default_restrictions`) match OLX bit for bit |
| Bot loadouts (spec §20 F1): `initWeaponSelection`'s presets (rifles, 100lt, mortars by mod name, `LoadingTime` and restrictions), `GetRandomWeapons`, `getRandomEnabledWpn`; the bot's own `rand()` stream | `src/common/CWormBot.cpp:4129-4192`, `src/common/CWorm.cpp:788-844`, `src/game/Game.cpp:1405-1420`, `src/common/MathLib.cpp:71-81`, `include/Utils.h:38-43` | `rl-bots::{loadout, BotRng}` | done (plan 6 task 3) | `bot_presets_match_olx`, `random_weapons_follow_restrictions` |
| Bot navigation, motor and aim (spec §20 F2, F3, F7): `getMaxFreeArea`, `searchpath_base` (checklist, passage rule, costs, worm-line goal test, `buildPath`, `simplifyPath`, `splitUpNodes`), `traceWormLine`/`fastTraceLine`/`simpleTraceLine`, the node advance of `AI_MoveToTarget`, `AI_SetAim`, `AI_GetAimingAngle`, `AI_Carve`/`AI_Jump` timers, `isJumpingGivingDisadvantage`; the difficulty numbers | `src/common/CWormBot.cpp:110-878`, `1022-1122`, `1651-1713`, `1896-1960`, `2834-2853`, `3304-3366`, `3578-3627`, `src/level/FastTraceLine.h:24-104`, `include/MathLib.h:140-153`, `include/CVec.h:157-185` | `rl-bots::{nav, motor, aim, Bot, Budget}` | done (plan 6 task 4; the brain is task 5) | `areas_grow_like_olx`, `search_finds_paths_and_resumes_across_ticks`, `motor_reaches_target_angle_like_olx_turning`, `motor_switches_weapons_in_two_ticks_per_slot`, `ballistic_hits_static_target`, `think_stays_within_budget`, `bots_are_deterministic`, `think_does_not_allocate`, `think_never_panics_on_odd_worlds` |
| Bot brain, weapons, rope and chaos (spec §20 F2, F10): `getInput` (THINK ↔ MOVINGTOTARGET, re-thinking every 3 s, every 0.5 s while a search runs and at once on the target's death, the 0.4 s spawn pause, `AI_Carve`), `AI_Think` (critical health, weapon bonus, nearest enemy, health, a random high spot), `AI_CreatePath` (restart after 5 s or when worm lines break, path extension, 0.5 s between searches), `AI_Shoot` (`traceWeaponLine`, clearing weapons through dirt above, lead, feasibility, aim by distance, `weaponCanHit`, bad-aim jumps), `AI_GetBestWeapon` (buckets, change delay; 100lt, rifle and mortar tactics), `AI_ReloadWeapons`, `AI_FindClearingWeapon`, `AI_MoveToTarget` (rope timers, best rope spot, re-throws, pulling away, overshoot, dirt above, dirt ahead, jetpack, rifle reloading, mortar retreat, worms within 50 px, stuck handling), `AI_SimpleMove`, `AI_FindShootingSpot`, `isMoveGivingDisadvantage`; the chaos personality and Deathmatch's chaos rotation | `src/common/CWormBot.cpp:993-1122`, `1180-1644`, `1719-1893`, `1963-2809`, `2869-2983`, `3200-3366`, `3370-4034`, `4120-4126` | `rl-bots::{brain, moving, shoot, weapons, rope, spot, chaos, probe}`, `rl-modes::{RespawnLoadout, ChaosCycle}` | done (plan 6 task 5) | `xtreme_bot_kills_static_target_quickly`, `easy_bot_hits_less`, `bots_use_the_rope_to_climb`, `stuck_bots_stay_within_budget_and_match_ends`, `bots_survive_odd_loadouts`, `chaos_fires_every_weapon_over_a_match_grid`, `chaos_cycle_gives_the_rotation_on_every_spawn`, `bot_matches_are_deterministic_and_allocation_free`, `olx_direct_aim_still_fights`, `a_search_toward_a_moving_target_finishes`, `the_jump_check_is_never_starved`, `a_scan_cut_short_does_not_go_back` |
| Bot aim error floor (the user's direction, plan 6 ruling R6): every difficulty's aim errors (offset, target, gravity and speed noise) scale with at least 1/6, so Xtreme keeps half of Hard's error; the fire tolerance stays OLX's | `src/common/CWormBot.cpp:1682-1706`, `1836-1850` (OLX's Xtreme has none) | `rl-bots::{Difficulty::params, AIM_ERROR_FLOOR}` | done (plan 6 task 9; a deviation, below) | `no_difficulty_aims_perfectly`, `weapon_can_hit_noise_scales_with_the_level`; kill rates measured before and after (below); goldens re-blessed (`fixtures/blessings.log`) |
| Replays (spec §20 F8; no OLX equivalent): the setup, pack ids and content hashes, every worm's keys per tick, checksums at tick 0, every 100 ticks and the end; the `RLRP` format, version refused when unknown | — | `rl-replay::{Recorder, Replay, Player, Cursor}`, `rl-content::{replay_packs, world_for_replay, play_replay}` | done (plan 6 task 6) | `record_then_replay_gives_identical_checksums`, `replay_reports_the_first_mismatching_tick`, `replay_reports_a_different_setup_at_tick_0`, `replay_refuses_unknown_format_versions`, `replay_refuses_mismatched_content`, `bot_match_replays_from_content`, `replay_size_is_reasonable`, `decode_never_panics_on_junk`, `a_failing_tick_is_recorded_without_a_checksum` |
| Headless match runner (spec §7.1, §7.3; no OLX equivalent): `rl-cli match` and `replay`, one match builder shared with `rl-game` (plan 6 ruling R5), invariants every tick, tick-time percentiles, determinism and replay round trip, coverage (weapons, projectile actions, bonuses, rope), failure capture to `target/failures/`, golden checksum streams | — | `rl-cli::{spec, runner, verify, coverage, failure, golden, invariants}`, `rl-content::build_match`, `xtask bless-regression` | done (plan 6 task 7) | `classic_deathmatch_bots_are_deterministic_and_replay`, `mw_chaos_bots_are_deterministic_and_replay`, `failure_writes_replay`, `golden_match_checksums` (`fixtures/matches/`); in `cargo xtask full`: `coverage_report_lists_every_weapon` (both mods × 4 levels × 3 seeds: every weapon of both mods and every projectile action kind) and `long_bot_match_bounded` (30 minutes, review focus 5) |
| Deathmatch against bots in the game: `--mode deathmatch`, the bots thinking on the main thread before each tick, the HUD (names, event log, time-limit clock, Tab's scoreboard), "Game Over" then the results after `GAMEOVER_WAIT`, F5's rematch, `--record` and `--replay` | `src/client/CClient_Draw.cpp:893-905`, `1840-1897`, `2211-2236` | `rl-game::{setup, session, drive, hud, panels}` | done (plan 6 task 8) | `deathmatch_session_ends_with_results`, `rematch_starts_without_the_last_ticks_events`, `bots_think_within_frame_budget`, `replay_session_matches_recorded_checksums`, `scoreboard_hides_when_the_window_loses_focus`; the user's playtest |
| Settings and profiles files (spec §9.4, §21 G2): versioned RON in the platform data directory, atomic writes, bad or newer files set aside as `.bak` with defaults and a warning | OLX's `cfg/options.cfg` (`src/client/Options.cpp`) and `cfg/players.dat` (`src/client/ProfileSystem.cpp`), laid out anew | `rl-ui::{store, settings, profiles, match_prefs}` | done (plan 7 task 1) | `bad_user_files_fall_back_to_defaults_and_keep_a_backup`, `settings_round_trip_and_version` |
| Menus (§21 G1): main menu, local play (mod, level grid with thumbnails, OLX's Deathmatch options and their ranges, bots, the profile), the replays list, the screen state machine; starts that fail return to the menu | `src/client/DeprecatedGUI/Menu_Main.cpp`, `Menu_Local.cpp`, `src/common/FeatureList.cpp:31-35` (laid out anew) | `rl-ui::{state, screens::{main, local_play, replays}, replays}`, `rl-game::shell` | done (plan 7 task 2) | `state_machine_navigates_and_emits_commands`, `main_to_local_play_to_match`, `local_play_builds_match_options`, `local_play_pickers_are_found_by_their_label`, `local_play_replaces_a_saved_selection_no_longer_imported`, `starting_with_missing_content_returns_to_menu`, `menu_actions_are_idempotent`, `replay_list_shows_header_problems`; the v0.1 acceptance runs (`docs/porting/acceptance-v0.1.md`) |
| Profiles and the skin preview: create, edit, delete, select; name, colour, searchable skins; OLX's menu animation from the game's own colorization and frame cut; the human plays the profile's look, bots OLX's default skin in OLX's `cpuColours` | `src/client/DeprecatedGUI/Menu_Player.cpp:746`, `src/client/ProfileSystem.cpp:47-60`, `src/client/CGameSkin.cpp` | `rl-ui::{profiles, skin_preview, screens::profiles}`, `rl-content::skin_sprite`, `rl-game::shell` | done (plan 7 task 3; bot colours task 5) | `profile_edits_persist`, `profile_names_are_validated`, `skin_preview_uses_colorization_like_the_game`, `the_profiles_screen_gets_its_preview_textures`, `a_menu_match_plays_the_selected_profile_look` |
| Options (§21 G3): window presets and fullscreen through the paced path, VSync, filter, interpolation, UI scale; volumes live; key and gamepad rebinding with explicit conflicts, never leaving an action unreachable (the match is set up in local play only) | `src/client/DeprecatedGUI/Menu_Options.cpp`, `src/client/Options.cpp:45-73` (laid out anew) | `rl-ui::screens::{options, keys}`, `rl-input::Capture`, `rl-game::{video, window}` | done (plan 7 task 4) | `rebinding_never_leaves_an_action_unreachable`, `options_rebind_a_key`, `options_set_video_and_audio`, `video_changes_go_through_the_paced_path`, `volume_changes_apply_live`, `the_saved_window_opens_with_the_flags_on_top_unsaved`; e2e (`fixtures/autoplay/menus.ron`: quick window-size changes) |
| Window changes on Wayland (v0.1 playtest; no OLX equivalent): a size or mode change makes the window again at its new shape on the same device, so WSLg leaves no stale frame; X11 changes it in place | — | `rl-game::{window, gpu, app}` (`Plan`, `Shape`, `Gpu::rebind`, `App::recreate`) | done | `a_shape_says_when_the_window_shows_a_change`, `wayland_makes_the_window_again`, `a_window_made_again_keeps_the_game_ticking`; real-screen captures under WSLg (1280 × 720 → 1920 × 1080 → back; the desktop's size → fullscreen → back) and the options' autoplay on Wayland |
| Weapon selection (§21 G4): the profile's slots by name, `findEnabledWeapon`, left/right over the enabled weapons (×6 with Select weapon), Random, Done on Fire's release, skipped with one weapon or none; the choice in `MatchOptions` and the replay | `src/common/CWormHuman.cpp:702-1000`, `src/common/CWorm.cpp:788-844` | `rl-ui::{weapon_select, screens::weapon_select}`, `rl-content::Player::HumanWith`, `rl-game::shell` | done (plan 7 task 5) | `weapon_selection_respects_restrictions`, `a_refused_choice_leaves_the_match_playable` |
| HUD and minimap (§21 G5, Modern only): an opaque bar along the bottom fifth of the window's 16:9 frame (OLX 0.58's status bar; deviation below) with health, five weapon slots with OLX's bar states, the minimap in the centre (OLX's proportions, colours and dots; no view rectangle, the user's ruling: the player's bigger dot is enough) and the event log; clock and kill limit over the playfield, Tab's scoreboard; the game drawn above the bar | `src/client/CClient_Draw.cpp` (HUD, `:137-138` the local game's minimap, `:483-515` the bar (commented out at `d1ad953f`), `drawOnMiniMap`), `docs/porting/olx-notes/presentation.md` §2.4, §9 | `rl-ui::{hud, theme}`, `rl-game::{hud, app, gpu}`, `rl_scene::Viewport::GAME`, `rl_render::Renderer::render_in` | done (plan 7 task 5; the bar, the 16:9 frame and the side layout after the v0.1 playtest) | `hud_layout_holds_at_all_scales` (both layouts), `the_scale_follows_the_height`, `the_game_view_is_fixed`, `the_playfield_is_the_frame_above_the_bar`, `hud_paints_at_every_scale`, `log_keeps_the_last_lines_oldest_first`, `hud_hides_the_weapons_during_the_selection`, `names_stay_inside_the_view`, `hud_snapshot_1080p`, `hud_snapshot_1440p_with_scores`, `hud_snapshot_3440x1440_side_panels` (GPU tier) |
| Pause and results: Esc's pause menu (no tick while paused), the results after `GAMEOVER_WAIT` with OLX's columns plus score, deaths and suicides; Rematch, Save replay, Back to menu | `src/client/CClient_Draw.cpp:1840-1897` | `rl-ui::screens::{ingame, results}`, `rl-game::shell` | done (plan 7 task 5) | `pause_stops_ticks_and_replays_stay_valid`, `pause_menu_holds_the_game_and_opens_the_options`, `results_match_world_scores`, `results_show_the_columns_and_their_buttons`, `save_replay_once_per_match` |
| Level thumbnails (§21 G6, narrowed): `thumb.png` per level pack (160 × 96, box-filtered, letterboxed), pack format 4, left out of the content hash; decoded lazily and bounded in the menus | `src/game/Level.cpp:33-44` (`minimapForLevel`) | `rl-import-gfx`, `rl-pack`, `rl-ui::screens::local_play` | done (plan 7 task 6) | `thumbnails_are_imported_for_every_level`, `thumbnails_are_letterboxed_to_a_fixed_size`, `thumbnails_average_the_art`, `thumbnails_decode_lazily_and_bounded` |
| About (§21 G9): credits (Liero, Liero Xtreme, the OpenLieroX team), licenses, each pack's recorded origin | `src/client/DeprecatedGUI/Menu_Main.cpp:276-283` | `rl-ui::screens::about` | done (plan 7 task 6) | `about_credits_the_authors_and_the_licenses` |
| Import screen: an OLX game directory imported on a background thread into the data directory's `packs/`, layered over the bundled packs; progress, report, cancel; bounded reads and image decodes | `Menu_Local_FillModList`, `checkLXBinMod`, OLX's level list, `src/main.cpp:1003-1021` | `rl-content::import`, `rl-ui::{import, screens::import}`, `rl-pack::image_io`, `rl-game::shell` | done (plan 7 task 7) | `import_runs_off_the_ui_thread_and_reports`, `import_screen_flow`, `import_reports_every_item_and_writes_whole_packs`, `imported_packs_override_bundled`, `import_refreshes_the_menus_and_overrides_bundled`, `quitting_mid_import_leaves_whole_packs`, `a_decompression_bomb_is_refused_on_import_and_load`, `huge_images_are_refused_before_decoding` |
| Fast-forward (the user's request; no OLX equivalent): replays and `--spectate` at 1×–16× or max, a played game always 1×, sound thinned | — | `rl-game::speed` | done (plan 7 task 8) | `fast_forward_never_changes_the_simulation`, `a_played_game_never_runs_fast`, `speeds_step_parse_and_thin_sound`, `the_fake_clock_needs_an_autoplay` |
| Autoplay and end to end (§21 G7): `--autoplay` clicks widgets by label, holds the game's keys, takes checked screenshots and logs JSON lines; `--fake-clock` makes a run deterministic | — | `rl-game::autoplay`, `xtask e2e` | done (plan 7 task 8) | `scripts_parse_and_name_what_exists`, `steps_click_wait_and_fail_in_time`, `widgets_are_found_by_their_label`, `blank_pictures_fail_the_sanity_check`; `cargo xtask e2e` (in `full`, Xvfb) |
| Benchmarks (§21 G8): a full 3000-projectile pool tick, 32 worms, 8 bots thinking, the draw list, an offscreen frame at 1080p and 1440p; machine-local baselines, more than 10 % slower fails | — | `rl-sim/benches/tick.rs`, `rl-cli/benches/game.rs`, `rl-render/benches/frame.rs`, `xtask bench` | done (plan 7 task 8) | `fixtures/bench/baselines.ron`; `regressions_need_more_than_the_tolerance`, `baselines_from_another_machine_only_notice` |
| Engine stress test (the user's request): 32 chaos bots on Castle Strike, both mods, OLX's pool and 20 000; tick, think, pool, entities, events, render and memory; invariants every tick, determinism over two runs | — | `rl-cli stress`, `xtask stress` | done (plan 7 task 8) | `stress_run_is_bounded_and_deterministic`; `fixtures/bench/stress.ron` (in `full`) |
| Performance envelope (plan 8): sweeps of worms, pool, effects, map and art; the GPU on WSL GL and Windows DX12/Vulkan with timestamps and the shader probe; OLX frame timing | `tools/olx-oracle` (`ORACLE_TIMING`) | `xtask/src/envelope/`, `rl-cli gpu-bench`, `rl-game --frame-log` | measured | `fixtures/bench/envelope/<host>.ron`, `docs/perf/envelope.md` |
| v0.1 acceptance (spec §1.3): each of the 4 levels × 2 mods from the menus against 4 Medium bots for 60 s in a real window, timings and screenshots | — | — | done (plan 7 task 9) | `docs/porting/acceptance-v0.1.md`; `cargo xtask check` and `full` |

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
- **Game over** (`GameServer::GameOver` and its gates) is in `rl-sim`; when
  a match ends is the mode's (`GameMode::is_over`, OLX's
  `CGameMode::CheckGameOver`: Deathmatch in `rl-modes`). The scenarios
  play `OracleMode` and reach neither limit, except `dm-kill-limit`, which
  plays Deathmatch and checks the end (the trace's `go`: the frame and
  winner OLX's `GameServer::GameOver` computed) and its effects. Only its
  kill limit is checked by the oracle: the time limit and "fewer than two
  not out" by unit tests (OLX's oracle server is not a local game and
  allows empty games). Three details differ:
  `S2C_GAMEOVER` is parsed right after the frame's other server messages
  (OLX queues it among them, before that frame's bonus messages when
  `CheckGameOver` ends the match), skipped projectiles' clocks move by
  10 ms a frame (`skipProjectiles` adds the real frame time), and
  `GameMode::on_spawn` runs after the spawn (OLX asks `CGameMode::Spawn`
  first; Deathmatch's always agrees).
- **Mode commands have no OLX equivalent** (spec §12.1 H1): they are applied
  after `CGameMode::Simulate`, in queue order. `give_weapon` makes the slot
  full and ready, `LastFire` included (a weapon bonus keeps `LastFire`).
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
- **Weapon restrictions:** a `WeaponRestrictionsFile` that starts with `cfg/`
  (a path OLX opens as is) finds no imported preset, so every weapon is
  enabled, as OLX does when the file is missing. Mod packs carry only the
  presets OLX finds for the mod (`cfg/presets/<mod dir>/*.wps`, then
  `cfg/presets/*.wps`).
- **Bots draw from their own `rand()` stream** (`rl_bots::BotRng`, seeded from
  the match seed, the worm and the personality), not from the game's `rand()`
  as OLX's do, so loadouts and later bot decisions never shift the world's
  generators (spec §20 F7).
- **Bots think once per 100 Hz tick, not per 11 ms physics step,** on
  `World::time()`, and spend a budget of work units (terrain pixels read),
  not wall-clock time; pathfinding runs as a resumable state machine instead
  of a thread (spec §20 F7). Areas are kept in a per-match `AreaCache`
  shared by every search and bot (OLX grows them anew for each search), the
  open set breaks ties by insertion order (OLX's `std::set` drops tied
  areas), `simplifyPath`'s dead second loop is left out, a search visits at
  most 1024 areas, a path has at most 256 nodes, and the node advance looks
  8 nodes ahead and back (OLX: all of them).
- **Bots press keys** (spec §20 F3; `rl_bots::Motor`): the aim turns through
  UP/DOWN at about 100°/s with 0.2 s to speed up and to brake (60° takes
  0.8 s; OLX's bot sets `fAngle` at 91°/s, 0.66 s, and snaps within 3°);
  turning around takes one step of walking; carving needs walking (OLX
  carves standing too); a jump is never pressed while the rope is out (it
  would release it); a weapon change takes two ticks a slot and stops
  walking. The aim offset of a difficulty is drawn per bot from its own
  generator (OLX shares one per difficulty between all bots), and the aim is
  good only when the worm also faces the target's side.
- **The bots' brain** (`rl-bots`, spec §5.9: parity not required):
  `findTarget` takes the nearest enemy without its line traces (OLX prefers
  one in sight only within 50 squared px); the 100lt Cannon tests
  `isAttached` where OLX tests `isReleased`; the rope spot's angle term takes
  the hit's direction from the worm (OLX normalises its absolute position);
  `AI_FindShootingSpot`'s left fallback steps back right, towards the
  target, whenever a worm line is blocked (OLX's steps a further 5 px left,
  `CWormBot.cpp:4026`, so its spots by a wall on the target's left lie 10 px
  farther off, and at the map's left edge outside the map); at the end of
  its path, with the target worm behind dirt (the first pixel that blocks
  the weapon line is dirt, not rock), the bot heads for the worm itself and
  digs on (OLX's stands at the shooting spot, which on a dirt level is as
  deep in the dirt as the worm); the shooting spot is searched after the
  node advance and its jump check, with half of what is left (OLX finds it
  first);
  the old path is dropped on respawn; the client-only lag guard and the
  unused `iRandomSpread` draws are left out; there are no teams. With the
  ballistic aim (F2) `weaponCanHit`'s gravity and speed noise bend the aim
  (dead in OLX), the solver uses the projectile's own gravity, its mean
  speed (`SpeedVar` is symmetric; OLX's decisions add `SpeedVar·100`, kept)
  and the velocity it carries (`AddParentVel` with `ProjRelativeVel`), and
  the trajectory check flies the real arc (centre pixel, 2 px steps); OLX's
  parabola, kept for `AimModel::OlxDirect`, also reads only the centre pixel
  (not `CheckCollision`'s box). A probe that does not fit the budget waits
  a tick: the rope spot's 20 rays and the shooting spot's circle are spread
  over ticks and kept 50 ms and 100 ms; shooting gets at most a quarter of a
  think, moving the rest of half, and the search (OLX's thread) the rest.
- **The chaos personality** (spec §20 F10) is no OLX bot: Deathmatch gives
  chaos bot `i` of `n` the weapons `(i·5 + r·5·n + j) mod W` on its `r`-th
  life (`rl_modes::RespawnLoadout::ChaosCycle`, by `give_weapon`), and the
  bot fires whatever is ready (at the nearest enemy in sight, else at a
  random angle), prefers a slot it has not fired, and walks, jumps, ropes
  and digs at random.
- **The bonus spawn timer starts at the world's start** (`T0 + K`), as the
  oracle resets it when a recording starts; OLX sets it in `PrepareGame`.
- `World::place_bonus` refuses a weapon id outside the mod (the oracle
  reports an error; OLX's `CBonus::Spawn` would read the weapon name out of
  bounds) and more than 32 placements a tick, and places nothing when no
  server slot is free.
- A grabbed weapon bonus does not call `WormShootEnd` for the replaced weapon
  if the worm holds fire: it acts only on a `FinalProj`, which LX56 mods do not have.
- **`Message` events are the mode's** (`ModeCtx::message`, a text id and
  three numbers): OLX's kill and spree texts come with Deathmatch
  (`rl-modes`); `rl-sim` sends none itself (the `GameOver` event carries
  the winner).
- **Deathmatch's texts and sounds** (`rl-modes`): the kill text's weapon is
  always unknown (-1; OLX then drops " with <weapon>"), as `rl-sim` does not
  keep the weapon an injury came from. The end's texts ("Kill limit … hit by
  worm …", "Timelimit has been reached", "… has won the match") are queued
  in the `on_tick` of the frame that ended, so in a tick's events they come
  after `GameOver`, and a kill's texts are applied with the frame's other
  commands, after the physics. OLX's sounds of `CGameMode` (sprees,
  "N frags left", lead changes, time remaining, "terminated",
  "timeoutcalled") are not sent: modes have no sound command.
- **Deathmatch's results order** keeps ties in id order (a stable sort);
  OLX's `std::sort` leaves it unspecified, and its comparator is not a
  strict order (with limited lives, two worms at the kill limit each rank
  before the other).
- **A Deathmatch needs two worms** (spec §20 F5, `Deathmatch::prepare`);
  the check itself keeps OLX's single-worm local game (fewer than one not
  out). A local game never allows empty games, so the "fewer than two not
  out" check always runs.
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
- **Window** (`rl-game`, plan 6 hotfix, the user's ruling): the window
  cannot be drag-resized or maximised; its size comes from the defaults
  (later the options menu, M7) and `--fullscreen` (borderless). The game's
  own size changes are applied at most every 250 ms, because WSLg's Weston
  crashes on quick resizes of a decorated Wayland window. On Wayland each
  change closes the window and opens a new one (v0.1 playtest: WSLg left
  pieces of a changed window's frame on the screen); the compositor places
  it, so the window does not keep its position.
- **Deathmatch in the game** (`rl-game`, plan 6 task 8; full menus are M7):
  a command-line Deathmatch (`--mode deathmatch`) gives the human random
  weapons the restrictions enable (OLX's weapon dialog's "Random"); menu
  matches have OLX's weapon selection (plan 7 task 5, below). The HUD is egui,
  not OLX's: the time-limit clock is frozen at the end in the HUD
  (`Deathmatch::time_left_ms` keeps counting, harmlessly), kill-feed lines
  name no weapon (`rl-sim` does not record the injuring weapon; OLX drops
  `" with <weapon>"` when it is unknown); "Game Over", then after
  `GAMEOVER_WAIT` (3 s) the results, as OLX. F5's rematch plays the next seed.
- **Menus** (`rl-ui`, `rl-game`, plan 7 task 2): the main menu and local
  play are laid out anew in egui, not OLX's `DeprecatedGUI` screens. Local
  play sets OLX's local game options (mod, level, `Lives`, `KillLimit`,
  `TimeLimit`, `LoadingTime`, `Bonuses`, `WeaponRestrictionsFile`; the
  sliders' ranges from `FeatureList.cpp:31-35`) and the bots as a count with
  one difficulty or each its own, where OLX adds bot profiles to a player
  list one by one. Esc pauses a match (a pause menu) instead of OLX's
  in-game menu; every Deathmatch played from the menus is saved as a
  replay (`replays/`, the last 20), which OLX does not do.
- **Profiles** (`rl-ui`, `rl-game`, plan 7 task 3): OLX's player menu
  (`Menu_Player.cpp`) laid out anew: one screen creates, edits, deletes
  (confirmed; never the last) and selects human profiles (bot profiles are
  local play's bot choices). Names are trimmed, not empty and at most 20
  characters as OLX's; unlike OLX they keep Unicode (`RemoveSpecialChars`
  drops non-ASCII), refuse control characters and refuse another profile's
  name (ignoring case), so a name picks one profile. The skin preview
  animates OLX's menu formula (`Menu_Player.cpp:746`, 200° a second) and
  shows the worm faced left too (OLX's menu faces it right only), drawn
  from the game's colorized sheet and frame cut (`rl_content::skin_sprite`,
  the left-facing column quirk included). In a match the human (worm 0)
  wears the profile's skin and colour and every other worm OLX's default
  skin (OLX's bot profiles, `ProfileSystem.cpp:59`); until now every worm
  wore the player's skin.
- **Options** (`rl-ui`, `rl-input`, `rl-game`, plan 7 task 4): OLX's
  options menu (`Menu_Options.cpp`) laid out anew in four sections. Video
  offers one display list (windows of 1280 × 720, 1600 × 900 and
  1920 × 1080, or borderless fullscreen; a desktop-sized window was dropped
  after the v0.1 playtest: its bottom was under the taskbar), VSync or uncapped, the upscale filter,
  interpolation and the UI scale, where OLX sets a fixed 640 × 480 mode and
  its scaling; size changes are paced (at most one every 250 ms). Audio has
  OLX's sound volume (default 70) and an effects volume, multiplied. Keys:
  OLX's controls tab has one keyboard and one gamepad slot per control and
  lets one key do several controls silently; here an action holds any
  number of keys and gamepad inputs, a key another action has is moved or
  shared only on the player's choice, an action never loses its last key
  or last gamepad input (a hand-edited file that leaves one without gets
  OLX's default back, with a warning), and the game's own keys (Esc, F2,
  F5, Tab, H; PageUp and PageDown since task 8) cannot be bound. Capture is OLX's "Input for:" box (Esc
  cancels). The match is set up in local play only, before each game (it
  remembers the last); the options have no Gameplay tab (removed after the
  v0.1 playtest: it edited the same settings).
- **In game** (`rl-ui`, `rl-game`, plan 7 task 5). *Weapon selection:*
  OLX's dialog (`CWormHuman.cpp:702-1000`: the profile's slots by name, an
  unenabled one replaced by `findEnabledWeapon`, left/right over the enabled
  weapons, six at a time with Select weapon, Random, Done on Fire's
  release, skipped with one enabled weapon or none) shown over the level
  before the match, driven by the game's keys or the mouse; the world waits
  untouched until Done, then is built with the choice (`Player::HumanWith`
  in `MatchOptions`, so the replay's setup holds it). OLX's stick
  auto-repeat (0.4 s, then 0.12 s) applies to held keys too. A rematch from
  the results chooses again (OLX's selection at each game); F5's in-place
  rematch keeps the weapons. *HUD bar* (the user's ruling after the v0.1
  playtest, replacing the overlay HUD of plan 7 task 5): an opaque status
  bar along the bottom of the window, as OLX 0.58's local game drew one
  under its viewports (`CClient_Draw.cpp:483-515`, its background
  `DrawRectFill(0, 382, 640, 480)`, kept there commented out;
  `:137-138` the minimap at 255, 382 of 640 × 480: centred, in the bar).
  The commit we port, `d1ad953f`, instead puts a single player's minimap
  top right over a full-height viewport (`CClient_Draw.cpp:610-650`): a
  deliberate deviation. Left the health and five weapon rows, centre the
  minimap, right the event log (OLX's kill messages, the last 64 kept,
  the newest that fit shown; OLX's local chat showed 6 for 3 s over the
  playfield; built as a log of typed lines so a multiplayer chat can join
  it), in OLX's colours (health `(64,255,64)`, weapon bar states,
  lives/kills, weapon label `#02bff6`). *The 16:9 frame and the fixed
  view* (the user's second ruling after the v0.1 playtest, for a
  competitive multiplayer game): the game is designed for 16:9, the
  largest 16:9 frame centred in the window (a 16:9 window has no black
  strips; a narrower one black above and below, a wider one at the
  sides). Its bottom fifth is the bar, as OLX 0.58 (98 of 480 pixels;
  its minimap 128 × 96, a fifth of the height); the minimap fills the
  bar's height less a little (OLX's 119 × 94 proportions). The playfield
  above it (16 : 7.2) shows `rl_scene::Viewport::GAME`, 427 × 192 world
  pixels, for every window and layout (spec §19 E4 amended: 16:9's width
  at 240, 80 % of the height, as OLX 0.58 showed 320 × 191 above its
  bar, `CClient.cpp:1712`; the fork's full-height 240 is gone). The UI's
  scale follows the height: pixels per point are the frame's height (in
  game; the window's in the menus) over 720 times the UI scale, so the
  HUD and menus fill the same share of the screen on every display; the
  HUD scales with the frame alone. *Side layout* (Options > Video > HUD
  layout: Auto, Bottom bar, Side panels; `--hud-layout`): on a window
  with a quarter of the frame's height free on each side (21:9 and
  wider), the health and weapons go, larger, in a column left of the
  frame and the event log, taller and wrapped, in one right of it; the
  bar keeps the minimap. The frame and the view do not change, so no
  layout shows more. The clock and kill limit
  stay top centre over the playfield, the keys top right; the weapon
  selection, the pause menu and the results are centred over the
  playfield. Nothing fades (the overlay HUD faded where a worm was under
  it; the bar covers no playfield). No weapon
  rows show during the weapon selection; worm names stay inside the
  picture and are not drawn through the selection, the pause menu or the
  results.
  *Minimap:* OLX's 119 × 94 proportions and half opacity, box-filtered
  from the colour terrain at twice OLX's resolution, refreshed every 10
  ticks (OLX updates edited areas at once), as tall as the bar allows; worm dots as `drawOnMiniMap` (2 × 2, the human 3 × 3, OLX's
  blink), plus the camera's view as a rectangle, which OLX does not draw.
  *Pause:* Resume, Options (the match stays paused) and Quit to menu; no
  tick runs while paused or during the weapon selection, so replays hold
  only played ticks. *Results:* OLX's columns plus score, deaths and
  suicides; Rematch, Save replay (`replays/replay-NNN.rlrp`, never pruned)
  and Back to menu. *Bot colours:* OLX's seven `cpuColours`
  (`ProfileSystem.cpp:47-59`) in turn, skipping those within 120 (RGB
  distance) of the human's colour; OLX gives each bot profile its colour
  regardless. Cosmetic: replays draw every worm in the default colours.
  *Keys:* Esc and F5 ignore key repeat; a settings or profiles file that
  binds a game key (Esc, F5, F2, Tab, H, PageUp, PageDown) loses it with a warning; while the
  options capture a key, egui hears no key at all.
- **Level thumbnails** (`rl-import-gfx`, `rl-pack`, plan 7 task 6): OLX's
  level list previews a level by its minimap, the drawn image resampled and
  stretched into 128 × 96 (`minimapForLevel`, `src/game/Level.cpp:33-44`;
  `gusUpdateMinimap`, `src/gusanos/level.cpp:486-527`), made each time the
  list selects it. The remake makes it once at import as the level pack's
  `thumb.png`: the front art box-filtered into 160 × 96 and letterboxed
  (transparent bars), so wide and tall levels keep their shape. It is
  presentation only and left out of the pack's content hash (ruling Q4).
- **No Classic HUD** (plan 7 task 6, the user's call; a deviation from
  plan 7 G5 and G6): the in-game HUD is egui only. OLX's bitmap fonts
  (`CFont`, `data/gfx/font.png`, `out_font.png`) and HUD images
  (`healthbar1.png`, `weaponbar1.png`, `clock.png`, `infinite.png`) are not
  imported or drawn: making the low-resolution art look right at every DPI,
  UI scale and window size, and keeping a second HUD in step, cost more
  than it gave. OLX's colours and information stay in the Modern HUD.
- **About** (`rl-ui`, plan 7 task 6): OLX shows its credits in the corner
  of the main menu (`Menu_Main.cpp:276-283`); the remake has an About
  screen from the main menu with the credits, the licenses and each
  bundled pack's origin.
- **Import screen** (`rl-content::import`, `rl-ui`, `rl-game`, plan 7
  task 7): OLX reads its game directory as it is; the remake imports one
  (a typed path: the `share/gamedir`, or an OpenLieroX root holding one)
  into packs in the user data's `packs/`, on a background thread, and
  layers them over the bundled packs (spec §9.4): a pack id in `packs/`
  replaces the bundled pack of that id (an OLX item always imports to the
  same id), and an imported pack this build cannot read hides nothing.
  What is imported is found where OLX's menus look: the directories with a
  `script.lgs` (`Menu_Local_FillModList`, `checkLXBinMod`), `levels/*.lxl`
  (OLX's map list), `data/themes/*` and `skins/*.png`. Not imported: Liero
  `.lev` and directory (Gusanos) levels, Gusanos mods, skins other than
  PNG (OLX also lists `.bmp`, `.tga` and `.pcx`, `src/main.cpp:1003-1021`),
  and the core pack unless all 18 of its files are there (a partial one
  would replace the bundled sprites with placeholders). A mod whose
  `script.lgs` is broken is listed as a failed item; OLX's list hides it.
  Replays recorded on a bundled pack stay valid when the same OLX item is
  imported again (the content hash leaves `pack.ron` and `thumb.png`
  out); a different item under the same id makes them refuse with the
  content-hash error. No game starts while an import runs.
- **Fast-forward and spectating** (`rl-game`, plan 7 task 8, the user's
  request; OLX has neither): a game nobody plays, a replay or a
  Deathmatch of bots alone (`--spectate`, 2 to 32 bots), runs at 1×, 2×,
  4×, 8×, 16× or max (`--speed`, PageUp and PageDown; "Fast-forward N×"
  in the HUD): a frame runs that many times the ticks owed and draws the
  last; max runs ticks for 12 ms a frame. A game a person plays always runs
  at 1× (`--speed` refused, the keys ignored). The simulation is the same
  at any speed (`tests/speed.rs`: the same world and replay bytes at 1×,
  4×, 16× and max). Sound is thinned: every tick's up to 2×, only each
  frame's last tick's from 4× to 16×, none at max. The menus have no
  spectating yet (the command line's `--spectate`), and replays have no
  seeking (deferred).
- **Engine stress test** (`rl-cli stress`, plan 7 task 8, the user's
  request): its matches go past OLX's limits on purpose: 32 chaos bots,
  `LoadingTime` 0 (in OLX's range, below any menu default) and, in one
  configuration, a projectile pool of 20 000 (`MatchOptions::pool_capacity`;
  OLX's `MAX_PROJECTILES` is 3000). Every other match keeps OLX's pool and
  loading time; the oracle, match and render goldens are unchanged.
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

## Spec §20 decisions that deviate from OLX (plan 6, M6)

Each decision is in spec §20; the bullets above give the details.

- **F1, bot loadouts:** OLX's presets by default, as OLX; a `Random` policy
  (`GetRandomWeapons`) is ours. Bots draw from their own `rand()` stream.
- **F2, ballistic aim:** bots aim with a gravity-aware solver (the low root,
  the projectile's own gravity and carried velocity); OLX computes the
  ballistic angle and then aims straight at the target. `weaponCanHit`'s
  gravity and speed noise bend this aim (dead code in OLX).
  `AimModel::OlxDirect` keeps OLX's aim for comparison.
- **F3, bots press keys:** the 8 keys through a motor (aim turned at about
  100°/s, two ticks a weapon slot); OLX's bots set the worm's angle and
  weapon directly.
- **F4, weapon restrictions:** `.wps` presets are imported into mod packs
  (closing the plan-4 deviation); a `cfg/` path finds no preset.
- **F5:** a Deathmatch needs two worms; the single-worm sandbox stays.
- **F6, modes and commands:** OLX's `CGameMode` callbacks run where OLX runs
  them; mode commands (spawn, give weapon, terrain stamp, message, end)
  have no OLX equivalent and are applied after `CGameMode::Simulate`.
- **F7, bots per tick on a work budget:** once per 100 Hz tick, not per
  physics step; terrain pixels read, not wall-clock time; a resumable
  search, not a thread; one shared area cache per match.
- **F8, replays:** no OLX equivalent (keys, setup, content hashes,
  checksums).
- **F9, matches start at 100 000 ms** (`MatchSetup::T0`), as the oracle
  does; OLX starts at the wall clock.
- **F10, the chaos personality:** no OLX bot; its loadout rotates on every
  spawn through Deathmatch's `give_weapon`.
- **Bot aim error floor** (the user's direction, plan 6 ruling R6): OLX's
  Xtreme bots aim with no offset and no `weaponCanHit` noise. Here every
  difficulty's aim errors scale with at least `rl_bots::AIM_ERROR_FLOOR` =
  1/6 of Easy's (Hard 1/3, Medium 2/3), so Xtreme keeps half of Hard's
  error: an aim offset up to ±8.3°, target noise up to ±8.3 px per axis,
  gravity ±0.83 and speed ±3.3. Its 3° fire tolerance is OLX's. Measured
  with 4 bots, 20 seeds of 3 minutes each (all bots' kills per minute, and
  damage dealt to other worms per shot):

  | config | Xtreme, OLX (no error) | Xtreme, floor | Hard | Medium |
  |---|---|---|---|---|
  | Classic, Dirt Level | 5.37 (5.46/shot) | 4.90 (4.94) | 3.98 (2.70) | 3.97 (2.45) |
  | Classic, Liero Factory | 3.70 (4.09) | 3.88 (4.25) | 3.55 (2.76) | 3.12 (2.19) |
  | MW 1.0, Dirt Level | 6.38 (3.16) | 6.02 (2.81) | 4.37 (1.23) | 6.12 (1.45) |
  | MW 1.0, Liero Factory | 4.72 (6.15) | 4.48 (5.62) | 4.13 (3.23) | 3.45 (2.12) |

  Xtreme stays the strongest level (within noise of Medium on MW Dirt
  Level, where our Medium already scores above OLX's: see
  `bot-baseline-2026-10-11.md`) and deals 1.5-2.3× as much damage per shot as Hard.
  Against a target standing still (`xtreme_bot_kills_static_target_quickly`,
  seeds 1-32) the median kill takes 35 s, against 29 s with no error, 52 s
  for Medium and 71 s for Hard.

**Future: bot skill.** The user's direction for bot difficulty: harder bots
come from strategy, quick reactions and predictive reloading (teamwork once
team modes exist), never from perfect aim. Long-range fast weapons in a
perfect aim's hands feel like instakills. The aim-error floor is the first
step. The redesign (difficulty scaling by reaction time, target choice,
reloading ahead of a fight, positioning) is a later milestone, measured
against OLX with the baseline harness (`bot-baseline-2026-10-11.md`).

## Spec §21 decisions (plan 7, M7, v0.1)

Each decision is in spec §21; the bullets under "Deliberate deviations"
give the details. OLX's menus are a 640 × 480 `DeprecatedGUI`; the remake
keeps their options and ranges and lays the screens out anew.

- **G1, `rl-ui`:** menu logic (`UiState::update`) apart from the egui
  widgets, so flows are tested headless by label. A Replays screen was
  added to G1's list.
- **G2, files:** `settings.ron`, `profiles.ron`, `replays/` and `packs/`
  in the platform data directory, versioned; a bad file is kept as `.bak`
  and defaults are used. OLX's `options.cfg` and `players.dat` are not
  read.
- **G3, window:** presets and fullscreen only, paced at 250 ms (the user's
  ruling after WSLg's compositor crashes); no drag-resize or maximise.
- **G4, weapon selection:** OLX's dialog before each menu match, with its
  keys and mouse; a command-line Deathmatch still gets random weapons.
- **G5, HUD:** one Modern (egui) style in OLX's colours. The Classic
  bitmap style was dropped (the user's call, plan 7 task 6).
- **G6, assets:** level thumbnails are imported (pack format 4); OLX's
  fonts and HUD images are not (G5).
- **G7, autoplay:** `--autoplay` and `--fake-clock`, run on Xvfb by `xtask
  e2e` in `full`.
- **G8, benchmarks:** divan medians against machine-local baselines in
  `fixtures/bench/` (a baseline from another host only gives a notice),
  plus the engine stress test (the user's request).
- **G9, About:** credits and licenses as spec §11 asks.
- **Additions by the user's request:** fast-forward for games nobody
  plays (replays, `--spectate`), never for a played game; the engine
  stress test past OLX's limits.
- **Deferred:** replay seeking; spectating from the menus; recording the
  human's skin and colour in replays (they are drawn with the default
  look); the bot-skill redesign and a tournament command.

## Performance envelope decisions (plan 8)

Plan 8 measured and changed nothing in the simulation: no deliberate
deviation. Its rulings (`docs/superpowers/plans/2026-10-11-plan-8-performance-envelope.md`):

- **P1:** the reference configuration is `rl_cli::stress::spec` with 16
  worms: chaos bots of mixed difficulty, `LoadingTime` 0, bonuses every
  5 s, no kill or time limit, seed 1.
- **P2:** the pool axis ends at 65 536 (`MatchSetup::MAX_POOL_CAPACITY`),
  not 100 000; raising it is a simulation change.
- **P3:** generated levels bypass the content loader's caps; a point above
  `MAX_COLOR_MAP_BYTES` still runs and the cap is its breaking cause.
- **P4:** sweeps are headless; the frame budget is judged in the GPU
  scenes (the heavy replay at four sizes, each map and art level at
  2560 × 1440 on WSL); breaking points use 144 Hz, 60 Hz beside it.
- **P5:** windowed runs (`rl-game --frame-log`) measure wall frame times
  only, one run each; GPU timestamps are taken offscreen.
- **P6:** the colour-terrain edits are timed inside the presentation tick.
- **P7:** an effects or pool cap reached at its own value is saturated, not
  a break; a content cap, the bots' area cache full, a terrain cropped to
  the texture limit or a failed run breaks a point.
- **P8:** OLX's timed span is its whole simulated frame
  (`OracleHooks::beforeFrame` to `afterFrame`), on six busy oracle
  scenarios and two heavy ones in `fixtures/perf/olx/`; the same work is
  checked by per-frame projectile counts and `rand()` calls.
- **P9:** envelope runs skip the determinism replay (`--no-verify`);
  `--quick` is one run per point.
- **P10:** results live in `fixtures/bench/envelope/<CPU slug>-<n>cpus.ron`,
  the host compared in full as for `baselines.ron`.
- **P11:** GPU scenes draw `Viewport::GAME` over the whole target with the
  default filter, no HUD.
- **P12:** the Windows binaries and a content copy are staged in
  `%TEMP%\rust-liero-envelope` and run there; `WGPU_BACKEND` goes through
  `WSLENV`.
- **P13:** sweep runs pass `--memory-ceiling-mib 16384`, so points past
  2 GiB are measured.
- **P14:** OLX's rendering is not timed: the oracle's OLX is a dedicated
  build with no renderer; the report says so.

## Networking decisions (plan 9)

Plan 9 adds the rollback netcode core and changes no simulation behaviour. Its
rulings (`docs/superpowers/plans/2026-10-12-plan-9-netcode-core.md`):

- **N1:** client prediction limit 28 ticks (amended during Task 7 from 20,
  measured). The spec's 12-tick window is the host's fill deadline (§3.2). A
  client may predict up to `MAX_PREDICTION = 28` ticks past its confirmed
  tick before it stalls: a client's frontier waits for the slowest client's
  inputs to reach the host (up to the 12-tick fill window under loss bursts)
  on top of its own two one-way legs minus its input delay. Measured in the
  `bad` profile (10 seeds, 4 clients), stall events by limit: 20 → 310, 22 →
  68, 24 → 14, 26 → 3, 28 → 0. A stall freezes the player's game, which is
  worse than a deep correction, so the limit is 28 (snapshot ring of 29).
  The §3.3 budget is measured at 12 re-simulated ticks (the usual depth) and
  reported at 28.
- **N2:** input delay fixed per match: `d` is chosen once, at match start,
  from the round-trip time (or pinned by the player), not changed mid-match:
  changing it mid-match leaves a tick with two inputs or none.
- **N3:** profiles are pings: A profile's latency is the round-trip time;
  each direction takes half, with half the jitter.
- **N4:** the sync checksum: Rollback compares `World::sync_checksum`,
  which equals `World::checksum` except that the terrain enters as an
  incrementally maintained, order-independent cell hash (O(1) to read)
  instead of FNV over every cell (570 µs at Castle Strike, about 13 ms at
  4096²). `World::checksum` (replays, goldens, oracle) is unchanged.
- **N5:** world-state transfer by serde: The world's dynamic state crosses
  the network as `rl_sim::WorldState` (serde-derived, postcard, deflated),
  decoded into a world built from the same match setup, and validated before
  use. The game mode transfers its own state through two new `GameMode`
  methods.
- **N6:** disconnected slots idle: In plan 9, when a peer is dropped the
  host fills its slot with `Keys(0)` until the match ends; what happens to
  the worm (removal, a bot taking over) is decided with the lobby in plan
  10.
- **N7:** time sync by arrival margin: The host measures, for each client,
  how many ticks before it needs them the client's inputs arrive, and
  reports the median of the last 16 samples; the client runs up to 1 %
  faster or slower to hold it at 1 tick. A client more than 8 ticks off
  jumps (catches up up to 10 ticks per call, or holds).
- **N8:** on links with a round-trip time of 100 ms or less each input
  change is repeated in 4 datagrams and resent from the ack when the peer's
  echo shows a gap, instead of every datagram repeating every unacked input
  (spec §3.1); longer links keep full redundancy. Measured: a 32-worm
  typical client 28.7 → 16.7 KB/s, feel numbers unchanged (ledger ruling
  T11-A).
- **N9:** the 24 KB/s budget is held at the typical profile; on bad and
  awful links a 32-worm client receives about 45 KB/s or more, because the
  full redundancy keeps it stall-free (ruling T11-B).
- **N10:** time sync has an integral term; after a jump, margins are ignored
  until the catch-up ends and 16 fresh samples arrive; a client behind its
  own pace is never sped up; a frozen peer's clock stops (ruling T7-B).
- **N11:** desync repair re-arms 300 ticks after a state was sent if reports
  still differ; after 3 repairs per peer per match the host raises
  `RepairFailed` (the driver drops the peer); a spectator cannot take a
  player's seat (`JoinRefused`) (ruling T8-A and the Task 8 fix).
- **N12:** a received world state is validated before use, with bounds
  derived from the receiving world's settings (e.g. the projectile clock lag
  from the physics rate and game speed), and refused all-or-nothing (Task
  3).
- **N13:** a received world state with any non-finite number (positions,
  velocities, health, aim, rope, slots, projectiles, bonuses, shots) is
  refused; positions are not bounded by the map, since worms can leave it
  at a corner. No shipped mod produces non-finite values; a mod that does
  gets its states refused like a stranger's (no desync repair, no late
  spectators; the match itself plays on) (final review, ruling F-A).
- **N14:** a rollback or a loaded state reports the terrain areas it put
  back (`Session::take_restored_areas`, at most 256 rectangles, merged
  beyond); the game rebuilds those areas of its colour image, which loses
  blood stains and the level's dirt art inside them (final review, ruling
  F-B; an exact colour undo is left to plan 10).

## Online play decisions (plan 10)

Decided in `docs/superpowers/plans/2026-10-12-plan-10-online-play.md` ("Decisions this plan makes") and by rulings during its
execution.

- **N15:** the native WebRTC stack is `str0m` (sans-IO, clock-injected,
  `rust-crypto`); it still compiles `aws-lc-sys` (C) through dimpl's rcgen,
  needs no system C library and cross-builds with mingw (ruling T1-C).
- **N16:** a peer's ICE candidates are its UDP socket's port on the IP its
  signalling TCP connection uses, in both directions; no STUN until plan 12.
  LAN and direct play, a forwarded port included, need nothing more. A
  forward must map TCP and UDP on the same external port (ruling F-G: the
  client takes the host's candidates as the address it dialled).
- **N17 (as amended by T1-A):** both data channels are pre-negotiated:
  `inputs` id 0, unordered with a 1 ms packet lifetime (effectively
  unreliable: `MaxRetransmits(0)` triggers a FORWARD-TSN/SACK storm in
  sctp-proto 0.10.5) and `control` id 1, reliable and ordered. Browsers use
  the same ids (plan 13).
- **N18:** a `rl_netcode::Session` lives for one match; the replay is made
  after the match on a worker thread by re-simulating the confirmed inputs.
- **N19:** each player picks a loadout in the lobby or keeps "Random"; the
  host's match setup carries every slot's choice, so every peer builds the
  identical world.
- **N20:** after each `advance` the game rebuilds the colour image's
  restored areas from the final terrain, then applies the advance's terrain
  edits in tick order (inexact only inside a corrected area's art, F-B).
- **N21:** while an online lobby or match exists the game keeps polling the
  network and ticking when its window is hidden or minimised (no rendering);
  the hidden window's event loop uses `ControlFlow::WaitUntil` every 4 ms.
  Hidden is detected from the platform's `Occluded` event or a zero window
  size. Wayland (WSLg, Linux desktops) may report neither for a minimised
  window, so the game would keep rendering there (and a compositor that
  throttles a hidden window's frames could stall a host until its clients
  drop it after 10 s): to be verified in playtest (minimise during
  `--net-sim` on WSLg and native Wayland); Windows and X11 report it.
- **N22 (T4-A):** a signalling offer's candidates whose IP differs from the
  TCP peer's are filtered out, not refused (forged candidates cannot aim the
  host at a third party); a NAT'd client joining a forwarded host connects
  through a peer-reflexive candidate. Ruling F-G: the client rewrites every
  candidate of the host's answer to the address it dialled (IP and port),
  so a host behind NAT (port forward, Docker, cloud VM), whose answer names
  its private address, is reachable; TCP and UDP must be forwarded on the
  same external port.
- **N23 (T6-A):** a forced start (spec §7.2) is `LobbyHost::force_start`:
  players not ready, or without a complete setup, spectate that match. The
  Lobby has a "Force start" button for the host.
- **N24 (T6-B):** `MAX_LOBBY` is 60 KiB, with a worst-case encoding test;
  `rl-net`'s reliable cap is `max(MAX_CONTROL, MAX_LOBBY)` plus the tag.
- **N25 (T13-A):** `rl-server` depends on `ctrlc` 3.5.2 (termination
  feature) for a clean shutdown on Ctrl+C or SIGTERM (everyone gets `Bye`).
- **N26 (T13-B):** `/ban` bans the key only, `/banip` the key and the
  address, `/unban` lifts either; the game's Ban button bans the key only
  (spec §5.2). Bans are soft: a banned player returns with a new key.
- **N27 (T13-C):** players' ready marks survive setup changes that only
  touch bots.
- **N28 (T14-A):** `MatchEndWire` carries the host's last tick, so a match
  ends at the host's tick on every peer and the replays are equal (after
  `/next` the clients had run 200 ticks past the server).
- **Note:** replays and bot-only games no longer count worm 0's kills as the
  player's; a worm a mode adds mid-match gets the default look.
- **Note:** when the host's clock is frozen its clients need about ten
  jumps (about 2.5 s of stall episodes, bounded by `MAX_JUMP_BACK`) to
  recover; a running host clock catches up through `MAX_CATCHUP`.
- **Note (disk chore):** dependencies build without line tables in the dev
  profile, tests are one binary per crate, and `cargo xtask check` sweeps
  superseded builds automatically.

## Content download decisions (plan 11)

Decided in `docs/superpowers/plans/2026-10-13-plan-11-content-download.md`
("Decisions this plan makes") and by rulings during its execution
(PF11-1 to PF11-6, R4-*, R6-*, Task 8's).

- **N29 - pack archives.** A pack travels as a `.rlpk` archive: magic
  `RLPK1\n`, a `u32` file count, then per file, sorted by path, a `u16`
  path length, the UTF-8 `VPath`, a `u64` length and the bytes. No
  compression (packs are mostly PNG and OGG). The same pack always gives
  the same bytes, so a resumed download continues byte-exactly. The
  archive's SHA-256 (the *archive hash*) identifies a partial download;
  the *content hash* (`rl_pack::content_hash`, unchanged) identifies the
  pack. Hidden files (a path component starting with `.`) are not part of
  a pack: hashing and archiving skip them, decoding refuses them (PF11-2:
  a stray `.DS_Store` would make a pack hashable but unservable). Archive
  reading is bounded (512 MiB, 4096 files, 255-byte paths, 64 MiB a file);
  `rl-proto` restates the archive limit without depending on `rl-pack`,
  and a test pins the two equal (PF11-1). Paths are portable (final
  review C1: on Windows a `C:` component replaced the cache's path):
  every component a portable name (no `:`, `<>"|?*`, control characters,
  trailing dot or space, or Windows device name, any case and
  extension), and no two paths or directories alike but for ASCII case
  (one file on Windows and macOS); `encode` and `decode` refuse the same.
  `DirFs` writes only portable names and builds native paths only from
  plain components, on every platform; the OLX mod importer renames a
  weapon id that is a device name and skips a non-portable asset name.
  What an archive unpacks to is bounded (final review C2: a 61 KB skin of
  deep empty files took 111 MB on disk): at most 8 components a path, at
  most as many directories as files and 512, and a disk charge (a file's
  length rounded up to 4 KiB blocks plus a block, a block a directory) of
  at most 512 MiB, charged before each file is written; a skin's archive
  holds at most 16 files of depth 2 and 256 KiB of charge
  (`SKIN_LIMITS`). The charge assumes 4 KiB blocks: on large-cluster file
  systems (exFAT) the real use can be several times the charge.
- **N30 - a content-addressed cache.** Downloads live in
  `<cache>/packs/<content hash>/`, one pack directory each, with
  `<cache>/index.ron` (each entry's id, kind, bytes and a use counter).
  Above `CACHE_BYTES` (1 GiB, partial downloads counted) the least
  recently used entries are evicted, never one the current lobby lists,
  and the oldest partials not listed go first. The index is checked
  against `packs/` when the cache opens (a pack installed but not recorded
  is adopted, a record without its pack dropped). A match's content is the
  player's own content with the needed cached packs laid over by id
  (`Content::with_packs`); a local pack of the same id but another hash is
  hidden for that match only, never changed. Paths: the game's cache is
  `ProjectDirs::cache_dir()/downloads` (`--data-dir D`: `D/cache/downloads`),
  the server's `<data dir>/downloads`. One directory belongs to one open
  cache (PF11-3): opening holds an exclusive lock on `<dir>/lock`, and a
  second game falls back to `downloads-2` to `downloads-4`; the peers of
  one process share one cache. A peer without a cache (none opens:
  PF11-4) has downloads off and the lobby says why; a server whose cache
  is held (another server on the same data directory) logs a warning and
  runs without one: it still serves its own packs, and members' skins it
  lacks draw as the default skin. The Options' Online tab shows the
  cache's size and clears it, refused while an online game is on. The
  cache counts each pack by its disk charge (N29), not its bytes, so the
  budget and eviction see real disk use (final review C2). "Never one the
  current lobby lists" holds for the whole process: every list (a host's
  member-skin fetchers and its own list, a client's fetcher, both peers
  of `--net-sim`) registers its hashes with the shared cache
  (`rl_content::Keeper`), and every eviction keeps their union (final
  review Minor 2). Partial downloads are charged as installed packs are
  (plus a block for the meta file), at most 32 per cache (a new one
  discards the oldest not kept; when all are kept it is refused), and
  indexed in memory, so no fetch start scans the directory (final
  re-review R1).
- **N31 - skins by hash.** `Look` gains `skin_hash` (64 lowercase hex).
  A client offers its own skin to the host with a one-item content list
  (R6-1; accepted only for its look's id and hash); the host fetches a
  member's skin it lacks from that member and serves it to the others.
  Bots' looks in the setup keep `NO_HASH`; the host lists a bot's skin
  with its own pack's hash (R6-2: filling them would change the setup
  under its owner). A skin is laid over only when no local pack has its
  id: local packs win (PF11-5), and a skin is never taken under the id
  of a required pack. Skins never block the ready mark: one that cannot
  be fetched draws as the default skin. Mod, level and theme are
  *required*. A host starts at most one new skin fetch from a member
  every 5 s (an offer waits; only the look then worn counts), and takes
  at most 4 `SetLook`s a second from a member (final review C2); the
  latest look beyond them is held and taken once the window allows, so a
  member's last look is never lost. The 5 s interval is kept by the
  member's address for 5 s after it leaves, all members together start
  at most 12 skin fetches a minute, and a member-skin partial is
  discarded when its offer is dropped (final re-review R1, R2). The
  host lists one skin per id, the first member's (final review Minor 1:
  a match loads skins by id, so another of the same id is never shown).
- **N32 - transfers.** Reliable-channel tag `TAG_CONTENT = 3` carries
  `ContentMsg` both ways (`List`, `Want`, `Begin`, `Chunk`,
  `Unavailable`, `Cancel`); chunks are 32 KiB. A peer serves one archive
  at a time to each peer and queues the other wants. Pacing: at most
  2 MiB/s to each receiving client, 4 MiB/s host-wide (the server's
  `upload_kib_s`, default 4096, clamped to 64..=65536), 256 KiB/s
  host-wide while a match starts or runs, 128 KiB/s for a client serving
  its own skin, at most 256 KiB queued per peer. Resume: the fetcher keeps
  `<hash>.part` and `<hash>.part.ron` (archive hash and size) and asks
  `Want { hash, offset }`; another archive hash restarts from 0. Every
  download is verified (archive SHA-256, bounds, content hash, id, kind,
  `rl_pack::validate`); one that fails is fetched once more from 0, then
  failed. The host sends no list until every required archive is made; a
  required pack it cannot archive is listed with size 0 and answered
  `Unavailable`.
- **N33 - lacking content waits, never drops.** At a start, a member
  whose required content is incomplete sits the match out as
  `SeatWire::Waiting`: it is not in the session, gets no state, and stays
  connected. A joiner during a match waits the same way until its content
  is complete, then spectates. This replaces plan 10's "a spectator
  lacking content is dropped at every start". A member counts as lacking
  (not ready, waiting at a start) until its `Have` answers the host's
  current list (each list has a revision, raised only when its required
  items change: a skin change keeps every ready mark). A waiting member
  counts toward `max_players` only if it wants to play.
- **N34 - protocol version 2.** `PROTOCOL_VERSION` becomes 2, because
  `Look`, `Member` and the reliable tags change.
- **N35 (PF11-6) - the download prompt.** A list whose missing items not
  yet approved need more than `PROMPT_BYTES` (50 MiB) still to download
  waits for the player's Download (or Leave). The bytes counted are each
  item's size less what its partial download already holds ("missing
  bytes" is what the player will wait for), and items approved before are
  not asked about again: a re-sent list (a skin change, a new member)
  never re-prompts for an approved download, and the prompt's number does
  not change while approved items download. A list under the limit
  approves itself. Sizes show in MiB as "MB", the threshold's unit, so
  the prompt's floor reads "50.0 MB" (final review Minor 7).
- **N36 (plan 11 task 8) - replays read the caches without opening
  them.** A replay's packs come from the player's content, else from any
  of the game's cache directories (`downloads`, `downloads-2` to `-4`)
  by content hash, read without the lock or the index
  (`DownloadCache::lookup`); `check_replay_packs` hashes them again. So a
  replay finds a pack whichever cache directory a session used, plays
  while a game holds the cache, and `--replay` never creates, locks or
  writes a cache.

## Kept OLX quirks

- **Healing passes 100 health:** `CGameObject::injure`
  (`src/common/CGameObject.cpp:35-44`) subtracts negative damage (MW 1.0's
  healing weapons) with no cap, unlike a health bonus; `InjureWorm` caps a
  heal only when the worm's `shieldFactor` is 0
  (`src/client/CClient_Game.cpp:350-357`). The
  headless matches' invariants (`rl-cli::invariants`) only require health to
  be a number of at least 0.
- **A worm can be wedged in a rock ceiling:** standing half in dirt under a
  low rock ceiling, a jump makes `moveAndCheckWormCollision` take the dirt
  at the worm's own row for the floor (`y = 0` counts as below) and lift it
  5 px, into rows it no longer checks (`src/common/PhysicsLX56.cpp:216-238`);
  its centre then sits in rock, stuck until it dies (seed 303 of the bot
  grid, Classic on FossilFacility: 686 ticks). The headless invariants
  allow a centre in rock but not a worm buried in it, with rock over its
  whole collision cross (`rl_cli::invariants::buried_in_rock`).
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
