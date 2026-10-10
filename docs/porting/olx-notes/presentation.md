# OLX presentation: rendering, camera, cosmetics, sound, input

A porting reference for everything OLX shows and plays but does not simulate:
the camera, map colours, worms, rope, projectiles, bonuses, cosmetic entities,
sounds, the HUD and input defaults. Every citation is OpenLieroX at commit
`d1ad953f`. Paths are relative to the OLX repository, and assets are relative to
`share/gamedir/`.

This commit is a fork of OLX 0.59. It has a widescreen local game, split screen
for up to 4 players, a transparent top bar, touch and gamepad input. Where the
fork changed classic behaviour, this note says so.

Conventions used below:

- **World px** is a material (physics) pixel.
- **Screen px** is a pixel of OLX's 640×480 (or wider) video surface. Gameplay
  is drawn at **2 screen px per world px**: `physicToReal` is
  `(p − World) * 2 + Left` (`include/CViewport.h:166-178`). The map's colour
  images are 2× surfaces (`src/common/CMap.cpp:460-470`).
- Most sprites are authored at the 2× resolution and blitted 1:1 onto the
  screen, so they have half-world-pixel detail: skins, projectile images,
  crosshair, hook, explosion, smoke, spawn, sparkle, gibs, bonus and health
  icons. Only the muzzle flash (and the gibs' map stamps) are 1× art stretched
  2× (§3.6, §7).
- **Sub-step** is one LX56 physics step of `LX56PhysicsDT` = 11 ms (84 FPS
  rounded to whole ms, `include/PhysicsLX56.h:19-21`,
  `src/common/PhysicsLX56.cpp:47-59`). **Tick** is one 10 ms game frame
  (`src/game/Game.h:104-105`).

---

## 1. Camera and viewport (`CViewport`)

### 1.1 Geometry

`CViewport::Setup(l, t, vw, vh, type)` sets `VirtWidth = vw`, `VirtHeight = vh`
(screen px) and `Width = vw/2`, `Height = vh/2` (world px)
(`src/client/CViewport.cpp:63-85`).

`CClient::SetupViewports` (`src/client/CClient.cpp:1527-1551`, `1685-1744`)
lays the viewports out as follows:

- **Top and height.** The top bar is treated as transparent: `top = 0`,
  `h = 480` (`:1591-1595`, `:1712-1716`).
- **Width.** `sw = displayScreenWidth()`. For a local game that is the full
  screen width `round_even(480 * desktop_w / desktop_h)`: 640 for 4:3, 768 for
  16:10, 854 for 16:9 (`src/client/AuxLib.cpp:450-469`,
  `src/game/Game.h:73`). A network game gets a fixed 640
  (`include/AuxLib.h:148`).
- **One player** gets the whole screen: `Width = sw/2`, `Height = 240` world px.
  At 640×480 that is **320×240 world px**. This matches the remake's 240 px
  field of view.
- **Two players** get side-by-side halves with a 4 px gap: `vw = (sw−4)/2`, so
  159 world px wide at 640, by 240 high.
- **Three or four players** get a 2×2 grid of `(sw−4)/2 × (480−4)/2` screen px,
  so 119 world px high (`splitScreenViewportRect`,
  `src/client/CClient.cpp:1653-1677`). With three players the fourth quarter
  holds the radar map.
- The gaps are filled with `clViewportSplit` (`#000000`,
  `data/frontend/colours.cfg`) after the viewports are drawn
  (`src/client/CClient_Draw.cpp:561-609`).
- Each viewport is cleared to black before the map is drawn
  (`CClient_Draw.cpp:1001-1015`). Any area outside the map is black, and there
  is no border art.

The final surface is scaled to the window with
`SDL_HINT_RENDER_SCALE_QUALITY = "best"`, which gives smoothed (linear)
upscaling (`src/client/AuxLib.cpp:602-603`).

### 1.2 Following the worm (`Process`, `CViewport.cpp:118-367`)

`Process` runs **once per drawn frame** (`CClient_Draw.cpp:553-557`), not per
tick. `hx = Width/2`, `hy = Height/2` (integer division).

`VW_FOLLOW` is the type for every local player (`:130-177`):

```
if smooth:                                    // spectating a worm we don't own
    setSmoothPosition(worm.pos.x - hx, worm.pos.y - hy, frameDt)
else:                                         // our own worm: a hard lock
    WorldX = (int)(worm.pos.x - hx)           // truncation, no lag, no lead
    WorldY = (int)(worm.pos.y - hy)
```

- The worm's position is used **even when it is dead** (`:160-162`).
- The smoothing flag is `setSmooth(!OwnsWorm(id))`, so it is off for local
  worms (`CClient.cpp:1735-1739`).
- There is no lead or look-ahead.

**Smoothed follow** (`setSmoothPosition`, `:488-519`) is used only when
spectating. The constants are `VelMax 2000`, `AccelMax 600`, `VelInc 100`,
`AccelInc 200`, `VelDecay 20`, `AccelDecay 50`. Steps longer than 15 ms are
split.

```
acc += (target - cur) * 200*dt;  acc -= acc*50*dt;  clamp |acc| <= 600
vel += acc * 100*dt;             vel -= vel*20*dt;  clamp |vel| <= 2000
cur += vel*dt;  WorldX = (int)cur.x;  WorldY = (int)cur.y
```

`setSmooth()` resets `cur` to the current `WorldX`/`WorldY` and zeroes the
velocity and acceleration (`:474-479`).

### 1.3 Clamping to the map (`Clamp`, `:417-431`)

```
if MapW >= Width:  WorldX = clamp(WorldX, 0, MapW - Width)  else WorldX = 0
if MapH >= Height: WorldY = clamp(WorldY, 0, MapH - Height) else WorldY = 0
```

Clamping is skipped for `InfiniteMap`. It is also skipped when the option
`Game.AlwaysCenterWorm` (default `false`, `src/client/Options.cpp:223`) or touch
controls are active (`:321-334`, `:360-362`). A map smaller than the view sits
at the top-left, with black around it.

### 1.4 Screen shake (`Shake`, `:453-461`; `Process`, `:336-358`)

`CClient::Explosion(.., shake, ..)` calls `Shake(shake)`, for a nonzero
`shake`, on every used viewport whose world rectangle contains the explosion. The test is inclusive:
`WorldX ≤ x ≤ WorldX + Width` (`inView`, `:466-472`;
`src/client/CClient_Game.cpp:307-317`).

```
Shake(amount):  if smooth: return            // never shakes spectator cameras
                start = now; shaking = true; ShakeAmount = max(ShakeAmount, amount)
Process (each drawn frame, after the follow):
  if shaking:
     if now - start > 0.2 s: shaking = false; ShakeAmount = 0
     elif type is not ACTIONCAM/FREELOOK and lobby[ScreenShaking]:      // default true
        Clamp()                                            // clamp first...
        WorldX += (int)(GetRandomNum() * ShakeAmount)      // GetRandomNum ∈ [-1, 1]
        WorldY += (int)(GetRandomNum() * ShakeAmount)
  Clamp()                                                  // ...and clamp again
```

The shake has these properties:

- It has **no decay**. The amplitude stays constant, a uniform random offset of
  up to ±`ShakeAmount` world px, redrawn every drawn frame.
- It lasts 0.2 s after the *last* shake. Each new shake restarts the window and
  can only raise the amount.
- Because of the second clamp, a camera at a map edge shakes only inward.

Shake amounts come from the projectile action's `Shake`: the maximum over the
events that fired (`src/common/PhysicsLX56_Projectiles.cpp:761-805`,
`1115-1131`). Classic uses about 1–4. A bonus blown up by an explosion or a beam
uses shake 5 (`CClient_Game.cpp:268`, `:1000`). `ScreenShaking` is feature 82,
default `true` (`src/common/FeatureList.cpp:103-104`).

The two `GetRandomNum()` calls are OLX's global `rand()`. The remake uses its own
RNG for them.

### 1.5 Death and respawn

- **On death the camera stays.** `VW_FOLLOW` keeps tracking the dead worm's
  frozen position (`CViewport.cpp:160-170`). It does not follow the killer.
- **On respawn the camera snaps.** `ParseSpawnWorm` re-locks the viewport whose
  original target is the worm: `VW_FOLLOW`, `setSmooth(!owned)`
  (`src/client/CClient_Parse.cpp:1056-1067`). Spawning teleports the worm, so
  the camera jumps there in one frame.
- `VW_CYCLE` switches to another living worm 0.5 s after its target dies
  (`CViewport.cpp:180-234`). Only spectators use it, via the viewport manager or
  spectator keys. Those keys work only once every local human is dead
  (`CClient_Draw.cpp:2518-2545`).
- **The dead player sees a message bar.** While its original target is dead
  and not out, the viewport's bottom 20% gets a bar at `x+2 .. x+w−2`, height
  `0.2·h − 2`. It is filled with `Color(0,0,0,100)`, or `(50,0,0,100)` /
  `(70,0,0,100)` for some states. It holds centred white text such as
  `"Press Jump (<key>) to respawn"` or `"Waiting for respawn ..."`
  (`CClient_Draw.cpp:1372-1414`).

### 1.6 Sound listener

At the end of `Process`, the viewport's listener position becomes the **view
centre**, `(WorldX + Width/2, WorldY + Height/2)` in world px, after shake and
clamp (`CViewport.cpp:365`). OpenAL uses only the first listener, which is
viewport 0 (`src/sound/sfxdriver_openal.cpp:55-65`). See §8.

---

## 2. Map drawing

### 2.1 Layers

A level has **two 2× colour images** (`CMap::CreateSurface`,
`src/common/CMap.cpp:443-475`):

- **`bmpDrawImage`** is the visible composite. Where material is background it
  already equals the back image.
- **`bmpBackImageHiRes`** is the background, revealed when dirt is removed.

The game draws **only `bmpDrawImage`**, as one blit (`CMap::gusDraw`,
`src/gusanos/level.cpp:257-287`). It does so before every object. There is no
separate front layer drawn over objects. `bmpForeground` and the parallax layer
exist only for Gusanos levels. Dark mode and lightmaps are Gusanos-only and can
be ignored.

How the images are built:

- **Image `.lxl` levels** pixel-double the stored back and front images. Every
  `PX_EMPTY` pixel copies back into front
  (`src/common/MapLoader_LieroX.cpp:105-145`).
- **Paletted / bitmask `.lxl` levels** tile the theme's `FrontTile.png` and
  `BackTile.png` (`CMap::TileMap`, `CMap.cpp:770-800`). Empty bits copy the
  back image (`MapLoader_LieroX.cpp:428-499`).

The remake's `Level { front, back }` is exactly this pair.

### 2.2 Edits to the colour image

Every edit updates material and colour together. Material tests read the flags
**before** the edit. The sim's `TerrainEdit { op, stamp, x, y }` carries the
integer position OLX passes. Stamps are centred as `x − w/2`, `y − h/2` (integer
division, `>>1`), and every edit is clipped to the map.

**Holes.** The theme's holes are `hole1..5.png`, at 5, 7, 9, 11 and 15 px in
Dirt. They are colour-keyed on black (`CMap.cpp:333-336`). Magenta means
"carve". Any other colour is the rim: `(108,56,0)` in Dirt.

**`CarveHole(size, pos)`** (`CMap.cpp:1007-1106`). `size` is clamped to 0..4,
and the stamp is `Holes[size]`. This covers `TerrainOp::Carve` and
`TerrainOp::Beam`.

```
for each stamp pixel s over map pixel m (clipped):
  if material[m] is DIRT:                       // rock and background untouched
    if s == magenta: material[m] = EMPTY; colour[m] = back[m]   // CopyPixel2x2
    elif s != black: colour[m] = s                               // brown rim, stays dirt
```

**`PlaceDirt(size, pos)`** (`CMap.cpp:1114-1224`) uses the same hole stamps.
The dirt colour comes from the **theme's `FrontTile.png`**, tiled from the map
origin. It does **not** come from the level's own front image.

```
for each stamp pixel s over map pixel (dx,dy), oldflag = flags before:
  if s != black (not colour-keyed) and !(oldflag & ROCK):
      material = DIRT; colour = FrontTile[dx % tw, dy % th]
  if s != black and s != magenta and (oldflag & EMPTY):
      material = DIRT; colour = s                  // rim colour over the fresh dirt
```

**`PlaceGreenDirt(pos)`** (`CMap.cpp:1231-1340`). The mask is
`data/gfx/greenball.png`, 8×9 px (`CMap.cpp:366-368`). Green `(0,255,0)` means
fill. Magenta means nothing. Any other colour is the rim, `(108,80,0)`.

```
greens = [(148,136,0), (136,124,0), (124,112,0), (116,100,0)]
for each mask pixel s over an EMPTY map pixel:
  if s == (0,255,0):      material = DIRT; colour = greens[GetRandomInt(3)]  // 0..3 inclusive
  elif s != magenta:      material = DIRT; colour = s
```

The green shade is drawn from `rand()` per pixel. The remake picks it with its
own RNG (spec D5).

**Persistent stains made by entities.** These change only the colour image,
never material:

- **Blood** that hits rock or dirt paints a 2×2 colour block at its pixel:
  `putColorTo` (`CMap.cpp:2365-2368`, `src/client/Entity.cpp:363-369`).
- **A slow gib** stamps a 4×4 low-res gib frame, doubled, at
  `(int)pos − (1,1)`: `putSurfaceTo` (`CMap.cpp:2370-2373`, `Entity.cpp:373-389`).
  It is colour-keyed, so magenta is skipped. `DrawImageStretch2`
  (`src/client/GfxPrimitives.cpp`) clips the destination but not the source
  offset: a gib clipped at the map's left or top edge still draws from the
  frame's first column and row.

A stain on dirt disappears when the dirt is carved, because carving copies the
back image. A stain on rock is permanent. Stains also change later dirt-particle
colours (§7.3), which read `bmpDrawImage`.

### 2.3 Map border, shadows and theme colour

**Map border.** Nothing is drawn. The view never goes past the edge (§1.3).

**Shadows: none in this build.** The option `Game.Shadows` defaults to `true`
(`Options.cpp:214`), but:

- The static map shadow `ApplyShadow` and `CalculateShadowMap` are commented
  out (`CMap.cpp:477-601`, `1475-1484`).
- `DrawObjectShadow`, used for worm and image-projectile shadows, is an empty
  stub: `// TODO: reimplement ...` (`CMap.cpp:967-970`).
- `DrawPixelShadow` (`:975-999`) handles pixel projectiles. At
  `(x+3, y+3)`, if that pixel is empty, it draws the **back-image colour at
  alpha 96**. That is invisible, since the draw image already equals the back
  there.

Classic OLX (the commented code) used `SHADOW_DROP = 3`
(`src/level/LXMapFlags.h:38`). It drew a copy of the back image darkened by
black at α 96, at +1..+3 px diagonally below solid pixels, and object shadows
with the skin silhouette at `SHADOW_OPACITY`. The remake should treat shadows as
off (faithful) or decide explicitly to restore them.

**Theme default colour** (`CMap.cpp:338-355`) is
`FrontTile[0,0]`, overridden by the last non-black, non-magenta pixel of
`hole1`. It is used only as the explosion's dirt-particle fallback, which is
never shown (§7.3).

### 2.4 Minimap (`DrawMiniMap`, `CMap.cpp:1774-1816`)

**Size.** `MinimapW`/`MinimapH` come from `data/frontend/frontend.cfg`: 119×94
in `[LocalGameInterface]`, 118×94 in network games, and 128×96 if missing
(`CClient_Draw.cpp:122-123`). They are applied with `SetMinimapDimensions`
(`src/game/Game.cpp:590`).

**Content.** The whole `bmpDrawImage` is scaled non-uniformly to the minimap
rectangle, first filled black. With `Game.Antialiasing` (default `true`) the
scaling is resampled, otherwise nearest (`level.cpp:486-527`). Map edits update
the minimap in a region padded by 10 px (`UpdateArea`, `CMap.cpp:605-673`). The
result is blitted with **per-surface alpha 128**.

**Position:**

| Players | Minimap position |
|---|---|
| 1 | top-right corner, `(displayW − MiniMapW, 0)` |
| 2 | centred on the vertical seam, at the top |
| 3 | centred in the empty quarter |
| 4 | centred on the screen |

(`CClient_Draw.cpp:611-647`.) It is shown if `FT_MiniMap`, which defaults to
`true` (`FeatureList.cpp:107-108`).

**Worm dots** (`drawOnMiniMap`, `CMap.cpp:1708-1770`) are drawn for alive worms:

```
blink += frameDt; if blink > 0.5: blink = 0           // sawtooth, per drawn frame
c = worm game colour;  c += (255 - c) * blink * 2      // brightens toward white, then snaps back
i = clamp(floor(pos.x / (MapW/mw)), 0, mw-1) + miniX;  j likewise
local human worm ("big"): 3x3 rect [i-1, i+2) x [j-1, j+2)
others:                   2x2 block at (i-1, j-1)
```

---

## 3. Worms (`CWorm::Draw`, `src/common/CWorm.cpp:1016-1334`)

Only **alive** worms are drawn: the loop is over `game.aliveWorms()`
(`src/gusanos/viewport.cpp:170-171`). A dead worm shows nothing but its gibs and
blood entities. The draw position is the current physics position
(`UpdateDrawPos`: `vDrawPos = vPos`, `CWorm.cpp:900-936`). There is no
interpolation.

`p = physicToReal(pos)` truncates the position to int world px, then gives
`(X − WorldX)*2 + Left`. That is always even.

Draw order inside `CWorm::Draw`:

1. ninja rope (§4);
2. damage popup;
3. health bar (if enabled);
4. crosshair;
5. **skin**;
6. muzzle flash;
7. CTF flag;
8. weapon name;
9. worm name.

Worms are drawn in worm-id order (`aliveWorms`).

### 3.1 Skin sheet and frame selection

The sheet is **672×36**: 21 frames of 32×18. The top row (y 0..17) is the image
and the bottom row (18..35) is the colorization mask (`include/CGameSkin.h:36-40`,
`src/client/CGameSkin.cpp:268-309`). Each frame's worm sprite is the 20×18
rectangle at `sx = frame*32 + 4`, `sy = 0` (`CGameSkin.cpp:592-606`). A skin with
fewer than 5 frames is rejected (`:296-301`).

```
walk = (int)worm.frame                         // sim field: += 7.5*dt while moving,
                                               // 0 when not moving, wraps at 3
                                               // (PhysicsLX56.cpp:340-346)
ang  = min((int)((worm.angle + 90) / 151 * 7), 6)   // angle -90 (up) .. +60 (LX56 max)
f    = walk*7 + ang                            // 0..20; frames 0..6 = walk frame 0, up -> down
```

Frame `f` is clamped to `[0, frameCount−1]` (`CWorm.cpp:1228-1234`).

**Facing right.** The sprite is source columns `[32f+4, 32f+24)`.

**Facing left.** OLX mirrors the whole sheet and blits from
`W − sx − 20 − 1` (`CGameSkin.cpp:597-601`). Destination column `k`
(0..19) shows original column `sx + 20 − k`. That is the mirror of columns
`sx+1 ..= sx+20`, **one column to the right** of the right-facing sprite. The
off-by-one is an OLX quirk.

**Placement on the 2× surface** (`CWorm.cpp:1225-1244`):

```
x = p.x - p.x%2;  y = p.y - p.y%2          // already even
blit 20x18 at (x - 10, y - 9)               // vertically half a world px off the grid
```

The skin is colorized with the worm's game colour (already ported:
`rl-data::skin::colorize`, `CGameSkin.cpp:720-790`). The bot icon is not drawn
in game (`draw_cpu = false`).

