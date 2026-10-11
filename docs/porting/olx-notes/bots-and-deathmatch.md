# Bots and Deathmatch match flow (OLX `d1ad953f`)

Scope: the LX56 bot (`CWormBotInputHandler`), the weapon selection of bots at
game start (random weapons and `.wps` restriction files), and the Deathmatch
rules: settings, scoring, game over, the results screen. It ends with a design
for `rl-bots` (spec §5.9, §7.1, §12.1 H3).

All OLX paths are relative to the OLX checkout (`scratchpad/openlierox`) at
commit `d1ad953f`. Line numbers are at that commit. `CWormBot.cpp` means
`src/common/CWormBot.cpp`. The kill, death and respawn mechanics are already in
`damage-death-bonuses.md`; section 3 only summarises them and adds what that
note lacks.

Exact parity with OLX bots is **not** required (spec §5.9). Section 1 still
records the quirks, because they are a large part of how OLX bots feel.

---

## 0. Summary: the findings that matter most

1. **The bot is one big function called at physics rate.** `CWormBotInputHandler::getInput`
   (`CWormBot.cpp:1022-1122`) runs in every LX56 physics sub-step (11 ms), from
   `PhysicsEngine::simulateWorm` (`src/common/PhysicsLX56.cpp:300-307`), like a
   human's input. It does not press keys. It writes the worm directly: `fAngle`,
   `iFaceDirectionSide`, `iCurrentWeapon`, `tState` (`bMove`, `bShoot`,
   `bJump`, `bCarve`), and `cNinjaRope.Shoot/Clear`. A Rust bot that may only
   emit `Keys` needs a "motor" layer (section 4.4).
2. **Shooting comes first and freezes movement.** Each call first tries
   `AI_Shoot`. While it returns true (it is aiming or firing), the bot does not
   walk. Movement (`AI_Think`, `AI_MoveToTarget`) runs only when `AI_Shoot` returns false.
3. **The ballistic solver's angle is thrown away.** `AI_Shoot` computes a
   gravity-aware angle `alpha` (`AI_GetAimingAngle`) but only uses it as a
   feasibility test. It then aims straight at the target (`AI_SetAim(cTrgPos)`).
   For slow gravity weapons it checks a parabola through the bot and the target
   that does **not** depend on speed or gravity (`weaponCanHit`, `Parabola`).
   OLX bots therefore lob grenades poorly.
4. **With default settings, OLX bots do not use random weapons.** With
   Classic (`Liero v1.0`), `LoadingTime` 100 and the default `Standard 100lt`
   restrictions, `initWeaponSelection` (`CWormBot.cpp:4151-4166`) gives every
   bot the same fixed "100lt" loadout: Super Shotgun, Napalm, Cannon, Doomsday,
   Chaingun. It also switches the bot to hard-coded per-slot tactics
   (`GAM_100LT`). Rifle and Mortar loadouts exist too. Only other setups get `GetRandomWeapons`.
5. **The pathfinder ignores dirt.** Areas are maximal rectangles free of
   **rock** only (`PX_ROCK`). Paths go straight through dirt, and the bot digs
   with `bCarve` every 0.2 s and with "clearing" weapons. In LX56, rock is never
   created or destroyed, so the area structure is **static for the whole match**.
6. **Difficulty changes noise, not intelligence.** Each level sets four numbers:
   the aim noise (±50/33/17/0°, refreshed every 0.5 s and **shared by all bots**),
   the aim tolerance before firing (23/16.3/9.7/3°), the target position noise
   in `weaponCanHit` (±50/33/17/0 px) and the weapon-change delay
   (0.5/0.3/0.2/0.12 s). One rifle-only tactic is enabled from Hard upwards.
7. **Nothing in the bot is deterministic.** It reads the wall clock
   (`tLX->currentTime`, and `GetTime()` in `AI_Jump`), draws from the
   **global** `rand()` (the gameplay stream), and finds paths in a separate
   thread that sleeps 1 ms per area. The remake must replace all three (section 4.3).
8. **The Deathmatch game ends** on the kill limit (`getScore() >= KillLimit`, default 15),
   the time limit (default 6 min), or, in a local game, when fewer than 2 worms
   still have lives. The winner is the last worm not out, else the best by
   (lives if limited) → not-out → score → damage. Ties keep the lower worm id.

---

## 1. The bot (`CWormBot.cpp`, `include/CWormBot.h`)

### 1.1 Files and entry points

| What | Where |
|---|---|
| Class, members | `include/CWormBot.h:22-152` |
| States `AI_THINK`, `AI_MOVINGTOTARGET` (`AI_AIMING`, `AI_SHOOTING` unused) | `CWormBot.cpp:69-75` |
| Target types `AIT_NONE/WORM/BONUS/POSITION` | `CWormBot.cpp:78-83` |
| Game types `GAM_RIFLES/100LT/MORTARS/OTHER` | `CWormBot.cpp:86-92` |
| Path node `NEW_ai_node_t` (float x, y; doubly linked) | `CWormBot.cpp:95-99` |
| Pathfinder `searchpath_base` | `CWormBot.cpp:272-878` |
| Constructor (difficulty from profile, default Medium) | `CWormBot.cpp:4088-4113` |
| `getInput` (per sub-step) | `CWormBot.cpp:1022-1122` |
| `onRespawn` → `AI_Respawn` | `CWormBot.cpp:4120-4126`, `993-997` |
| `initWeaponSelection` (loadout) | `CWormBot.cpp:4129-4192` |
| `subThink` (Gusanos only, see 1.13) | `CWormBot.cpp:4227-4238` |
| `fastTraceLine` | `src/level/FastTraceLine.h:24-104` |
| `Parabola` | `src/common/MathLib.cpp:182-225`, `include/MathLib.h:158` |
| Difficulty constants `AI_EASY`..`AI_XTREME` = 0..3 | `include/ProfileSystem.h:52-55` |
| Default CPU profiles | `src/client/ProfileSystem.cpp:49-60` |

There are no other AI helper files. `PRF_COMPUTER` creates the handler
(`CWormBot.cpp:4082-4086`).

### 1.2 Per-call control flow (`getInput`, `CWormBot.cpp:1022-1122`)

```text
getInput():                                   // every physics sub-step (11 ms)
  if !alive: if bCanRespawnNow: bRespawnRequested = true; return     // respawn at once
  ws.reset()                                  // bMove/bShoot/bJump/bCarve = false
  if currentTime - fSpawnTime < 0.4 s: return // "behave like humans" spawn delay
  bOnGround = CheckOnGround()
  iRandomSpread = 0; fLastRandomChange = 0    // (see 1.7: makes the spread re-roll every call)
  think_interval = 3 s (CTF: 0.5 s)
  if currentTime - fLastThink > think_interval and state != THINK: state = THINK
  if currentTime - fLastThink > 0.5 s and !bPathFinished:          state = THINK
  if target is a worm that is gone, dead or AFK:                      state = THINK
  AI_Carve()                                  // bCarve = true at most every 0.2 s
  if AI_Shoot():                              // aiming or firing: stand still
      if rope attached and |fAngle - rope_angle| <= 50°: rope.Clear()
      ws.bMove = false
  else:
      AI_ReloadWeapons()                      // select the first empty slot
      switch state: THINK -> AI_Think(); MOVINGTOTARGET -> AI_MoveToTarget()
  iMoveDirectionSide = iFaceDirectionSide     // no strafing
```

- **`AI_Carve`** (`3304-3314`): sets `bCarve` when `currentTime - worm.fLastCarve > 0.2 s`.
  The physics then carves at `pos + moveDir*4` (`PhysicsLX56.cpp:334-338`). This
  runs whatever the bot is doing, including in open air.
- **`AI_ReloadWeapons`** (`1594-1603`): `iCurrentWeapon` = the lowest slot
  with `Reloading`. In LX56 only the **current** slot recharges
  (`simulateWormWeapon`, `PhysicsLX56.cpp:471-497`), so this is how a bot reloads.
  The effect: the bot fires the current weapon until it is empty. While it
  cannot fire, it reloads the lowest empty slot, then fires again.

### 1.3 Thinking and target selection

`AI_Think` (`1430-1514`), Deathmatch path (not Race):

```text
AI_Think():
  clear target; fLastThink = currentTime; AI_ReloadWeapons()
  if health < 15 and AI_FindBonus(HEALTH): return              // critical health first
  if some slot has no weapon and AI_FindBonus(WEAPON):          // only with DisableWpns*
      select the first empty slot (the pickup fills it); return
  if findNewTarget(): AI_CreatePath(); return                   // DM: nearest enemy
  else fLastShoot = 0
  if health < 80 and AI_FindHealth(): return
  if cPosTarget.y < 75 and state == MOVINGTOTARGET:             // already high up
      if health < 100: AI_FindHealth()                           // (never true: AI_Think runs
      return                                                     //  in THINK and only the returns
                                                                 //  above set MOVINGTOTARGET)
  if findRandomSpot(highSpot=true): AI_CreatePath()             // "high ground is safer"
```

- **`findNewTarget`** (`1180-1293`): the game-mode switch (Tag, Hide and Seek,
  CTF, Race). Deathmatch takes the final `else` (`1272-1290`): `findTarget()`,
  then target type `AIT_WORM` and state `MOVINGTOTARGET`.
