# Directional glow: crisp beams from glowing pixels

Date: 2026-10-25. Status: design, the maintainer's direction agreed in
chat; the builder check's questions answered (the beam's strength by the
maintainer, the rest folded in).

## Intent

Liero Factory Reloaded has PCs in its offices and a truck in its yard.
The maintainer wants their light to behave as such light does:

- a PC seen from the side throws coloured light **away from its screen**,
  in a cone, and none back onto its own body;
- the truck's headlights throw a cone of light ahead, onto the
  background and the tunnel entrance in front of it. Where nothing is
  there to light, the cone still shows as light in the air.

Today a glowing pixel lights all around it, the same in every direction.
So the side PCs light their own screen and desk more than the room, and
the headlights light only the truck.

A throwaway spike (2026-10-25, worktree `/home/martin/olr-spike-light`,
branch `spike/light-blockers`, shader changes in
`crates/olr-render/src/spike.rs` and the `look/*.wgsl` diffs) rendered
the options from real frames of the factory:

- **Light-blocking pixels** (glow-impenetrable rock) were tried and
  dropped. The maintainer: "if we have directional glow, we do not need
  blockers … it is one or the other". Narrow cones did the blockers' job.
- **A cone on the light pass alone** (the radiance cascades weighted by a
  cone) puts no light behind the screen. But that light is soft, cells
  of about 2 world pixels, and it reads as mist.
- **Haze that scatters the glow light** looked like mist too, and washed
  out the sky. Restricted to chosen groups, it still looked like mist:
  "We need the light crisper in some way."
