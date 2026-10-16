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
| Cosmetic entities (spec §19 E1, E8): the pool (OLX's 1024 slots with `olx_quirks`; 16384 with a death reserve in play since 2026-10-13, see Deliberate deviations), per-sub-step physics and animation, stains, blood drips, gib spins and colours, drawing and clipping | `src/client/Entity.cpp:161-468`, `include/FastVector.h`, `src/common/CWorm.cpp:671-757`, `src/common/CMap.cpp:338-352` | `rl-fx` | done (plan 5 task 4) | `blood_falls_and_stains_like_olx`, `explosion_frames_and_invisible_start`, `gibs_use_the_victims_colour`, `entity_cap_drops_new_ones`, `animations_live_as_long_as_olx`, `fx_never_touches_sim_randomness`, `colour_edits_follow_the_sim_like_apply_edits` |
| Sound (spec §8.4, §19 E1, E12): positional mix (gain 0.7, inverse distance clamped, reference 50, rolloff 2, listener 20 px out at the view centre), THROW and PICKUP at the listener with gain 1.0, voice limits (a death sound steals since 2026-10-13, see Deliberate deviations), live voices re-mixed as the listener moves, silence without a device | `src/sound/sound_sample_openal.cpp:206-270`, `src/sound/sfx.cpp:120-134`, `src/sound/sfxdriver_openal.cpp:43-92`, `src/game/Sounds.cpp:25-30`, `src/common/CGameScript.cpp:1185-1205` | `rl-audio` | done (plan 5 task 6) | `positional_gain_matches_olx_openal_defaults`, `voice_limits`, `silent_when_no_device`, `plays_events_on_a_mock_device`, `decodes_every_mod_sound`; `device_check` example (`cargo xtask full`) |
| Whose rope throws are heard (the user's playtest report, 2026-10-16: every worm's throw played map-wide at gain 1.0, about 7 to 12 starts a second with 7 to 8 bots): only the player's own (the local slot; a replay's viewer), as OLX plays `smpNinja` only in the human input handler; a bot's or a remote worm's throw is silent. `rl-sim`'s THROW events carry no owner, so the session keeps as many as the player's `NinjaRope::throws` grew (they all play at the listener) | `src/common/CWormHuman.cpp:340-342`, `555-563`, `570-574`, `src/common/CWormBot.cpp:1753`, `3823`, `src/common/PhysicsLX56.cpp:300-303` | `rl-game::throw_sound`, `Session::sounds` | done (2026-10-16) | `only_the_players_rope_throws_are_heard`, `bots_alone_throw_silently`, `a_replay_hears_its_viewers_throws`, `an_online_match_runs_headless`; a throw while the worm may not use the rope (`can_use_ninja` false, never set today) is not heard, OLX's is |
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
| Replays (spec §20 F8; no OLX equivalent): the setup, pack ids and content hashes, every worm's keys per tick, the viewer's slot (the player whose game made it), checksums at tick 0, every 100 ticks and the end; the `RLRP` format, version refused when unknown | — | `rl-replay::{Recorder, Replay, Player, Cursor}`, `rl-content::{replay_packs, world_for_replay, play_replay}` | done (plan 6 task 6) | `record_then_replay_gives_identical_checksums`, `replay_reports_the_first_mismatching_tick`, `replay_reports_a_different_setup_at_tick_0`, `replay_refuses_unknown_format_versions`, `replay_refuses_mismatched_content`, `bot_match_replays_from_content`, `replay_size_is_reasonable`, `decode_never_panics_on_junk`, `a_failing_tick_is_recorded_without_a_checksum`, `the_viewer_is_recorded`, `the_viewer_is_one_of_the_worms` |
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
| Players leaving mid-match (plan 12d; user decisions, 2026-10-15): a player who leaves, times out or is kicked is withdrawn from the match at one tick on every peer, its worm gone (`CGameMode::Drop`, a kill credited to it credits nobody), fewer than 2 worms left ends the match, "<name> has left" / "<name> has timed out", the live scoreboard leaves it out and the results keep a "left" row (deviation below) | `src/server/CServer.cpp:1352-1482`, `src/server/CGameMode.cpp:254-263`, `316-336`, `src/client/CClient_Parse.cpp:1650-1680`, `src/game/Game.cpp:1359-1366`, `src/client/Options.cpp:769-770` | `rl-sim::World::withdraw`, `rl-modes` (`on_drop`), `rl-replay` (format 3), `rl-proto`/`rl-netcode` (the marks), `rl-lobby`, `rl-online`, `rl-ui`, `rl-game` | done (plan 12d) | `a_withdrawn_worm_leaves_the_board_and_the_last_one_wins`, `a_withdrawn_worm_never_wins`, `a_projectile_of_a_worm_that_left_credits_nobody`, `a_gone_worms_kill_credits_nobody`, `a_withdrawal_replays_at_its_tick`, `a_withdrawal_after_the_last_tick_replays`, `withdrawal_marks_are_bounded`, `a_withdrawal_mark_is_final`, `a_withdrawal_learned_late_rolls_back_to_it`, `a_leaving_client_is_withdrawn_at_one_tick_everywhere`, `a_leave_during_a_migration_is_withdrawn_alike`, `a_forged_withdrawal_never_removes_a_present_player`, `a_rejoined_player_marked_by_the_old_host_stays_and_its_holders_are_repaired`, `a_repaired_holder_that_hosts_later_withdraws_the_player_at_its_leave`, `a_member_that_times_out_is_said_to`, `the_last_opponent_leaving_ends_the_match`, `a_player_leaving_during_a_migration_is_removed_at_the_resume`, `a_leaving_player_is_removed_from_every_world`, `a_left_row_is_off_the_scoreboard_and_left_in_the_results`, `the_watched_worm_moves_on_when_it_leaves`, `an_online_watcher_moves_on_when_the_followed_worm_leaves`, `a_player_kicked_mid_match_is_withdrawn`, `a_client_crash_excuses_the_fills_stalls_until_its_timeout`, `a_client_leave_withdraws_its_worm_in_a_real_match`, `a_bundle_with_a_withdrawal_reproduces_to_the_fault` |

## Deliberate deviations

