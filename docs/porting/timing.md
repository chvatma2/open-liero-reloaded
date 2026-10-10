# OLX timing, as ported in `rl-sim`

Spec §5.2 asked for this to be settled before any physics. Everything below
was confirmed bit for bit by the oracle (`fixtures/oracle`); sources are at
OLX commit `d1ad953f`.

## Time types

- `AbsTime` and `TimeDiff` are whole **milliseconds** in a `u64` (`include/olx-types.h:29-100`).
- **Seconds** are `ms * 0.001f`. So 20 ms is `20 * 0.001f`, which is **not** the
  same float as `0.02f`. `rl_math::TimeDiff::seconds` reproduces this.

## The two clocks

| Clock | OLX | Step | `rl-sim` |
|---|---|---|---|
| **Frame clock** | `Game::simulationTime`, read by `GetPhysicsTime()` | `Game::FixedFrameTime` = 10 ms | `World::time` |
| **Physics clock** | `CClient::fLastSimulationTime` | `LX56PhysicsDT` = `TimeDiff(1000 / LX56PhysicsFPS)` | `World::physics_time` |

- **The physics step is 11 ms, not 1/84 s.** `1000 / 84` is integer division,
  so the default step is 11 ms (about 91 Hz), and the worm's `dt` is
  `11 * 0.001f` = 0.0110000009.
- **Game speed** divides both steps (`TimeDiff * (1/speed)`, truncated to whole
  ms, at least 1) unless `GameSpeedOnlyForProjs` is set.

## One frame (`Game::frameInner`, `src/game/Game.cpp:897-959`)

1. **`serverFrame++`.**
2. **`gusLogicFrame()` inside a `GusSpeedScope`.** Every worm's and every rope's
   velocity is multiplied by `0.01f` (`GusDT.seconds()`), then by `1.0f / 0.01f`
   (= 99.9999924f). The round trip is lossy, for example 6.60000134 becomes
   6.60000086. It runs every frame, even for classic mods.
   `World::gus_speed_round_trip` reproduces it.
3. **The frame's keys apply.** In OLX this is the oracle's `beforeFrame`; in
   real play it is the input events.
4. **`CClient::Simulation()`** (`src/client/CClient_Game.cpp:60-222`): while
   `fLastSimulationTime + step <= simulationTime`, simulate every worm (its
   `getInput`, then physics), then advance `fLastSimulationTime` by one step.
5. **`cServer->Frame()`, then the oracle's `afterFrame`,** which records the state.
6. **`simulationTime += 10 ms`.**

With both clocks aligned when a scenario starts, frames 0 and 1 run no physics
step, frames 2 to 11 run one each, frame 12 runs none, and so on: the physics
clock falls 1 ms further behind every frame.

## Input timing

- **`getInput` runs inside each physics step**, so up to once per frame, and
  sometimes not at all in a frame.
- **Its `dt` uses the frame clock:** `GetPhysicsTime() - fLastInputTime`.
  After a spawn at the frame time T, the first read (in frame 2) uses
  `dt` = 20 ms.
- **Key latches:** see `docs/porting/STATUS.md`, "Facts the oracle established".

## Spawning

`GameServer::SpawnWorm` spawns the worm, and the host's own client spawns it
again when the spawn message arrives. Both happen at the same frame time and
position before the first recorded frame, and carving the same hole twice
changes nothing. So `World::spawn_worm` spawns once.
