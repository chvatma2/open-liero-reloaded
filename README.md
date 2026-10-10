# rust-liero

A Rust reimplementation of [OpenLieroX](https://github.com/openlierox/openlierox),
built so physics, movement and shooting feel identical, and modernized for
today's displays. It loads classic OLX mods and levels by importing them into
an open, text-based pack format.

Status: early development. Milestone 5 is done: OLX content imports, the
simulation reproduces OLX's traces bit for bit, and a playable sandbox shows
it (a level, a mod, you and idle target worms; no menus or network yet).

## Quick start

```sh
cargo xtask fetch-olx-content    # sparse-clone OLX at the pinned commit and import
                                 # its levels, mods and sounds into target/content
cargo run --release -p rl-game -- --level "Liero Factory" --mod Classic
cargo xtask check                # fast gate: format, lint, layering, web build, tests
cargo xtask full                 # check, then the GPU and audio tier (below)
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
| F5 | restart the match |
| Esc | quit |

A gamepad works too (left stick walks, right stick aims; see `crates/rl-input`). The
`--targets` option sets how many idle worms stand around to shoot at (default 3).

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

## Checks

- `cargo xtask check` (about a minute) is the gate before every commit. It needs
  no GPU or sound device.
- `cargo xtask full` runs `check`, then the device tier: the renderer's GPU
  tests, the render goldens (`fixtures/render/`), the 10-minute headless
  sandbox soak (60 000 ticks, memory and entity counts must stay bounded) and a
  short sound on the default output. Each step skips with a notice when its
  adapter or device is missing.
- `cargo xtask bless-render --reason "<why>"` re-draws the render goldens and
  logs why in `fixtures/blessings.log`.

## Credits

Based on OpenLieroX by Albert Zeyer, Karel Petránek and the OpenLieroX team,
which is based on Liero Xtreme by Jason Boettcher, which is inspired by Liero
by Joosa Riekkinen. Bundled OLX mods and levels belong to their authors.

## License

GPL-3.0-or-later. See `LICENSE`.
