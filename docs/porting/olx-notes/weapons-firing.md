# M4 research 1: weapons, from the fire key to projectiles in the list

All OLX citations are at commit `d1ad953f` (tree at
`scratchpad/openlierox`). Scope: the classic LX56 engine in a local game, as recorded by the
oracle: a dedicated server whose local client owns every worm.

---

## 0. Key findings

1. **No shot reaches a projectile inside the frame loop.** Firing is split between two objects
   that share the same `CWorm` and `wpnslot_t` instances:
   - **In the physics sub-step (both objects):**
     - the server's `GameServer::WormShoot` gates fire on ROF, drains charge, and appends a
       `shoot_t` to every connection's `CShootList`;
     - the client's `CClient::PlayerShoot` handles specials (jetpack) and beam drawing.
   - **Later, over the network:** the shot list is serialized and sent through the reliable UDP
     channel (`GameServer::SendUpdate`). The client reads it back (`CClient::ReadPackets` →
     `ParseSingleShot`/`ParseMultiShot` → `ProcessServerShotList` → `ProcessShot` →
     `Proj_SpawnInfo::apply` → `SpawnProjectile`). This happens at the start of a later
     main-loop iteration, before that iteration's frames run.
2. **In real play, delivery takes 1–2 frames and depends on the wall clock:**
   - `SendUpdate` sends a non-empty list only if `tLX->currentTime − list.startTime > 0`.
     The start time is the wall-clock `tLX->currentTime` of the first `addShoot`, and
     `tLX->currentTime` is fixed for the whole main-loop iteration.
   - So a list started in iteration I is sent at the end of I+1 and processed at the start of I+2.
3. **The projectile's own clock starts at `shoot_t::spawnTime()`, which reads the wall clock:**
   `tLX->currentTime − (game.serverTime() − shot.fTime)`. Projectiles then catch up from that
   time to `GetPhysicsTime()` in 11 ms steps. So the spawn time decides bit for bit when a
   projectile first moves.
