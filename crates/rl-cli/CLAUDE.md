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
  [--alpha A]` replays the scenario with `rl-scene` and `rl-fx` attached
  (camera on worm 0, OLX's default skin and colours, a 4:3 view of 320 ×
  240 world pixels) and draws frame N (the state after its tick; -1 is the
  start) offscreen with `rl-render`: `S` output pixels per world pixel
  (default 2), OLX's whole-pixel latest state unless `--alpha` interpolates.
  Use it to see what a change does; view the PNG.

**Layer 6.** Later milestones add `match`, `replay` and `dump-state` (spec §7.4).

Test: `cargo nextest run -p rl-cli`. `tests/corpus.rs` checks every imported
pack when `RL_CONTENT_DIR` is set. To accept a reviewed snapshot change, run
`INSTA_UPDATE=always cargo test -p rl-cli --test corpus`.
`tests/render_golden.rs` (the GPU frame goldens) runs in `cargo xtask full`
(`RL_GPU_TESTS=1`).
