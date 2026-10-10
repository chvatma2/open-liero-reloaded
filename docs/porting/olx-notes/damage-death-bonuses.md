# M4 research 3: damage, death, respawn, bonuses, Deathmatch (OLX `d1ad953f`)

Scope: the classic LX56 engine in a **dedicated server with local worms** (the oracle
setup: `cmake -DDEDICATED_ONLY=Yes`, `bDedicated = true`, one local client `cClient`
connected to `cServer` over UDP 127.0.0.1). All paths are relative to the OLX root
(`scratchpad/openlierox`). Line numbers are at `d1ad953f`.

## 0. Summary: the findings that matter most

1. **One process, one worm list, two roles.** `cClient` simulates everything
   (worms, projectiles, bonuses) and calls `CClient::InjureWorm` for every damage.
   `cServer` does scoring, kills, lives, respawn, bonus spawning and bonus lifetime.
   Both use the same `CWorm` objects (`game.worms()`), so damage is applied once.
2. **The health check is `health < 0`, not `<= 0`.** (`CGameObject::injure`,
   `src/common/CGameObject.cpp:35-44`.) A hit that leaves exactly 0 health does not kill.
   The worm dies at the start of the **next** frame through the Gusanos think hook
   (`CWorm::think`, `src/gusanos/base_worm.cpp:365-381`). That kill is credited to
   **nobody** (killer −1), the worm's rope is cleared, and Gusanos death particles
   are spawned that consume the `rnd()` stream (section 2.6).
3. **Server timers use the wall clock.** Respawn delay, bonus spawn interval and
   bonus lifetime compare `tLX->currentTime` (`Game::frame`, `src/game/Game.cpp:699`,
   wall clock) with times taken from either the wall clock or `GetPhysicsTime()`
   (simulation clock). Under `simStep` they are **not deterministic**. The oracle
   hooks must pin `tLX->currentTime` to the simulation clock (section 11).
4. **Spawn positions come from a separate Mersenne Twister, not `rand()`.**
   `CMap::FindSpot` uses `rnd()`, a `boost::uniform_01<boost::mt19937>` that holds
   its **own copy** of the engine, default-seeded (5489). `simSetSeed` (`srand`)
   does not touch it (`src/util/math_func.cpp:3-6`).
5. **Cosmetic code consumes `rand()` in dedicated mode.** Blood, gibs, the death
   sound choice and bleeding all evaluate `GetRandomVec()`/`GetRandomNum()`/
   `GetRandomInt()` before calling the no-op `SpawnEntity`/`StartSound`. Gibs
   consume 2 more inside `SpawnEntity` while the never-freed entity pool (1024
   slots) has room. The port must replay these counts exactly.
6. **Bonuses exist twice.** The server keeps `cServer->cBonuses` (no physics). The
   client keeps `cClient->cBonuses` (physics, pickup). Network packets between them
   add 1-2 frames of latency in spawning, grabbing and destroying.
7. **With default settings, deaths are not counted at a score of 0.**
   (`AllowNegativeScore` = false, `CWorm::addDeath`.) `iDeaths` only grows once the
   worm has at least one kill.
8. **With default settings, respawn happens exactly when `RespawnTime` elapses.**
   `MaxRespawnTime` defaults to 0 (≥ 0 means enabled), so the server respawns the
   worm with `FindSpot` once `RespawnTime` has passed. No input is needed.

## 1. Frame structure (where each piece runs)

`Game::frameInner` (`src/game/Game.cpp:729-996`), once per Game::frame (wall-clock iteration):

```
HandlePendingCommands(); DedicatedControl::GameLoop_Frame();   // oracle commands
cClient->ReadPackets();      // client parses S2C_SPAWNBONUS / S2C_DESTROYBONUS here
cServer->ReadPackets();      // server parses C2S_GRABBONUS / C2S_REPORTDAMAGE here
prepare unprepared worms (lives/kills/health=100, ChangeGraphics, Prepare, PrepareWorm)  // 845-890
while (manualStep ? budget > 0 : simulationTime < tLX->currentTime) {     // 917
    if (state == Playing && !paused) serverFrame++;
    { GusSpeedScope; gusLogicFrame(); }     // worm think(): health<=0 -> die()   (945-949)
    cClient->Frame();                        // CClient::Simulation (LX56 sub-steps)
    if (isServer()) cServer->Frame();       // GameServer::SimulateGame
    simulationTime += frameDt;               // 956
}
cClient->Draw(...)
cClient->SendPackets(); cServer->SendPackets();     // packets leave here
```

`CClient::Simulation` (`src/client/CClient_Game.cpp:60-222`), per 11 ms sub-step:

```
for each worm w (game.worms() order):
    wasShootingBefore = w->tState.bShoot; oldWeapon = w->getCurWeapon()->weapon();
    PhysicsEngine::simulateWorm(w, w->getLocal())     // getInput, bleed, carve, move, weapon reload
    if (game.gameOver) continue;
    if (w->getAlive()) {
        mat = map->getMaterial(clamp(x), clamp(y)); if (mat.damage) InjureWorm(w, mat.damage, -1);  // 121-126
        if (FT_Bonuses) for n in 0..32: if used && CheckBonusCollision && (local || ssh) && GiveBonus
                                          -> DestroyBonus(n, local, id)                    // 130-151
    }
    if (isServer()) { if (bShoot && alive) cServer->WormShoot(w); FinalProj handling }   // 155-163
    if (alive) { if (bShoot) PlayerShoot(w); laser sight ... }
if (!bDedicated) SimulateEntities(...)           // skipped in dedicated -> entities never freed
if (shouldDoProjectileSimulation()) simulateProjectiles(...)   // projectile damage -> InjureWorm
simulateBonuses(cBonuses, 32)                    // client bonus physics
```

`GetPhysicsTime()` is `game.simulationTime` (`src/common/Physics.cpp:32-34`). It is the same
for every sub-step in a frame (the Rust `World::time`).

`GameServer::SimulateGame` (`src/server/CServer_Game.cpp:167-261`), once per frame, after the client:

```
if (recheckGame) {                        // set by BeginMatch/killWorm; never reset -> runs every frame
    if (state == Preparing) CheckReadyClient();
    if (!gameOver && state == Playing && gameMode->CheckGameOver()) GameOver();
    team scores ...
}
if (state != Playing) return;
if (gameOver && (gameOverTime > LX_ENDWAIT(9 s) || (bDedicated && localWorms<=1)) && !localGame) gotoLobby
if (paused) return;
respawn loop                    // 2.x
bonus lifetime loop             // 4.2
bonus spawn                     // 4.1
flags; gameMode->Simulate(); time limit -> RecheckGame()
```

## 2. Damage and death

### 2.1 `CClient::InjureWorm` — `src/client/CClient_Game.cpp:335-460`

Callers in our scope: projectile `PJ_INJURE`/`PJ_INJUREWORM` hits
(`src/common/PhysicsLX56_Projectiles.cpp:812-824`), `CClient::Explosion` (worms
within 5 px, `CClient_Game.cpp:320-329`), beams (`CClient_Game.cpp:1028-1031`),
material damage (`CClient_Game.cpp:121-126`) and the health bonus (`CWorm::GiveBonus`,
negative damage). In a dedicated local game: `game.isServer()` = true,
`w->getLocal()` = true for every worm, the server version ≥ 0.58.1 and
`tLXOptions->bServerSideHealth` = false (`Options.cpp:332`).

Near-C++, keeping the operation order (dedicated, local worm):

