# Open Liero Reloaded (OLR)

A Rust reimplementation of [OpenLieroX](https://github.com/openlierox/openlierox),
built so physics, movement and shooting feel identical, and modernized for
today's displays. It loads classic OLX mods and levels by importing them into
an open, text-based pack format. OLR is its own game, not a version of
OLX: it plays neither OLX's network games nor its replays.

Status: **v0.1, Deathmatch against bots, locally and online.** OLX content
imports, the simulation reproduces OLX's traces bit for bit, and you start
the game from menus: set up a Deathmatch against OLX's bots on any of the 4
levels with either mod, choose your weapons, play, see the results, watch
the replay; or host and join games online (`docs/online.md`). See
`CHANGELOG.md` for what v0.1 holds and leaves out; 0.2 is the graphics
milestone.

## Playing v0.1

### Install

1. **Rust:** install [rustup](https://rustup.rs); `rust-toolchain.toml`
   fetches Rust 1.99 (with rustfmt, clippy and the wasm target) on first
   use. `cargo install cargo-nextest` gives the test runner `cargo xtask
   check` prefers.
2. **System packages** (Debian/Ubuntu, WSL included):

   ```sh
   sudo apt install git pkg-config libasound2-dev libudev-dev \
       libxkbcommon-x11-0 mesa-utils libgl1-mesa-dri libegl1 \
       libasound2-plugins          # WSL: sound through WSLg's PulseAudio
   ```

   Under WSL, sound also needs an `~/.asoundrc` that sends ALSA to
   PulseAudio:

   ```
   pcm.!default pulse
   ctl.!default pulse
   ```

   `xvfb` is optional: `cargo xtask full` runs its end-to-end test on it.
3. **Build:** `cargo build --release -p olr-game` (or let `cargo run` below
   build it).

### Get the content

The game plays OLX's own levels, mods, skins and sounds, converted into
packs. They are committed in `content/packs` (the default `--content`, so
run the game from the repository root): a clone plays as it is. To add
more:

- edit `content/olx-manifest.ron` and run `cargo xtask fetch-olx-content`:
  it sparse-clones OpenLieroX at the pinned commit (network and `git`
  needed) and imports into `content/packs` (commit the result). Today's
  manifest gives the 4 levels, Classic and MW 1.0, the Dirt theme, 718 skins (409 from the repository, 309 more from
  OpenLieroX 0.58 rc5's release, committed in `content/olx-0.58-rc5/`) and
  the core sprites and sounds; or
- `olr-cli import --olx <game directory> --out <data>/packs --all`: imports
  everything in an OLX game directory (or name items with `--mod`, `--level`,
  `--theme`, `--skins`, `--core`) into the data directory's `packs/` (below),
  listed over `--content`'s: a pack of the same id there wins. It adds or
  replaces packs and never removes others (`--prune` does). The game has no
  Import screen; the editors will import too, later.

### Run

```sh
cargo run --release -p olr-game                 # the menus
```

Useful flags (`olr --help` lists them all):

| Flag | What it does |
|---|---|
| `--fullscreen` | borderless fullscreen for this run (not saved; the options also have it) |
| `--window-system x11` | X11 instead of Wayland (also `OLR_WINDOW_SYSTEM=x11`); try it if the Wayland compositor drops the game |
| `--uncapped`, `--filter nearest\|sharp`, `--no-interp` | no VSync; the upscale filter; OLX's whole-pixel drawing of the latest tick (for this run) |
| `--hud-layout auto\|bar\|sides` | where the HUD goes on a window wider than 16:9 (for this run) |
| `--spectate --bots N` | a Deathmatch of 2 to 32 bots alone, watched (no menus) |
| `--speed 1\|2\|4\|8\|16\|max` | how fast a watched game runs: a replay or `--spectate` (PageUp/PageDown change it); a game you play is always 1× |
| `--replay f.olrp` | watch a replay |
| `--mode deathmatch ...`, `--record f.olrp` | a command-line Deathmatch without the menus (below), saving its replay |
| `--data-dir <dir>` | settings, profiles, replays and user packs here instead of the platform's directory |
| `--content <dir>` | the imported content (default `content/packs` beside `olr-game`, else in the working directory) |
| `--no-sound` | open no audio device |

### The menus

- **Local Play:** the mod, the level (thumbnails), OLX's Deathmatch rules
  (lives, kill limit, time limit, loading time, bonuses, the weapon
  restriction preset), 1 to 31 bots at one difficulty (Easy, Medium, Hard,
  Xtreme) or each its own, their personality (OLX's bot or chaos) and
  loadout, and which profile you play. **Start** loads the match and
  shows OLX's **weapon selection**: five slots (Up/Down choose a slot,
  Left/Right the weapon, with Shift six at a time), **Random**, **Done**;
  the mouse works too. After the match (15 kills or 6 minutes by
  default), "Game Over", then the **results** (kills, deaths, suicides,
  score): Rematch, Save replay, Back to menu. The game is laid out for
  16:9 and shows everyone the same area of the level, whatever the
  window: a bar along the bottom shows your health and weapons (left),
  the minimap (centre) and the log of kills and deaths (right), and the
  game is drawn above it. On a 21:9 or wider window the health, weapons
  and log can move into the space beside the picture (Options → Video →
  HUD layout).
- **Sandbox:** you and idle targets on a level, to try weapons.
- **Profiles:** your name, colour and skin (all skins, searchable,
  with an animated preview); the one marked plays.
- **Options:** Video (display: a 1280 × 720, 1600 × 900 or 1920 × 1080
  window, or borderless fullscreen; VSync or uncapped, upscale filter, interpolation, HUD
  layout: Auto, Bottom bar or Side panels; UI scale: the menus and HUD
  always fill the same share of the screen's height, the scale on top), Audio (master and effects volume), Keys
  (any number of keys and gamepad inputs per action; Reset to OLX's keys).
  The match is set up in Local Play, which remembers the last one.
- **Replays:** every Deathmatch played from the menus (the last 20) and the
  ones you saved; click one to watch it.
- **About** (credits, licenses, each pack's origin).

### Controls

The game's keys are OLX's player 1 defaults, changed in Options → Keys:

| Key | Action |
|---|---|
| Arrows | move, aim |
| Left Ctrl | shoot |
| Left Alt | jump |
| X | ninja rope |
| Left Shift + Left/Right | change weapon |

A gamepad works too: D-pad or left stick to walk, right stick to aim, right
trigger shoots, A (south) jumps, Y (north) throws the rope, left trigger +
Left/Right changes weapon.

These keys are fixed (they cannot be bound to actions):

| Key | In a match |
|---|---|
| Esc | pause menu (Resume, Options, Quit to menu); in the menus, back |
| F5 | restart in place (a Deathmatch: a rematch, keeping your weapons) |
| F2 | debug panel (FPS, frame and tick time, bot think time, entities) |
| Tab (held) | scoreboard |
| H | health bars over worms |
| Left / Right | follow the previous / next worm, in a replay, a spectated game or an online match you watch |
| PageUp / PageDown | faster / slower, for a replay or a spectated game |

### Where things are kept

The data directory is `~/.local/share/open-liero-reloaded/` on Linux (another with
`--data-dir`):

- `settings.ron`: the options and the last local-play setup;
- `profiles.ron`: the profiles;
- `replays/`: `match-NNNNNN.olrp` (every menu Deathmatch, the last 20 kept)
  and `replay-NNN.olrp` (saved from the results, never removed);
- `packs/`: user packs (for example `olr-cli import`'s output), layered over `--content`.

A broken settings or profiles file is set aside as `<name>.bak` and the
game starts with defaults (and says so in its log). One from a newer
version of the game is left where it is, untouched: the game uses
defaults, saves nothing to it that session, and says so on the main menu.

### Known notes for WSL

- **Window size:** the window cannot be drag-resized or maximised (WSLg's
  compositor crashes on quick resizes); choose a size in Options → Video
  or use `--fullscreen`.
- **GPU:** under WSL the game restarts itself with Mesa's d3d12 driver
  (`GALLIUM_DRIVER=d3d12 WGPU_BACKEND=gl`) unless you set either variable,
  and with NVIDIA's WSL driver present it asks for the NVIDIA card
  (`MESA_D3D12_DEFAULT_ADAPTER_NAME=NVIDIA`), since Mesa otherwise takes the
  first adapter Windows lists, often the integrated GPU. Set
  `MESA_D3D12_DEFAULT_ADAPTER_NAME` to part of another adapter's name to
  choose it; the startup line `olr: GPU …` names the one in use.
- WSLg presents at the compositor's 60 Hz under Wayland; under X11
  (`--window-system x11`) it draws without VSync.

## Command-line games and tools

The flags of the earlier milestones still play a game at once, without
the menus (for scripted runs and tests).

### The sandbox

```sh
cargo run --release -p olr-game -- --level <id> --mod <id> [--skin <id>] [--targets N] [--seed N] [--bonuses]
```

`--level` and `--mod` take a pack id or an OLX name or prefix (levels: `Liero
Factory`, `Dirt Level`, `FossilFacility`, `CastleStrike`; mods: `Classic`, `MW
1.0`). Other options: `--fullscreen`, `--uncapped` (no VSync), `--no-interp`
(OLX's whole-pixel latest-state drawing), `--filter nearest|sharp`, `--content
<dir>`. Run `olr --help` for all of them, including the agent helpers
`--demo`, `--script`, `--shot`, `--autoquit-after` and `--stats`.

The controls are those above, but Esc quits (there is no pause menu) and
F5 restarts the sandbox. The `--targets` option sets how many idle worms
stand around to shoot at (default 3).

### Deathmatch against bots

```sh
cargo run --release -p olr-game -- --mode deathmatch --bots 3 --difficulty medium \
    --level CastleStrike --mod "MW 1.0" [--lives L] [--kill-limit K] [--time-limit M] \
    [--seed S] [--record match.olrp]
```

You (worm 0) play OLX's Deathmatch against 1 to 31 of OLX's bots, ported
from its `CWormBot` (they press the same keys you do). The defaults are
OLX's: 15 kills or 6 minutes end the match, unlimited lives. `--difficulty`
is `easy`, `medium`, `hard` or `xtreme`; no level aims perfectly (Xtreme
keeps a small aim error, unlike OLX's). Bots get OLX's preset loadouts; on
the command line you get random enabled weapons (the menus' matches have
OLX's weapon selection). After "Game Over" the results appear, F5 plays a
rematch on the next seed, Esc quits. `--spectate` lets the bots play alone
(2 to 32) while you watch, at `--speed` and PageUp/PageDown; Left/Right choose
the worm the camera follows.

#### Replays

`--record f.olrp` saves the match's replay when you quit and on each rematch
(`f-2.olrp`, `f-3.olrp`, ...). A replay stores the setup and every worm's keys
(bots too), and checksums of the world, so the same build replays it exactly:

```sh
cargo run --release -p olr-game -- --replay f.olrp     # watch it
cargo run --release -p olr-cli -- replay f.olrp        # check it headless
```

A replay names its mod and level packs by content hash and is refused on
other content; a divergence is reported at the first checksum that differs.

### Headless bot matches

```sh
cargo run --release -p olr-cli -- match --mod olx-classic --level olx-level-dirt-level \
    --bots 8 --difficulty mixed --seed 1 --ticks 6000 [--personality chaos] \
    [--coverage] [--record f.olrp]
```

`olr-cli match` plays bots against each other without a window, checks the
invariants every tick, plays the match again (determinism) and replays its
recording (round trip), and prints tick times. `--personality chaos` bots
fire every weapon they get; `--coverage` lists the weapons, actions and
bonuses used. A failure saves its replay and seed to `target/failures/`.

### Authoring packs

Packs are readable RON files and PNG images; [`docs/formats/README.md`](docs/formats/README.md)
explains the model with one complete example of each kind, and the pages
beside it list every field.

```sh
olr-cli pack new level my-level --size 640x480   # also: mod, or skin/theme/core --from <pack>
olr-cli pack check my-level                      # parse, compile, validate
olr-cli pack fmt my-level                        # canonical form (--check to list)
olr-cli level carve my-level --circle 100,100,20 # also fill, paint, stamp, resize, object
olr-cli level preview my-level --out preview.png --scale 2
```

## Windows Subsystem for Linux (development)

- **GPU:** on WSL the game picks the GL backend through Mesa's d3d12 driver by
  itself (`WGPU_BACKEND=gl GALLIUM_DRIVER=d3d12`), unless you set either
  variable. On a PC with integrated graphics and an NVIDIA card it asks for
  the NVIDIA card; to choose another adapter, set
  `MESA_D3D12_DEFAULT_ADAPTER_NAME` to part of its name (the startup line
  `olr: GPU …` shows the one in use). Software rendering works too: `WGPU_BACKEND=vulkan` uses lavapipe.
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
  `OLR_WINDOW_SYSTEM=x11`) runs it under X11 instead, which needs
  `libxkbcommon-x11-0` and draws without VSync on WSLg.
- **The Windows build:** `cargo xtask windows` builds `olr-cli` and `olr-game`
  for Windows with mingw-w64 (`rustup target add x86_64-pc-windows-gnu`
  once) and runs them through interop; nothing is installed on Windows.
  They are staged with the content in `%TEMP%\olr-envelope`, and
  the command lists the adapters DX12 and Vulkan see.

- **Disk:** WSL's virtual disk lives on the Windows drive, and a full debug
  `target/` plus Docker images can fill it. The dev profile keeps only line
  tables for the workspace (`debug = "line-tables-only"`; backtraces still
  show file:line in our crates) and no debug info for dependencies. `cargo xtask
  oracle-record` removes superseded `olr-olx-oracle:*` images (it keeps the
  current one and `:dev`). `cargo xtask tidy` removes superseded builds from
  `target/` (the older hashes cargo leaves beside each rebuilt artifact, and
  old incremental sessions) and prunes the images (never `content/packs` or
  `target/olx`); `cargo xtask tidy --docker-cache` also bounds Docker's build
  cache to 2 GB, which is machine-wide (other projects' build layers too),
  so it is opt-in. `check` runs the same sweep when `target/` passes 20 GB.
  (WSL's disk file does not shrink by itself; `wsl --manage <distro>
  --set-sparse true` after `wsl --shutdown` helps.)

## Building a release zip

`cargo xtask dist` builds portable zips for Linux and Windows in one run:
`target/release-zips/open-liero-reloaded-<version>-linux-x64.zip` and
`target/release-zips/open-liero-reloaded-<version>-windows-x64.zip`. Each holds one folder,
`open-liero-reloaded-<version>/`, with `olr-game` and `olr-server` (`.exe` on
Windows, built in the `dist` profile: release without symbols), the
content in `content/packs/`, this README, the license, the changelog,
`docs/online.md` and `server.example.ron`. Copy a zip anywhere, unpack it
and run `olr-game`: without `--content` both programs use the
`content/packs` beside them. Settings, profiles and replays stay in the
user's profile directory, as with any build. The Linux programs need
`libasound2` and `libudev1` (`olr-game` only) and a glibc at least as new
as the building machine's; the Windows ones need no extra DLLs.

The Windows build needs mingw-w64 and the Rust target (`sudo apt install
mingw-w64`, `rustup target add x86_64-pc-windows-gnu`). At the end each
zip is unpacked into a fresh directory and run from another one: on Linux
`olr --help` and an `olr-server` match with a bot client, on Windows
(through WSL interop; skipped with a notice without it) `olr.exe
--help` and `olr-server.exe` loading its map. `--no-verify` skips that.
The two whole-program-optimised builds take several minutes from cold.

## Checks

- `cargo xtask check` (about 30 s warm) is the gate before every commit. It
  needs no GPU or sound device.
- `cargo xtask full` runs `check`, then the bot-match grid (both mods × 4
  levels × 3 seeds of Deathmatch and chaos bots: every weapon fired, every
  projectile action triggered, and a 30-minute match within bounds), then
  the device tier: the renderer's GPU tests, the render goldens
  (`fixtures/render/`), the 10-minute headless sandbox soak (60 000 ticks,
  memory and entity counts must stay bounded), the HUD snapshots and a
  short sound on the default output; then the end-to-end runs on Xvfb
  with software GL (`fixtures/autoplay/menus.ron`, `olr --autoplay
  --fake-clock`, and the online run `fixtures/autoplay/online.ron`
  against an `olr-server` and a local Nostr relay: a typed join, then the
  same server from the Internet tab), `cargo xtask net-nat` (joins across
  Docker routers: port-restricted, symmetric, UPnP, PCP, NAT-PMP, a TURN
  relay; skipped with a notice without Docker), the benchmarks and short
  engine stress run against their baselines in `fixtures/bench/` (more
  than 10 % slower fails), the network feel check and the Windows and
  arm64 determinism runs. Each device step skips with a notice when its
  adapter, device or Xvfb is missing. Several minutes warm.
- `cargo xtask e2e`, `cargo xtask net-nat`, `cargo xtask bench [--bless
  --reason "<why>"]` and `cargo xtask stress [--seconds N]` run those
  steps alone; `cargo xtask net-soak [--minutes N] [--seed S]` (not part of
  `full`) plays seeded online cases, host crashes, departures and freezes
  among them, for N minutes (default 10) and keeps each failure as a case in
  `target/failures`; `stress` plays
  32 chaos bots on Castle Strike past OLX's limits and prints a report.
- `cargo xtask envelope [--quick] [--only <part>] [--gpu
  wsl|windows|both|none]` measures how far the game goes on this machine
  (about 7 minutes warm; not part of `full`) and compares with
  `fixtures/bench/envelope/<host>.ron`; `--bless --reason "<why>"` writes
  it, `--report` regenerates `docs/perf/envelope.md`. `cargo xtask
  windows` builds the Windows binaries.
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
