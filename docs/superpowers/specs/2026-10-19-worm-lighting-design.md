# Worms in the light — design

**Status:** implemented, 2026-10-19 (plan 15b; as built and its rulings:
`docs/porting/STATUS.md`, "Worms in the light (plan 15b)"). Follows the look system
(`2026-10-18-look-system-design.md`, plan 15a), whose terms this uses:
the light grid, placed lights, glow light, engine lights, the solid floor,
shade (self-light, marker, solid floor).

## Why

In a lit match the worms look flat. Today a worm takes the light at each
of its pixels, but never shows below a self-light floor of 50 % neutral
white (`olr_look::engine::FLOOR`). On a dark level that floor wins almost
everywhere: the worm is a half-bright grey cut-out that ignores the level's
colour, the lamp beside it and the shadow it stands in. Worms also cast no
shadows: only rock and dirt stop light.

The user wants worms to belong to the light: a lower floor, the side
facing a light lit, and worms blocking light as they pass.

## Goals

1. **A lower floor that takes the level's colour.** A worm never shows
   below 35 % of the level's ambient hue at full brightness: the ambient
   colour scaled until its brightest channel is full, then taken at 35 %
   (a white ambient gives 35 % white; `#203048`, a dark blue, gives 35 % of
   `#6ea5ff`). The ambient's own intensity does not dim it. The user asked
   for "a bit lower"; 35 % is a starting value they expect to retune.
2. **The lit side.** The side of a worm facing a light is lit by it, the
   far side falls towards the floor. It holds for every light: placed
   lights, glow light, engine lights (explosions, muzzle flashes, rockets).
   A lamp above lights a worm's back; a torch on its left, its left flank.
3. **Worms block light.** A living worm stops light as rock does: it
   throws a shadow on the walls, the ground and other worms behind it,
   from placed lights, engine lights and glow light. The shadow takes the
   level's shadow edges (hard, smooth or soft), as rock's does.

## Non-goals

- Projectiles, bonuses, gibs, blood, the rope and the crosshair do not
  block light (a call: a cloud of shots throwing shadows reads as noise).
- No per-player setting (spec rule: no per-player visual settings that
  change information).
- No change to the simulation, its traces, replays or the net protocol.
- A lit Castle Strike: separate work, after this.

## Anti-goals

- **A worm that can hide.** No shadow, darkness or facing may take a worm
  below its floor: the floor is the fairness guarantee and wins over
  everything here, including another worm's shadow.
- **A smooth, airbrushed worm.** The shading is pixel-exact on the worm's
  art (each art pixel one shade), never a gradient finer than the art: it
  keeps the Liero look.
- **Shading that jitters.** A worm standing still under a steady light
  does not flicker; a worm moving by one pixel changes its shading by at
  most that pixel's worth.
- **Levels without a look change.** None of this draws without a look.

## What is decided

- **Floor:** worms, with their rope and hook (drawn as part of the worm
  today; a call). Projectiles and bonuses keep the engine floor of 50 %
  white (a call: incoming fire stays easy to see). The crosshair and
  health bars stay markers. A black ambient (no hue) gives a white floor;
  a hue whose 35 % would be darker than 35 % white's luminance by more
  than half is raised to that half (a call: a near-black hue never makes a
  worm vanish).
- **A light inside a worm ignores that worm** (a call): the muzzle flash
  (at the shooter's centre) and an explosion centred in a worm light that
  worm all round and cast no shadow from it; every other worm blocks them.
  A light inside rock still lights nothing, as today.
- **Facing, the design's approach (a call; the builder may choose
  another that meets the goals):** each worm pixel is lit by the light just
  outside the worm on that pixel's side. The worm's outward direction at a
  pixel comes from its silhouette (the drawn frame's opaque pixels): a
  pixel on the worm's left edge faces left and takes the light found a
  pixel or two left of the worm, a pixel inside takes a blend of its
  neighbours'. With worms blocking light (goal 3), the light on the far
  side is the worm's own shadow, so facing and shadows agree by
  construction. The worm's interior gets no light of its own.
- **Blocking:** each frame, the living worms' silhouettes (their drawn
  body frames, at world pixels) join rock and dirt as occluders for the
  placed and engine lights' shadow maps and for the glow light's rays.
  Glow light is blocked at its own grid's resolution (a cell of 1 world
  pixel on most levels, coarser on very large ones): a worm on a huge
  level throws a coarser glow shadow.
- **Not solid for the soak or the solid floor:** a worm is an occluder
  only; the soak and the solid floor stay the terrain's.
- **Order of work:** the floor first (smallest, retunable at once), then
  blocking, then facing.
- **Goldens:** the oracle and the match goldens stay byte-identical. The
  look render goldens are re-blessed with a reason. The unlit render
  goldens do not move.
- **Measurement:** the cost of the extra occluder pass and the facing
  lookups is measured with the look's GPU bench, with the user's consent
  (they share the PC).

## What is left to the builder

- How the silhouettes reach the GPU (a per-frame occluder layer beside
  the solid map, or worm stamps into a copy of it), within the look
  system's GL limits (no R32Float or Rg32Float render targets).
- The outward-direction rule's exact form, how far outside the worm the
  light is read, and how a worm touching rock is handled, within the
  anti-goals.
- Whether the floor's ambient hue is the level's ambient as set or as
  graded.
- Tests: GPU tests in the look system's style (a worm between a lamp and a
  wall shadows the wall; its lamp side is brighter than its far side; a
  worm in another's shadow stays at its floor; a steady scene draws the
  same bytes twice), and the unit tests of the floor colour.
