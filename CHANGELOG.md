# Changelog

All notable changes to rust-liero are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## Unreleased

### Added — finding games

- Multiplayer has three tabs: **Internet** (games announced on public Nostr
  relays: name, map, mod, players, ping, region and a lock for a password;
  games of another version greyed out, games without a public address "cannot
  be joined yet"; Join, and Block this host), **LAN** and **Direct** (an
  address or an invite code). See `docs/online.md`.
- Invite codes (`rlx1…`): a hosted game's lobby shows its code with a Copy
  button; pasting one in Direct joins that game and only that game.
- Visibility, chosen when hosting ("Who can join"): Anyone (Internet), This
  network or Invite only. `rl-server` takes `visibility: Public`, `Lan` or
  `Invite` (`DirectOnly` still reads as `Invite`) and now defaults to Public.
- Options → Online: region, public address, the relays and the STUN servers
  (with Reset to defaults), and the blocked hosts.
- `rl-server` announces itself on the relays (`relays`, `region`), learns its
  public address from the STUN servers (`stun`) or `public_address`, prints
  its invite code in the log and answers `/invite`.
- A joiner with an invite or a listed game checks the host's key: a host that
  cannot prove it is refused ("this is not the game you were invited to"). A
  host that never answers is given up on after 10 s.
- `rl-cli nostr-probe` and `rl-cli stun-probe` measure public relays and STUN
  servers (by hand: the relay probe publishes short-lived notes to them).
- Still needed: players outside your LAN reach a game only through a
  forwarded port (NAT traversal comes with plan 12b).
- Privacy: opening Multiplayer connects to the relays in Options (8 public
  relays by default), which see your IP address; remove every relay to
  stay off them. Upgrading `rl-server`: a config without `visibility` is
  now Public and announces the server on the default public relays (set
  `visibility: Lan` or `Invite` to keep it private).

### Fixed — finding games (final review)

- A closed game no longer comes back for up to 3 minutes from a relay that
  missed its closing note, and a game hosted again right after closing
  shows at once.
- Announcements are dated by the wall clock at each note: a host that
  suspended or whose clock was corrected by NTP after it started is no
  longer dropped from every list; the game list judges ages the same way.
- A game's password proof is bound to the joiner and the connection and
  sent only once the host proved its key: an impostor can no longer relay
  it into a password-protected game, and an unproven "wrong password" is
  not believed.
- Relay edits in Options apply at once, also to the relays kept open after
  leaving Multiplayer and to a hosted Public game (which stops announcing
  on removed relays); blocking and unblocking apply at once.