4. **The current oracle cannot see projectiles at all.**
   - `oracle_control.py` runs the whole recording as one `simStep N`, so a single
     `Game::frameInner` call runs all N frames. The network is pumped only before and after
     those frames, and the shot list is never sent during them (its start time equals the
     batch's `currentTime`).
   - What the current traces can already verify:
     - the weapon slot state (`ch`, `rl`, `lf`): charge, reload and ROF timing;
     - DrawBeam's terrain carving (`mh`, `dirt`).
   - What they cannot show: projectiles, recoil and beam damage. Section 13 proposes a
     deterministic delivery hook.
5. **The encoding is lossy and has quirks** (section 4). The port must reproduce the
   write/read round trip:
   - positions → `(short)` truncated, 12 bits;
   - worm velocity → `(short)`;
   - angle → `(int)` degrees;
   - multi-shot packets: every shot after the first gets the first shot's position and random
     seed, and loses velocity increases;
   - time → a float round trip.
6. **Beams in a host or local game behave unexpectedly:**
   - `DrawBeam`, called from client `PlayerShoot` every sub-step while `LastFire > 0`, really
     carves terrain and adds dirt count.
   - When the shot packet arrives, `ProcessShot_Beam` sets the shared slot's `LastFire = 0.3`.
     That throttles the server's fire gate, so a beam fires in bursts. It can also switch the
     worm's current weapon back to the beam.
7. **Jetpack (`SPC_JETPACK`) never thrusts in a host or local game if its ROF > 0:**
   - the server code sets `LastFire = ROF` first in the same sub-step, and `PlayerShoot` then
     returns;
   - with ROF ≤ 0 it thrusts, but drains charge twice per sub-step.
   - Neither Classic nor MW 1.0 has a special weapon.
8. **`rand()` is global glibc `rand()`** and is consumed by:
   - gameplay-relevant calls: shot seed byte, projectile colour, initial rotation;
   - "cosmetic" calls whose arguments are evaluated even in dedicated mode: dig and rope
     particles, blood, jetpack spray, gibs.

   Section 8 has the inventory.

---

## 1. Call chain and timing in a local dedicated game

Main loop (`Game::frame`, `src/game/Game.cpp:692-718`):
`tLX->currentTime = GetTime()` (wall clock, ms), `tLX->fDeltaTime = …`, then `frameInner()`.

`Game::frameInner` (`src/game/Game.cpp:723-995`), simplified:

```
HandlePendingCommands(); DedicatedControl::GameLoop_Frame()   // oracle commands (simStep...) run here
...
cClient->ReadPackets();          // 827  ← shot packets parsed here → ProcessShot → projectiles spawned
cServer->ReadPackets();          // 838
prepare worms if needed          // 842-895 (weapon selection at match start, §9)
tLX->fDeltaTime = tLX->fRealDeltaTime = TimeDiff(FixedFrameTime=10)   // 904-905, restored at 958
while(manualStep ? budget>0 : simulationTime < tLX->currentTime) {    // 917
    if Playing: serverFrame++                                       // 942-943
    { GusSpeedScope; gusLogicFrame(); }                              // 946-949 (vel round trip)
    [oracle patch: OracleHooks::beforeFrame()]                       // latches keys
    cClient->Frame()   → CClient::Simulation()                       // 951
    cServer->Frame()   → SimulateGame(), CheckTimeouts()             // 952-953
    [oracle patch: OracleHooks::afterFrame()]                        // writes trace line
    manualStepBudget--; simulationTime += frameDt                    // 955-956
}
...
cClient->SendPackets(); ...; cServer->SendPackets() → SendUpdate() → shot lists   // 979-988
```

`CClient::Simulation` (`src/client/CClient_Game.cpp:60-222`) runs one iteration per LX56
sub-step (`simulateStart` loop). In each sub-step, **for each worm in `game.worms()` order**:

```
wasShootingBefore = w->tState.bShoot;                           // 108
oldWeapon = w->getCurWeapon()->weapon();                        // 109
PhysicsEngine::simulateWorm(w, w->getLocal());                  // 114: getInput, physics, simulateWormWeapon (§3.1)
if(game.gameOver) continue;                                     // 116
if alive: damaging material → InjureWorm; bonus pickup          // 119-152
if(game.isServer()) {                                           // 155
    if(bShoot && alive) cServer->WormShoot(w);                  // 157-158  (§3.2)
    if(oldWeapon && ((wasShootingBefore && !bShoot) || (wasShootingBefore && oldWeapon != curWeapon)))
        cServer->WormShootEnd(w, oldWeapon);                    // 161-162 (FinalProj; no-op for LX56)
}
if(alive) {                                                     // 167
    if(bShoot) PlayerShoot(w);                                  // 170-176 (§3.4)
    if(curWeapon && LaserSight && !Reloading) LaserSight(w, angle); // 179-181 (entity only)
    (hide&seek vision cone)
}
(tag time)
```

After all worms in the sub-step:

```
if(!bDedicated) SimulateEntities(orig_dt);                      // 210-211 (non-dedicated only, consumes rand!)
if(shouldDoProjectileSimulation()) simulateProjectiles(...);    // 214-215 (dedicated default: true, Options.cpp:278)
simulateBonuses(...);                                           // 218
fLastSimulationTime += frame_dt;                                // 220
```

**Server or client object, and is anything simulated twice?** In 0.59 there is one
`game.worms()` list shared by `GameServer` and `CClient` in the same process. Local worms'
updates are not sent over the network (comment at `CClient_Game.cpp:154`). So worms and
projectiles are simulated once, in the client's `Simulation`:

| Code | Object | When |
|---|---|---|
| `simulateWormWeapon` (reload, ROF countdown) | client (`PhysicsEngine`) | each sub-step, inside `simulateWorm` |
| `WormShoot` (fire gate, ROF, drain, `addShoot`) | server | each sub-step, right after the worm's physics |
| `PlayerShoot`, `ShootSpecial`, `DrawBeam` | client | each sub-step, right after `WormShoot` |
| `SendUpdate` (shot list serialization) | server | end of main-loop iteration |
| `ParseSingleShot`/`ParseMultiShot` → `ProcessShot*` → `Proj_SpawnInfo::apply` → `SpawnProjectile` | client | start of a later main-loop iteration (`ReadPackets`) |
| projectile simulation | client | each sub-step, after all worms |

So the server code and the client code both mutate the same slot (`Charge`, `LastFire`,
`Reloading`) in the same sub-step, server first.

### 1.1 When are shots processed? (critical)

These steps assume real play, a non-empty list, and the local client connection.

1. **Fire.** In iteration I, frame F, the physics sub-step runs `WormShoot` → `addShoot`
   (`CShootList.cpp:80-112`). If the list was empty, `m_fStartTime = tLX->currentTime`
   (wall clock at the start of iteration I).
2. **End of iteration I.** `GameServer::SendPackets` (`src/server/CServer.cpp:859-900`) calls
   `SendUpdate` (`src/server/CServer_Send.cpp:422-576`). Its shot-list block (`556-569`) is:

   ```
   float delay = shootDelay[cl->getNetSpeed()];   // {0.010f,0.005f,0.0f,0.0f}[3] = 0.0f (local client: NST_LOCAL, CClient.cpp:305-309)
   if(tLX->currentTime - sh->getStartTime() > delay && sh->getNumShots() > 0) {   // TimeDiff > float → TimeDiff(0.0f)=0 → "> 0 ms"
       sh->writePacket(&shootBs, clientVersion); sh->Clear();
       cl->getChannel()->AddReliablePacketToSend(shootBs);
   }
   ```

   Since `currentTime` has not changed during iteration I, `0 > 0` is false and nothing is sent.
3. **End of iteration I+1.** The wall clock has moved, so the whole list is sent. That is
   every shot added in iterations I and I+1. `CChannel3::Transmit` sends reliable data
   immediately (`src/common/CChannel.cpp:1104-1258`) over loopback UDP.
4. **Start of iteration I+2.** `cClient->ReadPackets()` → `CClientNetEngine::ParsePacket`
   (`CClient_Parse.cpp:468+`, cases 585/590) → `ParseSingleShot`/`ParseMultiShot`
   (`CClient_Parse.cpp:1918-1946`). The client list is re-read (`readSingle`/`readMulti`), then
   `ProcessServerShotList` runs: each shot is processed **with `sh->spawnTime()`**
   (`CClient_Game.cpp:813-824`).
5. **The same iteration's frames then run**, starting with `serverFrame++` and the Gus
   velocity round trip. Recoil was applied before them, in step 4.

With the default `MaxFPS = 95` (`Options.cpp:255`), most iterations run exactly one 10 ms frame,
so:

- **Real play (idealized: one frame per iteration):**
  - shots fired during frames F and F+1 form one list, which is sent after frame F+1 and
    processed before frame F+2;
  - at processing time, `serverFrame = F+1` and `GetPhysicsTime() = T(F+2)`;
  - `tLX->currentTime` is in `(T(F+2), T(F+2)+10]`, because the frame loop runs frame F+2 only
    if `simulationTime < currentTime`.
  - Continuous fire therefore produces lists of two shots per worm, which go out as
    `S2C_MULTISHOOT` when consecutive shots in the list share worm and weapon (see §4).
- **The oracle today:** one `simStep N` makes one `frameInner` with N frames, so nothing is sent
  during the recording. The batch after it would deliver them, but with wall-clock spawn
  times that bear no relation to `simulationTime`. `simulationTime` was initialized from
  `GetTime()` at `Game.cpp:670` and then advances 10 ms per manual frame. Projectiles from such
  a delivery would freeze or jump arbitrarily.

### 1.2 `shoot_t::spawnTime` (`src/common/CShootList.cpp:715-731`)

```
AbsTime fSpawnTime = tLX->currentTime - TimeDiff(((float)cClient->getMyPing() / 1000.0f) / 2.0f);
AbsTime time = fSpawnTime;
if(cClient->getServerVersion() >= OLXBetaVersion(0,57,8))          // always true locally
    if(this->fTime <= game.serverTime())                             // serverTime() = TimeDiff(serverFrame*10), Game.h:111
        time = tLX->currentTime - (game.serverTime() - this->fTime); // AbsTime - TimeDiff (u64 ms)
return time;
```

- `fTime` is the decoded `shoot_t::fTime` (§4). It is the server's `game.serverTime()` when the
  shot was added (`WormShoot`: `TimeDiff time = game.serverTime()`, `CServer_Game.cpp:435-437`).
- The ping fallback (`getMyPing`, wall-clock measured) applies only if
  `fTime > serverTime()`. That happens only through the float round trip error at
  `serverFrame ≥ 819205` (about 2.3 h of play; computed, see §4.3). The port can ignore it and
  should note that.

### 1.3 How the projectile clock uses the spawn time

This belongs to the projectile area; it is noted here because it decides bit-exactness.

- `CProjectile::Spawn` sets `fLastSimulationTime = fSpawnTime = time` (`CProjectile.cpp:68-70`).
- `LX56_simulateProjectile` (`PhysicsLX56_Projectiles.cpp:1378-1397`) steps the projectile
  while `fLastSimulationTime + frame_dt <= GetPhysicsTime()`. Each step first adds `frame_dt`
  to `fLastSimulationTime`. `frame_dt` is `LX56PhysicsDT * (1/GameSpeed)`, so 11 ms.
- Projectiles are simulated only inside `CClient::Simulation`'s sub-step loop. In a frame with
  no sub-step (frames 0, 1, 12 … after alignment) no projectile moves, and the catch-up happens
  at the next sub-step.

---

## 2. The fire key (`CWormHumanInputHandler::getInput`, `src/common/CWormHuman.cpp:81-591`)

Already ported (`rl-sim/src/physics.rs::get_input`). The firing-relevant lines:

```
// 401-403
const bool allowCombo = cClient->getGameLobby()[FT_WeaponCombos];
ws->bShoot = cShoot.isDown() || tFire;          // latched: down now or pressed since last reset
// 405-458: weapon change only if slots>0 && (!bShoot || allowCombo); SelectWeapon+left/right,
//          quick keys cWeapons[i] (not driven by the oracle), Prev/NextWeapon (not driven)
// 575-590: cShoot.reset() etc.
```

- `getInput` runs once per sub-step and only for a local worm with `bWeaponsReady`, no menu, no
  chat and no console (`PhysicsLX56.cpp:300-307`).
- A dead worm's `getInput` calls `clearInput()`, which resets `tState`, so `bShoot` is false
  (`CWormHuman.cpp:115-120`, `CWorm.cpp:493-501`).

---

## 3. Per-sub-step weapon code

`dt` everywhere below is `LX56PhysicsDT.seconds()` = `TimeDiff(1000/84=11).seconds()` =
`11 * 0.001f` (u64 → f32, then multiply) = 0.0110000009f.

### 3.1 `simulateWormWeapon` (`src/common/PhysicsLX56.cpp:471-498`)

It is called at the end of `PhysicsEngine::simulateWorm` (`466`), after the worm's movement,
and only for alive worms (`simulateWorm` returns at `311` if dead). Gusanos physics would also
reach it, but is out of scope.

```
static void simulateWormWeapon(float dt, CWorm* worm) {
    if(worm->tWeapons.size() == 0) return;
    if((bool)FT_GameSpeedOnlyForProjs) dt *= (float)FT_GameSpeed;          // 475-476
    wpnslot_t *Slot = worm->writeCurWeapon();   // &slots[MIN(size-1, (size_t)iCurrentWeapon)]  (CWorm.h:546-549)
    if(!Slot->weapon()) return;                 // WeaponId <0 or >= NumWeapons → NULL (CWorm.cpp:127-132)
    if(Slot->LastFire > 0)
        Slot->LastFire -= dt;                                               // 484-485
    if(Slot->Reloading) {
        if((int)FT_LoadingTime == 0)
            Slot->Charge = 1.f;
        else
            Slot->Charge += dt * (Slot->weapon()->Recharge * (1.0f/((int)FT_LoadingTime * 0.01f)));   // 491
        if(Slot->Charge >= 1.f) { Slot->Charge = 1.f; Slot->Reloading = false; }
    }
}
```

**Float order at line 491:**

```
t = (float)(int)LT * 0.01f
u = 1.0f / t
r = Recharge * u
Charge = Charge + dt * r
```

With LT = 100: `100 * 0.01f` = 1.0f exactly after rounding, so `u` = 1.0f.

**Quirks:**

- **Only the current slot reloads.** Other slots' charge and `LastFire` are frozen.
- **`LastFire` is only decremented while > 0.** A negative residue stays, and the next shot
  overwrites it with `ROF`; nothing carries over.

### 3.2 `GameServer::WormShoot` (`src/server/CServer_Game.cpp:366-466`)

```
if(gusEngineUsed) return;  if(game.gameOver) return;
if(!game.gameMode()->Shoot(w)) return;            // CGameMode default true (CGameMode.h:46); HideAndSeek false, Race custom
if(w->tWeapons.size() == 0) return;
wpnslot_t *Slot = w->writeCurWeapon();
if(Slot->Reloading) return;
if(Slot->LastFire>0) return;
if(!Slot->weapon()) return;
Slot->LastFire = Slot->weapon()->ROF;                                       // 392
if(Type != WPN_PROJECTILE && Type != WPN_BEAM) {     // specials: charge bookkeeping only
    Slot->Charge -= Drain / 100;                     // float / int → Drain/100.0f
    if(Slot->Charge <= 0.f) { Slot->Charge = 0.f;
        if(gameSettings[FT_DisableWpnsWhenEmpty]) { Slot->WeaponId = -1; SendWeapons(NULL, w); }
        else Slot->Reloading = true; }
    return;
}
float Angle = w->getAngle();                                                 // 415
if(w->getFaceDirectionSide() == DIR_LEFT) Angle = 180 - Angle;               // 180.0f - Angle
if(Angle < 0)   Angle += 360;
if(Angle > 360) Angle -= 360;
if(Angle == 360) Angle = 0;
float speed = 0.0f;
if(Type == WPN_PROJECTILE) { CVec vel = w->getVelocity(); speed = NormalizeVector(&vel); }  // only affects encoding flags
TimeDiff time = game.serverTime();      // (isClient && ownServerTime → no)
for each connected CServerConnection cl:                                     // 440-446
    cl->getShootList()->addShoot(Slot->weapon()->ID, time, speed, (int)Angle, w, false);
Slot->Charge -= Slot->weapon()->Drain / 100;                                 // 456
if(Slot->Charge <= 0.f) { Slot->Charge = 0.f;
    if(gameSettings[FT_DisableWpnsWhenEmpty]) { WeaponId = -1; SendWeapons } else Slot->Reloading = true; }
```

- **`nAngle = (int)Angle` truncates.** Facing right with angle −0.5° gives 359.5 → 359. Facing
  left with angle 60° gives 120.
- **`addShoot` is called once per connected client, and each call consumes one `rand()`**
  (§4.1). The dedicated oracle and a local game have exactly one connection: the local
  client.
- **The fire interval with ROF r > 0.** If the sub-step that fires sets `LF = r`, the next
  shot comes `n` sub-steps later, where `n` is the smallest n ≥ 1 for which the sequential f32
  subtraction `r − dt − dt − …` (n times) is ≤ 0. With r ≤ 0 the worm fires every sub-step.
  Beams also decrement in `PlayerShoot` (§3.4).
- **Drain does not depend on time** (comment at `450-453`).
- **A full shot list loses the shot.** If the list is full (256, `MAX_SHOOTINGS`), `addShoot`
  returns false before `GetRandomInt`, but charge is still drained.

### 3.3 `GameServer::WormShootEnd` (`CServer_Game.cpp:319-362`)

- It runs only if `wpn->FinalProj.isSet()`. LX56 (v7) mods never load `FinalProj`; only
  versions > LX56 do (`CGameScript.cpp:779-780`).
- So for Classic and MW it is a no-op: no state change, no `rand()`.
- When set, it adds a shot with `release = true`, and `ProcessShot` applies `FinalProj`.

### 3.4 `CClient::PlayerShoot` (`src/client/CClient_Game.cpp:496-544`)

It runs in the same sub-step, right after `WormShoot`.

```
if(w->tWeapons.size() == 0) return;  if(gusEngineUsed) return;
wpnslot_t *Slot = w->writeCurWeapon();
if(Slot->Reloading) return;
if(!Slot->weapon()) return;
if(Type == WPN_BEAM) {                                    // 515-521
    if(Slot->LastFire > 0) {
        DrawBeam(w);                                      // §6.2 — carves terrain, NOT cosmetic
        Slot->LastFire -= tLX->fDeltaTime.seconds();      // = 10*0.001f = 0.0100000007f inside the frame loop
    }
    return;
}
if(Slot->LastFire > 0) return;          // for projectile/special weapons on a host: WormShoot just set it
Slot->LastFire = Slot->weapon()->ROF;
if(Type == WPN_SPECIAL) { ShootSpecial(w); return; }      // §7
return;
```

- **Projectile weapons:** a no-op after a server shot (`LastFire > 0`) when ROF > 0. With
  ROF ≤ 0 it re-assigns the same value.
- **Beams:** while the fire key is held, not reloading and `LastFire > 0`, it calls `DrawBeam`
  and then decrements `LastFire`. `tLX->fDeltaTime` is forced to 10 ms inside the frame loop
  (`Game.cpp:904-905`). Note that `10 * 0.001f` = 0.010000000707805157, not 0.01f.

### 3.5 `CClient::LaserSight` (`CClient_Game.cpp:751-808`)

- It runs only for weapons with `LaserSight` that are not reloading.
- There is no gameplay effect beyond `w->setTarget(true)` (bot AI) and
  `SpawnEntity(ENT_LASERSIGHT)`.
- **Port note:** in dedicated mode, entities are never simulated, so never freed. Every
  `SpawnEntity` permanently fills the 1024-slot entity pool (`Entity.h:30`,
  `Entity.cpp:175-178`). After that, `SpawnEntity(ENT_GIB)` no longer reaches its internal
  `GetRandomNum()` calls (`Entity.cpp:198-200`). This matters to the death and gib area's
  `rand()` count, not to firing.

---

## 4. The shot list: building, encoding, decoding

### 4.1 `CShootList::addShoot` (`src/common/CShootList.cpp:80-112`)

```
if(m_nNumShootings >= MAX_SHOOTINGS(256)) return false;
if(m_fStartTime == AbsTime()) m_fStartTime = tLX->currentTime;      // wall clock (send gate)
shoot_t *s = &m_psShoot[m_nNumShootings++];
s->cPos     = pcWorm->getPos();                                       // after this sub-step's movement
s->cWormVel = gameSettings[FT_ProjRelativeVel] ? pcWorm->getVelocity() : CVec(0,0);
s->fTime    = fTime;                                                  // TimeDiff(serverFrame*10)
s->nAngle   = nAngle;
s->nRandom  = GetRandomInt(255);                                      // ONE rand(); value in [0,255]
s->nSpeed   = gameSettings[FT_ProjRelativeVel] ? (int)(fSpeed*100) : 0;   // never used by apply()
s->nWeapon  = weaponID;  s->nWormID = pcWorm->getID();  s->release = release;
```

### 4.2 Writing (`writePacket` 119-141, `writeSingle` 146-196, `writeMulti` 201-306, `writeSmallShot` 311-431)

**`writePacket`** writes nothing for 0 shots, `writeSingle(0)` for 1 shot, and `writeMulti(0)`
otherwise.

**`writeMulti(index)` splits the list into runs.** A run starts at `index` and grows over the
following shots i while all of these hold:
- `S[i].nWormID == S[index].nWormID` and `S[i].nWeapon == S[index].nWeapon`;
- `(S[i].fTime − S[i−1].fTime).ms ≤ 255`;
- `|S[i].nAngle − S[i−1].nAngle| ≤ 255`;
- `|S[i].nSpeed − S[i−1].nSpeed| ≤ 255`;
- `|(int)S[i].vx − (int)S[i−1].vx| ≤ 255`, and the same for vy;
- `num < 255`.

The rule is checked against the source and the `multi-shot` trace. Note that
`nSpeed` is `(int)(|worm velocity| * 100)` with `ProjRelativeVel`, so a change of
only 2.55 px/s in the worm's speed between two shots breaks the run: a walking
worm's two-shot lists often go out as two single-shot packets (with their own
`nRandom`). Comparing the *decoded* `spd` values does not show this, because they
are not `nSpeed` (§4.3).

