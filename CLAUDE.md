# rust-liero

A Rust reimplementation of OpenLieroX (OLX) that feels identical and is
modernized for current displays.

- **Design:** `docs/superpowers/specs/2026-10-10-foundation-and-local-play-design.md`.
- **Plans:** `docs/superpowers/plans/`.
- **Porting status:** `docs/porting/STATUS.md`.

## Rules

1. **Run `cargo xtask check` before every commit.** It prints only failures.
   Without network access, use `cargo xtask check --offline`. `cargo xtask
   full` adds the bot-match grid (`bot-grid`: `rl-cli`'s `tests/grid.rs`, both
   mods × 4 levels × 3 seeds, every weapon and action covered, a 30-minute
   match bounded), the device tier (renderer GPU tests, render goldens,
   the 10-minute soak, the audio device check), the end-to-end autoplay on
   Xvfb (`e2e`) and the benchmarks and short stress run against
   `fixtures/bench/` (`bench`, `stress`; re-baseline with `cargo xtask bench
   --bless --reason "<why>"`); run it before merging bot, rendering, audio
   or performance-sensitive changes. `cargo xtask stress` is the long
   engine stress run.
2. **One crate per concern, under `crates/`.** Each crate has a `CLAUDE.md`:
   read it before editing the crate. Register every new crate in `xtask/layers.ron`.
   A crate may depend only on lower layers (`cargo xtask layers`).
   A crate's integration tests are one binary (`autotests = false`): a new
   `tests/<name>.rs` is a `mod <name>;` of `tests/integration.rs`, or its own
   `[[test]]` only when it needs a process of its own (allocation counting,
   process-global state) or xtask runs it by name (`cargo xtask layers`
   fails on a test file no binary compiles).
3. **Learn a dependency's API from its summary.** Run `cargo xtask api`, then
   read `target/api/<crate>.txt` instead of the crate's source.
4. **Cite OLX for ported behavior.** Ported code gets a doc comment naming the
   OLX source, such as `/// OLX: src/common/PhysicsLX56.cpp:272-469`, at
   commit `d1ad953f`. Faithfulness beats elegance; record deliberate deviations
   in `docs/porting/STATUS.md`.
5. **Keep the simulation pure.** Simulation-layer crates are `#![no_std]` and
   must also build for `wasm32-unknown-unknown`.
6. **Never panic on content.** Inputs from mods, levels and packs are untrusted.
7. **Keep output deterministic.** Use no `HashMap` iteration in output, and no wall clock in logic.
8. **Commits:** small, imperative subject (`rl-pack: validate projectile cycles`).

## Oracle

`fixtures/oracle/<name>.ron` are scenarios, and `<name>.jsonl.gz` are their
traces recorded from a patched OLX (`tools/olx-oracle/`).
- `cargo xtask oracle-record [name...]` re-records them; it needs Docker.
- `--check` compares fresh recordings with the committed traces.
- The simulation must reproduce the traces bit-exactly (crate `rl-oracle`).
- `rl-cli scenario [name...] [--dump <frame>]` replays scenarios in `rl-sim`
  and reports the first divergence. Use it when a change breaks the oracle.

## Render goldens

`fixtures/render/*.png` are committed render goldens, and `fixtures/ui/*.png`
the HUD's image snapshots. When a rendering or HUD change is intended,
`cargo xtask bless-render --reason "<why>"` re-generates both and logs the
reason in `fixtures/blessings.log`.

## Match goldens

`fixtures/matches/*.ron` are golden bot matches: a `rl-cli match` spec and
the world's checksum stream (`rl-cli`'s `tests/golden.rs`, in `check`). Any
change to the simulation, the modes or the bots' decisions changes them.
When the change is intended, `cargo xtask bless-regression --reason "<why>"`
re-records them and logs the reason in `fixtures/blessings.log`;
`--from target/failures/<name>.ron` adds a failing match as a new golden.

## Content

The converted packs are committed in `content/packs/` (the default
`--content`), so a fresh clone plays and tests offline. `cargo xtask
fetch-olx-content` sparse-clones OLX at the commit pinned in
`content/olx-manifest.ron` into `target/olx/<commit>/` and imports the
listed items into `content/packs/`: run it after changing the manifest or
an importer, and commit the result. `check` imports afresh into
`target/content-fresh` and fails if `content/packs` differs.
The render goldens in `fixtures/render/` are drawn from them at test time
(scenario, level and art scale from the packs); the goldens are accepted
fixtures, changed only with `cargo xtask bless-render --reason "<why>"`.
`rl-cli render <scenario> --frame N --out f.png` draws any scenario frame.