```cpp
void InjureWorm(CWorm* w, float damage, int owner, const std::string& weapon = "") {
    if (!w->getAlive()) return;
    CWorm* ownerWorm = (owner >= 0 && owner < 32) ? game.wormById(owner, false) : NULL;
    if (damage > 0) w->setLastWeaponHitBy(weapon);               // kill message only
    bool me = ownerWorm && ownerWorm->getID() == w->getID();
    if (ownerWorm) damage *= ownerWorm->damageFactor();          // float * float
    if (w->shieldFactor() == 0) {
        if (damage > 0)      damage = w->getHealth() + 1;
        else if (damage < 0) damage = MIN(w->getHealth() - 100, 0.0f);
    } else
        damage /= w->shieldFactor();
    // team / self protection only stops POSITIVE damage with a known owner
    if (damage > 0 && ownerWorm && isTeamGame() && !Lobby[FT_TeamInjure] && !me
        && w->getTeam() == ownerWorm->getTeam()) return;          // isTeamGame(): GMT_TEAMS; false in DM
    if (damage > 0 && ownerWorm && !Lobby[FT_SelfInjure] && me) return;
    float realdamage = MIN(w->getHealth(), damage);
    if (realdamage > 0) QueueReportDamage(...);                  // network, see 2.5
    if (ownerWorm) { w->damageReport[owner] += realdamage; ...}  // HUD only
    if (ownerWorm) ownerWorm->addDamage(realdamage, w, false);   // score "damage" (2.5)
    if (w->injure(damage)) {                                      // FULL damage, not realdamage
        // dead (health went below 0)
        cServer->killWorm(w->getID(), owner);                    // 2.3
        int s = GetRandomInt(2);                                  // rand x1  (death sound)
        StartSound(...);                                          // no-op in dedicated
        for (int n = 0; n < 7; n++)
            SpawnEntity(ENT_GIB, 0, pos, GetRandomVec()*80, Color(), w->getGibimg());   // rand x2 (+2 inside, 2.7)
        int amount = (int)(50.0f * ((float)iBloodAmount / 100.0f));   // = 50
        for (int i = 0; i < amount; i++) {
            float sp = GetRandomNum()*100 + 50;                   // rand x1
            SpawnEntity(ENT_BLOODDROPPER, 0, pos, GetRandomVec()*sp, ...);   // rand x2
            SpawnEntity(ENT_BLOOD,        0, pos, GetRandomVec()*sp, ...);   // rand x2
            SpawnEntity(ENT_BLOOD,        0, pos, GetRandomVec()*sp, ...);   // rand x2
        }
    }
    if (damage > 0) {                                             // also after a kill
        float amount = CLAMP(damage, 10.f, 100.f) * ((float)iBloodAmount / 100.0f);
        for (ushort i = 0; i < amount; i++) {                     // ceil(amount) iterations, >= 10
            float sp = GetRandomNum() * (50 + damage * 2);        // rand x1
            SpawnEntity(ENT_BLOODDROPPER, 0, pos, GetRandomVec().multPairwise(sp, sp*4), ...); // x2
            SpawnEntity(ENT_BLOOD, 0, pos, GetRandomVec()*sp, ...);                           // x2
            SpawnEntity(ENT_BLOOD, 0, pos, GetRandomVec()*sp, ...);                           // x2
        }
    }
}
```

Quirks:
- `damage` used for the blood amount is the value **after** the damage and shield
  factors. With shield 0 it is `health + 1`.
- Healing (negative damage) never makes blood, never counts in the damage score,
  and is not clamped at 100 (only the health bonus clamps, 4.5).
- A dead worm cannot be injured (the first line). Material damage is 0 for
  every `.lxl` material: LX levels use material indices 0-2 only
  (`src/gusanos/material.h` `indexFromLxFlag`). Only Gusanos material 5 has
  damage 100 (`src/gusanos/level.cpp:129`). So the material check never fires in scope.
- The weapon name is kept only for the kill message.
- With a negative damage factor (allowed range −2..10), a health bonus becomes
  positive damage. It then passes through `SelfInjure` and can kill the worm.

`rand()` per injury (positive `damage`): `7 × ceil(clamp(damage,10,100))`.
Per kill, add `1 + 7×(2 or 4) + 350` (gibs: section 2.7).

### 2.2 `CGameObject::injure` — `src/common/CGameObject.cpp:35-44`

```cpp
bool injure(float damage) { health -= damage; if (health < 0.0f) { health = 0.0f; return true; } return false; }
```
There is no upper clamp. **Exactly 0 is alive** (see 2.6).

### 2.3 `GameServer::killWorm` — `src/server/CServer_Game.cpp:108-162`

```cpp
void killWorm(int victim, int killer, int suicidesCount = 0) {
    if (game.gameOver) return;                    // kill ignored: worm stays alive with 0 hp!
    if (victim < 0 || victim >= 32) return;
    if (victim != killer) suicidesCount = 0;
    if (victim == killer && suicidesCount == 0) suicidesCount = 1;
    CWorm* vict = game.wormById(victim); CWorm* kill = game.wormById(killer, false);  // -1 -> NULL
    if (vict->getLives() < 0 && (int)gameSettings[FT_Lives] >= 0) {   // "cheat prevention"
        vict->setLives(WRM_OUT); return; }                             // worm NOT killed
    if (suicidesCount > 1 && Lives != WRM_UNLIM) vict->setLives(MAX(0, lives - suicidesCount + 1)); // never in scope
    game.gameMode()->Kill(vict, kill);            // 2.4
    SendWormScore / SendWormDied to clients       // local client: return early (CServer_Send.cpp:1122-1172)
    DedicatedControl WormDied signal; SendTeamScoreUpdate(); RecheckGame();
}
```

The local client never receives `S2C_WORMDOWN`, `S2C_SPAWNWORM` or `S2C_SCOREUPDATE`
(`CServerNetEngine::SendWormDied/SendSpawnWorm/SendWormScore` return for
`isLocalClient()`). So `CClientNetEngine::ParseWormDown`
(`src/client/CClient_Parse.cpp:1817-1860`), with its own gibs, blood and rope
unattach, does **not** run in our setup.

Quirk: if the game is already over, `killWorm` returns and the victim stays alive
with health 0. The next frame's `CWorm::think` then calls `die()` → `killWorm`,
which returns again. The worm keeps 0 health and stays "alive" while the game is over.

### 2.4 `CGameMode::Kill` (Deathmatch uses the base class) — `src/server/CGameMode.cpp:69-252`

Gameplay-relevant parts (messages and sounds omitted):

```cpp
if (killer) {
    if (killer == victim)                              killer->addSuicide();     // gated, 2.8
    else if (GameTeams() > 1 && same team)             killer->addTeamkill();    // gated (not in DM)
    else                                               killer->addKill();        // iKills++ always
}
// killer == NULL (owner -1, or the 0-hp die() path): no attribution at all
iKillsInRow / iDeathsInRow bookkeeping (sounds/messages only)
victim->Kill(true);                                    // 2.5
```
The killer may already be dead (its projectile hit later). It still gets the kill.

### 2.5 `CWorm::Kill` — `src/common/CWorm.cpp:1399-1413`

```cpp
void Kill(bool serverside) {
    bAlive = false;
    fTimeofDeath = GetPhysicsTime();          // simulation time of the current frame
    bCanRespawnNow = false; bRespawnRequested = false;
    if (serverside) { addDeath(); if (iLives != WRM_UNLIM) iLives = iLives - 1; }
}
```
- Lives: `Lives = N ≥ 0` means the worm can die N+1 times. At lives 0, one more
  death gives −1 = `WRM_OUT` (`CWorm.h:85-86`: OUT = −1, UNLIM = −2).
- Death leaves these unchanged: position, velocity, health (stays 0), weapons,
  `iCurrentWeapon`, `tState` (including `bShoot`) and the **rope**. The normal
  path does not clear it; it stays as it was, unsimulated, until respawn.
  Projectiles already fired keep flying and keep their owner id.
- The dead worm is not removed from `game.worms()`. `simulateWorm` still calls
  `getInput()` (2.10) and then returns (`PhysicsLX56.cpp:300-311`). The worm
  skips damage and bonus checks and shooting. Explosions only hit `aliveWorms()`,
  and ropes ignore dead worms (already ported in `rope.rs`).
- FinalProj quirk (weapons area): `tState.bShoot` stays true through death and is
  reset at `Spawn` (`clearInput`). So `WormShootEnd` never fires for a worm
  that dies while shooting.

