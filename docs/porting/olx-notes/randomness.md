# M4 research 4: randomness in OLX (classic LX56 engine, dedicated server)

All OLX paths are relative to the OLX checkout at commit `d1ad953f` (unpatched:
`scratchpad/openlierox`). The line numbers are from that commit. The oracle patch
(`tools/olx-oracle/oracle-hooks.patch`) adds no RNG code. `simSetSeed`/`simStep` are
already upstream in this commit (`src/common/Command.cpp:2269-2293`).

Test programs: `scratchpad/m4-research/randtest/{t.cpp,b.cpp,c.c}`. Output:
`randtest/out.txt`. They ran in `rl-olx-src` (Ubuntu GLIBC 2.35-0ubuntu3.15,
x86_64, g++ -O3, boost 1.74).

---

## 1. Randomness primitives

### 1.1 libc `rand()`: the only gameplay generator

- **Seeding.**
  - `srand((unsigned int)time(NULL))` runs once at startup (`src/client/AuxLib.cpp:212`, `InitializeAuxLib`).
  - `simSetSeed` reseeds with `srand((unsigned int)seed)`.
  - Nothing else in OLX calls `srand`, `srandom`, `initstate` or `setstate`.
  - The `setstate` hits in `src/gusanos/omfgscript` are a local parser macro, not libc.
- **Wrappers.** Declared in `src/util/Random.h:6-11`, defined in `src/common/MathLib.cpp`:

| Function | Lines | Code | `rand()` calls |
|---|---|---|---|
| `HALF_RAND` | 29 | `const int HALF_RAND = (RAND_MAX / 2);` = 1073741823 | — |
| `GetRandomNum()` | 58-62 | `int rn = rand(); return ((float)(rn - HALF_RAND) / (float)HALF_RAND);` | 1 |
| `GetRandomPosNum()` | 64-67 | `int rn = rand(); return (float)rn / (float)RAND_MAX;` | 1 |
| `GetRandomInt(int max)` | 71-76 | `float f = GetRandomPosNum()*(float(max)+1); return CLAMP((int)f, 0, max);` | 1, **even for `max == 0`** |
| `GetRandomLong(unsigned long max)` | 78-81 | `float f = GetRandomPosNum()*(float(max)+1); return MIN((unsigned long)f, max);` | 1 |
| `GetRandomVec()` | 126-130 | `float angle = GetRandomPosNum() * 2.0f * (float)PI; return CVec(cosf(angle), sinf(angle)) * GetRandomNum();` | 2: PosNum first (its own statement), then Num |
| `randomChoiceFrom(vec)` | `include/Utils.h:39-43` | `data[GetRandomLong(data.size() - 1)]` | 1 |

- **Lua.** Lua 5.1's `math.random`/`math.randomseed` also use `rand()`/`srand()`
  (`libs/lua/lmathlib.c:181-211`). The math lib is opened for Gusanos Lua
  (`src/gusanos/luaapi/context.cpp:132`). No script under `share/` calls
  `math.random`, so it is not reachable in a classic game. Note it anyway: a Lua
  script calling it would share the generator.

### 1.2 Fixed random table (`RandomNumbers[]`): deterministic, no state

- **Table.** `include/RandomNumberList.h:4-…` is
  `static const float RandomNumbers[256]`, with values in (-1, 1).
  `GetFixedRandomNum(uchar index)` returns `RandomNumbers[index]`
  (`src/common/Misc.cpp:84-87`).
- **Shots** (`shoot_t::nRandom`):
  - Set by `CShootList::addShoot`: `psShot->nRandom = GetRandomInt(255)`
    (`src/common/CShootList.cpp:100`). This is one libc `rand()` call, range 0..=255,
    sent to clients as a byte.
  - In `Proj_SpawnInfo::apply` (`src/common/PhysicsLX56_Projectiles.cpp:563-640`),
    each spawned projectile `i` of a shot does the following:
    1. `fixedRandomFloat()` = `table[nRandom]` for the spread (585).
    2. `nRandom = (nRandom+1) % 255` (598-601).
    3. `fixedRandomFloat()` for SpeedVar (606).
    4. `nRandom = (nRandom*5) % 255` (610-613).
    5. `random = fixedRandomIndex()` = `nRandom` (624).
    6. `SpawnProjectile(..., random, ...)`.
    7. `nRandom = (nRandom+1) % 255` (634-637).
  - Only the very first lookup can use index 255, when `GetRandomInt(255)` returned 255.
    After the first `% 255`, `nRandom` is always below 255.
- **Projectiles** (`CProjectile::iRandom`):
  - `SpawnProjectile` does `_random %= 255` (`src/client/CClient_Game.cpp:727`).
    `Spawn` sets `iRandom = _random` and `fTimeVarRandom = table[iRandom]`
    (`src/common/CProjectile.cpp:64-68`).
  - `getRandomFloat()` returns `table[iRandom++]`, then `iRandom %= 255`
    (`CProjectile.cpp:118-124`).
  - Consumers:
    - wallshoot time: `0.011f + getRandomFloat()/1000` (`CProjectile.cpp:101-102`, when `FT_LX56WallShooting`).
    - fast projectiles (|v|² ≥ 250000) in `CalculateCheckSteps`:
      `rnd = (iRandom % 3) * SIGN(getRandomFloat())` (`CProjectile.cpp:151-153`).
      It runs at spawn (74) and when the speed changes by more than 50000 in |v|²
      (`PhysicsLX56_Projectiles.cpp:435-438`).
    - the parent of spawned children: `fixedRandomIndex() = iRandom + 1`
      and `fixedRandomFloat() = getRandomFloat()` (`PhysicsLX56_Projectiles.cpp:504-519`).
    - the LX56 timer: `Time + TimeVar * fTimeVarRandom` (`PhysicsLX56_Projectiles.cpp:949-951`).

