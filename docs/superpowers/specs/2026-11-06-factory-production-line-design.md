# The factory's production line

**Date:** 2026-11-06. **Status:** design, for approval.

## Intent

Liero Factory Reloaded is a Liero 1.0 weapons factory, and its Dirt-ball
line should really work. Dirt is dug in the mine, carried up by the crane
into a reservoir, fed by a moving belt under the press, and pressed into
balls that roll into the crate. The loose dirt design
(`2026-11-05-loose-dirt-design.md`) foresaw it: "later … a crane that
scoops real dirt from the mine and a press that consumes the dirt under
it".

The maintainer's rule for the whole line: **the dirt is real**. Each
grain is a dirt pixel that keeps existing through the whole transport and
unload, moved only by the engine's ordinary physics for loose dirt and
solid parts. There are no special hacks and no script that teleports,
counts out or redraws dirt. Scripts only drive the machines (the crane's
moves, the jaws, the belt's run and pause, the press). The physics moves
the dirt.

Two playtest findings on 0.3.0-dev.275 come with it:

- Falling dirt keeps its heat: a hot grain that falls or slides stays hot
  where it lands.
- Today a moving rock part crushes all the dirt it moves into (commit
  `0b1105f6`). This design narrows that so the crane can carry dirt.

## One shift (the maintainer's, confirmed on the companion screen)

1. **The mine.** A pile of loose dirt lies on the mine tunnel's floor
   under the hole in the hall's floor. It holds enough for several crane
   rounds. The mine is reshaped slightly for this.
2. **The crane** moves over the hole, lowers its claw down the shaft,
   closes on a handful of dirt and rises.
3. It carries the handful right, over the reservoir.
4. It opens. The handful falls into the **reservoir** as loose dirt and
   settles. At the start of the match the reservoir already holds enough
   dirt for a few press runs. Its shape is adjusted slightly.
5. **The belt** runs for a little while. The reservoir's dirt pours onto
   it, and the belt carries it right, under the press.
6. The belt pauses. **The press** comes down, crushes the dirt under it
   and rises again. Then the belt runs again. Rinse and repeat.
7. **A ball** leaves the press only if that stroke crushed some dirt. It
   rolls along the belt, through the drive and up the incline into the
   crate, as now. No dirt crushed, no ball.

At the start of the match no dirt lies on the belt right of the press.

## The physics this needs (approved on the companion screen)

### Moving parts push and carry loose grains

A moving live rock part now treats a **loose** grain much as it treats a
worm:

- **Pushed.** A grain in the part's new footprint moves one pixel the
  part's way when that pixel is free (inside the level, empty, not under a
  part).