### 3.2 Gibs image

`data/gfx/giblets.png` is 32×12, colorized per worm by `CWorm::ColorizeImage`
(`CWorm.cpp:655-757`). The colorization formula:

- uses no mask;
- skips magenta and the three gun greys `(216,216,216)`, `(180,180,180)`,
  `(144,144,144)`;
- tints everything else with `min(255, c * worm / (96,156,252))`, nudging magenta
  results to `(240,0,240)`.

The image has two parts:

- **Rows 0..7** hold four 8×8 2× frames, drawn while flying.
- **Rows 8..11** hold 4×4 1× frames, stamped onto the map image when a gib
  lands (§7.2).

### 3.3 Crosshair (`CWorm.cpp:1184-1201`)

It is drawn for **local** worms (bots in a local game too) when `drawExtras` is
set, which is always true in deathmatch.

```
a = (int)worm.angle;  if facing left: a = 180 - a
forw = (cos a°, sin a°) * Game.CrosshairDistance   // default 32.0 (Options.cpp:234), in screen px = 16 world px
cp = p + (int)forw                                   // per component, truncated
frame = bGotTarget ? 1 : 0;  bGotTarget = false      // reset after every draw
blit crosshair.png src (frame*6, 0, 6, 6) at (cp.x - 2, cp.y - 2)
```