### 1.3 Other generators

- **Boost MT19937, three independent streams** (`src/util/math_func.cpp:3-6`, `math_func.h:12-31`):
  - `boost::mt19937 rndgen;` is default-seeded with 5489 and never reseeded
    (no `rndgen.seed` anywhere).
  - `boost::uniform_01<boost::mt19937> rnd(rndgen);` holds a **copy** of the engine.
    Boost 1.74's `backward_compatible_uniform_01` stores the engine by value. It
    returns `double(engine()) * 2^-32`, retrying while ≥ 1.
  - `midrnd` (a `variate_generator<mt19937, uniform_real<>>`) holds another copy.
  - Verified in the container: `rnd()` gives 0.81472369190305471 = 3499211612/2^32
    (the first MT output) even after `rndgen` had been advanced 3 times.
    So `rndgen`, `rnd` and `midrnd` are three separate MT19937(5489) streams.
  - Reachable in a classic game:
    - `rnd()` in `CMap::FindSpot` (`src/common/CMap.cpp:2381-2395`):
      `pos.x = float((double)rnd() * Width); pos.y = float((double)rnd() * Height);`
      up to 1000 tries until `IsGoodSpawnPoint`. It is used by:
      - server respawn without a given position (`src/server/CServer_Game.cpp:67,71`).
      - `SpawnBonus` (`CServer_Game.cpp:269`).
      - `FindSpotCloseToPos` (`CMap.cpp:2398-2430`, 101 calls).
      - `CHideAndSeek.cpp:117`.
      - the `findSpot` command.
    - `rndgen()` for the `ProxyPlayer` uniqueID (`src/common/CWorm.cpp:388`).
      That is only for remote worms when `needProxyWormInputHandler()`.
    - `rndInt(...)` for Gusanos weapons in `CWorm` init (`CWorm.cpp:365`). It only
      runs when `gusGame.weaponList` is non-empty, which is never the case for LX mods.
  - All other `rnd`/`rndInt`/`midrnd` users are Gusanos-only (`src/gusanos/*`).
  - **None of these is touched by `simSetSeed`.** Their state depends on every
    `FindSpot` since the process started.
- **`SyncedRandom`** (`include/MathLib.h:180-262`, `MathLib.cpp:252-255`): defined
  but **never instantiated** (grep finds only its definition). Ignore it.
- **Not used:** `std::random`, `std::random_shuffle`, `std::shuffle`, `drand48`.
  Breakpad's `random()` calls are compiled out (`BREAKPAD` defaults to `No`,
  `CMakeOlxCommon.cmake:37`; not enabled in the Dockerfile). HawkNL's `rand()` is
  in its win32 pthread shim only.
- **Libraries.** None of the linked libraries touches the libc `rand`/`random` state on
  the paths used:
  - libxml2 uses `rand_r` with a private seed.
  - curl and OpenSSL use their own generators.
  - libzip's `random()` fallback runs only when writing archives; OLX only reads
    downloaded mods.

---

## 2. Seeding, startup and threads

### 2.1 What `simSetSeed` does

- **Code** (`src/common/Command.cpp:2269-2275`):
  ```cpp
  int seed = from_string<int>(params[0], fail);
  srand((unsigned int)seed);
  ```
  It reseeds glibc `random()`'s global TYPE_3 state. Negative seeds wrap to `u32`,
  and 0 behaves as 1. Nothing else is reset: not the MT19937 streams, not the
  entity pool, not any static bot timers.
- **When it runs.** Commands from the dedicated-control pipe are read on a
  thread-pool thread (`src/server/DedicatedControl.cpp:157,172-181`). That thread
  only **queues** them: `Execute` pushes to `cmdQueue` under a mutex
  (`Command.cpp:2636-2639`). They execute on the game-loop thread at the start of
  `Game::frameInner()` (`HandlePendingCommands()`, `src/game/Game.cpp:731`;
  `Command.cpp:2646-2668`).

### 2.2 Recording sequence and RNG consumers (`oracle_control.py`)

1. `simSetSeed S`, `simStep 0`.
2. `startlobby`. The local client connects. `ChallengeTable_issue` may consume 2×
   `rand()` (`src/server/ChallengeTable.cpp:32`: `(rand() << 16) ^ rand()`) if the
   connect goes through `getchallenge` (`CServer_Parse.cpp:1083`).
3. `addBots n`: `CClient::AddRandomBots` calls `randomChoiceFrom(bots)`, 1 `rand()`
   per bot (`src/client/CClient.cpp:1976`).
4. `startgame` and the steps until Playing. When each worm is prepared, random
   weapons are picked **several times**. Each `GetRandomWeapons()` is 1 `rand()` per
   slot (`CWorm.cpp:802-803`).
   - `CWorm::Prepare`: `GetRandomWeapons()` (`CWorm.cpp:329`).
   - `initWeaponSelection` for bots (`src/common/CWormBot.cpp:4180-4188`):
     `GetRandomWeapons()`, plus `getRandomEnabledWpn()` (1 `rand()` each) for any
     empty slot.
   - `GameServer::PrepareWorm` with `ForceRandomWeapons`: `GetRandomWeapons()`
     again (`src/server/CServer.cpp:421-422`).
   - Other forced-random paths: `CWorm.cpp:526-527` and `Command.cpp:2168-2169`.
   - Bot AI may also start consuming during this stepping (§6).
5. `simSetSeed S` again. **The state is clean only from here on.**
6. `simLoadInputs`: no RNG. It installs `CWormHumanInputHandler` only for worms
   that appear in the schedule.
