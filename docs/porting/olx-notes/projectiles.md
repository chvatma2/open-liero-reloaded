# M4 research 2: the life of a projectile (OLX `d1ad953f`, LX56 engine)

Everything below was read from the unpatched OLX source at
`scratchpad/openlierox` (commit `d1ad953f`). Paths are relative to that tree.
"Dedicated" means our oracle build: `cmake -DDEDICATED_ONLY=Yes -DDEBUG=No`
(Release, so `assert` is compiled out) and `bDedicated == true`.

Scope: what happens to a `CProjectile` from the moment it is in
`cClient->cProjectiles` until it is freed, plus everything those paths call
(explosions, carving, dirt, worm damage, `rand()`), plus spawning of children
from projectiles. Shot spawning (`CClient::ProcessShot`, `Proj_SpawnInfo::apply`
with a `shoot_t` parent) is only summarised where it fixes projectile state.

---

## 0. Summary of the findings that matter most

1. **Who simulates.** Only `cClient` (the host's own `CClient`) owns projectiles
   (`FastVector<CProjectile,3000> cProjectiles`, `include/CClient.h:215-216`).
   `GameServer` has none (`src/server/CServer.cpp:82` is commented out). In the
   dedicated server `CClient::shouldDoProjectileSimulation()` is true because
   `Misc.DoProjectileSimulationInDedicated` defaults to `true`
   (`src/client/Options.cpp:278`, `src/client/CClient_Game.cpp:52-55`; adding
   worms is impossible otherwise, `CClient.cpp:2118-2125`). Worm damage is
   applied by `CClient::InjureWorm` on the client object; since the worms are
   local to the host, the client calls `w->injure()` and then
   `cServer->killWorm()` directly (`CClient_Game.cpp:368-438`).
2. **Projectiles are simulated only inside the worm physics sub-step loop**
   (`CClient_Game.cpp:213-215`), but each projectile catches up to the **frame
   clock** (`GetPhysicsTime()` = `Game::simulationTime`), on its **own** 11 ms
   clock (`CProjectile::fLastSimulationTime`). Frames with no worm sub-step
   (frames 0, 1, 12, ... after alignment) simulate no projectile; the next call
   catches up (possibly 2 steps).
3. **Wall-clock dependence (oracle blocker).** `tLX->currentTime` is the real
   wall clock (`Game::frame`, `src/game/Game.cpp:697-700`); in manual-step mode
   it is unrelated to `simulationTime`. It is used on these paths:
   - worm hit tests and explosion damage read the worm position from
     `posRecordings.getBest(11, tLX->currentTime - prj->fLastSimulationTime)`
     (`PhysicsLX56_Projectiles.cpp:88`, `CClient_Game.cpp:324`);
   - shot spawn time: `shoot_t::spawnTime()` = `tLX->currentTime - (serverTime - shot.fTime)`
     (`src/common/CShootList.cpp:715-731`) becomes the projectile's
     `fLastSimulationTime`/`fSpawnTime`;
   - (outside this area) respawn timing, `fTimeofDeath`.
   The traces are therefore not reproducible until the hook pins
   `tLX->currentTime` to the simulation clock. See "What the oracle hooks need".
4. **`VecTimeRecorder::get(0)` is not the latest entry.** `get(framesBack)`
   returns `m_data[(cur + 100 - framesBack%100) % 100]`, and `cur` is the
   *next write* slot (`src/util/VecTimeRecorder.h:37-59`). So `framesBack = 0`
   returns the **oldest** slot (100 pushes ago, or stale data from a previous
   life, or `(0,0)` if never written); `framesBack = 1` is the newest.
   `framesBack = ms_back / 11`, so in real play (`ms_back` in about `(0, 21]`)
   the worm hitbox is often the 1.1 s old position. Port the ring buffer
   exactly (100 entries, `cur`, `count`, stale contents survive `clear`).
5. **Explosions do not fall off.** Every alive worm with
   `|pos - wPos|^2 <= 25` (float) takes the full `damage`
   (`CClient_Game.cpp:320-329`). Hole size is `clamp((int)damage, 0, 4)`,
   i.e. `HoleN.png` with `N = size + 1`.
6. **`rand()` in a dedicated server.** Cosmetic code still runs on the
   dedicated server and consumes `rand()`: pixel-colour choice at spawn,
   explosion dirt particles (4 calls), blood on every injury (7 calls per blood
   iteration), gibs and death blood on a kill, `PlaceGreenDirt` (1 call per green
   pixel). `SpawnEntity` itself consumes 2 calls for `ENT_GIB` **only while the
   1024-slot entity pool has room**, and in a dedicated server entities are never
   freed (`SimulateEntities` is skipped, `CClient_Game.cpp:210-211`), so the
   pool fills permanently. The port must count entity spawns.
7. **Dedicated-only semantics.** `proj_t::bmpImage` is never loaded in a
   dedicated server (`CGameScript.cpp:937`, `1159-1161`). Consequences:
   `ANI_ONCE` never deletes a projectile, and a projectile with no action at all
   is always deleted after its first step (the "junk projectile" rule,
   `PhysicsLX56_Projectiles.cpp:1217-1224`). A non-dedicated client behaves
   differently. **Correction (plan 4 task 5):** the oracle patch loads the
   images in dedicated mode too (spec §18 D3), so the traces show the
   client's behaviour: `ANI_ONCE` ends projectiles, and an action-less
   `ANI_ONCE` image projectile lives until its animation ends.
9. **`Attr` assignments skip equal values** (`src/game/Attr.h:194-204`).
   `vPos`, `vVelocity` (and `health`) are `Attr`s. `vPos = v`, `setPos`,
   `setVelocity`, `vPos += d` and `vVelocity *= f` (`AttrWithBasicOpts`
   assigns `value op d` through `operator=`) do nothing when the new value
   compares equal, so a zero keeps its old sign. `.write()` paths
   (`vVelocity.write().y += g`, `multPairwiseInplace` in `Bounce`,
   `vPos.write().x = ...`) assign directly. In `Spawn`, a reused slot keeps
   the previous projectile's `-0` when the new velocity is `+0`
   (`nuclear-mines`, frame 206).
8. **Macro double evaluation.** `CalculateCheckSteps` uses
   `SIGN(getRandomFloat())`; `SIGN` evaluates its argument up to twice
   (`include/MathLib.h:60`), so a non-positive first draw advances `iRandom`
   twice and takes the sign of the second draw.

---

## 1. Data types and constants

### 1.1 `CProjectile` (`include/CProjectile.h:74-212`)

| field | type | meaning / used by |
|---|---|---|
| `bUsed` | bool | slot used |
| `fSpawnTime` | AbsTime (u64 ms) | wallshoot window |
| `fLife` | f32 | timer, `life += dt.seconds()` each step |
| `fExtra` | f32 | smoke-trail accumulator (cosmetic) |
| `iOwner` | int | `-1` or worm id `0..MAX_WORMS-1` |
| `iColour` | Color | cosmetic, but chosen with `rand()` |
| `fIgnoreWormCollBeforeTime` | AbsTime | owner immunity |
| `timerInfo` | map | v8 `Proj_TimerEvent` state only |
| `fLastTrailProj` | AbsTime | `TRL_PROJECTILE` |
| `fTimeVarRandom` | f32 | `RandomNumbers[(uchar)iRandom]` at spawn |
| `tProjInfo` | proj_t* | definition |
| `vOldPos` | CVec | **last terrain-safe position** (not the previous frame position) |
| `fRotation` | f32 | cosmetic |
| `radius` | VectorD2<int> | `Width/2, Height/2` (v7: 1 for pixel, 2 for image) |
| `iRandom` | int | index into the 256-entry fixed table |
| `MAX/MIN/AVG_CHECKSTEP`, `*2` | int | collision sub-stepping |
| `fWallshootTime` | f32 | seconds |
| `bChangesSpeed`, `iCheckSpeedLen` | bool, int | check-step recalculation |
| `CollisionSide` | int | `COL_TOP 1, COL_RIGHT 2, COL_BOTTOM 4, COL_LEFT 8` |
| `bFrameDelta`, `fFrame`, `iFrameX` | | animation (cosmetic in dedicated) |
| `fLastSimulationTime` | AbsTime | **per-projectile physics clock** |
| `health` (CGameObject) | f32 | 100 at spawn; v8 only |

`CollisionSide` is never initialised by the constructor or `Spawn`; in v7 it is
always written before it is read (see 3.6), so no state carries over.

`ProjCollisionType` (`CProjectile.h:53-63`): `withWorm:1` plus a 7-bit union of
`wormId`/`colMask`. `PJC_TERRAIN 1, PJC_WORM 2, PJC_MAPBORDER 4, PJC_DIRT 8`.
`operator bool` is `withWorm || colMask != 0`.

### 1.2 Time types (`include/olx-types.h:27-97`)

`TimeDiff`/`AbsTime` are u64 milliseconds. `seconds()` is `ms * 0.001f`.
`TimeDiff(float s)` is `(u64)(s * 1000.0f)` (f32 multiply, truncation).
`AbsTime + float` is `time + (u64)(td * 1000.0f)`. `AbsTime - AbsTime` is
`end - start` in u64; the `assert(end >= start)` is compiled out, so a negative
difference **wraps** to a huge value. `TimeDiff * float` is
`(u64)((float)ms * f)` (the u64 is converted to float first).

`LX56PhysicsDT = TimeDiff(1000 / LX56PhysicsFPS)` = 11 ms
(`include/PhysicsLX56.h:20-21`); `dt.seconds()` = `11 * 0.001f` = `0.0110000009f`.

### 1.3 Fixed random table

`GetFixedRandomNum(uchar index)` returns `RandomNumbers[index]`
(`src/common/Misc.cpp:84-87`, table: `include/RandomNumberList.h`, 256 floats).
The parameter is `uchar`, so `-1` reads index 255.

`CProjectile::getRandomFloat()` (`CProjectile.cpp:118-125`):

```
r = RandomNumbers[(uchar)iRandom]; iRandom++; iRandom %= 255;  // C remainder
return r;
```

### 1.4 `rand()` helpers (`src/common/MathLib.cpp`)

glibc `rand()`, `RAND_MAX = 2147483647`, `HALF_RAND = RAND_MAX / 2 = 1073741823` (`:29`).

```
GetRandomNum()    = (float)(rand() - HALF_RAND) / (float)HALF_RAND        // :58-62, [-1,1]
GetRandomPosNum() = (float)rand() / (float)RAND_MAX                         // :64-67, [0,1]
GetRandomInt(max) = CLAMP((int)(GetRandomPosNum() * ((float)max + 1)), 0, max)  // :71-76
GetRandomVec():  angle = GetRandomPosNum() * 2.0f * (float)PI;              // :126-130
                 return CVec(cosf(angle), sinf(angle)) * GetRandomNum();    // 2 calls: PosNum, then Num
NormalizeVector(v): len = (float)sqrt(x*x + y*y); if(len) { x /= len; y /= len; } return len;  // :109-120
GetVecFromAngle(yaw): a = yaw * (float)(PI/180); return (cosf(a), sinf(a))  // :135-155 (1*cy, 1*sy)
```

`PI` is the double literal `3.14159265358979323846` (`include/MathLib.h:31`).
`SIGN(x)` is `(((x) > 0)?1:(((x) < 0)?-1:0))` (`MathLib.h:60`): it evaluates `x`
twice when `x <= 0`. `FMOD(a, b)`: `a -= b * floor(a / b); if(a < 0) a += b;`
(`MathLib.h:86`).

Float details: Release `-O3`, x86-64 SSE2, no `-march`, so no FMA contraction.
`<cmath>`/SDL pull in libstdc++'s `math.h` wrapper, so unqualified
`sqrt(float)`, `floor(float)`, `atan2(float,float)` resolve to the float
overloads (see open question 5).

---

## 2. The container (`include/FastVector.h:27-152`)

`typedef FastVector<CProjectile, MAX_PROJECTILES> Projectiles` with
`MAX_PROJECTILES = 3000` (`CProjectile.h:36`, `CClient.h:215`). Fields:
`m_objects[3000]`, `m_firstUnused` (starts 0), `m_lastUsed` (starts -1).

- **Allocation** `getNewObj()` (`:87-94`): `if(first >= 3000) return NULL;`
  `idx = first; if(idx > last) last = idx; findNewFirstUnused(); return &obj[idx];`
  `findNewFirstUnused` (`:34-41`) steps `first` up until `first > last`,
  `first >= SIZE`, or an unused slot. Result: **always the lowest free slot**.
  When full, `CClient::SpawnProjectile` returns silently
  (`CClient_Game.cpp:716-730`); the parent's `iRandom` draws (and shot
  `nRandom`/rot draws) already happened, but the child's spawn `rand()` does not.
