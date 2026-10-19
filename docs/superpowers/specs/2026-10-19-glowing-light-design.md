# Glowing light — design

**Status:** a draft for the user's approval, 2026-10-19. It follows the
brainstorm `2026-10-19-lighting-evolution-brainstorm.md`. It changes the
look system (`2026-10-18-look-system-design.md`, plan 15a) and builds on
plan 15b (`2026-10-19-worm-lighting-design.md`).

## Why

The user wants lighting that gives this old game a "wow" effect, the
feeling players had when ray tracing came to 3D games: light that behaves
physically and reacts. That means:

- coloured light spilling and bouncing off walls;
- soft shadows that grow from the light's shape;
- flames that make the rock breathe;
- daylight pouring through openings into rooms.

Today's model falls short of that. Lamps are invisible point lights
with shadow maps. A small source throws long, razor-edged shadow
columns, worms included. Light never bounces, and a single direction
(the moon) looks unnatural. The user said what to avoid: no hard shadow
columns.

The light the user pictures comes from glowing things, not from
invisible lamps:

- screens that glow softly and flicker a little;
- candles and torches with an unsteady, shaky flicker;
- lamps built from pixels. A glowing bulb inside a rock housing lights
  only out of its opening: the direction comes from the shape the artist
  drew.

## Goals

1. **Glowing pixels are the only light sources** of a level: glow groups,
   painted with the editor's tools. The level's point lights (light
   objects) go, from the format, the editor and the CLI. The ambient
   stays as a base.
2. **Soft shadows from shapes.** Every shadow comes out of the glow
   light's tracing. Its softness follows the size of the glowing shape
   and the distance. No light anywhere is drawn with a single-ray shadow
   map, so nothing throws hard shadow columns.
3. **Bounce.** Light reflects off rock and dirt, tinted by their colour,
   so an opening lets daylight into a room and a torch's warmth spreads
   along a tunnel and round corners. Bounces may build up over a few
   frames (the user accepted this as an experiment, to change if it does
   not look right).
