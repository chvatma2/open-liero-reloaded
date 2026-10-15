# rl-replay

Match replays (spec §20 F8): the setup a match started from, every worm's
keys for every tick (bots included: bots are not replayed, their keys are),
and the world's checksum before the first tick, every `CHECKSUM_EVERY` (100)
ticks and at the end.

- **Layer 3, `#![no_std]` + `alloc`, wasm32.** Depends on `rl-sim` and
  `rl-modes` (same-layer pairs in `xtask/layers.ron`), `rl-math`,
  `rl-terrain`, `serde`, `postcard` and `miniz_oxide` (both pinned exactly:
  the encoding is the format). Learn the match API from
  `target/api/rl_sim.txt` and `rl_modes.txt`.
- **What a replay holds** (`lib.rs`): `ReplaySetup` (the `MatchSetup`'s
  settings, start, seed, worms, pool size, bonus weapons and the `viewer`,
  the slot of the player whose game made it, `Recorder::set_viewer`, none
  for bots alone or a spectator, a slot that is no worm refused by decode; its mod,
  terrain, holes and green mask are named by `Packs`, ids and content
  hashes, never stored; the setup calls), `ModeSpec` (`Oracle` or
  `Deathmatch(DeathmatchSettings)`; `ModeSpec::game_mode(&settings)` makes
  it with `Deathmatch::resume`, which refuses settings that disagree with
  its rules: the setup is recorded after `prepare`), checksums, and the
  keys row by row.
- **Recording** (`record.rs`): `Recorder::new(packs, &setup, &mode)`;
  every change to the world before its first tick goes through
  `Recorder::apply(&mut world, SetupCall)` (`set_health`, `place_bonus`,
  `find_spot`, `align_physics_clock`), or the replay diverges; then
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
  replay plays into the failure (`rl-cli match` rebuilds the world by
  replaying when a panic in `World::tick` left it broken).
- **Replaying** (`play.rs`): `Replay::world(Assets)` rebuilds the world
  (`rl-content`'s `Content::world_for_replay` resolves the packs and refuses
  other content hashes); `Replay::play(world)` or a `Player` stepping a tick
  at a time checks each checksum and reports the first `Mismatch`; a
  `Cursor` is a `Player` without the borrow (`rl-game` owns the replay next
  to it).
  `rl-content` returns `Mismatch` and `ReplayError` inside `anyhow::Error`
  typed: `downcast_ref::<Mismatch>()` gives the tick.
- **Withdrawals in play:** `Cursor::step` withdraws every `(t, worm)` with
  `t` the tick about to be played, in order, before `World::tick`; those
  with `t == ticks` after the last tick, before its checksum is checked.
- **Format** (`wire.rs`, `mirror.rs`): `RLRP`, `FORMAT_VERSION` (u16 LE; 3 since
  withdrawals),
  then postcard; keys by worm, deflated. Other versions are refused, never
  migrated, while no replay is published. The format has its own copies of
  `SimSettings`, `WormSetup`, `Loadout`, `Slot` and `DeathmatchSettings`,
  converted by naming every field: a new field there does not compile until
  it is recorded and `FORMAT_VERSION` is bumped.
- **Never panic on a replay:** `decode` checks every bound (`MAX_TICKS`,
  `MAX_WORMS`, `MAX_SLOTS`, `MAX_SETUP_CALLS`, ordered checksums,
  withdrawals (at most `MAX_WORMS`, `t` ascending and at most `ticks`, worm
  below `MAX_WORMS`, none twice), the keys' exact size, inflated with a limit, settings and mode numbers in OLX's
  ranges: `SimSettings::validate`, `ModeSpec::validate`); `Replay::world`
  checks the mode again and refuses off-level spawns and bad setup calls;
  `World::new` checks the settings. `Replay::check_header(bytes)` checks
  only the first `HEADER_BYTES` (magic and format version), for a menu's
  list of files; `decode` starts with it.

Test: `cargo nextest run -p rl-replay` (`tests/content.rs` writes small packs
to a temporary directory; its bot match needs `RL_CONTENT_DIR`).