A run of 1 is written as a single shot. A longer run gets an `S2C_MULTISHOOT` header followed by
`writeSmallShot(prev = S[k−1], cur = S[k])` for each later shot. The writer then recurses to the
rest of the list.

**Header** (single or multi), in order:

1. `cmd`, `flags`, `nWormID` (byte).
2. `writeFloat((float)fTime.seconds())`, where `seconds()` = `(float)ms * 0.001f`.
3. `nWeapon` (byte), then `num` (multi only).
4. `write2Int12((short)pos.x, (short)pos.y)`: 12 bits each.
5. `writeInt16((short)vel.x)`, `writeInt16((short)vel.y)`.
6. `nRandom` (byte).
7. The angle byte: `nAngle − 255` if `nAngle > 255`, with `SMF_LARGEANGLE`.
8. The speed byte, with the negative and large flags.
9. A release bit byte (receiver ≥ 0.58.1, which is always the case locally).

**`writeSmallShot` has bugs**, and the port must reproduce their effect (§4.4):

- **The worm-velocity flags are set in `flags` instead of `extraflags`** (`360-363`):
  - `SHF_XWRMVEL` (0x10) lands on `SHF_SPEEDOFF`;
  - `SHF_YWRMVEL` (0x20) lands on `SHF_NG_XPOSOFF`.
- **Positive velocity deltas are therefore never written.** Only the `SHF_NG_*WRMVEL` bits
  (0x40, 0x80) are correctly placed in `extraflags`.
- **Position offsets are written but useless.** They are 4-bit when small (`write2Int4` keeps
  `& 0xf`, while a small offset may be up to 31). A lone x or lone y offset is not written
  unless it is large. The reader discards all position offsets anyway.

### 4.3 Reading (`readSingle` 437-484, `readMulti` 489-542, `readSmallShot` 547-643)

**Header decode:**

- `fTime = MAX(readFloat(), 0.0f)` is assigned through `TimeDiff::operator=(float)`, which
  computes `(Uint64)(f * 1000.0f)`. The full round trip is
  `ms → (float)ms*0.001f → ×1000.0f → truncate`.
  - Brute-force check (`scratchpad/m4-research/c/rt.c`): exact for every `ms = 10·serverFrame`
    with `serverFrame < 819205`. From 819205 on, about 70 % of frames come back +1 ms.