Score "damage" (`fDamage`), from `CWorm_addDamage` (`src/common/CWorm.cpp:1600-1627`):
```cpp
if (damage < 0) return;
if (self) { if (SuicideDecreasesScore) fDamage -= damage; }
else if (GMT_TEAMS && same team) { if (TeamkillDecreasesScore) fDamage -= damage; if (CountTeamkills) fDamage += damage; }
else fDamage += damage;
```
**Double count:** `InjureWorm` calls `addDamage(realdamage, w, false)` straight
away. It also queues `C2S_REPORTDAMAGE`, batched by `tLX->currentTime`
(`CClient_Send.cpp:345-378`). The server parses that a frame or more later
(`CServerNetEngineBeta9::ParseReportDamage`, `src/server/CServer_Parse.cpp:960-988`)
and calls `offender->addDamage(damage, w, true)` on the **same** worm object.
So `fDamage` ends up about 2× the real damage, part of it late. Unknown owners
(−1 → byte 255) are dropped. `fDamage` only breaks score ties (2.9).
**Recommendation:** keep it out of the oracle, or document a deviation.

### 2.6 The exact-zero-health path (gameplay quirk)

`gusLogicFrame()` (`src/gusanos/gusanos.cpp:63-97`) runs at the start of every frame,
before `cClient->Frame()`. For LX mods OLX still loads the default `Gusanos` mod
(`Game.cpp:450-460`). Its weapons exist (`share/gamedir/Gusanos/weapons`), so
`gusGame.isLoaded()` is true. Worms are in `game.objects` (`Game::onPrepareWorm`,
`Game.cpp:1137-1140`), so `CWorm::think()` runs every frame while
`shouldDoPhysicsFrame()`:

```cpp
void CWorm::think() {                        // base_worm.cpp:365-381
    if (!bPrepared) return;
    if (!gusEngineUsed()) { if (getAlive() && health <= 0) die(); return; }
}
void CWorm::die() { if (weOwnThis() || !m_node) base_die(); }      // 655-659
void CWorm::base_die() {                                             // 623-653
    bAlive = false; fTimeofDeath = tLX->currentTime;                 // overwritten by Kill()
    if (m_owner) gusGame.displayKillMsg(...);
    if (weOwnThis() && game.isServer())
        cServer->killWorm(getID(), m_lastHurt ? ... : -1);           // m_lastHurt is always NULL for LX -> -1
    cNinjaRope.write().remove();                                     // CNinjaRope::remove() == Clear()
    m_timeSinceDeath = 0;
    if (gusGame.deathObject) gusGame.deathObject->newParticle(...);  // Gusanos/objects/death.obj
}
```

Effects of a hit that leaves exactly 0 health:
- The worm lives through the rest of the frame. It bleeds (health < 15, 2.11) and
  keeps moving and shooting on the remaining sub-steps.
- The next frame, before physics, it dies with killer −1: nobody gets a kill, the
  victim gets a death (gated, 2.8) and loses a life. Its time of death is that
  frame's `GetPhysicsTime()`.
- Its rope is cleared. On the normal death path the rope is left as it was.
- `death.obj` spawns Gusanos particles: `shoot_particles` of meat, bloodgen and blood,
  plus `play_sound_static`. These consume **`rnd()`** (`game_actions.cpp:198`, the
  same stream as `FindSpot`), `midrnd()` and `rndInt` (`rndgen`) now and in later
  frames (particle timers). They never call `rand()`. They do shift every later
  `FindSpot` (spawn positions, bonus positions). Replicating this exactly means
  simulating Gusanos particles.
  **Recommendation:** port the gameplay part (delayed death, no killer, rope
  cleared). Document the `rnd()`-stream divergence after such a death as a
  deviation, unless the oracle shows it is needed.
  **Oracle (plan 4 task 7):** confirmed. In `weapons/mw-1-0/assassin-s-shot`,
  `flak` and `plasma-launcher` the dummy dies at exactly 0 health, and OLX's
  next `FindSpot` starts 597, 544 and 669 draws into the stream: the particles
  draw over their whole life, not a fixed count, so only simulating them
  would reproduce it. Kept as the D9 deviation.

### 2.7 The entity pool and rand inside `SpawnEntity` — `src/client/Entity.cpp:161-205`

```cpp
void SpawnEntity(int type, int type2, CVec pos, CVec vel, Color c, SmartPointer<SDL_Surface> img) {
    if (!bParticles && (type == ENT_PARTICLE || type == ENT_BLOOD)) return;   // bParticles default true (Options.cpp:215)
    if (type == ENT_GIB && img == NULL) return;
    entity_t* ent = tEntities.getNewObj(); if (!ent) return;                 // FastVector<entity_t,1024>
    ... ; if (type == ENT_GIB) { fAnglVel = fabs(GetRandomNum())*20; iRotation = (int)(fabs(GetRandomNum())*3); }  // rand x2
}
```
- Arguments, including `GetRandomVec()`, are evaluated **before** the call, so
  their `rand()` calls happen even when the entity is dropped.
- In dedicated mode `SimulateEntities` is skipped (`CClient_Game.cpp:210-211`), so
  entities are never freed. The pool is emptied only by `ClearEntities()` in
  `prepareGameloop` (`Game.cpp:545`). Once 1024 entities have been accepted,
  `getNewObj()` returns NULL forever, and **gibs stop consuming their 2 extra `rand()`**.
- The gib image `bmpGibs` comes from `ChangeGraphics()` (`Game.cpp:863`,
  `CWorm.cpp:655-667`), which loads `data/gfx/giblets.png` (present in the gamedir).
  `LoadGameImage` works in dedicated mode (`GfxPrimitives.cpp:2934-2996`), so the
  image is very likely non-NULL. Check this with a hook (section 11).
- **Port rule:** keep a saturating counter (0..1024) of accepted `SpawnEntity`
  calls since the match began. Count every call site that passes the two early
  returns: blood, blood droppers, gibs (with image), dig and rope dirt
  particles, explosions, smoke, the bonus `ENT_SPAWN`, laser sights (one **per
  sub-step** while a laser-sight weapon is held), beams, jetpack spray, air-jump
  sparkles, and so on. The list is in `grep SpawnEntity(`. A gib costs 4 `rand()`
  while the counter is below 1024, and 2 after.

### 2.8 Score helpers — `src/common/CWorm.cpp:1415-1449`

```cpp
int getScore() const {
    int score = iKills;
    if ((float)DeathDecreasesScore > 0.0f) score -= (int)(iDeaths * (float)DDS + 0.01f);
    if (SuicideDecreasesScore)  score -= iSuicides;
    if (TeamkillDecreasesScore) score -= iTeamkills;
    if (CountTeamkills)         score += iTeamkills;
    return score;
}
void addDeath()    { if (!AllowNegativeScore && getScore() <= 0) return; iDeaths++; }
void addSuicide()  { same gate; iSuicides++; }
void addTeamkill() { same gate; iTeamkills++; }
```
With the defaults (AllowNegativeScore false, DDS 0), **deaths and suicides do not
count while the worm has 0 kills.** These read `cClient->getGameLobby()`, which
equals `gameSettings` in a local game.

### 2.9 Who applies what (local dedicated game)

| Effect | Object | Where |
|---|---|---|
| Health change, blood/gib `rand()` | client | `CClient::InjureWorm` |
| `fDamage` (first count) | client | `addDamage(..., false)` |
| Kill, lives, kills, deaths, suicides | server, called **synchronously** from the client's InjureWorm | `killWorm` → `CGameMode::Kill` → `CWorm::Kill(true)` |
| `fDamage` (second count, late) | server | `ParseReportDamage` |
| Zero-health death | gus think, next frame | 2.6 |
| Respawn | server | `SimulateGame` |

### 2.10 Dead worms' input — `src/common/CWormHuman.cpp:112-119`, `src/common/CWormBot.cpp:1022-1027`

```cpp
// human handler, inside getInput(), after reading the isDownOnce latches:
if (!m_worm->getAlive()) { if (m_worm->bCanRespawnNow && jump) m_worm->bRespawnRequested = true; clearInput(); return; }
// bot: if dead { if (bCanRespawnNow) bRespawnRequested = true; return; }
```
`clearInput()` resets every `CInput` latch (`CWormHuman.cpp:597-613`), but not `tState`.
The current port (`physics.rs` `get_input`: `if !w.alive { keys.reset(); return; }`) matches,
except for the `bRespawnRequested` line. That line only matters when `MaxRespawnTime < 0`.