- **Removal** `CProjectile::setUnused()` (`CProjectile.cpp:34-39`): `bUsed = false;
  timerInfo.clear(); onInvalidation(...)` which calls `refreshObj(i)` (`:97-102`):
  `if(i < first) first = i; if(i >= last) findNewLastUsed();`
  (`findNewLastUsed`, `:43-50`: `last--` until used, `< first`, or `< 0`).
- **Iteration** `Iterator` (`:104-132`): starts at index -1, `next()`:
  `++idx; while(idx < SIZE) { if(isUsed(idx)) break; ++idx; if(idx > parent.m_lastUsed) idx = SIZE; }`.
  It reads `m_lastUsed` live. Ascending slot order.
- **Clear**: `Game` prepare (`src/game/Game.cpp:552-554`) clears the list at game start.

Consequences for simulation order:
- Projectiles are simulated in ascending slot order, each one catching up fully
  (all its pending 11 ms steps) before the next slot is touched.
- A projectile frees its own slot only at the very end of its step
  (`Proj_DoActionInfo::execute`), **after** spawning its children, so children
  never take the parent's slot.
- A child spawned during the pass lands in the lowest free slot. If that slot is
  **above** the iterator's current index it is simulated **in the same pass**
  (from its spawn time, the parent's `fLastSimulationTime`, up to the frame
  time); if it is **below**, it waits for the next pass and then catches up.
  Either way it runs the same steps; only the interleaving (terrain edits,
  `rand()` order, damage order) differs, so the port must allocate and iterate
  exactly like this.
- Nothing removes *another* projectile in v7 (`InjureProj` only lowers health).

`updateCollMapInfo` (`CProjectile.cpp:453-479`) maintains `projPosMap` only when
the mod needs collision info (v8 `ProjHit` events) or `FT_CollideProjectiles`;
it has no gameplay effect for v7 mods.

---

## 3. Simulation

### 3.1 Call site and clocks (`src/client/CClient_Game.cpp:60-222`)

```
CClient::Simulation():
  currentTime = GetPhysicsTime()                     // Game::simulationTime (frame clock)
  frame_dt = 11ms (* 1/clamp(GameSpeed,.05,10) unless GameSpeedOnlyForProjs)
  if !game.shouldDoPhysicsFrame():                   // paused / game over (Game.cpp:1267-1275)
      PhysicsEngine::skipProjectiles(...)            // every prj->fLastSimulationTime += tLX->fRealDeltaTime (=10ms in the frame loop)
      fLastSimulationTime = currentTime; return
loop:
  if cClient->fLastSimulationTime + frame_dt > currentTime: return
  for each worm (map order): simulateWorm (pushes posRecordings), shooting, ...
  if !bDedicated: SimulateEntities(11ms)              // never in dedicated
  if shouldDoProjectileSimulation(): simulateProjectiles(cProjectiles.begin())
  simulateBonuses(...)
  cClient->fLastSimulationTime += frame_dt
```

`LX56_simulateProjectiles` (`PhysicsLX56_Projectiles.cpp:1400-1411`):
`currentTime = GetPhysicsTime()` once, then for every projectile in slot order
`LX56_simulateProjectile(currentTime, p)`.

`LX56_simulateProjectile` (`:1378-1397`):

```
orig_dt  = LX56PhysicsDT                                          // 11 ms
frame_dt = orig_dt * (1.0f / CLAMP((float)GameSpeed, 0.05f, 10.0f))  // (u64)(11.0f * f); ALWAYS applies GameSpeed
if frame_dt <= 0: frame_dt = 1ms
oldPos = (VectorD2<int>)pos; oldRadius = radius                   // collision map only
loop:
  if prj->fLastSimulationTime + frame_dt > currentTime: break
  prj->fLastSimulationTime += frame_dt                            // clock advanced BEFORE the step
  if !LX56ProjectileHandler_doFrame(currentTime, orig_dt, prj): break   // deleted
updateCollMapInfo(&oldPos, &oldRadius)
```