- **`findTarget`** (`1297-1364`): over `game.aliveWorms()` (id order). It skips
  self, AFK, team mates (team games) and non-"it" worms (Tag). For each worm:
  - `l = |w - me|²`, then `traceLine(w.pos, &len, &type, divs=1)`. This stops
    at the first **non-empty** pixel and returns its flags (`2641-2685`).
  - Line of sight means "no rock was hit". **Dirt counts as line of sight.**
  - `trg` = the nearest worm in line of sight; `nonsight_trg` = the nearest worm overall.
  - The result is `trg`, unless the two squared distances differ by more than 50
    (`fSightDistance - fDistance > 50`, squared px, so almost always), or the
    game is `GAM_RIFLES`, or there is no visible worm. In those cases it is the
    nearest worm overall.
  - So, in practice: **the nearest enemy, regardless of walls.**
- **`AI_GetTargetPos`** (`1609-1644`): bonus position (THINK if the bonus is
  gone), position, or worm position (THINK if dead).
- **`AI_FindBonus`** (`1551-1589`): returns false unless `FT_Bonuses`. It picks the
  nearest used bonus of the type, sets `AIT_BONUS`, and starts a path.
  "TODO: verify that the target is not in some small cavern".
- **`findRandomSpot(high)`** (`1516-1541`): up to 10 tries of
  `(|GetRandomNum()|·W, |GetRandomNum()|·H/5)` that pass `IsGoodSpawnPoint`
  (no rock in the 7×7 box, `src/game/CMap.h:460`). Each try draws 2 `rand()`.
- **`AI_FindShootingSpot`** (`3951-4034`), used instead of the worm's position
  as the path target (`AI_CreatePath`, `AI_MoveToTarget`, `AI_SimpleMove`):
  ```text
  classify own weapons by g = (custom gravity or 100) * ProjGravityFactor:
      g >= 5 -> have_falling;  -5 <= g < 5 -> have_straight
  if have_falling or (have_straight and target on ground): j in [π/2, 3π/2]  // upper half circle
  elif have_straight:                                       j in [0, 2π]
  else (only rising):                                      j in [-π/2, π/2]  // lower half
  for j step 0.3 rad:
      p = target + 40·(sin j, cos j)
      if AI_IsInAir(p, 1) (15×15 box without rock/dirt)
         and traceLine(p, target) >= 40: return p
  fallback: walk right from the target in 5 px steps (up to 10) until traceWormLine
            is blocked; step back; return if >= 15 px away
            same to the left (bug: steps back the wrong way, `x -= 5` at 4026;
            at the map's left edge it returns a point outside the map)
  return the last tried position (target.x - 50)
  ```
  The `traceLine(...) >= 40` test truncates a float length of about 40 to an
  int. It may fail often, which would make the fallback the usual case (open question 6).
  On a dirt level `AI_IsInAir` (no rock or dirt in 15×15 px) fails anyway, so the
  spot is the fallback's, as deep in the dirt as the target (`rl-bots` digs on to
  the worm from there: `STATUS.md`).

### 1.4 Pathfinding (`searchpath_base`, `CWormBot.cpp:110-878`)

The idea (by AZ): an A*-like search whose nodes are **maximal free rectangles**
("areas"), grown lazily from the start towards the target.

**Constants:** `wormsize = 7` (`56`) for areas and passages. The checklist
spacing is 5 px (`checklistRowHeight/ColWidth`, `371-374`). `traceWormLine`
uses its own width of 3 (`2837`).

**Free test:** `material.toLxFlags() & PX_ROCK` (`169`, `246`, `257`, `472-480`). Dirt is free.

**Growing one area (`getMaxFreeArea(p, PX_ROCK)`, `115-200`):**
```text
rect = (p, p); blocked = 0; dir = RIGHT
loop until blocked == all four:
    dir = next unblocked direction in the cycle DOWN, LEFT, UP, RIGHT (starting with DOWN)
    probe the line just outside that edge (full edge length)
    if out of map or any rock pixel on it: blocked |= dir
    else: move that edge out by 1 px
clip the rect to the map
```
The result is a greedy maximal rectangle, not necessarily the largest one. The
cost is about the area's pixel count, because each probed line is new.

**Area node (`area_item`, `280-499`):**
- `dist_from_source` = the predecessor's distance + `getDistToArea(pred)`:
  centre → connector → centre (`339-365`).