- **Carried.** A grain resting on the part (the pixel under it is in the
  part's old footprint) moves with the part when its new place is free.
  Grains stacked on carried grains ride too, so a bowl of grains rises as
  one.
- **Crushed.** A grain with nowhere to go is crushed: it becomes
  background, as today.

**Plain (not loose) dirt is still crushed**, as `0b1105f6` does. The
press therefore still destroys the loose dirt under it: the belt's rock
below leaves it nowhere to go.

Every grain a part pushes, carries or crushes is reported in order with
the tick's other terrain edits, so the colour map (and the heat, below)
follows it exactly. *(My call, at planning: as edit events, not in
`World::loose_moves`, which the colour map replays after the tick's
edits: the live step runs before the tick's explosions, and a pushed
grain a blast then carved would come back in the picture.)* Crushed
grains stay `TerrainOp::Crush` edits. Dirt still never stops a part.

### Worms and the claw

The claw is solid rock and keeps the engine's rules for worms (the
maintainer's): it pushes a worm out of its way where there is room, and
when it pins a worm against dirt or rock the worm is wedged and slowly
crushed (`CRUSH_CHUNK` 5 every `CRUSH_EVERY` 10 ticks), with no `hurt` of
the crane's own. This replaces issue #97's ruling that the crane as drawn
must not crush.

### The crane's grab

- The claw's two jaws become **two rock parts** that close inward and
  open outward (offsets, the asset's own values). Closed, their tips meet
  to form a bowl that holds grains.
- One round: the claw moves over the hole with its jaws open, lowers
  down the shaft and across the tunnel into the pile, closes its jaws
  (squeezing grains into the bowl; those with no room are crushed),
  rises, moves right over the reservoir, opens its jaws (the grains fall),
  and goes back.
- Players see the handful in its jaws the whole way: real grains resting
  on the jaws.
- The claw lowers to a fixed depth each round, just above the tunnel's
  rock. As the pile shrinks the bites get smaller; on an empty pile the
  crane still cycles and carries nothing.
- **The cable pays out** (the maintainer's pick, after the builder check
  found the first idea unworkable: the ceiling over the rail is about 16
  px of rock with the upper deck's room above it, so a full-length
  cable drawn behind it would stick up into that room). One continuous
  cable picture, about 190 px, moves with the claw and is **drawn only
  inside its copy's rectangle**, so more of it shows below the trolley
  the lower the claw goes, like a real winch. This needs a small new
  engine feature, general to any asset: a part (here a background one)
  clipped to its copy's rectangle when drawn. The crane's box grows down
  to the claw's lowest place.

### The belt carries what rests on it

A running belt carries what rests on its top at its speed, though its
rock never moves:

- **Worms** standing on it ride along (the maintainer's), as a standing
  worm rides a moving part.
- **Loose grains** resting on it slide along.
- **Bonus boxes** resting on it ride too. *(My guess, approved.)*

The belt's speed is a value of the Dirt-ball line copy that its script
sets (run: the belt's speed, pause: 0). A conveyor value is a new kind of
asset value usable by any asset, not a factory special. A carried thing
stops where its way is blocked (rock, a part, a grain that cannot move).

### The press counts what it crushed

The line's script learns how much dirt each stroke crushed. It sends a
ball out only when that is more than 0 (the maintainer's: "in case there
was no dirt destroyed by its press"). Any amount above 0 gives one ball.

### Heat follows the grains

Every grain move, whoever makes it (falling, sliding, pushed, carried,
the belt), takes the grain's temperature with it: the heat texture's
value moves from the old pixel to the new one, and the old pixel is
cold. A crushed grain's heat is gone. *(My call, from the builder check:
the heat takes a pixel's material from the level's fixed surface maps,
and where a grain lands was usually open air, whose map names no heating
material, so a grain moving only its temperature would lose it there. A
moved grain therefore carries its surface material too: the front
material, its heat and its soak travel with it, bounded per frame.)*
Today a grain that moves starts
cold at its new place and its old place goes cold
(`olr-render`'s layer reset, issue #67), which this replaces for moved
grains. This is presentation: the simulation and its goldens do not see
it.

## The line's rhythm

- *(My call.)* The belt runs during the press's countdown (its seven
  LEDs), pauses when all seven are lit, the die drops, crushes, rises,
  and the belt starts again with the next countdown. That is "run for a
  little while, pause, press, return up, unpause".
- *(My call.)* The belt's speed stays close to the balls' (`BELT()` 12
  px/s), so a countdown's run (about 2.8 s, about 34 px) brings roughly
  the die's width (33 px) of fresh dirt under it. The numbers are
  tunable.
- The crane runs its rounds on its own pace, independent of the press.
- The shift, the alarm and the blackout keep their orders. `Stopped`
  stops the belt as it stops the line; `Off` stops the crane where it is,
  its jaws as they are, any grains it holds still resting in them.

## The level

- **The mine.** A loose pile (the level's `spoil`, repose 35, or a
  second material if the bite needs another angle) on the tunnel floor
  under the shaft, sized for several rounds. *(My call: 5 to 8 rounds of
  a full bite.)* It must stay put in an empty match, as the spoil does
  today, until the claw bites it.
- **The reservoir.** The press heap (`SPOIL`'s "Press heap" box, x
  820-910, y 320-370) becomes a reservoir whose dirt pours onto the
  belt's left end. Its shape is adjusted slightly. *(My call: rock walls
  where it needs them, open at its foot onto the belt.)* It starts with
  dirt for a few press runs, *(my call: 3 to 5 runs without the
  crane)*, and grains the crane drops land in it.
- **The belt right of the press** starts clean: the plain dirt there
  today (rows 346-350) is removed from the ground. The dirt left of the
  die and under it becomes the reservoir's loose dirt, or is removed.
- *(My call, from the builder check.)* The stretch of belt that
  carries dirt runs from the reservoir to under the die and ends there,
  a few pixels short of the die's right edge: carried dirt stops under
  the die, where the next stroke crushes it, so the belt right of the
  press stays clean (a run about the die's width brings a ragged front
  past it otherwise, piling at the drive's casing). Right of the die
  the belt still carries worms (and bonus boxes) toward the drive; dirt
  thrown there by a blast rides it and stops at the casing.
- The belt's top must be part of the Dirt-ball line copy (a rock layer)
  for its value to drive it. Today the stripe is a status light on the
  ground's rock.
- All of it is made by `tools/levels/factory_works.py` and
  `factory_live.py`, which must still reproduce the packs byte for byte.

## Goals

1. On Liero Factory Reloaded, an empty match (no worms) runs the whole
   loop by itself. The crane carries dirt from the mine to the
   reservoir, the belt brings it under the press, and balls reach the
   crate only after strokes that crushed dirt.
2. The handful the crane carries is real loose dirt from bite to drop:
   the number of loose grains in the level falls only by what the claw
   and the press crush.
3. A worm on the running belt rides it; a worm under the claw is pushed,
   or slowly crushed against dirt or rock.
4. A moved grain keeps its heat.
5. The new physics is general: any level's parts carry loose grains, and
   any asset may have a conveyor value.

## Non-goals

- Carrying plain dirt. It stays crushed.
- A belt that carries projectiles, blood or other effects.
- Anything new in the crate: balls still vanish there.
- Bots that understand the line. They play the level as they do now.

## Anti-goals

- Dirt that a script makes, counts, deletes or moves. If the claw's bite
  looks wrong, the fix is the physics or the jaws' shape, never a script
  shortcut.
- A belt or crane that desyncs: everything here is the simulation's,
  deterministic, rolled back and transferred like the rest.
- Breaking the levels without loose dirt or parts: their goldens and the
  oracle stay as they are.

## Constraints

- Simulation crates stay `#![no_std]` and build for wasm32. The live step
  allocates nothing once warm (`tests/no_alloc.rs`).
- A change to the state transfer bumps `olr_proto::PROTOCOL_VERSION`.
- Content is untrusted: a conveyor value is bounded like any value, and a
  hostile level must not make the push or carry step unbounded.
- Rule 4 (cite OLX) does not apply: nothing here is ported. Record the
  deliberate changes (push and carry of loose grains, the conveyor, the
  narrowed crush rule) in `docs/porting/STATUS.md`.
- Match, live and render goldens of the factory and the live-test level
  will change. They are re-blessed with reasons.

## Left to the builder

- How the push and carry rules order grains (far side first, so a column
  moves as one), and how a grain pushed by two parts in one step is
  settled.
- The jaws' exact pictures and travel, and the crane's script (states,
  speeds, waits), within the round above.
- The conveyor value's name, units and range, which part's top it moves
  things on, and how the loose step and the worm physics read it.
- How the press script hears its crushed count (a handler, a getter, an
  event), and the heat move's GPU form, with its bound per frame (past
  the bound a moved grain may start cold).
- The exact sizes of the pile, the reservoir and the belt's run, tuned so
  the empty-match loop works and reads well in a playtest.