- `nWeapon = CLAMP(byte, 0, NumWeapons−1)`, `nWormID = byte`.
- `pos = ((float)x12, (float)y12)`, where `x12 = (ushort)(short)pos.x & 0xfff`. Maps up to
  4095 px are exact; the value is truncated toward zero.
- `vel = ((float)(short)vel.x, (float)(short)vel.y)`, truncated toward zero.
- `nRandom` is a byte (0..255, exact). The angle and the release bit are exact.
- `nSpeed` is decoded but **never used** by `ProcessShot` or `apply`.

**Small shot decode.** `*psShot = *psFirst` copies the **decoded** previous shot, then applies:

- **Time** (`SHF_TIMEOFF`): `fTime += TimeDiff((float)byte / 1000.0f)`, i.e.
  `(Uint64)((b/1000.0f)*1000.0f)`. Exact except b = 251 → 250 and b = 253 → 252.
- **Angle:** `nAngle` += or −= the delta byte, which gives the exact original angle.
- **Speed:** changes `nSpeed` only, which nothing uses. The decoded value is
  still wrong, and the oracle traces record it (`s[].spd`):
  - in a header, the speed byte is `nSpeed − 255` truncated to a byte when
    `|nSpeed| > 255`, so speeds above 510 come back modulo 256;
  - in a small shot, `SHF_XWRMVEL` set in `flags` is `SHF_SPEEDOFF`, so a change
    of the integer x velocity also writes `nSpeed − prev.nSpeed` as a byte
    (wrapped when negative), which the reader adds on top of the
    `SHF_NG_SPEEDOFF` byte it subtracts.

  `rl-sim::shot_codec` ports the bytes, not a formula.
- **Position:** x and y offsets are read, negated, and **never applied to `cPos`**. Every
  small shot keeps the first shot's decoded position.
- **Velocity:** `cWormVel.x -= byte` only when `SHF_NG_XWRMVEL` is set, i.e.
  `(int)S.vx < (int)P.vx`, and the same for y. Increases are lost.
- **`nRandom`** is not transmitted, so every shot of a run has the run header's `nRandom`.
  `apply()` later mutates each list entry separately, but each one starts from the header
  value.
- **`release`** comes from its bit.

### 4.4 The round trip as a function (for the port)

```
fn transmit(list) -> Vec<Decoded>:          // OLX order: runs in list order
  for each run R = [S0, S1, ..., Sk] (rule in §4.2):
    D0 = Decoded {
      worm: S0.worm, weapon: S0.weapon (clamped),
      pos:  ((((S0.pos.x as i16) as u16) & 0xfff) as f32, same for y),
      vel:  ((S0.vel.x as i16) as f32, (S0.vel.y as i16) as f32),   // (0,0) if !ProjRelativeVel
      angle: S0.angle, rnd: S0.rnd, release: S0.release,
      time: ((S0.time_ms as f32 * 0.001f32) * 1000.0f32) as u64 (max 0)
    }
    for j in 1..=k:  (P = S[j-1] original, D = D[j-1] decoded)
      Dj = D
      if Sj.time != P.time: Dj.time = D.time + (((Sj.time-P.time) as u8 as f32 / 1000.0) * 1000.0) as u64
      Dj.angle = Sj.angle
      if (P.vel.x as i32) > (Sj.vel.x as i32): Dj.vel.x = D.vel.x - ((P.vel.x as i32 - Sj.vel.x as i32) as u8 as f32)
      if (P.vel.y as i32) > (Sj.vel.y as i32): Dj.vel.y = D.vel.y - (...)
      Dj.release = Sj.release
      // pos, rnd, worm, weapon unchanged from D (= header)
```

Here `as i32` is C's `(int)` truncation of the original float. A run breaks when a difference
exceeds 255, so the byte casts are exact. A Rust `as` cast saturates on out-of-range floats,
whereas C `(short)` is UB; |vel| ≥ 32768 cannot occur in practice.

### 4.5 Client parse (`CClient_Parse.cpp:1918-1946`)

- If `!client->canSimulate()` (`state < S_Preparing` or the map is not ready), the shot is
  skipped.
- Otherwise `cShootList.readSingle/readMulti` (which `Clear()`s the client list first) is
  followed by `ProcessServerShotList()`.
- Each packet is processed before the next is parsed.

---

## 5. From a decoded shot to projectiles

### 5.1 `CClient::ProcessServerShotList` / `ProcessShot` (`CClient_Game.cpp:813-824`, `852-921`)

```
for i in 0..num: ProcessShot(sh_i, sh_i->spawnTime());          // spawnTime evaluated per shot, before processing it

ProcessShot(shot, fSpawnTime):
  if(0 <= nWormID < MAX_WORMS) {
     w = wormById(nWormID,false); if(!w) return;
     if(!w->getAlive()) return;            // worm dead at DELIVERY time → shot silently dropped (beams too)
  }
  if(!GetWeapons()) return; if(nWeapon out of range) return;
  wpn = &Weapons[nWeapon];
  if(shot->release) { wpn->FinalProj.apply(shot, fSpawnTime); return; }   // never for LX56
  if(wpn->Type == WPN_BEAM) { ProcessShot_Beam(shot); return; }            // §6.1 (no recoil!)
  if(wpn->Proj.Proj == NULL) return;
  if(wpn->UseSound) StartSound(...)                 // no-op in DEDICATED_ONLY build (Sounds.cpp:87); cosmetic
  if(0 <= nWormID < MAX_WORMS) {
     CVec dir = GetVecFromAngle(float(shot->nAngle));        // (cosf(a*(float)(PI/180)), sinf(...))
     w->velocity() -= dir * (float)wpn->Recoil;               // vel.x = vel.x - dir.x*R; vel.y = vel.y - dir.y*R
     w->setDrawMuzzle(true);                                  // cosmetic
  }
  wpn->Proj.apply(shot, fSpawnTime);                           // §5.2 (Proj_SpawnParent(shot) → mutates shot->nRandom)
```

- **Recoil applies at delivery, to the worm's current velocity.** It uses the decoded integer
  angle. In real play it comes before the next frame's Gus round trip.
- **Special weapons** (`Type == WPN_SPECIAL`) never reach the shot list.

### 5.2 `Proj_SpawnInfo::apply` (`src/common/PhysicsLX56_Projectiles.cpp:562-640`)

`Proj_SpawnParent` accessors for a shot parent (`495-554`):

| Accessor | Value |
|---|---|
| `ownerWorm()` | `shot->nWormID` |
| `fixedRandomIndex()` | `shot->nRandom` |
| `fixedRandomFloat()` | `GetFixedRandomNum(shot->nRandom)` = `RandomNumbers[(uchar)nRandom]` (256-entry table, `include/RandomNumberList.h`, `Misc.cpp:84-87`) |
| `position()` | `shot->cPos + GetVecFromAngle((float)nAngle) * (float)FT_ShootSpawnDistance` (8.0f) |
| `velocity()` | `shot->cWormVel` |
| `angle()` | `(float)shot->nAngle` |

```
const float heading = Useangle ? parent.angle() : 0;          // weapons: Useangle=true → (float)nAngle
Proj->WeaponName = wpn->Name;                                 // mutates proj_t (kill messages only)
for(int i = 0; i < Amount; i++) {
  const CVec sprd = UseParentVelocityForSpread ? parent.velocity()*ParentVelSpreadFactor     // weapons: false
                  : GetVecFromAngle( ((float)Angle + heading) + parent.fixedRandomFloat() * (float)Spread );
  int rot = 0;
  if(UseRandomRot && Proj->Rotating) {                        // weapons: UseRandomRot=true
      if(Proj->RotIncrement == 0) Proj->RotIncrement = 1;     // PERSISTENT mutation of the mod's proj_t
      rot = GetRandomInt( abs( 360 / Proj->RotIncrement ) ) * Proj->RotIncrement;   // int division; ONE rand()
  }
  shot->nRandom++; shot->nRandom %= 255;
  const CVec& speedVarVec = UseSpecial11VecForSpeedVar ? CVec(1,1) : sprd;           // weapons: sprd
  const CVec v = ( sprd * (float)Speed
                 + (speedVarVec * (float)SpeedVar) * parent.fixedRandomFloat() )     // uses the incremented nRandom
               + ( AddParentVel ? ParentVelFactor * parent.velocity() : CVec(0,0) ); // weapons: AddParentVel=true
  shot->nRandom *= 5; shot->nRandom %= 255;
  ignoreWormCollBeforeTime = GetPhysicsTime() + 0.1f;         // AbsTime + (u64)(0.1f*1000.0f) = +100 ms
  const int random = shot->nRandom;
  CVec pos = parent.position() + PosDiff;                     // PosDiff (0,0); SnapToGrid (0,0) → skipped
  cClient->SpawnProjectile(pos, v, rot, nWormID, Proj, random, spawnTime, ignoreWormCollBeforeTime);
  shot->nRandom++; shot->nRandom %= 255;
}
```

**Exact float order:**