### 2.11 Bleeding — `src/common/PhysicsLX56.cpp:317-330`

This code runs inside `simulateWorm` for **alive** worms, on every sub-step, after
`getInput` and before carving:

```cpp
if (worm->getHealth() < 15 && worm->isVisibleForEverybody()) {     // bVisibleForWorm empty in DM -> true
    if (GetPhysicsTime() > worm->getLastBlood() + 2.0f) {          // AbsTime + float: +2000 ms, strict
        worm->setLastBlood(GetPhysicsTime());
        const float amount = ((float)iBloodAmount / 100.0f) * 20;  // = 20.0f
        for (short i = 0; i < amount; i++) {
            SpawnEntity(ENT_BLOOD, 0, pos, GetRandomVec()*100, Color(200,0,0), NULL);   // rand x2
            SpawnEntity(ENT_BLOOD, 0, pos, GetRandomVec()*100, Color(180,0,0), NULL);   // rand x2
        }
    }
}
```
- **Yes, it consumes `rand()` in a dedicated server:** 80 calls per bleed, and 40
  entities while the pool has room.
- `fLastBlood` is initialised to `AbsTime()` = 0 in the constructor
  (`CWorm.cpp:235`). Nothing resets it: not `Prepare`, not `Spawn`.
- The time is the frame time, the same for every sub-step, so a worm bleeds at most once per frame.
- The first bleed requires `simulationTime > 2000 ms`. `simulationTime` starts at
  `GetTime()` (ms since process start) in `prepareGameloop` (`Game.cpp:670`), a
  wall-clock-dependent base. Pin it (section 11).
- `iBloodAmount` defaults to 100 (`Options.cpp:213`). Dedicated mode does not override it.

## 3. Respawn

### 3.1 The server's respawn loop — `src/server/CServer_Game.cpp:200-225`

```cpp
for each worm w (id order):
  if (!w->getAlive() && w->getLives() != WRM_OUT && w->bWeaponsReady) {
    if (!w->haveSpawnedOnce() || tLX->currentTime > w->getTimeofDeath() + TimeDiff((float)RespawnTime)) {
      if ((float)MaxRespawnTime >= 0.f && tLX->currentTime > w->getTimeofDeath() + TimeDiff((float)MaxRespawnTime))
          SpawnWorm(w, "max respawn-time reached");
      else if (w->bRespawnRequested) SpawnWorm(w, "respawn requested");
      else if (!CanWormHandleClientSideRespawn(w)) SpawnWorm(w, ...);   // false for the local client (>=0.59.10)
      else w->bCanRespawnNow = true;                                    // wait for the input handler's request
    }
  }
```
- `TimeDiff(float s)` is `(Uint64)(s * 1000.0f)` (`include/olx-types.h:31`): it
  truncates (0.7 → 699 ms). The comparisons are strict `>`.
- With the defaults (`RespawnTime` 2.5, `MaxRespawnTime` **0**), the worm is
  spawned by the server in the first frame where
  `currentTime > timeOfDeath + 2500`. With `currentTime` pinned to the frame's
  simulation time + 10 ms (the oracle, spec §18 D2), that is **250 frames**
  after the frame of death (`explode-kill-respawn`: death in frame 61, respawn
  in frame 311). The respawn is synchronous (the local client shares the worm;
  `SendSpawnWorm` returns for it), so no message latency applies. The respawn
  happens in the server phase, so the first physics sub-step comes in the frame after.
- With `MaxRespawnTime < 0`: `bCanRespawnNow` is set first, the jump key (once,
  in a later sub-step) sets `bRespawnRequested`, and the next `SimulateGame` spawns
  the worm. Bot handlers request it immediately.
- Respawns happen even after game over, but `SpawnWorm` returns at once while
  `gameOver`. A worm whose lives are `WRM_OUT` is never respawned.
- At match start (`!haveSpawnedOnce`), every ready worm is spawned at once by
  `FindSpot`. This also happens in the oracle before its `spawnWorm` commands
  (open question 3).

### 3.2 `GameServer::SpawnWorm` — `src/server/CServer_Game.cpp:38-104`

```cpp
if (game.gameOver || Worm->isSpectating()) return;
if (_pos) pos = *_pos;                                       // oracle spawnWorm "x,y" (int -> float)
else if (map->getPredefinedSpawnLocation(Worm, &pos)) ;      // gusanos/level.cpp:414-431; LX levels have none, no RNG used
else if (GameMode == CTF) ...
else if (RespawnGroupTeams && gameMode->GameTeams() > 1) pos = map->FindSpotCloseToTeam(team, Worm);  // TDM: 101 FindSpots
else pos = map->FindSpot();                                  // Deathmatch
if (pos.x == -1 && pos.y == -1) pos = map->FindSpot();
if (!gameMode->Spawn(Worm, pos)) return;                     // DM: true
Worm->Spawn(pos);
if (Worm->getLives() == WRM_OUT) Worm->setLives(Lives < 0 ? WRM_UNLIM : 0);   // only for forced spawns
send spawn/score (local client: no-op, but SendSpawnWorm sets bSpawnedOnce);
if (EmptyWeaponsOnRespawn) SendEmptyWeaponsOnRespawn(Worm);  // all slots: Charge=0, Reloading=true (CServer_Send.cpp:1066-1072)
```
The position from `FindSpot` is **fractional**. The local worm keeps the float;
only remote clients get `(int)pos`.

### 3.3 `CMap::FindSpot` — `src/common/CMap.cpp:2381-2396`, `src/game/CMap.h:420-461, 291-302`

```cpp
CVec FindSpot() {
    CVec pos; size_t tries = 1000;
    do {
        pos.x = float((double)rnd() * Width);     // rnd(): double in [0,1); Width is uint
        pos.y = float((double)rnd() * Height);
        tries--;
    } while (tries > 0 && !IsGoodSpawnPoint(flags, pos));
    return pos;                                   // after 1000 failed tries: the last (bad) position
}
bool IsGoodSpawnPoint(flags, CVec p) { long x = (long)p.x, y = (long)p.y;
    for (yy = y-3; yy < y+3; ++yy) for (xx = x-3; xx < x+3; ++xx)   // 6x6, upper bounds EXCLUSIVE
        if (GetPixelFlag(xx, yy) & PX_ROCK) return false;            // out of map counts as PX_ROCK
    return true; }
```
- Only rock is rejected. Dirt is fine, because the spawn hole carves it.
- `rnd()` (`src/util/math_func.cpp:3-4`):
  `boost::uniform_01<boost::mt19937> rnd(rndgen);`. That is
  `backward_compatible_uniform_01<mt19937, double>`, which stores the engine **by
  value**: a copy of `rndgen`, made at static init in the default state
  (seed 5489). Each call is `double(mt() - 0) * (1.0 / 4294967296.0)`, retried
  while ≥ 1, which never happens. That is `mt()/2^32` exactly.
  `boost::mt19937` equals `std::mt19937`.
- This stream is independent of `rand()`, `rndgen` and `midrnd` (another copy),
  and `simSetSeed` does not reseed it. Other users of the `rnd` copy: Gusanos
  code only (particles, sounds, Lua `random`, water).
- `FindSpotCloseToPos` (`CMap.cpp:2399-2429`), used by TDM and CTF only: one
  FindSpot, then 100 more, keeping the best score. Not needed for Deathmatch.

### 3.4 `CWorm::Spawn` — `src/common/CWorm.cpp:591-651`