`data/gfx/crosshair.png` is 12×6: a red `(255,0,0)` frame and a green
`(84,216,84)` frame. It is magenta-keyed. Blitting at `−2` puts the 6×6 sprite
1 screen px off-centre.

**The crosshair turns green** when, in the latest sub-step, the worm's
**laser-sight trace** hit another worm (`setTarget(true)`,
`CClient_Game.cpp:786-793`). Only laser-sight weapons do this.

### 3.4 Health bar above worms

The bar is off by default: `Game.ShowWormHealth` is `false`
(`Options.cpp:217`). The general key `h` toggles it
(`CClient_Draw.cpp:1456-1458`). It is shown only for **non-local or bot** worms
(`CWorm.cpp:1135-1178`).

- The image is `data/frontend/worm_health.png` (26×13), a `CBar` with one fore
  state and one background state. Each band is `(13−1)/2 = 6` px high
  (`DeprecatedGUI/CBar.cpp:49-77`).
- It is drawn at `(x − 13, y − 9 − 6 − 1)` in screen px. Its fill width is
  `26 * health/100`.
- **Quirk:** `Draw` runs before `SetPosition(health)`, so the bar shows the
  previous frame's health.
- Without the image, an old-style bar is drawn: five 4×5 segments in red,
  orange, yellow, light green and green, inside border `(0x49,0x50,0x65)`
  (`:1148-1175`).

### 3.5 Name, weapon name and damage popup

**Name** (`CWorm.cpp:1317-1332`). Text is drawn with the outline font, centred
at `(x, y − WormNameY)`. `WormNameY = fontHeight + 12`, plus
`healthbarH + 2` if the health bar is drawn. Colour is `clPlayerName`
(`#FFFFFF`), or the team colour in team games. **Local human worms get no
name**, except an AFK message. In a 2-player local game no names are shown.

**Weapon name** (`:1306-1315`) is drawn for local humans while
`isChangingWpn()`. That is true while SelectWeapon is held, or for 0.75 s after
a cycle or quick-select (`src/gusanos/base_worm.cpp:678-684`,
`src/common/CWormHuman.cpp:418-456`). It is the outline font, centred at
`(x, y−30)` screen px, colour `clPlayerName`.

**Damage popup** (`:1064-1128`). It is on by default:
`Game.DamagePopups = true` (`Options.cpp:231`). It is drawn per victim from
`cDamageReport[owner]`. The client adds `min(health, damage)` to an entry only
when the owner is a worm (`owner ≥ 0`), stamping `lastTime`
(`CClient_Game.cpp:372-394`).

```
order entries by lastTime; t0 = oldest lastTime
yoff = WormNameY + fontH + 30 * (now - t0)          // rises 30 screen px/s
sum  = Σ damage;  colour = sum >= 0 ? (0xff,0x80,0x40) : (0x40,0xff,0x00)
text = itoa(Round(sum)) with '-' replaced by '+' for healing; skipped if sum == 0
outline font, centred at (x, y - yoff)
when now > t0 + 1.5 s: clear all entries
```

With `ColorizeDamageByWorm` (off by default) there is one line per attacker in
the attacker's colour.

### 3.6 Muzzle flash (`CWorm.cpp:1272-1299`)

`ProcessShot` sets `bDrawMuzzle` for **projectile weapons** only. Beams return
earlier and never get a muzzle (`CClient_Game.cpp:888-917`).

The image is `data/gfx/muzzle.png`, 224×16: 14 frames of 16×16 at **1×**,
blitted **stretched 2×** (32×32 screen px = 16×16 world px) with the magenta key
(`DrawImageStretch2Key`, `src/client/GfxPrimitives.cpp:952`). The offset tables
are `CWorm.cpp:896-897`:

```
RightMuzzle = [ 2,3,  5,3,  4,0,  5,-8,  3,-9,  2,-13, -2,-12 ]
LeftMuzzle  = [ 4,-12, -1,-12, -1,-9, -3,-8, -2,0, -2,4,  1,3 ]
a = (int)((worm.angle + 90) / 151 * 7)          // 0..6, not clamped (max angle 60 gives 6)
facing right: k = 6 - a; frame = k;     at (x - 12 + RightMuzzle[2k], y - 10 + RightMuzzle[2k+1])
facing left:  k = a;     frame = k + 7; at (x - 21 + LeftMuzzle[2k],  y - 10 + LeftMuzzle[2k+1])
```

Frames 0..6 point from down-right to up, and frames 7..13 are their mirrors.
`bDrawMuzzle` is cleared after the first `Draw` that reaches that code, so the
flash shows for **one drawn frame, in the first viewport that draws the worm**.
`Spawn` also clears it (`CWorm.cpp:616`).

There is no weapon sprite. The gun is part of the skin, which is why the gun
greys are not tinted.

### 3.7 Spawn effect

When a worm (or bonus) spawns, an `ENT_SPAWN` entity appears at its position
(`CClient_Parse.cpp:1049-1053`, `1515`). See §7.

