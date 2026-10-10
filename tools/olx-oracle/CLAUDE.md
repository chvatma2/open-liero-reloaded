# tools/olx-oracle

A patched OpenLieroX, built in Docker, that records reference traces for the
fidelity oracle (spec §6). `cargo xtask oracle-record` builds it and runs it;
you rarely need to touch these files.

## Files

- `Dockerfile`: OLX at commit `d1ad953f` from GitHub, plus the patch, built as a
  headless dedicated server (Ubuntu 22.04, as OLX's own Docker image).
  The image tag is a hash of the three files here, so any edit rebuilds it.
- `oracle-hooks.patch`: the test hooks, 8 OLX files and about 320 lines.
- `oracle_control.py`: drives one recording over OLX's dedicated-control pipe.

## What the patch adds

- **`simLoadInputs <file>`:** loads `frame worm keys` lines (key bits in `OracleHooks.h`).
  - The listed worms switch to OLX's human input handler, driven by virtual keys
    (`CInput::virt`), so aim, facing, rope and weapon switching run through
    OLX's real keyboard code.
  - A key press is latched until the first `getInput()` consumes it, as
    `CInput::nDownOnce` does. `getInput()` runs once per 84 Hz physics
    sub-step, not once per 100 Hz frame.
  - It also aligns the LX56 physics clock with the frame clock. Without that,
    frame 0 would replay however much time the setup took.
- **`simRecord <file|off>`:** writes one JSON line per simulated frame.
  - First a header with the OLX version and every feature value, then frame −1
    (the state when recording starts), then frames 0..n.
  - Floats use `%.9g`.
- **Game loop hooks** (`Game::frameInner`): `OracleHooks::beforeFrame()` and
  `afterFrame()` around each simulated frame.

## The recording sequence (`oracle_control.py`)

1. **Settings.** It forces LX56 aim (`ForceLX56Aim`), so traces don't depend on client options.
2. **Manual stepping from the lobby on.** `simSetSeed`, `simStep 0`, then
   `startlobby`, `addBots`, `startgame`, stepping until `Playing`. No frame
   depends on the wall clock.
3. **Inputs, then spawns.** `simSetSeed` again, `simLoadInputs`, then
   `spawnWorm` for each worm (after the clock alignment), then `simRecord` and
   `simStep frames`.

## Changing the patch

1. Edit an OLX checkout at commit `d1ad953f`.
2. Regenerate the patch: `git add -N <new files> && git diff > oracle-hooks.patch`.
3. Run `cargo xtask oracle-record --check`. If behaviour is meant to stay the
   same, every committed trace must still match.