- The connector point is the centre of the intersection of the two rectangles (`347-358`).
- Priority: `f = dist_from_source + |centre - target|` (`331-337`).
- **Expansion (`process` → `forEachChecklistItem` + `check_checkpoint`, `384-497`):**
  ```text
  checkpoints: every 5 px along each edge, centred (initChecklists 321-328);
               sorted by distance to the target (multiset; order only matters for ties)
  for each checkpoint pt on an edge, with dist = 5 px outward:
      count parallel 1-px-apart traces of length 5 outward (simpleTraceLine, rock only):
          'left' going +dir until blocked (max 7), 'right' going -dir (max 7-left)
      if left + right >= 7:  addAreaNode(pt' + dist, this)   // pt' = the last probed point
  ```
  So a passage must be at least 7 px wide and 5 px deep beyond the edge.
  Precisely (checked in `rl-bots`' `areas_grow_like_olx`): `pt` itself is
  never traced, so a gap of 7 passes only when the checkpoint is on the row
  just outside it (7 traces on one side), one of 8 or more always passes,
  6 never; and the new area is grown at `pt + dist`, one step past the last
  `-dir` trace, so a long corridor must itself hold an area (8 px, since
  `v2 - v1 >= 7`).
- **`addAreaNode(p, pred)`** (`648-679`):
  - It ignores points outside the map.
  - If an existing area contains `p` (a linear scan, `562-584`), it relaxes:
    if `f(via pred) < f`, re-parent the area and re-insert it into the open set.
  - Otherwise it grows a new area at `p`. The area is kept only if it is at least
    7 px wide and 7 px high (`671`).
- **Open set** (`areas_stack`): a `std::set` ordered by `f`, where two values
  within 0.0001 compare **equal** (`293-304`). An area whose `f` ties with one
  already queued is silently **not inserted**, and `erase(a)` can remove the
  other one. This is a quirk. A Rust port should use a heap with an insertion-index tie-break.

**Search (`findPath`, `618-642`):**
```text
open.clear(); addAreaNode(start, null)
while open not empty:
    sleep 1 ms (SDL_Delay)                         // thread throttle
    abort if asked to stop/restart or the map is gone
    a = open.pop_min_f()
    if traceWormLine(target, a.centre): return buildPath(a)    // target visible from the centre
    a.process()
return null
```

**Path assembly (`buildPath`, `593-615`):** walking back from the found area,
the node list is, from the start:
`centre(a1), centre(a1), conn(a2,a1), centre(a2), …, conn(an,an-1), centre(an), target`.

**Post-processing in the search thread (`threadSearch`, `699-743`):**
1. `completeNodesInfo` (`800-806`): fixes the `psPrev` links.
2. `simplifyPath` (`838-876`):
   - For each node, scan ahead. Keep `len` = the running sum of **squared**
     segment lengths. If at least 3 nodes ahead, `|node→cand|² < len`, and
     `traceWormLine(cand, node)`, shortcut `node.next = cand` (later candidates
     override earlier ones).
   - The second "simplify" loop never runs, because `closest_node` is null after
     the first loop. It is dead code.
3. `splitUpNodes` (`808-835`): while a segment is longer than 50 px on either
   axis, insert a node 50 px along the dominant axis. The result is nodes about every 50 px.

**Lifecycle (`AI_CreatePath`, `2869-2955`), asynchronous:**
```text
trg = target pos (AI_FindShootingSpot for worms)
if force_break: restart the thread search(pos, trg); bPathFinished = false; return
if !bPathFinished:
    if searcher ready: adopt its result as NEW_psPath (current = first node); return true
    else if searching for >= 5 s, or start/target no longer worm-visible from the
            searcher's start/target: restart the search
    return false
if last node sees trg: append trg as a new last node, current = first node
                       that sees me; return true
if < 0.5 s since the last path was created: return (path exists)
start a new search(pos, trg); bPathFinished = false
```
Searches start from `AI_Think` (every 3 s), `AI_Respawn`, `AI_FindBonus` and the stuck handler.

**`traceWormLine(target, start)`** (`2834-2853`): three `fastTraceLine`s,
1 px apart, perpendicular to the line, checking `PX_ROCK` only.
`fastTraceLine` (`FastTraceLine.h:24-104`) walks the dominant axis 1 px per
step and calls the action on matching pixels. Every bot action returns false,
so the trace stops at the first hit. Out-of-map pixels also go to the action.

### 1.5 Following a path: `AI_MoveToTarget` (`3370-3942`)

This is where a path becomes movement, rope, jumps and digging.

```text
AI_MoveToTarget():
  if no target (or target worm gone): state = THINK; return
  ws.bMove = ws.bShoot = ws.bJump = false
  rope timers: fRopeAttachedTime (+= fDeltaTime while attached),
               fRopeHookFallingTime (+= while shooting, not attached)
  if fRopeAttachedTime > 5 s: rope.Clear()             // hung too long
  if fRopeHookFallingTime >= 2 s: rope.Clear()         // hook missed
  cPosTarget = target pos (shooting spot for worms)
  [GAM_MORTARS: 0.2-1 s after a shot, if moving towards the shot: release rope if it
   pulls that way, face away, move]
  [GAM_MORTARS / GAM_100LT: on ground and a worm within 50 px with a clear line: AI_Jump()]
  [GAM_MORTARS: return for 1 s after a shot ("don't do anything crazy")]
  if bStuck: move, AI_Jump(), flip facing every 0.5 s; un-stick after 2 s; return
  [GAM_MORTARS: within 0.2 s after a shot: release rope if attached >= 0.1 s; return]

  if no path: state = THINK; AI_SimpleMove(); return

  // node advance
  if |node - pos|² <= 100: node = node.next (if any)
  for every later node n: if traceWormLine(n, pos): node = n      // skip to the farthest visible node
  if none: once per second, walk back to the nearest earlier visible node
  if still none (node null): THINK; AI_SimpleMove(); return

  if rope attached and the rope force points away from the node: rope.Clear()
  we_see_the_target = traceWeaponLine(cPosTarget) is all empty

  [GAM_RIFLES, difficulty >= HARD: if <= 3 rifles are loaded and the target is not
   visible: reload when >= 3 nodes away; stop (and release a rope attached >= 0.7 s)
   when 3-5 nodes away]

  // digging vs roping
  (len, type) = traceLine(pos → node)                  // single line, first non-empty pixel
  direct = len² > |node - pos|²
  if |dx| <= 50 and type has DIRT and node above:      // dirt right above
      if w = AI_FindClearingWeapon() >= 0: select w; bShoot = AI_SetAim(node);
                                           if bShoot: no move, no jump
      else: AI_SetAim(node); AI_Jump(); fireNinja = true
  if !fireNinja and !direct and type has DIRT and len < 10:
      if node is 10-30 px above: AI_Jump()             // dig through by walking + carving
  else:
      fireNinja = true                                 // free way: rope is faster
      if a weapon has Recoil < 0 (jetpack) and is loaded: select it;
          bShoot = AI_SetAim(node); if bShoot: fireNinja = false

  [GAM_RIFLES: no rope when the target is visible and within 60 px]

  if fireNinja:
      fireNinja = false
      ropespot = AI_GetBestRopeSpot(node)
      aim = AI_SetAim(ropespot); stillAiming = !aim
      if aim or !onGround:
          if rope not out: fireNinja = true
          elif attached and fRopeAttachedTime > 0.5 s: fireNinja = true   // re-shoot
          if node more than 10 px above: AI_Jump()
  if fireNinja: rope.Shoot(faceDirection); reset rope timers
  else:
      if !we_see_the_target and !stillAiming: AI_SetAim(node)
      if canAirJump: (node > 10 px above and more vertical than horizontal) → AI_Jump(); move
      elif node 10-30 px above: if !AI_Jump(): move
      if stillAiming and isMoveGivingDisadvantage(...): bMove = false; return

  if rope attached, in air, hook below me, node above, |hook.x - x| <= 50, vy <= 0:
      (meant to release when we will fly past the node; `force` is a zero
       vector, so `time` is inf and the test is effectively diff < 0 → Clear)
  stuck check: if within 5 px of cStuckPos: fStuckTime += dt
      if > 3 s: jump, move, carve, bStuck = true, rotate aim up by BotAngleSpeed·dt,
                at >= 5 s: node = node.prev, fStuckTime = 0;
                AI_CreatePath(); return
  else: reset the stuck state, cStuckPos = pos
  bMove = |x - node.x| > 3   (always true if the mode forbids shooting)
```

Helpers:
- **`AI_GetBestRopeSpot(trg)`** (`3200-3280`):
  - It casts 20 rays (π in steps of 0.05π), each `RopeMaxLength` long (300).
    The fan is half a circle starting perpendicular to the direction to `trg`.
    Each ray stops at the first rock or dirt (`PX_ROCK|PX_DIRT`).
  - Each hit point is scored, ignoring hits closer than 30 px and, when the bot
    wants to go up, hits below `y - 10`:
    `1e5/|hit - (trg - (0,30))| + 1e4/|aimDir - hit/|hit|| + 0.001·max(0, -len(len-200))`.
    If the bot must go up by more than 40 px, the score is only the first term.
    The `hit/|hit|` term normalises the absolute position (a bug). It was meant
    to be the direction from the worm.
  - It returns `trg` if nothing scored.
- **`AI_Jump`** (`3353-3366`): sets `bJump` if `GetTime() - fLastJump > 0.3 s`
  (**real** wall clock), the worm is on the ground or can air-jump, and
  `!isJumpingGivingDisadvantage(currentNode)`.
  - That test (`3317-3333`) traces a worm line from the node to `pos + (0, dy)`,
    with `dy = g·0.3²/2 + jumpForce·0.3`.
  - Bug: `g` is read from `FT_WormJumpForce` (`3320`), so `dy ≈ -26` px.
- **`isMoveGivingDisadvantage`** (`3335-3349`): `dx = clamp(vx + ±speed·90, ±30)·0.3`;
  it is true if `pos + (dx, 0)` cannot see the target.
- **`AI_SimpleMove`** (`1719-1768`), the fallback without a path:
  - It moves towards the target and aims at it.
  - If `traceLine` stops at once or the target is above: every 1 s it flips
    facing. It then aims at `pos + (rand·10, rand·10 + 10)`, which is *below*
    the worm, despite the comment "look up". If aimed, it shoots the rope;
    otherwise it jumps, moves and releases the rope.
  - It draws 2 `rand()`.
  - `fDist < 0.75` is never true when the trace advanced, because of the int
    division in `CWorm::traceLine` (`2643`).
- **`AI_FindClearingWeapon`** (`1776-1816`):
  - With `SelfInjure` on and `GAM_MORTARS`, it finds none.
  - Otherwise it takes the first loaded projectile weapon whose hit type is not
    `Dirt`, `GreenDirt` or `Bounce`, and whose name lacks
    "dirt/napalm/grenade/nuke/mine".
  - With `SelfInjure` on, it also skips weapons with speed < 100 unless they have
    custom gravity ≤ 30.
  - Then any beam. Otherwise -1.
- Unused: `AI_FindClosestFreeSpotDir` (`3024-3150`) and `AI_FindClosestFreeCell` (`2688-2691`).
  `AI_CheckFreeCells` always returns true (`2857-2864`).

### 1.6 Shooting: `AI_Shoot` (`2001-2326`)

```text
AI_Shoot():                                       // true = "busy aiming/shooting, don't move"
  if the mode forbids shooting, or Race: return false
  if !canShootRightNowWithCurWeapon(): return false   // current slot reloading, LastFire > 0,
                                                      // empty; specials only for the jetpack
  w = findTarget(); if none: return false          // its own target, not nAITargetWormId
  d = |w - me|
  (fDist, nType) = traceWeaponLine(w.pos)          // 1.6.1
  bDirect = !(nType & ROCK)
  if team game and a mate is in the line: return false
  if nType & DIRT:
      bDirect = false
      if the target is above (diff.y < 0, |dy| > |dx|) and d - fDist > 40
         and w = AI_FindClearingWeapon() >= 0 and AI_GetRockBetween(me, w) <= 3:
          select w; if AI_SetAim(target): fire; return true
          return false
  GAM_MORTARS: if the line is not clear and fDist <= 25: return false   // fDist is a fraction: always
  wpn = AI_GetBestWeapon(gameType, d, bDirect, fDist); if < 0: return false
  select wpn
  if the projectile's hit type is Dirt/GreenDirt: (no aim, bAim = false)
  elif projectile weapon:                          // 1.6.2
      v = Proj.Speed + SpeedVar·100 + (my velocity · dir_to_target)
      if v < 0: bAim = false
      (x, y) = target - me, y up; t = 0.7·|(x, y)|/v
      x += t·target.vx; y -= t·target.vy           // lead
      g = gravity along the spawn chain (1.6.3) · ProjGravityFactor
      bAim = AI_GetAimingAngle(v, g, x, y, &alpha) // feasibility only (alpha is unused)
      iRandomSpread = GetRandomInt({13,8,3,0}[diff]) · sign(GetRandomNum()); alpha += …  // unused
      if g <= 10 or v >= 200: bAim = bDirect      // fast/straight weapons need line of sight
      if !bAim: break
      bShoot = true
      r² = |target - me|²
      if r² >= 2500 (50 px): bAim = AI_SetAim(target)
      elif r² >= 100:        bAim = AI_SetAim(target); break        // close: no trajectory check
      else:                  bAim = bShoot = false                  // < 10 px: move away
      if bAim and g >= 10 and v <= 200: bShoot = bAim = weaponCanHit(g, v, me + (x, -y))
  elif beam:
      if bDirect and Bm.Length >= d: bShoot = bAim = AI_SetAim(target) or d <= 25
      else: if bDirect: AI_SetAim(target); bShoot = bAim = false
  (client-only lag guard: if moving fast (|v|² >= 3600) towards the aim within 30°
   with a damaging projectile: return false)
  if !bAim:
      fBadAimTime += dt; if > 4 s: jump if there is room above; reset
      return bShoot                                 // true = keep standing and aiming
  fBadAimTime = 0; vLastShootTargetPos = target
  ws.bShoot = true; fLastShoot = currentTime; return true
```

#### 1.6.1 `traceWeaponLine` (`2695-2809`)

It steps from the worm towards the target and stops at the first **dirt or rock** pixel:
- Step sizes `(first, then)`: beam (1, 1); rifles (10, 6); mortars (7, 2);
  100lt (6, 3); otherwise (7, 5). The first step gets +5 for the worm's
  thickness, so a bot can "shoot through" thin walls next to it.
- It returns `fDist` = the travelled **fraction** (0..1) and `nType` = the pixel
  flags (`PX_EMPTY` if clear).
- In team games, a mate within 20 px of a sample point adds `PX_WORM`.

#### 1.6.2 The aim solver `AI_GetAimingAngle(v, g, x, y)` (`1896-1960`)

```text
if |y| < 3 and y <= 0: y = -y          // "small hack"
D  = |v⁴ - 2v²yg - g²x²|               // fabs: an unreachable target is never rejected here
t2 = -x²√D + x²yg + v²(x² + 2y²);  if t2 < 0: fail
t3 = √D - yg + v²;                     if t3 <= 0 or x == 0: angle = 60, fail
a  = atan(√t2 / (x√t3));  if x < 0: a = -a;  if y > 0: a = -a
a in degrees; fail if a > (FullAimAngle ? 90 : 60) or a < -90 (clamped)
```

Checked numerically against the textbook solution
`tan θ = (v² ± √(v⁴ − g(gx² + 2yv²)))/(g|x|)`:
- It is the **low** root.
- It is expressed in OLX's worm angle convention: negative is up, relative to
  the facing side.
- For targets slightly below or level (`-3 < y ≤ 0`), the sign is wrong: it aims down.
- Because of `fabs`, out-of-range targets still produce an angle.

Since the angle is discarded, only the fail cases matter. An unreachable target
mostly fails through `t2 < 0`.

#### 1.6.3 Gravity used (`2176-2200`)

`g = 100`, or the projectile's custom gravity. Then it walks
`GeneralSpawnInfo.Proj` children and takes the largest gravity (non-custom
counts as 100). It stops at a projectile whose timer spawns at `Time >= 0.5`,
or whose hit, player-hit or touch spawns. Finally it multiplies by
`ProjGravityFactor`.

#### 1.6.4 `weaponCanHit` (`1823-1893`)

- It needs `nAITargetWormId >= 0`.
- Below Xtreme, it adds noise from 4 shared statics, refreshed every 0.5 s:
  gravity `±5f`, speed `±20f`, target `±50f` px on each axis, with `f = (3 - level)/3`.
- Only the target noise has any effect.
- It builds `Parabola(me, alpha = ±fAngle in radians, target)` (`MathLib.cpp:204-225`):
  the parabola through both points with tangent `fAngle` at the bot. It does not
  depend on `v` or `g`.
- If `p.a > 0.1`, it fails. Otherwise it samples x from `from.x + 5` to `to.x`
  in 1 px steps and fails on `CProjectile::CheckCollision` with dirt or rock.
- The bot aims straight at the target, so the parabola is nearly a straight line.
  The check therefore asks "is the straight line clear of terrain?".

#### 1.6.5 `AI_SetAim(pos)` (`1651-1713`)

```text
dir = normalise(pos - me); wanted side = dir.x < 0 ? LEFT : RIGHT
if currentTime - fLastFace > 0.3 s: face = wanted side; fLastFace = now   // turn at most every 0.3 s
ang = atan2(dir.x, dir.y) in degrees; LEFT: ang += 90; RIGHT: ang = 90 - ang   // worm convention
ang = max(ang, -90)
if level < XTREME:
    shared static angleDiff[level], refreshed every 0.5 s: GetRandomNum()·50·(3-level)/3
    ang += angleDiff[level]; ang = max(ang, -90)
fAngle += sign(ang - fAngle)·BotAngleSpeed·fDeltaTime    // 100°/s, BotAngleSpeed at 1646
if |fAngle - ang| < 3: fAngle = ang                       // snap
good = |fAngle - ang| < 3 + 20·(3-level)/3
fAngle = clamp(fAngle, -90, FullAimAngle ? 90 : 60)
return good
```

`fDeltaTime` is fixed at 10 ms during the simulation loop
(`src/game/Game.cpp:905`), while `getInput` runs per 11 ms sub-step. So bots
turn at about 91°/s. The "LX56 aim speed limit" is this constant, not the human
acceleration model.

### 1.7 Weapon choice: `AI_GetBestWeapon` (`2332-2633`)

```text
if currentTime - fLastWeaponChange > {0.50, 0.30, 0.20, 0.12}[level]: fLastWeaponChange = now
else: return current                                       // even if it is reloading

GAM_RIFLES / GAM_MORTARS: the first loaded slot of 0..4, else -1

GAM_100LT (slots 0 SSG, 1 Napalm, 2 Cannon, 3 Doomsday, 4 Chaingun):
  if above the target, within 100 px, worm line clear, target on ground, Napalm loaded: 1
  d < 50:   visible: SSG, Chaingun, Doomsday, Cannon (no rope; also bMove = false)
            not visible: jump; -1
  50-300:   visible: Chaingun, Cannon (bug: only when the rope is out, isReleased at 2422),
            SSG, Doomsday (no rope)
  > 300:    visible: Doomsday (no rope), SSG, Chaingun, Cannon (same bug)
  (then falls through to the generic rules below)

generic (also the fall-through):                           // "first loaded in slot order" per bucket
  1. target near the map bottom (y > H-50) and d < 200: first projectile with hit Explode
  2. d < 100 and direct: first beam, else first projectile whose hit is not Dirt/GreenDirt
  3. d < 150 and direct: the same as 2 (redundant)
  4. direct: first non-dirt projectile, then first beam, then `slots` random tries
             GetRandomInt(slots-1) for a loaded slot
  5. indirect, d > 190, health > 25, fTraceDist > 0.5, target 20+ px below:
     first projectile with hit Explode or Bounce
  6. first beam; `slots` random tries; first loaded slot; -1
```

Points to note:
- No damage, speed, splash or self-danger is considered in the generic path, so
  slot order dominates.
- "Loaded" means `!Reloading`. A partly charged slot counts as loaded.
- `AI_GetTargetPos()` (used for the 100lt "above" test and rule 1) is the
  *path* target. That can be a bonus, not the worm `AI_Shoot` chose.
- `iRandomSpread`: `getInput` resets `fLastRandomChange` to 0 every call, so the
  "every 0.5 s" re-roll happens on every call. It draws 2 `rand()` each time,
  even at Xtreme (`GetRandomInt(0)`). The value only changes the unused `alpha`.

### 1.8 Rope, digging, dodging, reloading, bonuses: where they live

| Behaviour | Where | Notes |
|---|---|---|
| Rope shoot | `AI_MoveToTarget` 3784-3825; `AI_SimpleMove` 1750-1754 | aim at the best rope spot first; re-shoot after 0.5 s attached |
| Rope release | 1081-1090 (shooting), 3410, 3420, 3632, 3853-3866, mortar cases | attached > 5 s, hook falling ≥ 2 s, pulling away from the node, overshoot |
| Digging | `AI_Carve` every 0.2 s; clearing weapon at dirt above (3696-3714, 2084-2097); stuck handler carves | the path ignores dirt |
| Jumping | `AI_Jump`, 0.3 s wall-clock cooldown; node 10-30 px above; stuck; mortar/100lt "worm within 50 px" | air jumps if `canAirJump` |
| Jetpack | 3736-3743: the first loaded weapon with `Recoil < 0` replaces the rope | aims at the node |
| Dodging | **not implemented** (`#if 0` block 3496-3554); only mortar "move away after a shot" and the lag guard 2276-2292 | |
| Reloading | `AI_ReloadWeapons` whenever not shooting, and in `AI_Think`; rifles (Hard+) wait before engaging | only the current slot recharges in LX56 |
| Bonus seeking | `AI_Think`: health < 15 (before targets), empty slot → weapon bonus, health < 80 or < 100 (only without a target) | needs `Bonuses` on |
| Stuck | 3870-3910: 3 s within 5 px → stuck mode for 2 s; 5 s → previous node | |
| Idle | no enemy: random high spot (top fifth of the map) | |

### 1.9 Difficulty levels

`iAiDiffLevel` = `CLAMP(profile.nDifficulty, 0, 3)`. Without a profile it is
Medium (`4104-4107`). `setAiDiff` can change it at runtime (`4217-4224`).

| Parameter | Easy (0) | Medium (1) | Hard (2) | Xtreme (3) | Source |
|---|---|---|---|---|---|
| Aim offset added to the wanted angle, `GetRandomNum()·50·(3-L)/3`, shared, 0.5 s | ±50° | ±33.3° | ±16.7° | 0 | 1682-1694 |
| Aim tolerance to fire, `3 + 20·(3-L)/3` | 23° | 16.3° | 9.7° | 3° | 1706 |
| `weaponCanHit` target noise, `±50·(3-L)/3` px per axis | ±50 | ±33.3 | ±16.7 | 0 | 1836-1850 |
| (dead) gravity/speed noise in `weaponCanHit` | ±5/±20 | ±3.3/±13.3 | ±1.7/±6.7 | 0 | 1846-1847 |
| (dead) `iRandomSpread` max | 13° | 8° | 3° | 0 | 2211-2218 |
| Min time between weapon changes | 0.50 s | 0.30 s | 0.20 s | 0.12 s | 2342-2348 |
| Rifle game: reload before engaging | no | no | yes | yes | 3650 |
| (dead) reaction time | 0.45 | 0.35 | 0.25 | 0 | 2313-2317, commented out |

- The aim offset moves the point the bot aims at: an Easy bot fires when within
  23° of a point up to 50° off.
- The offset is a **function-static array shared by every bot of that level**,
  so all Easy bots miss the same way at the same time.
- Default CPU profiles: CPU 1 Easy, CPU 2 Medium, CPU 3 Medium, CPU 4 Hard,
  CPU 5 Xtreme, CPU 6 Medium, CPU 7 Easy (`ProfileSystem.cpp:55-59`).

### 1.10 Wall clock, threads and `rand()` in the bot

Wall clock (`tLX->currentTime`, constant within a frame and advanced by the
real clock between frames):
- It is read by `fLastThink`, `fLastFace`, `fLastShoot`, `fLastWeaponChange`,
  `fLastRandomChange`, `fLastGoBack`, `fLastCreated`, `fSearchStartTime` and
  `fStuckPause`, and by the worm's `fLastCarve` and `fSpawnTime` test.
- The static refresh times in `AI_SetAim` and `weaponCanHit` also use it.
- `GetTime()` (the true clock, not `currentTime`) is used by `AI_Jump` (`3356-3357`).
- `tLX->fDeltaTime` (fixed 10 ms inside the loop, while getInput runs at
  ~91 Hz) feeds the rope, stuck and bad-aim timers and the aim speed.

Threads: one pathfinding thread per bot (`searchpath_base()` → `threadPool->start`, `520`):
- It polls every 100 ms when idle (`712`) and sleeps 1 ms per area expanded (`623`).
- It reads the live map under a lock while the game runs, so the result and the
  moment it arrives are not reproducible.

`rand()` (the global gameplay stream, `src/common/MathLib.cpp`):

| Site | Draws |
|---|---|
| `AI_SetAim` aim offset refresh (shared, every 0.5 s) | 3 `GetRandomNum` |
| `weaponCanHit` noise refresh (shared, every 0.5 s) | 4 `GetRandomNum` |
| `AI_Shoot` spread (every projectile aim) | `GetRandomInt` + `GetRandomNum` |
| `AI_GetBestWeapon` random fallbacks | up to `slots` × `GetRandomInt` |
| `findRandomSpot` | 2 per try, up to 10 tries |
| `AI_SimpleMove` | 2 `GetRandomNum` |
| `GetRandomWeapons`, `getRandomEnabledWpn` (loadout) | 1 per slot / `GetRandomLong` |
| `subThink` `rand()%8`, `%15`, `%50` | **Gusanos mods only**: `CWormInputHandler::think` calls `subThink` only if `gusEngineUsed()` (`src/game/WormInputHandler.cpp:98-108`) |
| `do_some_tests_with_fastTraceLine` | `_AI_DEBUG` only |

So bots in OLX change the gameplay `rand()` sequence. A match with bots is not
reproducible from the seed alone.

### 1.11 Game-mode coverage

`startGame` (`1001-1018`) accepts DM, TDM, Tag, Hide and Seek, CTF, Race and
Team Race. The Deathmatch path is the generic `else` everywhere. For TDM it adds
team filtering in `findTarget` and `traceWeaponLine`, and the
"don't shoot mates" return in `AI_Shoot`.

### 1.12 Bugs and quirks worth knowing (none need porting)

- The ballistic angle is unused; the parabola check ignores `v` and `g` (1.6).
- The aim solver has the wrong sign for level targets and accepts unreachable
  targets (`fabs`) (1.6.2).
- `fSightDistance - fDistance > 50` compares squared distances
  (`1358`). The line-of-sight preference almost never applies.
- `d - fDist > 40` and `fDist <= 25` mix pixels with a 0..1 fraction (`2086`, `2102`).
- 100lt: the Cannon is used only while the rope is out (`isReleased`, `2422`, `2466`).
- `estimateYDiffAfterJump` uses the jump force as gravity (`3320`).
- The rope overshoot test uses a zero force vector (`3855-3866`).
- `simplifyPath`'s second loop is dead (`864-875`).
- `AI_FindShootingSpot`'s left fallback steps the wrong way (`4026`).
- `AI_SimpleMove` "looks up" by aiming down (`1750`).
- The open set drops areas whose priorities tie (`293-304`).
- Global `float fLastDirChange = 99999;` is unused (`1770`).

---

## 2. Weapon selection at game start

### 2.1 Who chooses

- **Server first** (`GameServer::PrepareWorm`, `src/server/CServer.cpp:403-430`):
  - `DisableWpnsAtStartup`: all slots empty.
  - A spectator, or `ForceRandomWeapons`: `GetRandomWeapons()`.
  - `SameWeaponsAsHostWorm`: clone the host's weapons.
  - Ready worms get `SendWeapons`.
- **Then the worm itself** (`CWorm::initWeaponSelection`, `src/common/CWorm.cpp:503-534`):
  - If `shouldDoOwnWeaponSelection()` (`CWorm.cpp:877-881`: the server does
    not choose, or this is the host worm with `SameWeaponsAsHostWorm`), it calls
    the handler's `initWeaponSelection`.
  - For a bot that is `CWormBotInputHandler::initWeaponSelection`
    (`CWormBot.cpp:4129-4192`). Its `doWeaponSelectionFrame` does nothing (`4194-4200`).

### 2.2 The bot's loadout (`CWormBot.cpp:4129-4192`)

`mod` is `FT_Mod`'s `ModInfo::name`, the script name: `Liero v1.0` for
Classic, `Modern Warfare1.0` for MW 1.0. `LT` is `FT_LoadingTime` (percent).

```text
random = true
if 15 < LT < 26 and mod contains "Classic" or "Liero v1.0":            // rifle "combo"
    if restrictions.isEnabled("Rifle"): all slots = Rifle; GAM_RIFLES; random = false
elif mod contains "Liero" or "Classic" and LT == 100:                  // 100lt
    if all of Super Shotgun, Napalm, Cannon, Doomsday, Chaingun are enabled and slots >= 5:
        slots = [Super Shotgun, Napalm, Cannon, Doomsday, Chaingun]; GAM_100LT; random = false
elif mod contains "MW 1.0" or "Modern Warfare1.0" and LT < 50:         // mortars
    if restrictions.isEnabled("Mortar Launcher"): all slots = Mortar Launcher; GAM_MORTARS; random = false
if random: GetRandomWeapons(); GAM_OTHER
for every slot still without a weapon: WeaponId = game.getRandomEnabledWpn()
bWeaponsReady = true
```

- `FindWeaponId` is the weapon's index by name.
- The rifle and mortar loadouts fill **every** slot with the same weapon.
- **With the defaults (Classic, LoadingTime 100, `Standard 100lt.wps`), every
  bot gets the 100lt loadout.** All five weapons are enabled in Classic's
  `Standard 100lt.wps` (2.5).
- MW 1.0 with defaults (LT 100) gets random weapons. Its `Standard 100lt.wps`
  bans the Mortar Launcher anyway.

### 2.3 `CWorm::GetRandomWeapons` (`src/common/CWorm.cpp:788-844`)

```text
for slot i in 0..slots:
    num = max(1, GetRandomInt(numWeapons - 1))    // never starts at weapon 0 (it can be reached by wrap-around)
    n = num; lastenabled = -1
    loop:
        if n >= numWeapons: n = 0
        selected = some slot k < i already holds weapon n (compared by ID)
        if restrictions missing or isEnabled(name(n)):
            if !selected: break
            lastenabled = n
        n += 1
        if n == num: n = lastenabled; break         // full loop: accept a duplicate, or -1
    slot[i].WeaponId = n if valid else -1
```

- It draws one `rand()` per slot.
- Weapons are distinct while enough are enabled. After that it repeats the
  **last** enabled duplicate seen in the scan.
- It gives -1 (an empty slot) if nothing is enabled. The bot then fills such
  slots with `getRandomEnabledWpn`, which returns -1 again if nothing is enabled.
- `Game::getRandomEnabledWpn` (`src/game/Game.cpp:1405-1420`):
  `randomChoiceFrom(enabled ids)` = `GetRandomLong(n - 1)` (`include/Utils.h:38-43`).

### 2.4 The restriction file (`.wps`) and `CWpnRest` (`src/common/CWpnRest.cpp`, `include/CWpnRest.h`)

- **Setting:** `WeaponRestrictionsFile` (`FeatureList.cpp:41`), default
  `"Standard 100lt"`. `Game::loadWeaponRestrictions`
  (`src/game/Game_loadWeaponRestrictions.cpp:16-28`) loads it on the server,
  then `updateList(mod weapons)`.
- **Path resolution** (`loadList`, `CWpnRest.cpp:183-213`):
  - A name starting with `cfg/` is used as is.
  - Otherwise `.wps` is appended unless the name already ends with it.
  - The file is then `cfg/presets/<mod dir>/<file>` if it exists, else `cfg/presets/<file>`.
  - `<mod dir>` is the mod's directory (`Classic`, `MW 1.0`).
  - A missing file only logs a warning; the list then stays empty.
- **Format:** text, one `Name,State` per line, read with `ReadUntil('\n')`, then
  `explode(",")`. Lines with fewer than 2 fields are ignored.
  - `State` is an int taken `MOD 3`: `0` enabled, `1` bonus only, `2` banned (`CWpnRest.h:28-32`).
  - Names are compared **case-insensitively** (`std::map` keyed by `WeaponName`,
    `stringcasecmp`). They are stored **as written** (`addWeapon` does not
    trim), but `findWeapon` trims the query (`TrimSpaces`, C `isspace`), so
    a file name with surrounding spaces can never be found, and a mod weapon
    whose name has them is banned (`updateList` stores it untrimmed). No
    bundled file has such names. (Corrected in plan 6 task 3: names are not
    trimmed on load.)
  - Duplicates: the last line wins (`addWeapon`, `146-152`).
  - Some files have CRLF line endings (Classic `Firebird`, `Retro*`, `Standard*`;
    MW `Standard 100lt`). The `\r` stays on the state field, and
    `from_string<int>` (`src/util/StringConv.h:44-48`, an `istringstream >> int`)
    stops at it, so `"2\r"` gives 2. A state without digits gives 0 (enabled),
    and overflow the nearest `int` limit. The port (`rl_import_lgs::parse_wps`)
    reads the state the same way and keeps names as written.
  - The file name: `.wps` is appended only to a name **longer than 4 bytes**
    that lacks it, and a name starting with `cfg/` is a path (`rl-data`'s
    `WeaponRules` finds no imported preset for it: all enabled).
  - The files list weapons of **many** mods (Classic `Standard 100lt.wps` has
    215 lines). Entries for other mods are harmless.
- **Missing weapons:** `updateList` adds every mod weapon missing from the file
  as **enabled** (`59-75`).
- **Queries:**
  - `isEnabled(name)`: state == enabled. A name missing from the list counts as
    **banned** (`216-226`). After `updateList`, every mod weapon is listed.
  - `isBonus` is analogous.
  - Weapon bonuses spawn only weapons whose state is not `banned`
    (`damage-death-bonuses.md` §4.1).
- **Network:** clients reset to enabled and apply the server's non-enabled list
  (`sendList`/`readList`, `301-349`). The local client shares the server's object.

### 2.5 What the bundled presets enable (computed against each mod's weapon list)

`share/gamedir/cfg/presets/<mod>/*.wps`. Weapons in script order are
`content/olx-manifest.ron` → `target/content/olx-*/mod.ron`. Nothing is
"bonus only" except in `Bonus only.wps` (MW's `Standard 100lt.wps` has 62
bonus-only lines, all for other mods' weapons). Plan 6 task 3 checks these
tables against the imported presets (`wps_presets_enable_what_olx_enables`).

**Classic (38 weapons):**

| File | Enabled (state 0) | Banned |
|---|---|---|
| **`Standard 100lt.wps` (default)** | Blaster, Cannon, Chaingun, Dart, Doomsday, Handgun, Mini Rockets, Napalm, Shotgun, Super Shotgun, Uzi, Winchester (12) | Bazooka, Big Nuke, Booby Trap, Bouncy Larpa, Bouncy Mine, Chiquita Bomb, Cluster bomb, Crackler, Dirt Ball, Explosives, Flamer, Float Mine, Gauss Gun, Greenball, Grasshopper, Grenade, Hellraider, Larpa, Laser, Mine, Minigun, Mini Nuke, RB Rampage, Rifle, Spikeballs, Zimm (26) |
| `Standard 20lt.wps` | Gauss Gun, Rifle | the other 36 |
| `Restricted 100lt.wps` | Cannon, Chaingun, Doomsday, Handgun, Napalm, Super Shotgun | the other 32 |
| `Rifle only.wps` | Rifle | the other 37 |
| `Retro Combo.wps` | Gauss Gun, Rifle | the other 36 |
| `Retro Combo Unlimited.wps` | Bazooka, Gauss Gun, Mini Nuke, Napalm, Rifle, Spikeballs, Zimm | the other 31 |
| `Retro Classic.wps` | Bazooka, Blaster, Cannon, Chiquita Bomb, Dart, Doomsday, Explosives, Grasshopper, Grenade, Mini Rockets, Napalm, Shotgun, Super Shotgun, Uzi, Winchester | the other 23 |
| `Firebird.wps` | Bazooka, Cannon, Chaingun, Dart, Doomsday, Flamer, Grasshopper, Handgun, Shotgun, Super Shotgun, Uzi | the other 27 |
| `Bonus only.wps` | none (all 38 bonus only) | none |

**MW 1.0 (50 weapons):**

| File | Enabled | Banned |
|---|---|---|
| **`Standard 100lt.wps` (default)** | Cluster Grenade, DoomRockets, Electro Gun, Flamer, Poison Rifle, Shockrifle, Twin Rockets, Twin Shotgun, Toxic Waste, Volcanic Gun, Hellfire, Napalm Grenade, Plasma Launcher, Rocket Launcher, Ripper (15) | Acid Gun, Anti-Air Gun, Apache, Assassin's Shot, Bazooka, Energy Ball, Fireball, Flak, Flak Grenade, Sunray Beamer, Floating Bomber, Medical Camp, NW Bomb, Inferno, Jet Pack, Laser, **Mortar Launcher**, Nuclear Bomb, Nuclear Mines, Nuclear Missile, Nuclear Waste, Petrol Bomb, ShockStrike, Smoking Bomb, Spikeballs, Sunray Grenade, Surface Sweeper, Trap Mines, Toxic Bomb, Toxic Strike, Disease Bomb, Disease Grenade, Infest, Force Field, Demolition Grenade (35) |
| `Mortar Only.wps` | Mortar Launcher | the other 49 |

(The script that produced this lives in the research scratchpad. It parses the
files with last-line-wins and case-insensitive names, and treats mod weapons
missing from a file as enabled.)

---

## 3. Deathmatch

The mechanics are in `damage-death-bonuses.md`: kills (§2.3-2.5), score helpers
(§2.8), respawn (§3), bonuses (§4), the Deathmatch rules (§5) and the settings
table (§7). This section collects what a `rl-modes` Deathmatch needs, and adds
the game-over flow, messages and results screen.

### 3.1 Settings (defaults from `src/common/FeatureList.cpp`)

The default preset is `Standard.gamesettings` (`src/game/SettingsPreset.cpp:75-78`), which sets
nothing, so the FeatureList defaults apply. `LieroX 0.56.gamesettings` changes
only physics and aim options. None of them are Deathmatch rules.

| Setting (line) | Default | Meaning |
|---|---|---|
| `Lives` (31) | −2 | < 0: unlimited (`WRM_UNLIM` −2). `N ≥ 0`: the worm can die N+1 times; then `WRM_OUT` (−1). Prepare sets `Lives < 0 ? WRM_UNLIM : Lives` (`Game.cpp:851`); `Game.cpp:490-491` normalises other negatives to −2 |
| `KillLimit` (32) | 15 | > 0: game over when any worm's **score** reaches it |
| `TimeLimit` (33) | 6.0 min | > 0: game over when `serverTime > TimeLimit·60 s` (`CGameMode.cpp:31-33`, `305-314`) |
| `LoadingTime` (35) | 100 % | weapon recharge; also picks the bot loadout (2.2) |
| `LevelName` (37) / `ModName` (39) | `Dirt Level.lxl` / `Classic` | |
| `GameType` (38) | Death Match | |
| `WeaponRestrictionsFile` (41) | `Standard 100lt` | 2.4 |
| `ForceRandomWeapons` (42) | false | server gives `GetRandomWeapons` to everyone |
| `SameWeaponsAsHostWorm` (43) | false | |
| `Bonuses` (50), `BonusFrequency` (52), `BonusLife` (53), `BonusHealthToWeaponChance` (54) | false, 30 s, 60 s, 0.5 | |
| `RespawnTime` (56) / `MaxRespawnTime` (57) | 2.5 s / 0 | with `MaxRespawnTime >= 0`, the server respawns after `RespawnTime` without input |
| `EmptyWeaponsOnRespawn` (59) | false | |
| `SuicideDecreasesScore` (110), `TeamkillDecreasesScore` (112), `DeathDecreasesScore` (114), `CountTeamkills` (116), `AllowNegativeScore` (118) | false, false, 0.0, false, false | scoring (3.2) |
| `SelfInjure` (125), `SelfHit` (127) | true, true | |
| `AllowEmptyGames` (130) | false | only for network games with unlimited lives |
| `FillWithBotsTo` (151) | 0 | |
| `AllowWeaponsChange` (168) | true | |
| `ImmediateStart` (170) | true | |
| `WeaponSlotsNum` (172) | 5 | |
| `DisableWpnsWhenEmpty` (173), `DisableWpnsAtStartup` (175) | false | |
| `TeamScoreLimit` (205) | −1 | teams only |

Non-feature options (`src/client/Options.cpp`):
- Killing-spree thresholds: 3/5/7/9/10 (`243-247`).
- Dying-spree thresholds: 3/5/7/9/10 (`249-253`).
- Network texts (`787-820`):
  - `"<killer> killed <victim> with <weapon>"`
  - `"<player> commited suicide"` (sic)
  - `"<player> drew first blood"`
  - `"<player> is out of the game"`
  - `"<player> has won the match"`
  - `"Timelimit has been reached"`
  - sprees: `"is on a killing spree!"`, `"is on a rampage!"`, `"is dominating!"`,
    `"is unstoppable!"`, `"is GODLIKE!"`
  - dying sprees: `"is on a dying spree!"`, `"is a target!"`, `"is a sitting duck!"`,
    `"is free kills!"`, `"is WORTHLESS!"`

### 3.2 Scoring

`CDeathMatch` (`src/server/CDeathMatch.cpp:19-26`) is the bare `CGameMode`. Per kill,
`CGameMode::Kill(victim, killer)` (`src/server/CGameMode.cpp:69-252`):

```text
if killer:
    if killer == victim:                 killer.addSuicide()     // message "commited suicide"
    elif team game and same team:        killer.addTeamkill()    // never in DM
    else:                                killer.addKill()        // iKills++; "killed … with <weapon>"
                                                                 // ("with <weapon>" is dropped if unknown)
(killer null: no attribution, no message)
first blood: the first kill with killer != victim
kills-in-row (killer) / deaths-in-row (victim) → spree messages and sounds
victim.Kill(true): bAlive = false; addDeath(); lives -= 1 unless unlimited     (CWorm.cpp:1399-1413)
if victim.lives == WRM_OUT: "is out of the game"
lead change sounds; "N frags left" (bug: looks up the leader by team id, 236)
```

```text
score = kills
      - (int)(deaths · DeathDecreasesScore + 0.01)   if DeathDecreasesScore > 0
      - suicides                                      if SuicideDecreasesScore
      - teamkills                                     if TeamkillDecreasesScore
      + teamkills                                     if CountTeamkills
// CWorm::getScore, CWorm.cpp:1415-1428
```

- `addDeath`, `addSuicide` and `addTeamkill` do **nothing** while
  `!AllowNegativeScore && score <= 0` (`CWorm.cpp:1430-1449`).
- With the defaults, `score == kills`, and suicides cost nothing.
- `fDamage` (the damage score) only breaks ties. OLX counts it about twice
  (`damage-death-bonuses.md` §2.5).

### 3.3 Game over and winner

**Check order** (`CGameMode::CheckGameOver`, `CGameMode.cpp:276-360`). It is
evaluated every server frame while playing: `SimulateGame` checks only while
`recheckGame` is set (`CServer_Game.cpp:169-176`), but `BeginMatch` sets it
(`CServer.cpp:521`) and only `GameServer::Clear` resets it (`CServer.cpp:80`),
so during a match it stays set, and a kill reaching the limit ends the match
in its own frame (the local client's kill calls `killWorm` directly,
`CClient_Game.cpp:419`):
1. Team score limit (teams only).
2. Kill limit: `KillLimit > 0` and some worm (id order) has `getScore() >= KillLimit`.
   Message: `"Kill limit 15 hit by worm <name>"`.
3. Time limit: `TimeLimit() > 0` and `serverTime() > TimeLimit()`. It plays
   `timeoutcalled`; message: `"Timelimit has been reached"`.
4. Unless `allowEmptyGames`:
   - The game is over when fewer than 2 worms have `lives != WRM_OUT`.
   - In a local game with a single worm, the threshold is fewer than 1.
   - `allowEmptyGames` (`316-320`) needs `AllowEmptyGames`, connecting during
     the game allowed, **not a local game**, and unlimited lives. So for local
     play it is always false.
5. Teams: only one team with live worms left.

**Time-remaining sounds:** `CGameMode::Simulate` (`362-386`) plays 5..1 s,
1 min and 5 min.

**Winner** (`Winner`, `388-396`; `HighestScoredWorm`, `398-407`;
`CompareWormsScore`, `409-434`):
```text
if count(lives != OUT) < 2 and one exists: winner = the first such worm (id order)
else: winner = the max over worms (id order) of the comparator; only a strictly better worm replaces the leader:
    if Lives setting >= 0:
        a worm with score >= KillLimit (if > 0) is best
        more lives wins
    not OUT beats OUT
    higher score wins
    Round(damage1 - damage2), Round = (int)ceil(x - 0.5) (MathLib.cpp:85-88):
    0 for a difference in (-0.5, 0.5] → tie keeps the earlier worm
```
With limited lives, a worm at the kill limit ranks first whichever side of
the comparison it is on, so when two worms reach the limit together the
**later** one (by id) replaces the leader and wins; with unlimited lives the
tie goes to the earlier worm.
`getAliveWormCount`/`getFirstAliveWorm` count **not OUT**, not `bAlive`
(`CServer.cpp:671-690`). A dead worm waiting to respawn still counts.

**`GameServer::GameOver`** (`src/server/CServer.cpp:532-625`):
```text
if gameOver: return
gameOver = true; gameOverFrame = serverFrame
winner = Winner(); message "<player> has won the match"
(winnerTeam for team games)
send S2C_GAMEOVER (winner id; team data for team games) to every client
for each worm: clearInput(); addTotalWins() if winner else addTotalLosses()   // profile stats
gameMode.GameOver()                                  // DM: nothing
```

After game over:
- **Worms:** `simulateWorm` skips `getInput` (`PhysicsLX56.cpp:300`), and the
  client loop skips pickups, shooting and material damage. Physics and
  projectiles keep running until `GAMEOVER_WAIT` = 3 s. Kills are ignored
  (`killWorm` returns, `damage-death-bonuses.md` §2.3), and `SpawnWorm` returns.
- **Client side:** `ParseGameOver` (`src/client/CClient_Parse.cpp:1382-1460`)
  sets `iMatchWinner` and `gameOver`, and **clears all client projectiles** (`1451`).
- **Results screen:** after `GAMEOVER_WAIT`, `CClient::InitializeGameMenu`
  (`src/client/CClient_Draw.cpp:893-905`, `1580-1667`) shows the "game over" scoreboard.
- **Dedicated server:** returns to the lobby after `LX_ENDWAIT` = 9 s (non-local games).
- **Local game:** an OK button.

### 3.4 Results screen data (`CClient::UpdateScore`, `CClient_Draw.cpp:1851-1966`)

- **Header:** "Winner:" with the winner's name and skin
  (`"noone"` if there is no winner).
- **Rows:** one per worm, sorted by `ScoreCompare` (`1840-1846`), i.e. the mode's
  `CompareWormsScore` from 3.3, descending.
  - Rows 0-13 go in the left list, 14+ in the right one.
- **Columns (GMT_NORMAL):** command button, skin, name, lives (∞ icon for
  unlimited, `"out"`, or the number), **kills (`getKills()`, not the score)**,
  damage (`Round(fDamage)`), ping (network host only).
- Local game: an OK button. Network: Leave, and "Returning to Lobby in N" with
  `N = 5 + GAMEOVER_WAIT - elapsed`.

What `rl-modes` should emit as `GameOver` (spec §5.7): the winner id (or none),
the reason (kill limit, time limit, last worm standing), and per worm the
lives, kills, score, deaths, suicides and damage, in comparator order.

---

## 4. Recommendations for `rl-bots`

### 4.1 Crate shape

- A new crate `rl-bots`, registered in `xtask/layers.ron` at layer 3 with the
  same-layer pair `rl-bots → rl-sim` (plan 6; this note first said layer 4). Make it `#![no_std]` + `alloc` and build it
  for `wasm32`: bots must run in browser clients too (spec §12.2).
  It depends on `rl-sim`, `rl-terrain`, `rl-math` and `rl-data`.
- API sketch:
  ```rust
  pub struct BotConfig {               // H3 (§12.1): data, not code
      pub personality: Personality,    // Deathmatch | Chaos
      pub difficulty: Difficulty,      // Easy | Medium | Hard | Xtreme -> DifficultyParams
      pub params: DifficultyParams,    // overridable numbers from 1.9
      pub aim: AimModel,               // OlxDirect | Ballistic (4.2)
      pub budget: u32,                 // work units per tick (4.3)
  }
  pub struct Bot { /* brain, nav, motor, rng */ }
  impl Bot {
      pub fn new(worm: usize, cfg: BotConfig, seed: u64, world: &World) -> Self;
      /// Called once per tick before `World::tick`, with the last tick's events.
      pub fn think(&mut self, world: &World, events: &TickEvents) -> Keys;
  }
  pub fn choose_loadout(cfg: &BotConfig, ctx: &MatchSetup, rng: &mut BotRng) -> Loadout;
  ```
- **Replays need no bots:** the replay stores every worm's `Keys`, so a change
  in bot code cannot break recorded matches. Bot matches (§7.1) stay
  reproducible because the bot is a pure function of (seed, world history).

### 4.2 What to port closely, what to simplify

| Part | Port closely | Simplify or fix |
|---|---|---|
| State machine | THINK ↔ MOVINGTOTARGET; "shoot first, freeze movement while aiming"; re-think every 3 s, every 0.5 s while a path is pending, at once on target death; 0.4 s idle after spawn | — |
| Target choice | the nearest enemy (DM), as OLX effectively does | drop the broken line-of-sight preference, or fix it (compare real distances) behind a flag |
| Bonus seeking | the `AI_Think` priorities (health < 15 before targets; < 80 and idle high spot without a target) | — |
| Pathfinding | rock-only maximal rectangles, a 7 px passage rule, the 5 px checklist, centre→connector→centre costs, a worm-line goal test, shortcutting, 50 px splitting | precompute or cache areas (rock is static, 4.3); a heap with tie-breaks; drop the dead second simplify loop |
| Path following | node skipping by worm line, go-back once per second, stuck handling (3 s / 5 s), rope timers (5 s, 2 s, re-shoot 0.5 s), best rope spot scoring, clearing weapon for dirt above, jetpack use | limit visibility checks to the next K nodes; fix the rope-spot direction term (`hit/|hit|`) |
| Aim | `AI_SetAim` turning model (face at most every 0.3 s, ~91-100°/s, snap within 3°), the difficulty noise and tolerance (1.9); lead with `0.7 · dist/v` | **`AimModel::Ballistic`** (default): aim at the solver's angle (textbook low root, correct sign, reject when `D < 0`). `AimModel::OlxDirect` keeps OLX's direct aim for parity tests. Give each bot its **own** noise stream, not a shared static |
| Fire decision | `canShootRightNowWithCurWeapon`; < 10 px don't shoot; 10-50 px shoot when aimed; beams need range and line of sight; fast or straight weapons need line of sight; slow lobbed weapons need a clear trajectory | check the real trajectory (step the ballistic arc through the terrain flags) instead of OLX's parabola |
| Weapon choice | the OLX buckets (bottom of map → Explode; < 100 / < 150 direct → beam, then non-dirt projectile; direct → projectile, beam, random; indirect far below → Explode/Bounce; fallbacks); the change delay per level; "reload the lowest empty slot when you can't shoot" | within a bucket, optionally score by expected damage, speed and splash safety (self-injury) instead of slot order; keep slot order as `Faithful` |
| Preset loadouts | 2.2 exactly (rifle, 100lt, mortar detection by mod name, LoadingTime and restrictions); `GetRandomWeapons` exactly (with the bot's or the setup's RNG) | — |
| 100lt / rifle / mortar tactics | port (they define how default Classic bots fight) | fix the Cannon `isReleased` bug |
| Dodging | none in OLX | optional later; a "flee from a fast, damaging projectile heading at me" rule behind config |

### 4.3 Determinism and per-tick work

- **RNG:** each bot owns a small PRNG (e.g. PCG32 or SplitMix64) seeded from
  `(match_seed, worm_index, personality)`. It **never** touches the world's
  `rand()`. Every OLX `rand()` site in 1.10 becomes a bot RNG draw.
- **Clock:** use `World::time()` (the frame clock) for every OLX
  `currentTime`/`GetTime()` comparison. Use a 10 ms tick for `fDeltaTime`
  timers. The bot runs once per 100 Hz tick, not per 11 ms sub-step; scale
  rates (aim speed) by 10 ms.
- **No iteration over hash maps.** Areas live in a `Vec` with stable indices;
  ties break on index.
- **Work units, not time:**
  - Count one unit per terrain flag probe (rectangle growth, checklist traces,
    `traceWormLine`, `traceWeaponLine`, rope rays, trajectory steps).
  - Each `think` gets `cfg.budget` units, e.g. 8192 (tune with the §7.1 p99 tick-time check).
  - Pathfinding is a resumable state machine: the open list, the area being
    grown with its cursor, and the expansion cursor. It continues across ticks
    until done, like OLX's thread but reproducible.
  - When the budget runs out, the bot keeps following the old path or uses `AI_SimpleMove`.
  - Cap the areas per search (e.g. 1024), the path nodes (e.g. 256) and the
    rope rays per tick. Fail cleanly when a cap is hit: never panic (rule 6).
- **Static rock lets you cache:**
  - LX56 never creates or removes rock: `CarveHole` only removes dirt, and
    `PlaceDirt` only fills empty pixels.
  - So rectangles found by `getMaxFreeArea(PX_ROCK)` stay valid all match. Keep
    them in a per-level cache shared by all bots (built lazily under the budget,
    or eagerly at match setup outside `tick`). A search then mostly re-uses areas.
  - **Only the expansion order and the goal test depend on start and target.**
  - Bots still need `TerrainEdit` events for dirt-aware tests (line of sight,
    "dirt above me"). Those read the live `World::terrain()`, so they need no cache.
- **Bounded per-tick work otherwise:**
  - `findTarget` is O(worms) traces.
  - Visibility to later path nodes: at most K = 8 nodes ahead per tick.
  - Rope spot: 20 rays at most every N = 5 ticks while the rope is wanted.

### 4.4 The motor: intents to `Keys`

The brain produces an `Intent`, for example:
- an aim target angle and a wanted face side;
- move, jump, fire, rope shoot or release, carve;
- a wanted slot.

The motor turns it into the 8 key bits under `rl-sim`'s rules
(`crates/rl-sim/src/physics.rs:36-132`, `input.rs` latches):

| OLX bot action | Keys | Caveats |
|---|---|---|
| set `fAngle` (≈100°/s) | UP/DOWN. LX56 aim: accel 500°/s² while held, **no cap while held**; on release clamp to ±100, −200°/s², reset below 5 | a controller: press towards the target while the stopping distance `ω²/400` is shorter than the error, and release when `|ω| >= 100` to match OLX's ~100°/s turning |
| set face side | LEFT or RIGHT | there is no turn without walking: tap one tick (one sub-step of movement); OLX's 0.3 s face cooldown limits this |
| `bMove` | hold LEFT/RIGHT (face = move direction) | CHANGE held disables movement |
| `bJump` | JUMP press edge | press, then release for at least one tick before the next jump; a jump also **releases the rope** if it is out |
| `bShoot` | hold FIRE | |
| `iCurrentWeapon = k` | hold CHANGE + tap RIGHT/LEFT (one slot per press edge, wrapping) | the shortest cyclic direction; no movement while CHANGE is held; with `WeaponCombos` off, not while firing; takes about 2 ticks per slot |
| `bCarve` | hold the move direction + tap the opposite | only while moving; OLX carves even when standing (accepted deviation) |
| `rope.Shoot(faceDir)` | ROPE press edge | fires along the aim at the start of the read; aim first |
| `rope.Clear()` | JUMP press edge (rope out) | also jumps when on ground |
| respawn request | JUMP press edge while dead | the default `MaxRespawnTime` 0 respawns anyway |

`KeyLatch` keeps a press until the next read, so a one-tick tap is never lost
when a frame runs no sub-step. Two taps between reads count as one.

### 4.5 Difficulty mapping (keep OLX's numbers)

`DifficultyParams { aim_offset_max_deg: [50, 33.3, 16.7, 0], fire_tolerance_deg: [23, 16.3, 9.7, 3],
target_noise_px: [50, 33.3, 16.7, 0], noise_refresh_s: 0.5, weapon_change_delay_s: [0.5, 0.3, 0.2, 0.12],
rifle_reload_before_engage: [false, false, true, true] }`.

One possible addition, kept off by default: OLX's commented-out reaction time
(0.45/0.35/0.25/0 s).

### 4.6 The "chaos" personality (spec §5.9, coverage §7.1)

Goal: over a match grid, fire every weapon of both mods and trigger every
projectile action type the mods use.

- **Loadout:**
  - Ignore restrictions, so banned weapons get fired too.
  - Chaos bot `i` takes weapons `[(i·5 + r·5·n_chaos + j) mod W for j in 0..5]`
    on its `r`-th life.
  - Loadouts change on respawn through the game mode's command interface
    ("give a weapon", spec §5.8 H1). `World::set_loadout` is setup-only, so it
    cannot be used mid-match.
  - Over a few lives, every weapon id cycles through.
- **Firing:**
  - Always fire the current slot when it is ready, at the nearest enemy if one is
    visible, else at a random angle (bot RNG). Specials too (jetpack).
  - Switch to the next slot (CHANGE+RIGHT) when the current one starts reloading.
    The previous slot does not recharge while unselected, so return to it in rotation.
  - Ignore self-safety rules (no `SelfInjure` filter, no "too close" check).
- **Movement:** a random walk with jumps, rope throws at random angles and digging
  (carve combo), to cover rope attachment, digging and air-jump events.
- **Coverage bookkeeping:**
  - The bot tracks which weapon ids it has fired, from `WeaponFired` events.
  - It prefers unfired weapons when the mode allows choosing.
  - `rl-cli match` reports the union across bots.

### 4.7 Loadout at match start (`choose_loadout`)

1. If the settings force random weapons, or a human chooses, follow the mode.
2. Bot (Deathmatch personality): apply 2.2 literally. With the defaults
   (Classic, `LoadingTime` 100, `Standard 100lt`) that gives the fixed 100lt set
   and the 100lt tactics.
3. Otherwise run `GetRandomWeapons` (2.3) over the mod's weapon list with the
   imported restrictions.
   - This needs the `.wps` import: `STATUS.md` notes that restriction files are
     not imported yet.
   - Store them per mod in the pack, as the computed enabled sets in 2.5, or as
     raw `name → state` with case-insensitive lookup.

### 4.8 Tests

- **Determinism:** two bot matches with the same seed give identical key and
  checksum streams; a different seed gives different streams.
- **Budget:** a test hook counts units per `think` and asserts `<= budget`.
- **Robustness:** `think` never panics on fuzzed worlds: tiny levels, all-rock
  levels, worms off-map, empty slots, mods without projectiles (rule 6).
- **Behaviour smoke:** on `Dirt Level`, an Xtreme bot kills a stationary target
  within N seconds, and an Easy bot hits measurably less often.
- **Pathfinding unit tests:** synthetic terrains (a corridor of width 6 vs 7, a
  U-shape, dirt plugs) check `getMaxFreeArea` growth order and the 7 px passage rule.

---

## 5. Open questions

1. **Faithful loadout or variety?** Should Deathmatch bots on default Classic
   settings really all use the 100lt set, as OLX's do? It is faithful and
   defines the "OLX bot feel", but it is monotonous. A config flag
   (`loadout: OlxPreset | Random`) defaulting to `OlxPreset` seems right.
2. **Aim model default.** `Ballistic` (better bots, uses the solver as the spec
   asks) or `OlxDirect` (faithful, bad at lobbing)? `Ballistic` is proposed, with
   OLX's noise and tolerance keeping difficulty feel.
3. **Turning without walking.** `Keys` cannot change the facing side without at
   least one sub-step of movement. Is that acceptable, or should `PlayerInput`
   get spec §5.6's "optional mouse-aim target" for bots? That would make bots
   unlike keyboard humans.
4. **Restriction import format.** Are raw `.wps` files imported into the pack
   (with OLX's lookup rules), or only resolved enabled-id sets per preset? This
   also decides how `World::set_bonus_weapons` is fed.
5. **Game over for the remake's local play:** `allowEmptyGames` is false in local
   games, so a 1-human + bots match with unlimited lives ends only on the kill or
   time limit. Should a match with one human and no bots (practice) end at once
   when only one worm exists? OLX lowers the threshold to 1 only for a single-worm local game.
6. **`AI_FindShootingSpot` `traceLine(...) >= 40`:** does float truncation make
   the circle search always fail, so the fallback runs? Worth a quick OLX test.
   It only matters for parity, not for the remake design.
7. **Does the local client get `S2C_GAMEOVER`** in the oracle setup, and so clear
   its projectiles at game over (`ParseGameOver`)? `GameOver` sends through
   `SendPacket` directly, unlike the `SendWormDied`-style helpers that skip the
   local client. This matters if game over gets an oracle scenario.
8. **The damage tie-breaker:** OLX double counts `fDamage`
   (`damage-death-bonuses.md` §2.5). The remake keeps a single count (STATUS).
   Rare time-limit ties could resolve differently; record it as a deviation.
9. **Pause and `serverTime`:** the time limit uses `serverFrame · 10 ms`, which
    stops while paused. Confirm the remake's mode uses the frame counter, not `World::time`.