### 3.8 Laser sight and beams

These are cosmetic entities, described in §7.4 and §7.5.

---

## 4. Ninja rope (`CNinjaRope::Draw`, `src/client/CNinjaRope.cpp:132-180`)

The rope is drawn only while `released` and the worm is alive.

```
h = (int)hook.pos;  if attached to a worm: h = (int)thatWorm.pos   // corrects order effects
hs = (h - World)*2 + Left;   ps = ((int)worm.pos - World)*2 + Left; both snapped even
DrawRope(hs -> ps)                      // from the hook to the worm
if hs inside the viewport and drawExtras: blit hook.png at (hs.x - 2, hs.y - 2)
```

`data/gfx/hook.png` is a 6×6 2× sprite (3×3 world px) in greys
`(192,192,192)`, `(143,143,143)` and `(255,248,255)`.

**Rope colours** come from `data/frontend/colours.cfg`: `RopeColour1 = #A05000`
and `RopeColour2 = #C86400` (`src/client/DeprecatedGUI/Graphics.cpp:311`). The
`Color(159,79,0)` argument is ignored. `DrawRope`
(`GfxPrimitives.cpp:2458-2477`) clamps the endpoints to `w−3`/`h−3`, then
draws one of two ways.

**`Game.Antialiasing = false`**: an Allegro Bresenham line
(`perform_line`, `GfxPrimitives.cpp:1906-1995`) from the hook. Every step writes
a 2×2 block. `RopePutPixel` (`:1830-1846`) toggles a phase each call and swaps
colour every second call:

```
alt = 0; col = 0
per screen-px step: alt = !alt; if alt: col = !col; plot 2x2 in RopeColour[col]
=> colours C8,C8,A0,A0,C8,C8,...: alternates every world px, starting with RopeColour2 at the hook
```

At world resolution this is a 1-px line alternating colours per pixel.

**`Game.Antialiasing = true`**, the default (`Options.cpp:221`): `AntiAliasedLine`
(`:2260-2344`). It may swap the endpoints so it runs left to right or top to
bottom. Each step calls `RopePutPixelA` (`:1799-1826`) twice: the main pixel at
weight `255 − frac` and its neighbour at `frac`, each as an alpha-blended 2×2
block. The phase is `alt = (alt+1) % 3`, swapping colour when it reaches 2. The
effect is a soft, ~1.5 world px band whose colour changes about every 1.5 screen
px. That cannot be reproduced exactly at world resolution, so the
non-antialiased form is the natural world-res port.

There is one rope segment, a straight line. There are no rope physics segments
to draw.

---

## 5. Projectiles (`CProjectile::Draw`, `src/common/CProjectile.cpp:227-320`)

`p = physicToReal((int)pos)`.

**`PRJ_PIXEL`.** If `posInside(p)`, draw a 2×2 block of `iColour` at
`(p.x−1, p.y−1)` (`DrawRectFill2x2`, `GfxPrimitives.cpp:2245-2255`). It covers
screen px `2X−1 .. 2X`, which is **half a world pixel up-left of the grid**.

`iColour` is picked at spawn as `Colour[GetRandomInt(n−1)]` (`:86-95`). The sim
keeps it as `Projectile::colour`. The colour's alpha comes from the mod.

**`PRJ_IMAGE`.** The image is a horizontal strip of square `size×size` frames,
with `size = image height`. It is blitted 1:1 at `(p.x − size/2, p.y − size/2)`,
so images are 2× art. It is magenta-keyed or uses PNG alpha. The frame is chosen
by the **first** rule that applies, and only while `posInside(p)`. Off-screen
the frame is 0.

```
framestep = 0
if Animating:            framestep = proj.frame            // sim field (fFrame)
elif UseSpecAngle:
    angle = -atan2(vel.x, vel.y) * 180/PI;  direct = 0
    if angle > 0: angle = 180 - angle
    if angle < 0: angle = 180 + angle; direct = 1
    if angle == 0: direct = 0
    num = (AngleImages - 1) / 2                               // int
    framestep = direct == 0 ? (151 - angle)/151 * num
                            : angle/151 * num + num + 1
elif UseAngle:
    angle = fmod-wrap(-atan2(vel.x, vel.y) * 180/PI, 360)     // FMOD -> [0, 360)
    framestep = angle / (360 / AngleImages)
elif RotIncrement != 0 && Rotating && vel != (0,0):
    framestep = proj.rotation / RotIncrement                 // sim field (fRotation, degrees)
frameX = ((int)framestep * size) mod image.w                 // MOD: always non-negative
blit src (frameX, 0, size, size)
```

The sim maintains `rotation` and `frame` (`PhysicsLX56_Projectiles.cpp:1251-1288`):

- **Rotation.** `rotation += RotSpeed*dt`, wrapped to `[0, 360)`.
- **Animation.** `frame ± AnimRate*dt`, and on reaching
  `NumFrames = w/h`:
  - `ANI_ONCE` deletes the projectile;
  - `ANI_LOOP` resets to 0;
  - `ANI_PINGPONG` reverses and sets `NumFrames−1`, and at `< 0` reverses and
    sets 0.

**`RotIncrement` 0→1 mutation.** When a spawn with `UseRandomRot` creates a
`Rotating` projectile whose `RotIncrement == 0`, OLX writes
`RotIncrement = 1` **into the shared `proj_t`**, permanently for the rest of the
loaded mod's life (`PhysicsLX56_Projectiles.cpp:589-595`).

- Before that happens, such projectiles draw unrotated, because the last rule
  requires `RotIncrement != 0`.
- After it, they draw frame `(int)rotation` (degrees!) modulo the frame count.

`rl-sim` uses the 1 for its draw but **does not keep the mutation**
(`crates/rl-sim/src/spawn.rs:91-97`). The renderer must track it itself; see the
open questions.

`DrawShadow` is a no-op (§2.3). Circle, rect and polygon types are not used by
LX56 mods.

**Trails** are cosmetic entities spawned by the sim, all at the projectile
position (`PhysicsLX56_Projectiles.cpp:1290-1324`):

| Trail | Interval | Entity |
|---|---|---|
| `TRL_SMOKE` | every 0.075 s of projectile time | `ENT_SMOKE` |
| `TRL_CHEMSMOKE` | every 0.075 s | `ENT_CHEMSMOKE` |
| `TRL_DOOMSDAY` | every 0.05 s | `ENT_DOOMSDAY`, with the projectile's velocity, unused |
| `TRL_EXPLOSIVE` | every 0.05 s | `ENT_EXPLOSION` size 10 |
| `TRL_PROJECTILE` | | real projectiles, nothing cosmetic |

---

## 6. Bonuses (`CBonus::Draw`, `src/common/CBonus.cpp:49-76`)

Bonuses are drawn if `FT_Bonuses` (`CClient_Draw.cpp:2178-2191`), at
`p = physicToReal((int)pos)`, with the whole image blitted 1:1:

- **Health:** `data/gfx/health.png` (10×10, a white box with a red cross) at
  `(p.x−5, p.y−5)`.
- **Weapon:** `data/gfx/bonus.png` (12×8, an orange crate) at `(p.x−5, p.y−5)`.
  This is off-centre: it covers `x−5..x+7`, `y−5..y+3`. With `FT_ShowBonusName`
  the weapon name is drawn too: outline font, centred at `(p.x, p.y−20)`,
  `clPlayerName`.

**Flashing near expiry.** The sim keeps `life` and `flash_time` per client bonus
(`PhysicsLX56.cpp:623-637`):

```
life += dt;  if life > BONUS_LIFETIME - 3 (= 57 s):  flash += dt; if flash > 0.3: flash = 0
draw: if life > 57 and flash > 0.15: skip           // ~0.15 s on, ~0.15 s off
```

`BONUS_LIFETIME` is 60 (`include/CBonus.h:30`). Bonuses fall at 80 px/s² and
rest 2 px above the ground (`PhysicsLX56.cpp:618-686`), which the sim already
does. A spawning bonus shows `ENT_SPAWN`.

---

## 7. Cosmetic entities (`src/client/Entity.cpp`)

### 7.1 The pool and its clock

- **Pool.** `FastVector<entity_t, 1024>` (`include/Entity.h:30`). Free slots are
  reused lowest-first (`include/FastVector.h:87-94`), so iteration and draw
  order is **slot order**. When the pool is full, spawns are dropped
  (`Entity.cpp:175-179`).
- **Particles option.** With `Game.Particles = false`, `ENT_PARTICLE` and
  `ENT_BLOOD` are never spawned (`:163-168`). The default is `true`
  (`Options.cpp:215`).