- **Analytic beams** were chosen.
  - A beam is a wedge computed per pixel from its source: crisp at the
    source and softer further out, fading with distance, and shadowed by
    rock.
  - It is **as wide as its source** at the start ("It's start cannot be
    a one pixel tip, but be as wide as its source").
  - It is **not an object placed in the level**. It comes from
    parametrizing the glow pixels to shine in one direction and angle.
- The last round tuned edges and shadows. The maintainer said "the edge
  of the beam could be smoother. As soon as it hit some edge, the
  remaining path was very edgy", and picked the **softest variant**:
  - soft shadows traced from 8 points across the source;
  - the softest edge;
  - the PCs at the lowest of the three strengths.

The maintainer: "We need to test how real implementation will look."
So the built feature is judged the same way: real frames of the factory
next to the spike's picked frames.

## The picked look (the reference)

The spike's frames on Liero Factory Reloaded with the scratchpad's
`lf-s2` changes (see "Content" below) are the reference the built
feature is judged against. The maintainer picked the third variant, "PCs 0.12, soft shadows,
softest edge": the bottom row of `p-pcs.png` and the right-hand column of
`p-zoom.png` and `p-truck.png`, in the session scratchpad's `spike/`
folder. The builder re-renders them from
the spike worktree if the scratchpad is gone.

**The beam** (`composite.wgsl`'s `beams_at` in the spike):

- **Geometry.**
  - The source is a segment across the beam: its centre `at`, half-width
    `half`, direction `dir`.
  - A point at distance `along` ahead and `side` across is inside when
    `|side| < half + along·tan(spread)`.
  - The edge is a smoothstep over a penumbra of `0.5 + along·0.03`
    world pixels.
- **Falloff.** `(1 − along/reach)³` over the group's reach.
- **Shadows.** 8 points spread evenly across the source, each traced
  back from the pixel through the solid map. The share of points the
  pixel sees scales the light.
- **Strength.**
  - PCs: 0.12 of the beam colour in linear light (normalised own
    colour).
  - Headlights: 0.8 of the group's tint `#fff0c8`.
  - Both groups are at intensity 1000, reach 56 for the PCs and 120 for
    the headlights.
- **Where it shows.** The beam is added over every open, in-level pixel
  that does not glow, plus the distant sky behind see-through back
  pixels. It is added after light × art, before the grade.
- **The cone on the light pass.** The same pixels' radiance-cascade
  light is weighted by the cone: smoothstep from `cos(spread + soft)`
  to `cos(spread)` of the angle between the ray and the direction. So
  no light falls behind the source.
- **The cones used:**
  - the PC at x 344 and the one at x 513 face right, 35°;
  - the PC at x 498 faces left, 35°;
  - the headlights face left and a little down, direction (−1, 0.25),
    15°.

  Spread is a half-angle. The blue floor's PC faces the player and
  stays undirected.

## Goals

1. **A glowing element can be directional.** Its glow takes an optional
   direction and spread. Without them it glows in every direction, as
   today, and a level without directional glow draws exactly as before.
2. **Each separate patch of a directional element's glowing pixels
   becomes one beam:**
   - its start lies on the patch's front face, as wide as the patch;
   - it points the element's direction and widens by its spread;
   - its colour and strength come from the patch's glow group;
   - it fades over the group's reach.

   Nothing is placed by hand.
3. **The beam looks like the reference:** a crisp start, an edge that
   softens with distance, and soft shadows wherever rock cuts it. A
   shadow edge never shows as a staircase.
4. **No light behind the source.** The directional pixels' light on the
   light pass is limited to the same cone, so a screen's body and a
   truck's cab are not lit by their own glow. No light-blocking pixels.
5. **Haze.**
   - A level's haze sets how strongly beams show as light in the air.
   - An optional painted haze map sets it area by area, for example a
     smoky hall with clear offices.
   - Haze makes the light of a cone with nothing in it show.
6. **The editor sets it.**
   - An element's glow gets a direction and a spread in its properties.
   - The look gets a haze strength.
   - A Haze brush paints the haze map.
   - The canvas shows beams as the game does.
7. **Liero Factory Reloaded** gets the factory changes held from this
   session (see "Content") with its side PCs and headlights directional,
   matching the reference.

## Non-goals

- **Light-blocking pixels.** Dropped for cones (the maintainer's
  ruling).
- **Haze for undirected light.** Scattered glow looked like mist (the
  spike). Haze affects beams only.
- **Directional distant layers.** A distant layer's glow stays
  undirected.
- **Directional play lights.** Explosions, muzzle flashes and mod lights
  are unchanged.
- **Quality settings** or a lighter path for weaker GPUs: every player
  sees the same frame. Cost is held by a cap (see "Cost") and judged on
  the Windows GPU at 1080p.
- **The simulation:** untouched. Light is presentation only.

## Anti-goals

- **Mist.** A beam whose edge you cannot see, or light that fogs a room
  evenly, is a failure. The beam must read as a shaft with an edge.
- **A one-pixel tip.** A beam narrower than its source at the source.
- **Stair-stepped shadows.** A beam cut by rock whose shadow edge shows
  the rock's pixel steps.
- **Light behind a directional source.**
- **Light through rock**, for beams too. A beam never passes rock or
  dirt that is not clear.
- **Shimmer.** A still scene with steady glow draws the same bytes each
  frame. No noise in the beam's sampling.
- **Camera dependence.** Moving the view does not change the light of
  what stays in view. The beam is a function of world position.
- **Changing levels that do not use it.** A lit level without
  directional glow and without haze keeps its exact bytes:
  `fixtures/render/lit-hashes.ron` and the render goldens do not move.

## What is decided

- **Directional glow is a setting of the element's glow.**
  `ElementGlow` gets an optional `beam` with:
  - `direction`: integer degrees, 0 to 359, 0 pointing right, clockwise
    on screen (y down), so 90 points down;
  - `spread`: the half-angle in integer degrees, 1 to 90.

  The left- and right-facing side PCs are two elements (*my call*:
  per-element, like the glow strength. A per-pixel painted direction is
  left out).
- **It is baked like the rest of the glow.** The bake writes the
  direction and spread of every directional glowing pixel into the
  level's look files, alongside `look/glow.png` and `look/groups.png`
  (front and back). That way a plain level (no composition) and the
  runtime see it, and it is part of the look's hash. The builder picks
  the file layout.
- **Beams are derived when the level loads:**
  - a beam is a connected patch (8-connected) of glowing pixels with the
    same group, direction and spread;
  - its face is the patch's furthest extent along the direction, its
    width the patch's extent across it.

  The derivation is pure and deterministic: patches in scan order, no
  hash maps. This is the spike's `beams_from_pixels.py`, done properly.
  The spike measured the face along a tilted direction and left a 1-px
  gap at the tilted headlights. The built derivation puts the beam's
  start flush with the patch.
- **Colour and strength come from the glow group:**
  - colour: `Own` uses the patch's mean art colour normalised to a full
    brightest channel, `Tint` the tint, `Filtered` the tint filtered by
    that mean;
  - strength: the element's glow strength (its mask included, averaged
    over the patch) times the group's intensity, times the group's
    animation (a flickering screen's beam flickers with it, at its
    patch's phase). There is no separate beam-strength setting: the
    maintainer's ruling (2026-10-25), accepting that a dimmer beam
    also dims the element's own shine and its light-pass light;
  - reach: the group's.

  The builder calibrates one scale from glow strength × intensity to
  the beam, so the headlights at glow strength 100 and intensity 1000
  give the reference's 0.8. The PCs' weaker beam (the reference's 0.12)
  then comes from a lower glow strength on their elements.
- **Reach.** A directional element's group must have a reach in pixels.
  `Unlimited` is refused with an error naming the element. *My call*:
  a beam needs a length to fade over.
- **Shadows** come from opaque rock and dirt (not clear ones) only.
  Worms cast no shadow in a beam: since issue #53 (the maintainer's
  call, 2026-10-23) worms take no part in any light
  (`worms_change_no_light` stays true). A worm in a beam is lit by it.
- **Directional glow only on pixels that cannot be dug.** A directional
  element's glowing pixels must be Rock or Background (a mixed layer's
  dirt pixels may not be directional), so beams derived at load never
  outlive their source. *My call*: re-deriving beams as dirt is dug
  is left for later, if a level ever wants it.
- **Haze**:
  - `look.haze` is percent, 0 to 100, default 0;
  - an optional painted haze map at world resolution (grey, 0 none to
    255 full) scales it per pixel; the editor saves it under `editor/`
    and the bake writes it into `look/`;
  - with no map, the haze is the level's everywhere.
- **What haze does is the builder's to settle by renders, within two
  rules:**
  1. With the factory's haze set as the builder proposes, the frames
     match the reference.
  2. At haze 0 a beam still lights what it reaches: the background
     wall, rock faces, worms. Over open sky it shows nothing.

  The spike added the beam over every open pixel. Splitting that into
  light on surfaces and light in the air is the open part (see "Left to
  the builder").
- **The editor:**
  - the element's Glow properties get "Direction" (a dial or a number)
    and "Spread";
  - the Look panel gets "Haze";
  - a Haze brush (beside the Glow brush) paints the haze map at the
    brush's strength;
  - the canvas preview renders beams.
  - The CLI's `olr-cli level element` gets the same settings, so
    agents can author it.
- **The thumbnail** (`level_thumbnail`'s CPU model) limits directional
  pixels' light to their cone and draws no beams or haze. *My call*:
  the thumbnail stays a cheap sketch.
- **Format docs.** New fields get doc comments (the schema docs'
  source), so `docs/formats/level.md` regenerates. Compatibility breaks
  are fine; this one needs none.

## Cost

- At most **64 beams** per level (*my call*, to be confirmed by
  measurement). A level with more is an error naming the count; the
  lenient reader plays it with its beams dropped and a warning.
- 8 shadow points per beam per pixel.
- The builder culls beams per pixel or per tile, so a pixel pays only
  for the beams whose wedges reach it.
- Measured with `cargo xtask profile` (exempt from asking) on the
  Windows GPU at 1080p, on the factory with all its beams in view. The
  cap and the shadow-point count may be tuned from that, and the result
  goes in `docs/porting/STATUS.md`.

## Content: Liero Factory Reloaded

The session scratchpad holds the level's changes, which waited for this
design:

- frost 1.0 px (`frost: 10`) on the glass;
- the PCs' screens glowing:
  - new Rock layer `rock-pcs`, group `pcs`;
  - the blue PC's pixels moved there from `rock-screens`;
- the truck's headlights glowing: `rock-headlights`, group `headlights`,
  Tint `#fff0c8`;
- the control room's lower consoles brighter: `screens` 450, reach 48;
- the upper screen split out: `control-screen`, group `control`, Tint
  `#d0e0ff`, 450, reach 48. It lights all around, blue-white.

In the editor's own terms they become:

- the side PCs in two directional elements: right-facing (x 344 and
  513) and left-facing (x 498), both 35°;
- the blue PC undirected;
- the headlights directional at about 166°, 15°;
- the side PCs' glow strength lowered so their beams match the
  reference's 0.12 against the headlights' 0.8 (about 15 %, with the
  intensities as in `lf-s2`). Their screens' own shine and their light
  on the walls drop with it, by the maintainer's ruling. The groups'
  intensities may be retuned so the screens still read well. The
  maintainer judges the renders;
- the haze set so the frames match the reference.

The builder rebuilds these changes in `content/reloaded/` through the
editor's or the CLI's operations (or the scratchpad's scripts, as a
guide to the pixels: `factory_lights.py`, `factory_pcs.py`). The
`lf-s2` copy in the scratchpad is the visual reference.

## Left to the builder

- **How a beam splits between surface light and air glow, and what
  haze scales.** For example, beam light × art on the near pixels, plus
  haze × beam added in the air and over the sky. Judged by renders of
  the factory shown to the maintainer next to the reference, before
  it's final. The anti-goals bind it.
- **Where the beam is drawn:** in the composite as in the spike, or in
  the light grid at full resolution. Bloom and tone mapping treat it
  like other light, without glare of its own unless the renders call
  for it.
- **Animation** of a beam: sampled each frame from its group, with the
  patch's flicker phase as the glow light has it.
- **The file layout** of the baked direction (and haze) maps, within
  the format's rules: hashed, read leniently when playing (a bad map
  plays without beams or haze, with a warning), checked strictly by
  `pack check`.
- **Culling and resource layout** of beams on the GPU, within the cap.
- **Tests** (GPU tests in the look system's style, unit tests for the
  derivation and the format):
  - a directional patch lights ahead and nothing behind;
  - the beam starts as wide as its patch, flush with it, tilted
    included;
  - a one-pixel wall stops a beam;
  - a shadow's edge is smooth (no staircase where a rock step cuts it);
  - steady bytes once settled;
  - camera independence;
  - haze 0 shows no beam over open sky, and haze > 0 does;
  - the haze map scales it;
  - a level without either keeps its frame hashes;
  - the derivation's deterministic order and its cap;
  - the format's limits and hostile files never panic.
- **Order of work.** Suggested:
  1. the format and bake;
  2. the load-time derivation;
  3. the renderer;
  4. the editor and the CLI;
  5. the factory content, with renders shown to the maintainer.
