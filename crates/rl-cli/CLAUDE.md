# rl-cli

The headless command-line tool. Its commands:

- `rl-cli import --olx <share/gamedir> --out <dir> [--mod D]... [--level F]... [--theme D]... [--skins]
  [--core F]...` writes one pack per item to `<out>/<pack id>/`, plus `import-report.ron`;
  the `--core` files (shared sprites, game sounds, `colours.cfg`) make one pack, `olx-core`.
  It exits with status 1 if any item failed, and still imports the rest.
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

**The library** (`src/lib.rs`) holds the match runner for the commands and
the tests: `spec::MatchSpec` (the options, serialized in failures and
goldens; `match_options` gives what `rl_content::build_match` assembles,
the same builder as `rl-game`'s Deathmatch), `runner` (`Match` stepwise: `advance` = bots think + tick, no
allocation; `settle` = record, invariants, coverage; `step` catches a panic,
and a panic inside `World::tick` rebuilds the world from the logged keys so
`Recorder::finish_failed` saves a replay that plays into the failure;
`play`), `verify` (determinism and replay round trip), `coverage` (required
weapons and action kinds derived from the mod: `rl_sim::projectile_actions`
and `World::actions_triggered`), `failure`, `golden`.

**Layer 6.** `dump-state` comes later (spec §7.4).

Test: `cargo nextest run -p rl-cli`. `tests/corpus.rs` checks every imported
pack when `RL_CONTENT_DIR` is set. To accept a reviewed snapshot change, run
`INSTA_UPDATE=always cargo test -p rl-cli --test corpus`.
`tests/render_golden.rs` (the GPU frame goldens) runs in `cargo xtask full`
(`RL_GPU_TESTS=1`). `tests/matches.rs` (two 8-bot matches, determinism and
round trip; failure capture through the binary) and `tests/golden.rs` (the
golden checksum streams in `fixtures/matches/`; accept a change with `cargo
xtask bless-regression --reason "<why>"`) run in `check`; `tests/grid.rs`
(both mods × 4 levels × 3 seeds, chaos and Deathmatch bots, every weapon and
action kind covered; `long_bot_match_bounded`, 30 minutes) runs in `full`
(`RL_FULL_TESTS=1`).