- **When entities run.** `SimulateEntities(LX56PhysicsDT)` runs **once per
  physics sub-step** inside `CClient::Simulation`. It runs after the worm loop
  (worm physics, bonus pickups, client-side shooting, laser sight) and before
  the projectiles and bonuses (`CClient_Game.cpp:58-222`, `:206-208`). So
  entities spawned by worms are simulated in the same sub-step, and those
  spawned by projectiles in the next one.
- **Two time bases:**
  - **Motion** uses `dt = 0.011 s`, the sub-step.
  - **Animation** uses `realdt = tLX->fRealDeltaTime`, which inside the game
    loop is fixed at **0.010 s** (`src/game/Game.cpp:905`). So frames advance
    `rate × 0.01` per sub-step, about `0.909 × rate` per second.

### 7.2 Spawn and simulate (`SpawnEntity`, `:161-205`; `SimulateEntities`, `:320-468`)

At spawn: `fFrame = 0`, `fLife = 0`, `fExtra = 0`, with these exceptions:

- `ENT_EXPLOSION`: `fFrame = 15 − size`.
- `ENT_GIB`: `fAnglVel = |GetRandomNum()|·20`, `iRotation = (int)(|GetRandomNum()|·3)`.
- `ENT_JETPACKSPRAY`: `fLife = 3`, unused.

**Falling entities** are `ENT_PARTICLE`, `ENT_BLOOD`, `ENT_BLOODDROPPER`,
`ENT_GIB` and `ENT_JETPACKSPRAY` (`:337-397`):

```
if GIB: iRotation = (iRotation + (int)(fAnglVel * 0.01)) % 5   // (int)(<=0.2) == 0: gibs never spin
vel.y += 100 * 0.011;  pos += vel * 0.011
if pos outside [0,W) x [0,H): remove
elif material at (uint)pos is ROCK or DIRT:
    BLOOD:  paint colour at (int)pos onto the map image (2x2); remove
    GIB:    if |vel|^2 > 25600: vel *= -0.4 (both axes); fAnglVel *= 0.8   // bounce
            else: stamp giblets.png src (iRotation*4, 8, 4, 4) doubled at (int)pos - (1,1); remove
    other:  remove
```

Gibs start at no more than 80 px/s plus gravity, so they almost always stick on
first contact.

**Animated entities** (`:401-464`), with `+=` meaning per sub-step:

| Entity | Frame advance | Removed when | Lifetime |
|---|---|---|---|
| `EXPLOSION` | `+0.40` | `fFrame > 15` | `(size)/0.4` sub-steps (8: ~21 ≈ 0.23 s) |
| `SMOKE`, `CHEMSMOKE` | `+0.15` | `(int)fFrame > 4` | 34 sub-steps ≈ 0.37 s |
| `DOOMSDAY` | `+0.15` | `(int)fFrame > 3` | 27 ≈ 0.30 s |
| `SPAWN` | `+0.15` | `(int)fFrame > 5` | 40 ≈ 0.44 s |
| `SPARKLE` | `+0.01·fade`, fade = `type2` or 5; air-jump sparkles use 10 | `(int)fFrame > 2` | fade 10: 30 ≈ 0.33 s |
| `JETPACKSPRAY` | `+2.0` | `(int)fFrame > 150` (or hits terrain) | 76 ≈ 0.84 s |
| `BEAM`, `LASERSIGHT` | `if (int)fFrame == 1: remove; fFrame++` | the sub-step after the one it was spawned in | 2 sub-steps |
| `BLOODDROPPER` | `if fExtra > 0.1 s: spawn ENT_BLOOD(pos, GetRandomVec(), (colour[GetRandomInt(1)],0,0)) with colour = {128,200}; fExtra = 0`, then `fExtra += 0.011` | as a falling entity | drips a blood drop every 10 sub-steps (first at the 11th) |

`SPARKLE` also moves without gravity: `pos += vel*0.011`.

**Worm beam slot** (`DrawBeamInfo`, `:35-157`). There is one per worm.
`SetWormBeamEntity` sets `frame = 0`. Each sub-step, `simulate()` does
`if frame >= 1: unused else frame++`. A beam set every sub-step stays visible,
and one not refreshed disappears after one more sub-step.

### 7.3 Drawing (`DrawEntities`, `:210-315`)

The screen position keeps half-pixel precision:
`x = int((pos.x − WorldX)*2) + Left`, and likewise `y`. Entities whose `(x, y)`
is outside the viewport are skipped. Beams and laser sights are never skipped.

| Type | Sim `CosmeticKind` | Drawing |
|---|---|---|
| `ENT_PARTICLE` | `Dirt` | 2×2 block of its colour at `(x−1, y−1)` |
| `ENT_BLOOD` | `Blood{red}`, colour `(red,0,0)`: 200, 128 or 180 | 2×2 at `(x−1, y−1)` |
| `ENT_BLOODDROPPER` | `BloodDropper` | 2×2 `(128,0,0)` at `(x−1, y−1)` |
| `ENT_EXPLOSION` | `Explosion{size}` | `explosion.png` (512×32, 16 frames 32×32, small→big→fading) frame `(int)fFrame` at `(x−16, y−16)` |
| `ENT_SMOKE` | `Smoke` | `smoke.png` (70×14, 5 frames 14×14) at `(x−7, y−7)` |
| `ENT_CHEMSMOKE` | `ChemSmoke` | `chemsmoke.png` (40×10, **4** frames 10×10) at `(x−5, y−5)`; frame 4 is past the image, so the last sub-steps draw nothing |
| `ENT_SPAWN` | `Spawn` | `spawn.png` (160×32, **5** frames 32×32) at `(x−16, y−16)`; frame 5 draws nothing |
| `ENT_GIB` | `Gib` | worm's colorized `giblets.png` src `(iRotation*8, 0, 8, 8)` at `(x−2, y−2)` (off-centre) |
| `ENT_SPARKLE` | `Sparkle` | if `colour == Color()`: `sparkle.png` (30×10, 3 frames 10×10, lavender star) at `(x−5, y−5)`; else two 2×2 blocks at `(x, y−1)` and `(x−1, y)` |
| `ENT_DOOMSDAY` | `Doomsday` | 2×2 of `doomsday[(int)fFrame]` = `(244,244,112)`, `(248,192,36)`, `(248,108,20)`, `(248,108,20)` at `(x−1, y−1)` (`:81-86`) |
| `ENT_JETPACKSPRAY` | `JetpackSpray` | 2×2 at `(x−1, y−1)` of `(min(0.314(255−f),255), min(0.588(255−f),255), min(0.784(255−f),255))`; `(80,150,200)` fading to `(33,62,82)` |
| `ENT_LASERSIGHT` | `LaserSight` | §7.4 |
| worm beam slot | `Beam{worm}` | §7.5; drawn after all other entities |

**Explosion size and frame.** The sim's `Explosion { size }` is
`size = damage > 5 ? (int)damage : 8` for `CClient::Explosion`
(`CClient_Game.cpp:296-300`). Other sources:

- 5 where `DrawBeam` hits terrain with a beam of width ≤ 2;
- 3 at `p+(1,1)` where it touches a worm (`:665`, `:681`);
- 10 for explosive trails.

The start frame is `15 − size`. Large explosions start small and grow, while
small ones start already big and fade. **Quirk:** for `size > 15`, `fFrame`
starts negative. `int(fFrame) ≤ −1` blits from `x < 0`, and SDL clips that away
(`DrawImageAdv` uses `SDL_BlitSurface`, `GfxPrimitives.cpp:742-754`). So a
damage-20 explosion is **invisible for about 0.14 s** before playing frames
0..15.

**Dirt-particle colour.** It is read from the colour image **before** the carve
(the sim emits `Dirt` before the `TerrainEdit`). Each source probes a different
pixel:

- **Digging** (`CarveHole(pos)`, `CMap.cpp:2172-2199`). The first `PX_DIRT`
  pixel scanning `x = (int)pos.x−2 ..= +2` at `y = clamp((int)pos.y, 0, H−1)`.
  It throws 3 particles with `GetRandomVec()·(30,10)`.
- **Explosion** (`CClient_Game.cpp:229-251`). The first dirt pixel at
  `x = px−2 .. px+1` (exclusive `< px+2`), `y = min((uint)pos.y, H−1)`.
  It throws 2 particles with `·(30,10)`, only if dirt was found.
- **Rope hook landing in diggable material** (`PhysicsLX56.cpp:600-607`). The
  colour at the wrapped hook position. 5 particles at `hook + (0,2)` with
  `·40`.

### 7.4 Laser sight

The sim spawns `LaserSight` **every sub-step** for an alive worm whose current
weapon has `LaserSight` and is not reloading. `pos` is the worm and `vel` is the
unit aim direction (`CClient_Game.cpp:178-181`, `751-808`). OLX's trace, which
the renderer redoes:

```
a = facing left ? 180 - angle : angle; normalise to (-180, 180]
p = worm.pos; i = 0
while i < 9999:
    if !material(wrapX((int)p.x), wrapY((int)p.y)).particle_pass: break   // getMaterialWrapped: wraps even on finite maps
    if any other alive worm w: |p - w.pos|^2 < 25: worm.gotTarget = true; break   // green crosshair
    p += dir*3; i += 3
end = worm.pos + dir * i
```

Drawing (`DrawLaserSight` with `LaserSightPutPixel`,
`GfxPrimitives.cpp:2507-2520`, `1878-1901`):

- It is a Bresenham line in **screen px** from
  `(int((pos−World)*2)+L, …)` to `(((int)end − World)*2 + L, …)`, with the
  endpoints snapped even.
- Per screen-px step:

  ```
  static alt; alt = (alt + 1) % (GetRandomInt(35) + 1)
  if alt != 0: skip
  plot 2x2 at (x - x%2, y - y%2) in LaserSightColours[GetRandomInt(1)]
  ```

  The colours are `#BE0000` and `#A00000` (`colours.cfg`, `Graphics.cpp:313`).
  This gives a **sparse, randomly flickering dotted line**, redrawn with fresh
  randomness every frame. It is never antialiased. The counter is static, shared
  across lines.
- At world resolution: run the same recurrence twice per world pixel along the
  line, and plot a world pixel if either run fires.
- `colour = Color()` is ignored. The line is drawn only if the worm is visible
  to everybody, which is always true in deathmatch.

### 7.5 Beams

The sim emits `Beam { worm }` with `pos` = worm and `pos + vel` = where the
trace stopped. It does so in every sub-step during the 0.3 s after a beam shot
is delivered, while not reloading (`DrawBeam`, `CClient_Game.cpp:590-711`). The
colour is the weapon's `Bm.Colour` (from the worm's current weapon). Beams with
colour magenta `(255,0,255)` are not drawn (`:622-627`).

- **Width 1** (`InitWidth == 1 && WidthIncrease == 0`). `DrawBeam` draws from
  `(int)start` to `(int)end` (`Entity.cpp:54-65`, `GfxPrimitives.cpp:2482-2502`)
  with the endpoints snapped even:
  - without antialiasing: a Bresenham line of 2×2 blocks, a solid 1-world-px
    line (`BeamPutPixel`, `:1851-1872`);
  - with antialiasing (default): an antialiased line of alpha-weighted 2×2
    blocks.
- **Wide beams** are drawn as a filled polygon (`Entity.cpp:111-131`,
  `GfxPrimitives.cpp:2593-2615`). `o = orth(dir)`. The vertices are truncated to
  int world px:
  1. `start − o·InitWidth/2`
  2. `start + o·InitWidth/2`
  3. `end + o·w/2`
  4. `end − o·w/2`

  The end width `w = min(int(InitWidth + WidthIncrease·n), min(MapW, MapH))`,
  where `n` is the index of the last step reached, `|vel|` rounded. It is
  `Length−1` if the trace ran out.

The old `ENT_BEAM` path is dead code (`CClient_Game.cpp:1049-1054`).

### 7.6 Which `rand()` each effect uses in a real client

OLX has one global `rand()` (`GetRandomNum`, `GetRandomInt` and `GetRandomVec`,
`src/common/MathLib.cpp:58-130`). Every draw below advances it.

- **Already drawn by `rl-sim`** in OLX's order, because the dedicated server
  makes the same calls:
  - particle, blood, dropper and gib **velocities**;
  - a gib's two spin draws (`crates/rl-sim/src/damage.rs:140-145`);
  - the death-sound pick.
- **Client-only in OLX**, so the remake must use its own RNG:
  - the dropper's per-drip `GetRandomInt(1)` and `GetRandomVec()`
    (`Entity.cpp:456-462`), since a dedicated server never runs
    `SimulateEntities`;
  - laser-sight dots (2 per screen step);
  - shake (2 per drawn frame);
  - green-dirt shades (`GetRandomInt(3)` per pixel);
  - the renderer's own `fAnglVel` and `iRotation` for gibs.

In a real local OLX game these client draws interleave with gameplay draws. The
remake follows the dedicated-server stream (spec D4/D5).

### 7.7 Other `SpawnEntity` sources the sim emits

| Effect | Entity | Source |
|---|---|---|
| Air jump | 3 `ENT_SPARKLE`, fade 10, at `pos+(0,4)`, `(2,4)`, `(−2,4)` with `vel+(0,40)`, `(20,40)`, `(−20,40)` | `PhysicsLX56.cpp:388-393` |
| Bleeding below 15 HP, every 2 s | 20 pairs of `ENT_BLOOD` (200 and 180), `GetRandomVec()·100` | `PhysicsLX56.cpp:317-330` |
| Hit | `CLAMP(damage,10,100)` triples of dropper `(sp, 4sp)` + blood 128 + blood 200, `sp = r·(50+2·damage)` | `CClient_Game.cpp:449-459` |
| Death | 7 gibs `·80` and 50 triples (dropper + 200 + 128), `sp = r·100+50` | `CClient_Game.cpp:426-437` |
| Jetpack | `ENT_JETPACKSPRAY` with `vel = (15·GetRandomNum(), Thrust)`, colour `(80,150,200)` | `CClient_Game.cpp:566-572` |

`EntityEffect` sparkles (`Entity.cpp:480-576`) are not emitted in plain
deathmatch: only for tag, or damage and shield factors above 1.

---

## 8. Sounds

### 8.1 Files and loading

| Sound | Code path | File loaded | Max simultaneous |
|---|---|---|---|
| rope throw | `data/sounds/throw.wav` | `data/sounds/throw.ogg` | 4 |
| bonus pickup | `pickup.wav` | `pickup.ogg` | 2 |
| bump | `bump.wav` | `bump.ogg` | 2 |
| death 1/2/3 | `death1..3.wav` | `death1..3.ogg` | 2 each |
| team score (CTF/teams) | `teamscore.wav` | `teamscore.ogg` | 2 |
| mod sounds | `<mod>/sfx/<name>`, else `data/sounds/<name>` | `.wav`, or the same name as `.ogg` | 10 |

Sources: `src/game/Sounds.cpp:24-41`, `src/sound/Sounds.cpp:97-124`,
`src/common/CGameScript.cpp:1185-1205`. A missing `.wav` is retried as `.ogg`
(`Sounds.cpp:112-114`). The shared files in this commit are `.ogg`. A play is
skipped when that sample already has `max` copies playing
(`src/sound/sfx.cpp:120-134`).

### 8.2 When each sound plays

| Sound | Trigger | Call | Positional? |
|---|---|---|---|
| throw (`SoundId::THROW`) | rope thrown (`CWormHuman.cpp:572-574`) | `PlaySoundSample` | **no**, global |
| pickup (`PICKUP`) | bonus picked up (`CClient_Game.cpp:142`) | `PlaySoundSample` | **no**, global |
| bump (`BUMP`) | worm hits the map at `|v.x| > 30` horizontally or `|v.y| > 30` vertically (`PhysicsLX56.cpp:260-266`) | `StartSound` | yes |
| death (`DEATH[GetRandomInt(2)]`) | worm dies (`CClient_Game.cpp:423-424`) | `StartSound` | yes |
| weapon `smpSample` | projectile weapon shot processed, if `UseSound` (`CClient_Game.cpp:900-906`); **beams play no sound** | `StartSound` | yes, at the shot position |
| projectile action sound | `Hit`, `Timer` or `PlyHit` action with a sound (`PhysicsLX56_Projectiles.cpp:1209-1211`) | `StartSound` | yes, at the projectile |
| announcer | server `SendPlaySound`, played via `PlayGameSound` (`src/server/CGameMode.cpp:50`, `136-152`, `226-248`, `360-381`; `CClient_Parse.cpp:2280-2289`) | `PlaySoundSample` | no |

The announcer files are `data/sounds/game/*.ogg`: `prepareforbattle`, `begin`,
`03kills`/`05kills`/`10kills`/`15..30kills` for kill streaks of exactly that
length, `leadgained`/`leadlost`, `Nfragsleft`, `1..5`,
`1minuteremains`/`5minutesremain`. They are optional flavour.

The sim's `Sound` events carry a position for every sound. **Throw and pickup
must be played non-positionally** to match OLX.

### 8.3 Volume and attenuation (OpenAL)

`StartSound` ignores its `local` and `volume` arguments. The old pan code is
commented out (`src/sound/Sounds.cpp:216-261`). Both paths copy the sample into
a new OpenAL source.