So the physics `dt` is always 11 ms; `GameSpeed` only changes how often steps
happen. The projectile clock is independent of `CClient::fLastSimulationTime`:
it starts at the spawn time and advances in `frame_dt` steps while
`<= simulationTime`.

### 3.2 One step: `LX56ProjectileHandler_doFrame` (`:1330-1375`)

```
info = Proj_DoActionInfo{}                          // explode=false, damage=-1, timer=false, shake=0, ...
serverTime = game.serverTime() - (currentTime - prj->fLastSimulationTime)   // or 0; v8 global timers only
1. projInfo.Timer.checkAndApply(Unspec(serverTime, dt), prj, &info)        // BEFORE moving, life not yet incremented
2. result = LX56_simulateProjectile_LowLevel(prj->fLastSimulationTime, dt, prj,
                                             &info.trailprojspawn, &info.deleteAfter)
3. ev = Col(serverTime, dt, &result)
4. if !result.withWorm && (result.colMask & PJC_TERRAIN): projInfo.Hit.applyTo(ev, prj, &info)
5. if result.withWorm && !info.explode:                 projInfo.PlyHit.applyTo(ev, prj, &info)
6. for a in projInfo.actions: a.checkAndApply(ev, prj, &info)               // v8 only; empty for v7
7. info.execute(prj, currentTime)
return !info.deleteAfter
```

A step never reports terrain and worm at once (the worm sweep overrides).
A timer explosion suppresses the worm-hit action of the same step (no
`InjureWorm`, no push).

### 3.3 `LX56_simulateProjectile_LowLevel` (`:1234-1327`)

```
res = LX56Projectile_checkCollAndMove(proj, dt, map)
proj->life  += dt.seconds()                 // += 0.0110000009f
proj->extra += dt.seconds()
if Rotating:  rotation += (float)RotSpeed * dt.seconds(); FMOD(rotation, 360.0f)     // cosmetic
if Animating:                                                                         // cosmetic in dedicated
   frame += / -= (float)AnimRate * dt.seconds()  (by bFrameDelta)
   if bmpImage:                              // NULL in dedicated -> nothing below runs
       NumFrames = w / h
       if frame >= NumFrames: ONCE -> *deleteAfter = true; LOOP -> frame = 0;
                              PINGPONG -> flip delta, frame = NumFrames - 1
       else if frame < 0 and PINGPONG: flip delta, frame = 0
switch Trail.Type:
   SMOKE:     if extra >= 0.075f { extra = 0; SpawnEntity(ENT_SMOKE, 0, pos, (0,0), Color(), NULL) }
   CHEMSMOKE: if extra >= 0.075f { extra = 0; SpawnEntity(ENT_CHEMSMOKE, ...) }
   DOOMSDAY:  if extra >= 0.05f  { extra = 0; SpawnEntity(ENT_DOOMSDAY, 0, pos, vel, ...) }
   EXPLOSIVE: if extra >= 0.05f  { extra = 0; SpawnEntity(ENT_EXPLOSION, 10, pos, (0,0), ...) }
   PROJECTILE:
       // currentTime here is prj->fLastSimulationTime (already advanced)
       if currentTime > lastTrailProj:                       // strict; lastTrailProj starts at 0
           lastTrailProj = currentTime + Trail.Delay / (float)GameSpeed   // AbsTime + (u64)(x * 1000.0f)
           *projspawn = true
return res
```

Smoke/explosive trails have no gameplay effect and no `rand()`, **but** each
`SpawnEntity` takes an entity-pool slot in a dedicated server (section 6.3).
`TRL_PROJECTILE` is gameplay. The first step after spawn always spawns trail
children (`lastTrailProj` is 0).

### 3.4 `LX56Projectile_checkCollAndMove` (`:433-491`)

```
if bChangesSpeed:
    len = (int)vel.GetLength2()                       // float -> int (x86 cvttss2si: out of range -> INT_MIN)
    if abs(len - iCheckSpeedLen) > 50000: CalculateCheckSteps()     // may consume iRandom (3.8)
dmp = Dampening
if dmp != 1.0f:
    if dt == LX56PhysicsDT: vel *= dmp                // always this branch (doFrame passes orig_dt)
    (other branches: powf(dmp, dt/LX56PhysicsDT), unreachable here)
v2 = vel.GetLength2()                                 // after dampening, before gravity
if (int)(v2 * dt.seconds() * dt.seconds()) <= MAX_CHECKSTEP2:      // ((v2*dts)*dts) then truncate
    return _Frame(prj, dt)
cs = (float)AVG_CHECKSTEP / sqrt(v2)                  // float sqrt
cs = MAX(cs, 0.001f)
cstep = TimeDiff(cs)                                  // (u64)(cs * 1000.0f): WHOLE ms
if cstep < dt:
    for (t = 0; t < dt; t += cstep):
        r = _Frame(prj, (t + cstep > dt) ? dt - t : cstep)
        if r: return r                                // rest of the step is dropped
    return NoCol
return _Frame(prj, dt)
```

### 3.5 `LX56Projectile_checkCollAndMove_Frame` (`:367-430`)

```
g = UseCustomGravity ? (float)Gravity : 100.0f
vel.y += (float)ProjGravityFactor * g * dt.seconds()        // ((factor * g) * dts)
if ProjFriction > 0:                                          // default 0
    applyFriction(vel, dts, (rx + ry) * 0.5f, radius.GetLength(), 0.02f, friction)   // include/Physics.h:63-66
vOldVel = vel
vFrameOldPos = pos
pos += vel * dt.seconds()
if (int)(vOldPos - pos).GetLength2() < MIN_CHECKSTEP2:       // vOldPos = last SAFE position
    return FinalWormCollisionCheck(prj, vFrameOldPos, vOldVel, dt, NoCol)
px = (int)pos.x; py = (int)pos.y                             // truncation toward 0
if MapBoundsCollision(px, py):                               // resets CollisionSide = 0, then sets border bits
    pos = vOldPos; vel = vOldVel
    return FinalWormCollisionCheck(..., Terrain(PJC_TERRAIN | PJC_MAPBORDER))
if fLastSimulationTime <= fSpawnTime + TimeDiff(fWallshootTime):   // wallshooting window
    return FinalWormCollisionCheck(..., NoCol)               // NOTE: vOldPos is NOT updated
c = TerrainCollision(px, py)
if c.collided && HandleCollision(c, vFrameOldPos, vOldVel, dt):
    mask = PJC_TERRAIN | (c.onlyDirt ? PJC_DIRT : 0)
    return FinalWormCollisionCheck(..., Terrain(mask))
vOldPos = pos                                                 // the move was safe
return FinalWormCollisionCheck(..., NoCol)
```

All sub-steps of one step share the same `fLastSimulationTime`, so the
wallshoot test is per step, not per sub-step.

**`MapBoundsCollision(px, py)`** (`:176-196`): `CollisionSide = 0`;
`InfiniteMap` -> false. Bits: `LEFT` if `px < 0 || px - rx < 0`; `RIGHT` if
`px >= W || px + rx >= W`; `TOP` if `py < 0 || py - ry < 0`; `BOTTOM` if
`py >= H || py + ry >= H`. Returns `CollisionSide != 0`.

**`TerrainCollision(px, py)`** (`:220-280`), non-wrap rect (all v7 projectiles):

```
res = {left=0,right=0,top=0,bottom=0,collided=false,onlyDirt=true}
for y in py-ry ..= py+ry: for x in px-rx ..= px+rx:
    m = material(x, y)
    if !m.particle_pass:                 // dirt or rock (LX materials 0 rock, 1 bg, 2 dirt)
        if y < py: top++  elif y > py: bottom++
        if x < px: left++ elif x > px: right++
        if !m.destroyable: onlyDirt = false
        collided = true
```

(The `PRJ_CIRCLE` variant skips pixels with `dx^2+dy^2 > rx^2+ry^2`; v8 only.)
Bounds are safe because `MapBoundsCollision` already rejected anything closer
than `radius` to an edge.

**`HandleCollision(c, oldpos=vFrameOldPos, oldvel=vOldVel, dt)`** (`:285-357`):