- **Results keep a "left" row** (N85, user decision 2026-10-15): OLX
  deletes a leaving client's worm (`Game::removeWorm`,
  `src/game/Game.cpp:1359-1366`), so its results lose it; ours keep its
  row, its lives "left", its kills and deaths as it left (greyed). The
  live scoreboard leaves it out, as OLX.
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
- **The board counts every death and suicide** (user decision, 2026-10-15,
  after a playtest read "11 deaths" for three worms and "7" for a fourth
  that was also out, with 10 lives). OLX's `iDeaths` and `iSuicides`
  (`CWorm::addDeath`/`addSuicide`, `src/common/CWorm.cpp:1430-1442`)
  count nothing while the worm's score is 0 or less without
  `AllowNegativeScore` (the default). They are scoring counters: OLX never
  shows them (`CClient::UpdateScore`, `CClient_Draw.cpp:1851-1950`, shows
  lives, kills and damage). `rl-sim` keeps them on the worm
  (`Worm::deaths`/`suicides`, the oracle's), and the score, ranking and
  winner still read them (`olx_score`). Deathmatch also counts every death
  and suicide (`died`, `killed_self`, in `Deathmatch::kill`), and the
  board's `Score::deaths`/`suicides`, which the results and the Tab
  scoreboard show, are those. A worm that leaves keeps its counts on its
  left row. With `Lives` N, a worm that is out has died N + 1 times, as in
  OLX.
- **Mode commands have no OLX equivalent** (spec §12.1 H1): they are applied
  after `CGameMode::Simulate`, in queue order. `give_weapon` makes the slot
  full and ready, `LastFire` included (a weapon bonus keeps `LastFire`).
- **The damage score (`fDamage`) is counted once**, on the client
  (`addDamage(.., false)` in `InjureWorm`). OLX also counts it a second time on
  the server, a frame or more later, from the batched `C2S_REPORTDAMAGE`
  (`ParseReportDamage`), so its `fDamage` is about twice the real damage. It
  only breaks score ties and is not in the trace.
- **Hits test the worm's current position; every death is credited and
  shown (2026-10-13; spec §18 D7 and D9, amended).** Three OLX bugs are
  fixed in play and kept only for the oracle, behind
  `SimSettings::olx_quirks` (on when the settings come from an OLX trace,
  `SimSettings::from_olx_features`: the oracle, `rl-scenario`, `rl-cli
  scenario` and the render goldens drawn from scenarios; off in
  `SimSettings::lx56` and so in local play, bot matches, online matches
  and replays, which record it). Every peer of an online match plays it
  off: it is a constant, not a lobby setting.
  - **The hit position.** OLX's `DataTimeRecorder::get(framesBack)`
    (`src/util/VecTimeRecorder.h:37-59`) reads
    `m_data[(cur + 100 - framesBack % 100) % 100]` while `cur` is the next
    slot to write, so frames-back 0 is the *oldest* of the 100 entries.
    `getBest` (`:54-59`), from `CProjectile::ProjWormColl`
    (`src/common/PhysicsLX56_Projectiles.cpp:52-111`) and
    `CClient::Explosion` (`src/client/CClient_Game.cpp:320-329`), asks for
    frames-back 0 about one time in ten (spec §18 D2's clock), so about one
    hit test in ten used where the worm was about 1.1 s earlier: worms died
    to shots that visibly missed (the investigation's
    `classic-dirt-level` 2-bot seed-2 match: a self-kill at tick 4629, the
    worm 27-33 px from every pellet), and shots passed through worms. In
    play `get(k)` reads `m_data[(cur + 100 - 1 - k % 100) % 100]` (frames-back
    0 is the newest), the hit tests and explosions read the newest entry
    (`PosRecorder::hit_position`: the worm's position after its last
    physics step, which the projectile step follows; OLX's frames-back
    would read the entry a step older most of the time), and a respawn
    fills the recorder with the spawn position, so a new life is never hit
    where the previous one was. `rl-cli`'s `tests/hits.rs` checks, through
    `World::hit_audit`, that no hit of seeded bot matches comes from
    outside the struck worm's current hit geometry (`|dx|, |dy| < 4 +
    radius`; explosions `d² <= 25`).
  - **The exact-zero death's credit.** OLX's `CWorm::base_die`
    (`src/gusanos/base_worm.cpp:623-653`) credits the death to
    `m_lastHurt`, which classic damage never sets, so killer -1. In play it
    is credited to the worm whose damage left 0 health
    (`Worm::last_damager`, set by every damage that lowers health, cleared
    on respawn).
  - **The exact-zero death's effects.** OLX plays the death sound, 7 gibs
    and 50 rounds of blood only in `InjureWorm`'s lethal branch
    (`CClient_Game.cpp:419-437`) and in a remote client's `ParseWormDown`
    (`src/client/CClient_Parse.cpp:1817-1860`); `killWorm`'s
    `SendWormDied` sends the local client nothing
    (`src/server/CServer_Send.cpp:1122-1135`), so the local player saw an
    exact-zero death without sound or gibs. In play `die_at_zero_health`
    plays the lethal hit's effects (and their `rand()` draws) for every
    death. Every death path now shows: a lethal hit (from a projectile, an
    explosion, a beam), a hit on a worm already at 0 health (below 0:
    killed at once, credited to that hit), and the exact-zero death. After
    the game over `killWorm` kills nobody: a lethal hit then plays its
    effects and leaves the worm alive at 0, as in OLX, and its next-frame
    death plays none (`Worm::death_shown`), so no death shows twice; a
    worm left at exactly 0 then vanishes with its effects (OLX: silently).
    `rl-cli`'s `tests/deaths.rs` checks every kill of seeded bot matches.
  - **No death effect is dropped downstream** (amended the same day after
    review). A death's sound, 7 gibs and 150 blood events are pushed as
    the death's where they are emitted (`TickEvents::death`,
    `Draws::death_entity`, marked `Event::death`) and count against
    `TickEvents`' gameplay quota, not the cosmetic one, so a tick's other
    cosmetics never crowd them out. Online play shows a death's events
    once per worm and life (`NetPresenter`, the life counting the worm's
    spawns as last simulated), whatever tick within the rollback window
    (`MAX_PREDICTION`) a correction moves the death to and whichever of
    the three sounds it drew; a death erased by a correction never hides
    the real one later in that life, nor after a state load. The entity pool keeps a
    reserve for deaths and a death sound steals a voice: the two entries
    below.
- **More cosmetic entities in play (2026-10-13; spec §19 E1, amended).**
  OLX's client keeps at most 1024 cosmetic entities (`MAX_ENTITIES`,
  `include/Entity.h:30`; `FastVector<entity_t, MAX_ENTITIES>`,
  `src/client/Entity.cpp:31-32`) and drops new ones beyond it; in a 4-bot
  MW 1.0 chaos match 6 of 24 deaths found it full of blood and showed no
  gibs. The user's ruling: OLX's count was a baseline to recreate, not a
  limit to keep, and graphics fidelity will rise. In play (`rl_fx::Fx::for_match`
  without `olx_quirks`) the pool holds `PLAY_ENTITIES` (16384), allocated
  once at match start (about 1 MB), and its last `DEATH_RESERVE` (1024)
  slots take only a death's gibs and blood (`Event::death`): other
  spawns and blood drips stop at 15360, so a death always shows (157
  entities; the reserve holds six at once). The hard cap stays, with
  OLX's drop-when-full beyond it, for hostile mods. With `olx_quirks`
  (scenarios, render goldens) the pool is OLX's 1024 with no reserve. The
  simulation's own entity counter (`rl-sim`'s `rng.rs` `MAX_ENTITIES`,
  the dedicated server's pool that gates the gibs' draws) is unchanged.
  Cost (`cargo xtask envelope --only fx`, 16 chaos bots on Castle Strike,
  2026-10-13): a match peaks at 7572 entities; `present` p50/p99 26.6/54.0 µs
  and the draw-list build 37.1/82.6 µs against 8.8/19.7 and 11.5/26.7 µs
  with 1024, the tick unchanged. The heavy replay (MW 1.0, 32 chaos bots,
  a 20 000 pool; `envelope --only gpu`, offscreen on the RTX 5070 Ti)
  takes about 1 ms more CPU a frame: tick and frame p99 at 1080p-4K
  2.7-4.9 ms against 2.2-3.2 ms before (DX12 3.3-4.9, Vulkan 2.7-2.9,
  WSL's GL 4.8-5.0), the GPU's own time +5 %; every native path still
  holds 144 Hz (6.9 ms). On the software renderer (llvmpipe) the stress
  run's 1440p frame p50 rose 10-29 % (6.0-6.3 ms to 6.5-7.8 ms).
- **A death sound steals a voice (2026-10-13).** OLX skips a sound
  beyond its `maxplaying` (two of each death sound,
  `src/game/Sounds.cpp:25-30`, `src/sound/sfx.cpp:120-134`) or when no
  OpenAL source is free, so deaths close together could go unheard. The
  user's ruling: every death plays its sound. `rl_audio::Voices::admit`
  answers start, steal or skip: a death sound at its own limit takes the
  place of the oldest voice of that sound; at the global limit (256) the
  oldest of that sound, else the oldest voice that is not a death's
  (push order, deterministic). `Audio::play` takes the voice only once
  the new one has started, and stops it with a 5 ms fade. Every other sound is skipped as in OLX. Audio never runs
  in the oracle, so it applies always.
- **Exact-zero deaths' Gusanos particles (spec §18 D9).** Three scenarios stay
  pinned in `rl-scenario/tests/oracle.rs`, the only ones that do not match
  bit for bit (each row there carries this reason: bit-exactness is impossible
  without porting the Gusanos particle system, whose `rnd()` draws shift the
  next random respawn spot). A worm left at exactly
  0 health dies at the start of the next frame, uncredited, as in OLX (with
  `SimSettings::olx_quirks`; play credits it, see above); the
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
    the remake skips a start beyond 256 voices (OpenAL Soft's default),
    except a death sound's, which steals one (Deliberate deviations,
    2026-10-13).
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
- **Weapon name over the worm** (`rl-game`, the user's request;
  `src/common/CWorm.cpp:1304-1315`, `src/gusanos/base_worm.cpp:678-684`,
  `src/common/CWormHuman.cpp:405-428`): as OLX, while the change-weapon
  key is held, and for 0.75 s (75 ticks) after a change made with it, the
  player's own worm shows its current weapon's name (the mod's, dots kept)
  in OLX's `PlayerName` white; bots, other players online and the worms of
  a game nobody plays show none. It is egui text in the worm names' font
  with their drop shadow, not OLX's outline font, and it takes the place
  of the player's own name label (OLX draws no name over a local human
  worm and puts the weapon name's top 15 world pixels above it, `y-30` at
  double resolution: where the name label sits here), kept inside the
  picture at its edges. The 0.75 s are counted in ticks, so they stand
  still while the game is paused (OLX's run in real time). With one local
  player (no split screen), OLX's weapon name of every local human worm in
  every viewport is the player's worm's alone.
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
  seeking (deferred). In every game nobody plays (a replay, `--spectate`,
  an online match watched) Left/Right choose the worm the camera and the
  HUD follow, in worm order, round (`Session::spectating`, `watch_key`;
  `tests/watch.rs`); PageUp/PageDown are the speed's alone, and do nothing
  online. A replay starts on its viewer: the worm of the player whose
  game made it (`ReplaySetup::viewer`, in format version 2: the first human
  of a local match, the peer's own slot online; none for bots alone or an
  online spectator, then worm 0).
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
  Amended 2026-10-13: kept for the oracle only (`SimSettings::olx_quirks`);
  play hit-tests the worm's current position (Deliberate deviations,
  "Hits test the worm's current position").
- **D9, exact-zero health** (see above): the death is ported, the Gusanos
  particles' `rnd()` use is not. Amended 2026-10-13: in play the death is
  credited to the last damager and plays the death effects; uncredited and
  silent only with `SimSettings::olx_quirks` (the oracle).

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
  **Amended by plan 12d (N83):** the slot is still filled with `Keys(0)`
  (until the host's 10 s timeout the lagging rule repeats its last final
  keys instead, N87), and from its first unknown tick its worm is
  withdrawn from the match (`World::withdraw`); the user decided removal
  over a bot (2026-10-15).
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
  signalling TCP connection uses, in both directions; no STUN for
  candidates until plan 12b (plan 12a's STUN only learns the public
  address, N40).
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

## Finding games decisions (plan 12a)

Decided in `docs/superpowers/plans/2026-10-13-plan-12a-finding-games.md`
("Decisions this plan makes") and by rulings during its execution
(PL12-1 to PL12-3, PF12-4 to PF12-8, R1-* to R10-*).

- **N37 - the host proves its key.** The joiner sends its own 32-byte
  nonce in `Hello`; the host answers `ToClient::HostProof { signature }`
  over `b"rust-liero host v1" ++ client nonce ++ client key ++ offer DTLS
  fingerprint ++ answer DTLS fingerprint`. The joiner's own proof
  (`challenge_message`) covers both fingerprints too, so a relayed or
  swapped handshake fails both proofs. The fingerprints come from the
  certificates str0m verified for the connection (`LobbyClient::connected`),
  never from parsed SDP (R1-1). A joiner with `expected_host = Some(k)`
  (an invite's key, or a listed game's) refuses a host whose key is not
  `k` or whose proof fails: "this is not the game you were invited to"
  (`WRONG_HOST`). The password proof travels in `Prove`, only after the
  host's proof verified, and is `SHA-256(host nonce ++ joiner key ++ offer
  fingerprint ++ answer fingerprint ++ password)` (plan 12a final review
  B-I1: it was `SHA-256(nonce ++ password)` in `Hello`, so an impostor
  relaying the host's challenge could replay a player's proof under its
  own key, and any host could ask for a proof to guess offline). A
  password refusal before the host's proof closes the join unbelieved. A joiner also gives up on a host that opens the link and
  then stays silent (`JOIN_DEADLINE` 10 s, "the game did not answer",
  `NO_ANSWER`; keepalives alone would keep it waiting for ever). Protocol
  stays 2 (unreleased).
- **N38 - one Nostr core, no Nostr library.** `rl-discovery` implements the
  NIP-01 subset it needs (event serialization and id, BIP-340 signing
  through `Identity`, the frames `EVENT`/`REQ`/`CLOSE`/`EOSE`/`OK`/`NOTICE`/
  `CLOSED`) and NIP-40 expiration, sans-IO, so the dependency tree and the
  WASM build (plan 13) stay small. Event kinds: 31470 (addressable
  announcement, tag `d` = `rust-liero`) and 21470 (ephemeral, reserved for
  plan 12b's handshakes); neither is in the NIPs kinds table or the
  registry-of-kinds (checked 2026-10-13, to re-check before a release).
  The relays and STUN servers are the lists below, chosen by measurement
  (`rl-cli nostr-probe`, `rl-cli stun-probe`, run once by hand on
  2026-10-13 from WSL2 over IPv4, one vantage point), not by reputation.
  `DEFAULT_RELAYS` is append-only: an invite code names a relay by its
  index, so a dead relay stays in place and a test pins the released
  prefix. "OK ms" is the round trip from publishing the note to its `OK`;
  the operators come from public directories, not from the probe.

  Default relays (each accepts kind 31470, serves the note at once and
  after 3 minutes, forwards kind 21470 live, asks for no payment or
  authentication):

  | index | url | operator | accepts 31470 | keeps ≥3 min | forwards 21470 | no payment/auth | RTT (OK ms) | connect ms |
  |---|---|---|---|---|---|---|---|---|
  | 0 | wss://relay.damus.io | Damus | yes | yes | yes | yes | 201 | 615 |
  | 1 | wss://nos.lol | nos.lol | yes | yes | yes | yes | 40 | 141 |
  | 2 | wss://relay.primal.net | Primal | yes | yes | yes | yes | 60 | 212 |
  | 3 | wss://relay.snort.social | Snort | yes | yes | yes | yes | 60 | 315 |
  | 4 | wss://offchain.pub | offchain.pub | yes | yes | yes | yes | 161 | 474 |
  | 5 | wss://nostr.oxtr.dev | 0xtr | yes | yes | yes | yes | 40 | 131 |
  | 6 | wss://relay.nostr.net | nostr.net | yes | yes | yes | yes | 70 | 282 |
  | 7 | wss://nostr.bitcoiner.social | bitcoiner.social | yes | yes | yes | yes | 211 | 635 |

  These relays passed but were not chosen:

  | url | operator | why not chosen |
  |---|---|---|
  | wss://nostr.mom | nos.lol's sibling relay (likely the same operator) | not a separate operator |
  | wss://nostrue.com | nostrue | passed; 9th by order, spare |
  | wss://nostr-pub.wellorder.net | scsibug (nostr-rs-relay author) | the idle connection was reset 5 times during the 3-minute wait |

  These relays failed:

  | url | why |
  |---|---|
  | wss://relay.nostr.band | connect timed out |
  | wss://relay.nostr.bg | no DNS |
  | wss://relay.0xchat.com | no DNS |
  | wss://nostr.fmt.wiz.biz | connect timed out |

  Skipped without probing as paid or whitelist-only: nostr.wine,
  nostr.land, nostr21.com; purplepag.es takes only profile and relay-list
  kinds. The chosen 8 come from 8 operators, as far as public information
  shows.

  Default STUN servers (`public_ip` takes the first that answers):

  | server | operator | answers | RTT ms | mapped family |
  |---|---|---|---|---|
  | stun.cloudflare.com:3478 | Cloudflare | yes | 16 | IPv4 |
  | stun.l.google.com:19302 | Google | yes | 20 | IPv4 |
  | global.stun.twilio.com:3478 | Twilio | yes | 23 | IPv4 |
  | stun.nextcloud.com:443 | Nextcloud | yes | 20 | IPv4 |
  | stun.sipgate.net:3478 | sipgate | yes | 23 | IPv4 |

  These answered but were not chosen: stun1.l.google.com (Google again),
  stun.voip.blackberry.com, stun.miwifi.com (Xiaomi, 340 ms).
  stun.stunprotocol.org, one of the placeholders, no longer resolves and
  was dropped. The STUN probe never records the mapped address (it is the
  prober's own IP). Open: wellorder reset an idle connection every ~36 s,
  so another relay may do the same under the driver's 30 s idle ping; a
  shorter idle ping is the remedy.
- **N39 - the announcement.** The content is JSON (`serde_json`, at most
  4 KiB): `{v, name, mode, map, map_name, mod, mod_hash, players,
  max_players, bots, spectators, max_spectators, password, region,
  playing, addr, port}`. Tags: `["d","rust-liero"]`, `["t","rust-liero"]`,
  `["f","<fingerprint, 16 hex>"]`, `["expiration","<unix+180>"]`. A note is
  re-published every 60 s and when the lobby changes (at most every 12 s,
  R5-3). On close the host publishes a note with `"closed": true` and an
  expiry 10 s ahead, `created_at` at least a second newer than its last
  note (a same-second tie would let a relay keep the open note, R9-5).
  `created_at` is the wall clock read at each note (final review A-I2: it
  was the unix time at start plus the monotonic clock's elapsed time, so a
  suspend or an NTP step after the start dated every note expired at
  birth); the game list's ages in rl-game use the wall clock read at each
  poll too. A game hosted again right after its closing note dates its
  first note 10 s after that note (final review A-M6: relays keep the
  newest note of a slot and lists refuse one older than the tombstone, so
  it stayed hidden up to a minute).
  One builder, `GameSummary::of_lobby`, serves rl-server and rl-game and
  also gives the LAN broadcast (`.lan()`); players and spectators are held
  to their maximums (R9-1, R9-8). **Amended 2026-10-15** (the user's report
  of 2026-10-15: "4/8 players" listed for one player who added three
  bots, misleading joiners, since bots take no seat: rl-lobby's
  `admit_seat` counts members alone against `max_players`, and only the
  32-worm total limits bots): `players` counts the seated members only,
  and the new `bots` (the setup's bots, at most `MAX_PLAYERS`;
  `#[serde(default)]`, so an older note reads as none) counts the bots
  apart, in the note and in `LanAnnounce` (appended; postcard is
  positional, so an older build's LAN datagram does not decode, as its
  fingerprint already differs). Both lists show "1/8 +3 bots" ("1/8"
  without bots), and a game is full when its seats are: bots never fill
  one.
- **N40 - the public address.** A host learns its public IP from the
  configured `public_address`, else a STUN binding request (RFC 5389) to
  the first answering server of the STUN list (IPv4 only, 2 tries of
  500 ms per server, a 3 s deadline in all, on a thread; again every
  10 minutes). Only a global address is kept: a later round with no answer
  keeps the known address, one answering a non-global address drops it
  (R9-4). The announcement's `addr` is that IP and `port` the host's port.
  Without either, a public game is still announced, without an address:
  the list shows it as "cannot be joined yet" (plan 12b adds handshakes)
  and its invite carries no address (the lobby says "Not listed: no public
  address (forward port N or set it in Options)"). STUN is only about the
  address: ICE candidates stay as in N16 until plan 12b. **Amended by plan
  12b (N47):** STUN no longer gives the advertised address; a note's or
  an invite's address is a usable router mapping or `public_address`, and
  STUN finds only an ICE candidate (see "Through routers decisions").
- **N41 - the game list.** `GameList` verifies every event (id and
  signature), keeps the newest note per key, drops expired notes and closed
  games, shows games of another fingerprint greyed out ("needs version X"),
  accepts at most one note per key per 10 s of arrival, or dated 10 s after
  the key's last (a closing note of a listed game is exempt), at most 500
  games and 4096 remembered keys (the oldest unlisted one is evicted when
  full, so hostile keys cannot lock honest hosts out, R5-2), refuses notes
  older than 180 s or more than 60 s in the future, and hides blocked keys.
  Every accepted closing note leaves a tombstone, listed or not, so a relay
  that missed the close cannot bring the game back with its older open
  note (final review A-I1). Cheap refusals (a full list's new key too)
  come before the signature check. Announced addresses must be global (PF12-5). Ping is a TCP connect
  to `addr:port`, timed, 2 s timeout, at most 4 at once, on a game's first
  selection; a failed ping sinks the game to the bottom for the life of the
  list.
- **N42 - invite codes.** bech32m with HRP exactly `rlx` (`rlx1…`, any one
  case); the payload is postcard `{version: u8 = 1, key: [u8;32], relays:
  Vec<RelayHint>, addr: Option<(IPv4, u16)>}`. `RelayHint` is `Default(u8)`,
  an index into the append-only default relay list, or `Url(String)`.
  Hints are at most 3 `Default` and 1 `Url` of at most 64 bytes, so the
  worst case is 191 characters and a code never exceeds `MAX_CODE` (200):
  this narrows the plan's "at most 2 URLs of 100 bytes" (PF12-6). A
  `Default` index beyond the build's list is skipped, so an old build reads
  a new build's code; a wrong checksum, a plain-bech32 checksum or another
  HRP is "damaged"; a newer version is "newer". Joining by code uses the
  address and expects the code's key (N37). A code without an address
  cannot be joined: "this invite has no address: the game cannot be joined
  yet". **Amended by plan 12b (N46):** such a code joins through the
  relays.
- **N43 - visibility.** `Visibility { Public, Lan, Invite }`. Public is
  announced on Nostr and broadcast on the LAN; Lan is broadcast on the LAN
  only; Invite is neither, and its invite code is shown in the lobby (with
  Copy) and printed by the server (`/invite` repeats it: anyone's in a
  Public game, admins' otherwise). The plan 10 `DirectOnly` reads as
  `Invite` (serde alias). The server's default is Public (it was Lan: a
  config with no `visibility` now announces on public relays); the game's
  own hosting stays Lan until the player picks "Anyone (Internet)" (R10-1).
  STUN runs for every server visibility, since every server prints its
  invite and the invite needs the address (R9-2): a Lan or Invite server
  contacts the STUN servers at its start and every 10 minutes. The game asks
  STUN for Public and Invite games only, never for Lan (R10-4). (Amended by
  plan 12b, N47 and N48: STUN now runs on the game socket every 25 s for
  ICE, and no longer gives the invite's address.) LAN
  announcements keep the old broadcast, with counts now held to the
  maximums.

Plan rulings:

- **PL12-1:** the spec's plan 12 row is split into 12a (finding games: the
  Nostr list, invites, host-key proof, the STUN address), 12b (through
  routers: Nostr handshakes, ICE with STUN, port mapping, TURN relays,
  Docker NAT tests) and 12c (host migration, net-soak). Too large for one
  plan; each ends testable. If wrong: two extra plan documents.
- **PL12-2:** event kinds 31470 (addressable announcement) and 21470
  (ephemeral handshake, 12b), unassigned in the NIPs kinds table on
  2026-10-13; Task 2 re-checked the registry. If wrong: a kind change
  before release.
- **PL12-3:** the carried finding F-B (the host-key proof bound to the DTLS
  fingerprints) is Task 1, before invites rely on `expected_host`. If
  wrong: none; it is N37.

Pre-flight and process rulings:

- **PF12-4:** the relay driver uses non-blocking sockets with explicit short
  waits (send retry, handshake, idle read), not `SO_RCVTIMEO`/`SO_SNDTIMEO`:
  after a send timeout the socket's state is indeterminate on Windows
  (partial sends duplicated or lost), while `WouldBlock` is well defined
  everywhere. Cost: about 10 ms polling latency per idle relay (about 100
  wakeups a second per relay; a longer idle wait would suit laptops).
- **PF12-5:** the game list refuses announced addresses that are not global
  (hand-written v4 and v6 ranges, v4-mapped unwrapped) unless `allow_local`
  is set, with no port policy: a note from a remote relay must never steer
  clients at their own network. If wrong: a LAN-only relay setup must enable
  `allow_local` (see PF12-8).
- **PF12-6:** invite hints are at most one URL of at most 64 bytes plus at
  most 3 defaults, so the worst case fits 200 characters and `encode` never
  produces an undecodable code (it drops bad hints and truncates). If wrong:
  a custom relay URL over 64 bytes cannot be hinted (the defaults still
  can).
- **PF12-7:** `rl-game` takes `rl-server` as a dev-dependency (same layer 6,
  tests only; `xtask layers` skips dev-dependencies): the end-to-end test of
  finding a game needs an in-process server. If wrong: one test helper to
  move.
- **PF12-8:** `allow_local` switches on by itself when every configured
  relay is loopback, private or `localhost`, so an office LAN relay lists
  LAN games; the shipped defaults keep it off and any public relay in the
  list keeps PF12-5's policy. If wrong: a user with only a LAN relay can be
  steered at LAN addresses by notes on that relay.

Task rulings:

- **R1-1:** the fingerprints in N37 are taken from str0m's verified local and
  remote certificates, not parsed from SDP; the host proof's `aux` is the
  challenge nonce. If wrong: the proof binds to what a peer claimed, not
  what was verified.
- **R4-1:** the pool's rate limits are token buckets that allow a burst: a
  sustained 200 frames/s and 1 MiB/s, each bucket holding 5 s of burst
  (1000 frames, 5 MiB). **Deviation from a strict per-second window:** a
  `REQ`'s stored events arrive at once (the list's filter asks for at most
  500, `GameList::filter`'s `limit`), and a strict window would drop such a relay and
  reconnect it into the same burst for ever. A sustained flood is still
  dropped (`a_flooding_relay_is_dropped`). If wrong: a relay that bursts
  more than 5 s of budget is dropped and retried.
- **R4-2:** the "5 s read timeout" is two deadlines: the TLS and WebSocket
  handshakes get 5 s together; an established connection must answer
  `EOSE` (10 s) and `OK` (5 s) when asked, and is pinged after 30 s idle
  (dropped 10 s after the ping unanswered). An idle connection is never
  timed out otherwise. A missing `OK` is reported, not fatal. If wrong: a
  slow relay is reported or dropped too soon (`Timings` is tunable).
- **R4-3:** a frame that is not a Nostr message drops the connection; unknown
  types (`AUTH`, `COUNT`) and a known frame with one bad field are skipped,
  so one bad stored event cannot make a relay useless. Events are verified
  in the driver, so a forged event carrying a real id cannot poison the
  cross-relay dedupe. Incoming is bounded at 4096 events or 8 MiB per relay
  (a count alone allowed 512 MiB). If wrong: a relay is dropped or an event
  lost where a laxer reading would keep it.
- **R5-1:** `GameList` accepts loopback and private addresses only with
  `allow_local` (PF12-5). **R5-2:** `seen` evicts the oldest unlisted key
  when full (a flood of fresh hostile keys no longer locks honest hosts out
  for 3 minutes). **R5-3:** the announcer's change gap is 12 s (the plan
  said 10 s), so a lobby that changes at every tick publishes at most one
  note per 12 s, inside the list's 10 s per-key gap.
- **R6-1:** `StunResponder` and `TestRelay` exist only behind the
  `test-relay` feature; STUN is IPv4 only, as `public_ip` is. If wrong:
  an IPv6-only host learns no address.
- **R8-1:** the probe lives in the library (`rl_cli::nostr_probe`), one
  fresh random key per run (never the player's identity), every event
  marked `rust-liero-probe` with a 10 minute expiry; the command publishes
  to public relays, so it is run by hand only and no test touches the
  internet. A relay's first connection failure fails it (no waiting for the
  pool's reconnect). Order of the defaults: well-known large operators
  first, since the index is fixed by invite codes. If wrong: a relay that
  needed a second try is left out of the defaults.
- **R9-1:** the summary builder lives in `rl-discovery` (the layer-5 crate
  that already depends on `rl-proto`), not `rl-online`: no new same-layer
  pair. **R9-2:** STUN runs for every visibility (see N43). **R9-3:** the
  server prints `invite code: rlx1…` once the address is settled
  (immediately when configured or when there are no STUN servers, else at
  the first answer, at most ~3 s) and again whenever it changes. (Amended
  by plan 12b, N47: the invite waits for the reachability to settle, at
  most 5 s; one without an address reads `invite code (joins through the
  relays): rlx1…`.) **R9-4:**
  see N40. **R9-5:** the closing note is forced newer (see N39).
  **R9-6:** `/invite` is admins-only in a Lan or Invite game. **R9-7:** an
  unknown `region` in a config reads as `Unknown`. **R9-8:** the server's
  default visibility is Public; LAN counts are held to the maximums, so a
  note is never refused for `players > max_players`. If wrong: an operator
  who wanted a private server must say `visibility: Invite`.
- **R10-1:** the game's default visibility stays Lan (see N43). **R10-2:**
  relays and STUN servers in the settings are `Option<Vec<String>>`, `None`
  meaning the defaults, so a player who never edits them follows later
  default lists (append-only); `rl-ui` (layer 5) cannot read the defaults
  without a same-layer pair, so the app passes them through `MenuData`.
  **R10-3:** PF12-8. **R10-4:** the address and invite are for Public and
  Invite games only, and the invite is shown to the host only (a client
  may not know the host's key). **R10-5:** the lobby's listing lines:
  "Finding the public address…", "Not listed: no public address (forward
  port N or set it in Options)", "Not listed: no relays (set them in
  Options)", "Not listed: no relay reachable", "Announcing on the
  Internet…", "Listed on the Internet" (once a relay's `OK` accepted a
  note), "Not listed: your public address setting is not reachable from the
  Internet" (a configured address that is IPv6 or not global; such a game
  publishes no note on public relays, so its LAN address never leaves,
  final review B-M2). "No relay reachable" is checked before "Listed", and
  "Listed" counts only this hosted game's notes (B-M1). An Invite game's
  line is about its code (B-I3): "Finding the public address…", then
  "This code has no address: forward port N or set the public address in
  Options" when it carries none. **Amended by plan 12b (Task 9, N46,
  N47):** "Finding the public address…" became "Checking how players can
  reach you…"; "Not listed: no public address …" is gone (a Public game is
  listed with or without an address); an Invite game's code without an
  address says "This code joins through the relays" (or "This code has no
  address and no relays: it joins nothing (set relays in Options)"); the
  current lines are under "Through routers decisions". **R10-6:**
  Options → Online has a "Public address" field (checked as an IP) next to
  Region, Relays, STUN servers and Blocked hosts; a relay or STUN entry
  that cannot be added says why ("needs wss:// or ws://", "needs
  host:port", "already listed", "the list is full", at most 32). **R10-7:**
  the relay pool lingers 30 s after the Multiplayer and Host screens (and a
  Public host) stop needing it, so going back and forth does not reconnect,
  and a Public host's closing note leaves on the open pool. Relay edits
  apply at once (final review B-I2): an open pool, lingering or in use,
  whose relays differ from the settings is closed and one on the new list
  opened; a Public host stops announcing on removed relays, sends its last
  note to the new pool at once, and its invite hints the new relays.
  Blocks and unblocks apply to the open list at once (an unblocked host's
  game shows with its next note: the list never kept a blocked host's
  notes). STUN and public-address edits apply the next time a game is
  hosted (Options cannot be opened from the lobby; from a paused match
  they wait for the next hosting). **R10-8:** a
  game is pinged on its first selection, not on every one. **R10-9:** the
  Join button on the Internet tab is labelled "Join"; the Direct tab's
  section is "Join by address or invite". **R10-10:** the pool is dropped on
  a thread (its drop can wait 2 s). **R10-11:** `hosting` (the address
  watch, hints, invite, `Publisher`) and `ping` moved into `rl-discovery`,
  shared by `rl-server` and `rl-game`; `rl-game/src/finding.rs` holds the
  pool and the list. **R10-12:** an IPv6 address in a listed game still
  shows as joinable but `start_client` refuses it ("IPv6 is not supported
  yet"), as before. If wrong: a hosted game appears for a player who cannot
  join it.

Final review rulings (the plan's final fix pass, 2026-10-14):

- **FR-1 (B-I1):** the password proof is both bound (to the joiner's key
  and both DTLS fingerprints) and moved into `Prove`, after the host's
  proof. Binding alone stops the relay; moving alone leaves the relay open
  when no key is expected (LAN, direct); together an impostor gets no
  proof to replay, and one that names the expected key gets none at all to
  guess from. If wrong: one extra field in `Prove` (protocol 2 is
  unreleased).
- **FR-2 (A-M1):** a full list (500 games) still refuses new keys rather
  than evicting incompatible or address-less entries: a flood of valid
  notes from fresh keys defeats any eviction order, and the spec's defence
  is signatures, per-key limits, blocking and sinking. The REQ keeps its
  `limit` of 500 (R4-1): without it a relay holding a flood would burst
  past the rate buckets and be dropped and reconnected for ever. If wrong:
  honest hosts stay hidden while a flood fills the list (proof of work,
  NIP-13, is a later option).
- **FR-3 (A-M4):** the full-list check now comes before the signature
  check, but `offer` still re-verifies events the pool's driver already
  verified: a hostile relay's burst costs about 0.1 s of Schnorr work on
  the UI thread once, bounded by the pool's rate limits, and `GameList`
  keeps verifying for callers without a pool. If wrong: a UI hitch under a
  hostile relay (an `offer_verified` would remove it).
- **FR-4 (B-M6):** the Internet tab's view is rebuilt only when a note
  changed the list, but the list is still cloned and laid out in full each
  frame (up to 500 rows in a `Grid`): `ScrollArea::show_rows` needs fixed
  column widths, a layout change of its own. If wrong: frames slow under a
  full hostile list.
- **FR-5 (B-I2):** unblocking cannot show a game before its next note (up
  to a minute): the list never keeps a blocked host's notes, and the
  pool's dedupe drops a stored note it already delivered. If wrong: an
  unblocked game shows up to a minute late.

## Through routers decisions (plan 12b)

Decided in `docs/superpowers/plans/2026-10-14-plan-12b-through-routers.md`
("Decisions this plan makes", N44-N62, PL12b-1 to PL12b-3) and by rulings
during its execution: PL12b-0, PF12b-1 to PF12b-5 (pre-flight), TR-1 to
TR-4 (the TURN relay), PM-1 and PM-2 (port mapping), NT-1 and NT-2 (tests
and exit), RH-1 (re-hosting) and UI-1 (Options), and the final review's
FR12b-1 to FR12b-4, TR-5 (amends TR-1) and N47-a (amends N47). Where a
ruling changed a decision, the decision below says so and gives its final
form. "If wrong"
is the cost of the decision being wrong.

- **N44 - handshake encryption.** NIP-44 v2 (`rl_discovery::nip44`):
  secp256k1 ECDH x-coordinate, HKDF-SHA256 extract with salt `nip44-v2`,
  a per-message HKDF expand to 76 bytes, ChaCha20 (counter 0) and
  HMAC-SHA256 over nonce and ciphertext, checked in constant time. It is
  checked against the official vectors, fetched once from
  `paulmillr/nip44` (`nip44.vectors.json`, last changed in 1f8dba17,
  2023-12-19). The committed file
  `crates/rl-discovery/tests/fixtures/nip44.vectors.json` has SHA-256
  `269ed0f69e4c192512cc779e78c555090cebc7c785b609e338a62afc3ce25040`, the
  checksum NIP-44's "Tests" section gives (nostr-protocol/nips `44.md` at
  733a0471). The joiner signs and encrypts with a fresh random key per
  join (`Asker::new` draws again on an invalid secret; after 16 invalid
  draws in a row it has no key and sends nothing), so relays cannot link
  joins to players; the lobby's proofs (N37) still prove the player's
  real key over the data channel. The host answers with its own key.
  Only NIP-44's 2-byte length prefix is read: its newer 6-byte prefix (for
  plaintexts of 64 KiB and more) decrypts as an error, and our content is
  at most 16 KiB. If wrong: a handshake that other Nostr clients cannot
  read, which nothing needs.
- **N45 - handshake wire.** Kind 21470, tags `["p", <recipient hex>]` and
  `["expiration", <now + 60>]`; content NIP-44 of `{"v":1,
  "session":"<32 hex>","signal":<rl_proto::Signal>,"mapped":"<ip:port>"|
  null}`, at most `MAX_HANDSHAKE_CONTENT` (16 KiB). An offer or answer
  that would not fit is never sent: the join expires, and the host
  answers `Refused(Malformed)` instead. `mapped` is always `null` from our
  joiners (only a host holds a mapping, N47).
  - **Joiner** (`Asker`): it subscribes for the answer first and sends its
    offer after that subscription's first `EOSE` (at most 2 s), then every
    `RESEND` (2 s), each time to every relay; `NO_REPLY` after
    `ANSWER_DEADLINE` (10 s). No trickle ICE: it gathers for at most
    `GATHER_DEADLINE` (2 s) first.
  - **Host** (`Inbox`): cheap checks first (size, kind, `p`, a date within
    `MAX_SKEW`, 60 s, either way). For a key with a live session, an
    event id already seen is ignored without a signature check, and a
    session takes at most 5 signed resends (counted after the signature,
    so forgeries in a joiner's name spend none); a repeat gets the cached
    reply. For a new key: the signature, ECDH and decryption, the `Offer`
    and its SDP size, and only then a rate token (`OFFER_RATE` 10 a
    second, `OFFER_BURST` 20). One session per key, kept
    `SESSION_MEMORY` (2 minutes) from its offer or reply, at most
    `MAX_SESSIONS` (4096; the oldest goes). The inbox runs on rl-online's
    listener thread, never the game's UI thread.
  - **Token after decryption** (Task 7 review, Important 1): the plan took
    the token before decryption, so a flood of signed but undecryptable
    events from fresh keys starved honest joiners. Now junk spends no
    token; its cost (a signature check, an ECDH, a ChaCha20 and an HMAC
    per event) is bounded by the pool's per-relay limits (200 frames a
    second) and stays on the listener thread. **Residual:** valid offers
    from fresh keys still share the 10 a second budget; PF12b-2 and the
    joiner's resends soften it.
  - **Over-limit input is dropped, not the sender** (pre-flight fix 29):
    an over-limit or hostile event is dropped and counted
    (`Inbox::ignored`). Its sender cannot be dropped, since its key is
    ephemeral. This deviates from spec §5.4's "a peer breaking a limit is
    dropped".
  - If wrong: up to 2 s more per Nostr join (no trickle); a flood of valid
    offers from fresh keys delays honest joiners.
- **PF12b-2 - a full pending table evicts an unproven setup** (as amended
  by FR12b-1). When the signalled-pending table (`MAX_SIGNALLED_PENDING`,
  8) is full and a new valid offer arrives, the oldest unprotected
  pending signalled setup is closed (`SetupFailed { Timeout }`, the host's
  `JoinFailed`), and the new offer takes its place. Only when every
  pending setup is protected is the offer refused `Busy`, which a joiner
  shows as `BUSY` ("the game is busy with other joins: try again in a
  moment"). **FR12b-1** (final review A-I1): PF12b-2 first protected a
  setup once its peer sent one ICE check that str0m accepted, but a
  flooder holds its own answer's ufrag and password and can send valid
  checks from any forged source, so 8 offers with one check each per 15 s
  locked honest joiners out. A setup is now protected only once one of
  the host's own checks to it was answered from the address it went to
  (its transaction and destination remembered, the binding success
  response matched), which a forged source cannot do; and only the oldest
  `MAX_ANSWERED_PER_IP` (2) answered setups of one IP are protected, so
  one real IP holds at most two slots. Honest joiners answer the host's
  checks within about 1 s of the answer. Pinned by `a_flood_of_offers_
  leaves_room_for_an_honest_joiner` (an 8 s flood of 100 junk events and
  5 valid offers a second, each sending valid checks from a forged source
  once answered; the honest joiner, starting 3 s in with the table full,
  connects during it; RED before FR12b-1: refused `Busy`) and
  `answered_checks_protect_at_most_two_setups_per_ip`. **Residual:** a
  flooder with many real IPs (each answering the host's checks) still
  fills the table, 2 slots per IP; with a real IP it could as well
  complete the link, which is the lobby's problem (the lobby's limits
  apply). If wrong: an honest joiner slow to answer the host's checks is
  evicted during a flood and retries.
- **N46 - join order** (as amended by PF12b-3).
  - A typed address or a LAN game: direct only (plan 10). A typed name is
    looked up on a thread (`rl-game resolve`), never on the frame.
  - A listed game or an invite with an address: direct first, then
    through Nostr if the link never opened (a `Disconnected` before
    `Connected`, not a `Refused`).
  - Without an address: Nostr only. A listed game without one is joinable
    (the Internet tab's ping column says "via relays").
  - The joiner's relays (`join_relays`, PF12b-3): the invite's hints
    first, then its own list, each once, at most `MAX_JOIN_RELAYS` (8).
    The host listens on all of its relays, so any relay that carried its
    note reaches it; notes carry no handshake hints (PL12b-2).
  - With neither an address nor relays: `NO_WAY_TO_JOIN` ("this game has
    no address, and there are no relays to join it through …").
  - Failure texts (`rl_online::join`): `NO_RELAYS` (every relay failed
    before an answer), `RELAYS_REFUSED` (every reachable relay refused the
    join's events by a NIP-01 policy prefix such as `blocked:` or
    `restricted:`; `rate-limited:`, `error:` and `duplicate:` do not
    count; added so a web-of-trust relay's refusal is not blamed on the
    host), `NO_REPLY`, `BUSY`, and after the host's answer `no_path(port,
    relay)` ("no network path to the game: hole punching failed and no
    TURN relay could be used …", or "… not even through the TURN relay
    …"), after `SIGNALLED_SETUP_TIMEOUT` (15 s).
  - If wrong: a stale address costs at most `CONNECT_TIMEOUT` (5 s)
    before Nostr is tried.
- **N47 - what an address in a note or invite means.** It is present only
  when the host is directly reachable: `Reachability::direct()` is a
  usable router mapping that forwards TCP and UDP on one external port
  (F-G) with a global external IP, else the configured `public_address`
  on the game's port (the mapping is taken first). The port is then the
  external one (`Hosting::public_port`); the LAN announcement keeps the
  local port (`Hosting::lan`). A STUN-learned address is only an ICE
  candidate, never advertised: this replaces N40's use of STUN, and
  `AddressWatch` is gone (Task 9). The first note and the invite wait for
  `Reachability::settled()` (the router answered or gave up, and the
  first STUN round settled), at most `SETTLE_WAIT` (5 s), in rl-server
  and in the game; the invite is printed again whenever `direct()`
  changes. A Public game without an address is announced all the same
  and joined through the relays. **N47-a** (final review C-I1): an
  address is also advertised when STUN shows no NAT (a VPS or cloud VM:
  the server-reflexive IP is one of the host's own addresses,
  `Reach::open`, usable when global or `allow_local`), on the local port,
  after the mapping and the configured address; the reach line then says
  "No router: players reach you directly at …" instead of the router's
  failure. Before, an upgraded VPS lost its listed and invite address and
  its log blamed a router it does not have. The CHANGELOG and online.md
  name VPS servers. **FR12b-2** (final review A-I2): every renewal reads
  the router's external address again (PCP from its answer, NAT-PMP op 0,
  UPnP `GetExternalIPAddress`); a changed one is a new `Mapped` state, so
  `direct()`, the invite, the notes and a relay on the mapping's address
  (restarted on it) follow an ISP's renumbering. If wrong: a manual
  forward without `public_address` joins through Nostr and ICE, which
  needs hole punching or a TURN relay; a host behind a firewall that drops
  inbound traffic but has no NAT advertises an address that times out
  after 5 s, then Nostr.
- **N48 - ICE candidates.**
  - **Host:** the default interface's IPv4 addresses, at most 2
    (`netdev =0.46.3` with feature `gateway`; else the OS's source address
    toward the first STUN server; else 127.0.0.1). A signalled setup made
    before that lookup ended takes `local::quick_base` at once (the
    source address toward 192.0.2.1 by a `connect`, nothing sent), so no
    SDP goes out without candidates. Every candidate shares the one UDP
    socket; datagrams reach str0m as arriving at the first (the base).
  - **Server-reflexive:** STUN binding requests from the game socket to
    the first 4 configured servers that resolve to IPv4 (the plan said the
    first two that answer), at the start and every `STUN_EVERY` (25 s,
    which keeps the router's mapping alive), each retried once after
    500 ms. Two answers with different ports mean a symmetric NAT;
    an answer equal to the base means no NAT (`NatBehaviour`).
  - **Mapped:** a server-reflexive candidate at the mapping's external
    address.
  - **Relayed:** the joiner only, at most 2 TURN allocations (IPv4). The
    host never allocates: a relay's per-user limit (32 KiB/s) fits one
    client's traffic (spec §3.1: at most 24 KB/s), not a host's. An
    allocation no open link uses is released 5 s after a link is
    `Connected` on another path (`RELEASE_AFTER`). A relay that names our
    own address, or a non-global one (without `allow_local`), as its
    relayed address is refused and released (Task 3 review), so direct
    traffic is never wrapped.
  - **Budget** (Task 2 review, I-1): an SDP keeps at most `MAX_CANDIDATES`
    (8), so server-reflexive, mapped and relayed candidates come first and
    host candidates fill what is left, base first.
  - If wrong: a host behind a symmetric NAT whose joiner reaches no TURN
    relay fails; friends on a shared VPN connect over public paths (other
    interfaces are not offered).
- **N49 - path names.** `rl_proto::Path { Direct, MappedPort,
  HolePunched, Relayed }`, from str0m's selected pair
  (`set_stats_interval(1 s)`), classified by `candidates::classify`:
  Relayed when either side's selected address is a TURN relay; MappedPort
  when the remote's address is its mapped hint; Direct for a remote host
  candidate; HolePunched otherwise. A signalled link's `Connected` waits
  for the first stats naming a pair, at most 1.5 s after the link opened
  (then HolePunched, with the source of the last datagram the link took as
  the address, never 0.0.0.0: Task 2 review). A TCP-signalled link is
  `Connected` at once: Direct for the joiner; for the host MappedPort when
  it holds a mapping that forwards TCP and the joiner's address is global
  (or `allow_local`), else Direct (Task 5 review, I1: no task held this
  part of N49 until then). A client's path follows ICE afterwards
  (`OnlinePeer::connection`). Tags (`path_tag`): "direct", "open port",
  "punched", "relayed"; the client's line (`path_text`): "connected
  directly to …", "connected through the host's open port …", "connected
  by hole punching through both routers", "connected through the TURN
  relay …". If wrong: the host may label a mapped-port ICE join
  "hole-punched".
- **N50 - signalled offers' candidates.** With no TCP connection, T4-A
  (keep only the TCP peer's IP) cannot apply. A signalled offer keeps at
  most 8 `a=candidate` lines that are UDP, IPv4, unicast, not unspecified,
  multicast or broadcast, with a port and a known type
  (`candidates::sanitize`, fuzzed). The joiner sanitizes the host's answer
  the same way (Task 2: a hostile host could aim checks at a third party
  too). The host's checks are ICE-paced and bounded by
  `MAX_SIGNALLED_PENDING`. If wrong: a hostile joiner can aim a few STUN
  checks at a third party.
- **N51 - port mapping** (as amended by PM-1, PM-2, RH-1 and NT-2).
  - **Protocols:** NAT-PMP (RFC 6886), PCP (RFC 6887) and UPnP-IGD, all
    hand-written in `rl_net::portmap` (PM-1 replaced igd-next; see below).
    PCP, falling back to NAT-PMP after any PCP failure (refusal,
    `UNSUPP_VERSION` or silence), on port 5351 of the default gateway, on
    one thread, races UPnP (SSDP, then HTTP) on another; each has its own
    deadline (PCP and NAT-PMP 3 s, UPnP 4 s) and the first mapping wins
    (the loser deletes what it mapped). A router offering both therefore
    shows PCP.
  - **Ports:** the game's port, TCP and UDP on the same external port
    (F-G): the local port first, then +1 … +7. A relay's ports are exact
    (`MapRequest::exact`/`extra`): never shifted; one that cannot be
    mapped exactly is reported.
  - **Lifetimes:** PCP and NAT-PMP 2 h, UPnP 3600 s, renewed at half; a
    router answering 725 (`OnlyPermanentLeasesSupported`) gets lease 0
    (not renewed). A renewal that gets no answer is retried halfway to
    the mapping's end; a refused one drops the mapping and says so.
  - **Exit:** mappings are deleted within `DROP_WAIT` (1 s); see NT-2 for
    who waits.
  - **Useless mapping:** an external IP that is not global
    (`rl_proto::is_global`), without `allow_local`, means a double NAT or
    carrier-grade NAT: reported (`MappingState::Mapped { usable: false }`)
    and never advertised.
  - **State:** a mapping host is `Trying` from the endpoint's start (Task
    11 review: it showed "Port mapping is off" for a moment first), and
    `Failed` when the mapper can never start.
  - **Tests never reach a router:** with `test-hooks`, `PortMapper::start`
    refuses a gateway or SSDP address that is not loopback, before any
    socket exists.
  - If wrong: a router quirk our client misses leaves hole punching and
    relays.
- **N52 - TURN relays** (as amended by TR-1 to TR-4, PF12b-4 and the Task 8
  session cap).
  - **Code:** the client is `turn-client-proto =0.7.2` (sans-IO, inside
    `Core`); the server is our own (`rl_net::turn`, TR-2), not
    turn-server-proto.
  - **Who runs them:** the project runs no TURN server and ships no TURN
    list (public TURN services need paid credentials). Volunteers run
    `rl-server --relay`, which announces a kind-31471 note `{v, addr,
    port, region, capacity, load, user, pass}` with public long-term
    credentials (realm `rust-liero`, fresh each start, 16 hex characters
    each), at once and every 60 s (expiry 180 s), and a closing note
    (capacity 0, expiry 10 s) at shutdown. Players may add their own TURN
    servers (`user:pass@host:port`, Options → Online → TURN relays, at
    most 8; `rl-cli --turn`), for joining. rl-server's config has no TURN
    list (PF12b-4: a host never allocates).
  - **Limits per client IP:** 32 KiB/s relayed each way (a bucket holding
    2 s), 4 allocations, 20 requests a second (split 15 for requests from
    a proven source, 5 for the rest: TR-2's fix rounds); at most 16
    permissions and 16 channels per allocation, at most 8 peers per
    CreatePermission, LIFETIME clamped to 600-3600 s, requests at most
    1500 B, any datagram at most 4096 B, 4096 IPs tracked. Each allocation
    ends `MAX_SESSION` (6 h) after it was made, refreshes included (Task 8,
    the Task 3 review: 8 IPs × 4 allocations could hold a default relay
    forever; a relayed player still connected then loses the link and
    joins again). Over a limit, the datagram or request is dropped and
    counted, not the client: a relay has no session to drop, a deviation
    from spec §5.4's "a peer breaking a limit is dropped".
  - **Capacity and ports:** 32 allocations by default on relay ports
    49160-49191, UDP listening port 3478. With port mapping, one
    `PortMapper` maps the listening port and each relay port exactly
    (PM-2). The relay starts only once the router answered for every port
    and its public IPv4 is known: `public_address`, else the listening
    port's usable exact mapping's external IP, else the server-reflexive
    IP when the NAT is `Open`; otherwise it logs "relay: not announced: no
    public address …" and never starts (its random credentials would serve
    no one). When the router maps, only the relay ports it mapped exactly
    are handed out (Task 8 review, I1), and the announced capacity is at
    most their count; with no mapping (a manual forward) the whole range
    is.
  - **Choice ("the nearest few"):** the joiner's own servers first (at
    most 2), then announced relays (`TurnList::pick`): the joiner's region
    first, then the least loaded (load/capacity compared exactly), never a
    full one; at most 2 allocations in all. The joiner collects notes
    until 1 s after the first `EOSE`, at most 4 s. No latency probe.
  - If wrong: until volunteers run relays, pairs that hole punching cannot
    join (a symmetric NAT against anything but an open port) cannot
    connect; content downloads through a relay run at about 32 KiB/s; a
    match on one relay ends after 6 h.
- **N53 - kind 31471** (addressable, `d` and `t` = `rust-liero-turn`,
  `v` checked) for TURN relay notes. `TurnList` validates like the game
  list: addresses global unless `allow_local`, never unspecified,
  multicast or broadcast; credentials 1-64 printable ASCII; capacity at
  most 4096; one note per key per 10 s; at most 64 relays; a closing note
  is exempt from the gap and leaves a tombstone, which outlives a flood
  of closing notes (never-listed keys are evicted first). The kinds
  check is below ("Measured"). If wrong: a kind change before release.
- **N54 - the host listens on Nostr.** Public and Invite games and
  servers listen on their relays with a pool of their own, on a thread
  (`rl-online listen`) that owns the pool and the `Inbox`; LAN games and
  Lan servers do not. Offers wait unbounded for the peer's poll (the
  inbox's rate bounds them) and one older than `OFFER_STALE` (8 s) is
  skipped. Relay edits in the game's Options move the listener at once
  (`OnlinePeer::listen_on`, Task 9). If wrong: a public host keeps two
  connections per relay (the announcing pool and the listening pool).
- **N55 - same-layer pair** `("rl-online", "rl-discovery")`: rl-online
  drives the handshake pool, so rl-game, rl-server and rl-cli share one
  join and one listener. If wrong: a pair to unwind.
- **N56 - `Endpoint` splits into a sans-IO `Core` and its socket driver**,
  and `natsim` (feature `test-hooks`) runs `Core`s behind virtual NATs on
  a virtual clock in `check`. NAT models in RFC 4787 terms: full cone,
  address-restricted, port-restricted, symmetric; a 120 s idle mapping
  lifetime, no hairpinning, 10 ms per hop, seeded port choice, and each
  node's STUN and impairment seeds drawn from the network's. str0m still
  draws ICE credentials, DTLS certificates and SCTP state from the OS, so
  the bytes differ from run to run but the outcomes do not (the Task 2
  review's reasoning: delivery does not depend on content). If wrong:
  test infrastructure to maintain.
- **N57 - the STUN binding codec and `is_global` move to `rl-proto`**
  (`rl_proto::stun`, `rl_proto::is_global`, `core::net`); rl-discovery
  re-exports them. If wrong: none beyond a wider rl-proto.
- **N58 - the Docker NAT tests** (`cargo xtask net-nat`, in `full`; as
  carried out in Task 11).
  - **Image:** `FROM <ID>:<VERSION_ID>` of the host's `/etc/os-release`
    (Debian-like only: the packages come from apt), plus `iproute2`,
    `nftables` and `miniupnpd-nftables`. Here `ubuntu:26.04`, 196 MB.
  - **Networks:** internal macvlan networks (`docker network create -d
    macvlan --internal`), not bridges: with `br_netfilter` on, Docker's
    rules drop every bridged packet an internal bridge's router forwards.
    WAN 203.0.113.0/24.
  - **Routers:** A port-restricted cone (`masquerade`), B symmetric
    (`masquerade fully-random`), C UPnP-IGD only, D NAT-PMP and PCP, E
    NAT-PMP only (PCP requests dropped by their version byte), each
    dropping unsolicited WAN input as home routers do (without it, an
    early ICE check confirmed a conntrack entry and broke hole punching
    between two cone routers). One method per router so both UPnP and PCP
    are exercised (D alone would always show PCP). miniupnpd needs
    `ext_ip` for a 203.0.113.x WAN.
  - **Binaries:** `e2e`'s dev builds of `rl-server` and `rl-cli
    --features nat-tools` (libc only, so they run in the image), mounted
    read-only. Our processes run with `allow_local` (N62).
  - **Scenarios** (9): `direct`, `upnp`, `pcp`, `nat-pmp`,
    `hole-punching`, `relay`, `no-relay`, `finding`, `nat-check`; each
    path is asserted at both ends (the joiner's `path:` line, the host's
    `via` tag). About 85-100 s warm.
  - **Skips** (`skip net-nat (<why>)`): no Docker, a non-Debian
    `/etc/os-release`, no content, a Docker that cannot make macvlan
    networks (the daemon's own message), a base image that cannot be
    pulled, apt offline or lacking a package, `--offline` without the
    image. A broken Dockerfile or script fails.
  - **Cleanup:** every container ends by itself (`RL_NAT_LIFE`, 120 s);
    Ctrl+C removes containers and networks (xtask's `ctrlc` handler, only
    during a run); names carry xtask's process id; a machine-wide lock
    serialises runs.
  - If wrong: a host whose distribution lacks an official image, or a
    rootless Docker, skips the step.
- **N59 - a relayed member's IP ban is a key ban.** Its address is the
  relay's: banning it would ban the relay for everyone.
  `LobbyHost::ban_with_address(member, by: Option<u8>)` tells the admin
  who asked (`by`; `None`: the host's own, or a server command) "reached
  through a TURN relay: banned by key only". rl-server's `/banip` keeps no
  IP for such a member, so the bans file never holds a relay's address
  across a restart. If wrong: a relayed troublemaker returns with a new
  key (bans are soft anyway).
- **N60 - `rl-server --nat-check`.** Reports, in order: the base
  addresses and gateway; each configured STUN server's answer and `NAT:
  <nat_text>` ("no NAT", "a NAT that keeps ports (hole punching works)",
  "symmetric NAT", "not known yet"); the router (a random UDP port of
  49152-65535 mapped exactly, its result, then "router: the mapping is
  deleted" only when the router confirmed it); each Nostr relay's status
  with its clock against this PC's (FR12b-3) and a warning past a minute,
  whether each forwards a 16 KiB handshake from a new key (final review
  B-M3), and the TURN notes on them; `nat-check done`. Exit 0. Without
  `--config` it uses the defaults (8 relays, 5 STUN servers). It ships in
  the release zips (it is `rl-server`); it uses public servers only when
  run by hand. If wrong: a report that misleads a playtester; it changes
  nothing.
- **N61 - protocol stays 2:** `Member` gains `path: Option<Path>`
  (appended; `None` for the host's own player); `Signal` is unchanged.
- **N62 - `allow_local`.** rl-server's config `allow_local: bool`, `rl-cli
  --allow-local`, `TurnServerConfig::allow_local` and
  `CoreConfig::allow_local`: lists, TURN notes, mappings, a relay's peers
  and relayed addresses, and the server's own advertised address accept
  non-global addresses (PF12-5's switch, extended to everything that
  judges an address). For office and LAN setups and the Docker tests. The
  game keeps PF12-8's automatic rule. If wrong: a misconfigured public
  server steers players at private addresses.

Plan rulings:

- **PL12b-0:** the plan's decisions N44-N62 were accepted without asking
  the user: each lies within the spec (U9 volunteer TURN relays, §5.3's
  order, §1.2's no central server). If wrong: one decision reworked,
  visible here.
- **PL12b-1:** 12 tasks; the `rl-net` refactor (Task 1) came first, so
  every later transport feature landed in sans-IO code with natsim
  tests. If wrong: none now.
- **PL12b-2:** relay URLs are not added to announcements. Any relay that
  carried a note reaches its host (N46). This deviates from spec §6.1's
  "handshake hints" (the spec points here). If wrong: a joiner whose
  relays share none with the host's fails with `NO_REPLY`; an invite's
  hints cover the usual case.
- **PL12b-3:** spec §9.7's "a browser host" scenario moves to plan 13 (no
  browser build yet). If wrong: none.

Pre-flight rulings:

- **PF12b-1:** the pre-flight scan's 29 fixes were applied to the plan
  before Task 1, among them: `is_global` moved to rl-proto; tests use
  documentation externals only with `allow_local`; every test host sets
  `port_mapping: false` or aims at a `FakeGateway`; relay ports are never
  shifted; `relay_only` adds only relayed candidates; the signalled setup
  timeout lives in `Core`; `TurnList::pick(region, 2 - own)` saturates;
  the probe sends a 16 KiB event; nat-tools' test runs in `check`. If
  wrong: plan rework only.
- **PF12b-2:** see above, after N45.
- **PF12b-3:** invite hints first, then the player's own relays, cut to 8,
  everywhere (an invite's hints are the relays the host is known to use).
  If wrong: one of the player's own relays dropped from a join.
- **PF12b-4:** no TURN servers in rl-server's config (the host never
  allocates, N48). If wrong: a later config field.
- **PF12b-5:** no test maps a real router port or sends SSDP to the
  multicast group: every test host sets `port_mapping: false` or targets
  a `FakeGateway` through `test-hooks`; TURN tests use ephemeral ports and
  generous limits. If wrong: none.

Task rulings:

- **TR-1** (as amended by TR-5): a TURN relay refuses peers that are not
  global (`rl_proto::is_global`: loopback, private, link-local, multicast,
  unspecified, documentation, CGNAT …; 403, IPv6 peers 443), unless
  `allow_local` (N62): a public relay with public credentials must never
  reach its operator's network. **TR-5** (final review B-I3): TR-1 first
  refused the relay's own public IP too, so a relay could never carry a
  game hosted behind its own router (the `rl-server --relay` beside its
  own game, a player hosting on its LAN), and a two-PC playtest's only
  relay was likely the user's own. The relay now refuses only its own
  sockets: its listening port and its allocations' relay ports, at its
  public or listening IP, a loopback address or any of the machine's
  interface addresses (`TurnCore::own`; a multi-homed relay, final
  review A-M3), even with `allow_local` (a loop). online.md says a relay
  beside the host needs a router with hairpin NAT, else run it elsewhere.
  If wrong: an office relay needs `allow_local`; a relay can reach
  services forwarded on its own router (already public).
- **TR-2:** our own UDP-only TURN server core replaces turn-server-proto:
  the RFC 8656 subset our client and browsers use (Allocate, Refresh,
  CreatePermission, ChannelBind, Send and Data indications, ChannelData,
  Binding), long-term credentials (HMAC-SHA1 under MD5(user:realm:pass);
  no SHA-256), FINGERPRINT, every table bounded. The library kept a nonce
  per source address forever, grew its permission tables without bound,
  relayed to any peer and addressed binding answers to itself. The nonce
  is stateless: 16 hex digits, a wrapping u16 10-minute period and a
  48-bit HMAC tag of the source IP, port and period under a per-relay
  random key, valid in its period and the next; a tag of ours for the
  exact source up to 8 periods old still proves that source (it gets a
  fresh nonce, 438, outside the unproven budgets), so a spoofed flood
  cannot starve a live client's renewals. Requests take a token before
  any integrity check: a proven source from its IP's larger bucket,
  anything else from the smaller. If wrong: about 1000 lines of
  security-sensitive code to maintain; turn-client-proto and the interop
  tests pin compliance.
- **TR-3:** the relay listens on IPv4 only (an IPv6 `listen` or `public`
  is refused), per N48's IPv4-only candidates. If wrong: no IPv6
  relaying.
- **TR-4:** reflection is accepted at 1.6× (a 20 B Binding gets 32 B;
  turn-client-proto's first 40 B Allocate a 64 B 401) and 2.3× (a bare
  28 B Allocate), under a relay-wide cap of 64 KiB/s (a bucket of 2 s)
  on every answer to an unproven source. Only an unsigned Allocate with
  REQUESTED-TRANSPORT gets a 401 (64 B: no reason text, no FINGERPRINT);
  other unsigned requests get nothing. An RFC 401 cannot be smaller than
  about 52 B, and refusing our client's first Allocate breaks TURN;
  public STUN servers reflect alike. If wrong: a relay reflects at most
  64 KiB/s at a victim set.
- **PM-1:** our own UPnP-IGD client replaces igd-next: SSDP M-SEARCH with
  `MX: 1`, three search targets and resends within a 2 s window, bad
  replies skipped, a `LOCATION` host that must equal the sender and be
  non-global, at most 4 devices (the default gateway's first); a
  proxy-free HTTP/1.1 client (no DNS, no redirects, heads at most 8 KiB,
  bodies 64 KiB, `Content-Length`, chunked or close framing, deadlines
  between every read); a flat, linear scan of the description for
  WANIPConnection:1/2 or WANPPPConnection:1 `controlURL`s; SOAP
  `GetExternalIPAddress`, `AddPortMapping` (description `rust-liero`)
  and `DeletePortMapping` in the namespace found. igd-next parsed a LAN
  device's XML recursively (a 35 KB nested description overflowed the
  stack: an uncatchable abort), read bodies without a cap, followed
  `HTTP_PROXY`/`ALL_PROXY` and could panic on a `Duration` underflow. A
  LAN device must never crash or exhaust the game. If wrong: about 450
  lines to maintain, and router quirks igd-next handled must be covered
  by tests (chunked bodies, PPP and URLBase descriptions are).
- **PM-2:** one `PortMapper` request carries a list of ports
  (`MapRequest::extra`): one discovery and one session per protocol map
  the relay's 33 ports, each exact, renewed and deleted on its own
  (`PortMapper::extras`), instead of 33 mappers and 66 threads. If wrong:
  a port mapping API change.
- **NT-1:** tests send only to 127.0.0.0/8. A `FakeGateway` whose mapping
  a client uses reports external 127.0.0.2 with `allow_local` on both
  sides; documentation addresses only go to code that never sends to
  them. The planned TCP connect to 192.0.2.1:9 (a router that does not
  hairpin) became rl-net's `connect_stall` test hook: the connect sends
  nothing and fails after `CONNECT_TIMEOUT`. If wrong: none.
- **NT-2:** `Endpoint::shutdown(self) -> Shutdown` never blocks: it closes
  every link and hands the mapper to a thread that deletes the mappings
  and waits for the router's confirmation (`PortMapper::close`);
  `Shutdown::wait(until)` (at most 1 s) says whether the router
  confirmed, `Shutdown::poll` the same without waiting.
  `OnlinePeer::shutdown` wraps it. rl-server waits at exit (and warns
  when the router did not confirm), rl-cli's bot client waits, and the
  game waits at most `UNMAP_WAIT` (1 s) after its window closed, off the
  frame; the game also prunes the shutdowns of games it left. Dropping
  the endpoint stays the fallback (nobody waits). If wrong: up to 1 s
  longer exit.
- **RH-1:** a new hosting never maps a port while an earlier shutdown of
  that port is still deleting: a process-wide set of ports being deleted
  (`begin_delete`, on the caller's thread before the mapper's thread
  starts) makes a new `PortMapper`'s threads wait for it, at most
  `DROP_WAIT`, on their own threads. Re-hosting at once on the same port
  is a normal playtest action. If wrong: up to 1 s before a re-host's
  mapping.
- **UI-1:** the Options and Multiplayer tab rows fit at every offered
  window size and UI scale (`screens::tab_row`: equal shares in a narrow
  column), and the Online tab fits down to 320 points (borderless 4:3 at
  UI scale 3): its form labels take at most 40 % of a narrow column, its
  fields shrink, its server rows wrap, and the port mapping switch sits
  on its own line. A tab that cannot be reached is a bug. If wrong: a
  layout change.

Final review rulings (2026-10-14; the fix pass):

- **FR12b-1** (A-I1): see PF12b-2 above.
- **FR12b-2** (A-I2): see N47 above.
- **FR12b-3** (B-I1): the host's side reports joins through the relays.
  Its lobby log and the server's log (`relays: …`) say each outcome:
  answered, refused (busy, or the reason), answered but not connected
  within 15 s (rl-net's new `NetEvent::JoinFailed`, for a host's
  signalled setups only), and handshakes dated more than a minute off the
  host's clock ("… check both PCs' clocks"; the host's subscription now
  reaches 10 minutes back, `CLOCK_SEEN`, so a joiner whose clock is
  behind is seen); at most one line of each kind every 5 s. `NO_REPLY`
  names the clocks, and an `invalid:` refusal is `RELAYS_INVALID` (B-M2).
  `rl-server --nat-check` prints each relay's clock against the PC's,
  from its WebSocket handshake's `Date` header (`Pool::clocks`), and
  warns past a minute. Clock skew is the likeliest silent failure at a
  playtest. If wrong: a few more log lines.
- **FR12b-4** (B-I2): a relayed joiner's connection line names the TURN
  relay's address (the server of the selected pair's local, relayed
  candidate, `Endpoint::relay_server`), not the host's; its test's relay
  listens on 127.0.0.3 so it can fail. If wrong: none.
- **TR-5** (B-I3): see TR-1 above.
- **N47-a** (C-I1): see N47 above.

Review findings that changed behaviour beyond the plan:

- The game ran its network frame only while an online game existed
  (since plan 10), so in the real binary the Internet tab and the LAN
  list never filled and a typed host name hung on "Looking up …"; tests
  called the frame directly. One predicate, `Shell::network_live`, now
  gates the app's frame and the tests' (Task 9 review, Critical 1). A
  hidden window wakes every 4 ms only with a live online game, every
  60 ms for the lists, lookups and lingering relays, else never.
- TURN passwords are never printed: `FindingSettings` and
  `TurnServerSpec` have redacting `Debug`s, and warnings never quote a
  TURN entry.
- rl-server's `/banip` on a relayed member wrote the relay's IP to the
  bans file, which after a restart banned the relay for everyone (N59).
- A stalled link no longer floods its peer when it recovers (the bot-grid
  flake of 2026-10-14, f4a1f26): sctp-proto 0.10.5 starts an unreliable
  message's lifetime only at its first send, so every input written during
  a stall left at once after it, past the host's 400 messages a second,
  and an honest peer was dropped. A link now holds at most 100 inputs in
  SCTP and keeps only the newest 4 of its own (`PeerStats::stale_dropped`
  counts the rest); each netcode datagram repeats what is unacknowledged.
- The final review's minors, fixed: the mapping race's loser asks the
  winner to renew at once (A-M1: on a router with one table for every
  protocol its deletion removed the winner's mapping); PCP and NAT-PMP
  keep their whole deadline after an RH-1 wait (A-M2); signalled SDP keeps
  loopback candidates only with `allow_local` (A-M4); a member's path
  follows its link, for its tag and N59's ban (B-M1); `--nat-check` says
  whether each relay forwards a handshake from a new key (B-M3); a
  host's listener says which relay fails (B-M4); a failed listener restart
  no longer holds the old relays (B-M5); a join's relays count a trailing
  `/` as the same relay (B-M6); the host's console says each member's
  path (C-M1); a new TURN relay is masked as it is typed (C-M5).

Measured:

- **Relay probe, 2026-10-14** (`rl-cli nostr-probe`, run once by hand on
  the 8 default relays from WSL2: a fresh key, neutral content, a
  3-minute wait). It adds a kind-31471 note, read back at once and after
  the wait, and a 16 KiB kind-21470 event (`MAX_HANDSHAKE_CONTENT`) to
  plan 12a's probe (N38).

  | relay | connect ms | OK ms | accepts 31470 | 31470 now/later | accepts 31471 | 31471 now/later | forwards 16 KiB 21470 | free | verdict | notes |
  |---|---|---|---|---|---|---|---|---|---|---|
  | wss://relay.damus.io | - | - | - | - | - | - | - | - | FAIL | connect: HTTP 503 Service Unavailable |
  | wss://nos.lol | 131 | 40 | yes | yes/yes | yes | yes/yes | yes | yes | pass | |
  | wss://relay.primal.net | 212 | 50 | yes | yes/yes | yes | yes/yes | yes | yes | pass | |
  | wss://relay.snort.social | 253 | 60 | yes | yes/yes | yes | yes/yes | yes | yes | pass | ephemeral OK: "will not be stored" |
  | wss://offchain.pub | 494 | 151 | yes | yes/yes | yes | yes/yes | no | yes | FAIL | "Policy violated and pubkey is not in our web of trust." |
  | wss://nostr.oxtr.dev | 141 | 50 | yes | yes/yes | yes | yes/yes | yes | yes | pass | |
  | wss://relay.nostr.net | 202 | 50 | yes | yes/yes | yes | yes/yes | yes | yes | pass | |
  | wss://nostr.bitcoiner.social | 585 | 221 | yes | yes/yes | yes | yes/yes | no | yes | FAIL | "Policy violated and pubkey is not in our web of trust." |

  Every relay that connected kept kind 31471 for the 3 minutes; 5 of 8
  forwarded the 16 KiB handshake. offchain.pub and nostr.bitcoiner.social
  refuse events from keys outside their web of trust: every joiner's key
  is fresh (N44), so they will likely never carry a handshake (in plan
  12a both forwarded a small ephemeral event from a fresh key; one run
  cannot tell size, key or a policy change apart). damus answered 503 at
  the time. None was removed: `DEFAULT_RELAYS` is append-only, the host
  listens on all 8, and a refusal is told apart (`RELAYS_REFUSED`). Re-run
  the probe before a release.
- **Kinds registry re-check, 2026-10-14:** registry-of-kinds
  `schema.yaml` at d18576cc (2026-10-02) and the NIPs README kinds table
  at 6631b3eb list none of 31470, 31471 and 21470; in 21000-21999 only
  21000-21003 and 21059 are taken. The 31400-31499 range is not empty, as
  plan 12a's check said (PL12-2): 31436 ("Gopherhole Document") is
  listed. It touches none of our kinds.
- **NIP-44 vectors:** SHA-256 `269ed0f6…3ce25040` (in full under N44).
- **Docker NAT scenarios** (Task 11, warm, on WSL2): direct 4.1 s, upnp
  5.0-5.2 s, pcp 5.2 s, nat-pmp 6.7-6.8 s, hole-punching 6.1-6.3 s, relay
  22.0 s (a match played), no-relay 19.6 s (the `no_path` text), finding
  7.1 s (listed without an address, relayed), nat-check 5.4 s ("NAT:
  symmetric NAT").

Plan 12a decisions amended (by pointer; their text above is kept as
decided): N40's STUN address, N42's "a code without an address cannot be
joined", N43's and R9-2's "STUN for the address", R9-3's invite timing
and R10-5's lobby lines are replaced by N47 and N46 here. The lobby's
lines now: "Checking how players can reach you…" while the reachability
settles; a Public game is listed with or without an address ("Not listed:
no public address (forward port N or set it in Options)" is gone); an
Invite game says nothing when its code has an address, else "This code
joins through the relays", or "This code has no address and no relays:
it joins nothing (set relays in Options)"; below them the reachability's
lines (`Reachability::lines`: the router, the configured address, a
symmetric NAT, the listener).

Open items (deferred minors):

- The TURN relay's listening socket and its relay sockets share one
  1024-entry queue: a listening-port flood that outruns the core can
  drop relayed game data (only near gigabit rates).
- When a signalled setup took the quick base before the DNS thread's
  base came, the later base's other host addresses are not offered.
- Valid offers from fresh keys share the inbox's 10 a second budget
  (N45's residual).
- A relay's ports are deleted one after another at exit: 32 ports on a
  router needing about 100 ms per call can pass `DROP_WAIT` (1 s), and
  the rest expire with their lease (the log says the router did not
  confirm). Extras' renewal and many-port deletion are untested.
- If a relay's listening-port mapping fails, its extras are not asked,
  and with `public_address` set the whole relay range is handed out (the
  manual-forward case). The relay port set is fixed at start: an extra
  whose renewal later fails is still handed out.
- rl-cli's NT-2 exit wait is untested (a client maps no port).
- A mapping whose answer was lost (silence, garbage) is not deleted; it
  lasts its lease (2 h at most) and points at our own port only.
- The Video tab's rows overflow at 384 and 320 points (UI scale 3,
  borderless 16:10 and 4:3), older than this plan.
- The benchmark gate (side task, d75269a): a regression of exactly about
  15 % can pass when one window measures under +10 %, and Windows-side
  load is invisible to `/proc/stat`.
- From f4a1f26 (the stalled-link fix): sctp-proto's unreliable lifetime
  starting at the first send is worth reporting upstream; the `awful`
  profile still drops some inputs (by design now: stale ones); the
  bot-grid's p99 budgets can fail under heavy machine load.
- FR12b-1's residual: a flooder with many real IPs, each answering the
  host's checks, fills the pending table 2 slots per IP.
- A relay restarted on its router's new address (FR12b-2) drops its
  allocations: they were on the old address, which no peer reaches.

## Host migration decisions (plan 12c)

Decided in `docs/superpowers/plans/2026-10-15-plan-12c-host-migration.md`
("Decisions this plan makes", N63-N81, PL12c-1 and PL12c-2) and by rulings
during its execution: PL12c-0 (the plan's decisions accepted without
asking), PF12c-1 to PF12c-3 (pre-flight), MG-1 to MG-3 (the lobby), PU-1
and PU-2 (the pump), RS-1 and RS-2 (the time sync after a resume), SB-1
and SB-2 (the soak's real-transport bugs), SK-1 to SK-10 (the soak's
stall rules and what it found), UI-2 (the hover fix) and FR12c-1 to
FR12c-5, with FR12c-1a and FR12c-3a (the final review's fixes). Where a ruling changed a decision, the decision below says
so and gives its final form. "If wrong" is the cost of the decision being
wrong. Spec §3.7 and §10 are the source; the code's constants are
`rl_netcode::migration` (timing), `rl_online::migration` (the open
timeout, the texts) and `rl_game::net::pump`.

- **PL12c-0 - the plan's decisions are accepted** without asking the user
  (as for plan 12b: all are within the spec), with N70 sent to the
  pre-flight scan as a named risk (a Windows window drag or resize blocks
  the event loop). If wrong: rework of one decision.
- **PF12c-1 - the pre-flight scan's 24 problems** (`preflight.md`, 61
  rows) were fixed in the plan before Task 1 (12 tasks; `MAX_AHEAD` 6000
  for a resume; the hooks `test_delay_rejoin` and `test_stop_pump`; the
  fifth-migration test). If wrong: plan rework only.
- **N63 - the standby endpoint.** Every client (player or spectator)
  opens an `Endpoint::host` on port 0 (TCP and UDP, port mapping off, its
  STUN servers as configured) when it is welcomed. Its gate refuses every
  offer `NotHost` until the peer takes over (a shared `AtomicBool`, then
  the lobby's banned set). It reports `ToHost::Standby { port, relays }`
  (its first 2 Nostr relays) after each `Welcome`; the host takes at most
  one change per member per `STANDBY_WINDOW_US` (5 s) and holds a later
  one for `tick`. A client whose standby cannot bind, or that has none
  (`MigrationConfig::standby = None`, `rl-cli net-client --no-standby`),
  cannot host but still follows a migration (N79). If wrong: one more
  listening socket per client while it is online.
- **N64 - the succession list.** Order: the match's players in join
  order, then its spectators, then members waiting for the next match
  (followers only). `can_host`: a `Standby` arrived and the member holds a
  world (a player, or a spectator that got this match's `Start`: ruling
  MG-3). A direct address only when the host saw the member at a
  non-global IPv4 address, its path is not `Relayed` (N59) and its port
  is at least `MIN_STANDBY_PORT` (1024); anyone else is reached through
  Nostr. At most 2 relays per entry, each `ws://` or `wss://` of at most
  128 bytes (a bare `ws://` is refused). The receiving side repeats the
  address check (`n64_direct`), so a hostile host cannot aim its members'
  connects at an internet address or a privileged service. `Succession`
  goes out with the match's `Start` and whenever it changes, coalesced
  like `Lobby`. **Ruling MG-1** dropped the plan's `member <
  MAX_MEMBERS` bound on a `SuccessorWire` (member ids grow to 255 before
  wrapping, so players with an id of 48 or more were left out of the
  succession and refused `Closed` from their own match; nothing indexes by
  id): the list stays capped by its length, unique ids and keys. If wrong
  (N64): a successor on another network is never tried directly, its join
  takes the relays' few seconds.
- **N65 - timing.** `HOST_SILENT_US` 2 s (nothing from the host while the
  peer itself kept advancing); each `can_host` successor has a window of
  `WINDOW_US` 5 s, in order, from the moment the migration began;
  `GIVE_UP_US` 10 s after it began (a host is lost at most 12 s after its
  last message); a `NotHost` is tried again after `RETRY_US` 250 ms. A
  successor that answered as a host (contact) is kept past its window
  until the give-up, but only while it commits: one that has not
  committed by its window's end closes its gate, its pending joins fail
  (`Endpoint::refuse_pending`) and the peers follow the later windows
  (pre-flight D8). A peer welcomed by the new host drops its `Migration`.
  Suspended peers stay where they are (at most `MAX_PREDICTION` past their
  confirmed tick) and send a frame-less keepalive every 250 ms.
  **Residual races:**
  - A peer that contacts a successor in the last moment of its window can
    see that successor close; it goes on to the next window (D8). The
    peers' `began_us` differ by their detection skew, which widens the
    window's edge, and a rejoin handled in the poll where the window just
    ended still commits (one poll more; the mutation "commit outside the
    turn" survives the suite).
  - Two successors can each commit only if their peers reached them in
    disjoint windows with each failing to reach the other. This was
    thought to need a network partition between peers; net-soak's
    `lan-16s7c0v-seed2762456828` (plan 12d) split a match without one:
    two peers cut off during the first successor's window never asked it
    again and met in the second's. **Closed by ruling FR12d-Q** (plan
    12d): a successor commits only with a majority quorum, so the match
    continues on one side at most; the harness now models partitions
    (`MigrationPlan::partitions`).
  - Through relays D8's closed gate is learned only when the open timeout
    lapses (6 s, ruling SB-1), so a failed successor costs liveness, not a
    split. A slow relayed follower makes "reachable by some peers only"
    likelier.
  - Breaking the contacts the moment a successor's gate closes is
    unpinned (mutation M16 of Task 3 survives): a player may wait longer
    before it reaches the next successor.

  If wrong: only two successors fit in 10 s; when both die with the host,
  the third is never tried.
- **N66 - "the match continues within 3 s".** Read as: from the moment
  the peers find the host gone to the new host's resume, on LAN, good and
  typical links, when every other peer survives. Measured in the harness
  (worst of 12 seeds, crash and leave, from the first `HostSilent` or
  `PeerLeft` to the resume; Task 3): lan 22 ms, good 143 ms, typical
  422 ms, bad 996 ms, awful 2997 ms; the harness gates 3 s on lan, good
  and typical and `WINDOW_US` on bad and awful. rl-online's loopback test
  (a frozen host, real clocks) bounds the resume at 2 s of silence plus
  3.5 s (its measured time was not recorded). A host that sends `Bye`
  (a leave, a step-down, a closing link: N76) is followed at once with no
  2 s wait. A departed peer the new host still waits for costs up to its
  window (5 s). In the real game a migration takes about 2.5 s on a LAN
  (2 s of silence and the resume's hold); through the public relays it is
  unmeasured (the tests' relay is on the same PC; **ruling FR12c-5**: the
  relay connections are open from the match's start, so an attempt pays
  the handshake and ICE only, and the playtest gives the first number). If wrong: a
  stricter reading (3 s from the host's last message) cannot be met with
  the spec's own 2 s detection on anything but LAN.
  **Plan 12d (`aca289a`):** rl-cli net-sim's N66 verdict (`netsim.rs`) counts only the first
  host and hosts that resumed as "hosts left": a successor that committed
  and crashed before its resume no longer counts (the others wait out its
  window and move on, 10 s, as designed).
- **N67 - the merge.** The new host takes, per slot, the union of every
  report's final inputs; all came from the old host, so they cannot differ
  (a differing value is ignored and counted in `Stats::report_conflicts`;
  its reporter is repaired by the desync path). A reporter's provisional
  inputs are taken only where they extend its slot's known prefix. Nothing
  is filled before the reports are in; at the resume bot slots get their
  previous final value, the new host's own slot the keys it pressed and
  abandoned slots (the old host's player, players not back) `Keys(0)`
  (N6). A report is bounded by `MAX_REPORT_FRAMES` (3000 ticks) and
  `MAX_CONTROL`, cut from its end; a cut value the host decides otherwise
  is repaired. A report's known bits are checked against the match's slot
  count (an out-of-slot report is `Malformed`). **The trust bound:** a
  rejoined, key-proven member's report sets final values for any slot,
  the new host's own and the bots' included, for every tick the host
  lacks, up to `current + MAX_AHEAD` (6000 ticks). An honest reporter
  never reaches past the old host's horizon, but a hostile member could
  forge other players' inputs in that gap; the merge trusts members, as
  the decision says (a hostile host can already end the match). If wrong:
  a peer with a cut report shows a correction after the resume.
- **N68 - collection.** The successor commits (drops its old host and
  becomes the host) at the first proven rejoin, or at once when it expects
  nobody **and its old host is known gone** (its `Bye` came: the session's
  `PeerLeft` or the lobby's `Closed`; **ruling FR12c-1**, below, which
  changed this: before, a successor that expected nobody committed at
  once). It expects the succession entries holding a slot or a spectator
  seat from the start (`LobbyHost::away_in_match`); followers are never
  waited for. It resumes when every expected peer rejoined and reported;
  the window's end stops the wait only for peers not yet back, and the
  reports of peers already back are waited for `REPORT_WAIT` (1 s, in
  rl-online) past it, later ones merged as late reports. A successor
  nobody rejoined by its window's end, while it expected someone, closes
  its gate (D8). **N68's case changed (FR12c-1):** a 1-vs-host match whose
  host crashes, or host plus spectator only, no longer continues for the
  lone survivor against bots: it ends in Host lost about 12 s after the
  crash (2 s of silence, then `GIVE_UP_US`; the text is plain `HOST_LOST`,
  no last try, as the player's own turn found nobody). A host that quits
  or steps down sends its `Bye`, so the player still takes over at once.
  If wrong: a crash in a two-peer match ends it, for the survivor too.
  **Replaced by ruling FR12d-Q (plan 12d):** the successor commits only
  with a majority quorum, not at the first rejoin (see there).
- **N69 - the old host speaks again.** A peer that has not committed
  returns to its old host (`HostHeard`) unless its attempt has contact
  with a hosting successor; a successor that has not committed closes its
  standby again. A follower in contact remembers the news and goes back
  when its contact is lost (rl-online's `Planner::host_heard`; the harness
  does the same since `d0a1a31`, soak bug 2). **The ~3 s outage cliff:**
  a peer's own outage is covered only up to about 3 s (a transport
  cliff: longer, the peer's links close and it cannot return). **SB-2
  narrows N69:** see SB-2. If wrong: a peer whose own link blipped for
  2-5 s rejoins its host instead of the match moving; a peer in contact
  with a successor follows it. **FR12c-1:** in a two-peer match a blip of
  2 s or more on either side, below the transport's cliff, is a stall, not
  a migration: the client's top line reads "you are taking over the
  game…" until it hears the host, then `HOST_BACK`, and nobody hosts.
- **N70 - a stalled host steps down.** A host whose own polls stop for
  `STEP_DOWN_US` (1.5 s, below `HOST_SILENT_US`) leaves the match with a
  `Bye` to everyone, when its succession has a `can_host` entry. The gap
  is measured between two polls from whichever thread (the pump's, PF12c-2).
  The monotonic gap counts; a wall-clock gap counts only beyond the
  monotonic gap plus `HOST_SILENT_US` (a suspend), and a backward step
  never counts (C3). A game host shows "Match lost"; a dedicated server
  logs it and opens its lobby again. It does not rejoin. If wrong: a host
  whose whole machine stalled for 1.5 s loses that match.
- **PF12c-2 - the network pump** (amended by PU-1 and PU-2, beside N70).
  rl-game's `PeerHandle` holds each online peer (host and client alike,
  `--net-sim`'s host too) behind one lock shared by the UI thread and a
  thread `rl-game net`, which polls the peer whenever the UI has not for
  `PUMP_AFTER`. A Windows window drag or resize, a long match load
  (`start_online`) or a slow frame blocks the UI thread and, without it,
  silences a host for 2 s (its peers migrate away from a healthy host and
  N70 steps it down): this is spec §10's worker idea, on native. The
  pump's `Keys(0)` and bots still think for the bot slots; its events wait
  in a bounded queue (`PUMP_QUEUE`, 4096). Nested locking of one peer on a
  thread panics instead of deadlocking. **Ruling PU-1 (the review's
  Critical 1)** replaced the plan's 20 ms: `PUMP_AFTER` is **100 ms**
  (`PUMP_EVERY` 10 ms), well inside N70's 1.5 s and the 2 s host-silent
  window, because at 20 ms any UI frame slower than about 20 ms (30-45
  fps) let the pump run ticks with the player's keys zeroed (40-60 % of
  ticks lost their input). The pump presses the keys the UI last latched
  (and still held) for up to `PUMP_HOLD` (250 ms), then `Keys(0)` (a long
  drag must not keep firing). Frames of 17-99 ms poll the pump zero times
  (an app-path test at 30 and 45 ms). **Ruling PU-2:** the queue never
  evicts control events (`MatchStarting`, `MatchEnded`, `Disconnected`,
  `Kicked`, `Refused`, lobby events); it thins the oldest `Confirmed`
  within a match first (keeping kills, spawns, messages and the end, never
  the newest), then the oldest other event. N21's hidden-window 4 ms wake
  is kept: a hidden frame does more than poll. App-path tests:
  `a_host_whose_frames_stop_keeps_hosting`,
  `a_client_whose_frames_stop_stays_joined`,
  `a_match_load_does_not_silence_the_host`. If wrong: a stall under 100 ms
  is not covered (peers tolerate 2 s), and a lock shared between two
  threads has to be maintained.
- **PF12c-3 - no "former hosts skipped" rule.** A former host is never in
  a later succession (it left or stepped down and does not rejoin), so the
  plan's rule was untestable and was dropped; N74's cap stays. If wrong: a
  former host may host again after rejoining.
- **N71 - migration happens only during a match** (from `Start` until the
  host's `Ended`); a host lost in the lobby or on the results ends the
  game as before ("the connection to the host was lost"). If wrong: a
  group whose host quits between matches must host again.
- **N72 - the migrated game is closed.** It takes back the succession's
  members by key with their member ids, seats and admin marks, and nobody
  else (`Refusal::Closed`); it has no password, is not announced and maps
  no port. A member rejoining after the resume is welcomed as a mid-match
  joiner (waiting, then watching from a state, then playing the next
  match; **plan 12d (N88): its old slot's worm was withdrawn at the
  resume and it watches until the next match; the slot only ever reads
  `Keys(0)`**), keeps its replay, and its migration
  count is cleared at `Ended`. A late rejoiner welcomed after the new
  host's match has ended goes to the lobby (`UiAction::MatchLeft`).
  **Ruling MG-2** (amends the brief's signature): `LobbyHost::migrated`
  also takes the match's member ids (from the `MatchStartWire`) and
  reserves them all, so a departed player's id cannot go to a new member
  while its slot is still in the match. **Ruling MG-3:** the lobby
  records per member whether it got this match's `Start`; `can_host` and
  `away_in_match` use it (`migrated` approximates it from the state), so a
  world-less mid-match spectator is never waited for or chosen. If wrong:
  new players need a fresh hosted game.
- **N73 - the joins to a successor.** Direct first when the entry has an
  address (`MIGRATION_CONNECT_TIMEOUT`, 1 s; a failed direct connect is
  not tried again within the window; **amended by FR12d-Q2 (plan 12d):**
  an unreached successor is asked again every 1 s within its window, an
  attempt under way never doubled), then through Nostr (the entry's
  relays first, then the peer's own, cut to 8, with the successor's key
  expected). A successor listens on its own relays from the moment its
  gate opens and stops if it cancels. A `NotHost` through the relays is
  refused for the window. **Ruling SB-1 (amends the Task 7 timeouts):**
  one open timeout for every route, `ANSWER_OPEN_TIMEOUT` **6 s** after
  the answer (Task 7 had 2 s for a direct link and 6 s through relays,
  measured to rl-net's delayed `Connected`, up to 1.5 s after the link
  opened). A direct join runs the same ICE, DTLS and SCTP handshake as a
  relayed one: on `bad` (150 ms, 5 % loss; its losses were bursty when this was
  measured, before SK-5 made them independent) str0m's lost checks go
  again 0.25, 0.75, 1.75 and 3.75 s after the first, and direct links
  opened 0.75-3.3 s after the answer (soak-transport-125 lost live
  clients as "host lost" at 2 s, 3 of 3 reruns; 6 s passes). A successor
  that died or froze after answering holds its joiners that long, bounded
  by its window and the give-up; a follower can be held up to 6 s through
  relays. If wrong: up to 1 s more on a stale direct address, 1-2 s more
  through the relays than a listener kept open all match, and 6 s instead
  of 2 s for a dead-after-answer successor.
- **N74 - repeated migrations.** At most `MAX_MIGRATIONS` (4) per match;
  past that, host lost (the count is carried by a late rejoiner). The
  new host's succession lists everyone but itself. If wrong: a fifth
  migration in one match ends it.
- **N75 - bots restart** at the new host (`Bot::with_cache` for the bot
  slots of the `MatchStartWire`, on the world as it stands, a new lazy
  area cache). Only the host thinks for bots, so nobody's simulation
  changes. If wrong: bots forget their routes once per migration.
- **N76 - the host's `Bye`, or its link closing, during a match starts the
  migration at once**, with no 2 s wait (also the lobby client's `Closed`,
  pre-flight A7). If wrong: none known.
- **N77 - net-soak.** `cargo xtask net-soak [--minutes N] [--seed S]`
  (default 10 minutes, a clock seed printed and replayable; release
  build; not in `full`) runs seeded random net-sim cases on the real
  simulation (both mods, the 4 bundled levels, 2-16 worms, 1-7 clients,
  0-2 spectators, 0-3 profile changes, 0-2 outages, a host crash, leave
  or freeze in half of them, a spectator arriving) with every 5th run a
  real-transport case on 127.0.0.1, half of them with a server freeze or
  leave (the other half a clean control run with migration off). A
  failure is kept in `target/failures/` (a `NetCase`, replayable with
  `rl-cli net-sim --from`; a transport failure is `soak-transport-<seed>/`),
  at most 32 per invocation, and the soak stops at the cap. A
  `bless-regression --from` makes a failing case a golden; 20 cases are
  kept in `fixtures/net/cases` (the soak's finds, each with its line in
  `fixtures/blessings.log`). The memory test (`allocation_counter`)
  catches only multi-MiB leaks. If wrong: soak-only bugs surface
  overnight, not in `full`.
- **N78 - protocol stays 2** (appended variants; plan 12b's reasoning:
  online play is unreleased); no new dependency. Task 8b appended
  `Control::Resume`'s fields and `Datagram::Resume`, equally unreleased.
- **N79 - a peer that cannot host** (no standby) follows a migration but
  is never a successor.
- **N80 - privacy of the succession.** A member's LAN address and standby
  port reach only the other members of its match, and only when the host
  saw it on a private network (N64). A successor's Nostr listener answers
  with its own identity key, as any host's does.
- **N81 - what the players see.** The HUD's top line, while moving: "The
  host is gone: finding who takes over…", then "…moving the game to
  <name>…" or "…you are taking over the game…". After it the log says
  "<name> is now hosting" or "You are now hosting"; a call-off logs "The
  host is back: the match goes on"; a late rejoiner "You reached <host>
  after the match resumed: you watch until the next match" (or "…after
  the match ended: you play the next one"). The new host's lobby
  shows host controls and, in its Invite section, the final text of
  `MIGRATED_LISTING`: "Taken over from the host: closed to new players"
  (the plan said "only this match's players can join"). **Host lost:** a
  screen titled "Host lost" with the reason (**FR12c-4**: one text per
  case, `LostWhy`: "The host left and no other player could take over
  the game." plus " Last try: <attempt>." when this game tried a
  successor; "The host left, and this match has moved to a new host 4
  times already: it cannot move again."; "The host left while you were
  still joining the match, so you could not follow it."), "The match's replay is saved in
  Replays." once saved, and Replays and Back; the replay is the peer's
  confirmed inputs. For the host itself, whose own game stalled (N70:
  "Your game stopped for N.N s, so the match moved on without you.") or
  whose network cut it off (SB-2), the same screen is titled "Match lost".
- **PL12c-1 / PL12c-2.** 12 tasks, the wire and the sans-IO netcode with
  its harness first so every rule has a seeded test before any socket
  (PL12c-1); spec §9.7's scenarios gain `migration` (PL12c-2, below).
- **RS-1 / Task 8b - the time sync after a resume.** Task 8's review found
  that on bad links clients stalled again after a resume (clients restarted
  one trip late and `Jumped` acted on stale margins): 22 of 80 toy
  migrations, up to 119 ticks. Fixed in rl-netcode, with Resume's wire
  extended: the new host holds its first tick `D` behind R (the longest
  rejoined round trip, whole ticks, at most `MAX_RESUME_HOLD_US` 500 ms);
  `Control::Resume { tick, at_ms, first, frames }` places each player's
  pace on its own clock (`at_ms` = the moment its inputs reach the new
  host, from the median of `arrival - sent` per peer, clamped to
  ±`RESUME_AT_BOUND_MS` 2 s) and carries the finals the player lacks;
  `Datagram::Resume` is an unreliable twin in every datagram to a rejoined
  player until its keys arrive or `RESUME_REPEAT_US` (2 s), because a
  reliable message lost once arrives a ping late; rehosted peers' keepalives
  go every 20 ms (about 3 KB/s per player during a collection); jumps
  ignore out-of-order margins; `tick_phase` is `None` while a pace is held.
  Measured, before and after: the pin test's 80 bad migrations 35 post-resume
  stall episodes to 0; 400 toy bad runs 233 to 0; 90 real-world net-sim bad
  runs (24 runs, 56 episodes) to 0; the grid's bad crash/leave/freeze 8/0/2
  to 0/0/0. Normal play is unchanged (net-feel byte-identical to the
  baseline). `awful` stalls after a resume as it does without one (its
  steady state: 2332 episodes in ticks 800-1500 of 20 seeds with no
  departure, 2413 migrated). The integral term is still zeroed at a rehost
  (keeping it left 3 stall episodes in 400 runs). If wrong: a medium change
  to Task 2's code.
- **RS-1a (plan 12d's soak) - a new host behind the match catches up.**
  A survivor whose own outage stalled it before its host went could take
  over 300+ ticks behind the rows the others reported (net-soak's
  `good-3s2c0v-seed1857283410`: its tick 964, the reports final to 1302).
  It resumed at its own tick and played those final rows at real-time
  pace, so the peer that had run ahead stood still 3.7 s after the resume.
  R is now the later of its tick and its log's frontier after the merge;
  its pace starts that far ahead and it catches up at `MAX_CATCHUP` a call
  (a few hundred ms), as a client does after an outage. Normal migrations
  (R already past the frontier) are unchanged; net-feel unchanged. If
  wrong: the new host's player sees a short fast-forward of the ticks it
  missed.
- **RS-2 - net-sim's stall excusal is bounded.** `RESYNC_TICKS` is gone: a
  stall is excused only if its episode starts and ends inside the window
  from the old host's departure (less `MAX_PREDICTION`) to the resume plus
  `MAX_PREDICTION` (`StallTracker`, rl-cli); N66 bounds the migration
  itself. If wrong: unrelated stalls pass inside a long window.
- **SB-1** is under N73. **SB-2 - a host cut off from its players leaves
  its match.** A host that hears none of its players for `HOST_SILENT_US`
  during a match, with migration on, ends its match view with
  `CUT_OFF` ("Connection lost: the others play on under a new host."),
  keeping its replay (`Session::members_silent`, renamed by FR12c-1; the server logs it and
  opens its lobby again) instead of playing alone. Its peers, finding it
  silent at the same moment, move on, so a host's own network drop must not
  leave it playing a ghost match. This narrows N69: a host's own drop of
  more than 2 s always hands the match on, **but only with two or more
  remote members** (FR12c-1: with one player it plays on, since neither
  side can tell whose network dropped). The harness models it. If
  wrong: a host whose players all crashed at once sees that text instead of
  "everyone left".
- **SK-1 to SK-3 - the soak's stall rules.** Outage windows join the
  migration windows in one episode check (SK-1): for the peer cut off (all
  peers while it is the current host) `from - MAX_PREDICTION ..= until +
  outage_recovery_ticks`; a spectator's outage excuses nothing; every stall
  rule stays on in soak cases. Two real netcode defects were found after an
  outage (SK-2, commit `552b6f0`): the catch-up inputs came late, and the
  first margin jump went into the prediction limit; missed inputs were
  resent one 64-frame datagram per round trip, now one per tick.
  `Profile::outage_recovery_ticks` is two worst-case round trips plus
  re-simulation at the catch-up rate; a called-off migration's window is
  excused, outages add up, on `bad` the host's fills stall others
  (excused), and a player that left is not judged by its last stall (SK-3).
  The soak also found harness bugs: a host cut off while its peers move on
  is "left behind" and its rows bounded like an exited host's, and the
  harness's N69 remembers contact as rl-online does. Two flakes were fixed
  or noted: rl-server's `nat_check` timing test (`close_within`) and the
  e2e `Click("Join")` (autoplay's `ClickUntil` waits for a click's effect;
  the mechanism is unproven). If wrong: none (rules only).
- **SK-4 - bad-link stalls with drifting clocks: fixed or explained.**
  Soak cases on `bad` with 4-6 clients and drifting clocks stalled
  outside any outage window (and failed on the pre-round netcode too).
  - Real netcode fixes:
    - a peer back from its host's silence catches up the pause
      (`45ed63e`, accepted by SK-7);
    - a frame datagram ahead of the frontier waits for its predecessor,
      at most 16 kept (`c9dfdda`);
    - a peer's overlapping outages join (`85c0779`);
    - an outage begun during a migration starts at its tick (`29bb3b7`).
  - Three of the four cases were net-sim's burst-loss tail: 13-15
    datagrams in a row reached `MAX_PREDICTION` 28. SK-5 makes `bad` lose
    datagrams independently, as spec §9 defines it (`dffe613`).
  - SK-6: cases where a link worsens mid-match are no longer judged from
    the change on.
  - The soak then found and fixed two more: SK-8 (a mid-match joiner whose
    host goes is Host lost, `eaecaf2`) and SK-9 (`InputsReport.origin`,
    `78985f1`); SK-10 accepts a legitimate "every survivor host lost"
    (`34d0f30`).
  - Every case is kept in `fixtures/net/cases`. The last soak ran 16 runs
    with 0 failures.
- **SK-5 - `bad` has no bursts.** The spec defines `bad` as 150 +/- 50 ms,
  5 % loss and reordering, with bursts only in `awful` (§9 Profiles);
  net-sim's `bad` with `burst: 2` deviated from it, so its bursts are
  dropped (`dffe613`), `MAX_PREDICTION` 28 (N1) kept, and net-feel
  re-blessed. Re-bless reason: "bad loses independently (SK-5): bad's
  rollback p99 18 -> 17; good's `up_bytes_per_s` 7206 -> 7207, from the
  SK-2/SK-4 netcode changes between the previous bless (`ecb7622`) and
  this one (the final review's A-M3; not bisected)". Net-feel is
  otherwise unchanged. If wrong: real bursty links stall like `awful`
  (accepted by the spec); N1 can be revisited with envelope numbers.
- **SK-6 - a link that worsens mid-match.** A mid-match profile change to
  a worse profile is judged like the awful-change cases: no no-stall
  expectation from the change on for players whose input delay was chosen
  on the better profile (input delay is fixed per match; real links that
  worsen stall). Deferred minor: it is broad (any change to a worse link,
  lan to good too, stops the check). If wrong: a stall bug that shows only
  after a profile change hides in the soak.
- **SK-7 - unsuspend catches up the pause** (accepted, since the host ran
  on); the API comment and rl-netcode's CLAUDE.md say so (`4613a96`).
- **SK-8 - a joining session detects host silence.** A spectator that
  joined mid-match and has no world yet finds its host silent like a
  member and ends with Host lost ("…while you were still joining the
  match…", `eaecaf2`); following the migration as a joiner is deferred.
  If wrong: such a joiner rejoins by hand.
- **SK-9 - a report says where the reporter's log begins**
  (`InputsReport.origin`; the decoder refuses an origin past the ack): a
  spectator that follows a migration after its state loaded got frames
  encoded against a base it never had (4,433 `Malformed`); the new host's
  first frames to it are sent without a base (`78985f1`). Appended to a
  protocol that is unreleased.
- **SK-10 (amended by plan 12d: FR12d-Q's `lacked_quorum`, SK-10b's
  three additions, FR12d-Q2's `reaches`; below) - the verdict accepts "every survivor host lost"** only when the
  harness's own link model shows no successor was reachable through its
  window, the survivors' rows agree as far as they go, and every survivor
  gave up within N68's bound (`34d0f30`). FR12c-3 generalises it.
- **UI-2 - hover.** The theme makes every widget state's stroke equal so
  hovering never resizes a control (`d0d0a3a`,
  `90603b8`: a themed layout harness and tab rows that fit). TextEdit
  keeps its focus outline but loses only its hover border; a Slider shows
  hover by its handle's fill: a per-TextEdit scoped style (about 15
  sites) is not worth it. If wrong: a slightly weaker hover affordance on
  text fields.
- **FR12c-1 - a blip in a two-peer match is a stall** (final review A-C1
  and B-I2). A successor commits only once at least one other peer has
  rejoined it, unless its old host is known gone (its `Bye`); SB-2's
  cut-off fires only for a host with at least two remote members. Neither
  side can tell its own drop from the other's in a two-peer match. Cost:
  N68's case (above) ends in Host lost. **FR12c-1a** (the implementer's
  reading, accepted): spectators count among the members that must all be
  silent, else host, player and spectator could never be a stall. Code:
  `Migrating::gone`, `Session::members_silent`.
  **FR12c-1b (plan 12d's soak, ruling, option B):** the code never did
  what the sentence above says for a `Bye` in general: "known gone"
  commits at once only when the successor **expects nobody** (D8 keeps
  its no-split protection). A literal reading (commit at once on a known
  `Bye` whoever is expected; commit `8b83a4f`, reverted in `bfb6237`)
  split a match in the next soak (`lan-4s2c2v-seed723619267`: a client
  in an outage heard the resent `Bye` late and committed alone beside the
  first successor). So when the old host said `Bye` but an expected peer
  never rejoins (it crashed within the host's 10 s timeout, N87), the lone
  survivor ends Host lost about 12 s after the host left. Cost: a rare
  lone survivor sees Host lost instead of playing on (three players, one
  crashes, the host quits within 10 s); a split match is worse and no
  option avoids both. The SK-10 verdict accepts it narrowly: a successor
  that had exited (crashed or left) by the time its ask was answered
  counts as unreachable, with a negative twin (`ce6b352`); exits are
  paired with migrations by time, `exited_us <= resumed_us` (`bd621bb`).
  Kept soak cases: `good-15s2c0v-seed960552337`, `lan-4s2c2v-seed723619267`
  (no split), `typical-14s4c1v-seed1857358438`. rl-online has no test for
  "a client crashes, then the host's `Bye`" (deferred).
  **Amended by FR12d-Q, FR12d-Q1 (plan 12d), kept above as history:** the
  commit rule is no longer "at least one rejoin" (FR12c-1) nor "expects
  nobody" (FR12c-1b) but a majority quorum. The members are the old
  host's whole succession (players, spectators and followers alike,
  followers in the numerator and the denominator); the old host counts as
  a member unless its `Bye`/`Closed` arrived. A successor commits when it
  and the members back with it are more than half. The two cases above
  still end as before (a two-peer blip is a stall; the lone survivor of
  three is Host lost), but a crashed host counts, see the costs under
  FR12d-Q.
- **FR12c-2 - a rehost drops what belongs to the old host:** `suspend`,
  `rehost` and `take_over` clear the ahead buffer and `rehost` clears the
  queue (final review A-I1; `28f9cca`). If wrong: none.
- **FR12c-3 - the verdict judges every host-lost survivor** (A-I2,
  `cdf7038`): one lost while the match continues must show no successor
  was reachable for it, else it fails. **FR12c-3a** (accepted): the test
  is `Report::could_join` (a hosting successor within the peer's reach
  longer than one ask), generalising SK-10's outage-at-the-ask test, which
  would fail a legitimate window whose successor never hosts; SK-10's
  all-lost rule is unchanged. If wrong: a reach-model gap.
- **FR12c-4 - a failed migration is diagnosable** (B-I1, `42b4471`): every
  attempt's outcome is kept and logged with the successor's name, address
  and reason (`OnlineEvent::MigrationLog`, to the console and log only);
  `HostLost` carries `LostWhy` (see N81). docs/online.md lists the lines.
- **FR12c-5 - standing relay connections** (B-I3, `90afed4`): each client
  with relays holds one WebSocket per relay (at most 8) from the match's
  start to its end and fetches the TURN list then and every 2 minutes,
  subscribing to nothing keyed to it (N80); an attempt through the relays
  borrows the pool. The timing of a relayed migration over the public
  relays stays unmeasured for the playtest. If wrong: idle relay
  connections during every online match.

**Measurements.**
- Harness resume after detection per profile: see N66 (Task 3's table),
  and Task 8b's `bad` figures under RS-1.
- rl-online's loopback tests (real clocks, `127.0.0.0/8`, in `check`):
  4-17 s each (the frozen host 16.7 s with the match's end and results;
  D8's 15.2 s; host lost 11.9-13.5 s after the freeze); a slow relayed
  link opened about 3.75 s after the answer and the resume came about
  4.0 s after `Connecting`.
- `cargo xtask net-nat`: 12 scenarios 145.6 s (11 took about 97 s); the
  `migration` scenario alone 48.9 s (the host killed 8 s into the match,
  the first successor hosts, the other client rejoins relayed, 331
  identical checksum lines, 2 relay allocations). It also fails without the
  successor's standby (`--no-standby`), checked once.
- Net-sim grid in the full tier (`net_migration_grid`): 28.8-30.3 s; the
  server freeze and leave transport cases 23.7-23.9 s each.
- The added `full` time (net_sim's and transport's migration cases and the
  `net-nat` scenario): about 2 minutes by the plan's estimate plus the
  scenario's 49 s.
- **The plan's full run, at `5114219`:** passed, about 14.5 minutes. Per
  step: test 124 s, bot-grid 141 s, gpu 39 s, e2e 35 s, net-nat 147 s (12
  scenarios), bench 161 s (the worst +3.2 %), stress 21 s, net-feel 44 s,
  determinism-windows 141 s. The final review's fix pass changed code
  after it, so a re-run follows that pass.
- A 3-minute `net-soak` found one real desync (the soak-bug chain above);
  after the fixes seeds 101, 201, 301 and 401 ran with 0 failures (the
  SK-4 cases appeared in later runs).

Open items (deferred minors):

- Task 3: breaking contacts the moment a successor's gate closes is
  unpinned (a player waits longer before reaching the next successor if it
  broke).
- Task 8b: a hostile host's `Resume` with a tick far below ours can hold a
  client up to 60 s (a hostile host can already end the match).
- Task 8b: the unreliable `Resume` copy bursts 35-49 KB/s for 0.1-2 s per
  migration (no 1-second window exceeds 24 KiB/s).
- Task 8b: its determinism test covers one scenario and no stats; one
  interpolation step back remains at the resume hold's end.
- Task 9: the lobby's setup panels overflow 320- and 384-point windows
  (older than this plan; not at 4K or ultrawide).
- Task 10: the net-nat migration scenario's `relay_allocates` is satisfied
  before the crash (the post-migration `path: relayed` line is the real
  proof); the first successor's order relies on Docker's start latency; a
  hardcoded `MIGRATION_LIMIT` in a message; a bound message drops its
  seconds.
- Task 6: the dead-successor timeout (6 s after SB-1) and the resume's
  one-poll commit check are the residuals under N65; the resume's report
  deadline `REPORT_WAIT` is decided in rl-online, not rl-netcode.

## Players leaving mid-match (plan 12d)

Decided in `docs/superpowers/plans/2026-10-15-plan-12d-leaving-players.md`
(N82-N89) after the user's playtest report: a player who left a match
stayed in game as a frozen worm. Compatibility with older builds was not a
goal (proof of concept): the replay format is 3, the game bundle 2, the
net-sim bundle 3, the build fingerprint changed; protocol stays 2.

**The user's decisions (2026-10-15).**
1. A leaving player's worm is removed, as OLX does, not handed to a bot.
2. The final results keep a "left" row for it (N85; the deviation under
   "Deliberate deviations").
3. Fewer than 2 worms left ends the match (Deathmatch's `CheckGameOver`;
   a 2-worm match whose host leaves with migration on resumes with one
   worm and ends `LastStanding`).
4. The 10 s crash timeout stays (N87).
5. Slots a migration abandons are removed at the resume (N88; the
   controller's default, accepted).
Also the user's: the results count every death (below) and only the
player's own rope throws are heard (the porting table).

**Rulings.**
- **PL12d-0:** N82-N89 accepted without asking (within OLX parity and the
  user's decisions). Cost if wrong: rework of one decision.
- **PF12d-1:** every pre-flight fix accepted and the plan amended before
  Task 1. Cost: plan rework only.
- **PF12d-2:** N84's projectile-in-flight claim keeps its own test
  (`a_projectile_of_a_worm_that_left_credits_nobody`, a projectile in the
  air when its worm is withdrawn). Cost: none.
- **PF12d-3:** at a migration's resume the old host is withdrawn silently
  (the migration's lines say it); players who did not come back get
  "<name> has left" (`MigratedLobby::drop_away`, `Gone::Left`). Cost: one
  log line.
- **PF12d-4:** the marks travel once per datagram as a compact `(slot,
  tick)` list (`gone`), not a bit on every frame. Measured (Task 3,
  net-feel): about +1 B per host datagram, **about +100 B/s down** on every
  profile (lan 9248 to 9348, good 12405 to 12505, typical 12799 to 12898,
  bad 32085 to 32181, awful 55354 to 55443, bandwidth32 16565 to 16663),
  up unchanged, rollbacks, corrections and stalls identical; inside the
  gate's 10 %, so `fixtures/net/feel.ron` was not re-blessed. Cost: a
  slightly more complex encoder.
- **PF12d-5:** a client crash's 10 s of host fills (`fills_stall`, true on
  `bad`) gets an excused window in net-sim, from the crash tick to the
  timeout plus the recovery, with a negative twin. Cost: a stall bug
  inside that window hides.
- **PF12d-6:** `GameMode::on_drop` must not queue commands (enforced in
  `World::withdraw`: `CommandError::Closed`); carrying them would put
  `Command` on the wire. Cost: a future mode needs another hook.
- **PF12d-7:** a replay's decode accepts withdrawals at `t <= ticks`;
  `Cursor` applies trailing ones before the final checksum, and `finish`
  replaces a periodic checksum at `ticks` (a leave after the final push is
  reachable). Cost: none.
- **PF12d-8:** a withdrawal mark a new host reads from a report is held
  until the resume and applies only to slots whose player is not present
  (not rejoined, not the new host's own); a mark for a present player is
  refused and counted in `Stats::report_conflicts`, after the resume too.
  A forged mark must never remove a live player. Cost: an absent slot's
  exact withdrawal tick may be the resume's instead of the old host's.
- **PF12d-8a (amends PF12d-8's cost):** an honest refused mark (the old
  host timed out a client that was alive, then vanished, and the client
  rejoined the new host) costs **each survivor that held the mark one
  desync and one state repair** (and a desync bundle in rl-online), not
  only an `Inconsistent`. The player stays. The refused mark's values are
  kept and taken unmarked, so the holders' rows agree and exactly one
  repair mends them; the SK verdict accepts exactly that one repair
  (`repaired_withdrawals`). Pinned by
  `a_rejoined_player_marked_by_the_old_host_stays_and_its_holders_are_repaired`.
  The repair's state also drops the refused mark from the holder's log,
  with its checksums confirmed from it (final review A-I1:
  `Event::WithdrawalUndone`): before, the holder's record and replay
  withdrew the player at the old host's tick, and a holder that hosted
  later never marked that player's real leave (`fill` saw it withdrawn).
  Pinned by `a_repaired_holder_that_hosts_later_withdraws_the_player_at_its_leave`.
- **FR12c-1b:** see plan 12c's FR12c-1. The lone survivor ends Host lost
  about 12 s after the host left, when an expected peer had crashed.
- **RS-1a:** see plan 12c's RS-1a: the new host's R is the later of its
  tick and the log frontier after the merge (a new host behind the match
  catches up through its final rows).
- **FR12d-Q - a successor's majority quorum** (net-soak's
  `lan-16s7c0v-seed2762456828`, kept). A host and seven players; players
  2 and 6 lost their links before the host left, so its `Bye` reached the
  other five. Player 1 took the match over with 3, 4, 5 and 7 at its first
  rejoin. Players 2 and 6 found the host silent, asked player 1 while
  still cut off, and an unreached successor is not asked again in its
  window (N73), so they never learned it hosted although their outages
  ended inside that window. In player 2's own window player 6 reached it,
  and the first rejoin committed player 2 too: a split, without any
  partition at that moment. Not asking again would not have closed it (an
  outage over the whole first window does the same). The ruling: a
  successor commits only when it and the members back with it are
  strictly more than half of the match's members not known gone (the
  players and spectators it expects back, itself, and the old host unless
  its `Bye`/`Closed` came; followers did not count, **amended by FR12d-Q1:**
  they count in both numerator and denominator). Two quorums share a
  member, so a match continues on one side at most. It replaces FR12c-1's
  "at least one rejoin" and its exception, keeps FR12c-1 (a two-peer blip
  is a stall) and FR12c-1b (the lone survivor of three is host lost).
  `rl_netcode::migration::quorum`, called by the harness and rl-online:
  a join to an uncommitted successor is held (rl-online holds the
  member's `Welcome` and what follows), the commit takes every held join
  in. Pinned by `only_the_majority_side_of_a_partition_continues`,
  `an_even_split_continues_on_neither_side`,
  `a_successor_short_of_a_quorum_is_host_lost_legitimately` (rl-netcode)
  and `a_successor_without_a_quorum_never_hosts` (rl-online). SK-10 accepts
  an all-lost match whose reached successors could not gather a quorum
  (`lacked_quorum`, generous). **Cost:** an even split continues on
  neither side, and a crashed host still counts: in a match of a host and
  three players, the host's crash with one more player out of reach (dead,
  slow, cut off) is host lost for the other two (before, they played on);
  a host plus one player plus mid-match spectators whose host crashed is
  host lost even if the spectators reach the player (**amended by
  FR12d-Q1:** it now plays on). The players cut off
  in the soak case end host lost, as FR12c-3 judges them (their asks were
  unanswered); asking an unreached successor again in its window would
  bring them back, a change to N73 not made here (then made: FR12d-Q2).
- **FR12d-Q1 - followers count in the quorum** (controller's ruling).
  The quorum's members are the old host's whole succession, followers
  (spectators that joined mid-match) too, counted alike in the numerator
  and the denominator, so the majority property holds. A host, one player
  and mid-match spectators whose host crashes now plays on when the
  spectators reach the player (`a_player_and_its_followers_carry_on_when_the_host_crashes`,
  rl-online's `a_player_and_its_spectators_carry_on_when_the_host_crashes`).
  The harness counts only followers that were in the match (a spectator
  whose join tick never came is not); rl-online counts its migrated
  lobby's `away()` at the turn's opening (every succession member). Cost:
  a member that cannot come back (a spectator still loading, SK-8) still
  counts (rl-online's `a_joiner_still_loading_when_its_host_crashes_is_host_lost`
  now has a third watcher). Net-soak's `bad-10s1c2v-seed1616063176`: the
  player now takes the match over with both spectators at 15.55 s.
- **FR12d-Q2 - an unreached successor is asked again** (controller's
  ruling, amends N73). An ask not reached (no answer, a connect that
  failed or timed out, a link that never opened, a relayed join that
  failed) is made again `REASK_US` (1 s) later, within its window
  (`rl_netcode::migration::REASK_US`; harness `Stage::Retry`; rl-online's
  `Planner`: a direct route again after 1 s, the relays meanwhile, both
  again after 1 s when the relays fail). An attempt under way is never
  doubled. FR12c-3's `could_join` slack is `REASK_US` (was `RETRY_US`),
  and SK-10's reach model (`reaches`) now asks for a cut-free span long
  enough for an ask and a join while the successor is open. Pinned by
  `a_successor_is_asked_again_once_the_askers_link_returns` (harness: the
  asker cut off past its first asks joins, the match goes on; RED
  without re-asks), the planner's unit tests and
  `a_hostile_succession_is_bounded` (relays failing at once: each route
  at most once per second within its window, 20 joins over two windows).
  Net-soak's `lan-16s7c0v-seed2762456828`: player 2 rejoins player 1 at
  8.7 s, player 6 at 10.8 s just after the resume (a late rejoiner, N72).
  Not covered end to end in rl-online: its blackout hook drops datagrams
  only, so a direct join's TCP answer still comes and the link opens on
  ICE's resend (`a_slow_direct_link_is_waited_for`); an ask that fails
  outright on loopback is the planner's tests'. Cost: up to one join a
  second per successor while it is not reached.
- **SK-10b - a joined spectator among the lost** (net-soak's
  `bad-10s1c2v-seed1616063176`, kept). The host crashed with one player
  and two mid-match spectators, each spectator cut off when it asked the
  player: all three gave up. Legitimate (FR12d-Q: the player alone, the
  crashed host counting, has no quorum; the spectators are followers), but
  SK-10 compared every survivor's rows from tick 0 and a joined
  spectator's log has zeros before its state. It now compares those rows
  from the spectator's last state on, as the rest of the verdict does
  (`a_joined_spectator_among_the_lost_is_compared_from_its_state` with its
  twin). Alike (net-soak's `awful-8s1c1v-seed963469720`, kept; it failed
  before FR12d-Q too): a spectator due to join at a tick the match never
  reached, its host crashed before, had no session, yet the verdict asked
  it for a state; a peer without a log is now no survivor and not judged
  (`a_spectator_whose_join_never_came_is_no_survivor`). And (net-soak's
  `awful-6s3c1v-seed1453819890`, kept; it failed before FR12d-Q too): a
  player that found the host gone before its successor did asked it
  before its turn opened, was refused, asked again and was cut off by
  then; SK-10 judged only the first ask. An outage or partition touching
  the asks until the successor's turn opened (`Report::began`) now counts
  as a cut (`an_asker_cut_off_before_its_successor_opens_is_host_lost_legitimately`).
- **A-I1 (final review A, fixed, `1fd255f`) - a repaired holder's log.**
  After PF12d-8a's repair a survivor that held the refused mark kept it in
  its input log: its replay showed a withdrawal the match did not have, and
  if it hosted later, `fill` never marked that player's real leave (a
  frozen worm again). `Session::state_arrived` now drops each of its marks
  below the state's floor whose bit the loaded world lacks
  (`Simulation::withdrawn()`, `InputLog::unmark`), with its checksums
  confirmed from that tick, and pushes `Event::WithdrawalUndone { tick,
  slot }` before `StateLoaded`. Not done: adding marks the state has and
  the log lacks (the state carries no tick to place them at). Pinned by
  `a_repaired_holder_that_hosts_later_withdraws_the_player_at_its_leave`.
- **FR12d-B5 - no client-side de-duplication of leave lines** (final
  review B, ruling). A "<name> has left" notice is not de-duplicated:
  member ids are a wrapping `u8` reused across joins, so it could hide a
  real second departure, and it cannot fix the reverse (a survivor that
  never reads the notice). The window is a host crash within a resend of a
  `Bye`; the worlds stay right, the marks decide them. **Cost if wrong:**
  one duplicated or missing "<name> has left" log line in that rare window.
- **FR12d-B9 - the crash window keeps PF12d-5's recovery term** (final
  review B, ruling). The stall window after a client's crash (the timeout
  plus `outage_recovery_ticks`) models a peer that comes back; narrowing it
  to two trips and ending it at a migration's resume is a verdict change
  against a ruled text, left with the deferred "the crash window does not
  model migrations". **Cost if wrong:** about 0.8 s more excused stalls on
  `bad` after a crashed client's withdrawal, so a stall bug there hides.
- **FR12d-Q, Q1, Q2 - the quorum's costs, together** (plan 12d, accepted):
  - An even partition continues on neither side (both Host lost).
  - A crashed host counts as a member: with a host and three players, a
    host crash plus one more player out of reach is Host lost for the
    other two.
  - A member that can never come back (a spectator still loading, SK-8)
    counts against the quorum.
  - Cost of Q2: up to one join a second per successor while it is not
    reached.

**Decisions.**
- **N82 - withdrawal, not deletion.** `World::withdraw(i)` keeps worm `i`
  in `World::worms()` (slot `i` stays worm `i`, which the netcode, replays
  and the presentation index by) and makes it dead with `lives = WRM_OUT`,
  its rope, latches and state cleared; the mask is `World::gone()`, in the
  world state (refused if it names no worm or a living one) and hashed
  only when not 0, so a match nobody left hashes as before. Every
  gameplay path already ignores a dead `WRM_OUT` worm. OLX deletes the
  worm (`Game::removeWorm`, `src/game/Game.cpp:1359-1366`; the server side
  `CServer.cpp:1352-1482`). Cost if wrong: a reader of `World::worms()`
  must skip gone worms, as it skips dead ones.
- **N83 - the withdrawal tick and how it travels.** The tick is the slot's
  first unknown tick when the host abandons it (`Session::peer_lost`: a
  `Bye`, a timeout, a kick or a failed repair; at a resume, absent
  players); N6's `Keys(0)` begins there. The log stores the mark with the
  slot's value (`InputLog::set_final(T, s, 0, true)`), final like the
  input. On the wire it is a `(slot, tick)` entry of `gone` beside the
  frames of `HostInputs`, `HostResume`, `InputsReport` and `Control::Resume`
  (1 byte when empty), bounded by `valid_marks` (at most `MAX_SLOTS`, each
  slot once, ticks within the frames); a mark without its value in the
  frames is dropped. Frames kept ahead keep their marks. Every peer (host,
  clients, spectators, the replay) withdraws before simulating T; one that
  simulated T or later rolls back to T. A spectator that joins later
  learns the mask with the world state. Cost if wrong: one byte per
  datagram and a more complex encoder.
- **N84 - the mode's side, as OLX.** `GameMode::on_drop` is
  `CGameMode::Drop` (`CGameMode.cpp:254-263`): Deathmatch zeroes the
  worm's runs, its kills and deaths in a row (`kills_in_row`,
  `deaths_in_row`: `drop_worm`); its board row keeps its kills and deaths. The game over below two
  worms is the existing `CheckGameOver` port (`CGameMode.cpp:316-336`): a
  gone worm is out of lives, no longer counted, and never the winner or
  the highest (OLX's `HighestScoredWorm` sees none removed). A kill
  credited to a gone worm (its projectile in flight, its damage leaving a
  worm at 0 health) credits nobody (`wormById` is null,
  `CServer_Game.cpp:126-127`; `kill_worm` maps it to -1), and neither does
  its damage (`CClient::InjureWorm`, `CClient_Game.cpp:340-349`, `401-404`:
  no damage factor, no damage score; final review A-M2). Cost if wrong:
  none known.
- **N85 - the scoreboard** (the deviation). `Score::left` marks a gone
  worm's row (hashed only when true). The live scoreboard (`rl_ui::hud::rows`)
  leaves it out; the results (`Results`) keep it, its lives reading "left",
  greyed, ranked as an out worm by `CompareWormsScore`. `MatchEndWire` is
  unchanged. Cost if wrong: one table row.
- **N86 - the texts.** OLX's `NetworkTexts` (`Options.cpp:769-770`):
  "<name> has left" (a `Bye` or a closed link) and "<name> has timed out"
  (the session's `PeerTimedOut` or the link's `Why::Timeout`);
  `Notice::TimedOut` is new (appended). Kicks and bans keep their own
  notices. At a resume see PF12d-3. Cost if wrong: one log line.
- **N87 - the crash timeout stays 10 s** (`SILENCE_TIMEOUT_US` in the
  session, `SILENCE_TIMEOUT` in rl-net). Until it fires, the lagging rule
  repeats the slot's last final keys (a crashed player's worm keeps its
  last move for up to 10 s); at it the worm is withdrawn. Those fills
  stall players whose link cannot bear them (`fills_stall`, true on
  `bad`); net-sim excuses them in a window (PF12d-5). Cost if wrong: a
  stall bug inside that window hides.
- **N88 - migrations.** Slots a resume abandons (the old host's player,
  players not back) are withdrawn at their first unknown tick, by the same
  fill (that tick may lie before R: every peer rolls back to it). The
  merge **does not** take every reported mark at once (the plan's text):
  marks are held and decided at the resume (PF12d-8, PF12d-8a), so a
  survivor's mark for an absent slot is kept and a forged one never
  removes a present player. A peer that rejoins and then leaves is
  withdrawn at the resume's fill; a late rejoiner's old slot is withdrawn
  at the resume and it watches until the next match. Cost if wrong: none
  known.
- **N89 - a watcher's camera** (a spectator, a replay or bots alone)
  skips gone worms; when the one it follows is withdrawn it moves on to the
  next worm not gone (`follow_next`, `after_tick`). Cost if wrong: a
  watcher presses an arrow key once.

**Also done in this plan (recorded in the porting table and deviations
above).** The results and the Tab board count every death and suicide
(`953565a`, user decision 2026-10-15; OLX's gated counters still drive the
score, ranking and winner, so the oracle is unchanged and the match goldens'
checksum lines were re-blessed). Only the player's own rope throws are
heard (`e55299b`, OLX `CWormHuman.cpp:340-342`, `555-563`, `570-574`).
Open and the user's call: the online pickup sound still plays for every
worm on every peer; OLX plays it for local worms only
(`CClient_Game.cpp:136-142`).

**Soak and feel.** `net-soak` found two real faults after Task 7: a new host
behind the match (RS-1a, fixed, `c7d74d7`) and a lone survivor's Host lost
(FR12c-1b, accepted as a cost, `aca289a`). After the fixes the soaks ran
18 + 20 (HEAD) and 20 + 24 runs with 0 failures (about 3 minutes each, four
transport cases each); `net-feel` passes every profile, rollbacks,
corrections and stalls equal the baseline, down bytes +100 B/s as Task 3.
Net-sim's soak cases draw a client departure in one case of four.

**The plan's full run, at 8ff6b51:** passed in about 14.5 min (test
135 s, bot-grid 151 s, gpu 25 s, e2e 38 s, net-nat 12 scenarios 145 s,
bench 161 s with the worst +2.6 % on `bots_8_tick`, stress 21 s,
net-feel 44 s, determinism-windows 126 s).

Open items (deferred minors):

- Task 1: a command a mode queues in `on_drop` is not carried by world
  state transfer (Deathmatch queues none; PF12d-6 forbids it).
- Deaths fix: `ingame::results_match_world_scores` asserts only that the
  board's counts are at least the worm's gated ones.
- Task 6: no replay test reaches the results screen with a left row;
  `follow_next` has no test for "all but the followed worm left" or "all
  gone"; the rl-ui unit test's RED was only a compile error.
- FR12c-1b: no rl-online test pins "a client crashes, then the host's
  `Bye`" (the lone survivor's Host lost).
- Task 7: `reproduce` filters `gone` per tick (O(inputs x gone)); no twin
  for a profile where `fills_stall` is false; the crash window does not
  model migrations.
- Final: minor (deferred): a member that can never come back (a spectator
  still loading) counts against the quorum.
- Final: minor (deferred): FR12d-Q2 has no end-to-end rl-online test (the
  outage hook drops UDP only; the planner's tests and the harness cover it).
- Final: minor (deferred): SK-10's reach model stays generous
  (`lacked_quorum`, `reaches`).

**Re-run full after the final fixes, at c76ffad:** passed in about 15 min
(test 144 s, bot-grid 163 s, gpu 37 s, e2e 47 s, net-nat 12 scenarios
146 s, bench 162 s with the worst +3.8 % on `bots_8_tick`, stress 21 s,
net-feel 43 s, determinism-windows 121 s).

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
  their first 100 steps after a spawn. Since 2026-10-13 kept only with
  `SimSettings::olx_quirks` (the oracle); play fixes it (Deliberate
  deviations).
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
  a killer that is already dead still gets the kill. These are the worm's
  counters, which the score reads. The board counts every death (see
  "Deliberate deviations").
- **Gibs draw 4 `rand()` while the entity pool has room, 2 after** (the
  dedicated server's pool never frees; `disease-bomb`, `nuclear-bomb` and
  `sunray-beamer` kill with a full pool). The gib image exists in dedicated
  mode (`explode-kill-respawn`'s 580 draws).
- **The position recorder survives a respawn** (`clear(vPos)`): `get(0)` is
  a position from the previous life (D7; since 2026-10-13 play fills every
slot with the spawn position, a first life's too).
- **Bleeding:** below 15 hp a worm bleeds in the first sub-step and then every
  2 s, 80 `rand()` calls and 40 entities each time (`bleed`).
- **Bonuses:** a grab reaches the server one frame later, and its
  S2C_DESTROYBONUS the client one more frame later; the server keeps its copy
  until BonusLife ends (`bonus-health`, `bonus-weapon`).
- **Aim:** the traces use LX56 aim (`ForceLX56Aim`: max speed 100,
  acceleration 500, no friction, `REDUCE_CONST(200)` when no key is held). The
  angle is clamped to [−90°, 60°].
