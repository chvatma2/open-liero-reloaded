# olr-replay

Match replays (spec §20 F8): the setup a match started from, every worm's
keys for every tick (bots included: bots are not replayed, their keys are),
and the world's checksum before the first tick, every `CHECKSUM_EVERY` (100)
ticks and at the end.

- **Layer 3, `#![no_std]` + `alloc`, wasm32.** Depends on `olr-data` (`text`: names), `olr-sim` and
  `olr-modes` (same-layer pairs in `xtask/layers.ron`), `olr-math`,
  `olr-terrain`, `serde`, `postcard` and `miniz_oxide` (both pinned exactly:
  the encoding is the format). Learn the match API from
  `target/api/olr_sim.txt` and `olr_modes.txt`.
- **What a replay holds** (`lib.rs`): `ReplaySetup` (the `MatchSetup`'s
  settings, start, seed, worms, pool size, bonus weapons and the
  `viewers`, the local slots of the players whose game made it in local
  order (issue #29: up to `MAX_VIEWERS` (4, split screen's views), so a
  split-screen replay plays back with every view; `viewer()` the first,
  where a watcher starts), `Recorder::set_viewers` (a slot that is no worm
  or comes twice dropped, the first 4 kept), none for bots alone or a
  spectator, decode refusing more than 4, a slot that is no worm or one
  twice;
  `looks`, one `WormLook` (name, colour, skin pack id and content hash) per
  worm, `looks.rs`: presentation only, never in `MatchSetup`, the world or
  its checksums; `Recorder::new(.., looks)` and `set_looks` fit them to the
  worms (extra dropped, missing `WormLook::numbered`) and sanitise them;
  every recorder passes the looks it shows; its mod,
  terrain, holes, green mask, live copies (`MatchSetup::live`,
  `Assets::live`, issue #86) and their scripts (`MatchSetup::director`,
  `Assets::director`, issue #88: compiled again from the same played
  form) are named by `Packs`, ids and content
  hashes, never stored; the setup calls), `ModeSpec` (`Oracle` or
  `Deathmatch(DeathmatchSettings)`; `ModeSpec::game_mode(&settings)` makes
  it with `Deathmatch::resume`, which refuses settings that disagree with
  its rules: the setup is recorded after `prepare`), checksums, and the
  keys row by row.
- **Recording** (`record.rs`): `Recorder::new(packs, &setup, &mode)`;
  every change to the world before its first tick goes through
  `Recorder::apply(&mut world, SetupCall)` (`set_health`, `place_bonus`,
  `find_spot`, `align_physics_clock`, issue #86's `set_live_state` and
  `set_live_value` as `SetupCall::LiveState`/`LiveValue`, refused as
  `ReplayError::Live`), or the replay diverges; then
  `start(&world)` (tick 0's checksum: a different setup or content is
  reported at tick 0), `push(&keys, &world)` after each `World::tick`, and
  `finish(&world)`. A worm that leaves goes through
  `Recorder::withdraw(&mut world, worm)` (`World::withdraw`, recorded as
  `(next tick, worm)`; `NotStarted` before `start`, `NoWorm` for a worm the
  world lacks or has withdrawn): it ends in `Replay::withdrawals`, which
  `finish_failed` keeps. A withdrawal after the last pushed tick has
  `t == ticks` and is valid: `finish` then takes the final checksum after it
  (replacing a periodic one at `ticks`). `push` and `finish` refuse a world whose server frame
  is not `start`'s plus the ticks pushed (a tick played unrecorded).
  `finish_failed(&world, &failing_keys)` saves a match whose next tick
  failed (spec §7.3): that tick's keys with no checksum after them, so the
  replay plays into the failure (`olr-cli match` rebuilds the world by
  replaying when a panic in `World::tick` left it broken). A trailing
  withdrawal then plays before the failing tick, and the checksum at it is
  the one before it, or none (final review A-M1,
  `a_failing_tick_after_a_trailing_withdrawal_replays`).
- **Replaying** (`play.rs`): `Replay::world(Assets)` rebuilds the world
  (`olr-content`'s `Content::world_for_replay` resolves the packs and refuses
  other content hashes); `Replay::play(world)` or a `Player` stepping a tick
  at a time checks each checksum and reports the first `Mismatch`; a
  `Cursor` is a `Player` without the borrow (`olr-game` owns the replay next
  to it).
  `olr-content` returns `Mismatch` and `ReplayError` inside `anyhow::Error`
  typed: `downcast_ref::<Mismatch>()` gives the tick.
- **Withdrawals in play:** `Cursor::step` withdraws every `(t, worm)` with
  `t` the tick about to be played, in order, before `World::tick`; those
  with `t == ticks` after the last tick, before its checksum is checked.
- **Format** (`wire.rs`, `mirror.rs`): `OLRP`, `FORMAT_VERSION` (u16 LE; 8 since the
  live setup calls (issue #86, `CallW::LiveState`/`LiveValue`), 7 the
  Weapon 1-5 keys (issue #48), 6 the local slots, 5 the 16-bit keys, 4 the looks, 3 withdrawals),
  then postcard; keys by worm, deflated (every key's low byte, then every
  high byte, so the mostly zero high plane costs next to nothing). Other versions are refused, never
  migrated, while no replay is published. The format has its own copies of
  `SimSettings`, `WormSetup`, `Loadout`, `Slot` and `DeathmatchSettings`,
  converted by naming every field: a new field there does not compile until
  it is recorded and `FORMAT_VERSION` is bumped.
- **Looks on decode:** the count must equal the worms' (`Invalid`); a name
  goes through `sanitise_name` (`olr_data::text::sanitise_name`, the rule
  olr-proto's `valid_name` checks: no control, hidden or bidi character,
  trimmed, `MAX_NAME_CHARS` 20, "Worm N" when empty); the skin hash is 32
  raw bytes on the wire (hex in `WormLook`, `NO_SKIN_HASH` all zeros); a skin id
  must be a pack id (`[a-z0-9-]`, at most `MAX_SKIN_ID` 64), else `Invalid`. A viewer picks skins with
  `Content::skin_matching(id, hash)`: never downloaded.
- **Never panic on a replay:** `decode` checks every bound (`MAX_TICKS`,
  `MAX_WORMS`, `MAX_SLOTS`, `MAX_SETUP_CALLS`, `MAX_VIEWERS`, ordered checksums,
  withdrawals (at most `MAX_WORMS`, `t` ascending and at most `ticks`, worm
  below `MAX_WORMS`, none twice), the keys' exact size, inflated with a limit, settings and mode numbers in OLX's
  ranges: `SimSettings::validate`, `ModeSpec::validate`); `Replay::world`
  checks the mode again and refuses off-level spawns and bad setup calls;
  `World::new` checks the settings. `Replay::check_header(bytes)` checks
  only the first `HEADER_BYTES` (magic and format version), for a menu's
  list of files; `decode` starts with it.

Test: `cargo nextest run -p olr-replay` (`tests/content.rs` writes small packs
to a temporary directory; its bot match and issue #88's
`a_scripted_level_replays_from_content` (the live fixture, its gate given a
test script) need `OLR_CONTENT_DIR`).