```
if Hit.Type == EXPLODE && c.onlyDirt: return true          // keep pos: explode inside the dirt
bounce = false
switch Hit.Type:
  BOUNCE:    bounce = true
  NOTHING:   pos = oldpos + (vel * dts) * 0.5f; vOldPos = pos      // then FALLS THROUGH to side code below
  GOTHROUGH: pos = oldpos + (vel * dts) * GoThroughSpeed; return false   // not a collision
  default:   pos = vOldPos; vel = oldvel; return true      // EXPLODE (rock), CARVE, DIRT, INJURE, DISAPPEAR, ...
vx = (int)vel.x; vy = (int)vel.y
if (c.left > c.right || c.left > 2) && c.left > 1 && vx <= 0:  { if bounce: pos.x = oldpos.x; if vx: CollisionSide |= LEFT }
if (c.right > c.left || c.right > 2) && c.right > 1 && vx >= 0: { if bounce: pos.x = oldpos.x; if vx: CollisionSide |= RIGHT }
if c.top > 1 && vy <= 0:    { if bounce: pos.y = oldpos.y; if vy: CollisionSide |= TOP }
if c.bottom > 1 && vy >= 0: { if bounce: pos.y = oldpos.y; if vy: CollisionSide |= BOTTOM }
if abs(vx) < 2: vel.x = 0
if abs(vy) < 2: vel.y = 0
return true
```

(`TOP`/`BOTTOM` test `vy`, `LEFT`/`RIGHT` test `vx`.) Quirks: `vel` is the
velocity after this sub-step's gravity, so `vel = oldvel` in `default` changes
nothing; `default` restores the **last safe** position, which may be several
steps back (or the spawn position, if the wallshoot window kept `vOldPos` at
spawn). `NOTHING` also zeroes slow velocity components and reports a terrain
collision, which ends the sub-step loop for this step.

### 3.6 Worm collision

**`FinalWormCollisionCheck(prj, vFrameOldPos, vFrameOldVel, dt, cur)`** (`:130-171`):

```
if PlyHit.Type != PJ_NOTHING:
    dif = pos - vFrameOldPos; len = NormalizeVector(&dif)      // dif stays (0,0) if len == 0
    len = MIN(len, (float)W + (float)H)
    for (float p = 0.0f; p <= len; p += 2.0f):                 // p = 0 tests vFrameOldPos itself
        curpos = vFrameOldPos + dif * p
        id = ProjWormColl(curpos)
        if id >= 0:
            if PlyHit.Type != PJ_GOTHROUGH: pos = curpos; vOldPos = curpos; vel = vFrameOldVel
            return Worm(id)                                    // overrides any terrain result
(InfiniteMap wrapping of pos/vOldPos: rejected by rl-sim)
return cur
```

Note the terrain handling (position reset, bounce position) has already been
applied when a worm overrides it, and the terrain `Hit` action then does not run.

**`ProjWormColl(CVec pos)`** (`:52-111`):

```
s.pos = (VectorD2<int>)pos  (truncate); s.radius = radius
if Type != PRJ_CIRCLE: if s.radius.x <= 2: s.radius.x = 0; if s.radius.y <= 2: s.radius.y = 0   // v7: always (0,0)
owner = hasOwner ? wormById(owner) : NULL
preventSelfShooting = fIgnoreWormCollBeforeTime > fLastSimulationTime
for w in game.wormMap() (ascending id):
    if !alive: continue
    if preventSelfShooting && w == owner: continue
    if owner && isTeamGame && !TeamHit && w != owner && w.team == owner.team: continue
    if owner && !SelfHit && w == owner: continue
    wp = (VectorD2<int>) w->posRecordings.getBest(11, (size_t)(tLX->currentTime - fLastSimulationTime).ms)
    // rect-rect: |s.x - wp.x| < s.rx + 4 && |s.y - wp.y| < s.ry + 4   (Geometry.h:104-108)
    if hit:
        CollisionSide = 0
        if s.x < wp.x-2: |= LEFT elif s.x > wp.x+2: |= RIGHT
        if s.y < wp.y-2: |= TOP  elif s.y > wp.y+2: |= BOTTOM
        return w.id
return -1
```

**`posRecordings`** (`DataTimeRecorder<CVec,100>`, `src/util/VecTimeRecorder.h:21-62`):
`push_back` writes `m_data[cur]`, `cur = (cur+1) % 100`, `count = min(count+1, 100)`.
`clear()` only sets `count = 0`; `clear(v)` = `clear(); push_back(v)`.
`getBest(11, ms)`: `fb = ms / 11; if fb < count: return get(fb); if count > 0: return get(count-1); return m_data[(cur+99)%100]`,
with `get(k) = m_data[(cur + 100 - k%100) % 100]`. With one entry (right
after `clear(vPos)`), `get(count-1)` is `get(0)` too: every `getBest` returns
the stale slot, not the spawn position. Pushed once per physics step
for each **alive** worm at the end of `simulateWorm` (`PhysicsLX56.cpp:468`;
dead worms return at `:311`); `clear()` at `CWorm` prepare (`CWorm.cpp:398`),
`clear(vPos)` at spawn (`CWorm.cpp:611`) and on network updates
(`CWorm_SendRecv.cpp:301,468`). Worms are simulated before projectiles in a
step, so `get(1)` is the worm's current position. See section 0 item 4 for
`get(0)`.

### 3.7 Bounce (`CProjectile::Bounce`, `CProjectile.cpp:376-429`) — LX56 "wrong" version

```
x = y = 1
if CollisionSide & TOP:    x = c;  y = -c
if CollisionSide & BOTTOM: x = c;  y = -c
if CollisionSide & LEFT:   x = -c; y = c
if CollisionSide & RIGHT:  x = -c; y = c      // left/right override top/bottom
vel = (vel.x * x, vel.y * y)
```

### 3.8 Spawn-time state (`CProjectile::Spawn`, `CProjectile.cpp:44-112`)

Order matters for `iRandom` and `rand()`:

```
iOwner = (owner in 0..MAX_WORMS-1) ? owner : -1
bUsed = true; tProjInfo = proj; fLife = 0; fExtra = 0
vOldPos = vPos = pos; vVelocity = vel; fRotation = (float)rot
radius = (Width/2, Height/2); health = 100
fLastTrailProj = 0; iRandom = random; iFrameX = 0; fIgnoreWormCollBeforeTime = ignore
fTimeVarRandom = RandomNumbers[(uchar)iRandom]
fLastSimulationTime = fSpawnTime = time; timerInfo.clear()
fSpeed = |vel|
CalculateCheckSteps()                                   // may draw from iRandom (below)
fFrame = 0; bFrameDelta = true
if Type in {RECT, PIXEL, CIRCLE, POLYGON} && Colour.size() > 0:
    iColour = Colour[GetRandomInt(Colour.size() - 1)]    // ONE rand(), even with one colour
if FT_LX56WallShooting: fWallshootTime = 0.011f + getRandomFloat() / 1000   // float / int; iRandom++
else: fWallshootTime = 0
bChangesSpeed = ((int)Dampening == 1) && (Hit.Type != PJ_BOUNCE || (int)Hit.BounceCoeff == 1)
updateCollMapInfo()
```

`CClient::SpawnProjectile` does `_random %= 255` before `Spawn` (C remainder, so
`-1` stays `-1`).

**`CalculateCheckSteps()`** (`CProjectile.cpp:130-166`):

```
iCheckSpeedLen = (int)vel.GetLength2()
if   iCheckSpeedLen < 14000:  MIN=0, MAX=3, AVG=2
elif iCheckSpeedLen < 75000:  MIN=0, MAX=4, AVG=2
elif iCheckSpeedLen < 250000: MIN=1, MAX=5, AVG=2
else:
    rnd = iRandom % 3                                    // C remainder (iRandom may be -1)
    r1 = getRandomFloat()                                // iRandom++
    if r1 > 0: s = 1
    else: r2 = getRandomFloat(); s = (r2 < 0) ? -1 : 0   // SIGN macro: second draw
    rnd *= s
    MIN = 6
    if Hit.Type == PJ_BOUNCE: MAX = 2, AVG = 2
    else:                     MAX = 9 + rnd, AVG = 6 + rnd
MAX2 = MAX*MAX; MIN2 = MIN*MIN
```

---

## 4. Events and actions

### 4.1 Event checks

