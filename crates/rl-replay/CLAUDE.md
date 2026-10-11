# rl-replay

Match replays (spec §20 F8): the setup a match started from, every worm's
keys for every tick (bots included: bots are not replayed, their keys are),
and the world's checksum every `CHECKSUM_EVERY` (100) ticks and at the end.

- **Layer 3, `#![no_std]` + `alloc`, wasm32.** Depends on `rl-sim` and
  `rl-modes` (same-layer pairs in `xtask/layers.ron`), `rl-math`,
  `rl-terrain`, `serde`, `postcard` and `miniz_oxide` (both pinned exactly:
  the encoding is the format). Learn the match API from
  `target/api/rl_sim.txt` and `rl_modes.txt`.
- **What a replay holds** (`lib.rs`): `ReplaySetup` (the `MatchSetup`'s
  settings, start, seed, worms, pool size and bonus weapons; its mod,
  terrain, holes and green mask are named by `Packs`, ids and content
  hashes, never stored; the setup calls), `ModeSpec` (`Oracle` or
  `Deathmatch(DeathmatchSettings)`, made with `Deathmatch::new`: the setup is
  recorded after `prepare`), checksums, and the keys row by row.
- **Recording** (`record.rs`): `Recorder::new(packs, &setup, &mode)`;
  every change to the world before its first tick goes through
  `Recorder::apply(&mut world, SetupCall)` (`set_health`, `place_bonus`,
  `find_spot`, `align_physics_clock`), or the replay diverges; then
  `push(&keys, &world)` after each `World::tick` (it refuses a world out of
  step), and `finish(&world)`.
- **Replaying** (`play.rs`): `Replay::world(Assets)` rebuilds the world
  (`rl-content`'s `Content::world_for_replay` resolves the packs and refuses
  other content hashes); `Replay::play(world)` or a `Player` stepping a tick
  at a time checks each checksum and reports the first `Mismatch`.
- **Format** (`wire.rs`, `mirror.rs`): `RLRP`, `FORMAT_VERSION` (u16 LE),
  then postcard; keys by worm, deflated. Other versions are refused, never
  migrated, while no replay is published. The format has its own copies of
  `SimSettings`, `WormSetup`, `Loadout`, `Slot` and `DeathmatchSettings`,
  converted by naming every field: a new field there does not compile until
  it is recorded and `FORMAT_VERSION` is bumped.
- **Never panic on a replay:** `decode` checks every bound (`MAX_TICKS`,
  `MAX_WORMS`, `MAX_SLOTS`, `MAX_SETUP_CALLS`, ordered checksums, the keys'
  exact size, inflated with a limit); `Replay::world` refuses off-level
  spawns and bad setup calls; `World::new` checks the settings.

Test: `cargo nextest run -p rl-replay` (`tests/content.rs` writes small packs
to a temporary directory; its bot match needs `RL_CONTENT_DIR`).
