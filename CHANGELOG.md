# Changelog

All notable changes to rust-liero are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## Unreleased

### Changed

- The menus are centred: every screen the main menu opens is one column in
  the middle of the window, its title on top, its sections in panels and
  its main button (Start, Import) large and centred at the bottom. Local
  Play shows the map on the left and the match and players on the right,
  with "Reset the match to OLX's settings".
- The options have no Gameplay tab: the match is set up in Local Play
  before each game (it remembers the last one), and the tab edited the
  same settings.
- The in-game HUD is an opaque bar along the bottom, as OLX 0.58's local
  game had: health and weapons on the left, the minimap in the centre, a
  log of kills, deaths and the match's messages on the right. The game is
  drawn above the bar, never under it, and nothing fades any more. The
  clock stays top centre, the keys top right.
- The options' Video section has one Display list: a 1280 × 720,
  1600 × 900 or 1920 × 1080 window, or borderless fullscreen. The
  desktop-sized window is gone (its bottom sat under the taskbar); a saved
  "Desktop size" opens fullscreen.

### Fixed

- On Wayland (WSLg), changing the window's size or fullscreen no longer
  leaves pieces of the old window frame on the screen: the window is made
  again at its new size.

## 0.1.0 - 2026-10-11

The first playable release: a local Deathmatch against OLX's bots, started
from menus, on any of the 4 bundled levels with either mod (spec §1.3,
milestone M7). It is a Rust reimplementation of OpenLieroX (OLX, commit
`d1ad953f`). Physics, movement and shooting reproduce OLX's own recorded
traces bit for bit.

### Added

- **Content import (M1).**
  - Importers for OLX's gamescript v7 mods (`script.lgs`), image `.lxl`
    levels, themes, skins, the core sprites and sounds, and `.wps`
    weapon-restriction presets.
  - Imported content becomes open, text-based packs (`rl-pack`): versioned
    RON plus PNG and OGG, with content hashes.
  - `cargo xtask fetch-olx-content` imports the pinned OLX content: Liero
    Factory, Castle Strike, Fossil Facility and Dirt Level; Liero v1.0
    (Classic) and Modern Warfare 1.0; the Dirt theme; all 409 skins.
  - The importers have unit and property tests, and the imported content
    has a corpus snapshot.
- **Fidelity oracle (M2).**
  - A patched OLX in Docker (`tools/olx-oracle`) records scenarios as
    per-frame traces.
  - `rl-oracle` compares them field by field.
  - `rl-cli scenario` replays them and reports the first divergence.
- **Simulation (M3, M4).** `rl-sim` is `no_std` and also builds for
  `wasm32-unknown-unknown`. It covers:
  - OLX's 100/84 Hz clocks;
  - worm movement and collision, and the ninja rope;
  - the projectile tree, every projectile action, shots and beams;
  - explosions, carving, dirt, damage, death and respawn, and bonuses;
  - glibc-exact `sinf`, `cosf` and `atan2f`.

  109 of the 112 recorded oracle scenarios match OLX bit for bit in every
  field. The other 3 are pinned to their first divergence (see Deviations).
- **Presentation (M5).**
  - `rl-scene` holds OLX's camera, skins, sprites, beams and layer order.
  - `rl-render` (wgpu) draws at art scale and upscales: whole multiples
    (nearest) or sharp bilinear. Frames are interpolated.
  - `rl-fx` holds OLX's 1024-slot cosmetic entities: blood, gibs and
    stains.
  - `rl-audio` (kira) mixes positional sound with OLX's OpenAL defaults.
  - `rl-input` has OLX's keyboard and gamepad defaults, latched per tick.
  - A sandbox in `rl-game`, a fixed 100 Hz loop and render goldens.
- **Deathmatch and bots (M6).**
  - OLX's Deathmatch rules and texts (`rl-modes`).
  - OLX's `CWormBot` ported in `rl-bots`: navigation, aim, weapons, rope,
    four difficulties, and a "chaos" personality that fires everything.
  - Replays (`.rlrp`, checksummed, refused on other content).
  - `rl-cli match` and `rl-cli replay` for headless bot matches, with
    invariants, determinism and coverage.
  - Golden match checksums, and a bot-match grid over both mods, 4 levels
    and 3 seeds in `cargo xtask full`.