```cpp
if (bSpectating) return;
vPos = vDrawPos = vLastPos = ... = position; velocity = 0;
health = 100.0f; bSpawnedOnce = true; bCanRespawnNow = bRespawnRequested = false;
resetAngleAndDir();                     // fAngle = 0, fAngleSpeed = 0, face = RIGHT
fMoveSpeedX = 0; iMoveDirectionSide = DIR_RIGHT;
fLastInputTime = GetPhysicsTime();
posRecordings.clear(vPos); cNinjaRope.Clear();
fFrame = 0; bOnGround = false;
for each slot: Charge = 1.f; Reloading = false; LastFire = 0.f;   // WeaponId kept; iCurrentWeapon kept
m_lastHurt = NULL; gus weapons reset;
fSpawnTime = GetPhysicsTime();
if (bLocal) { clearInput(); /* fLastInputTime = now; tState.reset(); handler->clearInput() (all latches) */
              m_inputHandler->onRespawn(); }        // human: no-op; bot: AI reset
cDamageReport.clear();
map->CarveHole(SPAWN_HOLESIZE /*4*/, pos(), InfiniteMap);   // no rand, dirt not counted (CMap.cpp:1007+)
bAlive = true;
```
Spawning leaves these unchanged: lives, kills, `iDirtCount`, `fLastBlood`,
`fLastAirJumpTime` (reset only by `StartGame`), `bCanAirJump`, speed, damage and
shield factors, and the weapon IDs. Weapons are **not** re-randomised on respawn.
Input timing: the first `getInput` after the respawn sees
`dt = its frame time − spawn frame time` (10 ms or more).

## 4. Bonuses

Constants (`include/CBonus.h`): `MAX_BONUSES 32`, `BONUS_LIFETIME 60`
(hard-coded flashing threshold), `BNS_WEAPON 0`, `BNS_HEALTH 1`. `CBonus::Spawn`
(`src/common/CBonus.cpp`) sets: `bUsed`, `vPos`, `vVelocity = 0`, type, weapon,
`fLife = 0`, `fSpawnTime = tLX->currentTime`, `fFlashTime = 0`, and the weapon
name. The server (`cServer->cBonuses`) and the client (`cClient->cBonuses`,
recorded by the oracle) keep separate arrays.

### 4.1 Server spawn — `src/server/CServer_Game.cpp:246-250, 266-316`

```cpp
// in SimulateGame, after the lifetime loop:
if (tLX->currentTime - fLastBonusTime > BonusFreq && Bonuses && !gameOver) { SpawnBonus(); fLastBonusTime = tLX->currentTime; }

void SpawnBonus() {
    CVec pos = map->FindSpot();                                         // rnd() x2 per try
    int type = (GetRandomInt(999) >= (float)BonusHealthToWeaponChance * 1000.0f) ? BNS_HEALTH : BNS_WEAPON;  // rand x1
    spot = first i in 0..32 with !cServer->cBonuses[i].used; if none return;   // FindSpot + rand already consumed
    goodWpns = [i for i in 0..numWeapons if wpnRest->getWeaponState(name) != wpr_banned];  // "bonus" state allowed
    int wpn = goodWpns.size() ? randomChoiceFrom(goodWpns) : -1;        // rand x1, also for HEALTH bonuses
    if (type == BNS_WEAPON && wpn < 0) return;
    cServer->cBonuses[spot].Spawn(pos, type, wpn, gs);                  // float pos, no hole carved server-side
    send S2C_SPAWNBONUS {type, [wpn if weapon], spot, (int)pos.x (2 bytes), (int)pos.y (2 bytes)}  // reliable
}
```
- `fLastBonusTime` is set to `tLX->currentTime` in `GameServer::PrepareGame`
  (`CServer.cpp:383`), when the game starts (wall clock). So the first bonus comes
  `BonusFreq` (30 s, strictly more) after preparation.
- `GetRandomInt(999)` returns 0..999. A weapon bonus comes when `r < chance*1000`.
  With 0.5: `r < 500`, a 50 % chance.
- `randomChoiceFrom` is `GetRandomLong(size-1)` (`include/Utils.h:38-43`).
- Weapon restrictions come from `gameSettings[FT_WeaponRest]`, default
  `"Standard 100lt"`, loaded from `cfg/presets/<mod>/Standard 100lt.wps`
  (`CWpnRest::loadList`, `src/common/CWpnRest.cpp:180-210`). Lookups ignore case
  (`CWpnRest.h:58-68`). Mod weapons missing from the file are added as enabled
  (`updateList`). **For Classic, this file bans Bazooka, Grenade, Larpa, Laser,
  Minigun and others (state 2).** They are never bonus weapons (nor random
  weapons) in the oracle setup.

### 4.2 Server lifetime — `src/server/CServer_Game.cpp:227-244`

```cpp
if (Bonuses) for i in 0..32: if (used && tLX->currentTime - spawnTime > BonusLife) { used = false; send S2C_DESTROYBONUS(i); }
```
- This loop runs **before** the spawn check in the same call, so a freed slot can be reused in the same frame.
- The server's slot is **not** freed when a worm picks the bonus up
  (`ParseGrabBonus` never calls `setUsed(false)`, `CServer_Parse.cpp:834-903`),
  nor when an explosion destroys it client-side. A server slot is busy for the
  whole `BonusLife` (60 s). With the defaults at most about 2 bonuses exist at once.

### 4.3 Client spawn — `CClientNetEngine::ParseSpawnBonus`, `src/client/CClient_Parse.cpp:1464-1516`

Runs when the packet is read: `cClient->ReadPackets()` at the start of the next Game::frame.

```cpp
type = clamp(readByte, 0, 2); if (type == WEAPON) wpn = readInt(1); id = readByte(); x = readInt(2); y = readInt(2);
reject if state < Preparing, id out of range, x > Width || x < 0, y > Height || y < 0, invalid weapon
cBonuses[id].Spawn(CVec((float)x, (float)y), type, wpn, gs);   // integer position
map->CarveHole(SPAWN_HOLESIZE /*4*/, p, InfiniteMap);          // TERRAIN CHANGE, client side only, no rand
SpawnEntity(ENT_SPAWN, ...);                                    // takes an entity slot
```

### 4.4 Client bonus physics — `src/common/PhysicsLX56.cpp:618-715`

`simulateBonuses` runs after the projectiles on every sub-step, only if
`Bonuses`, for used bonuses in index order.

```cpp
float dt = LX56PhysicsDT.seconds();            // 11 * 0.001f
life += dt;
if (life > BONUS_LIFETIME - 3) { flashTime += dt; if (flashTime > 0.3f) flashTime = 0; }   // cosmetic, 57 s
vel.y += 80 * dt;
pos += vel * dt;
px = (int)pos.x; py = (int)pos.y;
for (y = py-2; y <= py+2; y++) {
    if (y < 0) continue;
    if (y >= mh) { /* !InfiniteMap */ vel = 0; pos.y = (float)y - 2; return; }
    for (x = px-2; x <= px+2; x++) {
        if (x < 0) continue;
        if (x >= mw) { vel = 0; pos.y = (float)y - 2; return; }
        if (GetPixelFlag(x, y) & (PX_DIRT | PX_ROCK)) { vel = 0; pos.y = (float)y - 2; return; }
    }
}
```
Quirks:
- A resting bonus snaps back to `hitRow − 2` on every sub-step.
- If solid ground is found in the **top** row (`py − 2`), the bonus moves **up**
  2 px per sub-step until it is free.
- Only the bottom and right map edges count; the left edge is skipped.
- Bonuses do not move sideways.

### 4.5 Pickup — `CClient_Game.cpp:128-151`, `src/common/CWorm.cpp:1454-1506`

This happens per worm, after that worm's `simulateWorm`, if the worm is alive and
the game is not over. The loop checks all 32 slots, so a worm can take several
bonuses in one sub-step. Later worms in the loop see the updated array.

