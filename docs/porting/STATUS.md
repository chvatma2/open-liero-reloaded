# Porting status

What has been ported from OpenLieroX (commit `d1ad953f`) and how it is verified.

| Area | OLX source | Rust | Status | Verified by |
|---|---|---|---|---|
| Gamescript v7 loader | `src/common/CGameScript.cpp:582-1152` | `olr-import-lgs` | ported | unit and property tests; Classic and MW 1.0 import exactly |
| `.lxl` image levels | `src/common/MapLoader_LieroX.cpp` | `olr-import-lxl` | ported | unit tests; 4 levels in scope, dirt counts match |
| Skin colorization | `src/client/CGameSkin.cpp:720-790` | `olr-data::skin`, `olr-import-gfx` | ported | unit tests; all 409 repository skins and 0.58 rc5's 309 more (`content/olx-0.58-rc5/`) import |
| Theme loading | `src/common/CMap.cpp:308-370` | `olr-import-gfx` | ported | Dirt theme imports |
| Gamescript v8 | `CGameScript.cpp` (v8 branches) | — | not started | no content in scope |
| Pixmap `.lxl` | `MapLoader_LieroX.cpp:440+` | — | not started | no content in scope |
| Fidelity oracle (test hooks) | `src/game/Game.cpp`, `src/client/CInput.cpp`, `src/common/CWormHuman.cpp` | `tools/olx-oracle`, `olr-oracle` | done (plans 2–3) | 7 movement scenarios recorded twice and identical; `cargo xtask oracle-record --check` |
| Oracle v2: shots, weapons, bonuses, clocks (spec §18) | `src/common/CShootList.cpp`, `src/server/CServer_Send.cpp:556-569`, `src/server/CServer_Game.cpp:266-316`, `src/common/CGameScript.cpp:937,1159` | `tools/olx-oracle`, `olr-oracle` (`Fields`, `compare_fields`), `olr-scenario::CHECKED` | done (plan 4 task 1) | `fire-minigun` recorded twice and identical; the 7 M3 traces changed only in added fields |
| Oracle v2: message delivery (spec §18 D10); weapon, damage and bonus scenarios | `src/common/CChannel.cpp:69-85`, `src/client/CClient_Parse.cpp:468`, `src/server/CServer_Parse.cpp:90` | `tools/olx-oracle`, `xtask oracle-gen-weapons`, `fixtures/oracle/weapons/` | done (plan 4 task 3) | 88 weapon scenarios and 10 special ones recorded twice and identical; `olr-oracle/tests/fixtures.rs` checks what each shows |
| Frame and physics clocks | `src/game/Game.cpp:897-959`, `src/client/CClient_Game.cpp:60-222` | `olr-sim::World::tick` | ported | all 7 oracle scenarios, bit for bit; `docs/porting/timing.md` |
| Worm movement and collision | `src/common/PhysicsLX56.cpp:88-469` | `olr-sim::physics` | ported | `fall`, `walk`, `jump`, `dig` |
| Keyboard input (aim, move, jump, dig, rope, strafe, previous/next weapon, Weapon 1-5) | `src/common/CWormHuman.cpp:81-560` (strafe `:465-503`, previous/next `:443-456`, Weapon 1-5 `:430-440`), `src/client/CInput.cpp` | `olr-sim::physics::get_input`, `olr-sim::input` | ported (keyboard, LX56 aim; strafe and previous/next by issue #23, Weapon 1-5 by issue #48, their oracle traces owed: `docs/deferred-verification.md`; a Weapon key beyond the worm's slots does nothing in play, a deviation below) | `aim`, `tap`, `walk`, `jump`, `dig`, `rope`, `strafe_moves_both_ways_without_turning`, `previous_and_next_weapon_wrap`, `previous_and_next_weapon_stack_with_select_weapon`, `previous_and_next_weapon_wait_while_firing_without_combos`, `the_new_keys_are_hashed_only_when_not_at_rest`; #48: `each_weapon_key_selects_its_slot`, `a_held_weapon_key_selects_again_at_every_read`, `a_tapped_weapon_key_is_seen_by_the_next_read`, `with_two_weapon_keys_held_the_highest_wins`, `a_weapon_key_beyond_the_slots_does_nothing_in_play_and_wraps_with_olx_quirks`, `weapon_keys_wait_while_firing_without_combos`, `weapon_keys_come_after_select_weapon_and_before_previous_and_next`, `a_dead_worm_ignores_the_weapon_keys`, `weapon_keys_pressed_and_released_hash_as_never_pressed` |
| Ninja rope | `src/client/CNinjaRope.cpp`, `PhysicsLX56.cpp:505-604` | `olr-sim::rope` | ported | `rope` |
| Hole carving (material) | `src/common/CMap.cpp:1005-1105`, `2172-2199` | `olr-terrain::Terrain::carve_hole` | ported | `dig` (244 dirt cells), spawn holes (`mh` at frame −1) |
| Float `sinf`/`cosf` (aim directions) | glibc 2.35 `sysdeps/ieee754/flt-32/s_sinf.c`, `s_cosf.c` (x86-64 FMA variant) | `olr-math::trig` | ported | all 2³² inputs bit-identical to the oracle container's glibc (a throwaway FFI comparator run in `rl-olx-src`) |
| Mod compiled to OLX's projectile tree (`SimMod`) | `src/common/CGameScript.cpp:833-1157` (`LoadProjectile` order) | `olr-sim::mod_rt` | ported (plan 4 task 4) | every scenario: indices and path names equal the trace header's (`olr-scenario` checks) |
| Weapon slots, firing, laser sight entity | `src/common/PhysicsLX56.cpp:471-498`, `src/server/CServer_Game.cpp:366-466`, `src/client/CClient_Game.cpp:496-544`, `751-808` | `olr-sim::weapons` | ported | `s` (slots) of all 112 scenarios for the whole trace (109 match fully); `beam-burst`; the laser sight event's length by a unit test |
| Shot list, encode/decode round trip, Model R delivery, `ProcessShot` | `src/common/CShootList.cpp`, `CClient_Game.cpp:813-946` | `olr-sim::shots`, `olr-sim::shot_codec` | ported | `s` of every scenario, `multi-shot` in particular (corrupted `spd`), `die-before-delivery` (dropped shots) |
| Beams (`ProcessShot_Beam`, `DrawBeam`) and specials (`ShootSpecial`, the jetpack) | `CClient_Game.cpp:549-711`, `926-1055` | `olr-sim::beam` | ported (plan 4 task 8; the bonus check task 9) | `weapons/classic/laser`, `weapons/mw-1-0/laser` and `beam-burst` match fully; specials by unit tests only (`jetpack_quirks`: no special weapon in Classic or MW 1.0) |
| Projectile pool, `Proj_SpawnInfo::apply` (shots and projectiles), `CProjectile::Spawn` | `include/FastVector.h`, `src/common/PhysicsLX56_Projectiles.cpp:495-640`, `src/common/CProjectile.cpp:43-166` | `olr-sim::projectile`, `olr-sim::spawn` | ported | every projectile field (`p`, hidden fields included) of every scenario for the whole trace; the pool limit by `pool_full_drops_new_projectiles_like_olx` |
| Projectile motion, collision, timers, trails, animation, worm hit test, position recorder | `PhysicsLX56_Projectiles.cpp:52-491`, `1234-1411`, `src/util/VecTimeRecorder.h`, `PhysicsLX56.cpp:468` | `olr-sim::proj_sim`, `olr-sim::worm::PosRecorder` | ported (plan 4 task 5) | as above; trails and animation by every weapon scenario (`mine`, `float-mine`, `nuclear-mines`, `trap-mines`, `booby-trap`, `electro-gun`, `jet-pack`, `big-nuke` for the timers and recorder) |
| Projectile actions (`applyTo`, `execute`): bounce, disappear, nothing, go-through, speed and heading actions, children, the junk rule | `PhysicsLX56_Projectiles.cpp:670-945`, `1115-1230`, `CProjectile.cpp:376-429` | `olr-sim::actions` | ported (plan 4 tasks 5-6) | all 88 weapon scenarios of both mods (`p`, `mh`, `dirt`, `hp`, `rc`); 109 of 112 scenarios match fully |
| Float `atan2f` (projectile headings) | glibc 2.35 `sysdeps/ieee754/flt-32/e_atan2f.c`, `s_atanf.c` | `olr-math::atan2f` | ported | 6 million input pairs bit-identical to the oracle container's glibc; 20 pinned in a test |
| Explosions, carve, dirt, green dirt and their dirt counts; injury actions | `CClient_Game.cpp:227-330`, `CMap.cpp:1114-1340`, `PhysicsLX56_Projectiles.cpp:781-824`, `1115-1154`, `CWorm.cpp:1381-1389` | `olr-sim::effects`, `olr-terrain::Terrain::{place_dirt, place_green_dirt}` | ported (plan 4 task 6; the bonus chain task 9) | `mh`, `dirt` and `hp` of every scenario; 109 of 112 scenarios match fully (the 3 D9 ones up to their respawn); `place_dirt_at_the_edges` and `green_dirt_at_the_edges` for the map edges |
| Worm damage (`InjureWorm`): factors, `SelfInjure`, damage score, blood draws | `CClient_Game.cpp:335-460`, `CGameObject.cpp:35-44`, `CWorm.cpp:1600-1627` | `olr-sim::damage::injure_worm` | ported (plan 4 task 7) | `hp`, `rc`, `ents` of every weapon scenario (`classic/*`, `mw-1-0/*`); factors and `SelfInjure` by unit tests only |
| Death, kills, deaths, suicides, lives; death draws (sound, gibs by the entity pool, blood); the exact-zero death (`CWorm::think`) | `CServer_Game.cpp:108-162`, `CGameMode.cpp:69-252`, `CWorm.cpp:1399-1449`, `src/gusanos/base_worm.cpp:365-381`, `623-659`, `src/client/Entity.cpp:161-205` | `olr-sim::damage::{kill_worm, die_at_zero_health, olx_kill_credit}` | ported (plan 4 task 7); the crediting is the game mode's since plan 6 task 1 (`GameMode::on_kill`; `OracleMode` is OLX's rule) | `explode-kill-respawn`, `die-before-delivery`, the weapon scenarios' kills; exact-zero deaths in `assassin-s-shot`, `flak`, `plasma-launcher` |
| Bleeding | `PhysicsLX56.cpp:317-330` | `olr-sim::physics::bleed` | ported | `bleed`, `die-before-delivery` |
| Respawn loop, `SpawnWorm`, `FindSpot`, `CWorm::Spawn` | `CServer_Game.cpp:38-104`, `200-225`, `CMap.cpp:2381-2396`, `CWorm.cpp:591-651`, `CWormHuman.cpp:115-119` | `olr-sim::respawn` | ported (DM; no predefined spawns, no team spawns) | `explode-kill-respawn` and every weapon scenario whose dummy dies (`tod`, `respawn`, positions, `mt`) |
| Bonuses: server spawn, lifetime and grab handling; client spawn (hole), physics, pickup, health and weapon effects; the messages between them (D10); explosions and beams destroying bonuses | `src/server/CServer_Game.cpp:227-316`, `src/server/CServer_Parse.cpp:834-903`, `src/client/CClient_Parse.cpp:1464-1516`, `1977-1990`, `PhysicsLX56.cpp:619-715`, `CClient_Game.cpp:128-151`, `255-270`, `736-745`, `986-1002`, `CWorm.cpp:1454-1506` | `olr-sim::bonus`, `olr-sim::effects::explosion`, `olr-sim::beam` | ported (plan 4 task 9) | `bonus-health`, `bonus-weapon` (`b`, `sb` and every other field); natural spawning, lifetime, explosions and beams by unit tests only |
| Acceptance: all weapons of both mods | `fixtures/oracle/weapons/` | `olr-scenario/tests/oracle.rs` (`CHECKED == Fields::ALL`) | done (plan 4 task 11) | 109 of 112 scenarios match OLX bit for bit in every field; the other 3 are pinned to their first divergence (D9, below) |
| Tick events (sounds, map edits, explosions, cosmetics, shots, damage, kills, pickups, spawns) and the state checksum | every `StartSound`, `PlaySoundSample`, `SpawnEntity` and `SetWormBeamEntity` above; `src/game/Sounds.cpp:25-30` | `olr-sim::events`, `olr-sim::checksum` | done (plan 4 task 10) | unit tests; the oracle traces are unchanged (no draw added); `olr-scenario/tests/smoke.rs` (8 matches, clone checksum streams) |
| Event data for rendering (spec §19 E6): explosion `shake`, the gib's worm, the dirt particle's probe pixel, each event's sub-step (entity passes before it) and `World::substeps_last_tick`, `Damage.dealt`, the `RotIncrement` 0→1 bump (`World::rot_bumped`), `World::physics_time`, `Projectile::serial`, `NinjaRope::throws` (a throw count; not OLX state) | `CClient_Game.cpp:206-208`, `229-330`, `380`, `PhysicsLX56_Projectiles.cpp:590-593`, `777`, `1115-1130`, `CMap.cpp:2172-2199`, `PhysicsLX56.cpp:595-607`, `CProjectile.cpp:291-292` | `olr-sim::events`, `olr-sim::world` | done (plan 5 task 1) | `events_carry_e6_data`, `projectile_serials_differ_across_slot_reuse`, `every_rope_throw_counts_even_while_the_rope_is_out`; the oracle traces are unchanged (no draw added); smoke matches check every event's sub-step |
| Core pack: shared game sprites, game sounds (`.ogg`), `colours.cfg` (spec §19 E9) | `src/client/DeprecatedGUI/Graphics.cpp:56-97`, `134-321`, `src/common/CWorm.cpp:661`, `src/game/Sounds.cpp:25-30`, `src/common/StringUtils.cpp:409-447` | `olr-import-gfx::import_core`, `olr-pack::CorePack`, `olr-content::CorePack` | done (plan 5 task 1) | `olr-content/tests/content.rs` (`loads_every_manifest_item`), the corpus snapshot |
| Colour terrain (spec §19 E7): carve (back pixels, brown rim), `PlaceDirt` (`FrontTile`), `PlaceGreenDirt` (OLX's shades), beam holes, blood and gib stains, dirty rectangles | `src/common/CMap.cpp:1005-1340`, `2355-2373`, `src/client/Entity.cpp:355-390`, `src/client/GfxPrimitives.cpp` (`DrawImageStretch2`) | `olr-terrain::ColorMap`, `olr-content::{color_edit, apply_edits}` | done (plan 5 task 2) | `olr-terrain` colour tests; `olr-content/tests/color.rs` (`color_map_agrees_with_material_after_a_match`, 3000 ticks); golden `fixtures/render/dig-color.png` (`olr-scenario/tests/render_golden.rs`) |
| Presentation (spec §19 E1-E4): camera (hard lock, clamp, shake, dead worm, respawn), skin frames (the facing-left mirror one column off), crosshair, muzzle flash, old-style health bar, rope, laser sights, beams, projectile frames (animation, special angle, angle, rotation with the `RotIncrement` bump), bonuses (blinking), layer order; interpolation (E2) | `src/client/CViewport.cpp:118-472`, `src/common/CWorm.cpp:1016-1300`, `src/client/CGameSkin.cpp:584-601`, `src/client/CNinjaRope.cpp:132-180`, `src/common/CProjectile.cpp:227-300`, `src/common/CBonus.cpp:49-76`, `src/client/Entity.cpp:30-157`, `305-314`, `src/client/GfxPrimitives.cpp:1795-2520`, `src/gusanos/viewport.cpp:105-253` | `olr-scene` | done (plan 5 task 3) | `camera_locks_clamps_and_shakes_like_olx`, `skin_frame_matches_olx_table`, `projectile_frames_match_olx`, `interpolation_never_blends_reused_slots_or_respawns`, `draw_list_order_matches_olx_layers`, `beams_and_laser_sights_live_two_entity_passes`; scene snapshots (`olr-scene/tests/snapshots`) |
| Renderer (spec §8.2, §19 E3): art-scale world pass (terrain, instanced sprites from one atlas, world-pixel lines), upscale (integer nearest, sharp bilinear), offscreen PNGs; OLX's corner-rule colour key for sprites lives in the importers since plan 13c (`olr_format::image_io::olx_sprite_to_alpha`, applied once at import; the renderer draws the alpha it is given) | `src/client/GfxPrimitives.cpp:116-150` (`SetColorKey`); OLX blits with SDL onto a 2× screen | `olr-render`, `olr_format::image_io`, `olr-cli render` | done (plan 5 task 5) | `dirty_rect_upload_equals_full_upload`, `upscale_filters_are_selectable`, `missing_assets_use_placeholders`, `srgb_targets_show_the_same_colours`; several views a frame (issue #26, split screen's fair view: "Split screen (issue #26)" below): `two_views_in_one_frame_match_single_views`, `the_glow_light_steps_once_a_frame_with_four_views`, `a_view_draws_only_its_area`; goldens `fixtures/render/frame-*.png` (`olr-cli/tests/render_golden.rs`, `cargo xtask full`) |
| Cosmetic entities (spec §19 E1, E8): the pool (OLX's 1024 slots with `olx_quirks`; 16384 with a death reserve in play since 2026-10-13, see Deliberate deviations), per-sub-step physics and animation, stains, blood drips, gib spins and colours, drawing and clipping | `src/client/Entity.cpp:161-468`, `include/FastVector.h`, `src/common/CWorm.cpp:671-757`, `src/common/CMap.cpp:338-352` | `olr-fx` | done (plan 5 task 4) | `blood_falls_and_stains_like_olx`, `explosion_frames_and_invisible_start`, `gibs_use_the_victims_colour`, `entity_cap_drops_new_ones`, `animations_live_as_long_as_olx`, `fx_never_touches_sim_randomness`, `colour_edits_follow_the_sim_like_apply_edits`; the player's blood amount (issue #6, Deliberate deviations): `tests/blood.rs` |
| Sound (spec §8.4, §19 E1, E12): positional mix (gain 0.7, inverse distance clamped, reference 50, rolloff 2, listener 20 px out at the view centre), THROW and PICKUP at the listener with gain 1.0, voice limits (a death sound steals since 2026-10-13, see Deliberate deviations), live voices re-mixed as the listener moves, silence without a device; with several local views (issue #27) each sound heard from the nearest (a deviation: "Split screen (issue #26)" below) | `src/sound/sound_sample_openal.cpp:206-270`, `src/sound/sfx.cpp:120-134`, `src/sound/sfxdriver_openal.cpp:43-92`, `src/game/Sounds.cpp:25-30`, `src/common/CGameScript.cpp:1185-1205` | `olr-audio` | done (plan 5 task 6) | `positional_gain_matches_olx_openal_defaults`, `voice_limits`, `silent_when_no_device`, `plays_events_on_a_mock_device`, `decodes_every_mod_sound`, `each_sound_is_heard_from_the_nearest_listener`; `device_check` example (`cargo xtask full`) |
| Whose rope throws are heard (the user's playtest report, 2026-10-16: every worm's throw played map-wide at gain 1.0, about 7 to 12 starts a second with 7 to 8 bots): only the player's own (the local slot; a replay's viewer), as OLX plays `smpNinja` only in the human input handler; a bot's or a remote worm's throw is silent. `olr-sim`'s THROW events carry no owner, so the session keeps as many as the player's `NinjaRope::throws` grew (they all play at the listener) | `src/common/CWormHuman.cpp:340-342`, `555-563`, `570-574`, `src/common/CWormBot.cpp:1753`, `3823`, `src/common/PhysicsLX56.cpp:300-303` | `olr-game::throw_sound`, `Session::sounds` | done (2026-10-16) | `only_the_players_rope_throws_are_heard`, `bots_alone_throw_silently`, `a_replay_hears_its_viewers_throws`, `an_online_match_runs_headless`; a throw while the worm may not use the rope (`can_use_ninja` false, never set today) is not heard, OLX's is |
| Input devices (spec §8.5, §19 E5): OLX's player 1 keyboard and gamepad defaults (Strafe, Previous/Next weapon since issue #23; Weapon 1-5 since issue #48: `1`-`5`, keyboard 2's `6`-`0`, no pad input), trigger and stick thresholds, presses and releases latched between ticks; a Dig action pressing OLX's dig gesture (issue #23, not OLX's) | `src/client/Options.cpp:45-80`, `src/client/CInput.cpp:200-226`, `src/common/CWormHuman.cpp:357-394` (Dig's 0.2 s repeat) | `olr-input` | done (plan 5 task 6; issues #23, #48) | `default_bindings_match_olx`, `gamepad_maps_to_keys`, `tap_between_ticks_is_latched`, `tick_latch_sees_every_press_and_release`, `dig_presses_the_other_direction_for_one_tick`, `held_dig_digs_every_twenty_ticks`, `dig_digs_exactly_as_the_gesture`, `the_new_actions_rebind_and_an_older_file_gets_their_defaults`, `the_weapon_keys_rebind_need_no_pad_and_an_older_file_gets_their_defaults`, `the_keys_screen_lists_and_rebinds_the_weapon_keys`, `digits_typed_in_chat_select_no_weapon` |
| Mouse controls (issue #24, experimental): left click Shoot, right click Rope, the wheel Previous/Next weapon, Space Jump, A/D strafe, S Dig, rebindable (issue #25: the mouse's buttons and wheel, mouse mode's own keys); the worm faces and aims at the cursor through a client-side controller pressing a key player's keys (never OLX's direct mouse aim); the pointer grabbed in a match | `src/common/CWormHuman.cpp:81-560` (the input read it plans with; `:280-306`, OLX's mouse aim, deliberately not ported) | `olr-input::mouse`, `olr-game::pointer`, `olr-online::KeysFor` | done (issue #24; playtest owed: `docs/deferred-verification.md`) | `aim_is_no_faster_than_holding_the_key_and_settles`, `facing_follows_the_cursor_with_hysteresis`, `turning_never_digs`, `a_and_d_strafe_and_the_gesture_passes_through`, `wheel_notches_press_one_each`, `the_cursor_aims_and_turns_a_real_worm`, `a_and_d_keep_the_facing_online_and_in_a_stall`, `wheel_notches_change_the_weapon_one_each`, `a_mouse_player_and_a_key_player_pressing_the_same_keys_agree`, `local_keys_are_told_the_world_and_the_look_ahead`, `the_grab_falls_back_and_frees`, `mouse_controls_are_switched_in_the_keys_tab`; #25: `every_mouse_input_is_captured_and_bound`, `mouse_mode_has_its_own_keys_and_rule`, `the_unreachable_rule_holds_in_both_modes`, `a_file_from_before_mouse_bindings_is_repaired`, `rebound_mouse_inputs_press_their_actions`, `rebound_buttons_work_in_the_controller_on_a_real_worm`, `the_keys_screen_rebinds_mouse_controls`, `an_options_file_from_before_mouse_bindings_is_repaired` |
| Game loop and sandbox (spec §8.6, §19 E10-E12): fixed 100 Hz tick, interpolated frames, catch-up cap, window surface states, WSL GPU defaults, F5 restart from assets loaded once, overlay | `src/game/Game.cpp:897-959` (the frame clock it drives) | `olr-game` | done (plan 5 task 7) | `loop_never_runs_more_than_n_catchup_ticks`, `surface_survives_resize_and_outdated`, `sandbox_setup_spawns_targets`, `session_plays_and_restarts_from_loaded_assets`; scripted runs in a WSLg window on all 4 levels × 2 mods |
| Long sessions (plan 5 review focus 5): 60 000 ticks (10 minutes) of the sandbox with chaos keys, `olr-fx`, mock audio and an offscreen render every 10th tick | — | `olr-game/tests/soak.rs` | done (plan 5 task 8) | `soak_sandbox_10_minutes_headless` (`cargo xtask full`, about 15 s): cosmetic entities, projectiles, entity count, audio voices, popups, instance buffer, atlas and terrain texture stay bounded, no panic |
| Match API (spec §20 F6, F9): `MatchSetup`, `GameMode` callbacks and commands, `Message` and `GameOver` events; game over's effects on the simulation (kills ignored, no respawns or bonus spawns, no input or shooting, the client's projectiles cleared, the physics stopped after `GAMEOVER_WAIT`) | `src/server/CGameMode.cpp:69-252`, `src/server/CServer.cpp:532-625`, `src/server/CServer_Game.cpp:38-42`, `108-113`, `167-261`, `src/client/CClient_Game.cpp:76-91`, `116-117`, `src/common/PhysicsLX56.cpp:300`, `src/client/CClient_Parse.cpp:1436-1451`, `src/game/Game.cpp:1267-1276`, `include/FastVector.h:72-77`, `src/common/Physics.cpp:36-44` | `olr-sim::{setup, mode, referee}` | done (plan 6 task 1) | `oracle_mode_matches_the_hard_coded_credit` and the oracle (unchanged), `mode_commands_apply_inside_tick_deterministically`, `game_over_stops_kills_spawns_input_and_bonuses`, `give_weapon_mid_match_does_not_allocate`; the oracle scenario `dm-kill-limit` (plan 6 task 2): the end in the kill's frame, the client's projectiles cleared, input ignored, no respawn, the physics stopped 300 frames later (`physics_stops_at_exactly_three_seconds_like_olx`) |
| Deathmatch (`olr-modes`): kill bookkeeping and texts (first blood, killing and dying sprees, out), `CheckGameOver` (kill limit, time limit, fewer than two not out), `Winner`/`HighestScoredWorm`/`CompareWormsScore`, the results order, `Lives` and the respawn gate | `src/server/CGameMode.cpp:31-434`, `src/server/CDeathMatch.cpp`, `src/server/CServer.cpp:435-625`, `src/server/CServer_Game.cpp:167-260`, `src/client/Options.cpp:243-253`, `787-822`, `src/common/MathLib.cpp:85-88`, `src/client/CClient_Draw.cpp:1840-1897` | `olr-modes::{Deathmatch, rules, text}` | done (plan 6 task 2) | `deathmatch_game_over_and_winner_like_olx`, `lives_out_removes_worm_from_respawn`, `spree_messages`, `compare_like_olx`, `check_order_like_olx`, `deathmatch_is_deterministic_and_allocation_free`; the oracle scenario `dm-kill-limit` (KillLimit 2, a double kill: the end and its winner, `go`, match OLX bit for bit) |
| Weapon restrictions (spec §20 F4): `.wps` presets imported into mod packs (`cfg/presets/<mod dir>/`, then `cfg/presets/`), `CWpnRest` (`loadList`'s parsing and path rule, `updateList`, `findWeapon`, `isEnabled`, `getWeaponState`), natural bonuses from the weapons not banned (`goodWpns`) | `src/common/CWpnRest.cpp:59-277`, `include/CWpnRest.h:28-32`, `src/game/Game_loadWeaponRestrictions.cpp:16-28`, `src/server/CServer_Game.cpp:292-297`, `src/game/Game.cpp:1405-1420`, `src/common/CGameScript.cpp:1211-1231` | `olr-import-lgs::{parse_wps, import_presets}`, `olr-data::restrictions`, `olr-pack` (`restrictions.ron`, format 3), `olr-content` (`LoadedMod::weapon_rules`, `SandboxSetup::restrictions`), `olr-scenario` (the header's `WeaponRestrictionsFile`) | done (plan 6 task 3) | `wps_presets_enable_what_olx_enables` (every bundled Classic and MW 1.0 preset), `parses_like_load_list`, `lookups_follow_cwpnrest`, `natural_bonuses_hold_what_the_restrictions_allow`; the oracle scenarios `bonus-natural` (`Bonus only`) and `bonus-natural-default` (`Standard 100lt`, `natural_bonuses_use_default_restrictions`) match OLX bit for bit |
| Bot loadouts (spec §20 F1): `initWeaponSelection`'s presets (rifles, 100lt, mortars by mod name, `LoadingTime` and restrictions), `GetRandomWeapons`, `getRandomEnabledWpn`; the bot's own `rand()` stream | `src/common/CWormBot.cpp:4129-4192`, `src/common/CWorm.cpp:788-844`, `src/game/Game.cpp:1405-1420`, `src/common/MathLib.cpp:71-81`, `include/Utils.h:38-43` | `olr-bots::{loadout, BotRng}` | done (plan 6 task 3) | `bot_presets_match_olx`, `random_weapons_follow_restrictions` |
| Bot navigation, motor and aim (spec §20 F2, F3, F7): `getMaxFreeArea`, `searchpath_base` (checklist, passage rule, costs, worm-line goal test, `buildPath`, `simplifyPath`, `splitUpNodes`), `traceWormLine`/`fastTraceLine`/`simpleTraceLine`, the node advance of `AI_MoveToTarget`, `AI_SetAim`, `AI_GetAimingAngle`, `AI_Carve`/`AI_Jump` timers, `isJumpingGivingDisadvantage`; the difficulty numbers | `src/common/CWormBot.cpp:110-878`, `1022-1122`, `1651-1713`, `1896-1960`, `2834-2853`, `3304-3366`, `3578-3627`, `src/level/FastTraceLine.h:24-104`, `include/MathLib.h:140-153`, `include/CVec.h:157-185` | `olr-bots::{nav, motor, aim, Bot, Budget}` | done (plan 6 task 4; the brain is task 5) | `areas_grow_like_olx`, `search_finds_paths_and_resumes_across_ticks`, `motor_reaches_target_angle_like_olx_turning`, `motor_switches_weapons_in_two_ticks_per_slot`, `ballistic_hits_static_target`, `think_stays_within_budget`, `bots_are_deterministic`, `think_does_not_allocate`, `think_never_panics_on_odd_worlds` |
| Bot brain, weapons, rope and chaos (spec §20 F2, F10): `getInput` (THINK ↔ MOVINGTOTARGET, re-thinking every 3 s, every 0.5 s while a search runs and at once on the target's death, the 0.4 s spawn pause, `AI_Carve`), `AI_Think` (critical health, weapon bonus, nearest enemy, health, a random high spot), `AI_CreatePath` (restart after 5 s or when worm lines break, path extension, 0.5 s between searches), `AI_Shoot` (`traceWeaponLine`, clearing weapons through dirt above, lead, feasibility, aim by distance, `weaponCanHit`, bad-aim jumps), `AI_GetBestWeapon` (buckets, change delay; 100lt, rifle and mortar tactics), `AI_ReloadWeapons`, `AI_FindClearingWeapon`, `AI_MoveToTarget` (rope timers, best rope spot, re-throws, pulling away, overshoot, dirt above, dirt ahead, jetpack, rifle reloading, mortar retreat, worms within 50 px, stuck handling), `AI_SimpleMove`, `AI_FindShootingSpot`, `isMoveGivingDisadvantage`; the chaos personality and Deathmatch's chaos rotation | `src/common/CWormBot.cpp:993-1122`, `1180-1644`, `1719-1893`, `1963-2809`, `2869-2983`, `3200-3366`, `3370-4034`, `4120-4126` | `olr-bots::{brain, moving, shoot, weapons, rope, spot, chaos, probe}`, `olr-modes::{RespawnLoadout, ChaosCycle}` | done (plan 6 task 5) | `xtreme_bot_kills_static_target_quickly`, `easy_bot_hits_less`, `bots_use_the_rope_to_climb`, `stuck_bots_stay_within_budget_and_match_ends`, `bots_survive_odd_loadouts`, `chaos_fires_every_weapon_over_a_match_grid`, `chaos_cycle_gives_the_rotation_on_every_spawn`, `bot_matches_are_deterministic_and_allocation_free`, `olx_direct_aim_still_fights`, `a_search_toward_a_moving_target_finishes`, `the_jump_check_is_never_starved`, `a_scan_cut_short_does_not_go_back` |
| Bot aim error floor (the user's direction, plan 6 ruling R6): every difficulty's aim errors (offset, target, gravity and speed noise) scale with at least 1/6, so Xtreme keeps half of Hard's error; the fire tolerance stays OLX's | `src/common/CWormBot.cpp:1682-1706`, `1836-1850` (OLX's Xtreme has none) | `olr-bots::{Difficulty::params, AIM_ERROR_FLOOR}` | done (plan 6 task 9; a deviation, below) | `no_difficulty_aims_perfectly`, `weapon_can_hit_noise_scales_with_the_level`; kill rates measured before and after (below); goldens re-blessed (`fixtures/blessings.log`) |
| Replays (spec §20 F8; no OLX equivalent): the setup, pack ids and content hashes, every worm's keys per tick, the viewer's slot (the player whose game made it), each worm's name, colour and skin (format 4, 2026-10-17; a viewer draws a skin only by id and content hash, never downloaded), checksums at tick 0, every 100 ticks and the end; the `OLRP` format, version refused when unknown | — | `olr-replay::{Recorder, Replay, Player, Cursor}`, `olr-content::{replay_packs, world_for_replay, play_replay}` | done (plan 6 task 6) | `record_then_replay_gives_identical_checksums`, `replay_reports_the_first_mismatching_tick`, `replay_reports_a_different_setup_at_tick_0`, `replay_refuses_unknown_format_versions`, `replay_refuses_mismatched_content`, `bot_match_replays_from_content`, `replay_size_is_reasonable`, `decode_never_panics_on_junk`, `a_failing_tick_is_recorded_without_a_checksum`, `the_viewer_is_recorded`, `the_viewer_is_one_of_the_worms`, `the_looks_round_trip`, `looks_are_sanitised_and_bounded`, `a_format_3_replay_is_refused`, `the_replay_carries_the_lobby_looks`, `replay_session_matches_recorded_checksums` (names, colours, skin fallback) |
| Headless match runner (spec §7.1, §7.3; no OLX equivalent): `olr-cli match` and `replay`, one match builder shared with `olr-game` (plan 6 ruling R5), invariants every tick, tick-time percentiles, determinism and replay round trip, coverage (weapons, projectile actions, bonuses, rope), failure capture to `target/failures/`, golden checksum streams | — | `olr-cli::{spec, runner, verify, coverage, failure, golden, invariants}`, `olr-content::build_match`, `xtask bless-regression` | done (plan 6 task 7) | `classic_deathmatch_bots_are_deterministic_and_replay`, `mw_chaos_bots_are_deterministic_and_replay`, `failure_writes_replay`, `golden_match_checksums` (`fixtures/matches/`); in `cargo xtask full`: `coverage_report_lists_every_weapon` (both mods × 4 levels × 3 seeds: every weapon of both mods and every projectile action kind) and `long_bot_match_bounded` (30 minutes, review focus 5) |
| Deathmatch against bots in the game: `--mode deathmatch`, the bots thinking on the main thread before each tick, the HUD (names, event log, time-limit clock, Tab's scoreboard), "Game Over" then the results after `GAMEOVER_WAIT`, F5's rematch, `--record` and `--replay` | `src/client/CClient_Draw.cpp:893-905`, `1840-1897`, `2211-2236` | `olr-game::{setup, session, drive, hud, panels}` | done (plan 6 task 8) | `deathmatch_session_ends_with_results`, `rematch_starts_without_the_last_ticks_events`, `bots_think_within_frame_budget`, `replay_session_matches_recorded_checksums`, `scoreboard_hides_when_the_window_loses_focus`; the user's playtest |
| Settings and profiles files (spec §9.4, §21 G2): versioned RON in the platform data directory, atomic writes, bad or newer files set aside as `.bak` with defaults and a warning | OLX's `cfg/options.cfg` (`src/client/Options.cpp`) and `cfg/players.dat` (`src/client/ProfileSystem.cpp`), laid out anew | `olr-ui::{store, settings, profiles, match_prefs}` | done (plan 7 task 1) | `bad_user_files_fall_back_to_defaults_and_keep_a_backup`, `settings_round_trip_and_version` |
| Menus (§21 G1): main menu (Local Play, Multiplayer, Editor, Profiles, Options, Replays, About, Report a bug with a relay, Quit; its Sandbox button removed in issue #33: Local Play's Idle bots do it, and the engine's sandbox stays for the editors' Play and the command line's start), local play (mod, level grid with thumbnails, OLX's Deathmatch options and their ranges, bots, the profile), the replays list, the screen state machine; starts that fail return to the menu | `src/client/DeprecatedGUI/Menu_Main.cpp`, `Menu_Local.cpp`, `src/common/FeatureList.cpp:31-35` (laid out anew) | `olr-ui::{state, screens::{main, local_play, replays}, replays}`, `olr-game::shell` | done (plan 7 task 2) | `state_machine_navigates_and_emits_commands`, `main_to_local_play_to_match` (no Sandbox), `the_editors_play_starts_the_sandbox`, `local_play_builds_match_options`, `local_play_pickers_are_found_by_their_label`, `local_play_replaces_a_saved_selection_no_longer_imported`, `starting_with_missing_content_returns_to_menu`, `menu_actions_are_idempotent`, `replay_list_shows_header_problems`; the v0.1 acceptance runs (`docs/porting/acceptance-v0.1.md`) |
| Profiles and the skin preview: create, edit, delete, select; name, colour, searchable skins; OLX's menu animation from the game's own colorization and frame cut; the human plays the profile's look, bots OLX's default skin in OLX's `cpuColours` | `src/client/DeprecatedGUI/Menu_Player.cpp:746`, `src/client/ProfileSystem.cpp:47-60`, `src/client/CGameSkin.cpp` | `olr-ui::{profiles, skin_preview, screens::profiles}`, `olr-content::skin_sprite`, `olr-game::shell` | done (plan 7 task 3; bot colours task 5) | `profile_edits_persist`, `profile_names_are_validated`, `skin_preview_uses_colorization_like_the_game`, `the_profiles_screen_gets_its_preview_textures`, `a_menu_match_plays_the_selected_profile_look` |
| Options (§21 G3): window presets and fullscreen through the paced path, VSync, filter, interpolation, UI scale; volumes live; key and gamepad rebinding with explicit conflicts, never leaving an action unreachable (the match is set up in local play only) | `src/client/DeprecatedGUI/Menu_Options.cpp`, `src/client/Options.cpp:45-73` (laid out anew) | `olr-ui::screens::{options, keys}`, `olr-input::Capture`, `olr-game::{video, window}` | done (plan 7 task 4) | `rebinding_never_leaves_an_action_unreachable`, `options_rebind_a_key`, `options_set_video_and_audio`, `video_changes_go_through_the_paced_path`, `volume_changes_apply_live`, `the_saved_window_opens_with_the_flags_on_top_unsaved`; e2e (`fixtures/autoplay/menus.ron`: quick window-size changes) |
| Window changes on Wayland (v0.1 playtest; no OLX equivalent): a size or mode change makes the window again at its new shape on the same device, so WSLg leaves no stale frame; X11 changes it in place | — | `olr-game::{window, gpu, app}` (`Plan`, `Shape`, `Gpu::rebind`, `App::recreate`) | done | `a_shape_says_when_the_window_shows_a_change`, `wayland_makes_the_window_again`, `a_window_made_again_keeps_the_game_ticking`; real-screen captures under WSLg (1280 × 720 → 1920 × 1080 → back; the desktop's size → fullscreen → back) and the options' autoplay on Wayland |
| Weapon selection (§21 G4): the profile's slots by name, `findEnabledWeapon`, left/right over the enabled weapons (×6 with Select weapon), Random, Done on Fire's release, skipped with one weapon or none; the choice in `MatchOptions` and the replay | `src/common/CWormHuman.cpp:702-1000`, `src/common/CWorm.cpp:788-844` | `olr-ui::{weapon_select, screens::weapon_select}`, `olr-content::Player::HumanWith`, `olr-game::shell` | done (plan 7 task 5) | `weapon_selection_respects_restrictions`, `a_refused_choice_leaves_the_match_playable` |
| HUD and minimap (§21 G5, Modern only): an opaque bar along the bottom fifth of the window's 16:9 frame (OLX 0.58's status bar; deviation below) with health, five weapon slots with OLX's bar states, the minimap in the centre (OLX's proportions, colours and dots; no view rectangle, the user's ruling: the player's bigger dot is enough) and the event log; clock and kill limit over the playfield, Tab's scoreboard; the game drawn above the bar | `src/client/CClient_Draw.cpp` (HUD, `:137-138` the local game's minimap, `:483-515` the bar (commented out at `d1ad953f`), `drawOnMiniMap`), `docs/porting/olx-notes/presentation.md` §2.4, §9 | `olr-ui::{hud, theme}`, `olr-game::{hud, app, gpu}`, `olr_scene::Viewport::GAME`, `olr_render::Renderer::render_in` | done (plan 7 task 5; the bar, the 16:9 frame and the side layout after the v0.1 playtest) | `hud_layout_holds_at_all_scales` (both layouts), `the_scale_follows_the_height`, `the_game_view_is_fixed`, `the_playfield_is_the_frame_above_the_bar`, `hud_paints_at_every_scale`, `log_keeps_the_last_lines_oldest_first`, `hud_hides_the_weapons_during_the_selection`, `names_stay_inside_the_view`, `hud_snapshot_1080p`, `hud_snapshot_1440p_with_scores`, `hud_snapshot_3440x1440_side_panels` (GPU tier) |
| Pause and results: Esc's pause menu (no tick while paused), the results after `GAMEOVER_WAIT` with OLX's columns plus score, deaths and suicides; Rematch, Save replay, Back to menu | `src/client/CClient_Draw.cpp:1840-1897` | `olr-ui::screens::{ingame, results}`, `olr-game::shell` | done (plan 7 task 5) | `pause_stops_ticks_and_replays_stay_valid`, `pause_menu_holds_the_game_and_opens_the_options`, `results_match_world_scores`, `results_show_the_columns_and_their_buttons`, `save_replay_once_per_match` |
| Level thumbnails (§21 G6, narrowed): `thumb.png` per level pack (160 × 96, box-filtered, letterboxed), pack format 4 (5 since plan 13c), left out of the content hash; decoded lazily and bounded in the menus | `src/game/Level.cpp:33-44` (`minimapForLevel`) | `olr-import-gfx`, `olr-pack`, `olr-ui::screens::local_play` | done (plan 7 task 6) | `thumbnails_are_imported_for_every_level`, `thumbnails_are_letterboxed_to_a_fixed_size`, `thumbnails_average_the_art`, `thumbnails_decode_lazily_and_bounded` |
| About (§21 G9): credits (Liero, Liero Xtreme, the OpenLieroX team) and licenses; the list of each pack's recorded origin (mods, levels, themes, the core, skins) was removed on 2026-10-24, the maintainer's call | `src/client/DeprecatedGUI/Menu_Main.cpp:276-283` | `olr-ui::screens::about` | done (plan 7 task 6) | `about_credits_the_authors_and_the_licenses` |
| Import (the Import screen was removed in plan 13c; `olr-cli import`, and later the editors, import OLX content into `packs/`, layered over the bundled packs): bounded reads and image decodes | `Menu_Local_FillModList`, `checkLXBinMod`, OLX's level list, `src/main.cpp:1003-1021` | `olr-import`, `olr-pack::image_io`, `olr-game::shell` (`packs_dir`) | done (plan 7 task 7; the screen removed in plan 13c) | `import_reports_every_item_and_writes_whole_packs`, `imported_packs_override_bundled`, `user_packs_layer_over_the_bundled_content`, `a_decompression_bomb_is_refused_on_import_and_load`, `huge_images_are_refused_before_decoding` |
| Fast-forward (the user's request; no OLX equivalent): replays and `--spectate` at 1×–16× or max, a played game always 1×, sound thinned | — | `olr-game::speed` | done (plan 7 task 8) | `fast_forward_never_changes_the_simulation`, `a_played_game_never_runs_fast`, `speeds_step_parse_and_thin_sound`, `the_fake_clock_needs_an_autoplay` |
| Autoplay and end to end (§21 G7): `--autoplay` clicks widgets by label, holds the game's keys, takes checked screenshots and logs JSON lines; `--fake-clock` makes a run deterministic | — | `olr-game::autoplay`, `xtask e2e` | done (plan 7 task 8) | `scripts_parse_and_name_what_exists`, `steps_click_wait_and_fail_in_time`, `widgets_are_found_by_their_label`, `blank_pictures_fail_the_sanity_check`; `cargo xtask e2e` (in `full`, Xvfb) |
| Benchmarks (§21 G8): a full 3000-projectile pool tick, 32 worms, 8 bots thinking, the draw list, an offscreen frame at 1080p and 1440p; machine-local baselines, more than 10 % slower fails | — | `olr-sim/benches/tick.rs`, `olr-cli/benches/game.rs`, `olr-render/benches/frame.rs`, `xtask bench` | done (plan 7 task 8) | `fixtures/bench/baselines.ron`; `regressions_need_more_than_the_tolerance`, `baselines_from_another_machine_only_notice` |
| Engine stress test (the user's request): 32 chaos bots on Castle Strike, both mods, OLX's pool and 20 000; tick, think, pool, entities, events, render and memory; invariants every tick, determinism over two runs | — | `olr-cli stress`, `xtask stress` | done (plan 7 task 8) | `stress_run_is_bounded_and_deterministic`; `fixtures/bench/stress.ron` (in `full`) |
| Performance envelope (plan 8): sweeps of worms, pool, effects, map and art; the GPU on WSL GL and Windows DX12/Vulkan with timestamps and the shader probe; OLX frame timing | `tools/olx-oracle` (`ORACLE_TIMING`) | `xtask/src/envelope/`, `olr-cli gpu-bench`, `olr --frame-log` | measured | `fixtures/bench/envelope/<host>.ron`, `docs/perf/envelope.md` |
| v0.1 acceptance (spec §1.3): each of the 4 levels × 2 mods from the menus against 4 Medium bots for 60 s in a real window, timings and screenshots | — | — | done (plan 7 task 9) | `docs/porting/acceptance-v0.1.md`; `cargo xtask check` and `full` |
| Players leaving mid-match (plan 12d; user decisions, 2026-10-15): a player who leaves, times out or is kicked is withdrawn from the match at one tick on every peer, its worm gone (`CGameMode::Drop`, a kill credited to it credits nobody), fewer than 2 worms left ends the match, "<name> has left" / "<name> has timed out", the live scoreboard leaves it out and the results keep a "left" row (deviation below) | `src/server/CServer.cpp:1352-1482`, `src/server/CGameMode.cpp:254-263`, `316-336`, `src/client/CClient_Parse.cpp:1650-1680`, `src/game/Game.cpp:1359-1366`, `src/client/Options.cpp:769-770` | `olr-sim::World::withdraw`, `olr-modes` (`on_drop`), `olr-replay` (format 3), `olr-proto`/`olr-netcode` (the marks), `olr-lobby`, `olr-online`, `olr-ui`, `olr-game` | done (plan 12d) | `a_withdrawn_worm_leaves_the_board_and_the_last_one_wins`, `a_withdrawn_worm_never_wins`, `a_projectile_of_a_worm_that_left_credits_nobody`, `a_gone_worms_kill_credits_nobody`, `a_withdrawal_replays_at_its_tick`, `a_withdrawal_after_the_last_tick_replays`, `withdrawal_marks_are_bounded`, `a_withdrawal_mark_is_final`, `a_withdrawal_learned_late_rolls_back_to_it`, `a_leaving_client_is_withdrawn_at_one_tick_everywhere`, `a_leave_during_a_migration_is_withdrawn_alike`, `a_forged_withdrawal_never_removes_a_present_player`, `a_rejoined_player_marked_by_the_old_host_stays_and_its_holders_are_repaired`, `a_repaired_holder_that_hosts_later_withdraws_the_player_at_its_leave`, `a_member_that_times_out_is_said_to`, `the_last_opponent_leaving_ends_the_match`, `a_player_leaving_during_a_migration_is_removed_at_the_resume`, `a_leaving_player_is_removed_from_every_world`, `a_left_row_is_off_the_scoreboard_and_left_in_the_results`, `the_watched_worm_moves_on_when_it_leaves`, `an_online_watcher_moves_on_when_the_followed_worm_leaves`, `a_player_kicked_mid_match_is_withdrawn`, `a_client_crash_excuses_the_fills_stalls_until_its_timeout`, `a_client_leave_withdraws_its_worm_in_a_real_match`, `a_bundle_with_a_withdrawal_reproduces_to_the_fault` |
| Mod authoring format (spec 2026-10-16 formats §2): authoring types, canonical writer, compile to `ModDef`, lift from it | — | `olr-format::mods` | done (plan 13a); the packs' format (plan 13c) | `bundled_mods_survive_lift_write_read_and_compile`, `authoring_values_round_trip_canonically`, `mutated_files_never_panic`, the compile and lift unit tests |
| Level, theme, skin and core-pack authoring formats (spec 2026-10-16 formats §§3-6): authoring types, canonical files, compile to the runtime structs, lift from them | — | `olr-format::{levels, themes, skins, core_pack, image_io}` | done (plan 13b); the packs' format (plan 13c) | `bundled_levels_survive_lift_write_read_and_compile`, `the_bundled_theme_survives_lift_write_read_and_compile`, `the_bundled_core_pack_survives_lift_write_read_and_compile`, `every_bundled_skin_draws_the_same` |
| Pack tooling (spec 2026-10-16 formats §6): `olr-cli pack check\|fmt\|new`, the level operations and preview, the schema docs | — | `olr-edit`, `olr-cli` (`pack_cmd.rs`, `level_cmd.rs`), `xtask format-docs` | done (plan 13d) | `load_then_files_reproduces_every_bundled_pack`, `bundled_packs_are_canonical`, `the_background_invariant_holds_at_every_art_scale`, `every_guide_example_parses_and_is_canonical`, `format-docs` check step |
| Level composition (spec 2026-10-17 editor §1-3, §7): `editor/composition.ron` (issue #74: the ground asset's `editor/asset.ron`, placed by `editor/placed.ron`; `olr-cli level convert` converts the old file) of image, drawn and raster elements, baked into `material.png`, `front.png`, `back.png` and `thumb.png`; JPEG originals; `olr-cli level element …`, `level bake`, stale bakes in `pack check` | — | `olr-format` (`levels::composition`, `image_io::decode_image_within`), `olr-pack` (`is_authoring`), `olr-edit` (`pixelate`, `compose`, `element`), `olr-cli` (`element_cmd.rs`), `olr-detcheck --bake` | done (plan 14a) | `every_bundled_level_round_trips_through_a_raster_base`, `region_bakes_equal_the_full_bake_and_keep_the_invariant`, `editor_files_are_not_content`, `the_fixture_is_baked`, `cargo xtask determinism`'s bake digest |
| Level editor core (spec 2026-10-17 editor §4-§5, §8): the document (elements decoded, an image cache, one bake), reversible edits and a 256 MiB history, the tools (select, brush, eraser, rectangle, ellipse, line, spawn, eyedropper), the panels and shortcuts, tested headless; hosted in the game (the Editor screen, the canvas drawn by `olr-render`, Play, the file browser, recovery, `olr --edit`) | — | `olr-format` (`image_io::jpeg_orientation`, `levels::check_elements`), `olr-edit` (`level::{draw_art, erase_art, rescale_art, Shape::{Ellipse, Line}}`, `compose::{Decoded, ImageCache, bake_elements}`, `new::pack_id`), `olr-editor` (`browser`), `olr-terrain` (`ColorMap::replace_rect`), `olr-content` (`LoadedLevel::{from_source, first_spawn}`, `build_match_spawning`), `olr-ui` (`screens::editor`), `olr-game` (`editor`) | done (plans 14b-14c) | `undo_and_redo_restore_everything_exactly`, `every_operation_undoes_to_the_same_files`, `opening_and_saving_changes_nothing`, `art_painting_is_clipped_and_total`, `a_brush_stroke_is_one_step`, `locked_elements_are_not_changed_by_the_canvas`, `history_drops_the_oldest_steps_over_the_cap`, the `flows` tests; plan 14c: `a_bundled_level_is_edited_as_a_copy`, `recovery_survives_a_missing_original`, `a_damaged_recovery_is_discarded_not_fatal`, `a_later_session_keeps_the_crash_copy`, `imports_are_bounded_before_reading`, `closing_the_window_asks_when_unsaved`, `quitting_without_asking_keeps_a_recovery_copy`, `the_first_spawn_is_used_only_when_inside_the_level`, `the_canvas_draws_the_bake`, the `browser` tests, `cargo xtask e2e`'s editor run |
| Skin editor (issue #14; no OLX counterpart): the Editor screen's Skin tab (New blank skin, Edit yours, Edit a copy), the 21-frame grid, onion skin, frame copy, paste to a frame, row or column, flip and shift, the paint tools (pencil 1 and 2 × 2, eraser, fill, eyedropper, line, rectangle) on the Colour, Player colour and Glow targets, the colour picker and recent colours, undo and redo per stroke, zoom and pan, the skin light, Make double resolution, a live preview (walking, aiming, both ways, light and dark), saving to `packs/` | `src/common/CWorm.cpp:1225-1244` (the preview's frame) | `olr-skin-editor`, `olr-edit` (`skin::preview_frame`), `olr-ui` (`screens::editor`'s Skin tab), `olr-game` (`editor::skin`) | done (issue #14) | the `olr-skin-editor` `state` and `flows` tests, `a_preview_frame_is_the_previews_cell`, `the_skin_tab_edits_yours_and_copies_the_others`, `a_copy_is_saved_to_your_packs_and_offered_by_the_skin_picker`, `a_bad_skin_pack_is_refused_with_a_message`, `closing_the_window_asks_when_unsaved` (skin) |
| Mod editing operations (issue #16; no OLX counterpart): get and set any field by path (through shared projectiles), weapons added (blank or from another mod with their projectiles and files), removed, moved, copied and renamed with the presets and loadouts following, shared projectiles listed, inlined and shared, every error located; `olr-cli mod` | — | `olr-format` (`mods::mod_errors`), `olr-edit` (`mods`), `olr-cli` (`mod_cmd.rs`) | done (issue #16) | `set_changes_exactly_the_fields_set`, `every_leaf_round_trips_through_set`, `a_cycle_and_two_effects_in_an_event_are_refused_with_a_path`, `weapons_add_remove_move_copy_and_rename`, `a_weapon_of_another_mod_brings_its_projectiles_and_files`, `used_by_matches_the_files`, `inline_and_share_round_trip`, `check_lists_every_error_with_its_path`, `mod_errors_lists_every_error_and_compile_mod_the_first`, `olr-cli`'s `tests/mods.rs` |
| Mod editor (issue #17; no OLX counterpart): the Editor screen's Mod tab (Edit yours, Edit a copy of any mod as a new mod, recoveries), the left list (World, the weapons in order, the shared projectiles), the world's fields, presets and shipped loadouts, a weapon's fields and fire, the projectile tree (shot, projectile, event spawns, trail, timer; shared ones with their users, Make a copy for this weapon, Make it shared), events as one effect and a spawn, weapons added (blank or copied from any mod), removed, moved, duplicated and renamed, undo and redo, the check before saving with each problem a link, the base mod's values with Reset, recovery copies | — | `olr-format` (`mods::ModEditorFile`, `editor/mod.ron`), `olr-pack` (it is read), `olr-edit` (`mods::set_lenient`), `olr-mod-editor` (`fields.ron` from `xtask format-docs`), `olr-ui` (`screens::editor`'s Mod tab, `Store::mod_recovery_dir`), `olr-game` (`editor::mods`) | done (issue #17) | the `olr-mod-editor` `table`, `state` and `flows` tests, `a_lenient_set_keeps_the_error_and_the_save_refuses_it`, `the_mod_editors_file_is_read`, `the_field_table_keeps_the_hand_columns`, `the_mod_tab_edits_yours_and_copies_any`, `a_new_mod_is_a_copy_saved_to_your_packs_and_edited_again`, `unsaved_work_is_kept_in_a_recovery_copy`, `format-docs` check step |

## Deliberate deviations

- **Rope is `Z` and Strafe `X` by default.** OLX's player 1 has them the
  other way round (`src/client/Options.cpp:45-51`); the maintainer's
  choice. A saved options file keeps whatever keys it holds.

- **Not a deviation: the skin importer hides a key-colour pixel under a
  tint mask.** OLX's `SetColorKey_Alpha` gives key-coloured pixels alpha 0
  before `Colorize_Execute`, which keeps alpha, so OLX hides them as the
  importer does (corrected by plan 13c). Mask-alpha edge cases that differ
  from OLX and are left as they are: OLX tests a black mask by RGB only, so
  a black mask pixel with alpha 0 still copies its pixel, and it treats any
  non-opaque non-black mask pixel as transparent; the importer hides a pixel
  only when the mask alpha is 0. Partial image alpha is dropped (format 5
  cannot express it either).

- **`LevelDef::spawns` is not read by the runtime.** Spawns are
  `FindSpot`'s; `LevelObject::Spawn` in the level format is not read by
  the runtime yet.

- **Left-facing skin frames mirror the right-facing frame exactly.** OLX
  draws the source columns one to the right (`src/client/CGameSkin.cpp:584-601`),
  showing a column of the frame's spacing and losing one of the sprite.
  Dropped 2026-10-16 (the user's ruling); render goldens re-blessed.
  `olr_content::skin_source`; `left_is_the_mirror_of_right`,
  `every_bundled_skin_mirrors`.

- **Results keep a "left" row** (N85, user decision 2026-10-15): OLX
  deletes a leaving client's worm (`Game::removeWorm`,
  `src/game/Game.cpp:1359-1366`), so its results lose it; ours keep its
  row, its lives "left", its kills and deaths as it left (greyed). The
  live scoreboard leaves it out, as OLX.
- `olr-sim` supports LX56 aim only (`ForceLX56Aim`). It rejects wrap-around maps
  (`InfiniteMap`), Gusanos worm physics, `WormFriction > 0` and
  `ProjFriction > 0` (`SimSettings::from_olx_features` errors).
- Projectile actions assume a deathmatch (no team game): `TeamHit` is not
  read, an "enemy" is any other worm and there are no team mates.
- The green-dirt mask, OLX's shared `data/gfx/greenball.png`, is imported
  into the theme pack (optional `green_ball.png`). Without it green dirt
  places nothing, as in OLX.
- `olr-sim` does not keep the weapon name an injury passes on (OLX uses it only
  for the kill message).
- `olr-scenario` refuses a mod pack with an image projectile whose image is
  missing; OLX would run it without an image (no `ANI_ONCE` end, the junk rule).
- `olr-sim` rejects settings outside the ranges OLX's feature list gives
  (`src/common/FeatureList.cpp`), and NaN or infinite values
  (`SimSettings::validate`). Outside them the port could overflow, hang or
  recurse without bound. For hand-built settings, the physics step is at least 1 ms.
- **Each mod's worm and rope physics (fixed 2026-10-16).** OLX copies five
  worm values (ground and air speed, gravity, jump force, air friction) and
  three rope values (max and rest length, strength) from the mod into its
  bottom settings layer, `lx56modSettings` (`src/common/CGameScript.cpp:798-817`,
  `src/game/Game.cpp:477`), which the "LieroX 0.56" preset does not override.
  Until this fix every game played Classic's (`SimSettings::lx56`), so MW 1.0
  had gravity 100, jump -65, rope rest 27 and strength 4.5 instead of its own
  50, -75, 30 and 3.5. `olr-content::match_setup` now applies them
  (`SimSettings::with_mod_physics`) for local, bot and online matches; replays
  record the result. The oracle was never affected: a trace's settings come
  from its header. Classic's values equal `lx56`'s, so only MW's match golden
  changed.
- **Game over** (`GameServer::GameOver` and its gates) is in `olr-sim`; when
  a match ends is the mode's (`GameMode::is_over`, OLX's
  `CGameMode::CheckGameOver`: Deathmatch in `olr-modes`). The scenarios
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
  lives, kills and damage). `olr-sim` keeps them on the worm
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
  `SimSettings::from_olx_features`: the oracle, `olr-scenario`, `olr-cli
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
    where the previous one was. `olr-cli`'s `tests/hits.rs` checks, through
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
    `olr-cli`'s `tests/deaths.rs` checks every kill of seeded bot matches.
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
  limit to keep, and graphics fidelity will rise. In play (`olr_fx::Fx::for_match`
  without `olx_quirks`) the pool holds `PLAY_ENTITIES` (16384), allocated
  once at match start (about 1 MB), and its last `DEATH_RESERVE` (1024)
  slots take only a death's gibs and blood (`Event::death`): other
  spawns and blood drips stop at 15360, so a death always shows (157
  entities; the reserve holds six at once). The hard cap stays, with
  OLX's drop-when-full beyond it, for hostile mods. With `olx_quirks`
  (scenarios, render goldens) the pool is OLX's 1024 with no reserve. The
  simulation's own entity counter (`olr-sim`'s `rng.rs` `MAX_ENTITIES`,
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
- **A reload bar over one's own worm (2026-10-18).** OLX shows a
  weapon's reload only in the HUD's slots. The user's request: under the
  player's own worm's name, while its current weapon reloads, a bar
  filled to its charge (`olr-game`'s `overlay::reload_bar`); other worms'
  reload stays hidden, as in OLX. Overlay only: no simulation change.
- **A gib stamps only rock and dirt (2026-10-18).** A slow gib that
  lands stamps its 4 × 4 frame around the landing pixel
  (`CMap::putSurfaceTo`, `src/client/Entity.cpp:373-389`), and OLX paints
  the open air in that box too; digging clears only dirt, so those pixels
  float for the rest of the match (the playtest's "blood and remains stuck
  in the air": in a 4-bot Classic chaos match on Dirt Level, 62 cells
  after 3000 ticks, all gib colours). In play (`Fx::for_match` without
  `olx_quirks`) `ColorMap::stain_image` paints only rock and dirt; with
  `olx_quirks` (scenarios, render goldens) it is OLX's. Blood already
  stained only the pixel it hit, rock or dirt. Test: `olr-cli`'s
  `no_effect_leaves_colour_on_open_air`.
- **A death sound steals a voice (2026-10-13).** OLX skips a sound
  beyond its `maxplaying` (two of each death sound,
  `src/game/Sounds.cpp:25-30`, `src/sound/sfx.cpp:120-134`) or when no
  OpenAL source is free, so deaths close together could go unheard. The
  user's ruling: every death plays its sound. `olr_audio::Voices::admit`
  answers start, steal or skip: a death sound at its own limit takes the
  place of the oldest voice of that sound; at the global limit (256) the
  oldest of that sound, else the oldest voice that is not a death's
  (push order, deterministic). `Audio::play` takes the voice only once
  the new one has started, and stops it with a 5 ms fade. Every other sound is skipped as in OLX. Audio never runs
  in the oracle, so it applies always.
- **Exact-zero deaths' Gusanos particles (spec §18 D9).** Three scenarios stay
  pinned in `olr-scenario/tests/oracle.rs`, the only ones that do not match
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
- **Blood amount and particles:** `olr-sim` uses OLX's defaults, `Game.Blood`
  100 and `Game.Particles` on (blood entities exist), which dedicated servers
  keep; `olr-scenario` refuses a trace header with other values. The
  player's own blood amount is the effects layer's (next entry).
- **A blood amount per player, in the effects layer (issue #6,
  2026-10-21).** OLX lets each player set how much blood they see
  (`Game.Blood`, `tLXOptions->iBloodAmount`), and that setting changes how
  many `rand()` draws a hit, a death and bleeding make
  (`CClient_Game.cpp:414-459`, `PhysicsLX56.cpp:319-329`;
  `olx-notes/randomness.md` rows 1, 5, 6), on the stream gameplay shares.
  OLR's simulation always makes OLX's 100 % draws (`olr-sim`'s
  `BLOOD_AMOUNT`, bleeding's 20 rounds), so the oracle stays bit-exact and
  online peers stay in sync; the player's **Blood** (Options → Video,
  0-500 % in steps of 10, 100 by default; `VideoSettings::blood`) scales
  only what `olr-fx` makes of the simulation's cosmetic events
  (`Fx::set_blood`, through `olr_present::Presentation::set_blood`): with
  `s` the amount, `Blood` events `s` times as many, `BloodDropper` events
  `min(s, 1)` (never multiplied), a dropper's drops `max(s, 1)` (none at
  0 %), so bleeding shows `s` times its blood as hits and deaths do (5× at
  500 %, not 25×). Fewer are spread evenly by accumulators (50 %: every
  other one; 0 %: no blood, droppers or stains); more are copies with
  velocities jittered by a generator of the amount's own, never the
  simulation's nor the other entities' (gib spins and drips draw as
  before). Copies spawn only below `Fx::extra_blood_limit`, an eighth of
  the pool (`EXTRA_BLOOD_HEADROOM`, 2048 in play) under the ordinary
  limit: extra blood is refused before ordinary effects, never takes the
  death reserve, and with OLX's 1024 pool (`olx_quirks`: render goldens,
  `olr-cli scenario`) none spawns at all. Stains come from landed blood, so
  they scale with it; online, the amount scales the cosmetics
  `NetPresenter` hands the entities, as local play's. **The deliberate
  exception** to the ruling of 2026-10-16 that every player in a match
  sees the same picture: the maintainer accepts a per-player setting for
  blood, as the information blood carries is small and comfort matters
  (less gore, none while streaming, or more mess). Replays, the netcode
  and the world never carry it. At 100 % nothing differs from before
  (render goldens, HUD snapshots, match goldens unchanged). Tests:
  `olr-fx`'s `tests/blood.rs`, `fx_never_touches_sim_randomness_at_500_percent_blood`;
  `olr-present`'s `the_blood_amount_reaches_the_entities`; `olr-game`'s
  `the_blood_amount_applies_at_once_and_never_to_the_world`. The frame
  time at 500 % in a heavy fight is still to measure
  (`docs/deferred-verification.md`).
- Only deathmatch: no team protection in `InjureWorm`, no teamkills, no
  `FindSpotCloseToTeam`, and LX levels have no predefined spawn points.
- **Weapon restrictions:** a `WeaponRestrictionsFile` that starts with `cfg/`
  (a path OLX opens as is) finds no imported preset, so every weapon is
  enabled, as OLX does when the file is missing. Mod packs carry only the
  presets OLX finds for the mod (`cfg/presets/<mod dir>/*.wps`, then
  `cfg/presets/*.wps`).
- **Bots draw from their own `rand()` stream** (`olr_bots::BotRng`, seeded from
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
- **Bots press keys** (spec §20 F3; `olr_bots::Motor`): the aim turns through
  UP/DOWN at about 100°/s with 0.2 s to speed up and to brake (60° takes
  0.8 s; OLX's bot sets `fAngle` at 91°/s, 0.66 s, and snaps within 3°);
  turning around takes one step of walking; carving needs walking (OLX
  carves standing too); a jump is never pressed while the rope is out (it
  would release it); a weapon change takes two ticks a slot and stops
  walking. The aim offset of a difficulty is drawn per bot from its own
  generator (OLX shares one per difficulty between all bots), and the aim is
  good only when the worm also faces the target's side.
- **The bots' brain** (`olr-bots`, spec §5.9: parity not required):
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
  life (`olr_modes::RespawnLoadout::ChaosCycle`, by `give_weapon`), and the
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
  (`olr-modes`); `olr-sim` sends none itself (the `GameOver` event carries
  the winner).
- **Deathmatch's texts and sounds** (`olr-modes`): the kill text's weapon is
  always unknown (-1; OLX then drops " with <weapon>"), as `olr-sim` does not
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

- **`SimMod` limits** (`olr-sim::mod_rt`): a pack's projectile tree may have at
  most 4096 occurrences after expansion and 128 levels, and a spawn amount at
  most 1000 (Classic and MW 1.0 use at most 150); a missing reference is an error. OLX loads anything and
  crashes on a missing projectile. A projectile that spawns an ancestor (a
  cycle, which only hand-made packs can express) refers back to it.
- **Spawn time past frame 819 205** (about 2.3 h): when the decoded shot time is
  later than the server time, OLX falls back to `currentTime - ping / 2`
  (`shoot_t::spawnTime`). The port takes the local client's ping as 0.
- `olr-sim` delivers the shot list at the start of `World::tick`, before
  `serverFrame` advances; that is the oracle's `betweenFrames` moment, and a
  frame's state is recorded before it, as in OLX's trace.

- Importers reject malformed input that OLX would read as garbage: truncated
  files, unknown weapon or projectile types, and nesting deeper than 128 levels.
- A missing projectile image is an import error. OLX would load the mod and draw nothing.
- Imported level art is stored at 1×, because OLX's 2× surfaces are plain pixel doubling.
- **Colour terrain** (`olr-terrain::ColorMap`): the map keeps its own copy of
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
- **Scene** (`olr-scene`, plan 5 task 3):
  - *Interpolation* (E2, the default) draws positions between the last two
    ticks at sub-pixel precision, without OLX's `(int)` truncation; the
    camera follows the blended position. `Interpolation::Latest` is OLX's
    whole-pixel drawing of the latest state. Issue #58: a living worm on the
    ground (`Worm::on_ground`) in either state is blended from that state's
    `y` truncated to a whole world pixel, OLX's `(int)y`; `x` and a worm in
    the air stay exact. OLX's ground contact lets a standing worm's `y` creep
    down about 0.012 px a tick and pushes it back up a pixel once it crosses
    one (`fixtures/oracle/fall`), which OLX's whole pixels never show; drawn
    exactly, the worm and the camera bobbed a pixel. A presentation deviation
    from exact blending that restores OLX's look; the simulation is
    unchanged.
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
- **Entities** (`olr-fx`, plan 5 task 4):
  - *Dirt colour of a bonus chain:* OLX reads an explosion's dirt colour
    before the bonuses it destroys explode, but the sim reports the chained
    explosions (and their carves) before that explosion's particles. A
    particle whose probe is no longer dirt when `olr-fx` reaches it takes the
    theme's `iDefaultColour` instead of the colour before the carve.
  - Entities are drawn at their latest state, not interpolated (E2 applies
    to worms, projectiles and bonuses).
  - A falling entity whose position is not finite is removed (OLX's
    `(uint)` cast of NaN is undefined), so it cannot hold a slot forever.
  - Only the air jump's sparkles exist in the sim (`Color()`, fade 10), so
    the coloured-sparkle drawing (`EntityEffect`) is not ported.
- **Sound** (`olr-audio`, plan 5 task 6):
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
- **Input devices** (`olr-input`, plan 5 task 6):
  - OLX's twin-stick controls (left stick walks, right stick aims
    analogue) are not ported: the sim has 11 digital keys. The left stick's
    X axis presses Left/Right and the right stick's Y axis Up/Down, from
    OLX's 60 % deadzone.
  - *Mouse controls* (issue #24, experimental) are stricter than OLX's
    mouse mode on purpose (the maintainer's rule: the mouse must never be
    an advantage). OLX's mouse aim and twin-stick aim set the angle
    directly (up to 100°/s with no acceleration, `CWormHuman.cpp:280-306`),
    faster off the mark than keys; OLR does not port them. The mouse never
    reaches the simulation: `olr-input`'s controller presses the keys a
    keyboard player could press each tick (Up/Down through the bots' aim
    controller, a one-tick Left or Right without Strafe to turn, Strafe
    with A and D, one Previous/Next press per wheel notch), so the
    simulation, the netcode, replays and the oracle see only keys. The
    simulation gained two read-only queries for it
    (`World::substeps_ahead`, `World::latches`), nothing else. Since
    issue #25 mouse mode has its own bindings (#24's as defaults):
    keys of its own (the keyboard's come back untouched when it is off)
    and the mouse's five buttons and two wheel directions, rebindable in
    Options → Keys; Up, Down, SelectWeapon and Strafe are the
    controller's and take none (the issue named the first three; Strafe
    is the controller's too since #24, so a binding would do nothing).
  - Between ticks, a key released and pressed again is up for one tick and
    down for the next, so the sim sees both edges (OLX's `CInput` reads the
    devices every sub-step and needs no tick latch).
  - *Strafing is always allowed* (issue #23). OLX allows it only when the
    host says so (`isHostAllowingStrafing`, `lx:strafingAllowed`,
    `CServer_Parse.cpp:1515-1520`), from the host's `AllowStrafing` option
    (`Options.cpp:331`, on by default); OLR has no such option, so the sim
    acts as OLX's default.
  - *Dig* (issue #23) is OLR's own action: OLX has no dig key. It presses
    OLX's gesture for the player in `olr-input` (one direction held: a tap
    of the other; none held: both; both or SelectWeapon held: nothing; a
    key down at the last tick waits one tick), so the sim and the wire see
    only the classic keys. Held, it digs again every 0.2 s, the
    rate of OLX's auto-carve (`CWormHuman.cpp:357-394`). Its keyboard
    default `C` and the pad defaults of the four new actions (Strafe X /
    west, Previous/Next weapon the bumpers, Dig B / east) are OLR's: OLX's
    gamepad slot binds none of them.
  - *Weapon 1-5* (issue #48) are OLX's quick-select keys, ported
    (`CWormHuman.cpp:430-440`: each held key selects its slot, the highest
    held winning; after the SelectWeapon change, before Previous/Next;
    not while firing without `WeaponCombos`), with OLX's defaults
    (`1`-`5`, player 2 `6`-`0`, no gamepad input: a pad needs none, the
    only actions so exempt). They take the keys' bits 11-15, where
    olr-input's own Dig bit was; Dig moved into olr-input's 32-bit action
    mask. *A Weapon key beyond the worm's slots does nothing in play*
    (a perceived OLX bug fixed): OLX wraps it (`MOD`, `MathLib.h:85`: with
    3 slots Weapon 5 selects the second); `SimSettings::olx_quirks` keeps
    OLX's wrap. Both counts are 5 (`WeaponSlotsNum`, `MAX_SLOTS`), so only
    a worm with fewer slots tells them apart; unit tests cover it, not
    the oracle. Digits typed in the in-game chat reach no seat (OLX keeps
    keys bound to controls from its chat, `CWorm.cpp:1548-1567`).
- **Game loop** (`olr-game`, plan 5 task 7): the 100 Hz ticks are drawn
  at the display's rate with interpolation (spec §8.6); at most 10 ticks
  run per drawn frame, and the rest of a longer stall is dropped. The
  sandbox HUD, names and damage popups are egui text at the display's
  resolution (E10), not OLX's bitmap fonts.
- **Weapon name over the worm** (`olr-game`, the user's request;
  `src/common/CWorm.cpp:1304-1315`, `src/gusanos/base_worm.cpp:678-684`,
  `src/common/CWormHuman.cpp:405-457`): as OLX, while the change-weapon
  key is held, and for 0.75 s (75 ticks) after a change made with it or
  with Previous/Next weapon (issue #48; #23 showed it for neither), and
  while a Weapon 1-5 key selects and 0.75 s after (at every read, changed
  or not, as OLX's `:433-438`), the player's own worm shows its current
  weapon's name (the mod's, dots kept)
  in OLX's `PlayerName` white; bots, other players online and the worms of
  a game nobody plays show none. It is egui text in the worm names' font
  with their drop shadow, not OLX's outline font, and it takes the place
  of the player's own name label (OLX draws no name over a local human
  worm and puts the weapon name's top 15 world pixels above it, `y-30` at
  double resolution: where the name label sits here), kept inside the
  picture at its edges. The 0.75 s are counted in ticks, so they stand
  still while the game is paused (OLX's run in real time). Every local
  player's worm has its own (issue #29: its own change, its own change
  key held), as OLX's per-worm `bForceWeapon_Name`; a bot's never.
- **Window** (`olr-game`, plan 6 hotfix, the user's ruling): the window
  cannot be drag-resized or maximised; its size comes from the defaults
  (later the options menu, M7) and `--fullscreen` (borderless). The game's
  own size changes are applied at most every 250 ms, because WSLg's Weston
  crashes on quick resizes of a decorated Wayland window. On Wayland each
  change closes the window and opens a new one (v0.1 playtest: WSLg left
  pieces of a changed window's frame on the screen); the compositor places
  it, so the window does not keep its position.
- **Deathmatch in the game** (`olr-game`, plan 6 task 8; full menus are M7):
  a command-line Deathmatch (`--mode deathmatch`) gives the human random
  weapons the restrictions enable (OLX's weapon dialog's "Random"); menu
  matches have OLX's weapon selection (plan 7 task 5, below). The HUD is egui,
  not OLX's: the time-limit clock is frozen at the end in the HUD
  (`Deathmatch::time_left_ms` keeps counting, harmlessly), kill-feed lines
  name no weapon (`olr-sim` does not record the injuring weapon; OLX drops
  `" with <weapon>"` when it is unknown); "Game Over", then after
  `GAMEOVER_WAIT` (3 s) the results, as OLX. F5's rematch plays the next seed.
- **Menus** (`olr-ui`, `olr-game`, plan 7 task 2): the main menu and local
  play are laid out anew in egui, not OLX's `DeprecatedGUI` screens. Local
  play sets OLX's local game options (mod, level, `Lives`, `KillLimit`,
  `TimeLimit`, `LoadingTime`, `Bonuses`, `WeaponRestrictionsFile`; the
  sliders' ranges from `FeatureList.cpp:31-35`) and the bots as a count with
  one difficulty or each its own, where OLX adds bot profiles to a player
  list one by one. Esc pauses a match (a pause menu) instead of OLX's
  in-game menu; every Deathmatch played from the menus is saved as a
  replay (`replays/`, the last 20), which OLX does not do.
- **Profiles** (`olr-ui`, `olr-game`, plan 7 task 3): OLX's player menu
  (`Menu_Player.cpp`) laid out anew: one screen creates, edits, deletes
  (confirmed; never the last) and selects human profiles (bot profiles are
  local play's bot choices). Names are trimmed, not empty and at most 20
  characters as OLX's; unlike OLX they keep Unicode (`RemoveSpecialChars`
  drops non-ASCII), refuse control characters and refuse another profile's
  name (ignoring case), so a name picks one profile. The skin preview
  animates OLX's menu formula (`Menu_Player.cpp:746`, 200° a second) and
  shows the worm faced left too (OLX's menu faces it right only), drawn
  from the game's colorized sheet and frame cut (`olr_content::skin_sprite`,
  the left-facing column quirk included). In a match the human (worm 0)
  wears the profile's skin and colour and every other worm OLX's default
  skin (OLX's bot profiles, `ProfileSystem.cpp:59`); until now every worm
  wore the player's skin.
- **Options** (`olr-ui`, `olr-input`, `olr-game`, plan 7 task 4): OLX's
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
  OLX's default back, with a warning; an options file from before
  issue #23 so gets the defaults of Strafe, Previous/Next weapon and Dig,
  one from before issue #48 those of Weapon 1-5),
  and the game's own keys (Esc, F2,
  F5, Tab, H; PageUp and PageDown since task 8) cannot be bound. Capture is OLX's "Input for:" box (Esc
  cancels). The match is set up in local play only, before each game (it
  remembers the last); the options have no Gameplay tab (removed after the
  v0.1 playtest: it edited the same settings).
- **In game** (`olr-ui`, `olr-game`, plan 7 task 5). *Weapon selection:*
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
  above it (16 : 7.2) shows `olr_scene::Viewport::GAME`, 427 × 192 world
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
  suicides; Rematch, Save replay (`replays/replay-NNN.olrp`, never pruned)
  and Back to menu. *Bot colours:* OLX's seven `cpuColours`
  (`ProfileSystem.cpp:47-59`) in turn, skipping those within 120 (RGB
  distance) of the human's colour; OLX gives each bot profile its colour
  regardless. Cosmetic: replays draw every worm in the default colours.
  *Keys:* Esc and F5 ignore key repeat; a settings or profiles file that
  binds a game key (Esc, F5, F2, Tab, H, PageUp, PageDown) loses it with a warning; while the
  options capture a key, egui hears no key at all.
- **Level thumbnails** (`olr-import-gfx`, `olr-pack`, plan 7 task 6): OLX's
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
- **About** (`olr-ui`, plan 7 task 6): OLX shows its credits in the corner
  of the main menu (`Menu_Main.cpp:276-283`); the remake has an About
  screen from the main menu with the credits, the licenses and each
  bundled pack's origin.
- **Import** (`olr-import`, `olr-cli import`; plan 7 task 7's
  Import screen was removed in plan 13c, imports are `olr-cli import`'s
  and later the editors'): OLX reads its game directory as it is; the
  remake imports one (the `share/gamedir`, or an OpenLieroX root holding
  one) into packs, and the game layers the user data's `packs/` over the
  bundled packs (spec §9.4): a pack id in `packs/`
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
  content-hash error.
- **Fast-forward and spectating** (`olr-game`, plan 7 task 8, the user's
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
- **Engine stress test** (`olr-cli stress`, plan 7 task 8, the user's
  request): its matches go past OLX's limits on purpose: 32 chaos bots,
  `LoadingTime` 0 (in OLX's range, below any menu default) and, in one
  configuration, a projectile pool of 20 000 (`MatchOptions::pool_capacity`;
  OLX's `MAX_PROJECTILES` is 3000). Every other match keeps OLX's pool and
  loading time; the oracle, match and render goldens are unchanged.
- **Renderer** (`olr-render`, plan 5 task 5): OLX blits each sprite with SDL
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
  difficulty's aim errors scale with at least `olr_bots::AIM_ERROR_FLOOR` =
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

- **G1, `olr-ui`:** menu logic (`UiState::update`) apart from the egui
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
- **G6, assets:** level thumbnails are imported (pack format 4, 5 since plan 13c); OLX's
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

- **P1:** the reference configuration is `olr_cli::stress::spec` with 16
  worms: chaos bots of mixed difficulty, `LoadingTime` 0, bonuses every
  5 s, no kill or time limit, seed 1.
- **P2:** the pool axis ends at 65 536 (`MatchSetup::MAX_POOL_CAPACITY`),
  not 100 000; raising it is a simulation change.
- **P3:** generated levels bypass the content loader's caps; a point above
  `MAX_COLOR_MAP_BYTES` still runs and the cap is its breaking cause.
- **P4:** sweeps are headless; the frame budget is judged in the GPU
  scenes (the heavy replay at four sizes, each map and art level at
  2560 × 1440 on WSL); breaking points use 144 Hz, 60 Hz beside it.
- **P5:** windowed runs (`olr --frame-log`) measure wall frame times
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
  `%TEMP%\olr-envelope` and run there; `WGPU_BACKEND` goes through
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
  the network as `olr_sim::WorldState` (serde-derived, postcard, deflated),
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
- **N18:** an `olr_netcode::Session` lives for one match; the replay is made
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
  `olr-net`'s reliable cap is `max(MAX_CONTROL, MAX_LOBBY)` plus the tag.
- **N25 (T13-A):** `olr-server` depends on `ctrlc` 3.5.2 (termination
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

- **N29 - pack archives.** A pack travels as a `.olpk` archive: magic
  `OLPK1\n`, a `u32` file count, then per file, sorted by path, a `u16`
  path length, the UTF-8 `VPath`, a `u64` length and the bytes. No
  compression (packs are mostly PNG and OGG). The same pack always gives
  the same bytes, so a resumed download continues byte-exactly. The
  archive's SHA-256 (the *archive hash*) identifies a partial download;
  the *content hash* (`olr_pack::content_hash`, unchanged) identifies the
  pack. Hidden files (a path component starting with `.`) are not part of
  a pack: hashing and archiving skip them, decoding refuses them (PF11-2:
  a stray `.DS_Store` would make a pack hashable but unservable). Archive
  reading is bounded (512 MiB, 4096 files, 255-byte paths, 64 MiB a file);
  `olr-proto` restates the archive limit without depending on `olr-pack`,
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
  runs without one: it still serves its own packs. The Options' Online tab shows the
  cache's size and clears it, refused while an online game is on. The
  cache counts each pack by its disk charge (N29), not its bytes, so the
  budget and eviction see real disk use (final review C2). "Never one the
  current lobby lists" holds for the whole process: every list (a host's
  own list, a client's fetcher, both peers of `--net-sim`) registers its hashes with the shared cache
  (`olr_content::Keeper`), and every eviction keeps their union (final
  review Minor 2). Partial downloads are charged as installed packs are
  (plus a block for the meta file), at most 32 per cache (a new one
  discards the oldest not kept; when all are kept it is refused), and
  indexed in memory, so no fetch start scans the directory (final
  re-review R1).
- **N31 - skins by id and hash, never transferred (plan 13c, spec §4;
  replaces plan 11's skin download).** `Look` carries `skin_hash` (64
  lowercase hex), the match key. A match draws the local player's own slot
  by its skin id; every other slot by the skin pack whose id is exactly
  the look's `skin` and whose content hash is `skin_hash`
  (`Content::skin_matching`, `setup::load_online`, keyed by slot so two
  slots of one id and different hashes do not collide). A skin this
  install lacks, or has with other content, draws as the default skin, with
  a warning logged once; it never blocks the ready mark. A bot's look keeps
  `NO_HASH` and gets only the default skin. Nothing offers, fetches, serves
  or lays over a skin: the host lists only the setup's packs (mod, level,
  theme: all *required*, and `valid_content` refuses an optional item), and
  ignores a member's `List`. The host still takes at most 4 `SetLook`s a
  second from a member; the latest beyond them is held and taken once the
  window allows (final review C2). `SKIN_LIMITS` and `MAX_SKIN_ARCHIVE`
  stay as the small-archive limits the tests use. The network protocol
  version goes 2 to 3, so a 0.1 client is refused at the handshake ("the
  host plays another version").
- **N32 - transfers.** Reliable-channel tag `TAG_CONTENT = 3` carries
  `ContentMsg` both ways (`List`, `Want`, `Begin`, `Chunk`,
  `Unavailable`, `Cancel`); chunks are 32 KiB. A peer serves one archive
  at a time to each peer and queues the other wants. Pacing: at most
  2 MiB/s to each receiving client, 4 MiB/s host-wide (the server's
  `upload_kib_s`, default 4096, clamped to 64..=65536), 256 KiB/s
  host-wide while a match starts or runs, at most 256 KiB queued per peer. Resume: the fetcher keeps
  `<hash>.part` and `<hash>.part.ron` (archive hash and size) and asks
  `Want { hash, offset }`; another archive hash restarts from 0. Every
  download is verified (archive SHA-256, bounds, content hash, id, kind,
  `olr_pack::validate`); one that fails is fetched once more from 0, then
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
  current list (each list has a revision, raised when the list changes). A waiting member
  counts toward `max_players` only if it wants to play.
- **N34 - protocol version 2 (3 since plan 13c, N31).** `PROTOCOL_VERSION` becomes 2, because
  `Look`, `Member` and the reliable tags change.
- **N35 (PF11-6) - the download prompt.** A list whose missing items not
  yet approved need more than `PROMPT_BYTES` (50 MiB) still to download
  waits for the player's Download (or Leave). The bytes counted are each
  item's size less what its partial download already holds ("missing
  bytes" is what the player will wait for), and items approved before are
  not asked about again: a re-sent list (a new member)
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
  over `b"olr host v1" ++ client nonce ++ client key ++ offer DTLS
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
- **N38 - one Nostr core, no Nostr library.** `olr-discovery` implements the
  NIP-01 subset it needs (event serialization and id, BIP-340 signing
  through `Identity`, the frames `EVENT`/`REQ`/`CLOSE`/`EOSE`/`OK`/`NOTICE`/
  `CLOSED`) and NIP-40 expiration, sans-IO, so the dependency tree and the
  WASM build (plan 13) stay small. Event kinds: 31470 (addressable
  announcement, tag `d` = `open-liero-reloaded`) and 21470 (ephemeral, reserved for
  plan 12b's handshakes); neither is in the NIPs kinds table or the
  registry-of-kinds (checked 2026-10-13, to re-check before a release).
  The relays and STUN servers are the lists below, chosen by measurement
  (`olr-cli nostr-probe`, `olr-cli stun-probe`, run once by hand on
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
  playing, addr, port}`. Tags: `["d","open-liero-reloaded"]`, `["t","open-liero-reloaded"]`,
  `["f","<fingerprint, 16 hex>"]`, `["expiration","<unix+180>"]`. A note is
  re-published every 60 s and when the lobby changes (at most every 12 s,
  R5-3). On close the host publishes a note with `"closed": true` and an
  expiry 10 s ahead, `created_at` at least a second newer than its last
  note (a same-second tie would let a relay keep the open note, R9-5).
  `created_at` is the wall clock read at each note (final review A-I2: it
  was the unix time at start plus the monotonic clock's elapsed time, so a
  suspend or an NTP step after the start dated every note expired at
  birth); the game list's ages in olr-game use the wall clock read at each
  poll too. A game hosted again right after its closing note dates its
  first note 10 s after that note (final review A-M6: relays keep the
  newest note of a slot and lists refuse one older than the tombstone, so
  it stayed hidden up to a minute).
  One builder, `GameSummary::of_lobby`, serves olr-server and olr-game and
  also gives the LAN broadcast (`.lan()`); players and spectators are held
  to their maximums (R9-1, R9-8). **Amended 2026-10-15** (the user's report
  of 2026-10-15: "4/8 players" listed for one player who added three
  bots, misleading joiners, since bots take no seat: olr-lobby's
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
- **N42 - invite codes.** bech32m with HRP exactly `olr` (`olr1…`, any one
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
- **PF12-7:** `olr-game` takes `olr-server` as a dev-dependency (same layer 6,
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
- **R8-1:** the probe lives in the library (`olr_cli::nostr_probe`), one
  fresh random key per run (never the player's identity), every event
  marked `open-liero-reloaded-probe` with a 10 minute expiry; the command publishes
  to public relays, so it is run by hand only and no test touches the
  internet. A relay's first connection failure fails it (no waiting for the
  pool's reconnect). Order of the defaults: well-known large operators
  first, since the index is fixed by invite codes. If wrong: a relay that
  needed a second try is left out of the defaults.
- **R9-1:** the summary builder lives in `olr-discovery` (the layer-5 crate
  that already depends on `olr-proto`), not `olr-online`: no new same-layer
  pair. **R9-2:** STUN runs for every visibility (see N43). **R9-3:** the
  server prints `invite code: olr1…` once the address is settled
  (immediately when configured or when there are no STUN servers, else at
  the first answer, at most ~3 s) and again whenever it changes. (Amended
  by plan 12b, N47: the invite waits for the reachability to settle, at
  most 5 s; one without an address reads `invite code (joins through the
  relays): olr1…`.) **R9-4:**
  see N40. **R9-5:** the closing note is forced newer (see N39).
  **R9-6:** `/invite` is admins-only in a Lan or Invite game. **R9-7:** an
  unknown `region` in a config reads as `Unknown`. **R9-8:** the server's
  default visibility is Public; LAN counts are held to the maximums, so a
  note is never refused for `players > max_players`. If wrong: an operator
  who wanted a private server must say `visibility: Invite`.
- **R10-1:** the game's default visibility stays Lan (see N43). **R10-2:**
  relays and STUN servers in the settings are `Option<Vec<String>>`, `None`
  meaning the defaults, so a player who never edits them follows later
  default lists (append-only); `olr-ui` (layer 5) cannot read the defaults
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
  watch, hints, invite, `Publisher`) and `ping` moved into `olr-discovery`,
  shared by `olr-server` and `olr-game`; `olr-game/src/finding.rs` holds the
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

- **N44 - handshake encryption.** NIP-44 v2 (`olr_discovery::nip44`):
  secp256k1 ECDH x-coordinate, HKDF-SHA256 extract with salt `nip44-v2`,
  a per-message HKDF expand to 76 bytes, ChaCha20 (counter 0) and
  HMAC-SHA256 over nonce and ciphertext, checked in constant time. It is
  checked against the official vectors, fetched once from
  `paulmillr/nip44` (`nip44.vectors.json`, last changed in 1f8dba17,
  2023-12-19). The committed file
  `crates/olr-discovery/tests/fixtures/nip44.vectors.json` has SHA-256
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
  "session":"<32 hex>","signal":<olr_proto::Signal>,"mapped":"<ip:port>"|
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
    `MAX_SESSIONS` (4096; the oldest goes). The inbox runs on olr-online's
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
    looked up on a thread (`olr-game resolve`), never on the frame.
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
  - Failure texts (`olr_online::join`): `NO_RELAYS` (every relay failed
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
  first STUN round settled), at most `SETTLE_WAIT` (5 s), in olr-server
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
- **N49 - path names.** `olr_proto::Path { Direct, MappedPort,
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
    hand-written in `olr_net::portmap` (PM-1 replaced igd-next; see below).
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
    (`olr_proto::is_global`), without `allow_local`, means a double NAT or
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
    `Core`); the server is our own (`olr_net::turn`, TR-2), not
    turn-server-proto.
  - **Who runs them:** the project runs no TURN server and ships no TURN
    list (public TURN services need paid credentials). Volunteers run
    `olr-server --relay`, which announces a kind-31471 note `{v, addr,
    port, region, capacity, load, user, pass}` with public long-term
    credentials (realm `olr`, fresh each start, 16 hex characters
    each), at once and every 60 s (expiry 180 s), and a closing note
    (capacity 0, expiry 10 s) at shutdown. Players may add their own TURN
    servers (`user:pass@host:port`, Options → Online → TURN relays, at
    most 8; `olr-cli --turn`), for joining. olr-server's config has no TURN
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
- **N53 - kind 31471** (addressable, `d` and `t` = `open-liero-reloaded-turn`,
  `v` checked) for TURN relay notes. `TurnList` validates like the game
  list: addresses global unless `allow_local`, never unspecified,
  multicast or broadcast; credentials 1-64 printable ASCII; capacity at
  most 4096; one note per key per 10 s; at most 64 relays; a closing note
  is exempt from the gap and leaves a tombstone, which outlives a flood
  of closing notes (never-listed keys are evicted first). The kinds
  check is below ("Measured"). If wrong: a kind change before release.
- **N54 - the host listens on Nostr.** Public and Invite games and
  servers listen on their relays with a pool of their own, on a thread
  (`olr-online listen`) that owns the pool and the `Inbox`; LAN games and
  Lan servers do not. Offers wait unbounded for the peer's poll (the
  inbox's rate bounds them) and one older than `OFFER_STALE` (8 s) is
  skipped. Relay edits in the game's Options move the listener at once
  (`OnlinePeer::listen_on`, Task 9). If wrong: a public host keeps two
  connections per relay (the announcing pool and the listening pool).
- **N55 - same-layer pair** `("olr-online", "olr-discovery")`: olr-online
  drives the handshake pool, so olr-game, olr-server and olr-cli share one
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
- **N57 - the STUN binding codec and `is_global` move to `olr-proto`**
  (`olr_proto::stun`, `olr_proto::is_global`, `core::net`); olr-discovery
  re-exports them. If wrong: none beyond a wider olr-proto.
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
  - **Binaries:** `e2e`'s dev builds of `olr-server` and `olr-cli
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
  - **Cleanup:** every container ends by itself (`OLR_NAT_LIFE`, 120 s);
    Ctrl+C removes containers and networks (xtask's `ctrlc` handler, only
    during a run); names carry xtask's process id; a machine-wide lock
    serialises runs.
  - If wrong: a host whose distribution lacks an official image, or a
    rootless Docker, skips the step.
- **N59 - a relayed member's IP ban is a key ban.** Its address is the
  relay's: banning it would ban the relay for everyone.
  `LobbyHost::ban_with_address(member, by: Option<u8>)` tells the admin
  who asked (`by`; `None`: the host's own, or a server command) "reached
  through a TURN relay: banned by key only". olr-server's `/banip` keeps no
  IP for such a member, so the bans file never holds a relay's address
  across a restart. If wrong: a relayed troublemaker returns with a new
  key (bans are soft anyway).
- **N60 - `olr-server --nat-check`.** Reports, in order: the base
  addresses and gateway; each configured STUN server's answer and `NAT:
  <nat_text>` ("no NAT", "a NAT that keeps ports (hole punching works)",
  "symmetric NAT", "not known yet"); the router (a random UDP port of
  49152-65535 mapped exactly, its result, then "router: the mapping is
  deleted" only when the router confirmed it); each Nostr relay's status
  with its clock against this PC's (FR12b-3) and a warning past a minute,
  whether each forwards a 16 KiB handshake from a new key (final review
  B-M3), and the TURN notes on them; `nat-check done`. Exit 0. Without
  `--config` it uses the defaults (8 relays, 5 STUN servers). It ships in
  the release zips (it is `olr-server`); it uses public servers only when
  run by hand. If wrong: a report that misleads a playtester; it changes
  nothing.
- **N61 - protocol stays 2:** `Member` gains `path: Option<Path>`
  (appended; `None` for the host's own player); `Signal` is unchanged.
- **N62 - `allow_local`.** olr-server's config `allow_local: bool`, `olr-cli
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
- **PL12b-1:** 12 tasks; the `olr-net` refactor (Task 1) came first, so
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
  before Task 1, among them: `is_global` moved to olr-proto; tests use
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
- **PF12b-4:** no TURN servers in olr-server's config (the host never
  allocates, N48). If wrong: a later config field.
- **PF12b-5:** no test maps a real router port or sends SSDP to the
  multicast group: every test host sets `port_mapping: false` or targets
  a `FakeGateway` through `test-hooks`; TURN tests use ephemeral ports and
  generous limits. If wrong: none.

Task rulings:

- **TR-1** (as amended by TR-5): a TURN relay refuses peers that are not
  global (`olr_proto::is_global`: loopback, private, link-local, multicast,
  unspecified, documentation, CGNAT …; 403, IPv6 peers 443), unless
  `allow_local` (N62): a public relay with public credentials must never
  reach its operator's network. **TR-5** (final review B-I3): TR-1 first
  refused the relay's own public IP too, so a relay could never carry a
  game hosted behind its own router (the `olr-server --relay` beside its
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
  `GetExternalIPAddress`, `AddPortMapping` (description `Open Liero Reloaded`)
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
  hairpin) became olr-net's `connect_stall` test hook: the connect sends
  nothing and fails after `CONNECT_TIMEOUT`. If wrong: none.
- **NT-2:** `Endpoint::shutdown(self) -> Shutdown` never blocks: it closes
  every link and hands the mapper to a thread that deletes the mappings
  and waits for the router's confirmation (`PortMapper::close`);
  `Shutdown::wait(until)` (at most 1 s) says whether the router
  confirmed, `Shutdown::poll` the same without waiting.
  `OnlinePeer::shutdown` wraps it. olr-server waits at exit (and warns
  when the router did not confirm), olr-cli's bot client waits, and the
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
  within 15 s (olr-net's new `NetEvent::JoinFailed`, for a host's
  signalled setups only), and handshakes dated more than a minute off the
  host's clock ("… check both PCs' clocks"; the host's subscription now
  reaches 10 minutes back, `CLOCK_SEEN`, so a joiner whose clock is
  behind is seen); at most one line of each kind every 5 s. `NO_REPLY`
  names the clocks, and an `invalid:` refusal is `RELAYS_INVALID` (B-M2).
  `olr-server --nat-check` prints each relay's clock against the PC's,
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
- olr-server's `/banip` on a relayed member wrote the relay's IP to the
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

- **Relay probe, 2026-10-14** (`olr-cli nostr-probe`, run once by hand on
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
- olr-cli's NT-2 exit wait is untested (a client maps no port).
- A mapping whose answer was lost (silence, garbage) is not deleted; it
  lasts its lease (2 h at most) and points at our own port only.
- ~~The Video tab's rows overflow at 384 and 320 points (UI scale 3,
  borderless 16:10 and 4:3), older than this plan.~~ Fixed by issue #49
  (its rows stack, the tab scrolls).
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
`olr_netcode::migration` (timing), `olr_online::migration` (the open
timeout, the texts) and `olr_game::net::pump`.

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
  (`MigrationConfig::standby = None`, `olr-cli net-client --no-standby`),
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
  and typical and `WINDOW_US` on bad and awful. olr-online's loopback test
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
  **Plan 12d (`aca289a`):** olr-cli net-sim's N66 verdict (`netsim.rs`) counts only the first
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
  olr-online) past it, later ones merged as late reports. A successor
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
  when its contact is lost (olr-online's `Planner::host_heard`; the harness
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
  olr-game's `PeerHandle` holds each online peer (host and client alike,
  `--net-sim`'s host too) behind one lock shared by the UI thread and a
  thread `olr-game net`, which polls the peer whenever the UI has not for
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
  measured to olr-net's delayed `Connected`, up to 1.5 s after the link
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
  `olr-cli net-sim --from`; a transport failure is `soak-transport-<seed>/`),
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
  migrations, up to 119 ticks. Fixed in olr-netcode, with Resume's wire
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
  `MAX_PREDICTION` (`StallTracker`, olr-cli); N66 bounds the migration
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
  harness's N69 remembers contact as olr-online does. Two flakes were fixed
  or noted: olr-server's `nat_check` timing test (`close_within`) and the
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
  on); the API comment and olr-netcode's CLAUDE.md say so (`4613a96`).
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
  (no split), `typical-14s4c1v-seed1857358438`. olr-online has no test for
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
- olr-online's loopback tests (real clocks, `127.0.0.0/8`, in `check`):
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
- ~~Task 9: the lobby's setup panels overflow 320- and 384-point windows
  (older than this plan; not at 4K or ultrawide).~~ Fixed by issue #49
  (the lobby stacks its setup over the players and the chat).
- Task 10: the net-nat migration scenario's `relay_allocates` is satisfied
  before the crash (the post-migration `path: relayed` line is the real
  proof); the first successor's order relies on Docker's start latency; a
  hardcoded `MIGRATION_LIMIT` in a message; a bound message drops its
  seconds.
- Task 6: the dead-successor timeout (6 s after SB-1) and the resume's
  one-poll commit check are the residuals under N65; the resume's report
  deadline `REPORT_WAIT` is decided in olr-online, not olr-netcode.

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
  desync and one state repair** (and a desync bundle in olr-online), not
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
  `olr_netcode::migration::quorum`, called by the harness and olr-online:
  a join to an uncommitted successor is held (olr-online holds the
  member's `Welcome` and what follows), the commit takes every held join
  in. Pinned by `only_the_majority_side_of_a_partition_continues`,
  `an_even_split_continues_on_neither_side`,
  `a_successor_short_of_a_quorum_is_host_lost_legitimately` (olr-netcode)
  and `a_successor_without_a_quorum_never_hosts` (olr-online). SK-10 accepts
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
  olr-online's `a_player_and_its_spectators_carry_on_when_the_host_crashes`).
  The harness counts only followers that were in the match (a spectator
  whose join tick never came is not); olr-online counts its migrated
  lobby's `away()` at the turn's opening (every succession member). Cost:
  a member that cannot come back (a spectator still loading, SK-8) still
  counts (olr-online's `a_joiner_still_loading_when_its_host_crashes_is_host_lost`
  now has a third watcher). Net-soak's `bad-10s1c2v-seed1616063176`: the
  player now takes the match over with both spectators at 15.55 s.
- **FR12d-Q2 - an unreached successor is asked again** (controller's
  ruling, amends N73). An ask not reached (no answer, a connect that
  failed or timed out, a link that never opened, a relayed join that
  failed) is made again `REASK_US` (1 s) later, within its window
  (`olr_netcode::migration::REASK_US`; harness `Stage::Retry`; olr-online's
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
  Not covered end to end in olr-online: its blackout hook drops datagrams
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
  worm's row (hashed only when true). The live scoreboard (`olr_ui::hud::rows`)
  leaves it out; the results (`Results`) keep it, its lives reading "left",
  greyed, ranked as an out worm by `CompareWormsScore`. `MatchEndWire` is
  unchanged. Cost if wrong: one table row.
- **N86 - the texts.** OLX's `NetworkTexts` (`Options.cpp:769-770`):
  "<name> has left" (a `Bye` or a closed link) and "<name> has timed out"
  (the session's `PeerTimedOut` or the link's `Why::Timeout`);
  `Notice::TimedOut` is new (appended). Kicks and bans keep their own
  notices. At a resume see PF12d-3. Cost if wrong: one log line.
- **N87 - the crash timeout stays 10 s** (`SILENCE_TIMEOUT_US` in the
  session, `SILENCE_TIMEOUT` in olr-net). Until it fires, the lagging rule
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
  gone"; the olr-ui unit test's RED was only a compile error.
- FR12c-1b: no olr-online test pins "a client crashes, then the host's
  `Bye`" (the lone survivor's Host lost).
- Task 7: `reproduce` filters `gone` per tick (O(inputs x gone)); no twin
  for a profile where `fills_stall` is false; the crash window does not
  model migrations.
- Final: minor (deferred): a member that can never come back (a spectator
  still loading) counts against the quorum.
- Final: minor (deferred): FR12d-Q2 has no end-to-end olr-online test (the
  outage hook drops UDP only; the planner's tests and the harness cover it).
- Final: minor (deferred): SK-10's reach model stays generous
  (`lacked_quorum`, `reaches`).

**Re-run full after the final fixes, at c76ffad:** passed in about 15 min
(test 144 s, bot-grid 163 s, gpu 37 s, e2e 47 s, net-nat 12 scenarios
146 s, bench 162 s with the worst +3.8 % on `bots_8_tick`, stress 21 s,
net-feel 43 s, determinism-windows 121 s).

## Format switch decisions (plan 13c)

Decided in `docs/superpowers/plans/2026-10-17-plan-13c-format-switch.md`
(D1-D15).

- **Packs are format 5.** Level, theme, skin, mod and core packs are built
  over `olr-format`, and `PACK_FORMAT_VERSION` is 5. A pack of another
  version is refused ("re-import the pack"). There are no migrations.
- **D15 - content hashes changed, nothing is re-blessed.** Nothing committed
  pins a pack content hash, and no replay is committed. Old replays and
  cached packs are refused or dropped, because the format version and the
  protocol version both changed.
- **Keying happens once, in the importers (D3).** `olr-scene` and `olr-fx`
  draw the PNGs as they are. The runtime digest
  (`olr-cli`'s `digest` snapshot) hashes what the game draws: each skin's
  drawn frame windows, since format 5 drops the spacing columns, which
  OLX's renderer never samples either.
- **D14 - asset names are kept.** Asset files keep OLX's lowercased names
  (such as `cluster_bits.png`); renaming them to kebab-case is deferred.
- **D6 - the import pipeline is `olr-import`** (layer 3). `olr-cli import
  --out <data>/packs` fills the user layer that the game still reads. It
  only adds or replaces packs; `--prune` (which `cargo xtask
  fetch-olx-content` passes, so `content/packs` is exactly the manifest's
  import) also removes the packs the run did not produce, and `--all`
  imports everything `olr_import::discover` finds (an error with item
  flags).
- **`olr-cli validate` is rebuilt on the format-5 readers** (D13: the name
  stays until plan 13d; plan 13d replaced it with `olr-cli pack check`).

## Pack tooling decisions (plan 13d)

Decided in `docs/superpowers/plans/2026-10-17-plan-13d-pack-tooling.md`.
These are tools, not OLX behaviour; none changes the simulation or a pack.

- **R2 - a save replaces the whole pack.** `olr_edit::pack::save` writes the
  complete new pack beside the old one and swaps it in
  (`olr_pack::install::replace_dir`, the importer's procedure): the pack on
  disk is the old one or the new one. Files the format does not read are
  copied byte for byte; a name a save could not copy (not UTF-8, or one
  `VPath` refuses) makes the pack refuse to load instead. Empty directories
  and file permissions are not kept.
- **R3 - what the level operations mean** (spec §6 named them only).
  They work in world pixels on the level's source:
  - `carve`: the material becomes background, and the front shows the
    back there;
  - `fill`: rock or dirt, the front painted with a colour or a tile;
  - `paint`: art only; painting the back also paints the front where the
    material is background, painting the front touches solid pixels only;
  - `stamp`: an image composited onto an art layer, the same layer rule
    (the spec's `stamp` was open; a theme's stamps are the simulation's
    carving shapes, so the image stamp is the editor's tool);
  - `resize`: a new size and an offset for the old content; objects move
    with it, and those left outside are dropped and reported;
  - `object add|move|remove|list`, by index.
  A touched world pixel changes its whole art block, so `front` equals
  `back` wherever the material is background, and every operation
  regenerates `thumb.png` (a hand-made thumbnail is replaced).
- **R4 - schema pages show one example per type, not per field** (spec §6
  asked for an example per field; it would repeat the default or the
  type). Each file type shows the guide's checked block of that file.
  The pages are generated from the doc comments, which state each field's
  unit and OLX source (`mod-fields.md`); OLX names the research left open
  are not guessed.
- **`pack new` templates (R6).** A mod (one weapon, `gun`, enabled by a
  `Standard` preset) and a level (dirt in a rock border, with a
  thumbnail) are made in code; skin, theme and core packs start from a copy
  (`--from`), which gets the new id and name and loses its origin.
- **A mod's stray files are warnings.** In `weapons/` and `projectiles/`,
  only `.ron` files that are not hidden are definitions; an editor's backup
  or `.DS_Store` there is "not read by the format", as anywhere else.

## Level composition decisions (plan 14a)

Decided in `docs/superpowers/plans/2026-10-17-plan-14a-level-composition.md`
and its batch reviews. Tools, not OLX behaviour: a level without `editor/`
reads, plays and saves exactly as before.

- **R1 - raster operations on a composed level are refused.** `olr-cli
  level carve|fill|paint|stamp` exit 1 on a composed level (the spec had
  them edit a drawn element "CLI edits", but carve and paint act on
  several materials at once, which one drawn element cannot hold); agents
  paint with `level element draw|erase`. Object commands keep the
  composition; `level resize` resizes it too.
- **R2 - `editor/` is authoring.** `olr_pack::is_authoring`: never in the
  content hash or an archive (`decode` refuses one), so a player downloads
  only the bake.
- **R3, R4, R5 - the bake.** A world pixel's material is the topmost
  visible element covering the centre art pixel of its block; an art
  pixel's front the topmost covering it, else its back; its back the
  topmost background image or drawn element covering it, or a raster's
  back, else the theme's back tile; then `front := back` on background
  blocks. Image, drawn and tile colours are baked opaque; a raster's art
  is kept as stored, so a level composed of its raster base bakes back to
  its own bytes. That holds for levels that keep the front/back invariant
  (every import, `olr-cli` level and bundled level): a hand-painted
  `front.png` that differs from `back.png` on background bakes to the back
  there (invisible in play, since the game shows the back on background).
- **R6 - the raster base is `base`** ("Base"), its files encoded as
  `write_level_files` encodes the level's.
- **R7, R8 - names.** Originals keep their bytes and name
  (`editor/images/<name>`, `-2`, `-3` … before the extension when taken,
  case aside, or a Windows device name); two originals differing only in
  case are refused. Ids are slugs of the name or file stem, at most 64
  bytes, skipping taken ids, ids whose files another element uses, and
  Windows device names. `editor/` is authoring in any case (`Editor/`).
- **R9 - the back tile** is the theme pack's (by manifest id) in
  `--content`, `content/packs` by default.
- **The image pipeline** is integer only: area resample, alpha threshold,
  median cut (the exact colours when they fit; splits moved to a boundary
  between channel values, so a rare colour is not averaged away),
  Bayer 8 × 8 or Floyd-Steinberg in sixteenths, nearest colour exact
  (ties: the lowest index).
- **Bounds.** At most 256 elements; an image box's art (size × art scale)
  within `LEVEL_ART`; all elements' decoded pixels together within
  `MAX_COMPOSED_PIXELS` (256 Mi); originals within `IMAGES`. A JPEG must
  end its scan (EOI): a cut file is refused rather than filled in grey.
- **JPEG decoding** (`zune-jpeg` 0.5.15, pinned) runs its AVX2/NEON paths
  when another dependency (`image` through `olr-game`) enables them;
  measured bit-identical to the scalar decode on this machine (AVX2) for
  baseline, 4:2:0 and greyscale JPEGs. NEON is unverified: `cargo xtask
  determinism`'s arm64 leg compares the bake digest, but `olr-detcheck`
  alone builds without those features. EXIF orientation is applied at
  decode since plan 14b (ruling R5 below).

## Level editor decisions (plan 14b)

Decided in `docs/superpowers/plans/2026-10-17-plan-14b-level-editor.md`
and its batch reviews. Tools, not OLX behaviour.

- **R1 - 14b's scope.** `olr-editor` holds the document, the history, the
  tools, the panels, the shortcuts, New/Open/Save/Close with the
  unsaved-changes dialog, and Play as a command. Plan 14c hosts it in
  `olr-game` (the canvas drawn by `olr-present`), adds the file browser
  (14b takes image and pack paths through actions), the 2-minute recovery
  copy, the bundled-pack copy offer, `olr --edit` and the e2e script.
- **R2 - the document works on decoded data.** Each element is kept
  decoded (`olr_edit::compose::Decoded`); a dab changes a drawn layer in
  memory and rebakes one world rectangle; PNGs are encoded only on save
  (`Document::to_pack`). Opening rebakes from the elements; the thumbnail
  is refreshed only when the bake differs from the stored one, so an
  unedited composed pack saves byte-identical.
- **R3 - the image cache.** `ImageCache` keeps each original decoded once
  (at most `ORIGINAL_PIXELS`, 64 Mi, the least recently used dropped) and
  each source's 4 most recently used processed grids, keyed by box art size
  and pixelation. Entries carry their bytes' fingerprint, so bytes changed
  under the same name are processed afresh. A move, a reorder or a
  material change looks nothing up.
- **R4 - locks protect content, not order.** A locked element refuses
  property changes (except `Locked`), canvas moves and resizes, painting
  and Delete; raise, lower and drag-to-reorder still work. Resize and art
  scale, level-wide, change locked layers too.
- **R5 - EXIF orientation is applied at decode** (`image_io::jpeg_orientation`,
  `orient`): a JPEG turns by its `Orientation` (2-8); the original keeps
  its bytes; `image_size` stays the stored frame's size.
- **R6 - live diagnostics are cheap checks**: `check_elements` (the
  composition's metadata), spawns outside the level, a level without
  spawns (warnings). `pack::save` runs the full validation.
- **R7 - pointer actions are in world pixels**; only the view (`View`)
  maps screen points, with floats.
- **R8 - a resize previews** its outline and applies on release (one
  reprocess); a move is live (no reprocess, the old and new boxes
  rebaked). Every action that can change the document ends a drag first,
  so a drag's step is recorded against the document it changed.
- **R9 - Play defaults**: `olx-classic` and 1 bot (0-8), remembered in
  `EditorSettings` (the host stores them).
- **R10 - shapes.** `Shape::Ellipse` covers a pixel by its centre;
  `Shape::Line` covers centres within `width / 2` of the segment (an even
  width covers `width + 1` across, as a circle of radius `width / 2`).
  Both are integer, in doubled coordinates (a line's distance compared in
  256 bits), clipped to the level before iterating. A brush stroke is dabs
  joined by lines of the brush's width.

## Level editor decisions (plan 14c)

Decided in `docs/superpowers/plans/2026-10-17-plan-14c-editor-in-game.md`
and its batch reviews. Tools, not OLX behaviour.

- **R1 - the canvas is `olr-render`'s**, not `olr-present`'s (`Presentation`
  needs a `World`): an `olr_terrain::ColorMap` of the bake drawn as a
  terrain-only `DrawList` with `Renderer::render_in` into the canvas's part
  of the window, under the egui panels and overlays; each edit replaces its
  dirty rectangle (`ColorMap::replace_rect`, `Renderer::update_terrain`).
  The canvas draws with SharpBilinear whatever the options' filter (whole
  multiples would letterbox off the overlays); its view is the world under
  the whole canvas (black off the level, so panning keeps the renderer's
  world target), cut to what one device texture holds
  (`Canvas::max_view_for`); its map is dropped while the editor is closed
  or its Play runs.
- **R2 - the material view**: the canvas's one colour map is rebuilt from
  `MATERIAL_PALETTE` at art scale 1 when it is turned on, and from the bake
  when it is turned off; a resize or an art-scale change rebuilds it too.
- **R3 - Play**: 0 bots is the sandbox (the player alone), 1-8 a Deathmatch
  against the Local Play screen's saved bot level, personality and loadout,
  under OLX's default rules and `DEFAULT_PRESET`. Neither saves a replay;
  there is no weapon selection (random weapons). Esc ends the play at once
  (no pause menu), and so does Stop over the playfield; F5 plays it again
  from its start; a match over returns to the editor when its results
  would show (no results screen).
- **R4 - the first spawn**: the human worm's `WormSetup::spawn` is the
  level's first spawn when it is a good spawn point by OLX's own rule
  (`CMap::IsGoodSpawnPoint`: no rock in the 6 × 6 square around it, outside
  the level counting as rock), else a free spot (`LoadedLevel::first_spawn`; `build_match_spawning`, with
  `build_match` unchanged). Later respawns are the simulation's.
- **R5 - where levels are edited**: the data directory's `packs/`. The
  Editor screen offers Edit for its level packs and Edit a copy for the
  others (`olr_edit::new::new_pack`, `Template::From`); new ids are
  `pack_id` slugs of the name, unique against the catalog and `packs/`.
  The host also refuses to open, create or save inside any bundled root
  (the browser could reach one).
- **R6 - `olr --edit <dir>`** refuses a directory inside `--content`
  ("bundled content is not edited in place: use the Editor menu's Edit a
  copy", exit status 2) before any window opens.
- **R7 - recovery**: `recovery/<id>/` in the data directory holds the pack's
  files and `recovery.ron` (origin, level name), written atomically
  (`replace_dir`) every 2 minutes while there are unsaved changes, and
  removed on a save, on Close and when the document is left. Each opened
  pack's session writes a copy of its own id (`pack_id` against
  `recovery/`), so a crash's copy survives opening its pack again; a
  recovered document keeps its copy's id (the next copy replaces it), and
  reopens in its folder only when that still holds its pack (else a new
  one, under a new id when its own was taken). The copy is made from a
  document snapshot and written on a thread of its own (250 ms for Castle
  Strike on the UI thread before). Recovering opens the
  original directory when it is there, else a new `packs/<id>` (Save makes
  it); a damaged copy is listed and can be discarded; a recovery's own
  folder is never opened as a pack.
- **R8 - the editor's settings** (Play's mod and bots, the browser's last
  folder) are `editor.ron`, written through `Store::save_file` (versioned
  and atomic like the settings) and read with `Store::load_file`.
- **R9 - the file browser lives in `olr-editor`**, as state and a modal
  window; the host lists directories and reads files (`EditorCommand::List`,
  `ReadImage`: an import is refused by its metadata above 64 MiB and by its
  first bytes when it is not a PNG or JPEG, before it is read whole). A
  click selects; a double click or Choose picks (a folder is entered).
- **Loaded maps** (the playtest after 14c): a level without a composition
  opens as three drawn layers, background, dirt and rock
  (`olr_edit::compose::split_from`, byte-identical bake), not one raster
  base, so it can be erased and painted; a raster element splits the same
  way on request (Split into layers, one step). The CLI's element commands
  keep the raster base (`ensure_composed`).
- **Closing the window** with the editor open sends its Close first: unsaved
  changes ask (Save, Don't save, Cancel), and the window closes once the
  editor does.

## Level editor decisions (plan 14d)

Decided in `docs/superpowers/plans/2026-10-18-plan-14d-layers-masks-merge.md`
(spec `docs/superpowers/specs/2026-10-18-level-editor-polish-design.md`)
and its batch reviews. Tools, not OLX behaviour.

- **R1 - no format bump**: every new composition field has a default and
  is left out when default; compositions written before read unchanged,
  `content/packs` stays byte-identical, `PACK_FORMAT_VERSION` stays 5.
- **R2 - a drawn layer's box**: `Drawn { at, size }` (world pixels).
  `size: None` is the level's size at the origin and follows a level
  resize (re-laid); `Some` is a box of its own, which a resize moves by
  the offset. Moving a level-sized layer pins its box to the level's size;
  back at the origin with the level's size it is `None` again. A resize
  pins a level-sized layer off the origin too, so none of its pixels are
  lost (batch A review).
- **R3 - mixed and backed layers**: `Drawn { mixed, back }` with
  `editor/layers/<id>.material.png` (a material per world pixel of the
  box, `material.png`'s palette; spec C2) and `<id>.back.png` (what shows
  where its dirt or rock is carved). In memory `Decoded::Drawn { art,
  cells, back }`; the flags and the data change together, a plain layer
  becoming mixed only through a whole-item swap (`Edit::Layer`).
- **R4 - the bake of a drawn layer**: it covers an art pixel inside its
  box where its art is not transparent; a covered block's material is its
  cell (or the layer's material); its backdrop is its art where covered
  with background, else its back art where opaque.
- **R5 - erasing digs**: erasing a mixed layer's dirt or rock whose block
  has back art makes it background showing that art; every other erased
  block is cleared, back art too (batch B review). Erasing a merged map's
  dirt shows the level's own background, as erasing the Dirt layer did.
- **R6 - "Paint as"**: the brush's material (the layer's, Rock, Dirt,
  Background) is set on each world pixel whose block a stroke painted; a
  stroke of another material into a plain layer converts it to mixed
  first, in the same history step. Painting never changes back art.
- **R7 - masks**: `Image { mask }`, `editor/masks/<id>.png` (grey, 0
  erased), the oriented original's size, applied before `process_grid`;
  the grid cache keys the mask's fingerprint. A brush masks an original
  pixel when the world pixel holding its centre is covered. The first mask
  stroke gives the picture a full mask in the stroke's step; a stroke
  that changes nothing leaves no step, nor its conversion (batch C
  review). A dab reprocesses the grid: 3 ms in release for a 1024²
  original in a 256² box, so no cheaper preview.
- **R8 - Remove colour** (the Wand, W): a 4-neighbour flood of the
  original from the clicked pixel over colours alike within the tolerance
  (percent; default 10; the largest channel difference × 100 at most
  tolerance × 255), whatever their mask state; one step.
- **R9 - selecting and moving drawn layers**: Select hits, top down, an
  image's box or a visible drawn layer's covered pixel. A drawn layer moves
  only once it is selected (a first press selects), so a click on the map
  never drags a whole layer; arrows nudge it; no resize handles; the
  selected layer's box is drawn. A move is live (it rebakes the level:
  7-9 ms a move in release on Castle Strike and Liero Factory).
- **R10 - merge down**: into the drawn layer directly below, both visible
  and unlocked; per pixel rock, then dirt, then background wins, the upper
  layer on a tie (C1); the art the winner's where opaque, else the
  other's; where dirt or rock won over background, that background becomes
  back art (decision 7). The result keeps the lower layer's id, name,
  place and material, covers both boxes, and is refused beyond
  `MAX_SIDE` or `LEVEL_ART`. One step; Ctrl+E.
- **R11 - split** any visible unlocked element (image, drawn, raster)
  into Background, Dirt and Rock layers with its box, bottom to top, an
  empty one left out; the Background layer takes the back art under dirt
  and rock. A plain level still opens as all three layers.
- **R12 - masks in the history**: `Arc<Mask>`, copy-on-write;
  `Edit::Mask` holds the changed rectangle; a swap's masks count in full.
  The editor's live check counts masks in the pixel budget (batch A
  review).
- **R13 - the toolbar has three rows**: the tools and the chosen tool's
  options (Paint as, Restore, Tolerance) in a reserved slot, so choosing a
  tool moves nothing; the brush and paint; the session. One drawing row
  did not fit 1280 points under the game's theme.
- **C4 - the material property**: on a plain layer it reassigns every
  pixel; on a mixed layer it reads "Mixed", and a choice makes the layer
  plain of it (one swap).

## Level editor decisions (plan 14e)

Decided in `docs/superpowers/plans/2026-10-18-plan-14e-textures-clipboard.md`
(spec `docs/superpowers/specs/2026-10-18-level-editor-polish-design.md`)
and its batch reviews. Tools, not OLX behaviour; no format change.

- **R1 - built-in textures are code**: `olr_edit::textures::builtin()`
  makes Dirt, Rock, Grass, Brick, Metal and Wood, 64 × 64 and opaque, from
  a fixed integer hash: value noise on lattices that wrap every 64 pixels,
  quantised to 8 shades of the material's palette; bricks, planks and
  plates are patterns with a fixed shade beside each joint, so the tile's
  own seam steps like any joint. Every generator repeats exactly every 64
  pixels (`builtin_texel`). No files, no pack kind, nothing to
  licence (the spec's shipped texture pack is this).
- **R2 - the user's library**: the data directory's `textures/`, one
  `<name>.png` per texture (`olr_ui::Store::textures_dir`). Import reads a
  PNG or JPEG as an image import is (`MAX_IMPORT_BYTES`, signature first),
  refuses a side above `MAX_TEXTURE_SIDE` (512) from its header and a
  library of `MAX_USER_TEXTURES` (256, counted by name), names it after
  the file's stem (letters, digits, `-`, `_` and spaces; at most 64
  characters, cut cleanly; made unique case aside, `-2`, `-3`, …) and
  writes it through a hidden `.part` file. A change re-lists the library
  by name and decodes only textures it did not hold; opening the editor
  decodes the library once. Swatches are at most 64 pixels a side. The host lists it when the editor opens (an
  unreadable file is a warning) and answers `AddTexture` and
  `RemoveTexture` with the library re-listed (`TexturesChanged`), and
  `Failed(why)` when the change failed.
- **R3 - what paint is**: `PaintChoice::{Colour, FrontTile, BackTile,
  Texture(TextureRef::{Builtin(i), User(name)})}`, resolved to a
  `Paint::Tile` when painted: in step with the level's art origin, as the
  theme's tiles. A texture no longer there paints the default colour (and
  the choice falls back to it).
- **R4 - crater brushes**: `BrushShape::Crater(i)`, the level theme's hole
  `i` as `Shape::Stamp` (`StampMask`: covered where neither black nor
  transparent, OLX's carve and paint pixels), scale `brush size ÷ the
  hole's longest side`, clamped to 1-16; a stroke joins its dabs with more
  dabs along the segment, at most half a crater apart (at most 1024 dabs
  a move), not a thick line. A
  hole the theme lacks dabs round and reads "None" in the picker, which
  lists the theme's holes (none: hidden). A stroke paints with the paint
  it began with.
- **R5 - fill** (G): the 4-connected world pixels of the selected drawn
  layer from the clicked one, within its box and the level, sharing the
  seed's covered state and, covered, a centre colour alike within the
  tolerance (Fill's own, default 0); whole blocks painted with the paint
  and "Paint as" (shown beside Fill's tolerance; its conversion to mixed
  first, undone when nothing changed); one step; refused on a picture. A
  draw or fill of another material that paints nothing leaves a plain
  layer plain. 0.21 s in release for an open
  4096 × 2048 layer. `olr-cli level element fill`.
- **R6 - the element clipboard**: the editor's own, kept across Open, New
  and Close (`Clipboard::Elements`, `Snapshot`s and their art scale).
  Copies of drawn layers are cropped to their covered pixels
  (`drawn::crop`). A paste puts copies on top, (4, 4) further than the
  last paste, selected, as one step: new ids, names `<name> 2`, `3`, …;
  drawn and raster art resampled to the level's art scale, images
  processed again; an image shares any original here of its exact bytes,
  else takes a new name, never overwriting another picture; a raster
  pastes only into a level of its size. Cut is copy, then delete.
  `olr-cli level element duplicate` copies an element directly above.
- **R7 - pixels**: Select pixels (M) drags a rectangle on the selected
  drawn layer; Ctrl+C copies its art and cells (`PixelClip`), Ctrl+X
  copies then erases it (14d's erase: dirt or rock over back art digs),
  Delete erases it, Escape drops it (as do a tool change, Open and
  Close). A paste makes a new boxed layer "Pasted" at the copied place
  plus (4, 4), plain when its covered pixels share a material, else mixed
  (C3). The spec's floating paste is this simpler first version; the step
  after it is Merge down. A clip holds art and cells, not back art: cut
  dirt pasted elsewhere does not dig to its old background (a deviation,
  for later). The rectangle lies within its layer's box and the level (one
  off the layer is none, and says so), and goes with the selection or its
  layer. Copy or Cut with nothing to copy says so; a pasted layer's
  material is its covered pixels' most common, a tie going to rock, then
  dirt.
- **R8 - multi-select**: `Selection::Many`, two elements or more in stack
  order. Ctrl+click toggles (list and canvas), Shift+click in the list
  selects the range from the single selection; a press on a member drags
  the group; arrows nudge it; Delete, Copy, Cut, Visible, Locked and
  Material apply to each, one step each. A move, delete, hide or material
  with a locked member is refused, naming it; Locked applies to all; a
  raster member does not move. The panel shows the count, Visible and
  Locked (on when all are), Material ("Different" when they disagree);
  no Merge down or Split. The properties panel keeps 260 points at least.
- **R9 - shortcuts**: Ctrl+C, X and V as key presses and as the window's
  `Event::Copy`, `Cut` and `Paste`, never while a text field has the
  keyboard; G Fill, M Select pixels. egui-winit sends Ctrl+V only as a
  paste of the system clipboard's text, and nothing when it has none: the
  game adds an empty paste then (`app::paste_swallowed`, batch C review).
- **R10 - `olr-cli`**: `duplicate <id>` (`added element <id>`; an image
  shares its original, files of its own copied under the new id),
  `fill <id> --at X,Y [--tolerance N] <paint> [--material M]` (a seed
  outside the layer refused, nothing changed), `--texture` an alias of
  `--tile`.

## Look system decisions (plan 15a)

Spec `docs/superpowers/specs/2026-10-18-look-system-design.md`, plan
`docs/superpowers/plans/2026-10-18-plan-15a-look-system.md` (its rulings
R1-R16, as built):

- **Crates and data.** `olr-look` (layer 1, `no_std` with `alloc`, wasm)
  holds the runtime look, animation, `LookFrame` and the engine's look; it
  depends on `libm` (it needs `powf`, `floor`, `sqrt`), not `olr-math`. The
  look stays out of `olr_data::level::Level`: `LevelSource` and
  `LoadedLevel` carry `look` and `glow`; a mod's look compiles beside the
  mod (`compile_mod_look`). Glow lives inside `ColorMap` (`set_glow`) and
  shares its dirty rectangles. Look colours are hex strings, as every
  colour in the formats.
- **Loading.** A level whose look or glow does not check plays without it
  (one warning, its lights and glow dropped); `pack check` stays strict. A
  level with lights but no `look` gets the default look (full ambient). A
  16-bit grey look image reads as 8 bits. At most 1024 light objects.
- **Item shading.** `olr_scene::Shade` (self-light, marker, solid floor) is
  a field of every sprite and line. In a lit match worms, projectiles and
  bonuses never show below 50 % (the engine floor), and the rope and hook
  take it too; explosions, sparkles, spawns, the muzzle flash and beams
  show at full self-light; the crosshair, health bars and laser sight are
  markers (unlit, ungraded). Blood, droppers and gibs never show below the
  level's solid floor (playtest, 2026-10-19).
- **Lights.** Order in a frame: level lights (file order), projectile
  lights (slot), end flashes (start), muzzle lights (worm: one each, a new
  shot restarts it), explosion lights (effect slot); culled to the view's
  reach, then the first 256 (`MAX_LIGHTS`). A level light sits at its world
  pixel's centre. End flashes key on (slot, serial, kind, spawn time) and
  are remembered 2 s, so a rollback neither repeats nor erases one; the
  first re-simulated tick after a rollback reads no ends. The clock is the
  world's time plus `alpha × 10` ms; the editor has its own. A looping
  animation's default phase comes from its id; a once-only curve starts at
  0. Explosion light: (255, 200, 140), `min(60 + 8 × size, 250)` %, reach
  `24 + 8 × size`, fitted to the explosion's life; muzzle light: (255, 220,
  160), 80 %, reach 40.
- **Passes.** The lit world pass writes the albedo and a shade target; then
  the glow light (radiance cascades over the whole level, fixed to the
  world: cascade 0 a probe per cell and 4 directions, each cascade doubling
  spacing, directions and interval (2 cells at cascade 0) until the
  intervals reach the grid's diagonal; rays sphere-traced at full
  resolution with a jump-flood distance to the nearest stopper, at most 96
  steps; `GI_CELL_BUDGET` 262 144 cells), placed lights' shadow maps (512
  rays a light), the light grid's sum, soak, floor, blur, cap and steps, and
  the composite. The light grid reaches 16 world pixels past the world
  target, so soak and softness never depend on the camera. On GL nothing
  renders into `R32Float`: distances are split in two `Rgba16Float` halves.
- **Shadow edges.** A light's fade runs from its radius' edge; outside the
  level is open; a wall's face is lit. Playtest (2026-10-19): `Soft` by
  default (a penumbra as wide as the light's radius, over the neighbouring
  rays); `Smooth` (4 sub-samples) and `Hard` (world-pixel) remain a level's
  choice.
- **Own name.** No name label over one's own worm (closer to OLX, which
  draws none); its weapon name still shows while changing weapons, and the
  reload bar keeps its place.
- **Editor.** Glow brush (H), Light tool (K), a look panel and a fourth
  toolbar row (views, brush glow, the preview clock); colours are hex text
  fields (one step a commit). A glow mask edit swaps the whole mask. An
  element's glow edits strength and group; groups are edited in the look
  panel. A plain level's glow and clear flags made elsewhere are kept while
  no element glows or is clear. The canvas is lit for a look, lights, glow
  or clear solids. Playtest (2026-10-19): objects copy and paste; pixels
  picked by colour (Select pixels, By colour, Ctrl adds) or by rectangle
  take a glow (Glow selection), and the Glow brush lights a colour patch in
  a click or starts a dark glow on an element without one; copy, cut and
  delete take a rectangle selection only.
- **Test content.** `fixtures/look/levels/look-test/` and
  `fixtures/look/mods/look-test-mod/` (Glow Rocket); no rocket scenario
  (scenarios replay OLX traces, which cannot load these packs): the play
  golden is `olr-cli render-match` of chaos bots (`look-rocket.png`).

- **Stop 1 (2026-10-18), the user's verdict on the first lit renders:** the
  look is good, but rock read black (only faces that saw a light were lit).
  Light now soaks into solids (`light_soak`, 0-12 world pixels, a separable
  maximum faded by depth) and a level sets the least light a solid shows
  (`solid_floor`); the light grid's pad grew to 16 world pixels so neither
  depends on the camera. Clear solids (`look/clear.png`, an element's
  `clear`) let light through as air while worms and projectiles collide.
- **Stop 1b (2026-10-18):** the defaults for a level with a look are soak 8
  and a white floor at 30 % (the recommendation, taken as provisional: the
  user expects to retune them after playtests). World-pixel shadow edges
  stay; 2 × 2 shadow sampling remains the planned fix if they look coarse
  in play.
- **Playtest (2026-10-19):** the user likes the look. Shadow edges: from
  a comparison of hard, anti-aliased, soft and blurred, lamps' shadows are
  `Soft` by default (a level's `shadows`: `Hard`, `Smooth`, `Soft`).
  Objects (spawns, lights) copy, cut and paste in the editor.
- **Ambient occlusion** (issue #10): a level's look may darken the
  ambient light in air near rock and dirt (`occlusion`); see "Ambient
  occlusion (issue #10)" below.

## Worms in the light (plan 15b)

Spec `docs/superpowers/specs/2026-10-19-worm-lighting-design.md`, plan
`docs/superpowers/plans/2026-10-19-plan-15b-worm-light.md`, as built:

- **The worm floor.** In a lit match a worm, its rope and hook show at
  least `olr_look::engine::WORM_FLOOR` (35 %) of the level's ambient hue at
  full brightness (`worm_floor_of`: the ambient colour as set, scaled until
  its brightest linear channel is 1; its intensity ignored; black gives
  white; a hue darker than half of 35 % white's luminance is mixed towards
  white to that half). Without a level look (a mod's look alone) it is 35 %
  white. `olr_scene::Shade`'s `floor` (`Floor::None | Solid | Worm`,
  `Shade::WORM`) replaces `solid_floor`; projectiles and bonuses keep the
  engine `FLOOR` (50 %) as self-light. The user saw it on a darkened look
  test level and kept 35 % (2026-10-19).
- **Worms block light — removed by issue #53** (glitches outweighed the benefit, the maintainer's call, 2026-10-23: worms
  no longer block any light, and the light grid is byte for byte the same
  with or without them, `worms_change_no_light`; the occluder map is
  still drawn: the worm halo of issue #54 reads it). As it was (since 2026-10-20 only over 4
  world pixels: see "Glowing light (plan 15c)", short worm shadows): the scene lists each living worm's body (its skin
  frame) as an `Occluder` (`DrawList::occluders`, worm index + 1) in a lit
  match, whatever the view. The renderer draws them each lit frame into an
  `R8Uint` occluder map, one texel per world pixel over the light grid
  (`LookGlobals.occluders`). The placed and engine lights' shadow march and
  the glow light's emission treat a worm texel as solid; a light ignores
  the worm it belongs to (`FrameLight::owner`: a muzzle flash, whatever the
  skin's art at its centre, the final review's Important 1), else the worm
  its centre is in (an explosion in a worm). The
  map covers only the light grid: a worm more than 17 world pixels outside
  the view throws no shadow into it, and the glow light (otherwise fixed to
  the world) is blocked only by worms in it. Worms take no soak and no
  solid floor.
- **The lit side — removed by issue #53** (glitches outweighed the benefit, the maintainer's call, 2026-10-23: a worm
  pixel now takes the light where it is, as a rope or hook pixel does,
  floored by the worm floor, `a_worm_takes_the_light_where_it_stands`).
  As it was: the composite lit a worm pixel (`Floor::Worm` over a
  texel of the same worm in the occluder map) with the light just outside
  the worm on its side: the outward direction is the sum of `w - q` over the
  worm's pixels in a 7 × 7 window (up when they cancel), and the light was
  read one pixel past the first non-worm pixel within 4. One value per world
  pixel. A rope or hook pixel (no occluder under it) took the light where
  it was. The worm floor still bounds it from below.
- **Light never passes rock, however thin** (the user's request,
  2026-10-19): the shadow march walks every world pixel a ray crosses
  (a grid walk; the old 1-pixel steps passed between rock pixels touching
  at a corner) and stores where it enters rock; an air pixel is lit only
  before that entry (half a pixel of slack), a solid one within its face
  (1 pixel); the softness blur stops at walls for air. Glow light already
  stopped at 1-pixel walls (`a_one_pixel_wall_stops_glow_light`).
- **Not measured yet:** the occluder pass's GPU cost (`olr-cli gpu-bench
  --look`) waits for the user's consent. Since issue #53 the light takes
  no occluder cost: no facing lookups, no level-wide occluder copy, no
  worm reads in the emission, cascades or soak; only the occluder map's
  draw remains (in the world group).

## Worm halo (issue #54)

Part 2 of 2 of the worms' look after #53 took them out of the light: a
steady cue that keeps a worm readable and lifts it off the level. No OLX
counterpart (OLX draws no light). As built:

- **A centred halo, not a shadow** (the maintainer's choice,
  2026-10-23): a soft dark outline on the background behind each living
  worm, with no direction, so it cannot drift, pop or flip. It is drawn in
  the composite (`composite.wgsl`'s `worm_halo`; no new pass) and only in
  lit frames: an unlit match draws today's bytes, and the light grid is
  untouched (`light_is_camera_independent`, `worms_change_no_light`).
- **Only on the background**: the level's back (terrain texels whose
  world pixel is not rock or dirt, the opaque backing under see-through
  pixels included) and distant layers (issue #11). Never on rock or dirt,
  a glowing texel (a lamp on the back wall, and so a glowing sky such as
  Castle Strike Reloaded's, takes none), any sprite or the worm. An
  opaque `Shade::LIT` sprite writes the terrain's shade, so the lit
  terrain and the distant layers write a **background tag**, 0.25 in the
  shade's marker channel; the composite reads (0.125, 0.375) as
  background and 0.5 or more as a marker, as before. A see-through sprite
  over the background blends the tag down and takes little or no halo
  (accepted). With the halo off the tag changes no byte
  (`the_tag_changes_no_other_byte`; `lit-hashes.ron` drawn with the halo
  off was #53's).
- **Shape**: per world pixel (never finer than the art), `d` the distance
  from its centre to the nearest pixel of any living worm's body in the
  view's occluder map (1 right next to the silhouette), `x = (d − 1) /
  r`: the light the texel takes (ambient and glow light, capped, and the
  bloom on it; not its self-light, floor or grade) is scaled by `1 − s ×
  (1 − x)²`, nothing from `x = 1` (`olr_look::engine::worm_halo`, the
  CPU twin). The search runs only within `ceil(r)` of a worm box (at most
  11 × 11). Overlapping halos take the strongest (the nearest worm),
  never the sum. Each view draws it from its own occluder map, which
  covers the view, its margin and `LIGHT_PAD`, so a worm just outside a
  view darkens its edge as a wider view shows it.
- **Depth**: 0 for the level's back, `1 − shade alpha` for a distant
  layer (8-bit: compared with half a step of tolerance). `r` and `s` go
  linearly from their depth-0 values to `WORM_HALO_RADIUS_MAX` and 0 at
  `WORM_HALO_FADE`; nothing at or past it (a night sky at depth 90).
- **Final constants** (`olr_look::engine`, prepended to the shaders):
  `WORM_HALO_RADIUS` 2 world pixels, `WORM_HALO_RADIUS_MAX` 5,
  `WORM_HALO_STRENGTH` 0.5, `WORM_HALO_FADE` 0.6: the issue's starting
  values, kept after looking at the re-blessed `look-rocket.png` (a clear
  dark outline round the worms on the lit back wall) and `render-match`
  frames of Castle Strike Reloaded and its night playtest copy and of
  `look-test-distant` on the software adapter. Not yet judged in play
  (`docs/deferred-verification.md`).
- **Not done** (the issue's non-goals): no light-dependent or directional
  shadow, no per-player toggle, no per-level or per-mod setting, no halo
  for ropes, hooks, projectiles, bonuses or dead worms.

## Glowing light (plan 15c)

- **Render profile (2026-10-24):** `cargo xtask profile` ranks a heavy
  lit frame's costs by phase (`docs/perf/2026-10-24-render-profile.md`):
  the glow light's cascades are 78 to 90 % of every frame on the RTX 5070
  Ti and on the integrated GPU, whatever the output size; the heat's own
  passes are nearly free.

Spec `docs/superpowers/specs/2026-10-19-glowing-light-design.md`, plan
`docs/superpowers/plans/2026-10-19-plan-15c-glowing-light.md`, as built
(the spec's "My calls" are binding; the user approved the day renders,
2026-10-19):

- **One light model.** The glow light's radiance cascades are the only
  light: the placed lights, their shadow maps and the `shadows` setting
  are gone from the format (`LevelObject::Light` and `look.shadows` are
  errors naming them), the editor (the Light tool, K) and the CLI (`level
  light`). The night Castle Strike lost its light objects. Light never
  passes rock or dirt however thin: the cascades walk exactly cell by cell
  near stoppers (a corner both of whose side cells stop is closed) and
  merge only upper probes whose ray start the lower ray's end can see (the
  bilinear fix; without it a 1-px wall leaked 0.53 behind a flash).
  `GI_CELL_BUDGET` is 2^20, so Castle Strike's light has a cell a world
  pixel. **Caveat (a deviation, the final review's Important 4):** a level
  of more than 2^20 world pixels (2048 × 1024 and up), or one a device's
  texture limit crops, gets cells of 2 or 4 world pixels, and there a wall
  one pixel thin can share a cell with air and let light through. The
  guarantee holds at a cell a world pixel. A march that runs out of
  steps counts as blocked, and a disc sum, a ray and the history are
  clamped and never NaN (Importants 1-2).
- **Play's lights as glowing discs.** Explosions (`explosion_radius`, 2-12
  by size), muzzle flashes, shot lights and end flashes (`disc_radius`, an
  eighth of the reach, 1-6) are `LookFrame::emitters`, drawn as instanced
  quads into a dynamic emission layer (a cell whose centre is in the disc,
  in the open, takes its radiance whole; reach max-blended, unsplit f16).
  `disc_radiance` calibrates each so its light at `d0 = max(reach / 3, 2r)`
  (at most halfway to the reach) matches the old point light. A disc's
  cells never count as a worm. Discs are capped at `MAX_EMITTERS` (1024) in
  order, the engine's explosions and muzzle flashes pushed before mods'
  shot lights and end flashes (Important 3), and never culled to a view: bounced light may reach the view from
  past a disc's reach, so culling made light camera-dependent. A disc
  outside the level emits nothing.
- **Bounce.** A ray meeting rock or dirt takes the cell's albedo (its art,
  linear) × the last frame's fluence of the open cell it came from (never
  through the wall) × `engine::BOUNCE` (0.6, clamped 0-0.9). The history
  is the fluence copied after each frame; a new level, a frame without a
  look or one with no glow starts it afresh.
- **Independent flicker.** A `Flicker` group's flicker is the renderer's:
  each 4-connected region of its pixels (labelled on the CPU, only for
  groups that flicker, again for regions touching a changed rectangle)
  flickers with its own seed, value noise like `olr_look::anim`'s on a
  32-bit hash. `Pulse`, `Keys` and `Steady` stay group-wide.
- **Glare.** The light grid is toned (`olr_render::tone`: the identity up
  to 1, a smooth shoulder to `max_light`; a white point of 1 clips) and
  banded after. The bloom spreads the light above 1 (soft-saturated)
  over the light grid, 12 world px (`BLOOM_RADIUS`, a third of it the
  Gaussian's sigma; `LIGHT_PAD` 28), stopping at rock for air, added at
  0.6 before the grade. A glowing pixel shows its own art: other light
  lifts it to 1 at most, no glare lands on it and it feeds none (a
  daylight sky lights the level without washing out; a flame's
  luminosity is its halo). Opaque rock and dirt likewise show at most
  their art, with no glare on them (#1, the day Castle Strike's playtest:
  rock beside the sky soaked light above 1 and took the bloom, so the
  red-roofed building read oversaturated, brighter than OLX's art);
  `max_light` above 100 % and the bloom brighten only the open (air,
  clear solids) and play's items over it.
- **Short worm shadows — removed by issue #53** (glitches outweighed the benefit, the maintainer's call, 2026-10-23:
  worms block no light at all; `WORM_SHADOW`, the emission's worm marks
  and the soak's worm-shade rows are gone). As it was (the user's playtest
  of the day Castle Strike, 2026-10-20: outdoors a worm's shadow was massive, popped at the edge of
  the glowing sky and drifted after the worm stopped). A worm blocks a
  ray only within `olr_render::WORM_SHADOW` (4) world pixels of the ray's
  probe; past that it is invisible to light and the cell shows what lies
  under it (its glow, its rock). The emission marks a mostly-worm cell 2
  + its rock share, and a worm's pixels keep the glow under them. The
  cascades' merging carries the shade a little further, nothing more than
  10 px from a worm (`a_worm_throws_no_long_shadow`,
  `a_worm_changes_no_light_away_from_it`); the lit side (plan 15b) and the
  worm floor hold. This replaces plan 15b's "worms block light" for the
  glow light. The shade colours only the rock face: from 2 px into rock
  the soak takes, for a texel near a worm, the nearest unshaded light
  along its row (the user's playtest: it went 12 px deep).
- **The idle bot** (`Personality::Idle`): presses nothing, not even to
  respawn (the server's `MaxRespawnTime` brings it back); online wire
  index 2.
- **Castle Strike by day** (`content/playtest/olr-level-castlestrike-day`;
  since issue #7 Castle Strike Reloaded, `content/reloaded/olr-level-castlestrike-reloaded`).
  The user found explosions perhaps a bit bright: a later milestone, to
  verify by playthrough.
- **Not measured yet:** the cascades' cost at a cell a world pixel and
  the bloom passes, on Windows at 1080p with the user's
  consent. Issue #34 built the tools (`docs/perf/graphics-tools.md`):
  `olr-cli gpu-bench` prints each pass group's GPU time, `bench-quick`
  draws a lit Castle Strike frame, and `fixtures/render/lit-hashes.ron`
  holds a lit match's exact bytes; the Windows table is deferred
  (`docs/deferred-verification.md`).
- **Issue #35, picture-identical optimisations** (each checked against
  `lit-hashes.ron`): no glow-light history copy (the cascades read the
  fluence before `fs_fluence` rewrites it), the flood's seeds `Rg16Uint`
  and the reaches `Rg16Float`/`R16Float`, the disc layer zeroed only under
  last frame's discs, the soak's worm search only near the worms' boxes,
  the terrain uploaded by dirty tiles of 16 world pixels (stains colour
  only), no across blur or soak at 0, and only the frame's groups packed.
  On lavapipe the lit Castle Strike frame went 188.5 → 174.6 ms and 37 → 36
  passes, a tick's terrain uploads 87 % fewer instructions. No deviation
  from OLX (the renderer is OLR's own). The bigger ideas, which change
  the picture (static/dynamic split, fewer flood passes, cached merge
  visibility, the light per tick, a smaller budget), are weighed in
  `docs/perf/2026-10-graphics-candidates.md`; the Windows before/after
  table is deferred (`docs/deferred-verification.md`).
- **Follow-ups** (the final review, 2026-10-20; open):
  - *Important, the user's call:* thin walls leak at cells of 2+ world
    pixels (levels over 2^20 px, or cropped by the device): raise
    `GI_CELL_BUDGET` (about 64 B of GPU memory a cell) or accept it.
  - *Owed before trusting it on hardware:* `cargo xtask full` and the
    Windows 1080p measurement (`olr-cli gpu-bench --look` on a recorded
    day Castle Strike match); the f16-overflow and step-exhaustion guards
    were never seen to fail on lavapipe, so watch for NaN or leaks there;
    check steady scenes keep the same bytes (the f16 bounce feedback could
    settle in a one-step cycle on hardware).
  - Explosions perhaps a bit bright (the user, 2026-10-19): a later
    milestone, by playthrough.
  - Small discs (cells lit whole) pulse slightly as they move sub-pixel:
    normalise by πr² over the cells lit, or a radius of at least a cell.
  - Flicker regions relabel a whole big flickering region per nearby dig,
    and `Regions` holds about 7 B a world pixel even when nothing flickers
    (allocate lazily; bitset stamps).
  - Bloom no longer comes from glowing pixels' own brightness (a "My
    calls" item changed by Task 8's ruling; renders approved): confirm.
  - `bench_look` has no `Flicker` group (`--look`'s doc no longer says
    placed lights and soft shadows: issue #34).
  - Overlapping discs share the largest reach (max-blended).
  - A faint static star-ray pattern around small bright sources (the
    cascades' directions), seen in the look goldens.

## Loadouts (issue #5)

OLX chose a match's weapons with a `.wps` restriction preset
(`WeaponRestrictionsFile`, `src/common/CWpnRest.cpp`), set the loading time
apart (`LoadingTime`, `src/common/FeatureList.cpp:31`) and could drop
bonuses (`Bonuses`). OLR puts the weapons and the loading time together in
a **loadout** and plays without bonuses. Deliberate deviations:

- **Loadouts** (`olr_data::loadouts::MatchLoadout`): a name, the weapons
  on, the loading time (0-500 %, OLX's range) and the **Unique** rule (not
  OLX's): no weapon twice in a worm's five slots unless every weapon on is
  already in one (`unique_ok`). Every mod has `Default`, which is no data:
  every weapon, 100 %, Unique off. It is listed first, a fresh install's
  choice and the fallback for a loadout that is gone, in Local Play, the
  lobby, the server's rotation and `olr`'s command line.
  `DEFAULT_PRESET` (Standard 100lt) stays only for the oracle and the
  developer tools (`olr-cli`, scenarios).
- **Shipped loadouts** are the imported presets (`ModDef::loadouts`, by
  preset name; `olr-import-lgs`'s `PRESET_LOADING_TIMES`): the weapons the
  preset enables (`CWpnRest::updateList`; a weapon in the `Bonus` state is
  off, bonuses being gone), Unique off, 20 % for Standard 20lt, Rifle only
  and Mortar Only, 100 % for the rest. A preset enabling no weapon makes no
  loadout (Classic's Bonus only). They are read-only; the player's own
  (`olr-ui`'s `loadouts.ron`, per mod, unique names ignoring case, a
  shipped name refused) never lose a loadout: a weapon a mod update
  removed is left out, with a warning, only when it is played.
- **Unique in play:** the weapon selection steps over weapons another
  slot holds (`olr_ui::weapon_select`); a bot's repeats are replaced by
  the first unused enabled weapon in a deterministic draw
  (`olr_bots::unique_weapons`; no draw without the rule, so the bots of a
  loadout without it choose as OLX's); an online player's choice that
  breaks it, or holds a weapon the loadout has off, is replaced by random
  weapons from the match seed on every peer alike (`olr_online::matches::
  resolve`).
- **Bonuses are gone from play:** Local Play's checkbox, `olr --bonuses`,
  the lobby's setup (`MatchSetupWire`), the server's rotation field (an
  older config's `bonuses` is ignored with a warning) and `/settings
  bonuses`. The simulation keeps `SimSettings::bonuses`, ported and
  checked by the oracle; `olr-cli`'s specs and the tests keep using it.
- **Local Play** lost "Reset the match to OLX's settings", the loading
  time slider and the preset picker; an older settings file's
  `restrictions`, `loading_time` and `bonuses` are ignored (it plays
  `Default`).
- **Online:** the host's loadout travels in full in the setup (protocol
  4; at most `MAX_LOADOUT_WEAPONS` 128 names, a larger one sent as
  `Default`); a server's rotation entry may name a shipped loadout.

## Reloaded levels (issue #7)

OLX shipped its levels; OLR ships them (imported, `content/packs`) and
its own remade "Reloaded" levels beside them:

- **A second bundled content root, `content/reloaded/`,** for OLR's own
  hand-made packs: `content/packs` holds only what the OLX import makes
  (`cargo xtask check` compares it with a fresh import), so hand-made
  packs cannot live there. The programs lay it over the content wherever
  it is opened (`olr_content::bundled_roots`, `Content::open_bundled`):
  the game (the user's data `packs/` on top, then `content/reloaded`,
  then `content/packs`), `olr-server` and `olr-cli`'s commands, from
  `--content` or `default_content_dir` (`reloaded` beside it). `cargo
  xtask dist` ships it in the Windows zip and the Flatpak. A higher layer
  still hides a lower one's pack of the same id (spec §9.4); the two
  shipped roots share none (a test). Bundled, so the editor does not edit
  them in place (`--edit` and the Editor menu).
- **Liero Factory Reloaded** (`olr-level-liero-factory-reloaded`): OLX's
  Liero Factory with a look (the factory at night, its ceiling lamps,
  screens, displays and roof lights glowing, a faint moonlit sky); its
  material, front and back are the imported level's byte for byte
  (tested), so it plays as OLX's. The look was judged from
  `olr-cli look-preview` renders on the software adapter, not yet
  played.
- **Castle Strike Reloaded** (`olr-level-castlestrike-reloaded`): the
  day Castle Strike of plan 15c, moved from `content/playtest/`
  unchanged but for its id and name. The night one stayed a playtest pack
(`content/playtest/`) until the maintainer had it removed, with its
test, on 2026-10-25.
- Both are in the example server config's rotation
  (`fixtures/server/example.ron`), and `dist`'s verification server
  loads them.
- **Parallax skies (issue #63, part 2 of #62).** Both skies lie on a
  glowing distant layer, made by `tools/levels/reloaded_skies.py`
  through `olr-cli level element` (`content/reloaded/README.md`): the
  sky region (Castle Strike's sky glow mask, Liero Factory's `sky`
  layer) erased from every back element (`erase --mask`, a PNG added to
  every shape option for it), and a level-sized image at (0, 0) taking
  the old sky element's glow: Castle Strike's at depth 85 (glow 100,
  group `sky`), Liero Factory's at depth 90 (glow 35, `sky`, locked as
  the old `sky` layer was, which is gone). At the reference placement
  each image shows the old sky's art at the old sky's pixels, so the
  baked glow maps (`look/glow-back.png`, `groups-back.png`, the surface
  maps) are byte for byte as before and the light grid's hashes too
  (`lit-hashes.ron`'s for Castle Strike, `olr-cli frame-hashes` with the
  same flags for Liero Factory); only the drawn sky changed. Behind the
  hills and the buildings, seen once the camera moves, each radial
  gradient goes on from its own colours (fitted centres: Castle Strike
  (724, 291), Liero Factory (592, 226)); the factory's gets about 1
  star per 230 pixels of the part ever shown, a fixed seed. Only the
  middle of a level-sized layer is ever shown in play (`view + (1 −
  depth)(size − view)`); the rest lights the level from the reference
  placement. Liero Factory Reloaded's front and back are now the
  imported level's but where the sky is see-through (the distant sky's
  colours there, OLX's but for the stars; tested); the material is
  still byte for byte. Deviation for the eye: a depth-90 layer takes a
  tenth of the light beyond the ambient, so the factory's sky draws
  darker than the old sky element, lit by its glow at 35 % (listed for
  the playtest in `docs/deferred-verification.md`).
- Renamed Liero Factory After Dark (issue #70);
  `olr-level-liero-factory-reloaded` is free for the asset-built level
  (#96). Its pack is `olr-level-liero-factory-after-dark`, and five
  element ids are shorter so every shipped path stays within 80
  characters (`rock-scr`, `rock-disp`, `rock-bcn`, `ctrl-scr`,
  `backdrop`, were `rock-screens`, `rock-display`, `rock-beacons`,
  `ctrl-screen`, `background`); the bake is byte for byte the old
  factory's (`tests/reloaded.rs`' `after_dark_bakes_as_the_old_factory`,
  and `castle_strike_bakes_as_before` for the other level, hashes taken
  before the renames). The four built-in assets cut from the renamed
  layers carry the new ids, their pieces unchanged. Nothing maps the old
  id: replays and settings naming it find no level until #96.

## Liero v1.0 Reloaded (issue #21)

The 0.2 scope's Reloaded mod: OLX's Classic ("Liero v1.0") with OLR's
look, shipped in `content/reloaded/olr-mod-liero-v1-reloaded/`.

- **Gameplay is Classic's.** Made with `olr-cli pack new mod --from
  content/packs/olx-classic`; its `origin` is Classic's (OLX
  `share/gamedir/Classic` at `d1ad953f`), as Liero Factory Reloaded
  records Liero Factory's. Only the name and `look` blocks differ:
  `olr-cli`'s `tests/reloaded.rs` compares the compiled `ModDef` (but for
  the name), the assets, the simulation's `SimMod`, presets and physics
  with Classic's, plays the same bot matches on both (OLX's bots on Dirt
  Level, chaos bots on Castle Strike Reloaded) to the same checksums, and
  arms OLX's bots alike under every preset (`olx_bot_weapons` matches
  "Liero v1.0" in the name: rifles at 16-25 %, the 100lt set at 100 %).
  `olx-classic` itself, the goldens, the oracle and the benchmarks are
  unchanged and keep naming Classic.
- **The look** (`content/reloaded/README.md` lists every light): glowing,
  tunnel-lighting rockets and missiles, the laser's red muzzle light, the
  flamer's flames, napalm's burst and burning drops, the Blaster's ball
  and burst, the Gauss Gun's red tracers, the Zimm's pulsing ball and
  flash, the Uzi's tracer rounds and stronger muzzle flashes for the
  Super Shotgun, Cannon, Rifle and Winchester. Crowded projectiles (the
  Crackler's and Hellraider's balls, the Blaster's sparks) only glow;
  plain bullets, grenades, mines, nukes and dirt stay dark. Issue #15
  gave it explosion lights: hot, wide blasts for the big explosives, a
  cave-wide flash for the nukes, medium ones for grenades, rockets and
  the like, and a small one (`mod.ron`'s default) for bullets and
  shrapnel. Judged from `olr-cli render-match` renders on the software
  adapter, not yet played.
- **Deviation: the default mod.** A fresh profile (no `settings.ron`
  match) plays Liero v1.0 Reloaded, locally and hosting
  (`olr_ui::DEFAULT_MOD`, `MatchPrefs::default`; the maintainer's call, so
  a playtest starts on it); OLX's menu starts on Classic. A saved choice
  stands; where the mod is not installed the menus play the first listed
  mod. The command line's games (`olr --mod`, default "Classic"), the
  level editor's Play mod and the tools keep Classic.

## Level editor layout (issue #8)

The maintainer's "option B": the casual path (import an image, adjust,
place, say what it is, make parts glow) obvious, the rest one click
away. A layout change only: no tool, operation or format changes.

- **Left: the tool rail** (`ui/rail.rs`, `ui/icons.rs`): Import (set
  apart), Select, Draw ▸, Pixels ▸, Glow, Spawn; each an icon drawn in
  lines (no emoji) over a short label. `ToolGroup::{Draw, Pixels}`
  (`tools.rs`) hold Brush, Eraser, Fill, Rectangle, Ellipse, Line and
  Select pixels, Remove colour, Eyedropper. A group's button keeps its
  group's name as label; its icon and hover are those of the group's tool
  chosen last (`EditorState::last_tool`, set by any `Tool` action,
  shortcuts too), and a click on it chooses that tool again. A second
  click (the group's tool already chosen), a click on its corner mark or
  a right click opens its flyout: the tools with their shortcuts; a pick,
  Escape, a click elsewhere or a press on the canvas closes it (a stroke
  is no click; the canvas closes every popup on a press). Escape while a
  popup is open closes only the popup (the pixel selection stays). The
  rail's Glow is named `labels::TOOL_GLOW` ("Glow brush", `Tool::Glow`'s
  name now) for AccessKit, never the element's Glow checkbox.
- **Top: two bars** (`ui/toolbar.rs`). The session bar: File ▾ (Open…,
  Save, Close), Undo, Redo, the unsaved mark's kept space; at the right
  View ▾ (Collision view, Grid, Lit / Full bright / Light only, Materials
  view since issue #43, the lights' clock) and Play with its ▾ ("Play options": the mod, a list,
  and the bots). The ▾ are drawn (a font may lack the glyph); the menus
  are named without them (`labels::FILE`, `VIEW`, `PLAY_OPTIONS`). View
  and Play's menus stay open while used (a click outside closes them).
  The tool options bar shows the chosen tool's options only, by what the
  tool uses (`Tool::uses_brush`: the painting tools and the Glow brush;
  `Tool::lays_paint`: Brush, Rectangle, Ellipse, Line, Fill): the brush
  (Round and Square as icons, Crater, Size), the paint (the colour
  button, which chooses the colour and opens its picker; Front tile, Back
  tile, Textures…), then the tool's own: Paint as, Restore, Tolerance;
  By colour (a slot of its own, its tolerance shown or not) and the glow
  % ("Glow %", one value) for the Glow brush and Select pixels, which
  also has Glow selection (a tool change drops the pixel selection, so it
  cannot be the Glow brush's). Select, Spawn and the Eyedropper leave the
  bar empty, as tall as ever. The playtest's rules hold: pickers keep a
  fixed slot (a long choice is cut), the brush sits in one place for
  every tool that dabs, and everything fits 1024 points under the game's
  theme (`the_toolbar_stays_in_the_window_and_in_place`,
  `a_toolbar_row_is_one_straight_line`).
- **Right: one panel** (`ui/elements.rs`, `ui/properties.rs`): tabs
  Elements | Assets | Spawns (`PanelTab`, `EditorAction::Tab`); Assets is
  the asset library (issue #9, below). The tab follows the selection
  (`update`: an element or group selected shows Elements, a spawn
  Spawns, on any change of the selection and on any `Select` or a press
  with Select or Spawn); otherwise only a click changes it. Under the
  tabs, the selection's properties: Name; "What is it?" (Rock, Dirt,
  Background as one switch; a mixed layer none of them, "Mixed" under
  it); Glow (the checkbox, strength, group, and "Make parts glow…": the
  Glow brush with By colour on); Place (X, Y, an image's Width and
  Height); "More" (Visible, Locked, Lets light through, Split into
  layers, Merge down, an image's pixel size, colours, dither,
  threshold), folded at first. With nothing selected, the level (name,
  notes, theme, art scale, resize) and its Lighting (Add/Remove look,
  ambient, the glow groups), the rest of the look under "Fine tuning"
  (max light, steps, softness, soak, solid floor, the grade). "More" and
  "Fine tuning" are remembered in `EditorSettings` (`more_open`,
  `fine_tuning_open`; `ShowMore`, `ShowFineTuning` answer
  `SaveSettings`). A group's and a spawn's panels keep their content
  (the group's material is the same switch).
- **Labels** (`ui::labels`): `OBJECTS` and `MATERIAL` went; new ones name
  the bars, the rail's groups, the tabs, "What is it?", "Make parts
  glow…", "More" and "Fine tuning". `ui::relabel` names a widget for
  AccessKit whatever it shows. The e2e editor script (`fixtures/
  autoplay/editor.ron`) follows: Draw's flyout for the brush, Make parts
  glow… instead of plan 15c's removed Light tool, the Spawns tab, File's
  Save and Close.

## Level editor asset library (issue #9)

Ready-made pieces of a level, saved once and dropped into any level. No
OLX counterpart: OLR's own. The format is `docs/formats/asset.md`
(`olr_format::assets`), the operations `olr_edit::asset`, shared by the
editor and `olr-cli asset save|place|check`.

- **An asset is a folder** `<asset-id>/`: `asset.ron` (name, the art
  scale it was made at, size, composition elements placed from its
  top-left, the glow groups they name, with their settings), the
  elements' files as a composition's `editor/` holds them without
  `editor/`, and `thumb.png`. No raster element (an asset is a piece, not
  a level). Untrusted: read with the level format's limits, ids and
  sources checked before any file is read, refused with a message. No
  version field or shims: the format is new.
- **The thumbnail** is the asset baked alone and drawn lit by
  `level_thumbnail` under a fixed dark preview look (ambient `#304060` at
  20 %, the asset's groups), so a glow shows: under a full-bright look a
  glow adds nothing.
- **Libraries**: built-in in `content/assets/` beside the content
  directory (read-only, shipped by `dist`, not a content root: assets are
  not packs, so the fresh-import check and the pack loaders never see
  them); the user's in `<data>/assets/`, at most 256 (`MAX_USER_ASSETS`,
  as textures). Sharing is copying a folder in, or Import asset… (the
  folder checked and decoded whole, then written anew under a folder name
  of its own; a name clash numbered). Folder names are slugs of the name
  (`torch`, `torch-2`).
- **Save as asset** cuts the selected elements (each cropped to what it
  covers; a picture whole), or with a pixel rectangle the world rectangle
  from every visible layer (not clipped to the layer the rectangle was
  dragged on, as the copy's is): a picture wholly inside kept whole (its
  original and settings), anything else cropped as drawn pixels (a
  layer's art, cells and back art; a picture partly inside as its baked
  pixels); empty pieces, all-zero glow masks (their group not carried)
  and pieces identical in pixels, material and glow to one below are
  dropped. `olr-cli asset save --layers` restricts the layers (the torch
  of Castle Strike Reloaded: `background-2`, a near copy of `background`
  with holes, would come along otherwise). The name is asked; a name of
  one of the user's asks Replace or Keep both.
- **Placing** (a thumbnail dragged onto the canvas, or clicked and then
  a press on the canvas; its top-left at the point, its box outlined at
  the pointer meanwhile) is one undo step, as a paste: new ids and names
  (the asset's name for a one-piece asset, "<asset> <piece>" otherwise,
  numbered when taken), resampled to the level's art scale, masks kept,
  the new elements selected. Its glow groups come first in the same step:
  a group the level has (same id) is joined and keeps the level's
  settings; a missing one is added with the asset's (not a new group's
  defaults). A level without a look gets `LookSource::default()` and the
  groups, and the status bar says "Added lighting for the asset's glow:
  lower the ambient light in the level's lighting to see it": the
  maintainer's choice over darkening the level unasked. A placed asset is
  ordinary elements; no level refers to its folder.
- **Remove** asks first and takes only the user's; built-in assets
  cannot be removed. The editor stays pure: the host lists, reads,
  writes, imports and removes (`ReadAsset`, `SaveAsset`, `ImportAsset`,
  `RemoveAsset`, answered by `AssetsChanged`, `AssetSaved`, `AssetRead`).

## Ambient occlusion (issue #10)

No OLX counterpart (a look setting, presentation only; direction E of
`docs/superpowers/specs/2026-10-19-lighting-evolution-brainstorm.md`).
The maintainer's rulings, as built:

- **Per level, off unless set.** `look.occlusion: (strength, reach)`
  (`OcclusionSource`): strength 1-100 percent (0 refused in a file: absent
  is the only off), reach 2-32 world pixels (`olr_look::MIN_OCCLUSION_REACH`,
  `MAX_OCCLUSION_REACH`), default 12. No OLX level and no shipped level
  sets it; no engine default, no per-player setting. In `level.ron`, so
  `content_hash` follows it.
- **What it darkens.** Only the ambient term of the light grid's sum, and
  only in air (the solid map: opaque rock and dirt; background and clear
  solids are air; outside the level is open). Glow light, the sky's
  `Unlimited` glow, lamps, explosions, muzzle flashes and bounce are not
  occluded (the cascades already shade them near walls). Rock and dirt
  keep their ambient; soak and the solid and worm floors are unchanged, so
  a worm beside a wall shows at least the worm floor. The bloom may drop a
  little where ambient plus glow was above 1 (accepted).
- **The model (the builder's call).** Not a distance field: the
  Gaussian-weighted share of opaque solids within the reach (sigma a third
  of the reach, taps out to it), in two separable passes per world pixel:
  `fs_occlusion_h` weighs each row from the world-size solid map into a
  texture of the light grid's width and its height plus
  `MAX_OCCLUSION_REACH` rows above and below, and `fs_sum` weighs the
  rows down. The share is divided by a thick flat wall's face share and
  saturates smoothly (`1 - (1 - x)²`, flat where it meets 1): air at a
  thick wall's face loses the strength, a corner, a pocket or a narrow
  tunnel loses as much further out (they read deep), a thin wall takes
  less (a 1-pixel wall about 40 % of the strength at reach 12), and it
  fades smoothly to nothing by the reach. A pure distance field (the
  brainstorm's wording) would darken a corner no more than a flat wall.
  It is per world pixel, then blurred by `softness` and banded by
  `light_steps` with the rest of the light; camera-independent (the rows
  read the world-size map past the grid); a dig is occluded the next
  frame (the solid map follows the dirty rectangles).
- **Cost.** Off, no pass runs and the sum's branch on a uniform keeps
  the frame's bytes; the rows texture (`Rgba16Float`, grid width × grid
  height + 64) exists with every lit target. On, two passes of up to 65
  taps a world pixel. `olr-cli gpu-bench --look --occlusion S[,R]` draws
  the bench look with it; the Windows 1080p measurement waits for the
  maintainer (`docs/deferred-verification.md`).
- **Elsewhere.** `olr-cli level look --occlusion S[,R]` / `--no-occlusion`;
  the editor's Lighting panel (Occlusion % and, when on, its reach, right
  after the ambient colour; 0 removes it). The lit thumbnail leaves it out
  (as soak and softness). Golden: `look-occlusion.png` and
  `look-occlusion-light.png`, a copy of the look test level with ambient
  `#405880` at 40 and occlusion 80 over 16 px (the look test level's own
  ambient is too dim for it to show).

## Distant (parallax) layers (issue #11)

No OLX counterpart (decoration only; OLX levels and themes have none).
As built:

- **What is distant.** A Background element of the level editor, an
  imported image or a plain drawn layer (not mixed, no back art, not a
  raster), with a depth of 1-99 percent ("near" to "far"); no glow, not
  clear. Every distant element lies below every other; their order is the
  element order (far first). The editor keeps it on a drag (clamped to
  the element's group, else refused with "distant layers stay below every
  other element"), a paste (on top of the distant layers), merge down and
  split (refused), the Distant switch (to the top of the distant layers,
  or just above them when switched off) and placing an asset (assets
  carry none: Save as asset leaves them out). `olr-cli level element
  raise|lower` refuse to cross the boundary.
- **Scrolling.** A layer's top-left is `at + depth × (camera − level size
  / 2)` on both axes: at the level's centre it lies at `at`, and it moves
  on screen by `(1 − depth)` of the camera's movement. The camera is the
  view's centre in play; in the editor the canvas's centre (the drawn view
  is cut to the device's target there; since issue #66 the level's
  centre, unless View ▾ Preview parallax is on). The offset is rounded to whole
  target texels against the snapped world-target origin: no shimmer.
- **See-through.** In a level with distant layers, back pixels no other
  background covers bake to the back tile's colour at alpha 0
  (`back.png`); a solid block's front stays opaque. The distant layers
  show there, nearest on top, else the back tile; digging reveals them at
  once (carving copies the back), blood and gibs do not stain those
  pixels, the minimap shows the back tile's colour. A level without
  distant layers bakes, loads and draws exactly as before.
- **Light.** A distant texel takes the ambient and `1 − depth` of the
  light beyond it (glow light, explosions, flashes, bloom), read from the
  light grid at its screen point, then the grade. Distant layers never
  glow and never cast light.
- **Decoration only.** No material, collision, simulation or bot reads
  them; they are content (`distant/<i>.png` and `level.ron`'s list, in
  `content_hash`).
- **Not built** (the issue's non-goals): animation, fog, per-layer tints,
  per-axis speeds, a per-player setting, OLX levels or themes with
  distant layers, assets carrying them.
- **Cost.** Without distant layers nothing runs (the backing and the
  layer draws need a see-through terrain; the composite's branch is gated
  by a uniform). With them, one backing draw, one quad per layer and the
  terrain drawn twice. The Windows 1080p measurement waits for the
  maintainer (`docs/deferred-verification.md`). The editor remakes the
  level's distant layers whenever the distant elements change (a stroke on
  a distant layer remakes them per dab).
- **Demo.** `fixtures/look/levels/look-test-distant` (the look test level,
  its base split, two gaps erased, a night sky at depth 90 and a far city
  at 55); goldens `look-distant-a.png`, `look-distant-b.png`,
  `look-distant-full.png`.

## Double-resolution and glowing skins (issue #12)

No OLX counterpart: OLX skins are 672 × 18 sheets of 20 × 18 frames at
art scale 2 (`src/client/CGameSkin.cpp`). Classic skins load, draw and
hash exactly as before. As built:

- **Two layouts.** A skin pack is OLX's layout (art scale 2, 20 × 18
  frames) or double resolution (art scale 4, 40 × 36 frames, a 1344 × 36
  sheet of 64-pixel slots, `FrameLayout::DOUBLE`); both 7 aim × 3 walk
  frames, the worm 10 × 9 world pixels either way. Frames are cut, mirrored
  and placed by the layout (`olr_content::skin_source`, `olr_scene::skin_frame`;
  the body's offset `BODY_OFFSET` is OLX's in world pixels).
- **The world target's scale** is the largest of 2, the terrain's art
  scale and the art scale of any image in the atlas (a double skin's 4,
  capped at `MAX_SPRITE_SCALE`, 8): a double skin draws one target texel
  per art pixel. **Deviation:** with integer nearest the output keeps whole
  multiples of the level's own scale (`dest_rect_in_units`), not of the
  target's, so a level looks as large with a double skin in the match as
  without one (at 1080p, 4 output pixels a world pixel either way); a
  window too small for the target's texels shows it at a fraction of a
  texel a pixel (½ in a 1280 × 720 window).
- **Glowing pixels.** `glow.png` (greyscale, the size of `skin.png`)
  rounds to quarters (`glow_level`: 32 and above glows); each quarter is
  an overlay image drawn over the body with that self-light (25-100 %) and
  `Floor::Worm`, in the player's tint, in a lit match only (an unlit match
  draws as always). Glow on a transparent pixel is dropped.
- **Skin light.** `skin.ron`'s `light: (intensity, reach)`, refused above
  `MAX_SKIN_LIGHT_INTENSITY` (200 %) and `MAX_SKIN_LIGHT_REACH` (48 world
  pixels) and clamped there again at runtime. One light per frame,
  computed when the worm is added: its colour the frame's glowing pixels
  as drawn (tint included, sRGB averaged by glow), its intensity scaled by
  the frame's glow over 3 world pixels of full glow (`FULL_LIGHT_GLOW`).
  Cast as a steady disc at the worm as drawn, after the engine's and the
  mods' lights (so the emitter cap drops skins first), walls stopping it;
  in a level with a look only (a mod's look alone casts none).
  `olr_look::engine::SKIN_LIGHT` (on) is the one switch; no per-player
  setting or lobby option.
- **Not built** (the issue's non-goals): the editor, `olr-cli skin`
  commands, animated glow, a lobby option, `left.png`.
- **Cost.** A double skin's sheet is 4 × the pixels and the world target
  4 × the texels of art scale 2 for the whole level; glow overlays add up
  to four sprites a worm in a lit match; skin lights one disc a worm. The
  renderer keeps the scale of the last atlas uploaded, so the level
  editor's canvas after a Play in a double skin draws at 4 too (correct,
  its view cut by `max_view_for`, only dearer) until the next match. The
  Windows 1080p measurement waits for the maintainer
  (`docs/deferred-verification.md`).
- **Demo.** `fixtures/look/skins/look-test-skin` (OLX's default skin
  doubled, a cyan visor at full glow, the gun's tip at half, light 200 %
  over 40) on `fixtures/look/levels/look-test-skin-level` (a dark room, a
  rock pillar), both built by `fixtures/look/make-look-test-skin.sh`;
  golden `look-skin.png`.

## Skin commands and the Firefly sample (issue #13)

No OLX counterpart (OLX's skins are made in an image editor). As built:

- **Commands.** `olr-cli skin new <out-dir> [--from <skin>] [--double]
  --name <name>`, `skin double <skin> --out <dir>`, `skin check <dir>…`
  and `skin preview <skin> --colour RRGGBB --out f.png`, over
  `olr_edit::skin` (layer 3, no egui), which the skin editor will share.
  A skin is named by directory, or by pack id or OLX name as the
  profiles' picker lists them.
- **Scale2x, the masks following the pixel.** `double` scales every
  frame by Scale2x (a frame's edge its own neighbour, so frames never
  mix). **Deviation from the issue's wording** ("`tint.png` / `glow.png`
  with nearest scaling"): an art pixel takes its tint and glow from the
  source pixel Scale2x copies its colour from, and pixels compare by
  colour, tint and glow together. Where Scale2x keeps the source pixel
  this is nearest scaling. Where Scale2x rounds a corner, a nearest mask
  would leave an untinted body-coloured pixel on a tinted worm's edge (it
  shows in any colour other than the skin's own). Only OLX's layout
  doubles; a double skin is refused.
- **Preview.** 14 × 6 cells of 16 × 14 world pixels, so a classic and a
  double skin preview at the same size (1792 × 672 at `--scale` 8). The
  dark half is a preview of glow and light: the worm at a fifth of its
  colour, glowing pixels at their glow quarter (the game's
  `glow_level`, now `olr_data::skin`'s), the light a disc of its reach
  around the frame's glowing pixels, clipped to the cell. It is not the
  game's renderer; `olr-cli render-match --skin` is.
- **Firefly** (`content/reloaded/olr-skin-firefly`): OLX's default skin
  doubled, then refined by `tools/skins/firefly.py`: shading, body
  rings, eyes, the gun's metal, and a glowing tail lantern with a light
  of 80 % over 24 world pixels. It is in no golden and is not the
  default skin (bots and fresh profiles keep `olx-skin-default`).
- **Not built** (the issue's non-goals): the editor UI (issue #14
  since: "Skin editor (issue #14)"), more sample skins.

## Skin editor (issue #14)

No OLX counterpart (OLX's skins are made in an image editor). As built:

- **A crate of its own.** `olr-skin-editor` (layer 5) follows the level
  editor's pattern (pure `update(action) -> commands`, egui panels found
  by label, the host doing files) instead of sharing `olr-editor`'s
  parts: its history, view and dialogs are of level edits, and a skin
  needs far less (a 7 × 3 grid of 20 × 18 or 40 × 36 frames). The skin
  editor's images are egui textures its panels make (the level editor's
  swatches' way); the host does no GPU work.
- **The way in.** The Editor screen has two tabs, Level and Skin (the
  issue's "tabs on its start screen"). New blank skin is double
  resolution; "Edit a copy" of any installed skin is the "copy of any
  installed skin as the start"; "Edit" only for the data directory's skin
  packs. A new skin and a copy have no pack until the first save, which
  asks the name; the pack id is the name's slug, free among the installed
  packs. A built-in or OLX skin is only ever read.
- **Opening is checked.** A skin pack opens only when `olr_edit::skin::
  check_dir` finds no error and it has OLX's layout or double
  resolution's; anything else is refused with why on the Editor screen. A
  tint other than 0 or 255 (which the runtime refuses anyway) opens as the
  player's colour. `left.png` is kept as it is, not edited (a non-goal).
- **Painting.** Tint is on or off (the runtime's rule); glow is painted
  in quarters (25-100 %), as the game draws it (`glow_level`). A pixel
  made transparent loses its tint and glow (the format drops glow on a
  transparent pixel); tint and glow paint only opaque pixels. A fill takes
  the pixels joined to the one clicked of the same colour, whatever the
  target. Shifting wraps round (nothing is lost). The eyedropper returns
  to the pencil after a pick. Recent colours are the last 12 painted.
- **The onion skin** shows the previous aim (none before the first) or
  the previous walk frame (the walk wraps: walk 1 shows walk 3).
- **The preview** is a CPU approximation, `olr_edit::skin::preview_frame`
  (one cell of `skin preview`'s sheet: light background, and the dark one
  with the glow and a soft disc of the skin light), not the renderer. The
  worm walks at 7.5 frames a second while its aim sweeps 200° a second
  between 90° up and 60° down (OLX's player menu), turning round after
  every sweep; its frame is the game's (`CWorm.cpp:1225-1244`, not the
  menu's formula). Shown at 3 and 7 points a world pixel.
- **Fits every window** (issue #55). The side panels take a share of
  the window's width (tools 20 %, 200-220 points; frames 37 %, from 240
  points); below 900 points one shows at a time (Tools | Frames in the
  session bar). The frames panel's widest is what the frames need at full
  size (about 510 points in the game's fonts), not the issue's 470: within
  470 points the grid's "Walk n" column and spacing leave no room for 60 ×
  54 frames, which the issue also asks for at 1400 × 860. The aim columns
  are headed 1 to 7 (the hint above the grid and on hover); the
  preview's enlarged pair shrinks by whole points a pixel, then wraps under
  the game-size one. In a canvas too narrow for its hint line, how to pan
  and zoom is the zoom label's hover text.
- **No recovery copy.** Unlike the level editor, a crash or a kill loses
  the skin's unsaved changes; closing (the editor's Close or the window's)
  asks first. A skin is small and saved in a moment.
- **Not built** (the issue's non-goals): importing a picture, other
  layouts, `left.png`, animated glow, online sharing, gibs, rope,
  crosshair; nor the e2e autoplay script (deferred:
  `docs/deferred-verification.md`).

## Explosion lights (issue #15)

No OLX counterpart (OLX draws no light). As built:

- **On the projectile's look, not on the effect.** A projectile look's
  `explosion` (`ExplosionLightSource`: colour, intensity, `radius`,
  `reach`, optional `animation` and `phase_ms`) replaces the engine's
  explosion light for every explosion that projectile makes: its effects'
  or timer's `Explode` (`execute` merges a step's into one) and a `Bounce`
  explosion. `mod.ron`'s `look: (explosion: …)` (`ModWideLookSource`) is
  the default of every projectile without its own. Without either, the
  engine's light applies exactly as before (Classic and MW set none: the
  render goldens are unchanged).
- **Engine's light kept for the rest.** A bonus blown up by an explosion
  or a beam, a beam's own sparks and a projectile's explosive trail are
  no projectile's explosion (OLX's `Explosion` for a bonus is its own
  call; a trail is a cosmetic of the flight): they keep the engine's
  light, whatever the mod sets.
- **The link** is cosmetic: `CosmeticKind::Explosion { size, proj }`
  (`proj` the projectile's `SimMod` index, -1 for none; the event keeps
  its size, so `TickEvents` is unchanged) and `olr_fx::Kind::Explosion {
  proj }`. `effects::explosion` takes the projectile only to tag the
  entity: no simulation state, checksum, oracle comparison or network key
  (`olr-present`'s `event_keys` hashes the size alone, as before) reads
  it.
- **Caps** (`olr_look::engine`): `MAX_EXPLOSION_RADIUS` 16 world pixels
  (the renderer's largest disc; the engine's grow 2-12), `MAX_EXPLOSION_REACH`
  4096 (the engine's largest), intensity 0-1000; `Unlimited` reach is
  refused (the maintainer's call), as is a `phase_ms` without an
  `animation`. `olr-format` refuses past them with the field's path;
  `olr-look` clamps again where it reads (`ExplosionLight::radius`,
  `reach`).
- **Clock.** No `animation`: the engine's flash keys (100 → 0 over
  `EXPLOSION_MS`), fitted to the explosion's life (size × 25 ms), as the
  engine's (`engine::explosion_flash`). With one, the clock is the
  explosion's age in ms, and the light still ends with the explosion's
  entity (it is drawn per explosion entity, identity its slot).
- **Disc order.** The engine's explosion lights stay in the engine's
  part (after the muzzle lights); the mods' explosion lights lead the
  mods' part (before the projectile lights and end flashes), then the
  skins' (#12), so the 1024-disc cap keeps its priorities.
- **Golden:** `look-blast.png` is `look-rocket.png`'s match (chaos bots,
  the look test level) with `fixtures/look/mods/look-test-blast-mod`, the
  look test mod's copy whose rocket explodes blue (`#4080ff`, 500 %, a
  disc of 14, reach 160).
- **Not built:** an editor for mod looks (none exists yet; the issue's
  "Size" with reach under More waits for one).

## Mod editing operations (issue #16)

No OLX counterpart (OLX's mods are edited as text). The mod editor's
operations below its UI (`olr_edit::mods`, layer 3) and `olr-cli mod`.
No simulation, Classic, MW, oracle or golden changes. As built:

- **Paths.** `<file>.<field>…`: the file is `mod`, `restrictions`,
  `weapons/<id>` or `projectiles/<id>` (the longest existing id, so ids
  holding dots resolve); fields are `.name`, `[i]` (a list) and
  `["key"]` (a map key not a plain name, such as a preset). A tuple
  variant's field is `0` (`weapons/x.fire.shot.projectile.gfx.0`). The
  navigator goes through structs, enums, `Option` and `Box`; a spawn's
  `projectile` leads into the shared projectile it names, so `get` and
  `set` read and write `projectiles/<id>` there (every user sees the
  change). `mod.weapons` is the id list, read-only (the weapon commands
  change it).
- **Setting a variant** by name makes it with its defaults: the variant's
  serde defaults, or a hand-made start where those are not a valid value
  (`Fire::Projectile` the blank weapon's shot, `Beam` red and 1 wide,
  `Special` Off, `Trail::Projectile` a white pixel every 0.1 s,
  `Effect::Spawn` names `spawn`, `Gfx::Pixels` white; `Gfx::Image` starts
  with an empty file, which is refused as a missing asset until a file
  is set). `Unknown(i32)` variants (imported OLX numbers) are listed,
  read and kept, never offered as a choice. A value is RON of the
  field's type, else a bare string for a string field.
- **What is refused.** An edit is kept only when it adds no error the
  mod did not have (`olr_format::mods::mod_errors` plus missing asset
  files, compared by file, path and message), so a mod with an old
  error stays editable; and only when its canonical text reads back as
  set (a `NaN` is refused). So what LX56 cannot express is refused with
  the compiler's path: per event at most one effect besides a `Spawn`,
  one `Spawn`, one `spawns` entry, every event naming the same spawn; a
  projectile cycle; a duplicate key.
- **`mod_errors`** (`olr-format`): the compiler records every error and
  carries on with placeholders, so `check` lists them all at once, each
  at its field path; `compile_mod` still returns the first. A shipped
  loadout's error path is now `loadouts["<name>"]` (was
  `loadouts.<name>`), so a name with a dot stays one step.
- **Weapons.** The index is the weapon's id in the game (`move`).
  - `add`: blank (the `pack new mod` gun) or a copy of a weapon of this
    or another installed mod, with its shared projectiles and the files
    they and it name. A shared projectile equal to one of the same id
    here (after the files' renames) is reused; another gets a fresh id
    (`<id>-2`…). A file with the same bytes is reused, a missing one
    copied, a different one copied as `<stem>-N.<ext>`. Inline keys that
    clash are given an explicit `id`.
  - A new weapon is in no preset: OLX's `updateList` enables a weapon a
    preset does not name, so it is on in every preset and loadout.
  - `remove` refuses the last weapon and drops its preset entries (unless
    another weapon has the name); shared projectiles only it used stay,
    with `check`'s "projectile is never used" warning.
  - `copy` inserts after the original ("Bazooka 2").
  - `rename` (and `set weapons/<id>.name`, or a whole weapon set): the
    presets' entries for the old name (OLX's match: trimmed, ignoring
    ASCII case) become one for the new name with the first's state; the
    shipped loadouts take their weapons from the presets, so they follow.
    A name must be non-empty, without surrounding spaces or control
    characters, and unique.
  - `rename_mod` (`set mod.name`) sets `mod.ron`'s name and the
    manifest's; nothing in the presets or loadouts names the mod.
- **Shared projectiles.** `used_by` lists each with the spawns naming it.
  `inline` copies a spawn's shared projectile into it (the shared file is
  removed when no other spawn uses it); `share` makes an inline one
  `projectiles/<id>` (default id its key). Inline after share, and share
  after inline, give the files back.
- **`check`**: `olr_pack::validate`'s errors and warnings (without the
  ones repeating ours), every compile error at its path, missing assets
  at the field naming them, files that do not parse with line and
  column, and unused `gfx/` and `sfx/` files and shared projectiles (also
  when the mod does not compile).
- **Not built** (the issue's non-goals): the editor UI, Play, art and
  sounds, practice.

## Mod editor (issue #17)

No OLX counterpart (OLX's mods are edited as text). The editor over #16's
operations; no simulation, Classic, MW, oracle or golden changes. As
built:

- **A crate of its own.** `olr-mod-editor` (layer 5) follows the skin
  editor's pattern: pure `update(action) -> commands`, egui panels found
  by label, the host (`olr-game`'s `editor::mods`) doing files. Its
  document is the `olr_edit::pack::Pack` itself; every change goes
  through `olr_edit::mods`, so the editor and `olr-cli mod` change mods
  the same way.
- **The way in.** The Editor screen's third tab, Mod: the mod recovery
  copies first, then every installed mod, the user's first, with Edit
  for the data directory's mod packs and Edit a copy for any (a form
  names the new mod). A copy is made in `packs/` at once
  (`new_pack`, `Template::From`; its id the name's slug free among the
  installed packs), not at the first save as a skin is: the mod's
  shared projectiles and files come with it, and the recovery copy and
  Save then always have a folder. A built-in or OLX mod is only read.
- **The base mod.** A copy's `editor/mod.ron` (`ModEditorFile { base }`,
  an authoring file `pack check` reads and the game ignores) names the mod
  it was copied from; the editor loads that mod (when installed) and
  shows its value beside each field that differs, with Reset. A mod
  whose base is gone shows no originals.
- **Errors are kept, not refused.** #16's operations refuse an edit that
  adds an error; the editor sets fields with `set_lenient`, which keeps
  the edit and returns the new errors, so a mod can pass through an
  invalid state (a cycle made and then broken elsewhere). The check
  (`mods::check`) runs after every change, undo and redo; errors and
  warnings are listed with their path, each a link to its item, node and
  field (marked and scrolled to). Save refuses a mod with errors (the
  editor first, `pack::save` again); structural operations (add, move,
  share, inline) keep #16's refusal and say why in the status.
- **One step per change.** The history holds snapshots of the mod's
  source and name (and its files, when a weapon copied from another mod
  brought some), at most 200; the same value set again is no step.
- **Weapons.** Added at the end (blank, or a copy of a weapon of any
  installed mod or this one; the copy's id is its name's slug made
  unique), removed, moved one place at a time, duplicated (after the
  original) and renamed through their Name field (the presets follow,
  #16). The first move says once that a weapon's place is its id in the
  game. Changing a fire's kind asks first (its settings start again from
  the new kind's start values; undo brings them back).
- **Projectiles as a tree.** The weapon's shot, its projectile, and under
  each projectile its event spawns, its trail's spawn and its timer, each
  a node by its field path; a spawn naming a shared projectile leads into
  it (a cycle shown once, not followed). A shared projectile shows its
  users (links); a spawn's projectile is chosen among its own and the
  shared ones; Make a copy for this weapon is `mods::inline`, Make it
  shared `mods::share`.
- **Events** are shown as LX56 runs them: one effect (a dropdown of the
  effects, `Spawn` and `Unknown` left out; its fields below) and whether
  it spawns (the projectile's spawn, made with one white pixel when it has
  none; turning the last user off removes the spawn). An imported event
  with more effects says how many more (the check's error).
- **Fields** come from `crates/olr-mod-editor/fields.ron`, which `cargo
  xtask format-docs` writes from the format's types and doc comments
  (the hover help) and whose unit, soft range, place and asset folder are
  edited by hand and kept; `check` fails when it drifts. A number is typed
  (any value: the format validates) and has a slider over the soft range;
  common fields come first and the rest under More ▸; image and sound
  fields are dropdowns of the mod's `gfx/` (with thumbnails) and `sfx/`
  files. The Look fields are left to the art issue.
- **Recovery copies.** As the level editor's (`recovery.rs`), in
  `<data>/mod-recovery/<pack id>/`, written every 120 s while there are
  unsaved changes, at a quit that does not ask, and removed on Save and
  Close. A recovered mod opens unsaved in its own folder (one of the
  user's packs only).
- **Not built** (the issue's non-goals): play from the editor (issue
  #18, below), art, sounds, the look fields, practice, a raw RON editor.

## Mod editor Play (issue #18)

No OLX counterpart. The mod editor's Play, as the level editor's (plan
14c R3, R4); no simulation, Classic, MW, oracle or golden changes. As
built:

- **The mod as it is.** Play takes the editor's `Pack`, unsaved changes
  included: `olr_edit::pack::mod_pack` writes its files in memory, checks
  them as Save does (`pack::check`) and reads them with
  `olr_pack::read_mod`; `Content::load_mod_pack` makes the `LoadedMod`
  as `load_mod` makes an installed one (placeholders, the core pack's
  game sounds). Nothing is written: the saved pack stays as it was. The
  session plays only `Assets::m`, so nothing else changed.
- **Refused with errors.** The editor checks first: a mod with errors
  does not start, its problems list opens and the status says "Not
  played"; the host's check again turns any remaining refusal into the
  editor's error dialog ("The mod could not be played").
- **One edited Play.** `setup::EditedPlay` now holds a `PlayLevel`
  (`Edited`: the level editor's source; `Installed`: a pack id) and a
  `PlayMod` (`Installed`: a pack id; `Edited`: the mod editor's pack).
  The level editor's Play is `Edited` level and `Installed` mod, built
  as before. The mod editor's is an `Installed` level and the `Edited`
  mod: 0 bots the sandbox, else a Deathmatch with Local Play's
  personality, rules and the `Default` loadout, as the level editor's;
  on an installed level the human spawns at a free spot, as in any match
  (the level's first spawn is the level editor's Play's alone). No replay
  is saved.
- **Play ▾** lists every installed level by name and 0 to 8 bots; the
  choice is remembered in `<data>/mod-editor.ron` (`PlaySettings`),
  first Local Play's level and one bot.
- **Back where it was.** During the match the editor is not drawn but
  kept (item, node, undo history, unsaved mark; egui keeps its scroll);
  Escape or Stop ends it (`Shell::stop_edited`), F5 restarts it, a match
  over ends it when its results would show, and closing the window ends
  it before asking about unsaved changes. Recovery copies keep being
  written meanwhile.
- **Not built** (the issue's non-goals): practice setup (choosing the
  weapons; the sandbox has the mod's first five, a Deathmatch the
  random ones of any edited Play; issue #20 added a practice beside
  Play), online play of an unsaved mod.

## Mod editor art and sounds (issue #19)

No OLX counterpart: OLX's mods have no glow, and its sprites are edited
outside the game. The mod editor's Art, Sounds and Look, the glow masks
they paint, and `olr-cli mod gfx|sfx`. No simulation, oracle, Classic or
MW change: only the frame count of an image reaches the simulation, and
nothing here changes one unasked. As built:

- **Glow masks** (`docs/formats/mod.md`): `gfx/<name>.glow.png` beside
  `gfx/<name>.png`, an 8-bit greyscale PNG of the image's size, 0 none
  to 255 full, drawn in quarters (`olr_data::skin::glow_level`, the
  skins' `GLOW_LEVELS`, issue #12). A mask without its image, of another
  size or that is no greyscale PNG is a pack error on the mask
  (`olr_pack::validate`); a mask of a used image is not "never used".
  The game loads a mask that is wrong with a warning and draws its image
  without glow: a bad mask never fails a match.
- **Drawn as a skin's glow** (#12's method extended to projectiles): an
  image with a mask gets one overlay per glow quarter in the atlas,
  drawn over the sprite at the same frame, in a lit match only, at
  `max(quarter × 25, the sprite's self-light)`. An unlit match (Classic
  and MW on OLX levels) draws the sprite as before, so their look is
  unchanged. The mask lights nothing by itself: a light is the Look
  group's.
- **Liero v1.0 Reloaded**: masks on the flame, napalm, blaster, mini nuke
  and nuke (`content/reloaded/README.md`); its images stay Classic's
  (a test), so its frame counts and play are unchanged.
- **The Art section** paints with the skin editor's tools (its `paint`
  module and `Frame`; a same-layer dependency) on one square frame at a
  time: frames are the strip's height wide, as the simulation counts
  them. A stroke is one undo step holding only the files it changed.
  Adding or removing a frame, or a replacement PNG of another frame
  count, asks first and says the weapon may then behave differently:
  Play is the way to check (the issue's ruling: no frame-count
  analysis).
- **The preview** approximates play: the first user's animation (its
  frame rate, as `proj_sim` steps it), or for heading frames a turn
  through all of them in `PREVIEW_TURN_S` (3 s), else every frame at
  `PREVIEW_FPS` (10); over a night background with the glow bright
  (`art::dark`, the image at `PREVIEW_DARK` 25 %, the glow at full).
- **Assets** (`olr_edit::mods::art`): new images (blank, or a copy),
  PNG and sound (WAV or Ogg, checked as `olr-content` loads them; the
  check moved to `olr-pack`) imports, rename (every field naming it
  follows, the mask too) and remove, refused while a field uses it, the
  users listed. A name taken gets a number.
- **Sounds** play from the Sounds section through `Audio::preview`: one
  file at the listener at the master volume, apart from the game's
  voices (a new preview stops the last). OLX has no such preview.
- **The import browser** is the mod editor's own (folders and the files
  an import takes, hidden entries left out), following the level
  editor's rather than sharing it (its purposes are a level's); the
  host reads a regular file of at most a mod file's size.
- **Look group**: a projectile's `glow`, `light`, `end_flash` and
  `explosion`, a weapon's `muzzle_light`, each switched on with a
  default and edited in place; a look all default is removed.
- **Not built**: drawing a mask from the sprite's colours in the editor
  (`olr-cli mod gfx glow --colour` does it), and frame-count checks
  beyond the warning (as the issue rules).

## Mod editor practice (issue #20)

No OLX counterpart. Play ▾'s **Practice with this weapon**, beside the
mod editor's Play (issue #18), which stays as it was. No simulation
rule, oracle, golden, bot or normal-match change: a practice is the
sandbox with existing settings. As built:

- **The weapon shown.** Enabled while the list shows a weapon (any node
  of its tree); it takes the mod as it is, unsaved changes included, and
  is refused with errors as Play is. The player holds the weapon in its
  first slot and the mod's first other weapons in the other four
  (`setup::practice_setup`; the sandbox's own loadout is the mod's
  first five): the rest of the loadout is the builder's call.
- **The sandbox, not a Deathmatch.** One to four targets (Play ▾'s
  Targets, 1 by default) are the sandbox's idle targets
  (`SandboxOptions::targets`): they press nothing, as
  `olr_bots::Personality::Idle`, and the sandbox's LX56 settings
  (`MaxRespawnTime` 0) respawn them after `RespawnTime`. Worms spawn at
  `FindSpot` positions; the match never ends (no lives, kill or time
  limit), Escape or Stop returns to the editor where it was, F5 restarts.
  Running Idle bots through a Deathmatch would add its end, its weapon
  selection and the bots' loadouts for the same idle worms.
- **Reloading.** `LoadingTime` 100 % (LX56's), so the weapon's own
  `recharge` shows; Play ▾'s **Fast reload** (off by default) sets
  `LoadingTime` 0, which fills the charge at once (`olr_sim`'s
  `simulate_worm_weapon`), leaving only the weapon's rate of fire.
- **What shows.** The targets' health bars (`SceneOptions::
  show_health_bars`, OLX's `ShowWormHealth`; off in every other game, so
  the issue's "already show" holds only from here) and the last hit at
  the picture's top centre: `Last hit: <amount> on <name>`, and `(took
  <dealt>)` when the worm had less health, from the last
  `EventKind::Damage` (`Session::last_hit`, kept only in a practice; a
  restart forgets it). The summed damage popups stay as in any game.
- **The level.** Play ▾'s Practice level, its own list (remembered with
  the targets and Fast reload in `<data>/mod-editor.ron`); until one is
  chosen Castle Strike (`olx-level-castlestrike`, OLX's most open: a
  wide sky), else Play's level.

## Split screen (issues #26 to #32)

Local split screen for 2 to 4 players (OLX's fork: up to four,
`docs/porting/olx-notes/presentation.md` §1.1; `CClient.cpp:1527-1551`,
`1653-1744`; `CClient_Draw.cpp:561-609`). Issue #26 is its first part:
the renderer draws several views in one frame; issue #27 the second: a
camera per view and the nearest listener; issue #32 the last: joining
in the menus. Done: 1 to 4 local players join local play from their own
seats and play it split. As built:

- **The fair view** (the maintainer's ruling, a deliberate deviation):
  every local player sees the same area of the level as a solo player,
  `olr_scene::Viewport::GAME`'s 427 × 192 world pixels, drawn smaller to
  share the screen. OLX's split gives each player less of the level
  instead (two: 159 × 240 world pixels each at 640 × 480; three or four:
  159 × 119). Each view keeps its own world target, light grid, bloom
  and composite at the solo size; only the upscale into its part of the
  screen is smaller (`Renderer::render_views`).
- **One glow light a frame.** The glow light (plan 15c) is world-fixed
  and runs once a frame for every view, one history step a frame,
  whatever the number of views. Since issue #53 worms take no part in it
  (the level-wide occluder copy each view's map went into is gone), so
  no view's worms change another's light. One view draws exactly as
  before.
- **A camera per view** (issue #27): one `olr_scene::Scene` holds a
  camera per view (`Scene::add_view`, `olr_present::Presentation::
  set_views`, at most four), each following its own worm with the solo
  rules (hard lock, clamp, shake for an explosion in its own view, a dead
  worm's view staying, the jump at its respawn), as OLX's viewport per
  local player (`CClient.cpp:1527-1551`). The rest is one: the snapshots
  and marks, the entities (`Fx`), the colour map, the look frame and its
  records (shots, ended projectiles, drawn projectiles and worms, taken
  from the first view's pass). `build_views` draws a list per view; view
  `v`'s is what one view with `v`'s camera draws, laser-sight dots and
  all (their generator moves on once a frame, every view drawing the
  same dots). One view draws exactly as before (scene snapshots, match
  and render goldens unchanged).
- **The nearest listener** (the maintainer's call in issue #27, a
  deliberate deviation): OLX's OpenAL hears viewport 0's listener only
  (`src/sound/sfxdriver_openal.cpp:55-65`, presentation note §1.6), so a
  second player would hear their own fight from the first's view. OLR
  hears each positional sound once, from the local view whose centre is
  nearest to it (`olr_audio::mix::nearest`, the first of equally near
  ones), and re-mixes each playing voice from the view now nearest
  (`Audio::play_heard`, `Voices::follow_nearest`); one mix, nothing
  doubled. THROW and PICKUP still play at the listener, the first view's.
  With one view it is OLX's mix. If wrong: hear viewport 0 only by
  passing the first listener alone.
- **Input seats** (issue #28, `olr_input::Seat`, `Seats`): keyboard 1
  (the keys as before, and the mouse: only it has mouse controls),
  keyboard 2 on the same keyboard and one seat per gamepad, events routed
  by gilrs pad id; each seat has its own state, latch and Dig, as OLX's
  controls per local player (presentation note §10.1). Keyboard 2's defaults are OLX's player 2 (`Options.cpp`
  `ply_def2`, presentation note §10.1: the keypad, Strafe kp ., Previous/
  Next kp 7/9); Dig, which OLX lacks, is kp 3. The bindings belong to the
  seat (the maintainer's call): Options → Keys chooses Keyboard 1,
  Keyboard 2 or Gamepads (every pad shares them), each under its own rule
  (every action reachable by that seat's device alone: a keyboard-only
  player 2 needs no pad input). A key both keyboards bind is allowed and
  shown (one press is both players'). A profile's own bindings stay
  keyboard 1's and the pads' (as before seats); keyboard 2's are always
  the options'. A pad's Start pauses and resumes in game as Esc does
  (OLX has no pad pause), never bound to an action (a file binding it
  is repaired). An options file from before seats keeps its lists and
  gets OLX's player 2. A player alone plays from keyboard 1 and every
  pad as before (keyboard 2 heard by nobody).
- **Several local players** (issue #29, headless first): `olr-game`
  holds a list of local players (`Assets::locals`, `Session::locals`),
  each with its own slot, look, skin, keys (`Session::tick_locals`), view
  (the fair view above), tally (kills, deaths, spawns), weapon name,
  reload bar and rope throws heard, as OLX's local worms each with its
  own viewport and counters (`CClient.cpp:1527-1551`, `1653-1744`;
  `CClient_Draw.cpp:561-609`). Among several, player `k` sits on
  keyboard 1 (with the mouse), keyboard 2, pad 0 or pad 1 (`players::SEATS`;
  from the menus, the join step's seats: #32 below); each seat hears only its own
  devices, so one player's keys never reach another's worm. `olr --mode
  deathmatch --locals N` plays one (the humans worms 0 to N-1), and
  `olr-cli match --locals N` one with scripted keys for tests. Replays
  record every local slot (`ReplaySetup::viewers`, format 6), so a
  split-screen replay plays back with a view each. Without a profile
  each is "Player N" in OLX's default worm colour `N-1` (one alone stays
  "You"); bots keep their colours away from every player's. The
  simulation, the oracle and the match goldens are unchanged.
- **The layout on screen** (issue #30, `olr_ui::hud::SplitLayout`): 2,
  3 and 4 equal views, each the solo playfield's shape (the fair view
  above: the same 427 × 192 world pixels, smaller), at the largest size
  the window allows, with a thin black seam between them (OLX: 4 screen
  pixels of 480, filled with `clViewportSplit`, black;
  `CClient.cpp:1653-1677`, `CClient_Draw.cpp:561-609`; OLR's seam is
  the same share of the frame's height, 6 of 720). *Arrangement* (a
  deviation, from the fair view): OLX puts two players side by side and
  three or four in quarters; OLR's views are wide, so two are stacked
  top and bottom (the maintainer's call), three and four in a 2 × 2
  grid, and side by side, or in a row, where a window is wide enough
  for that to give larger views (`split::candidates`, the largest
  wins). With three in the grid the fourth quarter holds the shared
  minimap, as OLX's radar; with two and four the minimap, the clock and
  the event log go in a strip under the views or the columns beside
  them, whichever leaves larger views. One minimap and one log for all
  players, every local player's dot big. *A compact HUD in each view*
  at its bottom left, where OLX's fork drew each viewport's bars
  (`CClient_Draw.cpp:1192-1257`): the player's colour and name, lives
  and kills, the health bar, the weapon in hand and its charge in OLX's
  bar colours; the solo bar gives way to it, and the keys are not shown.
  Worm names, weapon names, reload bars and damage popups are drawn in
  every view. One player keeps the solo layout exactly (the HUD
  snapshots unchanged). `olr-cli render-match --locals N` draws every
  view (two stacked, three or four in a 2 × 2 grid).
- **Menus by pad** (issue #31, `olr_ui::pad`; not OLX's: its menus take
  no pad): any pad's d-pad and left stick move egui's keyboard focus
  (OLR's own wish so a player on the couch needs no keyboard), A clicks
  the focused widget, B is Esc (Back). It goes through egui's own focus
  (arrow keys, Tab, Enter, Esc), so the keyboard and the mouse work as
  before. A slider moves with left and right; a drop-down steps through
  its choices with them, or A opens its list and A picks an entry; a
  text field is skipped (there is no on-screen keyboard); up or down
  finding nothing within egui's 45° goes on in the widgets' order (round
  at either end), so every widget is reachable; the focused widget has a
  ring in the accent colour while the pad leads. Nobody's pad drives the
  menus while a match runs or on the weapon selection (the game's keys
  drive it, a pad's too); the pause menu and its options hear only the
  pad whose Start paused (any pad after Esc). The editors take no pad.
- **Joining** (issue #32, `olr_ui::join`, a deviation: OLX sets its
  local players up in its player menu before a game, presentation note
  §1.1; the routing is still OLX's, a worm per seat,
  `CClient.cpp:1527-1551`): local play's Start opens a join step
  ("Players") only once two or more seats were used in this run (a
  bound key of each keyboard or a pad's button pressed); otherwise it
  starts at once, so a solo player sees nothing new. The seat that
  pressed Start is in; any other joins by pressing a button of its own,
  steps through the profiles no other player has (or "Player N") with
  left and right, readies with Fire, Jump or Start and leaves with Rope
  or Dig; the match starts when every player is ready (or by the mouse's
  Start), bots filling the rest. Each seat's own keys drive only its own
  card, and the mouse only keyboard 1's. Keyboard 1 is always the first
  local player (its view the first), so mouse controls and the pointer
  kept in that view need nothing new; a lineup without keyboard 1 has
  no mouse controls. Every player chooses its weapons at once, each in a
  panel over its own view with its own keys (the mouse in keyboard 1's);
  the last Done arms them all. The pause menu hears the joined pad whose
  Start paused it (an unjoined pad's Start does not pause). *Unplugged*:
  a joined pad's worm stands still (its keys released); the same pad
  back, or another with its gilrs UUID, takes the seat back, else the
  first pad no player has that presses a button (that press does
  nothing else). F5's rematch and the results' keep the lineup; leaving
  the match clears it.
- The pause menu and the results (the overlay's too) are everyone's:
  with two to four views they centre on the whole window, solo over the
  playfield as before (issue #49; `MenuData::views`, from
  `present.views()`). `olr-cli gpu-bench --views N` lays its views out in
  halves (top and bottom) or quarters for the measurement only.

## Surface materials (issue #37)

No OLX counterpart (OLX's pixels are only rock, dirt or background). The
first of ten (spec `docs/superpowers/specs/2026-10-22-materials-and-heat-design.md`):
the format, the built-in set and the per-pixel maps; nothing reads them
in a match yet (heat comes later). As built:

- **The material.** A file `<id>.ron` (`docs/formats/material.md`): id,
  name, notes and an optional `heat` (sensitivity percent 0-100, default
  100; capacity 10-1000, conductivity 0-1000 and cooling 1-1000, each
  percent of stone's 100; a `look` of a colour ramp, temperature 1-1000
  to colour and glow percent, empty for the engine's lava ramp, and
  `light`, percent of the glow cast). The spec nests `look` in `heat`:
  without heat a material has no look of its own yet. Temperatures run
  0-1000 (a fresh crater's rim at 1000). Integers only; `olr-format`
  refuses out of range, `olr-look` clamps again where read. Room for later
  effects: each is a field beside `heat`.
- **The built-in set** (`content/materials/`, read-only, shipped beside
  the content by `cargo xtask dist`): stone (the reference: 100, 100,
  100), dirt, sand, brick, concrete, wood (its own ember ramp), metal
  (conducts six times stone) and glass (an amber ramp, half its glow as
  light). The values are a first guess, to tune in the heat issue's
  playtest. The user's materials are `materials/` of the data store;
  the library lists built-in first, the user's at most 256, an
  unreadable file a warning.
- **In a level.** `look.materials` holds copies (at most 255, ids
  unique), so a level plays anywhere; `look/surface.png` (front) and
  `look/surface-back.png` (back) at world resolution, `v` the look's
  `materials[v - 1]`, 0 none, both or neither. A level without materials
  writes neither (no byte changes: render and match goldens unchanged).
  The lenient reader plays bad maps without them, with a warning;
  `pack check` reports them. Distant layers take one `surface` each
  (`level.ron`'s `distant[i].surface`); "no material" in their docs reads
  "no collision material" now.
- **The bake.** An element's `surface` lands where its pixels land: a
  solid world pixel takes the surface of the element giving its collision
  material (Rock and Dirt in the front map; a mixed layer or a raster per
  pixel); the front map is 0 over background. The back map follows the
  backdrop as the glow groups do (the element at the centre art pixel of
  the block, else the first): a Background element's own art gives its
  `surface`, back art (a drawn layer with `back`, or a raster) its
  `back_surface`. The issue lists the raster among back-art elements, so
  a raster takes both. Painted maps (`editor/surface/<id>.png` and
  `<id>.back.png`, the element's box, a raster's the level, in world
  pixels) override per pixel; 0, or a value past the look's materials,
  keeps the default. A plain level's maps survive being opened in the
  editor (the raster's or the three layers' painted maps).
- **Not yet built** (later issues): heat and its look (3/10, issue #39
  below: `LevelPack` and the match read the maps there), mods, the
  editors' material tools (7/10). The level editor keeps painted maps as opened, re-lays
  them on a level resize, resamples an image's on a new box size, drops
  them on split and merge down (as glow) and drops ones that no longer
  fit on saving.

## Surface materials in assets and olr-cli (issue #38)

No OLX counterpart. The second of ten (the same spec): assets carry
materials, and `olr-cli` gives a level materials without the editor. As
built:

- **Assets.** `asset.ron`'s `materials` holds the materials the elements
  name (a `surface`, a `back_surface` or a painted map's value), each
  used, at most 255; the painted maps are `surface/<id>.png` and
  `.back.png` in the folder, their values indexing the asset's own list.
  Without materials an asset writes no `materials`, no surfaces and no
  maps: every built-in asset is byte for byte what its command made
  before. An asset still holds no distant layer.
- **Saving.** A cut (elements or a rectangle) keeps each element's
  surfaces, re-lays its maps with the crop and carries the look's
  materials it names, in the look's order, the maps re-numbered to them;
  a surface naming none of the look's materials goes (it named nothing).
  A raster cut as a drawn layer keeps its `surface`, `back_surface` and
  maps: where its background shows its own art in the drawn layer, the
  back map still applies there (an edge case, a raster is rarely cut).
- **Placing** joins a material the level has by id, the level's settings
  kept (never replaced), and adds the others after the level's, exactly
  as glow groups; a level without a look gets the default one (it plays
  lit). The landed elements keep their surfaces, their maps re-numbered
  to the level's materials. The editor's placing does the same in one
  step; its paste still lands no surfaces (the clipboard holds none; the
  editor's material tools are 7/10).
- **The colour rule** (`olr-cli level element surface --rule`), the
  builder's call: `[rock:|dirt:|background:]COLOUR[/TOLERANCE]=MATERIAL`,
  first match wins, unmatched pixels keep their value, so a script's
  rules re-run to the same files. A world pixel's colour is its block's
  centre art pixel (as the bake picks a pixel's group), compared by
  `level::similar` (the editor's percent tolerance, alpha included); a
  transparent one (not covered) is never painted. `--map` takes a
  greyscale PNG of values instead, and `level material list` prints the
  values.
- **The library on the command line**: the user's library is the game's
  data directory's `materials/` (`directories`, as `olr-ui`'s store) or
  `--user`; `level material add` takes the built-in copy when both
  libraries hold an id, and keeps the level's copy of one it has.

## Heat in the renderer (issue #39)

No OLX counterpart. The third of ten (the same spec): the renderer's heat
behind a direct deposit API (`olr_render::Renderer::deposit_heat`,
`advance_heat`, `reset_heat`); nothing deposits yet (explosions are
4/10), so no level, golden or match changes. `LevelPack::surface` and
`LoadedLevel::surface` carry the maps; `olr-present` hands them with the
materials to `set_surface` on every whole terrain upload. As built
(`crates/olr-render/CLAUDE.md`, Heat), the builder's calls:

- **The texture** is `R16Uint`, degrees × 64 (GL renders into no
  `R32Float`): 1/64 of a degree, so a slow cooling still moves it. The
  step runs every 20 ms of match time (`HEAT_STEP_MS`), at most 250 a
  frame (5 s; the rest wait for the next frames); a deposit lands before
  the step it was made in, so the split of match time into frames never
  changes the bytes.
- **Stability by a clamped coefficient**, no substeps: each neighbour
  gives at most 0.24 of the difference, so a step is a convex
  combination (never above the hottest neighbour, never oscillating) for
  every conductivity and capacity. Stone (100 everywhere) takes 0.05 a
  neighbour a step; a pixel's own capacity scales what it takes, the
  smaller conductivity of the pair what flows. Outside the level conducts
  nothing (as rock with no material).
- **Cooling** is `cooling% × (0.6 % of T + 0.05°)` a step beside air, so
  every pixel reaches 0 (the linear part alone never would). A pixel with
  no open 4-neighbour loses a quarter of that (the spec says only pixels
  beside air cool; deviation: otherwise heat sealed in an insulator would
  keep the pass running for ever). A back-wall pixel (open in the solid
  map) counts as beside air.
- **The active region** is at most 8 boxes on the CPU, grown a pixel a
  step to 48 px past the deposit; a box is zeroed (a pass) and dropped
  once an upper bound of its temperature, cooled at the slowest bulk rate,
  is under the level's visible threshold (the lowest first ramp key) and
  2 s passed since its last deposit (faint hits below the threshold may
  pile up), or after 5 min of match time whatever its bound (a material
  of cooling 0 lets go: the heat vanishes then, a deliberate cap). Heat
  below the visible threshold is dropped with its box; it showed nothing.
- **The look**: the ramp colour is mixed over the art by its glow and the
  glow is the pixel's self-light, so it shows in the dark and in full
  bright. A hot pixel emits linear colour × glow × `light`% (a glow group
  at 100 % casts 1) with a reach of 200 px; the back wall's hot pixels
  emit too (the emission reads the heat wherever the material rules).
  Constants await a playtest (`docs/deferred-verification.md`).

## Explosions heat the level (issue #40)

No OLX counterpart. The fourth of ten (the same spec): explosions deposit
the engine's heat into the renderer's (issue #39). The simulation, the
oracle and every match golden are unchanged; a level without heating
materials draws the same bytes (every render golden kept). As built, the
builder's calls:

- **What heats:** each `EventKind::Explosion` (`effects::explosion`: a
  projectile's explosion and a bonus blown up), recorded per tick by
  `olr_scene::Scene::explosions` beside the ended projectiles and shots.
  An explosive trail or a beam only spawns its cosmetic explosion, never
  that event, so it deposits nothing. Each is keyed by its tick's time and
  its place among the tick's explosions through `olr-present`'s `Once`
  memory: a re-simulated tick deposits nothing twice (a correction moving
  it leaves the heat where it first landed).
- **The engine's curve** (`olr_look::engine::explosion_heat`, by OLX's
  `Explode` damage, which also sizes the hole): `1.2 × damage⁴` degrees
  at the centre (at most 1000), over twice the hole's radius (OLX's dirt
  theme holes, 2.5-7.5 px) plus half the damage (at most 48 px). A minigun
  hit (3: 97° over 12.5 px) stays under the lava ramp's first key (100; on
  rock: diggable terrain takes 2.5× since issue #65);
  a Bazooka (4: 307° over 17 px) leaves its rim glowing a few pixels deep;
  a Big Nuke's fragments (15: 1000° over 22.5 px) pile up white-hot. A
  theme with other holes keeps the curve.
- **The back takes a fifth** (`olr_render::BACK_HEAT`, 0.2): a back pixel
  (no rock or dirt) lies behind the explosion, so an explosion in the
  open only warms the wall behind it, while a crater's rim takes the
  whole deposit. Taken whole, mid-air rockets left glowing discs on the
  back wall (the spec says only that the exposed back takes heat).
  Issue #65 replaced it with the ball's slice 14 px back.
- **Frame-rate independence.** `Presentation::upload` hands the renderer,
  after the terrain, each deposit after the match time up to its tick,
  then the time up to the frame (ticks never drawn included), and runs
  the renderer's heat between (`Renderer::run_heat`) whenever 64
  deposits wait or more than 250 steps are due, so nothing is dropped or
  delayed. The deposit pass adds each deposit in whole texel units, so
  deposits landing in one pass or several give the same bytes. The heat
  still reads the terrain as last uploaded: a frame drawn once at the end
  of a match sees only the final terrain, so where a pixel was dug or
  filled after heat reached it, the heat differs from a frame drawn every
  tick (in the test match, 13 % of hot texels, all within 24 px of such a
  pixel; `olr-cli`'s `heat_is_the_same_drawn_every_tick_or_once`). In
  play a frame draws one or two ticks.
- **Resets:** a restart (a fresh heat texture with the terrain), a loaded
  state (online repair or a late spectator) and
  `Presentation::reset_heat` (for a replay seek: replays have none yet,
  they only play forward or restart).
- **Fixtures:** `fixtures/look/levels/look-test-heat` (a dark cave of
  stone, dirt, wood, metal and brick, made by
  `fixtures/look/make-look-test-heat.sh`) and
  `fixtures/look/mods/look-test-heat-mod` (Classic's Bazooka and Minigun
  and a smaller Big Nuke, pixel art, a 12.5 s reload, so one volley heats
  and then cools). Goldens: `heat-fresh.png` (tick 380) and
  `heat-cooled.png` (tick 900) of two chaos bots, seed 1.

## Distant layers heat as slices of the ball (issue #41)

No OLX counterpart (distant layers are OLR's, issue #11). The fifth of
ten (the same spec, "Distant layers: the ball"). A level without
distant materials draws the same bytes (every render golden kept); the
simulation and the match goldens never see it. As built, the builder's
calls:

- **`K`** (`olr_look::engine::HEAT_DEPTH_PX`) is 1.1 world pixels a
  percent of depth: a Big Nuke's fragments (heat radius 22.5) reach a
  layer of depth 20, a Bazooka's (17) depth 15. A layer gets a heat
  texture only when its material heats and the renderer's largest
  radius (64) reaches it: depth 58 at most. Issue #65 moved the layers
  behind the back wall: `z = 14 + d × K`, depth 45 at most.
- **The back's share holds:** a layer's pixels count as back (as a wall
  behind the explosion): `BACK_HEAT` (0.2) of the slice, every pixel
  facing the air as it cools, so a layer never glows more than the back
  wall would at its place. Within a layer heat spreads by its material
  over the pixels its image covers; none to the level or another layer.
- **Placement:** the circle's centre is the layer pixel drawn behind the
  explosion (the renderer's rounded texel placement, `distant_origin`)
  with the camera of the frame that first shows the explosion, the view
  nearest it (by its rectangle; the first of those as near). Cameras move
  per drawn frame, so unlike the plane's heat the spot depends on which
  frame first shows the explosion (`render-match`'s one frame at the end
  places it by the last camera); the amount does not.
- **Look:** the lit distant pass mixes the layer's ramp colour over its
  art and takes the glow as self-light; it casts no light (the glow light
  reads only the level's heat), as the spec rules.
- **Fixture:** `fixtures/look/levels/look-test-heat-distant`
  (`fixtures/look/make-look-test-heat-distant.sh`: the heat cave of
  stone open onto a wooden palisade at depth 8 before a stone sky at
  depth 80, beyond reach). Golden: `heat-distant.png` (the heat match at
  tick 380 there).

## Mods' heat (issue #42)

No OLX counterpart. The sixth of ten (the same spec, "In a mod"): a mod
tunes its explosions' heat and lets projectiles heat what they pass.
Presentation only: the simulation's mod, its checksums, the oracle and
every match golden are unchanged (the explosion event only gained a
presentation tag, below); a mod without these fields heats as before
(every render golden kept). As built, the builder's calls:

- **Fields** (`docs/formats/mod.md`): a projectile's
  `look.explosion_heat` (`amount` percent of the engine's heat for the
  explosion, 0-1000, default 100; `radius` world pixels 1-48, default the
  engine's for the explosion) and the mod-wide `look.explosion_heat` for
  projectiles without their own, falling back as `look.explosion` does
  for light (a bonus blown up, which is no projectile's, keeps the
  engine's); a projectile's `look.heat` (`amount` 1-100 degrees of stone a
  tick, `radius` 1-16, both required). The radius cap is the engine's own
  widest (48); a deposit stays at most 1000 whatever the percent.
- **Which explosion is whose:** `EventKind::Explosion` gained `proj` (the
  exploded projectile's `SimMod` index, -1 for none), the same tag as the
  explosion entity's (issue #15), in no checksum; `Scene::explosions`
  carries it.
- **`amount × ticks elapsed at its drawn position`, a tick at a time:**
  the spec's per-tick heat is deposited once a tick the projectile lives,
  where it is after that tick (`Scene::hot_projectiles`, recorded per
  tick as the explosions are), rather than once a frame at its
  interpolated position times the ticks since the last frame. The sum is
  the same, but a frame's one blob would depend on the frame rate (and
  `render-match`, drawing once after hundreds of ticks, would leave one
  spot instead of a path); a tick's deposit lands on the path itself and
  the heat is the same however often frames are drawn (`olr-cli`'s
  `a_projectiles_heat_warms_its_path_drawn_every_tick_or_once`). Keyed by
  the tick's time, slot and serial through `olr-present`'s `Once`: a
  re-simulated tick deposits nothing twice.
- **Caps against a hostile mod** (heat every tick on every projectile):
  at most `olr_scene::MAX_HOT_PER_TICK` (64) heating projectiles a tick
  (slot order), each clamped to 100 degrees over 16 px, sharing the heat
  queue (2048 between uploads) with the explosions in tick order; the
  renderer's own caps (64 deposits a pass, `run_heat` between) then hold.
  64 a tick at 100 Hz is up to 100 extra heat runs a second, a cost no mod
  ships yet (Liero v1.0 Reloaded's values are Materials 10/10).
- **Mod editor:** the projectile's Look group, beside the explosion
  light: Explosion heat (amount, own radius or the engine's) and Heat
  (amount a tick, radius), bounded by the caps; the World panel's Look
  edits `mod.ron`'s explosion light (not editable before) and heat.

## Level editor materials (issue #43)

No OLX counterpart. The seventh of ten (the spec's "Editors"): the level
editor gives a level surface materials, per element and per pixel. The
simulation, the oracle, every match and render golden are unchanged; a
level using no materials saves as before. As built, the builder's calls:

- **The level's Materials** (the Level panel, under its look, beside the
  glow groups): a list of the look's materials, each with its Materials
  view tint, **Add from library** (the built-in `content/materials` and
  the data directory's `materials/`, a copy taken in: the level keeps
  its own), and **Remove**, disabled while it is used, the users named
  ("used by …": elements naming it as a surface or painting its value;
  `olr_edit::look::material_users`, `Document::material_users`). Removing
  one moves the materials after it down a value, the painted maps with
  them (one step). Adding a material to a level without a look gives it
  the default look (it plays lit); removing the look clears the
  elements' surfaces and the painted maps in the same step.
- **Pickers:** an element's Surface (None = the level's default),
  Back surface only with back art (a drawn layer's back, or a raster),
  and a distant layer's one Surface (it takes no painted map). A surface
  must name a material of the level.
- **Surface brush** (`Tool::Surface`, U, the rail beside the Glow
  brush): paints a level material, or "Element's" (0, the element's
  surface), onto the selected element's map at world resolution with
  the brush's shapes and sizes; Front / Back with back art. A stroke is
  one step (the map, then the element's flag); a map all 0 is dropped.
  Locked and distant elements are refused.
- **Views:** the old "Material view" (rock, dirt, background) is named
  **Collision view** now, to tell it from the new **Materials view**,
  which tints each pixel by its baked material (the front's, or the
  back's where the front is carved; 50/50 with the art, untinted grey
  where none) with a legend; the two are exclusive.
- **Save and Play:** Save bakes `look/surface.png` (and the back's) from
  the maps as `olr-edit` does (issue #37); Save as asset carries the
  materials (issue #38); Play takes the edited level's baked surface, so
  explosions heat it.

## Material editor (issue #44)

No OLX counterpart. The eighth of ten (the spec's "Material editor"): a
new crate, `olr-material-editor`, hosted by `olr-game`
(`editor/material.rs`), opened from the Editor screen's **Material** tab.
Nothing in play, the oracle or any golden changes. As built, the
builder's calls:

- **One editor over the whole library**, not one material a session: the
  Material tab offers the recoveries and **Open the material editor**;
  the editor lists the built-in materials (`content/materials`, read-only)
  and the user's (`materials/` of the data directory), and one is open at
  a time. Another one opened, New, Copy or Close with unsaved changes
  asks first (Save changes, Don't save, Cancel).
- **New and Copy write the new file at once** (the mod editor's Edit a
  copy makes its pack the same way): a name is asked, the id is its slug
  (`free_id`: letters and digits, `-` for the rest, `-2`, `-3`, … until it
  is free of the built-in and the user's ids, so a level's copy is never
  joined with another material by id). New starts from stone's heat and
  the engine's ramp; Copy (a built-in one's **Edit a copy**) takes the
  material as shown. **Rename** asks a name and renames the file too (the
  id follows the name, kept when its slug is the same); the saved values
  go to the new file, unsaved changes stay unsaved. **Delete** asks
  first. None of them is an undo step: they are file operations.
- **Fields are clamped as they are set**, to `olr_look`'s bounds (the
  sensitivity and light 0-100, the capacity 10-1000, the conductivity
  0-1000, the cooling 1-1000, a key's glow 0-100), and a ramp key's
  temperature to between its neighbours' (1-1000 at the ends), so an
  edited material is always valid; Save still checks
  (`compile_material`) for a recovered one. Each number is a typed field
  and a slider with its bounds beside it.
- **The colour ramp**: "Own colour ramp" off is the engine's lava ramp
  (an empty `ramp`); on starts from the lava ramp's six keys. **Add key**
  puts a key in the middle of the widest gap (of equal ones the hottest),
  coloured as the ramp is there; at most `MAX_RAMP_KEYS` (16). The strip
  draws the colour from 0 to 1000 over the glow, checks where a pixel
  shows its art, the keys marked.
- **Recovery** is one file a material, `material-recovery/<id>.ron` (the
  material as edited, written canonically but unchecked), every
  `RECOVERY_INTERVAL` while there are unsaved changes, removed once they
  are saved or thrown away, or another material's copy is written; a
  recovered material whose library file is gone is listed as "(not
  saved)" and written again by Save.
- **No `olr-skin-editor` dependency**: its widgets are crate-private and
  the material editor needs none of its painting; the colour is egui's
  picker and a `#rrggbb` field.
- **The preview's place** is the right panel ("Heat preview"), empty
  until Materials 9/10's live previews; the ramp strip is beside the keys.

## Material editor's live previews (issue #45)

No OLX counterpart. The ninth of ten (the spec's "Previews"): the
material editor's right panel shows the material open and one compared,
blasted and heating, drawn by the real renderer's heat pass. Nothing in
play, the oracle or any golden changes. As built, the builder's calls:

- **The host renders, the editor asks**: `olr-game` owns a second
  `Renderer` on the window's device (`editor/heat_preview.rs`,
  `HeatPreview`) drawing into an offscreen texture registered with
  egui-wgpu (`Gpu::register_texture`); the editor crate stays free of
  `olr-render` and asks through `PreviewCommand::{Blast, SetCompare,
  Restart, TimeScale}`, the material open read from its state every
  frame. The preview exists only while the editor is open (made on its
  first frame, its texture freed on the frame after it closes) and draws
  only on frames that draw the editor; once nothing is hot it draws 30
  more frames (`SETTLE_FRAMES`, the glow light settling) and then none
  until something changes.
- **One scene, two patches**: a level of 136 × 72 world pixels, two
  identical 64 × 72 patches (open air over dirt, a rock boulder and
  bedrock) split by an 8-pixel rock wall without a material, which
  neither heats nor lets light through, so the patches never warm or
  light each other. The material open is value 1 (its front and its
  back wall), the one compared value 2; none compared is a material that
  never heats. The look is the engine's at an ambient of 0.1 (dark: the
  air shows almost nothing, solids their floor light, the glow all of
  it).
- **A blast is play's**: a click on either patch blasts both at that
  pixel. Small, Bazooka and Nuke are OLX damage 3, 4 and 15 (the engine
  heat curve's calibration points: a minigun hit, a Bazooka, a Big Nuke
  fragment); the crater is the theme's hole `clamp(damage, 0, 4)` as
  `CarveHole` takes it (`ColorEdit::Carve`, the dirt theme's holes when
  the content has them, else discs of their sizes), so only dirt is dug;
  the heat is `olr_look::engine::explosion_heat(damage)` deposited at
  the pixel's centre after the carve, as `olr-present` does.
- **Its clock** is the frame's time, at most 100 ms a frame
  (`MAX_FRAME_MS`: a stall or a hidden window is not caught up), times
  the time scale (1× or 4×); Restart builds the scene again (craters
  filled, the heat gone).
- **Edits apply live**: a changed field changes the renderer's material
  table without touching the heat (`Renderer::set_surface_materials`,
  new in `olr-render`: the table, the ramps and the bounds rewritten, the
  temperatures, boxes and clock kept); only when no material heats any
  more (or none did) is the surface made again, and then there is no
  heat to keep. A material that does not check keeps the last values
  that did. Selecting another material keeps the left patch's heat under
  the new one.
- **The material compared** is any of the library's (the open one too:
  its saved form beside the edited one); the editor sends it again when
  the library's copy changes (saved, renamed, which it follows; deleted:
  none).

## The Reloaded content's materials and heat (issue #46)

No OLX counterpart. The last of ten (the same spec): OLR's own Reloaded
levels get surface materials and Liero v1.0 Reloaded heat values
(`content/reloaded/README.md` lists them). The levels' collision
material, front and back are byte for byte as before, so every match
golden and the oracle are unchanged. As built, the builder's calls:

- **The rule is a script**, `tools/levels/reloaded_materials.py`: it
  reads each element's art, sorts every colour it holds into a material
  by collision kind and colour (a function per level: hue, saturation
  and how grey), and hands `olr-cli level element surface` one
  exact-colour `--rule` per colour unlike the element's default (in
  batches of 400), after `--clear`, so a run always makes the same
  files. Colour boxes (`--rule`'s tolerance) cannot tell the factory's
  stone from its concrete: the art's cave rock is pure grey and the
  building a grey a step warm, which only exact colours separate.
  `--preview DIR` draws the result as false colours without editing.
- **No material is a default, not a painted value**: a painted 0 means
  the element's default, so an element holding pixels that must not heat
  (Castle Strike's `background-2`: the sky, the hills, the water) has no
  default and the rule paints the rest; the factory's sky is a layer of
  its own. Castle Strike's `background` keeps a default (stone): it
  shows only at the torches, where `background-2` is cut away.
- **Locked layers** (the factory's glowing ones) are unlocked for the
  edit and locked again; each takes one material whole, no rule.
- **The built-in assets** cut from these levels (`content/assets/`)
  were saved again by their README commands and now carry their
  materials (`olr-cli`'s `built_in_assets_are_what_their_commands_make`).
- **The mod's values** keep the engine's heat where it reads well and
  tune the big explosives: a percent of the engine's amount cannot heat
  an explosion of no damage (Big Nuke's detonation stays cold, its
  bomblets carry the blast), and it saturates at 1000 for a worm hit
  (damage 27-32), so the mines' and Bazooka's own radius keeps those
  from glowing 28-31 pixels wide. The flames' 8 a tick over 5 pixels was
  where flamed walls began to show in `render-match` frames (3 hardly
  did: the flames fly in the open, where the back wall takes a fifth).
- **Renders for review** were made with `render-match` (a scripted local
  worm and a chaos bot, review copies of the mod with only the Bazooka,
  nukes and Mine, or only the Flamer or Napalm) and the same frames on
  the levels without materials; they are not goldens. The playtest and
  the Big Nuke frame are in `docs/deferred-verification.md`.

## Heat stops at the rock face (issue #56)

No OLX counterpart. An explosion (or a mod's projectile heat, issue #42:
the same deposit) heats only what it reaches, and conduction carries the
heat in. This replaces issue #40's rule, under which a deposit heated
every material pixel in its radius (rock and dirt whole however deep,
the back a fifth): heat glowed deep inside solid rock, in a tunnel
behind a wall and on both faces of a wall. Presentation only: the
simulation, the oracle and every match golden are unchanged. As built,
the builder's calls:

- **The walk** (`heat.wgsl`'s `heat_reach`): for each pixel in a
  deposit's circle whose material heats, a grid walk from the deposit's
  centre to the pixel's centre through the pixels the segment crosses,
  the centre's own and the pixel included, each classed by the solid map.
  Before the first rock or dirt pixel a back pixel takes `BACK_HEAT`
  (0.2) as before; the first `HEAT_SKIN` (2) solid pixels in a row are
  the skin, the k-th taking `HEAT_SKIN_GAIN` (1.0) × `HEAT_SKIN_WEIGHTS`
  (1, 0.5)'s k-th; past them (deeper rock, or the air once the walk
  leaves the rock) a pixel takes nothing. A centre in rock is its own
  skin's first pixel: it heats its pixel and the 8 round it. The
  constants are `olr_render`'s (`renderer/heat.rs`), prepended to the
  shaders; the maintainer tunes `HEAT_SKIN_GAIN` in play.
- **Corners:** passing exactly through a corner (within 10⁻⁵ of the
  segment) whose two side pixels are both rock or dirt counts as meeting
  the first solid pixel, the glow light's rule, so no heat slips through
  a diagonal wall one pixel thin. Once the walk is in the rock a corner
  adds nothing (a diagonal step through rock is one pixel deeper, not
  two). Outside the solid map is open, as for the glow light.
- **Grazing angles:** a face pixel seen at a shallow angle is reached
  through its neighbours on the face, so it takes the half of a second
  skin pixel or nothing; a flat face heats whole within about 45° of its
  normal from the centre, and conduction along the face evens the rim
  out. The issue's walk, kept as specified.
- **Cost:** the walk stops as soon as the pixel is shielded or past the
  skin, runs only inside the circle and for a heating material, and
  visits at most `HEAT_WALK_STEPS` (2 × `MAX_HEAT_RADIUS` + 4, 132)
  pixels. Each deposit still adds whole texel units, so deposits in one
  pass or several give the same bytes; the CPU's boxes and bounds hold
  (the walk only ever puts in less heat).
- **Distant layers** (issue #41) are unchanged: their solid map is all
  background, so a walk there never meets rock and gives the old bytes.
- **Timing:** the walk reads the solid map as the frame draws it, the
  crater already dug, so its rim heats (as `BACK_HEAT`'s rule).
- **Tests adjusted:** `tests/gpu/heat.rs`'s
  `a_deposit_heats_only_material_pixels_and_spreads_only_into_conducting_ones`,
  `heat_cools_to_nothing`, `a_hot_pixel_adds_glow_light`,
  `a_hot_pixel_shows_its_colour_in_every_view` and
  `a_dug_pixel_is_ruled_by_its_back` deposited inside the test level's
  dirt and read heat deep in it; their blasts now lie in the air beside a
  face (the first over each material, the last with a blast of its own
  past the hole). (Issue #67 replaced `a_dug_pixel_is_ruled_by_its_back`
  with `a_dug_pixel_loses_its_heat`: a dug pixel starts cold.) The
  material editor's preview (`olr-game`'s
  `a_blast_heats_the_patch_and_it_glows`) asks its rim 12 more red, not
  30: only the rim's outer 2 px glow now (43 to 60 on lavapipe).
- **Goldens:** `heat-fresh.png`, `heat-cooled.png`, `heat-distant.png`
  (its level plane) and `lit-hashes.ron` (the Reloaded content heats)
  re-blessed: thin hot rims, the rock behind them cold.

## Light soak by material (issue #57)

No OLX counterpart. The light soak (how deep light seeps into rock and
dirt from the air beside them) was one number per level; it now belongs
to the surface material too: `MaterialSource::light_soak` (0 to
`MAX_SOAK` 12, none the level's `look.light_soak`), carried into
`olr_look::SurfaceMaterial::light_soak` by every copy (a level's
`look.materials`, the library, the assets). The fade curve and the
brightest-in-a-square rule are unchanged; heat is unchanged. As built,
the builder's calls:

- **Whose soak:** an opaque solid texel's front surface material's (the
  visible surface rule of heat: the front's where rock or dirt stands;
  the soak reads only opaque solids, so the back never counts), else the
  level's. A pixel outside the maps, with no material or a material
  without `light_soak`, takes the level's.
- **The two passes** each take the receiving texel's radius `r` and fade
  with `fade(i, r + 1)`. At a border between materials the down pass
  reads its column's across results, made with those texels' radii: a
  texel of a thin material next to a deep one can take, down its column,
  light its neighbour soaked across. Accepted (the issue's ruling).
- **The map:** the front surface map is static; the solid map beside it
  follows every dirty rectangle, so digging and new dirt change the soak
  at once. The soak binds the heat's own surface texture when the level
  heats, else an `R8Uint` front map of its own: one copy either way.
- **Bytes:** a level without a material that sets a soak draws today's
  bytes (the plain soak pipelines, no table, no map); one with such a
  material runs the across pass even at a level soak of 0.
- **`olr-cli look-preview`** now hands the renderer the level's surface
  materials and maps as play does (it gave none before, so it showed no
  material soak and no cold heat; with nothing hot the heat changes no
  byte).
- **Content:** Liero Factory Reloaded's `concrete` and `stone` (its
  floors, ceilings, walls and ground) and Castle Strike Reloaded's
  `stone` soak 2; the levels' own soak stays (6 and 12), so the
  factory's furniture (already `metal` and `wood` in its surface map, no
  re-paint needed) and the castle's brick, wood and metal keep their
  light. The library's `concrete`, `stone` and `brick` soak 2; the
  built-in assets cut from those levels were saved again with
  `olr-cli asset save` (their commands in `content/assets/README.md`)
  and differ only by the soak of their copies.
- **Material editor:** a Light soak combo box ("Level's", 0 to 12 px).
  The live preview draws lit solids with no level look (the level's soak
  0 there), so a material's own soak shows as the hot rim's light seeping
  in.

## Layer folders (issues #59, #60, #61)

No OLX counterpart. A level's composition gains nested layer folders, so
an object's pieces (a desk with its PC and chair, a lamp and its glow
layer) are selected, moved, hidden and locked together. Named folders
throughout: "group" is the look's glow groups. Part 1 (issue #59) is the
format, `olr_edit::folders`, the bake and `olr-cli`; part 2 (#60) the
editor's document, history, clipboard and assets; part 3 (#61) its
Elements tree and canvas. As built in part 1, the builder's calls:

- **The format:** `Composition::folders` (`Folder { id, name, visible,
  locked, collapsed, parent }`) beside `elements`, which stays the
  bottom-to-top list every bake walks; `Element::folder` its innermost
  folder. Both are left out when empty, so a composition without folders
  reads and writes byte for byte as before (every committed one, a
  test).
- **The rules are the format's** (`check_folders`, run by
  `check_elements`, so by every load and every pack edit): one id
  namespace with the elements; every reference known; no cycle, at most
  `MAX_FOLDER_DEPTH` 16 deep and `MAX_FOLDERS` 256; every folder one
  contiguous run of `elements` and never empty; distant elements never
  in a folder with ordinary ones; `folders` in the canonical order (by
  run start, a parent before its children). The order is a rule, not a
  normalisation on read: a hand-written file listing them otherwise is
  refused with a message naming the folder, so equal trees always save
  alike. `olr_edit::folders` re-exports the check and the bounded walk
  (`folder_chain`) rather than holding a copy: the format, a lower layer,
  needs them for the load.
- **The tree operations** (`olr_edit::folders`) build the tree from the
  runs, edit it and write it flat again, which drops every folder left
  empty and lists the rest canonically; a result breaking a rule (or the
  distant layers' order) is refused and the composition left as it was.
  Every pack operation runs `tidy` after its edit, so one emptying a
  folder removes it in the same step.
- **Group** puts the new folder inside the innermost folder all members
  share; a member lying in another selected folder goes with it; members
  not one block move to the topmost member's place, or, when that member
  lies in a subfolder of the shared folder, just above that subfolder.
  The name defaults to the first free "Folder n".
- **Raise and lower** swap an element or a folder (its whole run) with
  its neighbouring sibling, so an element below a folder jumps past the
  folder's run; at a folder's edge an element or subfolder leaves it,
  landing just outside (the stack unchanged); the top and bottom stay.
  Distant layers stay below the rest (`DISTANT_ORDER`).
- **Move** (`set --folder`): into a folder on top of it, at a position
  when given; out to a folder holding it (`none`: the top level) just
  above the folder it leaves.
- **Locks** (plan 14d R4): an element is locked when it or any folder
  holding it is, which refuses its content changes (`set` but `Locked`,
  draw, erase, fill, masks, glow, surfaces, merge, split, remove); group,
  ungroup, move and reorder are allowed. A locked folder takes only
  `Locked` itself (as an element), and removing a folder is refused while
  any member is locked.
- **The bake** reads effective flags (`folders::fold`: shown when it and
  every folder holding it are visible, locked when any is): `Baker`
  bakes folded clones, and `DecodedLevel::of_pack` cuts assets from
  them. Decoding follows the element's own `visible`, so hiding or
  showing a folder rebakes and never decodes again (a hidden folder's
  pictures are still processed; a test).
- **Other operations:** `split`'s parts take the element's folder,
  `merge_down` the lower element's, `duplicate` places the copy directly
  above in the same folder; an element made (or made no longer) distant
  leaves its folders. Assets carry no folders (`asset::plain` and `land`
  clear them; `check_asset` refuses one); placing an asset of two pieces
  or more puts them in a new folder named after it.
- **The editor** (part 2, issue #60; no new UI before part 3):
  - **The document** holds `folders` beside `items` and saves them with
    the composition (save, `to_pack`, `PackSnapshot`, every check). A
    folder a removal or a flat move emptied stays in the list, so its
    undo finds it; `composition()` (what is saved and checked) leaves
    such folders out and lists the rest canonically (`folders::tidy`), so
    removal and the flat move stay one edit each (`Change`, as before).
  - **Effective flags:** the rebake bakes the folded `visible` (only when
    a folder is hidden) and compares the distant items so folded, so
    hiding a folder of distant layers makes them again; decoding and the
    image grids follow the element's own `visible`, so a folder toggle
    only rebakes. Every lock check of the document and the canvas reads
    the effective lock (`locked_at`; `with_property` takes it), `hit`
    and the overlays the effective visibility (`shown_at`).
  - **Edits:** `Edit::Folders { before, after }` swaps the whole folder
    list, rebaking the members whose visibility changed; an element's
    `folder` travels in its `Element` edit or in `Move { from, to,
    before, after }`. Group, ungroup and moves into or out of a folder
    are `olr_edit::folders`' on the composition, carried out by
    `restructure` (the folder list, then the moves, then each changed
    element): one step each.
  - **The flat move** (the Elements list's drag, `move_to`) lands the
    element in the innermost folder holding both its new neighbours;
    at a folder's edge it stays in a folder it was already in (a reorder
    at the edge of its folder keeps it in), else it is outside. Every
    folder stays one block.
  - **Folder fields** (`FolderField`: name, visible, locked, collapsed):
    a locked folder, or one in a locked folder, takes only its lock and
    its folding. Removing a folder removes its elements (refused while
    any is locked) and drops it in the same step.
  - **Split** parts take the element's folder, **merge down** keeps the
    lower layer's, **duplicate** (`Document::duplicate`, no UI yet)
    places the copy directly above in the same folder, and an element
    made distant or no longer leaves its folders, as part 1's pack
    operations; split and merge see a hidden folder's members as hidden.
  - **Clipboard:** `copy_tree` copies a folder all of whose elements
    are copied (a folder id stands for its elements), with its subtree;
    the copies name only copied folders. A paste (`paste_tree`) makes
    them anew (new ids, a name taken gets `copy_name`d) and lands the
    block on top, outside every folder of the level's. `copy` stays
    flat.
  - **Assets:** placing one of two pieces or more groups them into a
    folder named after it in the same step; the editor selects its
    elements (part 3 selects the folder). Save as asset saves the pieces
    flat (`plain`), a hidden folder's members cut as hidden.
  - **Selection** is unchanged (`Element`, `Many`); `EditorState::group`
    takes a folder id as its elements rather than dropping it (part 3
    selects the folder itself).
- **The editor's tree and canvas** (part 3, issue #61; Figma's and
  Illustrator's behaviour, the maintainer's choice 2026-10-23):
  - **Selection:** `Selection::Folder(id)` stands for the folder's whole
    run; `Many` holds elements and folders in stack order (a folder by
    where its run starts), leaving out an id a selected folder holds
    (`EditorState::group`). `selected_elements()` expands the folders;
    the drag (`Drag::MoveMany`), the arrows, `SetMany`, delete
    (`Document::remove_items`, the emptied folders in the same step),
    copy and cut (`copy_tree` of the folder ids) and Save as asset (named
    after a lone folder) each act on it as one step. A paste selects its
    top-level items, so a pasted folder is selected whole; placing an
    asset of two pieces or more selects its folder.
  - **The Elements list** is the tree (`Document::rows`: top first, a
    folded folder's members left out): a folder row has a fold arrow, an
    eye, a lock, a mark and its name; members are indented 14 points per
    level, and an eye or lock a folder holding it overrides is dimmed. A
    click selects a folder, Ctrl adds it, Shift selects the rows shown
    between (a folder row standing for its folder, so a range over a
    folder's row and its members' is the folder). A row dropped on
    another takes its place, in that row's folder (below it when it came
    from above, as the flat list always did), so a drop on a row outside
    a folder moves it out; one dropped on a folder row's middle goes into
    it, on top; the top 30 % of a folder row (and the bottom 30 % of a
    folded one) drops beside it. A line or the folder's outline shows
    where. These are `Document::drop_row` (`RowDrop::{Above, Below,
    Into}`, through `move_into`), and a drop the folder rules or the
    distant order refuse says why in the status bar. A level without
    folders still reorders through `MoveElement` (`move_to`, which clamps
    a drop across the distant layers rather than refusing it). "New
    folder" beside Add layer groups the selection.
  - **Canvas clicks** (Select): a click selects the outermost folder
    holding the element hit, or the element; a selected item on the way
    to the element hit (the folder, a member of the group, the element
    itself) stays selected and drags, so a folder moves only once
    selected (as a drawn layer). A double click (`EditorAction::
    DoubleClick`) selects the selected folder's child holding the hit
    and makes that folder the focus (`EditorState::focus`): plain clicks
    on its members then pick its children, and a click outside it (or on
    nothing) leaves it. Alt+click (`EditorAction::AltDown`, a press for
    any other tool) selects the element itself and focuses its folder;
    a list click focuses the item's folder. Ctrl+click toggles at the
    level a plain click picks. Escape, after closing a popup,
    `CancelPlacing` and `DropPixels`, selects the parent folder
    (`SelectParent`), and at the top level clears the selection; not
    when a text field had the keyboard as the frame began (its Escape
    drops the draft). A selected folder's outline is one box around its
    shown members' boxes.
  - **Shortcuts:** Ctrl+G groups the selection (`GroupSelection`, the
    new folder selected), Ctrl+Shift+G ungroups the selected folder
    (`Ungroup`, its children selected); Ctrl+Shift+G is checked first
    (`consume_key` ignores an extra Shift), both before G (Fill).
  - **The properties** of a folder: Name (committed when typing ends),
    Visible and Locked (its own), Ungroup, then its elements' count and
    the group panel's shared material, Delete and Save as asset.
  - **Tools needing one element** (the brush, eraser and Restore, the
    shapes, Fill, the Wand, Select pixels, Glow and Surface brushes)
    refuse a folder as they refuse a group, their message adding how to
    go inside (`DOUBLE_CLICK_INTO`).
- **`olr-cli level element list`** prints a folder as `folder <id>[
  "<name>"][ hidden][ locked][ collapsed]` before its first member,
  members indented two spaces per folder; a level without folders prints
  exactly as before.

## Glowing distant layers (issue #62)

Part 1 of 2 (part 2, #63, puts the Reloaded skies on parallax layers:
"Reloaded levels (issue #7)"). A
distant (parallax) layer may glow, as an ordinary image or drawn element
does (`Element::glow`: strength, group, painted mask); before, the format
refused it. No OLX counterpart (OLX has neither parallax nor light). As
built:

- **The light is baked at the reference placement.** The glow light runs
  once a frame for the whole world (issue #26), but a distant layer is
  drawn at a different world place for every camera (`at + depth ×
  (camera − centre)`): light following it would need one glow-light run
  per view. So a layer's light comes from where it lies with the camera
  at the level's centre (its box, `at`): the bake (`olr_edit::compose`)
  writes, for every see-through back pixel (no element covers it), the
  glow and group of the nearest visible distant layer covering it there
  into `look/glow-back.png` and `look/groups-back.png`, exactly as a
  glowing back element would, and that layer's colour at alpha 0 (so a
  group coloured `Own` emits the layer's colour). The nearest covering
  layer is opaque: a nearer layer that does not glow hides a farther one
  that does; an opaque back pixel never takes a distant glow. The
  renderer's emission is unchanged (it reads the glow maps whatever the
  back's alpha), so the light enters the level through the openings with
  no per-view cost, and a glowing sky element replaced by a glowing
  distant layer of the same art and glow lights the level the same.
- **The picture is lit by its own glow where it is drawn.** The distant
  quad's instance carries the group and the strength (the shade's `y` and
  `z`; `INSTANCE_FLOATS` unchanged), a mask texture per layer; the lit
  distant shader gives a texel self-light `strength × mask × the group's
  self-light scale`, with the group's flicker (the layer flickers as one
  region), at its drawn (parallax) place; a hot layer (issue #41) takes
  the larger self-light. The composite floors the light at self-light and
  shows a self-lit texel's art with no glare, so a glowing sky shows its
  art as before. The drawn pixels never cast light nor feed the bloom.
  Unlit frames draw the layer's art as before.
- **The trade-off, for authors.** A small bright feature on a distant
  layer (a lit window in a far city) lights the level as if the camera
  were at the level's centre: with the camera far from the centre, its
  light and its drawn place part. A broad source (sky, sunset, moon glow)
  shows no difference. `docs/formats/level.md`'s `DistantEntry` and the
  editor's Glow hint on a distant layer say so.
- **The format.** `level.ron`'s `DistantEntry` gains `glow` (percent) and
  `glow_group` (an id of the look's groups), and a painted mask is
  `distant/<i>-glow.png` (grey, the layer's art size); a level without a
  glowing distant layer reads and writes byte for byte as before. The
  lenient reader plays a bad distant glow without glow. A removed glow
  group unglows the distant layers naming it. `asset.md` changed only in
  the shared `Element::distant` text (it said "no glow"); assets still
  carry no distant layers.
- **Not done** (the issue's non-goals): no per-view light, no change to
  the glow light's passes; distant layers still never collide and are
  never clear.

## Short shipped paths (issue #64)

Explorer's **Extract All** stops at `MAX_PATH` (259) and extracts the
Windows zip into a folder named after it, so the version is in the path
twice before the zip's top folder (about 123 characters before
`content\` for a 4-letter user name). Every file the release ships is
now at most 80 characters below that top folder (`content/…`,
`olr.exe`), leaving room for a longer user name, OneDrive and the
updater's `.olr-update\new\`. The zip's name and top folder, which
installed builds' updates rely on, are unchanged. As built:

- **Compact projectile keys.** OLX names nothing: a nested projectile's
  key is OLR's. `olr_data::mod_def::child_key` gives the root's name, a
  dot and the chain from it as `s` (spawn) and `t` (trail), a run its
  letter and count: `rocket.spawn` became `rocket.s`,
  `medical-camp.spawn.spawn.spawn.trail.spawn…` (137 characters as a
  path) `medical-camp.s3ts9t`. Both the importer (`intern`) and the
  format (`compile_mod`'s and `lift_mod`'s derived keys, `olr_edit`'s
  walk) use it, so imported files still carry no explicit `id`; a key
  ending in no such chain is a root (`crackler.trail` → `crackler.trail.s`).
  The simulation's own occurrence names (`SimProjectile::name`,
  `<path>.spawn`, as the oracle traces' headers name OLX's projectiles)
  and its load order are unchanged: the oracle and the match goldens
  hold. `content/packs` is re-imported (names only) and Liero v1.0
  Reloaded's shared projectiles follow Classic's (`bazooka.s`,
  `big-nuke.s2`, `crackler.t`), as its `ModDef` must equal Classic's.
- **Liero Factory Reloaded's rock layers** are `rock-display`,
  `rock-screens` and `rock-beacons` (were `displays-in-rock`, …): its
  `editor/layers/` is 64 characters. The bake and the `lit-sign` asset
  are otherwise byte for byte as before.
- **The guards:** `xtask`'s `the_shipped_content_keeps_its_paths_short`
  (in `check`) and `write_zip`, which refuses a zip with any longer
  entry below its top folder (`dist::MAX_SHIPPED_PATH`, UTF-16 units).

## Every screen fits (issue #49)

No OLX counterpart (OLX's menus are a fixed 640 × 480). Every menu
screen, Options tab, dialog and in-match panel fits a window at 320 and
384 points (UI scale 3 in borderless fullscreen on 4:3 and 16:10
desktops), 427 points (1280 × 720 at scale 3), 1280 × 720 at scale 1,
4K and 3440 × 1440, the in-match panels at the match's scale (the 16:9
frame's height). `olr-ui`'s `tests/fits.rs` checks it: every widget
lies across the window, and every control, focused, scrolls whole into
it. As built, the builder's calls:

- **No redesign:** a wide window looks as before (every render golden
  and HUD snapshot is unchanged). Below a width, a screen stacks what
  stood side by side: a form row's name over its control (under 450
  points), the Keys tab's table as a list per action (under 560),
  Local Play's and the lobby's map over the match settings and the
  players, chat last (under 640), the profiles' panels (under 700), the
  loadout editor's list over its details (under 520).
- **A short window scrolls the whole screen:** below 480 points tall a
  screen is laid out 480 points tall in a scroll area, and a dialog
  scrolls inside the window. Lists that have a scroll area of their own
  (levels, skins, members, loadouts) lose it there and scroll with the
  screen: a list a few rows tall inside a scrolling screen was hard to
  use, and egui could not reliably show its rows. The Editor's tabs
  scroll on their own there, as its page column hides its overflow.
- **Scroll areas follow the focus** (`pad::follow_focus`): a keyboard's
  Tab, a pad or an accessibility focus brings the control into every
  scroll area holding it, the innermost first, one area a frame.
  egui's own `scroll_to_me` (accessibility's ScrollIntoView) moves only
  the innermost area; the mouse wheel passes to the outer area when the
  inner one is at its end.
- **The results' table narrows** in a narrow window (monospace 11, a
  smaller gap, names cut with "…"); the HUD's (F1) is unchanged.
- `olr-game`'s `finding` test now waits for the relay's stored games to
  end before checking the Internet tab's status: under the whole
  workspace's load the server's game came among them, a poll earlier.

## Distant layers stay where placed in the editor (issue #66)

No OLX counterpart (distant layers are OLR's, issue #11). The level
editor drew and hit-tested distant layers as if the game's camera were at
the canvas's centre; the wheel zooms about the pointer
(`View::zoom_about`), so every step moved that centre and, by `depth ×
(camera − level centre)`, every distant layer, with nothing panned. As
built:

- **Reference placement by default.** The editor's camera for distant
  layers (`EditorState::parallax_camera`) is the level's centre, so a
  distant layer shows and hits at its `at` (its box in
  `docs/formats/level.md`) whatever the pan and zoom: `parallax(e)` is
  (0, 0). Its picture and the light baked at that placement (issue #62)
  coincide.
- **View ▾ Preview parallax** (`labels::PREVIEW_PARALLAX`, beside Grid;
  `EditorAction::PreviewParallax`, `preview_parallax()`): off at first;
  on, the camera is the canvas's centre (`View::centre`) as before, so
  panning and zooming move the layers as in play, and their light parts
  from the picture as in play. A view option like Grid: state only, not
  in `EditorSettings`, never a step, acting while a dialog is open.
- **One camera.** `olr-game`'s `Canvas::draw` takes the editor's camera
  (`parallax_camera()`) for the draw list's `parallax`, instead of
  computing the canvas's centre itself, so the drawing and the
  hit-testing never disagree.
- **Unchanged** (the issue's non-goals): play (its camera,
  `DrawList::camera()`, the renderer's parallax), the zoom about the
  pointer and panning, and the level format.

## Heat on one depth line (issue #65)

No OLX counterpart. Background heat gave no depth (an explosion warmed
the back wall in a circle as big as its rim, `BACK_HEAT` 0.2 of it), and
dirt, which the blast carves away, barely glowed at its crater's edge.
The maintainer chose from rendered frames (variant E of five).
Presentation only: the simulation, the oracle and every match golden are
unchanged. As built:

- **One depth line.** A deposit is a ball centred on the worms' plane.
  The level's back wall lies `olr_look::engine::BACK_DEPTH_PX` (14) world
  pixels behind it, in pixels rather than a share of the radius so that
  huge explosions reach further: a back pixel at distance `d` takes
  `amount × (1 − (d² + 14²)/R²)` (a minigun's hit, 12.5, nothing; a
  Bazooka's, 17, a circle of radius 9.6 whose centre takes 32 %; a Big
  Nuke's fragment, 22.5, one of 17.6, 61 %). Smaller than before but
  brighter at the centre than `BACK_HEAT`'s fifth, so a mid-air blast
  leaves a disc on the wall; the maintainer kept it, as the ball has it.
  `BACK_HEAT` is gone. The deposit
  pass reads the back depth from the table's head (`w`: 14 for the level,
  0 for a distant layer).
- **Distant layers** lie behind the wall, `z = layer_z(d) = 14 + 1.1 d`,
  and take the CPU's slice whole (no fifth on top): a Big Nuke's
  fragments reach depth 7, a Bazooka's depth 2; `layer_heats` stops at
  depth 45 (the largest radius, 64). The back wall stays its own thing
  (one texture, conduction, the solid map); only the heat's geometry is
  shared.
- **Dirt takes the blast whole.** A dirt pixel in the walk takes
  `amount × DIRT_HEAT_GAIN (2.5) × DIRT_SKIN_WEIGHTS[k − 1]` (1, 1, 0.8,
  0.6, 0.4, 0.2) with no falloff, `DIRT_SKIN` (6) deep while every solid
  pixel met is dirt; once the walk meets rock, rock's two pixels hold
  (rock shields the dirt behind it). Rock is unchanged. `HeatSim`'s bound
  counts dirt's gain, so a hot dirt box is never dropped while it glows.
- **Small heat no longer reaches the back.** A projectile's heat is
  capped at 16 px, so at most a 7.7 px circle at 23 % warms the wall
  behind it; Liero v1.0 Reloaded's Flamer (5 px) and Napalm (6 px)
  flames heat only what they touch. Kept so, as the maintainer chose;
  a wider radius would need the cap raised.
- **Frame rate.** Heat a ball smaller than 14 px leaves lands only on
  solids, and on dirt that is mostly dirt a bullet then digs: drawn every
  tick, its walk sees the dirt before the hole; drawn once at the end, the
  crater edge behind it. `olr-cli`'s
  `a_projectiles_heat_warms_its_path_drawn_every_tick_or_once` therefore
  gives its bullets the cap's 16 px, whose paths through the air warm the
  back as before (2749 of 11843 hot texels differ, all near a dug pixel;
  with 6 px, 1321 of 1435). `R` (24) and the quarter are kept. In play a
  frame spans a few ticks, not a match, so the difference is a pixel or
  two at a bullet hole's rim.
- **Fixture:** `look-test-heat-distant`'s palisade moved from depth 8 to
  4 (`z` 18.4, within a Big Nuke fragment's 22.5).
- **Tests:** issue #56's rim tests run on rock levels;
  `the_back_takes_the_balls_slice_14_px_back`,
  `dirt_takes_the_blast_whole_six_pixels_deep`,
  `rock_shields_the_dirt_behind_it`, `a_hot_dirt_box_outlives_its_glow`;
  the distant tests at the new `z`. The heat goldens and the lit hashes
  were re-blessed ("heat on one depth line, dirt at full force").

## Heat stays in its layer (issue #67)

No OLX counterpart. The maintainer played development build 700: the
glow on the edges of dirt and of objects made of dirt looked wrong; it
belongs on the dirt pixels only and goes with them when they are
destroyed. Issue #65's dirt heat (2.5×, 6 px deep) made two old rules
plain: hot dirt warmed the background wall beside it (a glowing fringe
round every dirt patch), and a dug pixel kept its heat under its back
(the dirt's glow lingered where the dirt was). Presentation only: the
simulation, the oracle and every match golden are unchanged. As built:

- **Conduction stays within its layer.** In `fs_heat_step` two
  4-neighbours exchange heat only when both are solid (rock or dirt, the
  solid map's `heat_is_solid`) or both are background: a neighbour of
  the other layer's coupling is 0, nothing else changes, so the step is
  still a convex combination. Rock and dirt still warm each other (the
  maintainer's choice); the back wall's heat still spreads along it; a
  clear solid, background in the solid map, conducts as background.
  Whether a pixel faces the air (its cooling) is unchanged.
- **A pixel that changes layer starts cold.** `HeatTextures` keeps
  `seen`, an `R8Uint` copy of the solid map as the heat last saw it.
  Each `update_terrain` rectangle that rewrites the solid map
  (`LookTextures::write`) is, while anything may be hot
  (`HeatSim::active`), a layer reset queued in `HeatSim`'s waiting list
  with the deposits, in call order (`HeatSim::relayer`): after every
  deposit and step already queued, before any queued later. The frame
  runs it as `HeatOp::Layer`: `fs_heat_layer` zeroes, over the boxes
  within the rectangle (scissored, into the scratch, copied back), each
  pixel whose class (solid or background) differs from `seen`'s, then
  copies the rectangle of the solid map into `seen`. Rock ↔ dirt is no
  change. With nothing hot nothing is reset and no pass runs: the same
  bytes are written into `seen` as into the solid map (an upload of the
  dirty rectangle's bytes, the solid map's again). A new `HeatTextures`
  copies the solid map into `seen` in a submit of its own, and a
  `HeatOp::Clear` (a reset) copies it whole. A rectangle queued right
  after another joins it; past `MAX_HEAT_LAYERS` (64) waiting, one joins
  the last (a larger rectangle compares more pixels and resets the same
  ones; only a hostile host would reach it).
- **The order is call order**, so a fresh crater's back still takes its
  slice of the blast that dug it: `Presentation::upload` runs
  `update_terrain`, then `upload_heat`'s deposits. The reset reads the
  solid map as the frame draws it (as the step and the deposits' walk
  do): a pixel dug and filled again between two frames is no change.
- **The material editor's preview** deposited in `blast()` before its
  `frame()` uploaded the crater, so under call order its crater's back
  lost the blast's heat: `blast()` now calls `update_terrain` right
  after carving, before depositing, as play does
  (`a_second_blast_on_a_hot_patch_heats_its_crater`).
- **Distant layers** are unchanged: their solid map is all background,
  and they take no `update_terrain`. `HeatSim`'s boxes and bounds hold
  as they are: both rules only take heat away.
- **Frame rate.** `olr-cli`'s `heat_is_the_same_drawn_every_tick_or_once`
  (27837 hot texels, 3961 differ, all near a dug pixel) and
  `a_projectiles_heat_warms_its_path_drawn_every_tick_or_once` (11150
  hot, 2022 differ, all near a dug pixel) pass with their `R` and
  quarter unchanged.
- **Tests:** `tests/gpu/heat.rs`'s `heat_stays_in_its_layer`,
  `a_dug_pixel_loses_its_heat` (in place of
  `a_dug_pixel_is_ruled_by_its_back`), `a_fresh_craters_back_takes_its_slice`
  (with and without heat before the carve), `new_dirt_starts_cold`;
  `heat.rs`'s `layer_resets_land_in_call_order`; `olr-game`'s
  `a_second_blast_on_a_hot_patch_heats_its_crater`. The heat goldens
  (`heat-fresh.png`, `heat-cooled.png`) and `lit-hashes.ron` were
  re-blessed ("#67: heat stays in its layer": no fringe on the
  background round the dirt); `heat-distant.png` came out the same.

## A cooling pixel fades back to its cold look (issue #68)

No OLX counterpart. The maintainer played development build 700: heated
background, as it cooled, first turned dark and only then snapped back
to its own colour. The lit terrain mixed the ramp's colour over the art
by its glow (the lava ramp's cool end is dark red), took the glow as
self-light (capped at 1, no bloom), and dropped both at a glow of 0.
Presentation only: the simulation, the heat (deposit, step), the ramps,
the glow light's emission, the oracle and every match golden are
unchanged. As built:

- **A hot pixel is its cold look plus its heat as emitted light.** The
  cold part is everything the texel shows without heat (its art lit as
  before: the light, the cap, the bloom, the soak, the worm halo, its
  own glow from the glow maps); heat is no longer self-light, so it
  changes none of them. The heat part, `E = to_linear(ramp colour) ×
  g²` (`g` the ramp's glow, 0-1), is added to the composite's linear
  light after the cap, before the grade. The art takes no part in `E`:
  it shows through the cold part, lit by the light that reaches it, the
  heat's own cast light (`HEAT_CAST`) included. Before the grade a hot
  pixel is never darker than cold, and with a ramp whose colour and glow
  do not fall as it heats (every channel of `LAVA_RAMP` rises) it moves
  steadily, channel by channel, to exactly its cold bytes as it cools.
- **How `E` reaches the composite:** a third target of the lit world
  pass, the heat light (`HEAT_LIGHT_FORMAT`, `Rgba8UnormSrgb`: linear
  light kept as sRGB bytes, as fine as the picture in the dark, blended
  in linear light), cleared to 0; `hot` in `world_lit.wgsl` writes it for
  the terrain (rock, dirt, the back, its opaque backing) and hot distant
  layers (`fs_distant_lit`, issue #41); a sprite writes 0 with its alpha,
  so it blends the heat under it down as it does the shade. The
  composite reads it at binding 8. A level without heat writes 0 there
  and draws the same bytes (`no_materials_draws_todays_bytes`, the
  unheated render goldens, `look_none_draws_todays_bytes`).
- **In the dark** a hot pixel looks about as before, not to the byte:
  the art at a rim is lit by the heat's cast light, so where that light
  is strong (a white-hot rim) the art adds to the ramp's colour and the
  rim reads whiter than the ramp alone. A dull red glow on a brightly
  lit wall barely shows, as embers in daylight.
- **View modes.** Full bright and light only keep issue #39's look of a
  hot pixel (the ramp colour over the art by the glow, the glow as
  self-light, no heat light): the material editor's full bright and
  `a_hot_pixel_shows_its_colour_in_every_view`'s full-bright case are
  unchanged.
- **`to_linear`** moved to `common.wgsl` (the composite and the glow
  light shared copies; the world pass now needs it too).
- **Tests:** `tests/gpu/heat.rs`'s `a_cooling_pixel_fades_back_to_its_cold_look`
  (a hot back pixel of a lava material casting no light, under ambient
  1.6 with bloom, the grade neutral, ten heat steps a frame until the
  hottest pixel is under 97°: no texel below its cold value less 1, the
  centre's distance from cold never growing by more than 1, then exactly
  the cold frame), `a_cooling_distant_layer_fades_back_too` (the same on
  a distant layer at depth 5), `a_hot_rim_still_glows_in_the_dark` (a
  blast of the Bazooka's radius, 17, at the hottest amount on dirt:
  yellow-white over its centre, redder at the first pixel along the face
  at half the heat or less). `a_hot_pixel_shows_its_colour_in_every_view`
  checks its "redder at half the heat" pixel at (30, 74), the dirt
  skin's last pixel, which does take half since issue #65: (30, 78) is
  at the full heat, and lit it now shows its white art lit by the heat
  plus the ramp, white. Both cooling tests fail on the old shaders.
- **Goldens:** `heat-fresh.png` and `lit-hashes.ron` were re-blessed
  ("#68: a cooling pixel fades back to its cold look": the hot back
  keeps its light and bloom under its heat, white-hot rims read whiter). `heat-cooled.png` and `heat-distant.png` change too (the
  cooled rims redder, none darker than the rock or the back round them),
  but within the goldens' perceptual tolerance, so the bless kept them.

## A hot back wall lets the light through

No OLX counterpart. The maintainer played development build 707: on
Castle Strike Reloaded's brick hall, a mortar's glow on the back wall
still faded into what looked like a shadow, after issue #68. The cause
was the glow light, not the look: a hot pixel's cast light made its
cell a glowing cell, and a glowing cell stops a ray (`stops` in
`gi.wgsl`), as a lamp or the sky does. The hot patch blocked the light
from the windows and the chandelier, and gave back only its own heat
light, which fell to nothing as it cooled while the cell kept blocking
until its glow was 0: a dark ring round the glow, then a dark patch and
a shadow beside it for seconds after the red was gone. Issue #68's
cooling test cast no light (`light: 0`), so it never saw it. Worked out
in a session with the maintainer from rendered frames of the hall and
the heat test cave (their choice: keep the back wall's light on the
room, at the strength a glowing cell has, "1× for now"). Presentation
only: the simulation, the heat, the ramps, the oracle and every match
golden are unchanged. As built:

- **Hot open background is haze to the glow light.** `fs_emission`
  puts the cast of a hot pixel that is open in the solid map (the back
  wall, a clear solid) into a fourth target, the haze (`Rgba16Float`,
  averaged over the cell), instead of the emission; hot rock and dirt
  still cast into the emission and stop rays as before.
- **A ray takes the brightest haze it meets and goes on.** A hazy cell
  is a jump-flood seed (`gi_seed.wgsl`), so the march walks it cell by
  cell and never jumps it; the ray keeps the brightest haze it met (by
  luminance, × `fade(distance, HEAT_REACH)`, the reach in the cascade's
  params, `e.z`), added to what stops it or, open, to the next
  cascade's light. A cascade's alpha (read by no other pass) carries the
  luminance of the haze in its light: merging, a ray keeps the brighter
  of its own haze and the next cascade's, never both (the next's taken
  off in the ray's own haze's colour). So a hot patch lights as the
  glowing cells did, a wide one no more than one hot cell: a first
  version that summed the haze along the path (× 0.25 a world pixel,
  the maintainer's first pick) flooded the heat test cave yellow after
  its nuke and was dropped before commit. A ray out of steps stays
  blocked (0), haze or not.
- **Strength 1×:** haze gives a ray what the glowing cell gave
  (`HEAT_CAST`); 2× tinted Castle Strike's hall more, as the first pick
  did, but washed the heat cave's heavy blasts out. The patch's own red
  is softer than build 707's, whose dark ring set it off.
- **Cost:** the emission pass writes a fourth target (8 bytes a cell)
  and the seed pass reads it, every lit frame that casts; a ray reads
  the haze only on its walked steps, and walks more only near hot
  background. Unmeasured: `docs/deferred-verification.md`.
- **Tests:** `tests/gpu/heat.rs`'s `a_hot_back_wall_lets_the_light_through`
  (a lamp left of a hot patch of casting stone back wall, in the dark:
  the light beyond the patch is at least 99 % of cold while it glows,
  then `fades_back`, which now takes its look). It fails on the old
  shaders (the light beyond fell from 1.0 to 0.03).
- **Goldens:** `heat-fresh.png` and `lit-hashes.ron` re-blessed ("a hot
  back wall lets the light through"): the cave's back wall between the
  blasts takes their warm light where it was shadowed. `heat-cooled.png`
  changes too (a faint red fill), within the goldens' perceptual
  tolerance, so the bless kept it.

## Beams light and heat (issue #69)

No OLX counterpart. The maintainer asked for beam weapons (Liero v1.0
Reloaded's, Classic's and MW 1.0's Lasers) to light what is round them
and heat what they hit, a laser's tunnel through dirt glowing at its
edges. The look was picked on 2026-10-24 from a throwaway spike's
frames (a laser boring into Liero Factory Reloaded's dirt): of four
lights (none, along the beam, at the tip, both) and three heats (dull,
medium, hot), **both lights with medium heat**, as the engine's default
for every beam weapon, a mod's own values set in the mod editor.
Presentation only: the simulation's state, the oracle, the checksums
and every match golden are unchanged. As built:

- **The record** (`olr-sim`'s `beam.rs`, cosmetics only, in no
  checksum): `CosmeticKind::Beam` gains the weapon and `stopped` (a part
  stopped on rock, dirt or a worm), and each part that stopped on rock or
  dirt inside the level gives a `CosmeticKind::BeamHit { worm, weapon,
  parts }` at that pixel, `parts` the sub-step's count of them (at most
  `MAX_BEAM_HITS` 8 recorded). The end pixel could not tell: the carve
  has just emptied it. **The level's edge stops nothing:** `solid` reads
  past the edge as rock (OLX's rule, kept for the trace), but a beam fired
  into an open sky ends there with no tip light and no heat
  (`a_drawn_beam_says_what_stopped_it`). A beam's carve explosions carry
  `proj: BEAM_EXPLOSION` (-2) instead of -1, so they keep their sprite
  but cast no light: the tip light replaces them (the maintainer's
  choice); a bonus blown up's and an explosive trail's stay -1 and keep
  the engine's light (`trail_explosions_keep_the_engines_light`).
- **The scene** (`olr-scene`): `drawn_beams()`, the beams the last build
  drew in a lit match (from the worm as drawn to the end), and
  `beam_hits()`, the ticks' `TickBeamHit`s with a per-tick index (a key a
  re-simulated tick gives again). `Scene::shots` records beam weapons'
  shots too, so a beam weapon's muzzle light (its look's, else the
  engine's) shows: the Reloaded Laser's red one was never drawn before.
- **The lights** (`olr-present`'s `look.rs`, last of the frame's
  emitters, after the skins': past `MAX_EMITTERS` the beams' discs drop,
  never another light): discs every `BEAM_DISC_SPACING` 8 world pixels
  along each drawn beam, both ends included, at most `MAX_BEAM_DISCS` 64
  (spaced wider to fit: 8 beams take at most 520 of the 1024), radius
  `BEAM_DISC_RADIUS` 1.5, the engine's `beam_light`: the beam's colour at
  21 % reaching 40 px; and, when it stopped, one at its end, radius
  `BEAM_TIP_RADIUS` 3, `beam_tip_light`: the beam's colour (linear) × 2.8
  + 0.2 a channel, normalised into a colour and an intensity (the red
  Laser's linear (3.0, 0.2, 0.2), a little whiter than the beam),
  reaching 90 px. A weapon's own lights are read by reference: no
  allocation per frame.
- **The heat:** each `BeamHit` queues the weapon's beam heat, else the
  engine's (`BEAM_HEAT`: 60 degrees of stone at the centre over 5 px),
  divided by `parts` (a wide beam heats its face no hotter than a 1-wide
  beam's tip), as a heating projectile's tick does (issue #42's deposit),
  once through re-simulation (`BeamKey`, its tick and index, through its
  own `Once`), merged in tick order with the explosions and heating
  projectiles (a tick's explosions, then its projectiles, then its
  beams). Per sub-step, not per frame, so it never depends on the frame
  rate; the carve lands first in the frame (`update_terrain`, then the
  deposits: issue #67's call order), so the tunnel's remaining edges take
  it. A beam on a worm or in the open heats nothing.
- **Format** (`olr-format`'s `look.rs`): a Beam weapon's
  `WeaponLookSource::beam` (`BeamLookSource`: `light`, `tip_light`, each
  a `LightSource` with a reach `Pixels` 1-`MAX_BEAM_REACH` 256, never
  `Unlimited`; `heat`, `BeamHeatSource { amount 0-100, radius 1-16 }`);
  each left out keeps the engine's, an intensity or amount of 0 turns it
  off; `beam` on a weapon that is not a Beam is refused ("only a `Beam`
  weapon has a beam look"). The shipped mods do not set it.
- **Mod editor:** a Beam weapon's Look group has Beam light, Beam tip
  light and Beam heat switches beside its muzzle light: off, the
  engine's; switched on, the engine's values in the beam's colour.
- **Tests:** `olr-sim`'s `a_drawn_beam_says_what_stopped_it` (dirt, a
  worm, the level's edge, 1 and 3 wide); `olr-look`'s
  `the_engines_beam_lights_are_the_picked_frames`,
  `beam_heat_is_the_weapons_else_the_engines`; `olr-present`'s
  `a_beam_heats_where_it_stops_each_sub_step_once` (with and without each
  tick told twice), `a_drawn_beam_lights_along_it_and_at_its_tip`,
  `a_weapons_own_beam_look_replaces_the_engines` (the muzzle light
  included), `trail_explosions_keep_the_engines_light`,
  `beam_discs_space_out_to_fit_the_bound`; `olr-format`'s
  `a_beam_look_round_trips_and_compiles`, `beam_look_limits`;
  `olr-mod-editor`'s `the_look_group_edits_beam`.
- **Goldens:** new `beam-fresh.png` and `beam-cooled.png`
  (`render-match` of two chaos bots with `fixtures/look/mods/look-test-laser-mod`,
  Classic's Laser emptying in five bursts and reloading 12.5 s, on the
  heat test level at ticks 200 and 300: beams lighting the cave and the
  dug edges hot, then cooling). `lit-hashes.ron` and the other goldens
  are unchanged (their matches fire no beam).

## Background glass

No OLX counterpart. Spec
`docs/superpowers/specs/2026-10-25-background-glass-design.md`, plan
`docs/superpowers/plans/2026-10-25-background-glass.md`; chosen by the
maintainer from spike renders (clear glass; frost as an option; tint as
a filter, not a hue). Presentation only: the simulation, the oracle and
every match golden are unchanged; levels without glass draw byte for
byte as before (the render goldens and `lit-hashes.ron` held without a
bless). As built:

- **Glass** is a see-through level's back pixel of alpha 1 to 254, from
  a Background image or drawn element's `glass: (opacity, tint, frost)`
  (not distant; in assets since issue #71). `ColorMap::new_see_through` keeps that
  alpha; carving reveals it; stains land only on alpha 255.
- **Planning calls:** tint and frost are baked into `look/glass.png`
  (RGBA at the level's art size: the filter `mix(255, colour, tint)`,
  the frost in tenths of a world pixel), written only when some glass
  tints or frosts; both show only in lit frames (the unlit renderer and
  thumbnails draw the plain blend); a level without distant layers
  draws glass opaque in its own colour, and the editor says so with a
  hint rather than a check warning.
- **Light:** glass leaves the shade to the distant layer behind it, so
  it is lit as that layer (a tenth of the light beyond the ambient at
  depth 90), shows no glow of its own (its glow casts: the window's
  moonlight) and still glows hot. A distant layer's own glow is baked
  only where the back is alpha 0: it never passes through glass.
- **Filter and frost** are applied in `fs_distant_lit`, at the art pixel
  under the fragment. The world pass's per-view globals carry the view's
  world frame for it (the look's globals hold the first view's only).
  Frost offsets the layer texel by the world pixel's hash times the
  frost, rounded to whole layer texels: fixed to the glass, moving only
  with the parallax. The renderer uploads the glass map whole only when
  `ColorMap::glass_generation` changes.
- **`Filtered(colour)`** glow groups cast the colour × the pixel's art
  scaled to a full brightest channel (`olr_look::engine::filtered`, the
  renderer's emission and the thumbnail alike): a stained pane lights
  the room in its own hue.
- **Editor:** a Glass switch and Opacity, Tint and Frost sliders under
  "What is it?"; the group colour switch Own / Tint / Filtered.
- **Liero Factory Reloaded:** its windows are glass at 35 % over the
  sky, a broken pane in the hall window, a moon (`content/reloaded/README.md`).
- **Cost:** a see-through level binds one more texture; the distant pass
  reads it per fragment. Unmeasured.

## Directional glow

No OLX counterpart. Spec
`docs/superpowers/specs/2026-10-25-directional-glow-design.md`, the look
picked by the maintainer from spike renders ("PCs 0.12, soft shadows,
softest edge"). Presentation only; levels without directional glow and
haze draw byte for byte as before (the render goldens and
`lit-hashes.ron` held without a bless).

The spec's decisions, in brief:

- **Analytic beams.** Light-blocking rock, a cone on the light pass alone
  and a haze that scatters the glow light all looked like mist and were
  dropped.
- **A beam is not an object.** A glowing element takes an optional
  direction and spread (`ElementGlow.beam`); each separate patch of its
  glowing pixels becomes one beam, as wide as the patch at its front face,
  with the group's colour, strength and reach. Nothing is placed by hand.
- **No light behind the source:** the same cone limits the directional
  pixels' light on the light pass.
- **Haze** (the look's strength plus an optional painted haze map) makes a
  cone with nothing in it show as light in the air.
- **Not directional:** distant layers' glow, and play lights (explosions,
  muzzle flashes, mod lights). The simulation is untouched.
- **Editor:** direction and spread under the glow properties, a haze
  strength in the look, a Haze brush (`level haze paint` in the CLI), and
  the canvas draws the beams as the game does, re-deriving them after
  every glow or art upload (final review I1, below).
- **Liero Factory Reloaded:** its side PCs and its headlights cast beams
  (`content/reloaded/README.md` has the rebuild).

The maintainer's rulings on Liero Factory Reloaded's renders (2026-10-25),
shown beside the spike's picked frames:

- **The side PCs glow at 15.** Their beams are then 0.12 of their
  colour, as in the picked look. Their own screens are duller than at
  glow 100: the teal one is half as bright, and its faint light on the
  walls is weaker. The maintainer accepted that. The `pcs` group is
  already at 1000, its top intensity.
- **Haze is rescaled: 50 is a beam's own light in the air, 100 twice
  it.** The renderer's air term is `beam × haze / 50 × haze map`. The
  factory's haze is 50, and its frames are as rendered at the old 100
  before the rescale (compared byte for byte then; the judged frames
  were later lost to a reboot). Before the rescale, the old 100 (the
  top) still gave only 95 % of the picked truck beam.
- **Office beams need the haze.** The surface term alone (beam light ×
  art) is faint on the offices' dark teal walls: 7 to 14 levels below
  the picked look. With the haze, the beams match within about 1 level.
- **A new haze map starts full (ruling T6a).** `level haze paint` creates
  the map at 255 (the look's haze) everywhere, so painting one area
  never takes the haze from the rest. Before, it started at 0, and
  painting the offices alone removed the truck's beam over the sky.

The renderer, as built (`olr-render`'s `renderer/glow_beams.rs`):

- **Two parts:** the glow light of a directional patch is weighted by
  its cone in the cascades (smoothstep from `cos(spread + 0.26)` to
  `cos(spread)`: no light behind it), and each patch draws a crisp,
  soft-shadowed beam in the composite (the spike's `beams_at`: 8 shadow
  points, penumbra `0.5 + 0.03 along`, falloff cubed).
- **Shadows, past the spike:** each shadow point is an exact grid walk
  (Amanatides-Woo) over every pixel the way crosses, so a one-pixel wall,
  diagonal or touching the face, stops the beam (the spike's 1-px steps
  let some through and skipped the last pixels before the source). A
  rock patch's walk ends at its face, decided per pixel entered (its
  farthest corner behind the face plane), so float drift over a long walk
  never stops it early and leaves no stripes 300 px out; a backdrop
  patch's goes into its own pixel, so dirt over it hides it until dug. A
  rock or dirt pixel takes its beam once, walking from its open neighbour
  facing the source (no stripes across a slanted face). A background
  patch is one beam (ruling T5a): the bake writes the composite's beam
  texels on rock only, Background and Dirt get `(0, 0)` there, so the
  backdrop's beam lives in the back map alone and `check` counts it once;
  the renderer still reads the composite's on rock only (older files).
- **Strength:** `BEAM_SCALE` 0.08 × the group's cast × the patch's colour
  × its mean glow; no setting of its own. A `Flicker` group's beam
  flickers with its patch's first pixel's region, as its glow light does.
- **Surface and air (judged on the factory's renders, kept):** the beam is
  added to a pixel's light before the cap and `light × art` wherever it
  falls on a pixel that does not glow and is not a distant layer's (rock
  faces, the back wall, sprites); `beam × haze / 50 × haze map` is added after
  `light × art` in the open air and over the sky.
- **Planning calls, in the formats and tools:**
  - **Direction in 2 degree steps (P3):** the beam map stores a direction
    in 2 degree steps; the element keeps whole degrees.
  - **The haze map lives in `look/haze.png` only (P2):** edited in place,
    not baked and not copied into `editor/` as the spec had it (the map
    has no element source). A new map starts at 255, the look's haze
    everywhere (T6a), and `level resize` fills its new area with 255 too
    (T6c).
  - **`level haze paint` (T4a)** takes `level paint`'s shape arguments
    (rect, circle, mask), not glow's `--colour-at`: haze has no element
    art to pick by colour.
  - **A background patch is one beam (T5a):** see Shadows above.
  - **Short layer ids (T6b):** the factory's layers are `ctrl-screen`,
    `pcs-right`, `pcs-left` and `headlights`, for the 80-character
    shipped path rule; the names are unchanged.
- **Planning calls, in the renderer:** the beams go in a uniform (64 × 48 bytes), not a
  storage buffer, so no device without fragment storage buffers is
  refused; the beam map the cascades read is composed from the live
  material (the backdrop's texel where a pixel is background), so digging
  dirt reveals a backdrop patch's cone; a beam whose group's reach is
  unlimited (which `check` refuses) is not drawn, and a reach is drawn to
  512 at most (the shadow walk is bounded by four times it); the beams are derived
  again, and the maps uploaded, when the editor sets new maps or uploads
  glow or art over a level with beams (`ColorMap::beams_generation`).
- **Cost:** per pixel and beam whose wedge holds the pixel, 8 shadow
  marches of up to the distance to the source; a few arithmetic rejects
  outside the wedges; no pass of its own, and a level without beams reads
  only the count. Bounded: a walk takes at most 2048 steps, at most 64
  beams, and the beams are evaluated per world-target texel and again per
  split-screen view. A wide spread's reach is cut to fit the walk bound
  (`walk_reach`), so it fades out rather than draws dark on its far side.
- **Deferred measurement:** unmeasured (the maintainer deferred it,
  2026-10-25: the PC was in use). `docs/deferred-verification.md` lists
  the `cargo xtask profile` of the factory's beams against the last base
  and a playtest of the beams in motion; the maintainer judged still
  frames only.
- **The final review's fixes (2026-10-26):**
  - **I1, stale beams in the editor:** a directional element's glow
    strength (the beam's only strength control), its group, a mask stroke
    that zeroes no pixel and art under an own-colour group leave the beam
    map as it is, so the canvas kept the old beams. `replace_glow_rect`
    and `replace_rect` now bump `beams_generation` on a level with beams
    (none without, so such levels draw as before).
  - **I2, thin patches on large levels:** above 2^20 px a glow light cell
    is 2 or more world pixels, and the cascade's cone read one texel a
    cell, so a one-pixel directional column at an even x cast all round.
    `beam_cone` now reads the cell's texels (at most 4 × 4) and takes the
    widest cone of its directional ones: light behind a source stays out,
    at the cost of coning undirected glow that shares its cell. A cell of
    one pixel reads its pixel as before.
  - **I3, no live warning past 64 beams:** the editor's diagnostics show
    the Error "N glow beams, above 64" (`olr_format::levels::glow_beam_count`
    of the bake).
  - **M1:** a plain level's glow beam maps are kept with its glow
    (`Kept`), not dropped by the first bake.
  - **M2:** a beam's reach is cut at pack time (`walk_reach`: the walk to
    any source point from any pixel it lights stays within
    `BEAM_MAX_STEPS`), so a wide beam fades rather than ends in a dark cut.
  - **M3:** the editor's Direction field steps by 2 degrees (the map's
    step); the CLI and the format still take whole degrees.

## Asset-based levels: the asset format (issue #71)

No OLX counterpart. The design
`docs/superpowers/specs/2026-10-26-asset-based-levels-design.md` ("The
asset tab", calls 2, 3, 5, 10, 11, 18, 19 and 21); the format's page is
`docs/formats/asset.md`. Nothing plays yet (#85-#94); this is the format,
its checks and the decoded model (`olr_edit::asset`).

- **A superset of the old format.** An asset folder written before is a
  valid asset: one implicit state (`Normal`), nothing exposed, no sounds,
  no health, no script. Every new `asset.ron` field is left out at its
  default, so a stateless asset writes byte for byte as before
  (`a_stateless_asset_writes_as_before` reads and writes every built-in
  asset); converting today's assets (call 18), the built-in ones and each
  user's library, is reading and writing them again, with no conversion
  code and no shim. Folders and glass, refused in assets until now, are
  allowed (a piece cut from a level still drops the level's folders).
- **A state's pictures stack in the layer's own files.** There are no
  files per state: a layer owned by n states has `layers/<id>.png` (and
  its material, back art and glow mask) `1 + n` pictures tall, the
  starting state's on top, then each owner's in `states` order; surface
  maps and image masks are the start's in every state. Shipped paths stay
  within 80 characters (issue #64), and a level pack holds copies of its
  assets under a long prefix (#73), so a `states/<name>/…` tree would not
  fit. Every stacked picture counts in `MAX_COMPOSED_PIXELS`, checked
  from `asset.ron` and the headers before anything is decoded.
- **A state needs no exposure.** Every asset can be put in any of its
  states by name (#88's API), so "expose just its states" is automatic:
  `PropertySource` has no `State`.
- **A state is a complete look** (`olr_edit::asset::posed`, and #86 when
  a state is entered in a match): each property takes the state's own
  value, else the copy's value (exposed properties only), else the
  exposed default, else the layer's own setting. Entering a state resets
  whatever another state set and this one does not: a lamp's On need not
  repeat "glow 100" to undo Off's "glow 0". A script's `.set` (#88)
  changes one value until the next state change (call 11).
- **Glow strength is mask × value.** `GlowStrength` replaces
  `ElementGlow::strength` and keeps the painted mask, so each pixel glows
  at its mask × the value; 0 is dark (the glow removed).
- **An exposed default is optional** (a deliberate reading of the issue,
  whose struct shows it required while its rules need "a `GlowColour` on
  an `Own` group without a default" to be an error): none means the
  layer's own setting; an `Own` group's colour has none, so exposing it
  needs one.
- **Landing an asset** (`land`, the editor's placing, until #82;
  `place_in_pack`, `olr-cli asset place`'s, until #75): the starting state posed with its defaults, as
  ordinary elements in one folder, its own folders flattened; its states,
  values, sounds, health and script stay behind and the status line says
  so. An `Opacity` below 100 is not expressible in a level's elements and
  is not landed (0 drops the piece), and an image piece mirrored by
  `Facing` lands unmirrored (a landed image is processed again from its
  original).
- **Editing an asset** (issue #72; `olr_edit::asset`'s operations, shared
  by the asset tab (#79, #80) and `olr-cli asset`): each operation edits a
  copy and keeps it only when the folder `encode` would write checks
  (`check_asset`), so a refusal leaves the asset (and, from the CLI, the
  folder: `replace_dir`) as it was, and its message names the state,
  layer, property or sound. A new state with no `--from` copies the
  start's values and owns nothing, so it looks like the start; its sounds
  are not copied (a deliberate reading: "the start's values, owning
  nothing"). Renaming a state moves the start and `health.at_zero` with
  it; scripts are text and are not rewritten (#89 reports a name a script
  no longer finds).
- **`set_start` keeps every state's look** (settled with the maintainer
  while writing #72 up). For each layer `L` the new start `B` owns, the old
  start comes to own `L`'s base picture, every other state that followed
  the base for `L` comes to own a copy of it, then `L`'s base becomes `B`'s
  picture and `B` owns nothing; values are untouched. A state's own picture
  without a glow mask shows the base's mask, so one owning `L` without a
  mask is given the old base's (else it would take `B`'s); `B`'s own mask,
  when it has one, becomes the base's. A property test
  (`set_start_keeps_every_states_look`) holds `posed` equal for every
  state before and after, through the folder too.
- **The CLI's property syntax** (`asset state value`, `asset expose`,
  `asset info`; `olr_edit::asset::parse_property` and `property_label`):
  the asset's own `x`, `y`, `visible`, `facing`, `health`, and a layer's
  `<layer>.x`, `.y`, `.visible`, `.opacity`, `.glow`, `.glow-colour`,
  `.light`, `.reach`, `.animation`, `.beam-direction`, `.beam-spread`,
  `.glass-opacity` and `.glass-tint`, split at the **last** dot because an
  element id may hold one (`door.left.visible` is layer `door.left`).
  Values read their type from their form (`parse_value`): a whole number,
  `true`/`false`, `#rrggbb`, `steady` or an `AnimationSource` in RON
  (`Flicker(speed: 40, depth: 60)`); the property's type is checked where
  it is set ("true is not a whole number"), and `none` clears a state's
  value.

## Asset-based levels: the level format (issues #73, #74)

No OLX counterpart. The design
`docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`; the
format's page is `docs/formats/level.md` (`PlacedFile`). #73 is the format, its checks and
the bake (`olr_edit::placed`), tested on placements built in code; #74
and #75 bring the packs, the conversion, the editor and the CLI. Nothing in
a pack on disk changes in #73: the committed levels stayed compositions
until #74 (below).

- **The ground lives in `editor/` itself.** A level's own painted asset
  (key `ground`, always an `Own` asset) is an asset
  folder rooted at `editor/`: `editor/asset.ron`, `editor/layers/<id>.png`
  and so on, the paths a composition's files have today, so converted
  levels keep their paths (After Dark's longest are 80 characters, the
  limit, at today's depth). Every other asset is a folder `editor/<key>/`; a
  copy's script is `editor/scripts/<copy-id>.rhai`, the level script
  `editor/level.rhai`. Everything under `editor/` stays authoring.
- **Generated keys are short and internal.** The editor gives `a`, `b`, …
  `z`, `aa`, `ab`, … (`next_key`: the first free, never `ground` nor one
  of the ground's folder names `images`, `layers`, `masks`, `glow`,
  `surface`, `sounds`, `scripts`), in the order the assets join the level.
  Scripts use copy ids, never keys. A chosen key is
  `[a-z0-9-]`, 1 to 16 bytes, not starting with `-`, never a ground folder
  name. The path budget: under
  `content/reloaded/olr-level-liero-factory-reloaded/` (50 characters)
  a 2-character key leaves a library asset's file 20 characters
  (`layers/<id>.png` a 9-character id, `.back.png` 4, `.material.png`
  none, `glow/<id>.png` 11, `masks/<id>.png` 10, `images/<source>` and
  `sounds/<name>` 13; a mixed layer's `.material.png` leaves no room
  there, so the level that needs one takes a 1-character key or the asset
  drops the mix) and a scripted copy's id 10
  (`editor/scripts/<id>.rhai`); keys stay short for that, and `xtask`'s
  `the_shipped_content_keeps_its_paths_short` enforces the limit on
  whatever ships.
- **The gathering rule** (`olr_format::levels::gather_groups`,
  `gather_materials`, `gather_id`; the bake and `check_placed` share
  them): copies are gathered in placement order, each one's posed glow
  groups in its asset's order. A group keeps its id when nothing gathered
  has it, shares a gathered one of that id with identical settings, and
  is otherwise renamed `<id>-2`, `<id>-3`, … (the first candidate that is
  free or holds identical settings; the base is cut so the id stays
  within 64 bytes). Materials alike (all their fields but the id compared),
  the painted maps' values renumbered to the gathered order. `level.ron`'s
  look is the level's own settings with `glow_groups` and `materials`
  replaced by the gathered ones, so the ground carries the level's groups
  and materials (#74's conversion puts them there); a level without a look
  gets the default one when any copy, Live included, brings a group or a
  material (`ADDED_LIGHTING`).
- **The stretch mapping.** An asset `w` world pixels wide placed `W` wide:
  an element's box `[b, b + n)` becomes `[at + floor(b × W / w), at +
  floor((b + n) × W / w))` (floor division below 0; a box stretched to
  nothing is left out), and its data (an image's processed grid, never
  re-processed from its original; drawn art, cells, back art, glow masks,
  surface maps) is resampled nearest into it, value `v` of the new box
  taking `floor(v × n / N)` of the old (art pixels likewise at art
  resolution, which also resamples across art scales as `rescale_art`
  does). For an element filling the asset this is the design's "world
  column `u` shows asset column `floor(u × w / W)`". The issue's test
  sketch said a 3 × 1 asset at 2 × 1 "drops the middle" column; by that
  formula it shows columns 0 and 1 and drops the last, and the test
  (`a_stretched_copy_maps_columns_by_floor`) holds the formula.
- **Positions scale apart from the box, rounded half away from zero.**
  `posed` resolves `X`, `Y`, `OffsetX` and `OffsetY` in asset pixels and
  reports each piece's move (`PosedPiece::offset`, mirrored with `Facing`);
  the bake maps the box before the move by the floor rule and adds the
  move × `W / w` rounded half away from zero (`stretch_offset`), so a
  stretched crane's claw travels with it.
- **Opacity** (`PosedPiece::opacity`, issue #71's `Opacity`): a Background
  piece below 100 lays its own back pixels over the back below at that
  opacity, as glass mixes (`glass_over`, straight alpha, rounded), in any
  level, and never over a see-through pixel (a distant layer shows there
  unchanged). At 100 the bake is today's; nothing in a composition makes
  less.
- **Checks: metadata in `olr-format`, cells in `olr-edit`.**
  `check_placed` (below `olr-edit`) reads only `placed.ron`, the assets'
  `asset.ron`s and file headers: keys, ids, states, values, the Live and
  distant rules as `asset.ron` shows them, rasters (at `(0, 0)`,
  unstretched, not distant, not Live, of the level's size: a raster covers
  the level) and distant layers (unstretched), scripts, and the budgets
  (stretched pixels with stacked state pictures, gathered groups and
  materials, distant layers). Its group count gathers the assets' own
  settings; a copy's values may change a group's settings, so the bake
  gathers the posed groups and checks the count again. A mixed layer's
  dirt cell is seen only once decoded: the bake runs `live_refusal` on
  every Live copy (`a_live_copy_with_dirt_cells_is_refused_by_the_bake`).
  A distant copy's elements also refuse glass, beams and painted surface
  maps (what `check_elements` refuses of a distant element).
- **Flattening.** Every distant piece first (copy order, then piece
  order: the ground's own distant layers and distant copies), then the
  rest; element ids `<copy id>.<element id>` (copy ids hold no dot, so
  these never clash), never written. A ground alone, unstretched at
  `(0, 0)` with the level's groups and materials, bakes byte for byte as
  `compose::bake` of its composition (`a_ground_alone_bakes_as_its_composition`).
- **Live copies are left out of the bake** (issue #85: saved as their
  played form, "Live assets (issues #85-#94)"; until #85 they baked as
  static); their groups and materials stay counted for the look.
- **Bounds.** At most `MAX_LEVEL_ASSETS` (256) assets and `MAX_COPIES`
  (1024) copies, checked with every key and id before any path is made;
  each asset within `MAX_ASSET_BYTES`, the whole `editor/` within
  `MAX_PLACED_BYTES` (`olr_pack::archive::MAX_UNPACKED`, a test holds them
  equal: `olr-format` sits below `olr-pack`), counted as it is read.
  `valid_asset_id` moved into `olr_format::assets` (an origin's folder is
  one; `olr_edit::asset` re-exports it).

Issue #74 brings the packs: `olr_edit::pack` reads and writes
`editor/placed.ron` and never reads `editor/composition.ron`; the shipped
Liero Factory After Dark and Castle Strike Reloaded, and
`fixtures/edit/composed-level`, are converted (their files outside
`editor/` byte for byte as before; the goldens and lit hashes unchanged).
The plain OLX levels stay plain.

- **The ground stays a composition in memory.** `Pack::composition` is
  the ground asset's layers while the placement is the ground alone (one
  `Own` ground copied once at the origin, at the level's size and art
  scale, nothing Live or posed), so every composition operation, `level
  element …`, `asset save`/`place` and the editor work on the ground
  unchanged; save writes them into `editor/asset.ron` (its other fields
  kept), every other asset's files byte for byte unless edited, and bakes
  the placement (`olr_edit::placed::bake`, equal to the composition's bake
  for a ground alone). While the ground is alone, `level.ron`'s
  `look.glow_groups` and `look.materials` are the ground's: save writes
  the look's into `asset.ron`, so the look stays where every editor edits
  it; a hand edit of `asset.ron`'s groups shows as `level.ron` stale in
  `pack check`. `level group …` and `level material add` edit the ground
  asset's by id; one only another copy brings is refused ("it comes with
  copy <id>'s asset: edit that asset").
- **The conversion** (`olr_edit::placed::from_composition`, `olr-cli level
  convert`, the level editor on opening): the ground "Ground" of the
  level's size and art scale, the composition's elements, folders and
  files at their paths, the look's groups and materials; `placed.ron`
  lists `(key: "ground", origin: Own)` and the locked copy `ground`;
  `composition.ron` goes. The command bakes and writes nothing when a
  file outside `editor/` would change (a 0.2 level whose bake was stale).
- **An unconverted level is refused, never guessed at.** A pack with
  `editor/composition.ron` and no placement loads (`legacy_composition`)
  and plays its bake, but every command that changes or bakes it, `asset
  save`/`place` and `ensure_composed` refuse with "editor/composition.ron:
  a level from OLR 0.2: run olr-cli level convert <pack>"; `pack check`
  and `validate` warn. A `composition.ron` left beside a placement is
  not read, a warning too.
- **The editor opens a level whole** (issue #82's level tab; until
  then it refused one placing other assets): an OLR 0.2 level converts
  on opening, unsaved ("Converted from OLR 0.2: save to keep it").

Issue #75 brings the operations on copies (`olr_edit::placed`'s
`add_copy`, `duplicate_copy`, `move_copy`, `resize_copy`, `set_copy`/
`set_copies`, `set_copy_script`, `set_level_script`, `raise_copy`,
`lower_copy`, `remove_copy`), shared by the level tab (#82, #84) and
`olr-cli level copy …`/`level script`, which replace `olr-cli asset
place` (`olr_edit::asset::place_in_pack` is gone). Each edits a copy of
the placement, runs `check_placed` and keeps it only then; none bakes.

- **An asset joins a level once.** `add_copy` reuses an asset of the
  level with the same origin and the same files (#76's comparison:
  `asset.ron`, the element files, sounds and script; never the
  thumbnail, never the ground); otherwise it copies the folder in under
  a new key. `--key` (`add_copy`'s `key`) names the key of an asset new
  to the level, checked as #73's keys are (`valid_key`, not `ground`, not
  taken, stray folders in `editor/` counting as taken), and is refused
  when the asset is reused; without it the key is `next_key`'s first
  free (1 or 2 characters, the path budget above). A chosen key's length
  is the author's to keep short.
- **Copy ids number from the asset's name.** `new_copy_id`: the asset's
  `asset_slug` (`copy-<slug>` when it does not start with a letter, a
  copy id's rule; cut to 32 bytes), then `<slug>-2`, `-3`, … the first
  free among the copies and stray scripts in `editor/scripts/`. A
  duplicate (`duplicate_copy`, the level tab's Copy and Paste) numbers
  from the copy's own id without a trailing `-<n>` (`lamp-2`'s is
  `lamp-3`), as `element::copy_name` numbers names; it keeps every
  setting but its place, id and lock (a duplicate starts unlocked) and
  lands directly above. Numbering always terminates (one more candidate
  than there are ids).
- **`keep_aspect(asset_size, Size)`** (public, the level tab's handles
  compute what the command would): `Width(w)` takes height `max(1,
  round(w × h0 / w0))`, rounded half up, `Height` alike; `Width`/`Height`
  clear `free_aspect`, `Free(w, h)` sets it, `Natural` leaves the size
  none and the flag as it was; `FreeAspect(false)` snaps the height to
  the width's aspect.
- **Live is refused with the asset's reason** (`live_refusal`'s text,
  `LiveRefusal::NothingToDrive` first): the cell-level check
  `check_placed` cannot make. #85's limits will refuse here too.
- **A locked copy** refuses moves, resizes, its script, removal and every
  setting but `Locked`, as a locked element does; its order is not its
  content (`raise_copy`/`lower_copy` move it). `set_copies` applies an
  unlocking first and a locking last, so one `level copy set --unlocked
  …` edits a locked copy.
- **The order is the stack's.** `raise_copy`/`lower_copy` move a copy one
  place, whatever the copy it passes; a distant copy's pieces bake below
  the rest wherever it stands, so the order matters among distant and
  among near copies only.
- **Removing a copy's last copy removes its asset** (an `Own` one too:
  removing the ground's only copy, once unlocked, removes the ground);
  `Removed` gives the copy, its place, its script and the asset back for
  the editor's undo. A placement emptied of copies is a level of no
  copies (it bakes to background).
- **The ground alone again** is the editors' composition again: when an
  operation leaves the ground placed alone, the pack's composition is the
  ground's and the look's groups and materials are set to the ground's
  (what the next bake gathers), so a removed copy's groups do not stay
  in the ground.

## Asset library links, export and import (issue #76)

No OLX counterpart. The design
`docs/superpowers/specs/2026-10-26-asset-based-levels-design.md` ("The
library, export and import", calls 7, 9, 17, 18); the code is
`olr_edit::library`, `olr_pack::archive`'s level export and `olr-cli
level library|export|import`. The level tab's notice and buttons are
#84's.

- **Compared by bytes, `thumb.png` left out.** A linked copy is `Same`
  as its library asset when `asset.ron` (as read: the parsed file, which
  both sides write canonically through `write_asset_files`), the element
  files, the sounds and the script are byte for byte equal
  (`placed::same_asset`). The thumbnail changes with rendering and
  re-encoding, so it is never compared. A library folder that does not
  read or check is `Missing`, as an absent one is.
- **Pure cores, path wrappers.** `compare`, `update` and `relink` take
  the placement and a `Libraries` value (each linked origin's library
  version, or none), so the level editor's pure state can carry it
  through #83's host round trip; `read_libraries`, `compare_from`,
  `update_from` and `relink_from` read the libraries from disk, and the
  CLI uses them only. `restore`, `adopt`, `export` and `import` touch
  disk or nothing and need no core.
- **`Builtin` never reads the user's library** (call 7: built-in levels
  never follow it): a `Builtin` origin is looked up in the built-in
  library only, a `Yours` origin in the user's only. A user's edited copy
  of a built-in asset is a new user asset (call 18); a level follows it
  only when relinked (`relink`, `olr-cli level library relink`).
- **Breaks and the forced resolutions.** A changed version's breaks are
  #73's `check_placed` rules and #71's `live_refusal` applied to each
  copy with the new version: a value naming an exposed property it lacks
  or of another type, a starting state it lacks, Live on an asset that
  may no longer be, a distant copy of an asset that may no longer be
  distant (`olr_format::levels::distant_refusal`, the rule
  `check_placed` uses). A size change is no break (call 5's limits are
  relative to the asset). Without force a breaking version is refused,
  naming each break; forced, each is resolved and listed, never silently:
  the value dropped, the copy started in the asset's start, made static,
  made near. `Break::Script` is reserved for #89 (scripts are not
  checked here); forced, it is listed as left for the script. Any other
  problem of the result (`check_placed`, the bake) still refuses the
  update whole.
- **Restore and import never write over.** `restore` writes the level's
  copy verbatim (`add_source_to_library`, so it compares `Same`) into a
  new folder of the user's library, a name the user has numbered as
  `import_into_library` numbers it (the level's copy renamed alike), and
  links it `Yours`, whether it was `Yours` or `Builtin`. `import` links
  a copy identical to a built-in asset to it, else one identical to a
  user's asset (the origin's own folder first), else adds it (numbered,
  "Console 2", in a new folder) and links it; a level whose id is taken
  among the user's packs or the bundled ones gets `<id>-2`, … and a
  numbered name. Every folder is chosen, and `MAX_USER_ASSETS` checked,
  before anything is written; the level is written first, then the
  assets, and a failure midway removes what the import wrote.
- **Deviations from the issue's signatures.** `export` and `import` take
  the theme's back tile as a lookup by theme id (`BackTile`), since the
  theme is known only once the pack is read; `import` takes its places
  as one `Places { packs, user, builtin, bundled }`, `bundled` being the
  bundled packs' ids (`olr-content` is a layer above `olr-edit`, so the
  caller lists them). `Updated` also lists the keys already the
  library's (`same`).
- **Why a new magic, not `.olpk`.** The export must carry `editor/`
  (call 17: the archive is essentially the pack), which `.olpk` leaves
  out and refuses so that players download only the bake. A flag inside
  `.olpk` would let an editable archive pass where a download is
  expected, so `.olrlevel` is its own format (`OLRL1\n`): the same
  layout, `Limits::PACK` bounds and portability and case checks, with
  `editor/` files allowed, spelled so. `.olpk` is unchanged byte for
  byte, and so is the content hash.
- **Downloads stay play-only.** Online content download, replays and
  desync bundles keep `.olpk` and the baked files; a downloaded level
  plays and is not editable. Call 17's "a shared level is fully
  editable" is met by the export archive, the way levels are shared for
  editing.

## Asset tab (issues #77, #78, #79, #80)

No OLX counterpart. The design
`docs/superpowers/specs/2026-10-26-asset-based-levels-design.md` ("The
asset tab", call 18); the code is `olr-editor`'s `asset_tab.rs` and
`state.rs`, laid out in `ui/asset.rs` and `ui/previews.rs`. Part 1
(#77) is the second document on today's layout; #78 moves it to the
skin editor's layout, #79 adds the states panel (`state_ops.rs`,
`ui/states.rs`), #80 the Properties, Sounds and Script tabs
(`parts_ops.rs`, `ui/parts.rs`).

- **Two tabs inside the level editor, not a second editor.** One session
  holds the level and an asset (#81 opens the level's ground in the asset
  tab and needs both): the session bar's Level | Asset switch. Each tab
  has its own document, undo history, unsaved mark, selection, view, tool
  and options and status line (`state::Session`); the shown tab's live in
  `EditorState`'s fields and the other's waits beside them, the two
  swapped on a switch, so every operation, tool and panel works on the
  asset tab unchanged. The catalog, settings, clipboard (elements copied
  in one tab paste into the other, resampled as before), textures and
  libraries are shared. Play, the Spawn tool, the Haze brush, the
  Spawns tab, the level's theme, notes, art scale field and resize with
  an offset are the level tab's; distant layers are not offered in the
  asset tab (an asset carries none).
- **The asset's document is a level of its size.** `Document::open_asset`
  bakes the asset as a level of its size and art scale under the open
  level's theme (so the Front and Back tile paints and the craters work
  as on the level; the default theme without a level), with no spawns
  and `olr_edit::asset::preview_look` holding the asset's glow groups
  and materials; `to_asset` makes the asset again, the glow groups and
  materials no layer names left out (`olr_edit::asset::carried_look`,
  as `check_asset` asks). An asset stores no theme or lighting settings:
  tile paints are stored as pixels, and a change of the Lighting's
  ambient or grade in the asset tab previews only.
- **Saving writes the user's library only.** The user's own asset is
  saved over its folder; a built-in one (call 18) or a new one is saved
  as a new asset of the user's, a name the user has asking Replace or
  Keep both. A built-in asset opens read-only in place: its key stays
  built-in until the first save ("Built-in: saving makes your own copy"),
  then the tab edits the user's copy.
- **Crash recovery covers the level only.** `EditorState::document()` and
  `dirty()` stay the level's, whichever tab shows, so the host writes the
  level's recovery copy as before; the asset tab's unsaved work is not
  recovered after a crash. Close asks once for each tab with unsaved
  changes, the asset first.
- **Edits that would break exposure are refused (#80 edits it).**
  An asset made by #71's commands keeps its states, exposed properties,
  sounds, script and health through an open and a save. Each step the
  asset tab records is checked against them (`Document::asset_refusal`:
  the exposed entries' and states' values' layers still offering the
  property, then `olr_format::assets::check_states`, metadata only); a
  refused step is reverted and forgotten at once, so the document, the
  unsaved mark and the selection are as they were and the status line
  names the entry ("lamps is used by exposed lamp_colour: unexpose it
  first"). This applies the edit and takes it back rather than editing a
  scratch copy, which gives the same result without copying the
  document; a refused edit does clear the redo list, as any edit does.
  An asset without states and exposure (every built-in one) is never
  refused. #77 also refused moving or resizing a layer a state owns a
  picture of; #79 lifted that (below).
- **States edit pictures in place (#79).** The states panel shows one
  state at a time on the canvas: while a state other than the start is
  shown, the document's layers hold its own pictures (the start's wait
  in its place), so every tool paints it unchanged and undo, redo and
  saving swap back. Each step is bracketed by the state it was made in
  (`Edit::Show`), so undoing it shows that state again; the states
  themselves are one `Edit::States` (their own pictures shared, counted
  once). A stroke, fill or Glow-brush stroke on a drawn layer the state
  does not own makes it the state's own in the same step; a stroke that
  changes nothing does not.
- **Images and surfaces are the same in every state.** A state's own
  picture is a drawn layer's (`olr_edit::asset`'s `OwnPicture`), so in a
  state other than the start an image's stroke and Glow brush are
  refused (merge it into a layer first, or paint its glow in the start),
  and the Surface brush paints in the starting state only (an asset's
  surface maps are not per state).
- **The canvas ignores position values.** The canvas draws the shown
  state as `olr_edit::asset::posed` gives it (its pictures, visibility,
  opacity and glow values) but at the layers' own places: X, Y, the
  offsets and Facing are left out (`posed_in_place`), since tools paint
  at the layer's box. The previews show the state fully posed.
- **Layer edits carry the states along.** Moving or resizing a layer and
  resizing the asset re-lay the states' own pictures; turning a layer
  mixed or plain converts them; removing a layer forgets the states'
  pictures of it and the state values naming it (the status line lists
  them; undo restores them), while an exposed entry naming it still
  refuses. Merge down and Split are refused on a layer a state owns
  ("Alarm paints screens itself: make it follow the start first"):
  splitting or merging each state's picture would need a choice of
  which state's layers survive. The starting state and health's state
  cannot be removed.
- **The skin editor's layout, fitted at every size (#78).** The asset tab
  is laid out as the skin editor (the session bar, the tools on the left,
  the canvas in the middle, the layers, properties and previews on the
  right, the status line), its side panels a share of the window's width
  and below 900 points one at a time (Tools | Layers), at every size the
  menus fit (issue #55's rules, `tests/fits.rs`). The level tab keeps
  issue #8's layout until #83 fits it.
- **Options under the tools.** The chosen tool's options (the level
  tab's options bar) sit in the tools panel under the tools, as the skin
  editor keeps its options in its tools panel: a row of options across
  the top cannot fit a 235-point side panel's window.
- **Previews on the CPU, at most 512, on release.** The light (full
  bright) and dark (under the thumbnail's preview look) previews are
  drawn by the thumbnail's CPU light model at any size
  (`olr_format::levels::level_lit`, of which the thumbnail is the
  160 × 96 case, byte for byte), at the asset's art resolution and at
  most 512 pixels a side, not by the renderer: the editor's panels do no
  GPU work. They approximate the lit canvas (no shadows, soak, softness
  or light steps, as the thumbnail), and are made again only when an
  edit is done, never during a stroke or every frame.
- **A row shows the value posed in the selected state (#80).** The
  Properties tab lists each of `olr_edit::asset::offers` once, under its
  layer (Whole asset first), with the value the selected state poses
  (`value_in_state`: the state's own value, else the exposed default,
  else the layer's own setting), marked when the state sets it itself
  (Clear drops it). A value is edited in the selected state only;
  exposing one gives it a name (`<layer>_<property>`, unique, at most
  32 bytes) and the start's value as its default, a position's min and
  max at that value (widened by hand). Edits a state's value or an
  exposed default would contradict are refused with the state or name
  ("Alarm sets claw_x to 40: change it first").
- **The script commits on a pause (#80).** Typing in the Script tab
  changes a draft; it becomes one undoable step after about a second of
  UI time without a keystroke, or when the editor loses the keyboard,
  so undo takes back a burst of typing rather than a character.
- **Broken scripts save (#80).** The script's syntax errors are listed
  (`line:column: message`, a click puts the cursor there) but never
  refuse an edit or a save: half-written scripts are kept, and nothing
  runs a script before #88. The check is syntax only (`olr-script`'s
  `check`, Rhai's parser) until #89 checks the names a script uses.
- **Rhai is pinned (#80).** `rhai = "=1.27.0"`, default features off,
  with `no_std`, `sync`, `no_float`, `only_i64`, `no_module`,
  `no_custom_syntax`, `no_time`, `no_closure` and `internals`: #88's
  runtime's pin and features exactly, so the editor never parses a
  script the simulation would read differently. Parsing is bounded
  (expression depth 32, 16 in a function; strings 4096 bytes; scripts
  64 KiB).
- **The script editor does not wrap (#80).** Lines run on and the
  editor scrolls across, so the line-number gutter stays beside them.

## Level tab (issues #81 to #84)

No OLX counterpart. The design
`docs/superpowers/specs/2026-10-26-asset-based-levels-design.md` ("The
level tab", call 16); the code is `olr-editor`'s `ground.rs` and
`state.rs`. Part 1 (#81) moves layer painting to the asset tab: the
level tab shows the ground as one copy. Part 2 (#82, `copies.rs`,
`level_tab.rs`) makes it a placement editor of copies. Part 3 (#83,
`links.rs`, `ui/library.rs`, `ui/level_tab.rs`) puts the asset library
on the left and keeps the copies linked to it. Part 4 (#84,
`copy_ops.rs`, `notice.rs`, `ui/copy_panel.rs`, `ui/level_script.rs`)
gives copies their ids, Live, starting state, values and scripts, the
level its script, and the level its library-change notice, export and
import.

- **The level tab shows one locked copy, the ground (#81).** The right
  panel's first tab is Copies (`labels::COPIES`), listing the ground
  alone (🔒); a click on its row or Select on any pixel of the level
  selects it (`Selection::Copy`), drawn as one `Overlay::Box` without
  handles. Remove and moves are refused (`GROUND_LOCKED`). Its panel
  shows its name, its size and **Edit in level**, a label of its own
  (the Assets tab's is "Edit asset"); a double click on it does the
  same.
- **Edit in level opens a copy of the level's document (#81).**
  `Document::ground_of` clones the level's layers, folders, originals,
  glow and surface maps, look, theme, haze and `Kept` glow, without the
  spawns, into the asset tab (asking first when that tab holds unsaved
  work), titled "Ground in <level>". Its canvas draws under the level's
  look (never `preview_look`), its tile paints are the level's theme's,
  and it takes distant layers and rasters, the Distant switch and View ▾
  Preview parallax: it is the level's own ground, not a library asset
  (`asset: None`: no asset refusals). It has no states, properties,
  sounds, script or previews (a deviation until the ground is an asset
  like any, #82); its size, art scale, name and theme are the level
  tab's (`GROUND_IS_THE_LEVELS`), so Resize there grows the ground in
  one step and the asset tab follows (`sync_ground`).
- **Edits reach the level as one step, "Edit Ground" (#81).** On a
  switch to the level tab, Play (from either tab: the asset tab's
  session bar has Play while it holds the ground) or the asset tab's
  Save, the ground's changes since the last time become one level step,
  `Edit::Ground` (`GroundSnap`: items, folders, originals, masks,
  surface maps, and the look with the ground's glow groups and
  materials; the level keeps its ambient, occlusion, haze and grade),
  the level unsaved. The asset tab keeps its own stroke history. Save in
  the asset tab saves the level pack, both unsaved marks clearing; the
  level tab's Save saves the ground as last applied. Undo's hover names
  the step ("Undo Edit Ground").
- **Undoing "Edit Ground" reopens the ground (#81).** An undo or redo of
  the step while the asset tab holds the ground makes it again from the
  level, its history cleared, with the status "The ground went back to
  before your last edits", so the two tabs never disagree; a level
  change that the asset tab's copy did not see (a resize, a placed
  asset) does the same on the way back ("The ground changed in the
  level: shown as it is now"), and a look edit is just followed.
- **The level tab keeps Select, Spawn and the Haze brush (#81).** Draw,
  Pixels, the Glow and Surface brushes, Import image…, the layer
  properties, folders, merge and split, glass, masks, glow groups and
  materials are the asset tab's; the level tab refuses those actions
  with `LAYERS_IN_ASSET_TAB`, a tool with how to reach it. The level
  panel keeps its Lighting without glow groups, the size fields and
  Play.
- **A level of placed copies opens whole (#82).** Every copy is posed,
  laid and kept; the editor bakes the placement as `olr_edit::placed::
  bake` does, byte for byte, so an unchanged open and save writes the
  same files. A moved copy rebakes its old and new boxes; a change to
  the gathered look (a new group or material) the whole level.
- **Place makes a copy (#82).** The Assets tab's Place (at the view's
  centre), a press with an asset picked, or a thumbnail dropped add a
  copy of the library asset (`add_copy`, its origin Built-in or Yours)
  on top, selected, one step; the asset tab places none. 0.2's landing
  of pieces into the ground is gone.
- **Click by covered pixel (#82).** Select picks the topmost copy with a
  covered pixel there, a transparent pixel falling through to what is
  behind, else the topmost copy whose box holds the point; Ctrl+click
  toggles, Escape selects none. A drag moves the selection live, one
  step on release; the arrows nudge by 1.
- **Handles, and Shift to snap (#82).** One selected unlocked copy has
  eight handles: a corner resizes from the opposite corner, an edge from
  the opposite edge (the other side centred while the aspect is kept);
  the aspect is kept unless Keep aspect is off (`free_aspect`), the side
  changed more leading. Shift snaps to whole multiples of the asset's
  size, the nearest, never below 1× (each side its own when free). The
  resize is previewed and applied on release as one step, the status
  line saying "58 × 4, 2×". A small copy's middle stays its own, so it
  can be dragged: a handle reaches at most a quarter of a side into it.
- **The ground stays locked (#82).** Its copy never moves, resizes, goes,
  duplicates or goes above another's place under it; a level resize
  moves every other copy, locked ones too, with the level in one step,
  and the ground grows with the level.
- **Delete, copy and paste (#82).** Delete removes the selection in one
  step, an asset no copy uses going too (the status says so; undo brings
  both back). Ctrl+C and Ctrl+V paste copies (8, 8) further each paste,
  Ctrl+D duplicates; elements paste only in the asset tab and copies only
  in the level tab, each refusal saying so; Cut is refused there.
- **The Copies list and the copy panel (#82).** Every copy, top first:
  its asset's name, its id, a lock toggle and a mark: 🔗 for a library
  copy, the weak word "own" for the level's own assets (never ★, the
  starting state's mark); a row dragged onto another takes its place
  (raise and lower, one step a drop). The panel: Position X/Y,
  Width/Height, Keep aspect, Locked, and Edit in level (an own asset:
  the ground as #81, another own asset in the asset tab, its edits one
  level step on the way back) or **Edit library asset** (as the Assets
  tab's Edit asset); a double click on a copy does the same.
- **The level tab's layout (#83).** As the asset tab's (#78): the
  session bar wrapped to the window (Level | Asset, File ▾, Undo, Redo,
  the unsaved mark, View ▾, Play); on the left the tools (Select, Spawn,
  the Haze brush), the chosen tool's options under them, then the
  library: Search (by name, case aside), the thumbnails grouped
  Built-in then Yours in rows of as many as fit, and Place, Edit asset,
  Remove asset (the user's), New asset…, Import asset…; on the right
  Copies | Spawns, then the selection's panel or the level's. The
  options bar and the right panel's Assets tab are gone. Both side
  panels take a share of the width; below 900 points one shows at a
  time, switched by the session bar's Library | Copy. `the_level_tab_fits`
  replaces `the_toolbar_stays_in_the_window_and_in_place`.
- **Placing again reuses the level's copy of the asset (#83).** A drag or
  a pick then a click places the library asset; a second placing of the
  same origin reuses the level's asset (`olr_edit::placed::add_copy`'s
  `same_asset`). When the library's version differs, the new copy takes
  it (a new key) and the status offers the older copies the update
  (`offer_newer`: "Update <asset>", as below).
- **The host compares the library (#83).** The editor never reads
  files: whenever the library may have changed (`AssetsChanged`,
  `AssetSaved`) it sends `CompareLibrary { origins }`, the level's
  linked origins, and the host answers `LibraryCompared` with each
  origin's source (`olr_edit::library::read_origins`, `None` for one
  gone; a built-in origin read only from the built-in assets).
  `olr_edit::library::compare` sorts them: an unbreaking change is
  applied as one step named "Update <asset>" (the Undo button's hover;
  the status "2 copies of Lamp updated"), the copies of one origin
  merged into one asset again; a breaking change (a copy's values or
  state no longer fitting the new version) is not applied, the status saying "Lamp
  changed: 2 copies would break"; a missing asset changes nothing.
  `AssetSaved` carries the saved asset's source too.
- **Use your copy in this level (#83).** After an edited built-in asset
  is saved as the user's, the status line offers `Use your copy in this
  level`: one step relinking every copy of the built-in to the user's
  (`olr_edit::library::relink`), named "Use your <asset>".
- **The copy panel's link (#83).** A library copy shows its origin
  ("Built-in: Lamp", "Yours: Lamp") and **Unlink**, which makes its asset
  the level's own (`olr_edit::library::adopt`, one step); an own asset
  shows **Save to library**, asking its name as Save as asset does, then
  relinking the copies to the new asset of yours in one step. It is
  refused with the reason for a distant or raster layer ("… with
  distant layers stays the level's"), an unused glow group ("remove the
  unused glow group 2 first") or any other `check_asset` refusal.
- **Save to library on the ground saves a copy (#83, a deviation).** The
  ground cannot be linked (`check_placed` keeps it Own), so its Save to
  library writes its placement as a library asset and the level keeps
  its own ground; the status says so.
- **`AssetRead` still carries the source only (#83).** The design's
  decoded `Asset` beside the source is not sent: `add_copy` decodes the
  source itself, once.
- **The copy panel's Id, starting state and values (#84).** Under the
  link: **Id** (refused with `set_copy`'s reason when taken or not an
  id; a rename keeps the copy's script and selection) with the hint
  "Scripts use this id"; **Starting state**, a picker whose first entry
  is "The asset's (★ <start>)", hidden for a one-state asset; **Values**,
  one row per exposed property by its type within the exposed limits
  (a set row marked ●, **Default** clearing it), the canvas posing the
  copy. Each change is one step.
- **Live has three looks (#84, call 2).** Hidden when the asset drives
  nothing (one state, no values, sounds or script); shown off and
  disabled, the reason as hover text and a line under it, when it holds
  dirt ("it holds dirt: only rock and background can be live"), a raster
  or a distant layer, or the copy is distant; otherwise a switch, which
  while on says "Plays from a later version: left out of play for now"
  (issue #85: the ground leaves it out, its played form waits for #86
  and #87; until #85 it said "Moves in play from a later version: bakes
  as placed for now"). A copy Live already can always be switched
  off. **Distant** (with **Depth**, 1 to 99, on at 50) is offered only for
  an asset of Background images and plain drawn layers; Distant and Live
  each disable the other, with the reason.
- **Scripts open in the middle (#84).** The copy panel's **Copy script**
  (New script, or Edit script and Remove script, which asks first; "Also
  run <asset>'s script" when the asset has one, offered while Live) and
  the level panel's **Level script** open the script editor in place of
  the canvas on `editor/scripts/<id>.rhai` or `editor/level.rhai`, with
  **Back to level**; a new script is one step, and so is each commit
  (after a pause, losing the keyboard or leaving the script). Nothing
  runs them yet.
- **Saving with breaks (#84).** The level's problems list each library
  break ("crane: value claw_x: … in Crane") and each copy of a missing
  asset ("crane: Crane is not in your library") as errors, and each
  script's syntax errors ("crane's script, line 3: …", the level's and
  the assets' default scripts too) as warnings; none refuses a save, and
  a level saved with them keeps them. The status line shows the first
  problem cut to the line and "(+N more)", all of them on hover.
- **The library-change notice (#84).** Opening a level asks the library;
  when anything differs, "Your library changed since this level was
  saved" lists the changed assets that break nothing (Update, Update
  all: one step each), those that break copies (the breaks listed;
  **Update anyway** applies the library's version in one step, resolving
  each break, the resolutions said in the status line and added to the
  level's notes) and the missing ones (**Add to your library** saves the
  level's copy as yours, its name numbered on a clash, and relinks the
  copies in one step; **Make it the level's own** unlinks them). Nothing
  is taken from the library while the notice is answered on opening.
- **Later keeps the errors (#84).** Later closes the notice; its breaks
  and missing assets stay the level's errors, and a later library write
  updates only the assets whose library version changed since, so the
  notice does not come back unless asked: File ▾ **Library changes…**
  opens it again (or says nothing changed).
- **Export through the name field (#84).** File ▾ **Export level…** asks
  to save first (Save or Cancel: an export is of the saved level), then
  opens the browser in its one save purpose: a **File name** field,
  `<pack id>.olrlevel` by default (`.olrlevel` added when missing, a
  click on a file taking its name), and "Replace <name>?" for a file
  there. The host writes `olr_edit::library::export`'s archive and says
  "Exported castle.olrlevel (812 KiB, 3 assets)".
- **Import opens the level (#84).** File ▾ **Import level…** picks a
  `.olrlevel` file; the host imports it into the data folder
  (`olr_edit::library::import`: the pack in `packs/`, its new assets in
  the user's library) and the editor opens it (asking about unsaved
  changes first), the status line naming what came in ("Castle 2:
  reused yours console; added Console 2"); the game's level lists are
  read again.
- **A plain level is one locked ground (#81).** It opens split into its
  three layers as before; its `Kept` glow, beams and clear flags stay
  while no layer glows or is clear, an Edit Ground included.

## Live assets (issues #85-#94)

The design is `docs/superpowers/specs/2026-10-26-asset-based-levels-design.md`
(calls 8, 11, 13 and 19). Issue #85 gives Live copies their played form;
#86 puts them in the world, #87 draws them, #88 runs their scripts (#89
adds motion and the scripts' name checks).

- **The played form is play files (#85).** `live.ron` and `live/` sit
  beside `level.ron`, outside `editor/`, so `olr_pack::content_hash`
  covers them and downloads, replays and online setups carry and check
  them; `editor/` stays authoring (`the_live_files_are_in_the_content_hash`).
  Every path is made of indices (`live/<copy>/<k>[-rock|-glow|-groups|-beam|-glass].png`,
  `live/<k>.rhai`, `live/<k>.wav|ogg`, `MAX_LIVE_PATH` 22), so the shipped
  paths' 80 characters hold whatever the assets are named. A played form
  that does not check refuses the level (`olr_pack::read_level`, the
  game's lenient read too): live copies are gameplay, not a look; a live
  sound is checked as a mod's.
- **Parts (#85).** A layer some value or state targets is a part of its
  own (`LivePartFile::layer`, its index in the asset); a targeted mixed
  layer, or a drawn rock layer with back art, is a background part then
  a rock part driven together; untargeted shown layers flatten in runs of
  one kind, keeping the stack (a layer joins the part just below it when
  that is a run of its kind, else starts one). A targeted layer
  hidden as authored that nothing makes visible gets an unnamed
  `LayerVisible` value of default 0.
- **Values (#85).** The exposed ones first (their names), then every
  property a state sets (unnamed), then `Health` for a destructible asset
  that does not expose it (0 to its points, default the points). Every
  value is an `i64`: positions in world pixels, scaled with the copy's
  stretch (`stretch_offset`, half away from zero, as the static bake),
  a `Bool` 0 or 1, a `Colour` `0xRRGGBB` (-1 the group's own), an
  `Animation` an index into the copy's table. **A state is a complete
  look:** its values are the state's own, else the copy's, else the
  default, as a static copy's pose (`posed`); `start_values` hold the
  copy's own values of properties no state sets, applied once after the
  start's.
- **Pictures (#85).** Each part's picture is baked at the copy's placed
  size with the static bake's resampling (`placed::lay` and
  `compose::bake_elements_with_opacity` on a level of the part's box, a
  transparent back tile): a rock part's front art at its rock cells and
  its solid mask, a background part's back art. A state shares the
  start's picture unless it owns a picture of one of the part's layers.
  A part's box is the union of its layers' boxes clipped to the copy,
  never cropped to its pixels, so the Live switch's budget check counts
  exactly what the bake writes. The glow mask is baked at strength 100
  (the shown glow is mask × the part's `glow_strength`, the element's as
  authored for a targeted part, 100 for a run); the groups are the
  copy's own (two copies change colour apart).
- **Limits (#85).** 32 parts a state, 128 pictures and 96 values a copy
  (`LiveRefusal::{Outside, TooManyParts, TooManyPictures, TooManyValues}`,
  so the Live switch says why), 64 Live copies, 64 sounds and 129 scripts
  a level, 2048 files (half an archive's 4096) and 64 MiB decoded:
  `check_live_budget` at the Live switch (`placed::set_copies`) and at
  the bake.
- **The ground leaves Live copies out (#85).** `placed::bake` bakes the
  level without them and sets `level.live` from `live::bake_live`; the
  thumbnail is drawn from a bake with them as static, and a level lit
  only by Live copies still gets the default look. Since #86 a copy's
  state lives in the world and since #87 it is drawn; **gap:** its rock
  is not solid for worms and shots until #90.
- **Loading (#85).** `olr_pack::LevelPack::live` (the `LiveSource`) and
  `olr_content::LoadedLevel::live` (`LoadedLive { source, sim }`):
  `live_sim()` is the `Arc<olr_sim::LiveLevel>` a match's worlds share,
  data only (sorted lookups by copy id, state and value name; each
  picture's solid pixels as bits). `LoadedLevel::from_source` gives the
  same `LiveLevel` as the saved pack (`from_source_gives_the_saved_packs_live_level`).
  `fixtures/live/levels/live-test` (made by `fixtures/live/make-live-test.sh`
  from the assets beside it) is the test level.
- **Not yet carried (#85).** A live part has no surface materials (its
  heat waits for the world, #86/#87) and no glass map (`glass` is never
  set by the bake).
- **In the world (#86).** `MatchSetup::live` (`None` by default; the
  oracle's scenarios pass none, `olr-content`'s `match_setup` and
  `replay_assets_from` the level's `live_sim()`) gives the world its
  `LiveWorld`: each copy's state (an index) and its values (one `i64` a
  value, flat, sized at setup: a tick allocates nothing). At setup every
  copy enters its starting state, then its `start_values` apply;
  entering a state sets every value of its complete look and emits
  `EventKind::LiveState { copy, state }` (gameplay class; setup's come
  with the first tick's). A value set is clamped into its limits,
  `Health` also into `0..=points`. `World::set_live_state` and
  `set_live_value` (by index, `LiveError` for an unknown copy, state or
  value) are setup calls, recorded by replays; tests and tools may call
  them between ticks, which a recorded game never does. `World::live()`
  reads each copy's id, rectangle, state, values by name and visible
  (rock) parts at their whole-pixel places. Nothing moves and nothing
  collides with a part yet (#89-#91, #94).
- **Positions in 1/256 pixel (#86).** The world keeps `X`, `Y`, `OffsetX`
  and `OffsetY` in 1/256 world pixel (`LIVE_SUBPIXELS`; the played form's
  numbers, limits included, are whole pixels, scaled when they enter the
  world), so a motion moves in sub-pixel steps without floats; a part's
  place is rounded down to the pixel. No live state is a float.
- **The checksum's tail (#86).** `hash_state` hashes the live part (every
  copy's state, then every value, each after its count) after every
  other field, and only when the world has one: a world without live
  copies hashes exactly as before (the match goldens, the oracle traces,
  `a_world_without_live_copies_hashes_as_before`). A rollback snapshot
  copies the small buffers and the setup's `Arc`, no map-sized buffer.
- **The transfer's checks (#86).** `WorldState::live` (`Option<LiveWire>`,
  the layout's last field) is refused with `StateError::Live`, nothing
  changed, when present while the receiver has no live copies or the
  reverse, when its copy or value count is not the setup's, when a
  state is out of its copy's range or a value outside its limits. The
  receiving world's `LiveLevel` must be the same setup (the level's
  played form is in its pack hash).
- **Versions (#86).** `olr_proto::PROTOCOL_VERSION` 7 (the state's new
  field), `olr-replay`'s `FORMAT_VERSION` 8 (`SetupCall::LiveState`,
  `LiveValue`), `olr-cli`'s desync `BUNDLE_VERSION` 6; older ones are
  refused. #88 (8: script state), #89 and #90 extend `LiveWorld` and
  `LiveWire` and bump the protocol again. `olr-present`'s rollback keys class `LiveState` by
  its copy and state, so a re-simulated tick's entry plays its effects
  once (#92).
- **Drawn into the terrain (#87).** Live copies are composited into the
  colour map the renderer uploads (`olr_terrain::ColorMap`'s live layer,
  `set_live_art`, `set_live`), never drawn as sprites: the lit passes
  (the glow light and its occlusion through the solid map) and the
  unlit renderer read their pictures as they read the ground's, so a live
  lamp is exactly a static one in another state, with no second path to
  keep in step. A rock part (where its mask is solid) replaces the
  ground's colour, glows and is solid in the look's solid map (it
  occludes light); a background part shows where the ground is air (dug
  ground reveals it), its straight alpha × `Opacity` laid over as the
  bake's `glass_over`, stack order kept (a background part over a rock
  part is the topmost there and clears its solid, as the bake does). The
  ground never sees them: the colour map's ground, `material()`,
  `rebuild_area`, every edit rule, the beam map and the static beams'
  derivation stay the level's (rock parts stop worms only from #90).
  `X`, `Y`, `OffsetX`, `OffsetY`, `Visible`, `LayerVisible` place and
  show a part (`World::live`'s whole-pixel places), `Facing` mirrors the
  copy within its rectangle, `Opacity` fades a background part (rounded
  to an alpha as the bake), and a part glows mask × its `GlowStrength`
  value, else its authored strength (`olr_format::levels::glow_value`):
  the same pixels as the static bake of the copies
  (`glow_strength_is_mask_times_value`). Each tick a copy whose parts
  changed is recomposed where it was and where it is, and only those
  rectangles are uploaded; an unchanged tick uploads nothing. A ground
  edit under a copy recomposes its tiles. `olr-present` syncs after the
  tick's terrain edits (`on_tick`), in `restart` and `state_loaded`, and
  online after the restored areas and every tick (the newest world, so a
  correction draws the corrected state). **Not drawn yet:** the group
  values `GlowColour`, `LightIntensity`, `LightReach` and
  `GlowAnimation` (a copy's groups light as authored), and the beam and
  glass maps of a part (#85 never sets them).
- **Glow groups of their own (#87).** The group map is one byte: the
  level's look's groups keep `1..=n`, then copy `c`'s group `g` is `n +
  offset(c) + g`, copies in order (`offset` the groups of the copies
  before it). A copy whose groups would pass 255 draws without glow,
  with a warning in `LoadedLevel::warnings`. `LookFrame::push_live_group`
  pushes each copy's groups after the level's every frame, without
  allocating; each one's identity (its flicker's seed) is
  `identity_of("<copy id>/<group id>")`, so two copies of one asset
  flicker apart and a copy keeps its flicker whatever copies come
  before it. A level without live copies builds the frame it always did
  (`a_level_without_live_copies_builds_todays_frame`). A level lit only
  by live copies (no look, no glow maps) gets a glow channel
  (`GlowLayers::empty`) and draws lit (`LoadedLevel::has_look`).
- **Scripts run (#88).** `olr-script`'s `Scripts` (Rhai 1.27.0, pinned:
  its operation count is part of the simulation, so a Rhai bump is a
  simulation change, and `olr-proto`'s fingerprint covers the crate) is
  the world's `Director`, compiled with the level (`olr_content`; a script
  that does not compile refuses the level on every peer alike). Once a
  tick, after the mode's `on_tick` and its commands, the level script's
  instance, then each copy's in level and run order (its asset's
  default, then its own); in each, `start()` once, its due timers in the
  order set, its handlers of the tick's events, `tick()`. A frozen world
  (after the match's end) runs none. Handlers see the events emitted
  before the script phase and the last phase's state changes: a state
  change a script makes reaches `entered` next tick, never in a loop. The
  API (`crates/olr-script/CLAUDE.md`, `docs/formats/level.md`) reaches
  only states, exposed values, timers, memory, the scripts' own
  generator and reading the match (time, worms, events).
- **Determinism rules for scripts (#88).** **No floats** (`no_float`,
  `only_i64`: a float literal does not compile; overflow is a script
  error). **Their own generator** (`ScriptRng`, SplitMix64 seeded from
  the match seed, drawn in run order, in the world): `rand`/`chance`
  never draw from the world's `rand()` or `FindSpot` streams
  (`rand_calls`, `mt_calls` unchanged). **No clock, files, modules,
  console or closures** (`no_time`, `no_module`, `eval` disabled,
  printing does nothing). **No hash order:** Rhai's maps are `BTreeMap`s
  and the compiled functions are sorted before they get an index (game
  builds seed `ahash` at random). **Stateless director:** the engine
  holds no mutable state its functions write (the interner off); all
  script state (each instance's memory, timers, error count, switched
  off and started flags, the generator, the last phase's entries) is in
  `LiveWorld`, hashed (only with a director: a live level without
  scripts hashes as before), cloned for rollback and transferred
  (`olr_proto::PROTOCOL_VERSION` 8). **A budget per run:** a run is
  charged its own operation count (read from Rhai's runtime state by a
  native `__done` at the end of each call's wrapper), a failed one its
  limit (5,000); a run starts only while the tick has that much of its
  20,000 left, so no run overruns and the same runs are skipped on every
  peer (their timers fire next tick). A failed run (an error, overflow,
  a limit) applies none of its commands and keeps its memory, and emits
  `ScriptError`; 16 switch the instance off for the match.
  `World::script_peak()` reports the most one run and one tick used (not
  state).
- **Allocation in scripted ticks (#88).** Rhai allocates while it runs
  (the API object, maps, strings), so a tick that runs scripts may
  allocate, bounded by the limits (strings 256 bytes, arrays and maps 64,
  variables 64, call depth 16) and freed by the run's end; script memory
  lives in one flat buffer per world. A world without a director keeps
  `World::tick` allocation-free (`no_alloc`). The cost of a tick at the
  scripts' budget is `docs/deferred-verification.md`'s.

## Kept OLX quirks

- **Healing passes 100 health:** `CGameObject::injure`
  (`src/common/CGameObject.cpp:35-44`) subtracts negative damage (MW 1.0's
  healing weapons) with no cap, unlike a health bonus; `InjureWorm` caps a
  heal only when the worm's `shieldFactor` is 0
  (`src/client/CClient_Game.cpp:350-357`). The
  headless matches' invariants (`olr-cli::invariants`) only require health to
  be a number of at least 0.
- **A worm can be wedged in a rock ceiling:** standing half in dirt under a
  low rock ceiling, a jump makes `moveAndCheckWormCollision` take the dirt
  at the worm's own row for the floor (`y = 0` counts as below) and lift it
  5 px, into rows it no longer checks (`src/common/PhysicsLX56.cpp:216-238`);
  its centre then sits in rock, stuck until it dies (seed 303 of the bot
  grid, Classic on FossilFacility: 686 ticks). The headless invariants
  allow a centre in rock but not a worm buried in it, with rock over its
  whole collision cross (`olr_cli::invariants::buried_in_rock`).
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
  infinity and values from 2^64 on give 0 (`olr-sim::projectile::c_u64`).
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
  `olr-sim` follows this for projectiles (`attr_set`).
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
  - Skins, core sprites, mod projectile images and gibs: applied once, at
    import (`olr_format::image_io::olx_sprite_to_alpha`, the same rule).
    Before plan 13c the last three were stored raw and keyed at load.
    One affected file in scope is `skins/Wario.png`; no core sprite, mod image
    or theme image at `d1ad953f` has a (254,0,254) corner.
- **Hi-res level art.** With an "OLX hi-res data" chunk, the front 2×2 block
  becomes the back block wherever `PX_EMPTY` is set, and PNG alpha is ignored
  (`LoadLevelImageHiRes`, `MapLoader_LieroX.cpp:285-360`).
- **Entities** (`olr-fx`): a large explosion (size > 15) starts before the
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