- **LX56 timer** `Proj_LX56Timer::checkAndApply` (`ProjAction.h:463-468`):
  `if(!hasAction()) return` where `hasAction = (Type != NOTHING || Projectiles || additionalAction...) && Time > 0`;
  `checkEvent` (`PhysicsLX56_Projectiles.cpp:949-956`):
  `f = fTimeVarRandom; if(Time > 0 && (Time + TimeVar * f) < life) { timerHit = true; return true; }`
  (`life` before this step's increment). It fires **every step** from then on.
  Timer actions Explode, Carve, Dirt, GreenDirt and Disappear2 delete the
  projectile; Bounce, Injure, Disappear, Nothing, GoThrough and PlaySound do
  nothing on a timer (all but PlaySound also cancel `spawnprojs`).
- **Terrain hit** (v7 `Hit`): applied when `!withWorm && colMask & PJC_TERRAIN`.
- **Worm hit** (v7 `PlyHit`): applied when `withWorm && !info.explode`.
- v8 only (`actions` vector, empty for v7): `Proj_TimerEvent` (`:958-997`),
  `Proj_ProjHitEvent` (`:1000-1040`, scans `projPosMap` 20x20 cells, ordered
  `std::set<CProjectile*>` = slot order), `Proj_WormHitEvent` (`:1044-1087`),
  `Proj_TerrainHitEvent` (`:1091-1100`), `Proj_DeathEvent` (`health < 0`,
  never true because `CGameObject::injure` clamps health to 0,
  `CGameObject.cpp:35-44`), `Proj_FallbackEvent` (`!info.hasAnyEffect()`).
  `Proj_EventAndAction::checkAndApply` requires all events to match.

### 4.2 `Proj_Action::applyTo(ev, prj, info)` (`PhysicsLX56_Projectiles.cpp:740-945`)

Common prologue/epilogue:

```
push_worm = true; spawnprojs = Projectiles
switch Type { ... }
if push_worm && ev.colType && ev.colType->withWorm:
    d = vel; NormalizeVector(&d)
    w = wormById(wormId); if w: w.vel += (d * 100) * ev.dt.seconds()     // dt = 11 ms
if spawnprojs:
    if Proj.isSet(): info.otherSpawns.push_back(&Proj)   // v8 per-action spawn
    else:            info.spawnprojectiles = true          // GeneralSpawnInfo
if additionalAction: additionalAction->applyTo(ev, prj, info)   // v8
```

Per kind (enum values from `include/ProjAction.h:147-177`; "timer" means
`ev.timerHit`, "terrain" means `colType && !withWorm`, "worm" means
`colType && withWorm`):

| # | kind | behaviour |
|---|---|---|
| 0 | Bounce | timer: `spawnprojs = false`, nothing else (push not reached, colType NULL). Otherwise `push_worm = false; prj->Bounce(BounceCoeff)` (uses current `CollisionSide`); `if BounceExplode > 0: cClient->Explosion(fLastSimulationTime, pos, (float)BounceExplode, shake 0, owner, weapon)` **immediately**, every colliding step. |
| 1 | Explode | `info.explode = true; info.damage = Damage; if timer: info.timer = true; info.shake = max(info.shake, Shake); sound` (deferred). On a worm: push. |
| 2 | Injure | worm: `info.deleteAfter = true; InjureWorm(w, (float)Damage, owner, weapon)` **immediately**, then push. Otherwise falls through to Disappear. |
| 4 | Carve | if timer or terrain: `d = CarveHole(Damage, pos)` **immediately**, `info.deleteAfter = true`, owner `incrementDirtCount(+d)`. On a worm: nothing, but push (every step while overlapping). |
| 5 | Dirt | `info.dirt = true; if timer: shake = max(shake, Shake)`. Worm: push. |
| 6 | GreenDirt | `info.grndirt = true; if timer: shake = max(...)`. Worm: push. |
| 7 | Disappear | worm: `break` (push applies, no delete). Otherwise falls through to Nothing. |
| 8 | Nothing | `if terrain && colMask & PJC_MAPBORDER: info.deleteAfter = true; push_worm = false; if timer: spawnprojs = false`. |
| 9 | Disappear2 | `info.deleteAfter = true`; push on worm. |
| 10 | GoThrough | same as Nothing. |
| 11 | InjureProj | for each target that is a `CProjectile`: `target->injure((float)Damage)` (health, clamped at 0). v8. |
| 12 | PlaySound | `info.sound = Sound` (NULL in dedicated). |
| 13 | InjureWorm | worm: `InjureWorm(w, Damage, ...)` immediately, no delete; push. |
| 14 | ChangeRadius | `radius += ChangeRadius; clamp each >= 0`. |
| 15 | OverwriteOwnSpeed | `vel = Speed`. |
| 16 | MultiplyOwnSpeed | `vel = SpeedMult * vel` (MatrixD2: `(v1.x*x + v2.x*y, v1.y*x + v2.y*y)`). |
| 17 | DiffOwnSpeed | `vel = vel + Speed`. |
| 18-20 | Overwrite/Multiply/DiffTargetSpeed | same on every object in `ev.targets` (v8 hit events fill it). |
| 21 | HeadingToNextWorm | `vel += SpeedMult * getHeadingVelTo(nearestWorm(pos), prj)`. |
| 22 | HeadingToOwner | if owner valid and exists: same toward owner. |
| 23 | HeadingToNextOtherWorm | nearest alive worm with id != owner. |
| 24 | HeadingToNextEnemyWorm | nearest alive, id != owner, and not same team if team game. |
| 25 | HeadingToNextTeamMate | team game only; same team, id != owner. |
| 26 | HeadTargetToUs | each target: `target.vel += SpeedMult * getHeadingVelTo(prj, target)`. |

Fall-through chain: `Injure -> Disappear -> GoThrough/Nothing`. So a timer
`Injure` does nothing at all; a terrain `Injure`/`Disappear` only deletes at the
map border (and the projectile sits at its last safe position, because
`HandleCollision` resets it every step).

`nearest*` (`:670-724`) iterate `wormMap` in id order and keep the first worm
with the strictly smallest `GetLength2`. `getHeadingVelTo(target, src)`
(`:726-738`): `ret = normalize(src.vel.orthogonal())` (`(y, -x)`);
`a = Rotation(d.x, -d.y) * src.vel` with `d = normalize(target.pos - src.pos)`
(`Rotation(x,y) = MatrixD2(x, y, -y, x)`); `ang = atan2f(a.y, a.x); if ang >= PI: ang -= 2PI;`
`return ret * (ang / PI)`; NULL target gives `(0,0)`.

Fields only v8 can set (defaults for v7): `GoThroughSpeed = 1.0`,
`ChangeRadius = (0,0)`, `Speed = (0,0)`, `SpeedMult = 1.0` (diag),
`additionalAction = NULL`, per-action `Proj` unset (`Amount = 1`).

### 4.3 What v7 mods can actually reach

The v7 loader (`CGameScript.cpp:833-1157`) reads: `Hit.Damage` only for
Explode and Carve, `Hit.Projectiles/UseSound/Shake` only for Explode,
`Hit.BounceCoeff/BounceExplode` only for Bounce; `Timer.Type` only when
`Time > 0`, `Timer.Damage/Projectiles/Shake` only for Explode; `PlyHit.Damage`
and `PlyHit.Projectiles` only for Injure/Explode, `PlyHit.BounceCoeff` only for
Bounce (`PlyHit.BounceExplode` and `PlyHit.Shake` stay 0). Width/Height are
2 for `PRJ_PIXEL`, else 4. `Dampening` is a float read from 4 bytes.

### 4.4 `Proj_DoActionInfo::execute` (`:1165-1230`)

```
if explode:
    if !timer: if damage != -1: Explosion(fLastSimulationTime, pos, (float)damage, shake, owner, weapon)
    else:      d = Timer.Damage; if PlyHit.Type == EXPLODE: d = PlyHit.Damage      // quirk
               if d != -1: Explosion(fLastSimulationTime, pos, (float)d, shake, owner, weapon)
    deleteAfter = true
if dirt:    projectile_doMakeDirt(prj);  deleteAfter = true
if grndirt: projectile_doMakeGreenDirt(prj); deleteAfter = true
if trailprojspawn:   Trail.Proj.apply(prj, fLastSimulationTime)
if spawnprojectiles: GeneralSpawnInfo.apply(prj, fLastSimulationTime)
for s in otherSpawns: s->apply(prj, fLastSimulationTime)          // v8
if sound: StartSound(...)                                          // no-op in dedicated
hasAnyAction = Hit.hasAction() || PlyHit.hasAction() || Timer.hasAction() || any actions[i].hasAction()
if !hasAnyAction && (!Animating || AnimType != ANI_ONCE || bmpImage == NULL): deleteAfter = true
if deleteAfter: prj->setUnused()
```

`timer` stays true for the whole step once the timer fired, so if the timer
and a terrain `Explode` both fire, the timer damage rule wins. All effects use
the position **after** this step's move (except `Carve` and `BounceExplode`,
which act inside `applyTo`, and the timer's `Carve`, which acts **before** the
move). Explode and dirt can both happen in one step (explosion first).

