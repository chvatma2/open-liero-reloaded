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

**Layer 6.** Later milestones add `match`, `replay`, `scenario`, `dump-state` and `render` (spec §7.4).

Test: `cargo nextest run -p rl-cli`. `tests/corpus.rs` checks every imported
pack when `RL_CONTENT_DIR` is set. To accept a reviewed snapshot change, run
`INSTA_UPDATE=always cargo test -p rl-cli --test corpus`.
