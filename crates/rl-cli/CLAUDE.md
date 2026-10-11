# rl-cli

The headless command-line tool. Its commands:

- `rl-cli import --olx <share/gamedir> --out <dir> [--mod D]... [--level F]... [--theme D]... [--skins]
  [--core F]... [--extra-skins DIR --extra-skins-source S --extra-skins-revision R]` writes one pack per item to `<out>/<pack id>/`, plus `import-report.ron`;
  the `--core` files (shared sprites, game sounds, `colours.cfg`) make one pack, `olx-core`.
  It exits with status 1 if any item failed, and still imports the rest.
  The pipeline is `rl_content::import` (shared with the game's Import
  screen, plan 7 ruling Q3), with `prune`: `<out>` is this import's.
- `rl-cli validate <pack-or-parent-dir>...` prints diagnostics and exits with
  status 1 on any error.
- `rl-cli scenario [name...] [--dump F] [--all | --fields a+b]` replays oracle
  scenarios in `rl-sim` and prints the first divergence of each.
- `rl-cli render <scenario> --frame N --out f.png [--scale S] [--filter F]
  [--alpha A]` replays the scenario with an `rl-present` presentation (`rl-scene` and `rl-fx`)
  (camera on worm 0, OLX's default skin and colours, a 4:3 view of 320 ×
  240 world pixels) and draws frame N (the state after its tick; -1 is the
  start) offscreen with `rl-render`: `S` output pixels per world pixel
  (default 2), OLX's whole-pixel latest state unless `--alpha` interpolates.
  Use it to see what a change does; view the PNG.
- `rl-cli match --mod <id> --level <id> [--mode deathmatch] --bots N
  [--difficulty easy|medium|hard|xtreme|mixed] [--personality dm|chaos]
  --seed S --ticks T [--bonus-frequency F] [--kill-limit K] [--time-limit M]
  [--record f] [--coverage] [--no-verify] [--failures DIR] [--p99-budget-ms B]`
  plays a headless Deathmatch of bots (spec §7.1): invariants every tick
  (`invariants.rs`; a worm's centre may sit in rock where OLX's collision
  wedges it, but never with rock all over its collision cross:
  `buried_in_rock`), the end, p50/p99/max tick time (bots + sim), the bots'
  work units; then plays it again (determinism) and replays its recording
  from the content (round trip). `--coverage` prints weapons fired, actions
  triggered, events, bonuses, rope attachments and kills by the killer's
  selected weapon. A failure (a panic in a bot or the tick, an invariant, a
  budget, a divergence) writes `<stem>.rlrp` and `<stem>.ron` (the spec, so
  the seed, and the tick) to `target/failures/` (spec §7.3) and exits 1.
- `rl-cli replay <file> [--until N] [--dump]` plays a replay on its content,
  checking every checksum; `--dump` prints the world (before tick N) as JSON.
- `rl-cli stress [--mod M] [--level L] [--worms 32] [--pool 3000] [--seconds
  180] [--seed S] [--render-every 100] [--render-width W --render-height H]
  [--no-verify] [--out report.ron]`: the engine stress test (plan 7 task 8,
  `stress.rs`): `stress::spec` (32 chaos bots, mixed difficulties,
  `LoadingTime` 0, bonuses every `BONUS_FREQUENCY` s, no limits, the pool
  at `--pool`) played by the runner (invariants every tick), its
  presentation's entities stepped every tick and a frame rendered
  offscreen (lavapipe/llvmpipe) every `--render-every` ticks; asserts
  bounded buffers (pool, `rl_fx::MAX_ENTITIES`, `TickEvents::CAPACITY`:
  fixed capacities today, so cheap asserts against a future change) every
  tick and memory under `MEMORY_CEILING_MIB` (`VmHWM`, read every 1000
  ticks; an absolute stop, `xtask bench` gates the peak against its
  baseline), then `verify` (determinism and the replay). `Report`
  (RON with `--out`): tick, sim and think µs (p50/p99/max; `Stats::think_ns`
  splits the bots' share), units, pool peak/mean/ticks at the cap, entity
  peak/ticks at the cap, events a tick, drops, the frame ms, peak MiB. A
  failed match writes its replay to `target/failures/`. `cargo xtask
  stress` runs both mods × pool 3000/20 000 on Castle Strike.

**The library** (`src/lib.rs`) holds the match runner for the commands and
the tests: `spec::MatchSpec` (the options, serialized in failures and
goldens; `match_options` gives what `rl_content::build_match` assembles,
the same builder as `rl-game`'s Deathmatch), `runner` (`Match` stepwise: `advance` = bots think + tick, no
allocation; `settle` = record, invariants, coverage; `step` catches a panic,
and a panic inside `World::tick` rebuilds the world from the logged keys so
`Recorder::finish_failed` saves a replay that plays into the failure;
`play`), `verify` (determinism and replay round trip), `stress` (the
stress test's runner and `Report`), `coverage` (required
weapons and action kinds derived from the mod: `rl_sim::projectile_actions`
and `World::actions_triggered`), `failure`, `golden`.

**Layer 6.** `dump-state` comes later (spec §7.4).

Test: `cargo nextest run -p rl-cli`. `tests/corpus.rs` checks every imported
pack when `RL_CONTENT_DIR` is set. To accept a reviewed snapshot change, run
`INSTA_UPDATE=always cargo test -p rl-cli --test corpus`.
`MatchSpec`'s `--loading-time` and `--pool-capacity` (serialized only when
given, so goldens and failure records keep their form) are the stress
test's; without them a match is OLX's (`LoadingTime` 100, pool 3000).
`tests/stress.rs` (check) plays a 4 s stress run on a 200-slot pool.
`benches/game.rs` (divan, `cargo xtask bench`): 8 bots' think + tick and a
draw-list build on a warm Classic match (needs `RL_CONTENT_DIR`).
`tests/render_golden.rs` (the GPU frame goldens) runs in `cargo xtask full`
(`RL_GPU_TESTS=1`). `tests/matches.rs` (two 8-bot matches, determinism and
round trip; failure capture through the binary) and `tests/golden.rs` (the
golden checksum streams in `fixtures/matches/`; accept a change with `cargo
xtask bless-regression --reason "<why>"`) run in `check`; `tests/grid.rs`
(both mods × 4 levels × 3 seeds, chaos and Deathmatch bots, every weapon and
action kind covered; `long_bot_match_bounded`, 30 minutes) runs in `full`
(`RL_FULL_TESTS=1`).
