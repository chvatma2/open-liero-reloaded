# rust-liero

A Rust reimplementation of [OpenLieroX](https://github.com/openlierox/openlierox),
built so physics, movement and shooting feel identical, and modernized for
today's displays. It loads classic OLX mods and levels by importing them into
an open, text-based pack format.

Status: early development. Milestone 6 is done: OLX content imports, the
simulation reproduces OLX's traces bit for bit, and you can play a sandbox
or a Deathmatch against OLX's bots, record it and watch the replay (no menus
or network yet: options are command-line flags).

## Quick start

```sh
cargo xtask fetch-olx-content    # sparse-clone OLX at the pinned commit and import
                                 # its levels, mods and sounds into target/content
cargo run --release -p rl-game -- --level "Liero Factory" --mod Classic
cargo run --release -p rl-game -- --mode deathmatch --bots 3 --level "Dirt Level"
cargo xtask check                # fast gate: format, lint, layering, web build, tests
cargo xtask full                 # check, the bot-match grid, the GPU and audio tier (below)
```

Converted OLX assets are never committed; `target/content` is rebuilt from the
pinned commit in `content/olx-manifest.ron`. `cargo run -p rl-cli -- validate
target/content` checks the import.

## The sandbox

```sh
cargo run --release -p rl-game -- --level <id> --mod <id> [--skin <id>] [--targets N] [--seed N] [--bonuses]
```

`--level` and `--mod` take a pack id or an OLX name or prefix (levels: `Liero
Factory`, `Dirt Level`, `FossilFacility`, `CastleStrike`; mods: `Classic`, `MW
1.0`). Other options: `--fullscreen`, `--uncapped` (no VSync), `--no-interp`
(OLX's whole-pixel latest-state drawing), `--filter nearest|sharp`, `--content
<dir>`. Run `rl-game --help` for all of them, including the agent helpers
`--demo`, `--script`, `--shot`, `--autoquit-after` and `--stats`.

| Key | Action |
|---|---|
| Arrows | move, aim |
| Left Ctrl | shoot |
| Left Alt | jump |
| X | ninja rope |
| Left Shift + Left/Right | change weapon |
| H | health bars |
| F2 | debug overlay (FPS, frame and tick time, entities, projectiles, voices) |
| Tab (held) | scoreboard |
| F5 | restart the match (a Deathmatch: a rematch on the next seed) |
| Esc | quit |

A gamepad works too (left stick walks, right stick aims; see `crates/rl-input`). The
`--targets` option sets how many idle worms stand around to shoot at (default 3).

## Deathmatch against bots

```sh
cargo run --release -p rl-game -- --mode deathmatch --bots 3 --difficulty medium \
    --level CastleStrike --mod "MW 1.0" [--lives L] [--kill-limit K] [--time-limit M] \
    [--seed S] [--record match.rlrp]
```

You (worm 0) play OLX's Deathmatch against 1 to 31 of OLX's bots, ported
from its `CWormBot` (they press the same keys you do). The defaults are
OLX's: 15 kills or 6 minutes end the match, unlimited lives. `--difficulty`
is `easy`, `medium`, `hard` or `xtreme`; no level aims perfectly (Xtreme
keeps a small aim error, unlike OLX's). Bots get OLX's preset loadouts; you
get random weapons until the weapon menu exists (M7). The HUD shows the kill
feed and the clock; Tab shows the scores; after "Game Over" the results
appear, F5 plays a rematch on the next seed, Esc quits.

### Replays

`--record f.rlrp` saves the match's replay when you quit and on each rematch
(`f-2.rlrp`, `f-3.rlrp`, ...). A replay stores the setup and every worm's keys
(bots too), and checksums of the world, so the same build replays it exactly:

```sh
cargo run --release -p rl-game -- --replay f.rlrp     # watch it
cargo run --release -p rl-cli -- replay f.rlrp        # check it headless
```

A replay names its mod and level packs by content hash and is refused on
other content; a divergence is reported at the first checksum that differs.

### Headless bot matches

```sh
cargo run --release -p rl-cli -- match --mod olx-classic --level olx-level-dirt-level \
    --bots 8 --difficulty mixed --seed 1 --ticks 6000 [--personality chaos] \
    [--coverage] [--record f.rlrp]
```

`rl-cli match` plays bots against each other without a window, checks the
invariants every tick, plays the match again (determinism) and replays its
recording (round trip), and prints tick times. `--personality chaos` bots
fire every weapon they get; `--coverage` lists the weapons, actions and
bonuses used. A failure saves its replay and seed to `target/failures/`.

### Windows Subsystem for Linux

- **GPU:** on WSL the game picks the GL backend through Mesa's d3d12 driver by
  itself (`WGPU_BACKEND=gl GALLIUM_DRIVER=d3d12`), unless you set either
  variable. Software rendering works too: `WGPU_BACKEND=vulkan` uses lavapipe.
- **Sound** needs the ALSA PulseAudio plugin: `sudo apt install
  libasound2-plugins`, and an `~/.asoundrc` with

  ```
  pcm.!default pulse
  ctl.!default pulse
  ```

  Without a working device the game runs silently and says so once.
- **Window:** the window has a fixed size (no drag-resizing, no maximise
  button; `--fullscreen` is borderless fullscreen). WSLg's compositor
  (Weston) crashes when a window is resized quickly, which ends every WSLg
  app; the game then says the window system stopped it.
  The game runs under Wayland by default; `--window-system x11` (or
  `RL_WINDOW_SYSTEM=x11`) runs it under X11 instead, which needs
  `libxkbcommon-x11-0` and draws without VSync on WSLg.

- **Disk:** WSL's virtual disk lives on the Windows drive, and a full debug
  `target/` plus Docker images can fill it. The dev profile keeps only line
  tables for the workspace and dependencies (`debug = "line-tables-only"`;
  backtraces still show file:line, only debugger variable info is dropped). `cargo xtask
  oracle-record` removes superseded `rl-olx-oracle:*` images (it keeps the
  current one and `:dev`) and bounds the Docker build cache. `cargo xtask
  tidy` removes `target/*/incremental` and prunes the images (never
  `target/content` or `target/olx`); `check` warns when `target/` passes 25 GB.
  (WSL's disk file does not shrink by itself; `wsl --manage <distro>
  --set-sparse true` after `wsl --shutdown` helps.)

## Checks

- `cargo xtask check` (about a minute) is the gate before every commit. It needs
  no GPU or sound device.
- `cargo xtask full` runs `check`, then the bot-match grid (both mods × 4
  levels × 3 seeds of Deathmatch and chaos bots: every weapon fired, every
  projectile action triggered, and a 30-minute match within bounds), then
  the device tier: the renderer's GPU tests, the render goldens
  (`fixtures/render/`), the 10-minute headless sandbox soak (60 000 ticks,
  memory and entity counts must stay bounded) and a short sound on the
  default output. Each device step skips with a notice when its adapter or
  device is missing.
- `cargo xtask bless-render --reason "<why>"` re-draws the render goldens and
  logs why in `fixtures/blessings.log`.
- `fixtures/matches/` holds golden bot matches (their checksum streams);
  `cargo xtask bless-regression --reason "<why>"` re-records them after an
  intended change and logs it the same way.

## Credits

Based on OpenLieroX by Albert Zeyer, Karel Petránek and the OpenLieroX team,
which is based on Liero Xtreme by Jason Boettcher, which is inspired by Liero
by Joosa Riekkinen. Bundled OLX mods and levels belong to their authors.

## License

GPL-3.0-or-later. See `LICENSE`.