```cpp
bool CheckBonusCollision(b) { CVec d = vPos - b->pos; return fabs(d.x) < 7 && fabs(d.y) < 7; }   // float
bool GiveBonus(b) {
    if (weaponSlots.size() == 0) return false;
    if (b->type == BNS_WEAPON) {
        if (0 <= b->wpn < numWeapons) { cur = writeCurWeapon();          // slot MIN(size-1, iCurrentWeapon)
            cur->WeaponId = b->wpn; cur->Charge = 1.f; cur->Reloading = false; }   // LastFire NOT reset
        return true;                                                     // picked up even if wpn invalid
    }
    if (b->type == BNS_HEALTH) {
        if (health >= 100.0f) return false;                              // stays on the map
        float h = (float)(GetRandomInt(40) + 10);                        // rand x1, 10..50
        h = MIN(h, 100 - getHealth());
        cClient->InjureWorm(this, -h, getID());                          // own id as owner (2.1)
        return true;
    }
    return false;
}
// on success:
DestroyBonus(n, local=true, id): cBonuses[n].used = false; SendGrabBonus(n, id, getCurrentWeapon());
```
The health bonus goes through `InjureWorm` with negative damage. The result is
`health += h * ownerDamageFactor / shieldFactor`. With shield 0 the worm is healed
to exactly 100. No blood, no score.

### 4.6 The server's grab handling — `CServerNetEngine::ParseGrabBonus`, `src/server/CServer_Parse.cpp:834-903`

This is read at the start of the next Game::frame.

```cpp
if (state != Playing) return; ...
CBonus* b = &server->cBonuses[id];
if (b->getUsed()) {
    if (b->type == BNS_WEAPON && 0 <= curwpn < slots) {
        wpn = &w->weaponSlots[curwpn]; old = wpn->weapon();
        if (valid b->wpn) { wpn->WeaponId = b->wpn; wpn->Charge = 1.f; wpn->Reloading = false; } else wpn->WeaponId = -1;
        if (old && wpn->weapon() != old && w->tState.bShoot) server->WormShootEnd(w, old);
    }
    send S2C_DESTROYBONUS(id)            // client clears its slot one more frame later (no-op normally)
}                                         // server slot stays used
```
Quirks:
- **The charge is restored a second time.** It happens one frame after the pickup,
  on slot `curwpn` (the slot that was current at pickup). If the worm fired the
  new weapon in between, which is possible in the very sub-step of the pickup
  (`WormShoot` runs right after the bonus check), the charge and reload state
  are reset to full.
- If the server's slot expired in that frame (4.2) and was refilled by a new
  bonus in the same `SimulateGame`, the late grab applies the **new** bonus's
  weapon to that slot. This is a rare edge case.

### 4.7 Explosions destroy client bonuses — `CClient::Explosion`, `CClient_Game.cpp:255-270`

```cpp
if (Bonuses) for i in 0..32: if used && |b.x - pos.x| <= 15 && |b.y - pos.y| <= 15 {
    b->setUsed(false);                     // client only; the server is not told
    Explosion(time, pos, 15, 5, owner);    // recursive, at the ORIGINAL explosion pos, damage 15, weapon ""
}
```
- This runs before the dirt particles, the carving and the worm damage of the outer explosion.
- Each recursive explosion carves a size-15 hole (clamped to 4) at the same place,
  hurts worms within 5 px by 15, and spends `rand()` on dirt particles
  (2× `GetRandomVec`, if dirt is found).
- The inner call goes over the bonus list again; already-destroyed bonuses are skipped.
- Explosion internals belong to the weapons research. Worm positions there use
  `posRecordings.getBest(11, (tLX->currentTime - time).ms)`, which is wall-clock again.

### 4.8 Bonus message latency (oracle frame N)