4. **Dynamic light in play.**
   - Explosions, muzzle flashes and shots with a light light their
     surroundings through the same model, as glowing discs (see "My
     calls").
   - Worms block it all, as they block glow light today (plan 15b).
   - A shot's light, end flash and muzzle light in a mod's look become
     emission.
5. **Independent flicker.** Each separately drawn flame or screen
   flickers on its own, without the author making a glow group per
   torch.
6. **Glare.** Bright light reads as bright: bloom on glowing pixels and
   strong light, and tone mapping instead of a hard clip. A glowing
   screen or a torch's flame should feel luminous.
7. **The showcase: Castle Strike by day.** A new playtest level,
   `content/playtest/olr-level-castlestrike-day`, shows all three kinds of
   space in one match (the user's choice, day, to stress the contrast):
   - **outside**, bright under a glowing daylight sky;
   - **the hall, the library loft, the towers and the gatehouse** (open
     air), lit by daylight coming in through their openings and bouncing
     off their walls;
   - **the caverns** at the bottom, dark, carried by flickering torches
     and candles made of glowing pixels.
8. **A bot that does nothing.** A match option, in the menus and on the
   command line, for bots that stand idle (no movement, no fire). The
   user can then explore a level freely, with worms around to see light
   on.

## Non-goals

- **A lighter version for weaker GPUs.** None is planned (the user,
  2026-10-19). The look is judged on a desktop GPU first. Performance is
  measured afterwards on Windows at 1080p, on a representative match (not
  the heavy 32-bot stress replay), with the user's consent. Lighter
  options come only from what that shows.
- **Quality settings.** Every player sees the same frame (spec rule: no
  per-player visual settings that change information).
- **Hard shadows** as a style. The `shadows` setting (hard, smooth, soft)
  goes with the point lights.
- **The night Castle Strike's conversion.** It loses its light objects
  (the format drops them). Its glowing sky and flames stay, and it is
  touched only as far as it must load and check.
- **The simulation:** untouched. Light is presentation only.

## Anti-goals

- **Hard shadow columns.** Any long, sharp-edged wedge of shadow from a
  small source is a failure.
- **A worm that can hide.** The worm floor (35 % of the ambient hue) and
  the lit side of worms (plan 15b) stay. No darkness, shadow or bounce
  takes a worm below its floor.
- **Shimmering or crawling light.** A still scene with steady glow draws
  the same bytes frame after frame once settled. Flicker comes only from
  animations, never from noise in the algorithm.
- **Light through rock.** Light never passes opaque rock or dirt, however
  thin (the user's request, 2026-10-19, `16222315`). That holds for
  bounced light too.
- **Light that depends on the camera.** Moving the view does not change
  the light of what stays in view, as `light_is_camera_independent`
  holds today.

## What is decided

- **One light model:** the glow light's radiance cascades (`gi.rs`,
  `gi_*.wgsl`), extended. The placed lights' shadow maps (`shadow.wgsl`)
  and the per-light sum (`light.wgsl`'s `placed`) go. The light grid is
  then the ambient plus the glow light's fluence (plus whatever bounce
  adds), soaked into solids, blurred (no blur through walls), capped and
  graded, as now.
- **Emitters:**
  - **The level's glowing pixels.** Glow groups, as baked today.
  - **Dynamic light.** Each lit frame, play's lights are drawn into the
    emission as glowing discs: the engine's explosion and muzzle lights,
    and a mod look's shot light, end flash and muzzle light (see "My
    calls"). Worms are occluders only, as now.
- **Bounce (the brainstorm's direction A):** a ray that hits a solid cell
  takes, beside that cell's own emission, the cell's albedo times the
  light that reached it in the last frame. Albedo is the cell's art
  colour, averaged and linear. Its strength is capped so the feedback
  always converges.
- **The level format:** `LevelObject::Light` is removed, and so are
  `look.shadows` and every placed-light field. A file that still has them
  is an error naming the field: compatibility breaks are fine at this
  stage (the user's standing rule). The night Castle Strike and the look
  test fixtures are updated to match.
- **The editor:** the Light tool (K) and light objects go. The Look panel
  keeps the ambient, the groups and the light settings that still apply.
  The Glow brush and Glow selection stay as the way to make light.
- **The CLI:** `olr-cli level light` goes. `look-preview` stays, and so
  does the lit thumbnail (`level_thumbnail`), following the new model:
  glow and ambient, no point lights.
- **The idle bot:** a bot behaviour that presses nothing. In the menus
  it is chosen where the other bot settings are, and on the command line
  with `--personality idle`.
- **Measurement:** the desktop GPU judges the look first. On Windows at
  1080p, a representative lit match on Castle Strike by day is
  benchmarked, with the user's consent, before any performance work.
- **Goldens:** the look goldens and `look-rocket.png` are re-blessed with
  a reason. The unlit goldens, the oracle and the match goldens do not
  move. An idle bot in a golden match would be a new golden, not a
  changed one.

## My calls (the builder check's questions; the user may overrule)

- **Which play items emit light.** Only lights do; a shade's self-light
  stays what it is today, visibility in the dark, and casts nothing. So
  every shot or bonus with the visibility floor emits nothing, and the
  explosion and muzzle sprites add no light of their own (it would count
  twice). These emit:
  - the engine's default lights (`engine::explosion_light`, growing with
    the explosion's size, and `engine::muzzle_light`), in every lit match
    as today;
  - a mod look's shot light, end flash and muzzle light.

  Each becomes a glowing disc in the emission at its position, with its
  colour, animation and clock as today.
- **Disc size and brightness.**
  - **Size.** No new format field. An explosion's disc grows with its
    size. A muzzle flash, shot light or end flash gets a radius derived
    from its reach, at least the glow grid's cell (about 1/8 of the
    reach, from 1 to 6 world pixels; the builder may tune it). A visible,
    soft source is the point.
  - **Brightness.** Each kind is calibrated so its light at about a
    third of its reach in the open matches today's point light. Authors'
    values keep roughly their meaning. The fixtures and the rocket golden
    are re-blessed after.
- **Tone mapping and `max_light`.** The art's colours stay exactly as
  today wherever the light is at most 1: the curve is the identity up to
  1 and compresses only the light above it, up to `max_light`, its white
  point (100–200, kept in the format). The curve applies to the light
  grid, before light × art. Bloom comes from the light above 1, and from
  glowing pixels' own brightness, added after the composite. A lit level
  that never exceeds 1 then looks as it does now, apart from its new
  bounce.
- **Independent flicker.** Automatic for `Flicker` animations: each
  separately drawn region of a group's pixels (4-connected) flickers with
  its own phase. `Pulse` and `Keys` stay in step across the group, so a
  row of signs pulsing together or a keyed effect stays as authored. A
  `Steady` group, such as a sky, is unaffected. No format change.

## What is left to the builder

- **Resolution and cost.** The glow light's cell budget
  (`GI_CELL_BUDGET`, now 262 144: Castle Strike gets cells of 2 world
  pixels). Raise it, or cascade fine near the view and coarse over the
  level (the brainstorm's C), if small flames need finer cells. Light
  must stay camera-independent.
- **Bounce detail.** How the bounce is stored and fed back (per cell, per
  cascade 0 probe), its strength cap, and whether it runs at a coarser
  resolution than direct light (bounced light is soft by nature).
- **Dynamic emission.** How dynamic emitters reach the emission pass: a
  per-frame emission layer drawn like the occluder map, or stamps into
  it.
- **Independent flicker's mechanism.** How a region gets its phase: a
  region id baked with the glow, or another way. Within the anti-goals:
  no seams inside one flame.
- **Bloom's details.** Its size and strength, within the calls above. The
  grade stays the level's own tool, after it.
- **Daylight.** How a daylight sky reads: a glow group on the sky with
  unlimited reach, a sky colour, a strength. Possibly a new kind of glow
  group if the sky needs to light without looking blown out. The night
  Castle Strike's sky layer shows the approach.
- **The day showcase's content:** which pixels glow, the groups and the
  flicker. It is built in our tools (`olr-cli level …` or the editor),
  checked by `olr-cli`'s `tests/playtest.rs`, and judged by renders shown
  to the user.
- **Tests.** In the look system's GPU-test style:
  - an opening lets light into a room;
  - a lamp housing gives directed light;
  - bounce tints a neighbouring wall and settles;
  - no light past a one-pixel wall, bounced light included;
  - steady bytes once settled;
  - independent flicker of two flames of one group;
  - a worm's soft shadow (no sharp column);
  - the worm floor holds;
  - the idle bot presses nothing.
- **Order of work.** The user suggested none. Something like: the
  emitters and bounce on today's grid first, then removing point lights,
  then glare and the showcase.
