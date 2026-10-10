# tools/olx-oracle

A patched OpenLieroX, built in Docker, that records reference traces for the
fidelity oracle (spec §6, §18). `cargo xtask oracle-record` builds it and runs it;
you rarely need to touch these files.

## Files

- `Dockerfile`: OLX at commit `d1ad953f` from GitHub, plus the patch, built as a
  headless dedicated server (Ubuntu 22.04, as OLX's own Docker image).
  The image tag is a hash of the three files here, so any edit rebuilds it.
- `oracle-hooks.patch`: the test hooks, 18 OLX files and about 830 added lines.
- `oracle_control.py`: drives one recording over OLX's dedicated-control pipe.

## What the patch adds

- **`simLoadInputs <file>`:** loads the schedule.
  - `frame worm keys` lines (key bits in `OracleHooks.h`) drive worms through
    OLX's human input handler with virtual keys (`CInput::virt`), so aim,
    facing, rope, firing and weapon switching run through OLX's real keyboard code.
  - `frame bonus x y health|weapon [name]` lines place a bonus at the start of
    that frame (as `simPlaceBonus`).
  - **Every worm** gets the human handler, listed or not, so no bot AI runs (D8).
  - A key press is latched until the next `getInput()` resets it. It counts as
    held (`pressed`, like `CInput`'s `nDown`) and as pressed once (`once`, like
    `nDownOnce`). `getInput()` runs once per 84 Hz physics sub-step, not once
    per 100 Hz frame.
  - It restarts `simulationTime` at 100 000 ms (`START_TIME_MS`) and aligns the
    LX56 physics clock with it. OLX starts the clock at the wall clock, which
    varies by run and decides when time-since-0 checks (bleeding) first pass.
- **`simRecord <file|off>`:** writes one JSON line per simulated frame.
  - First a header, then frame −1 (the state when recording starts), then frames 0..n.
  - Starting a recording clears the entity pool and resets the server's bonus timer.
  - `off` reports any failure during the recording (such as a bonus that found
    no free slot) as a command error.
- **`simSetSeed <n>`** also reseeds the three `FindSpot` MT19937 streams
  (`rnd`, `rndgen`, `midrnd`) and restarts the `rc` and `mt` counters (D6).
- **`simSetWeapons <id> "<name0>,...,cur=<k>"`:** one OLX weapon name per slot
  (`WeaponSlotsNum`, 5 by default), all full (charge 1, not reloading, no last
  fire), slot `k` selected.
- **`simSetHealth <id> <hp>`.**
- **`simPlaceBonus <x> <y> <health|weapon> [name]`:** `GameServer::SpawnBonus`
  with the given position and kind (no `FindSpot`, no `rand()`); its packet is
  parsed by the local client at once, so server and client both have it.
- **Game loop hooks** (`Game::frameInner`):
  - `beforeFrame()` at the start of each simulated frame: pins the clock,
    applies the frame's key changes and bonuses.
  - `afterFrame()` after `cServer->Frame()`: writes the trace line, unpins.
  - `betweenFrames()` after `simulationTime` advanced: message and shot delivery.
- **Shot delivery, Model R (D1).** The shots added during frames F and F+1 form one
  list. After frame F+1 the hook encodes it (`writePacket`) and feeds it to the
  client's packet parser (`readSingle`/`readMulti`, `ProcessServerShotList`), so
  the projectiles exist before frame F+2. While recording, `GameServer::SendUpdate`
  sends no shot lists.
- **Message delivery (D10).** After each frame, with the clock pinned, `betweenFrames()`
  takes the local connection's queued reliable messages out of both channels
  (`CChannel::Messages`), so the network never sends them, and parses them in the
  order a real local game at one frame per main-loop iteration would at the start
  of the next iteration:
  1. the server's messages to the client (bonus spawns and destruction, kill
     messages, ...), by `CClientNetEngine::ParsePacket`;
  2. the shot list (Model R, above);
  3. the client's messages to the server (bonus grabs, damage reports), by
     `CServerNetEngine::ParsePacket`. What the server queues in reply goes out
     after the next frame.

  `olx.log` lists each delivery as `OracleHooks: frame F: server -> client <message
  ids>` (`include/Protocol.h`). Unreliable data (none for the local client in
  these scenarios) still goes over the network.