- **Source setup.** `AL_ROLLOFF_FACTOR = 2`, `AL_GAIN = 0.7`
  (`sound_sample_openal.cpp:206-221`).
- **Positional** (`play2D`, `:258-270`). The source is at `(x, y, 0)` in world
  px, with `AL_REFERENCE_DISTANCE = 50`. The gain stays 0.7.
- **Global** (`play`, `:237-255`). The source is at the listener's `(x, y)`
  with z 0. The **gain is set to 1.0**.
- **Listener.** The listener is at `(viewCentre.x, viewCentre.y, −20)`
  (`SFX_LISTENER_DISTANCE`, `src/sound/sfxdriver.h`), oriented `at = (0,0,−1)`,
  `up = (0,1,0)`, so world +x is right. The listener gain is
  `Audio.Volume / 100`, default 70 (`Options.cpp:209`,
  `src/sound/sfxdriver.cpp:63-67`, `sfxdriver_openal.cpp:33-48`, `88-93`).
- **Distance model.** None is set, so OpenAL's default
  `AL_INVERSE_DISTANCE_CLAMPED` applies:

```
d     = sqrt(dx^2 + dy^2 + 20^2)                 // dx, dy from the view centre, world px
att   = 50 / (50 + 2 * (max(d, 50) - 50))        // 1.0 within ~46 px; 0.185 at the 320-px view's side edge
gain  = (Volume/100) * 0.7 * att                 // positional
gain  = (Volume/100) * 1.0                       // global (throw, pickup, announcer)
pan   = OpenAL stereo panning by direction (dx, dy, 20); roughly dx/d
```

There is no culling: off-screen sounds play, just attenuated. With split screen
only viewport 0's listener is used.

---

## 9. HUD in this OLX build (reference only)

The remake will redesign the HUD; this section is for reference. Bars and
labels are drawn per viewport after it (`CClient_Draw.cpp:1103-1357`). The fork
moved them onto the playfield, at the viewport's bottom-left
(`:1192-1209`):

- **Health bar.** `data/frontend/healthbar1.png`, fore colour `(64,255,64)`,
  bg `(128,128,128)` when there is no image, at `x = Left+5`,
  `y = Top+VirtH−5−fontH−2·barH−4`.
- **Weapon (charge) bar.** `weaponbar1.png`, 3 fore and 3 bg states, at
  `y = Top+VirtH−5−fontH−barH`. Its value is `Charge·100`. Its state is:
  - **reloading:** state 1, `(128,64,64)`;
  - **cooldown** (`LastFire > 0` and `ROF > 0.2`): state 2, `(255,161,66)`;
  - **ready:** state 0, `(64,64,255)`.

  (`:1221-1257`, `InitializeBar` `:276-385`.)
- **Weapon name** below the bars, colour `clWeaponLabel` `#02bff6`, with dots
  stripped.
- **Top bar**, if `Game.TopBarVisible` (default `true`). It has boxes for
  `Lives: N / ∞ (infinite.png) / Out` (`#00DE00`), `Kills: N` (`#DE0000`) and a
  special message (dirt count, "You are IT!"). Positions are in
  `data/frontend/frontend.cfg` `[LocalGameInterface]`
  (`CClient_Draw.cpp:1262-1355`). The top-left of the screen also shows the
  time-limit clock (`clock.png` plus `mm:ss`, white, or `(255,50,50)` in
  the last 10 s; `:762-787`, `DeprecatedGUI/Graphics.cpp:225-226`), FPS and
  ping.
- **Kill feed.** Kills are chat lines such as
  `"<killer> killed <victim> with <weapon>"`, `"<player> commited suicide"` and
  `"<player> drew first blood"` (`Options.cpp:787-792`). In a local game the
  last 6 lines are shown for 3 s each at `(LocalChatX, LocalChatY)` =
  `(0, 25)`, with a 1-px black shadow (`DrawLocalChat`, `:2211-2236`).
- **Scoreboard.** While **Tab** (`ShowScore`) is held, a full-width list is
  drawn over `clScoreBackground` (`DrawScoreboard`, `:2924-2953`).
- **Game over.** "Game Over" text, then the results menu.

---

## 10. Input

### 10.1 Default bindings

Player 1's keyboard defaults are slot 0, with gamepad `j1_*` in slot 1
(`src/client/Options.cpp:44-51`, `291-321`):

| OLX control | Player 1 (non-Apple) | Player 1 (Apple) | Player 2 | `rl-sim` key |
|---|---|---|---|---|
| Up | `up` | `up` | `kp 8` | `UP` (aim up) |
| Down | `down` | `down` | `kp 5` | `DOWN` (aim down) |
| Left | `left` | `left` | `kp 4` | `LEFT` |
| Right | `right` | `right` | `kp 6` | `RIGHT` |
| Shoot | `lctrl` | `lalt` | `kp +` | `FIRE` |
| Jump | `lalt` | `lmeta` | `kp enter` | `JUMP` |
| Rope | `x` | `x` | `kp -` | `ROPE` |
| SelectWeapon | `lshift` | `space` | `kp 0` | `CHANGE` |
| Strafe | `z` | `c` | `kp .` | none (strafing is off; the sim does not port it) |
| PreviousWeapon / NextWeapon | `a` / `s` | `a` / `s` | `kp 7` / `kp 9` | none |
| Weapon1..5 | `1`..`5` | `1`..`5` | `6`..`0` | none |

The general keys are (`Options.cpp:82-83`):

| Key | Action |
|---|---|
| `i` | Chat |
| `tab` | ShowScore |
| `h` | ShowHealth |
| `space` | ShowSettings |
| `F12` | Screenshot |
| `F2` | ViewportManager |
| `alt+enter` | SwitchMode |
| `F8` | ToggleTopBar |
| `o` | TeamChat |
| `F4` | IrcChat |
| `F3` | Console |

### 10.2 How the controls drive the worm

From `CWormHumanInputHandler::getInput`, `src/common/CWormHuman.cpp:81-592`,
which `rl-sim::physics::get_input` ports:

- **Aim.** Up and Down change the aim speed (LX56 aim).
- **Moving and facing.** Left and Right set facing and move, unless
  SelectWeapon is held.
- **Digging** is a *tap* of the opposite direction while holding a direction
  (`rightOnce` while Left is held, and vice versa, `:470-499`). There is no dig
  key.
- **Changing weapon.** `CHANGE` (SelectWeapon) is a **hold modifier**: while it
  is held, a Left or Right tap cycles the weapon, and movement is suppressed. On
  its own it does nothing (`:405-428`). The weapon cannot change while firing
  unless `WeaponCombos` is on.
- **Rope.** `ROPE` is the dedicated rope key: a press shoots the rope and plays
  the throw sound (`:567-576`). Jump releases an attached rope (`:535-540`).
  `Game.OldSkoolRope` (SelectWeapon+Jump) is off by default and not ported.
- **Respawn.** Jump while dead and allowed requests a respawn (`:115-119`).
- The prev/next and quick-select keys and strafe have no equivalent among the
  sim's 8 keys.

The keys are latched between sub-steps (`docs/porting/STATUS.md`, Input).

---

## 11. Frame order, interpolation and layers

### 11.1 Main loop (`Game::frame` / `frameInner`, `src/game/Game.cpp:692-722`, `897-966`)

```
ProcessEvents()                                     // keyboard
while simulationTime < now:                         // fixed 100 Hz ticks (catch-up)
    cClient->Frame() -> Simulation()                // 0..n 11-ms sub-steps per tick:
                                                    //   worms -> entities -> projectiles -> bonuses
    cServer->Frame(); simulationTime += 10 ms
cClient->Draw(videoSurface)                         // once, at the latest state
CapFPS()                                            // Advanced.MaxFPS default 95 (Options.cpp:255; AuxLib.cpp:276+)
```

- **There is no interpolation** between physics states. Worms
  (`vDrawPos = vPos`), projectiles, bonuses and entities are all drawn at their
  latest simulated positions.
- `CViewport::Process` (follow, shake, listener), the minimap blink and the
  muzzle reset run per **drawn frame** with the real frame delta.
- Entity animation and lifetimes advance per **sub-step** (§7.1).

### 11.2 Layer order per viewport

From `CViewport::gusRender`, `src/gusanos/viewport.cpp:105-253`, called from
`CClient::DrawViewport_Game`, `CClient_Draw.cpp:1001-1015`:

1. Clear the viewport to black.
2. Map composite image, `bmpDrawImage`, one layer.
3. Shadows, if `Game.Shadows`: projectile pixel shadows, then worm shadows. Both
   are effectively invisible no-ops (§2.3).