- `sprd`: `a = (0.0f + (float)nAngle)`, plus `r1 * Spread` → `GetVecsFromAngle`. Then
  `angle = yaw * (float)(PI/180)` (`(float)` of the double 0.017453292519943295) →
  `(1*cosf, 1*sinf)` (`MathLib.cpp:135-155`).
- `v.x = ((sprd.x*S) + ((sprd.x*SV)*r2)) + (1.0f*pvx + 0.0f*pvy)`. The matrix is
  `MatrixD2(f)`, i.e. `v1=(f,0), v2=(0,f)`; `M*v = (v1.x*v.x + v2.x*v.y, v1.y*v.x + v2.y*v.y)`
  (`include/CVec.h:206,214-216`). Same for y.
- `pos = (cPos.x + dir.x*8.0f, cPos.y + dir.y*8.0f) + (0,0)`.

**The random index sequence**, starting from `n0` (0..255; `RandomNumbers[255]` exists):

| Step | Value |
|---|---|
| spread random | `T[n]` |
| `n = (n+1)%255` | |
| speed-var random | `T[n]` |
| `n = (n*5)%255` | projectile `iRandom = n` (and `SpawnProjectile`'s `_random %= 255` is a no-op) |
| `n = (n+1)%255` | the next projectile of the same shot starts here |

**`rand()` order per projectile:** first `rot` (only if `Proj->Rotating`), then the colour
(inside `Spawn`, §5.3).

### 5.3 `CClient::SpawnProjectile` (`CClient_Game.cpp:716-730`) and the parts of `CProjectile::Spawn` that matter here (`src/common/CProjectile.cpp:43-112`)

```
CProjectile* proj = cProjectiles.getNewObj();  if(proj == NULL) return;   // pool full → dropped (rot rand already consumed)
_random %= 255;
proj->Spawn(_proj,pos,vel,rot,owner,_random,remotetime, ignoreWormCollBeforeTime);
```

`Spawn` does:

- `iOwner`, `fLife = 0`, `vOldPos = vPos = pos`, `vVelocity = vel`, `fRotation = (float)rot`,
  `radius = (Width/2, Height/2)` (int);
- `iRandom = _random`, `fTimeVarRandom = T[iRandom]`,
  `fLastSimulationTime = fSpawnTime = time`, `fIgnoreWormCollBeforeTime`;
- `fSpeed = |vel|`, then `CalculateCheckSteps()`. At `|vel|² ≥ 250000` that uses `iRandom` and
  `getRandomFloat()`.
- **Colour:** `GetRandomInt(Colour.size()-1)` for `PRJ_PIXEL/RECT/CIRCLE/POLYGON` with a
  non-empty colour list. **One `rand()` even with a single colour**, since
  `GetRandomInt(0)` still calls `rand()`.
- If `FT_LX56WallShooting`: `fWallshootTime = 0.011f + getRandomFloat()/1000`, which advances
  `iRandom`;
- `bChangesSpeed` and `updateCollMapInfo()`.

The full projectile semantics belong to the projectile report.

### 5.4 The projectile container (`include/FastVector.h`, `CClient.h:215`, `MAX_PROJECTILES = 3000` `CProjectile.h:36`)

- **`getNewObj`:** returns `m_firstUnused` (NULL if ≥ 3000). If that index is
  `> m_lastUsed`, it sets `m_lastUsed = index`. Then `findNewFirstUnused()` increments
  `m_firstUnused` until it is `> m_lastUsed`, `≥ SIZE`, or points at an unused slot.
- **`setUnused`** fires `onInvalidation` → `refreshObj(i)`:
  - if `i < firstUnused`, then `firstUnused = i`;
  - if `i >= lastUsed`, it calls `findNewLastUsed()`, which steps back while slots are unused
    and stops below `firstUnused`.
- **So slot allocation is lowest-free-index first.** Within one delivery, projectiles take
  slots in shot-list order, then `i` order.
- **`Iterator::next`** (used by `simulateProjectiles`) visits used slots in index order. It
  stops after reaching `m_lastUsed`. Projectiles spawned during iteration at a higher index are
  visited in the same pass (projectile report).
- The trace records projectiles by slot index `i`, so the port must reproduce this allocator
  exactly.

---

## 6. Beams (`WPN_BEAM`)

### 6.1 `CClient::ProcessShot_Beam`: damage and terrain (`CClient_Game.cpp:926-1055`)

It is reached from `ProcessShot`, i.e. **at delivery time** (§1.1), after the dead-worm check.

```
wpn = &Weapons[nWeapon];
if(0 <= nWormID < MAX_WORMS) {
   w = wormById(nWormID,false); if(!w) return;
   if(w->getCurWeapon()->weapon() != wpn)                        // 936-943: GAMEPLAY: switch current weapon
       for i in slots: if(slot[i].weapon() == wpn) { w->setCurrentWeapon(i); break; }   // first matching slot
   w->writeCurWeapon()->LastFire = 0.3f;                          // 946: throttles the server fire gate (§6.3)
}
GetVecsFromAngle((float)shot->nAngle, &dir, NULL);               // integer angle
CVec pos = shot->cPos;                                            // decoded (short, 12-bit) worm position — NOT a muzzle
CVec orth_dir = dir.orthogonal();                                 // (dir.y, -dir.x)
goodWidthParts = []; width = 0;
worms = [all game.aliveWorms() except nWormID]  (std::list, game order)
for(int i=0; i<Bm.Length; ++i) {
   newWidth = int(float(Bm.InitWidth) + Bm.WidthIncrease * i);          // LX56: always 1
   newWidth = (int)MIN(newWidth, MIN(map.W, map.H));
   if(newWidth != width) { goodWidthParts.resize(newWidth);
       for j in width..newWidth: goodWidthParts[j] = (width>0) ? goodWidthParts[MAX(j-2,0)] : true;
       width = newWidth; }
   if(width <= 0) break;
   bool stopbeam = true;
   for(uint j=0; j<goodWidthParts.size(); ++j) {
      if(!goodWidthParts[j]) continue;
      int o = (j%2==0) ? int(j/2) : -int((j+1)/2);
      CVec p = pos + orth_dir * o;                                // VectorD2*int: x*o in float
      uchar px = (p.x <= 0 || p.y <= 0) ? PX_ROCK : map->GetPixelFlag((int)p.x,(int)p.y);   // OOB → PX_ROCK (CMap.h:291-301)
      if(Bm.Damage != -1) {                                       // bonuses (off in scope)
         for each used bonus b: if((p - b.pos).GetLength() < 3) { Explosion(shot->spawnTime(), p, 0, 5, nWormID); good[j]=false; break; }
         if(!good[j]) continue;
      }
      if(px & (PX_DIRT|PX_ROCK)) {
         if(Bm.Damage != -1) {
            int damage = Bm.Damage; if(Distribute) { damage /= width; if(damage==0) damage = SIGN(Bm.Damage); }
            int d = map->CarveHole(damage, p, FT_InfiniteMap);    // size clamped to [0,4] = theme hole index (CMap.cpp:1007-1016)
            if(0 <= nWormID < MAX_WORMS) wormById(nWormID)->incrementDirtCount(d);
         }
         good[j] = false; continue;                               // beam stops on dirt/rock even with Damage -1
      }
      for w2 in worms: if((p - w2->getPos()).GetLength2() < 5.0f*5.0f) {    // x*x + y*y < 25
           int damage = Bm.PlyDamage; if(Distribute) {...SIGN(PlyDamage)}
           InjureWorm(w2, (float)damage, nWormID, wpn->Name);      // damage report (consumes rand for blood)
           good[j] = false; worms.erase(w2); break; }
      if(!good[j]) continue;
      stopbeam = false;
   }
   if(stopbeam) break;
   pos += dir;                                                    // float accumulation, step 1 px
}
```

For LX56 beams (width 1), the beam steps 1 px from the quantized worm position along the integer
angle, up to `Length` steps. It stops at the first dirt or rock pixel, or the map edge (which
carves a `Damage`-sized hole there), or at the first other alive worm within 5 px (which takes
`PlyDamage`).

- **Width:** `MIN(newWidth, MIN(W, H))` mixes `int` and `uint`, so it resolves to
  `MIN(unsigned long, unsigned int)` (`include/MathLib.h:68`): a negative `newWidth`
  becomes `MIN(W, H)`. The width is always in `[0, MIN(W, H)]`.
- **No recoil** is ever applied for beams (`ProcessShot` returns before the recoil code).
- **No randomness** in the beam itself; only `InjureWorm`'s blood calls `rand()`
  (`CClient_Game.cpp:451-458`, damage report).
- **Classic Laser** (rl-data `laser.ron`): Damage 0, so every shot still carves the smallest
  theme hole (`bmpHoles[0]`). PlyDamage 1, Length 999999, ROF 0.009 s, Drain 1.25,
  Recharge 0.18.

### 6.2 `CClient::DrawBeam`: client side, every sub-step (`CClient_Game.cpp:590-711`)

Called from `PlayerShoot` while `LastFire > 0` and not reloading. **It is not cosmetic: it
carves terrain** and runs in dedicated mode too.

```
if(Slot->Reloading) return;
float Angle = w->getAngle(); if(face LEFT) Angle = 180 - Angle;      // float, NOT normalized, NOT truncated
CVec pos = w->getPos();                                               // current float position
GetVecsFromAngle(Angle,&dir,NULL); orth_dir = dir.orthogonal();
drawBeam = (Colour != pink && visibleForEverybody)                    // entity only
worms = alive worms except w
loop i < Bm.Length: same width bookkeeping as §6.1;
  for each good part j:
     p = pos + orth_dir*o; px as in §6.1 (no bonus check)
     if(px & (DIRT|ROCK)) {
        if(Bm.Damage != -1) {
           if(width <= 2) SpawnEntity(ENT_EXPLOSION,5,p,...);         // entity (no rand)
           damage = Bm.Damage (distribute...); d = CarveHole(damage, p, FT_InfiniteMap); w->incrementDirtCount(d);
        }
        good[j]=false; continue;
     }
     for w2 in worms: if((p - w2.pos).GetLength2() < 25) { if(Damage!=-1) SpawnEntity(ENT_EXPLOSION,3,...);
                                                          good[j]=false; erase; break; }   // NO damage
     stopbeam = false;          // NOTE: set even after a worm hit (no `continue`), unlike §6.1;
                                // the next iteration then finds no good part and breaks — same outcome
  if(stopbeam) break;
  pos += dir;
SetWormBeamEntity(...)          // cosmetic
```

### 6.3 Beam timing in a host or local game

Per sub-step, with fire held, the beam weapon current and enough charge:

1. `simulateWormWeapon`: `if(LF>0) LF -= 0.0110000009f`.
2. `WormShoot`: if `LF ≤ 0`: `LF = ROF`, add the shot, `Charge -= Drain/100`.
3. `PlayerShoot`: if `LF > 0`: `DrawBeam`, then `LF -= 0.0100000007f`.

When the shot packet is processed (§1.1), `LF = 0.3f` on the then-current slot, and steps 1 and
3 count it down by about 0.021 per sub-step. So, for example, the Classic laser fires on two
consecutive sub-steps, then pauses about 15 sub-steps once delivery sets `LF = 0.3`.

`DrawBeam` carves the terrain on every sub-step `LF > 0` as long as the fire key is held. That
includes all the throttled sub-steps.

With the current oracle (no delivery), the laser fires on every sub-step: `LF` = 0.009 is
followed by `0.009 − 0.0100000007 < 0`.

---

## 7. Special weapons (`WPN_SPECIAL`) — `CClient::ShootSpecial` (`CClient_Game.cpp:549-585`)

```
if(w->tWeapons.size() == 0) return;
wpnslot_t *Slot = w->writeCurWeapon();  TimeDiff dt = tLX->fDeltaTime;       // 10 ms in the frame loop
if(!Slot->weapon()) return;
switch(Special) {
  case SPC_NONE: break;
  case SPC_JETPACK:
     w->velocity().y += -50.0f * ((float)Thrust * (float)dt.seconds());       // vy = vy + (-50.0f * ((float)Thrust * 0.0100000007f))
     CVec s = CVec(15,0) * GetRandomNum();                                    // ONE rand() (also in dedicated)
     SpawnEntity(ENT_JETPACKSPRAY, 0, pos, s + CVec(0,1)*(float)Thrust, blue, NULL);   // entity (pool slot)
}
Slot->Charge -= Drain / 100;
if(Slot->Charge < 0.f) { Slot->Charge = 0.f; Slot->Reloading = true; }       // '<' here vs '<=' in WormShoot; ignores DisableWpnsWhenEmpty
```

In a host or local game it is reached only through `PlayerShoot`, after `WormShoot` already
handled the special in the same sub-step (§3.2, §3.4):

- **ROF > 0:** `WormShoot` sets `LastFire = ROF` and drains. `PlayerShoot` returns at
  `LastFire > 0`. **The jetpack never thrusts.** Charge is drained once per ROF interval.
- **ROF ≤ 0:** `WormShoot` drains (`<= 0` empty rule, `DisableWpnsWhenEmpty`). If that did not
  start reloading, `PlayerShoot` → `ShootSpecial` applies thrust and drains **again**
  (`< 0` rule).
- **Scope:** Classic and MW 1.0 have no `WPN_SPECIAL` weapon (checked with
  `content/packs/olx-{classic,mw-1-0}/weapons/*.ron`). Only beams: Classic `Laser`; MW `Laser`
  and others, with `Laser` banned in MW's "Standard 100lt". `Sunray Beamer` is a projectile
  weapon.

---

## 8. Randomness

### 8.1 Primitives (`src/common/MathLib.cpp:29-81`, glibc 2.35 `rand()`, TYPE_3 additive generator)

```
HALF_RAND = RAND_MAX/2 = 1073741823
GetRandomNum()    = (float)(rand() - HALF_RAND) / (float)HALF_RAND        // (float)HALF_RAND = 1073741824.0f; in [-1,1]
GetRandomPosNum() = (float)rand() / (float)RAND_MAX                       // (float)RAND_MAX = 2147483648.0f
GetRandomInt(max) = CLAMP((int)(GetRandomPosNum() * (float(max)+1)), 0, max)   // one rand(); still one when max==0
GetRandomVec()    = angle = GetRandomPosNum()*2.0f*(float)PI; CVec(cosf(angle),sinf(angle)) * GetRandomNum()   // two rand() calls, that order
```

- **Seeding.** `simSetSeed` calls `srand(seed)` (`Command.cpp:2269-2274`); the oracle calls it
  right before `simLoadInputs` and `spawnWorm`. Nothing between that and frame 0 calls
  `rand()`: `CWorm::Spawn` → `CMap::CarveHole` uses no `rand()` (`CMap.cpp:1007-1105`).
- **The fixed table.** `RandomNumbers[256]` (`include/RandomNumberList.h`) is indexed with a
  `uchar`.

### 8.2 `rand()` consumers on or near this path (in order within a sub-step)

| Where | Calls | Condition |
|---|---|---|
| bleeding, `PhysicsLX56.cpp:320-330` | 80 (`amount = (100/100.0f)*20` → 20 × 2 `GetRandomVec`) | health < 15 and `GetPhysicsTime() > lastBlood + 2.0f` (`iBloodAmount` = 100) |
| worm carve `CarveHole(CVec)`, `CMap.cpp:2172-2199` | 6 (3 `GetRandomVec`) | a `PX_DIRT` pixel among x−2..x+2 on row y; break at the first |
| rope first hit on destroyable material, `PhysicsLX56.cpp:603-607` | 10 (5 `GetRandomVec`) | `px.destroyable && firsthit` |
| `WormShoot` → `addShoot` | 1 per connected client (1 locally) | each shot (not on a full list) |
| `ShootSpecial` (jetpack) | 1 | thrust applied (§7) |
| `InjureWorm` (beam hit, damage material) | blood: `amount = CLAMP(dmg,10,100)` × (1 + 2 + 2 + 2) | damage > 0; plus death gibs/blood when it kills (damage report) |
| `ProcessShot` → `apply` | 1 per projectile if `Proj->Rotating` | at delivery |
| `CProjectile::Spawn` | 1 if pixel-like type with ≥ 1 colour | at delivery |
| `SpawnEntity(ENT_GIB)` internals | 2 | only while the entity pool has room (never freed in dedicated) |
| non-dedicated only: `SimulateEntities` (`Entity.cpp:457-517`) | many | **not in the oracle**; the Rust port should keep cosmetics on a separate RNG and record this as a deviation from non-dedicated OLX |
| bot AI (`CWormBot.cpp`) | many | any worm the oracle schedule does not mention stays a bot (`OracleHooks::loadInputs` installs human handlers only for listed worms) |

---

## 9. Weapon selection at match start, and respawn

### 9.1 Defaults and preparation

- **Slot defaults** (`CWorm.h:95-99`): `WeaponId = -1`, `Charge = 1.f`, `Reloading = false`,
  `LastFire = 0.f`. There are `FT_WeaponSlotsNum` = 5 slots.
- **`CWorm::Prepare`** (`CWorm.cpp:293-403`): if `weOwnThis()`, sets `iCurrentWeapon = 0`,
  resizes and resets the slots, then calls `GetRandomWeapons()`.
- **`frameInner` worm preparation** (`Game.cpp:842-895`): it sets lives, health and
  `bWeaponsReady = false`, then:
  1. for local worms: `initWeaponSelection()`;
     - human: slots from the profile, restricted, `Charge=1`/`Reloading=false`/`LastFire=0`
       (`CWormHuman.cpp:704-749`);
     - bot: Classic/Liero at 100 % loading gets a preset (Super Shotgun, Napalm, Cannon,
       Doomsday, Chaingun), else random (`CWormBot.cpp:4129+`);
  2. then `cServer->PrepareWorm` (`CServer.cpp:404-430`):
     - `DisableWpnsAtStartup` → slots reset, ready;
     - otherwise `SameWeaponsAsHostWorm` → clone the host worm's weapons;
     - otherwise `ForceRandomWeapons` (the oracle sets it) → `GetRandomWeapons()`, ready.

### 9.2 `CWorm::GetRandomWeapons` (`CWorm.cpp:788-841`)

```
for each slot i:
  num = MAX(1, GetRandomInt(NumWeapons-1))      // weapon 0 is never the start; one rand()
  n = num; lastenabled = -1
  loop: if(n >= NumWeapons) n = 0
        bSelected = any earlier slot k<i holds weapon n
        if(!restrictions || restrictions->isEnabled(Weapons[n].Name)) { if(!bSelected) break; lastenabled = n; }
        n++; if(n == num) { n = lastenabled; break; }
  slot[i].WeaponId = (0 <= n < NumWeapons) ? n : -1      // Charge/Reloading/LastFire untouched here
```

- **`isEnabled(name)`** is true only for `wpr_enabled` (`CWpnRest.cpp:214-223`).
- **Weapons missing from the restriction file** are added as enabled (`updateList`, `59-75`).
- **The restriction file** is `FT_WeaponRest`, "Standard 100lt" in the oracle header
  (`cfg/presets/<mod>/Standard 100lt.wps`).

### 9.3 Respawn and spectators

- **`CWorm::Spawn`** (`CWorm.cpp:591-650`): for every slot, `Charge = 1.f`, `Reloading = false`,
  `LastFire = 0.f`. `WeaponId` and `iCurrentWeapon` are kept. `bDrawMuzzle = false`.
- **`FT_EmptyWeaponsOnRespawn`** (default false): after the spawn, the server sets every slot
  to `Charge = 0`, `Reloading = true` (`CServer_Game.cpp:101-102`,
  `CServer_Send.cpp:1066-1072`).
- **`CloneWeaponsFrom`** (`883-892`) resets charge state too.

**For the oracle:** the loadout is decided before the second `simSetSeed`, and the trace's
frame −1 records it (`slots[].id/ch/rl/lf`, `wpn`). The Rust replay can therefore copy it, as
`World::set_loadout` does. To *choose* weapons, a hook is needed (§13).

---

## 10. Features read on these paths

The OLX default is (unset, default) from `src/common/FeatureList.cpp`. The oracle value comes
from the trace header.

| Feature | Read at | OLX default | Oracle |
|---|---|---|---|
| `LoadingTime` (5) | `simulateWormWeapon` 488-491 | 100 | 100 |
| `GameSpeed` (80) | `simulateWormWeapon` (only with OnlyForProjs), clocks | 1.0 | 1 |
| `GameSpeedOnlyForProjs` (81) | `simulateWormWeapon` 475, `Simulation` 65 | false | false |
| `WeaponCombos` (169) | `getInput` 401 | true | true |
| `DisableWpnsWhenEmpty` (161) | `WormShoot` 405/460 (`gameSettings`) | false | false |
| `ProjRelativeVel` (181) | `addShoot` 97/101 (`gameSettings`) | true | true |
| `ShootSpawnDistance` (184) | `Proj_SpawnParent::position` 525 | 8.0 | 8 |
| `LX56WallShooting` (183) | `CProjectile::Spawn` 101 | true | true |
| `InfiniteMap` (170) | beam `CarveHole` wrap | false | false (rl-sim rejects true) |
| `WeaponSlotsNum` (160) | `Prepare` | 5 | 5 |
| `ForceRandomWeapons` (11) | `PrepareWorm`, `bServerChoosesWeapons` | false | **true** |
| `SameWeaponsAsHostWorm` (12) | `PrepareWorm`, `initWeaponSelection` | false | false |
| `DisableWpnsAtStartup` (162) | `PrepareWorm` | false | false |
| `EmptyWeaponsOnRespawn` (43) | `SpawnWorm` | false | false |
| `AllowWeaponsChange` (158) | weapon menu only | true | true |
| `Bonuses` (30) | beam bonus check, `Simulation` pickups | false | false |
| `LX56PhysicsFPS` (20) / `ForceSameLX56PhysicsFPS` (21) | `dt` | 84 / true | 84 / true |
| `GameMode` | `gameMode()->Shoot(w)` | Death Match → true | Death Match |
| `TeamInjure`, `SelfInjure` (100, 102), `WormDamageFactor`/`WormShieldFactor` (152, 153) | `InjureWorm` (beam hits) | true, true, 1, 1 | same |

Client options that matter (`Options.cpp`):

| Option | Value | Effect |
|---|---|---|
| `doProjectileSimulationInDedicated` | true (278) | projectiles are simulated in the dedicated server |
| `iBloodAmount` | 100 (213) | blood `rand()` count |
| `bParticles` | true (215) | only gates entity creation, not `rand()` |
| `nMaxFPS` | 95 (255) | real-play delivery |
| `iNetworkSpeed` | overridden to `NST_LOCAL` for a host (`CClient.cpp:305-309`) | shot delay 0 |

---

## 11. Mod data used (`rl-data/src/mod_def.rs` ↔ OLX)

`WeaponDef` ↔ `weapon_t` (`include/WeaponDesc.h:96-133`), loaded at `CGameScript.cpp:687-781`:

| rl-data | OLX | Used by |
|---|---|---|
| `rof` (already /1000) | `ROF` | `WormShoot` 392, `PlayerShoot` 529 |
| `recharge` (already /10) | `Recharge` | `simulateWormWeapon` 491 |
| `drain` (raw) | `Drain` (÷100 at use) | `WormShoot` 401/456, `ShootSpecial` 580 |
| `laser_sight` | `LaserSight` | `Simulation` 180 (entity only) |
| `Projectile.recoil` | `Recoil` (int) | `ProcessShot` 913 |
| `Projectile.sound` | `UseSound`/`SndFilename` | cosmetic |
| `Projectile.class` | `Class` | bot AI only |
| `Projectile.spawn.projectile` | `Proj.Proj` | `apply` |
| `spawn.amount`, `speed` (int), `speed_var`, `spread` | `Amount`, `Speed`, `SpeedVar`, `Spread` | `apply` |
| `spawn.use_angle` (true), `angle` (0) | `Useangle`, `Angle` | `apply` (`weapon_t()` ctor sets `Useangle=UseRandomRot=AddParentVel=true`, `WeaponDesc.h:98-101`) |
| `spawn.use_random_rot` (true), `add_parent_vel` (true), `parent_vel_factor` (1.0) | same names | `apply` |
| `spawn.use_parent_velocity_for_spread` (false), `parent_vel_spread_factor` (0.3), `use_special11_vec_for_speed_var` (false) | same | `apply` (not used for weapon shots) |
| — (always (0,0) for LX56) | `PosDiff`, `SnapToGrid` | `apply` |
| — (never set for v7) | `FinalProj` | `WormShootEnd`/`ProcessShot` |
| `Beam.recoil` | `Recoil` | **never used** (no recoil for beams) |
| `Beam.beam.colour` | `Bm.Colour` | drawing only (pink check) |
| `beam.damage` | `Bm.Damage` | hole size [0..4], −1 = no carve |
| `beam.player_damage` | `Bm.PlyDamage` | `InjureWorm` |
| `beam.length` | `Bm.Length` | steps |
| `beam.init_width` (1), `width_increase` (0), `distribute_damage_over_width` (false) | same | width logic |
| `Special.special` | `Special` | `ShootSpecial` |
| `Special.thrust` (u32) | `tSpecial.Thrust` (Uint32) | jetpack |

`ProjectileDef` fields read at creation:

- `gfx: Image{rotating, rot_increment}` → `Rotating`/`RotIncrement` (`apply`; rotation exists
  only for `PRJ_IMAGE`, `CGameScript.cpp:945-947`);
- `gfx: Pixel{colours}` → `Colour` (colour `rand()`);
- `width`, `height` → radius;
- the rest belongs to the projectile report.

**Weapon ids** are the gamescript load order (`wpn->ID = n`, `CGameScript.cpp:690`). The
trace's slot `id` and the shot `nWeapon` use them, so the Rust weapon list order must match
(see §13).

---

## 12. Dedicated versus non-dedicated, and cosmetic-only code

- **Cosmetic only:**
  - sounds (`StartSound` is a no-op in `DEDICATED_ONLY`);
  - `setDrawMuzzle`, `bForceWeapon_Name`, `shouldRepaintInfo`;
  - the `LaserSight` entity and `SetWormBeamEntity`;
  - every `SpawnEntity` *except* that its arguments (and `ENT_GIB`'s internal calls) consume
    `rand()`, and that it fills the never-freed pool in dedicated mode.
- **Non-dedicated only:**
  - `SimulateEntities` (`CClient_Game.cpp:210-211`) consumes `rand()`. This is a real `rand()`
    divergence between a dedicated server and a local GUI game.
  - Projectile images and sounds are loaded only when `!bDedicated`
    (`CGameScript.cpp:936-943`, `759-765`). With no `bmpImage`, the projectile animation
    end-of-loop logic (`ANI_ONCE` delete, `PhysicsLX56_Projectiles.cpp:1262-1283`) never runs
    in dedicated mode. This is for the projectile report: the oracle and a local game differ
    here.
- **Not cosmetic despite its name:** `DrawBeam` carves terrain and counts dirt (§6.2).

---

## 13. What the oracle hooks need

### 13.1 Deterministic shot delivery (required for anything projectile-related)

Today shots travel through UDP and use wall-clock times (§1.1–1.2). Proposed patch addition,
`OracleHooks::betweenFrames()`:

**Placement.** Call it in `Game::frameInner` right after `simulationTime += frameDt;`
(`Game.cpp:956`). At that point `serverFrame` and `GetPhysicsTime()` equal what
`ReadPackets` sees at the start of the next main-loop iteration.

**Model R** (recommended: real play at one frame per main-loop iteration):

```
CServerConnection* cl = cServer->localClientConnection();
CShootList* sl = cl->getShootList();
if(sl->getNumShots() > 0) {
    if(g_shotListFrame < 0) g_shotListFrame = frameIndex();        // list started this frame → not sent yet
    else if(g_shotListFrame < frameIndex()) {                       // started in an earlier frame → "send" now
        CBytestream bs; sl->writePacket(&bs, cl->getClientVersion()); sl->Clear();
        AbsTime saved = tLX->currentTime;
        tLX->currentTime = game.simulationAbsTime() + TimeDiff(10);  // δ = 10 ms, see Open questions
        bs.ResetPosToBegin();
        while(!bs.isPosAtEnd()) {
            uchar cmd = bs.readByte();
            if(cmd == S2C_SINGLESHOOT) cClient->getShootList()->readSingle(&bs, version, nw-1);
            else /* S2C_MULTISHOOT */  cClient->getShootList()->readMulti(&bs, version, nw-1);
            cClient->ProcessServerShotList();
        }
        tLX->currentTime = saved;
        g_shotListFrame = -1;
    }
}
```

- **Corner case:** a list that becomes non-empty and is sent in the same call cannot happen,
  because a new list waits one frame.
- **Model S** (simpler alternative): send and process every frame
  (`g_shotListFrame` unused). Packets then hold only one frame's shots, so multi-shot encoding
  happens only with ≥ 2 sub-steps per frame.

**Stop the real network path from also sending.**
- The hook clears the list, but with idle main-loop iterations (`getFrame`, `simStep 0`)
  `SendUpdate` could still send a list left over from the last frame of a batch, because by
  then the wall clock has moved.
- Make `GameServer::SendUpdate`'s shot-list block (`CServer_Send.cpp:556-569`) skip while the
  hooks are active.
- Pumping the network between frames is not needed for anything else in this area.

### 13.2 Choosing weapons

`simSetWeapons <wormId> "<name0>,<name1>,...[,cur=<n>]"`:

- for each slot: `WeaponId = game.gameScript()->FindWeaponId(name)` (name based, so it is
  independent of id order);
- `Charge = 1.f`, `Reloading = false`, `LastFire = 0.f`;
- `iCurrentWeapon = n`;
- call it after `spawnWorm` (which resets charge state anyway) or before; both work.

Optionally also `simSetSlot <wormId> <slot> <charge> <reloading> <lastFire>`, to test
reloading directly.

### 13.3 Avoiding bot AI

Install human handlers for **all** worms in `loadInputs`, not only the listed ones. Otherwise
an unlisted worm stays a bot and calls `rand()` every frame (§8.2). An alternative is to
require every scenario to list every worm.

### 13.4 Trace additions

- **Header:**
  - `"weapons"`: `[names by id]`;
  - `"projectiles"`: `[proj_t filename/index → name]` (the trace's `p[].t` is
    `projectileIndex`, the load-order index);
  - the seed.
- **Per frame:**
  - `"sf"`: `serverFrame`, and `"st"`: `simulationTime` ms. The spawn-time formula uses
    `serverTime`; absolute values matter only for the float round trip at huge frame numbers.
  - `"rc"`: the cumulative count of `rand()` calls. Count it in a wrapper inside
    `GetRandomNum`/`GetRandomPosNum` (`MathLib.cpp:58-67`), the only `rand()` callers on
    gameplay paths besides bot AI and `ChallengeTable`. This localizes a `rand()` desync to a
    frame.
  - `"s"`: shots processed in `betweenFrames`, decoded as processed: `{w, wpn, x, y, vx, vy,
    ang, rnd, t_ms, rel, spawn_ms}`, plus the recoil-adjusted worm velocity.
- **Per projectile** (coordinate with the projectile report): `owner`, `iRandom`, `rot`,
  `lst` (`fLastSimulationTime` ms), `spawn` (`fSpawnTime`), `icwt`
  (`fIgnoreWormCollBeforeTime`), `col` (`iColour`), `fWallshootTime`.
- **Already present and sufficient for slot logic:** `slots[].ch/rl/lf`, `wpn`, `keys` (bit 1
  = shoot), `dirt`, `mh`.

### 13.5 Scenarios worth recording

1. Hold fire with a projectile weapon until empty, then wait for the reload. This checks ROF
   counting, drain, recharge and reload completion, and only slots, so it already works with
   the current patch.
2. Switch weapons while one reloads: the frozen slots.
3. A single shot, then a burst with one worm (multi-shot quirks under Model R), then two worms
   firing in the same frames (singles).
4. A shotgun-like weapon with `Amount > 1` and `Spread`: the `nRandom` chain.
5. A rotating projectile weapon: the rot `rand()` and the `RotIncrement` mutation.
6. The Classic laser: into dirt (DrawBeam carving and dirt count), then at another worm
   (`PlyDamage`, the `LastFire = 0.3` throttle, the weapon switch-back).
7. Fire, then die before delivery (shot dropped). This needs the damage area.

---

## 14. Open questions

1. **Answered (plan 4, D1, D2): Model R with δ = 10 ms.** **Which delivery model should the port adopt?** Real OLX is wall-clock dependent. Model R
   (lists span two frames, processed before frame F+2) matches a 95–100 FPS main loop; Model S
   (every frame) is simpler. Model R also reproduces the multi-shot quirks that players get
   with continuous fire.
   - Both models need the free parameter δ in `tLX->currentTime := simulationTime + δ`. Real
     play has δ ∈ (0, 10] ms; δ = 10 is the "frame loop just caught up" steady state.
   - δ shifts every projectile's clock origin, and thus which frame it first moves in.
   - The oracle hook and `rl-sim` must use the same model and δ. This decision belongs to the
     user and lead.
2. **Answered (plan 4, D1): no; one frame per main-loop iteration, recorded in STATUS.** **Should the port model iterations that run two frames, or a delivery that slips a frame?**
   Real play has them (95 FPS cap with 100 Hz frames). The recommendation is no: assume the
   idealized one frame per iteration and record it as a deliberate deviation in `STATUS.md`.
3. **Answered (plan 4): only the main formula is ported; the ping is taken as 0 (STATUS).** **The ping-based `spawnTime` fallback** (`fTime > serverTime`, only from `serverFrame` ≥
   819205 through the float round trip) depends on the measured ping. Proposal: port only the
   main formula and document that.
4. **Answered (plan 4, D2): the pinned clock covers respawn, bonus timers and spawn times.** **`tLX->currentTime` is wall clock in other gameplay code too:**
   - respawn timing (`GameServer::SimulateGame`, `CServer_Game.cpp:204-216`, and
     `fTimeofDeath = tLX->currentTime`);
   - carve-throttle bookkeeping (`fLastCarve`);
   - damage-report pacing.

   Respawn timing is therefore also nondeterministic in the oracle today. The death and
   respawn report should decide whether the hooks pin `tLX->currentTime` around the frame
   loop. That is risky: the time goes backwards against the real clock used by `CChannel` and
   ping code, but asserts are off in the Release build.
5. **Answered (plan 4): weapon ids are `ModDef` order, and the trace header's names are checked against `SimMod` in every scenario.** **Weapon id order:** `content/packs/olx-classic/mod.ron` lists weapons alphabetically. This
   was not verified against OLX's gamescript load order. If `rl-import-lgs` preserves file
   order, the two agree. Recording weapon names in the header (§13.4) settles it.
6. **Still open: the specials are covered by unit tests only (STATUS).** **Jetpack and other specials** cannot be checked against the oracle with Classic or MW 1.0,
   which contain none. A test mod would be needed if the port must be verified there.
7. **Answered (plan 4, D4): a saturating dedicated-server entity counter (`rl-sim::rng::Draws`).** **Entity pool saturation in dedicated mode** (1024 slots never freed) changes the `rand()`
   count of later gib spawns, so oracle and local play differ. The death report should confirm
   it and decide how the port models it, probably as a dedicated-only counter of entity slots.
8. **Answered (plan 4 tasks 7-8): beam hits injure through `damage::injure_worm`.** **`InjureWorm` from beam hits** (damage report, `QueueReportDamage`, kills via
   `cServer->killWorm`) is owned by the damage and death report. This report covers only the
   call site and its arguments.
