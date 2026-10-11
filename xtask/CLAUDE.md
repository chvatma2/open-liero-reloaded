# xtask

Workspace automation (`cargo xtask <cmd>`): `check`, `full`, `layers`, `api`,
`oracle-record`, `tidy`, `bless-render`, `bless-regression`, `fetch-olx-content`.

- `full` runs `check`, then `bot-grid` (`check::FULL_TESTS`: `rl-cli`'s
  `tests/grid.rs` with `RL_FULL_TESTS=1`), then the GPU and audio steps.
- `bless-regression --reason <why> [--from target/failures/<name>.ron]`
  re-records `fixtures/matches/*.ron` (runs `rl-cli --test golden` with
  `RL_BLESS_REGRESSION=1`); `--from` first copies a failure record in as a
  new golden. Like `bless-render`, it appends the date, the reason and the
  changed files to `fixtures/blessings.log` (`bless::log_blessing`).

## Disk use

- The dev profile (workspace `Cargo.toml`) sets `debug = "line-tables-only"`
  for workspace crates AND dependencies (`[profile.dev.package."*"]`). Every
  integration test is its own binary linking wgpu/winit/egui, so full debug
  info once grew `target/` to 45 GB. Line tables keep panic backtraces with
  file:line everywhere, including inside wgpu/winit/egui; only variable and
  type info (debugger-only) is dropped. Symbols are not stripped. Our workflow
  reads backtraces and nobody steps a debugger; if a debugger session is ever
  needed, set `debug = true` for that crate locally (do not commit it).
- `oracle-record` prunes after building a new `rl-olx-oracle:<hash>` image:
  it removes every other `rl-olx-oracle:*` image except the current tag and
  `:dev`, then runs `docker builder prune -f --keep-storage 2GB`. Pruning
  failures only warn. `images_to_prune` (unit-tested) picks the tags.
- `tidy` deletes `target/*/incremental` (and `target/<triple>/*/incremental`)
  and prunes oracle images as above. It never touches `target/content` or
  `target/olx`. `check` prints a `warn` line (not a failure) when `target/`
  exceeds 25 GB (`tidy::WARN_BYTES`); the size walk gives up after 1 s.
- WSL footnote: the Windows drive hosts the virtual disk; `wsl --shutdown`
  then `wsl --manage <distro> --set-sparse true` lets it shrink.