- An Invite only game says when its code has no address ("This code has no
  address: forward port N or set the public address in Options").
- Left/Right follow another worm in `rl-game --spectate` and `--replay`.
- "Listed on the Internet" no longer lingers after every relay dropped or
  from a previous hosted game; a private or IPv6 public address is never
  announced on public relays (the game says so; `rl-server` warns).
- A failed host proof on a LAN or direct join says "this game could not
  prove who it is" (nobody was invited).
- An invite code with a typo in `rlx1` says "damaged" instead of being
  looked up as a host name; the Internet tab forgets a password typed for
  another game.
- `rl-server` checks `stun` entries (`host:port`), holds `relays` to 32,
  each once; the example config leaves both lists out so a copy follows
  later defaults.
- Relay connections: events a relay sends outside our subscription are
  dropped and the inbox is bounded by memory; reconnects are spread out
  (±20 %); STUN survives a stale ICMP reset on Windows.

### Added — content download

- A player who lacks a game's mod, level, theme or a player's skin
  downloads it from the host in the lobby: a "Downloads" list with
  progress, Ready once the content is complete. More than 50 MB still to
  download asks first ("This game needs N MB of content from the host").
  Downloads are verified, resume where they stopped and are kept in a
  download cache (1 GB at most, the least recently used dropped first);
  Options → Online shows its size and clears it. See `docs/online.md`.
- Skins travel through the host: every player sees everyone's skin, even
  one only its wearer has.
- A player still downloading when a match starts waits it out in the
  lobby, connected, and joins the next one (or watches once complete)
  instead of being dropped.
- `rl-server` serves its packs to players who lack them, keeps their
  skins in `<data dir>/downloads`, and sends at most `upload_kib_s`
  (default 4096 KiB/s) in all.
- `rl-cli net-client --cache <dir>`: bot clients download what their
  `--content` lacks (a temporary cache by default).
- Replays of matches on downloaded content play back, from the Replays
  screen and from `--replay`.

### Added — release zips

- `cargo xtask dist` builds portable zips for Linux and Windows,
  `target/release-zips/rust-liero-<version>-<platform>.zip`: `rl-game`,
  `rl-server`, the content, the docs and an example server config in one
  folder, to unpack and run anywhere. It unpacks and runs each zip to
  check it (`--no-verify` skips that).
- `rl-game` and `rl-server` without `--content` use the `content/packs`
  beside the program when there is one, else the one in the working
  directory as before.

### Added — choosing the worm to watch

- Left/Right choose the worm the camera and the HUD follow in any game
  nobody plays: a replay, a `--spectate` match and an online match you
  watch (before, only the last, with PageUp/PageDown). PageUp/PageDown
  change the speed only; online they do nothing. A replay starts on the
  worm of the player who recorded it (worm 0 for a match of bots alone).

### Added — in game

- While the change-weapon key is held, the selected weapon's name shows
  above your worm, in place of your name, and stays for 0.75 s after a
  change, as in OLX.

### Changed — more effects on screen

- Up to 16 384 effects (blood, gibs, dirt, smoke, explosions) at once
  instead of OpenLieroX's 1024, so busy fights no longer lose their
  blood and gibs. The last 1024 are kept for deaths, so a death's blood
  explosion always shows. The room is set aside once when a match
  starts. A 16-bot chaos match peaks at about 7 600 effects; with 32
  bots firing everything a frame takes up to about 1-2 ms more on the
  CPU, well within 144 Hz.

### Fixed — hits and deaths

- Shots hit a worm where it is. About one hit in ten tested where the worm
  had been a second earlier (an OpenLieroX bug the port had kept on
  purpose): worms died to shots that visibly missed, and shots passed
  through worms. The oracle still reproduces OLX's behaviour.
- A worm left at exactly 0 health now dies with the death sound and the
  blood explosion, like any other death, and the kill goes to whoever
  dealt the last damage (OLX credited nobody and showed nothing). This
  holds in the seconds after the game over too.
- Every death shows in full: its blood is never dropped when a busy
  moment fills the effects, and online a correction that moves a death
  to another moment no longer shows it twice.
- Every death is heard: when two of the same death sound already play,
  the oldest makes way (a 5 ms fade) instead of the new one being
  skipped.
- Replays recorded before this change no longer play (replay format 2).

### Changed

- Online play's protocol version is 2: this version and the earlier one
  cannot play together.
- Disk use of builds: dependencies are built without debug info in dev,
  tests are one binary per crate, and `cargo xtask check` sweeps
  superseded builds automatically.
- Release builds use whole-program optimisation (fat LTO, one codegen
  unit). Small hot paths got up to 12 % faster (32 worms' tick, the draw
  list); the heavy ticks and frames are unchanged within noise. A release
  rebuild takes about two minutes.
- The menus are centred: every screen the main menu opens is one column in
  the middle of the window, its title on top, its sections in panels and
  its main button (Start, Import) large and centred at the bottom. Local
  Play shows the map on the left and the match and players on the right,
  with "Reset the match to OLX's settings".
- The options have no Gameplay tab: the match is set up in Local Play
  before each game (it remembers the last one), and the tab edited the
  same settings.
- The in-game HUD is an opaque bar along the bottom, as OLX 0.58's local
  game had: health and weapons on the left, the minimap in the centre,
  as tall as the bar, a log of kills, deaths and the match's messages on
  the right. The game is drawn above the bar, never under it, and nothing
  fades any more. The clock stays top centre, the keys top right.
- The game is laid out for 16:9 and shows every player the same area of
  the level (427 × 192 world pixels, OLX 0.58's height above its bar):
  a 16:9 window has no black strips, a narrower one black above and
  below, a wider one at the sides. The bar is a fifth of the height.
- The menus and the HUD fill the same share of the screen's height on
  every display (1280 × 720 looks as before; 1440 lines are twice the
  size); the UI scale is on top of that.
- The options' Video section has one Display list: a 1280 × 720,
  1600 × 900 or 1920 × 1080 window, or borderless fullscreen. The
  desktop-sized window is gone (its bottom sat under the taskbar); a saved
  "Desktop size" opens fullscreen.

### Added

- Online play from the menus: host a game, find one in the LAN list or
  join by address; a lobby with chat, ready marks, spectators, loadouts,
  kick and ban; matches that run with rollback netcode. See `docs/online.md`.
- The dedicated server `rl-server`: a map rotation, bots filling empty
  seats, admin chat commands, bans kept in a file, LAN announcement.
- `rl-game --net-sim <profile>`: feel a network (lan, good, typical, bad,
  awful) against bots in one process.
- The network overlay (F3): ping, input delay, rollback and loss.
- Online replays: every online match saves its replay, equal on every peer;
  a desync saves a bundle.
- The Keys tab sets the online chat key (T by default).
- The netcode core for online play (not yet playable from the game):
  rollback with the host ordering inputs, spectators, desync detection and
  repair, tested by whole online matches with bots over simulated networks
  (`rl-cli net-sim`).
- `rl-cli net-repro` reads a desync bundle and names the first tick and
  world field that differ.
- The simulation is checked to give identical results on Linux, in the
  browser's WebAssembly, on Windows and on arm64 Linux (`cargo xtask
  determinism`; the WebAssembly leg needs `sudo apt install nodejs`, the
  arm64 leg `sudo apt install qemu-user gcc-aarch64-linux-gnu`).
- `cargo xtask net-feel` measures how online play feels on five simulated
  network profiles (rollback depth, visible corrections, freezes,
  bandwidth) and fails `full` if it gets worse.
- On a window 21:9 or wider the health, weapons and log can sit beside
  the 16:9 picture instead of in the bar: Options → Video → HUD layout
  (Auto, Bottom bar, Side panels), or `--hud-layout`.
- `cargo xtask envelope`: the performance envelope (players, projectiles,
  effects, map size, art scale, resolution, full-screen shader passes) on
  this machine, on WSL and native Windows, against OLX; results in
  `fixtures/bench/envelope/`, report in `docs/perf/envelope.md`.
- `rl-cli gpu-bench` (a recorded match drawn on the GPU at several sizes,
  with GPU timestamps and a shader-headroom probe) and `rl-cli stress`
  options `--fx-cap`, `--build-every`, `--memory-ceiling-mib`, `--record`
  and generated levels (`--level gen-<w>x<h>-a<scale>-s<seed>`).
- `rl-game --frame-log`: a windowed run's frame times as RON.
- `cargo xtask windows`: `rl-cli` and `rl-game` cross-compiled for Windows
  and run from WSL.

### Added

- 309 more worm skins (Hotrod among them): the ones OpenLieroX 0.58 rc5's
  release ships that its repository lacks, bundled from
  `content/olx-0.58-rc5/skins/` (718 skins in all).

### Fixed

- Import reads OLX images that are Windows bitmaps under a `.png` name, as
  OLX did: OLX 0.58's release has two such skins (Teletubbie, capworm).

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