7. `spawnWorm id "x,y"` for each worm.
   - `GameServer::SpawnWorm` with an explicit position: no `FindSpot`, no RNG.
   - `CWorm::Spawn` calls `m_inputHandler->onRespawn()` (`CWorm.cpp:635-643`):
     - human handler: a no-op.
     - **bot handler:** `AI_Respawn()` → `findNewTarget()` (`CWormBot.cpp:993-997`).
       In Deathmatch it does not use RNG. The bot's later `AI_Think` →
       `findRandomSpot` does: up to 20 `GetRandomNum` (`CWormBot.cpp:1512-1540`).
8. `simRecord`, then `simStep frames`.

Consequences:
- The pre-game draws (bots, weapons) are messy and wall-clock dependent (§6).
  The Rust port should **not** try to reproduce them. Take the initial weapons
  from the scenario or the trace's frame −1.
- From the second `simSetSeed` on, the stream is deterministic only if no bot AI
  runs (§6).

### 2.3 Threads: no concurrency risk found

Threads in a dedicated server:
- the game-loop thread ("main loop", `src/MainLoop.cpp:366`), where all game code runs.
- "Ded pipe watcher": only queues commands.
- "main lock detector".
- timers (`Timer.cpp:204`).
- TaskManager.
- socket event checkers (`Networking.cpp:680-681`).
- the HTTP/curl threads.
- the stdin/tee threads.
- **bot pathfinding** (`CWormBot.cpp:520`).

Every `rand()` caller listed in §3 runs on the game-loop thread:
- The bot pathfinding search does not call any `GetRandom*`.
- `do_some_tests_with_fastTraceLine` (`CWormBot.cpp:980-985`, which uses `rand()`)
  is never called.
- glibc's `rand()` takes a lock internally, so even a concurrent caller could not
  corrupt the state, only interleave with it. No such caller exists.

The real nondeterminism risks are **wall-clock gating** and **network delivery
timing**, not threads (§3.4, §6, Open questions).

---

## 3. `rand()` call sites reachable in a classic dedicated-server game

### 3.1 Frame structure

**`Game::frameInner()`** (`src/game/Game.cpp:729-1000`) runs once per main-loop
iteration, even when the `simStep` budget is 0:
1. **[P0]** `HandlePendingCommands()` (731): `simSetSeed`, `spawnWorm`, etc.
2. **[P1]** `cClient->ReadPackets()` (827) parses server→local-client packets.
   **Shot lists are turned into projectiles here** (see §3.4).
3. **[P2]** `cServer->ReadPackets()` (837) parses connectionless packets
   (`getchallenge` → 2 `rand()`).
4. **[P3]** prepares new worms (845-890): weapon randomisation.
5. **[P4]** the fixed-step loop (917-957). It runs `manualStepBudget` frames, all of
   them inside **one** `frameInner` call. Each frame:
   1. `gusLogicFrame()`: classic mods use no libc RNG here.
   2. `OracleHooks::beforeFrame()`.
   3. **`cClient->Frame()` → `CClient::Simulation()`** (`src/client/CClient_Game.cpp:60-222`):
      - It loops over 84 Hz sub-steps: `goto simulateStart`, while
        `fLastSimulationTime + dt <= now`.
      - For each worm in `game.worms()` order:
        1. **[W1]** `simulateWorm` (114): input (human or **bot AI**), bleeding,
           carving, ninja rope.
        2. **[W2]** damaging material → `InjureWorm` (126).
        3. **[W3]** bonus pickup → `GiveBonus` (139).
        4. **[W4]** server only: `cServer->WormShoot` (158), then `WormShootEnd` (162).
        5. **[W5]** `PlayerShoot` (175): jetpack only.
        6. **[W6]** `LaserSight` (181): no RNG, but it fills the entity pool.
      - Then, after the worm loop:
        - **[E]** `SimulateEntities` is **skipped when `bDedicated`** (210-211).
        - **[PR]** `simulateProjectiles` (215).
        - **[B]** `simulateBonuses` (218): no RNG.
   4. **`cServer->Frame()` → `SimulateGame()`** (`src/server/CServer_Game.cpp:167-255`):
      - respawns: `SpawnWorm` → `FindSpot` (MT), plus the bot `onRespawn` (libc).
      - `SpawnBonus` (libc + MT).
      - Both are gated on `tLX->currentTime`, i.e. **the wall clock**.
   5. `OracleHooks::afterFrame()`.
6. **[P5]** `SendPackets` (975-990): the server sends the shoot list once
   `tLX->currentTime - startTime > 0` (`src/server/CServer_Send.cpp:558-570`;
   local client `iNetSpeed = NST_LOCAL`, delay 0.0, `CServer_Send.cpp:425`).

### 3.2 Entity pool rule (dedicated server)

- `SpawnEntity` (`src/client/Entity.cpp:160-205`) **evaluates its arguments first**,
  in the caller. `GetRandomVec()` in an argument therefore always consumes `rand()`,
  even in dedicated mode.
- `SpawnEntity` itself has no `bDedicated` guard:
  - It returns early only for `ENT_PARTICLE`/`ENT_BLOOD` when `!tLXOptions->bParticles`.
    The default is `true` (`src/client/Options.cpp:215`).
  - It also returns early for `ENT_GIB` with a NULL image.
  - Otherwise it takes a slot from `tEntities` (`FastVector<entity_t,1024>`, `Entity.cpp:31-32`).
- For **`ENT_GIB`**, if a slot was obtained, it calls
  `fabs(GetRandomNum())*20; (int)(fabs(GetRandomNum())*3)`: 2 more `rand()` (`Entity.cpp:198-201`).
- In dedicated mode `SimulateEntities` never runs (`CClient_Game.cpp:210-211`), so
  **slots are never freed**. The pool is cleared only at game preparation
  (`ClearEntities()`, `Game.cpp:545`; `Entity.cpp:101-108`).