`projectile_doMakeDirt` (`:1132-1144`): `d = PlaceDirt(5, pos - (6,6)) + PlaceDirt(5, pos + (6,-6)) + PlaceDirt(5, pos + (0,6))`
(float sums, which `PlaceDirt` truncates toward zero: `5.5 - 6` lands on x = 0);
`if hasOwner()` (owner in 0..31): owner `incrementDirtCount(-d)`.
`projectile_doMakeGreenDirt` (`:1146-1154`): `d = PlaceGreenDirt(pos)`; `if hasOwner()`: owner `incrementDirtCount(-d)`.
`incrementDirtCount(d)` (`CWorm.cpp:1381-1389`): `if d != 0: iDirtCount += d; if isServer: gameMode()->Carve(this, d)`.

### 4.5 Spawning children from a projectile (`Proj_SpawnInfo::apply`, `:562-640`)

Parent is the projectile (`PSPT_PROJ`):

```
heading = Useangle ? parent.angle() : 0
   // angle(): v = vel; NormalizeVector(&v); h = (float)(-atan2f(v.x, v.y) * (180.0f / PI));  // float*double -> double -> float
   //          h += 90; FMOD(h, 360.0f)
Proj->WeaponName = parent.proj->WeaponName                     // cosmetic, mutates shared proj_t
for i in 0..Amount:
    if UseParentVelocityForSpread: sprd = parent.vel * ParentVelSpreadFactor          // no draw
    else: sprd = GetVecFromAngle(((float)Angle + heading) + parent.getRandomFloat() * Spread)   // parent iRandom++
    rot = 0                                                    // UseRandomRot is false for projectile parents
    svv = UseSpecial11VecForSpeedVar ? (1,1) : sprd            // true for trails (Proj_Trail ctor, ProjAction.h:137)
    v = sprd * (float)Speed + (svv * (float)SpeedVar) * parent.getRandomFloat()       // parent iRandom++
        (+ ParentVelFactor * parent.vel only if AddParentVel: shots only)
    ignore = parent.fIgnoreWormCollBeforeTime
    random = parent.iRandom + 1
    pos = parent.pos + PosDiff (0,0 in v7); SnapToGrid unused in v7
    cClient->SpawnProjectile(pos, v, 0, parent.owner, Proj, random, spawnTime = parent.fLastSimulationTime, ignore)
```

So a child starts on the parent's current step time and runs its own steps from
there. `GeneralSpawnInfo` (event spawns) reads `Useangle, Angle, Amount, Spread,
Speed, SpeedVar` from the file; trails read `UseParentVelocityForSpread, Delay,
Amount, Speed, SpeedVar, Spread`. Guard: `Proj == NULL` would crash OLX.

---

## 5. Explosions, carving, dirt

### 5.1 `CClient::Explosion(time, pos, damage, shake, owner, weapon)` (`CClient_Game.cpp:227-330`)

```
// 1. dirt colour probe (cosmetic result, but decides rand())
gotDirt = false
y = MIN((uint)pos.y, H - 1); px = (int)(uint)pos.x
for x in px-2 .. px+1:                    // 4 pixels, "< px+2"
    if x < 0: continue
    if x >= W: break
    if GetPixelFlag(x, y) & PX_DIRT: gotDirt = true; break          // CMap.h:291-302
// 2. bonuses (FT_Bonuses, default off)
if Bonuses: for each used bonus with |bx-x|<=15 && |by-y|<=15: setUsed(false); Explosion(time, pos, 15, 5, owner)
// 3. particles: rand() even in dedicated
if gotDirt: repeat 2: SpawnEntity(ENT_PARTICLE, 0, pos, GetRandomVec().multPairwise(30,10), colour, NULL)   // 4 rand()
// 4. cosmetic entity
SpawnEntity(ENT_EXPLOSION, damage > 5 ? (int)damage : 8, pos, ...)            // no rand, takes a pool slot
// 5. carve
d = CarveHole((int)damage, pos)               // wormById(owner) (any worm, dead too): incrementDirtCount(+d)
                                              // (int) is cvttss2si: NaN or |damage| >= 2^31 gives INT_MIN -> Hole1
// 6. viewport shake: cosmetic, no rand
// 7. damage: NO falloff
for w in aliveWorms (id order):
    wPos = w.posRecordings.getBest(11, (size_t)(tLX->currentTime - time).ms)   // as the hit test (§3.6)
    if (pos - wPos).GetLength2() <= 25: InjureWorm(w, damage, owner, weapon)
```

`damage` is the float passed in (`(float)Damage` of the action). Damage 0 still
carves `Hole1` and "injures" with 0 (no blood, no score). Negative damage other
than -1 heals (and carves `Hole1`).

### 5.2 `CClient::InjureWorm(w, damage, owner, weapon)` (`CClient_Game.cpp:335-460`)

```
if !w.alive: return
ownerWorm = (owner in 0..31) ? wormById(owner) : NULL
if damage > 0: w.setLastWeaponHitBy(weapon)
me = ownerWorm && ownerWorm.id == w.id
if ownerWorm: damage *= ownerWorm.damageFactor()                         // FT_WormDamageFactor at prepare (CWorm.cpp:321)
if w.shieldFactor() == 0: damage = damage > 0 ? health + 1 : (damage < 0 ? MIN(health - 100, 0) : damage)
else: damage /= w.shieldFactor()                                         // FT_WormShieldFactor
if damage > 0 && ownerWorm && isTeamGame && !TeamInjure && !me && same team: return
if damage > 0 && ownerWorm && !SelfInjure && me: return
realdamage = MIN(health, damage)
(network damage report, damage report map, ownerWorm->addDamage(realdamage, w, false): scoreboard stat)
if w.injure(damage):                       // health -= damage; if health < 0 { health = 0; return true }
    if local or (server && bServerSideHealth):
        if isServer:                         // our case
            cServer->killWorm(w.id, owner)
            s = GetRandomInt(2)                                              // 1 rand
            repeat 7: SpawnEntity(ENT_GIB, 0, pos, GetRandomVec()*80, ..., w.getGibimg())   // 2 rand each (+2 inside, see 6.3)
            amount = (int)(50.0f * ((float)iBloodAmount / 100.0f))
            repeat amount: sp = GetRandomNum()*100+50; 3x SpawnEntity(blood..., GetRandomVec()*sp)     // 7 rand each
if damage > 0:
    amount = CLAMP(damage, 10.f, 100.f) * ((float)iBloodAmount / 100.0f)
    for (ushort i = 0; i < amount; i++):    // ceil(amount) iterations
        sp = GetRandomNum() * (50 + damage * 2)
        SpawnEntity(ENT_BLOODDROPPER, ..., GetRandomVec().multPairwise(sp, sp*4))
        SpawnEntity(ENT_BLOOD, ..., GetRandomVec()*sp)
        SpawnEntity(ENT_BLOOD, ..., GetRandomVec()*sp)                       // 7 rand per iteration
```

A worm dies only when health goes **below** 0 (exactly 0 survives).
`iBloodAmount` is `Game.Blood`, default 100 (`Options.cpp:213`). Cross-refs
(other areas, also dedicated `rand()`): `ParseWormDown` on the host's own client
repeats sound/gibs/blood when the death message arrives
(`CClient_Parse.cpp:1817-1856`, blood amount `CLAMP(health,50,100)*blood/100`);
`simulateWorm` bleeds below 15 HP every 2 s with 2 `GetRandomVec` per
iteration, 20 iterations (`PhysicsLX56.cpp:318-327`).