- **Pinned clock (D2).** While recording, `tLX->currentTime = simulationTime + 10 ms`
  during each frame (from before `gusLogicFrame()`) and during delivery. Between
  frames it is the wall clock again, for the network code.
- **Projectile images (D3)** are loaded in dedicated mode too (full images), so
  `ANI_ONCE` animations end and delete projectiles as on a player's client.
- **`rand()`/`srand()` are interposed** (defined in `OracleHooks.cpp`, delegating
  to `random()`/`srandom()` as glibc does) to count calls. `CMap::FindSpot` counts
  its `rnd()` draws.

## Trace format (v2)

- **Header:** `olx` (version), `features` (every feature value), `weapons`
  (names by OLX weapon id), `projectiles` (by OLX projectile index: the path
  `rl-import-lgs` derives, `<weapon id>[.spawn|.trail]...`), `options`
  (`Game.Blood`, `Game.Particles`, `DoProjectileSimulationInDedicated`, `K` = 10,
  `T0` = frame −1's absolute `simulationTime`), `seed`.
- **Frame:** `f`, `sf` (`serverFrame`), `st` (`simulationTime`), `ct`
  (`tLX->currentTime`), `rc` (`rand()` calls since the seed), `mt` (`rnd()` draws
  since the seed), `ents` (used entity slots), `s` (shots delivered right before
  this frame), `w`, `p`, `b` (client bonuses), `sb` (server bonuses), `go`
  (from the frame `GameServer::GameOver` ran in: `{"f": gameOverFrame, "w":
  winner}`, the winner it computed; absent before, so traces of matches that
  never end are unchanged), `mh`.
- **Times** (`st`, `ct`, a worm's `tod` and `blood`, a projectile's `ls`, `sp`,
  `ig`, `tr`, a shot's `spawn`, a server bonus's `spawn`) are integer ms since
  frame −1's `simulationTime`, so traces do not depend on absolute process time.
  `null` means never set (OLX's `AbsTime` 0). `tod` is `null` while the worm is
  alive. A shot's `t` is its absolute server time (`serverFrame * 10`), and a
  projectile's `ws` is a duration in seconds.
- Floats use `%.9g`. Field meanings are documented on the Rust types in
  `crates/rl-oracle/src/trace.rs`.

## The recording sequence (`oracle_control.py`)

1. **Settings.** It forces LX56 aim (`ForceLX56Aim`), so traces don't depend on client options.
2. **Manual stepping from the lobby on.** `simSetSeed`, `simStep 0`, then
   `startlobby`, `addBots`, `startgame`, stepping until `Playing`. No frame
   depends on the wall clock.
3. **Inputs, then spawns.** `simSetSeed` again, `simLoadInputs` (keys and
   bonuses), then `spawnWorm` for each worm (after the clock restart),
   `simSetWeapons` and `simSetHealth`, then `simRecord` and `simStep frames`.

## Known limits

- The server spawns bonuses on its own timer (`BonusFrequency`, default 30 s) at
  random `FindSpot` positions. This is deterministic: the timer is reset to the
  pinned clock when the recording starts, `SimulateGame` reads the pinned
  `currentTime`, and `FindSpot` and the kind and weapon draws use the seeded
  generators (`bonus-natural` sets `BonusFrequency` to 1 s). Other scenarios
  place bonuses with `simPlaceBonus` and stay shorter than 30 s.
- Weapon bonuses use the weapons `WeaponRestrictionsFile` does not ban. `rl-sim`
  does not import restriction files yet, so `bonus-natural` uses OLX's
  `Bonus only` preset, which allows every Classic weapon.

## Changing the patch

1. Edit an OLX checkout at commit `d1ad953f`.
2. Regenerate the patch: `git add -N <new files> && git diff > oracle-hooks.patch`.
3. Run `cargo xtask oracle-record --check`. If behaviour is meant to stay the
   same, every committed trace must still match.