- After 1024 accepted `SpawnEntity` calls, `getNewObj()` returns NULL
  (`include/FastVector.h:87-94`). From then on, **gibs stop consuming their extra
  2 `rand()`**.
- Every accepted `SpawnEntity` counts toward the 1024, random or not:
  - explosions and the `TRL_EXPLOSIVE`, smoke, chemsmoke and doomsday trails
    (`PhysicsLX56_Projectiles.cpp:1290-1316`).
  - **`ENT_LASERSIGHT` on every 84 Hz sub-step** for a worm holding a laser-sight
    weapon (`CClient_Game.cpp:180-181, 806-807`).
  - air-jump sparkles (`src/common/PhysicsLX56.cpp:390-392`).
  - `ENT_SPAWN` from `ParseSpawnBonus` (`src/client/CClient_Parse.cpp:1515`).
  - beams and explosions from `DrawBeam` (`CClient_Game.cpp:665,681,1053`).
  - particles, blood, blood droppers and gibs.
- Gib images: `bmpGibs` comes from `ChangeGraphics()` → `LoadGameImage("data/gfx/giblets.png")`.
  `LoadGameImage` has a dedicated-mode path that still creates a surface
  (`src/client/GfxPrimitives.cpp:2960-2967`). So gibs are probably non-NULL in
  dedicated mode (unverified; see Open questions).

### 3.3 Call-site table

Only sites that can run during a classic DM game on a dedicated server are listed.
- "Cosmetic" means the value is only used for a visual or sound effect.
- **Every** listed site still advances the shared stream, so every one of them
  matters for bit-exactness of later gameplay randomness.
- "Count" is the number of `rand()` calls.