- **Menus, HUD and v0.1 (M7).**
  - **`rl-ui`:** the menus' logic and egui screens, with settings and
    profiles saved as versioned RON in the platform data directory. A
    broken file is set aside as `.bak`; one from a newer version is left
    untouched, and the main menu says so.
  - **Main menu:** Local Play, Sandbox, Profiles, Options, Replays, Import,
    About, Quit.
  - **Local Play:**
    - the mod and the level, with thumbnails made at import;
    - OLX's Deathmatch rules (lives, kill and time limits, loading time,
      bonuses, restriction preset);
    - 1 to 31 bots at one difficulty (Easy, Medium, Hard, Xtreme) or each
      its own, with their personality and loadout;
    - the profile you play.
  - **Profiles:** name, colour and skin, with a live colorized preview of
    the walk animation.
  - **Options:**
    - window-size presets and fullscreen, VSync or uncapped, the upscale
      filter, interpolation and UI scale;
    - master and effects volume;
    - key and gamepad rebinding that never leaves an action unreachable;
    - the default match.
  - **OLX's weapon selection** before each match: five slots, Random,
    Done, the keyboard, a gamepad or the mouse.
  - **The in-game HUD:**
    - health and five weapon slots with reload bars;
    - kill feed, clock and kill limit;
    - a minimap in OLX's colours;
    - Tab's scoreboard;
    - the minimap and the health and weapons panel fade while a worm is
      under them.
  - **Pause menu:** Esc gives Resume, Options and Quit to menu. No tick
    runs while paused.
  - **Results:** OLX's columns plus score, deaths and suicides, with
    Rematch, Save replay and Back to menu.
  - **Replays:** every Deathmatch played from the menus is saved (the
    last 20), and saved ones are watched from the Replays screen.
  - **In-game import** of an OLX game directory on a background thread,
    layered over the bundled packs.
  - **About:** the credits and licenses.
  - **Fast-forward** for games nobody plays (replays and `--spectate`):
    1×, 2×, 4×, 8×, 16× and max, with `--speed` and PageUp/PageDown. The
    simulation is identical at every speed.
  - **`--autoplay <script.ron> [--fake-clock]`** drives the real app by
    widget label. `cargo xtask e2e` runs the menus end to end on Xvfb.
  - **Benchmarks** (divan): tick, bots, draw list and render frame.
  - **An engine stress test:** 32 chaos bots, OLX's projectile pool and a
    20 000 pool. Baselines are in `fixtures/bench/`, and a regression of
    more than 10 % fails `cargo xtask full`.
  - The exit summary gives frame and tick-time percentiles and the bots'
    think time.
- **Tooling.**
  - `cargo xtask check`: format, clippy, layering, the wasm build, content
    and tests, about 30 s.
  - `cargo xtask full`: the bot grid, the GPU, render and HUD goldens, the
    10-minute soak, the audio check, e2e, bench and stress.
  - `bless-render` and `bless-regression` record a reason with every
    re-recorded golden.
  - `tidy` reclaims disk space.

### Deviations from OLX

The full list, with reasons, is in `docs/porting/STATUS.md`.

- **Exact-zero deaths (spec §18 D9).** A worm left at exactly 0 health dies
  as in OLX, but OLX's Gusanos death particles are not simulated. Their
  random draws shift later respawn spots, so 3 MW 1.0 oracle scenarios
  (`assassin-s-shot`, `flak`, `plasma-launcher`) diverge from their respawn
  frame on.
- **Bots never aim perfectly** (the user's direction). Every difficulty
  keeps at least 1/6 of the aim error, so Xtreme has half of Hard's error
  where OLX's has none. Bots think once per 100 Hz tick on a work budget,
  draw from their own random stream and press the same keys as a player.
- **The damage score is counted once.** OLX counts it twice; it only breaks
  score ties.
- **Modern presentation.**
  - The art is drawn at art scale, then upscaled to any window and
    interpolated (`--no-interp` gives OLX's whole-pixel drawing).
  - The HUD, names and menus are egui at the display's resolution. OLX's
    bitmap-font Classic HUD was dropped (the user's call).
  - The menus are laid out anew rather than copied from OLX's 640 × 480
    screens.
  - The minimap adds the camera's view rectangle.
  - The HUD lies over the playfield, where OLX keeps it outside: the
    minimap and the health and weapons panel fade to 25 % in 150 ms while
    a worm is under them, and the minimap is at most 1.5 × OLX's size.
- **Things OLX does not have:**
  - replays, fast-forward, `--spectate` and the engine stress test;
  - level thumbnails made at import;
  - profiles that keep Unicode names;
  - any number of keys per action.
- **Fixed window size.** The window is not drag-resizable or maximizable;
  sizes come from Options presets or `--fullscreen`, paced at most one
  every 250 ms (WSLg's compositor crashes on quick resizes).
- **OLX content is converted, not read in place.** Liero `.lev` levels,
  Gusanos mods and levels, and non-PNG skins are not imported.

### Known limitations

- **No network play.** Its model, with no master server, is a later
  design.
- **Bot skill** is OLX's, with the aim-error floor. The planned
  difficulty redesign (strategy, reactions, predictive reloading,
  teamwork) is a later milestone, together with a tournament command.
- **Game modes:** Deathmatch only, with no teams.
- **Replays** have no seeking, and they draw every worm with the default
  skin and colours (the human's look is not recorded).
- **Spectating** a bot match is only possible from the command line
  (`--spectate`), not from the menus.
- **No Classic HUD:** OLX's bitmap fonts and HUD images are not imported
  (the user's call).
- **No level editor or map making yet.**
- **`--autoplay` in a real window** sometimes loses a click (3 of 14 in
  the acceptance runs, none on Xvfb); it is tooling only.
- Developed and tested on Linux (WSL2 with WSLg); other platforms are
  untested.