| Event | Where | Visible in trace |
|---|---|---|
| Server `SpawnBonus` | frame N, server phase | client array: frame N+1 (parsed before N+1's sub-steps, hole carved then) |
| Client pickup | frame N sub-step | client slot freed in frame N |
| Server re-applies the weapon | before frame N+1 | frame N+1 |
| `S2C_DESTROYBONUS` (grab or expiry) | sent end of N (+1) | client slot cleared the frame after |

This assumes exactly one simulated frame per Game::frame and immediate,
unthrottled loopback delivery. See section 11 and open question 1.

Confirmed by the port (plan 4 task 9) with the oracle's D10 delivery: a
placed bonus (`simPlaceBonus`) is in both arrays in the frame it is placed
and gets that frame's sub-steps; a grab in frame N re-applies the weapon
before frame N+1 and its `S2C_DESTROYBONUS` reaches the client before frame
N+2; an expiry in frame N clears the client's slot before frame N+1.

### 4.9 Also on these paths (found while porting, plan 4 task 9)

- **Beams stop at bonuses.** `CClient::ProcessShot_Beam` (`CClient_Game.cpp:986-1002`),
  unless `Bm.Damage == -1`: a beam part within 3 px of a used client bonus
  (`(p - b.pos).GetLength() < 3`) stops there in `Explosion(spawnTime, p, 0, 5, owner)`.
  This check does not read `FT_Bonuses`; with bonuses off the beam still
  stops, and the explosion (which then skips the bonus chain) leaves the bonus.
  `DrawBeam` has no such check.
- **No other projectile code reads bonuses** (`PhysicsLX56_Projectiles.cpp`,
  `CProjectile.cpp`): projectiles fly through them; only explosions destroy them.
- **The `wpn` field differs between the arrays.** The client's health bonus
  has weapon 0 (`ParseSpawnBonus` reads a weapon only for weapon bonuses);
  the server's has the random `goodWpns` choice (natural) or -1 (`simPlaceBonus`).
- **Packet widths:** `S2C_SPAWNBONUS` sends the weapon in 1 byte and the
  position as `(int)pos` in 2 bytes (`writeInt(.., 2)`, read back unsigned);
  `C2S_GRABBONUS` sends the slot, worm id and `getCurrentWeapon()` in 1 byte each.
- **`WormShootEnd` on a grab** (`ParseGrabBonus`) acts only on a `FinalProj`,
  so it is a no-op for LX56 mods.

## 5. Deathmatch rules

`CDeathMatch` (`src/server/CDeathMatch.cpp`) is the bare `CGameMode`:
`GameTeams()` = 1, `GeneralGameType` = GMT_NORMAL, `Spawn` → true,
`Shoot` → true, `Carve` → nothing.

- **Lives:** set when a worm is prepared (`Game.cpp:851`): `Lives < 0 ? WRM_UNLIM : Lives`.
- **Kills, score, death counters:** see 2.4 and 2.8.
- **`CheckGameOver`** (`CGameMode.cpp:276-360`), evaluated every frame
  (`recheckGame` is never reset):
  1. If `!cServer || lobby || gameOver`, return true.
  2. Team score limit: teams only.
  3. **Kill limit:** if `KillLimit > 0` and any worm (id order) has
     `getScore() >= KillLimit`.
  4. **Time limit:** if `TimeLimit*60 > 0` and
     `serverTime() > TimeDiff(TimeLimit*60.0f)`, where
     `serverTime() = serverFrame*10 ms` (`Game.h:111`).
  5. Unless `allowEmptyGames`: the game is over when fewer than 2 worms have
     `lives != WRM_OUT` (fewer than 1 in a local game with a single worm).
     `allowEmptyGames = AllowEmptyGames && bAllowConnectDuringGame (default
     true, Options.cpp:329) && !isLocalGame() && Lives < 0`. In the oracle it
     is true, so a single worm never ends the game.
- **`GameOver`** (`CServer.cpp:533-625`):
  - Sets `gameOver`, then sends the winner and calls `clearInput()` on every worm.
  - The client then clears local inputs each frame. `simulateWorm` skips
    `getInput` (`!game.gameOver`), and the client loop `continue`s right after
    `simulateWorm`: no pickup, no shooting, no material damage. Worm physics and
    projectiles keep running.
  - `shouldDoPhysicsFrame()` turns false `GAMEOVER_WAIT` = 3 s after game over
    (`Game.cpp:1267-1276`).
  - A dedicated server then goes to the lobby after `LX_ENDWAIT` = 9 s, or at
    once if it has at most 1 local worm.
- **Winner** (`CGameMode.cpp:388-434`):
  - If fewer than 2 worms are not OUT, the first such worm wins.
  - Otherwise `HighestScoredWorm` decides, using `CompareWormsScore`:
    - With `Lives >= 0`: a worm at or above the kill limit wins; otherwise more lives wins.
    - A non-OUT worm beats an OUT worm.
    - Then the higher score wins.
    - Last, `Round(damage1 - damage2)`. Damage is double-counted (2.5).
  - Ties keep the earlier worm.
- **Suicides:** no kill. `addSuicide` is gated by score.
  `SuicideDecreasesScore` (default false) subtracts suicides from the score.
- **Teamkills:** not applicable in DM (`GameTeams() == 1`).

## 6. Every RNG use on these paths

`rand()` is glibc's (`srand(seed)` from `simSetSeed`, `Command.cpp:2269-2275`).
Helpers are in `src/common/MathLib.cpp:29, 56-80, 126-130`:

```cpp
const int HALF_RAND = RAND_MAX / 2;                              // 1073741823
float GetRandomNum()    { int rn = rand(); return (float)(rn - HALF_RAND) / (float)HALF_RAND; }   // [-1,1]
float GetRandomPosNum() { int rn = rand(); return (float)rn / (float)RAND_MAX; }  // (float)RAND_MAX = 2^31, can be 1.0
int   GetRandomInt(int max) { float f = GetRandomPosNum()*(float(max)+1); return CLAMP((int)f, 0, max); }
unsigned long GetRandomLong(unsigned long max) { float f = GetRandomPosNum()*(float(max)+1); return MIN((unsigned long)f, max); }
CVec  GetRandomVec() { float a = GetRandomPosNum() * 2.0f * (float)PI; return CVec(cosf(a), sinf(a)) * GetRandomNum(); }  // 2 calls
```

| Site | Calls | Gameplay or cosmetic | Runs in dedicated? |
|---|---|---|---|
| `InjureWorm` kill sound `GetRandomInt(2)` (`CClient_Game.cpp:423`) | 1 | cosmetic | **yes** |
| `InjureWorm` kill gibs ×7 (`:427-428`) | 2 each, +2 inside `SpawnEntity` while the pool has room and the gib image exists | cosmetic | **yes** |
| `InjureWorm` kill blood ×50 (`:431-437`) | 7 each = 350 | cosmetic | **yes** |
| `InjureWorm` hit blood ×ceil(clamp(dmg,10,100)) (`:450-459`) | 7 each | cosmetic | **yes** |
| Bleeding (`PhysicsLX56.cpp:320-330`) | 80 per bleed | cosmetic | **yes** |
| `GiveBonus` health amount `GetRandomInt(40)` (`CWorm.cpp:1494`) | 1 | **gameplay** | yes |
| `SpawnBonus` type `GetRandomInt(999)` (`CServer_Game.cpp:275`) | 1 | **gameplay** | yes |
| `SpawnBonus` weapon `randomChoiceFrom` (`:297`) | 1 (also for health bonuses, skipped if there is no free slot) | **gameplay** | yes |
| `CClient::Explosion` dirt particles ×2 (`CClient_Game.cpp:289-292`), also per bonus-triggered explosion | 2 each | cosmetic | yes |
| `ParseWormDown` gibs/blood (`CClient_Parse.cpp:1846-1860`) | — | — | **no** (local client never gets WORMDOWN) |
| `CWorm::GetRandomWeapons` (`CWorm.cpp:788-845`) | 1 per slot | gameplay (weapons research) | at prepare, **not** on respawn |
| Bot AI `rand()` (`CWormBot.cpp:982-985, 4234-4236`) | — | — | yes, for any worm still on a bot handler |
| **`FindSpot` `rnd()`** (`CMap.cpp:2388-2389`) | 2 per try, up to 2000 | **gameplay** | yes; separate MT copy, see 3.3 |
| `getPredefinedSpawnLocation` `rndInt` (`rndgen`) | 0 (no spawn points on LX levels) | — | — |
| `CWorm::Prepare` Gus weapons `rndInt` (`rndgen`, `CWorm.cpp:363-366`) | 5 per prepare | none | yes; `rndgen` only |
| Zero-health `death.obj` particles (2.6) | `rnd`, `midrnd`, `rndgen` | changes the `rnd` stream | yes |

Not on these paths: the `RandomNumbers[]` table in projectile code (weapons research).

## 7. Features and settings read on these paths

Defaults are the "Default" column of `src/common/FeatureList.cpp`. They match the
oracle trace header (Standard preset plus `ForceLX56Aim`).

| Feature (line) | Default | Read by |
|---|---|---|
| `Lives` (31) | −2 (unlimited) | prepare (`Game.cpp:851`), `killWorm`, `SpawnWorm`, `CheckGameOver`, `CompareWormsScore` |
| `KillLimit` (32) | 15 | `CheckGameOver`, `CompareWormsScore` |
| `TimeLimit` (33) | 6.0 min | `CGameMode::TimeLimit` ×60 |
| `GameType` (38) | Death Match | game mode |
| `ForceRandomWeapons` (42) | false (oracle: **true**) | `PrepareWorm` (weapons) |
| `WeaponRestrictionsFile` (41) | "Standard 100lt" | bonus weapon choice (4.1) |
| `Bonuses` (50) | false | pickup, explosions, `simulateBonuses`, spawn and lifetime |
| `BonusNames` (51) | true | drawing only |
| `BonusFrequency` (52) | 30.0 s | `SimulateGame` spawn interval |
| `BonusLife` (53) | 60.0 s | server lifetime |
| `BonusHealthToWeaponChance` (54) | 0.5 | `SpawnBonus` |
| `RespawnTime` (56) | 2.5 s | respawn loop |
| `MaxRespawnTime` (57) | 0.0 (≥0 = enabled) | respawn loop |
| `RespawnGroupTeams` (58) | true | `SpawnWorm` (teams only) |
| `EmptyWeaponsOnRespawn` (59) | false | `SpawnWorm` |
| `SuicideDecreasesScore` (110) | false | `getScore`, `addDamage` |
| `TeamkillDecreasesScore` (112) | false | same |
| `DeathDecreasesScore` (114) | 0.0 | `getScore` |
| `CountTeamkills` (116) | false | same |
| `AllowNegativeScore` (118) | false | `addDeath`, `addSuicide`, `addTeamkill` |
| `TeamInjure` (121) | true | `InjureWorm` |
| `SelfInjure` (125) | true | `InjureWorm` |
| `TeamHit`, `SelfHit` (123, 127) | true | projectile collision (weapons research) |
| `AllowEmptyGames` (130) | false (oracle: **true**) | `CheckGameOver` |
| `WormDamageFactor` (156) | 1.0 (range −2..10) | `Prepare` → `fDamageFactor` |
| `WormShieldFactor` (158) | 1.0 (range −2..10) | `Prepare` → `fShieldFactor` |
| `ImmediateStart` (170) | true | state goes to Playing in `prepareGameloop` (`Game.cpp:658`); "begin" sound |
| `WeaponSlotsNum` (172) | 5 | `Prepare` |
| `DisableWpnsWhenEmpty`, `DisableWpnsAtStartup` (173, 175) | false | weapons research |
| `InfiniteMap` (179) | false | carving, bonus physics |
| `TeamScoreLimit` (205) | −1 | teams only |

Per-worm overrides exist as commands: `setWormLives`, `setWormTeam`,
`setWormSpeedFactor`, `setWormDamageFactor`, `setWormShieldFactor`,
`setWormCanUseNinja` and `setWormCanAirJump` (`Command.cpp:1502-1700`).

Non-feature options (`src/client/Options.cpp`), with their effective dedicated
defaults: `Game.Blood` = 100 (213), `Game.Particles` = true (215),
`AllowConnectDuringGame` = true (329), `ServerSideHealth` = false (332, forced
off for non-dedicated servers, `CServer.cpp:141`),
`DoProjectileSimulationInDedicated` = true (278).

Constants:
- `MAX_BONUSES` 32, `SPAWN_HOLESIZE` 4, `MAX_ENTITIES` 1024 (`Entity.h:30`).
- `GAMEOVER_WAIT` 3 s, `LX_ENDWAIT` 9 s (`Consts.h:47, 50`).
- `WRM_OUT` −1, `WRM_UNLIM` −2.

## 8. What the oracle hooks need

### Determinism fixes (without these, death and bonus traces are not reproducible)

1. **Pin `tLX->currentTime` to the simulation clock in manual-step mode.** For
   example, in `OracleHooks::beforeFrame()` set
   `tLX->currentTime = game.simulationAbsTime()`. Respawn, bonus frequency and
   bonus lifetime (and explosion `posRecordings`) all compare it with
   `GetPhysicsTime()` values. Then define the port's server clock as the frame
   time. Re-record the existing traces with `--check`: they should not change.
2. **Pin the absolute simulation time.** For example, at `simLoadInputs` set
   `game.simulationTime` to a fixed value such as 100000 ms. Also set
   `cClient->fLastSimulationTime` (already aligned there) and record the value in
   the header. Bleeding (`fLastBlood` = 0) and relative air jumps depend on the
   absolute value.
3. **Reseed every Mersenne Twister in `simSetSeed`:** `rnd.base().seed(seed)`
   (`backward_compatible_uniform_01::base()` is public), `rndgen.seed(seed)`, and
   `midrnd.engine().seed(seed)`. Otherwise `FindSpot`'s stream depends on process
   history (startup, the automatic first spawns, any Gusanos particles). Then
   specify the port's `FindSpot` stream as MT19937 with that seed.
4. **Reset `cServer->fLastBonusTime`** to the (pinned) current time at scenario
   start, so the first natural bonus appears at a known frame
   (`BonusFrequency*100 + 1` frames later).
5. **Clear the entity pool at scenario start** (`ClearEntities()`), so the count
   in 2.7 starts at 0. Record `tEntities` usage (`firstUnused()` /
   `lastUsed()`), or at least a counter.
6. **Packet latency:** guarantee exactly one simulated frame per Game::frame
   (each `simStep 1` handled in its own frame; or a hook option that caps the
   loop at 1 frame per `frameInner` while keeping the budget). Bonus spawn,
   grab and destroy messages then follow 4.8. Verify with two recordings
   (`--check`).
7. **Give every worm a scripted handler.** A worm left on the bot handler runs
   AI that calls `rand()` and `FindSpot`-like logic. Add a no-key input line for
   dummy worms, or have `simLoadInputs` replace all handlers.

### Scenario controls (new commands)

- `simSetHealth <id> <hp>`: `w->setHealth(hp)`. Use it for low-health bleeding,
  one-hit kills and the exact-zero case.
- Use the existing `setWormLives`, `setWormDamageFactor` and
  `setWormShieldFactor`, plus game settings: `Bonuses`, `BonusFrequency`,
  `BonusLife`, `BonusHealthToWeaponChance`, `RespawnTime`, `MaxRespawnTime`,
  `Lives`, `KillLimit`, `SelfInjure`, `AllowNegativeScore`.
- `simBonusNow`: set `fLastBonusTime` far enough in the past that the next
  `SimulateGame` spawns through the real path (FindSpot and rand).
- `simPlaceBonus <slot> <type> <wpn> <x> <y>`: write both arrays the way
  `SpawnBonus` and `ParseSpawnBonus` do (server `Spawn` with a float position;
  client `Spawn` with the integer position, `CarveHole(4)` and `ENT_SPAWN`), with
  no RNG. This makes deterministic pickup, physics and explosion tests possible.
- `simKill <id> <killer>`: optional. It would call `cServer->killWorm` to test
  scoring and respawn without weapons.
- A way to deal damage without weapons, such as `simInjure <id> <dmg> <owner>`
  calling `cClient->InjureWorm`. This tests the factors, `SelfInjure`, blood
  `rand()` counts, `health == 0` and the kill flow before projectiles are ported.

### What to record (extra trace fields)

- Per worm:
  - Already recorded: `hp`, `alive`, `lives`, `kills`, `deaths`.
  - Add: `suicides`, `teamkills`, `dmg` (`fDamage`), `canRespawn`,
    `respawnReq`, `spawnedOnce`, `tod` (`fTimeofDeath` ms), `lastBlood` (ms),
    `dmgF` and `shieldF`.
- Per client bonus: add `vx`, `vy`, `life`, `wpn` (`getWeapon`) and `flash`.
  `x`, `y` and `t` are already there.
- The server bonus array (`cServer->cBonuses`): `i`, `t`, `wpn`, `x`, `y`, `spawnTime`.
- Per frame:
  - Global: `serverFrame`, `simulationTime`, `tLX->currentTime`, `gameOver`.
  - Entity pool used count.
  - **The number of `rand()` calls this frame:** add a counter in
    `GetRandomNum`/`GetRandomPosNum` (`MathLib.cpp` is the only gameplay
    caller; ChallengeTable only runs on connect).
  - **The number of `rnd()` calls:** count them in `FindSpot`. To also catch
    Gusanos users, wrap the engine.
  - These counters make first-divergence reports point at the RNG directly.

## 9. Open questions

1. **Answered (plan 4, D10): reliable messages between the local client and the server arrive after the frame they were queued in, and the oracle hands them over exactly so (`bonus-health`, `bonus-weapon`, `explode-kill-respawn`).** **Loopback latency:** does reliable `CChannel` delivery between the local
   client and the server ever take more than one Game::frame (window or ack
   limits, `GetTime()`-based rate control)? Check with recordings once hook 6 is in.
2. **Answered (plan 4 task 7): yes**, gibs draw 4 `rand()` with room in the pool
   (`explode-kill-respawn`'s 580 draws at its kill). **Gib image in dedicated mode:** is `bmpGibs` non-NULL? Almost certainly
   (`giblets.png` exists, `LoadGameImage` works headless), but confirm with a
   hook (record `w->getGibimg().get() != NULL`), since it changes the gib
   `rand()` count from 2 to 4.
3. **Resolved in effect (plan 4): every scenario matches `mh` and `mt` from frame −1, so the automatic first spawn needs no special handling.** **Automatic first spawn:** once the state reaches Playing, `SimulateGame`
   spawns every ready worm with `FindSpot` (`!haveSpawnedOnce`) before
   `oracle_control.py` calls `spawnWorm`. That carves a size-4 hole at a
   `FindSpot` position and uses `rnd()`. The M3 traces match `mh` at frame −1,
   so either that spot was already empty (likely on Dirt Level) or the first
   spawn happens later than assumed. Record the position of the first spawn, or
   reset the terrain/`rnd` state. This matters for bonus and respawn scenarios on maps with dirt.
4. **Answered (plan 4 task 7): no** (see 2.6). **Zero-health deaths:** should the port emulate the Gusanos death-particle
   `rnd()` consumption (2.6)? It needs a minimal Gusanos particle simulation:
   `meat.obj`, `bloodgen.obj` and `blood.obj` with ground collision on the same
   map. Recommendation: document it as a deviation, and avoid exact-zero health
   in oracle scenarios except one that checks the delayed, uncredited death.
5. **Lua callbacks:** `LUACALLBACK(afterUpdate)` and other Lua callbacks in the
   default Gusanos scripts may call Lua `random` (`rnd()`/`rndInt`) during the
   game. A quick check found only GUI/score code in `common.lua`. Confirm with
   the `rnd()` counter in the hooks.
6. **Answered (plan 4): `fDamage` is not in the trace; the port counts it once, on the client (STATUS, deviations).** **`fDamage` double count:** should the oracle compare `fDamage`? It depends
   on `C2S_REPORTDAMAGE` batching by `tLX->currentTime`
   (`0.1 × (NST_LOCAL − netSpeed)` s) and on packet timing. It affects only the
   winner tie-break. Suggest excluding it, or comparing only the client-side
   half (track the two sources separately in the port).
7. **Answered (plan 4): the trace header's `T0`, 100 000 ms (spec §18 D2).** **Which `simulationTime` base should the Rust world use?** The port's
   `World::new(start)` already takes one, so hook 2 should define it.
8. **Still open: recorded in STATUS (bonus weapons ignore restrictions unless `World::set_bonus_weapons` is given).** **Weapon restrictions:** the port must load the mod's `Standard 100lt.wps`
   (or whatever `WeaponRestrictionsFile` names) for bonus weapons and random
   weapons to match. This needs coordinating with the weapons research. Is that
   file in `content/olx-manifest.ron`?