| # | Phase | File:line | Expression | Count | Kind | Runs in dedicated? |
|---|---|---|---|---|---|---|
| 1 | W1 (`simulateWorm`) | `src/common/PhysicsLX56.cpp:319-329` | bleeding: `if(health<15 && isVisibleForEverybody() && GetPhysicsTime() > lastBlood+2.0f)` → `for(short i=0;i<amount;i++){ SpawnEntity(ENT_BLOOD,…,GetRandomVec()*100,…); SpawnEntity(ENT_BLOOD,…,GetRandomVec()*100,…); }`, `amount = (iBloodAmount/100.0f)*20` = 20.0 by default | 20×2×2 = **80** per bleed | cosmetic | yes (arguments are evaluated; physics time, deterministic) |
| 2 | W1 (carve) | `PhysicsLX56.cpp:334-337` → `src/common/CMap.cpp:2172-2199` (global `CarveHole(CVec)`) | finds the first `x` in `[(int)pos.x-2, (int)pos.x+2]` at row `y = clamp((int)pos.y)` with `PX_DIRT`, then `for n<3: SpawnEntity(ENT_PARTICLE,…,GetRandomVec().multPairwise(30,10),…)`. Checked **before** `CMap::CarveHole(3,…)` | **6** if dirt found, else 0 | cosmetic | yes (`ws->bCarve`, i.e. digging) |
| 3 | W1 (rope, `simulateNinjarope`) | `PhysicsLX56.cpp:590-608` | the hook attaches on a `destroyable` pixel and `firsthit` (`!rope->isAttached()` at the start of this possibly dt-halved call) → `for i<5: SpawnEntity(ENT_PARTICLE,…,GetRandomVec()*40,…)` | **10** | cosmetic | yes |
| 4 | W1 (`getInput`, bots only) | `src/common/CWormBot.cpp` (§6) | AI | varies | gameplay for the bot | yes, if any worm keeps a bot handler |
| 5 | W2 / any `InjureWorm` with `damage > 0` | `CClient_Game.cpp:450-459` | `amount = CLAMP(damage,10,100)*(iBloodAmount/100)`; `for(ushort i=0;i<amount;i++){ sp = GetRandomNum()*(50+damage*2); 3×SpawnEntity(…GetRandomVec()…) }` | **7×ceil(amount)** (10..100 iterations) | cosmetic | yes |
| 6 | `InjureWorm` → death (server side) | `CClient_Game.cpp:414-438` | when `w->injure()` kills a local worm: `cServer->killWorm` (419, no `rand()`), then `GetRandomInt(2)` (death sound, 423), then `for n<7: SpawnEntity(ENT_GIB,…,GetRandomVec()*80,…)` (427-428), then `amount=(int)(50*iBloodAmount/100)`=50: `for i<50 { GetRandomNum(); 3×GetRandomVec }` (431-437). Then the #5 blood loop runs too (same call, `damage > 0`) | 1 + 7×(2 + 2 if pool slot) + 50×7 = **365 or 379**, plus #5 | cosmetic | yes |
| 7 | W3 | `src/common/CWorm.cpp:1494-1500` | health bonus: `(float)(GetRandomInt(40)+10)`, then `InjureWorm(this, -health)` (negative damage → no blood) | **1** | **gameplay** | yes (bonuses off by default, `FeatureList.cpp:50`) |
| 8 | W4 | `src/server/CServer_Game.cpp:445` → `CShootList.cpp:100` | `WormShoot`: for each `CServerConnection` with status `!= NET_DISCONNECTED`: `addShoot(...)` → `nRandom = GetRandomInt(255)` | **1 per connected client** (1 in the oracle: the local client) | **gameplay** (seeds spread and speed variation) | yes, projectile and beam weapons only |
| 9 | W4 | `CServer_Game.cpp:360` | `WormShootEnd` (fire released or weapon changed while shooting, if `FinalProj.isSet()`): same `addShoot`, `release=true` | 1 per client | gameplay | yes |
| 10 | W5 | `CClient_Game.cpp:570` | jetpack `CVec(15,0) * GetRandomNum()` | 1 | cosmetic | only `WPN_SPECIAL` jetpack with `LastFire<=0` after `WormShoot` set it (ROF 0). **No jetpack in Classic** |
| 11 | PR → `Proj_DoActionInfo::execute` → `projectile_doExplode` / `doTimerExplode` → `CClient::Explosion` | `CClient_Game.cpp:227-330` | order: (a) find dirt at `x ∈ [px-2, px+1]` (`x<px+2`), `y=MIN((uint)pos.y,H-1)`; (b) if bonuses are enabled, bonuses within 15 px explode recursively (`Explosion(time,pos,15,5,owner)`, 268) **before** the particles; (c) `if(gotDirt) for x<2: SpawnEntity(ENT_PARTICLE,…,GetRandomVec().multPairwise(30,10),…)` (289-292); (d) `ENT_EXPLOSION`; `CarveHole`; (e) `InjureWorm` (#5/#6) for each alive worm within 5 px (`GetLength2() <= 25`) of its `posRecordings` | **4** if dirt, plus injuries | cosmetic (+ #5/#6) | yes |
| 12 | PR → `projectile_doMakeGreenDirt` → `CMap::PlaceGreenDirt` | `src/common/CMap.cpp:1231-1340` (rand at 1303) | for each mask pixel (row-major, clipped) with `pixel == green && (flag & PX_EMPTY)`: `gr = greens[GetRandomInt(3)]` | **1 per green pixel placed on empty material** | colour cosmetic, but the **count depends on gameplay state** | yes. The branch needs `bmpBackImageHiRes`, which the LXL loader creates even in dedicated mode (`MapLoader_LieroX.cpp:498`); `bmpGreenMask` comes from `greenball.png` (`CMap.cpp:367-368`). Classic has Greenball |
| 13 | PR → `Proj_SpawnInfo::apply` (projectile trails or spawns, `execute` 1189-1207), and P1 for shots | `PhysicsLX56_Projectiles.cpp:588-596` | `if(UseRandomRot && Proj->Rotating) rot = GetRandomInt(abs(360/Proj->RotIncrement)) * Proj->RotIncrement;` (`RotIncrement==0` is set to 1). `UseRandomRot` is true only for weapon shots (`include/ProjAction.h:97`, `WeaponDesc.h:92-93`) | **1 per spawned projectile** of a rotating type from a shot (Classic rotating: grenade, mine, cluster-bomb, chiquita, explosives, napalm, spikeballs, bouncy-mine, …) | cosmetic (start frame) | yes. **Happens even if the projectile pool is full** |
| 14 | after #13, `CClient::SpawnProjectile` → `CProjectile::Spawn` | `src/common/CProjectile.cpp:79-94` | for `PRJ_RECT/PIXEL/CIRCLE/POLYGON` with `Colour.size() > 0`: `GetRandomInt(Colour.size()-1)` | **1 per spawned projectile** of these types, **even with one colour** | cosmetic | yes, only if `cProjectiles.getNewObj()` succeeded (`CClient_Game.cpp:718-724`) |
| 15 | P1 (ReadPackets → `ParseSingleShot`/`ParseMultiShot` → `ProcessServerShotList` → `ProcessShot`) | `CClient_Parse.cpp:1917-1946`, `CClient_Game.cpp:813-923` | shots → `wpn->Proj.apply` (#13, #14 per projectile), `FinalProj.apply` for releases; beams → `ProcessShot_Beam` (926-…) → `Explosion` (1000, bonus) / `InjureWorm` (1031) (#5/#6) | per above | mixed | yes. **Timing:** see §3.4 |
| 16 | SimulateGame | `CServer_Game.cpp:265-297` | `SpawnBonus`: `FindSpot()` (MT `rnd`, 2 draws per try) → `GetRandomInt(999) >= chance*1000.0f` → … `randomChoiceFrom(goodWpns)` (**always**, even for health) | **2** libc + MT | **gameplay** | only if `FT_Bonuses` (default false). Wall-clock gated: `tLX->currentTime - fLastBonusTime > BonusFreq` |
| 17 | SimulateGame → `SpawnWorm` (respawn) → `CWorm::Spawn` → bot `onRespawn` | `CServer_Game.cpp:199-224` | bot `AI_Respawn` | bot-dependent | — | wall-clock gated respawn |

These sites exist but are **not reachable** in the oracle's classic DM dedicated game,
or run only before the second seed:
- `CClient_Game.cpp:841`: `DoLocalShot`, dead code with `assert(false)`.
- `CClient_Parse.cpp:1844-1858`: `ParseWormDown`. Never sent to the local client
  (`SendWormDied` returns for `isLocalClient()`, `CServer_Send.cpp:1124`).
  It is reachable for remote clients only.
- `src/client/Entity.cpp:455-461`: blood dropper drips, in `SimulateEntities`
  (skipped in dedicated mode).
- `Entity.cpp:507-517`: `EntityEffect::Process`, from `CWorm::Draw`.
- `src/client/CViewport.cpp:354-355`: screen shake, in `CClient::Draw`, which
  returns at `CClient_Draw.cpp:466` in dedicated mode.
- `GfxPrimitives.cpp:1882,1887`: laser drawing.
- `src/server/CTag.cpp:203`: Tag game mode only.
- `CServer.cpp:891`, `CClient.cpp:1118`, `CServer_Send.cpp:992-994`, `CClient_Send.cpp:284-299`:
  `FUZZY_ERROR_TESTING`, which is not defined.
- `src/common/CChannel.cpp:805-832`: a network robustness self-test.
- `src/common/sex.cpp:256`: the `sex` command.
- `CWpnRest.cpp:103`: menu randomise.
- `CMap.cpp:405`: random theme.
- `CWormHuman.cpp:915,922`: the "Random" button in the weapon menu.
- `CServer.cpp:1720-1734`: team assignment, non-local team games.
- `ChallengeTable.cpp:32`: only when a `getchallenge` packet arrives. It could
  arrive during a game only from an outside client.
- Pre-game only: `CClient.cpp:1976` (`AddRandomBots`), weapon randomisation (§2.2).

### 3.4 Ordering hazard: shot RNG vs projectile RNG

- **Where the shot's draw happens.** `nRandom` (#8) is drawn in the simulation
  phase W4 of frame *k*.
- **Where the projectile's draws happen.** The projectiles' `rand()` (#13, #14) is
  consumed in phase P1 of a later `frameInner`:
  1. The shoot list is sent in P5 only when `tLX->currentTime - startTime > 0`.
     `currentTime` is set once per `Game::frame()` (`Game.cpp:699`), so that is
     normally **not** in the same `frameInner`.
  2. It then crosses a real loopback UDP channel.
  3. It is parsed in a later P1.
- **Effect.** The projectile draws interleave with sim-phase draws (digging, rope,
  blood) of the frames run in between. The P1 processing happens before the next
  fixed-step loop.
- **Under `simStep N`,** all N frames run inside one `frameInner`. Shots from
  those frames are spawned only after the batch.
- The exact delivery delay is determined by wall time and the socket, so it is
  not a simulation property.

---

## 4. glibc 2.35 `rand()`, exactly

- `rand()` is `(int)__random()`. `random`/`srandom` share one global
  `struct random_data`, protected by a lock. The default is TYPE_3: degree 31,
  separation 3, `RAND_MAX = 2147483647`.
- **`srand(seed)`** (`__srandom_r`):
  ```
  if seed == 0: seed = 1
  r[0] = (int32)seed                  // seed >= 2^31 becomes negative here
  word = (int32)seed
  for i in 1..31:
      hi = word / 127773; lo = word % 127773        // C truncating division (Rust i32 '/' '%' match)
      word = 16807*lo - 2836*hi                     // computed in i64, fits in i32
      if word < 0: word += 2147483647
      r[i] = word
  fptr = 3, rptr = 0
  discard 310 outputs (kc = 31*10)
  ```
- **Each output** (`__random_r`):
  ```
  val = (u32)r[fptr] + (u32)r[rptr]   (wrapping)
  r[fptr] = val
  out = val >> 1                      // 0..=2^31-1
  fptr = (fptr+1) % 31; rptr = (rptr+1) % 31
  ```
  This is equivalent to `o[i] = o[i-31] + o[i-3]` (mod 2^32) over the extended
  sequence, with `rand() = o[i] >> 1` starting at i = 341.
- **Verification.** This reimplementation was checked against the container's glibc
  for 5,000,000 outputs on each of these seeds: 0, 1, 2, 42, 12345, 123456789,
  2147483647, 2147483648, 3000000000, 4294967295. All are identical
  (`randtest/t.cpp`, struct `Ref`).

### Expected values for Rust unit tests (glibc 2.35, x86-64)

```
RAND_MAX = 2147483647, HALF_RAND = 1073741823
srand(1):      1804289383 846930886 1681692777 1714636915 1957747793 424238335 719885386 1649760492 596516649 1189641421
srand(12345):  383100999 858300821 357768173 455528251 133005921 116285904 591987137 102557902 689413528 585691128
srand(0):      (same as srand(1))
srand(42):     71876166 708592740 1483128881 907283241 442951012 537146758 1366999021 1854614940 647800535 53523743
srand(4294967295): 254925627 1205188300 366127624 1401405153 76053476
srand(2147483648): 1336741213 1210407648 1447044896 337392383 82502902
1000th output (index 999):    srand(1) 1143565421, srand(12345) 121832119, srand(4294967295) 1892540048
1000000th output (index 999999): srand(1) 429357853, srand(12345) 1485618129, srand(4294967295) 949151631
```

---

## 5. Converting `rand()` to floats

- **Arithmetic.** All conversions are **f32**. x86-64 GCC uses SSE scalar
  arithmetic, so there is no extended precision. The build uses `-O3` without
  `-ffast-math` (`CMakeOlxCommon.cmake:369`).
- **Constants.**
  - `(float)RAND_MAX = 2147483648.0f` (2^31).
  - `(float)HALF_RAND = 1073741824.0f` (2^30).
  - So both divisions are exact power-of-two scalings of the rounded numerator.
- **Formulas.**
  - `GetRandomNum() = ((rn - 1073741823) as f32) / 1073741824.0f`. The int
    subtraction cannot overflow. The int→f32 conversion rounds to nearest, ties to
    even, like Rust `as f32`. The range is **[-1.0, 1.0] inclusive**:
    - `rn=0` and `rn=1` give -1.0.
    - `rn=1073741823` gives 0.0.
    - `rn=2147483647` gives 1.0.
  - `GetRandomPosNum() = (rn as f32) / 2147483648.0f`. The range is
    **[0.0, 1.0] inclusive**: any `rn >= 2147483584` rounds to 2^31 and gives 1.0.
  - `GetRandomInt(max) = clamp((PosNum * (max as f32 + 1.0)) as i32, 0, max)`.
    The f32 multiply rounds. The result can be `max` exactly, and is clamped
    when PosNum = 1.0. For max = 255, the ×256 is exact.
  - `GetRandomLong(max) = min((PosNum * ((max as f32) + 1.0)) as u64, max)`.
    `float(max)` is the u64→f32 conversion.
  - `GetRandomVec() = (cosf(a)*n, sinf(a)*n)`, where:
    - `a = (PosNum * 2.0f) * 3.14159274f`, i.e. `(float)PI`, left-to-right.
    - `n = GetRandomNum()`, drawn after `a`.
    - `cosf` and `sinf` are glibc 2.35 (already ported in `rl-math`).
    - `CVec * float` multiplies component-wise (`include/CVec.h:112`).

Sample outputs (container, `-O3`):
```
edge mapping (rn → Num, PosNum, Int(255), Int(2)):
  0          → -1,          0,           0,   0
  1          → -1,          4.65661287e-10, 0, 0
  64         → -0.99999994, 2.98023224e-08, 0, 0
  1073741823 →  0,          0.5,         128, 1
  1073741824 →  9.31322575e-10, 0.5,     128, 1
  2147483583 →  0.99999994, 0.99999994,  255, 2
  2147483584 →  0.99999994, 1,           255, 2
  2147483647 →  1,          1,           255, 2
srand(12345), fresh each line:
  GetRandomNum ×5:    -0.643209398(0xbf24a95f) -0.200645074(0xbe4d75e7) -0.666802406(0xbf2ab390) -0.575756252(0xbf1364c3) -0.876128554(0xbf6049f6)
  GetRandomPosNum ×5:  0.178395301(0x3e36ad42) 0.399677455(0x3ecca286) 0.166598782(0x3e2a98df) 0.212121874(0x3e59367a) 0.0619357079(0x3d7db04c)
  GetRandomInt(255) ×10: 45 102 42 54 15 13 70 12 82 69
  GetRandomInt(2) ×10:   0 1 0 0 0 0 0 0 0 0
  GetRandomInt(40) ×10:  7 16 6 8 2 2 11 1 13 11
  GetRandomVec ×3: (-0.0872566327,-0.180678517)[0xbdb2b39b,0xbe3903ca] (-0.288090765,-0.498496711)[0xbe9380a2,0xbeff3af6] (-0.825028419,-0.338315904)[0xbf533510,0xbead37be]
srand(1):
  GetRandomNum ×5: 0.680375457(0x3f2e2d16) -0.211234152(0xbe584dc4) 0.566198468(0x3f10f262) 0.596880078(0x3f18cd22) 0.823294699(0x3f52c371)
  GetRandomVec ×3: (-0.113395192,0.178217277)[0xbde83bbd,0x3e367e97] (0.12323951,-0.584018707)[0x3dfc64ff,0xbf158240] (-0.514059246,0.318816245)[0xbf039963,0x3ea33be2]
```

Boost (only for `FindSpot`, if it is ever ported):
- `rnd()` = `mt19937_out * 2^-32` (f64); the first value from the default seed is 0.81472369190305471.
- `FindSpot` x = `float((double)rnd() * Width)`.

---

## 6. Bots (`src/common/CWormBot.cpp`)

**Generator.** Bots use the **shared libc `rand()`**, both directly and through
`GetRandom*`:

| Line | Where | What |
|---|---|---|
| 1527-1528 | `findRandomSpot` | `(long)(fabs(GetRandomNum()) * w)`, `... * h`, up to 10 tries = ≤ 20 calls |
| 1688 | `AI_SetAim` | 3 `GetRandomNum` per refresh. A **static**, shared timer gated on `tLX->currentTime` (0.5 s wall-clock) |
| 1750 | `AI_SimpleMove` | 2 `GetRandomNum` |
| 1841 | `weaponCanHit` | 4 `GetRandomNum`, static, wall-clock 0.5 s |
| 2214 | `AI_Shoot` | `GetRandomInt(diff) * SIGN(GetRandomNum())`, wall-clock 0.5 s |
| 2579, 2622 | `AI_GetBestWeapon` | `GetRandomInt(nWeapons-1)` |
| 4181, 4187 | `initWeaponSelection` | random weapons |
| 4234-4236 | `subThink` | raw `rand() % 8`, `% 15`, `% 50`. Only from `CWormInputHandler::think()` when `gusEngineUsed()` (`src/game/WormInputHandler.cpp:98-108`), so **not in classic** |
| 982-985 | `do_some_tests_with_fastTraceLine` | raw `rand()`. Never called |

**Bot timing reads the wall clock.**
- `getInput` returns early while `tLX->currentTime - fSpawnTime < 0.4f`
  (`CWormBot.cpp:1035-1037`). This mixes the wall clock with physics time.
- The think interval is 3 s wall-clock (1046-1051).
- The static refresh timers are wall-clock too.
- So **any worm that keeps a bot input handler during a recording makes the
  `rand()` stream nondeterministic**: the number of calls per frame depends on
  real time.

**Oracle impact.**
- `OracleHooks::loadInputs` installs the human handler only for worms named in the
  schedule. `oracle_control.py` writes only the scenario's `inputs`.
- **`fixtures/oracle/fall.ron` has no `inputs`**, so its single worm stays a bot during
  the recording. Its `spawnWorm` → `onRespawn` → `AI_Respawn` runs right after the
  second `simSetSeed`, and the AI then runs every sub-step.
- `fall` passes today only because nothing in it reads the RNG. It must be fixed
  before any RNG-sensitive scenario (or any check of RNG state) relies on this pattern.

---

## Recommendations for the Rust port

1. **Where the generator lives.**
   - Put one `GlibcRand` (TYPE_3 state: `[i32; 31]` + `f`, `r` indices) in the
     **simulation state**, not a global, in a `no_std` crate. It could be `rl-math`
     next to the ported `sinf`/`cosf`, or a new `rl-rand` in the same layer.
   - API: `seed(u32)`, `next() -> i32` (0..=RAND_MAX), plus the OLX wrappers
     `num()`, `pos_num()`, `int(max)`, `long(max)`, `vec()`, `choice_index(len)`.
     Each wrapper cites `MathLib.cpp` lines.
   - Unit-test it with the values in §4 and §5.
   - The state must be part of snapshot/replay. That is 33 words, so snapshots are cheap.
2. **Seed** the generator with the scenario seed at the point matching the
   **second** `simSetSeed`, before spawning. Do not emulate the pre-game draws
   (bots, weapon randomisation, challenge). Take the initial weapon slots from the
   scenario or the trace's frame −1.
3. **Emulate every `rand()` consumer in §3.3, rows 1-3 and 5-16, cosmetic ones
   included.** They must run in OLX's order: worm loop W1…W6 per 84 Hz sub-step,
   then projectiles, then the server's SimulateGame.
   - The cosmetic sites can be "draw and discard". The Rust renderer may reuse the
     values for its own particles, but must not draw extra numbers from this
     generator. Use a separate, non-simulation RNG for any extra visual
     randomness.
   - Hidden details to preserve:
     - `GetRandomInt(0)` still draws.
     - The projectile colour is drawn even with one colour, but only if a
       projectile slot was obtained.
     - The rotation draw happens even when the pool is full.
     - The dirt-particle checks scan **before** carving. Explosion scans
       `x ∈ [px-2, px+1]`, digging scans `[px-2, px+2]`.
     - Blood loop counts are `ceil(float)`.
     - The server-side death path runs both blood loops.
     - `SpawnBonus` always draws the weapon choice.
     - `addShoot` draws once **per connected client**. Model "1" for local play.
       For future netplay, decide explicitly.
4. **Emulate the dedicated entity-pool counter.** It only gates the 2 extra gib
   draws, and it is cheap:
   - a saturating counter (cap 1024), reset at game preparation.
   - Increment it on every `SpawnEntity` that passes the filters, cosmetic ones too:
     laser sight per sub-step, trails, explosions, sparkles, `ENT_SPAWN` on bonus
     spawn, and so on.
   - This is a deliberate faithfulness artefact of OLX's dedicated mode, and it
     differs from a client with graphics. Record the choice in `docs/porting/STATUS.md`.
5. **Keep the fixed table** `RandomNumbers[256]` as a `static [f32; 256]`, and the
   exact `nRandom` / `iRandom` index arithmetic from §1.2. It needs no state beyond
   the per-shot and per-projectile index.
6. **Skip:**
   - `SyncedRandom` (unused).
   - Lua `math.random`.
   - Gusanos `rnd`/`rndInt`/`midrnd`.
   - The fuzzy and network-test RNG.
   - Viewport shake, entity-simulation drips and `EntityEffect` (not run on a
     dedicated server).
   - Bot AI exactness: bots are wall-clock driven in OLX, so they cannot be
     oracle-tested. If a Rust bot ever exists, it must draw from a **separate**
     RNG, or deliberately from the shared one with no oracle claim.
   - The MT19937 streams for `FindSpot` (spawn and bonus positions). Port them
     only if spawn/bonus placement ever needs oracle exactness. Even then, they
     carry state from process start, so prefer explicit positions in scenarios.
7. **Oracle changes worth making** (in `tools/olx-oracle`, separately):
   - Make `simLoadInputs` install the human handler for **every** worm, or give
     `fall.ron` an `inputs` entry, so no bot AI runs during a recording.
   - Add an RNG probe to each trace frame, e.g. `"rc"`: `rand()` calls since the last seed.
     - Implementation option 1: an `LD_PRELOAD` or `-Wl,--wrap=rand` counter that `OracleHooks` reads.
     - Implementation option 2: snapshot the glibc state, after first calling `setstate` to switch it to a buffer OracleHooks owns.
     - Either way, an RNG desync shows up at the exact frame, instead of much later through some gameplay value.
   - For weapon scenarios, step with `simStep 1` per frame, with a pause or a
     packet flush in between, so that shot-list delivery (§3.4) is deterministic.
     Alternatively, add a hook that processes the local shoot list synchronously.

## Open questions

1. **Shot delivery latency (§3.4).** Under manual stepping, how many fixed frames
   pass between `WormShoot` (draw #8) and `ProcessShot` (draws #13, #14)?
   - Analysis says the earliest is the second `frameInner` after the shot.
     `SendPackets` needs `tLX->currentTime` to advance; then it crosses loopback UDP.
   - With `simStep N > 1`, the delay is the whole batch.
   - It needs an experiment (log `serverFrame` in `addShoot` and in
     `ProcessServerShotList`). It also decides when projectiles appear in traces.
   - The Rust port will probably need to model a fixed delivery delay to match,
     or the oracle needs a synchronous-delivery hook.
2. **Gib images in dedicated mode.** Is `bmpGibs` non-NULL? That decides whether
   gibs take a pool slot and draw 2 more. Check with a scenario that kills a worm
   plus the RNG probe, or log `getGibimg().get()` once.
3. **`bParticles` in the container's options.** It defaults to true; confirm that
   the dedicated server's `cfg/options.cfg` does not override it. It gates the
   pool accounting of `ENT_PARTICLE`/`ENT_BLOOD`, not the argument draws.
4. **Wall-clock gated server logic.** Respawn (`tLX->currentTime >
   timeOfDeath + RespawnTime`, where `timeOfDeath` is physics time) and bonus
   spawning compare the wall clock with physics time. Scenarios with deaths or
   bonuses will not be reproducible until the oracle patch pins `tLX->currentTime`
   to simulation time during manual stepping. This is not an RNG question, but it
   changes when the `rand()` draws in rows 16-17 happen.
5. **`getchallenge`.** Could a stray UDP packet reach port 23400 inside Docker
   during a recording? It would draw 2 `rand()` in P2. This is low risk with no
   published port.
6. **Pool counter start value.** Do any `SpawnEntity` calls happen between
   `ClearEntities` (game preparation) and frame 0 of the recording? For example
   `ENT_SPAWN` from `ParseSpawnBonus`, or laser sights during the stepping to
   Playing. The RNG probe (Recommendation 7) or a one-off log of
   `tEntities.size()` at `simRecord` would settle it.