4. Entities in slot order, then worm beams (`DrawEntities`).
5. Projectiles in pool slot order (`CClient_Draw.cpp:1419-1424`).
6. Bonuses (`:2178-2191`).
7. CTF flags (unused).
8. Alive worms in id order. Each worm draws its rope, popups, crosshair, skin,
   muzzle and names (§3).
9. Gusanos objects (none for LX56).

After all viewports are drawn:

- the split seams;
- the minimap;
- chat and kill lines;
- the HUD bars per viewport (drawn inside `DrawViewport` after the game layer);
- FPS and ping;
- weapon selection, overlaid with a `Color(0,0,0,100)` dim
  (`CClient_Draw.cpp:843-869`);
- scoreboard, menus and console.

---

## Assets the remake must import

Paths are relative to `share/gamedir/`. `data/gfx` and `data/sounds` are already
listed under `shared` in `content/olx-manifest.ron`.

| Asset | Size / layout | Used for |
|---|---|---|
| `data/gfx/crosshair.png` | 12×6, 2 frames 6×6 (red, green) | crosshair §3.3 |
| `data/gfx/muzzle.png` | 224×16, 14 frames 16×16 at **1×** | muzzle flash §3.6 |
| `data/gfx/explosion.png` | 512×32, 16 frames 32×32 | `ENT_EXPLOSION` |
| `data/gfx/smoke.png` | 70×14, 5 frames 14×14 | `ENT_SMOKE` |
| `data/gfx/chemsmoke.png` | 40×10, 4 frames 10×10 | `ENT_CHEMSMOKE` |
| `data/gfx/spawn.png` | 160×32, 5 frames 32×32 | `ENT_SPAWN` |
| `data/gfx/sparkle.png` | 30×10, 3 frames 10×10 | `ENT_SPARKLE` |
| `data/gfx/giblets.png` | 32×12: 4 flying frames 8×8, then 4×4 map stamps at y = 8 | gibs, colorized per worm |
| `data/gfx/hook.png` | 6×6 | rope hook |
| `data/gfx/bonus.png` | 12×8 | weapon bonus |
| `data/gfx/health.png` | 10×10 | health bonus |
| `data/gfx/greenball.png` | 8×9 | green dirt; already imported into theme packs |
| `data/gfx/infinite.png`, `font.png`, `out_font.png` | | HUD reference: lives ∞, normal and outline fonts |
| `data/sounds/throw.ogg`, `pickup.ogg`, `bump.ogg`, `death1.ogg`, `death2.ogg`, `death3.ogg` | | game sounds; `rl-sim::GAME_SOUNDS` names them `.wav`, so map them to `.ogg` |
| `data/sounds/teamscore.ogg`, `data/sounds/game/*.ogg` | | optional: teams, announcer |
| `data/sounds/<name>` | | fallback for mod sound names, such as Classic's weapon sounds |
| `data/themes/<Theme>/FrontTile.png` | | `PlaceDirt` colours; the Rust `Theme` already has `front_tile` |
| `data/themes/<Theme>/BackTile.png`, `hole1..5.png` | | tiling and hole stamps |
| `skins/*.png` | 672×36 | worm skins and masks; `skins/default.png` is the fallback |
| `data/frontend/colours.cfg` | | `RopeColour1/2` `#A05000`/`#C86400`, `LaserSightColour1/2` `#BE0000`/`#A00000`, `PlayerName` `#FFFFFF`, `ViewportSplit` `#000000`, team and HUD label colours |
| `data/frontend/worm_health.png` | 26×13 | optional over-worm health bar |
| `data/frontend/frontend.cfg` | | minimap size 119×94; HUD reference positions |
| `data/frontend/healthbar1/2.png`, `weaponbar1/2.png`, `clock.png`, `team_*.png` | | HUD reference only |
| `<mod>/gfx/*.png` | | projectile images (already in mod packs) |
| `<mod>/sfx/*` | | weapon and action sounds |

Not needed:

- `data/gfx/worm.png`, `worm2.png` (legacy, unused by the skin system);
- `doomsday.png` (`ENT_DOOMSDAY` uses fixed colours; the file is a mod
  projectile image);
- `gameover.png`, `scoreboard.png` (HUD, only if the remake wants them).

## Open questions

1. **Explosion shake is not reported.** `Event::Explosion { pos, damage }` lacks
   OLX's `shake` argument: the action's `Shake`, or 5 for bonus chain and beam
   bonus explosions. Add `shake: i32` to the event (`rl-sim::effects`,
   `actions.rs:112-148` already tracks `info.shake`).
   **Resolved** (spec §19 E6): `Explosion { pos, damage, shake }`.
2. **The gib's owner is not reported.** `CosmeticKind::Gib` has no worm, but each
   worm has its own colorized `giblets.png`. Gibs are emitted right after
   `Kill { victim }` at the victim's position, so the renderer can infer the
   owner. A `worm` field would be cleaner.
   **Resolved** (E6): `CosmeticKind::Gib { worm }`.
3. **The dirt-particle colour source is not reported.** The three `Dirt` sources
   (dig, explosion, rope hook) probe different pixels (§7.3). The renderer
   cannot tell them apart from `Dirt { pos, vel }`. Add the probe pixel or the
   colour to the event.
   **Resolved** (E6): `CosmeticKind::Dirt { probe }`, the pixel each source reads.
4. **Sub-step boundaries are not reported.** OLX runs entities per sub-step and
   spawns them mid-tick. Beams and laser sights live 2 sub-steps; dropper drips
   every 10. Events carry no sub-step index, and `World` does not expose how many
   sub-steps a tick ran. Either expose that, or let the renderer run its own
   11-ms accumulator against 10-ms ticks, accepting that spawns land on tick
   boundaries.
   **Resolved** (E6): `Event::substep` counts the entity passes before the
   event, and `World::substeps_last_tick()` the tick's sub-steps.
5. **The `RotIncrement` 0→1 mutation is not kept.** The renderer needs to know
   whether the mutation has happened for a projectile type (§5).
   - The approximation is: treat `RotIncrement == 0` as 1 once any projectile of
     that type has been spawned by a `UseRandomRot` spawn info.
   - Alternatively the sim could expose a per-type flag.
   - OLX keeps the mutation across games while the mod stays cached.
   **Resolved** (E6): the sim exposes `World::rot_bumped(type)`, per match.
6. **Muzzle and `WeaponFired`.** `WeaponFired` is emitted for beams too, but OLX
   shows no muzzle and plays no sound for beams. The renderer must check the
   weapon type. OLX shows the muzzle for one drawn frame, in the first viewport
   only. For split screen, decide whether to keep that quirk.
7. **Damage popups need the real damage.** They sum `min(health_before, damage)`
   per attacker. `Event::Damage` has the damage after factors, and `World` has
   only the health after. Keep the previous tick's health in the renderer, or add
   the clamped amount to the event.
   **Resolved** (E6): `Damage { .., dealt }`.
8. **Shadows.** This OLX build draws none (§2.3). Classic LX/OLX had map and
   object shadows (`SHADOW_DROP` 3, black α 96). Decide "faithful to
   `d1ad953f`" versus "classic look".
9. **Antialiasing.** `Game.Antialiasing` is on by default: antialiased rope and
   beam lines, resampled minimap, and smoothed final upscaling. At world
   resolution the non-antialiased rope (alternating per world pixel) and beam
   (solid 1 px) are exact. Pick one and record it in `STATUS.md`.
10. **Positional sounds for global ones.** The sim gives `THROW` and `PICKUP`
    sounds positions, but OLX plays them globally at gain 1.0. Should the remake
    keep that, or make them positional?
11. **Keys missing from the sim.** Prev/Next weapon, Weapon1–5 and Strafe exist
    in OLX's defaults but not in the sim's 8 keys. Leave them out (classic LX
    feel), or extend `Keys`, which is a sim change.
12. **Laser sight wraps at the map edge.** The trace reads material with
    wrap-around even on finite maps (`getMaterialWrapped`). At a map edge the
    line can run past the border until wrapped material stops it, up to
    9999 px. Keep this?
13. **Large explosions start invisible.** For `size > 15`, explosions are
    invisible for `(size−15)/0.4` sub-steps (§7.3). That is faithful, but
    confirm the remake wants it.
14. **The 1024-entity cap.** The real client's 1024-slot pool drops new
    cosmetics when full, for example during heavy blood. The renderer can keep
    the cap (faithful) or raise it.
15. **Fork layout or classic layout.** This fork's camera and HUD layout (full
    480-px-high viewports, a transparent top bar, widescreen local games, 4-way
    split) differs from classic OLX 0.58, where the viewports were 382 px high
    above an opaque bottom HUD (`CClient.cpp:1585-1589`, still computed but
    overridden). The remake's 240-world-px height matches the fork.