### 5.3 `CMap::CarveHole(size, pos)` (`src/common/CMap.cpp:1007-1106`)

Already ported (`rl-terrain::Terrain::carve_hole`). `size = clamp(size, 0, 4)`;
stamp `Theme.bmpHoles[size]` = `Hole{size+1}.png` (`CMap.cpp:332-336`,
colour key black). Top-left `((int)pos.x - w/2, (int)pos.y - h/2)`; clip with
the stamp **not** offset (kept quirk). Material: dirt under magenta (255,0,255)
becomes background, counted. Returns the count. No `rand()`.

### 5.4 `CMap::PlaceDirt(size, pos)` (`CMap.cpp:1114-1224`) — material part

```
size = clamp(size, 0, 4); hole = bmpHoles[size]          // projectile dirt: size 5 -> 4 -> Hole5 (15x15 in Dirt theme)
sx = (int)pos.x - (w >> 1); sy = (int)pos.y - (h >> 1)
clip_x = max(sx,0); clip_y = max(sy,0); clip_w = min(sx+w, W); clip_h = min(sy+h, H)
hole_x0 = -min(sx,0); hole_y0 = -min(sy,0)                // stamp IS offset by the clipped amount (unlike CarveHole)
count = 0
for dy in clip_y..clip_h, hy = hole_y0..: for dx in clip_x..clip_w, hx = hole_x0..:
    p = hole[hx,hy]; f = flags(material[dx,dy])          // flags read ONCE, before writing
    if p is not black (colour key, RGB compare) && !(f & ROCK):
        if f & EMPTY: count++
        material = DIRT
    if p not black && p != pink(255,0,255) && (f & EMPTY):
        material = DIRT; count++                           // a non-pink, non-black pixel on empty counts TWICE
return count
```

So: magenta or "edge colour" pixels turn background and dirt into dirt (rock
untouched); count +1 per background cell under magenta, +2 per background
cell under an edge-colour pixel. No `rand()`. All of this is inside
`if(bmpBackImageHiRes)`, which exists in the dedicated server (the `dig` oracle
scenario proves the same branch in `CarveHole` runs). The front tile is used
only for drawing.

### 5.5 `CMap::PlaceGreenDirt(pos)` (`CMap.cpp:1231-1340`)

Mask: `data/gfx/greenball.png`, loaded once (`CMap.cpp:365-367`), 8x9, no
colour key. Classes: green `(0,255,0)`, pink `(255,0,255)`, other (here
`(108,80,0)`, the rim). Decoded:

```
####b###
#bGGGGb#
#GGGGGGb
bGGGGGGG
GGGGGGGG
GGGGGGGb
bGGGGGG#
#bGGGGb#
###bbb##
```

```
sx = (int)pos.x - (8 >> 1) = (int)pos.x - 4; sy = (int)pos.y - (9 >> 1) = (int)pos.y - 4
same clipping as PlaceDirt (mask offset by the clipped amount)
for rows, cols (row-major):
    f = flags(material) (once)
    if p == green && (f & EMPTY): material = DIRT; count++; GetRandomInt(3)       // 1 rand() per placed green pixel
    if p != green && p != pink && (f & EMPTY): material = DIRT; count++           // rim pixel
return count
```

Only background cells change. `rand()` order is row-major over the clipped
area. Comparisons are raw `Uint32` in the RGBA8888 main format; every loaded
surface is opaque, so they are plain RGB compares.

### 5.6 Hole and mask images (Dirt theme, decoded here)

`Hole1` 5x5, `Hole2` 7x7, `Hole3` 9x9, `Hole4` 11x11, `Hole5` 15x15; colours:
black `(0,0,0)` (key), magenta `(255,0,255)`, edge `(108,56,0)`. The importer
already turns these into `rl_terrain::Stamp` (`Carve`/`Skip`/`Paint`);
`PlaceDirt` needs the same classes (`Skip` = transparent, `Carve` = pink,
`Paint` = other). The green-dirt mask needs a separate 3-class stamp
(green/pink/other), and it is not a theme image.

---

## 6. `rand()` and pseudo-random state on these paths

### 6.1 Fixed table (`iRandom`): gameplay, deterministic

| where | draws |
|---|---|
| `Spawn`: `fTimeVarRandom` | reads `table[iRandom]`, no advance |
| `CalculateCheckSteps` when `v^2 >= 250000` (at spawn, and when `bChangesSpeed` and `v^2` moved by > 50000) | 1 or 2 advances (`SIGN`) |
| `Spawn`: wallshoot (`LX56WallShooting`) | 1 advance |
| child spawn per child: spread (unless `UseParentVelocityForSpread`) | 1 parent advance |
| child spawn per child: speed var | 1 parent advance |

### 6.2 `rand()` (glibc): all run in the dedicated server

| site | calls | effect |
|---|---|---|
| `Spawn` colour, non-image with colours (`CProjectile.cpp:87-89`) | 1 | cosmetic |
| shot spawn `UseRandomRot && Proj->Rotating` (`:588-596`, shots only) | 1 per child | cosmetic rotation |
| `Explosion` dirt particles if `gotDirt` | 4 | cosmetic |
| `Explosion` bonus chain (`FT_Bonuses`) | via recursion | gameplay |
| `InjureWorm` damage blood, `damage > 0` | 7 x ceil(clamp(damage,10,100) * blood/100) | cosmetic |
| `InjureWorm` kill: sound, gibs, blood | 1 + 7 x 2 + 7 x (int)(50*blood/100), plus gib internals | cosmetic |
| `SpawnEntity(ENT_GIB)` when it gets a slot (`Entity.cpp:198-201`) | 2 | cosmetic |
| `PlaceGreenDirt` per green pixel placed | 1 | cosmetic |

Everything here is cosmetic in effect but **shifts the shared `rand()`
sequence**, which gameplay uses elsewhere (shot `nRandom`, bot AI, ...).
Argument expressions such as `GetRandomVec()*80` are evaluated before
`SpawnEntity` can reject the entity, so they are always consumed.

### 6.3 The entity pool gates the gib draws

`tEntities` is `FastVector<entity_t, 1024>` (`Entity.cpp:31-32`,
`include/Entity.h:30`). `SpawnEntity` (`Entity.cpp:161-205`):

```
if !bParticles && (type == ENT_PARTICLE || type == ENT_BLOOD): return
if type == ENT_GIB && img == NULL: return
ent = getNewObj(); if !ent: return          // pool full
... if ENT_GIB: fAnglVel = fabs(GetRandomNum())*20; iRotation = (int)(fabs(GetRandomNum())*3)
```

Entities are freed only by `SimulateEntities`, which a dedicated server never
runs, and the pool is cleared at game start (`ClearEntities`, `Game.cpp:545`).
So the port needs a saturating counter of successful `SpawnEntity` calls since
game start (all types, including smoke trails, explosions, laser sights,
beams, blood droppers, and the digging/rope particles noted in `STATUS.md`),
honouring the two filters. The gib image is loaded in dedicated
(`CWorm::ChangeGraphics`, `CWorm.cpp:655-667`; `LoadGameImage` works without a
screen, `GfxPrimitives.cpp:2958-2967`), so gibs are not filtered.
`bParticles` is `Game.Particles`, default true (`Options.cpp:215`).

---

## 7. Features read on these paths (`src/common/FeatureList.cpp`)

| feature | LX56 default | read at |
|---|---|---|
| `GameSpeed` (80) | 1.0 | projectile `frame_dt` (always), trail delay, v8 global timers |
| `GameSpeedOnlyForProjs` (81) | false | `CClient::Simulation` worm step only |
| `LX56PhysicsFPS` (20) | 84 | `LX56PhysicsDT` |
| `ProjGravityFactor` (182) | 1.0 | gravity |
| `ProjFriction` (180) | 0.0 | friction (off) |
| `LX56WallShooting` (183) | true | wallshoot window at spawn |
| `InfiniteMap` (170) | false | wrap paths (rl-sim rejects) |
| `TeamHit` (101), `SelfHit` (103) | true, true | `ProjWormColl` |
| `TeamInjure` (100), `SelfInjure` (102) | true, true | `InjureWorm` |
| `WormDamageFactor` (152), `WormShieldFactor` (153) | 1.0, 1.0 | via worm prepare |
| `Bonuses` (30) | false | explosion bonus chain |
| `CollideProjectiles` (191) | false | collision map only (no v7 effect) |
| `ShootSpawnDistance` (184) | 8.0 | shot spawn position (not this area) |
| `GameMode` | DM | `isTeamGame()` |
| `SuicideDecreasesScore`, `TeamkillDecreasesScore`, `CountTeamkills` | | `addDamage` statistic only |

---

## 8. Mod fields used, mapped to `rl-data::mod_def`

| OLX `proj_t` / `Proj_*` | rl-data | used for |
|---|---|---|
| `Type` (PIXEL/IMAGE) | `ProjectileDef::gfx` variant | worm-hit shape (rect), colour `rand()` |
| `Colour` | `ProjectileGfx::Pixel { colours }` | `rand()` count only (`len > 0`) |
| `Width`, `Height` | `width`, `height` | `radius = w/2, h/2` |
| `UseCustomGravity`, `Gravity` | `gravity: Option<i32>` | gravity |
| `Dampening` | `dampening` | per step, `bChangesSpeed` |
| `Timer.Time`, `TimeVar` | `timer.time`, `timer.time_var` | timer |
| `Timer` action | `timer.action` | timer action |
| `Trail.Type`, `Delay`, `Proj` | `trail.kind`, `trail.delay`, `trail.spawn` | trails |
| `Hit` | `hit` | terrain action and `HandleCollision` mode |
| `PlyHit` | `player_hit` | worm action; `PJ_NOTHING` disables the sweep |
| `GeneralSpawnInfo` | `event_spawn` | `spawns_projectiles` |
| `Proj_Action::Type/Damage/Projectiles/Shake/BounceCoeff/BounceExplode` | `ActionDef::kind/damage/spawns_projectiles/shake/bounce_coeff/bounce_explode` | |
| `UseSound/SndFilename/Sound` | `ActionDef::sound` | none in dedicated |
| `Rotating, RotIncrement, RotSpeed` | `ProjectileGfx::Image { rotating, rot_increment, rot_speed }` | cosmetic; shot rot `rand()` if `rotating` |
| `Animating, AnimRate, AnimType`, `bmpImage` | `Image { animation }`, image file | cosmetic in dedicated |
| `Proj_SpawnInfo` fields | `SpawnDef` (same names) | children |
| v8 only: `GoThroughSpeed`, `ChangeRadius`, `Speed`, `SpeedMult`, `additionalAction`, `PosDiff`, `SnapToGrid`, `actions`, RECT/CIRCLE/POLYGON | not modelled | defaults above |

---

## 9. Pseudo-code for the port (per frame, inside each worm physics step)

```
fn simulate_projectiles(world, now /* frame time */):
    let mut i = 0
    while let Some(slot) = pool.next_used_from(i) {        // reads last_used live
        let p = &mut pool[slot]
        loop {
            if p.last + frame_dt > now { break }
            p.last += frame_dt
            if !do_frame(world, now, DT_11MS, slot) { break }   // may spawn into the pool, may free slot
        }
        i = slot + 1
    }
```

`do_frame` follows 3.2 exactly; spawns allocate the lowest free slot; the
current projectile frees its own slot last.

---

## What the oracle hooks need

1. **Pin the wall clock.** In manual-step mode set `tLX->currentTime` from the
   simulation clock before each frame (for example in `beforeFrame`, or in
   `Game::frameInner` right before `cClient->Frame()`), e.g.
   `tLX->currentTime = simulationTime + K` with a fixed `K`. `K = 0` makes
   `ms_back` in `[0,11)`, so worm hit tests always use `get(0)` (the oldest
   slot); `K = 10` (the frame's end, closest to real play) mixes `get(0)` and
   `get(1)`. Either is reproducible; pick one and record it in the header.
   Without this, worm hits, explosion damage and shot spawn times depend on how
   fast Python drives the server. Also keep `tLX->fDeltaTime` as is (it is
   already fixed to 10 ms in the loop). Record `ct` (`tLX->currentTime`) and the
   simulation time in every frame line.
2. **Per projectile** (in addition to `i`, `t`, `x`, `y`, `vx`, `vy`, `life`):
   - `ls`: `fLastSimulationTime` (ms, relative to the recording start),
   - `ox`, `oy`: `vOldPos` (hidden state that decides resets),
   - `rnd`: `iRandom`, `own`: `iOwner`,
   - `ws`: `fWallshootTime`, `sp`: `fSpawnTime`, `ig`: `fIgnoreWormCollBeforeTime`,
   - `tr`: `fLastTrailProj`,
   - `mx`, `mn`, `av`, `csl`: `MAX/MIN/AVG_CHECKSTEP`, `iCheckSpeedLen`,
   - optionally `rx`, `ry` (radius) and `hp` (only v8 changes them).
   `fExtra`, `fRotation`, `fFrame` are cosmetic and can be skipped.
3. **`rand()` consumption.** Define `int rand(void)` / `void srand(unsigned)` in
   the patched binary (ELF interposition), delegating to `random()`/`srandom()`
   (what glibc's `rand` does) and counting calls; write the count per frame
   (`rc`). Optionally log the caller address (`__builtin_return_address(0)`)
   when debugging divergences. This catches every cosmetic draw listed in 6.2.
4. **Entity pool usage.** Export a getter from `Entity.cpp` (the pool typedef is
   file-local) and record `tEntities.size()` (`ents`) per frame; it decides
   whether gib draws happen.
5. **Options that are not features.** Add `Game.Blood` (`iBloodAmount`),
   `Game.Particles` (`bParticles`), `Misc.DoProjectileSimulationInDedicated`
   and the constant `K` from item 1 to the header.
6. **Worm position recorder** (optional but cheap): per worm, `cur`, `count`
   of `posRecordings`, or the two positions `get(0)` and `get(1)`, so a
   divergence in a hit test can be traced to the recorder.
7. Worm `dirt`, `hp`, `kills`, `deaths`, `lives` and `mh` are already
   recorded and cover carving, dirt counts and damage.

## Open questions

1. **Which `K`** (wall clock pinned to `simulationTime + K`)? It changes which
   worm position projectiles collide with, because of the `get(0)` quirk. Real
   play is a mix; any fixed `K` is a deviation to note in `STATUS.md`.
2. **Port the dedicated semantics or the client's?** In a client with images,
   `ANI_ONCE` deletes projectiles when their animation ends, and action-less
   animated projectiles survive until then; in the dedicated oracle they are
   deleted after one step and `ANI_ONCE` never ends anything. The same applies
   to the entity-pool gating of gib draws (a client frees entities). Matching
   the oracle means porting dedicated behaviour; feel-wise the client matters.
3. **The `get(0)` recorder bug**: should the port keep it for the oracle and
   offer a fixed variant later? A scenario where a bullet hits a walking worm
   will show it immediately.
4. Does the shared host client also receive and parse its own `WormDied` /
   shoot-list messages in the same frame or the next (the order of
   `ProcessServerShotList`, `ParseWormDown` and `Simulation` within
   `cClient->Frame()`)? This belongs to the shooting/death research but fixes
   projectile spawn times and the second batch of death `rand()` calls.
5. **Resolved (plan 4 task 5):** the oracle binary calls `sqrtf` in
   `LX56_simulateProjectile` (inlined `checkCollAndMove`), `atan2f` in
   `Proj_SpawnParent::angle` and `getHeadingVelTo`, and inlines `floorf` in
   `FMOD<float>`. `angle()` multiplies the float `atan2f` by the double
   `180.0f / PI` and rounds back to float.
6. Bots that are not scripted keep their AI and call `rand()` directly
   (`CWormBot.cpp:982-985`, `4234-4236`); scenarios should script every worm
   (or the port must emulate the bot AI's draws).
7. Integer edge cases to decide for untrusted content: `(int)` of huge or NaN
   floats (`(int)v.GetLength2()`, `(int)vel.x`) follows x86 `cvttss2si`
   (`INT_MIN`), and `abs(INT_MIN - x)` overflows in C; `Amount` may be
   negative; a NULL child `Proj` crashes OLX. The port should mirror x86 where
   the oracle can see it and refuse or clamp the rest (record in `STATUS.md`).
8. Themes other than Dirt have their own `Hole*.png`; `PlaceDirt`'s double
   count depends on how many edge-colour pixels a hole has, so it must use the
   level's theme stamps, not a fixed table.
